# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc F2, temps 3 (chargement sûr) : score COCOLA sans exécuter le dépôt
cloné ni dérouler le pickle d'origine (décision du 10 nov.).

    python -I cocola_sur.py <plan.json> <racine> <COCOLA_HP_v1.ckpt> <sortie.json> [poids.safetensors]

Réseau réécrit ici d'après gladia-research-group/cocola (MIT, © 2024 GLADIA
Research Group ; commit b3ac948, `contrastive_model/contrastive_model.py` et
`feature_extraction/feature_extraction.py`) : EfficientNet-B0 sans tête à
2 canaux (paquet PyPI `efficientnet_pytorch` 0.7.1, Apache-2.0), dropout,
projection 1280 → 512, LayerNorm, tanh, similarité bilinéaire. Entrée :
5 s mono à 16 kHz → STFT (1024, fenêtre 400, pas 160) → HPSS librosa →
deux mels (64 bandes, 60–7 800 Hz) en dB relatifs au maximum.

Poids : `torch.load(weights_only=True)`. Le checkpoint Lightning porte, dans
ses hyperparamètres, trois énumérations du dépôt (`EmbeddingMode`,
`ModelInputType`, `Dataset` : relevées par `pickletools`, seules globales
hors torch avec `OrderedDict`). Elles sont remplacées par une classe inerte
qui garde la valeur : le chargeur restreint de torch reste celui qui
déroule le fichier. Chargement strict du `state_dict` (aucune clé
manquante ni en trop). Score par fenêtre = `pairwise` (diagonale), comme
`CoCola.score`.

Le témoin de sens (décision du 10 nov.) : pour chaque source, sa batterie
d'origine démixée contre une batterie tirée au hasard dans une autre
source ; l'origine doit gagner dans au moins 80 % des cas.

