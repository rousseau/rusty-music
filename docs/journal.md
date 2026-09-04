# Journal de développement

Ce que `README.md` ne dit pas : les mesures, les pièges rencontrés et les
décisions prises au fil du chantier, dans l'ordre où elles sont arrivées.
`docs/suite.md` reste la référence pour ce qui **reste** à faire ; ce fichier
raconte ce qui a déjà été **fait**, et pourquoi.

Les exemples ci-dessous portent sur une bibliothèque de test réelle (27 044
morceaux) ; les morceaux et artistes cités en exemple sont anonymisés — seuls
les volumes, les mesures et les formats comptent ici.

## Application de bureau, pochettes et formats

### Application de bureau

```bash
cargo run -p rusty-music-desktop            # depuis le dépôt, mode Écoute
cd apps/desktop && cargo tauri build        # produit le .app et le .dmg
```

Le paquet fait **277 Mo** (208 en `.dmg`) : les poids voyagent avec lui, sans
quoi ni carte ni démixage.

**Où les modèles sont cherchés**, dans cet ordre — `crates/core/src/modeles.rs` :

1. la variable `RUSTY_MUSIC_MODELS`, pour désigner un autre dossier sans
   reconstruire ;
2. `Contents/Resources/models/` à côté de l'exécutable — disposition d'un
   paquet macOS ;
3. `models/` à côté de l'exécutable ;
4. `models/` dans le dossier courant — le cas du développement.

Un `.app` double-cliqué a pour dossier courant `/` : sans les étapes 2 et 3, il
ne trouvait rien. C'était le cas jusqu'à cette version.

**Les poids de CLAP échappent à cette liste**, et l'ordre y est inversé : le
chemin publié par le build (`RM_POIDS`) passe **avant** les ressources. Chaque
profil de compilation régénère code *et* poids ; charger ceux d'un autre profil
ne provoque aucune erreur, seulement des empreintes fausses. `RM_POIDS` désigne
toujours ceux qui vont avec le code exécuté, et n'existe que sur la machine de
build — une application installée tombe donc naturellement sur ses ressources.

Deux pièges payés sur cette étape, tous deux silencieux :

- **la sérialisation des poids par `burn-onnx` n'est pas déterministe.** Deux
  générations depuis le même ONNX donnent des fichiers différents, de taille
  identique. La copie d'empaquetage, qui comparait les tailles pour s'épargner
  du travail, ne se déclenchait donc jamais : le paquet embarquait des poids
  d'un build antérieur. Elle écrase désormais sans condition, et seulement
  depuis un build `release` — celui que produit `cargo tauri build` ;
- **aucune comparaison d'empreintes ne peut valider un paquet**, pour la même
  raison. Le seul contrôle qui vaille est fonctionnel :

  ```bash
  cargo run --release -p rusty-music-analysis --example empreinte_reference -- \
      "target/release/bundle/macos/Rusty Music.app/Contents/Resources/models/clap-audio-encoder-b5.bpk"
  ```

  À rejouer après chaque empaquetage.

La base est cherchée dans le dossier de données de l'application
(`~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db` sur macOS) et
non dans le répertoire courant : une application de bureau n'a pas de « cwd »
stable. Y copier une base déjà scannée évite de tout réingérer.

Coquille « Atelier » en direction 1a « Relief » (`docs/ui-spec-lecteur.md`) :

- **rail** — marque, modes, recherche, sommaire, accès aux réglages ;
- **centre** — parcours artistes → albums → pistes, listes virtualisées
  (3 543 lignes d'un coup figeraient la fenêtre) ;
- **inspecteur** — pochette et métadonnées du morceau courant ;
- **transport** — ▶/⏸, ‹ ›, barres de progression cliquables, minutage, volume ;
- **file d'attente** — panneau en superposition, clic pour sauter à une piste ;
- **réglages** — dossiers surveillés, ajout avec scan, retrait avec purge, et
  bouton **Analyser** : le nombre de morceaux en attente d'empreinte y est
  affiché avec une estimation de durée, et la passe se lance à la demande —
  jamais enchaînée au scan, une passe se comptant en heures ;
- **mode Explorer** — la carte : zoom, survol, clic pour écouter, filtres qui
  estompent, coloration par famille ou par année, voisins soniques dans
  l'inspecteur, **quatre façons de tracer un chemin** (voir ci-dessous) et
  **sélection au lasso** (`alt` + glisser) qui rend une zone en playlist ;

**Une contrainte de couleur, mesurée et non subjective.** Sur un nuage de
points, toutes les paires de teintes se côtoient : la palette validée y plafonne
à **trois** couleurs catégorielles. Colorer douze familles serait illisible par
construction — le contrôle refuse même deux teintes proches (ΔE 11,1 en vision
normale contre 15 requis, 6,6 en protanopie). D'où le mode « isoler une
famille », avec opacité et taille comme encodage secondaire. Une rampe
séquentielle, elle, n'oppose pas des identités : la coloration par année y
échappe.

**Lancer en `--release`.** Le décodage audio est entièrement processeur :
l'enveloppe d'une piste se calcule en 324 ms optimisée, contre 18 s sans
optimisation. Le `Cargo.toml` racine compile donc les dépendances en `opt-level
= 3` même en debug — notre propre code reste en debug, compilation rapide et
traces exploitables.

Points d'implémentation à connaître avant d'y toucher :

- **La file n'est pas chargée d'un bloc.** Ouvrir un fichier et lire son
  en-tête coûte ~100 ms sur la carte SD : préparer les 157 pistes d'un album
  immobilisait le lecteur 17 s, verrou tenu, et tout clic sur pause attendait
  d'autant. On n'en prépare que `PRECHARGE` (3), complétées par `completer()`
  à chaque sondage — d'où 1,9 s quelle que soit la taille de la file. Tout
  appelant du lecteur doit appeler `completer()` régulièrement, sinon la
  lecture s'arrête au bout de trois pistes.
- **L'onde est calculée, pas décorative.** Enveloppe crête et noyau RMS sur
  160 tranches, obtenues en décodant le fichier. Le calcul tourne dans un
  thread et la commande répond `null` en attendant : l'interface redemande.

- **Le scan lancé depuis les réglages ouvre sa propre connexion** dans un
  thread. Sous le verrou de la base partagée, il figerait l'interface pendant
  les dizaines de minutes que dure un scan. Le mode WAL autorise un rédacteur
  et des lecteurs simultanés : mesuré, une lecture répond en 56 ms pendant
  qu'un scan écrit.
- **`[hidden] { display: none !important }` en tête de feuille de style.** Sans
  ce garde-fou, une règle d'auteur comme `.voile { display: grid }` l'emporte
  sur le `[hidden]` du navigateur, et un panneau censé être masqué reste
  affiché — un voile plein écran avale alors tous les clics.
### Pochettes

`rusty_music_core::tags::read_cover()` cherche d'abord l'image embarquée, puis un
fichier à côté du morceau (`cover.jpg`, `folder.jpg`… — convention beets). Le
repli n'est pas décoratif : sur la bibliothèque de test, des albums pourvus
d'un `cover.jpg` n'ont aucune image embarquée (tag de 6 Ko).

Rien n'est stocké en base — les pochettes pèsent 4,9 Go et le scan les saute
volontairement. Le lecteur appelle cette fonction pour un morceau à la fois,
au moment de l'afficher : compter **50 à 210 ms à froid** sur la carte SD,
proportionnellement au poids de l'image. Un cache côté interface est à prévoir
si l'on affiche une grille d'albums.

### Formats lus

`rodio` décode via `symphonia`. Vérifié sur la bibliothèque réelle :

| Format | Fichiers | État |
|---|---:|---|
| mp3 | 26 655 | lu |
| m4a | 355 | lu (0 erreur sur 5 albums testés) |
| flac | 11 | lu |
| mp4 | 13 | lu, mais 2 erreurs `symphonia-codec-aac` par piste |
| opus | 10 | **non lu** — `symphonia` 0.5.5 n'a pas de décodeur Opus |

Les 10 fichiers Opus appartiennent à un seul album. Les couvrir demanderait
un décodeur hors `symphonia` (liaison C vers libopus) : disproportionné pour
0,04 % de la bibliothèque, à revoir si la proportion change.

## Premier build — fait

Le cœur compile, passe les tests et a été validé sur la bibliothèque réelle
(rustc 1.96, lofty 0.22.4, notify 8.2, rusqlite 0.32.1, macOS/Apple Silicon).
Aucune version du `Cargo.toml` racine n'a eu besoin d'être ajustée.

Contrairement à ce qui était anticipé, **les API de `lofty` étaient correctes**
telles qu'écrites : `lofty::probe::Probe`, `lofty::prelude::*` et les variantes
`ItemKey::AlbumArtist` / `ItemKey::MusicBrainzRecordingId` existent bien en
0.22. Le seul blocage était la **disposition des fichiers** : les sources
vivaient à plat dans `src/`, alors que le workspace déclare `crates/core` et
`crates/cli`. Elles ont été réparties, et `sql/` déplacé dans `crates/core/sql/`
pour que l'`include_str!` du schéma résolve.

Reste vrai : `rusqlite` en `bundled` compile SQLite depuis les sources — premier
build un peu long, nécessite un compilateur C (Xcode Command Line Tools sur macOS).

### Mesures sur la bibliothèque réelle (27 044 morceaux, carte SD exFAT)

| Opération | Résultat |
|---|---|
| Scan initial à froid, `-j 1` | 27 031 vus · 27 031 ingérés · **0 en échec** — 24 min 33 s (~18 fichiers/s) |
| Scan initial à froid, `-j 12` | 27 044 vus · 27 044 ingérés · 0 en échec — 25 min 38 s (~18 fichiers/s) |
| Rescan (rien de changé) | 27 044 vus · 27 044 inchangés · 0 retirés — **5,0 s** |

L'élagage des fichiers disparus coûte ~0,2 s sur ces 27 044 lignes.

### Pourquoi le parallélisme ne change rien ici

Le scan complet consomme **11 s de CPU pour 25 min de temps écoulé** (0,7 %) : le
processus passe son temps à attendre la carte SD. Il n'y a donc pas de calcul à
répartir — le décodage des tags coûte ~0,08 ms par fichier (360 fichiers en
0,031 s quand les données sont déjà en mémoire).

Le coût se décompose en ~4,9 Go d'octets de tags à lire (tag moyen 176 Ko,
médiane 96 Ko — c'est la pochette embarquée) à ~6 Mo/s, soit ~14 min, plus
~11 min de latence d'ouverture sur 27 044 fichiers. Superposer les lectures
devrait entamer cette seconde moitié ; mesuré, ça ne donne rien, à aucun nombre
de threads (1, 2, 4, 8, 12, 24). L'explication la plus probable est que le
pilote exFAT de macOS (FSKit, en espace utilisateur) sérialise les requêtes.

Le pool est conservé parce qu'il ne coûte rien et servira sur un support qui
tire parti de la profondeur de file (NVMe, montage réseau) ; `-j 1` le
court-circuite. **Le vrai levier serait de ne pas lire les octets des
pochettes** : `lofty` les lit puis les jette (`skip_frame` fait un `io::copy`
vers `io::sink()`). Les sauter par `seek` retirerait ~14 des 25 min, mais ce
n'est pas un patch trivial en amont — le lecteur de trames ID3v2 est générique
sur `Read` sans `Seek`, notamment parce que la désynchronisation ID3 rend le
flux d'octets pas toujours navigable directement.

Le scan initial est limité par le support, pas par le code : la carte SD lit à
~6 Mo/s, et `lofty` doit traverser tout le bloc de tags de chaque fichier. Le
débit suit d'ailleurs la taille des tags (81 Ko de tags → ~16 fichiers/s ;
768 Ko → ~6 fichiers/s). `ParseOptions::read_cover_art(false)` évite de décoder
les pochettes — que le cœur ne stocke pas — mais **n'accélère pas le scan** :
`lofty` lit puis jette les octets de l'image au lieu de les sauter.

Le chemin incrémental (taille + mtime identiques ⇒ pas de relecture des tags)
est en revanche très rapide : un rescan complet coûte quelques secondes.

### Pièges rencontrés, à connaître avant de toucher au cœur

- **`[profile.dev.package."*"]` ne couvre pas les membres du workspace.** Le
  `"*"` de Cargo désigne les *dépendances*. Nos propres crates restaient donc en
  `opt-level = 0`, où `distance2` n'est pas inliné : le graphe des 12 plus
  proches voisins demandait **524 s au lieu de 19 s** sur 27 031 morceaux — neuf
  minutes à saturer les douze cœurs, sans rien afficher, l'application passant
  pour plantée. Les trois crates de calcul (`analysis`, `player`, `editor`) sont
  désormais listées nommément dans le `Cargo.toml` racine. Mesure reproductible :
  `cargo run -p rusty-music-analysis --example cout_graphe`, qui affiche le coût
  et son extrapolation à la bibliothèque entière.
- **Une commande Tauri sans `(async)` s'exécute sur le fil principal**, donc
  fige toute l'interface dès qu'elle touche le disque. Mesuré au profileur :
  `play` ouvrait le fichier et laissait symphonia le sonder ; **la totalité des
  échantillons** montrait le fil principal bloqué dans un `read()`, pendant
  qu'une passe d'analyse saturait la carte SD. L'application paraissait tourner
  dans le vide alors qu'elle attendait le disque. Sur une fonction *non*
  asynchrone, `#[tauri::command(async)]` ne change ni les arguments, ni les
  verrous, ni les erreurs : il la déplace sur un pool de fils. **Les 41
  commandes le portent**, parce que la frontière n'est pas nette — `skip` ouvre
  la piste suivante, et même un lecteur d'état prend un verrou que ces
  opérations détiennent.
- **Une passe de fond peut étrangler l'application** sans qu'aucune des deux
  soit en faute : douze fils de décodage saturent un support qui facture
  l'accès. D'où `--fils` sur `descripteurs`, à baisser pour continuer d'écouter
  pendant une passe.
- **Un sondage sans garde-fou d'appel en vol devient un déni de service** dès
  que les commandes s'exécutent en parallèle. Le sondage des stems tournait à
  5 Hz sans cette garde : les appels s'empilaient sur le même verrou et un clic
  attendait derrière eux. Il fallait presser le bouton trois fois pour arrêter
  la lecture. Deux correctifs : la garde, et surtout **ne plus demander au
  moteur son état avant d'agir** — le bouton sait ce qu'il affiche, donc ce que
  l'utilisateur veut.
- **La surveillance ne peut pas se fier au type d'évènement.** FSEvents (macOS)
  livre en un seul lot l'historique cumulé d'un chemin : un fichier supprimé
  arrive en `Create` + `Remove` + `Modify`, sans ordre exploitable. `flush()`
  tranche donc sur l'état du disque (le chemin existe ⇒ ajout, il a disparu ⇒
  retrait), ce qui couvre du même coup le renommage. Test de non-régression :
  `watch::tests::flush_retire_les_chemins_disparus`.
- **`mp4` est dans `AUDIO_EXTS`** : la bibliothèque de test contient un album
  entier étiqueté `.mp4` plutôt que `.m4a`. Contrepartie à garder en tête, une
  vidéo posée dans le dossier surveillé serait ingérée.
- **L'élagage se fie au disque, pas au parcours.** `scan` retire les morceaux
  dont le fichier a disparu, mais uniquement sur un `NotFound` avéré : un
  dossier devenu illisible remonte une autre erreur et la ligne est conservée.
  Une racine absente (carte débranchée) échoue avant tout élagage. L'élagage ne
  touche que les chemins situés sous la racine scannée — scanner un seul album
  ne vide pas le reste de la base.
- La détection de changement repose sur `mtime` à la seconde : une réécriture
  de même taille dans la même seconde que le dernier scan passerait inaperçue.
- `cargo fmt` n'a pas été passé : le squelette est formaté à la main de façon
  compacte et le lancer produirait un diff sans rapport avec le code écrit ici.

## Analyse (module 2)

