#!/usr/bin/env bash
# Prépare le générateur AERO (super-résolution audio) pour l'exécution hors
# ligne — export ONNX du **seul réseau**, STFT laissée à Rust.
#
#   ./scripts/preparer-aero.sh --checkpoint CHEMIN.th [--segment 5]
#
# Le résultat, `models/aero-<lr>-<hr>.onnx`, prend en entrée un spectrogramme
# complexe en 2 canaux réels `[1, 2, nfft/2, T]` (T fixé par --segment) et rend
# la même forme. La STFT et l'iSTFT (`torch.stft` normalisée, Hann,
# center/reflect, bin de Nyquist retiré — `aero/src/models/spec.py`) sont
# refaites côté Rust avec `rustfft`, comme pour le module 3.
#
# Pourquoi le réseau seul : l'export PyTorch de `torch.stft` déroule la
# transformée en milliers de nœuds `Shape`/`Range`/`ScatterND` — le sondage
# demucs a mesuré 66 % du graphe rien que pour ça.
#
# Deux retouches nécessaires, appliquées ici :
#  1. `LocalState` (attention temporelle) masque sa diagonale par
#     `torch.eye(T, dtype=bool)` → `EyeLike`, que ONNX Runtime CPU
#     n'implémente pas. Remplacé par `(delta == 0)`, mathématiquement
#     identique, sur des opérateurs standard.
#  2. Les kwargs du checkpoint portent des clés qu'`Aero.__init__` ne connaît
#     pas (`channels_time`, `wiener_iters`…) — filtrées.
#
# Repliage des constantes : `ORT_ENABLE_BASIC`, recette `preparer-modele.sh`.
# Voir `experiments/burn-aero/README.md`.
#
# Checkpoints (Google Drive, dossier du dépôt AERO) : le dossier `musdb/`
# contient le modèle musique (11 025 → 44 100 Hz). Licence des poids : voir le
# README du sondage.

set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

CHECKPOINT=""
SEGMENT=5      # secondes d'audio basse résolution par segment (fige T)

while [ $# -gt 0 ]; do
  case "$1" in
    --checkpoint) CHECKPOINT="$2"; shift 2 ;;
    --segment)    SEGMENT="$2"; shift 2 ;;
    *) echo "argument inconnu : $1" >&2; exit 1 ;;
  esac
done

if [ -z "$CHECKPOINT" ] || [ ! -f "$CHECKPOINT" ]; then
  cat >&2 <<'EOF'

  Checkpoint AERO absent (--checkpoint CHEMIN.th).

  Récupérer depuis le dossier Google Drive du dépôt AERO :
      https://github.com/slp-rl/aero  →  README §  "pre-trained models"
      https://drive.google.com/drive/folders/1KuVJNkR7lZddvufmNsx-uAIluvb5XQ2L

  Le sous-dossier `musdb/aero-nfft=512-hl=256/checkpoint.th` est le modèle
  musique (11 025 → 44 100 Hz), ~437 Mo.

EOF
  exit 1
fi

# --- dépôt AERO (pour src.models) et environnement Python jetable -----------

AERO_DIR="${TMPDIR:-/tmp}/rusty-music-aero-src"
if [ ! -d "$AERO_DIR/.git" ]; then
  echo "→ clone d'AERO (src.models)"
  git clone --depth 1 https://github.com/slp-rl/aero "$AERO_DIR"
fi

VENV="${TMPDIR:-/tmp}/rusty-music-aero-venv"
if [ ! -x "$VENV/bin/python" ]; then
  echo "→ environnement Python de préparation"
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet --upgrade pip
  "$VENV/bin/pip" install --quiet torch --index-url https://download.pytorch.org/whl/cpu
  "$VENV/bin/pip" install --quiet onnx onnxruntime omegaconf einops
fi
PY="$VENV/bin/python"

TRAVAIL="$(mktemp -d)"
trap 'rm -rf "$TRAVAIL"' EXIT

echo "→ export ONNX du générateur (réseau seul, sans STFT), parité vs PyTorch"
PYTHONPATH="$AERO_DIR" "$PY" - "$CHECKPOINT" "$SEGMENT" "$RACINE/models" "$TRAVAIL" <<'PYCODE'
import sys, os, collections
import torch
import src.models.modules as M
from src.models.aero import Aero

ckpt, segment, models_dir, work = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]

# --- retouche 1 : EyeLike -> (delta == 0) ---------------------------------
def local_forward(self, x):
    B, C, T = x.shape
    heads = self.heads
    idx = torch.arange(T, device=x.device, dtype=x.dtype)
    delta = idx[:, None] - idx[None, :]
    q = self.query(x).view(B, heads, -1, T)
    k = self.key(x).view(B, heads, -1, T)
    dots = torch.einsum("bhct,bhcs->bhts", k, q) / (k.shape[2] ** 0.5)
    if self.nfreqs:
        raise NotImplementedError("nfreqs>0 non géré par ce script")
    if self.ndecay:
        d = torch.arange(1, self.ndecay + 1, device=x.device, dtype=x.dtype)
        dq = torch.sigmoid(self.query_decay(x).view(B, heads, -1, T)) / 2
        dk = -d.view(-1, 1, 1) * delta.abs() / self.ndecay ** 0.5
        dots = dots + torch.einsum("fts,bhfs->bhts", dk, dq)
    dots = dots.masked_fill(delta == 0, -100)          # ex-torch.eye(T, bool)
    w = torch.softmax(dots, dim=2)
    c = self.content(x).view(B, heads, -1, T)
    r = torch.einsum("bhts,bhct->bhcs", w, c).reshape(B, -1, T)
    return x + self.proj(r)
