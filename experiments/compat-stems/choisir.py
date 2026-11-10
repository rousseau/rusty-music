# SPDX-License-Identifier: GPL-3.0-or-later
"""Banc F2, temps 1 : choisit 20 morceaux sources et, pour chacun, 10 batteries
candidates (voisins CLAP au tempo replié), sur une COPIE de la base.

    python choisir.py <copie de rusty-music.db> <plan.json> [N] [exclus.txt]

N sources (20 par défaut). `exclus.txt` : des id de sources à écarter, un par
ligne (celles dont la batterie démixée est presque muette, `batteries.py`).
Avec N > 20, le plan ne garde que les sources (`paires` vide) : c'est le
premier passage, pour démixer les sources et vérifier leur batterie.
`CHOISIR_SOURCES=<fichier>` : les id des sources, dans l'ordre, au lieu de
la sélection par famille (le second passage reprend ainsi exactement les
sources vérifiées au premier).

Sources : une par famille CLAP (`features.cluster`) et par tour, les
familles les plus peuplées d'abord, le morceau le plus populaire de la famille, un artiste
jamais repris. Sont écartés les morceaux trop courts ou trop longs, sans
tempo, et les genres où une batterie est rare (classique, ambient…).

Candidats : les plus proches au cosinus CLAP (morceau entier, comme la
moitié « son » du préfiltre de F1), tempo replié à l'octave à ±10 % (la
tolérance de F1), autre artiste, une seule édition par (artiste, titre).
L'ordre CLAP est le classement « CLAP seul » du banc.
"""

import json
import math
import os
import sqlite3
import sys

import numpy as np

MODELE = "clap-htsat-unfused-5f"
N_SOURCES = 20
N_CANDIDATS = 10
TOLERANCE = 0.10
SANS_BATTERIE = (
    "classical", "classique", "ambient", "soundtrack", "bande originale",
    "opera", "opéra", "baroque", "new age", "spoken", "piano", "drone",
    "a cappella", "chanson",  # chanson : souvent guitare-voix
)


def replie(a, b):
    """Rapport de tempo ramené dans [1/√2, √2] (comme greffe::tempo_replie)."""
    f = b / a
    while f > math.sqrt(2):
        f /= 2
    while f < 1 / math.sqrt(2):
        f *= 2
    return f


def main():
    base, sortie = sys.argv[1], sys.argv[2]
    n_sources = int(sys.argv[3]) if len(sys.argv) > 3 else N_SOURCES
    exclus = set()
    if len(sys.argv) > 4:
        exclus = {int(x) for x in open(sys.argv[4]).read().split()}
    db = sqlite3.connect(f"file:{base}?mode=ro", uri=True)
    lignes = db.execute(
        """
        SELECT t.id, t.path, t.artist, t.title, t.genre, t.duration_ms,
               d.bpm, f.vector, f.cluster, COALESCE(p.relative, 0)
        FROM tracks t
        JOIN features f ON f.track_id = t.id AND f.model = ?
        JOIN descriptors d ON d.track_id = t.id
        LEFT JOIN track_popularite p ON p.track_id = t.id
        WHERE d.bpm BETWEEN 60 AND 200
          AND t.duration_ms BETWEEN 150000 AND 420000
        """,
        (MODELE,),
    ).fetchall()
    morceaux = []
    for (tid, path, artist, title, genre, dur, bpm, vec, cluster, pop) in lignes:
        g = (genre or "").lower()
        if any(m in g for m in SANS_BATTERIE):
            continue
        if not os.path.isfile(path):
            continue
        morceaux.append(
            dict(id=tid, path=path, artist=artist or "", title=title or "",
                 genre=genre or "", bpm=float(bpm), cluster=cluster,
                 pop=float(pop), v=np.frombuffer(vec, dtype="<f4"))
        )
    print(f"{len(morceaux)} morceaux éligibles")
    V = np.stack([m["v"] for m in morceaux])
    V /= np.linalg.norm(V, axis=1, keepdims=True)

    # Sources : une par famille, des plus peuplées aux moins peuplées.
    familles = {}
    for i, m in enumerate(morceaux):
        familles.setdefault(m["cluster"], []).append(i)
    ordre = sorted(familles, key=lambda c: -len(familles[c]))
    sources, artistes = [], set()
    while len(sources) < n_sources:
        avant = len(sources)
        for c in ordre:
            for i in sorted(familles[c], key=lambda i: -morceaux[i]["pop"]):
                a = morceaux[i]["artist"].lower()
                if i in sources or morceaux[i]["id"] in exclus:
                    continue
                if a and a not in artistes:
                    sources.append(i)
                    artistes.add(a)
                    break
            if len(sources) == n_sources:
                break
        if len(sources) == avant:
            break
    if os.environ.get("CHOISIR_SOURCES"):
        index = {m["id"]: i for i, m in enumerate(morceaux)}
        ids = [int(x) for x in open(os.environ["CHOISIR_SOURCES"]).read().split()]
        sources = [index[t] for t in ids if t in index][:n_sources]
    premier_passage = n_sources > N_SOURCES

    plan = {"modele": MODELE, "tolerance_tempo": TOLERANCE,
            "sources": [], "morceaux": [], "paires": []}
    utiles = set()
    for s in sources:
        ms = morceaux[s]
        sim = V @ V[s]
        vus, cands = set(), []
        for j in np.argsort(-sim):
            mj = morceaux[j]
            if j == s or mj["artist"].lower() == ms["artist"].lower():
                continue
            f = replie(ms["bpm"], mj["bpm"])
            if abs(f - 1) > TOLERANCE:
                continue
            cle = (mj["artist"].lower(), mj["title"].lower())
            if cle in vus:
                continue
            vus.add(cle)
            cands.append(dict(id=mj["id"], cos=float(sim[j]), facteur=f))
            if len(cands) == N_CANDIDATS:
                break
        plan["sources"].append(dict(
            id=ms["id"], artist=ms["artist"], title=ms["title"],
            genre=ms["genre"], bpm=ms["bpm"], famille=ms["cluster"],
            candidats=cands))
        utiles.add(s)
        for k, c in enumerate(cands):
            plan["paires"].append(dict(source=ms["id"], candidat=c["id"],
                                       rang_clap=k + 1, cos=c["cos"]))
    if premier_passage:
        plan["paires"] = []
        ids = {morceaux[s]["id"] for s in sources}
    else:
        ids = {p["source"] for p in plan["paires"]} | {p["candidat"] for p in plan["paires"]}
    par_id = {m["id"]: m for m in morceaux}
    # Les sources d'abord : ce sont elles qu'on vérifie en premier.
    rang = {morceaux[s]["id"]: k for k, s in enumerate(sources)}
    for tid in sorted(ids, key=lambda t: (rang.get(t, len(rang)), t)):
        m = par_id[tid]
        plan["morceaux"].append(dict(id=tid, path=m["path"], artist=m["artist"],
                                     title=m["title"], genre=m["genre"], bpm=m["bpm"]))
    with open(sortie, "w") as f:
        json.dump(plan, f, ensure_ascii=False, indent=1)
    print(f"{len(plan['sources'])} sources, {len(plan['paires'])} paires, "
          f"{len(plan['morceaux'])} morceaux à démixer")
    for s in plan["sources"]:
        print(f"- {s['artist']} — {s['title']} ({s['genre']}, {s['bpm']:.0f} BPM)")


if __name__ == "__main__":
    main()
