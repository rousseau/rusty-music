# Sécurité

## Signaler une vulnérabilité

Ouvre un signalement privé via l'onglet **Security → Report a vulnerability**
du dépôt GitHub (*private vulnerability reporting*). N'ouvre pas d'issue
publique pour une faille.

Réponse visée sous une semaine.

## Périmètre

Rusty Music calcule tout en local ; il ne va sur le réseau que pour
enrichir la bibliothèque et télécharger des modèles. La surface d'attaque est
limitée à ce qu'il lit et aux requêtes sortantes qu'il émet :

- **analyse de fichiers non fiables** : tags audio (`lofty`), flux audio
  (`symphonia`, `opus-decoder`), extraits OpenStreetMap `.osm.pbf` (`osmpbf`),
  dump Discogs (XML compressé) — un fichier malveillant dans le dossier
  surveillé ou une source piégée ;
- **réponses d'API** en HTTPS (`ureq` + `rustls`) : MusicBrainz, Cover Art
  Archive, ListenBrainz, Deezer, TheAudioDB, CritiqueBrainz, Discogs, Last.fm.
  Déclenchées par l'utilisateur (« Scanner », passes d'enrichissement, mode
  Découvrir) ou à l'affichage d'une pochette manquante ; seul un lien
  `https://www.deezer.com/` venu d'une réponse est ouvert dans le navigateur ;
- **Ollama** (`http://localhost:11434`), seulement pour le champ « Lama » : sa
  réponse est interprétée en spécification typée, jamais exécutée ;
- **modèles** : téléchargés depuis les *release assets* du dépôt
  (`scripts/telecharger-modeles.sh`), GitHub ou Hugging Face au premier usage,
  avec vérification de la taille et, quand elle est connue, du SHA-256
  (`crates/core/src/modeles.rs`).

Ne sont **pas** dans le périmètre : les vulnérabilités des dépendances tierces
(signale-les en amont ; `cargo deny check` suit les avis RUSTSEC, lancé avant chaque publication), et
tout scénario supposant un accès déjà obtenu à la machine de l'utilisateur.

## Versions

Le projet est en `0.1.x`. Les correctifs de sécurité vont sur `main` et dans la
release suivante ; il n'y a pas de branche de maintenance séparée.
