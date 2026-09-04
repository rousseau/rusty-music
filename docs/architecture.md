# Architecture — contexte et choix techniques

> **Registre historique.** Ce fichier garde le contexte de recherche et les
> raisons des choix. Plusieurs « points à trancher » ci-dessous **le sont
> depuis** : modèle d'embedding → **CLAP** (via `burn-onnx`), réduction 2D →
> **t-SNE Barnes-Hut** (`bhtsne`), time-stretch → crate **`wsola`**, démixage →
> **`demucs-core`**. La source n'est plus Plex mais le dossier surveillé.
> L'objectif « 100 % Rust » cède devant « ne pas réécrire » : `ort` (ONNX
> Runtime, C++) sert la super-résolution. État courant : `CLAUDE.md` et
> `suite.md`.

## Contexte de recherche
Ce projet s'inscrit dans une lignée de recherche démarrée avec *Islands of Music* (Pampalk, 2001) : cartes 2D de bibliothèques musicales basées sur la similarité audio, avec la métaphore d'une carte géographique. Suites notables : nepTune (paysage 3D), Globe of Music (GeoSOM), MusicRainbow, Last.fm Artist Map. Conférence de référence du domaine : ISMIR (pas SIGGRAPH — plutôt Eurographics/IEEE VIS pour le pan visualisation). Survey de référence : Khulusi et al., *A Survey on Visualizations for Musical Data*, Computer Graphics Forum, 2020.

Version moderne à embeddings deep learning : **Audio Atlas** (ETH Zurich, ISMIR 2024, open source, `ETH-DISCO/audio-atlas`) — CLAP + Milvus + t-SNE + Deepscatter/WebGL. Outil générique réutilisable : **Embedding Atlas** (Apple, open source, `apple/embedding-atlas`).

Projet le plus proche fonctionnellement de l'objectif final : **AudioMuse-AI** (self-hosted, clustering + music map + « song paths » entre deux morceaux). Sert de référence/benchmark, mais interface jugée trop lourde — d'où ce projet.

## Pipeline retenu
1. **Décodage** : symphonia lit les fichiers locaux (FLAC/MP3/etc.) depuis la bibliothèque Plex.
2. **Embeddings** : passage dans un modèle pré-entraîné (musicnn ou CLAP) via ONNX — pas d'entraînement from scratch.
3. **Réduction de dimension** : projection en 2D/3D (t-SNE ou UMAP) pour la carte.
4. **Clustering** : linfa (K-Means ou DBSCAN) pour les regroupements thématiques.
5. **Recherche de chemin** : plus proches voisins successifs entre morceau A et morceau B dans l'espace d'embedding (inspiré de Deej-AI / AudioMuse-AI « Song Paths »).
6. **Service des données** : le moteur Rust expose positions, clusters, métadonnées à l'interface (JSON/binaire).

## Points ouverts / à trancher
- Réduction de dimension : écosystème Rust pur (bhtsne, etc.) moins mature que scikit-learn/umap-learn — valider en Python d'abord si besoin de fiabilité, ou accepter une implémentation Rust plus rustique au démarrage.
- Choix définitif du modèle d'embedding (musicnn vs CLAP) — CLAP permet en plus la recherche texte→audio (« morceaux qui sonnent comme... »), musicnn est plus classique/musique pure.
- Taille de la bibliothèque à supporter (affecte la stratégie de rendu : LOD, densité, etc.) — non communiquée pour l'instant.

## Alternatives écartées ou de secours
- rerun comme UI finale : écarté (chrome peu personnalisable, API d'extension instable) — gardé uniquement comme outil de diagnostic pendant le développement.
- Prototype Python (librosa/essentia + scikit-learn) : option de repli pour valider la chaîne avant le portage Rust complet, si besoin.

## Extension du périmètre (écoute + édition) — voir modules.md
Le projet couvre désormais un cœur d'ingestion partagé et trois modules (lecteur, exploration, éditeur/MAO). Le présent fichier reste le registre technique transverse ; la décomposition produit vit dans `modules.md`.

### Briques ajoutées
- **Ingestion** : `notify` (surveillance dossier), `lofty` (tags), `rusqlite`/SQLite (base locale partagée).
- **Lecture audio** (module 1) : `rodio` / `cpal`.
- **Time-stretch / pitch** (module 3) : **écrit en Rust**. Signalsmith Stretch, Rubber Band et SoundTouch sont des références d'algorithme ; aucune n'entre dans l'arbre de dépendances.
- **Démixage** (module 3) : HTDemucs export ONNX MIT (`StemSplitio/htdemucs-onnx`) via `ort`, exécutable jusqu'en WASM. Lourd : ~316 Mo, GPU conseillé, overlap-add. Dépôt Demucs original non maintenu → fork `adefossez/demucs`.

## Projets de référence
- **AudioMuse-AI** — référence fonctionnelle du module 2 (clustering, music map, song paths). Interface trop lourde → motivation du projet.
- **Audio Atlas** (ETH Zurich, ISMIR 2024, `ETH-DISCO/audio-atlas`) — CLAP + Milvus + t-SNE + Deepscatter/WebGL. Le plus proche techniquement du module 2.
- **Embedding Atlas** (Apple, `apple/embedding-atlas`) — visualisation générique d'embeddings, réutilisable pour la carte.
- **Mixxx** — référence open source pour le mixage DJ (module 3 : beatmatching, tonalité, mixage harmonique).
- **ACE-Step + ACE-Step-DAW** — modèle de génération open source et DAW associé en Rust/WASM (time-stretch double moteur). Référence directe et à surveiller pour le module 3.
- **Islands of Music** (Pampalk, 2001) et suites (nepTune, Globe of Music, MusicRainbow) — lignée historique de la carte 2D.
