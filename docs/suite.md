# Ce qui reste, et dans quel ordre

État au 17 août 2026. Ce document remplace la section « Suite » du README dès
qu'il s'agit de séquencer ; le README garde les chiffres, celui-ci garde
l'ordre et les raisons.

## Où en est le projet

| Brique | État |
|---|---|
| Cœur d'ingestion | livré — 27 044 morceaux, scan et surveillance, décodage Opus |
| Module 1 — Lecteur | v1 livrée ; restent aléatoire/répétition et réordonnancement de la file |
| Module 2 — Exploration | 27 042 morceaux, 4 modes de chemin, lasso, familles nommées, **carte colorable par tempo et énergie** |
| Module 3 — Éditeur | **le périmètre de `ui-spec-editeur.md` est couvert** : démixage, vitesse, hauteur, réglage par stem, greffe, export |
| Métadonnées enrichies | genres MusicBrainz livrés ; **descripteurs audio livrés** ; restent pochettes et bios |

## Les dettes connues

Elles ne sont pas des fonctionnalités, mais elles bloquent ou fragilisent.

### D1. L'application ne tourne que depuis le dépôt — **réglé le 17 août**

Résolu : résolution partagée dans `crates/core/src/modeles.rs`, ressources
déclarées dans `tauri.conf.json`, `.app` et `.dmg` produits et vérifiés
fonctionnellement. Le détail, et les deux pièges silencieux rencontrés, sont
dans le README. Ce qui suit décrit ce qui n'allait pas.


Deux chemins la clouent à la machine de développement :

- les poids de CLAP sont trouvés par `env!("RM_POIDS")`, un chemin **absolu**
  vers l'`OUT_DIR` du build. Il n'existe sur aucune autre machine ;
- ceux du démixage par `Path::new("models")`, **relatif au dossier courant**.
  Un `.app` lancé depuis le Finder a pour dossier courant `/`.

Tant que ça n'est pas réglé, rien ne sort d'ici : ni une distribution, ni même
un lancement par double-clic. C'est peu de travail et ça conditionne tout le
reste.

### D2. Le noyau wgpu fautif — **cherché le 17 août, introuvable**

Les sept opérateurs que HTDemucs emploie et que CLAP n'emploie pas ont été
isolés un par un et comparés à ONNX Runtime, aux petites dimensions puis à
celles de HTDemucs : **tous justes**, écart maximal 4,1 × 10⁻⁷
(`experiments/wgpu-noyaux/`). L'hypothèse « un noyau cassé » est réfutée.

Le bénéfice visé est tout de même acquis : le module 2 tourne sur ce backend
avec un sous-ensemble de ces opérateurs, désormais vérifiés individuellement.
On est passé de « ça marche » à « ça marche, et voici les contrôles ».

Ce qui reste ouvert : **d'où vient alors l'écart de 33 % sur HTDemucs.** Le
trancher demanderait de bissecter le vrai graphe — ajouter des sorties
intermédiaires et remonter au premier point de divergence. Chantier à part,
sans urgence : la voie d'import concernée n'est plus empruntée, et les deux
modèles en production sont vérifiés.

### D3. Treize morceaux hors carte — **réglé le 18 août, il en reste deux**

Les 10 opus sont sur la carte. symphonia ne décode pas ce format ;
`crates/core/src/opus.rs` comble ce seul trou avec `ogg` (BSD-3) et
`opus-decoder` (MIT/Apache), **portage pur Rust de libopus, sans `unsafe` ni
FFI**. Le crate `opus` officiel aurait compilé libopus depuis ses sources et
imposé `cmake` à quiconque construit le projet — écarté pour cela, pas pour sa
licence.

**27 042 des 27 044 morceaux** sont désormais placés. Les deux restants — un
m4a et un mp3 — sont réellement corrompus, aucun décodeur n'y changera rien.

### D4. Le module 3 n'a pas de spécification d'interface — **réglée le 18 août**