Le modèle n'est pas dans le dépôt — 112 Mo. Le récupérer et le préparer une
fois :

```bash
mkdir -p models && curl -L -o models/clap-audio-encoder.onnx \
  https://huggingface.co/icybawss/clap-htsat-unfused-audio-encoder-onnx/resolve/main/audio_model.onnx
./scripts/preparer-modele.sh          # fige les formes ; sans lui, le build échoue
```

Encodeur audio de CLAP (`laion/clap-htsat-unfused`), conversion ONNX Apache-2.0,
poids non modifiés. Entrée log-mel `[5, 1, 1001, 64]` — cinq fenêtres de 10 s à
48 kHz —, sortie de 512 dimensions.

**Le modèle tourne sous Burn, pas sous ONNX Runtime.** `burn-onnx` le traduit
en Rust natif au moment du build ; le backend `wgpu` couvre Metal, Vulkan et
DX12 d'un même code. La bascule a été validée par comparaison directe :

| | ONNX Runtime | Burn `ndarray` (CPU) | Burn `wgpu` (Metal) |
|---|---|---|---|
| Lot de 5 fenêtres | 121 ms | 192 ms* | **97 ms** |
| Par fenêtre | 24,2 ms | 38,4 ms* | **19,4 ms** |
| Cosinus contre ORT | — | 1,0000000000 | 1,0000000000 |

<sub>* mesuré sur un lot d'une fenêtre, rapporté ici pour l'ordre de grandeur.</sub>

Le gain de calcul est modeste — 20 % — et invisible sur la passe, limitée par
le support. Ce qui compte : le GPU **libère les douze cœurs** pour le décodage,
et le module 3 (démixage) aura besoin de cette voie. `experiments/burn-clap/`
garde la trace complète de l'essai, y compris ce qui n'a pas marché.

**Trois pièges de cette migration, tous silencieux.**

- **`default-features = false` sur `burn` désactive `std`**, et Burn bascule
  sur ses implémentations mathématiques de substitution. Le modèle compile,
  tourne, et rend des empreintes entièrement fausses. Aucun avertissement.
- **Les poids ne se partagent pas entre profils de compilation.** Chaque build
  régénère code *et* poids ; charger ceux d'un autre build ne produit pas
  d'erreur, seulement des nombres faux. D'où le chemin publié par `build.rs`
  (`RM_POIDS`) plutôt qu'un emplacement commun.
- **Le modèle publié ne s'importe pas** : ses blocs Swin calculent leurs marges
  à l'exécution. D'où `scripts/preparer-modele.sh`, qui fige les formes — et
  surtout pas `onnx-simplifier`, qui produit ici un graphe invalide.

Ces trois-là ont été attrapés par un seul garde-fou, pas par le compilateur :

```bash
cargo run --release -p rusty-music-analysis --example empreinte_reference
```

Il compare l'empreinte d'une entrée calculée aux valeurs relevées sous ONNX
Runtime avant son retrait. À rejouer après tout changement de backend, de
version de Burn ou de préparation du modèle.

```bash
rusty-music analyze --limit 27044   # empreintes ; reprenable, écrit au fil de l'eau
rusty-music project                 # place tout sur la carte ; 6 s, rejouable
rusty-music analyze --limit 500 --project   # ou par lots
```

**La passe est en deux commandes, et ce n'est pas un choix de confort.** Les
empreintes se calculent morceau par morceau ; la projection, elle, exige le lot
entier — t-SNE place chaque point relativement aux autres, deux passes séparées
donneraient deux repères sans rapport. D'où `analyze`, incrémental et
reprenable, puis `project`, global.

### Étirement et transposition

```bash
rusty-music etirer entree.wav sortie.wav --facteur 1.25      # 25 % plus long
rusty-music etirer entree.wav sortie.wav --demi-tons 3       # +3 demi-tons, durée inchangée
```

Dans l'application, deux pas-à-pas dans l'en-tête du dock des stems : **vitesse**
(immédiate, la lecture change de tempo sans rien recalculer) et **hauteur**
(quelques secondes, mise en cache par réglage).

**`wsola`, et pas un vocodeur écrit à la main — c'est une correction.** J'avais
écrit un vocodeur de phase complet, avec verrouillage sur les crêtes du spectre,
avant de vérifier ce qui existait déjà en Rust. `wsola` fait mieux :

| | vocodeur écrit ici | `wsola` |
|---|---|---|
| méthode | phase, fréquentielle | **recouvrement-addition temporel** — celle d'`atempo` (ffmpeg) et de VLC |
| artefacts | flou de phase sur les transitoires, non mesuré | pas de phase à recoller |
| temps réel | à écrire | **conçu pour** — `push`/`pull`, `set_tempo` en direct |
| taille | ~500 lignes à maintenir | 468 lignes, zéro dépendance transitive |

Mesuré avant adoption : **hauteur inchangée à 441 Hz de 0,5× à 2×**, durées
justes, et le mode flux vérifié bloc par bloc.

**Ce que le changement coûte, et il faut le dire** : en traitement de bloc,
`wsola` met 17,9 s là où le vocodeur de phase mettait 0,84 s pour un stem de
184 s. Sans effet sur la lecture, où le coût est amorti au fil du son ; sensible
sur la transposition de l'éditeur, qui traite quatre stems d'un coup. Elle est
mise en cache par réglage, donc payée une fois.

Ce qui reste écrit ici : la **transposition**, que `wsola` ne fait pas. Elle
s'obtient en étirant puis en rééchantillonnant du même rapport — interpolation
cubique de Catmull-Rom, la linéaire ternissant tout le haut du spectre.

**La leçon est dans `CLAUDE.md`** : chercher une crate Rust *avant* d'écrire,
pas après.

### Ce que l'adoption de `wsola` a cassé ailleurs — corrigé le 19 août

Deux défauts, tous deux introduits par un changement juste, et tous deux dans
du code que ce changement ne touchait pas.

#### La vitesse craquait

`Voix::remplir()` pousse un bloc dans l'étireur **depuis le rappel audio**. Le
bloc valait 4 096 trames : un appel à `next()` sur quelques milliers faisait
tout le travail, les autres rien.

**Le débit moyen allait très bien** — WSOLA tient 3,2 fois le temps réel sur
quatre stems — et c'est pour cela que le défaut a survécu. Ce qui compte n'est
pas la moyenne mais le pic : un appel doit rendre la main avant que le
périphérique ait vidé son tampon, 11,6 ms pour 512 trames.

Pire salve, quatre stems (`cargo run --release -p rusty-music-player --example
cout_bloc`) :

| bloc | ×0,25 | ×0,5 | ×1,5 | ×4 |
|---|---|---|---|---|
| 4096 | 122,5 | 65,5 | 23,9 | 10,4 |
| 1024 | 33,9 | 20,6 | 9,6 | 5,3 |
| 256 | 10,0 | 5,2 | 5,1 | 5,1 |
| **128** | **6,1** | **5,8** | **5,1** | **5,1** |

**Le pire cas est la vitesse la plus lente, et c'est ce qui a failli être
manqué.** Mesuré au seul tempo 1,5 — le réflexe — un bloc de 512 semblait
suffire ; il craque à 19,6 ms dès qu'on ralentit, c'est-à-dire précisément
quand on ralentit pour écouter un détail. À ×0,25 un bloc rend quatre fois sa
durée, donc quatre fois plus de pas d'étireur d'un coup.

**Le plancher n'est pas le bloc, c'est le pas de l'étireur** : `wsola` rend sa
sortie par sauts de 15 ms et n'en rend jamais moins — d'où les 5,1 ms qui ne
descendent plus. 128 est le plus grand bloc qui reste à ce plancher sur toute
la plage de l'interface. Un test d'arithmétique le verrouille, plutôt qu'un
chronomètre qui serait capricieux :
`un_bloc_ne_couvre_jamais_plus_dun_pas_detireur`.

Reste à savoir : 6,1 ms sur 11,6, c'est de la marge, pas du confort. Un
périphérique demandant des tampons de 256 trames redeviendrait juste, et la
réponse de fond serait alors de sortir l'étirement du rappel audio.

#### La hauteur bloquait l'interface

La transposition tournait dans une commande `#[tauri::command(async)]`, et cela
ne suffit pas. **`async` sert à une commande qui attend, pas à une commande qui
calcule** : une boucle qui ne rend jamais la main monopolise un ouvrier du
runtime, et toutes les autres commandes attendent derrière — le sondage du
transport, l'état de lecture, le moindre clic. L'interface ne gelait pas, elle
faisait la queue, ce qui se voit pareil.

**L'enchaînement mérite d'être noté.** Le démixage avait déjà réglé cela dans
son fil. La transposition ne l'avait pas suivi parce qu'elle était courte —
0,84 s par stem du temps du vocodeur de phase. `wsola` l'a portée à 17,9 s,
soit plus d'une minute pour quatre stems, **sans que ce chemin-là soit revu**.
Un remplacement correct, mesuré et documenté a rendu inacceptable un choix qui
ne l'était pas la veille.

Corrigé sur le motif du démixage : `start_etirer` lance un fil et rend la main,
`etirer_state` se sonde. L'avancement se compte **en stems**, pas en
pourcentage — à vingt secondes pièce, un pourcentage global reste immobile
assez longtemps pour qu'on le croie bloqué. Quand tout est neutre ou déjà en
cache, aucun fil n'est lancé et le chargement reste immédiat.

#### Et la transposition restait lente — 92 s

Sortir le calcul du chemin de l'interface ne l'accélère pas. Mesuré phase par
phase sur un stem de 272 s (`cargo run --release -p rusty-music-editor
--example cout_transposition`) :

| phase | coût |
|---|---|
| décodage | 0,18 s |
| **étirement + rééchantillonnage** | **22,56 s** |
| écriture | 0,03 s |

Tout est dans l'étirement, et rien d'autre ne compte. Quatre stems en file :
92 s.

**« Ne faudrait-il pas transposer le morceau plutôt que chaque stem ? »**
L'intuition vise juste — quatre fichiers pour un seul geste, c'est quatre fois
le travail — mais le remède coûterait le module. Un morceau transposé est **un
fichier** : plus de solo, plus de coupure, plus de niveau par stem, et plus de
transposition par stem, alors que baisser la basse d'une quinte sans toucher au
reste est précisément un des gestes que la spec retient. On rendrait la
transposition rapide en supprimant ce sur quoi elle s'applique.

**Le même facteur quatre s'obtient sans rien céder** : un fil par stem. Les
quatre transpositions ne partagent rien, la mise à l'échelle est donc presque
parfaite — **92,0 s → 22,7 s, soit 4,0×**.

Ce qui rendait cette parallélisation risquée, et qui a dû être réglé d'abord :
`transposer` tenait **cinq tampons pleins** du signal — l'entrée, l'étiré, les
deux canaux séparés, les deux rééchantillonnés, le réentrelacé. Quatre à la
fois faisaient pagineur la machine, et **une machine qui pagine fige
l'interface aussi sûrement qu'un calcul mal placé**. `reechantillonner_entrelace`
lit au pas des canaux et écrit d'affilée : cinq tampons deviennent deux, la
pointe mesurée passe de 1,68 à 1,47 Go, et le cache s'en trouve mieux traité.

**Ce qui reste possible et n'est pas fait** : ne transposer que ce qui
s'entend. Un stem coupé, ou tous sauf le stem en solo, n'ont pas à être
calculés. C'est un quart à trois quarts de travail en moins selon le cas, sans
rien céder non plus.

### Régler au clavier plutôt qu'au pas-à-pas

Les valeurs de vitesse et de hauteur se tapent. **Les pas-à-pas restent**, et ce
n'est pas une hésitation : ils servent à chercher la bonne valeur en écoutant,
le champ sert à sauter d'un coup à une valeur qu'on a déjà en tête — ce qu'une
quinzaine de clics ne fait pas.

Le champ lit toujours des **pour cent**, jamais un rapport : « 2 » est ambigu
— deux pour cent ou deux fois ? Puisqu'il affiche « % », il lit des pour cent,
et 2 est refusé par la borne basse plutôt qu'interprété. Une saisie hors bornes
se marque et **garde ce qui a été tapé** : corriger suppose de voir son erreur.
Les flèches haut et bas font un pas, Échap rend la valeur courante.

### Tempo et tonalité du morceau

Mesurés par la passe « Descripteurs », affichés à deux endroits qui ne suivent
pas la même chose : l'**inspecteur** suit la sélection, le **transport** suit ce
qu'on écoute — les deux divergent dès qu'on explore la carte sans changer de
morceau.

Notation française : « Fa mineur », pas « F min ». La base note à l'anglaise
parce que c'est ce qu'écrivent les profils de Krumhansl-Schmuckler et tout le
domaine ; la traduction appartient à l'affichage. **Les altérations restent des
dièses** — la mesure ne distingue pas un fa dièse d'un sol bémol, et choisir
l'un des deux prétendrait le contraire.

**Un tiret quand ce n'est pas mesuré, jamais une valeur par défaut** : la passe
couvre 15 847 morceaux sur 27 044, et afficher « 120 BPM » sur le reste
donnerait une mesure qu'on n'a pas. L'ambiguïté d'octave vaut ici aussi — un
morceau à 174 BPM peut s'afficher à 87.

### Opus — le seul format que symphonia ne décode pas

Un album entier de la bibliothèque de test restait hors de la carte.
`crates/core/src/opus.rs` le décode avec deux crates choisies pour ce qu'elles
**n'exigent pas** : `ogg` (BSD-3) démultiplexe le conteneur, `opus-decoder`
(MIT/Apache) est un portage **pur Rust** de libopus — ni `unsafe`, ni FFI, ni
bibliothèque système, ni `cmake`. Le crate `opus` officiel compile libopus
depuis ses sources et aurait imposé `cmake` à toute personne construisant le
projet ; c'est cela qui l'a écarté, pas sa licence.

Le module vit dans le cœur parce que les trois modules en ont besoin et qu'ils
ne partagent que lui. Il honore le `pre-skip` de l'en-tête — les premiers
échantillons amorcent le décodeur et ne font pas partie du morceau — et le gain
de sortie.

Trois usages, un seul aiguillage (`opus_en_memoire`, dans le lecteur) : la
carte les analyse, le lecteur les joue, la barre du bas en trace l'onde. Le
morceau entier tient alors en mémoire — une soixantaine de mégaoctets pour
quatre minutes — parce que la chaîne en flux ne sait pas ouvrir ce format ; ça
ne concerne que ces fichiers.

**27 042 des 27 044 morceaux** sont sur la carte. Les deux restants, un m4a et
un mp3, sont corrompus.

### Tempo, tonalité, énergie

```bash
rusty-music descripteurs        # ~1,6 s/morceau, reprenable
```

Remplit la table `descriptors` et donne au rail deux façons de plus de colorer
la carte : **Tempo** et **Énergie**, à côté d'Année. Aucune dépendance ajoutée.

**Les algorithmes viennent des bibliothèques du domaine, pas de nulle part** :
flux spectral puis autocorrélation à peigne, comme `onset/specflux` et
`beattracking` d'aubio ; chroma corrélé aux profils de Krumhansl-Schmuckler,
comme QM-DSP, celui de Mixxx. Les écrire — 416 lignes — plutôt que les lier
évite une dépendance C et un passage sous GPL, et le seul usage qui exigerait
mieux, le calage de deux disques, est hors du périmètre du module 3.

Deux points appris en chemin :

- **deux fenêtres, pas une.** Une attaque se situe dans le temps, une note en
  fréquence, et la transformée échange l'une contre l'autre. Avec la seule
  fenêtre de 2 048 points, un do₃ et le la♯ voisin tombaient dans la même raie
  et le chroma d'un accord parfait s'étalait sur huit classes. Le chroma prend
  donc 8 192 points, les attaques 2 048 ;
- **on teste des tempos, pas des décalages entiers.** À 93,75 trames par
  seconde, un morceau à 150 BPM a une période de 37,5 trames qu'aucun décalage
  entier n'atteint : le double, lui, tombe juste, et le morceau ressortait à
  75 BPM. L'autocorrélation est donc évaluée à décalage fractionnaire, sur une
  grille de 240 tempos.

