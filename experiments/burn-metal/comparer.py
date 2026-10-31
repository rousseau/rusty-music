# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare des dossiers de stems (`<stem>.wav`) à une référence : par stem,
l'écart rapporté à l'énergie du **mélange** (dB, plus haut = plus proche).

Rapporter l'écart au stem lui-même tromperait : un stem presque muet (le
`other` de htdemucs_6s sur un passage sans autre instrument, −60 dB sous
le mélange) donne un mauvais rapport pour une erreur inaudible. La ligne
de tête donne le niveau de chaque stem de référence sous le mélange.

    <env>/bin/python -I comparer.py <melange.wav> <reference> <essai> [<essai>…]
"""
import os, sys
import numpy as np, soundfile as sf


def lire(chemin, n=None):
    x, _ = sf.read(chemin, dtype="float64", always_2d=True)
    return x[:n]


def main():
    melange, ref, essais = sys.argv[1], sys.argv[2], sys.argv[3:]
    stems = sorted(f[:-4] for f in os.listdir(ref) if f.endswith(".wav") and not f.startswith("._"))
    refs = {s: lire(os.path.join(ref, f"{s}.wav")) for s in stems}
    n = min(len(r) for r in refs.values())
    p = (lire(melange, n) ** 2).sum()
    print(f"{'':24}", "  ".join(f"{s:>7}" for s in stems))
    print(f"{'niveau du stem':24}", "  ".join(f"{10 * np.log10((refs[s][:n] ** 2).sum() / p):7.1f}" for s in stems))
    for e in essais:
        cases = []
        for s in stems:
            y = lire(os.path.join(e, f"{s}.wav"), n)
            cases.append(10 * np.log10(p / max(((refs[s][:n] - y) ** 2).sum(), 1e-30)))
        print(f"{os.path.basename(e.rstrip('/')):24}", "  ".join(f"{v:7.1f}" for v in cases))


if __name__ == "__main__":
    main()
