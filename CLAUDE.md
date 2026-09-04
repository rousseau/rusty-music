# Rusty Music — Suite musicale locale autonome

## Objectif
Logiciel autonome et local pour écouter, explorer et éditer une bibliothèque musicale. Point d'entrée unique : un répertoire de fichiers musicaux, surveillé automatiquement pour ingérer les nouveaux morceaux. Trois modules bâtis sur un cœur d'ingestion commun. Alternative légère et personnalisable à AudioMuse-AI (jugé trop lourd / orienté serveur), étendue vers l'écoute soignée et la MAO.

## Périmètre — cœur commun + 3 modules
Détail complet dans `docs/modules.md`.
- **Cœur d'ingestion (à écrire en premier)** : dossier surveillé + base locale + métadonnées. Consommé par les trois modules.
- **Module 1 — Lecteur** : lecture agréable et moderne, pochettes, infos artiste/style, (critiques : voir limites dans les docs). Spéc. : `docs/ui-spec-lecteur.md`.
- **Module 2 — Exploration** : carte 2D interactive, similarité artistes/pistes, chemins, filtres par caractéristiques (tempo, style…). Spéc. : `docs/ui-spec.md`.
- **Module 3 — Éditeur / MAO** : démixage (stems), time-stretch/pitch, greffe de stem. **Une piste à la fois** — ni session multipiste, ni mixage DJ (sorti du périmètre). Spéc. : `docs/ui-spec-editeur.md`.

Séquencement (fait) : cœur → lecteur → exploration → éditeur. État courant : `docs/suite.md`.

