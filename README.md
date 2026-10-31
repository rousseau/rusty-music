# Rusty Music — suite musicale locale

[![Licence : GPL-3.0-or-later](https://img.shields.io/badge/licence-GPL--3.0--or--later-blue.svg)](LICENSE)

Logiciel de bureau autonome et local pour **écouter, explorer et éditer** une
bibliothèque musicale. Point d'entrée unique : un répertoire de musique, scanné
puis surveillé automatiquement.

**Tout le calcul est local** — décodage, empreintes, démixage, transcription,
carte. **L'enrichissement, lui, passe par le réseau**, toujours en tâche de
fond et sans jamais bloquer : genres MusicBrainz (seulement si une adresse de
contact est renseignée), popularité ListenBrainz + Deezer, pochettes de repli
Cover Art Archive + Deezer, biographies TheAudioDB, crédits Discogs (dumps
mensuels), critiques CritiqueBrainz, tags Last.fm (clé personnelle de
l'utilisateur, décochés par défaut). Biographies, critiques, Discogs et Last.fm
se décochent dans le mode Bibliothèque ; la popularité et les pochettes de
repli n'ont pas encore de case. Les modèles se téléchargent depuis GitHub ou
Hugging Face au premier usage. Aucune clé d'API n'est requise, sauf pour
Last.fm. Le champ d'intention « Lama » (texte → playlist, mode Explorer)
demande un serveur [Ollama](https://ollama.com) lancé sur la machine
(`localhost:11434`) ; sans lui, le reste de l'application fonctionne.

Notes sur le développement de ce logiciel et les réflexions associées : [https://rousseau.github.io/rusty-music](https://rousseau.github.io/rusty-music)

En Rust de bout en bout à l'exécution. Les réseaux tournent sur
[Burn](https://github.com/tracel-ai/burn) (CLAP, démixage HTDemucs via
`demucs-core` et BS-RoFormer, all-in-one, MuScriptor), sauf trois qui passent
par ONNX Runtime (C++) via `ort` — AERO (super-résolution), Basic Pitch et
ADTOF (transcription) — et Beat This!, par `rten` (pur Rust). Voir
`docs/rust-audio-stack.md`.

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

Cette seule commande installe `tauri-cli` si besoin, télécharge les modèles
nécessaires au build et le plan de Paris (~0,9 Go, empreintes vérifiées), puis
construit `.app` et `.dmg` dans `target/release/bundle/`. Compte 15-30 min au
premier lancement.

## Modèles

Douze modèles, ≈ 3,3 Go si tout est téléchargé (≈ 2,0 Go sans MuScriptor).
Aucun n'est dans le dépôt Git. Remerciements, articles et licences :
`MODELES.md` ; empreintes et provenance : `models/README.md`.

| Modèle | Rôle | Taille | Exécuté par | Obtenu |
|---|---|---|---|---|
| CLAP, encodeur audio | empreintes, carte | 117 Mo | Burn (traduit d'ONNX au build) | `telecharger-modeles.sh` — **requis pour compiler** |
| CLAP, encodeur texte | Lama, recherche par description, noms de familles | 501 Mo | Burn (traduit d'ONNX au build) | `telecharger-modeles.sh` — **requis pour compiler** |
| HTDemucs 4 stems | démixage | 84 Mo | Burn (`demucs-core`) | `telecharger-modeles.sh` |
| HTDemucs 6 stems / affiné | démixage | 55 / 336 Mo | Burn (`demucs-core`) | au premier usage |
| BS-RoFormer « SW » | démixage de qualité | 699 Mo | Burn | au premier usage (licence non déclarée, jamais redistribué) |
| AERO | super-résolution « HD » | 156 Mo | ONNX Runtime | `telecharger-modeles.sh` |
| Beat This! | temps et mesures | 83 Mo | `rten` | au premier usage |
| all-in-one (8 plis) | sections | 10 Mo | Burn | au premier usage |
| Basic Pitch | transcription de la basse | 0,2 Mo | ONNX Runtime | au premier usage |
| MuScriptor medium | transcription de qualité | 1,2 Go | Burn | par l'utilisateur, après acceptation de ses conditions |
| ADTOF | transcription de la batterie | 1,8 Mo | ONNX Runtime | `scripts/preparer-adtof.sh` |

## État

| Brique | État |
|---|---|
| Cœur d'ingestion (dossier surveillé, tags, base SQLite, décodage Opus) | livré |
| Module 1 — Lecteur (mode Écouter) | livré (v0.1.0) ; file d'attente avec aléatoire, répétition et réordonnancement ; suite : `docs/plan-ecouter-v0.2.md` |
| Module 2 — Exploration (carte 2D, chemins, familles, filtres tempo/énergie) | livré ; carte sur plan de ville réel (Paris/OpenStreetMap) en cours |
| Module 3 — Éditeur (démixage, vitesse, hauteur, greffe calée sur les temps, export) | périmètre de `docs/ui-spec-editeur.md` couvert |
| Super-résolution audio hors ligne (bouton « HD ») | livré (`crates/superres`) |
| Compagnon iOS « Rusty Valise » (valise d'albums exportée par le Mac, écoutes rapatriées) | tourne sur iPhone, usage personnel ; `docs/plan-ios-compagnon.md` |
| Métadonnées enrichies | genres MusicBrainz + descripteurs audio + popularité + pochettes de repli (Cover Art Archive, Deezer) + bios (TheAudioDB) + crédits (Discogs) + critiques (CritiqueBrainz) livrés |

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
crates/editor/         module 3 — démixage (HTDemucs via demucs-core, BS-RoFormer ; Burn), pulsation, greffe
crates/transcription/  module 3 — transcription basse et batterie, sections
crates/superres/       super-résolution audio hors ligne (AERO via ONNX Runtime)
crates/cli/            binaire `rusty-music` — pilote le moteur sans interface
apps/desktop/          application Tauri : interface HTML/CSS/JS + WebGL
apps/mobile/           compagnon iOS « Rusty Valise » (Tauri) : lit une valise
scripts/               téléchargement et préparation des modèles, vérification, paquet
ui/prototype/          maquette HTML du modèle de navigation retenu
experiments/           sondages jetables, hors du workspace
```

## Développement

Pour itérer sans repasser par `release.sh` à chaque fois : les deux tours
de CLAP sont nécessaires pour compiler `crates/analysis`, la carte affiche le
plan de Paris si `ville-paris.db` est présent.

```bash
./scripts/telecharger-modeles.sh        # CLAP audio + texte, HTDemucs, AERO (~0,86 Go)
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

Les poids de modèles ont leur propre licence, distincte du code — certaines
non commerciales (AERO, ADTOF, MuScriptor), une non déclarée (BS-RoFormer
« SW »). Le détail, modèle par modèle : `MODELES.md`.
