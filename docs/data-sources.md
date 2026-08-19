# Sources de données

## Bibliothèque locale — Plex
- Serveur Plex local (macOS), déjà configuré et fonctionnel.
- URL locale : `http://127.0.0.1:32400`.
- Token API récupéré via le lien « Sign in with Plex » (méthode recommandée) ou manuellement via le XML d'un morceau (`View XML` → paramètre `X-Plex-Token`).

## AudioMuse-AI (référence / outil en parallèle)
- Installé en app native macOS, relié au serveur Plex ci-dessus.
- Fonctionnalités utilisées comme référence : clustering audio, « Music Map » (carte 2D), « Song Paths » (chemin entre deux morceaux), empreinte sonique.
- Sert de benchmark fonctionnel pendant que ce projet développe sa propre interface, plus légère.

## Connexions et collaborations entre artistes — API MusicBrainz
- Utiliser l'API MusicBrainz plutôt que du scraping web généraliste : données structurées, gratuites, bien documentées.
- Types de relations artiste-artiste pertinents : `collaborator on` (collaboration ponctuelle), `member of` (membre d'un groupe), `founder`, etc.
- Approche envisagée : pour chaque artiste présent dans la bibliothèque locale, interroger l'API MusicBrainz (`artist-rels`) et construire un graphe d'arêtes (artiste A — type de relation — artiste B).
- Projet de référence pour l'architecture de graphe : *discogsography* (open source, Neo4j) — fonctionnalités « Path Finder » (plus court chemin entre deux entités) et « Collaboration Network » (graphe + centralité), à partir de données Discogs + MusicBrainz.

## Ingestion locale (source primaire à terme)
- Point d'entrée réel du logiciel : un **répertoire surveillé** (`notify`), tags lus via `lofty`, base SQLite (`rusqlite`).
- Plex n'est qu'une **source de test** au démarrage ; la version cible ingère directement depuis le dossier.

## Enrichissement métadonnées (module 1 — Lecteur)
- **Pochettes** : Cover Art Archive (liée à MusicBrainz, gratuite, propre) via identifiant MusicBrainz de l'album.
- **Bio / genre / crédits** : Wikidata et Wikipédia, résolus par identifiant MusicBrainz de l'artiste.
- **Critiques d'albums** : PAS d'API libre exploitable (AllMusic, Pitchfork… sous copyright, sans API ouverte). Prévoir un lien sortant ou s'en tenir aux données factuelles — ne pas concevoir l'UI autour de cette donnée.
