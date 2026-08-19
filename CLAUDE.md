# Rusty Music — Suite musicale locale autonome

## Objectif
Logiciel autonome et local pour écouter, explorer et éditer une bibliothèque musicale. Point d'entrée unique : un répertoire de fichiers musicaux, surveillé automatiquement pour ingérer les nouveaux morceaux. Trois modules bâtis sur un cœur d'ingestion commun. Alternative légère et personnalisable à AudioMuse-AI (jugé trop lourd / orienté serveur), étendue vers l'écoute soignée et la MAO.

## Périmètre — cœur commun + 3 modules
Détail complet dans `docs/modules.md`.
- **Cœur d'ingestion (à écrire en premier)** : dossier surveillé + base locale + métadonnées. Consommé par les trois modules.
- **Module 1 — Lecteur** : lecture agréable et moderne, pochettes, infos artiste/style, (critiques : voir limites dans les docs).
- **Module 2 — Exploration** : carte 2D interactive, similarité artistes/pistes, chemins, filtres par caractéristiques (tempo, style…). C'est le module déjà spécifié (`docs/ui-spec.md`).
- **Module 3 — Éditeur / MAO** : **le périmètre de `docs/ui-spec-editeur.md` est couvert** (`crates/editor/`) — démixage, vitesse, hauteur, réglage par stem, greffe d'un stem venu d'un autre morceau, export. Restent hors de ce périmètre : mixage de deux pistes (il manque une grille de battements) et génération de piste.

Séquencement retenu : cœur → lecteur → exploration → éditeur (phasé).

