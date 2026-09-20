# Sources de données

## Bibliothèque locale — Plex (source de test possible)
- Un serveur Plex local peut servir de source de test au démarrage : API HTTP
  documentée, catalogue déjà étiqueté. Le token d'API s'obtient via
  « Sign in with Plex » ou dans le XML d'un morceau (`X-Plex-Token`).
- Ce n'est qu'une commodité de test — la source réelle est le dossier surveillé
  (voir plus bas).

## AudioMuse-AI (référence fonctionnelle)
- Fonctionnalités utilisées comme référence : clustering audio, « Music Map »
  (carte 2D), « Song Paths » (chemin entre deux morceaux), empreinte sonique.
- Sert de benchmark fonctionnel pendant que ce projet développe sa propre
  interface, plus légère.

## Connexions et collaborations entre artistes — API MusicBrainz
- Utiliser l'API MusicBrainz plutôt que du scraping web généraliste : données structurées, gratuites, bien documentées.
- Types de relations artiste-artiste pertinents : `collaborator on` (collaboration ponctuelle), `member of` (membre d'un groupe), `founder`, etc.
- Approche envisagée : pour chaque artiste présent dans la bibliothèque locale, interroger l'API MusicBrainz (`artist-rels`) et construire un graphe d'arêtes (artiste A — type de relation — artiste B).
- Projet de référence pour l'architecture de graphe : *discogsography* (open source, Neo4j) — fonctionnalités « Path Finder » (plus court chemin entre deux entités) et « Collaboration Network » (graphe + centralité), à partir de données Discogs + MusicBrainz.

## Ingestion locale (source primaire à terme)
- Point d'entrée réel du logiciel : un **répertoire surveillé** (`notify`), tags lus via `lofty`, base SQLite (`rusqlite`).
- Plex n'est qu'une **source de test** au démarrage ; la version cible ingère directement depuis le dossier.

## Enrichissement métadonnées (module 1 — Lecteur)
- **Pochettes** : Cover Art Archive (liée à MusicBrainz, gratuite, propre) via identifiant MusicBrainz de l'album. Repli à quatre étages quand ni les tags ni le dossier n'en ont : CAA par **release** (`MUSICBRAINZ_ALBUMID`), CAA par **release-group** (artiste MBID + titre), puis **Deezer** (API publique sans clé, artiste + album qui doivent concorder) pour les albums sans identifiant MusicBrainz. Les images Deezer ne servent qu'à l'affichage, en cache local ; rien n'est écrit dans la bibliothèque.
- **Bio / genre / crédits** : Wikidata et Wikipédia, résolus par identifiant MusicBrainz de l'artiste.
- **Biographies d'artiste** : TheAudioDB, résolu **exclusivement par MBID** (`artist-mb.php?i=`), jamais par nom — voir `docs/enrichissement-lecteur.md`. Biographie anglaise et française (`strBiographyFR`), quand disponibles.
- **Crédits détaillés par édition** (musicien de session, producteur, ingénieur du son) **et label + numéro de catalogue** : Discogs, via ses **dumps mensuels CC0** (`data.discogs.com`) — jamais l'API en direct, une seule passe pour les deux. Lien retrouvé par la relation d'URL que MusicBrainz porte vers Discogs, jamais une recherche par nom. Le label est affiché seul, sans navigation/filtrage par label (décision explicite) — détail : `docs/enrichissement-lecteur.md`.
- **Critiques d'albums** : ~~PAS d'API libre exploitable~~ — **inexact**. **CritiqueBrainz** (MetaBrainz, licence Creative Commons — BY-SA ou BY-NC-SA selon la critique) publie des critiques par identifiant MusicBrainz de release-group, sans clé. Le texte complet peut être stocké et affiché, à condition d'une attribution obligatoire (auteur, mention CritiqueBrainz, licence exacte) partout où il apparaît. Détail : `docs/enrichissement-lecteur.md`.
- **Tags de genre communautaires (Last.fm)** : `artist.getTopTags` par MBID d'artiste, clé personnelle gratuite (`last.fm/api`, pas de clé de test partagée). Ne sert pas le Lecteur — sert le nommage des 12 familles du mode Explorer, en vote aux côtés de MusicBrainz et d'un vocabulaire CLAP-texte construit depuis Wikipédia. Détail : `docs/nommage-familles.md`.
