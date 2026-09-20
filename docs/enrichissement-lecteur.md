# Trois sources d'enrichissement de plus — TheAudioDB, Discogs, CritiqueBrainz

## Intention

Le Lecteur affiche déjà pochettes, genre et bio (Wikidata/Wikipédia), et une
jauge de popularité (ListenBrainz + Deezer, `docs/popularite.md`). Ce chantier
en ajoute trois, chacune **best-effort, asynchrone, jamais bloquante**, et
**désactivable indépendamment** :

1. **TheAudioDB** — biographies d'artiste (anglais et français).
2. **Discogs** — crédits détaillés par édition (musicien de session,
   producteur, ingénieur du son) et label + numéro de catalogue, importés
   depuis les **dumps mensuels CC0**.
3. **CritiqueBrainz** — critiques d'albums, licence Creative Commons.

Une quatrième a suivi, pour un usage différent — voir « 4. Last.fm » plus
bas : elle ne s'affiche pas dans le Lecteur, elle vote le nom des familles du
mode Explorer (`docs/nommage-familles.md`).

`docs/data-sources.md` affirmait qu'aucune API libre n'existait pour les
critiques d'albums — **c'est devenu faux** : CritiqueBrainz couvre ce besoin.

Toutes trois affichent dans l'inspecteur déjà partagé par la carte, l'anneau
et la frise (`apps/desktop/ui/app.js`, `inspecter`/`inspecterAlbum`), et
réutilisent le même mécanisme de « cache » que la popularité : **pas un cache
HTTP générique** (il n'en existe pas dans le code), mais le patron à deux
tables « données + déjà-demandé » déjà employé trois fois (`mb_genres`/
`mb_fetched`, `decouvrir_*`, `popularite`/`popularite_fetched`).

## 1. TheAudioDB — biographies

**Résolution exclusivement par MBID, jamais par nom.** `artist-mb.php?i={mbid}`
retrouve un artiste directement par son identifiant MusicBrainz (vérifié en
direct : clé de test partagée `123`, gratuite, 30 requêtes/minute — une clé
personnelle, si l'utilisateur en a une, l'accélère). Comme `tracks.mb_artist_id`
est déjà rempli à l'ingestion, l'appariement approximatif par nom — risqué,
TheAudioDB porte des doublons documentés (plusieurs artistes identiques pour un
même nom) — ne se pose jamais : **un artiste sans MBID est ignoré**, jamais
mal attribué.

`strMusicBrainzID` de la réponse est vérifié contre le MBID demandé — défense
bon marché contre une réponse mal alignée. Champs retenus : `strBiography`
(anglais) et `strBiographyFR` (français, distinct — confirmé en direct, ex.
Radiohead), stockés avec leur MBID et la date de récupération.

- Client réseau : `crates/core/src/theaudiodb.rs` (`Client::artiste_par_mbid`).
- Passe : `crates/core/src/biographies.rs::actualiser`.
- Schéma : `theaudiodb_artistes` (biographies) + `theaudiodb_fetched` (déjà
  demandé, y compris sans résultat).
- Tauri : `start_biographies { cle }` / `biographies_state` / `bio_piste { id }`.
- CLI : `rusty-music biographies [--cle] [--limite]`.
- Attribution : aucune obligation, mais mention discrète « source :
  TheAudioDB » sous la biographie dans le panneau de droite, par courtoisie —
  même traitement que les crédits Discogs.
- **Photo d'artiste (mode Découvrir).** La même réponse porte `strArtistThumb` ;
  `Client::vignette_par_mbid` la télécharge, sous la même vérification du
  `strMusicBrainzID`. Elle sert de **repli** aux photos des artistes voisins
  (`decouvrir_photo_artiste`), après la recherche d'artiste Deezer — par nom, donc
  filtrée : nom normalisé identique, image non vide, artiste le plus suivi. Voir
  `docs/journal.md`, « Découvrir : du fil en lignes à une grille de pochettes ».

## 2. Discogs — crédits et label par édition, via les dumps CC0

**Jamais l'API Discogs en direct** — les dumps mensuels sous licence CC0
(`https://data.discogs.com/`) donnent le même contenu sans authentification ni
limite de débit, en un seul téléchargement. Un seul fichier est nécessaire :
`releases.xml.gz` (≈ 11 Go compressé, mesuré 2026 — les dumps `artists`/
`labels`/`masters` ne sont pas nécessaires : les crédits sont déjà dans
`releases`, et chaque `<release>` y porte aussi son propre `<labels>` — nom et
numéro de catalogue. Le dump `labels` séparé porterait la fiche complète d'un
label, filiation et sous-labels compris ; hors de propos ici).

**Deux passes séparées dans le temps :**

**a) Liaison** (`crates/core/src/discogs_import.rs::lier`) — légère : pour
chaque édition MusicBrainz connue (`tracks.mb_release_id`, lu à l'ingestion
via `ItemKey::MusicBrainzReleaseId` — Picard : `MUSICBRAINZ_ALBUMID`,
distinct du release-group), retrouve son édition Discogs par la relation
d'URL que MusicBrainz porte (`inc=url-rels`, `type="discogs"`,
`crates/core/src/musicbrainz.rs::discogs_release_id`) — **jamais une
recherche par nom**. Écrit le lien (ou son absence) dans `editions_discogs`.
Peut tourner à chaque enrichissement, comme la popularité.