## Stack technique retenue
- **Calcul / ML** : Rust + [Burn](https://github.com/tracel-ai/burn) 0.21, backend `wgpu` (Metal/Vulkan/DX12 d'un même code ; `--features cpu` pour le repli `ndarray`). Les modèles ONNX sont traduits en Rust natif au build par `burn-onnx`.
- **Surveillance du dossier** : `notify`. **Lecture des tags** : `lofty`. **Base locale** : SQLite (`rusqlite`).
- **Décodage audio** : [symphonia](https://github.com/pdeljanov/Symphonia) (100% Rust). **Lecture/sortie audio** : `rodio` / `cpal`.
- **Embeddings (module 2)** : **CLAP** (`laion/clap-htsat-unfused`), 512 dimensions, importé par `burn-onnx` — pas de ré-entraînement. **ONNX Runtime a été retiré** : Burn rend le même vecteur (cosinus 1,0000000000) et 20 % plus vite sur GPU. Le modèle publié n'est pas importable tel quel, ses formes doivent être figées : `scripts/preparer-modele.sh`.
- **Clustering** : k-means++ **écrit à la main** (`crates/analysis/src/cluster.rs`) et non `linfa` — une quarantaine de lignes contre `ndarray` et, selon les options, une BLAS système. `linfa` reste la voie documentée le jour où DBSCAN ou un mélange gaussien deviendront nécessaires. **Réduction 2D** : t-SNE Barnes-Hut (`bhtsne`), 27 s sur 27 000 points.
- **Démixage (module 3)** : **HTDemucs via `demucs-core`** (fork Apache-2.0 porté en Burn 0.21 : `github.com/rousseau/demucs-rs`, révision épinglée). La STFT reste en Rust, Burn ne reçoit que le réseau. La voie ONNX a été écartée après mesure — `docs/module3-demixage.md`.
- **Time-stretch / pitch (module 3)** : **`wsola`** — pur Rust, sans dépendance transitive, méthode d'`atempo` (ffmpeg) et de VLC. La transposition s'obtient en étirant puis en rééchantillonnant. Aucune liaison C ou C++.
- **Visualisation de diagnostic** : [rerun](https://rerun.io) pendant le dev uniquement.
- **Interface finale** : HTML/CSS/JS empaqueté avec **Tauri** (backend Rust exposé à l'UI). La carte est rendue en **Canvas 2D**, pas en WebGL : elle tient 27 000 points sans peine, et Three.js n'aurait rien apporté à un nuage de points plats. WebGL reste la porte de sortie si la bibliothèque grandit d'un ordre de grandeur ou si le rendu se complexifie.
- **Métadonnées / connexions artistes** : API MusicBrainz + Cover Art Archive + Wikidata/Wikipédia (voir `docs/data-sources.md`).
- **Bibliothèque source (test)** : Plex local (déjà relié à AudioMuse-AI). À terme, l'ingestion se fait directement depuis le dossier surveillé, Plex n'est qu'une source de test.

## Décisions d'architecture clés
- Cœur d'ingestion partagé : une seule base alimentée par le dossier surveillé, consommée par les 3 modules.
- Découplage strict moteur (Rust) / interface (HTML+WebGL) : rerun = diagnostic, pas UI finale (chrome peu personnalisable, API d'extension instable).
- Le moteur expose les données (positions 2D/3D, clusters, métadonnées) à l'interface en JSON ou binaire.
- Module 3 traité comme un chantier séparé et tardif pour ne pas bloquer l'ensemble sur la brique la plus risquée.

## Structure du dépôt
```
crates/core/     cœur d'ingestion (scan, tags, surveillance, SQLite) + consultation
crates/player/   lecture audio (module 1) : sortie, transport, file, onde
crates/analysis/ empreintes, projection, familles, chemins (module 2)
crates/editor/   démixage en stems (module 3)
crates/cli/      binaire `rusty-music` : scan / watch / consultation / play / analyze
apps/desktop/    application Tauri : modes Écoute et Explorer
models/          modèle ONNX CLAP (112 Mo, hors dépôt)
ui/prototype/    maquette de navigation (« Atelier ») + directions visuelles
docs/            spécifications
```
```
experiments/     essais isolés, hors du workspace — un essai doit pouvoir échouer
scripts/         préparation du modèle (fige les formes avant l'import Burn)
```
Modules 1 et 2 fonctionnels, démixage du module 3 en place : **27 042 des 27 044 morceaux sur la carte**
(restent 1 mp3 et 1 m4a corrompus). Voir README pour les chiffres
mesurés et les pièges rencontrés.

## Documents détaillés
- `docs/modules.md` — décomposition en cœur + 3 modules, périmètre et références par module.
- `docs/architecture.md` — contexte de recherche, choix techniques, projets de référence, alternatives.
- `docs/ui-spec.md` — brief d'interface du module 2 (brief de départ pour Claude Design).
- `docs/ui-spec-lecteur.md` — brief d'interface du module 1 (transport, file d'attente, vues de parcours, pochettes).
- `docs/ui-spec-editeur.md` — brief d'interface du module 3 : **une piste à la fois**, ouvrir/séparer/retoucher/exporter.
- `docs/suite.md` — **ce qui reste et dans quel ordre**, avec les dettes connues.
- `docs/module3-demixage.md` — le problème du démixage et les quatre voies possibles, mesures à l'appui.
- `docs/data-sources.md` — Plex/AudioMuse-AI, MusicBrainz, enrichissement métadonnées.
- `README.md` — état d'avancement, commandes, pièges du premier build.

## Tout en Rust — et chercher avant d'écrire
**Décidé le 18 août 2026.** Le projet n'embarque ni C ni C++. Mais la règle a
**deux temps, dans cet ordre** :

1. **chercher une crate Rust qui le fait déjà** — pure, sans FFI, à l'arbre de
   dépendances léger ;
2. **seulement sinon**, lire ce que font les bibliothèques C/C++ du domaine et
   l'écrire en Rust, court.

L'ordre n'est pas cosmétique : j'ai écrit cinq cents lignes de vocodeur de phase
avant de vérifier, et `wsola` faisait déjà mieux — conçu pour le temps réel,
sans artefact de phase, 468 lignes et zéro dépendance transitive.

| besoin | ce qui existait | ce qu'on fait |
|---|---|---|
| regroupement | linfa, et sa BLAS système | **écrit** : k-means++, `cluster.rs`, 148 lignes |
| tempo, tonalité | aubio (GPL), QM-DSP (GPL), tous en C | **écrit** : `descripteurs.rs`, 416 lignes |
| décodage Opus | crate `opus` — libopus, exige `cmake` | **pris** : `opus-decoder`, pur Rust, sans FFI |
| étirement temporel | Signalsmith, Rubber Band, SoundTouch — C++ | **pris** : `wsola`, pur Rust, méthode d'`atempo` |

**Ce que ça évite**, mesuré et non supposé : `cmake` et une bibliothèque système
imposés à quiconque construit le projet (cas du crate `opus`), une compilation
native à croiser, un arbre de licences à surveiller — et, quand une crate Rust
existe, plusieurs centaines de lignes de traitement du signal à maintenir.

## Licences — aucune exclusion
**Décidé le 17 août 2026, révision d'une contrainte antérieure.** Le projet
n'écarte plus aucune licence. Un outil sous GPL ou AGPL qui rend le service
attendu se prend, et **c'est la licence du projet qu'on adapte** pour rester
compatible. Seule exigence : le résultat reste ouvert — le code sera public sur
GitHub.

Ce que cela débloque concrètement, à examiner quand ces chantiers viendront :
- **tempo, battements et tonalité** — `aubio` (GPL-3.0), `QM-DSP` (GPL-2.0,
  celui de Mixxx), `Essentia` (AGPL-3.0). C'est le prérequis manquant du
  chantier « mixage », et la mesure a montré que celui d'AudioMuse-AI ne suffit
  pas (`experiments/audiomuse-comparaison/`) ;
- **auto-étiquetage audio** — les modèles d'Essentia, qui sont exactement ce
  qu'emploie AudioMuse-AI pour ses genres et humeurs ;
- **Rubber Band** (GPL/commerciale) pour le time-stretch, jusqu'ici écarté ;
- **FFmpeg** (LGPL/GPL) pour les 10 fichiers opus que symphonia ne décode pas.

Conséquence à ne pas perdre de vue : lier du GPL-3.0 impose GPL-3.0 au tout, et
de l'AGPL-3.0 impose l'AGPL. **La licence se change au moment où on prend la
dépendance, pas avant** — le dépôt reste MIT tant qu'il n'embarque que du
permissif.

## Notes
Fichier volontairement court. Le détail vit dans `docs/` — convention Claude Code (fichiers courts, décisions actionnables).
