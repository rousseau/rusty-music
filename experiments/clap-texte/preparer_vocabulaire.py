#!/usr/bin/env python
# SPDX-License-Identifier: GPL-3.0-or-later
"""Construit le vocabulaire CLAP-texte de production, depuis Wikipédia.

Remplace le vocabulaire choisi à la main de `sonder.py` (`LARGE`/`COUVRANT`,
une soixantaine à une centaine de phrases) par un vocabulaire construit à
partir des genres MusicBrainz **réellement présents** dans la bibliothèque —
voir `docs/nommage-familles.md`. Trois étapes, dans l'ordre :

  1. **genres candidats** — lus depuis `rusty-music genres-candidats
     --seuil N` (un genre par ligne, sur stdin ou un fichier) ;
  2. **paragraphe Wikipédia** — l'article du genre (résolu par recherche,
     pas par un titre deviné), son paragraphe de tête plutôt que l'article
     entier ;
  3. **réécriture Ollama** — le paragraphe devient 1-2 phrases descriptives
     au format qui marche pour CLAP (« a banjo and a fiddle playing a reel »,
     pas « celtic ») — un petit modèle local, pas un patron mécanique : un
     mot nu est un mauvais prompt CLAP, l'essai l'a mesuré.

Sortie : `vocabulaire.bin` (N × 512 `f32` petit-boutien) + `vocabulaire.txt`
(`genre<TAB>phrase` par ligne, même ordre) — écrits par défaut dans
`crates/analysis/vocabulaire/`, committés (quelques centaines de Ko, pas les
modèles lourds de `scripts/`).

Un genre pour lequel Wikipédia ou Ollama ne donne rien d'exploitable est
simplement omis — pas de repli mécanique, conformément à la décision prise
avec l'utilisateur. La liste des genres omis est imprimée à la fin : ce sont
les cas résiduels du point 5 (`docs/nommage-familles.md`), à écrire à la main.

Réutilise `sonder.py` pour la tour texte (`tour_texte`, `PROMPT`) — aucune
nouvelle architecture, le même modèle que l'essai a déjà validé.

Dépendances, dans le même environnement que `sonder.py` (voir son README) :
torch, transformers, numpy — plus `requests` pour Wikipédia. Ollama tourne à
part (`ollama serve`), interrogé en HTTP, aucune dépendance Python de plus.
"""

import argparse
import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
from sonder import PROMPT, tour_texte  # noqa: E402

WIKIPEDIA_API = "https://en.wikipedia.org/w/api.php"
OLLAMA_API = "http://localhost:11434/api/generate"

# Les exemples de l'essai qui ont marché — le vocabulaire décide, pas le
# modèle : un mot nu (« celtic ») échoue, une phrase descriptive réussit.
EXEMPLES = [
    ("celtic", "traditional celtic music with fiddle and flute"),
    ("boom bap", "a rapper over a boom bap beat"),
    ("trip hop", "a slow downtempo track with a trip hop beat"),
    ("classical", "a symphony orchestra"),
    ("ska", "a ska band with a horn section and offbeat guitar"),
]

PROMPT_REECRITURE = """Tu réécris un paragraphe Wikipédia décrivant un genre \
musical en UNE SEULE phrase descriptive courte, en anglais, dans le style \
d'une légende sonore — ce qu'on entendrait, pas le nom du genre.

Exemples (genre → phrase) :
{exemples}

Règles : décris la texture, l'instrumentation ou le rythme, jamais le nom du \
genre lui-même. Pas de guillemets, pas de ponctuation finale, une seule \
ligne, en anglais.

Genre : {genre}
Paragraphe Wikipédia :
{paragraphe}

Phrase :"""


def genres_depuis(source):
    """Un genre par ligne, depuis un fichier ou stdin (`-`)."""
    texte = sys.stdin.read() if source == "-" else Path(source).read_text(encoding="utf-8")
    return [l.strip() for l in texte.splitlines() if l.strip()]


def requete_json(url, params, methode="GET"):
    qs = urllib.parse.urlencode(params)
    if methode == "GET":
        req = urllib.request.Request(f"{url}?{qs}", headers={"User-Agent": "rusty-music/vocabulaire"})
    else:
        req = urllib.request.Request(url, data=qs.encode(), headers={"User-Agent": "rusty-music/vocabulaire"})
    with urllib.request.urlopen(req, timeout=15) as r:
        return json.loads(r.read().decode("utf-8"))


def article_wikipedia(genre):
    """Le titre de l'article Wikipédia le plus probable pour ce genre —
    résolu par recherche plutôt que deviné (« ska » n'est pas « Ska (unit) »,
    la recherche désambiguïse là où un titre construit à la main se
    tromperait)."""
    v = requete_json(
        WIKIPEDIA_API,
        {
            "action": "query",
            "list": "search",
            "srsearch": f"{genre} music genre",
            "srlimit": 1,
            "format": "json",
        },
    )
    resultats = v.get("query", {}).get("search", [])
    return resultats[0]["title"] if resultats else None


