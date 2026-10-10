# SPDX-License-Identifier: GPL-3.0-or-later
"""Règle Basic Pitch et la porte d'énergie contre les partitions publiées :
pour chaque morceau passé au banc, une grille de réglages (exemple
`banc -- reglages`), notée en F1 (même double croche, même hauteur) et en
notes écrites dans les silences de la partition.

    python3 regler_basic_pitch.py [id ...]
"""
import glob, json, os, subprocess, sys
from collections import defaultdict
from concurrent.futures import ProcessPoolExecutor
import banc, comparer

def noter_fichier(args):
    f, ref = args
    phase, N, R, chemin, st = comparer.comparer(json.load(open(f)), json.load(open(ref)))
    return os.path.basename(f)[3:-5], st

def main():
    voulus = set(sys.argv[1:])
    total = defaultdict(lambda: defaultdict(int)); f1s = defaultdict(list)
    taches = []
    for d in sorted(glob.glob(os.path.join(banc.CACHE, "*/"))):
        nom = os.path.basename(d.rstrip("/"))
        if voulus and nom not in voulus: continue
        ref = os.path.join(d, "reference.json")
        stems = os.path.join(d, "stems")
        if not (os.path.exists(ref) and os.path.isdir(stems)): continue
        grille = os.path.join(d, "reglages")
        if not os.path.isdir(grille):
            basse = next(f for f in os.listdir(stems) if "bass" in f)
            subprocess.run([banc.BANC, "reglages", os.path.join(stems, basse), os.path.join(d, "pulsation.json"), grille],
                           check=True, stdout=subprocess.DEVNULL, cwd=banc.RACINE)
        taches += [(f, ref) for f in sorted(glob.glob(os.path.join(grille, "*.json")))]
    with ProcessPoolExecutor() as ex:
        for nom, st in ex.map(noter_fichier, taches, chunksize=8):
            for k, v in st.items(): total[nom][k] += v
            f1s[nom].append(2 * st["exactes"] / max(1, st["ref"] + st["nous"]))
    f1 = lambda s, k="exactes", r="ref", n="nous": 2 * s[k] / max(1, s[r] + s[n])
    classe = sorted(total, key=lambda k: -f1(total[k]))
    print("attaque_trame_durée_harm_porte    F1   F1±1  F1tab  préc.  rappel  (F1 moyen)  notes/silences")
    for k in classe[:15] + ["0.6_0.3_8_0.6_0", "0.6_0.3_8_0.6_30"]:
        s = total[k]
        print(f"{k:32} {f1(s):.3f} {f1(s, 'tol'):.3f} {f1(s, 'exactes_tab', 'ref_tab', 'nous_tab'):.3f}  "
              f"{s['exactes'] / max(1, s['nous']):.3f}  {s['exactes'] / max(1, s['ref']):.3f}   ({sum(f1s[k]) / len(f1s[k]):.3f})"
              f"   {s['notes_silence']}/{s['vides']}")

if __name__ == "__main__":
    main()
