#!/usr/bin/env python3
"""Compare notre carte à celle d'AudioMuse-AI sur la même bibliothèque.

Hors du workspace : c'est un sondage, pas une dépendance. Rien de ce que
produit ce script n'entre dans rusty_music — il ne sert qu'à vérifier, contre
un outil bâti séparément, que nos familles décrivent quelque chose de réel.

    export AM_URL=http://127.0.0.1:8000 AM_USER=… AM_PASS=…
    python3 comparer.py [chemin/vers/rusty-music.db]

Les identifiants passent par l'environnement : ils n'ont rien à faire dans un
fichier versionné.
"""

import collections
import json
import math
import os
import random
import re
import sqlite3
import sys
import unicodedata
import urllib.parse
import urllib.request

URL = os.environ.get("AM_URL", "http://127.0.0.1:8000")
MODELE = "clap-htsat-unfused-5f"
BASE = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
    "~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db"
)


def session():
    """Ouvre une session : l'instance impose une authentification par cookie."""
    galette = urllib.request.HTTPCookieProcessor()
    client = urllib.request.build_opener(galette)
    corps = urllib.parse.urlencode(
        {"user": os.environ["AM_USER"], "password": os.environ["AM_PASS"]}
    ).encode()
    client.open(f"{URL}/auth", corps).read()
    return client


def exporter(client):
    """Aspire `/api/sync`, page par page. 27 000 morceaux en une poignée de secondes."""
    tout, page = [], 1
    while True:
        r = client.open(f"{URL}/api/sync?limit=500&include_embeddings=false&page={page}")
        d = json.loads(r.read())
        tout.extend(d["tracks"])
        if not d.get("has_more"):
            return tout
        page += 1


def cle(artiste, titre):
    """Nos identifiants sont des chemins, les leurs des GUID du serveur média.

    Le seul appariement possible passe donc par artiste + titre, dépouillés des
    accents et de la ponctuation. 93,7 % des morceaux se retrouvent ainsi.
    """
    s = f"{artiste or ''}\x00{titre or ''}"
    s = unicodedata.normalize("NFKD", s).encode("ascii", "ignore").decode().lower()
    return re.sub(r"[^a-z0-9\x00]+", "", s)


def caracteristiques(compte, total, global_, total_global, plancher=0.01):
    """Le même score que `nommer_les_familles` : part × log₂(sur-représentation).

    Réemployé tel quel pour que la comparaison porte sur les étiquettes, pas
    sur deux façons différentes de les classer.
    """
    out = []
    for etiquette, n in compte.items():
        if n < plancher * total:
            continue
        part = n / total
        s = part * math.log2(part / (global_[etiquette] / total_global))
        if s > 0:
            out.append((etiquette, s))
    out.sort(key=lambda x: -x[1])
    return [e for e, _ in out]


def dispersion(points, tirages=4000):
    """Distance moyenne entre deux points pris au hasard.

    Mesurée par échantillonnage : toutes les paires d'une famille de 4 000
    morceaux, c'est huit millions de distances pour un chiffre qui converge en
    quelques milliers.
    """
    s = 0.0
    for _ in range(tirages):
        (ax, ay), (bx, by) = random.choice(points), random.choice(points)
        s += math.hypot(ax - bx, ay - by)
    return s / tirages


def main():
    random.seed(1)
    eux = {}
    for x in exporter(session()):
        eux.setdefault(cle(x["artist"], x["title"]), x)

    conn = sqlite3.connect(f"file:{BASE}?mode=ro", uri=True)
    nous = conn.execute(
        "SELECT f.cluster, t.artist, t.title FROM features f"
        " JOIN tracks t ON t.id = f.track_id WHERE f.model = ?",
        (MODELE,),
    ).fetchall()

    familles = collections.defaultdict(list)
    apparies = 0
    for cluster, artiste, titre in nous:
        x = eux.get(cle(artiste, titre))
        if x:
            familles[cluster].append(x)
            apparies += 1
    print(f"{apparies} / {len(nous)} morceaux appariés ({100 * apparies / len(nous):.1f} %)\n")

    # 1. Les artefacts de leurs descripteurs, à vérifier avant de s'y fier.
    # `eux` est indexé par artiste+titre : son compte est celui des clés
    # distinctes, un peu inférieur au nombre de morceaux exportés.
    tempos = collections.Counter(round(x["tempo"]) for x in eux.values())
    modes = collections.Counter(x["scale"] for x in eux.values())
    print(f"tempo : {len(tempos)} valeurs distinctes sur {len(eux)} titres")
    print(f"mode  : {dict(modes)}\n")

    # 2. Nos familles décrites par leurs étiquettes calculées, à nous inconnues.
    globales, total_global = collections.Counter(), 0
    for v in familles.values():
        for x in v:
            for p in x["mood_vector"].split(","):
                globales[p.split(":")[0].lower()] += 1
                total_global += 1

    print(f"{'famille':>8} {'n':>6} {'tempo':>6} {'énergie':>8}  étiquettes AudioMuse caractéristiques")
    print("─" * 96)
    for c in sorted(familles, key=lambda c: -len(familles[c])):
        v = familles[c]
        locales, total = collections.Counter(), 0
        for x in v:
            for p in x["mood_vector"].split(","):
                locales[p.split(":")[0].lower()] += 1
                total += 1
        tempo = sorted(x["tempo"] for x in v)[len(v) // 2]
        energie = sum(x["energy"] for x in v) / len(v)
        tags = caracteristiques(locales, total, globales, total_global)[:4]
        print(f"{c:>8} {len(v):>6} {tempo:>6.0f} {energie:>8.3f}  " + " · ".join(tags))

    # 3. La mesure qui compte : nos familles se tiennent-elles dans leur espace ?
    tous = [(x["umap_x"], x["umap_y"]) for v in familles.values() for x in v]
    ref = dispersion(tous)
    print(f"\ndispersion de la bibliothèque entière dans leur projection : {ref:.3f}")
    print(f"\n{'famille':>8} {'n':>6} {'dispersion':>11} {'/ référence':>12}")
    print("─" * 42)
    for c in sorted(familles, key=lambda c: -len(familles[c])):
        pts = [(x["umap_x"], x["umap_y"]) for x in familles[c]]
        d = dispersion(pts)
        print(f"{c:>8} {len(pts):>6} {d:>11.3f} {d / ref:>11.2f}×")


if __name__ == "__main__":
    main()
