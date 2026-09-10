# Brief d'interface — Module 3 (Éditeur)

> **Périmètre : ce document ne couvre que le Module 3.** Le module 1 est dans
> `ui-spec-lecteur.md`, le module 2 dans `ui-spec.md`. Le *pourquoi* technique du
> démixage est dans `module3-demixage.md` ; ce document ne parle que de ce que
> l'utilisateur voit et fait.

## Ce que l'éditeur est — **décidé le 18 août**

**Une piste à la fois.** On ouvre un morceau, on le sépare en stems, on retouche,
on exporte. C'est tout.

Ce que cela exclut, et qu'il faut donc cesser d'attendre de ce module :

- **pas de session multipiste ni de montage.** Aucune ligne de temps où
  disposer plusieurs morceaux, aucun couper-coller ;
- **pas de projet sauvegardé.** Rien à ouvrir, rien à « enregistrer sous ». Les
  stems écrits sur le disque sont la seule persistance, et ils suffisent :
  rouvrir le même morceau les retrouve ;
- **pas de mixage de deux pistes.** Le territoire DJ — calage des battements,
  roue de Camelot — sort du module. `docs/suite.md` le gardait comme chantier 8 ;
  il devient un chantier à part, s'il se fait.

**Pourquoi ce périmètre.** C'est celui que le mode Éditer occupe déjà, et le
seul qui reste cohérent avec la promesse du projet : une suite d'écoute et
d'exploration qui sait aussi ouvrir un morceau. Une station de travail
demanderait une grammaire d'interface entière — pistes, régions, automation —
sans rapport avec la carte, et la carte est la colonne vertébrale.

**Non destructif, sans exception.** Le fichier d'origine n'est jamais réécrit.
C'est un fichier de la bibliothèque surveillée : le modifier ferait rescanner,
réanalyser, et déplacerait le morceau sur la carte.

## Ce qui est déjà tranché ailleurs — à reprendre tel quel

- **Modèle « Atelier »** (`ui-spec.md`) : rail à gauche, zone centrale propre au
  mode, inspecteur à droite, transport pleine largeur en bas. `ui-spec.md`
  disait « carte toujours au centre » ; `interface-guidelines.md` l'a retranché
  — **la zone centrale est propre à chaque mode**. Pour Éditer, elle porte
  l'établi (voir « Le parcours » ci-dessous).
