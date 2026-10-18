# Compagnon iOS du mode Écouter — plan

Établi le 18 oct. 2026, à partir du code (`crates/core`, `crates/player`,
`apps/desktop`, `crates/core/sql/schema.sql`) et de `plan-ecouter-v0.2.md`.
Même forme que les autres plans : état **constaté**, **options** comparées,
recommandation. Décidé : développement lancé le 18 oct. 2026, usage personnel seulement
(pas d'App Store). Le reste est une liste à trancher.

Légende d'effort : **S** < 1 jour · **M** 1 à 3 jours · **L** > 3 jours.

## L'idée

L'iPhone n'emporte pas la bibliothèque : il emporte une **valise**, c'est-à-dire
une sélection d'albums préparée par le Mac pour être découverte en mobilité.
Exemples : disques ajoutés récemment, albums peu écoutés, albums sur un thème.
La valise se renouvelle de temps en temps. Les écoutes faites sur le téléphone
reviennent au Mac.

Le Mac garde tout le calcul : CLAP, carte, Lama, enrichissement, loudness. Le
téléphone ne fait que **lire** : il lit une base SQLite réduite et des fichiers
audio, avec le même `player` que le bureau.

```
   Mac (rusty-music)                         iPhone (compagnon)
 ┌──────────────────────┐   valise/   ┌──────────────────────────┐
 │ sélection (règles)   │ ──────────▶ │ base réduite + audio     │
 │ réencodage Opus      │             │ lecture (crates/player)  │
 │ base réduite         │             │ journal d'écoutes, avis  │
 │ fusion du journal  ◀─┼──────────── │                          │
 └──────────────────────┘  journal    └──────────────────────────┘
```

## Constats dans le code

- **Tauri 2** (`Cargo.toml` du workspace), qui sait produire une app iOS
  (`tauri ios init`, projet Xcode généré, plugins natifs en Swift).
- **`crates/player`** ne dépend que de `core`, `wsola`, `rustfft`, `rubato`,
  `rodio` et `cpal`. `cpal` a un backend iOS (AudioUnit RemoteIO).
- **`crates/core`** est plus lourd qu'il ne faut pour un lecteur. Il embarque
  `notify`, `ureq`, `image`, `contour`, `geo-types`, `quick-xml`, `ebur128`
  ainsi que les modules d'enrichissement (MusicBrainz, Discogs, Last.fm,
  Deezer, Ollama…). Le lecteur n'en appelle que `decode`, `gapless`, `opus`
  et `loudness`. Il faudra des *features* (étape 1).
- **L'identité d'un morceau est son chemin absolu** : `tracks.path`,
  `ecoute.chemin`, `favori.chemin`, playlists. C'est commode : le journal
  du téléphone pourra porter le chemin d'origine, et la fusion se fait sans
  table de correspondance.
- **Critères de sélection déjà disponibles** :
  - récence : `tracks.added_at`, et la date d'ajout plus fine `ajoute_le` de
    `db.rs` ;
  - écoutes : table `ecoute` (journal), `jours_depuis_ecoute`, `favori` ;
  - popularité : `track_popularite`, `popularite` ;
  - thème : genres (`mb_genres`, `lastfm_tags`), familles de la carte
    (`features.cluster`), voisins (`decouvrir_voisins`), requête CLAP-texte,
    spec Lama ;
  - outils : `filtres_playlist.rs` (`selectionner`, `plafonner_par_artiste`,
    `ajuster_a_duree`) et `playlists.rs` (`ecrire_m3u8`).
- **Enrichissement à emporter** : biographies (`theaudiodb_artistes`),
  critiques (`critiques`, attribution CritiqueBrainz obligatoire), crédits
  (`credits_discogs`), loudness (`track_loudness`, `album_loudness`). Les
  pochettes sont dans un cache disque (`<données app>/pochettes/`, voir
  `cover` dans `apps/desktop/src/main.rs`).
- **« En cours de lecture »** passe par `souvlaki`, déclaré pour macOS
  seulement : il n'y a rien pour iOS.

## Étape 0 — Prototype qui lève les risques · **M**

