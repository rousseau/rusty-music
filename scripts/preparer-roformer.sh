#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Récupère les poids du démixage BS-RoFormer « SW » (6 stems, module 3).
#
#   ./scripts/preparer-roformer.sh
#
# Le `.ckpt` PyTorch est lu tel quel par `crates/editor/src/roformer/`
# (`burn-store`) : rien à convertir. Et rien à republier — la licence de ces
# poids n'est pas déclarée (`license: unknown`) et leur auteur n'est pas
# connu (`docs/module3-roformer.md`) : on les télécharge à la source, sur
# une révision épinglée, et on vérifie leur empreinte.
#
# Source : le miroir `enerjazzer/BS-ROFO-SW-Fixed` (le dépôt d'origine,
# celui de `jarredou`, a été supprimé). `FractalSurfer/BS-RoFormer-SW-6stem`
# sert les mêmes octets ; il prend le relais si le premier disparaît.
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FICHIER=BS-Rofo-SW-Fixed.ckpt
SORTIE="$RACINE/models/$FICHIER"
SHA_ATTENDU=24e7d35ee9c64415673d3fd33e06a67cac2c103c5df6267ba1576459c775916e
SOURCES=(
  "https://huggingface.co/enerjazzer/BS-ROFO-SW-Fixed/resolve/a443a2985534b3bc815ef54a5d446c6a0390f974/$FICHIER"
  "https://huggingface.co/FractalSurfer/BS-RoFormer-SW-6stem/resolve/2b98254b19bdab07ce225d884ec4aa9dd8aafce1/$FICHIER"
)

if [ -f "$SORTIE" ]; then
  echo "✓ déjà là : $SORTIE"
  exit 0
fi

echo "→ BS-RoFormer SW (699 Mo · 6 stems : basse, batterie, autre, voix, guitare, piano)"
echo "  Licence non déclarée, provenance inconnue : usage personnel, ne pas redistribuer."
mkdir -p "$RACINE/models"
for URL in "${SOURCES[@]}"; do
  if curl -L --fail --retry 3 --retry-delay 5 --progress-bar -o "$SORTIE.partiel" "$URL"; then
    REEL="$(shasum -a 256 "$SORTIE.partiel" | cut -d' ' -f1)"
    if [ "$REEL" = "$SHA_ATTENDU" ]; then
      mv "$SORTIE.partiel" "$SORTIE"
      echo "✓ $SORTIE"
      exit 0
    fi
    echo "✗ empreinte inattendue depuis $URL : $REEL" >&2
  fi
  rm -f "$SORTIE.partiel"
done
echo "✗ aucune source n'a servi les poids attendus" >&2
exit 1
