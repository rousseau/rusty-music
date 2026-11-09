#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Rend les fichiers de l'écoute à l'aveugle d'E3 sur le disque de dev.
#
#   ./rendre-ecoute.sh            # RUSTY_DEV=/Volumes/LaCieDock/rusty_music-dev par défaut
#
# 6 extraits × 5 réglages × 3 méthodes = 90 fichiers WAV (≈ 450 Mo) : ils vont
# sur RUSTY_DEV, jamais sur le SSD. Dossier d'écoute et clé sont séparés :
#   $RUSTY_DEV/banc-etirement/ecoute/<n>-<extrait>/<réglage>-{A,B,C}.wav
#   $RUSTY_DEV/banc-etirement/cle/cle.tsv  (et sources.tsv)
set -euo pipefail
cd "$(dirname "$0")"

RUSTY_DEV=${RUSTY_DEV:-/Volumes/LaCieDock/rusty_music-dev}
[ -d "$RUSTY_DEV" ] || { echo "RUSTY_DEV absent ($RUSTY_DEV) : brancher le disque" >&2; exit 2; }

MUSIQUE=${MUSIQUE:-$HOME/Music/MyMusic}
STEMS=${STEMS:-"$HOME/Library/Application Support/fm.rustymusic.desktop/stems"}
SORTIE="$RUSTY_DEV/banc-etirement"
[ -e "$SORTIE/ecoute" ] && { echo "$SORTIE/ecoute existe déjà : rien écrasé" >&2; exit 3; }
mkdir -p "$SORTIE/cle"

# Les stems se lisent sans les modifier ; la bibliothèque aussi.
LISTE="$SORTIE/cle/liste.tsv"
{
  printf 'batterie\t%s\n' "$STEMS/07 Black Crow/07 Black Crow — drums.wav"
  printf 'basse\t%s\n'    "$STEMS/05 Love Foolosophy/05 Love Foolosophy — bass.wav"
  printf 'pop-dense\t%s\n' "$MUSIQUE/Lizzo/Special/02 About Damn Time.mp3"
  printf 'piano\t%s\n'    "$MUSIQUE/Mozart/The Best of Mozart/00 Sonata in A, K. 331.mp3"
  printf 'voix\t%s\n'     "$STEMS/01 Donne Moi Ça/01 Donne Moi Ça — vocals.wav"
  printf 'jazz\t%s\n'     "$MUSIQUE/Bill Evans/Evans in England/01 Our Love Is Here to Stay.mp3"
} > "$LISTE"

cargo build --release --quiet
./target/release/banc-etirement ecoute "$SORTIE/ecoute" "$SORTIE/cle/cle.tsv" "$LISTE"
du -sh "$SORTIE/ecoute"
