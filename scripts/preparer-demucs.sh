#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Récupère les poids du démixage (module 3).
#
#   ./scripts/preparer-demucs.sh [htdemucs | htdemucs_6s | htdemucs_ft]
#
# En safetensors — huit fois moins lourd que l'export ONNX du même modèle, qui
# embarquait sa transformée de Fourier déroulée. Voir `docs/module3-demixage.md`.
#
# Poids HTDemucs d'origine (Meta, MIT), redistribués par set-soft ; c'est la
# source qu'emploie `demucs-rs`, dont on reprend le chargeur.
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VARIANTE="${1:-htdemucs}"

case "$VARIANTE" in
  htdemucs)     POIDS="84 Mo · 4 stems, un réseau généraliste" ;;
  htdemucs_6s)  POIDS="84 Mo · 6 stems, ajoute guitare et piano" ;;
  htdemucs_ft)  POIDS="333 Mo · 4 stems, un réseau par stem — plus lent, meilleur" ;;
  *)
    echo "Variante inconnue : $VARIANTE" >&2
    echo "Au choix : htdemucs (défaut) · htdemucs_6s · htdemucs_ft" >&2
    exit 1 ;;
esac

SORTIE="$RACINE/models/$VARIANTE.safetensors"
URL="https://huggingface.co/set-soft/audio_separation/resolve/main/Demucs/$VARIANTE.safetensors"

# Empreinte connue de la variante par défaut (celle qu'empaquette la release).
# Les autres ne sont pas vérifiées faute d'avoir été téléchargées ici.
SHA_ATTENDU=""
[ "$VARIANTE" = "htdemucs" ] && \
  SHA_ATTENDU=8193504cdfb3943adaf039b8acb524a46e87ebf232c383ac7a32c80a6578423e

if [ -f "$SORTIE" ]; then
  echo "✓ déjà là : $SORTIE"
  exit 0
fi

mkdir -p "$RACINE/models"
echo "→ $VARIANTE ($POIDS)"
curl -L --fail --retry 3 --retry-delay 5 --progress-bar -o "$SORTIE.partiel" "$URL"
if [ -n "$SHA_ATTENDU" ]; then
  REEL="$(shasum -a 256 "$SORTIE.partiel" | cut -d' ' -f1)"
  if [ "$REEL" != "$SHA_ATTENDU" ]; then
    echo "✗ empreinte inattendue : $REEL (attendu $SHA_ATTENDU)" >&2
    rm -f "$SORTIE.partiel"
    exit 1
  fi
fi
mv "$SORTIE.partiel" "$SORTIE"
echo "✓ $SORTIE"
