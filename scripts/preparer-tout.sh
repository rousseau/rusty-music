#!/usr/bin/env bash
# Reconstruit les modèles depuis leurs sources d'origine (au lieu de les
# télécharger prêts avec `telecharger-modeles.sh`).
#
#   ./scripts/preparer-tout.sh
#
# CLAP et HTDemucs se font tout seuls. AERO demande un checkpoint PyTorch que
# tu fournis : lance `scripts/preparer-aero.sh --checkpoint CHEMIN.th` à part.
set -euo pipefail

ICI="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "═══ CLAP ═══"
"$ICI/preparer-modele.sh"

echo
echo "═══ HTDemucs ═══"
"$ICI/preparer-demucs.sh"

echo
echo "═══ AERO ═══"
echo "À faire à part : ./scripts/preparer-aero.sh --checkpoint CHEMIN.th"
echo "(ou ./scripts/telecharger-modeles.sh aero)"
