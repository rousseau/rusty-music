#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Récupère les modèles déjà préparés depuis les *release assets* du dépôt.
#
#   ./scripts/telecharger-modeles.sh [clap | demucs | aero | pratiquer | tout]
#
# Sans argument : tout. `clap` seul suffit pour compiler (`crates/analysis`
# traduit l'encodeur au build) ; `demucs` et `aero` sont nécessaires à
# l'exécution du démixage et du bouton « HD » ; `pratiquer` (Basic Pitch, Beat
# This! mel et petit, les 8 plis d'all-in-one) est embarqué dans le paquet
# (V25) : ces fichiers-là viennent de leur source, à la révision et à
# l'empreinte que fixe le code Rust (le test `paquet` du bureau le vérifie).
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
    clap-text-encoder.onnx)     echo c4c6f19cb0939afc608b2ad96db55b78b2faaf119eec2a48b3d5d07cda2ce840 ;;
    htdemucs.safetensors)       echo 8193504cdfb3943adaf039b8acb524a46e87ebf232c383ac7a32c80a6578423e ;;
    aero-11025-44100.onnx)      echo c1fbe1f9c79553978d82a1fea31e2bb1d2d72d2bcfea94cee3d249688b6f339b ;;
    basic_pitch_nmp.onnx) echo 2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec ;;
    beat_this_mel.onnx) echo fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9 ;;
    beat_this_small.onnx) echo a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f ;;
    allinone-harmonix-fold0.safetensors) echo d6c6e2ac3ad7c73a203f71bfd7b45573cb2b0c20df454dfec23acf00c932d692 ;;
    allinone-harmonix-fold1.safetensors) echo c42d5e084ff67f911a9ec7776abf60f75f2fde7d04fa26059719ff52d8626087 ;;
    allinone-harmonix-fold2.safetensors) echo 89cf75907eced2552f3abfdd23d9c185bf0c2e396104ba6fc12221dc8444187d ;;
    allinone-harmonix-fold3.safetensors) echo 4da2595e41ab7056376579f619a2abb632b5a797fbb2e34e47f6403f62a453d5 ;;
    allinone-harmonix-fold4.safetensors) echo fdc5cdfe463197f6f74a5b7e64e6c2cb20960ba3de87eff8ee93e3eb49879d46 ;;
    allinone-harmonix-fold5.safetensors) echo 1cc098cd01c939e21a39a6f11eaa569d029e1e556f95706ab3fb2a991b31c60a ;;
    allinone-harmonix-fold6.safetensors) echo 22a78110f68deec1011c15cfb39c8c66e11a542e0548d7c1432577c04e186a6e ;;
    allinone-harmonix-fold7.safetensors) echo e206806a9272e3f757cf35be74e43235b1d138d00989af74daf10e683333de72 ;;
  esac
}

# Les poids embarqués pour Pratiquer ne sont pas dans les *release assets* :
# leur adresse d'origine, épinglée. Vide pour les autres.
url_source() {
  case "$1" in
    basic_pitch_nmp.onnx) echo https://raw.githubusercontent.com/spotify/basic-pitch/fa5997af0a8210982619003269994a1be25eddf3/basic_pitch/saved_models/icassp_2022/nmp.onnx ;;
    beat_this_mel.onnx) echo https://raw.githubusercontent.com/danigb/beat-this-rs/1ae768e78f1ad83b0ed3886241dc29ffde853c40/models/mel_spectrogram.onnx ;;
    beat_this_small.onnx) echo https://raw.githubusercontent.com/danigb/beat-this-rs/1ae768e78f1ad83b0ed3886241dc29ffde853c40/models/beat_this_small.onnx ;;
    allinone-harmonix-fold0.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold0.safetensors ;;
    allinone-harmonix-fold1.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold1.safetensors ;;
    allinone-harmonix-fold2.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold2.safetensors ;;
    allinone-harmonix-fold3.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold3.safetensors ;;
    allinone-harmonix-fold4.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold4.safetensors ;;
    allinone-harmonix-fold5.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold5.safetensors ;;
    allinone-harmonix-fold6.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold6.safetensors ;;
    allinone-harmonix-fold7.safetensors) echo https://huggingface.co/rousseau/allinone-harmonix/resolve/fd77931c07f8e7bf56244f869c4f3576092eeb2e/allinone-harmonix-fold7.safetensors ;;
  esac
}

PRATIQUER="basic_pitch_nmp.onnx beat_this_mel.onnx beat_this_small.onnx allinone-harmonix-fold0.safetensors allinone-harmonix-fold1.safetensors allinone-harmonix-fold2.safetensors allinone-harmonix-fold3.safetensors allinone-harmonix-fold4.safetensors allinone-harmonix-fold5.safetensors allinone-harmonix-fold6.safetensors allinone-harmonix-fold7.safetensors"

case "${1:-tout}" in
  clap)   FICHIERS="clap-audio-encoder-b5.onnx clap-text-encoder.onnx" ;;
  demucs) FICHIERS="htdemucs.safetensors" ;;
  aero)   FICHIERS="aero-11025-44100.onnx" ;;
  pratiquer) FICHIERS="$PRATIQUER" ;;
  tout)   FICHIERS="clap-audio-encoder-b5.onnx clap-text-encoder.onnx htdemucs.safetensors aero-11025-44100.onnx $PRATIQUER" ;;
  *)
    echo "Argument inconnu : $1" >&2
    echo "Au choix : clap · demucs · aero · pratiquer · tout (défaut)" >&2
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
  source_="$(url_source "$f")"
  if [ -n "$source_" ]; then
    curl -L --fail --retry 3 --retry-delay 5 --progress-bar -o "$dest.partiel" "$source_"
    mv "$dest.partiel" "$dest"
  elif [ "$VIA_GH" = 1 ]; then
    gh release download "$TAG" --repo rousseau/rusty-music --pattern "$f" --dir "$RACINE/models" --clobber
  else
    curl -L --fail --retry 3 --retry-delay 5 --progress-bar -o "$dest.partiel" "$URL_BASE/$f"
    mv "$dest.partiel" "$dest"
  fi
  verifier "$dest" "$sha"
  echo "✓ models/$f"
done
