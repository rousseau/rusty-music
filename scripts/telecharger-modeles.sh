#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Récupère les modèles déjà préparés depuis les *release assets* du dépôt.
#
#   ./scripts/telecharger-modeles.sh [clap | demucs | aero | tout]
#
# Sans argument : tout. `clap` seul suffit pour compiler (`crates/analysis`
# traduit l'encodeur au build) ; `demucs` et `aero` sont nécessaires à
# l'exécution du démixage et du bouton « HD ».
#
# Ces fichiers sont la sortie de `scripts/preparer-*.sh`, mis en ligne une fois
# pour éviter à chacun de refaire la préparation (venv Python, onnxruntime) et
# de se heurter au débit limité de Hugging Face. Pour les reconstruire depuis
# les sources : voir les `preparer-*.sh`.
#
# Utilise `gh release download` si `gh` est disponible et connecté (nécessaire
# tant que le dépôt est privé), sinon `curl` sur l'URL publique.
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAG="modeles-v1"
URL_BASE="https://github.com/rousseau/rusty-music/releases/download/$TAG"

sha_attendu() {
  case "$1" in
    clap-audio-encoder-b5.onnx) echo 9ee45d84b5765e79a26430ed12e9a494b252a6d255aa6d4f6fa9a865ba5f2244 ;;
    htdemucs.safetensors)       echo 8193504cdfb3943adaf039b8acb524a46e87ebf232c383ac7a32c80a6578423e ;;
    aero-11025-44100.onnx)      echo c1fbe1f9c79553978d82a1fea31e2bb1d2d72d2bcfea94cee3d249688b6f339b ;;
  esac
}

case "${1:-tout}" in
  clap)   FICHIERS="clap-audio-encoder-b5.onnx" ;;
  demucs) FICHIERS="htdemucs.safetensors" ;;
  aero)   FICHIERS="aero-11025-44100.onnx" ;;
  tout)   FICHIERS="clap-audio-encoder-b5.onnx htdemucs.safetensors aero-11025-44100.onnx" ;;
  *)
    echo "Argument inconnu : $1" >&2
    echo "Au choix : clap · demucs · aero · tout (défaut)" >&2
    exit 1 ;;
esac

verifier() { # fichier attendu
  local reel
  reel="$(shasum -a 256 "$1" | cut -d' ' -f1)"
  if [ "$reel" != "$2" ]; then
    echo "✗ empreinte inattendue pour $1" >&2
    echo "  attendu $2" >&2
    echo "  obtenu  $reel" >&2
    return 1
  fi
}

VIA_GH=0
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  VIA_GH=1
fi

mkdir -p "$RACINE/models"
for f in $FICHIERS; do
  dest="$RACINE/models/$f"
  sha="$(sha_attendu "$f")"
  if [ -f "$dest" ] && verifier "$dest" "$sha" 2>/dev/null; then
    echo "✓ déjà là : models/$f"
    continue
  fi
  echo "→ models/$f"
  if [ "$VIA_GH" = 1 ]; then
    gh release download "$TAG" --repo rousseau/rusty-music --pattern "$f" --dir "$RACINE/models" --clobber
  else
    curl -L --fail --retry 3 --retry-delay 5 --progress-bar -o "$dest.partiel" "$URL_BASE/$f"
    mv "$dest.partiel" "$dest"
  fi
  verifier "$dest" "$sha"
  echo "✓ models/$f"
done
