#!/usr/bin/env bash
# Prépare l'encodeur audio de CLAP pour l'import Burn.
#
# À jouer une fois, et à rejouer seulement si le modèle ou le fenêtrage change.
# Le résultat, `models/clap-audio-encoder-b5.onnx`, est ce que lit le
# `build.rs` de `crates/analysis` — pas le modèle d'origine.
#
# Pourquoi cette étape existe : le modèle publié déclare son entrée comme
# entièrement dynamique, si bien que les douze blocs Swin calculent leurs
# marges (`Pad`) à l'exécution. `burn-onnx` génère du Rust par inférence
# statique de formes et s'arrête net dessus :
#
#     PANIC => Runtime pads are not supported in burn-onnx
#
# Figer la forme d'entrée réduit le graphe de 8 031 nœuds à 882 et rend les
# douze marges constantes. Voir `experiments/burn-clap/README.md`.
#
# NE PAS utiliser `onnx-simplifier` pour cela : il replie encore mieux, mais
# produit ici un graphe que `onnx.checker` et ONNX Runtime refusent tous deux
# (les reshapes de partitionnement Swin sortent incohérents). L'outillage
# d'ONNX Runtime, lui, ne produit pas un graphe qu'il refuserait de relire.

set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODELE="$RACINE/models/clap-audio-encoder.onnx"
SORTIE="$RACINE/models/clap-audio-encoder-b5.onnx"

# Doit rester d'accord avec `decode::FENETRES` et le frontal log-mel.
LOT=5
TRAMES=1001
MELS=64

SOURCE_URL="https://huggingface.co/icybawss/clap-htsat-unfused-audio-encoder-onnx/resolve/main/audio_model.onnx"
SOURCE_SHA=a1c2b43c44f71e0fa841a4b86700886c199bf87699ea45632c4d831bc6c88957

if [ ! -f "$MODELE" ]; then
  echo "→ modèle source depuis Hugging Face"
  curl -L --fail --retry 3 --retry-delay 5 -o "$MODELE.partiel" "$SOURCE_URL"
  mv "$MODELE.partiel" "$MODELE"
fi

REEL="$(shasum -a 256 "$MODELE" | cut -d' ' -f1)"
if [ "$REEL" != "$SOURCE_SHA" ]; then
  echo "✗ le modèle source n'a pas l'empreinte attendue" >&2
  echo "  $MODELE" >&2
  echo "  attendu $SOURCE_SHA" >&2
  echo "  obtenu  $REEL" >&2
  echo "  (le supprimer et relancer, ou vérifier la source)" >&2
  exit 1
fi

# Environnement Python jetable : il ne sert qu'à cette préparation, il n'a rien
# à faire dans les dépendances du projet.
VENV="${TMPDIR:-/tmp}/rusty-music-prep-venv"
if [ ! -x "$VENV/bin/python" ]; then
  echo "→ environnement Python de préparation"
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet --upgrade pip
  "$VENV/bin/pip" install --quiet onnx onnxruntime
fi
PY="$VENV/bin/python"

TRAVAIL="$(mktemp -d)"
trap 'rm -rf "$TRAVAIL"' EXIT
cp "$MODELE" "$TRAVAIL/etape.onnx"

echo "→ formes figées : ${LOT} × 1 × ${TRAMES} × ${MELS}"
for paire in "audio_batch_size $LOT" "num_channels 1" "height $TRAMES" "width $MELS"; do
  set -- $paire
  "$PY" -m onnxruntime.tools.make_dynamic_shape_fixed \
    --dim_param "$1" --dim_value "$2" \
    "$TRAVAIL/etape.onnx" "$TRAVAIL/suivant.onnx"
  mv "$TRAVAIL/suivant.onnx" "$TRAVAIL/etape.onnx"
done

echo "→ repliage des constantes par ONNX Runtime"
# `BASIC` et non `EXTENDED` : les fusions poussées introduisent des opérateurs
# du domaine `com.microsoft`, que Burn ne connaît pas.
"$PY" - "$TRAVAIL/etape.onnx" "$SORTIE" <<'PYCODE'
import sys, collections, onnx, onnxruntime as ort

entree, sortie = sys.argv[1], sys.argv[2]
o = ort.SessionOptions()
o.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
o.optimized_model_filepath = sortie
ort.InferenceSession(entree, o, providers=["CPUExecutionProvider"])

# Contrôles : un graphe qu'ORT relit, sans opérateur exotique, sans marge
# calculée. Sans eux, un import Burn pourrait « réussir » sur un graphe cassé.
ort.InferenceSession(sortie, providers=["CPUExecutionProvider"])
m = onnx.load(sortie, load_external_data=False)
onnx.checker.check_model(m)

domaines = {n.domain for n in m.graph.node if n.domain}
if domaines:
    sys.exit(f"opérateurs hors du domaine standard : {domaines}")

connus = {i.name for i in m.graph.initializer}
connus |= {n.output[0] for n in m.graph.node if n.op_type == "Constant"}
calcules = [
    n.name for n in m.graph.node
    if n.op_type == "Pad" and len(n.input) > 1 and n.input[1] not in connus
]
if calcules:
    sys.exit(f"marges encore calculées à l'exécution : {calcules}")

ops = collections.Counter(n.op_type for n in m.graph.node)
print(f"   {sum(ops.values())} nœuds · {len(ops)} types · 0 marge calculée")
PYCODE

echo "✓ $SORTIE"