M.LocalState.forward = local_forward

# --- construction du modèle depuis le paquet -----------------------------
PARAMS = set("""in_channels out_channels audio_channels channels growth nfft
  hop_length end_iters cac rewrite hybrid hybrid_old freq_emb emb_scale
  emb_smooth kernel_size strides context context_enc freq_ends enc_freq_attn
  norm_starts norm_groups dconv_mode dconv_depth dconv_comp dconv_time_attn
  dconv_lstm dconv_init rescale lr_sr hr_sr spec_upsample act_func debug""".split())

pkg = torch.load(ckpt, map_location="cpu", weights_only=False)
g = pkg["models"]["generator"]
kw = {k: v for k, v in dict(g["kwargs"]).items() if k in PARAMS}
model = Aero(**kw)
model.load_state_dict(g["state"])
model.eval()

nfft = kw["nfft"]
hop = model.hop_length          # divisé par scale dans Aero
lr_sr, hr_sr = kw["lr_sr"], kw["hr_sr"]
Fq = nfft // 2                  # bin de Nyquist retiré (comme _spec)
lr_len = int(lr_sr * segment // hop) * hop
z = model._spec(torch.randn(1, 1, lr_len))
T = z.shape[-1]
print(f"   nfft={nfft} hop={hop} lr_sr={lr_sr} hr_sr={hr_sr}  entrée=[1,2,{Fq},{T}]")


class Reseau(torch.nn.Module):
    """`Aero.forward` sans `_spec` / `_ispec`."""
    def __init__(s, m):
        super().__init__()
        s.m = m

    def forward(s, x):                       # [B, 2, Fq, T]
        B, _, Fq, T = x.shape
        mean = x.mean(dim=(1, 2, 3), keepdim=True)
        std = x.std(dim=(1, 2, 3), keepdim=True)
        h = (x - mean) / (1e-5 + std)
        saved, lengths = [], []
        for idx, enc in enumerate(s.m.encoder):
            lengths.append(h.shape[-1])
            h = enc(h, None)
            if idx == 0 and s.m.freq_emb is not None:
                frs = torch.arange(h.shape[-2], device=h.device)
                emb = s.m.freq_emb(frs).t()[None, :, :, None].expand_as(h)
                h = h + s.m.freq_emb_scale * emb
            saved.append(h)
        h = torch.zeros_like(h)
        for dec in s.m.decoder:
            h = dec(h, saved.pop(-1), lengths.pop(-1))
        h = h.view(B, s.m.out_channels, -1, Fq, T)
        h = h * std[:, None] + mean[:, None]
        return h[:, 0]                       # [B, 2, Fq, T]


net = Reseau(model).eval()
ex_audio = torch.randn(1, 1, lr_len) * 0.3
with torch.no_grad():
    zc = model._spec(ex_audio)
    m_in = model._move_complex_to_channels_dim(zc)
    ref = net(m_in)

brut = os.path.join(work, "brut.onnx")
torch.onnx.export(net, m_in, brut, input_names=["spec"], output_names=["spec_hr"],
                  opset_version=17, dynamic_axes=None)

import onnx, onnxruntime as ort
sortie = os.path.join(models_dir, f"aero-{lr_sr}-{hr_sr}.onnx")
os.makedirs(models_dir, exist_ok=True)
o = ort.SessionOptions()
o.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
o.optimized_model_filepath = sortie
sess = ort.InferenceSession(brut, o, providers=["CPUExecutionProvider"])

got = torch.tensor(sess.run(None, {"spec": m_in.numpy()})[0])
rel = float((ref - got).norm() / ref.norm())
print(f"   parité ORT vs PyTorch : erreur L2 relative {rel:.2e}"
      f"  (max abs {float((ref - got).abs().max()):.2e})")
assert rel < 3e-3, "parité insuffisante — vérifier les retouches"

m = onnx.load(sortie, load_external_data=False)
onnx.checker.check_model(m)
dom = {n.domain for n in m.graph.node if n.domain}
if dom:
    sys.exit(f"opérateurs hors domaine standard : {dom}")
ops = collections.Counter(n.op_type for n in m.graph.node)
exotiques = {k: ops[k] for k in ("EyeLike", "Range", "ScatterND", "NonZero", "Loop") if k in ops}
print(f"   {sum(ops.values())} nœuds · {len(ops)} types · exotiques : {exotiques or 'aucun'}")
if exotiques:
    sys.exit("formes encore dynamiques")

# Métadonnées pour le lecteur Rust : tout ce qu'il faut pour la STFT.
meta = onnx.StringStringEntryProto
for key, val in [("rusty_music.nfft", str(nfft)), ("rusty_music.hop", str(hop)),
                 ("rusty_music.lr_sr", str(lr_sr)), ("rusty_music.hr_sr", str(hr_sr)),
                 ("rusty_music.frames", str(T)), ("rusty_music.freq_bins", str(Fq)),
                 ("rusty_music.stft_normalized", "true")]:
    e = m.metadata_props.add(); e.key, e.value = key, val
onnx.save(m, sortie)

print(f"\n✓ {sortie}  ({os.path.getsize(sortie)/1e6:.0f} Mo)")
print(f"  STFT Rust : nfft={nfft}, hop={hop}, Hann périodique, normalized "
      f"(×1/√nfft), center + reflect, {Fq} bins (Nyquist retiré), {T} trames.")
PYCODE
