# Enrichissement croisé : tags, sources en ligne et empreintes

Note de recherche du 23 novembre 2026, à la demande de François : « comment
enrichir l'analyse de la bibliothèque en utilisant conjointement les
métadonnées, les données externes et les empreintes sonores ? ». Rien n'est
codé ; ce document propose, il ne décide pas.

## Ce qu'on a déjà, et ce qui ne se parle pas

Trois familles de données vivent dans la base, chacune servie à un usage :

| Famille | Tables | Sert aujourd'hui à |
|---|---|---|
| **Métadonnées des fichiers** | `tracks` (titre, artiste, album, `genre`, `year`, MBID quand les tags le portent) | affichage, regroupement, familles (repli) |
| **Sources en ligne** | `mb_genres` (votes), `mb_release_groups`, `lastfm_tags` (poids, par artiste), `popularite` / `track_popularite` (ListenBrainz, Deezer), `credits_discogs`, `labels_discogs`, `critiques`, `theaudiodb_artistes` | nommer les familles, artères de la carte, inspecteur |
| **Le son** | `features` (CLAP 512 d, projection, `cluster`), `descriptors` (tempo, tonalité, énergie, timbre), `track_loudness` | carte, voisins, ✦, Lama, filtres |
| **L'écoute** (une quatrième) | `ecoute`, `favori`, `playlist_piste` | Accueil, historique |

Les familles croisent déjà deux sources (vocabulaire de genres, repli k-means
sur l'empreinte : `nommage-familles.md`), mais **dans un seul sens** : le son
ne corrige jamais un tag, et un tag ne contrôle jamais le son. Les erreurs
connues le montrent : Steve Coleman and Metrics rangé en Reggae·Dub, Soul
Coughing en Rock·Grunge — le clustering s'est trompé, rien ne l'a signalé.

Manques relevés en passant : le BPM de Deezer n'est pas conservé ; la date de
première sortie d'un release-group (MusicBrainz `first-release-date`) non
plus ; les tags Last.fm sont par artiste, pas par morceau.

## Principe : une fiche à plusieurs voix

Chaque attribut d'un morceau (genre, ambiance, voix, tempo, année…) peut
avoir **plusieurs valeurs, chacune avec sa source et sa confiance**. Trois
usages en découlent, du plus sûr au plus ambitieux :

1. **Vérifier** : quand les voix se contredisent, le dire.
2. **Compléter** : quand une voix se tait, la faire parler par les autres.
3. **Découvrir** : lire la bibliothèque à travers leurs écarts.

Stockage proposé, une migration :

```sql
CREATE TABLE attribut (
    track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    cle       TEXT NOT NULL,   -- 'genre', 'voix', 'ambiance', 'tempo', 'annee', 'live'…
    valeur    TEXT NOT NULL,
    source    TEXT NOT NULL,   -- 'tag', 'mb', 'lastfm', 'deezer', 'clap-voisins', 'clap-sonde', 'clap-texte', 'mesure', 'main'
    confiance REAL NOT NULL,   -- 0..1
    calcule_le INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (track_id, cle, valeur, source)
);
```

Une correction à la main (`source = 'main'`) l'emporte toujours et n'est
jamais réécrite par une passe — le même contrat que `edition_choisie`.

## 1. Vérifier : les désaccords

Le plus utile tout de suite : peu de calcul, des erreurs réelles, une liste
« À vérifier » dans la Bibliothèque (à côté des pochettes suspectes).

| Contrôle | Comment | Ce qu'il trouve |
|---|---|---|
| **Genre contre voisinage** | pour chaque morceau, la distribution des genres (tags + `mb_genres`) de ses k = 20 voisins CLAP ; signaler si le sien y pèse moins de 10 % | tags faux, *ou* morceaux que CLAP place mal (les deux erreurs connues) — l'écran dit lequel des deux est suspect, l'utilisateur tranche |
| **Famille contre voisinage** | même calcul sur la famille affectée | familles mal nommées, clusters hétérogènes |
| **Tempo mesuré contre Deezer** | conserver `bpm` de Deezer (déjà dans la réponse de l'API track) ; écart > 4 % hors rapports 2, 3/2, 1/2 | erreurs de mesure ; les rapports d'octave nourrissent le banc de tempo (`banc-tempo-gtzan`) |
| **Doublons** | cosinus CLAP > 0,98 et durée à ± 2 s sous des chemins différents | doublons, compilations, remasters — `editions` et MusicBrainz disent lequel est l'original |
| **Live ou studio** | score zéro-shot CLAP « enregistré en public, applaudissements » contre titre/édition (« Live », type de release MusicBrainz) | lives non marqués, studio mal tagués |
| **Année** | voir § 3, l'âge du son ; signaler seulement un écart > 15 ans | années de réédition à la place de l'originale |

Mesure : sur la bibliothèque de test (27 044 morceaux), compter les
signalements par contrôle et en relire 50 tirés au hasard ; un contrôle
qui se trompe plus d'une fois sur trois ne va pas dans l'interface.

## 2. Compléter : faire parler les silencieux

Beaucoup de morceaux n'ont ni MBID ni tags Last.fm (artistes rares, rips sans
identifiants) ; ce sont justement les « petits quartiers » de la carte.

**a. Propagation par les voisins** (le plus simple). Un morceau sans genre
reçoit la moyenne pondérée des genres de ses voisins CLAP, confiance = part
du genre majoritaire × similarité moyenne. Itérer deux fois au plus (label
propagation), jamais à travers un morceau propagé à confiance < 0,5.

**b. Une sonde linéaire sur les empreintes.** Une régression logistique par
attribut, entraînée sur les morceaux où les sources en ligne sont nettes
(tag Last.fm de poids ≥ 50, genre MusicBrainz à ≥ 3 votes). Attributs
visés, choisis pour être audibles : voix / instrumental, voix féminine /
masculine, acoustique / électrique, cuivres, cordes, piano, énergie, ambiance
(sombre / lumineuse, calme / agitée). 512 entrées, quelques milliers
d'exemples : quelques secondes d'entraînement en Rust (`linfa-logistic`, ou
vingt lignes de descente de gradient — vérifier la licence avant, voir
`rust-audio-stack.md`). Validation croisée à 5 plis, on ne garde que les
attributs à F1 ≥ 0,8.

**c. Le zéro-shot de l'encodeur texte CLAP** (déjà dans l'app pour Lama).
Score cosinus de chaque morceau contre un vocabulaire (« saxophone ténor »,
« voix féminine », « boîte à rythmes », « enregistré en public »). Utile pour
ce qu'aucune source ne nomme ; les tags externes servent à **régler les
seuils** (courbe précision-rappel sur les morceaux tagués), pas à les deviner.

