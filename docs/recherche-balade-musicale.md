# La balade musicale : état de l'art et pistes

> Recherche menée le 1ᵉʳ novembre 2026, à partir d'une question de François :
> *pourrait-on améliorer l'exploration par la carte avec une approche proche de
> [Qoyra](https://www.qoyra.app/) ? La carte de Paris reste limitée pour
> découvrir de nouvelles musiques ; on n'a pas réussi à recréer la notion de
> balade musicale dans un environnement.*
>
> **Rien n'est implémenté ici.** Ce qui est observé ou mesuré est signalé comme
> tel ; le reste est un relevé de lecture. Complète `carto-ville.md`,
> `carto-direction.md`, `carto-google-maps.md` et `carto-anneau.md`, sans les
> remplacer.

## En une phrase

La carte de Paris est un bon **plan** (on s'y repère, elle ne bouge pas) mais
un mauvais **terrain de promenade**. On la regarde d'en haut, elle reste
silencieuse quand on la parcourt, et le pas suivant dans une rue n'est pas
le pas suivant à l'oreille. Une balade demande l'inverse sur chacun de ces
trois points. La littérature dit lesquels comptent et dans quel ordre.

---

## 1. Qoyra, regardé de près

Testé dans le navigateur le 1ᵉʳ novembre 2026 (v0.9.1 bêta, build du 31 oct.,
démo sans compte, 25 notes). Peu documenté ailleurs : ce qui suit vient de
l'application elle-même.

**Ce que c'est.** « Put your notes on a map. » Un carnet de notes dont le
support est un **paysage topographique** sombre : des courbes de niveau
numérotées (03, 06, 09, 12…), des sommets, une vue 2D et une vue 3D inclinée.
Le constat de départ tient en deux lignes : *« You find things by their
surroundings. Lists and folders have none. »*

**Quatre gestes, quatre mots** (chacun adossé, sur la page d'accueil, à une
référence de psychologie cognitive) :

| geste | objet | référence citée |
|---|---|---|
| *Put* : écrire une note la pose sur la carte | **point** | double codage (Paivio) |
| *Connect* : clic droit glissé d'un point à l'autre, **seulement s'ils sont sur la même ligne** | **chaîne** | réseaux sémantiques |
| *Move* : on fait glisser un point, ou le **fuseau** (*spindle*) qui tient une chaîne, **le long d'une courbe de niveau** ; molette = avancer sur la courbe | **courbe de niveau** | hiérarchies conceptuelles |
| *Explore* : les courbes façonnent le relief ; cliquer une courbe puis son numéro ouvre tout ce qu'elle contient | **paysage** | cartes cognitives (Tolman) |

**Ce qui le distingue de notre carte**, et qui compte pour la suite :

1. **C'est l'utilisateur qui place.** Le relief découle de son rangement, il
   n'est pas calculé à sa place. La mémoire spatiale se forme parce qu'on a
   soi-même posé la chose (voir § 3.1).
2. **La courbe de niveau est un rail.** On ne se déplace pas librement en 2D :
   on glisse *le long* d'une ligne, à la molette. Un geste à une dimension dans
   un espace à deux. C'est exactement la forme d'une balade : un chemin, et des
   embranchements.
3. **L'altitude a un sens** (le niveau dans la hiérarchie), et chaque courbe
   est un contenant qu'on peut ouvrir.
4. **Le paysage est abstrait**, pas une vraie ville. Il ne porte aucune
   géographie empruntée, seulement des lignes qui naissent du contenu.

Qoyra est un outil de *rangement et de rappel*, pas de *découverte*. Il n'a ni
son ni recommandation. Ce qu'on peut lui prendre, c'est la grammaire (point,
chaîne, rail, relief qui a un sens), pas la fonction.

---

## 2. Diagnostic : pourquoi Paris ne fait pas balade

Tiré de nos propres documents et mesures. Aucune hypothèse nouvelle.

**a. La carte sert le repérage, et c'est tranché ainsi.** Le recouvrement
k = 12 entre voisins musicaux et voisins géographiques est de **8 %** (médiane
0 %, `carto-ville.md`, V8). La croissance chronologique a porté à **64 %** la
part des voisins de même famille, mais à l'échelle du morceau, deux adresses
voisines ne se ressemblent pas plus que le hasard ne le voudrait. Le 23 août, on a
explicitement choisi : *le plan sert à se repérer, l'exploration fine reste au
graphe des voisins*. Résultat : marcher dans une rue, c'est traverser un
quartier de style cohérent, mais chaque pas est un saut.

**b. Le modèle est Google Maps, c'est-à-dire l'itinéraire.** Départ, arrivée,
profil, durée : on *planifie* un trajet, puis on écoute une playlist. C'est le
contraire de la flânerie, où l'on décide à chaque coin de rue. Nos quatre
modes de chemin (`ui-spec.md`) répondent à « emmène-moi de là à là » ; seule
l'*errance* répond à « promène-moi », et elle vit dans le graphe des voisins,
pas dans la ville.

**c. La carte est muette.** Décision du 17 août, révisée depuis : le clic
sélectionne, il ne joue pas ; écouter passe par ▶ dans l'inspecteur.
Cohérent pour un outil de pointage. Fatal pour une promenade : on ne *sent*
rien en bougeant.

**d. Le temps de la ville n'est pas le son.** La croissance place par date
(ρ = 0,83 année ↔ coût de voirie) et par popularité (artères). C'est un récit
historique très réussi. Mais la dimension qu'on parcourt en marchant du centre
vers la périphérie, c'est **le temps**, pas l'ambiance.

**e. Rien n'indique où l'on est déjà passé ni ce qui reste inconnu.**
L'historique d'écoute existe (table `ecoute`, 4 oct.), « jamais écoutés »
est câblé dans Lama. Mais la carte ne montre aucune trace, et aucun brouillard
sur l'inexploré.

---

## 3. Ce que dit la littérature

Neuf enseignements, du plus solide au plus spéculatif. Pour chacun : ce qui est
établi, et ce qu'on en tire.

### 3.1 La mémoire spatiale fonctionne, si l'emplacement est stable

*Data Mountain* (Robertson et al., UIST 1998) range des pages web sur un plan
incliné, et l'utilisateur les place lui-même. Revenus **quatre à six mois
plus tard**, neuf sujets retrouvent leurs 100 pages sans ralentissement
significatif. Sans les vignettes, la gêne disparaît après deux blocs d'essais
(Czerwinski et al., INTERACT 1999 ; n = 9, à lire avec prudence). La synthèse
de Scarr, Cockburn et Gutwin (2013) fait de la **stabilité des emplacements**
la condition première pour exploiter la mémoire spatiale.

→ **Paris a raison sur ce point** : le plan ne bouge pas, et l'ancrage aux
monuments sert exactement cela. À garder. Ce que Qoyra ajoute, c'est le
placement par soi : aujourd'hui, l'utilisateur ne pose rien sur la carte.

### 3.2 La 3D plaît, elle n'aide pas

La même tâche en 2D et en 3D façon Data Mountain ne donne aucune différence de
performance significative. Les participants préfèrent pourtant nettement la 3D
(Cockburn et McKenzie, « 3D or not 3D? », CHI 2001).

→ La 3D peut créer une **ambiance** (§ 5, P6), mais elle ne réparera pas la
balade. Ne pas en faire le chantier principal.

### 3.3 Entendre en bougeant : le son est le sol

C'est le fil le plus constant de la littérature musicale, et celui qui nous
manque entièrement.

- **Sonic Browser** (Fernström et McNamara, ACM TAP 2005) : quand le curseur
  survole des icônes de sons, une *aura* les fait jouer, plusieurs à la fois.
  Avec des flux multiples, dix utilisateurs accomplissent les tâches de
  navigation **significativement plus vite** qu'avec un seul flux.
- **nepTune** (Knees, Schedl, Pohle et Widmer, ACM MM 2006 / IEEE MultiMedia
  2007) : îles de musique en 3D, parcourues à la manette. **Pendant le
  déplacement, les morceaux les plus proches jouent** en 5.1 et donnent
  l'impression du style local.
- **Radio Garden** (Moniker et Studio Puckey, 2016) : on fait tourner un globe,
  et la station sous le viseur joue aussitôt. Pas de frontières ni de noms de
  pays, « pour inciter à se perdre ».
- **Proteus** (Ed Key et David Kanaga, 2013) : un jeu de marche sur une île où
  chaque élément du paysage a son son. Se déplacer, c'est jouer la musique.
- Contre-exemple instructif : **EmoMTB** (Melchiorre et al., 2023), une ville
  de 436 000 morceaux en tours, **ne joue rien pendant le déplacement**. Il
  faut rester deux secondes au-dessus d'un bloc. Les retours qualitatifs (n = 8)
  saluent la découverte, mais jugent l'ensemble « simple ».

→ **Une carte musicale qui se parcourt en silence n'est pas un environnement.**
C'est la plus grosse différence entre ce que nous avons et toutes les
références qui donnent un sentiment de lieu.

### 3.4 Le flux et l'accroche : découvrir sans chercher

*Musicream* (Goto et Goto, ISMIR 2005) fait **défiler** des disques tirés de la
collection (*streaming*). On en **accroche** un pour attirer les morceaux
semblables (*sticking*), on range ses trouvailles en méta-playlists, et une
**machine à remonter le temps** rejoue n'importe quel état passé. Le constat qui
fonde l'outil : une recherche par l'exemple rend toujours la même liste, et
ferme la porte à l'inattendu.

→ Une balade a besoin d'un **défilement qui avance tout seul**, et d'un geste
pour bifurquer. Le choix est permanent, jamais obligatoire.

### 3.5 Le flâneur : orientation, continuité, sérendipité

Dörk, Carpendale et Williamson (CHI 2011) proposent le **flâneur de
l'information** : curieux, critique, créatif, plutôt que l'usager « en manque
d'information » des modèles classiques. Dörk en tire trois qualités pour une
interface qui se parcourt :

- **orientation** : savoir qu'on peut revenir rend libre de s'éloigner ;
- **continuité** : chaque vue se rattache à la précédente, les transitions sont
  animées ;
- **sérendipité** : ce qu'on n'a pas cherché, et qui relance.

Il plaide aussi contre la vue d'ensemble d'abord : *commencer n'importe où, au
niveau de la rue*, et se servir des données comme points d'appui.

→ Paris fournit l'orientation. Il nous manque la continuité (le pas suivant
s'entend comme un saut) et la vue « au niveau de la rue ». Cela recoupe la
distinction classique de Siegel et White (1975) entre trois connaissances de
l'espace : les **repères**, le **trajet** et la **vue d'ensemble**. Notre carte
ne donne que la vue d'ensemble. La balade construit la connaissance du
trajet.

### 3.6 L'odeur de l'information, au carrefour

La théorie du butinage de l'information (*information foraging*, Pirolli et
Card, 1999) appelle **odeur** (*scent*) les indices qui laissent deviner, avant
de s'y engager, ce qu'un chemin rapporte. Sans odeur, on n'avance pas ou on
avance au hasard.

→ À un carrefour, chaque rue doit **s'annoncer** avant qu'on y entre : par un
nom (on l'a déjà, la rue porte l'artiste dominant), une couleur (la famille) et
surtout **un son** (§ 3.3).

### 3.7 La trace : ce qu'on a parcouru reste visible

*Edit wear and read wear* (Hill, Hollan et al., CHI 1992) : les objets
numériques devraient s'user comme le papier, et porter la trace de leur usage.
*MoodPlay* (Andjelkovic, Parra et O'Donovan, UMAP 2016) fait marcher un avatar
dans un espace d'humeurs. Ses positions successives forment une **traîne** qui
pondère les recommandations, avec une décroissance sur les pas anciens.
**Mise en garde mesurée** : dans leur étude (240 sessions valides), la condition
avec traîne n'améliore rien de net et ajoute de la charge. Les auteurs
concluent que trop d'interaction nuit.

→ La trace doit être **montrée, pas manipulée** : des sentiers battus visibles,
pas un objet de plus à éditer.

### 3.8 La dérive et *The Naked City* : la référence est déjà parisienne

La **dérive** situationniste consiste à « se laisser aller aux sollicitations
du terrain et des rencontres qui y correspondent » (Debord, *Théorie de la
dérive*, 1956). La **psychogéographie** étudie
comment le milieu agit sur les émotions de qui le traverse. *The Naked City*
(Debord, 1957) en est la carte : des **morceaux découpés d'un plan de Paris**,
chacun une **unité d'ambiance**, flottent sur le blanc et sont reliés par des
**flèches rouges**. Ces flèches figurent les « pentes psychogéographiques »,
les tendances spontanées d'orientation de qui traverse ce milieu
(McDonough).

→ Il existe déjà un plan de Paris, réarrangé par ambiances et parcouru par
pentes, qui sert à flâner et non à se rendre quelque part. Nos quartiers sont
des unités d'ambiance. Il manque **les pentes** : les liaisons sonores
privilégiées d'un quartier à l'autre, que la balade suit par défaut.

### 3.9 Pourquoi ces interfaces n'ont pas pris, et ce qu'il faut en retenir

La synthèse de Knees, Schedl et Goto (TISMIR 2020) est sans appel : les
interfaces cartographiques de la « phase 1 » (Islands of Music, nepTune…)
**n'ont aucune adoption commerciale**, et leurs évaluations sont rares et
souvent anecdotiques. L'écoute s'est déplacée vers l'expérience *lean-back*
(playlists automatiques, recommandation). Les interfaces de la phase 2
« demandent une exploration plus active ». Seul Songrium, qui intègre la
découverte à l'écoute même, a été adopté hors du laboratoire.

→ **C'est l'enseignement décisif.** Une balade qu'il faut piloter à chaque pas
restera un jouet. La balade doit **avancer seule**, comme une radio, et la
carte doit la *montrer* ; l'utilisateur bifurque quand il en a envie. C'est
Musicream (§ 3.4) et Radio Garden (§ 3.3), pas nepTune.

### En complément : changer de point de vue sans changer de carte

*Islands of Music* (Pampalk, 2001) puis Pampalk, Dixon et Widmer (2004) proposent
plusieurs « vues » d'une même collection (timbre, rythme, rangement personnel).
On passe de l'une à l'autre en déplaçant la souris dans un triangle : la carte
se **déforme graduellement**. *Every Noise at Once* (McDonald) donne un sens à
ses axes (mécanique ↔ organique, dense ↔ aérée) et propose pour chaque genre
une carte inverse, « le plus loin d'ici ».

→ Un relief n'a pas besoin d'être unique. Il peut être une couche qu'on change
(énergie, tempo, familiarité) sur un plan qui, lui, ne bouge pas.

### 3.10 Deux projets regardés ensuite (1ᵉʳ nov. 2026)

**[SonicMaps](https://sonicmaps.xyz/)** (Recursive Arts, issu du centre
NOVARS de l'université de Manchester, 2012). Ce n'est pas une carte de
musique mais une plateforme de **promenades sonores géolocalisées** : on
dessine des zones sur un vrai plan et on y attache du son, qui se déclenche
quand le visiteur y entre (GPS dehors, ou un auditeur virtuel qu'on fait
glisser sur la carte à distance). C'est le métier de la balade sonore, et
plusieurs de ses réglages répondent directement à nos pistes :

- *snap-to-path* : la position est **aimantée au chemin** le plus proche. Le
  rail de Qoyra, sous un autre nom, et la façon dont notre promeneur doit
  coller aux rues ;
- **volume selon la distance** au centre d'une zone : l'aura ;
- comportement à la **sortie d'une zone** (pause, continuer, reprendre au
  début) : ce que fait une voix de l'aura quand on la quitte et qu'on y
  revient ;
- règles *Enable if / Disable if* : une zone **se déverrouille** quand on en a
  visité une autre. C'est la mécanique du livre-jeu (§ P9 du plan) ;
- *Beat Sync* : les zones qui se chevauchent sont calées sur une grille de
  tempo commune. Nous avons la grille de pulsation (Beat This!) pour caler une
  transition de carrefour sur un temps.

**[MusicAtlas](https://musicatlas.ai/)** (fondé en 2025 à New York, Neil
Shah). Plateforme commerciale de recherche par similarité sonore (API
`similar_tracks`, connecteurs pour assistants, outils de synchronisation et de
licence). Sa carte (*Explore Maps*) promet d'organiser les morceaux « par son,
style et contexte culturel plutôt que par genres ». Je n'ai pas pu la voir :
elle est derrière une vérification Cloudflare. Ce qu'en disent les pages
publiques : plusieurs modèles complémentaires (son, thèmes des paroles,
contexte) réunis dans un index « multi-représentation ». Pour nous :
inutilisable tel quel (service en ligne, catalogue mondial, contraire à
l'exécution locale), mais l'idée de **combiner plusieurs proximités** (son et
relations entre artistes) nourrit la pente de P3.

---

## 4. Transposer Qoyra

| Qoyra | Rusty Music | ce que ça demande |
|---|---|---|
| point | morceau, à son adresse | existe |
| chaîne (points reliés sur une même ligne) | **enchaînement** : morceaux successifs d'une balade, tracés sur la carte | le tracé d'itinéraire existe ; il manque d'en faire un objet gardé |
| fuseau (tête de chaîne qu'on déplace) | **le promeneur** : la position de lecture sur la carte | nouveau |
| courbe de niveau, qu'on suit à la molette | **iso-énergie** (ou iso-tempo) : suivre une courbe garde l'intensité et fait dériver le style | relief d'un descripteur, nouveau sur Paris |
| ouvrir une courbe et son numéro | « tout ce qui est à ce niveau, dans ce périmètre » → playlist | sélection au lasso existante, à brancher sur la courbe |
| le relief naît du rangement | le relief naît d'un **descripteur choisi** (énergie par défaut) | descripteurs : `crates/analysis/src/descripteurs.rs` |
| on retrouve par l'entourage | lieux personnels et traces (§ 5, P5-P6) | historique `ecoute` existant |

Une remarque technique au passage. `relief.rs` calcule son propre ombrage
parce que le monde fictif « n'a pas de mètres ». **Paris en a.** Le `raster-dem`
et le `hillshade` natifs de MapLibre redeviennent donc utilisables pour un
relief posé sur le plan réel.

---

## 5. Propositions

Détaillées, avec un prompt par piste, dans `plan-balade.md`. Dans l'ordre où
elles devraient être faites. Les deux premières décident si
les suivantes valent la peine.

### P0 — Mesurer la continuité avant de construire (½ journée)

Fidèle à la règle du projet : un chiffre avant une interface.

- **E1, continuité de marche.** Distance CLAP moyenne entre deux morceaux
  consécutifs pour (i) une marche aléatoire sur la voirie (`reseau_reel.rs`),
  morceaux pris dans l'ordre des façades (`facades.rs`) ; (ii) les trois
  profils d'itinéraire ; (iii) le chemin sonique (`Graphe::sonique`) ;
  (iv) un tirage au hasard. Si (i) ≈ (iv), aucune interface de balade ne
  sauvera la ville telle qu'elle est placée, et P7 passe en premier.
- **E2, intérêt des carrefours.** Pour chaque intersection, entropie des
  familles des rues qui en partent. Un carrefour où toutes les branches se
  ressemblent n'offre pas de choix, et un carrefour sans lien sonore entre ses
  branches n'offre que des sauts. On veut la zone intermédiaire.

Un `examples/continuite_balade.rs` dans `crates/carto`, sur le modèle de
`croissance_apercu`.

### P1 — Le promeneur : la lecture *est* une marche

Un repère sur la carte, à l'adresse du morceau en cours. Il **avance le long de
la rue au rythme de la lecture** : la durée du morceau couvre la longueur de
sa façade. Au bout du morceau, il arrive à l'adresse suivante. La carte suit
sans à-coup (la continuité de Dörk). Par défaut, la balade continue seule : tout
droit si possible, sinon par la pente la plus douce (P3). C'est la radio, avec
une position.

Différence avec l'itinéraire actuel : rien n'est planifié. La playlist n'existe
qu'à mesure qu'on marche. Une durée cible peut rester en option (« ramène-moi
vers le centre en 40 min »).

Réutilise `reseau_reel.rs` (graphe des rues) et `facades.rs` (morceau ↔ voie),
ainsi que le traçage de route de `js/itineraires.js`. Nouveau : un état
« balade » côté moteur, qui donne le morceau suivant à la demande au lieu d'une
liste calculée d'avance.

### P2 — Le carrefour : sentir avant de tourner

À l'approche d'un carrefour (les 20 dernières secondes du morceau), les rues
qui en partent **s'annoncent** : leur nom d'artiste, leur couleur de famille
et, au survol ou aux flèches, **quelques secondes du premier morceau de la
rue** en sourdine. Sans geste, le promeneur prend la pente par défaut. Une
flèche ou un clic choisit une autre branche. C'est l'odeur de Pirolli et Card,
et l'accroche de Musicream.

### P3 — Les pentes : *The Naked City* sur nos quartiers

Calculer, entre territoires voisins, une **pente** : la proximité sonore
moyenne entre les morceaux qui bordent leur frontière, plus un terme de
nouveauté (part de jamais-écoutés de l'autre côté). Les dessiner comme les
flèches rouges de Debord, à petite échelle. Ce sont elles que la balade suit
par défaut aux carrefours : elle dérive vers ce qui sonne proche et neuf.

C'est aussi la version « ville » de l'*errance* (`Graphe::errance`), qui
marche aujourd'hui dans le graphe des voisins et non dans les rues.

### P4 — L'aura : entendre la carte

Au survol de la carte, ou autour du promeneur, les **deux ou trois morceaux les
plus proches jouent ensemble, très bas, spatialisés** (panoramique selon la
direction), et le plus proche domine. Fernström et McNamara mesurent un gain
net avec plusieurs flux. nepTune et Radio Garden en font le cœur de
l'expérience.

Faisabilité :

- le mélange de plusieurs sources échantillon par échantillon existe déjà
  (`crates/player/src/multipiste.rs`, conçu pour les stems) ; `rodio` 0.22
  fournit aussi un lecteur spatial (`spatial_player`) ;
- la latence est le vrai sujet. Il faut des **extraits pré-découpés** (15-20 s,
  la fenêtre la plus représentative selon CLAP, décodés à l'analyse). Objectif :
  moins de 150 ms entre le survol et le son ;
- **une décision à rouvrir** : « le clic ne joue pas » (17 août) reste vrai,
  mais le *survol* deviendrait audible. Il faut une bascule visible (un
  haut-parleur sur la carte), désactivée pendant qu'un morceau joue en premier
  plan, sauf en mode balade.

### P5 — Traces et brouillard

Les rues parcourues (historique `ecoute`) s'éclaircissent comme des **sentiers
battus**. Les bâtiments jamais écoutés restent sous un léger **voile**, comme
le brouillard des jeux d'exploration. La balade le dissipe. On donne ainsi à
voir le « jamais écouté » que Lama sait déjà interroger, et l'inconnu de sa
propre bibliothèque devient un territoire. Le geste existe déjà : le style
« vacant » et `setFilter` / `setPaintProperty` du curseur temporel
(`carto-ville.md`).

Conforme à 3.7 : la trace se **voit**, elle ne s'édite pas. Une « machine à
remonter le temps » façon Musicream (rejouer la balade de mardi) est une option,
pas un prérequis.

### P6 — Le relief d'énergie, et ses courbes de niveau (l'apport propre de Qoyra)

Une couche facultative : une altitude **qui a un sens musical**, l'énergie par
défaut (tempo et « familiarité » en variantes, à la manière des vues de
Pampalk). Ombrée par MapLibre (le `hillshade` natif marche sur Paris) et
coupée de **courbes de niveau numérotées**.

Trois gestes repris de Qoyra :

- **suivre une courbe** : la balade avance à énergie constante, et le style
  dérive. À la molette, comme le fuseau de Qoyra ;
- **monter / descendre** : une balade qui gravit (montée en intensité) ou qui
  redescend (le soir) ;
- **ouvrir une courbe** : clic sur la ligne puis sur son numéro, la playlist de
  ce qu'elle enferme.

Le profil de « dénivelé » des itinéraires (`carto-direction.md`, popularité le
long du trajet) trouve là un relief réel sous les pieds.

La vue inclinée façon EmoMTB, avec des bâtiments extrudés selon la popularité,
est **possible** (`fill-extrusion`), mais 3.2 dit qu'elle plaira sans aider.
On la garde en ambiance du mode balade, pas plus.

### P7 — Rendre les rues continues à l'oreille (si E1 échoue)

Si la mesure E1 montre que marcher dans une rue revient à tirer au hasard,
corriger le placement **sans toucher au récit chronologique**. Deux leviers,
du moins coûteux au plus coûteux :

1. **sérier dans la cellule** : dans `croissance.rs`, les morceaux d'une
   cellule prennent leurs bâtiments dans l'ordre d'arrivée, chacun au plus
   près du précédent. On garde **le contenu de chaque cellule** (que la date,
   la popularité et le style ont décidé) et on change seulement l'attribution
   des bâtiments à l'intérieur. Les morceaux sont ordonnés par un chemin court
   dans l'espace CLAP (voyageur de commerce approché, quelques dizaines de
   morceaux), puis déroulés le long des façades. Le récit chronologique tient
   à l'échelle de la cellule ; seul l'ordre local change ;
2. **renforcer le terme de style** du score de cellule à la frontière entre
   cellules voisines. Plus cher, et à remesurer (ρ année, 64 % de même
   famille).

### P8 — Des lieux à soi (Qoyra, en petit)

L'utilisateur peut **poser une épingle** (« mon coin du dimanche »), nommer un
lieu, et **garder une chaîne** (une balade réussie devient un tracé
permanent sur le plan, rejouable). Il ne déplace pas les morceaux : la
stabilité du plan (3.1) passe avant. Les épingles deviennent des points de
départ de balade et des repères personnels. C'est la part de « placé par
soi » que Data Mountain et Qoyra montrent décisive pour la mémoire.

### Ce qu'on ne fait pas

- **Pas de déplacement libre en 3D façon jeu** (nepTune, EmoMTB) : préféré, pas
  plus efficace (3.2), et ces interfaces n'ont pas été adoptées (3.9).
- **Pas de traîne éditable** (MoodPlay) : charge accrue, gain non démontré.
- **Pas de changement de support.** Le problème n'est pas Paris. Le plan fait
  ce qu'on lui demande (se repérer). Ce qui manque, c'est le son, le
  mouvement et le choix au pas.

---

## 6. Ordre suggéré

| étape | contenu | coût estimé | décide |
|---|---|---|---|
| 1 | P0 (E1, E2) | ½ j | si P7 passe avant tout |
| 2 | P1 + P2 sans son : promeneur, carrefours, pente par défaut simple | 2-3 j | si « la radio avec une position » plaît à l'usage |
| 3 | P4, l'aura (extraits pré-découpés, mélange) | 2-3 j | le saut qualitatif attendu |
| 4 | P3 pentes, P5 traces et brouillard | 2 j | la découverte dans sa propre bibliothèque |
| 5 | P6 relief d'énergie et courbes | 2-3 j | l'emprunt à Qoyra proprement dit |
| 6 | P8 lieux à soi | 1-2 j | — |

Les coûts sont des ordres de grandeur, pas des engagements.

---

## Sources

Consultées le 1ᵉʳ novembre 2026.

- Qoyra, *Interactive memory map*, v0.9.1 bêta. <https://www.qoyra.app/> (testé dans l'application)
- Robertson, Czerwinski, Larson et al., *Data Mountain: Using Spatial Memory for Document Management*, UIST 1998. <https://bibbase.org/network/publication/robertson-czerwinski-larson-robbins-thiel-vandantzich-datamountainusingspatialmemoryfordocumentmanagement-1998>
- Czerwinski, van Dantzich, Robertson, Hoffman, *The Contribution of Thumbnail Image, Mouse-over Text and Spatial Location Memory to Web Page Retrieval in 3D*, INTERACT 1999. <https://www.microsoft.com/en-us/research/wp-content/uploads/1999/01/interact99.pdf>
- Cockburn, McKenzie, *3D or Not 3D? Evaluating the Effect of the Third Dimension in a Document Management System*, CHI 2001. <https://www.csse.canterbury.ac.nz/andrew.cockburn/2d3d.html>
- Scarr, Cockburn, Gutwin, *Supporting and Exploiting Spatial Memory in User Interfaces*, Foundations and Trends in HCI, 2013. <https://nowpublishers.com/article/DownloadSummary/HCI-046>
- Fernström, McNamara, *After Direct Manipulation — Direct Sonification*, ACM Transactions on Applied Perception 2(4), 2005. <https://pure.ul.ie/en/publications/after-direct-manipulation-direct-sonification/>
- Knees, Schedl, Pohle, Widmer, *nepTune*. <https://www.cp.jku.at/projects/nepTune/>
- Goto, Goto, *Musicream*, ISMIR 2005. <https://staff.aist.go.jp/m.goto/Musicream/>
- Andjelkovic, Parra, O'Donovan, *Moodplay: Interactive Mood-based Music Discovery and Recommendation*, UMAP 2016. <https://dparra.sitios.ing.uc.cl/pdfs/Moodplay_UMAP_2016.pdf>
- Melchiorre et al., *Emotion-aware music tower blocks (EmoMTB)*, International Journal of Multimedia Information Retrieval, 2023. <https://link.springer.com/article/10.1007/s13735-023-00275-8>
- Knees, Schedl, Goto, *Intelligent User Interfaces for Music Discovery*, TISMIR 3(1), 2020. <https://staff.aist.go.jp/m.goto/PAPER/TISMIR202010knees.pdf>
- Pampalk, Dixon, Widmer, *Exploring Music Collections by Browsing Different Views*, Computer Music Journal, 2004. <https://homes.luddy.indiana.edu/donbyrd/N560Site-Fall06/Pampalk-ExploreMusicCollsDiffViews.pdf>
- Dörk, Carpendale, Williamson, *The Information Flaneur: A Fresh Look at Information Seeking*, CHI 2011. <https://dblp.org/rec/conf/chi/DorkCW11.html> ; conférence *From Bird's-Eye Views to Street-Level Data Exploration*, compte rendu : <https://archinect.com/lian/live-blog-marian-d-rk-from-bird-s-eye-views-to-street-level-data-exploration-taking-text-for-a-stroll>
- Hill, Hollan, Wroblewski, McCandless, *Edit Wear and Read Wear*, CHI 1992. <https://bibbase.org/network/publication/hill-hollan-wroblewski-mccandless-editwearandreadwear-1992>
- Pirolli, Card, *Information Foraging*, Psychological Review, 1999 (*information scent*). <https://en.wikipedia.com/wiki/Information_scent>
- Siegel, White, 1975 (repères, trajet, vue d'ensemble), repris dans : <https://www.frontiersin.org/journals/psychology/articles/10.3389/fpsyg.2014.01522/full>
- Debord, *The Naked City*, 1957. <https://situationnisteblog.com/2015/06/07/the-naked-city-illustration-de-lhypothese-des-plaques-tournantes-en-psychogeographique-sic-1957/> ; McDonough, *The Dérive and Situationist Paris* : <https://chtodelat.org/b8-newspapers/12-68/thomas-mcdonough-the-derive-and-situationist-paris/>
- Radio Garden (Moniker, Studio Puckey, 2016). <https://docubase.mit.edu/project/radio-garden> ; entretien : <https://vice.com/en/article/radio-garden-global-radio-jonathan-puckey-interview>
- *The Sound and Music of Proteus — An Academic Case Study*. <https://www.gamedeveloper.com/audio/the-sound-and-music-of-proteus---an-academic-case-study>
- McDonald, *Every Noise at Once*, présentation : <https://popmatters.com/on-wandering-the-paths-of-a-spotify-analysts-mad-music-map-2495400875.html>
- Zheng, Xambó, Bryan-Kinns, *Exploring gestural affordances in audio latent space navigation*, Frontiers in Computer Science, 2025 (les musiciens se font une carte de leurs « points d'intérêt » sonores). <https://www.frontiersin.org/journals/computer-science/articles/10.3389/fcomp.2025.1575202/full>
- SonicMaps (Recursive Arts). <https://sonicmaps.xyz/>
- MusicAtlas, pages publiques (la carte elle-même n'a pas pu être consultée). <https://musicatlas.ai/> ; <https://musicatlas.ai/how-it-works> ; <https://musicatlas.ai/press/>
- `rodio` 0.22, `spatial_player`. <https://docs.rs/crate/rodio/latest/source/src/spatial_player.rs>
