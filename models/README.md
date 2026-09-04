# Modèles

Ce dossier reçoit les poids des modèles. Il est ignoré par git (sauf ce
fichier). Deux façons de le remplir :

```bash
./scripts/telecharger-modeles.sh        # récupère les trois, empreintes vérifiées
./scripts/telecharger-modeles.sh clap   # suffit pour `cargo build -p rusty-music-cli`
```

ou, pour reconstruire depuis les sources d'origine :
`scripts/preparer-modele.sh`, `scripts/preparer-demucs.sh`,
`scripts/preparer-aero.sh`.

Les fichiers téléchargés sont les *release assets* de la balise `modeles-v1`.

| Fichier | Taille | SHA-256 | Rôle | Consommé par | Licence des **poids** |
|---|---|---|---|---|---|
| `clap-audio-encoder-b5.onnx` | 112 Mo | `9ee45d84…ba5f2244` | encodeur audio de CLAP, formes figées à 5 fenêtres | `crates/analysis/build.rs` (traduit en Rust au build) | Apache-2.0 — `laion/clap-htsat-unfused` |
| `htdemucs.safetensors` | 80 Mo | `8193504c…6578423e` | HTDemucs 4 stems | `crates/editor` (`demucs-core`, à l'exécution) | MIT — poids Meta / HTDemucs |
| `aero-11025-44100.onnx` | 148 Mo | `c1fbe1f9…8b6f339b` | générateur AERO (super-résolution), réseau seul | `crates/superres` (ONNX Runtime, à l'exécution) | voir `slp-rl/aero` |

La licence des poids est **distincte** de celle du code (GPL-3.0-or-later).

Autres fichiers possibles ici, non requis :

- `clap-audio-encoder-b5.bpk` — poids de CLAP au format Burn, **régénérés** par
  `crates/analysis/build.rs` dans `OUT_DIR` puis recopiés ici **à chaque
  build** (c'est cette copie qu'empaquette `cargo tauri build`, et que
  `tauri-build` exige de trouver ici même pour un simple `cargo build`).
- `htdemucs_6s.safetensors`, `htdemucs_ft.safetensors` — variantes de démixage
  (`scripts/preparer-demucs.sh htdemucs_6s`).
