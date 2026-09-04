#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Aspire les genres MusicBrainz de tous les artistes de la bibliothèque.

Hors du workspace : c'est un sondage. Il répond à une question précise —
**si on nommait les familles avec MusicBrainz plutôt qu'avec les tags des
fichiers, que liraient-elles ?** — avant d'écrire la moindre ligne de moteur.

92,6 % des morceaux portent déjà un `mb_artist_id` dans leurs tags : aucun
appariement flou n'est nécessaire, on interroge par identifiant.

    python3 aspirer.py [chemin/vers/rusty-music.db]

Reprend où il s'est arrêté : le cache est écrit après chaque réponse.
"""

import json
import os
import sqlite3
import sys
import time
import urllib.error
import urllib.request

# MusicBrainz exige un agent identifiant l'application et un contact, et
# limite à une requête par seconde. Les deux sont des conditions d'accès, pas
# des recommandations : sans agent, on est bloqué.
CONTACT = os.environ.get("RUSTY_MUSIC_CONTACT", "contact-non-renseigne")
AGENT = f"rusty-music-sondage/0.1 ( {CONTACT} )"
DELAI = 1.1
CACHE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "genres.json")
MODELE = "clap-htsat-unfused-5f"
BASE = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
    "~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db"
)


def charger():
    return json.load(open(CACHE, encoding="utf-8")) if os.path.exists(CACHE) else {}


def interroger(mbid):
    """Les genres d'un artiste, du plus voté au moins voté.

    Sur 503 — la réponse de MusicBrainz quand on va trop vite — on patiente et
    on réessaie : c'est un signal de rythme, pas une absence de données.
    """
    url = f"https://musicbrainz.org/ws/2/artist/{mbid}?inc=genres&fmt=json"
    for essai in range(4):
        try:
            r = urllib.request.urlopen(
                urllib.request.Request(url, headers={"User-Agent": AGENT}), timeout=25
            )
            d = json.loads(r.read())
            g = sorted(d.get("genres", []), key=lambda x: -x.get("count", 0))
            return [x["name"] for x in g]
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return []
            time.sleep(2 ** essai)
        except Exception:
            time.sleep(2 ** essai)
    return None  # abandon : à retenter au prochain passage


def main():
    conn = sqlite3.connect(f"file:{BASE}?mode=ro", uri=True)
    # Les artistes les plus représentés d'abord : la couverture en *morceaux*
    # monte alors bien plus vite que la couverture en artistes, et le sondage
    # devient exploitable avant d'être terminé.
    mbids = [
        r[0]
        for r in conn.execute(
            "SELECT t.mb_artist_id FROM features f"
            " JOIN tracks t ON t.id = f.track_id"
            " WHERE f.model = ? AND t.mb_artist_id IS NOT NULL AND t.mb_artist_id <> ''"
            " GROUP BY t.mb_artist_id ORDER BY COUNT(*) DESC",
            (MODELE,),
        )
    ]
    cache = charger()
    reste = [m for m in mbids if m not in cache]
    print(f"{len(mbids)} artistes, {len(cache)} déjà en cache, {len(reste)} à chercher")
    print(f"~{len(reste) * DELAI / 60:.0f} min au rythme imposé par MusicBrainz\n", flush=True)

    for i, mbid in enumerate(reste, 1):
        g = interroger(mbid)
        if g is not None:
            cache[mbid] = g
            json.dump(cache, open(CACHE, "w"), ensure_ascii=False)
        if i % 100 == 0:
            avec = sum(1 for v in cache.values() if v)
            print(f"  {i}/{len(reste)} — {avec} artistes avec au moins un genre", flush=True)
        time.sleep(DELAI)

    avec = sum(1 for v in cache.values() if v)
    print(f"\n{len(cache)} artistes en cache, {avec} avec au moins un genre "
          f"({100 * avec / max(len(cache), 1):.0f} %)")


if __name__ == "__main__":
    main()
