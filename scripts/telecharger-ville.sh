#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Récupère le plan de Paris (`ville-paris.db`) depuis les release assets.
#
#   ./scripts/telecharger-ville.sh
#
# La carte de la v0.1.0 est le plan de Paris importé d'OpenStreetMap. La base
# fait ~56 Mo, hors dépôt (`.gitignore`). L'application la préfère si elle est
# à côté de la base de la bibliothèque, sinon carte procédurale de repli.
#
# Reconstruire depuis un extrait OSM :
#   cargo run --release -p rusty-music-cli -- ville <ile-de-france.osm.pbf> \
#     --commune Paris --sortie ville-paris.db
#
# Données © les contributeurs OpenStreetMap, sous ODbL. Attribution obligatoire
# à l'affichage (l'application la porte dans le coin de la carte).
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAG="ville-paris-v1"
FICHIER="ville-paris.db"
SHA=3ca00caff626f67bec43440da80a626cdae625818af18717aedc98a4e50f7c40
DEST="$RACINE/$FICHIER"

verifier() {
  local reel
  reel="$(shasum -a 256 "$1" | cut -d' ' -f1)"
  [ "$reel" = "$SHA" ] || { echo "✗ empreinte inattendue : $reel (attendu $SHA)" >&2; return 1; }
}

if [ -f "$DEST" ] && verifier "$DEST" 2>/dev/null; then
  echo "✓ déjà là : $FICHIER"
  exit 0
fi

echo "→ $FICHIER"
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  gh release download "$TAG" --repo rousseau/rusty-music --pattern "$FICHIER" --dir "$RACINE" --clobber
else
  curl -L --fail --retry 3 --retry-delay 5 --progress-bar \
    -o "$DEST.partiel" "https://github.com/rousseau/rusty-music/releases/download/$TAG/$FICHIER"
  mv "$DEST.partiel" "$DEST"
fi
verifier "$DEST"
echo "✓ $FICHIER"
