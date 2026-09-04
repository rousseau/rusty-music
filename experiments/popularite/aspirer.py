#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Sonde de popularité — récupère ListenBrainz + Deezer pour un échantillon.

Hors du workspace : c'est un sondage. Il répond aux questions de la phase 0 de
`docs/popularite.md` **avant** d'écrire la moindre ligne de moteur :

  1. quelle part de la bibliothèque chaque source atteint (échelon
     enregistrement et échelon album) ;
  2. la fiabilité du rapprochement Deezer, qui se fait par recherche
     « artiste + titre » faute de MBID ;
  3. l'accord ListenBrainz ↔ Deezer quand les deux répondent.

    python3 aspirer.py [chemin/vers/rusty-music.db]

Construit un échantillon reproductible au premier passage (`echantillon.json`),
puis interroge les deux API. Reprend où il s'est arrêté : le cache
(`cache.json`) est écrit après chaque réponse. `rapport.py` lit les deux.

Aucune clé, aucun compte : ListenBrainz agrège les écoutes de sa communauté
(CC0) et son endpoint `/1/popularity/*` est public ; l'API Deezer l'est aussi.
Les deux clients s'annoncent tout de même par un `User-Agent` identifiant
l'application, par courtoisie.
"""

import json
import os
import random
import sqlite3
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ICI = os.path.dirname(os.path.abspath(__file__))
ECHANTILLON = os.path.join(ICI, "echantillon.json")
CACHE = os.path.join(ICI, "cache.json")

MODELE = "clap-htsat-unfused-5f"
TAILLE = 200
GRAINE = 1234  # échantillon reproductible d'un poste à l'autre

# Contact envoyé aux API, comme leur usage le demande. À renseigner via
# l'environnement pour un sondage réel.
CONTACT = os.environ.get("RUSTY_MUSIC_CONTACT", "contact-non-renseigne")
AGENT = f"rusty-music-sondage/0.1 ( {CONTACT} )"

LB_LOT = 60          # MBID par requête POST ListenBrainz
DEEZER_DELAI = 0.15  # ~7 req/s, bien sous la limite (~50 / 5 s)

BASE = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(
    "~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db"
)


# --------------------------------------------------------------------------
# Normalisation des titres — port fidèle de `musicbrainz::normaliser_titre`
# (crates/core/src/musicbrainz.rs). Le lien morceau → release-group se fait
# par (mb_album_artist_id, titre normalisé), exactement comme le moteur.

def sans_accent(c):
    if "à" <= c <= "å" or "À" <= c <= "Å":
        return "a"
    if "è" <= c <= "ë" or "È" <= c <= "Ë":
        return "e"
    if "ì" <= c <= "ï" or "Ì" <= c <= "Ï":
        return "i"
    if "ò" <= c <= "ö" or "Ò" <= c <= "Ö":
        return "o"
    if "ù" <= c <= "ü" or "Ù" <= c <= "Ü":
        return "u"
    if c in "çÇ":
        return "c"
    if c in "ñÑ":
        return "n"
    if c in "ýÿÝ":
        return "y"
    return c


def normaliser_titre(titre):
    s = (titre or "").strip()
    while s:
        if s[-1] == ")":
            i = s.rfind("(")
        elif s[-1] == "]":
            i = s.rfind("[")
        else:
            break
        if i > 0:
            s = s[:i].strip()
        else:
            break
    return "".join(sans_accent(c).lower() for c in s if sans_accent(c).isalnum())


def cle_artiste(nom):
    """Réduit un nom d'artiste à ce qui permet de le reconnaître malgré la
    ponctuation, les accents et un « feat. » de fin."""
    nom = (nom or "").lower()
    for coupe in (" feat.", " feat ", " ft.", " featuring ", " & ", " and ", " x "):
        if coupe in nom:
            nom = nom.split(coupe)[0]
    return "".join(sans_accent(c) for c in nom if sans_accent(c).isalnum())


# --------------------------------------------------------------------------
# Échantillon

def construire_echantillon():
    conn = sqlite3.connect(f"file:{BASE}?mode=ro", uri=True)

    # Lien (artiste, titre normalisé) → release-group, comme `Library::mb_albums`.
    albums = {}
    for art, norm, mbid in conn.execute(
        "SELECT artist_mbid, title_norm, mbid FROM mb_release_groups"
    ):
        albums.setdefault((art, norm), mbid)

    # Effectif par artiste — pour ventiler la couverture par « taille » d'artiste.
    effectif = {
        art: n
        for art, n in conn.execute(
            "SELECT mb_album_artist_id, COUNT(*) FROM tracks"
            " WHERE mb_album_artist_id IS NOT NULL AND mb_album_artist_id <> ''"
            " GROUP BY mb_album_artist_id"
        )
    }

    candidats = conn.execute(
        "SELECT t.mb_recording_id, t.artist, t.title, t.album,"
        "       t.mb_album_artist_id"
        "  FROM features f JOIN tracks t ON t.id = f.track_id"
        " WHERE f.model = ?"
        "   AND t.mb_recording_id IS NOT NULL AND t.mb_recording_id <> ''"
        "   AND t.title IS NOT NULL AND t.artist IS NOT NULL",
        (MODELE,),
    ).fetchall()

    tire = random.Random(GRAINE).sample(candidats, min(TAILLE, len(candidats)))
    ech = []
    for rec, artiste, titre, album, art_mbid in tire:
        rg = albums.get((art_mbid or "", normaliser_titre(album)))
        ech.append(
            {
                "recording_mbid": rec,
                "artiste": artiste,
                "titre": titre,
                "album": album,
                "artiste_mbid": art_mbid,
                "rg_mbid": rg,
                "effectif_artiste": effectif.get(art_mbid or "", 0),
            }
        )
    json.dump(ech, open(ECHANTILLON, "w"), ensure_ascii=False, indent=1)
    print(f"échantillon : {len(ech)} morceaux, "
          f"{sum(1 for e in ech if e['rg_mbid'])} avec un release-group résolu")
    return ech


# --------------------------------------------------------------------------
# Réseau

def http_json(url, corps=None):
    """GET, ou POST si `corps` est un dict. Réessaie sur échec temporaire ;
    404 rend None."""
    data = None
    headers = {"User-Agent": AGENT}
    if corps is not None:
        data = json.dumps(corps).encode()
        headers["Content-Type"] = "application/json"
    for essai in range(4):
        try:
            r = urllib.request.urlopen(
                urllib.request.Request(url, data=data, headers=headers), timeout=30
            )
            return json.loads(r.read())
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            time.sleep(2 ** essai)
        except Exception:
            time.sleep(2 ** essai)
    return None


def lb_popularite(kind, mbids):
    """POST /1/popularity/{recording,release-group}. Rend {mbid: (ecoutes,
    auditeurs)} — seuls les MBID connus de ListenBrainz figurent."""
    champ = "recording_mbids" if kind == "recording" else "release_group_mbids"
    cle = "recording_mbid" if kind == "recording" else "release_group_mbid"
    out = {}
    for i in range(0, len(mbids), LB_LOT):
        lot = mbids[i:i + LB_LOT]
        rep = http_json(
            f"https://api.listenbrainz.org/1/popularity/{kind}", {champ: lot}
        )
        for e in rep or []:
            out[e[cle]] = {
                "ecoutes": e.get("total_listen_count") or 0,
                "auditeurs": e.get("total_user_count") or 0,
            }
        time.sleep(1.0)
        print(f"  ListenBrainz {kind} : {min(i + LB_LOT, len(mbids))}/{len(mbids)}",
              flush=True)
    return out


def deezer_chercher(kind, artiste, titre):
    """`/search/{track,album}` par « artiste + titre ». Rend le premier
    résultat dont l'artiste concorde, sinon le premier résultat marqué non
    concordant (pour mesurer le taux d'erreur), sinon None."""
    champ = "track" if kind == "track" else "album"
    q = f'artist:"{artiste}" {champ}:"{titre}"'
    url = "https://api.deezer.com/search/" + kind + "?" + urllib.parse.urlencode(
        {"q": q, "limit": 5}
    )
    rep = http_json(url)
    resultats = (rep or {}).get("data") or []
    if not resultats:
        return None
    attendu = cle_artiste(artiste)
    concordant = None
    for d in resultats:
        rendu = cle_artiste(d.get("artist", {}).get("name", ""))
        ok = bool(attendu) and (rendu == attendu or attendu in rendu or rendu in attendu)
        if ok:
            concordant = d
            break
    d = concordant or resultats[0]
    rendu_nom = d.get("artist", {}).get("name", "")
    info = {
        "trouve": True,
        "concordant": concordant is not None,
        "artiste_rendu": rendu_nom,
        "titre_rendu": d.get("title", ""),
    }
    if kind == "track":
        info["rank"] = d.get("rank") or 0
    else:
        info["album_id"] = d.get("id")
        # `fans` n'est pas dans la recherche : un aller-retour de plus.
        det = http_json(f"https://api.deezer.com/album/{d.get('id')}")
        info["fans"] = (det or {}).get("fans") or 0
        time.sleep(DEEZER_DELAI)
    return info


# --------------------------------------------------------------------------

def charger_cache():
    if os.path.exists(CACHE):
        return json.load(open(CACHE, encoding="utf-8"))
    return {"lb_recording": {}, "lb_release_group": {}, "deezer_track": {}, "deezer_album": {}}


def sauver_cache(cache):
    json.dump(cache, open(CACHE, "w"), ensure_ascii=False)


def main():
    ech = (
        json.load(open(ECHANTILLON, encoding="utf-8"))
        if os.path.exists(ECHANTILLON)
        else construire_echantillon()
    )
    cache = charger_cache()

    # --- ListenBrainz, par lots -----------------------------------------
    rec_a_faire = sorted(
        {e["recording_mbid"] for e in ech} - set(cache["lb_recording"])
    )
    if rec_a_faire:
        print(f"ListenBrainz — {len(rec_a_faire)} enregistrements")
        for mbid in rec_a_faire:
            cache["lb_recording"][mbid] = None  # marque « demandé »
        for mbid, v in lb_popularite("recording", rec_a_faire).items():
            cache["lb_recording"][mbid] = v
        sauver_cache(cache)

    rg_a_faire = sorted(
        {e["rg_mbid"] for e in ech if e["rg_mbid"]} - set(cache["lb_release_group"])
    )
    if rg_a_faire:
        print(f"ListenBrainz — {len(rg_a_faire)} release-groups")
        for mbid in rg_a_faire:
            cache["lb_release_group"][mbid] = None
        for mbid, v in lb_popularite("release-group", rg_a_faire).items():
            cache["lb_release_group"][mbid] = v
        sauver_cache(cache)

    # --- Deezer, un par un --------------------------------------------------
    reste = [e for e in ech if e["recording_mbid"] not in cache["deezer_track"]]
    print(f"Deezer — {len(reste)} morceaux à chercher "
          f"(~{len(reste) * DEEZER_DELAI * 3 / 60:.0f} min)")
    for i, e in enumerate(reste, 1):
        rec = e["recording_mbid"]
        cache["deezer_track"][rec] = deezer_chercher("track", e["artiste"], e["titre"]) or {
            "trouve": False
        }
        time.sleep(DEEZER_DELAI)
        cache["deezer_album"][rec] = deezer_chercher("album", e["artiste"], e["album"]) or {
            "trouve": False
        }
        time.sleep(DEEZER_DELAI)
        if i % 25 == 0:
            sauver_cache(cache)
            print(f"  Deezer : {i}/{len(reste)}", flush=True)
    sauver_cache(cache)
    print("\nterminé — `python3 rapport.py` pour le bilan")


if __name__ == "__main__":
    main()
