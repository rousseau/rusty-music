#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prépare la tour texte de CLAP pour l'import Burn — le pendant, côté texte,
# de `preparer-modele.sh`.
#
# À jouer une fois, et à rejouer seulement si la longueur de séquence change.
# Le résultat, `models/clap-text-encoder.onnx` (478 Mo), est ce que lit le
# `build.rs` de `crates/analysis` — voir `crates/analysis/src/
# encodeur_texte.rs` pour ce qui en est fait à l'exécution (toujours en CPU,
# une requête par prompt de playlist du champ d'intention d'Explorer).
#
# Contrairement à l'audio, rien à figer manuellement : `sonder.py export`
# (dans `experiments/clap-texte/`) fait tout — longueur de séquence fixée à
# 32 jetons, repliage des constantes par ONNX Runtime (`ORT_ENABLE_BASIC`,
# même recette que `preparer-modele.sh`), vérifié contre PyTorch avant de
# rendre la main. Voir `experiments/clap-texte/README.md` pour le détail de
# l'essai qui l'a établi (cosinus 0,9999994636, aucune marge calculée à
# l'exécution, aucun opérateur hors du domaine standard).
#
# Le tokeniseur (`crates/analysis/tokenizer/tokenizer.json`, 2 Mo) est commité
# séparément — un fichier de vocabulaire, pas un poids de modèle — et n'a rien
# à faire ici s'il est déjà en place.

set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SORTIE="$RACINE/models/clap-text-encoder.onnx"
LONGUEUR=32

if [ -f "$SORTIE" ]; then
  echo "✓ déjà là : $SORTIE"
  exit 0
fi

# Environnement Python jetable, comme `preparer-modele.sh` : il ne sert qu'à
# cette préparation, il n'a rien à faire dans les dépendances du projet.
VENV="${TMPDIR:-/tmp}/rusty-music-clap-texte"
if [ ! -x "$VENV/bin/python" ]; then
  echo "→ environnement Python de préparation"
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet --upgrade pip
  "$VENV/bin/pip" install --quiet torch transformers onnx onnxruntime numpy
fi

cd "$RACINE/experiments/clap-texte"
echo "→ export ONNX, longueur de séquence figée à $LONGUEUR"
"$VENV/bin/python" sonder.py export --longueur "$LONGUEUR" --sortie "$SORTIE"

echo "✓ $SORTIE"