| mesure | valeur |
|---|---|
| accord avec AudioMuse-AI à 6 % près | 73 % de 197 morceaux |
| accord à l'octave près | 80 % |
| coût | 1,6 s/morceau, dont 1,1 s de décodage |

**Limite connue** : les seuils de rejet n'écartent que le silence. Sur les
premiers morceaux mesurés, aucun n'est sorti sans tempo ni sans tonalité — un
conte lu reçoit donc un tempo qui ne veut rien dire.

### La grille de battements

```bash
rusty-music battements piste.wav                          # tempo, phase, netteté
rusty-music battements piste.wav --bpm 117.2 --phase 0.128   # éprouver une grille imposée
```

`descripteurs.rs` rend un tempo, c'est-à-dire une **période**. Deux morceaux à
124 BPM peuvent pulser en opposition de phase ; les caler demande de savoir *où*
tombe le premier temps. C'est ce que `crates/analysis/src/battements.rs`
ajoute, et ça sert **deux fois** : le mixage de deux pistes l'exigeait, mais la
greffe du module 3 aussi — elle calait les tempos sans caler les temps forts.

**La méthode.** Le tempo vient de l'autocorrélation à peigne déjà écrite ; la
phase, d'un peigne de Dirac glissé sur l'enveloppe d'attaques, comme
`beattracking` d'aubio après son autocorrélation.

**Deux corrections, et aucune n'était devinable :**

| | ce qu'on a trouvé | ce qu'on fait |
|---|---|---|
| **latence du détecteur** | le flux spectral place une attaque `N_FFT − HOP` échantillons trop tôt — elle n'apparaît qu'en entrant dans la part de fenêtre que la précédente ne couvrait pas | une constante de 32 ms, **dérivée puis mesurée** : −31 ms observés |
| **pas de la grille de tempo** | 240 candidats entre 60 et 200 BPM, géométriquement : **0,5 % par pas**. Sans conséquence pour colorer une carte, rédhibitoire pour une phase — 40 ms de dérive en 16 s, une demi-seconde sur un morceau | affinage **conjoint** de la période et de la phase dans une bande de ±1 % |

L'affinage conjoint est le point : on cherche le couple qui explique le mieux
les attaques, au lieu de prendre la période d'un critère et la phase d'un
autre. L'erreur passe de −31…−67 ms à **8,5 ms au pire**, contre un plancher de
méthode de 7,8 ms — un pas de balayage (`verif_battements`).

**La réserve qui compte, et elle n'était pas prévue : sur une batterie, la
phase est presque indéterminée.**

```
── 01 titre-test — drums.wav
 1.   117.2 BPM   phase 0.128 s   netteté 2.96
 2.   116.7 BPM   phase 0.426 s   netteté 2.93  (+42 % de battement)
```

Deux décalages presque à un demi-battement l'un de l'autre, et 0,03 de netteté
pour les départager : la caisse claire du 2 et du 4 pèse autant que la grosse
caisse du 1 et du 3. **Conséquence sur la manière de vérifier** — remesurer la
grille d'une greffe calée ne prouverait rien, on comparerait deux tirages
ambigus. On pose donc la grille de référence et l'on regarde ce qu'elle
ramasse :

| | grille du stem remplacé | meilleure trouvée à l'aveugle |
|---|---|---|
| greffe **calée** | **2,19** | 1,94 |
| greffe **non calée** | **1,08** | 2,15 |

La greffe non calée pulse — 2,15 pour sa propre grille — mais pas là où
l'original pulsait : 1,08, quand une phase tirée au hasard vaut 1,00.

**Ce qui reste ouvert** : le tempo est supposé constant, ce qui ne décrit ni un
live ni un batteur qui accélère ; et l'ambiguïté d'octave de `descripteurs.rs`
demeure — un morceau à 174 BPM est lu à 87, grille juste mais un battement sur
deux.

### Nommer les familles — trois sources

```bash
rusty-music enrich --contact "toi@exemple.org"   # genres MusicBrainz ; ~2 h, reprenable
rusty-music familles --artistes                  # ce que ça donne, artistes à l'appui
```

Les douze familles de la carte tirent leur nom de trois sources, **de la plus
précise à la plus grossière** :

| source | ce qu'elle apporte | couverture mesurée |
|---|---|---|
| album MusicBrainz | distingue deux disques d'un même artiste | variable |
| artiste MusicBrainz | vocabulaire curé — `afrobeat`, `nu metal`, `boom bap` | 64 % des artistes, 74 % des morceaux |
| tag du fichier | dernier recours, mais irremplaçable | 90 % des morceaux |

**Pourquoi trois et pas une.** Les tags des fichiers seuls nommaient
« Children's · Pop » la famille de plusieurs artistes de folk/pop acoustique à voix féminine —
121 fichiers y portent cette étiquette, rare ailleurs, donc gagnante au score.
MusicBrainz seul laisserait sans nom la famille de chant breton, dont aucun
artiste n'y figure avec un genre. Le sondage qui a tranché est dans
`experiments/musicbrainz-genres/` : neuf familles sur douze y gagnent.

Trois précautions, chacune tirée d'un défaut observé :

- **on ne mélange pas les sources pour un même morceau.** Les verser ensemble
  ferait cohabiter « Rock » et « rock », et le genre le plus grossier
  redeviendrait le plus lourd ;
- **un seul genre par entité, le mieux voté** — pas trois. Le premier est celui
  sur lequel les contributeurs s'accordent, les suivants décrivent les marges.
  En retenir trois versait « rock » et « pop » partout : mesuré sur la
  bibliothèque entière, un genre donne « Reggae · Afrobeat » là où trois
  donnaient « Reggae · Ska » ;
- **aucun plancher de votes, et c'est une correction.** Il avait d'abord été mis
  à deux pour écarter l'`amapiano` posé par un contributeur unique sur un
  artiste par ailleurs bien couvert. Mesuré, il faisait l'inverse : chez cet artiste `modern classical`,
  `neoclassicism`, `minimalism` et `instrumental` portent eux aussi une seule
  voix, et le seuil ne gardait que `rock`. Coût général : **55 % de couverture
  au lieu de 74 %**, et 360 artistes rendus muets. Ce qui règle vraiment le cas
  est le **départage à votes égaux par le nombre d'artistes qui portent le
  genre** — `amapiano` n'en a qu'un dans toute la bibliothèque, `instrumental`
  dix-sept ;
- **le titre d'album est normalisé** avant rapprochement — nos fichiers ne
  portent pas d'identifiant d'album, et « Nom de l'album (édition deluxe) »
  doit retrouver « Nom de l'album ».

`enrich` demande un contact parce que MusicBrainz l'exige dans l'agent, et
tient une requête par seconde parce que c'est leur limite. La passe reprend où
elle s'est arrêtée : chaque artiste est marqué dans la même transaction que ses
données.

### Ce qu'on calcule, et ce que calcule AudioMuse-AI

La comparaison vaut d'être posée : sur la même carte SD, AudioMuse-AI met
plusieurs jours là où cette passe met quelques heures. Ce n'est pas que la
représentation soit plus pauvre — **c'est le même vecteur, de la même taille**.

| | Rusty Music | AudioMuse-AI (≥ v0.6.0-beta) |
|---|---|---|
| Empreinte | CLAP `htsat-unfused`, **512 d** | CLAP « DCLAP », **512 d** |
| Frontal | 64 mels, n_fft 1024, hop 480 | 128 mels, n_fft 2048, hop 480 |
| Audio couvert | 3 fenêtres de 10 s | morceau entier |
| Autres modèles | — | MusiCNN 200 d sur **toutes** les fenêtres ; sa tête de prédiction (48 humeurs + 6 attributs) ; tempo/énergie/tonalité (librosa) ; **Whisper-small** jusqu'à 240 s |
| Langage | Rust | Python |

Ils font quatre à cinq analyses par morceau là où on en fait une, et leur FAQ
désigne la transcription (ASR) comme le poste le plus long. Le seul endroit où
nous sommes réellement plus maigres est la **couverture**. Et les 10 s ne sont
pas un raccourci qu'on s'est autorisé : l'encodeur HTSAT en variante `unfused`
prend une entrée de dix secondes, point — couvrir plus demande plusieurs passes.

### Les deux écarts chiffrés — 23 août 2026

Deux pistes d'amélioration identifiées en comparant à AudioMuse-AI : plus de
mels (128 contre 64) et plus de couverture (le morceau entier contre 50 s).
Chiffrées avant d'y toucher.

**128 mels — ce n'est pas un réglage, c'est un autre modèle.**
`crates/analysis/src/mel.rs` le dit dès son en-tête : 64 bandes, n_fft 1024,
hop 480, échelle HTK, non normalisé — « ne reproduisent pas au hasard le
prétraitement de `ClapFeatureExtractor` pour `laion/clap-htsat-unfused` », ce
sont **les valeurs que ce checkpoint exige**. Les poids embarqués ont été
entraînés sur des spectrogrammes de cette forme précise ; leur donner 128
bandes ne les affinerait pas, ça les rendrait aveugles — un modèle dont les
poids attendent 64 canaux ne sait rien faire d'un 129ᵉ. Le « 128 mels,
n_fft 2048 » d'AudioMuse-AI décrit **un autre point de contrôle** (« DCLAP »,
distinct de `htsat-unfused`) : changer de résolution reviendrait à sourcer et
importer un modèle entièrement différent — un chantier de l'ampleur de
`experiments/burn-clap/` lui-même, pas un paramètre à tourner.

**Couvrir le morceau entier — chiffré, et le résultat est net : aucun
bénéfice mesuré.** `couverture.rs` comparait déjà 1/3/5/9 fenêtres et
plafonnait à 9 (0,51×). Poussé à 15 et 25 fenêtres (jusqu'à 250 s — la
quasi-totalité de la plupart des morceaux) sur 318 morceaux réels, 8 albums :

| fenêtres | couverture | même album | même artiste | hasard |
|---|---|---|---|---|
| 1 | 10 s | 0,58× | 0,58× | 0,400 |
| 3 | 30 s | 0,52× | 0,52× | 0,359 |
| 5 | 50 s | 0,52× | 0,52× | 0,355 |
| 9 | 90 s | 0,52× | 0,53× | 0,353 |
| 15 | 150 s | 0,53× | 0,53× | 0,353 |
| 25 | 250 s | 0,53× | 0,53× | 0,353 |

**Le plafond est atteint à 50 s et ne bouge plus jusqu'à 250 s** — cinq fois
plus de matière sans le moindre gain mesurable. Le coût, lui, est réel et
linéaire : 806 s pour 318 morceaux à 25 fenêtres, soit 2,53 s/morceau contre
0,36 s à 5 fenêtres sur SSD (`Coûts mesurés`, plus bas) — environ 5-7× plus
cher pour un rapport au hasard identique au dix-millième près.

