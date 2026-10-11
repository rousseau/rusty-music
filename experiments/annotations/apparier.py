# SPDX-License-Identifier: GPL-3.0-or-later
"""Apparie des jeux d'annotations publiés sans audio aux morceaux de la
bibliothèque : la vérité vient du jeu, l'audio de nos fichiers. Rien n'est
téléchargé d'autre que les annotations (`$RUSTY_DEV/jeux/annotations`).

- Harmonix Set (Nieto et al. 2019) : temps, premiers temps, sections ;
  identifiants MusicBrainz d'enregistrement.
- Isophonics (Mauch et al. 2009) : Beatles, Queen, Carole King, Zweieck,
  Michael Jackson ; sections, accords, tonalités, temps (Beatles, Zweieck).
- SALAMI (Smith et al. 2011) : sections, deux annotateurs.

Appariement par MBID d'enregistrement, sinon par artiste et titre
normalisés. Dans les deux cas la durée doit tenir à ±3 s : une autre
édition (single, remaster allongé, live) donnerait une vérité décalée.
La base est ouverte en lecture seule.

    python apparier.py            # écrit $RUSTY_DEV/jeux/annotations/appariement.json
"""
import csv, glob, json, os, re, sqlite3, sys, unicodedata
from collections import Counter

DEV = os.path.expanduser(os.environ.get("RUSTY_DEV", "/Volumes/LaCieDock/rusty_music-dev"))
if not os.path.isdir(DEV):
    sys.exit(f"données de développement absentes : {DEV} (disque débranché ? sinon RUSTY_DEV=…)")
ANN = os.path.join(DEV, "jeux", "annotations")
BASE = os.path.expanduser(os.environ.get("RUSTY_DB", "~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db"))
TOLERANCE_S = 3.0

def norm(s):
    """Minuscules sans accents ni ponctuation ; sans parenthèses (« Remastered
    2009 », « Live ») ni article initial."""
    s = unicodedata.normalize("NFKD", s or "").encode("ascii", "ignore").decode().lower()
    s = re.sub(r"[\(\[].*?[\)\]]", " ", s)
    s = re.sub(r"\s-\s.*(remaster|version|mix|edit|mono|stereo).*$", " ", s)
    s = s.replace("&", " and ")
    s = re.sub(r"[^a-z0-9]+", " ", s).strip()
    return re.sub(r"^the ", "", s)

def harmonix():
    for r in csv.DictReader(open(os.path.join(ANN, "harmonixset", "dataset", "metadata.csv"))):
        d = os.path.join(ANN, "harmonixset", "dataset")
        yield dict(jeu="harmonix", id=r["File"], titre=r["Title"], artiste=r["Artist"], duree=float(r["Duration"]),
                   mbid=r["MusicBrainz Id"] or None,
                   temps=os.path.join(d, "beats_and_downbeats", r["File"] + ".txt"),
                   sections=os.path.join(d, "segments", r["File"] + ".txt"))

def isophonics():
    racine = os.path.join(ANN, "iso")
    for lab in glob.glob(os.path.join(racine, "seglab", "*", "*", "*.lab")):
        if os.path.basename(lab).startswith("._"): continue
        rel = os.path.relpath(lab, os.path.join(racine, "seglab"))
        artiste = rel.split(os.sep)[0]
        titre = re.sub(r"^(CD\d_-_)?(\d+-)?\d+(_-_|\s|_)", "", os.path.splitext(os.path.basename(lab))[0]).replace("_", " ")
        lignes = [l.split() for l in open(lab, errors="replace") if l.strip()]
        fin = max(float(l[1]) for l in lignes if len(l) >= 2 and re.match(r"^[\d.]+$", l[1]))
        e = dict(jeu="isophonics", id=rel, titre=titre, artiste=artiste, duree=fin, mbid=None, sections=lab)
        for cle, dossier in (("temps", "beat"), ("accords", "chordlab"), ("tonalites", "keylab")):
            for ext in (".txt", ".lab"):
                f = os.path.join(racine, dossier, os.path.splitext(rel)[0] + ext)
                if os.path.exists(f): e[cle] = f; break
        yield e

def salami():
    d = os.path.join(ANN, "salami-data-public")
    for r in csv.DictReader(open(os.path.join(d, "metadata", "metadata.csv"), errors="replace")):
        f = os.path.join(d, "annotations", r["SONG_ID"], "parsed", "textfile1_functions.txt")
        if not os.path.exists(f) or not r["SONG_DURATION"]: continue
        yield dict(jeu="salami", id=r["SONG_ID"], titre=r["SONG_TITLE"].replace("_", " "), artiste=r["ARTIST"].replace("_", " "),
                   duree=float(r["SONG_DURATION"]), mbid=None, sections=f)

def main():
    db = sqlite3.connect(f"file:{BASE}?mode=ro", uri=True)
    pistes = db.execute("select id, path, title, artist, album_artist, duration_ms, mb_recording_id from tracks").fetchall()
    par_mbid, par_nom = {}, {}
    for p in pistes:
        if p[6]: par_mbid.setdefault(p[6], []).append(p)
        for a in {p[3], p[4]} - {None}:
            par_nom.setdefault((norm(a), norm(p[2])), []).append(p)
    sortie, compte, vus = [], Counter(), Counter()
    for jeu in (harmonix, isophonics, salami):
        for e in jeu():
            vus[e["jeu"]] += 1
            cands, par = par_mbid.get(e["mbid"], []) if e["mbid"] else [], "mbid"
            if not cands: cands, par = par_nom.get((norm(e["artiste"]), norm(e["titre"])), []), "nom"
            # La piste la plus proche en durée, dans la tolérance.
            cands = sorted((abs(p[5] / 1000 - e["duree"]), p) for p in cands if p[5])
            if not cands or cands[0][0] > TOLERANCE_S: continue
            ecart, p = cands[0]
            sortie.append(dict(e, piste=p[0], chemin=p[1], duree_piste=p[5] / 1000, par=par))
            compte[e["jeu"]] += 1
    json.dump(sortie, open(os.path.join(ANN, "appariement.json"), "w"), ensure_ascii=False, indent=1)
    for j in vus: print(f"{j:12} {compte[j]:4} appariés sur {vus[j]}")
    print(f"{len(sortie)} morceaux → {os.path.join(ANN, 'appariement.json')}")

if __name__ == "__main__":
    main()
