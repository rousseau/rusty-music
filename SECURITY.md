# Sécurité

## Signaler une vulnérabilité

Ouvre un signalement privé via l'onglet **Security → Report a vulnerability**
du dépôt GitHub (*private vulnerability reporting*). N'ouvre pas d'issue
publique pour une faille.

Réponse visée sous une semaine.

## Périmètre

Rusty Music est un logiciel **local et hors ligne**. La surface d'attaque est
limitée à ce qu'il lit et aux quelques requêtes sortantes qu'il émet :

- **analyse de fichiers non fiables** : tags audio (`lofty`), flux audio
  (`symphonia`, `opus-decoder`), extraits OpenStreetMap `.osm.pbf` (`osmpbf`) —
  un fichier malveillant dans le dossier surveillé ou un extrait OSM piégé ;
- **requêtes sortantes** en HTTP vers MusicBrainz, ListenBrainz, Deezer et
  Cover Art Archive (`ureq` + `rustls`), déclenchées par les passes
  d'enrichissement — jamais automatiques ;
- **modèles** : les poids sont téléchargés depuis les *release assets* du
  dépôt avec vérification SHA-256 (`scripts/telecharger-modeles.sh`).

Ne sont **pas** dans le périmètre : les vulnérabilités des dépendances tierces
(signale-les en amont ; `cargo deny check` suit les avis RUSTSEC en CI), et
tout scénario supposant un accès déjà obtenu à la machine de l'utilisateur.

## Versions

Le projet est en `0.1.x`. Les correctifs de sécurité vont sur `main` et dans la
release suivante ; il n'y a pas de branche de maintenance séparée.