**Conclusion des deux chiffrages : ni l'un ni l'autre n'est rentable en
l'état.** Les 512 dimensions de l'empreinte elle-même ne sont pas en cause —
sondage à l'oreille fait le même jour (`rusty-music voisins`) : Nirvana
rapproche Nirvana/Tool/Soundgarden/Alice in Chains, Get Lucky rapproche
N*E*R*D et Michael Jackson *Off the Wall*. Le 8 % de recouvrement
musical/géographique mesuré sur la carte (`carto-ville.md`, objection V1)
n'accuse donc pas l'empreinte : il accuse la grossièreté du placement
inter-artiste, déjà objectivé indépendamment (le passage à 12 familles par
genre n'a rien changé à ce chiffre non plus — voir `carto-ville.md`).

### Coûts mesurés

Par morceau, chaîne complète, **avant** le positionnement :

| Support | Décodage | Log-mel | Empreinte | Total |
|---|---|---|---|---|
| Carte SD | 2 869 ms | 14 ms | 30 ms | **2 914 ms** |
| SSD interne | 316 ms | 14 ms | 29 ms | **359 ms** |

Le décodage est tout : 98 % du temps sur la carte, encore 87 % depuis le SSD.
Ramenée aux 27 044 morceaux sur 12 fils, la partie **calcul** ne pèse que
~13 min — la passe observée à 1,08 s/morceau, soit 8,1 h, était donc à **97 %
de l'attente sur la carte** (212,5 Go à ~7,3 Mo/s en accès concurrent).

**On croyait tenir la cause : on décodait 100 % du fichier pour en garder
20 %.** Corrigé — `decode::fenetres` ne décode plus que les fenêtres retenues,
et la chaîne y gagne 7,3 × (`cout_decodage`, mp3/m4a/mp4/flac, cosinus
0,997–1,000 : ce sont les mêmes fenêtres, donc la même empreinte).

**Mais la passe n'a pas accéléré d'une minute.** Trois stratégies mesurées sur
la vraie bibliothèque, 12 travailleurs, carte SD :

| Stratégie | s/morceau | Octets lus |
|---|---|---|
| Tout décoder, en flux | 1,08 | 100 % |
| **Lire d'un bloc, positionner en mémoire** | **1,13** | 100 % |
| Positionner dans le fichier | 1,42 | ~20 % |

Celle qui lit cinq fois moins est la plus lente. Le support facture l'accès,
pas l'octet : sous douze travailleurs concurrents, l'accès dispersé est ce
qu'il sert le plus mal. Et le nombre de travailleurs n'y change rien non plus —
`-j 4` donne 1,11 s/morceau, `-j 12` donne 1,13.

**Le plafond est ailleurs, et il est net.** `dd` sur la carte au repos, un seul
lecteur : **7,4 Mo/s**. Les 212,5 Go de la bibliothèque mettent donc ~8 h à
traverser, quelle que soit la stratégie de décodage. La passe observée à 8,3 h
tourne exactement à la vitesse du support. Décomposé fichier par fichier, à
froid : **lecture des octets 1,1–4,7 s, décodage 53–507 ms.**

La leçon vaut d'être retenue : le gain de 7,3 × est réel, il est simplement
invisible derrière un support qui coûte trente fois plus cher que le calcul
qu'il alimente. Il paiera le jour où la bibliothèque vivra sur un stockage
interne — là, la passe entière tient en quelques minutes de processeur.

À l'échelle de la bibliothèque, ce qui vient après ne coûte rien : **6 s** pour
la projection t-SNE, 0,6 s pour les familles, **5 ms** pour un plus proche
voisin en force brute — d'où l'absence d'index approché, qui serait de la
complexité gratuite.

### Nappe de densité de la carte — `crates/core::density`

Estimation à noyau gaussien par famille (grille régulière), isobandes
extraites avec `contour` (mthh/contour-rs). Mesuré avec
`cargo run --release -p rusty-music-core --example bench_density` (bibliothèque
synthétique, 27 000 points) :

| Résolution | Temps (première version, vraie convolution gaussienne) | Temps (flou en boîte × 3) |
|---|---|---|
| 512×512 | 240 ms | **150 ms** |
| 1024×1024 | 1,62 s | **550 ms** |

**Le noyau demandé — 1 à 2 % de l'étendue des données — grandit en cellules
avec la résolution**, puisqu'il reste fixe en unités de carte : une
convolution gaussienne directe coûte `O(rayon)` par cellule, et le rayon suit
la résolution. À 1024, cela suffisait à faire basculer le calcul dans la
seconde et demie. Remplacé par trois passes de flou en boîte à somme
glissante (`O(1)` par cellule, quel que soit le rayon) — l'approximation d'un
noyau gaussien par trois flous en boîte est une technique connue du
traitement d'image (Getreuer 2013), à moins de 3 % d'erreur, largement sous
ce qu'un œil distingue sur une carte de densité. Gain net : ×3 environ des
deux côtés, et 1024×1024 devient une option raisonnable plutôt qu'un pari.

Le second coût, lui, ne bouge pas avec le noyau : le carré marchant
(`contour::isobands`) balaie la grille une fois par seuil, indépendamment de
la largeur du flou — c'est lui qui domine à haute résolution une fois le flou
rendu bon marché.

**Revu après un premier essai visuel** (voir la revue du pavage par
territoires, plus bas dans ce chantier) : à 512×512 et 1,5 % de l'étendue, les
territoires restaient nettement plus arrondis qu'attendu — pas un défaut du
calcul, mais un noyau au milieu de la fourchette demandée (1 à 2 %) plutôt
qu'à son plancher. À données synthétiques comparables (plusieurs sous-amas
par famille — une seule gaussienne isotrope reste lisse en son cœur quel que
soit le noyau, ce n'est pas un test équitable), 1024×1024 et 1 % rendent des
contours sinueux, à plusieurs lobes, avec de vrais golfes et îlots. Défaut
retenu : **1024×1024, noyau à 1 %, sept bandes** — 550 ms, toujours sous la
seconde pour un recalcul déclenché à la demande, jamais par image.

### Ce que la carte vaut

Contrôle indépendant du modèle : l'album et l'artiste viennent des tags, que le
réseau n'a jamais vus. Sur la bibliothèque **entière**, 27 031 morceaux et
364 millions de paires —

| Paires | Distance moyenne | Nombre de paires | Rapport au hasard |
|---|---|---|---|
| Même album | 0,368 | 242 285 | **0,50 ×** |
| Même artiste | 0,489 | 999 182 | 0,66 × |
| Même famille (k-means) | 0,359 | 42 108 131 | 0,48 × |
| Au hasard | 0,742 | 364 270 536 | 1,00 × |

Le jalon 2, sur 999 morceaux, donnait 0,48 × pour l'album : **le résultat
généralise**. L'artiste se dégrade (0,52 → 0,66) et c'est attendu — à cette
échelle, beaucoup d'artistes traversent les genres, ce que le son reflète
honnêtement.

### Les 13 morceaux qui manquent

Sur 27 044, treize résistent au décodeur — et la cause n'est pas celle qu'on
croit. Quarante fichiers mp4/m4a échouaient d'abord ; ils avaient **le même
codec** (AAC-LC, 44,1 kHz, stéréo) que ceux qui passaient. La seule différence
tenait à la disposition du conteneur :

```
qui passe   : ftyp / moov / free / mdat      ← index en tête
qui échoue  : ftyp / mdat / mdat / moov      ← index en fin
```

`ffmpeg -c copy -movflags +faststart` déplace l'index sans réencoder : les
octets audio sont identiques. Trente-neuf récupérés ainsi. Restent **10 opus**
(symphonia ne les décode pas), **2 mp3 corrompus** et **1 m4a**.

Les exemples de `crates/analysis/examples/` rejouent chaque mesure :
`cout_inference`, `cout_chaine`, `cout_projection`, `verif_empreintes`,
`verif_carte`.

### Combien de fenêtres par morceau ?

Cinq. Mesuré, pas choisi (`couverture`, 1 743 morceaux, 60 333 paires de même
album et 40 555 de même artiste). Le juge est indépendant du modèle : album et
artiste viennent des tags, que le réseau n'a jamais vus. Les neuf fenêtres sont
décodées une seule fois, les fenêtrages plus courts en étant des sous-ensembles
exacts — les variantes portent donc rigoureusement le même audio.

| Fenêtres | Couverture | Même album | Même artiste |
|---|---|---|---|
| 1 | 10 s | 0,59 × | 0,58 × |
| 3 | 30 s | 0,53 × | 0,52 × |
| **5** | **50 s** | **0,51 ×** | **0,51 ×** |
| 9 | 90 s | 0,51 × | 0,51 × |

Rapport au hasard, plus bas vaut mieux. La courbe s'aplatit après cinq : neuf
n'apportent **rien** de plus. Le fenêtrage fait partie de la représentation,
d'où son inscription dans le nom du modèle (`clap-htsat-unfused-5f`) — sans
quoi deux passes différentes se mélangeraient dans la même carte sans que rien
ne le signale.

### La sélection au lasso

`alt` + glisser entoure une zone de la carte et en fait une playlist. Deux
choix qui ne sautent pas aux yeux :

- **le contour peut être concave** — lancer de rayon en règle pair-impair, pas
  d'enveloppe convexe. Un lasso tracé à la main l'est presque toujours ;
- **la playlist est ordonnée en parcours de proche en proche**, pas dans
  l'ordre de la base. Une zone donne des dizaines de morceaux ; les enchaîner
  tels quels sauterait d'un bout à l'autre de la sélection. Départ au morceau
  le plus central, puis glouton du plus proche voisin non encore pris.

### Les quatre chemins

`crates/analysis/src/chemin.rs`. Le mode se choisit dans le rail ; **Maj est le
modificateur dans les quatre**, seul le geste change.

| mode | geste | calcul |
|---|---|---|
| **Direct** | clic, maj+clic | droite **sur la carte** entre les deux points, plus proche morceau à chaque pas |
| **Lisse** | clic, maj+clic | plus court chemin (Dijkstra) dans le graphe des 12 plus proches voisins |
| **Errance** | maj+clic | marche aléatoire auto-évitante dans ce même graphe, reproductible à graine égale |
| **Dessiné** | maj+glisser | trait rééchantillonné à pas d'arc constant, cueillette dans un rayon de 24 px |

Mesuré depuis la ligne de commande — `rusty-music path "<départ>" "<arrivée>"`
rejoue les trois modes calculés et les chronomètre :

| Opération | 3 409 empreintes | Projection sur 27 044 |
|---|---|---|
| Graphe 12-ppv, 12 fils | 0,3 s | ~19 s (n², à remesurer) |
| Chemin direct | < 1 ms | 19 ms sur 27 031 |
| Chemin lisse (Dijkstra) | < 1 ms | — |
| Errance | < 1 ms | — |

Le graphe est donc **préparé en tâche de fond** dès qu'on choisit Lisse ou
Errance (`prepare_graph`), et gardé en cache tant que le nombre d'empreintes ne
bouge pas. Le construire à la demande ferait attendre une vingtaine de secondes
sur un clic.

Deux points qui se voient à l'écran :

- **Le direct a d'abord zigzagué.** Il interpolait entre les deux empreintes
  (interpolation sphérique, pour que les pas restent réguliers) : le trajet
  était juste, mais illisible à l'écran — une droite dans l'espace des
  empreintes n'en est plus une après t-SNE. Un mode nommé « direct » qui
  serpente ne tient pas sa promesse. **Il tire désormais une droite sur la
  carte** : mesuré sur un trajet entre deux styles éloignés (reggae → metal) en 8 étapes, l'écart
  maximal à la droite vaut 0,7 % de sa longueur et les étapes tombent
  exactement au septième.
- **Le lisse, lui, suit un trait continu sans calculer sur la carte** : il ne
  saute qu'entre proches voisins, et t-SNE préserve justement les voisinages.
  C'est le mode à prendre quand on veut la vérité sonore du trajet.

### Pièges de cette chaîne

- **Le regroupement porte sur les empreintes, pas sur la carte.** t-SNE déforme
  les distances ; regrouper sur ses coordonnées décrirait le dessin au lieu de
  la musique. Même règle pour les voisins et le chemin **lisse**. Exception
  assumée, à frontière nette : les deux modes où l'utilisateur désigne un geste
  à l'écran — le **dessiné** (un trait) et le **direct** (une droite entre deux
  points visibles).
- **Compter les empreintes, jamais les points de la carte, pour savoir si un
  cache est périmé.** `map_points` écarte les morceaux pas encore projetés :
  pendant une analyse les deux nombres diffèrent en permanence, et un cache
  réglé dessus se croit périmé à chaque appel — 55 Mo relus et le graphe des
  voisins reconstruit à chaque chemin demandé. D'où `count_embeddings`.
- **`completer()` doit être appelé régulièrement** par tout ce qui pilote le
  lecteur : la file n'est plus chargée d'un bloc, sans quoi la lecture s'arrête
  après trois pistes.
- **Une piste illisible fait avancer le rang avant la tentative**, sinon la
  file se bloquerait indéfiniment dessus (les fichiers Opus).
- **Le calcul d'une onde passe de 3,5 s à 13 s** quand une passe d'analyse
  sature la carte : les deux se disputent le même support.

## Rendu cartographique — tuiles vectorielles et MapLibre

`crates/carto` : projection, tuiles MVT, archive PMTiles, ombrage, style. Ni
encodeur MVT ni écrivain PMTiles écrits ici — `mvt` 0.15 et `pmtiles` 0.24
(MIT/Apache-2.0) les couvrent, et `pmtiles` sert justement de socle à
**martin**, le serveur de tuiles que `CLAUDE.md` désignait. `maplibre-rs` reste
écarté : archivé, rendu de texte inachevé.

### La projection : le carré de la carte est le monde entier

Le piège annoncé par `CLAUDE.md`. La décision tient en une phrase : le domaine
`[-1,08 ; 1,08]²` de la carte **est** le carré complet de Mercator, pas une
région posée quelque part sur une Terre. Le zoom 0 montre donc toute la
bibliothèque dans une seule tuile.

Deux conséquences, et la première n'était pas prévue :

- **la déformation de Mercator nous arrange.** Aux latitudes extrêmes une même
  distance occupe plus de pixels ; le nuage étant centré, ce sont ses bords —
  les familles rares, la longue traîne — qui gagnent de la place ;
- **la demi-étendue doit valoir exactement celle du champ de densité**
  (`1 + core::density::MARGE`), sinon relief et territoires se décalent l'un par
  rapport à l'autre sans qu'aucun des deux n'ait l'air faux.

### Le relief : ombrage calculé ici, et non par MapLibre

MapLibre sait ombrer un modèle numérique de terrain (`raster-dem` +
`hillshade`), et c'était la voie évidente. Écartée pour deux raisons :

- son calcul de pente part de la taille d'un pixel **en mètres**, déduite du
  zoom et de la latitude. Notre monde n'a pas de mètres : il fait 40 000 km de
  large parce que c'est ce que vaut un planisphère, et une altitude
  vraisemblable y serait rigoureusement plate ;
- ce calcul dépend du zoom, donc le relief **changerait d'aspect en zoomant**.
  Correct sur une vraie Terre, faux sur une carte inventée.

L'ombrage de Horn (1981) tient en trente lignes, et trois défauts en sont
sortis, tous attrapés par des tests :

1. **le point neutre n'est pas 0,5 mais `cos(zénith)`**, l'éclairement d'un sol
   plat — 0,71 à 45°. Prendre 0,5 couvrait toute la carte, mer comprise, d'un
   voile clair de 18 % ;
2. **le signe de la pente nord-sud.** La formule de Horn suppose déjà des
   ordonnées descendantes, comme `v` : « corriger » ce signe éclairait le
   sud-est ;
3. **l'exagération.** 24 donnait des pentes de 88° partout — plus rien que du
   noir et du blanc. La bonne échelle divise par l'écart **en unités de monde**,
   ce qui rend l'ombrage identique à tous les zooms ; 0,20 convient.

Un quatrième réglage s'est décidé à l'œil, pas au test : **le noyau de densité
du relief n'est pas celui des territoires.** À 0,02, le réglage des contours,
l'ombrage ressemble à du papier froissé — la nappe porte tout le détail des
27 000 morceaux. 0,05 rend des massifs et des vallées ; 0,08 déborde jusqu'aux
bords et fait perdre la forme d'île.

### Le défaut qui a coûté le plus cher : une expression de zoom imbriquée

Écrire

```json
"circle-radius": ["*", ["sqrt", ["get","effectif"]],
                       ["interpolate", ["linear"], ["zoom"], 3, 0.45, 9, 1.3]]
```

fait **rejeter le style entier**. La spécification exige que `["zoom"]` soit
l'entrée de l'expression la plus extérieure. Le symptôme : carte noire, `load`
qui ne se déclenche jamais, et **rien nulle part** — ni erreur, ni avertissement,
ni trace. Il se manifestait à l'identique dans la webview et dans un navigateur,
ce qui a d'abord fait soupçonner la webview.

D'où deux garde-fous, tous deux dans `crates/carto/src/style.rs` :

- **le style est engendré depuis Rust**, à partir des mêmes `Paliers` que les
  tuiles : ce qui n'est pas dans la tuile ne peut plus être déclaré dans le
  style ;
- un test parcourt chaque propriété et **refuse toute expression de zoom qui
  n'est pas en tête**.

### Ce que ça coûte — mesuré sur 27 042 morceaux

| | |
|---|---|
| lecture de la base | 0,64 s |
| nappe de densité | 0,58 s |
| tuiles vectorielles | 1,30 s |
| ombrage | 2,00 s |
| **total** | **4,5 s** |
| archive vectorielle | 16,7 Mo, 60 373 tuiles, zooms 0-9 |
| archive d'ombrage | 9,0 Mo, 85 tuiles, zooms 0-3 |

Par zoom : 1 tuile à z0 (211 Ko, la plus lourde de toutes), 626 à z5, 29 579 à
z9 (0,2 Ko en moyenne). Les territoires s'arrêtent à z6 et le sur-zoom les sert
au-delà — les produire jusqu'au bout multiplierait l'archive sans rien ajouter
à l'écran.

**Latence mesurée** (Chrome, GPU Metal, M4 Pro), d'un saut de caméra jusqu'à
l'image stabilisée, cache chaud :

| échelle | ms | ce qui est rendu |
|---|---|---|
| z0 planisphère | 316 | 255 territoires, 11 noms de familles |
| z3 continents | 602 | 120 territoires, 579 artistes |
| z5 territoires | 603 | 66 territoires, 37 villes et leurs étiquettes |
| z7 villes | 606 | villes et premiers morceaux |
| z9 morceaux | 614 | morceaux |

Les ~600 ms sont dominés par le placement des symboles de MapLibre, pas par nos
tuiles : le z0, qui n'en place que onze, tombe à 316 ms.

**La fluidité en images par seconde n'a pas pu être mesurée honnêtement.** Voir
ci-dessous.

### Ce qui ne marche pas : MapLibre dans la webview de Tauri

**MapLibre ne s'initialise pas dans la WKWebView.** Son fil de travail se crée
(`getWorkerCount()` rend 1) mais ne répond jamais : aucun `style.load`, aucun
`styledata`, aucune erreur, aucune trace — même avec un style minimal sans
aucune source. Écarté par l'expérience, dans cet ordre :

| hypothèse | vérification | verdict |
|---|---|---|
| notre style | style minimal, une seule couche de fond | échoue pareil |
| nos tuiles | aucune source déclarée | échoue pareil |
| la politique de sécurité | CSP désactivée | échoue pareil |
| un blob interdit | `new Worker(URL.createObjectURL(...))` | fonctionne |
| une régression de la v5 | v4.7.1 | échoue pareil |
| le fil issu d'un blob | bundle `maplibre-gl-csp` + `setWorkerUrl` | échoue pareil |

Ce qui a été appris en chemin et reste acquis :

- **le schéma d'URI personnalisé de Tauri n'est pas atteignable depuis
  MapLibre.** Son fil de travail est construit sur une URL `blob:`, d'origine
  opaque, et WKWebView refuse ses requêtes en silence. D'où le passage par
  `maplibregl.addProtocol`, qui charge sur le fil principal, où `invoke`
  fonctionne. C'est la bonne architecture indépendamment du reste ;
- **la fenêtre principale saturait l'IPC.** Elle charge ses 27 000 points au
  démarrage, et les réponses de la fenêtre carte attendaient derrière :
  **110 secondes mesurées** entre le retour d'une commande côté Rust et sa
  réception côté JavaScript ;
- **`Library::familles` coûtait 42 s à froid** — tout l'arbitrage des genres
  MusicBrainz sur 27 000 morceaux, refait à chaque ouverture de la carte. Le
  style est désormais écrit **avec** les tuiles et relu depuis un fichier : plus
  de recalcul, et plus de dérive possible entre les deux.

