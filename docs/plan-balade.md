# Plan : la balade musicale

> Suite de `recherche-balade-musicale.md` (1ᵉʳ nov. 2026). Ce document
> détaille chaque piste (P0 à P9) et donne pour chacune un **prompt prêt à
> coller** dans une nouvelle session. Les numéros sont ceux de la recherche.
> Rien n'est implémenté au moment de l'écriture.

## Le principe : un livre dont vous êtes le héros, qui se lit tout seul

L'idée vient de François : une playlist comme un livre-jeu, avec à chaque
étape plusieurs pistes à explorer. L'intérêt est le contrôle et
l'interactivité. Le coût est le temps demandé à l'utilisateur, et c'est
précisément ce coût qui a tué les interfaces cartographiques de recherche
(`recherche-balade-musicale.md` § 3.9).

La réponse retenue : **le choix est toujours offert, jamais exigé.**

| règle | pourquoi |
|---|---|
| **Un choix par morceau, au plus.** Il se présente pendant les 20 à 30 dernières secondes, au carrefour. | Un livre-jeu propose un choix par paragraphe, pas par phrase. Plus souvent, on ne fait plus qu'arbitrer. |
| **Deux ou trois branches, pas plus.** | Le temps de décision croît avec le nombre d'options (loi de Hick) ; au-delà de trois, on ne compare plus, on survole. |
| **Une branche par défaut, visible.** Sans geste, la balade la prend. | C'est la radio. Ne rien faire est un choix valable, pas une faute. |
| **Chaque branche s'annonce** : un nom, une couleur, quelques secondes de son. | Choisir sans indice, c'est tirer au sort (l'*odeur* de Pirolli et Card). |
| **Le choix ne coupe rien.** Le morceau en cours continue ; le choix porte sur le suivant. | Le temps demandé tombe à une touche pendant qu'on écoute déjà. |
| **On peut revenir au carrefour** et prendre l'autre branche. | Le « retournez au § 47 » du livre-jeu ; la machine à remonter le temps de Musicream. |
| **Le degré d'intervention se règle** une fois : *se laisser porter* (choix masqués), *proposer* (défaut), *demander* (la balade attend au carrefour). | Le même outil sert le soir en fond et le dimanche en exploration. |

Le mode *demander* est le seul où la balade s'arrête pour attendre. Il est
explicite et n'est jamais le réglage par défaut.

## Vocabulaire

| mot | sens |
|---|---|
| **balade** | une session d'écoute dont la suite est choisie pas à pas sur la carte, pas calculée d'avance |
| **promeneur** | le repère sur la carte, à l'adresse du morceau en cours |
| **carrefour** | le moment, et l'endroit, où se présente le choix du morceau suivant |
| **branche** | une suite possible au carrefour : une rue et son premier morceau |
| **pente** | la préférence par défaut entre deux branches (proche à l'oreille, neuve) |
| **aura** | les morceaux voisins qu'on entend en sourdine autour d'un point |
| **trace** | ce qui a été parcouru, visible sur la carte |
| **brouillard** | le voile sur ce qui n'a jamais été écouté |
| **chaîne** | une balade gardée, rejouable (mot de Qoyra) |

## Contraintes communes à tous les prompts

Chaque prompt ci-dessous y renvoie. Les répéter dans la session est inutile :
`CLAUDE.md` les porte déjà.

- Lire avant tout : `CLAUDE.md`, `docs/recherche-balade-musicale.md`, ce
  document, puis les documents cités par la piste.
- Rust uniquement à l'exécution ; ne rien réécrire qui existe ; toute nouvelle
  dépendance passe par `docs/rust-audio-stack.md` et `cargo deny check`.
- Une commande Tauri qui change de forme → `apps/desktop/ui-tests/fixtures.js`.
  Un changement de schéma → une migration (`db::migrations`).
- Interface : `docs/interface-guidelines.md`, puis `scripts/audit-interface.sh`
  après toute modification de mise en page.
- Fin de piste : `./scripts/verifier.sh` vert, une entrée datée dans
  `docs/journal.md` (ce qui a été fait **et mesuré**), l'état mis à jour dans
  `docs/suite.md` et dans le tableau d'avancement en bas de ce document.
- Ne pas toucher à `livre/`.

---

## P0 — Mesurer la continuité avant de construire

**Objectif.** Savoir si une marche dans les rues s'entend comme une
progression ou comme un tirage au hasard. Le résultat décide si P7
(réordonner les façades) passe avant tout le reste.

**Ce qui existe.** `crates/carto/src/reseau_reel.rs` (graphe des rues,
`voisins`, `accrocher`, `couts_depuis`) ; `facades.rs` (voie qui borde chaque
bâtiment) ; l'accrochage des morceaux à la voirie côté application
(`charger_accrochage_voirie`, `apps/desktop/src/explorer/carte.rs`) ;
`crates/analysis/src/chemin.rs` (`Graphe::sonique`, `Graphe::errance`) ;
l'exemple `croissance_apercu` comme modèle d'outil de mesure.

**Mesures.**

- **E1, continuité.** Pour 500 départs tirés au hasard (graine fixe), des
  suites de 20 morceaux :
  1. marche aléatoire auto-évitante sur la voirie, morceaux pris dans l'ordre
     des façades le long de chaque tronçon ;
  2. les trois profils de `itineraire_voirie`, sans arrivée, 60 min ;
  3. `Graphe::sonique` entre départ et arrivée de la marche 1 ;
  4. `Graphe::errance` ;
  5. tirage au hasard dans la bibliothèque.

  Pour chacune : distance cosinus CLAP médiane entre morceaux consécutifs,
  9ᵉ décile (les sauts), et part des transitions qui changent de famille.
- **E2, carrefours.** Pour chaque sommet de degré ≥ 3 de la zone peuplée :
  nombre de branches habitées, entropie des familles des premiers morceaux de
  chaque branche, distance CLAP entre ces premiers morceaux et le morceau
  d'arrivée. Histogrammes, et la part des carrefours « utiles » : au moins
  deux branches habitées, dont une proche (distance sous la médiane de 3) et
  une différente (autre famille).

**Critère de décision** (à écrire dans le journal avec les chiffres) : si la
médiane de E1-1 est plus proche de E1-5 (hasard) que de E1-3 (sonique), P7
passe avant P1.

**Pièges.** Les morceaux non accrochés (bois, hors zone) sautent sans être
comptés. Les artistes ancrés sur monuments font des îlots : les compter à
part. Toujours la même graine pour comparer d'un réglage à l'autre.

### Prompt P0

```text
Lis CLAUDE.md, docs/recherche-balade-musicale.md et docs/plan-balade.md (section P0).

Écris un exemple de mesure `crates/carto/examples/continuite_balade.rs`, sur le modèle de
`croissance_apercu`, qui lit `ville-paris.db` et la base de la bibliothèque (empreintes
CLAP, familles, positions réelles des morceaux) et calcule E1 (continuité de cinq façons de
fabriquer une suite de 20 morceaux, 500 départs, graine fixe) et E2 (carrefours utiles), tels
que définis dans plan-balade.md. Réutilise reseau_reel::Graphe, facades.rs, l'accrochage des
morceaux à la voirie et analysis::chemin ; ne duplique pas leur logique. Si une partie de
cette logique n'est accessible que dans apps/desktop, déplace-la dans crates/carto (ou
analysis), sans changer son comportement, et fais-le dans un commit séparé.

Sortie : un `continuite.md` (tableaux) et deux SVG (histogramme des distances consécutives par
méthode, histogramme d'entropie des carrefours). Lance-le sur ma bibliothèque, reporte les
chiffres et la décision (P7 avant P1 ou non) dans docs/journal.md et dans le tableau
d'avancement de docs/plan-balade.md. Pas d'interface dans cette étape.
```

---

## P1 — Le promeneur : la lecture est une marche

**Objectif.** Un mode de lecture « balade » où le morceau suivant n'est pas
dans une file calculée d'avance mais **choisi au moment où on en a besoin**,
depuis la position courante sur la carte. Le promeneur avance le long de la
rue au rythme de la lecture.

**Ce qui existe.** Le `Player` (`crates/player/src/lib.rs`) gère une file
(`set_queue`, `rebrancher_file`, `a_precharger` / `charger_precharge` pour
l'enchaînement sans blanc) ; `itineraire_voirie` sait déjà dérouler des
morceaux le long d'un trajet ; `js/itineraires.js` trace une route ;
`js/etat-lecture.js` est l'unique copie de l'état de lecture côté interface.

**Conception.**

- **Moteur.** Un état `Balade` côté application (pas dans `player` :
  le lecteur ne connaît pas la carte) : position courante (sommet de voirie et
  tronçon), sens de marche, sommets et morceaux déjà visités, réglage
  d'intervention, graine. Une fonction pure dans `crates/carto`
  (`balade.rs`) : `suivants(etat, graphe, accrochage) -> Vec<Branche>`, où
  chaque branche porte la rue, le prochain morceau et la séquence de
  sommets jusqu'à lui. Tant qu'on est au milieu d'une rue, une seule branche
  (on continue). Au bout de la rue, celles du carrefour.
- **File courte.** La file du lecteur ne contient jamais que le morceau en
  cours **et le suivant décidé**. Quand le choix change (P2), on remplace le
  suivant avant son préchargement. Dans les 10 dernières secondes, le suivant
  est figé pour que l'enchaînement sans blanc tienne.
- **Avance du promeneur.** L'interface interpole la position le long de la
  façade du morceau en cours selon `position / durée`. Rien de calculé côté
  moteur à chaque battement : le sondage de lecture existant suffit.
- **Pente par défaut, version simple** (la vraie arrive avec P3) : continuer
  tout droit si la rue continue ; sinon la branche dont le premier morceau est
  le plus proche en CLAP du morceau en cours, en excluant le déjà visité.
- **Entrée.** Un bouton « Flâner d'ici » dans l'inspecteur (à côté de ▶) et
  dans le menu du clic droit sur la carte. Sortie : ▶ sur autre chose, ou
  « Arrêter la balade ».
- **Repli** sans ville importée : la même balade dans le graphe des voisins
  (les branches deviennent 2-3 voisins non visités), dessinée sur le nuage.

**Critères.** Une balade de 30 min se lance d'un clic et s'enchaîne sans blanc
ni intervention. Le promeneur ne saute jamais de plus d'une façade d'un
battement à l'autre. Aucun morceau répété dans une balade. Reprise de session :
la balade reprend à sa position (`restaurer`).

**Pièges.** Le verrou du lecteur (lire le commentaire de `completer`) : ne
jamais calculer les branches sous ce verrou. Les impasses : demi-tour autorisé
seulement s'il n'y a rien d'autre, sinon on ressort par la branche la moins
visitée. Une rue sans morceau (un pont, un quai vide) se traverse sans jouer.

### Prompt P1

```text
Lis CLAUDE.md, docs/recherche-balade-musicale.md, docs/plan-balade.md (principe, contraintes
communes, section P1), docs/carto-ville.md (§ réseau et itinéraires) et
docs/interface-guidelines.md. Lis aussi le résultat de P0 dans docs/journal.md.

Implémente le mode « balade » décrit en P1 :
1. crates/carto/src/balade.rs : état de balade et fonction pure `suivants` (branches au
   carrefour, une seule au milieu d'une rue), avec tests unitaires sur un petit graphe
   synthétique (impasse, rue vide, carrefour à 4 branches, déjà visité exclu).
2. apps/desktop/src/explorer : commandes Tauri `balade_demarrer(depart)`,
   `balade_etat()`, `balade_arreter()`, et la logique « file courte » (en cours + suivant
   décidé, figé dans les 10 dernières secondes) branchée sur le sondage de lecture existant,
   sans calcul sous le verrou du lecteur. Mets à jour ui-tests/fixtures.js.
3. Interface : bouton « Flâner d'ici » dans l'inspecteur et au clic droit sur la carte ; le
   promeneur dessiné sur la carte et interpolé le long de la façade ; la trace de la balade
   en cours. Pas encore de choix aux carrefours (c'est P2) : la pente par défaut simple suffit.
4. Repli dans le graphe des voisins quand il n'y a pas de ville importée.

Vérifie à l'usage une balade de 30 minutes (enchaînement sans blanc, pas de répétition, pas
de saut du promeneur), lance scripts/verifier.sh et scripts/audit-interface.sh, puis mets à
jour journal.md, suite.md et le tableau d'avancement de plan-balade.md.
```

---

## P2 — Le carrefour : le choix du livre-jeu

**Objectif.** Pendant la fin de chaque morceau qui débouche sur un carrefour,
proposer 2 ou 3 branches, chacune annoncée par son nom, sa couleur et un
extrait. Ne rien faire = la branche par défaut.

**Conception.**

- **La carte du carrefour.** Un petit panneau ancré près du promeneur, trois
  cartes au plus : nom de la rue (l'artiste), titre du premier morceau,
  pastille de famille, une jauge « proche ↔ ailleurs » (distance CLAP au
  morceau en cours). La branche par défaut est marquée et pré-sélectionnée.
  Sur la carte, chaque branche est tracée sur quelques dizaines de mètres dans
  sa couleur.
- **Gestes.** Touches 1, 2, 3 (ou flèches selon la direction de la rue),
  clic sur la carte du panneau, ou clic sur la rue elle-même. Échap masque le
  panneau pour ce carrefour. Un anneau de temps restant sur la branche par
  défaut indique quand elle sera prise.
- **Annonce sonore.** Au survol d'une carte de branche : 5 s de son premier
  morceau, en sourdine sous le morceau en cours (le mélange à deux voix est la
  première brique de P4 ; si P4 n'est pas encore fait, l'extrait remplace
  momentanément le son principal, baissé, puis le rend).
- **Choix des branches proposées** quand le carrefour en a plus de trois :
  garder la pente par défaut, la branche la plus différente (autre famille) et
  la moins visitée. Toujours au moins une branche « proche » et une « ailleurs ».
- **Réglage d'intervention** (une fois, dans les réglages de la carte) :
  *se laisser porter* (aucun panneau, seulement le tracé), *proposer* (défaut),
  *demander* (la balade attend au carrefour, morceau en cours terminé =
  silence jusqu'au choix).
- **Revenir.** « ← Carrefour précédent » rejoue le carrefour passé : la
  balade y retourne et propose les branches non prises. La branche déjà
  parcourue apparaît comme lue.

**Critères.** Choisir prend une touche et ne coupe jamais la lecture. En mode
*se laisser porter*, aucune différence avec P1. Le panneau ne masque jamais le
promeneur. Mesure à journaliser après une semaine d'usage : part des carrefours
où un choix a été fait, part des choix « ailleurs ».

**Pièges.** Le figeage des 10 dernières secondes (P1) : le panneau doit
disparaître ou se griser à ce moment, sinon un choix tardif est perdu sans le
dire. Un carrefour au tout début d'un morceau très court (< 40 s) : proposer
le choix plus tôt, au début du morceau.

### Prompt P2

```text
Lis CLAUDE.md, docs/plan-balade.md (principe « livre dont vous êtes le héros », contraintes
communes, P1 et P2) et docs/interface-guidelines.md. P1 est fait : relis son entrée dans
docs/journal.md et crates/carto/src/balade.rs.

Implémente le carrefour de P2 : panneau de 2-3 branches ancré près du promeneur (nom de rue,
premier morceau, famille, jauge proche↔ailleurs, branche par défaut marquée avec l'anneau de
temps restant), gestes 1/2/3, flèches, clic sur la rue, Échap ; sélection des branches
proposées quand il y en a plus de trois (défaut + plus différente + moins visitée) ; réglage
d'intervention (se laisser porter / proposer / demander) persisté avec les autres réglages
de la carte ; retour au carrefour précédent avec les branches non prises. L'extrait de 5 s
au survol d'une branche : si P4 n'existe pas encore, baisse le son principal le temps de
l'extrait plutôt que de mélanger.

Le choix remplace le « suivant décidé » de la file courte avant son préchargement et devient
impossible (panneau grisé, et dit pourquoi) dans les 10 dernières secondes. Ajoute un
compteur local (carrefours vus, choix faits, choix « ailleurs ») pour la mesure d'usage.
Tests, verifier.sh, audit-interface.sh, journal, suite.md, tableau d'avancement.
```

---

## P3 — Les pentes : *The Naked City* sur nos quartiers

**Objectif.** Une pente par défaut plus intelligente que « le plus proche » :
elle mélange proximité sonore et nouveauté. Et la rendre visible entre
quartiers, comme les flèches rouges de Debord.

**Conception.**

- **Pente d'une branche** (au carrefour) :
  `p = a · proximité(CLAP, morceau en cours → premier morceau de la branche)
  + b · nouveauté(part de jamais-écoutés sur les N premiers morceaux de la
  branche) − c · déjà-visité(balade) − d · usure(écoutes récentes de la rue)`.
  `a, b, c, d` réglés par la mesure (P0 sert de banc) ; trois préréglages au
  lieu de quatre curseurs : *rester* (a fort), *dériver* (équilibre, défaut),
  *s'aventurer* (b fort, et une branche « ailleurs » toujours en défaut une
  fois sur trois).
- **Pentes entre quartiers** (le dessin). Pour chaque paire de territoires
  voisins (`ville::territoires_des_habitants`) : proximité CLAP moyenne des
  morceaux à moins de 150 m de part et d'autre de la frontière. On garde, par
  quartier, les deux pentes les plus fortes. Calculées à la génération des
  tuiles, une couche de flèches courbes rouges, visibles aux zooms
  intermédiaires seulement, une option d'affichage « Pentes ».
- **Les relations non sonores** (l'idée « multi-représentation » de
  MusicAtlas, voir la recherche) : un bonus de pente si l'artiste de la
  branche a une relation MusicBrainz (collaboration, membre) avec celui du
  morceau en cours. Ces relations sont déjà collectées par l'enrichissement
  (`crates/core/src/musicbrainz.rs`, `db/enrichissement.rs`).

**Critères.** E1 recalculé avec la pente *dériver* : la médiane des distances
consécutives reste sous celle de la marche aléatoire, et la part de
jamais-écoutés dans une balade de 60 min augmente nettement par rapport à P1
(chiffre à poser au journal). Les flèches ne se croisent pas plus de quelques
fois sur tout Paris.

### Prompt P3

```text
Lis CLAUDE.md, docs/recherche-balade-musicale.md (§ 3.8), docs/plan-balade.md (P0 à P3) et
docs/carto-ville.md. P1 et P2 sont faits.

1. Remplace la pente par défaut simple de crates/carto/src/balade.rs par la pente de P3
   (proximité CLAP, nouveauté d'après la table `ecoute`, déjà-visité, usure, bonus de
   relation MusicBrainz), avec trois préréglages rester / dériver / s'aventurer exposés dans
   le réglage de la balade. Règle les poids avec l'exemple continuite_balade de P0, étendu
   pour mesurer une balade simulée de 60 min par préréglage (distance consécutive, part de
   jamais-écoutés, part de changements de famille). Reporte les chiffres.
2. À la génération des tuiles, calcule les pentes entre territoires voisins (deux par
   quartier, proximité CLAP de part et d'autre de la frontière) et ajoute une couche de
   flèches courbes rouges dans le style (crates/carto/src/style.rs), aux zooms intermédiaires,
   derrière une option d'affichage « Pentes » ; un style par palette reste cohérent
   (palette.rs).
Tests, verifier.sh, audit-interface.sh, journal, suite.md, tableau d'avancement.
```

---

## P4 — L'aura : entendre la carte

**Objectif.** Les deux ou trois morceaux les plus proches d'un point jouent
ensemble, très bas, le plus proche dominant, avec un panoramique selon leur
direction. Le point est le curseur (exploration libre) ou le promeneur (balade),
et sert aussi aux extraits des carrefours (P2).

**Ce qui existe.** `crates/player/src/multipiste.rs` somme plusieurs sources
échantillon par échantillon, avec un niveau atomique par voix : c'est le
mélangeur. `rodio` 0.22 a un lecteur spatial (`spatial_player`). L'analyse
découpe déjà en fenêtres CLAP de 10 s (`crates/analysis/src/lib.rs`,
`FENETRE_S`).

**Conception.**

- **Extraits pré-découpés.** À l'analyse (une passe de fond de plus, ou
  l'étape d'empreinte elle-même) : pour chaque morceau, la fenêtre de 10 s dont
  l'empreinte est la plus proche de l'empreinte moyenne du morceau (la plus
  représentative), étendue à 15 s, décodée, rééchantillonnée, encodée en Opus
  mono 48 kbit/s dans un cache (`extraits/<hash>.opus`, ~90 Ko par morceau,
  ~2,5 Go pour 27 000 : **à mesurer**, et à rendre optionnel si trop lourd ;
  repli : décodage à la volée avec saut à l'offset mémorisé en base).
- **Mélange.** Une source « aura » à trois voix fondues : gain en
  `1 / (1 + (d / d0)²)` selon la distance à l'écran, panoramique selon l'angle,
  fondu de 300 ms à l'entrée et à la sortie d'une voix (comportement « entrer /
  sortir d'une zone » de SonicMaps : *reprendre* là où on l'avait laissée si on
  y revient dans les 30 s). Plafond global bas (−18 dB sous le morceau
  principal en balade ; seul en exploration libre).
- **Calage sur les temps** (idée *Beat Sync* de SonicMaps, et la grille de
  pulsation existe dans Éditer) : facultatif, pour plus tard. Les voix de
  l'aura ne se calent pas ; la transition de carrefour (P2) pourrait se faire
  sur un temps.
- **Latence cible** : moins de 150 ms entre l'arrêt du curseur et le son. Une
  interrogation spatiale côté interface (les positions sont déjà chargées
  pour la carte) et une commande `aura(ids, gains, pans)` vers le moteur, pas
  de recherche de voisins côté moteur à chaque mouvement.
- **Bascule.** Un haut-parleur sur la carte, éteint par défaut en exploration
  libre (le clic reste silencieux, décision du 17 août), allumé par défaut en
  balade. Désactivé quand un morceau joue au premier plan hors balade.

**Critères.** Latence mesurée et journalisée. Aucune coupure ni craquement
lors des entrées et sorties de voix. La consommation CPU de l'aura (trois
décodages Opus) est mesurée sur la machine de François.

**Pièges.** Ne pas passer par la file du `Player` : l'aura est une sortie
parallèle. Vérifier qu'une seconde sortie `rodio` n'introduit pas de dérive
avec la première, ou mélanger dans la même sortie. Le cache d'extraits doit
suivre les renommages et suppressions (même mécanisme que les autres caches
par empreinte de fichier).

### Prompt P4

```text
Lis CLAUDE.md, docs/rust-audio-stack.md, docs/plan-balade.md (contraintes communes et P4),
crates/player/src/multipiste.rs et crates/analysis/src/lib.rs.

Étape 1 (mesure, pas d'interface) : ajoute la sélection de la fenêtre la plus représentative
de chaque morceau (empreinte de fenêtre la plus proche de l'empreinte moyenne) et un banc qui,
sur 500 morceaux, mesure : temps d'extraction d'un extrait de 15 s, taille en Opus mono
48 kbit/s, latence entre la demande et le premier échantillon pour (a) un extrait en cache,
(b) un décodage à la volée avec saut. Reporte les chiffres au journal et choisis entre cache
et décodage à la volée (ou les deux) avant d'aller plus loin. Si une dépendance d'encodage
Opus est nécessaire, vérifie-la selon rust-audio-stack.md et cargo deny.

Étape 2 : une source « aura » à trois voix dans crates/player (réutilise le principe de
multipiste.rs : une seule source qui somme, niveaux atomiques), gain selon la distance,
panoramique selon l'angle, fondus de 300 ms, reprise d'une voix revisitée dans les 30 s.
Commande Tauri `aura(voix: [{id, gain, pan}])` ; l'interface calcule les voisins à l'écran
(positions déjà chargées) et l'appelle à l'arrêt du curseur. Haut-parleur sur la carte
(éteint par défaut hors balade, allumé en balade, sous le morceau principal). Branche aussi
les extraits des carrefours de P2 sur cette source.
Mesure la latence et le CPU, verifier.sh, audit-interface.sh, journal, suite.md, tableau.
```

---

## P5 — Traces et brouillard

**Objectif.** Que la carte montre où l'on est passé et ce qui reste inconnu.
L'inconnu de sa propre bibliothèque devient un territoire à dissiper.

**Ce qui existe.** La table `ecoute(chemin, le)` ; la requête de
`crates/core/src/db/carte.rs` sait déjà joindre la dernière écoute à chaque
morceau (`jours_depuis_ecoute`) ; le curseur temporel montre déjà le mécanisme
`setFilter` / `setPaintProperty` et le style « vacant ».

**Conception.**

- **Brouillard.** Les bâtiments dont le morceau n'a jamais été écouté reçoivent
  un voile (opacité réduite, teinte du papier). Une propriété de tuile
  `ecoute: bool` serait figée au moment de la génération : préférer un état
  côté interface (`feature-state`, si un identifiant stable par bâtiment est
  disponible ; sinon un filtre par liste d'identifiants rafraîchi à chaque
  morceau fini). **Trancher en lisant `carto-ville.md`** : le curseur temporel
  avait écarté `feature-state` faute d'identifiant stable. Ce point est
  peut-être à lever ici.
- **Sentiers battus.** Les tronçons de rue empruntés pendant des balades
  s'épaississent et se teintent légèrement avec le nombre de passages. Il faut
  pour cela enregistrer les balades : une table `balade(id, debut)` et
  `balade_pas(balade, rang, morceau, sommet_depart, sommet_arrivee, choisi)`
  (une migration). `choisi` dit si le pas vient d'un choix ou du défaut, ce qui
  nourrit la mesure de P2.
- **Compteur de quartier.** Au survol d'un territoire : « 312 morceaux,
  41 % jamais écoutés ». Une incitation douce, pas un score.
- **Rejouer** (Musicream) : la liste des balades passées dans le panneau de la
  carte, chacune redessinable et rejouable (devient une *chaîne*, P8).

**Critères.** Le brouillard se met à jour à la fin de chaque morceau sans
recharger les tuiles. Les traces ne gênent pas la lecture des rues (audit
d'interface). Vérifier sur la vraie bibliothèque la part de bâtiments sous
brouillard et la lisibilité de la carte avec ce voile.

### Prompt P5

```text
Lis CLAUDE.md, docs/plan-balade.md (contraintes communes et P5), docs/carto-ville.md (section
sur le peuplement rejouable : setFilter/setPaintProperty vs feature-state) et
crates/core/src/db/carte.rs.

1. Migration : tables `balade` et `balade_pas` (rang, morceau, sommets de départ et
   d'arrivée, `choisi`) alimentées par le mode balade (P1/P2).
2. Brouillard : voile sur les bâtiments jamais écoutés, mis à jour à la fin de chaque
   morceau sans régénérer les tuiles. Décide entre feature-state (en rendant l'identifiant de
   bâtiment stable dans les tuiles, si c'est raisonnable) et filtre par liste d'ids ; écris la
   raison du choix dans carto-ville.md.
3. Sentiers battus : épaisseur et teinte des tronçons selon le nombre de passages en balade.
4. Survol d'un territoire : nombre de morceaux et part de jamais écoutés.
5. Liste des balades passées dans le panneau de la carte, redessinables et rejouables.
Mesure la part de bâtiments sous brouillard sur ma bibliothèque, vérifie la lisibilité avec
audit-interface.sh, verifier.sh, journal, suite.md, tableau d'avancement.
```

---

## P6 — Le relief d'énergie et ses courbes de niveau

**Objectif.** L'emprunt propre à Qoyra : une altitude qui a un sens musical,
des courbes de niveau numérotées, et trois gestes (suivre une courbe, monter ou
descendre, ouvrir une courbe).

**Ce qui existe.** Les descripteurs (`crates/analysis/src/descripteurs.rs` :
tempo, tonalité, énergie) ; la coloration par énergie de la carte ;
`relief.rs` pour le monde fictif (ombrage maison, parce que ce monde « n'a pas
de mètres » : Paris en a, donc le `raster-dem` + `hillshade` de MapLibre
redevient possible) ; la sélection au lasso → playlist.

**Conception.**

- **Champ.** Pour chaque pixel de la zone peuplée, l'énergie moyenne des
  morceaux voisins, pondérée par un noyau gaussien (rayon ~120 m), lissée.
  Hors zone peuplée, l'altitude décroît vers zéro (pas de falaise au bord).
  Variantes au choix : tempo, popularité, *familiarité* (écoutes). Une couche
  à la fois, comme les vues d'Islands of Music.
- **Rendu.** Tuiles `raster-dem` (encodage Terrarium ou Mapbox) générées en
  Rust, `hillshade` MapLibre très léger (le plan doit rester lisible) et
  **courbes de niveau** en lignes fines numérotées (10 niveaux d'énergie), en
  tuiles vectorielles (`contour` existe déjà pour les isobandes du monde
  fictif).
- **Suivre une courbe.** En balade, préréglage *à niveau* : la pente (P3)
  pénalise l'écart d'énergie au morceau de départ. Hors balade, molette sur une
  courbe sélectionnée = avancer le long de la courbe, d'un morceau à l'autre
  (le fuseau de Qoyra), avec l'aura (P4) qui suit.
- **Monter, descendre.** Deux préréglages de balade *gravir* et *redescendre* :
  la pente favorise une énergie croissante ou décroissante, avec un plafond de
  pas pour éviter les sauts.
- **Ouvrir une courbe.** Clic sur une courbe puis sur son numéro : la playlist
  des morceaux de la bande d'énergie qu'elle borde, dans la région qu'elle
  enferme (réutilise le lasso).

**Critères.** La carte reste lisible avec le relief activé (audit). Une balade
*à niveau* de 30 min a un écart-type d'énergie nettement plus faible qu'une
balade *dériver* (chiffres au journal). Le passage d'une couche de relief à
une autre se fait par `setStyle`/changement de source, sans régénérer le plan.

### Prompt P6

```text
Lis CLAUDE.md, docs/recherche-balade-musicale.md (§ 1 et § 4 sur Qoyra),
docs/plan-balade.md (contraintes communes et P6), docs/carto-direction.md (relief du monde
fictif), crates/carto/src/relief.rs, crates/carto/src/style.rs et
crates/analysis/src/descripteurs.rs.

Implémente le relief d'énergie sur le plan réel : champ lissé de l'énergie des morceaux
(variantes tempo, popularité, familiarité), tuiles raster-dem générées en Rust, hillshade
MapLibre léger, courbes de niveau numérotées en tuiles vectorielles (réutilise ce qui produit
les isobandes du monde fictif), option d'affichage avec choix de la grandeur. Puis les gestes :
préréglages de balade « à niveau », « gravir », « redescendre » (dans la pente de
crates/carto/src/balade.rs) ; molette sur une courbe sélectionnée pour avancer d'un morceau
à l'autre le long de la courbe ; clic courbe puis numéro = playlist de la bande d'énergie
dans la région enfermée (réutilise le lasso).
Mesure l'écart-type d'énergie d'une balade « à niveau » vs « dériver » (exemple de P0),
audit-interface.sh, verifier.sh, journal, suite.md, tableau d'avancement.
```

---

## P7 — Rendre les rues continues à l'oreille (si P0 le demande)

**Objectif.** Si marcher dans une rue revient à tirer au hasard, corriger le
placement **sans toucher au récit chronologique**.

**Conception.**

1. **Sérier dans la cellule.** Le contenu de chaque cellule de
   `croissance.rs` ne change pas (date, popularité, style l'ont décidé). Seule
   l'attribution des bâtiments à l'intérieur change : on ordonne les morceaux
   par un chemin court dans l'espace CLAP (plus proche voisin puis 2-opt, ~50
   morceaux, en gardant chaque album contigu), et on déroule cet ordre le long
   des façades, rue par rue.
2. **Raccorder les cellules.** Choisir le sens de déroulement de chaque cellule
   pour que ses extrémités soient proches, à l'oreille, des cellules voisines
   sur la même rue.
3. Seulement si 1 et 2 ne suffisent pas : renforcer le terme de style du score
   de cellule à la frontière.

**Critères.** E1 (marche aléatoire) se rapproche nettement du sonique. Les
mesures de `carto-ville.md` ne bougent pas au-delà du bruit : ρ année ↔ coût
de voirie ≈ 0,83, 64 % de voisins de même famille, popularité des artères.

### Prompt P7

```text
Lis CLAUDE.md, docs/carto-ville.md (section « La croissance »), docs/plan-balade.md (P0 et
P7) et le résultat de P0 dans docs/journal.md.

Dans crates/carto/src/croissance.rs, garde le contenu de chaque cellule et change seulement
l'attribution des bâtiments à l'intérieur : ordre des morceaux par chemin court dans
l'espace CLAP (plus proche voisin + 2-opt, albums contigus), déroulé le long des façades, sens
de chaque cellule choisi pour raccorder ses extrémités aux cellules voisines sur la même rue.
Derrière un réglage (CR_SERIATION=0/1 dans croissance_apercu) pour comparer.
Relance croissance_apercu et continuite_balade avant/après : E1, ρ année ↔ coût, voisins de
même famille, popularité des artères. Si E1 ne s'améliore pas nettement, arrête-toi et
rapporte. Sinon, régénère les tuiles, verifier.sh, journal, carto-ville.md, suite.md,
tableau d'avancement.
```

---

## P8 — Des lieux à soi

**Objectif.** La part de « placé par soi » de Qoyra et de Data Mountain, sans
déplacer les morceaux : des épingles nommées et des chaînes gardées.

**Conception.**

- **Épingle.** Clic droit sur la carte → « Épingler ici » : un nom, une
  couleur, un emoji facultatif. Stockée en base (coordonnées lon/lat, pas un
  morceau : l'épingle survit aux réanalyses). Visible à tous les zooms comme un
  repère, point de départ de « Flâner d'ici ».
- **Chaîne.** Une balade passée (P5) ou un itinéraire peut être **gardé** sous
  un nom. Il devient un tracé permanent sur la carte, dans la couleur choisie,
  rejouable tel quel, ou comme point de départ d'une nouvelle balade.
  Lien naturel avec les playlists existantes : une chaîne *est* une playlist,
  avec en plus une géométrie.
- **Pas de déplacement des morceaux.** La stabilité du plan passe avant (§ 3.1
  de la recherche).

**Critères.** Épingles et chaînes survivent à une régénération des tuiles et à
une réanalyse. Elles passent dans la Valise si les playlists y passent (à
vérifier dans `plan-ios-compagnon.md`).

### Prompt P8

```text
Lis CLAUDE.md, docs/plan-balade.md (contraintes communes, P5 et P8), docs/plan-ecouter-v0.2.md
(playlists) et docs/plan-ios-compagnon.md.

Ajoute les épingles (migration : nom, couleur, emoji, lon/lat ; clic droit « Épingler ici » ;
repère visible à tous les zooms ; « Flâner d'ici » depuis une épingle) et les chaînes (garder
une balade passée ou un itinéraire sous un nom : une playlist existante plus sa géométrie,
tracé permanent sur la carte, rejouable). Vérifie que les deux survivent à une régénération
des tuiles et à une réanalyse, et dis si les chaînes doivent suivre les playlists vers la
Valise. Tests, verifier.sh, audit-interface.sh, journal, suite.md, tableau d'avancement.
```

---

## P9 — Des quêtes (facultatif, à juger après P2)

**Objectif.** Pousser le livre-jeu un cran plus loin, seulement si l'usage de
P2 montre qu'on aime choisir. Une **quête** est une destination ou une
condition, jamais une contrainte de temps :

- *rejoindre* : « rejoindre la Seine », « rejoindre mon épingle Dimanche » ;
  la balade garde ses choix, mais la pente penche vers la destination
  (l'itinéraire de voirie actuel, devenu une boussole plutôt qu'un trajet) ;
- *trouver* : « cinq morceaux jamais écoutés du Marais » ; le compteur
  avance, et le brouillard (P5) indique où chercher ;
- *déverrouiller* (le *Enable if* de SonicMaps) : une branche « mystère » au
  carrefour, dont on n'entend pas l'extrait, qui n'apparaît qu'après avoir
  visité un certain quartier.

**Garde-fou.** Aucun score, aucune série à entretenir, aucune notification.
C'est un jeu de promenade, pas une boucle d'engagement.

### Prompt P9

```text
Lis CLAUDE.md, docs/plan-balade.md (principe, P2, P5, P9) et l'entrée de journal qui mesure
l'usage des carrefours (part de choix faits). Si moins d'un carrefour sur cinq donne lieu à
un choix, arrête-toi et propose plutôt d'améliorer la pente par défaut. Sinon, conçois (sans
coder) trois quêtes « rejoindre », « trouver », « déverrouiller » sur la base de la balade
existante, avec les données nécessaires, l'interface minimale et les garde-fous (pas de score,
pas de série, pas de notification). Écris la conception à la suite de P9 dans
docs/plan-balade.md et attends ma validation avant d'implémenter.
```

---

## Ordre et dépendances

```
P0 ──► (P7 si E1 échoue) ──► P1 ──► P2 ──► P3 ──► P5 ──► P8
                               │      │                 
                               │      └──► P4 (extraits des carrefours)
                               └──► P6 (gestes de courbe : après P3 et P4)
                                              P9 : après un usage réel de P2
```

## Avancement

| piste | état | date | où lire |
|---|---|---|---|
| P0 mesure | fait : P7 avant P1 (hors album, la marche est proche du hasard) | 5 nov. 2026 | `journal.md`, « Balade musicale » ; `examples/continuite_balade.rs` |
| P1 promeneur | à faire | | |
| P2 carrefour | à faire | | |
| P3 pentes | à faire | | |
| P4 aura | à faire | | |
| P5 traces et brouillard | à faire | | |
| P6 relief d'énergie | à faire | | |
| P7 sériation | à faire, avant P1 (décidé par P0) | | |
| P8 lieux à soi | à faire | | |
| P9 quêtes | facultatif | | |
