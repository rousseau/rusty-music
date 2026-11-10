# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc F2, temps 5, après l'écoute : confronte les notes de François
(`ecoute/notes.tsv`, 1 à 5) aux trois méthodes, par la clé (`cle.json`).

    python -I correler.py <racine>

Par méthode : note moyenne de son premier choix, et nombre de sources où il
a la meilleure note (ex æquo comptés pour chacun).
"""

import csv
import json
import os
import sys


def main():
    racine = sys.argv[1]
    cle = json.load(open(os.path.join(racine, "cle.json")))
    notes = {}
    with open(os.path.join(racine, "ecoute", "notes.tsv")) as f:
        for r in csv.reader(f, delimiter="\t"):
            if len(r) >= 3 and r[2].strip() and r[1].endswith(".wav"):
                notes[r[1]] = float(r[2].replace(",", "."))
    par_methode = {"clap": [], "groove": [], "cocola": []}
    gagne = {m: 0 for m in par_methode}
    for num, e in cle.items():
        vus = {}
        for lettre, f in e["fichiers"].items():
            n = notes.get(f"{num}-{lettre}.wav")
            if n is None:
                continue
            for m in f["methodes"]:
                vus[m] = n
        if not vus:
            continue
        meilleure = max(vus.values())
        for m, n in vus.items():
            par_methode[m].append(n)
            gagne[m] += n == meilleure
    for m, v in par_methode.items():
        moy = sum(v) / len(v) if v else float("nan")
        print(f"{m:7s} : note moyenne {moy:.2f} sur {len(v)} sources, meilleure {gagne[m]} fois")


if __name__ == "__main__":
    main()