**b) Import** (`crates/core/src/discogs.rs` + `discogs_import.rs::importer`) —
lourd, **mensuel, séparé du scan normal** : télécharge (ou réutilise,
`--fichier`) le dump, puis n'en extrait que les éditions déjà reliées par (a) —
la table de crédits ne grossit donc que de ce que la bibliothèque possède
réellement, jamais des ~18 M éditions du dump entier.

**Streaming, jamais décompressé sur disque.** Le gzip est décodé en flux
(`flate2::read::GzDecoder`) pendant la lecture : le disque ne voit que le
fichier compressé téléchargé, jamais un XML intermédiaire de plusieurs
dizaines de gigaoctets. Chaque `<release>` est isolé par une simple recherche
de ses bornes (`<release `/`</release>`) avant d'être analysé — **tolérant
aux irrégularités documentées du format** : un fragment illisible (structure
inconsistante d'un type d'entité à l'autre, signalée par la communauté
Discogs) ne coûte que ses propres crédits, jamais tout l'import ni les
fragments suivants, puisque le repérage des bornes ne dépend en rien du
contenu XML lui-même.

**Le label ride la même passe.** `parser_fragment` (`crates/core/src/discogs.rs`)
lit `<extraartists>` et `<labels>` du même fragment `<release>` en une seule
traversée : aucun appel réseau ni passe séparée pour le label. **Affichage
seul, volontairement** : pas de filtre ni de navigation par label dans
Découvrir — question posée (Ninja Tune comme exemple), tranchée ainsi le
12 septembre 2026. Rouvrir le sujet demande de relire cette décision, pas
seulement d'ajouter la table qui, elle, existe déjà.

