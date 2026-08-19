# Rusty Music — suite musicale locale

Logiciel de bureau autonome et local pour **écouter, explorer et éditer** une bibliothèque musicale. Point d'entrée unique : un répertoire de musique, scanné puis surveillé automatiquement.

Voir `CLAUDE.md` pour le contexte de travail et `docs/` pour les spécifications.

## État

| Brique | État |
|---|---|
| Cœur d'ingestion (dossier surveillé, tags, base) | **compile et tourne** — validé sur 27 044 morceaux, voir ci-dessous |
| Module 1 — Lecteur | **v1 en place** — moteur (`crates/player/`) et fenêtre Tauri (`apps/desktop/`) |
| Module 2 — Exploration (carte 2D) | **livré** — 27 042 des 27 044 morceaux sur la carte |
| Module 3 — Éditeur / MAO | **démixage fonctionnel** — voir « Démixage » ci-dessous |

## Structure

```
CLAUDE.md              contexte permanent (lu par Claude Code à chaque session)
docs/                  spécifications détaillées
crates/core/           cœur d'ingestion : scan, tags, surveillance, base SQLite
crates/player/         lecture audio (module 1) : sortie, transport, file, onde
crates/analysis/       empreintes, projection, familles, chemins (module 2)
crates/editor/         démixage en stems (module 3)
crates/cli/            binaire `rusty-music` — pilote le cœur sans interface
apps/desktop/          application Tauri — modes Écoute et Explorer
models/                modèle ONNX CLAP (112 Mo, hors dépôt)
ui/prototype/          maquette HTML du modèle de navigation retenu
```

## Démarrer

```bash
cargo run -p rusty-music-cli -- scan  ~/Musique     # ingestion initiale
cargo run -p rusty-music-cli -- watch ~/Musique     # scan puis surveillance continue
cargo run -p rusty-music-cli -- stats               # état de la bibliothèque
cargo test                                    # tests du cœur
```

`-j N` fixe le nombre de threads de lecture des tags (défaut : nombre de
cœurs). Sans effet sur un support lent — voir « Pourquoi le parallélisme ne
change rien ici ».

Consultation de la base (ce que les modules liront via `Library`) :

```bash
cargo run -p rusty-music-cli -- artists                        # artistes et volumes
cargo run -p rusty-music-cli -- albums --artist Radiohead      # albums, filtrables
cargo run -p rusty-music-cli -- tracks "OK Computer" --artist Radiohead
cargo run -p rusty-music-cli -- search "bjork"                 # accents repliés
cargo run -p rusty-music-cli -- roots                          # dossiers surveillés
cargo run -p rusty-music-cli -- forget /ancien/dossier         # change de source
```

`forget` retire la racine **et les morceaux qui en dépendent** : c'est
l'opération « changer la source de la bibliothèque » des futurs réglages.

La recherche passe par un index FTS5 (`tokenize="unicode61 remove_diacritics 2"`)
tenu à jour par déclencheurs : « bjork » trouve « Björk », « kanan » trouve
« Kanañ a ri! ». Elle porte sur des mots entiers, le dernier valant préfixe
pour rester utilisable au fil de la frappe.

`scan --force` relit les tags de tous les fichiers, même ceux que la taille et
la mtime disent inchangés. À utiliser après avoir enrichi ce que l'on extrait
des tags : les fichiers n'ayant pas bougé, le chemin incrémental les sauterait
tous et les nouvelles colonnes resteraient vides.

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

Lecture audio et pochettes :

```bash
cargo run -p rusty-music-cli -- play "karma police"              # 1er résultat
cargo run -p rusty-music-cli -- play --album "OK Computer" --artist Radiohead
cargo run -p rusty-music-cli -- play "airbag" --seconds 10       # coupe après 10 s
cargo run -p rusty-music-cli -- cover "karma police" --out /tmp/pochette.jpg
```

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

Les 10 fichiers Opus sont un seul album (Fingathing). Les couvrir demanderait
un décodeur hors `symphonia` (liaison C vers libopus) : disproportionné pour
0,04 % de la bibliothèque, à revoir si la proportion change.

La base est créée dans `./rusty-music.db` (modifiable avec `--db`).

