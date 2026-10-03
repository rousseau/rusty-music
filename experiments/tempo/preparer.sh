#!/bin/sh
# Télécharge GTZAN (audio, ~1,2 Go, Hugging Face `marsyas/gtzan`) et ses
# annotations de tempo (GitHub `TempoBeatDownbeat/gtzan_tempo_beat`, un .bpm par
# clip) dans experiments/tempo/data/. Rien n'est copié dans le dépôt : GTZAN n'a
# pas de licence déclarée (jeu de recherche), on ne l'utilise qu'en local.
set -e
cd "$(dirname "$0")"
mkdir -p data/bpm && cd data
[ -f genres.tar.gz ] || curl -L -C - -o genres.tar.gz \
  https://huggingface.co/datasets/marsyas/gtzan/resolve/main/data/genres.tar.gz
[ -d genres ] || tar xzf genres.tar.gz
cd bpm
python3 - <<'PY'
import json, urllib.request, os
for x in json.load(urllib.request.urlopen("https://api.github.com/repos/TempoBeatDownbeat/gtzan_tempo_beat/contents/tempo")):
    if not os.path.exists(x["name"]): urllib.request.urlretrieve(x["download_url"], x["name"])
print(len(os.listdir(".")), "annotations")
PY