**Espace disque** : le seul coût est le fichier téléchargé (≈ 11 Go,
transitoire — à supprimer après un import réussi si l'espace presse). La
croissance de la base SQLite est négligeable : quelques milliers de lignes
dans `credits_discogs`, pas les millions d'éditions du dump.

**Attribution** : licence CC0, aucune obligation contractuelle. Mention
discrète « source : Discogs » gardée dans l'inspecteur par courtoisie
(décision utilisateur), non requise légalement.

- Schéma : `editions_discogs` (lien MB → Discogs, mémorise aussi les
  vérifications sans résultat) + `credits_discogs` (personne, rôle, portée-
  piste — notation Discogs brute, informative, pas résolue vers `track_no`)
  + `labels_discogs` (nom, numéro de catalogue — plusieurs lignes possibles
  par édition : réédition, ou sous-label et label parent) +
  `discogs_importes` (éditions déjà traitées par un import, y compris sans
  rien trouvé — voir plus bas, section « Interface »).
- Tauri : `start_discogs_liaison { contact }` / `discogs_liaison_state` /
  `credits_piste { id }` / `labels_piste { id }`, invoquées par la chaîne
  « Scanner » (case « Discogs » du rail), pas par un bouton dédié ;
  `discogs_dump_info` (taille et date sans rien télécharger) /
  `start_discogs_dump` / `discogs_dump_state`, seules liées à un bouton
  (« Télécharger le dernier dump », bloc « Dump Discogs ») ;
  `start_discogs_import` / `discogs_import_state` / `discogs_import_utile`,
  invoquées par la chaîne elle aussi (**sûre à relancer** :
  `credits_poser`/`labels_poser` remplacent toujours ce qui existait pour
  une édition, jamais un ajout).
- CLI, pour qui préfère ne pas passer par l'interface (cron, script) :
  `rusty-music discogs-lier [--contact] [--limite]` (liaison),
  `rusty-music import-discogs [--fichier <chemin>]` (import — télécharge le
  dernier dump si `--fichier` est omis, ou réutilise celui du bloc « Dump
  Discogs » de l'interface, à côté de la base : même fichier, les deux
  chemins interopèrent).

## 3. CritiqueBrainz — critiques d'albums

**Aucune clé pour la lecture**, interrogation par identifiant MusicBrainz de
release-group (`GET critiquebrainz.org/ws/1/review/?entity_id=…
&entity_type=release_group`, vérifié en direct, sans en-tête spécial) —
jamais de recherche approximative, `mb_release_groups` le porte déjà (même
résolution que la popularité et les genres à l'échelon album).

**Bornée à ce qu'on possède, pas à `mb_release_groups`.** Cette table porte
toute la discographie de chaque artiste connu (nécessaire ailleurs pour
désambiguïser un titre parmi l'ensemble des albums d'un artiste), bien plus
large que la bibliothèque réelle — sur un cas mesuré, 83 000 release-groups
pour 2 700 albums possédés, un facteur ~30. Sans filtre, la passe
interrogeait CritiqueBrainz pour des albums jamais affichés nulle part,
gonflant un premier passage complet de ~20 min à ~9 h pour le même résultat
utile. `critiques::actualiser` restreint désormais les candidats à
`Library::release_groups_possedes` (intersection de `mb_release_groups` et
des couples `(artiste, album)` réellement présents dans `tracks`) avant
d'appliquer `limite`.

**Licence Creative Commons — BY-SA ou BY-NC-SA selon la critique, jamais
supposée uniforme** : stockée par ligne (`licence_id`, ex. « CC BY-SA 3.0 »,
vérifié en direct). Une licence CC autorise explicitement à garder le texte
complet, à condition d'une **attribution obligatoire et non négociable**
partout où il apparaît : auteur (`user.display_name`), mention CritiqueBrainz,
licence exacte. Distincte visuellement de la mention Discogs (courtoisie, pas
obligation) dans l'inspecteur.

La plupart des albums n'ont **aucune** critique — état normal, pas une
erreur, marqué dans `critiques_fetched` comme pour les autres sources
(fenêtre de péremption plus longue que la popularité, 180 j : une critique
neuve reste rare). Dans ce cas, un lien de sortie vers la page CritiqueBrainz
de l'album invite à en écrire une plutôt qu'un vide silencieux.

- Client réseau : `crates/core/src/critiquebrainz.rs`
  (`Client::avis_pour_release_group`).
- Passe : `crates/core/src/critiques.rs::actualiser`.
- Schéma : `critiques` (une ligne par critique) + `critiques_fetched`.
- Tauri : `start_critiques { rafraichir }` / `critiques_state` /
  `critiques_piste { id }` / `lien_ecrire_critique { id }`.
- CLI : `rusty-music critiques [--limite] [--rafraichir-des]`.

## 4. Last.fm — tags de genre, pour le nommage des familles

**N'affiche rien dans le Lecteur** — sert le vote de nommage des familles du
mode Explorer, aux côtés de MusicBrainz et du vocabulaire CLAP-texte : voir
`docs/nommage-familles.md`. Interrogation `artist.getTopTags` par MBID
d'artiste, jamais par nom — même discipline que TheAudioDB.

**Seule des quatre sources à exiger une clé personnelle** — gratuite sur
`last.fm/api`, mais Last.fm n'offre pas de clé de test partagée comme
TheAudioDB. Décochée par défaut dans le rail tant qu'aucune clé n'est
renseignée (`docs/popularite.md` avait écarté Last.fm pour la popularité pour
cette même raison ; ici la clé est demandée, pour un usage différent).

Une clé refusée (HTTP 401/403, ou codes Last.fm 4/10/26/29) **arrête la
passe** et fait remonter l'erreur — `lastfm.rs::EchecTags::Bloquant` : un
autre artiste échouerait à l'identique, et un bilan « 0 tag » serait
indiscernable d'une bibliothèque sans tags. Un échec ponctuel (5xx, réseau)
laisse l'artiste pour un prochain passage (`Bilan.echecs`) sans interrompre.

- Client réseau : `crates/core/src/lastfm.rs` (`Client::tags_artiste`).
- Passe : `crates/core/src/lastfm_pass.rs::actualiser`.
- Schéma : `lastfm_tags` (une ligne par tag) + `lastfm_fetched`.
- Tauri : `start_lastfm { cle, rafraichir }` / `lastfm_state`.
- CLI : `rusty-music lastfm --cle <clé> [--limite] [--rafraichir-des]`.

## Interface

Rail d'analyse (mode Bibliothèque, près de « Rafraîchir aussi la
popularité… ») : trois cases indépendantes — « Biographies (TheAudioDB) »,
« Critiques (CritiqueBrainz) », « Discogs (crédits et labels par édition) »
— cochées par défaut, aucune clé requise. Décocher l'une n'affecte pas les
autres. Une quatrième, « Tags de genre (Last.fm) », décochée par défaut le
temps qu'une clé personnelle soit renseignée. **Un seul geste pour les
quatre** : cocher ce qu'on veut, puis « Scanner »/« Analyser » — aucune des
quatre n'a son propre bouton séparé, sur le même principe que le reste de la
chaîne (empreintes, descripteurs, genres, popularité).

**Discogs est le cas le plus composite des quatre**, mais reste actionné par
la seule case ci-dessus — deux boutons dédiés (« Lier maintenant »,
« Importer crédits et labels ») ont existé un temps puis ont été retirés,
jugés après coup comme de la complexité d'interface superflue une fois
l'enchaînement automatique fiable :

1. **Liaison** — `passeDiscogsLiaison` (`app.js`), retrouve l'édition
   Discogs de chaque édition MusicBrainz connue. Fait partie de la chaîne
   « Scanner » comme n'importe quelle autre étape ; rejouée à chaque passage,
   incrémentale (ne revérifie jamais une édition déjà réglée).
2. **Import** — se déclenche tout seul juste après, via
   `importerDiscogsSiUtile(phase)`, **si** `discogs_import_utile` (commande
   Tauri, voir `Library::discogs_import_utile`) répond qu'une édition reliée
   n'a encore jamais été importée, **et** qu'un dump est déjà sur le disque
   (`discogs_dump_info`).

**Le critère de déclenchement de l'import n'est jamais « cette liaison-ci
a-t-elle relié du neuf »** — une première version s'y fiait
(`EtatDiscogsLiaison.liees > 0`), mais une édition a pu être reliée par un
passage précédent (avant ce mécanisme, ou un import interrompu en cours de
route) sans jamais avoir été importée ; ce critère-là la manquait
indéfiniment, même en relançant liaison après liaison. `discogs_import_utile`
interroge directement ce qui reste à faire via `discogs_importes` — une
table qui marque une édition comme traitée dès qu'un import l'a réellement
rencontrée dans le dump, même sans rien à y ranger (l'absence dans
`credits_discogs`/`labels_discogs` ne distingue pas « jamais importé » de
« importé, rien trouvé »). Une édition reliée mais absente du dump n'est
jamais marquée : elle redevient candidate tant qu'un import ne l'a pas
réellement vue.

Relire tout le dump coûte ~10 min sur 11 Go (le format n'a pas d'index, pas
de raccourci possible) — d'où la vérification avant de s'y relancer à chaque
scan si tout est déjà importé. Silencieux (ni case ni message dédiés) si
aucun dump n'a jamais été téléchargé : rien ne se passe côté Discogs tant
que le bloc suivant n'a pas servi une première fois.

Un bloc séparé, « Dump Discogs », plus bas dans le même mode (après
« Source de la bibliothèque », avant « Stockage ») : taille et date du
dernier téléchargement (ou son absence), et un unique bouton « Télécharger
le dernier dump » avec sa jauge de progression en Go. Volontairement à
l'écart de la chaîne « Scanner » — le téléchargement, contrairement à la
liaison et à l'import, n'est ni incrémental ni borné par la bibliothèque
(l'intégralité du catalogue Discogs, ~11 Go, quelle que soit la taille de la
bibliothèque locale) : un geste délibéré, mensuel, jamais automatique.

Inspecteur (`apps/desktop/ui/app.js`, `montrerBio`/`montrerCredits`/
`montrerCritiques`, appelées par `inspecter`) : même patron que
`montrerDescripteurs` — garde de course sur le morceau visé, tiret ou bloc
caché quand l'information manque, jamais une valeur inventée. Ces trois blocs
restent cachés depuis `inspecterAlbum` (un nœud d'album de l'anneau ne porte
pas d'identifiant de piste compatible avec `bio_piste`/`credits_piste`/
`critiques_piste`) — simplification assumée, comme `inspecterAlbum` le fait
déjà pour les descripteurs (BPM/tonalité).

Le bloc « Famille » (`montrerNomFamille`, même inspecteur) affiche le nom
issu du vote entre MusicBrainz, CLAP-texte et Last.fm, mais ne suit **pas**
ce patron piste-par-piste : le vote est par famille, pas par morceau — voir
`docs/nommage-familles.md`.