Deux maquettes dans `ui/prototype/`, à ouvrir dans un navigateur (données
fictives) : `maquette-navigation.html` fixe la structure (modèle « Atelier »
retenu), `Directions visuelles - carto.fm.html` propose trois directions
typographiques et chromatiques — **le choix n'est pas fait**.

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
── 01 Hard as a Rock — drums.wav
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
« Children's · Pop » la famille de Regina Spektor, Agnes Obel et Nina Simone —
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
  à deux pour écarter l'`amapiano` posé par un contributeur unique sur Yann
  Tiersen. Mesuré, il faisait l'inverse : chez cet artiste `modern classical`,
  `neoclassicism`, `minimalism` et `instrumental` portent eux aussi une seule
  voix, et le seuil ne gardait que `rock`. Coût général : **55 % de couverture
  au lieu de 74 %**, et 360 artistes rendus muets. Ce qui règle vraiment le cas
  est le **départage à votes égaux par le nombre d'artistes qui portent le
  genre** — `amapiano` n'en a qu'un dans toute la bibliothèque, `instrumental`
  dix-sept ;
- **le titre d'album est normalisé** avant rapprochement — nos fichiers ne
  portent pas d'identifiant d'album, et « In Utero (super deluxe) » doit
  retrouver « In Utero ».

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
  carte** : mesuré sur un trajet Bob Marley → Metallica en 8 étapes, l'écart
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

## Démixage (module 3)

Les poids ne sont pas dans le dépôt — 84 Mo. Une fois :

```bash
./scripts/preparer-demucs.sh
cargo run --release -p rusty-music-cli -- demix "karma police" --seconds 30
cargo run --release -p rusty-music-cli -- demix /chemin/vers/morceau.flac --out ./stems/
```

Quatre WAV en sortie — batterie, basse, autre, voix — en PCM 16 bits 44,1 kHz.

Trois variantes, à récupérer séparément (`./scripts/preparer-demucs.sh <nom>`) :

| variante | poids | stems | vitesse |
|---|---|---|---|
| `htdemucs` (défaut) | 84 Mo | 4 | 7,8 × le temps réel |
| `htdemucs_6s` | 84 Mo | 6 — ajoute guitare et piano | 7,0 × |
| `htdemucs_ft` | 333 Mo | 4, un réseau par stem | ~4 × plus lent |

Le six-stems n'est pas seulement « plus de stems » : sur l'intro de « Karma
Police », le quatre-stems verse piano et guitare dans `other` (RMS 0,090) faute
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

30 s de « Karma Police », sur Metal :

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

## Suite

**Le plan détaillé vit dans `docs/suite.md`** — ordre, raisons, dettes connues.
Ci-dessous, le séquencement de principe.

Séquencement de référence (`CLAUDE.md`, `docs/modules.md`) : cœur → **Module 1
(Lecteur)** → Module 2 → Module 3. Le lecteur passe en premier parce qu'il
valide le cœur et donne vite un livrable utilisable.

Les quatre points qui figuraient ici sont faits : la passe d'analyse complète et
`project`, les questions ouvertes d'`ui-spec.md` (lasso et chemin depuis la
recherche), et le module 3, dont le périmètre de `docs/ui-spec-editeur.md` est
couvert — démixage, vitesse, hauteur, réglage par stem, greffe, export.

Ce qui vient ensuite, dans l'ordre de `docs/suite.md` :

1. **Nommer les familles par l'audio** — **sondé le 18 août**
   (`experiments/clap-texte/`), et le résultat demande un choix. La tour texte
   de CLAP s'importe du premier coup, mais elle pèse 501 Mo et ne tourne pas
   sur wgpu ; pour nommer les familles, **102 Ko de table précalculée
   suffisent** et rendent sept familles sur douze mieux nommées, deux fausses.
   Ce qui marche le mieux n'est pas ce qu'on cherchait : la recherche par
   description, qui est excellente et coûte les 501 Mo.
2. **Mixage de deux pistes** — prérequis manquant et confirmé manquant : une
   grille de battements, pas seulement un tempo. C'est le même manque qui
   empêche la greffe d'aligner les temps forts.
3. Hors périmètre de la v1 du lecteur : aléatoire et répétition,
   réordonnancement de la file.

## Licence

MIT (voir `LICENSE`) **tant que le dépôt n'embarque que des dépendances
permissives**, ce qui est le cas aujourd'hui.

**Aucune licence n'est exclue.** Un outil sous GPL ou AGPL qui rend le service
attendu se prend, et c'est la licence du projet qu'on adapte pour rester
compatible — lier du GPL-3.0 imposerait GPL-3.0 au tout, de l'AGPL-3.0
imposerait l'AGPL. Le changement se fait au moment où la dépendance est prise,
pas par anticipation. Seule exigence de fond : le résultat reste ouvert.

Cela concerne surtout le tempo, les battements et la tonalité (`aubio`,
`QM-DSP`, `Essentia`), prérequis manquant du mixage. Voir `CLAUDE.md`.
