#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Construit le paquet macOS (`.app` + `.dmg`) de Rusty Music — de bout en bout,
# en une commande une fois Rust et un compilateur C installés :
#
#   git clone https://github.com/rousseau/rusty-music && cd rusty-music && ./scripts/release.sh
#
#   ./scripts/release.sh [--universal]
#
# Sans option : pour l'architecture de la machine. `--universal` : binaire
# universel (aarch64 + x86_64) — plus long, demande les deux cibles rustup.
#
# Le `.dmg` produit n'est ni signé ni notarisé : au premier lancement, macOS
# le bloque. Contournement documenté dans le README (clic droit → Ouvrir, ou
# `xattr -dr com.apple.quarantine "Rusty Music.app"`).
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$RACINE"

command -v cargo >/dev/null 2>&1 || {
  echo "Rust absent : https://rustup.rs" >&2
  exit 1
}
command -v cc >/dev/null 2>&1 || command -v clang >/dev/null 2>&1 || {
  echo "Compilateur C absent : installer les outils en ligne de commande Xcode" >&2
  echo "  (xcode-select --install)" >&2
  exit 1
}
# Le modèle de langue intégré de Lama (feature `lama-local`) compile llama.cpp.
command -v cmake >/dev/null 2>&1 || {
  echo "cmake absent : brew install cmake (ou pip install cmake)" >&2
  exit 1
}
command -v cargo-tauri >/dev/null 2>&1 || {
  echo "→ tauri-cli (une fois — quelques minutes)"
  cargo install --locked tauri-cli --version "^2"
}

echo "→ modèles et plan de ville"
./scripts/telecharger-modeles.sh
./scripts/telecharger-ville.sh

# Chaque ressource du paquet (`bundle > resources` de tauri.conf.json) doit
# exister : sinon la fonction qui en dépend échoue dans l'app installée, et
# l'on ne s'en aperçoit qu'à l'usage. ADTOF, converti à la main, n'est pas
# téléchargé : `./scripts/preparer-adtof.sh`.
MANQUANTS=""
for source in $(sed -n 's/^ *"\(\.\.\/\.\.\/[^"]*\)": *"[^"]*",\{0,1\}$/\1/p' apps/desktop/tauri.conf.json); do
  [ -f "apps/desktop/$source" ] || MANQUANTS="$MANQUANTS ${source#../../}"
done
if [ -n "$MANQUANTS" ]; then
  echo "Ressources du paquet absentes :$MANQUANTS" >&2
  case "$MANQUANTS" in
    *adtof_frame_rnn.onnx*) echo "  ADTOF : ./scripts/preparer-adtof.sh" >&2 ;;
  esac
  case "$MANQUANTS" in
    *basic_pitch*|*beat_this*|*allinone*) echo "  Pratiquer : ./scripts/telecharger-modeles.sh pratiquer" >&2 ;;
  esac
  exit 1
fi

# Un tableau vide sous `set -u` fait planter bash 3.2 (celui de macOS) à son
# expansion — d'où une chaîne plutôt qu'un tableau pour l'argument optionnel.
CIBLE=""
if [ "${1:-}" = "--universal" ]; then
  for c in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target list --installed | grep -qx "$c" || rustup target add "$c"
  done
  CIBLE="universal-apple-darwin"
fi

if [ -n "$CIBLE" ]; then
  echo "→ cargo tauri build --target $CIBLE --features lama-local"
  cargo tauri build --target "$CIBLE" --features lama-local
else
  echo "→ cargo tauri build --features lama-local"
  cargo tauri build --features lama-local
fi

echo
echo "Paquets produits :"
find target -type d -name bundle -prune -exec find {} -name '*.dmg' -o -name '*.app' \; 2>/dev/null | sort -u