`docs/ui-spec-editeur.md`. Le périmètre est tranché : **une piste à la fois** —
ouvrir, séparer, retoucher, exporter. Ni session multipiste, ni projet
sauvegardé, ni mixage de deux pistes. Les stems écrits sur le disque sont la
seule persistance, ce qui évite d'inventer un format de projet.

Conséquence à noter : **le mixage DJ sort du module 3.** Il redevient un
chantier à part, s'il se fait.

## Le plan proposé

### 1. Rendre l'application autonome — **fait**

Régler D1, puis produire un vrai paquet.

- les poids vont dans le dossier de ressources de l'application, et
  `Demixeur`/`Embedder` les y cherchent d'abord, avec repli sur `models/` pour
  le développement ;
- `tauri.conf.json` déclare ces ressources dans son `bundle` ;
- vérification : lancer le `.app` depuis le Finder, démixer un morceau.

**Pourquoi d'abord.** C'est la seule tâche dont dépend la possibilité même de
montrer le résultat à quelqu'un, et elle est courte. Tout ce qu'on ajoutera
ensuite héritera de ce socle.

### 2. Isoler le noyau wgpu — **fait, résultat négatif**

Quatorze cas, sept opérateurs, deux échelles, deux backends : aucun n'est
fautif. Pas de ticket en amont — il n'y a rien à signaler. Le détail et la
suite possible sont dans `experiments/wgpu-noyaux/README.md`.

### 3. Fermer le module 2 — **fait**

Les deux questions ouvertes d'`ui-spec.md` sont tranchées et implémentées ; le
détail des décisions est dans cette spec, section « Tranché le 17 août ».
Reste, sans urgence, l'autocomplétion multi-type de la recherche — la voie
« recherche unique » est retenue de fait.

### 4. Spécifier le module 3 — **fait le 18 août**

`docs/ui-spec-editeur.md`. Les trois questions qui bloquaient sont tranchées :
**une piste**, pas de session ; **pas de projet sauvegardé**, les stems sur le
disque suffisent ; **time-stretch global par défaut**, par stem en option, avec
l'avertissement que des facteurs différents désynchronisent.

La spec relève au passage une fuite silencieuse : **124 Mo par jeu de stems**,
555 Mo déjà accumulés, et rien ne le dit à l'utilisateur ni ne le range.

### 5. Time-stretch et pitch — **fait le 18 août**

**`wsola`, pas un vocodeur écrit à la main** — et c'est une correction. J'avais
écrit un vocodeur de phase complet (verrouillage sur les crêtes, cinq cents
lignes) avant de vérifier ce qui existait. `wsola` fait mieux : recouvrement-
addition par similarité de forme d'onde, la méthode d'`atempo` chez ffmpeg et de
VLC — temporelle, donc sans artefact de phase — **conçue pour le temps réel**
(`push`/`pull`, `set_tempo` en direct), pure Rust, 468 lignes, zéro dépendance
transitive, MIT.

Mesuré avant adoption : hauteur inchangée à 441 Hz de 0,5× à 2×. La
transposition, que `wsola` ne fait pas, s'obtient en étirant puis en
rééchantillonnant.

**Livré** : `crates/editor/src/etirement.rs` (259 lignes, dont la transposition
et le rééchantillonneur), la vitesse de lecture immédiate dans `multipiste.rs`,
la commande `rusty-music etirer` et `cout_etirement` pour la mesure.

**Coût mesuré du changement** : 17,9 s contre 0,84 s pour étirer un stem de
184 s hors ligne. Sans effet sur la lecture ; sensible sur la transposition, qui
reste cependant mise en cache par réglage.

**Le risque d'artefacts est levé par le changement de méthode** : WSOLA est
temporel, il n'a pas de phase à recoller. Reste à juger à l'oreille sur des
stems de batterie, où toute méthode d'étirement se trahit.

**Branché dans le dock** : deux pas-à-pas, vitesse et tonalité, globaux. Chaque
combinaison est mise en cache sur le disque, donc l'aller-retour entre deux
valeurs est immédiat. ~~Reste le réglage **par stem**.~~ Fait le même jour, voir
ci-dessous.

