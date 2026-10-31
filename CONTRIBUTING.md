# Contribuer

Merci de l'intérêt. Ce document dit comment construire le projet et ce qu'on
attend d'une contribution.

## Construire

Prérequis :

- **Rust 1.82+** (`rustup` ; le dépôt épingle `stable` via `rust-toolchain.toml`)
  et un **compilateur C** (`rusqlite` compile SQLite depuis les sources).
- **Les modèles**, hors dépôt (liste et tailles : `README.md`, section
  « Modèles ») :

  ```bash
  ./scripts/telecharger-modeles.sh        # CLAP audio + texte, HTDemucs, AERO (~0,86 Go)
  ./scripts/telecharger-modeles.sh clap   # les deux tours de CLAP : de quoi compiler
  ```

  `crates/analysis/build.rs` traduit les deux tours de CLAP depuis l'ONNX au
  moment du build. **Sans elles**, le workspace compile et se teste quand
  même avec `--features rusty-music-analysis/sans-modeles` (encodeurs
  remplacés par des bouchons qui refusent de se charger) — c'est ce que fait
  la CI ; l'application ainsi construite ne calcule aucune empreinte. Détail :
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
| `crates/editor` | module 3 — démixage (HTDemucs via `demucs-core`, BS-RoFormer ; Burn), pulsation, étirement, greffe |
| `crates/transcription` | module 3 — transcription basse et batterie, sections |
| `crates/superres` | super-résolution audio hors ligne (AERO via ONNX Runtime) |
| `crates/cli` | binaire `rusty-music` |
| `apps/desktop` | application Tauri (interface HTML/CSS/JS + WebGL) ; `ui-tests/` : tests de fumée de l'interface |
| `apps/mobile` | compagnon iOS « Rusty Valise » |

`docs/` porte les décisions et leur pourquoi ; `docs/journal.md` est le journal
de développement ; `experiments/` garde les sondages qui ont tranché les choix.

## Avant de proposer un changement

```bash
./scripts/verifier.sh     # tout ; ou une étape : fmt · deny · clippy · test · fumee
```

| Étape | Commande | Ce qu'elle vérifie |
|---|---|---|
| `fmt` | `cargo fmt --all -- --check` | la forme (rustfmt par défaut) |
| `deny` | `cargo deny check` | licences, avis de sécurité, sources — `cargo install --locked cargo-deny` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` | lint |
| `test` | `cargo test --workspace` | tests Rust |
| `fumee` | `npx playwright test` dans `apps/desktop/ui-tests/` | l'interface dans Chrome headless, Tauri simulé ; Node et Chrome requis |

La CI GitHub (`.github/workflows/ci.yml`, runner macOS) passe les mêmes
contrôles sans les modèles. Après une modification de mise en page, lancer
aussi `scripts/audit-interface.sh` (l'application réelle, sur une copie de la
base, ~3 min).

**`cargo fmt` est imposé depuis le 31 oct. 2026** (tout le workspace a été
mis en forme d'un coup, commit listé dans `.git-blame-ignore-revs` ; pour que
`git blame` l'ignore en local : `git config blame.ignoreRevsFile
.git-blame-ignore-revs`).

**Changer le schéma de la base = ajouter une migration** à la fin de
`migrations()` (`crates/core/src/db.rs`), en SQL simple ; ne jamais modifier
une migration déjà publiée, ni `crates/core/sql/schema.sql` (la migration
n°1). `PRAGMA user_version` dit combien sont jouées. Essayer sur une copie de
la base, jamais sur l'originale.

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