## Stack technique retenue
- **Calcul / ML** : Rust + [Burn](https://github.com/tracel-ai/burn) — backend `wgpu` (Metal/Vulkan/DX12) ou `ndarray` (CPU), choisi à la compilation.
- **Surveillance du dossier** : `notify`. **Lecture des tags** : `lofty`. **Base locale** : SQLite (`rusqlite`).
- **Décodage audio** : [symphonia](https://github.com/pdeljanov/Symphonia), plus `opus-decoder` (pur Rust) pour l'Opus que symphonia ne fait pas. **Sortie audio** : `rodio` / `cpal`.
- **Empreintes (module 2)** : encodeur audio de **CLAP** (`laion/clap-htsat-unfused`), traduit d'ONNX en Rust natif par `burn-onnx` au build, exécuté sur Burn. Pas de ré-entraînement.
- **Familles de la carte** : vocabulaire de genres (MusicBrainz + tags), avec repli k-means maison (k-means++) sur l'empreinte pour ce que le vocabulaire ne nomme pas. `linfa` reste la voie documentée pour DBSCAN/GMM. **Réduction 2D** : `bhtsne` (t-SNE Barnes-Hut, pur Rust).
- **Démixage (module 3)** : HTDemucs via **`demucs-core`** (STFT en Rust, réseau sur Burn) — l'export ONNX a été écarté, voir `docs/module3-demixage.md`.
- **Super-résolution (bouton « HD »)** : AERO via ONNX Runtime (`ort`) — `crates/superres`.
- **Time-stretch / pitch (modules 1 et 3)** : crate **`wsola`** (recouvrement-addition, pur Rust). La transposition ajoute un rééchantillonnage (`rubato`). Vérifié avant d'écrire le nôtre — un vocodeur maison a été retiré.
- **Interface finale** : HTML/CSS/JS + WebGL, empaquetée avec **Tauri** (backend Rust exposé à l'UI). `rerun` : diagnostic pendant le dev seulement.
- **Métadonnées / connexions artistes** : API MusicBrainz + Cover Art Archive + Wikidata/Wikipédia (voir `docs/data-sources.md`).
- **Bibliothèque source (test)** : Plex local (déjà relié à AudioMuse-AI). À terme, l'ingestion se fait directement depuis le dossier surveillé, Plex n'est qu'une source de test.

## Licence — DÉCIDÉE
Projet publié sous **GPL-3.0-or-later** (texte intégral dans `LICENSE`).
**Toute dépendance open source est acceptable**, copyleft comprise.
Principe directeur : **ne jamais réécrire ce qui existe déjà.** Privilégier les briques éprouvées quelle que soit leur licence.
Seules exclusions : licences **non libres** (CC BY-NC-*, « research only ») et **GPL-2.0-only**. Vérifier aussi la licence des **poids de modèles**, distincte du code.
Contrôlé par `cargo deny check` (`deny.toml`) en CI. Détail : `docs/rust-audio-stack.md`.

## Support de la carte — DÉCIDÉ
**La carte est le plan de Paris**, importé d'OpenStreetMap et découpé sur la
limite communale — avec un **halo de petite couronne** (~2 km) de voirie/eau/
verts dessiné mais jamais habité, pour que le fondu de bordure du rendu ait de
la matière (`docs/carto-ville.md`, révisé). On n'engendre plus le terrain : une
ville réelle a déjà des rues d'épaisseurs variées, un fleuve, des parcs et une
silhouette reconnaissable. Le travail se concentre sur l'**affectation** —
répartir 27 000 morceaux sur une voirie donnée en préservant le voisinage
musical.
Données sous **ODbL** : attribution « © les contributeurs OpenStreetMap »
obligatoire à l'affichage. Détail : `docs/carto-ville.md`.

## Rendu de la carte — DÉCIDÉ
**MapLibre GL JS dans la webview Tauri.** Rust génère les tuiles vectorielles (fichier `.pmtiles` lu en local, ou serveur **Martin**) ; MapLibre fait le rendu, les étiquettes dépendantes du zoom et l'évitement de collisions.
`maplibre-rs` (portage Rust pur) est **archivé** — ne pas l'utiliser.
Piège : projeter l'espace d'embedding dans un « monde » de coordonnées géographiques fictives — le carré `[-1.08, 1.08]²` traité comme un planisphère entier, encore actif sur le chemin de repli sans ville importée.
**Fond de plan : plusieurs palettes au choix** (`crates/carto/src/palette.rs`, portées de *maptoposter*, MIT) — `osm-clair` par défaut plus `sepia`/`encre`/`nuit`/`bleu-plan`. Chaque `Palette` porte fond **et** ses 12 teintes de familles *sur la carte* (calées sur le fond) ; le nuage et la légende gardent `--familles`. `engendrer_tuiles` écrit un `style-<id>.json` par palette ; l'interface bascule sans régénérer les tuiles (`gl.setStyle`).
Détail : `docs/carto-ville.md` (le plan réel, chemin par défaut) ; `docs/carto-direction.md` (direction du monde fictif, chemin de repli) ; `docs/carto-etat-de-lart.md` (thèmes de fond de plan).

## Décisions d'architecture clés
- Cœur d'ingestion partagé : une seule base alimentée par le dossier surveillé, consommée par les 3 modules.
- Découplage strict moteur (Rust) / interface (HTML+WebGL) : rerun = diagnostic, pas UI finale (chrome peu personnalisable, API d'extension instable).
- Le moteur expose les données (positions 2D/3D, clusters, métadonnées) à l'interface en JSON ou binaire.
- Module 3 traité comme un chantier séparé et tardif pour ne pas bloquer l'ensemble sur la brique la plus risquée.

## Structure du dépôt
```
crates/core/     cœur d'ingestion (scan, tags, surveillance, SQLite, enrichissement)
crates/player/   module 1 — sortie audio, transport, file, amélioration « E »
crates/analysis/ module 2 — empreintes CLAP (Burn), projection 2D, descripteurs
crates/carto/    module 2 — tuiles vectorielles de la carte (MVT / PMTiles)
crates/osm/      import d'un plan de ville OpenStreetMap (jamais lié à l'app)
crates/editor/   module 3 — démixage (demucs-core / Burn), étirement, greffe
crates/superres/ super-résolution audio hors ligne (AERO via ONNX Runtime) — bouton « HD »
crates/cli/      binaire `rusty-music` : scan / watch / analyse / carte / démixage…
apps/desktop/    application Tauri (interface HTML/CSS/JS + WebGL)
scripts/         préparation des modèles (CLAP, HTDemucs, AERO)
ui/prototype/    maquette du modèle de navigation retenu (variante « Atelier »)
docs/            spécifications
```
Build : Rust 1.82+, un compilateur C, et les modèles préparés (`scripts/`,
`.gitignore`) — sans le modèle CLAP, `crates/analysis` ne compile pas. Détail
et pièges : README, section « Démarrer ».

## Documents détaillés
- `docs/suite.md` — **état d'avancement par brique et ordre de ce qui reste. À lire pour savoir où en est le projet.**
- `docs/modules.md` — décomposition en cœur + 3 modules, périmètre et références par module. Écrit avant la décision de licence : la règle « time-stretch/démixage en Rust pur, sans dépendance » y est **caduque** (voir stack ci-dessus).
- `docs/architecture.md` — contexte de recherche (Islands of Music, Audio Atlas, AudioMuse-AI), pipeline, alternatives. Registre historique : plusieurs « points à trancher » y sont depuis tranchés (CLAP, t-SNE).
- `docs/ui-spec.md` — brief d'interface du module 2 (Exploration).
- `docs/ui-spec-lecteur.md` — brief d'interface du module 1 (Lecteur).
- `docs/ui-spec-editeur.md` — brief d'interface du module 3 (Éditeur). Périmètre tranché : une piste, pas de projet sauvegardé.
- `docs/module3-demixage.md` — pourquoi le démixage passe par `demucs-core` (Burn) et non l'export ONNX + `ort`.
- `docs/data-sources.md` — Plex/AudioMuse-AI, MusicBrainz, enrichissement métadonnées.
- `docs/popularite.md` — **popularité générale (ListenBrainz + Deezer, sans clé API) : passe d'analyse (étape 5/5, rafraîchissement 90 j) + jauge à 5 segments dans la file et les listes de pistes. Livré. Reste hors chantier : popularité d'artiste pour la carte.**
- `docs/carto-ville.md` — **modèle retenu : la carte est le plan d'une vraie ville (Paris). Familles → quartiers, artistes → rues, morceaux → adresses. À lire en premier pour la carte.**
- `docs/carto-peuplement.md` — modèle du peuplement (morceaux = habitants, placement chronologique). Son support généré est remplacé par le plan de ville ; son intention est reprise par `carto-ville.md`.
- `docs/carto-peuplement-architecture.md` — **mécanique du peuplement : traits du générateur de monde, placement incrémental, typologie, schéma SQL, réglages et objections.**
- `docs/carto-etat-de-lart.md` — **génération de cartes : références du domaine, code disponible, et ce qui s'applique à nous. À lire avant d'améliorer l'aspect de la carte.**
- `docs/carto-etapes.md` — **ce qu'il reste pour que la carte soit une carte : blocage webview, intégration, lisibilité, peuplement. À lire avant de reprendre le chantier cartographique.**
- `docs/carto-google-maps.md` — réseau routier et profils de routage. Sa section 1 (placement hiérarchique en trois étages) est **abandonnée**, remplacée par le peuplement.
- `docs/carto-direction.md` — direction cartographique : mer et littoral, relief, toponymes, révélation par échelle, réseau, balade.
- `docs/rust-audio-stack.md` — **inventaire des crates audio et contraintes de licence. À lire avant d'ajouter toute dépendance.**
- `docs/amelioration-audio.md` — bouton « E » du mode Écouter : ligne de qualité, excitation psychoacoustique, rééchantillonnage `rubato`, et le sondage super-résolution neuronale.
- `docs/module3-superresolution.md` — rendu hors-ligne « régénérer en HD » (AERO via `ort`) : **livré** dans `crates/superres` + bouton « HD » du transport. Sondage : `experiments/burn-aero/`. Modèle : `scripts/preparer-aero.sh` → `models/aero-11025-44100.onnx`.
- `README.md` — état d'avancement, commandes, pièges du premier build.

## Notes
Fichier volontairement court. Le détail vit dans `docs/` — convention Claude Code (fichiers courts, décisions actionnables).