### 5 bis. Réglage par stem et greffe — **fait le 18 août**

Les deux dernières lignes de `ui-spec-editeur.md` ont été écrites ensemble, et
ce n'est pas un hasard : **la greffe ne cale pas les temps forts, et le réglage
par stem est ce qui permet de les caler à la main.**

**Le réglage par stem.** Chaque ligne du dock s'ouvre sur sa vitesse et sa
hauteur, plus « suivre l'ensemble ». L'avertissement de désynchronisation est
tenu par le code : revenir à l'ensemble réaligne les stems, sans quoi on
arrêterait la dérive en gardant l'écart déjà pris.

**La greffe** (`crates/editor/src/greffe.rs`, 7 tests). Mettre à la place d'un
stem celui d'un autre morceau demande trois choses :

1. **le tempo** — on étire du rapport des deux, **replié à l'octave** dans
   [1/√2, √2]. Une boucle à 70 BPM sous un morceau à 140 n'a pas à être
   accélérée du double, ses temps tombent déjà un sur deux ; le repliement
   borne l'étirement à ±41 % au pire ;
2. **le départ** — le greffon entre là où l'ancien stem entrait, pas là où
   lui-même commençait. L'attaque se cherche à l'énergie relative : un stem
   séparé n'est jamais exactement silencieux, le modèle y laisse un fond ;
3. **la longueur** — on boucle ou on coupe, avec 20 ms de fondu aux jonctions.

Deux choix qui rendent le geste réversible : **le voisinage vient du morceau
entier** (la bibliothèque n'a d'empreintes que de mélanges ; une par stem
supposerait de démixer les 27 000 morceaux), et **la greffe est un fichier de
plus** sous `greffes/`, jamais une réécriture — rouvrir le morceau retrouve ses
stems séparés.

**Ce qui manque, et qui s'entend** : les temps forts ne sont pas alignés. Il y
faudrait une grille de battements, que `descripteurs.rs` ne calcule pas — il
rend un tempo, pas une phase. C'est le même prérequis que le chantier 8, et il
reste manquant.

### 6. Genres MusicBrainz — **fait**

De `docs/data-sources.md`, **la part « genres » seulement** : c'est le périmètre
retenu. Cover Art Archive et Wikidata — les pochettes et les « infos
artiste/style » que le module 1 promet et que l'inspecteur n'affiche pas —
restent à faire, sur la même mécanique (cache local, débit respecté, reprise).

**Livré le 17 août.** `crates/core/src/musicbrainz.rs` (le client, cadencé),
`crates/core/src/enrichir.rs` (la passe, reprenable), l'arbitrage des trois
sources dans `genres_du_morceau`, et deux commandes : `rusty-music enrich` et
`rusty-music familles`. Dépendance ajoutée : `ureq` (31 crates, contre 106 pour
`reqwest`). Les quatre décisions prises — MusicBrainz d'abord, genres seuls,
échelon artiste + album, tags des fichiers en repli — sont documentées dans le
README.

Ce qui a motivé le chantier, mesuré avant de commencer — voir
`experiments/musicbrainz-genres/`. Les douze familles sont aujourd'hui nommées
par les tags de genre des fichiers, et ces tags décrochent : la famille de
Regina Spektor, Agnes Obel et Nina Simone sort « Children's · Pop ».
Trois chiffres :

- **92,6 % des morceaux portent déjà un `mb_artist_id`** dans leurs tags. Aucun
  appariement flou à écrire : on interroge par identifiant ;
- **75 % des artistes** ont au moins un genre chez MusicBrainz ;
- à score identique, **neuf familles sur douze y gagnent** : « trip hop ·
  downtempo » au lieu de « Pop · R&B », « nu metal » au lieu de « Metal »,
  « boom bap », « ska · reggae ».

