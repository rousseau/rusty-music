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

- **Modèle « Atelier »** (`ui-spec.md`) : rail à gauche, carte au centre,
  inspecteur à droite, **dock en bas** qui s'ouvre en mode Éditer et pousse la
  carte vers le haut sans la faire disparaître. La carte reste la réserve de
  matière : c'est là qu'on choisit quoi ouvrir.
- **Transport pleine largeur**, présent dans les trois modes.
- **Un seul curseur, un seul bouton de lecture.** Tranché à l'usage : les
  spectrogrammes des stems et la barre du bas partagent la même position, et le
  transport pilote les stems quand ils jouent. Le lecteur du module 1 se tait
  pendant ce temps.
- **Sobriété stricte** — le risque nommé dans `ui-spec.md` est que l'Atelier
  retombe en panneau d'administration. Un éditeur est précisément l'endroit où
  ça arrive.

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

## Le parcours, en quatre temps

### 1. Choisir

Depuis la carte, la file d'attente ou la vue Écoute — **le morceau en cours de
lecture est le candidat par défaut**. Passer en mode Éditer sans rien choisir
doit donc proposer ce qu'on écoute, pas un écran vide.

### 2. Séparer

Le rail annonce la variante et le coût *avant* le bouton, comme l'analyse le
fait : une séparation demande une trentaine de secondes, la variante affinée
plusieurs minutes. Un morceau déjà séparé s'ouvre sans recalcul.

**À faire** : l'avancement est aujourd'hui un pourcentage ; il gagnerait à dire
quel stem est en cours dans la variante affinée, où quatre réseaux passent l'un
après l'autre.

### 3. Retoucher

Ce qui existe : niveau par stem, muet, solo, spectrogramme, curseur partagé.

Ce qui manque, et dans cet ordre :

- ~~**Vitesse et hauteur.**~~ **Fait le 18 août.** Deux pas-à-pas dans l'en-tête
  du dock, **et ce sont deux choses différentes** :

  | réglage | ce que c'est | coût |
  |---|---|---|
  | **vitesse** | 25 à 400 %, pas de 5 %. 200 % lit **deux fois plus vite**. | immédiat — un flottant que la lecture relit à chaque trame |
  | **hauteur** | −12 à +12 demi-tons, durée inchangée | quelques secondes, 124 Mo par réglage |

  **La vitesse préserve la hauteur**, par `wsola` — recouvrement-addition
  temporel, la méthode d'`atempo` chez ffmpeg. Elle s'applique dans la lecture
  elle-même, donc sans recalcul ni rechargement, et la position ne bouge pas.
  À 100 % la matière n'est pas traitée du tout : la voie directe est court-
  circuitée.

  ~~Reste à faire : le réglage **par stem**.~~ **Fait le 18 août.** Chaque
  ligne du dock s'ouvre sur son propre pas-à-pas de vitesse et de hauteur, plus
  un bouton « suivre l'ensemble » qui l'y ramène. Le badge de la ligne dit ce
  qui s'écarte, pour qu'un stem réglé ne se cache pas derrière un panneau
  fermé.

  **L'avertissement est tenu par le code, pas seulement écrit** : deux vitesses
  différentes désynchronisent, et l'écart grandit tant que la lecture continue.
  Revenir à l'ensemble réaligne donc les stems dans la foulée — sans quoi on
  arrêterait la dérive en gardant l'écart déjà pris.
- ~~**Remplacer un stem**~~ **Fait le 18 août.** Le premier geste qui relie
  vraiment l'éditeur à la carte : le panneau d'un stem propose les morceaux
  sonorement voisins, on en prend la batterie, elle vient à la place de
  l'ancienne. Trois choses à faire tenir, et pas une de plus — le tempo, le
  départ, la longueur (`crates/editor/src/greffe.rs`).

  **Le voisinage se calcule sur le morceau entier, pas sur le stem.** Limite
  assumée : la bibliothèque n'a d'empreintes que de mélanges complets, et en
  embarquer une par stem supposerait de démixer les 27 000 morceaux.

  **Le tempo est une contrainte dure, pas un classement** : un candidat dont le
  rapport de tempo sort de ±10 % après repliement à l'octave n'est pas proposé,
  si proche soit-il. La liste dit combien ont été écartés, et pourquoi.

  **Ce qui n'est pas fait, et qui s'entend** : les temps forts ne sont pas
  alignés. `descripteurs.rs` rend un tempo, pas une phase. Les deux batteries
  pulsent au même tempo sans garantie de tomber sur le même temps ; caler à la
  main avec la vitesse par stem est le recours, et les deux réglages sont
  arrivés ensemble.
- **Réglages par stem** : gain fin, inversion de phase. Peu de travail, utile
  au diagnostic d'une séparation douteuse.

### 4. Exporter — **fait le 18 août**

Un bouton dans l'en-tête du dock, et **une seule sortie : ce qu'on entend.** La
spec en prévoyait trois — un stem, la sélection, le mélange. Elles n'en font
qu'une : mettre un stem en solo *est* la sélection, et un menu de plus n'aurait
dit que ce que le dock montre déjà. Les niveaux, la coupure, le solo, la vitesse
et la hauteur sont tous appliqués au rendu.

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
  GPU ; l'interface doit rester servie et l'annoncer. Même leçon que le graphe
  des voisins, qui passait pour un plantage tant qu'il se taisait.
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
7. **Si le dock montre des stems, ce sont eux la source.** Ils sont chargés dès
   l'affichage et reprennent la position et l'état du morceau mêlé, qui se tait.
   Tranché après un défaut à l'usage : le dock montrait des stems inertes — solo
   et coupure n'agissaient sur rien, et le bouton du bas commandait encore le
   morceau mêlé, si bien qu'il fallait plusieurs pressions pour arrêter ce qu'on
   entendait.

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
