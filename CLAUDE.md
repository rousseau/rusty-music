# [Nom du projet à définir] — Suite musicale locale autonome

## Objectif
Logiciel autonome et local pour écouter, explorer et éditer une bibliothèque musicale. Point d'entrée unique : un répertoire de fichiers musicaux, surveillé automatiquement pour ingérer les nouveaux morceaux. Trois modules bâtis sur un cœur d'ingestion commun. Alternative légère et personnalisable à AudioMuse-AI (jugé trop lourd / orienté serveur), étendue vers l'écoute soignée et la MAO.

## Périmètre — cœur commun + 3 modules
Détail complet dans `docs/modules.md`.
- **Cœur d'ingestion (à écrire en premier)** : dossier surveillé + base locale + métadonnées. Consommé par les trois modules.
- **Module 1 — Lecteur** : lecture agréable et moderne, pochettes, infos artiste/style, (critiques : voir limites dans les docs).
- **Module 2 — Exploration** : carte 2D interactive, similarité artistes/pistes, chemins, filtres par caractéristiques (tempo, style…). C'est le module déjà spécifié (`docs/ui-spec.md`).
- **Module 3 — Éditeur / MAO** : démixage (stems), time-stretch/pitch, mixage de deux pistes, remplacement/génération de piste. **Domaine à part, le plus lourd et le plus risqué — phase avancée**, potentiellement une app compagnon partageant le cœur.

Séquencement retenu : cœur → lecteur → exploration → éditeur (phasé).

## Stack technique retenue
- **Calcul / ML** : Rust + [Burn](https://github.com/tracel-ai/burn) — backends CUDA/ROCm/Metal/Vulkan/WebGPU/CPU (agnostique au matériel).
- **Surveillance du dossier** : `notify`. **Lecture des tags** : `lofty`. **Base locale** : SQLite (`rusqlite`).
- **Décodage audio** : [symphonia](https://github.com/pdeljanov/Symphonia) (100% Rust). **Lecture/sortie audio** : `rodio` / `cpal`.
- **Embeddings (module 2)** : modèle pré-entraîné (musicnn / CLAP) via ONNX Runtime (`ort`) ou import ONNX de Burn — pas de ré-entraînement au départ.
- **Clustering** : [linfa](https://github.com/rust-ml/linfa). **Réduction 2D** : à valider (écosystème Rust moins mature — voir `docs/architecture.md`).
- **Démixage (module 3)** : HTDemucs export ONNX (MIT) via `ort` — lourd (~316 Mo, GPU conseillé, découpage overlap-add).
- **Time-stretch / pitch (module 3)** : Signalsmith Stretch (MIT) via `ssstretch` / `signalsmith-stretch`. Éviter Rubber Band (GPL/commerciale) si on veut rester permissif.
- **Visualisation de diagnostic** : [rerun](https://rerun.io) pendant le dev uniquement.
- **Interface finale** : HTML/CSS/JS + WebGL (Three.js ou shader sur mesure), empaquetée avec **Tauri** (backend Rust exposé à l'UI).
- **Métadonnées / connexions artistes** : API MusicBrainz + Cover Art Archive + Wikidata/Wikipédia (voir `docs/data-sources.md`).
- **Bibliothèque source (test)** : Plex local (déjà relié à AudioMuse-AI). À terme, l'ingestion se fait directement depuis le dossier surveillé, Plex n'est qu'une source de test.

## Licence — DÉCIDÉE
Projet publié sous **GPL-3.0-or-later**. **Toute dépendance open source est acceptable**, copyleft comprise.
Principe directeur : **ne jamais réécrire ce qui existe déjà.** Privilégier les briques éprouvées quelle que soit leur licence.
Débloqué par cette décision : **bliss-rs** (analyse + playlists par similarité, en Rust — à évaluer en priorité, il couvre peut-être une large part du module 2), aubio, Essentia, Rubber Band, contour-isobands.
Seules exclusions restantes : licences **non libres** (CC BY-NC-*, « research only ») et **GPL-2.0-only**. Vérifier aussi la licence des **poids de modèles**, distincte du code.
Détail : `docs/rust-audio-stack.md` · Application : `deny.toml`.

## Rendu de la carte — DÉCIDÉ
**MapLibre GL JS dans la webview Tauri.** Rust génère les tuiles vectorielles (fichier `.pmtiles` lu en local, ou serveur **Martin**) ; MapLibre fait le rendu, les étiquettes dépendantes du zoom et l'évitement de collisions.
`maplibre-rs` (portage Rust pur) est **archivé** — ne pas l'utiliser.
Piège : projeter l'espace d'embedding dans un « monde » de coordonnées géographiques fictives.
Détail : `docs/carto-direction.md`.

## Décisions d'architecture clés
- Cœur d'ingestion partagé : une seule base alimentée par le dossier surveillé, consommée par les 3 modules.
- Découplage strict moteur (Rust) / interface (HTML+WebGL) : rerun = diagnostic, pas UI finale (chrome peu personnalisable, API d'extension instable).
- Le moteur expose les données (positions 2D/3D, clusters, métadonnées) à l'interface en JSON ou binaire.
- Module 3 traité comme un chantier séparé et tardif pour ne pas bloquer l'ensemble sur la brique la plus risquée.

## Structure du dépôt
```
crates/core/   cœur d'ingestion (scan, tags, surveillance, SQLite)
crates/cli/    binaire `carto` : scan / watch / stats
ui/prototype/  maquette du modèle de navigation retenu (variante « Atelier »)
docs/          spécifications
```
Le cœur est un squelette **non compilé** (écrit sans toolchain disponible) — voir README, section « Premier build ».

## Documents détaillés
- `docs/modules.md` — décomposition en cœur + 3 modules, périmètre et références par module.
- `docs/architecture.md` — contexte de recherche, choix techniques, projets de référence, alternatives.
- `docs/ui-spec.md` — brief d'interface (module 2 ; brief de départ pour Claude Design). Modules 1 et 3 à spécifier séparément.
- `docs/data-sources.md` — Plex/AudioMuse-AI, MusicBrainz, enrichissement métadonnées.
- `docs/carto-direction.md` — direction cartographique de la carte (relief, toponymes, navigation « balade », références).
- `docs/ui-workflow.md` — procédure de travail avec Claude Design (brief permanent, inventaire des composants, remontée des décisions).
- `docs/rust-audio-stack.md` — **inventaire des crates audio et contraintes de licence. À lire avant d'ajouter toute dépendance.**
- `README.md` — état d'avancement, commandes, pièges du premier build.

## Notes
Fichier volontairement court. Le détail vit dans `docs/` — convention Claude Code (fichiers courts, décisions actionnables).
