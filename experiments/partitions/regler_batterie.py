# SPDX-License-Identifier: GPL-3.0-or-later
"""Règle les seuils de détection d'ADTOF, pièce par pièce, contre les
transcriptions publiées (`banc_batterie.py`). Les classes se détectent
indépendamment : un balayage par classe, les autres au seuil d'ADTOF.

    python3 regler_batterie.py
"""
import glob, json, os, subprocess
from collections import defaultdict
import banc, banc_batterie, comparer

NOMS = ["grosse caisse", "caisse claire", "toms", "charleston", "cymbales"]
CODES = [36, 38, 45, 42, 49]

def main():
    conf = json.load(open(os.path.join(banc.ICI, "morceaux.json")))
    tot = defaultdict(lambda: [0, 0, 0])  # (classe, seuil) → exactes, réf, nous
    for m in conf["batterie"]:
        d, stem, puls = banc.preparer_audio(m, "drums")
        grille = os.path.join(d, "seuils")
        if not os.path.isdir(grille):
            subprocess.run([banc.BANC, "batterie", stem, puls, os.path.join(d, "batterie.json"), grille],
                           check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
        ref = json.load(open(os.path.join(d, "reference-batterie.json")))
        # alignement : celui de la transcription par défaut
        phase, N, R, chemin, _ = comparer.comparer(banc_batterie.en_evenements(json.load(open(os.path.join(d, "batterie.json")))["mesures"]), ref)
        for f in glob.glob(os.path.join(grille, "seuil_*.json")):
            _, c, v = os.path.basename(f)[:-5].split("_"); c = int(c)
            Nv = comparer.nos_mesures(banc_batterie.en_evenements(json.load(open(f))["mesures"]), phase)
            code = CODES[c]
            s = comparer.noter([[n for n in o if n[1] == code] for o in Nv], [[n for n in r if n[1] == code] for r in R], chemin)
            t = tot[(c, float(v))]; t[0] += s["tol"]; t[1] += s["ref"]; t[2] += s["nous"]
    for c, nom in enumerate(NOMS):
        ligne = sorted((v, 2 * e / max(1, r + n), e / max(1, r), e / max(1, n)) for (k, v), (e, r, n) in tot.items() if k == c)
        print(f"{nom:14}", "  ".join(f"{v:.2f}→{f1:.2f}" for v, f1, _, _ in ligne))

if __name__ == "__main__":
    main()