**Avancement (18 oct. 2026)** — `apps/mobile/` (« Rusty Valise »,
`fm.rustymusic.compagnon`) :
- `crates/core` et `crates/player` **compilent pour `aarch64-apple-ios` sans
  modification**. Le découpage de l'étape 1 n'est donc plus un préalable : il
  ne sert plus qu'à alléger le binaire ;
- la session audio se règle **en Rust** (`src/session_audio.rs`, par
  `objc2-avf-audio`, déjà tiré par `cpal`) : pas de plugin Swift pour cette
  partie ;
- l'app lit les valises du dossier Documents (`$RUSTY_VALISES` sur le Mac),
  affiche les albums par règle, les pistes, et joue par `Player`.
  `set_resolveur` traduit chemin d'origine → fichier de la valise, et le gain
  d'album vient de la base ;
- `Info.ios.plist` : `UIBackgroundModes = audio`, partage de fichiers par le
  Finder ;
- projet Xcode généré (`cargo tauri ios init`, `gen/apple/`). Il a fallu
  `xcodegen`, `libimobiledevice` et `cocoapods` (Homebrew) ;
- **l'app tourne sur le simulateur** (iPhone 18 Pro, iOS 27.1) : la
  valise d'essai s'ouvre, les albums s'affichent par règle avec leurs
  pochettes ;
- « En cours de lecture » et les commandes à distance sont écrits en Rust
  (`src/en_cours.rs`, `objc2-media-player`), sans pochette pour l'instant
  (il faudrait les liaisons UIKit) ;
- **reste** : vérifier la lecture, l'écran verrouillé et les commandes
  (sur le simulateur puis l'iPhone, ce qui demande une équipe de
  signature), puis les mesures (batterie, gapless).

**Pièges de construction (Xcode 27, Swift 6.4)** :
1. `swift-rs` 1.0.9 compile le paquet Swift de Tauri avec `--arch`, et le
   nouveau moteur de SwiftPM analyse alors les modules contre le SDK macOS
   (« clang dependency scanning failure … AppKit »). Corrigé en amont, mais
   pas encore publié : `[patch.crates-io]` dans `Cargo.toml` sur le commit
   corrigé, à retirer à la prochaine version.
2. Une bibliothèque statique ne déclare pas ses frameworks à l'édition de
   liens Xcode. AudioToolbox, CoreAudio, AVFAudio et MediaPlayer sont donc
   ajoutés à la main dans `gen/apple/project.yml` (puis `xcodegen
   generate`). La clé `bundle > iOS > frameworks` de Tauri n'a aucun effet
   ici. À reporter si `cargo tauri ios init` régénère le fichier.
3. iOS 27 arrête au lancement une app sans cycle de vie par scènes. `tao`
   ne branche son délégué de scène que si `Info.ios.plist` déclare
   `UIApplicationSceneManifest > UIApplicationSupportsMultipleScenes`.
4. `cargo tauri ios build` ne remplace pas un `.app` déjà présent dans
   `gen/apple/build/arm64-sim/` (« Directory not empty ») : le supprimer
   avant de reconstruire, sinon on réinstalle l'ancienne version sans le
   savoir.

**Préalable manquant (18 oct. 2026)** : Xcode n'est pas installé sur le Mac
(seulement les outils en ligne de commande). Il est indispensable : SDK iOS,
signature, déploiement sur l'iPhone. Il faut aussi les cibles Rust
`aarch64-apple-ios` et `aarch64-apple-ios-sim`. Attention à la place :
Xcode et un simulateur iOS demandent plusieurs dizaines de Go, et le SSD
n'en a plus que 29 de libres.

Toute la suite dépend de cette étape. Il faut construire le plus petit projet
Tauri iOS qui :

1. lit un FLAC, un M4A et un Opus embarqués dans l'app, par
   `rusty_music_player` (le chemin `ouvrir` réel, gapless compris) ;
2. **continue écran verrouillé** et application en arrière-plan ;
3. s'affiche dans « En cours de lecture » (écran verrouillé, centre de
   contrôle) et répond aux boutons des AirPods ;
