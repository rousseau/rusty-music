#!/usr/bin/env python3
"""Prépare le jeu de prompts MusicRecoIntent pour `robustesse` (Lama).

MusicRecoIntent (Deezer, NLP4MusA 2026, arXiv 2602.12301) : 2 291 requêtes
Reddit annotées de descripteurs Genre / Mood / Instrument / ListeningContext /
Decade / Country / NE, chacun marqué `+` (voulu), `-` (rejeté), `~` (référence,
« comme X »). Licence non précisée par le dépôt : les données ne sont donc
jamais copiées dans ce dépôt (`data/` est ignoré), seulement téléchargées.

    python3 experiments/musicrecointent/preparer.py [N=300] [graine=1]

Écrit data/cas.json (au format de prompts-playlist/prompts.json, sans
`attendu` : la notation se fait dans noter.py) et data/annotations.json.
Garde **toutes** les requêtes à négation (46 sur 2 291), complète au hasard
parmi celles qui ont au moins un descripteur que le schéma sait exprimer.
"""
import ast, csv, json, os, random, sys, urllib.request

URL = "https://raw.githubusercontent.com/deezer/MusicRecoIntent-NLP4MusA26/main/MusicRecoIntent_dataset.csv"
ICI = os.path.dirname(os.path.abspath(__file__))
DATA = os.path.join(ICI, "data")
EXPRIMABLES = {"Genre", "Decade", "NE"}

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 300
    random.seed(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
    os.makedirs(DATA, exist_ok=True)
    brut = os.path.join(DATA, "MusicRecoIntent_dataset.csv")
    if not os.path.exists(brut):
        urllib.request.urlretrieve(URL, brut)
    lignes = list(csv.DictReader(open(brut, encoding="utf-8")))
    annot = {}
    for r in lignes:
        d = ast.literal_eval(r["intents"])
        annot[r["ID"]] = {"query": r["query"], "desc": [(c.lower() if c in ("country",) else c, t, p) for c, v in d.items() for (t, p) in v]}
    negatives = [i for i, a in annot.items() if any(p == "-" for _, _, p in a["desc"])]
    autres = [i for i, a in annot.items() if i not in negatives and any(c in EXPRIMABLES for c, _, _ in a["desc"])]
    random.shuffle(autres)
    choix = negatives + autres[: max(0, n - len(negatives))]
    cas = [{"id": i, "categorie": "negation" if i in negatives else "autre", "prompt": annot[i]["query"]} for i in choix]
    json.dump({"_doc": "MusicRecoIntent — voir preparer.py", "cas": cas}, open(os.path.join(DATA, "cas.json"), "w"), ensure_ascii=False, indent=1)
    json.dump({i: annot[i] for i in choix}, open(os.path.join(DATA, "annotations.json"), "w"), ensure_ascii=False, indent=1)
    print(f"{len(cas)} prompts ({len(negatives)} à négation) -> {DATA}/cas.json")

main()
