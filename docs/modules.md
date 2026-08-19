# Décomposition — cœur d'ingestion + 3 modules

Le logiciel est autonome et local. Point d'entrée unique : **un répertoire de musique**, fourni une fois, puis **surveillé automatiquement** pour ingérer les nouveaux morceaux. Trois modules bâtis sur un cœur commun.

## Cœur d'ingestion (à écrire EN PREMIER — tout en dépend)
- **Surveillance du dossier** : crate `notify` (ajouts, suppressions, renommages en continu).
- **Lecture des tags** : `lofty` (titre, artiste, album, pochette embarquée), agnostique au format.
- **Base locale** : SQLite via `rusqlite` — table des morceaux + métadonnées + (plus tard) embeddings et caractéristiques.
- Sortie : une base unique que les 3 modules consomment. Aucun module ne relit le disque directement.

## Module 1 — Lecteur
Objectif : écoute agréable et moderne avec affichage riche.
- **Lecture audio** : `rodio` par-dessus `cpal`.
- **Affichage** : pochette, infos artiste, style/genre, année, crédits.
- **Enrichissement** : Cover Art Archive (pochettes), Wikidata/Wikipédia (bio, genre) — via identifiant MusicBrainz. Voir `data-sources.md`.
- **Limite connue — critiques d'albums** : pas d'API libre propre (AllMusic, Pitchfork… sont sous copyright, sans API ouverte). Prévoir un lien sortant plutôt que de concevoir l'UI autour de cette donnée.
- UI à spécifier séparément (pas encore couverte par `ui-spec.md`).

## Module 2 — Exploration
Objectif : redécouvrir la bibliothèque (équivalent AudioMuse-AI, en plus léger).
- Carte 2D interactive (nuage de points), similarité artistes/pistes, chemins entre deux morceaux, filtres par caractéristiques (tempo, rythme, style, année).
- **C'est le module déjà spécifié** : voir `ui-spec.md` (décisions thème/filtres/chemins déjà tranchées) et `architecture.md` (pipeline embeddings → réduction 2D → clustering).

## Module 3 — Éditeur / MAO
Objectif : éditer et recomposer à partir des fichiers de la bibliothèque. **Domaine à part entière (DAW + IA audio), le plus lourd et le plus risqué — phase avancée.** Peut devenir une app compagnon partageant le cœur.
- **Time-stretch / pitch** : **écrit en Rust**, pas lié. Les bibliothèques du domaine — Signalsmith Stretch (MIT), Rubber Band (GPL/commerciale), SoundTouch (LGPL) — sont toutes en C++ et servent de références d'algorithme, pas de dépendances. Voir la règle « tout en Rust » dans `CLAUDE.md`.
- **Démixage (stems)** : faisable localement mais lourd. HTDemucs (dépôt Meta non maintenu → fork `adefossez/demucs`) ; **export ONNX MIT** (`StemSplitio/htdemucs-onnx`) exécutable via `ort`, y compris en WASM côté client. Coût : ~316 Mo, GPU conseillé, découpage overlap-add à gérer. Roformer/Mel-Roformer = meilleurs pour l'isolation vocale.
- **Mixer deux pistes** : territoire DJ (beatmatching, détection de tonalité, mixage harmonique type roue de Camelot). Référence open source à étudier : **Mixxx**.
- **Génération de piste** : partie la plus fragile (qualité inégale, calcul très intensif, licences/droits flous). Référence directe : **ACE-Step** (modèle de génération open source) et son **ACE-Step-DAW** (DAW en Rust/WASM, time-stretch double moteur Signalsmith + Rubber Band). À traiter comme expérimental et tardif.

## Séquencement
Cœur d'ingestion → Module 1 (valide le cœur, donne vite un livrable utilisable) → Module 2 → Module 3 (phasé, éventuellement séparé). Ne pas bloquer tout le projet sur la brique la plus risquée (démixage/génération).
