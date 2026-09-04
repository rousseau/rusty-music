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
Objectif : éditer et recomposer à partir des fichiers de la bibliothèque. **Une piste à la fois** — ouvrir, séparer, retoucher, exporter ; ni session multipiste, ni projet sauvegardé (les stems sur le disque suffisent), ni mixage DJ (sorti du périmètre). Spéc. d'interface : `ui-spec-editeur.md`.

> **Mise à jour post-décision de licence.** Ce document précédait le passage à
> GPL-3.0 et à la règle « ne pas réécrire ». Ce qui suit décrit l'intention ;
> l'implémentation retenue est en italique.

- **Time-stretch / pitch** : ~~écrit en Rust, sans dépendance~~. *Retenu : le crate `wsola` (recouvrement-addition par similarité de forme d'onde, la méthode d'`atempo`/VLC, pur Rust, MIT). Un vocodeur de phase maison a été écrit puis retiré au profit de `wsola`. La transposition ajoute un rééchantillonnage (`rubato`).*
- **Démixage (stems)** : HTDemucs, poids Meta (MIT). ~~Export ONNX exécuté via `ort`~~. *Retenu : `demucs-core` (fork de `demucs-rs` épinglé, URL dans `crates/editor/Cargo.toml`), où la STFT reste en Rust et où Burn ne reçoit que le réseau — l'export ONNX déroulait la transformée de Fourier en milliers de nœuds et le backend GPU s'y trompait. Voir `module3-demixage.md`.* Coût : GPU conseillé, découpage overlap-add.
- **Greffe de stem** *(livré)* : mettre à la place d'un stem celui d'un autre morceau, calé sur le tempo et sur les temps forts (grille de battements, `analysis/src/battements.rs`).
- **Mixer deux pistes** : **hors du module 3** depuis la spec d'interface — redevient un chantier à part s'il se fait. Territoire DJ (beatmatching, tonalité, roue de Camelot). Référence : **Mixxx**.
- **Génération de piste** : non planifié. Partie la plus fragile (qualité inégale, calcul intensif, droits flous). Référence : **ACE-Step**.

## Séquencement
Cœur d'ingestion → Module 1 → Module 2 → Module 3. **Fait** (voir `suite.md`) : les quatre briques sont livrées à leur périmètre spécifié ; restent des finitions (aléatoire/répétition du lecteur, carte sur plan de ville réel, pochettes/bios).
