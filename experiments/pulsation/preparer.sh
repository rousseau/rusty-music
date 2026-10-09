#!/bin/sh
# Annotations de temps et de premiers temps de GTZAN (GitHub
# `TempoBeatDownbeat/gtzan_tempo_beat`, dossier `beats/`, un fichier par clip :
# « instant <TAB> rang dans la mesure », 1 = premier temps). L'audio est celui
# du banc tempo : lancer d'abord `experiments/tempo/preparer.sh`.
# Rien n'est copié dans le dépôt : ni GTZAN ni ces annotations n'ont de licence
# déclarée, on ne s'en sert qu'en local.
set -e
cd "$(dirname "$0")"
mkdir -p data/beats && cd data/beats
python3 - <<'PY'
import json, urllib.request, os
for x in json.load(urllib.request.urlopen("https://api.github.com/repos/TempoBeatDownbeat/gtzan_tempo_beat/contents/beats")):
    if not os.path.exists(x["name"]): urllib.request.urlretrieve(x["download_url"], x["name"])
print(len(os.listdir(".")), "annotations")
PY