Ce qui reste à trancher : soit trouver ce que WKWebView refuse à MapLibre (une
bissection du bundle, ou un rapport en amont), soit changer de coquille pour la
carte. Rien de tout cela ne touche `crates/carto`, qui est vérifié et mesuré
indépendamment.

## Réseau de circulation et profils d'itinéraire

`crates/analysis/src/reseau.rs`. `carto-google-maps.md` §2 et §3. **Aucun plus
court chemin n'est écrit** : `pathfinding` 4.15 (A*, Dijkstra, Yen),
`rustworkx-core` 0.18 (centralité de Brandes), `petgraph` 0.8 (arbre couvrant
minimal). `fast_paths` reste en réserve — voir les temps de routage plus bas,
il n'a pas lieu d'être.

### Un seul graphe, quatre coûts

La promesse d'OSRM, reprise telle quelle : le graphe des douze plus proches
voisins ne change jamais, seul le prix d'une arête change. Trois profils sont
de vraies fonctions de coût — `distance ÷ popularité`, `distance × popularité`,
pénalité de maintien dans le même territoire. **Le quatrième, les étapes
imposées, n'en est pas une** : c'est un enchaînement de tronçons, comme les
arrêts d'un itinéraire routier, et le document le range à tort dans la même
colonne. Un test vérifie que le voisinage d'un morceau est identique sous les
quatre profils : c'est ce qui garantit qu'il n'y a bien qu'un graphe.

### Deux défauts qui n'étaient pas devinables

**`1 − cosinus` n'est pas une distance.** C'est la moitié du carré de la corde :
l'inégalité triangulaire n'y tient pas, et la somme de petits sauts y est très
inférieure au saut direct. L'heuristique d'A* bâtie dessus **majorait** au lieu
de minorer, et A* rendait des trajets plus chers que l'optimum sans rien
signaler — 726 352 contre 344 551 sur le corpus d'essai, plus du double. On
route donc sur l'**angle**, qui est la géodésique de la sphère et une vraie
métrique ; il classe les arêtes dans le même ordre. `1 − cos` reste ce qu'on
*rapporte* comme distance sonique, la grandeur du document.

**L'itinéraire à durée cible rendait une promenade, pas un chemin.** Première
version : l'état de recherche portait la durée écoulée, quantifiée par paliers.
Résultat sur la bibliothèque réelle : deux titres de reels irlandais alternés
quatre fois chacun — rebondir entre deux voisins très proches est le moyen le
moins cher de remplir quarante minutes. Interdire le demi-tour n'y suffit pas,
le cycle passe simplement à trois morceaux.

La correction tient en une observation : **un plus court chemin à coûts positifs
ne repasse jamais par un nœud.** Il suffit de ne pas mettre la durée dans
l'état. La durée se traite alors par le choix de la destination :

- **sans arrivée imposée** — un seul `dijkstra_all` depuis le départ, la durée
  cumulée le long de l'arbre des plus courts chemins, et l'on garde la
  destination dont le trajet dure ce qu'on a demandé ;
- **avec une arrivée imposée** — Yen énumère des chemins simples du moins cher
  au plus cher, on retient celui dont la durée colle.

Les deux sont entièrement des appels de bibliothèque.

### La centralité : à quelle échelle la mesurer

Brandes coûte `O(V·E)`. Mesuré sur 27 042 morceaux et 261 270 arêtes :

| échelle | centralité | construction totale | autoroutes | nationales |
|---|---|---|---|---|
| morceaux | **226,6 s** | 243,7 s | 207 | 7 832 |
| artistes | **5,0 s** | 22,1 s | 197 | 7 833 |

**Quarante-six fois plus rapide pour une classification à une arête près.** Le
graphe contracté des artistes est donc le défaut — et ce n'est pas qu'un
raccourci : le document dit « autoroute : relie les grands pôles », et les pôles
sont les artistes, pas les morceaux. Mesurer le couloir plutôt que le brin est
plus proche de ce qu'on cherche. L'échelle exacte reste accessible
(`Echelle::Morceaux`) pour qui veut vérifier.

### La hiérarchie, sur la bibliothèque réelle

| classe | arêtes | part |
|---|---|---|
| autoroute | 197 | 0,1 % |
| nationale | 7 833 | 3,0 % |
| secondaire | 203 961 | 78,1 % |
| sentier | 49 279 | 18,9 % |

Plus 271 **refuges isolés** — des morceaux dont même le plus proche voisin est
loin.

Les autoroutes sont peu nombreuses **par construction** : ce sont les arêtes de
l'arbre de crête reliant les 80 grands pôles, pas un seuil de centralité. Un
seuil rendrait des tronçons épars ; l'arbre rend un réseau **continu**, ce qu'un
test vérifie. L'arbre suit la ligne de crête de densité parce que le coût d'une
arête y est majoré par le creux qu'elle traverse. Approximation de Steiner par
Kou-Markowsky-Berman : plus courts chemins entre pôles (`pathfinding`), arbre
couvrant minimal (`petgraph`), puis redéploiement des chemins.

### Ce que ça coûte à l'usage

Construction : 22,1 s, dont 16,1 s pour le graphe des voisins — c'est lui le
poste principal, pas la centralité. À faire une fois par session, comme le
graphe de `chemin.rs`.

Routage, sur 27 042 morceaux :

| | |
|---|---|
| autoroute, bout à bout | 9,3 ms |
| sentier | 19,5 ms |
| panoramique | 4,2 ms |
| **« 40 minutes »** | **1 ms** — 39,8 min rendues |
| trois alternatives (Yen) | 67 ms |

Les profils divergent réellement : popularité moyenne 0,720 par autoroute contre
0,381 par sentier, entre les deux mêmes morceaux.

**La popularité est le nombre de morceaux gardés d'un artiste.** ListenBrainz et
les compteurs de lecture locaux que prévoit `data-sources.md` n'existent pas, et
la base ne porte aucun compteur d'écoute. C'est une approximation locale et
honnête, normalisée en logarithme parce que la distribution s'étale de 1 à 769 :
en échelle linéaire, tout le monde vaudrait zéro sauf trois artistes.

### Commande

```
rusty-music itineraire <départ> [--arrivee N] [--profil autoroute|sentier|panoramique]
                       [--minutes 40] [--etapes a,b,c] [--eviter-autoroutes]
                       [--alternatives 3] [--k 12]
```

Elle affiche le profil de popularité en barres — le dénivelé du document — et la
classe de chaque tronçon.

## Démixage (module 3)

Les poids ne sont pas dans le dépôt — 84 Mo. Une fois :

```bash
./scripts/preparer-demucs.sh
cargo run --release -p rusty-music-cli -- demix <titre> --seconds 30
cargo run --release -p rusty-music-cli -- demix /chemin/vers/morceau.flac --out ./stems/
```

Quatre WAV en sortie — batterie, basse, autre, voix — en PCM 16 bits 44,1 kHz.

Trois variantes, à récupérer séparément (`./scripts/preparer-demucs.sh <nom>`) :

| variante | poids | stems | vitesse |
|---|---|---|---|
| `htdemucs` (défaut) | 84 Mo | 4 | 7,8 × le temps réel |
| `htdemucs_6s` | 84 Mo | 6 — ajoute guitare et piano | 7,0 × |
| `htdemucs_ft` | 333 Mo | 4, un réseau par stem | ~4 × plus lent |

Le six-stems n'est pas seulement « plus de stems » : sur l'intro d'un morceau
pop-rock, le quatre-stems verse piano et guitare dans `other` (RMS 0,090) faute
de mieux, là où le six-stems les sépare (guitare 0,078, piano 0,025). En
contrepartie sa reconstruction est plus lâche — SDR 18,3 dB contre 35,8 sur le
même extrait, écart qu'on retrouve sur signal synthétique (27,9 contre 35,6).
C'est une propriété du modèle, pas un défaut de la chaîne.

