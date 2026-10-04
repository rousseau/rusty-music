#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fabrique les fichiers du banc de raccord (`crates/player/examples/verif_gapless.rs`).
#
# Un balayage de fréquence continu (« chirp ») de 6 s, coupé **à l'échantillon
# près** en deux moitiés de 3 s, chaque moitié encodée à part dans chaque
# format — comme deux pistes d'album. Un balayage plutôt qu'un sinus : sa
# fréquence change à chaque instant, donc son décalage se lit sans ambiguïté
# (un sinus de 1 kHz ne distinguerait pas 0 de 1 ms).
#
#   scripts/gapless-fixtures.sh [dossier]     (défaut : /tmp/gapless)
#
# Demande ffmpeg avec libmp3lame, libvorbis et libopus.
set -euo pipefail

SORTIE="${1:-/tmp/gapless}"
SR=44100
COUPE=$((SR * 3))
mkdir -p "$SORTIE"
cd "$SORTIE"

# 0,5·sin(2π(200 t + 1000 t²)) : de 200 Hz à 12,2 kHz en 6 s, sous Nyquist.
EXPR='0.5*sin(2*PI*(200*t+1000*t*t))'
ffmpeg -y -loglevel error -f lavfi \
  -i "aevalsrc=${EXPR}|${EXPR}:s=${SR}:d=6" -c:a pcm_s16le chirp.wav

ffmpeg -y -loglevel error -i chirp.wav \
  -af "atrim=end_sample=${COUPE}" -c:a pcm_s16le gapless-a.wav
ffmpeg -y -loglevel error -i chirp.wav \
  -af "atrim=start_sample=${COUPE},asetpts=PTS-STARTPTS" -c:a pcm_s16le gapless-b.wav

for moitie in a b; do
  src="gapless-${moitie}.wav"
  ffmpeg -y -loglevel error -i "$src" -c:a flac "gapless-${moitie}.flac"
  ffmpeg -y -loglevel error -i "$src" -c:a libmp3lame -b:a 192k "gapless-${moitie}.mp3"
  ffmpeg -y -loglevel error -i "$src" -c:a aac -b:a 192k "gapless-${moitie}.m4a"
  ffmpeg -y -loglevel error -i "$src" -c:a libvorbis -q:a 5 "gapless-${moitie}.ogg"
  ffmpeg -y -loglevel error -i "$src" -c:a libopus -b:a 128k "gapless-${moitie}.opus"
done

echo "Fichiers écrits dans $SORTIE :"
ls "$SORTIE"