def paragraphe_wikipedia(titre):
    """Le paragraphe de tête de l'article — c'est là qu'un article de genre
    résume texture, instrumentation et rythme ; le reste (histoire,
    sous-genres, artistes) n'apporte rien à une phrase CLAP."""
    v = requete_json(
        WIKIPEDIA_API,
        {
            "action": "query",
            "prop": "extracts",
            "exintro": 1,
            "explaintext": 1,
            "titles": titre,
            "format": "json",
        },
    )
    pages = v.get("query", {}).get("pages", {})
    for page in pages.values():
        extrait = page.get("extract", "").strip()
        if extrait:
            # Le premier paragraphe seulement — les articles longs enchaînent
            # souvent une seconde section (étymologie, controverses de nom)
            # dès l'intro, sans rapport avec ce qu'on entend.
            return extrait.split("\n\n")[0]
    return None


def reecrire_ollama(genre, paragraphe, modele, hote):
    exemples = "\n".join(f"- {g} → {p}" for g, p in EXEMPLES)
    prompt = PROMPT_REECRITURE.format(exemples=exemples, genre=genre, paragraphe=paragraphe)
    req = urllib.request.Request(
        hote,
        data=json.dumps({"model": modele, "prompt": prompt, "stream": False}).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        v = json.loads(r.read().decode("utf-8"))
    phrase = v.get("response", "").strip()
    # Ollama rend parfois plusieurs lignes malgré la consigne — seule la
    # première compte, et sans guillemets ajoutés par le modèle.
    phrase = phrase.splitlines()[0].strip() if phrase else ""
    phrase = phrase.strip('"“” ')
    return phrase or None


def construire_entree(genre, modele_ollama, hote_ollama):
    titre = article_wikipedia(genre)
    if not titre:
        return None, "aucun article Wikipédia trouvé"
    paragraphe = paragraphe_wikipedia(titre)
    if not paragraphe or len(paragraphe) < 40:
        return None, f"paragraphe trop court ou absent ({titre!r})"
    phrase = reecrire_ollama(genre, paragraphe, modele_ollama, hote_ollama)
    if not phrase or len(phrase) < 8:
        return None, f"Ollama n'a rien rendu d'exploitable ({titre!r})"
    return phrase, None


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("genres", help="fichier de genres (un par ligne) ou '-' pour stdin")
    p.add_argument("--modele-ollama", default="qwen2.5:3b")
    p.add_argument("--hote-ollama", default=OLLAMA_API)
    p.add_argument(
        "--sortie",
        default=str(Path(__file__).parent.parent.parent / "crates/analysis/vocabulaire/vocabulaire.bin"),
    )
    args = p.parse_args()

    genres = genres_depuis(args.genres)
    print(f"{len(genres)} genres candidats", file=sys.stderr)

    entrees = []  # (genre, phrase)
    omis = []
    for i, genre in enumerate(genres, 1):
        try:
            phrase, raison = construire_entree(genre, args.modele_ollama, args.hote_ollama)
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as e:
            phrase, raison = None, f"réseau : {e}"
        if phrase:
            entrees.append((genre, phrase))
            print(f"[{i}/{len(genres)}] {genre} → {phrase}", file=sys.stderr)
        else:
            omis.append((genre, raison))
            print(f"[{i}/{len(genres)}] {genre} — omis ({raison})", file=sys.stderr)

    if not entrees:
        print("aucune entrée exploitable — rien à écrire", file=sys.stderr)
        sys.exit(1)

    encoder, _, _ = tour_texte()
    phrases = [p for _, p in entrees]
    v = encoder(phrases)  # v est déjà l2-normalisé par `tour_texte`

    chemin = Path(args.sortie)
    chemin.parent.mkdir(parents=True, exist_ok=True)
    with open(chemin, "wb") as f:
        f.write(v.astype("<f4").tobytes())
    with open(chemin.with_suffix(".txt"), "w", encoding="utf-8") as f:
        f.write("\n".join(f"{g}\t{p}" for g, p in entrees) + "\n")

    taille = chemin.stat().st_size
    print(
        f"\n{len(entrees)} × 512 → {chemin} ({taille} octets, "
        f"{taille / 1024:.1f} Ko) — {len(omis)} genres omis",
        file=sys.stderr,
    )
    if omis:
        print("\nCas résiduels (point 5, docs/nommage-familles.md) :", file=sys.stderr)
        for genre, raison in omis:
            print(f"  {genre} — {raison}", file=sys.stderr)


if __name__ == "__main__":
    main()
