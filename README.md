# [Nom à définir] — suite musicale locale

Logiciel de bureau autonome et local pour **écouter, explorer et éditer** une bibliothèque musicale. Point d'entrée unique : un répertoire de musique, scanné puis surveillé automatiquement.

Voir `CLAUDE.md` pour le contexte de travail et `docs/` pour les spécifications.

## État

| Brique | État |
|---|---|
| Cœur d'ingestion (dossier surveillé, tags, base) | squelette fonctionnel, **non compilé** — voir ci-dessous |
| Module 1 — Lecteur | à faire |
| Module 2 — Exploration (carte 2D) | interface arrêtée (`docs/ui-spec.md`), maquette dans `ui/prototype/` |
| Module 3 — Éditeur / MAO | phase avancée, non démarrée |

## Structure

```
CLAUDE.md              contexte permanent (lu par Claude Code à chaque session)
docs/                  spécifications détaillées
crates/core/           cœur d'ingestion : scan, tags, surveillance, base SQLite
crates/cli/            binaire `carto` — pilote le cœur sans interface
ui/prototype/          maquette HTML du modèle de navigation retenu
```

## Démarrer

```bash
cargo run -p carto-cli -- scan  ~/Musique     # ingestion initiale
cargo run -p carto-cli -- watch ~/Musique     # scan puis surveillance continue
cargo run -p carto-cli -- stats               # état de la bibliothèque
cargo test                                    # tests du cœur
```

La base est créée dans `./carto.db` (modifiable avec `--db`).

Ouvrir `ui/prototype/maquette-navigation.html` dans un navigateur pour la maquette d'interface (données fictives).

## Premier build — à lire

**Ce squelette n'a jamais été compilé** : il a été écrit sans toolchain Rust disponible. Attendez-vous à quelques erreurs au premier `cargo build`, principalement sur les API de `lofty` (les modules ont bougé entre versions) et éventuellement `notify`. Les versions sont épinglées dans le `Cargo.toml` racine — c'est là qu'il faut ajuster.

Points de friction probables :
- `lofty::probe::Probe` / `lofty::prelude::*` : chemins d'import selon la version.
- `ItemKey::AlbumArtist`, `ItemKey::MusicBrainzRecordingId` : noms de variantes à vérifier.
- `rusqlite` avec `bundled` compile SQLite depuis les sources — premier build long, nécessite un compilateur C.

## Suite

1. Faire compiler et tourner le cœur sur la vraie bibliothèque.
2. Pipeline d'analyse : décodage (`symphonia`) → embeddings ONNX → projection 2D → clustering, écrits dans la table `features`.
3. Application Tauri (`apps/desktop/`) servant l'interface HTML/WebGL et appelant le cœur.
4. Modules 1 et 3.

## Licence

**GPL-3.0-or-later.** Toute dépendance open source est acceptable, copyleft comprise.
Principe : ne pas réécrire ce qui existe — privilégier les briques éprouvées quelle que soit leur licence.
Voir `docs/rust-audio-stack.md` et `deny.toml`.

Le fichier `LICENSE` est un stub : récupérer le texte intégral avec
`curl -sSL https://www.gnu.org/licenses/gpl-3.0.txt -o LICENSE`.