Les trois écrivent dans `attribut` avec leur source ; l'inspecteur montre
« voix féminine · déduit du son (0,87) » autrement qu'un tag lu. Lama et les
filtres d'Explorer y gagnent des critères (« instrumental, cuivres, calme »)
sans nouvel appel réseau.

## 3. Découvrir : lire la bibliothèque par les écarts

| Idée | Données croisées | Ce qu'on voit |
|---|---|---|
| **Signature d'un artiste** | centroïde et dispersion de ses empreintes ; par année de sortie | artistes constants ou éclectiques ; la trajectoire d'une carrière (une ligne dans le nuage, année par année) |
| **« Sonne comme, sans que personne ne les relie »** | proximité CLAP entre artistes contre artistes similaires de ListenBrainz / Last.fm | les écarts sont des découvertes, les accords des confirmations ; une rangée de l'Accueil |
| **Pépites cachées** | voisins CLAP des favoris (`favori`, `ecoute`) × faible `track_popularite` | des morceaux proches de ce qu'on aime, que peu de gens écoutent |
| **L'âge du son** | régression de l'année sur l'empreinte (comme la sonde, entraînée sur les dates sûres) contre l'année réelle | rétro, en avance sur son temps, rééditions ; une frise « sonne comme les années 70 » |
| **Intrus d'un album** | distance de chaque piste au centroïde de son album | interludes, bonus, pistes mal rangées |
| **Filiations** | dates (MusicBrainz), proximité CLAP, liens de personnes (membres, producteurs : `credits_discogs`) | nourrit `frise-filiations.md` : une arête n'est proposée que si le son et une relation la soutiennent |

## Ordre proposé

1. **Conserver ce qui manque** : BPM Deezer, `first-release-date`. Petit,
   aucune décision d'interface.
2. **Vérifier** : table `attribut`, contrôles « genre contre voisinage » et
   « doublons », liste « À vérifier » dans la Bibliothèque. Mesure de
   précision sur 50 cas avant d'afficher.
3. **Compléter**, sonde linéaire d'abord (b), propagation ensuite (a) ;
   zéro-shot (c) pour les seuls attributs que la sonde ne couvre pas.
4. **Découvrir** : pépites cachées et « sonne comme » en rangées de l'Accueil ;
   signature d'artiste dans le bandeau de l'artiste.

Chaque étape se juge sur une mesure écrite dans `journal.md` avant d'atteindre
l'interface, comme le banc de tonalité ou de tempo.

## Questions ouvertes

- Les corrections à la main d'un genre réécrivent-elles le tag du fichier, ou
  seulement la base ? (Aujourd'hui l'app n'écrit jamais dans les fichiers.)
- Last.fm par artiste suffit-il, ou faut-il `track.getTopTags` (un appel par
  morceau : 27 000 appels, à étaler) ?
- La sonde s'entraîne sur la bibliothèque de l'utilisateur : ses biais
  (beaucoup de jazz, peu de rap) deviennent ceux des prédictions. Faut-il un
  jeu de référence commun (FMA, MTG-Jamendo, licences à vérifier) ?
- Faut-il remplacer le repli k-means des familles par la sonde de genre ?
