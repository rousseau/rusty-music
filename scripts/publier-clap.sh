#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Publie les poids de CLAP au format Burn (`models/*.bpk`, produits par un
# build `release`) sur Hugging Face, d'où l'application les télécharge au
# premier lancement (`crates/analysis/src/poids.rs`).
#
#   hf auth login          # une fois, jeton en écriture
#   ./scripts/publier-clap.sh
#
# À refaire quand `release.sh` signale que les poids publiés ne vont plus avec
# le code (nouvelle version de burn-onnx, ONNX repréparé). Le script affiche
# la révision et les empreintes à reporter dans `poids.rs`.
set -euo pipefail

RACINE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$RACINE"

DEPOT="rousseau/clap-htsat-unfused-burn"
FICHIERS="clap-audio-encoder-b5.bpk clap-text-encoder.bpk"

command -v hf >/dev/null 2>&1 || { echo "hf absent : pip install -U huggingface_hub" >&2; exit 1; }
hf auth whoami >/dev/null 2>&1 || { echo "Pas connecté : hf auth login" >&2; exit 1; }
for f in $FICHIERS; do
  [ -f "models/$f" ] || { echo "models/$f absent : cargo build --release -p rusty-music-analysis" >&2; exit 1; }
done

CARTE="$(mktemp -d)/README.md"
cat > "$CARTE" <<'CARTE'
---
license: apache-2.0
library_name: burn
tags:
  - audio
  - clap
  - burn
base_model: laion/clap-htsat-unfused
---

# CLAP (htsat-unfused), au format Burn

Les deux tours de [`laion/clap-htsat-unfused`](https://huggingface.co/laion/clap-htsat-unfused)
(Wu et al., « Large-scale Contrastive Language-Audio Pretraining with Feature
Fusion and Keyword-to-Caption Augmentation », ICASSP 2023), converties pour
[Burn](https://github.com/tracel-ai/burn) par `burn-onnx`, telles que les
charge [Rusty Music](https://github.com/rousseau/rusty-music) :

- `clap-audio-encoder-b5.bpk` : encodeur audio, figé sur des lots de 5 fenêtres ;
- `clap-text-encoder.bpk` : encodeur texte, figé sur 77 jetons.

Ces fichiers ne valent qu'avec le code Rust généré par la même version de
`burn-onnx` depuis les mêmes ONNX (`scripts/preparer-modele.sh`,
`scripts/preparer-clap-texte.sh` du dépôt). Licence d'origine : Apache-2.0.
CARTE

echo "→ $DEPOT"
hf repo create "$DEPOT" --repo-type model --exist-ok >/dev/null
hf upload "$DEPOT" "$CARTE" README.md --commit-message "Fiche du modèle"
for f in $FICHIERS; do
  hf upload "$DEPOT" "models/$f" "$f" --commit-message "$f"
done

# La révision du dernier envoi : python seulement pour la lire (outil de
# préparation, jamais à l'exécution de l'application).
REVISION=$(python3 -c "from huggingface_hub import HfApi; print(HfApi().model_info('$DEPOT').sha)" 2>/dev/null || true)
echo
echo "À reporter dans crates/analysis/src/poids.rs :"
echo "  REVISION = ${REVISION:-<voir https://huggingface.co/$DEPOT/commits/main>}"
for f in $FICHIERS; do
  meta=$(od -An -tu4 -j6 -N4 "models/$f" | tr -d ' ')
  debut=$(( (10 + meta + 255) / 256 * 256 ))
  echo "  $f"
  echo "    octets         $(stat -f %z "models/$f" 2>/dev/null || stat -c %s "models/$f")"
  echo "    sha256         $(shasum -a 256 "models/$f" | cut -d' ' -f1)"
  echo "    sha256_donnees $(tail -c +$((debut + 1)) "models/$f" | shasum -a 256 | cut -d' ' -f1)"
done
