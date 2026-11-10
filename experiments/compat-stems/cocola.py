# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc F2, temps 3 : score COCOLA de chaque batterie candidate posée sur la
source (`greffes/<source>/<candidat>.wav`, écrit par l'exemple
`banc_compat`) contre le reste de la source (`greffes/<source>/contexte.wav`).

    python -I cocola.py <plan.json> <racine> <COCOLA_HP_v1.ckpt> <dépôt cocola> <sortie.json>

Huit fenêtres de 5 s (16 kHz mono, l'entrée de COCOLA) réparties entre 20 % et
80 % du morceau, la moyenne des scores. Deux modes : `BOTH` (le score COCOLA
de l'article) et `PERCUSSIVE` (la moitié percussive seule). La batterie
d'origine de la source, démixée, est notée de la même façon : c'est le
témoin, la paire « vraie » que COCOLA devrait mettre en tête.

Poids : téléchargés à la source (Google Drive du dépôt), licence non
déclarée, jamais redistribués (`README.md`).
"""

import json
import os
import sys

import numpy as np
import soundfile as sf
import torch
import torchaudio.functional as AF

FENETRES = 8
DUREE = 5.0
SR_COCOLA = 16000


def mono16k(chemin):
    x, sr = sf.read(chemin, dtype="float32", always_2d=True)
    x = torch.from_numpy(x.mean(axis=1))
    return AF.resample(x, sr, SR_COCOLA)


def fenetres(n):
    """Débuts (en échantillons à 16 kHz) des fenêtres, entre 20 et 80 %."""
    lg = int(DUREE * SR_COCOLA)
    a, b = int(0.2 * n), int(0.8 * n) - lg
    if b <= a:
        return [max(0, (n - lg) // 2)]
    return [int(v) for v in np.linspace(a, b, FENETRES)]


def main():
    plan_p, racine, ckpt, depot, sortie = sys.argv[1:6]
    sys.path.insert(0, depot)
    from contrastive_model import constants
    from contrastive_model.contrastive_model import CoCola
    from feature_extraction.feature_extraction import CoColaFeatureExtractor

    modele = CoCola.load_from_checkpoint(ckpt, map_location="cpu")
    modele.eval()
    extracteur = CoColaFeatureExtractor()
    plan = json.load(open(plan_p))
    lg = int(DUREE * SR_COCOLA)
    resultats = json.load(open(sortie)) if os.path.isfile(sortie) else {}

    def traits(x, debuts):
        lot = torch.stack([x[d:d + lg] for d in debuts]).unsqueeze(1)
        return extracteur(lot).float()

    for k, s in enumerate(plan["sources"]):
        sid = str(s["id"])
        if sid in resultats:
            continue
        dossier = os.path.join(racine, "greffes", sid)
        ctx = mono16k(os.path.join(dossier, "contexte.wav"))
        debuts = fenetres(len(ctx))
        f_ctx = traits(ctx, debuts)
        if f_ctx.dim() == 3:
            f_ctx = f_ctx.unsqueeze(0)
        candidats = {str(c["id"]): os.path.join(dossier, f"{c['id']}.wav")
                     for c in s["candidats"]}
        stems = os.path.join(racine, "stems", sid)
        orig = [f for f in os.listdir(stems)
                if f.endswith("drums.wav") and not f.startswith(".")]
        if orig:
            candidats["original"] = os.path.join(stems, orig[0])
        notes = {}
        for cid, chemin in candidats.items():
            if not os.path.isfile(chemin):
                continue
            y = mono16k(chemin)
            if len(y) < len(ctx):
                y = torch.nn.functional.pad(y, (0, len(ctx) - len(y)))
            f_y = traits(y, debuts)
            if f_y.dim() == 3:
                f_y = f_y.unsqueeze(0)
            ligne = {}
            with torch.no_grad():
                for mode in ("BOTH", "PERCUSSIVE"):
                    modele.set_embedding_mode(constants.EmbeddingMode[mode])
                    # `score` met à zéro un canal en place : on lui passe des copies.
                    sc = modele.score(f_ctx.clone(), f_y.clone())
                    ligne[mode.lower()] = float(sc.diagonal().mean()) if sc.dim() == 2 else float(sc.mean())
            notes[cid] = ligne
        resultats[sid] = notes
        json.dump(resultats, open(sortie, "w"), indent=1)
        ordre = sorted((c for c in notes if c != "original"),
                       key=lambda c: -notes[c]["both"])
        rang_orig = None
        if "original" in notes:
            rang_orig = 1 + sum(notes[c]["both"] > notes["original"]["both"] for c in ordre)
        print(f"[{k + 1}/{len(plan['sources'])}] {s['artist']} — {s['title']} : "
              f"premier {ordre[0] if ordre else '—'}, original classé {rang_orig}/{len(ordre) + 1}",
              flush=True)


if __name__ == "__main__":
    main()