Deux précautions que le sondage a révélées : **pondérer par le nombre de votes**
(un tag `amapiano` erroné sur Yann Tiersen suffit à nommer une famille), et
**garder le tag du fichier en repli** — les artistes de chant breton n'ont
aucun genre chez MusicBrainz, et là les fichiers disaient juste.

**Plafond de cette voie** : les genres MusicBrainz sont attachés à l'artiste,
pas au morceau. Bob Marley est dans deux de nos familles et recevra les mêmes
étiquettes dans les deux. D'où le chantier suivant.

### 6 bis. Descripteurs audio — **fait le 18 août**

`ui-spec.md` promettait de colorer la carte par année, tempo et énergie ; la
table `descriptors` du schéma prévoyait les colonnes, et elle était vide. Le
module 2 avait été déclaré clos avec cette promesse non tenue.

Livré : `crates/analysis/src/descripteurs.rs` (416 lignes), la passe
`passe::descripteurs`, la commande `rusty-music descripteurs`, et le rail qui
gagne deux boutons. **Aucune dépendance ajoutée, le projet reste MIT.**

Les algorithmes sont ceux des bibliothèques du domaine, pas des inventions :
flux spectral puis autocorrélation à peigne comme aubio, chroma corrélé aux
profils de Krumhansl-Schmuckler comme QM-DSP.

**Ce que ça vaut, mesuré** : 73 % d'accord avec AudioMuse-AI à 6 % près, 80 % à
l'octave près, sur 197 morceaux. Assez pour colorer une carte ; pas pour caler
deux disques — ce qui tombe bien, le mixage DJ étant sorti du périmètre du
module 3.

**Limite connue** : les seuils de rejet n'écartent que le silence. Sur 336
morceaux mesurés, aucun n'est sorti sans tempo ni sans tonalité — un conte lu
reçoit donc un tempo qui ne veut rien dire. Les relever demanderait une vérité
terrain qu'on n'a pas.

### 7. Nommer les familles par l'audio — **sondé le 18 août**

Ce que ni les fichiers ni MusicBrainz ne donneront jamais : **des descripteurs
qui ne sont pas des genres.** Nos familles sortent d'un regroupement
acoustique ; l'une est « voix féminine, trip-hop et pop-rock », une autre
« chanson acoustique ». Aucune taxonomie de genres ne nomme ça.

Le sondage est fait — `experiments/clap-texte/`. **Le piège attendu n'était pas
le bon**, et le chantier se scinde en deux livrables de prix très différents.

**Ce qui était censé bloquer n'a pas bloqué.** La tour texte s'exporte et
s'importe du premier coup, sans rien figer d'autre que la longueur de séquence
— 613 nœuds repliés à 464, 2 081 lignes de Rust généré, cosinus 0,9999994636
contre PyTorch. C'est un RoBERTa : pas de partitionnement en fenêtres, donc
aucun des `Pad` calculés à l'exécution qui avaient arrêté la tour audio.

**Ce qui bloque vraiment est ailleurs, et c'est trois choses :**

1. **501 Mo**, quatre fois la tour audio ;
2. **elle ne tourne pas sur wgpu** — `burn-cubecl` n'implémente pas
   `bool_from_data`, et le masque d'attention est un booléen. 91 ms par phrase
   sur processeur, ce qui suffit ; mais contrairement à la chasse de
   `wgpu-noyaux`, **il y a ici quelque chose à signaler en amont** ;
3. **pour nommer les familles, on n'en a pas besoin à l'exécution.** Le
   vocabulaire est fixe : ses empreintes se calculent une fois et tiennent dans
   **102 400 octets**.

**Ce que le nommage vaut, mesuré** : sept familles sur douze mieux nommées que
par les genres, trois à égalité, **deux franchement fausses**. Le cas que ce
document citait — Regina Spektor et Nina Simone sous « Folk · Children's » —
est réparé, elles reçoivent « a female singer with a piano » — mais en second
mot, la tête de liste étant « a female choir singing in harmony ». Le ska
français sort « an african percussion ensemble » et le chant breton a cappella
« a man rapping in french ».

