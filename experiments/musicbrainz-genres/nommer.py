#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Que liraient les douze familles si on les nommait avec MusicBrainz ?

Applique à la lettre le score du moteur (`nommer_les_familles`, part × log₂ de
la sur-représentation) sur deux sources différentes, et les met côte à côte.
Seule la source change : la méthode est la même, sinon on comparerait deux
choses à la fois.

    python3 aspirer.py     # d'abord, ~30 min : remplit genres.json
    python3 nommer.py      # ensuite, instantané
"""

import collections
import json
import math
import os
import sqlite3
import sys

MODELE = "clap-htsat-unfused-5f"
ICI = os.path.dirname(os.path.abspath(__file__))
BASE = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
    "~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db"
)
# Au-delà des trois premiers, les genres MusicBrainz d'un artiste décrivent ses
# marges — un jazzman y récolte « pop » sur un album de commande.
GENRES_PAR_ARTISTE = 3


def score(locales, total, globales, total_global, plancher=0.01, mini=5):
    """`part × log₂(sur-représentation)`, comme `nommer_les_familles`."""
    out = []
    for etiquette, n in locales.items():
        if n < max(mini, plancher * total):
            continue
        part = n / total
        s = part * math.log2(part / (globales[etiquette] / total_global))
        if s > 0:
            out.append((etiquette, s))
    out.sort(key=lambda x: -x[1])
    return [e for e, _ in out]


def se_redisent(a, b):
    """Même règle lexicale que le moteur : un mot commun, ou un préfixe long."""
    mots = lambda g: [m.lower() for m in "".join(
        c if c.isalnum() else " " for c in g).split() if len(m) >= 3]
    ma, mb = mots(a), mots(b)
    return any(
        x == y or (min(len(x), len(y)) >= 5 and (x.startswith(y) or y.startswith(x)))
        for x in ma for y in mb
    )


def libeller(genres, vus):
    if not genres:
        return None
    tete, repli = genres[0], None
    for g in genres[1:]:
        if se_redisent(tete, g):
            continue
        nom = f"{tete} · {g}"
        if nom not in vus:
            return nom
        repli = repli or nom
    return tete if tete not in vus else (repli or tete)


def etiqueter(par_famille, effectifs):
    globales, total_global = collections.Counter(), 0
    for c in par_famille.values():
        for t, n in c.items():
            globales[t] += n
            total_global += n
    vus, sortie = set(), {}
    for famille, _ in effectifs:
        locales = par_famille.get(famille, collections.Counter())
        nom = libeller(score(locales, sum(locales.values()) or 1, globales, total_global or 1), vus)
        nom = nom or f"Famille {famille}"
        vus.add(nom)
        sortie[famille] = nom
    return sortie


def main():
    genres = json.load(open(os.path.join(ICI, "genres.json"), encoding="utf-8"))
    conn = sqlite3.connect(f"file:{BASE}?mode=ro", uri=True)
    lignes = conn.execute(
        "SELECT f.cluster, t.mb_artist_id, t.genre FROM features f"
        " JOIN tracks t ON t.id = f.track_id WHERE f.model = ?",
        (MODELE,),
    ).fetchall()

    effectifs = collections.Counter(c for c, _, _ in lignes)
    ordre = sorted(effectifs.items(), key=lambda kv: -kv[1])

    mb, tags = collections.defaultdict(collections.Counter), collections.defaultdict(collections.Counter)
    couverts = 0
    for cluster, mbid, tag in lignes:
        g = genres.get(mbid or "", [])[:GENRES_PAR_ARTISTE]
        if g:
            couverts += 1
        for x in g:
            mb[cluster][x] += 1
        if tag:
            tags[cluster][tag] += 1

    print(f"{couverts} / {len(lignes)} morceaux reçoivent au moins un genre MusicBrainz "
          f"({100 * couverts / len(lignes):.1f} %)")
    tagues = sum(1 for _, _, t in lignes if t)
    print(f"{tagues} / {len(lignes)} portent un genre dans leurs tags "
          f"({100 * tagues / len(lignes):.1f} %)\n")

    a, b = etiqueter(tags, ordre), etiqueter(mb, ordre)
    print(f"{'n':>6}  {'tags des fichiers (actuel)':<32}  MusicBrainz")
    print("─" * 84)
    for famille, n in ordre:
        print(f"{n:>6}  {a[famille]:<32}  {b[famille]}")


if __name__ == "__main__":
    main()
