#!/usr/bin/env bash
# Vérification locale avant push — les mêmes contrôles que la CI
# (`.github/workflows/ci.yml`) : forme, politique de dépendances, lint, tests,
# tests de fumée de l'interface.
# Usage : ./scripts/verifier.sh [fmt|deny|clippy|test|fumee]…   (sans argument : tous)
set -euo pipefail
cd "$(dirname "$0")/.."

etapes=("$@")
[ ${#etapes[@]} -eq 0 ] && etapes=(fmt deny clippy test fumee)

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
    fumee)
      # Chrome du système ; `npm ci` seulement la première fois.
      ( cd apps/desktop/ui-tests
        [ -d node_modules ] || npm ci --no-audit --no-fund
        npx playwright test ) ;;
    *)      echo "étape inconnue : $e" >&2; exit 2 ;;
  esac
done
echo "── tout passe"