4. se met en pause proprement sur un appel entrant ou un débranchement de
   casque, et reprend après l'appel.

**Ce qu'il faut en Swift** (petit plugin Tauri, environ 150 lignes) :
- `AVAudioSession` en catégorie `.playback`, et `UIBackgroundModes = audio`
  dans `Info.plist` ;
- les notifications d'interruption et de changement de route, renvoyées au
  Rust comme des événements ;
- `MPNowPlayingInfoCenter` (titre, artiste, pochette, position) et
  `MPRemoteCommandCenter` (lecture, pause, suivant, précédent, position).

Le projet tient à « tout en Rust à l'exécution ». Ce plugin n'y déroge pas
vraiment : ce n'est ni Python ni un processus externe, mais l'accès à des API
système qui n'existent qu'en Objective-C/Swift. Il fait le pendant de
`souvlaki` sur macOS. La voie 100 % Rust (`objc2` + `objc2-av-foundation`,
`objc2-media-player`) existe aussi ; le prototype dira laquelle est la moins
fragile.

**Mesures à rapporter** :
- consommation sur une heure, écran éteint, contre Musique d'Apple sur les
  mêmes fichiers. `cpal` garde le processeur éveillé pour décoder, alors
  qu'`AVPlayer` profite du décodage matériel ;
- blanc aux raccords d'album (`verif_gapless` adapté) ;
- charge du bouton « E » (WSOLA + excitation) sur un iPhone récent.

**Option de repli si la consommation est mauvaise** : lire par `AVPlayer` (plugin
Swift), en Rust ne garder que la base et la logique de file. On perd le bouton
« E », le time-stretch et le décodage maison. Opus n'est lu par `AVPlayer` qu'en
conteneur CAF, ce qui imposerait AAC dans la valise (étape 2). À ne retenir que
si le prototype l'impose.

## Étape 1 — Découper le code · **M**

**Objectif** : un binaire iOS qui ne tire ni Burn, ni `ort`, ni la carte, ni
l'enrichissement réseau.

