#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prépare le modèle de batterie ADTOF « Frame_RNN » (Zehren et al., Signals
# 2023) pour `crates/transcription::batterie` : poids TensorFlow publiés →
# ONNX, dans `models/adtof_frame_rnn.onnx`.
#
#   ./scripts/preparer-adtof.sh
#
# Licence : le dépôt ADTOF, poids compris, est sous CC BY-NC-SA 4.0. On
# n'utilise que les poids (l'architecture est réécrite dans
# `experiments/batterie/exporter_adtof.py`) ; l'ONNX produit reste sous
# CC BY-NC-SA 4.0 — voir MODELES.md. Python ne sert qu'ici, à la préparation.
#
# Prérequis : `uv` (https://docs.astral.sh/uv/). Environnement Python 3.11
# jetable dans un dossier temporaire : TensorFlow 2.15, tf2onnx, madmom.
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REV="b3968fb332f69b65ee07c089fc62f436503755db"
BASE="https://raw.githubusercontent.com/MZehren/ADTOF/$REV/adtof/models"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Poids ADTOF (révision ${REV:0:7})…"
curl -fsSL -o "$TMP/Frame_RNN_adtofAll_0.index" "$BASE/Frame_RNN_adtofAll_0.index"
curl -fsSL -o "$TMP/Frame_RNN_adtofAll_0.data-00000-of-00001" "$BASE/Frame_RNN_adtofAll_0.data-00000-of-00001"

echo "Environnement de préparation (TensorFlow, tf2onnx, madmom)…"
uv venv -q -p 3.11 "$TMP/venv"
uv pip install -q -p "$TMP/venv/bin/python" "tensorflow==2.15.1" "tf2onnx==1.16.1" "numpy==1.26.4" "cython<3.1" scipy mido setuptools
uv pip install -q -p "$TMP/venv/bin/python" --no-build-isolation "madmom @ git+https://github.com/CPJKU/madmom"

mkdir -p "$RACINE/models"
"$TMP/venv/bin/python" "$RACINE/experiments/batterie/exporter_adtof.py" \
  "$TMP/Frame_RNN_adtofAll_0" "$RACINE/models/adtof_frame_rnn.onnx" 2>&1 | grep -vE "^WARNING|^I0000|^W0000|tensorflow|oneDNN|absl" || true
shasum -a 256 "$RACINE/models/adtof_frame_rnn.onnx"
