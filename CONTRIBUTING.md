# Contribuer

Merci de l'intérêt. Ce document dit comment construire le projet et ce qu'on
attend d'une contribution.

## Construire

Prérequis :

- **Rust 1.82+** (`rustup` ; le dépôt épingle `stable` via `rust-toolchain.toml`)
  et un **compilateur C** (`rusqlite` compile SQLite depuis les sources).
- **Les modèles** (~0,35 Go, hors dépôt) :

  ```bash
  ./scripts/telecharger-modeles.sh        # les trois
  ./scripts/telecharger-modeles.sh clap   # suffit pour le binaire `rusty-music` seul
  ```

  `crates/analysis/build.rs` traduit l'encodeur CLAP depuis l'ONNX au moment
  du build : sans au moins ce fichier, rien ne compile. Détail :
  `models/README.md`.

```bash
cargo build --workspace
cargo run -p rusty-music-desktop            # l'application
cargo run -p rusty-music-cli -- --help      # le moteur en ligne de commande
```

## Plan du dépôt

| Crate | Rôle |
|---|---|
| `crates/core` | cœur d'ingestion : scan, tags, surveillance, base SQLite, enrichissement |
| `crates/player` | module 1 — sortie audio, transport, file, amélioration « E » |
| `crates/analysis` | module 2 — empreintes CLAP (Burn), projection 2D, descripteurs |
| `crates/carto` | module 2 — tuiles vectorielles de la carte (MVT / PMTiles) |
| `crates/osm` | import d'un plan de ville OpenStreetMap (jamais lié à l'app) |
| `crates/editor` | module 3 — démixage (`demucs-core` / Burn), étirement, greffe |
| `crates/superres` | super-résolution audio hors ligne (AERO via ONNX Runtime) |
| `crates/cli` | binaire `rusty-music` |
| `apps/desktop` | application Tauri (interface HTML/CSS/JS + WebGL) |

`docs/` porte les décisions et leur pourquoi ; `docs/journal.md` est le journal
de développement ; `experiments/` garde les sondages qui ont tranché les choix.

## Avant de proposer un changement

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check          # licences, avis de sécurité — cargo install --locked cargo-deny
```

Ces trois-là tournent en CI et doivent passer.

**`cargo fmt` n'est pas imposé.** Le style suit le code alentour ; certaines
lignes dépassent 100 colonnes à dessein. Ne reformate pas un fichier entier.

## Style des commits

Messages en français. Une ligne de résumé, puis un corps qui explique le
*pourquoi* et ce qui a été mesuré — pas seulement le *quoi*. Regarde
l'historique pour le ton.

## Licence

En contribuant, tu acceptes que ta contribution soit distribuée sous
**GPL-3.0-or-later**, comme le reste du projet. Ajoute l'en-tête SPDX en tête
de tout nouveau fichier source :

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
```

## Périmètre

Le projet vise une plateforme (**macOS**) pour l'instant. Les correctifs de
portabilité sont bienvenus ; une prise en charge Linux/Windows complète est un
chantier à part, à discuter dans une issue d'abord.
