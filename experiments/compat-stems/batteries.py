# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc F2 : la part de la batterie démixée dans chaque morceau source.

    python -I batteries.py <plan-sources.json> <racine> <exclus.txt> [seuil]

Écrit dans `exclus.txt` les sources dont la batterie porte moins de `seuil`
(0,12 par défaut) de l'énergie RMS du morceau, mesurée sur les fenêtres de
5 s les plus fortes de chaque stem : un morceau sans vraie batterie (guitare-
voix, interlude) ne dit rien d'une greffe de batterie.
"""

import json
import os
import sys

import numpy as np
import soundfile as sf


def rms_haut(x, sr):
    """RMS du quart le plus fort des fenêtres de 5 s."""
    lg = 5 * sr
    v = [np.sqrt(np.mean(x[i:i + lg] ** 2)) for i in range(0, max(1, len(x) - lg), lg)]
    v = sorted(v)
    return float(np.mean(v[-max(1, len(v) // 4):]))


def main():
    plan = json.load(open(sys.argv[1]))
    racine, exclus = sys.argv[2], sys.argv[3]
    seuil = float(sys.argv[4]) if len(sys.argv) > 4 else 0.12
    ecartes = []
    for s in plan["sources"]:
        d = os.path.join(racine, "stems", str(s["id"]))
        if not os.path.isdir(d):
            print(f"{s['id']} : pas de stems")
            continue
        parts = {}
        somme = None
        sr = 44100
        for f in os.listdir(d):
            if not f.endswith(".wav") or f.startswith("."):
                continue
            x, sr = sf.read(os.path.join(d, f), dtype="float32", always_2d=True)
            x = x.mean(axis=1)
            somme = x if somme is None else somme[:len(x)] + x[:len(somme)]
            parts[f.rsplit("—", 1)[-1].strip()[:-4]] = x
        part = rms_haut(parts["drums"], sr) / max(rms_haut(somme, sr), 1e-9)
        garde = part >= seuil
        if not garde:
            ecartes.append(s["id"])
        print(f"{'garde ' if garde else 'ÉCARTE'} {part:.2f}  {s['artist']} — {s['title']}")
    with open(exclus, "w") as f:
        f.write("\n".join(str(i) for i in ecartes) + "\n")


if __name__ == "__main__":
    main()
