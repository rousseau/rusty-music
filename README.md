# Rusty Music — suite musicale locale

[![CI](https://github.com/rousseau/rusty-music/actions/workflows/ci.yml/badge.svg)](https://github.com/rousseau/rusty-music/actions/workflows/ci.yml)
[![Licence : GPL-3.0-or-later](https://img.shields.io/badge/licence-GPL--3.0--or--later-blue.svg)](LICENSE)

Logiciel de bureau autonome et local pour **écouter, explorer et éditer** une
bibliothèque musicale. Point d'entrée unique : un répertoire de musique, scanné
puis surveillé automatiquement. Tout le calcul — décodage, empreintes,
démixage, carte — se fait sur la machine ; aucun service distant, aucune clé
d'API.

Notes sur le développement de ce logiciel et les réflexions associées : [https://rousseau.github.io/rusty-music](https://rousseau.github.io/rusty-music)

En Rust de bout en bout, à une exception près : l'inférence des modèles de
démixage et de super-résolution passe par ONNX Runtime (C++) via `ort`, un
choix assumé — « ne pas réécrire ce qui existe » l'emporte ici sur « tout en
Rust » (voir `docs/rust-audio-stack.md`).

Écrit intégralement par Claude. Contexte de travail : `CLAUDE.md`.
Spécifications : `docs/`.

## Installer

**Utiliser l'application (macOS, sans rien compiler)** — télécharger le
`.dmg` de la [dernière release](https://github.com/rousseau/rusty-music/releases/latest),
l'ouvrir, glisser l'app dans Applications. Le paquet n'est pas signé : au
premier lancement, **clic droit sur l'app → Ouvrir** (un double-clic normal
est bloqué par Gatekeeper).

**Construire depuis les sources, en une commande** — [Rust](https://rustup.rs)
et un compilateur C installés (`xcode-select --install`) :

```bash
git clone https://github.com/rousseau/rusty-music && cd rusty-music && ./scripts/release.sh
```

Cette seule commande installe `tauri-cli` si besoin, télécharge les modèles et
le plan de Paris (~0,4 Go, empreintes vérifiées), puis construit `.app` et
`.dmg` dans `target/release/bundle/`. Compte 15-30 min au premier lancement.

## État

| Brique | État |
|---|---|
| Cœur d'ingestion (dossier surveillé, tags, base SQLite, décodage Opus) | livré |
| Module 1 — Lecteur | livré ; file d'attente avec aléatoire, répétition et réordonnancement par glisser-déposer |
| Module 2 — Exploration (carte 2D, chemins, familles, filtres tempo/énergie) | livré ; carte sur plan de ville réel (Paris/OpenStreetMap) en cours |
| Module 3 — Éditeur (démixage, vitesse, hauteur, greffe calée sur les temps, export) | périmètre de `docs/ui-spec-editeur.md` couvert |
| Super-résolution audio hors ligne (bouton « HD ») | livré (`crates/superres`) |
| Métadonnées enrichies | genres MusicBrainz + descripteurs audio + popularité livrés ; restent pochettes et bios |

`docs/suite.md` tient le détail de ce qui reste et dans quel ordre.

## Structure

```
CLAUDE.md              contexte permanent (lu par Claude Code à chaque session)
docs/                  spécifications détaillées
crates/core/           cœur d'ingestion : scan, tags, surveillance, base SQLite
crates/player/         module 1 — sortie audio, transport, file, amélioration
crates/analysis/       module 2 — empreintes CLAP (Burn), projection 2D, descripteurs
crates/carto/          module 2 — tuiles vectorielles de la carte (MVT / PMTiles)
crates/osm/            module 2 — import d'un plan de ville OpenStreetMap
crates/editor/         module 3 — démixage HTDemucs (demucs-core / Burn), greffe
crates/superres/       super-résolution audio hors ligne (AERO via ONNX Runtime)
crates/cli/            binaire `rusty-music` — pilote le moteur sans interface
apps/desktop/          application Tauri : interface HTML/CSS/JS + WebGL
scripts/               préparation des modèles (CLAP, HTDemucs, AERO)
ui/prototype/          maquette HTML du modèle de navigation retenu
experiments/           sondages jetables, hors du workspace
```

## Développement

Pour itérer sans repasser par `release.sh` à chaque fois : les trois modèles
sont nécessaires (`tauri-build` vérifie les ressources), la carte affiche le
plan de Paris si `ville-paris.db` est présent.

```bash
./scripts/telecharger-modeles.sh        # les trois modèles (~0,83 Go)
./scripts/telecharger-ville.sh          # le plan de Paris (~56 Mo)
cargo run --release -p rusty-music-desktop
```

L'application tient sa propre base dans le dossier de données du système
(`app_data_dir()/rusty-music.db`) et propose un sélecteur de dossier au premier
lancement.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check                        # licences, avis de sécurité
```

## Licence

**GPL-3.0-or-later** — le texte intégral est dans `LICENSE`.
Toute dépendance open source est acceptable, copyleft comprise ; le principe
directeur est de ne pas réécrire ce qui existe. Voir `docs/rust-audio-stack.md`
et `deny.toml`.

Le plan de ville affiché sur la carte (`crates/osm`, module 2) vient
d'OpenStreetMap et est sous **ODbL** : attribution obligatoire — « © les
contributeurs OpenStreetMap », affichée dans le coin de la carte
(`apps/desktop/ui/app.js`, `attributionControl`) — et toute base dérivée
partagée à l'identique. Voir `docs/carto-ville.md`.

Les poids de modèles ont leur propre licence, distincte du code : CLAP
(Apache-2.0), HTDemucs (MIT), AERO (voir le dépôt d'origine).