- `crates/core` : features Cargo. Par défaut `complet` (comportement actuel),
  plus une feature `lecteur` qui ne garde que `db` (lecture), `decode`,
  `gapless`, `opus`, `loudness` (lecture des valeurs, pas la mesure),
  `memoire`, `session` et `playlists`. `notify`, `ureq`, `image`, `contour`,
  `geo-types` et `quick-xml` deviennent optionnels.
  - Option A : features dans `core` — peu de déplacement, mais beaucoup de
    `#[cfg(feature)]` dans `db.rs` (317 Ko).
  - Option B : extraire un crate `crates/base` (schéma, `TrackRow`, requêtes
    de lecture, mémoire d'écoute) que `core` réexporte.
  - **Recommandation : B.** `db.rs` mélange lecture et passes
    d'enrichissement ; une frontière nette sert aussi le bureau.
- `crates/player` : vérifier qu'il compile pour `aarch64-apple-ios` sans
  autre changement que la dépendance à `base`.
- Nouvelle application `apps/mobile/` (Tauri, iOS seulement au départ) qui
  dépend de `base` et `player`, plus le plugin Swift de l'étape 0.
- `cargo deny check` sur le sous-ensemble mobile (voir « Distribution »).
- `scripts/verifier.sh` : ajouter `cargo check --target aarch64-apple-ios -p
  rusty-music-mobile`.

## Étape 2 — Le format de valise · **M** — **livrée (18 oct. 2026), sans réencodage**

**Fait** : `crates/core/src/valise.rs` + `rusty-music valise exporter`.
Essai sur la bibliothèque réelle : sélection de 26 albums pour 3 Go en
1,5 s ; un export de 600 Mo donne 8 albums, 79 pistes, une base de 456 Ko et
des pochettes de 17 à 130 Ko. La copie est quasi instantanée sur APFS
(`fs::copy` clone les fichiers). Écarts au plan ci-dessous :
- les fichiers de la valise sont nommés `audio/<id>/NN - Titre.<ext>`, avec
  l'extension d'origine ;
- la correspondance piste → fichier vit dans une table `valise_fichier`
  (`track_id`, `album_id`, `fichier`) plutôt que dans une colonne de
  `tracks` : `tracks` garde exactement le schéma du bureau ;
- `valise_album` (ordre, règle, pochette) et `valise_info` (format, date,
  règles) complètent la base ;
- **pas de réencodage Opus** : la bibliothèque est en MP3 à 98,5 %
  (27 321 MP3, 355 M4A, 22 FLAC sur 27 721 pistes), qui se copient tels
  quels de toute façon. Le réencodage des quelques sources sans perte attendra.
  Il demanderait libopus, donc `cmake` au build.


Une valise est un **dossier** autonome, versionné et lisible sans le Mac :

```
<sortie>/
  valise.json        manifeste : version du format, id, date, règles qui
                     l'ont produite, budget, liste des albums
  base.sqlite        base réduite (schéma ci-dessous)
  pochettes/<id>.jpg 600 px, une par album
  audio/<album-id>/NN - Titre.<ext>
```

**Base réduite** : le même schéma que le bureau, restreint aux pistes de la
valise. Le code de lecture de `base` reste ainsi le même des deux côtés.

| Table | Contenu |
|---|---|
| `tracks` | pistes de la valise ; `path` = chemin **d'origine** (identité) ; le fichier dans la valise est donné par `valise_fichier` |
| `track_loudness`, `album_loudness` | valeurs mesurées sur le Mac (pas de remesure sur l'iPhone) |
| `track_popularite`, `mb_genres` | jauge, genres affichés |
| `theaudiodb_artistes`, `critiques`, `credits_discogs` | textes ; l'attribution CritiqueBrainz voyage avec la critique |
| `favori` | état au moment de l'export |
| `ecoute` | **vide** à l'arrivée ; c'est le journal du téléphone |

Exclus : `features` (vecteurs CLAP, inutiles sans calcul), carte, `session`
du bureau, files de travail (`*_fetched`, `scan_failures`).

**Audio : réencoder ou non.**

| Option | Taille d'un album (≈ 45 min) | Pour | Contre |
|---|---|---|---|
| Fichiers d'origine | FLAC ≈ 300 Mo | aucun traitement, fidèle | 30 albums ≈ 9 Go |
| **Opus 160 kb/s** | ≈ 55 Mo | transparent à l'écoute pour l'immense majorité, déjà lu par `opus-decoder`, gapless déjà vérifié (granule de fin) | réencodage sur le Mac (≈ quelques s par album) |
| AAC 256 kb/s | ≈ 85 Mo | lu nativement par `AVPlayer` | encodeur AAC : Apple (AudioToolbox, macOS seulement) ou FDK (licence non libre) |

**Recommandation initiale : Opus 160 kb/s par défaut, avec une case
« garder les originaux »** — révisée : voir « Fait » ci-dessus, les
originaux suffisent tant que la bibliothèque reste en MP3. Les sources déjà avec perte (MP3, M4A) sont copiées telles
quelles plutôt que réencodées : réencoder une source avec perte dégrade sans
rien gagner.

Encodage : crate `opus` (liaison de libopus, BSD) et `ogg` (déjà au
workspace) pour le conteneur. Sur le Mac, à l'export seulement : le téléphone
n'encode jamais. Les tags sont recopiés par `lofty` (commentaires Vorbis) pour
que les fichiers restent lisibles hors de l'app.

**Livrable** : `rusty-music valise exporter --regles <...> --budget 4G
--sortie <dossier>` dans `crates/cli`, testé en écrivant une valise et en la
relisant avec `base` en mode lecture.

## Étape 3 — La sélection · **L**

L'unité est l'**album**, entier, dans l'ordre des pistes. La sélection
répond à un **budget** (en Go, ou en nombre d'albums) partagé entre une ou
plusieurs règles, chacune avec sa part.

| Règle | Source | Détail |
|---|---|---|
| Récents | `ajoute_le` (repli `added_at`) | albums ajoutés depuis N jours, ou les N derniers |
| Peu écoutés | `ecoute` agrégé par album | jamais écoutés, puis écoutés il y a longtemps (`jours_depuis_ecoute`), en excluant les favoris déjà connus |
| Oubliés | `ecoute` + `favori` | aimés, plus écoutés depuis plus d'un an |
| Thème : genre / décennie | `mb_genres`, `year` | choix dans une liste |
| Thème : famille / quartier | `features.cluster`, carte | « un quartier de Paris » |
| Thème : autour d'un artiste | `decouvrir_voisins`, voisinage CLAP | l'artiste plus ses voisins |
| Thème : phrase libre | CLAP-texte ou spec Lama | « jazz modal des années 60 » ; le Mac fait le travail |
| Playlist | `playlist_piste` | albums complets des pistes d'une playlist, ou la playlist seule |

**Diversité** : pas plus de 2 albums par artiste (réutiliser l'idée de
`plafonner_par_artiste`), et un mélange de popularités pour que la valise ne
soit pas faite que d'obscurités.

**Mémoire des valises** : une table `valise` (id, date, règles) et
`valise_album` (valise, album, verdict) côté Mac. On évite ainsi de reproposer
un album déjà emporté et refusé, et la rotation (étape 7) devient possible.

**Interface (bureau)** : dans l'écran Bibliothèque, un panneau « Préparer une
valise ». Il présente le budget, les règles et leurs parts, puis un aperçu en
grille de pochettes, avec la possibilité de retirer ou d'épingler un album
avant l'export. L'aperçu se met à jour à chaque changement. À construire selon
`interface-guidelines.md` (règle 10, jetons de gabarit).

## Étape 4 — Le transfert · **S** puis **M**

| Option | Effort | Pour | Contre |
|---|---|---|---|
| **A. Finder (câble ou Wi-Fi de synchro)** | S | aucun code réseau : `UIFileSharingEnabled` + `LSSupportsOpeningDocumentsInPlace`, la valise se glisse dans le dossier de l'app depuis le Finder | geste manuel ; le journal revient par le même chemin, à la main |
| **B. Wi-Fi local** | M | un bouton « Synchroniser » des deux côtés : le Mac sert la valise en HTTP, l'iPhone la découvre (Bonjour, crate `mdns-sd`), télécharge ce qui manque et renvoie son journal | autorisation « réseau local » d'iOS (`NSLocalNetworkUsageDescription`, `NSBonjourServices`) ; un serveur de plus dans l'app de bureau |
| C. Tailscale + Garage | M | marche hors de la maison | dépend de Tailscale actif sur l'iPhone ; Garage est pensé pour les données d'étude, pas pour ça |
| D. iCloud Drive | S | rien à écrire | occupe le quota iCloud ; synchronisation opaque |

**Recommandation : A pour la v1, B ensuite.** A permet de tester tout le
reste (format, lecture, retour d'écoutes) sans écrire de réseau. B est ce
qui rend l'usage agréable et doit transférer par **différence** : seuls les
albums ajoutés transitent, les retraits se font par le manifeste.

Côté iPhone, marquer le dossier audio « exclu de la sauvegarde »
(`isExcludedFromBackup`) : sinon chaque valise gonfle la sauvegarde iCloud.

## Étape 5 — L'app Écouter mobile · **L**

Interface à part de `apps/desktop/ui/app.js` (14 000 lignes, pensées pour le
rail et l'inspecteur). On en reprend les **jetons** (couleurs, typographie,
`--familles`) et la logique de jauge de popularité, pas la mise en page.

Écrans :
1. **Valise** : grille de pochettes, regroupée par règle d'origine
   (« Récents », « Peu écoutés », « Jazz modal… »), avec la date de la valise
   et la place occupée.
2. **Album** : pochette, pistes, durée, jauge, biographie de l'artiste,
   critique (avec attribution), crédits.
3. **Lecture** : plein écran, mini-lecteur en bas des autres écrans,
   position, normalisation « N » (valeurs de la base), « E » si le
   prototype le permet.
4. **File** : ordre, retrait. Pas de glisser-déposer en v1.

**Avis sur un album** (ce qui rend la valise utile au retour) : « garder »
(→ favori), « pas pour moi », « à réécouter ». Un album écouté en entier est
marqué automatiquement.

**Persistance** : la session (file, position) dans la base de la valise,
table `session`, comme le bureau. Reprise au relancement.

`interface-guidelines.md` ne couvre que le bureau ; y ajouter une section
mobile (cibles tactiles ≥ 44 pt, une colonne, zones sûres) avant de
construire.

## Étape 6 — Le retour des écoutes · **M**

L'iPhone écrit dans la table `ecoute` de sa valise (chemin **d'origine**,
horodatage) et ses avis dans une table `avis`. À la synchronisation :

- **Mac** : insère les écoutes dans sa table `ecoute`, applique les avis
  (`favori`, `valise_album.verdict`). Pour ne pas importer deux fois, chaque
  ligne du journal porte un numéro de séquence ; le Mac retient le dernier
  numéro importé par valise.
- **Même seuil d'écoute** que le bureau : une écoute compte au même moment
  dans la piste des deux côtés, pour que « peu écoutés » garde son sens.
- Les écoutes pourront ensuite alimenter ListenBrainz/Last.fm si le scrobble
  arrive un jour côté bureau : il suffira de les avoir horodatées
  correctement.

Avec l'option A (Finder), le journal est un `journal.json` que l'iPhone
dépose dans son dossier et que le Mac lit par `rusty-music valise importer`.

## Étape 7 — La rotation · **M**

« Renouveler la valise » sur le Mac :
1. importe le journal (étape 6) ;
2. retire les albums écoutés en entier, refusés ou gardés (les gardés
   rejoignent les favoris : ils sont « découverts ») ;
3. complète selon les mêmes règles jusqu'au budget ;
4. produit un **manifeste de différence** (ajouts, retraits) que le transfert
   applique sans tout recopier.

Option : une valise peut épingler des albums qui survivent à la rotation.

## Distribution

- **Usage personnel** : compilation par Xcode sur son propre iPhone. Avec un
  compte Apple gratuit, l'app expire au bout de 7 jours et il faut la
  réinstaller. Avec l'Apple Developer Program (99 $/an), le profil dure un an
  et TestFlight devient possible.
- **App Store / TestFlight public : écarté (décidé le 18 oct. 2026).** Pas de
  publication prévue. La question de compatibilité GPL-3 / conditions de
  l'App Store ne se pose donc pas.

## Ordre et charge

| # | Étape | Effort | Dépend de |
|---|---|---|---|
| 0 | Prototype iOS (lecture, arrière-plan, « En cours ») | M | — |
| 1 | Découpage `base` / `player` / `apps/mobile` | M | 0 |
| 2 | Format de valise + `valise exporter` | M | 1 |
| 4A | Transfert par le Finder | S | 2 |
| 5 | App Écouter mobile (v1 : Valise, Album, Lecture, File) | L | 1, 2 |
| 6 | Retour des écoutes (`valise importer`) | M | 5 |
| 3 | Sélection complète + panneau « Préparer une valise » | L | 2 (une règle simple suffit avant) |
| 7 | Rotation | M | 3, 6 |
| 4B | Synchronisation Wi-Fi | M | 6 |

Une **v1 utilisable** = 0, 1, 2, 4A, 5, 6, avec deux règles seulement
(« récents » et « peu écoutés ») réglées en ligne de commande. Les thèmes,
le panneau de préparation, la rotation et le Wi-Fi viennent ensuite.

## Points à trancher

1. Lecture par `cpal` (parité avec le bureau, « E ») ou par `AVPlayer`
   (batterie) — **décidé par les mesures de l'étape 0**.
2. Plugin Swift ou `objc2` pour la session audio et « En cours de lecture ».
3. Crate `base` extrait (recommandé) ou features dans `core`.
4. Opus 160 kb/s par défaut ou originaux par défaut.
5. Seuil d'une écoute, et ce qui fait qu'un album est « écouté en entier ».
6. ~~Distribution au-delà de l'usage personnel~~ — écartée : usage personnel
   seulement (compte gratuit à 7 jours ou Developer Program à 1 an).