**Le modèle ne vient pas d'ONNX.** `docs/module3-demixage.md` détaille pourquoi
cette voie a été écartée : 66 % du graphe exporté n'était qu'une transformée de
Fourier déroulée nœud par nœud, et le backend GPU s'y trompait de 33 % sur un
stem. On s'appuie sur `demucs-core` — la STFT y reste en Rust, Burn ne reçoit
que le réseau — dans un **fork porté en Burn 0.21**
([`rousseau/demucs-rs`](https://github.com/rousseau/demucs-rs), branche
`burn-0.21` ; l'amont est figé en 0.20.1 depuis mars).

La dépendance épingle une **révision**, jamais une branche : une branche se
déplace, et le jour où elle bouge le build ne compilerait plus la même chose
sans que rien ne le signale. Suivre le fork demande de changer ce `rev`
sciemment.

### Mesuré sur un vrai morceau

30 s d'un morceau rock, sur le backend Metal (GPU) :

| | |
|---|---|
| Chargement du modèle | 0,3 s |
| Chauffe | 87 s la première fois, **7 s** ensuite (l'autotune se met en cache) |
| Séparation | 3,7 s — **8,1 × le temps réel** |
| Somme des stems contre l'entrée | **33,7 dB** (le test de `demucs-core` exige 20) |

La séparation sépare vraiment — répartition spectrale de chaque stem :

| stem | centroïde | < 250 Hz | 250 Hz – 2 kHz | 2 – 8 kHz | > 8 kHz |
|---|---|---|---|---|---|
| bass | 758 Hz | **71,9 %** | 21,9 % | 3,3 % | 2,9 % |
| drums | 4 889 Hz | 23,7 % | 12,1 % | **41,3 %** | 22,9 % |
| other | 2 982 Hz | 27,3 % | 32,8 % | 27,1 % | 12,8 % |
| vocals | 3 841 Hz | 5,7 % | **51,3 %** | 20,7 % | 22,3 % |

La basse tient 72 % de son énergie sous 250 Hz, la voix 51 % dans sa bande
fondamentale avec seulement 5,7 % de grave. Et les corrélations croisées entre
stems valent toutes moins de 0,04 : ce ne sont pas quatre copies du mélange.

### Le mode Éditer

Le bouton « Éditer » est actif. Dans le rail : le morceau à traiter, la
variante, un bouton **Séparer**. En bas, un **dock** qui pousse le centre vers
le haut sans le masquer — `ui-spec.md` veut que la carte reste la réserve de
matière.

Les stems s'y écoutent **ensemble**, avec solo et coupure à la volée. Ce n'est
pas quatre lecteurs lancés en même temps : rien ne garantirait qu'ils démarrent
au même échantillon, et quelques millisecondes de décalage entre la batterie et
la basse s'entendent. C'est **une seule source qui somme les pistes**
échantillon par échantillon (`crates/player/src/multipiste.rs`), chacune avec
son niveau lu dans un atomique — l'alignement est exact par construction, et un
solo s'applique sans interrompre la lecture.

Les stems sont chargés en mémoire en `i16` : ils sortent de notre écriture WAV
16 bits, la conversion ne perd rien, et c'est deux fois moins lourd que du
`f32`. Quatre stems d'un morceau de quatre minutes tiennent dans 186 Mo — le
prix d'un solo instantané et d'un déplacement sans relecture disque.

**Chaque stem a son spectrogramme.** L'onde dit *combien* de son il y a ; le
spectrogramme dit *quoi* — et sur des stems séparés c'est ce qui compte, une
basse et une batterie ayant des enveloppes voisines et des spectres sans
rapport. Axe des fréquences logarithmique (l'oreille l'est, et en linéaire les
trois quarts de l'image seraient des aigus vides), plage dynamique bornée à
80 dB, coloration par la **rampe séquentielle** déjà retenue pour la carte.

Vérifié plutôt que supposé (`verif_spectre`, 140 ms par stem) :

| stem | graves | médium | aigus |
|---|---|---|---|
| bass | **139,8** | 31,0 | 0,5 |
| drums | 83,1 | 50,7 | 37,0 |
| other | 166,1 | **128,7** | 41,2 |
| vocals | 66,6 | **82,0** | 31,6 |

La basse s'écrase sur le grave, la batterie s'étale, la voix culmine dans le
médium. Un spectrogramme joli n'est pas un spectrogramme utile : c'est cet
écart entre les bandes qui le rend lisible.

**Un seul bouton, une seule tête de lecture.** Le dock n'a aucune commande de
transport : le `▶` du bas charge les stems à la première pression puis les
pilote. Et la position est **une seule valeur** que tout le monde lit — la
barre du bas, et la tête tracée sur chacun des spectrogrammes. Cliquer sur
n'importe quel spectrogramme déplace la lecture exactement comme la barre du
bas : c'est le même axe des temps, donc le même geste.

Ce qui est déjà passé s'assombrit sur le spectrogramme plutôt que de compter
sur le seul trait : sur une zone claire du spectre, un trait fin se perd.

Avant que les stems ne jouent, la tête suit quand même le morceau d'origine —
les spectrogrammes servent alors de repère sur la version non séparée.

Deux choix à connaître :

- **Les stems vont à côté de la base**, jamais dans la bibliothèque : celle-ci
  est une source en lecture seule, et elle vit sur une carte SD lente.
- **Un démixage déjà calculé se retrouve** d'une session à l'autre. Trente
  secondes ne se rejouent pas parce qu'on a fermé la fenêtre.

### Régler un stem seul

Chaque ligne du dock s'ouvre sur sa propre vitesse et sa propre hauteur, plus un
bouton « suivre l'ensemble ». Le badge de la ligne dit ce qui s'écarte : un stem
réglé ne doit pas pouvoir se cacher derrière un panneau replié.

**Des vitesses différentes désynchronisent, et l'écart grandit tant que la
lecture continue.** L'avertissement n'est pas qu'une infobulle : revenir à
l'ensemble remet les vitesses à égalité *et* réaligne les stems, sans quoi on
arrêterait la dérive en gardant l'écart déjà pris.

### Greffer un stem

```bash
rusty-music greffer ancien.wav greffon.wav sortie.wav --bpm-source 124 --bpm-greffon 96
```

Prendre la batterie d'un autre morceau et la mettre à la place de celle-ci —
c'est le premier geste de l'éditeur qui va chercher quelque chose dans la
bibliothèque, et donc le premier qui relie l'éditeur à la carte. Le panneau d'un
stem propose les morceaux sonorement voisins ; on en choisit un, la greffe
s'écrit, le stem d'origine reste où il est.

Trois choses à faire tenir, et pas une de plus (`crates/editor/src/greffe.rs`) :

| | ce qu'on fait | pourquoi |
|---|---|---|
| **tempo** | étirer du rapport des deux, **replié à l'octave** dans [1/√2, √2] | un stem à 96 BPM sous un morceau à 124 flotte aussitôt ; mais une boucle à 70 sous 140 n'a pas à être accélérée du double, ses temps tombent déjà un sur deux. Le repliement borne l'étirement à **±41 % au pire** |
| **départ** | le greffon entre là où l'ancien stem entrait | une batterie qui commence après l'intro ne doit pas arriver trente secondes trop tôt |
| **longueur** | boucler ou couper, 20 ms de fondu aux jonctions | sans fondu, chaque répétition claque là où la fin rencontre le début |

**L'attaque se cherche à l'énergie relative**, pas au premier échantillon non
nul : un stem séparé n'est jamais exactement silencieux, le modèle y laisse un
fond. On prend la première fenêtre de 10 ms qui dépasse un vingtième de
l'énergie moyenne.

Deux choix qui rendent le geste réversible et honnête :

- **le voisinage se calcule sur le morceau entier, pas sur le stem.** Limite
  assumée : la bibliothèque n'a d'empreintes que de mélanges complets, et en
  embarquer une par stem supposerait de démixer les 27 000 morceaux ;
- **le tempo est une contrainte dure, pas un classement.** Au-delà de ±10 %
  après repliement, un candidat n'est pas proposé, si proche soit-il sur la
  carte. La liste dit combien ont été écartés et combien n'ont pas de tempo
  mesuré — 400 voisins sont examinés pour en retenir une poignée ;
- **la greffe est un fichier de plus**, sous `greffes/`, jamais une réécriture.
  Rouvrir le morceau retrouve ses stems séparés. Et une greffe se calcule
  toujours depuis le stem *d'origine* : greffer sur une greffe empilerait les
  étirements.

**Les temps forts se calent — depuis le 19 août.** La grille de battements
(voir plus haut) donne la phase des deux stems, et la greffe s'en sert pour
deux choses : le greffon **entre** sur un battement, et sa matière est **coupée
à un compte rond de battements**. La seconde compte autant que la première —
sans elle, une greffe qui boucle six fois se désaccorde six fois, un peu plus à
chaque tour, puisque la matière n'a aucune raison de mesurer un compte rond.

Vérifié plutôt qu'affirmé : la grille du stem remplacé obtient **2,19** sur la
greffe calée et **1,08** sur la même greffe non calée, où une phase quelconque
vaut 1,00. La commande le dit elle-même à chaque greffe.

**Ce qui reste** : le calage vaut ce que vaut la grille, et sur une batterie la
phase est presque indéterminée — deux décalages à un demi-battement l'un de
l'autre peuvent être départagés par 0,03 de netteté. Le recours reste la
vitesse par stem, mais il sert désormais à rattraper une ambiguïté, pas une
absence de calage.

**`crates/editor` ne dépend pas de `crates/analysis`**, et ce n'est pas un
détail : ce serait tirer CLAP, ses 117 Mo de poids et la génération de code de
son `build.rs` dans un crate qui n'a que faire d'un modèle d'empreintes. La
grille voyage en trois nombres, et c'est l'application qui relie les deux
modules.

### Pièges de cette chaîne

- **`fusion` et `autotune` ne sont pas optionnels côté GPU.** Sans eux, Metal
  met 90 s par segment au lieu de 728 ms. Ils sont activés par la feature `gpu`
  du crate.
- **La première inférence coûte 87 s**, le temps que l'autotune éprouve ses
  variantes de noyau. Le résultat se met en cache sur disque : les lancements
  suivants tombent à 7 s. D'où `Demixeur::chauffer()`, qui paie cette dette
  avant qu'on chronomètre quoi que ce soit.
- **Le décodage est stéréo 44,1 kHz**, pas mono 48 comme le module 2 :
  `crates/editor/src/decode.rs` est distinct de `crates/analysis/src/decode.rs`
  et le restera.
- **Les stems sont écrêtés à l'écriture.** Un stem peut dépasser 1,0 là où le
  mélange ne le faisait pas ; sans écrêtage, la conversion en 16 bits replierait
  le signal et le craquement s'entendrait.

### Correction bidirectionnelle du tempo — 26 août 2026

Suite à `docs/journal.md` (« Les deux écarts chiffrés ») et au plan de
recherche sur l'octave (voir l'historique de session) : `Cmd::Voisins` avait
révélé un bug concret sur « Hard Core 100% Fluor » (Watcha) — 46 BPM en base,
alors que l'évidence brute d'autocorrélation pointe nettement vers ~183 sur
4 de ses 5 fenêtres. Cause reconstituée : `descripteurs::tempo()` ne corrige
l'octave que dans un sens (division par deux) ; un gagnant de grille ambigu
(~92 BPM, à mi-chemin entre deux lectures plausibles) se faisait rediviser
au lieu d'être remultiplié vers le vrai tempo.

**Correctif livré** : `SEUIL_SUR_OCTAVE` (1,02) et `BPM_MAX_CORRECTION`
(266,7, symétrique de `BPM_MIN_CORRECTION` par le même rapport à sa borne de
grille) — `crates/analysis/src/descripteurs.rs`. Le sens montant l'emporte
quand les deux qualifient : un départage par simple comparaison d'évidence
brute favorise structurellement la moitié (elle recouvre une partie des
harmoniques du gagnant par construction du peigne), ce qui aurait annulé la
correction utile — mesuré en écrivant le correctif, pas deviné.

**Calibré et validé sur trois cas réels** avant tout déploiement :
- Watcha, cas signalé : 4 des 5 fenêtres remontent à ~183-184 BPM, la
  médiane passe de 46 à 183 — corrigé.
- Johnny Cash « Give My Love to Rose », référence historique du seuil
  descendant : résultat inchangé au niveau de la médiane (65,5 BPM),
  fenêtre par fenêtre identique à la calibration documentée en 2026
  (« Give My Love to Rose… 130… corrige juste à 65 »).
- Alexi Murdoch « Dream About Flying », déjà plausible (61 BPM) : médiane
  inchangée malgré une fenêtre isolée qui bascule.

Test synthétique portable ajouté (`le_tempo_remonte_un_rythme_accentue_a_la_mesure`) :
un train de clics à tempo rapide dont un clic sur deux est accentué de 10 %
(gain 1,0/0,9 — calé empiriquement, en dessous de 0,8 le rythme de la mesure
domine trop pour laisser une évidence testable au battement) reproduit le
mécanisme réel sans dépendre d'un fichier externe.

**Validation à plus large échelle — `crates/analysis/examples/octave_avant_apres.rs`,
300 morceaux au hasard de la bibliothèque réelle** : la concentration dans
[40, 90] BPM baisse de 83,7 % à 75,3 %, celle dans [150, 267] BPM monte de
3,7 % à 12,7 % — le mouvement prédit, dans le bon sens, sur un échantillon
non trié sur le volet (539 s, 1,8 s/morceau).

**Mais la validation révèle aussi une vraie limite, honnêtement à
signaler.** 42 morceaux sur 300 (14 %) se déplacent de plus de 3 %, et
plusieurs, inspectés, sont des **faux positifs plausibles** — une comptine
enfantine instrumentale (« Le chat est par terre ») passe de 55 à 220 BPM.
Sondé en détail : l'évidence brute élargie montre un vrai pic vers 73 BPM
(220,75 / 3 ≈ 73,6) que la comparaison gagnant-contre-double ne peut pas
voir, puisqu'elle ne compare que deux candidats à la fois. **Le seuil ne
sépare pas franchement les deux populations** : les ratios des vrais
positifs (Watcha : 1,02-1,05) et ceux de ce faux positif se chevauchent — un
seuil plus strict qui exclurait le second exclurait aussi le premier. Ce
n'est pas un bogue d'implémentation à corriger, c'est la limite structurelle
d'une comparaison à deux candidats déjà anticipée par le plan de recherche
(« aucune piste ne promet d'éliminer l'erreur d'octave, seulement d'en
réduire l'incidence ») — et par la littérature externe citée (erreurs de
facteur 2/3/4 qui restent un problème ouvert même à l'état de l'art.

**Décision** : gardé. Le bilan agrégé est net et mesuré positif (le
symptôme à 87 % à l'échelle de toute la bibliothèque se réduit
significativement), le cas qui a motivé le correctif est résolu, la
référence historique n'a pas régressé. Les faux positifs sur matière très
répétitive (comptines, boucles simples) restent un résidu documenté, du
ressort des pistes 2 (a priori adaptatif) ou 3 (consensus inter-fenêtres) si
on veut le réduire encore — pas de ce correctif-ci.

> **Suite (1er sept. 2026)** : le sens *descendant* décrit ici — hérité, pas
> ajouté par ce correctif — s'est avéré rabaisser d'une octave 55 % de la
> bibliothèque. Son discriminant (`brut(bpm/2)` contre `brut(bpm)`) ne
> discriminait rien. Remplacé par un test d'alternance fort/faible. Voir
> « La correction d'octave descendante rabaissait la moitié de la
> bibliothèque ».

## Mode Écouter — qualité du fichier et bouton « E » — 29 août 2026

Détail complet : `docs/amelioration-audio.md`.

### Ligne de qualité

`codec` / `bitrate` / `sample_rate` / `channels` étaient déjà en base
(lofty, au scan) mais nuls part exposés piste par piste. Ajout d'une commande
`qualite_piste(id)` calquée sur `descripteurs`, d'une colonne `bit_depth`
(migration, comme `bitrate`/`codec` avant elle — `NULL` jusqu'au prochain
rescan « relire les inchangés »), et d'une ligne sous le compteur de temps :
`FLAC · 16 bit · 44,1 kHz`, `MP3 · 320 kb/s · 44,1 kHz`. La profondeur de
bits n'est montrée que pour les codecs sans perte — lofty en rend parfois une
pour les formats avec perte, où elle n'a pas de sens.

### Bouton « E »

**Ce que « E » n'est pas** : une normalisation de loudness. Volume constant
d'un morceau à l'autre, c'est utile mais ce n'est pas une amélioration — ça
ira dans le mode Bibliothèque.

**Ce que « E » est** : un excitateur par non-linéarité. Synthèse des 2ᵉ et
3ᵉ harmoniques de la bande `[2,5 kHz, coupure]`, ajoutées dans le médium-aigu
audible autant qu'au-dessus de la coupure, dans le domaine STFT (pas de
suréchantillonnage, pas de repliement). Sur un FLAC ou un MP3 320,
`estimer_coupure` renvoie ≥ 18 kHz et la chaîne est un **passe-plat** — le
fichier ressort intact (test).

**Premier essai corrigé après écoute** : la version initiale ne recopiait
qu'une octave translatée *au-dessus* de la coupure (~16 kHz pour un MP3 128)
— quasi inaudible pour une oreille adulte (« je sens très peu de différence
sur un MP3 128 »). La synthèse d'harmoniques qui redescendent dans le
médium-aigu s'entend, elle. Ajout au passage d'une **intensité réglable**
(`0`..`1`, défaut `0,6`, courbe `intensité²`) — réglette dans le transport,
visible seulement quand « E » est actif, agit au relâché pour ne pas
réouvrir le morceau à chaque pixel.

Analyse écartée : la super-résolution neuronale (AERO, AudioSR via ONNX). Le
code est en MIT partout, mais les **poids musique d'AERO sont entraînés sur
MUSDB18-HQ, non commercial** — exclu par la politique de licence du projet.
AudioSR ne documente pas la licence de ses poids, et sa diffusion latente est
plus lente que le temps réel même sur A100. Reporté au module 3,
conditionné à un ré-entraînement sur corpus libre ou à une clarification de
licence.

Insertion : dans `ouvrir()`, sur le tampon déjà décodé en RAM, **hors du
verrou `Player`**. Bascule en cours d'écoute sans coupure —
`Player::remplacer_courant` réouvre le morceau en tâche de fond et le remet à
la position courante sous un verrou tenu quelques microsecondes, le
préchargement se reconstruit au sondage suivant. État global au processus
(`OnceLock`/`AtomicBool`) pour ne pas alourdir la signature de `ouvrir`.

### Rééchantillonnage `rubato`, toujours actif

Découvert en vérifiant si le bouton « E » devait porter le rééchantillonnage :
`rodio` 0.22 dit lui-même, dans `SampleRateConverter`, ne faire qu'une
interpolation linéaire pour monter en fréquence et jeter des échantillons
pour descendre — « may introduce audible distortions ». On rééchantillonne
donc le tampon vers la fréquence de la carte son avec `rubato` (`Fft`
synchrone) dans `ouvrir()`, et le convertisseur de `rodio` devient un
passe-plat. Passe-plat aussi quand les fréquences coïncident déjà (cas
courant : 44,1 kHz des deux côtés).

Nouvelles dépendances : `rubato` (MIT). `fundsp`, un temps envisagé pour un
excitateur temporel, écarté — la voie fréquentielle sur `rustfft` (déjà là,
et déjà l'outil du spectrogramme) évite le repliement sans suréchantillonnage
et sans arbre de dépendances supplémentaire.

### Sondage super-résolution neuronale (AERO)

Le dépôt `slp-rl/aero` (MIT) cloné et lu : c'est un U-Net sur spectrogramme
complexe de la lignée HDemucs — **la même famille d'opérateurs sur laquelle le
sondage `experiments/burn-demucs/` a mesuré que le GPU de Burn rend des
nombres faux**, plus LSTM, attention et l'activation Snake (`Sin`). La voie
Burn+GPU est donc une impasse pour AERO comme pour le module 3 ; la porte de
sortie est la même : `ort` + CoreML, STFT sortie du graphe et refaite en Rust.

**Bloqué sur la licence des poids** : le checkpoint musique d'AERO est
entraîné sur MUSDB18-HQ (non commercial), exclu par `CLAUDE.md`. Les
checkpoints `4-16` publics, eux, sont sur VCTK (CC BY) mais c'est de la voix
en 4→16 kHz. Sortie propre : ré-entraîner la config musique sur MTG-Jamendo
ou FMA (CC BY) — quelques jours de GPU, `train.py` fourni. C'est ce qui
reporte le chantier.

Livré : `experiments/burn-aero/README.md` (sondage), `scripts/preparer-aero.sh`
(export ONNX du réseau seul, STFT exclue — recette `preparer-modele.sh`),
`docs/module3-superresolution.md` (architecture du rendu hors-ligne
« régénérer en HD »).

**30 août — licence débloquée par le mandant** (« on utilise tous les outils
disponibles ; la licence sera adaptée à la publication »). Le sondage a donc
été mené jusqu'au bout :

- checkpoint musique récupéré (`musdb/aero-nfft=512-hl=256`, 437 Mo, via
  l'interstitiel « virus scan » de Google Drive) ;
- `preparer-aero.sh` reconstruit le modèle depuis
  `pkg['models']['generator']['kwargs']` (pas d'hydra), retire deux irritants
  — `torch.eye(T, bool)` de l'attention → `(delta == 0)` pour éviter `EyeLike`
  que ORT CPU n'implémente pas ; kwargs filtrés sur la signature d'`Aero` —,
  exporte le réseau seul et **vérifie la parité : erreur L2 relative
  2,8 × 10⁻⁵** contre PyTorch. Graphe de 628 nœuds, 0 opérateur exotique.
  `models/aero-11025-44100.onnx`, 156 Mo, métadonnées STFT embarquées ;
- **moteur d'inférence tranché par la mesure** (`experiments/burn-aero`,
  `cargo run --release`, segment de 5 s contre la référence PyTorch) :

  | moteur | fidélité | temps 5 s | verdict |
  |---|---|---|---|
  | `tract` (pur Rust) | cos **0,68 — faux** | 2,5 s (×2 TR) | un opérateur mal exécuté, comme wgpu sur demucs |
  | `ort` (ONNX Runtime) | cos **1,000000** (rel 3e-5) | 0,70 s (**×7 TR**, CPU) | retenu |

  Une piste de 4 min ≈ 35 s de rendu. `ort` télécharge ONNX Runtime (MIT) au
  build ; `ort-sys` tire `ureq`/`tar`/`flate2`.

Reste (`docs/module3-superresolution.md`, tableau d'étapes) : STFT/iSTFT Rust
fidèle à `torch.stft` (périodique, normalized, reflect — le prochain risque),
boucle de segments avec recouvrement, crate `crates/superres`, cache `hd/`,
commandes, aiguillage lecture, bouton « HD ». Rien encore dans le workspace.

### Super-résolution — crate `crates/superres`, bout à bout

Étapes 3 à 7 du tableau, faites dans la foulée.

**STFT/iSTFT Rust** (`crates/superres/src/stft.rs`) reproduisant les
`torch.stft` / `torch.istft` d'AERO. Les détails qui comptaient, tous à
vérifier contre une référence PyTorch et non à deviner : fenêtre de Hann
**périodique** (`0.5 - 0.5·cos(2πn/N)`, pas `N-1`) ; `normalized=True` ⇒
chaque trame ÷ √nfft ; `center=True` ⇒ repli (`reflect`) de nfft/2 avant
fenêtrage ; bin de Nyquist jeté (256 bins) ; et surtout `_spec` travaille en
`hop=64 win=128` (fenêtre de 128 zéro-complétée à 512), `_ispec` en
`hop=256 win=512` — c'est là qu'est le sur-échantillonnage ×4. Segment isolé,
pipeline complet : **erreur L2 relative 1 × 10⁻⁵** contre PyTorch.

**Segmentation** : le modèle a une entrée figée (T=862 trames ≈ 5 s). On
découpe le morceau en segments de 5 s avec 25 % de recouvrement, addition-
recouvrement avec fondu trapézoïdal. Fichier entier Rust vs `model()` PyTorch
non segmenté : **1,1 %** — la part de la segmentation (chaque segment
normalise par sa propre moyenne/écart-type), pas un défaut ; `predict.py`
d'AERO fait pire (segments de 10 s bout à bout, sans recouvrement).

**Piège `rubato`** : `Fft::process_all` de la 5.0 laisse ~1 s d'amorce fausse
en tête de signal — son retrait annoncé du retard de démarrage ne suffit pas
pour 44,1 → 11,025 kHz. Contourné en préfixant l'entrée d'un bloc réfléchi et
en jetant l'amorce correspondante : écart au rééchantillonneur de référence
12 % → 5 × 10⁻⁴. (Diagnostic : le signal était juste à 99,95 % **sauf** le
premier dixième, à 40 % d'erreur.)

**Format du cache** : WAV PCM 16 bits, comme les stems. `flacenc` (pur Rust)
produit un FLAC valide que `soundfile` relit mais que `symphonia` (donc
`rodio`, donc le lecteur) refuse — « end of stream ». Un WAV de 4 min stéréo
pèse ~42 Mo ; acceptable pour les quelques morceaux qu'on régénère.

**Aiguillage lecture** : `Player` porte un `resoudre: Fn(&Path) -> PathBuf`,
identité par défaut, que l'appli branche sur `superres::resoudre(&hd, p)`.
`Player.queue` garde les chemins d'origine (repérage de l'interface) ; seules
`a_precharger` / `completer` / la réouverture « E » passent par la résolution.
Drapeau global `lecture_hd` (comme « E »), bouton `#hd` à trois états dans le
transport.

**Test réel** : Lamb « Sacred Space », MP3 128, 7 min stéréo — régénéré en
**164 s** (×2,6 le temps réel), sortie 44,1 kHz 16 bits, crête rabattue à
−0,3 dBFS (le modèle n'est pas borné, on préfère réduire que d'écrêter).

### Le rééchantillonneur étouffait tout — deuxième piège `rubato`

Testé sur « Trawalc'h » de Startijenn (MP4 266 kb/s) : le HD **étouffait
complètement** le son, coupure à ~8 kHz là où l'original monte à 19 kHz et où
AERO en PyTorch rend ~17 kHz. Diagnostic par analyse de bandes : mon
pipeline, alimenté du `lr` de `torchaudio`, sortait un aigu correct ; alimenté
de mon propre `lr` (`rubato`), il s'effondrait. **`rubato::Fft::new` coupe
beaucoup trop bas** : son défaut vise un sous-bloc de ~256 trames pour la
latence, ce qui donne une **FFT de sortie de 64 points** à 44,1 → 11,025 kHz —
fenêtre anti-repliement si grossière que la coupure tombe à 4,3 kHz au lieu de
5,3. AERO voit alors une entrée déjà sur-filtrée et calibre sa reconstruction
en conséquence : sortie étranglée. `new_custom(…, sub_chunks = 1, chunk 16384,
Hann, …)` rend une FFT de sortie de milliers de points, coupure à 5,3 kHz
comme `torchaudio` — HD final à 16,6 kHz, comme la référence PyTorch.

### Puis « Beng Beng Beng » (MP3 128) : encore sourd — le fond du problème

Même après le rééchantillonneur, le HD étouffait « Beng Beng Beng ». Analyse
de bandes : le HD *étendait* bien la coupure (14 → 17 kHz) mais **creusait la
présence** — 8–12 kHz passait de 10,9 % à 7,6 %. Cause : le pipeline
**remplaçait tout le spectre** par la sortie du modèle, qui repart d'un 11 kHz
(Nyquist 5,5) et rebâtit le médium-aigu réel plus mou que le MP3 ne le
portait. Un MP3 128 monte à ~16 kHz : on jetait 5,5–16 kHz de vrai contenu
pour le reconstruire moins bien.

**Correctif : mélange HF.** `regenerer` garde le spectre de la source **sous
sa coupure estimée** et ne prend le modèle qu'**au-dessus** (croisement par
masque en cosinus surélevé de 300 Hz, domaine STFT 2048).
`melanger_hf(source_44k, hd, fc)`. Résultat mesuré sur « Beng » : bandes
0–16 kHz identiques à l'original (13,4 vs 13,6 % ; 10,8 vs 10,9 %), le HD
n'ajoute que 16–22 kHz (0,1 → 1,7 %). Sur « Trawalc'h » (coupure 21 kHz) le
mélange rend l'original à l'octet près — le HD ne peut plus ternir, il ne
peut qu'ajouter ou ne rien faire.

**Garde-fou** : `regenerer` rend la coupure estimée ; au-dessus de 16 kHz,
`start_superres` prévient — « la source monte déjà à 21 kHz, le HD n'ajoute
presque rien ».

`regenerer_depuis_lr` reste la sortie brute du modèle (sans mélange) pour les
tests de parité ; `regenerer` = décode → modèle → mélange → WAV.

### Le cache figeait les anciens sons — troisième itération

Après tout ça, « Beng Beng Beng » sonnait *encore* étouffé au relancement.
Cause : le **cache HD contenait le fichier d'une version antérieure** du
pipeline (coupure à 6,5 kHz, mesurée), joué tel quel — l'activation HD était
instantanée, sans régénération. Corrigés :

- **`VERSION_CACHE`** dans le nom du fichier (`<hash>-v3.wav`). Un cache d'une
  version antérieure n'est plus trouvé. `purger_anciens` supprime les fichiers
  qui ne portent pas la version courante ; appelé au démarrage et avant chaque
  régénération. **À incrémenter dès que `regenerer` change.**
- **Mélange par maximum spectral** (au lieu d'un masque fixe autour de la
  coupure) : sous `fc`, la source ; au-dessus, la raie la plus forte des deux.
  Garantie forte — le HD **ne peut qu'ajouter**, même si la coupure est
  sous-estimée : là où la source est encore présente au-dessus de `fc`, elle
  gagne. Mesuré sur « Beng » : bandes 0–16 kHz à la décimale de l'original.
- **Axe du spectrogramme fixé** (`spectre::F_MAX = 22 050 Hz`), plus lié à la
  fréquence de Nyquist de l'entrée. Deux spectrogrammes se comparent sur la
  même échelle : un aigu manquant est une bande sombre en haut, pas une image
  « écrasée » sur une échelle plus courte.

### Barre de lecture : spectrogramme au lieu de l'onde

Sur demande, `#wave` passe de 160 barres crête/RMS à un **spectrogramme du son
réellement joué** (canevas, axe log-fréquence, tête de lecture par-dessus,
clic pour se déplacer inchangé). Commande `spectre_transport(path, w, h)` :

- résout le chemin comme le lecteur (cache HD si la lecture HD est active) ;
- applique l'excitateur « E » au vol s'il est actif
  (`player::spectre_ameliore`, via `ouvrir`) ;
- rend **aussi** le spectrogramme de l'original quand le son joué en diffère
  (`pixels_ref`), pour que l'interface **teinte de l'accent ce que E ou HD a
  ajouté** — d'autant plus vif que le gain d'énergie est net.

Calcul en tâche de fond, mis en cache par (chemin, état HD) ; l'image paraît
quand elle est prête, un fond neutre sert de repère d'ici là. Zéro coût sur
la lecture (affichage seul), zéro sur la régénération. `spectre::calculer` est
refactorisé en `calculer_echantillons` réutilisable.

### La playlist ✦ ne prenait pas la main sur la file — 30 août 2026

Le bouton ✦ « playlist dans l'esprit de ce morceau » (inspecteur, mode
Écouter) envoyait la nouvelle liste par `set_queue`. Or `set_queue` est fait
pour la **régénération d'un chemin sur la carte** : même départ, on garde les
`prochain` premiers rangs déjà confiés à `rodio` et on ne remplace que la
suite. Quand la playlist part du morceau **en cours de lecture**, ce départ
identique déclenchait cette conservation — et les un ou deux morceaux de
l'ancienne file (les résultats de recherche) restaient en tête, préchargés,
avant que la playlist ne démarre. De l'extérieur : « la playlist Alchémie ne
démarre pas ».

Corrigé par `Player::rebrancher_file` + commande `remplacer_file` : on garde
la piste en cours **sans coupure** (sortie vidée, piste rouverte à sa
position — même procédé que `remplacer_courant` pour la bascule « E »/HD),
mais on **abandonne le préchargement** de l'ancienne file. `charges = [0]`,
`prochain = 1` : le sondage prépare la suite sur la nouvelle liste. Si rien
ne joue, ou si la tête de la nouvelle file n'est pas le morceau écouté, on
retombe sur `play`. `set_queue` reste inchangé pour la carte.

### Filtrer la grille de pochettes par famille — mode Écouter

La légende des familles du mode Explorer (pastille teintée + nom + effectif,
`rendreFamilles`) est réutilisée telle quelle dans le rail du mode Écouter,
au-dessus de « Bibliothèque ». Cochée, une famille restreint la grille aux
albums dont la **famille sonique dominante** est celle-là ; plusieurs
familles cumulent (OU), rien de coché montre tout, « Toutes les familles »
efface la sélection.

Commande `album_families` → `Library::familles_des_albums` : pour chaque
album (clé nom + `COALESCE(album_artist, artist)`, identique à `albums`), le
cluster de la carte le plus représenté parmi ses morceaux projetés, égalité
tranchée par le plus petit numéro. Un album dont aucun morceau n'est encore
sur la carte n'a pas d'entrée — le filtre le laisse alors visible.

Côté interface, `vue.lignes` reste la liste complète ; `lignesCourantes()`
applique le filtre au vol, partagé par la grille virtualisée, le repère
alphabétique et le compte du fil d'Ariane. Le mapping album→famille est
rechargé (et le filtre vidé) après tout recalcul du clustering
(`familleARecalculee`).

### La playlist ✦ se compose à vue, dans la file — 30 août 2026

Un clic sur ✦ (case d'album ou inspecteur) laissait l'utilisateur devant la
roulette système, sans rien dans le logiciel. Deux attentes s'y cachaient,
muettes : le **graphe des voisins** d'abord — un balayage complet, ~20 s la
première fois d'une session, gardé en cache ensuite — puis l'errance
elle-même, quasi instantanée, qui rend d'un bloc les 20 morceaux dans l'ordre
du trajet.

Les deux boutons passent maintenant par une fabrique commune,
`composerAlchimie({ bouton, chemin, demarrer })` :

- le glyphe ✦ tourne (`alchimie--travail`) tant que ça travaille ;
- le panneau **file d'attente** s'ouvre d'emblée sur 20 emplacements vides
  (la longueur cible visible tout de suite) surmontés d'un bandeau de phase
  et d'une jauge — « Préparation du graphe des voisins… » puis
  « Composition de la playlist… » ;
- la playlist vient s'y **poser piste par piste** (`revelerFile`, un rang par
  battement de 55 ms) ; la première porte la marque « graine » ;
- la **lecture démarre dès que le trajet est là**, sans attendre la fin du
  défilement — celui-ci n'est qu'un habillage, pas un calcul.
  `fileCompositionActive` empêche le sondage d'état de redessiner la file
  par-dessus l'animation.

Côté moteur, la jauge de la première phase est déterminée : `Graphe`
gagne `construire_suivi`, qui incrémente un `AtomicUsize` par empreinte
balayée (`construire` délègue avec un compteur jeté — CLI et tests
inchangés). `Etat` porte `graphe_fait`/`graphe_total` (`total == 0` : rien
en cours), lus par la commande `graphe_progress` que l'interface sonde à
5 Hz pendant l'attente. Le graphe déjà en cache : `preparerGraphe` rend
aussitôt, on saute droit à la composition.

### Actualiser les passes une à une, depuis l'Aperçu — 30 août 2026

L'histogramme du tempo du mode Bibliothèque semblait périmé : creux, calé sur
3 199 morceaux quand la sauvegarde `rusty-music.avant-remesure-tempo` du
20 août en portait 15 838. En réalité rien de périmé — `library_stats` relit
la base à chaque entrée dans le mode, sans cache. C'est la base qui était
incomplète : la remesure du 20 août avait été lancée avec « tout refaire »,
donc `start_descripteurs { force: true }` a d'abord appelé
`effacer_descripteurs()` (les 15 847 lignes), puis la passe s'est arrêtée à
3 200 (app fermée). Aucune reprise au démarrage, et le seul moyen de la
relancer était le bouton « Analyser » d'une racine, tout en bas des réglages
— qui rejoue *les quatre* passes à la suite. L'humeur (tempo × énergie)
affichait 23 845 « non mesurés » pour la même raison : `stats_humeur` compte
non mesuré dès que `bpm` **ou** `energy` est NULL.

Nouveau panneau **Actualiser** dans l'Aperçu, à côté de **Complétude** qui en
montre le besoin. Un bouton **Tout actualiser** rejoue la chaîne entière
(scan → empreintes → descripteurs → genres) comme « Analyser » d'une racine,
mais sur toutes les racines ; quatre boutons en dessous — *Scanner les
dossiers*, *Empreintes*, *Tempo · tonalité · énergie*, *Genres MusicBrainz* —
relancent une seule passe (`lancerPasse` dispatche vers `passeScan` /
`passeEmpreintes` / `passeDescripteurs` / `passeGenres`). S'y ajoutent la case
« Refaire aussi ce qui est déjà mesuré » et le champ contact MusicBrainz,
déplacés depuis le bloc « Source de la bibliothèque ». Le scan, seule passe à
prendre une racine, est rejoué sur chacune à la suite.

Avancement visible à chaque tour de sonde (`avancementActu`) : barre `<progress>`
graduée dès qu'un total est connu (empreintes, descripteurs, genres —
`faits / total (NN %) — reste ~Xh`), texte seul pour le scan qui n'annonce pas
de total (compte qui monte). Pendant « Tout actualiser », chaque ligne est
préfixée de l'étape en cours (« Étape 3/4 — … »). Un « démarrage… » est posé
avant le premier `invoke` pour qu'il n'y ait pas de trou muet.

`verrouillerActualisation` grise les boutons de l'Aperçu **et** les
« Analyser » des racines pendant qu'une passe tourne (une seule de chaque
sorte côté moteur). `reprendreActualisationEnCours`, appelé à l'entrée du
mode, raccroche l'affichage à une passe déjà en vol. Le bouton « Analyser »
d'une racine et sa jauge `racines-jauge` restent en place, inchangés.

### Mode Découvrir — le fil d'actualité — 30 août 2026

Le mode Découvrir n'était qu'un explorateur manuel : on cherchait un artiste,
on voyait ses collaborateurs MusicBrainz (`artist_links`), on naviguait le
graphe. Il lui manquait l'actualité — ce qui vient de sortir.

**Trois flux, reconstruits à partir des dates, faute de « news » libre.**
Aucune API ne rend un flux d'actualité musicale exploitable ; on la
reconstitue.

1. **Sorties récentes** — *un seul* appel à `explore/fresh-releases` de
   ListenBrainz (CC0) rend les sorties de la planète sur la fenêtre demandée
   (~8 000 pour 30 jours) ; on les croise avec les identifiants d'artistes de la
   bibliothèque. On garde Album/EP/Single, sans type secondaire disqualifiant
   (Live, Compilation, Remix…), datées des 30 derniers jours. Dates partielles
   (« 2026 », « 2026-08 ») complétées par `musicbrainz::completer_date` en
   `YYYY-MM-DD`, qui se compare et se trie comme une chaîne — SQLite
   `date('now', ?)` tient l'horloge, pas de crate de calendrier. Plafond de
   4 sorties par artiste et par passe : Buckethead publie une douzaine d'EP le
   même jour.
2. **Collaborations** — pas de flux dédié : une sortie créditée à plus d'un
   artiste (`artist_mbids` de longueur > 1). La colonne `collaborateurs` de
   `decouvrir_sorties` porte le libellé du crédit ; non vide, la sortie va dans
   la section « Collaborations » plutôt que « Sorties récentes ».
3. **Artistes voisins** — les candidats ne sont pas dans la bibliothèque, donc
   pas d'empreinte : la carte du mode Explorer ne s'applique pas. On prend
   `labs.api.listenbrainz.org/similar-artists` (agrégé sur les écoutes de la
   communauté), un appel par artiste, avec repli sur le graphe de collaboration
   déjà en base (`artist_links`, aucun réseau) pour ceux que ListenBrainz ne
   couvre pas. On écarte les artistes qu'on possède déjà.

**Pas d'historique d'écoute.** La reco n'est donc pas personnalisée par les
écoutes ; le classement se fait sur le nombre de portes d'entrée (combien
d'artistes connus mènent à un voisin) et la récence.

**Le piège « Various Artists ».** Une première version interrogeait MusicBrainz
artiste par artiste (`release-group?artist=…`) pour toute la discographie, puis
filtrait sur la date. L'artiste #1 de la bibliothèque de test par nombre de
morceaux est *Various Artists*, crédité sur des dizaines de milliers de
compilations : la pagination — 100 par page, une requête par seconde — ne
finissait jamais, la passe restait muette et l'interface aussi. Corrigé sur
deux fronts : `musicbrainz::albums_artiste` plafonne à 600 albums (utile aussi
pour la passe *genres*), et le mode Découvrir écarte les artistes spéciaux de
MusicBrainz (`ARTISTES_SPECIAUX` : `[unknown]`, `[traditional]`…). Surtout, la
passe ne parcourt plus les discographies : `fresh-releases` fait le travail en
une requête.

**La passe (`core::decouvrir::actualiser`)** est calquée sur `enrichir` :
additive, reprenable, `Bilan` + callback d'avancement. Le suivi des voisins
(`decouvrir_suivi`, `kind = "voisins"`) fonctionne comme `mb_fetched` mais
**périme** à 30 jours — la similarité bouge lentement ; l'étape sorties pose sa
propre marque (`@passe`). `decouvrir_elaguer` retire du fil ce qui a plus d'un
an.

Coût mesuré sur la bibliothèque réelle : ~1 min 30 pour une passe complète
(60 artistes-voisins × 1 s + un `fresh-releases`), 61 sorties retenues sur
30 jours. En régime établi (voisins périmés à 30 j) la passe se réduit au seul
appel `fresh-releases`, quelques secondes.

**Interface.** Commandes Tauri sur le modèle du trio enrichissement
(`start_decouvrir` / `decouvrir_state` / `decouvrir_feed` / `decouvrir_tout_vu`).
À l'entrée dans le mode, le fil s'affiche tel quel puis une passe se relance si
la dernière date de plus de 12 h **et** qu'une adresse de contact est
renseignée ; un bouton « Actualiser » force la passe. **Barre de progression**
`<progress>` (même style que l'Aperçu) pilotée par `decouvrir_state`
(`artistes / total`, ETA), plus une ligne de texte — sans quoi un clic sur
« Actualiser » ne montrait rien pendant une minute.

**Trois onglets, pas trois sections empilées.** Le défilement d'un long fil
mélangeait sorties, collaborations et voisins. On garde le même sélecteur
`.segments` que le rail (Sorties · Collaborations · À écouter ailleurs), avec
le compte sur chaque onglet, collant en haut au défilement ; l'onglet ouvert
est mémorisé (`localStorage`). Les cartes deviennent des **lignes compactes** :
pochette 44 px à gauche (Cover Art Archive, `front-250`, via une commande
`decouvrir_pochette` qui met en cache disque comme `cover` — la CSP interdit
les images distantes), titre, « artiste · type · il y a N jours », puis deux
liens sortants — **MusicBrainz** (la page du release-group) et **Last.fm** (la
page de l'album). Le nom d'artiste est cliquable et rouvre l'explorateur de
collaborations. `tauri-plugin-opener` pour les liens : la webview ne navigue
pas dehors.

CLI : `rusty-music decouvrir --contact … --jours 30`.

Schéma : `decouvrir_sorties`, `decouvrir_voisins`, `decouvrir_suivi` — trois
tables neuves, aucune migration de colonne.

### Deux pièges de la webview Tauri, rencontrés en chemin — 30 août 2026

**L'interface servie était systématiquement périmée.** Tauri renvoie les
fichiers embarqués **sans en-tête de fraîcheur** ; WKWebView les met alors en
cache disque et continue de servir l'`index.html` d'un ancien build, même après
recompilation — la webview affichait trois modes quand le code en portait cinq.
Et le `build.rs` du dépôt, censé forcer la reconstruction quand `ui/` change, ne
suffisait pas : `cargo:rerun-if-changed` relance le *script*, pas la compilation
de `main.rs` où vit `generate_context!`. Deux correctifs :

- `build.rs` hache tout `ui/` et publie le hachage en `cargo:rustc-env` ;
  `main.rs` le lit (`const _UI_HASH = env!(…)`), donc tout changement d'interface
  invalide `main.rs` et ré-embarque les assets. Plus besoin de
  `cargo clean -p rusty-music-desktop`.
- La fenêtre est bâtie en Rust (plus dans `tauri.conf.json`) pour lui accrocher
  `on_web_resource_request` : `Cache-Control: no-store` sur le protocole
  `tauri://`. Les assets sont en mémoire, aucun intérêt à les cacher. Une purge
  unique de la webview au premier lancement (marqueur `.webview-purgee-v1`)
  efface l'entrée périmée héritée des anciens builds.
- `RUSTY_MUSIC_INCOGNITO=1` : webview non persistante + fenêtre au premier plan,
  pour tester la version courante sans cache ni conflit avec l'app installée.

**Grille d'albums par-dessus le fil.** `charger("albums")` tourne en fond au
démarrage et se termine parfois *après* `basculerMode` (mode imposé par
`RUSTY_MUSIC_MODE`, ou clic rapide) : son `poser("albums", …)` remontrait la
grille et réécrivait le titre. `poser` respecte maintenant `modeCourant` — hors
mode Écoute, il met à jour les données sans toucher à la vue centrale.

### La correction d'octave descendante rabaissait la moitié de la bibliothèque — 1er septembre 2026

Après avoir basculé les données de la carte SD vers le SSD interne (pour
échapper aux blocages de lecture) et reconstruit la bibliothèque, l'histogramme
de tempo du mode Bibliothèque s'est révélé écrasé vers 40-70 BPM.
L'histogramme n'était pas périmé — `library_stats` relit la base à chaque
affichage — et la base était complète (27 118 / 27 170 morceaux mesurés).
C'était l'estimateur qui déraillait.

**Diagnostic par comparaison de sauvegardes.** La base du 20 août (ancien
estimateur, avant la correction bidirectionnelle) piquait vers 100-140 BPM,
rien sous 60. Les bases du 31 août et du 1er septembre piquent vers 40-90 avec
une falaise nette à 90. Jointure morceau à morceau, chemins normalisés SD→SSD,
15 838 paires : **55 % des morceaux divisés par ~2**, ratio moyen 0,75.

**Cause : le discriminant du sens descendant ne discriminait rien.**
`descripteurs::tempo()` divisait le gagnant de grille par deux dès que
`brut(bpm / 2) ≥ brut(bpm) * 0,85`. Or `brut(bpm / 2)` échantillonne
l'autocorrélation en 2T, 4T, 6T — tous des pics réels d'un train périodique de
période T. Le ratio vaut ~1 par construction, le seuil de 0,85 passe presque
toujours, la division se déclenchait sur quasi tout morceau au gagnant ≥ 90 BPM.
Le comble : même une comparaison `correle(env, 2d)` contre `correle(env, d)`
est biaisée — la quantification en trames d'une période fractionnaire fait
tomber `2d` sur un meilleur calage que `d` (un train de clics parfaitement
régulier donnait déjà un rapport de 1,22, mesuré).

**Correctif : `alternance_dents()`.** Le vrai signe qu'un gagnant est une
subdivision, c'est que les attaques *alternent* fort/faible (guitare en
boom-chick, charleston sur les croches). On pose un peigne à la période du
gagnant, on cherche sa meilleure phase, et on compare l'énergie moyenne des
dents de rang pair à celle des dents de rang impair. Même période, même
interpolation pour les deux familles de dents : le biais de quantification
s'annule dans le rapport. Un temps sur deux nettement plus faible (rapport
`faible / fort` sous `SEUIL_SOUS_OCTAVE`, calé à 0,60) → division par deux.
C'est la mécanique de `battements.rs::ramasse`, réutilisée ici. Le sens montant
(`corrige_vers_le_haut`, cas Watcha) est inchangé ; `SEUIL_PLANCHER_MI_DECALAGE`
disparaît, le nouveau test rejetant nativement le train de clics régulier.

**Validation — `examples/octave_avant_apres.rs`, 300 morceaux au hasard.**
Concentration dans [40, 90] BPM : **75,7 % → 39,3 %**. Concentration dans
[150, 267] : 11,7 % → 18,3 %. 150 morceaux déplacés, quasi tous ×2,00 exact,
et la liste inspectée est saine : Soundgarden « Kickstand » 46→92, RHCP
« Backwoods » 56→111, Killing Joke 47→94. Aucune ballade lente doublée à tort.
Le résidu de 39 % dans [40, 90] mélange des erreurs d'octave non rattrapées et
de la musique réellement lente — indépartageable sans vérité terrain. Les
66 tests de `rusty-music-analysis` passent.

**Reste à faire** : repasser les 27 000 morceaux (passe *Tempo · tonalité ·
énergie*, « refaire ce qui est déjà mesuré ») et vérifier que l'histogramme se
recentre. `SEUIL_SOUS_OCTAVE` est le seul réglage à bouger si le bilan global
penche encore.

### Filtrer le fil par famille — mode Découvrir — 1er septembre 2026

Même légende que l'Explorer (`rendreFamilles`), même multi-sélection que le
filtre de l'Écoute, dans un bloc « Familles » du rail. Cochée, une famille
restreint les trois onglets du fil (Sorties, Collaborations, À écouter
ailleurs) aux entrées dont l'**artiste-ancre** — celui de la bibliothèque qui
a fait remonter la sortie ou le voisin — appartient à cette famille. Plusieurs
familles cumulent (OU), rien de coché montre tout, « Toutes les familles »
efface. L'explorateur de collaborations reste non filtré : ses artistes
viennent de MusicBrainz et n'ont pas de famille.

Commande `artist_families` → `Library::familles_des_artistes` : pour chaque
`mb_album_artist_id`, le cluster de la carte le plus représenté parmi ses
morceaux projetés, égalité tranchée par le plus petit numéro. Même arbitrage
que `familles_des_albums`.

`VoisinFil` gagne un champ `src_mbids` (les ancres du voisin, agrégées par
`GROUP_CONCAT(DISTINCT v.src_mbid)`) — un voisin passe si l'une d'elles est
cochée. Les sorties portaient déjà `artiste_mbid`.

Côté interface, `filDecouvrir` garde le dernier fil reçu ; `rendreFilDecouvrir`
applique le filtre au vol sans refaire la requête. Les compteurs d'onglet
suivent le filtre ; « Pas encore de nouveautés » et « Tout marquer comme vu »
restent calés sur le fil brut. Mapping rechargé et filtre vidé après tout
recalcul du clustering (`familleARecalculee`).

### Popularité générale — sonde puis socle ListenBrainz + Deezer — 2 septembre 2026

Demande de l'utilisateur : une popularité de morceau qui ne vienne **pas** d'un
compteur local mais de la notoriété dans le monde, récupérée à l'analyse et
montrée en jauge dans la file et les listes de pistes. Plan complet dans
`docs/popularite.md`.

**Phase 0 — sonde** (`experiments/popularite/`, Python, cache versionné). 200
morceaux au hasard, ListenBrainz par MBID et Deezer par recherche. Verdict :
ListenBrainz **97 %** de couverture (enregistrement ou album), uniforme sur
toute la longue traîne ; Deezer **80 %** au niveau piste, **99 % de
rapprochements justes à condition d'exiger artiste *et* titre concordants**
(la sonde ne vérifiait d'abord que l'artiste : ~2 % de faux, plus quelques %
de mauvais morceau du bon artiste). Accord LB ↔ Deezer ρ ≈ 0,55 — modéré, donc
complémentaires. Spotify/YouTube écartés : ils exigent une clé et ne
combleraient que 1,5 % de trous.

**Phase 1 — socle.** Trois tables (`popularite` brute par source,
`popularite_fetched` avec péremption façon `decouvrir_suivi`, `track_popularite`
recalculée en bloc). Client Deezer neuf ; ListenBrainz gagne le POST
(`/1/popularity/*`, lots de 60). Passe `popularite::actualiser` sur le patron
d'`enrichir` : reprenable, additive, un échec réseau n'interrompt rien.

La **valeur affichée** n'est pas un compte — les échelles vont de 1 à 1,7 M
d'écoutes et le `rank` Deezer est borné et planchéré. C'est un **rang
percentile** par source (part des morceaux strictement moins écoutés), puis la
**médiane** des rangs des sources disponibles. Repli enregistrement →
release-group (lien titre normalisé, comme les genres) → pas de ligne, donc
grisé.

Étape **5/5** de la chaîne d'analyse (`analyserRacine`, `lancerChaineComplete`),
sans condition d'adresse — ListenBrainz et Deezer sont des API publiques, le
contact ne sert qu'au `User-Agent`. Mesuré : ~10 min ListenBrainz + ~2 h Deezer
(une recherche par enregistrement) sur 27 000 morceaux, du même ordre que la
passe de genres, et reprenable. `rusty-music popularite` côté CLI.

**Phase 2 — la jauge.** Composant `.jauge-pop` : cinq segments façon indicateur
de signal, `round(relative × 5)` pleins, mais **au moins un dès qu'une mesure
existe** — sans ça « connu et peu populaire » se confondrait avec « inconnu »,
qui se rend en contour seul (jamais une valeur inventée, comme les descripteurs
de l'inspecteur). Infobulle : « popularité : élevée · mesurée sur le morceau »
(ou « sur l'album » quand on est retombé sur le release-group).

Commande `popularites { ids }` séparée, comme `descripteurs` — la popularité ne
vit pas dans `TrackRow`, l'interface la demande par lot pour ce qu'elle
affiche. Cache `popParPiste` côté `app.js` ; `chargerPopularites` ne repeint
que s'il y avait quelque chose à charger, sinon le repaint la rappellerait sans
fin. Câblée dans la file d'attente (`dessinerFile`) et les listes de morceaux
(`ligne`, pour `vue.quoi === "pistes"` — pistes d'un album et résultats de
recherche). `popARecalculee` vide le cache et recharge après la passe.

Pas encore vue à l'œil dans la webview (l'extension navigateur n'était pas
connectée, et une WKWebView ne se pilote pas de l'extérieur) : `jaugePop` et le
cache sont testés hors interface, tout compile, les tests core passent.

**Phase 3 — rafraîchissement.** (Last.fm ayant été écarté, la phase Last.fm du
plan initial saute.) Le paramètre `depuis` de `popularite::actualiser` était
déjà là depuis la phase 1 ; `start_popularite` gagne un booléen `rafraichir`
qui pose `depuis = now − 90 j`. `pop_recordings_candidats` et `pop_deja_fait`
reprennent alors tout ce qui dépasse ce délai — une notoriété bouge lentement,
90 j est large.

Case « Rafraîchir aussi la popularité de plus de 90 jours » dans le rail, à
côté de « Refaire ce qui est déjà mesuré », décochée par défaut : rafraîchir
coûte jusqu'à deux heures (Deezer), l'utilisateur choisit. `popularite_fraicheur`
rend `(couverts, plus_ancienne_at, perimes)` ; la ligne d'alerte du mode
Bibliothèque n'apparaît que si de la popularité existe **et** qu'une partie
dépasse 90 j, avec un bouton « Rafraîchir » qui coche la case et lance la passe
seule. Rien d'automatique — convention du projet.
