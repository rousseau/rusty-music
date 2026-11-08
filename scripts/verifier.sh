#!/usr/bin/env bash
# Vérification locale avant push — les mêmes contrôles que la CI
# (`.github/workflows/ci.yml`) : forme, politique de dépendances, lint, tests,
# tests de fumée de l'interface ; en plus, ici seulement, le compagnon iOS
# compilé pour l'iPhone (`ios`).
# Usage : ./scripts/verifier.sh [fmt|deny|clippy|test|ios|fumee]…   (sans argument : tous)
set -euo pipefail
cd "$(dirname "$0")/.."

etapes=("$@")
[ ${#etapes[@]} -eq 0 ] && etapes=(fmt deny clippy test ios fumee)

# Sans les deux tours de CLAP, `crates/analysis` se compile avec des bouchons
# (`sans-modeles`) : tout se vérifie, rien ne calcule d'empreinte.
options=()
if [ ! -f models/clap-audio-encoder-b5.onnx ] || [ ! -f models/clap-text-encoder.onnx ]; then
  echo "(modèles CLAP absents : vérification avec --features rusty-music-analysis/sans-modeles)"
  options=(--features rusty-music-analysis/sans-modeles)
fi

for e in "${etapes[@]}"; do
  echo "── $e"
  case "$e" in
    fmt)    cargo fmt --all -- --check ;;
    deny)   cargo deny check ;;
    clippy) cargo clippy --workspace --all-targets ${options[@]+"${options[@]}"} -- -D warnings ;;
    test)   cargo test --workspace ${options[@]+"${options[@]}"} ;;
    ios)
      # Le compagnon (`apps/mobile`) ne compile pour iOS que dans Xcode : un
      # changement des crates qu'il lie (core, valise, player) n'y casserait
      # rien de visible ailleurs. Sans la cible Rust, l'étape est sautée.
      if rustup target list --installed 2>/dev/null | grep -qx aarch64-apple-ios; then
        cargo check -p rusty-music-mobile --target aarch64-apple-ios
      else
        echo "(cible aarch64-apple-ios absente : rustup target add aarch64-apple-ios)"
      fi ;;
    fumee)
      # Chrome du système ; `npm ci` seulement la première fois. D'abord les
      # réponses figées contre les types des commandes (`types/`, régénérés
      # par l'étape `test`), puis les tests eux-mêmes.
      ( cd apps/desktop/ui-tests
        [ -x node_modules/.bin/tsc ] || npm ci --no-audit --no-fund
        npm run --silent types
        npx playwright test ) ;;
    *)      echo "étape inconnue : $e" >&2; exit 2 ;;
  esac
done
echo "── tout passe"
