# Rusty Music — suite musicale locale autonome

Logiciel de bureau local pour écouter, explorer et éditer une bibliothèque
musicale. Point d'entrée : un dossier de musique, surveillé. Un cœur
d'ingestion commun, cinq modes dans le rail (Écouter, Explorer, Éditer,
Découvrir, Valise) plus Bibliothèque au-dessus. Alternative légère à
AudioMuse-AI. **État et restes : `docs/suite.md`.**

## Décisions en vigueur

**Périmètre.** Éditer travaille **une piste à la fois** : ni session
multipiste, ni projet sauvegardé, ni mixage DJ. Deux usages : *pratiquer*
(transcription, partition, boucles) et *créer* (greffe, génération).

**Exécution : Rust uniquement**, pas de Python ni de processus externe.
Python sert seulement à préparer (convertir des poids) et à vérifier (parité,
bancs). Exception à résorber : Lama appelle Ollama (`crates/core/src/ollama.rs`).

**Stack.**
- Calcul : [Burn](https://github.com/tracel-ai/burn), backend `wgpu` ou
  `ndarray` choisi à la compilation (bascule vers `burn/metal` étudiée :
  `experiments/burn-metal/README.md`).
- Ingestion : `notify`, `lofty`, SQLite (`rusqlite`). Décodage : symphonia +
  `opus-decoder`. Sortie : `rodio`/`cpal`. Étirement : `wsola` + `rubato`.
- Empreintes : CLAP (`laion/clap-htsat-unfused`) traduit d'ONNX par
  `burn-onnx` au build. Réduction 2D : `bhtsne`.
- Démixage : HTDemucs via `demucs-core` (Burn, pas d'export ONNX :
  `docs/module3-demixage.md`) ; BS-RoFormer « SW » porté en Burn, `.ckpt`
  d'origine téléchargé à la source, jamais redistribué (`docs/module3-roformer.md`).
- Par `ort` (ONNX Runtime) : AERO (« HD », `crates/superres`), Basic Pitch et
  ADTOF (`crates/transcription`). Beat This! par `rten`. MuScriptor et
  all-in-one portés en Burn.
- Interface : HTML/CSS/JS + WebGL dans **Tauri**. `rerun` = diagnostic de dev
  seulement. Le moteur expose ses données en JSON ou binaire.
- Enrichissement : MusicBrainz, Cover Art Archive, Deezer, ListenBrainz,
  TheAudioDB, Discogs (dumps), CritiqueBrainz, Last.fm (`docs/data-sources.md`,
  `docs/enrichissement-lecteur.md`, `docs/popularite.md`).

**Licence : GPL-3.0-or-later** (`LICENSE`). Toute dépendance open source est
acceptable, copyleft comprise ; **ne jamais réécrire ce qui existe**.
- Code lié au binaire : compatible GPL-3 (pas de CC BY-NC-*, pas de
  GPL-2.0-only lié) ; sinon réécrit en Rust ou hors processus. Contrôlé par
  `cargo deny check` (`deny.toml`).
- Poids : toute licence, NC comprise ; on adopte la licence, on s'y conforme,
  on cite (`MODELES.md`, écran « À propos »). Poids convertis republiés sous
  leur licence d'origine (Hugging Face, compte `rousseau`) si elle le permet ;
  licence non déclarée = téléchargés à la source, jamais redistribués.
  Détail : `docs/rust-audio-stack.md` — **à lire avant d'ajouter une dépendance.**

**Carte (Explorer).**
- Support : **le plan de Paris** (OpenStreetMap, ODbL : « © les contributeurs
  OpenStreetMap » affiché), découpé sur la commune, avec un halo de petite
  couronne dessiné mais jamais habité.
- Placement : **croissance chronologique** (`crates/carto/src/croissance.rs`) —
  les morceaux arrivent par date, l'île de la Cité d'abord ; le temps est le
  coût de voirie ; artères = morceaux populaires, petits quartiers = morceaux
  confidentiels, regroupés par style. Artistes les plus populaires ancrés aux
  monuments (`ancrage.rs`).
- Familles : vocabulaire de genres (MusicBrainz + tags), repli k-means++ sur
  l'empreinte pour ce qu'il ne nomme pas ; noms votés (`docs/nommage-familles.md`).
- Rendu : **MapLibre GL JS** dans la webview ; Rust génère les tuiles
  (`.pmtiles`). `maplibre-rs` est archivé — ne pas l'utiliser.
- Fond : palettes au choix (`crates/carto/src/palette.rs`, d'après
  *maptoposter*), `osm-clair` par défaut ; un `style-<id>.json` par palette,
  bascule par `setStyle` sans régénérer les tuiles.
- Repli sans ville importée : monde fictif engendré (`docs/carto-direction.md`,
  `docs/carto-peuplement-architecture.md`). Piège : y traiter le carré
  `[-1.08, 1.08]²` comme un planisphère.
- Détail : `docs/carto-ville.md`.

## Structure du dépôt

```
crates/core/          cœur : scan, tags, surveillance, SQLite, enrichissement
crates/player/        Écouter : sortie audio, transport, file, amélioration « E »
crates/analysis/      Explorer : empreintes CLAP, projection 2D, descripteurs
crates/carto/         Explorer : tuiles vectorielles (MVT / PMTiles)
crates/osm/           import d'un plan OpenStreetMap (jamais lié à l'app)
crates/editor/        Éditer : démixage, pulsation, étirement, greffe
crates/transcription/ Éditer : basse, batterie, sections
crates/superres/      super-résolution AERO (bouton « HD »)
crates/cli/           binaire `rusty-music`
apps/desktop/         application Tauri (ui/ : HTML/CSS/JS)
apps/mobile/          compagnon iOS « Rusty Valise »
scripts/              modèles, vérification (`verifier.sh`), paquet
experiments/          sondages et bancs, hors workspace
docs/                 spécifications ; docs/archives/ : historique
livre/                notes de l'auteur — ne jamais modifier sans demande
```

Build : Rust 1.82+, un compilateur C, les deux tours de CLAP dans `models/`
(`./scripts/telecharger-modeles.sh clap`) — sans elles `crates/analysis` ne
compile pas. Vérification avant push : `./scripts/verifier.sh`. Pièges : README.

## Index des documents

À lire avant de toucher à…
- **toute interface** : `docs/interface-guidelines.md` (contrat de cohérence
  des écrans) ; après une modification de mise en page :
  `scripts/audit-interface.sh`.
- **Écouter** : `docs/ui-spec-lecteur.md`, `docs/plan-ecouter-v0.2.md`,
  `docs/amelioration-audio.md` (« E »), `docs/module3-superresolution.md` (« HD »).
- **Explorer** : `docs/ui-spec.md`, `docs/carto-ville.md`,
  `docs/carto-etat-de-lart.md` (aspect, palettes), `docs/carto-direction.md`,
  `docs/carto-peuplement-architecture.md`, `docs/carto-google-maps.md`
  (réseau, routage), `docs/carto-anneau.md`, `docs/frise-filiations.md`,
  `docs/nommage-familles.md`, `docs/recherche-llm-playlist.md` (Lama).
- **Éditer** : `docs/ui-spec-editeur.md` (l'établi), `docs/plan-editer-pratique-creation.md`
  et `docs/recherche-editer-pratique-creation.md`, `docs/module3-demixage.md`,
  `docs/module3-roformer.md`.
- **enrichissement** : `docs/data-sources.md`, `docs/enrichissement-lecteur.md`,
  `docs/popularite.md`.
- **compagnon iOS, Valise** : `docs/plan-ios-compagnon.md`.
- **dépendances, licences** : `docs/rust-audio-stack.md`, `MODELES.md`.
- **contexte** : `docs/architecture.md` et `docs/modules.md` (registres
  historiques, plusieurs points depuis tranchés) ; `docs/journal.md` (ce qui a
  été fait et mesuré) ; `docs/archives/` (sections abandonnées).