Deux conditions découvertes en chemin, et la seconde n'était pas prévisible :

- **le vocabulaire décide, pas le modèle.** Donné à CLAP, le vocabulaire
  d'AudioMuse-AI — les 50 tags Last.fm de MusiCNN, à la lettre — rend « blues »
  et « Mellow » sur presque toutes les familles. Un mot nu est un mauvais
  prompt : CLAP a été entraîné sur des légendes, pas sur des étiquettes ;
- **les scores ne se comparent pas d'une phrase à l'autre.** « a children's
  song » plafonne à +0,74, « a reggae offbeat guitar and bass » à +0,52 :
  l'argmax brut classe les phrases, pas les morceaux. Centrer chaque colonne
  suffit ; réduire en plus sur-corrige.

**Le plafond n'a pas cédé** : une famille que rien dans le vocabulaire ne
décrit ne reste pas sans nom, elle en reçoit un faux. Élargir le vocabulaire a
été testé et ne suffit pas — le score `part × log₂(sur-représentation)`
récompense la phrase rare, et chaque ajout redistribue tout. Le nommage par
centroïde donne des paires plus cohérentes mais fait recevoir la même tête de
liste à deux familles.

**Ce qui marche le mieux n'est pas ce qu'on cherchait** : la recherche par
description est excellente sans réglage — « a symphony orchestra » remonte cinq
Ravel, « a saxophone solo » trois Steve Coleman, « an accordion » Yann Tiersen
et Fred Guichen. C'est aussi le livrable qui coûte les 501 Mo.

### 7 bis. Ce qu'il reste à décider

Le sondage a rendu son verdict ; **le choix est à faire, il n'est pas dicté par
la mesure.**

| | à embarquer | bénéfice |
|---|---|---|
| **nommer les familles** | 102 Ko de table, aucun modèle, aucun tokeniseur | mesuré : 7 mieux, 3 égales, 2 fausses |
| **chercher par description** | 501 Mo, un tokeniseur BPE RoBERTa, processeur seulement | le meilleur résultat de l'essai |

Le premier est presque gratuit et à moitié convaincant ; le second est net et
quadruple la taille de l'application. Rien n'oblige à prendre les deux, ni à
les prendre dans cet ordre.

**Une question d'interface reste ouverte, et elle n'a pas été sondée** : un nom
de famille doit tenir dans une légende, et « a female choir singing in harmony ·
a female singer with a piano » n'en est pas un. La voie évidente est un
vocabulaire de couples — la phrase pour CLAP, un libellé court pour l'écran.

### 8. Mixage de deux pistes — ~1 semaine

Territoire DJ : beatmatching, détection de tonalité, mixage harmonique.
**Prérequis manquant, et confirmé manquant.** On n'a ni tempo ni tonalité, et
l'idée d'emprunter ceux d'AudioMuse-AI a été mesurée puis écartée : leur champ
`scale` vaut `minor` pour les 26 928 morceaux, et leur `tempo` ne prend que
37 valeurs distinctes espacées de 6 % — une grille de classifieur, sans phase.
Caler deux morceaux demande mieux que ±3 % et une position de battement. Ces
deux grandeurs sont à calculer chez nous. Référence à étudier : Mixxx.

### 9. Génération de piste — non planifié

`docs/modules.md` la classe expérimentale et tardive : qualité inégale, calcul
très intensif, licences floues. À laisser de côté tant que le reste n'est pas
solide.

## Ce qui n'est pas dans ce plan, et pourquoi

- **Aléatoire et répétition** (module 1) : tu les as écartés de la v1. À
  reprendre quand la file d'attente sera retravaillée, pas avant.
- **Les 10 fichiers opus** : une dépendance pour 0,04 % de la bibliothèque.
  À faire si l'occasion se présente, jamais pour elle-même.
- **La PR amont chez `demucs-rs`** : ouverte, sans réponse. Notre révision est
  épinglée, on ne dépend pas de leur réactivité.
