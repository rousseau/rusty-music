# SPDX-License-Identifier: GPL-3.0-or-later
"""Règle les coûts du doigté et le poids de l'a priori de position par
validation croisée « un morceau de côté » : chaque morceau est doigté avec une
table apprise sans lui (`apprendre_positions.table`). Le doigté est une
réplique Python de `tablature::poser_avec` (même chemin, mêmes coûts ; écart
avec l'exemple `doigtes` < 0,002 sur le corpus).

    python3 regler_doigtes_loo.py <corpus.json>
"""
import collections, itertools, json, os, sys
from multiprocessing import Pool
import numpy as np
from apprendre_positions import table

ETENDUE = 4

def poser(notes, cordes, k, prior=None, w=0.0, frettes=24):
    max_main = frettes - (ETENDUE - 1)
    etats = []
    for n in notes:
        e = []
        for c, v in enumerate(cordes):
            f = n["hauteur"] - v
            if 0 <= f <= frettes:
                ms = range(1, max_main + 1) if f == 0 else range(max(f - 3, 1), min(f, max_main) + 1)
                e += [(c, f, m) for m in ms]
        etats.append(e)
    def jeu(c, f, m, h):
        vide = k["vide_haut"] * max(m - 1, 0) if f == 0 else 0
        x = vide + k["corde_aigue"] * c + k["pente_bas"] * max(m - 1, 0) + k["pente_haut"] * max(m - k["debut_haut"], 0)
        if prior is not None: x += w * prior(c, f, h, cordes)
        return x
    C = np.array([jeu(c, f, m, notes[0]["hauteur"]) for c, f, m in etats[0]]); R = []
    for i in range(1, len(notes)):
        sil = notes[i]["debut_s"] - notes[i-1]["fin_s"]; repli = 1 / (1 + max(sil - 0.5, 0) * 2)
        P = np.array(etats[i-1]); E = np.array(etats[i])
        trans = k["deplacement"] * np.abs(P[:, 2][None, :] - E[:, 2][:, None]) * repli + k["corde"] * np.abs(P[:, 0][None, :] - E[:, 0][:, None])
        tot = C[None, :] + trans
        arg = tot.argmin(1); h = notes[i]["hauteur"]
        C = tot[np.arange(len(E)), arg] + np.array([jeu(c, f, m, h) for c, f, m in etats[i]]); R.append(arg)
    e = int(C.argmin()); out = [None] * len(notes)
    for i in range(len(notes) - 1, -1, -1):
        out[i] = etats[i][e][:2]
        if i: e = int(R[i-1][e])
    return out
# Lu aussi par les processus du Pool (macOS : « spawn » réimporte le module).
corpus = json.load(open(sys.argv[1]))
TABLES = {m["nom"]: table(corpus, {m["nom"]})[0] for m in corpus}

def evaluer(K):
    res = collections.defaultdict(list)
    for m in corpus:
        T = TABLES[m["nom"]]; n = len(m["cordes"])
        pr = lambda c, f, h, cordes, T=T, n=n: T[min(n - 1 - c, 5)][min(f, 20)]
        out = poser(m["notes"], m["cordes"], K, pr, K["position"])
        res[m["nom"].split(":")[0]].append(sum(tuple(o) == tuple(v) for o, v in zip(out, m["verite"])) / len(out))
    s = {k: sum(v) / len(v) for k, v in res.items()}
    return sum(s.values()) / len(s), s, K

if __name__ == "__main__":
    g = [dict(corde=c, deplacement=1.0, vide_haut=v, corde_aigue=a, pente_bas=0, pente_haut=ph, debut_haut=9, position=p)
         for c, v, a, ph, p in itertools.product([0.3, 0.6, 1.0], [0, 0.1, 0.2], [0, 0.1, 0.2, 0.3], [0, 0.1], [0, 0.2, 0.3, 0.5])]
    # Avant le 17 oct. ; et le réglage retenu (`Couts::default`).
    AVANT = dict(corde=0.6, deplacement=1.0, vide_haut=0.3, corde_aigue=0, pente_bas=0, pente_haut=0, debut_haut=12, position=0)
    RETENU = dict(corde=0.6, deplacement=1.0, vide_haut=0.1, corde_aigue=0.3, pente_bas=0, pente_haut=0.1, debut_haut=9, position=0.3)
    g += [AVANT, RETENU]
    with Pool(os.cpu_count()) as p: R = p.map(evaluer, g)
    R.sort(key=lambda x: -x[0])
    for m, s, K in R[:10]: print(f"{m:.3f}", {k: round(v, 3) for k, v in s.items()}, K)
    for nom, ref in [("avant", AVANT), ("retenu", RETENU)]:
        m, s, _ = next(r for r in R if r[2] == ref); print(nom, f"{m:.3f}", {k: round(v, 3) for k, v in s.items()})
    best0 = max((r for r in R if r[2]["position"] == 0), key=lambda r: r[0]); print("meilleur sans a priori", f"{best0[0]:.3f}", best0[1], best0[2])