- **~~Dock en bas qui pousse la carte vers le haut.~~** Abandonné le 10
  septembre : l'atelier réel vivait dans un tiroir de 34 vh pendant que le
  centre montrait un sélecteur d'albums devenu inutile une fois le morceau
  choisi. Les stems montent au centre ; le dock se réduit à une barre d'outils
  pleine largeur (détail à l'état 3).
- **Transport pleine largeur**, présent dans tous les modes.
- **Un seul curseur, un seul bouton de lecture.** Tranché à l'usage : les
  spectrogrammes des stems et la barre du bas partagent la même position, et le
  transport pilote les stems quand ils jouent. Le lecteur du module 1 se tait
  pendant ce temps. Dans l'établi, ce curseur unique est **un playhead vertical
  qui traverse toute la pile de stems**, pas un repère par ligne.
- **Sobriété stricte** — le risque nommé dans `ui-spec.md` est que l'Atelier
  retombe en panneau d'administration. Un éditeur est précisément l'endroit où
  ça arrive : c'est pourquoi le détail d'un stem va dans l'inspecteur commun et
  non dans un panneau qui se déplie sous sa ligne (état 3).

## Ce que le moteur fournit déjà

| commande | ce qu'elle rend |
|---|---|
| `start_demix` / `demix_state` | sépare un fichier, avancement, chemins des stems |
| `stems_existants` | les stems déjà écrits pour ce morceau, sans recalcul |
| `stems_play`, `stems_gain`, `stems_transport`, `stems_state` | lecture simultanée, niveau par stem, position |
| `stem_spectre` | spectrogramme d'un stem, en intensités |

Trois variantes de séparation : 4 stems (7,8 × le temps réel), 6 stems (+
guitare et piano, même vitesse, les quatre de base un peu moins bien séparés),
4 stems affinés (quatre réseaux, ~4 × plus lent).

## Le parcours — le centre a trois états

Le centre d'Éditer n'est pas un composant fixe : c'est **l'établi**, et il
change selon l'avancement. Le rail ne porte que les contrôles permanents du
mode ; l'inspecteur de droite reste l'inspecteur commun
(`interface-guidelines.md`, Règle 1) ; le transport reste pleine largeur.

### État 1 — Choisir

Pas de morceau ouvert, ou on veut en changer. Le centre montre la grille
d'albums / liste d'artistes — le même composant qu'Écouter, réutilisé tel quel :
c'est là qu'on choisit quoi ouvrir, depuis la carte, la file d'attente ou la
vue Écoute. **Le morceau en cours de lecture est le candidat par défaut** et
s'affiche mis en avant : passer en mode Éditer sans rien choisir ne donne
jamais un écran vide.

En-tête : « Choisir un morceau ».

### État 2 — Séparer

Morceau choisi, pas encore de stems. Le centre montre **une carte de
séparation** : pochette et titre du morceau, les trois variantes avec leur coût
annoncé *avant* le bouton (comme l'analyse le fait — une séparation demande une
trentaine de secondes, la variante affinée plusieurs minutes), le bouton
Séparer, la barre de progression.

C'est l'ancien bloc « Démixage » du rail **sorti du rail et posé au centre** :
l'action ponctuelle va là où se fait la tâche, le rail garde ce qui est
permanent. Un morceau déjà séparé saute cet état — il s'ouvre directement dans
l'établi, sans recalcul.

**Fait le 10 septembre.** Une barre de progression sous le bouton, alimentée
par les évènements de `demucs-core` : indéterminée le temps du décodage et de
la chauffe du modèle (aucun segment connu), puis graduée segment par segment —
un morceau de quatre minutes se découpe en cinq passages qui se recouvrent. Le
texte donne le pourcentage, et **la variante affinée y ajoute le stem en
cours**, ses quatre réseaux passant l'un après l'autre. La greffe d'un stem
voisin affiche le même pourcentage dans l'aide de la barre d'outils pendant
qu'elle sépare le morceau source. Sous la fenêtre d'entraînement (~7,8 s) il
n'y a qu'un passage : la barre reste indéterminée, mais la séparation est quasi
instantanée.

### État 3 — Retoucher (l'établi)

Les stems présents, le centre est la **pile de stems**, en pleine hauteur —
c'est le composant distinctif d'Éditer, au même titre que la carte l'est pour
Explorer. Chaque stem est une ligne : nom, solo, muet, fader de niveau, et un
**grand spectrogramme** sur l'axe de temps partagé avec le transport.

Trois conséquences de mise en page, décidées le 10 septembre :

- **Un seul playhead**, vertical, traverse toute la pile — il rend visible la
  décision « un seul curseur » au lieu d'un widget à part. Cliquer n'importe
  quel spectrogramme déplace la lecture : même geste, même axe que la barre du
  bas.
- **Le détail d'un stem va dans l'inspecteur de droite.** Cliquer une ligne la
  sélectionne et peuple `#insp` : vitesse et hauteur du stem, bouton « suivre
  l'ensemble », zone de remplacement. Pas de panneau qui se déplie sous la
  ligne — un seul inspecteur (`interface-guidelines.md` Règle 1), divulgation
  progressive (Règle 8). Le badge de la ligne continue de dire ce qui s'écarte
  de l'ensemble, pour qu'un stem réglé ne se cache pas.
- **Le dock devient une barre d'outils fine, pleine largeur**, au-dessus du
  transport : vitesse d'ensemble, hauteur d'ensemble, badge de dérive +
  « réaligner », Exporter. Plus de tiroir de 34 vh, plus de scroll interne.

En-tête : « Établi — {titre} », avec un retour « ← Choisir un morceau » vers
l'état 1.

Ce qui existe déjà et ne bouge pas : niveau par stem, muet, solo,
spectrogramme, curseur partagé.

Ce qui manque, et dans cet ordre :

- ~~**Vitesse et hauteur.**~~ **Fait le 18 août.** Deux pas-à-pas dans la barre
  d'outils, **et ce sont deux choses différentes** :

  | réglage | ce que c'est | coût |
  |---|---|---|
  | **vitesse** | 25 à 400 %, pas de 5 %. 200 % lit **deux fois plus vite**. | immédiat — un flottant que la lecture relit à chaque trame |
  | **hauteur** | −12 à +12 demi-tons, durée inchangée | quelques secondes, 124 Mo par réglage |

  **La vitesse préserve la hauteur**, par `wsola` — recouvrement-addition
  temporel, la méthode d'`atempo` chez ffmpeg. Elle s'applique dans la lecture
  elle-même, donc sans recalcul ni rechargement, et la position ne bouge pas.
  À 100 % la matière n'est pas traitée du tout : la voie directe est court-
  circuitée.

  ~~Reste à faire : le réglage **par stem**.~~ **Fait le 18 août** (relogé le
  10 septembre). Sélectionner une ligne ouvre dans l'inspecteur son propre
  pas-à-pas de vitesse et de hauteur, plus un bouton « suivre l'ensemble » qui
  l'y ramène. Le badge de la ligne dit ce qui s'écarte, pour qu'un stem réglé
  ne se cache pas derrière une ligne non sélectionnée.

  **L'avertissement est tenu par le code, pas seulement écrit** : deux vitesses
  différentes désynchronisent, et l'écart grandit tant que la lecture continue.
  Revenir à l'ensemble réaligne donc les stems dans la foulée — sans quoi on
  arrêterait la dérive en gardant l'écart déjà pris.
- **La vitesse et la hauteur se disent en musique, pas en ratio — à faire (10
  septembre).** Le besoin qui l'appelle : faire tomber ensemble des matières
  de morceaux différents. Un pourcentage et un nombre de demi-tons ne disent
  pas si deux stems vont s'accorder ; un BPM et une tonalité, oui. Le moteur ne
  change pas — la lecture prend déjà un flottant de vitesse et la transposition
  un nombre de demi-tons ; c'est une vue par-dessus.

  - ~~**BPM cible.**~~ **Fait le 10 septembre.** Quand le morceau ouvert a un
    `bpm` mesuré (base, morceau entier) **et une pulsation franche** — netteté
    ≥ 2 mesurée sur le stem `drums` par `battements::grille_reechantillonnee`,
    commande `tempo_cible` —, un bouton d'unité dans la barre d'outils bascule
    le réglage « vitesse » de `%` en `BPM`. `edition.vitesse` reste le rapport
    `bpm_cible / bpm_source` ; l'unité n'est qu'une vue, et le `%` **reste
    accessible d'un clic**. Pas de pulsation franche : le bouton ne s'affiche
    pas, le `%` est le seul réglage.
    - **Plafond de qualité, tenu par l'affichage.** Au-delà de ±20 % d'étirement
      d'ensemble, la barre affiche « étirement franc — le son peut se ternir »,
      du même ton que « les stems ont dérivé de X s » : on ne bloque pas.
    - **Ambiguïté d'octave.** Un bouton « ½ / ×2 » apparaît quand
      `bpm_cible / bpm_source` sort de [1/√2, √2] et ramène le rapport près de 1
      en admettant une lecture à demi- ou double-tempo (`greffe::tempo_replie`).
  - **Tonalité cible.** Quand `tonalite` est mesurée, l'inspecteur d'un stem
    montre « +3 → G min » à côté du pas-à-pas en demi-tons. Trois limites à
    écrire dans l'interface, pas seulement dans la doc :
    - **la transposition ne change pas le mode.** On ne vise qu'une tonalité du
      même mode ; F min → F maj n'est pas au menu.
    - **la détection de tonalité est peu fiable** (confusion relatif
      majeur/mineur surtout). La cible est **suggérée, jamais imposée** ; «
      tonalité incertaine » s'affiche plutôt que tranché en silence, comme pour
      le nommage des familles.
    - **pas de tonalité sur une batterie.** Le réglage est grisé pour les stems
      percussifs (stem `drums`).
- ~~**Remplacer un stem**~~ **Fait le 18 août.** Le premier geste qui relie
  vraiment l'éditeur à la carte : l'inspecteur du stem sélectionné propose les
  morceaux sonorement voisins, on en prend la batterie, elle vient à la place
  de l'ancienne. Trois choses à faire tenir, et pas une de plus — le tempo, le
  départ, la longueur (`crates/editor/src/greffe.rs`).

  **À faire (10 septembre) : montrer ces voisins comme la carte les montre.**
  Aujourd'hui c'est une liste classée ; ce sont pourtant les mêmes voisins
  t-SNE que le nuage d'Explorer, à afficher sur une mini-bande reprenant la
  projection et la palette de familles (`--familles`). C'est ce qui fait de la
  greffe un vrai pont vers la carte plutôt qu'une liste de plus.

  **Le voisinage se calcule sur le morceau entier, pas sur le stem.** Limite
  assumée : la bibliothèque n'a d'empreintes que de mélanges complets, et en
  embarquer une par stem supposerait de démixer les 27 000 morceaux.

  **Le tempo est une contrainte dure, pas un classement** : un candidat dont le
  rapport de tempo sort de ±10 % après repliement à l'octave n'est pas proposé,
  si proche soit-il. La liste dit combien ont été écartés, et pourquoi.

  ~~**Ce qui n'est pas fait**~~ **Les temps forts se calent, depuis le 19
  août.** `rusty_music_analysis::battements` donne la phase des deux stems, et
  la greffe s'en sert pour deux choses : le greffon **entre** sur un battement,
  et sa matière est **coupée à un compte rond de battements** — sans quoi une
  greffe qui boucle six fois se désaccorde six fois.

  Le panneau le dit : « calé sur les temps » ou « calé sur la première
  attaque », selon que la grille a pu se mesurer.

  **Ce qui reste, et il faut le savoir** : sur une batterie, la phase est
  presque indéterminée — deux décalages à un demi-battement l'un de l'autre
  peuvent n'être départagés que par 0,03 de netteté. La vitesse par stem reste
  donc le recours, mais pour rattraper une ambiguïté et non une absence de
  calage.

  **À faire (10 septembre) — la greffe cale aussi la tonalité.** Elle amène
  déjà le greffon au tempo du morceau ouvert (`greffe::tempo_replie`, ±10 %) et
  le fait entrer sur un temps ; elle ne touche pas à sa hauteur, si bien qu'une
  basse voisine peut arriver dans la mauvaise tonalité et jurer. Quand les deux
  `tonalite` sont mesurées, transposer le greffon de leur écart (replié dans
  [−6, +6] demi-tons) et le dire dans le panneau : « basse greffée, tempo
  96 → 124, ton −2 ». `Plan` porte déjà `bpm_rendu` ; lui ajouter
  `demi_tons_rendu`. Si l'une des tonalités manque, on ne transpose pas et on
  l'écrit — même honnêteté que « calé sur la première attaque » quand la grille
  n'a pas pu se mesurer.
- **Réglages par stem** : gain fin, inversion de phase. Peu de travail, utile
  au diagnostic d'une séparation douteuse.

### État final — Exporter — **fait le 18 août**

Un bouton dans la barre d'outils, et **une seule sortie : ce qu'on entend.** La
spec en prévoyait trois — un stem, la sélection, le mélange. Elles n'en font
qu'une : mettre un stem en solo *est* la sélection, et un menu de plus n'aurait
dit que ce que l'établi montre déjà. Les niveaux, la coupure, le solo, la
vitesse et la hauteur sont tous appliqués au rendu.

WAV, comme ce que le démixeur écrit. Le nom porte ce qui n'est pas neutre —
`Die Oros — drums — 80% — +3.wav` — sans quoi deux rendus du même morceau
seraient indiscernables.

**Le moteur refuse d'écrire sous une racine surveillée**, et le message nomme le
dossier fautif. Un rendu y serait ingéré, analysé et placé sur la carte alors
que ce n'est pas un morceau. La comparaison se fait composant par composant :
`/Musique2` n'est pas dans `/Musique`, ce qu'un `starts_with` textuel aurait
prétendu. Testé.

## Le coût disque, à montrer

Un jeu de quatre stems pèse **124 Mo** (WAV PCM16, morceau de quatre minutes).
Ce n'est pas un détail d'implémentation : quinze morceaux séparés remplissent
deux gigaoctets, et rien aujourd'hui ne le dit à l'utilisateur ni ne le range.

**À faire** : l'écran Réglages affiche la taille du cache de stems et permet de
le vider, comme il affiche déjà les racines surveillées. Un morceau séparé puis
oublié ne doit pas être une fuite silencieuse.

## États limites — mesurés, pas supposés

- **Le démixage sature la machine.** Trente secondes à plusieurs minutes sur le
  GPU ; l'interface reste servie (calcul dans un fil, avancement sondé) et la
  barre de progression l'annonce. Même leçon que le graphe des voisins, qui
  passait pour un plantage tant qu'il se taisait.
- **Deux morceaux séparés en même temps** : refusé, avec un message. Le modèle
  occupe le GPU, deux copies n'y tiendraient pas.
- **Un morceau que le décodeur refuse** : deux fichiers de la bibliothèque de
  test sont corrompus. Le message doit nommer le fichier, pas dire « échec ».
- **Un stem effacé sous les pieds** de l'application — le dossier de cache est
  ordinaire, l'utilisateur peut le vider. Rouvrir doit reséparer, pas échouer.

## Ce que l'interface demandera au moteur

| commande | pourquoi |
|---|---|
| `stems_export(stems, destination, format)` | écrire ailleurs que dans le cache |
| ~~`stems_vitesse(vitesse)`~~ | **fait** — immédiat, sans recalcul |
| ~~`stems_exporter(…)`~~ | **fait** — écrit ce qu'on entend, hors de la bibliothèque |
| ~~`stems_etirer(stems, facteur, demi_tons)`~~ | **fait** — transposition, mise en cache par réglage |
| ~~`stems_cache()` / `stems_cache_vider()`~~ | **fait** — bloc « Stems démixés » dans les Réglages |
| ~~`voisins_de_stem(stem, k)`~~ | **fait** — les voisins dont le tempo se cale, ceux qui ont été écartés |
| ~~`stems_greffer(…)`~~ | **fait** — écrit la greffe sous `greffes/`, rend ce qu'il a fallu lui faire |
| ~~`stems_vitesse_stem(stem, vitesse)`~~ | **fait** — la vitesse d'un stem seul, immédiate |
| ~~`tempo_cible(id)`~~ | **fait** — le BPM du morceau (base) et la netteté de sa pulsation (grille du stem `drums`), pour décider si la barre propose un BPM cible |
| `stems_greffer(…)` rend aussi `demi_tons_rendu` | **à faire** — dire la transposition du greffon comme il dit déjà le tempo |

Le **réglage** en BPM cible ne change rien au moteur — `stems_vitesse` garde son
flottant, l'interface le convertit. `tempo_cible` ne sert qu'à *savoir s'il faut
proposer* le BPM : une pulsation franche, ça se mesure, ça ne se devine pas. De
même, la tonalité cible convertira en demi-tons sans nouvelle commande de
réglage.

## Décisions

1. **Une piste à la fois.** Ni session, ni projet, ni montage, ni mixage DJ.
2. **Non destructif.** Le fichier d'origine n'est jamais réécrit.
3. **Les stems sur le disque sont la seule persistance.** Pas de format de
   projet à inventer, à versionner, à migrer.
4. **Time-stretch global par défaut**, par stem en option, avec l'avertissement
   que des facteurs différents désynchronisent.
5. **Export en WAV**, hors de la bibliothèque surveillée.
6. **Le curseur et le bouton de lecture sont uniques**, partagés avec le
   transport. Déjà implémenté, à ne pas défaire.
7. **Si l'établi montre des stems, ce sont eux la source.** Ils sont chargés dès
   l'affichage et reprennent la position et l'état du morceau mêlé, qui se tait.
   Tranché après un défaut à l'usage : l'écran montrait des stems inertes — solo
   et coupure n'agissaient sur rien, et le bouton du bas commandait encore le
   morceau mêlé, si bien qu'il fallait plusieurs pressions pour arrêter ce qu'on
   entendait.
8. **Le centre d'Éditer est l'établi, à trois états** (choisir / séparer /
   retoucher). Décidé le 10 septembre en remplacement de « la carte reste au
   centre, le dock s'ouvre en bas ». Le détail d'un stem est porté par
   l'inspecteur commun ; le dock se réduit à une barre d'outils pleine largeur.
9. **La vitesse et la hauteur se disent en BPM et en tonalité** quand l'analyse
   les mesure avec assez de netteté ; le % et les demi-tons restent le repli,
   toujours accessibles. La greffe cale tempo **et** tonalité. Décidé le 10
   septembre — le besoin est de fondre des matières de morceaux différents, et
   un ratio ne dit pas si elles s'accordent.

## Questions ouvertes

- ~~**Le remplacement de stem est-il dans ce module ou au-delà ?**~~ **Tranché
  le 18 août : dedans.** Le second fichier n'ouvre pas de session — il ne
  survit pas à la fermeture, la greffe est un WAV de plus dans le cache et
  rouvrir le morceau retrouve ses stems séparés, pas la greffe. « Une piste à
  la fois » tient donc : on édite toujours un seul morceau, on va seulement
  chercher de la matière ailleurs.
- **Faut-il exporter aussi en FLAC ?** Le WAV est simple et sans perte ; le FLAC
  diviserait la taille par deux mais demande un encodeur, donc une dépendance.
- **Le cache de stems doit-il se purger tout seul** (les N derniers morceaux, ou
  une taille plafond), ou seulement à la main depuis les Réglages ?
