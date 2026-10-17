#!/usr/bin/env bash
# Vérification locale avant push — remplace l'ancienne CI GitHub Actions.
# Les trois contrôles de CONTRIBUTING.md : politique de dépendances, lint, tests.
# Usage : ./scripts/verifier.sh [deny|clippy|test]…   (sans argument : les trois)
set -euo pipefail
cd "$(dirname "$0")/.."

etapes=("$@")
[ ${#etapes[@]} -eq 0 ] && etapes=(deny clippy test)

# `crates/analysis` ne compile pas sans le modèle CLAP (voir README, « Démarrer »).
if [[ " ${etapes[*]} " =~ " clippy " || " ${etapes[*]} " =~ " test " ]] \
    && [ ! -f models/clap-audio-encoder-b5.onnx ]; then
  echo "Modèles absents : ./scripts/telecharger-modeles.sh" >&2
  exit 1
fi

for e in "${etapes[@]}"; do
  echo "── $e"
  case "$e" in
    deny)   cargo deny check ;;
    clippy) cargo clippy --workspace --all-targets -- -D warnings ;;
    test)   cargo test --workspace ;;
    *)      echo "étape inconnue : $e" >&2; exit 2 ;;
  esac
done
echo "── tout passe"