Poids téléchargés à la source (Google Drive du dépôt), licence non
déclarée, jamais redistribués (`README.md`).
"""

import json
import os
import random
import sys

import librosa
import numpy as np
import soundfile as sf
import torch
import torch._weights_only_unpickler as wou
from efficientnet_pytorch import EfficientNet
from torch import nn

FENETRES = 8
DUREE = 5.0
SR = 16000
GRAINE = 20261117


class Inerte:
    """Tient la place d'une énumération du dépôt : garde sa valeur, rien d'autre."""

    def __init__(self, valeur=None):
        self.valeur = valeur


def charger_state_dict(ckpt):
    autorises = wou._get_allowed_globals

    def elargis():
        g = dict(autorises())
        for nom in ("EmbeddingMode", "ModelInputType", "Dataset"):
            g[f"contrastive_model.constants.{nom}"] = Inerte
        return g

    wou._get_allowed_globals = elargis
    try:
        d = torch.load(ckpt, map_location="cpu", weights_only=True)
    finally:
        wou._get_allowed_globals = autorises
    hp = {k: getattr(v, "valeur", v) for k, v in d.get("hyper_parameters", {}).items()}
    return d["state_dict"], hp


class Cocola(nn.Module):
    def __init__(self, dim=512):
        super().__init__()
        self.encoder = nn.Module()
        self.encoder.encoder = nn.Module()
        self.encoder.encoder.model = nn.Sequential(
            EfficientNet.from_name("efficientnet-b0", include_top=False, in_channels=2),
            nn.Dropout(0.1), nn.Flatten())
        self.encoder.projection = nn.Linear(1280, dim)
        self.layer_norm = nn.LayerNorm(dim)
        self.similarity = nn.Module()
        self.similarity.w = nn.Parameter(torch.zeros(dim, dim))

    def plonger(self, x):
        e = self.encoder.projection(self.encoder.encoder.model(x))
        return torch.tanh(self.layer_norm(e))

    def score(self, x, y, mode):
        x, y = x.clone(), y.clone()
        if mode == "percussive":
            x[:, 0] = 0
            y[:, 0] = 0
        e = self.plonger(torch.cat((x, y)))
        ex, ey = e[: len(x)], e[len(x):]
        proj = torch.matmul(self.similarity.w, ey.t())
        return (ex * proj.t()).sum(-1)


def hpss(audio):
    s = librosa.stft(audio, n_fft=1024, win_length=400, hop_length=160)
    h, p = librosa.decompose.hpss(s)
    mels = []
    for c in (h, p):
        m = librosa.feature.melspectrogram(S=np.abs(c) ** 2, sr=SR, fmin=60.0, fmax=7800.0, n_mels=64)
        mels.append(librosa.power_to_db(m, ref=np.max))
    return torch.from_numpy(np.stack(mels)).float()


def mono16k(chemin):
    x, sr = sf.read(chemin, dtype="float32", always_2d=True)
    return librosa.resample(x.mean(axis=1), orig_sr=sr, target_sr=SR)


def fenetres(n):
    lg = int(DUREE * SR)
    a, b = int(0.2 * n), int(0.8 * n) - lg
    if b <= a:
        return [max(0, (n - lg) // 2)]
    return [int(v) for v in np.linspace(a, b, FENETRES)]


def traits(x, debuts):
    lg = int(DUREE * SR)
    morceaux = []
    for d in debuts:
        m = x[d:d + lg]
        if len(m) < lg:
            m = np.pad(m, (0, lg - len(m)))
        morceaux.append(hpss(m))
    return torch.stack(morceaux)


def batterie_origine(racine, sid):
    stems = os.path.join(racine, "stems", str(sid))
    if not os.path.isdir(stems):
        return None
    f = [f for f in os.listdir(stems) if f.endswith("drums.wav") and not f.startswith(".")]
    return os.path.join(stems, f[0]) if f else None


def main():
    plan_p, racine, ckpt, sortie = sys.argv[1:5]
    sd, hp = charger_state_dict(ckpt)
    modele = Cocola(hp.get("embedding_dim", 512))
    manque = modele.load_state_dict(sd, strict=True)
    print(f"chargement strict : {len(sd)} tenseurs, {manque} ; hyperparamètres {hp}", flush=True)
    modele.eval()
    if len(sys.argv) > 5:
        from safetensors.torch import save_file
        save_file({k: v.contiguous() for k, v in sd.items()}, sys.argv[5],
                  metadata={"source": "COCOLA_HP_v1.ckpt (gladia-research-group/cocola)",
                            "licence": "non déclarée — ne pas redistribuer"})
        print(f"poids écrits : {sys.argv[5]}", flush=True)

    plan = json.load(open(plan_p))
    sources = plan["sources"]
    hasard = random.Random(GRAINE)
    resultats = json.load(open(sortie)) if os.path.isfile(sortie) else {}
    with torch.no_grad():
        for k, s in enumerate(sources):
            sid = str(s["id"])
            if sid in resultats:
                continue
            dossier = os.path.join(racine, "greffes", sid)
            ctx = mono16k(os.path.join(dossier, "contexte.wav"))
            debuts = fenetres(len(ctx))
            f_ctx = traits(ctx, debuts)
            candidats = {str(c["id"]): os.path.join(dossier, f"{c['id']}.wav") for c in s["candidats"]}
            o = batterie_origine(racine, sid)
            if o:
                candidats["original"] = o
            autres = [x for x in sources if x["id"] != s["id"] and batterie_origine(racine, x["id"])]
            autre = hasard.choice(autres)
            candidats["hasard"] = batterie_origine(racine, autre["id"])
            notes = {}
            for cid, chemin in candidats.items():
                if not os.path.isfile(chemin):
                    continue
                y = mono16k(chemin)
                if len(y) < len(ctx):
                    y = np.pad(y, (0, len(ctx) - len(y)))
                f_y = traits(y, debuts)
                notes[cid] = {m: float(modele.score(f_ctx, f_y, m).mean()) for m in ("both", "percussive")}
            notes["hasard"]["source"] = autre["id"]
            resultats[sid] = notes
            json.dump(resultats, open(sortie, "w"), indent=1)
            ordre = sorted((c for c in notes if c not in ("original", "hasard")), key=lambda c: -notes[c]["both"])
            rang = 1 + sum(notes[c]["both"] > notes["original"]["both"] for c in ordre) if "original" in notes else None
            print(f"[{k + 1}/{len(sources)}] {s['artist']} — {s['title']} : premier {ordre[0] if ordre else '—'}, "
                  f"original {rang}/{len(ordre) + 1}, original {notes.get('original', {}).get('both', float('nan')):.2f} "
                  f"contre hasard {notes['hasard']['both']:.2f}", flush=True)

    gagne = [r["original"]["both"] > r["hasard"]["both"] for r in resultats.values() if "original" in r]
    gagne_p = [r["original"]["percussive"] > r["hasard"]["percussive"] for r in resultats.values() if "original" in r]
    print(f"témoin : l'origine bat le hasard {sum(gagne)}/{len(gagne)} (both), "
          f"{sum(gagne_p)}/{len(gagne_p)} (percussive)", flush=True)


if __name__ == "__main__":
    main()
