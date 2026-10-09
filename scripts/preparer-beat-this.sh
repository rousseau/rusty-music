#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Récupère les modèles de pulsation « Beat This! » (CPJKU, ISMIR 2024, MIT),
# dans leur export ONNX par `beat-this-rs` (danigb, MIT) — rien à convertir :
# `rten` les lit tels quels.
#
#   ./scripts/preparer-beat-this.sh [complet | petit | tout]
#
# Sans argument : complet (celui de l'application). L'application les
# télécharge aussi d'elle-même au premier usage (`editor::pulsation`) ; ce
# script sert la ligne de commande et les bancs (`experiments/pulsation/`).
# Mêmes URL et empreintes que `crates/editor/src/pulsation.rs` — les garder
# d'accord.
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REV="1ae768e78f1ad83b0ed3886241dc29ffde853c40"
BRUT="https://raw.githubusercontent.com/danigb/beat-this-rs/$REV/models"
mkdir -p "$RACINE/models"

prendre() { # nom url sha256
  local dest="$RACINE/models/$1"
  if [ -f "$dest" ] && [ "$(shasum -a 256 "$dest" | cut -d' ' -f1)" = "$3" ]; then
    echo "✓ $1 (déjà là)"; return
  fi
  curl -fL --retry 3 -o "$dest.partiel" "$2"
  local reel; reel="$(shasum -a 256 "$dest.partiel" | cut -d' ' -f1)"
  if [ "$reel" != "$3" ]; then
    echo "✗ empreinte inattendue pour $1 : $reel" >&2; rm -f "$dest.partiel"; exit 1
  fi
  mv "$dest.partiel" "$dest"; echo "✓ $1"
}

prendre beat_this_mel.onnx "$BRUT/mel_spectrogram.onnx" fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9
GRAND="https://github.com/danigb/beat-this-rs/releases/download/model-large/beat_this.onnx"
case "${1:-complet}" in
  complet) prendre beat_this.onnx "$GRAND" 5f810debe53459b559127fb55bbad40035bb47cc567b20e501670f968c770f02 ;;
  petit)   prendre beat_this_small.onnx "$BRUT/beat_this_small.onnx" a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f ;;
  tout)    prendre beat_this.onnx "$GRAND" 5f810debe53459b559127fb55bbad40035bb47cc567b20e501670f968c770f02
           prendre beat_this_small.onnx "$BRUT/beat_this_small.onnx" a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f ;;
  *) echo "Au choix : complet · petit · tout" >&2; exit 1 ;;
esac
