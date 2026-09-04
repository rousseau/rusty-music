# Génération de cartes : état de l'art, et ce qui nous sert

> **Support remplacé le 25 août 2026** : la question qui a motivé cette
> recherche (« la carte ne ressemble pas assez à une carte ») a depuis reçu
> une autre réponse — ne plus engendrer le monde, emprunter le plan réel de
> Paris (`docs/carto-ville.md`). Les emprunts documentés ici (Voronoï,
> érosion, biomes de Whittaker) visaient à améliorer le **monde fictif** ; ils
> restent valables pour le chemin de repli (sans ville importée) mais ne
> s'appliquent plus au plan de ville réel, qui a une vraie géométrie à la
> place. Conservé comme référence si ce chemin de repli est repris.

Recherche menée le 23 août 2026 pour répondre à une question précise : la carte
ne ressemble pas assez à une carte, et le domaine du jeu vidéo travaille ce
sujet depuis quinze ans. Que peut-on reprendre, et où est le code ?

**Ce qui est mesuré ici est signalé comme tel ; le reste est un relevé de
lecture.**

## Les quatre références du domaine

| projet | apport | code | licence |
|---|---|---|---|
| **Amit Patel — *Polygonal Map Generation for Games*** (2010) | Voronoï + relaxation de Lloyd, élévation depuis la côte, humidité par les rivières, biomes par table | `amitp/mapgen2` | ouverte |
| **Martin O'Leary — *Generating fantasy maps*** (2016) | maillage irrégulier, **érosion simulée**, villes et frontières, **étiquettes par recuit simulé** | `mewo2/terrain` | MIT |
| **Scott Turner — *Here Dragons Abound*** | la facture cartographique au long cours : côtes, lettrage, rendu | billets | — |
| **Azgaar — *Fantasy Map Generator*** | le plus complet, et il assume reprendre les trois précédents : relief, côtes, rivières, biomes, bourgs, routes, étiquettes, États | `Azgaar/Fantasy-Map-Generator` | MIT |

`carto-peuplement.md` citait déjà Patel. O'Leary est la référence qui manquait :
**c'est lui qui traite explicitement du réalisme des formes**, là où Patel traite
de la structure.

## La contrainte qui décide de ce qu'on peut reprendre

Dans un jeu, on **invente** le monde : le relief sort d'un bruit, les villes se
posent où l'on veut. Ici, **les positions sont imposées par la musique** — la
projection, puis le peuplement. D'où un partage net :

- ce qui **invente une disposition** (relief par bruit fractal, tectonique,
  placement des villes, génération des noms) ne nous sert pas ;
- ce qui **met en forme une disposition donnée** s'applique directement.

C'est dans la seconde moitié que se trouve tout ce qui nous manque.

## Les cinq emprunts, par ordre de valeur

### 1. Maillage de Voronoï au lieu des isolignes ★★★

**C'est le correctif des « formes bulbeuses », et il est gratuit.**

Nos territoires et notre littoral sortent de `contour::isobands` : des isolignes
sur une grille gaussienne. Une isoligne d'un champ lissé est ronde par
construction — d'où des taches, jamais des pays. Les quatre références font
autre chose : elles pavent le plan en cellules de Voronoï et déclarent chaque
cellule terre ou mer, d'un territoire ou d'un autre. La frontière suit alors les
arêtes des cellules : **irrégulière par construction**, sans qu'on ait à
fabriquer du désordre.

Et nous avons déjà le semis de points : les 27 042 morceaux.

**Mesuré** avec `voronoice` 0.2 (MIT), sur la bibliothèque réelle :

| relaxations de Lloyd | cellules | sommets | temps |
|---|---|---|---|
| 0 | 27 042 | 162 104 | **0,01 s** |
| 1 | 27 030 | 162 123 | 0,01 s |
| 2 | 27 030 | 162 125 | **0,02 s** |

Négligeable devant les 22 s du réseau. Le crate s'appuie sur `delaunator`, borne
au rectangle voulu et fournit la relaxation de Lloyd d'origine.

Ce que cela changerait, concrètement : le littoral, les territoires et les
agglomérations cesseraient d'être des ovales lissés pour devenir des polygones
aux bords brisés — l'aspect même d'une carte.

### 2. Érosion du champ d'altitude ★★

C'est l'apport propre d'O'Leary, et ce qui distingue ses cartes de celles de
Patel. Notre relief est une somme de gaussiennes : lisse, sans arêtes, sans
vallées. Quelques passes d'érosion y creusent des vallées et dégagent des
crêtes — et nos rivières, qui suivent déjà la pente, s'y installeraient au lieu
de dévaler des flancs réguliers.

**Aucun crate publié ne le fait.** Les implémentations trouvées sont des
programmes complets (`rj00a/heightmap-erosion`, `mustartt/hydraulic-erosion`),
pas des bibliothèques. C'est une centaine de lignes d'un algorithme documenté
— l'érosion par gouttelettes, méthode standard : une goutte descend la pente,
emporte de la matière quand elle accélère, la dépose quand elle ralentit.

À écrire, donc, mais sans rien inventer.

### 3. Biomes de Whittaker ★★

Prévu dans `carto-peuplement-architecture.md`, jamais fait, et c'est ce qui
explique qu'une grande part de la carte soit d'un beige uniforme. Deux axes —
altitude et une seconde propriété — décident d'un biome, donc d'une couleur :
forêt, prairie, lande, roche, neige. La variété de couleur d'une vraie carte ne
vient pas d'une palette plus riche, elle vient de ce que **le sol y est
différent d'un endroit à l'autre**.

Nous avons les propriétés : la brillance couvre 52 % de la bibliothèque,
l'énergie 88 %.

### 4. Côtes détaillées ★

Patel et O'Leary ajoutent du bruit au trait de côte. **Avec le maillage de
Voronoï, on l'obtient sans rien faire** : la côte est la frontière entre
cellules terrestres et marines, et elle est déjà brisée. Point 1 le règle.

### 5. Placement des étiquettes ★

O'Leary le résout par recuit simulé, et c'est élégant. **MapLibre le fait déjà**
— évitement de collisions natif, priorité par `symbol-sort-key`. Rien à
reprendre.

## Ce qu'il ne faut pas reprendre

- **Le rendu « à la plume »** (hachures, lettrage manuscrit) d'O'Leary et
  d'Azgaar : c'est une carte de fantasy. Nous visons OSM et Plans, une carte
  qu'on lit pour se repérer, pas pour rêver.
- **La génération de toponymes.** Les nôtres ont un sens — le nom de l'artiste
  fondateur — et un nom inventé le perdrait.
- **Les États, les cultures, les religions** d'Azgaar : sans équivalent musical.

## Ordre proposé

1. **Le maillage de Voronoï.** Mesuré gratuit, et c'est lui qui change l'aspect.
   Il touche le littoral, les territoires et les agglomérations — donc la moitié
   des couches.
2. **Les biomes.** Peu de code, beaucoup d'effet : la carte cesse d'être beige.
3. **L'érosion.** Le plus de travail, et son effet passe surtout par l'ombrage et
   le tracé des rivières.

## Thèmes de fond de plan

`crates/carto/src/palette.rs` porte le principe des **17 palettes de
[maptoposter](https://github.com/originalankur/maptoposter)** (Ankur Gupta, MIT) :
un jeu de couleurs de fond de plan keyé sur la hiérarchie de voirie OSM, qui se
transpose presque 1:1 sur les slots de `style.rs`. Cinq `Palette` sont livrées —
`osm-clair` (l'originale, défaut) plus `sepia`, `encre`, `nuit`, `bleu-plan` — et
l'interface (mode Explorer, bloc « Fond de plan », visible en mode Carte
seulement) bascule entre elles.

Chaque `Palette` porte **aussi ses 12 teintes de familles**, calées sur son fond
(matées et terreuses pour `encre`, chaudes pour `sepia`, vives sur sombre pour
`nuit`/`bleu-plan`) : sur un fond monochrome, les teintes vives de `--familles`
faisaient des confettis. Ces couleurs-là ne valent **que pour la carte** — bâti
habité, quartiers, points de morceau. Le nuage t-SNE, la légende et le mode
Écoute gardent `--familles` (thème clair/sombre de l'appli). `app.js`
(`FAMILLES_CARTE`) miroite `palette.rs`, comme `--familles` miroitait `style.rs`.

`engendrer_tuiles` écrit un `style-<id>.json` par palette à côté des tuiles ; la
palette n'affecte **que** le style, jamais les tuiles, donc changer de thème ne
régénère rien (`gl.setStyle` qui diffe).

### Le plan de ville, reconstruit couche par couche

`style::couches_ville` (le monde fictif garde `couches_fictif`) empile, du bas
vers le haut : **fond** (terre, Seine, bois/parcs, voirie nue — cinq rangs de
routes, cheveux fins au dézoom, ni liseré ni nom ni frontière, comme
maptoposter) → **quartiers** (lavis de genre, dézoom seulement) → **bâti**
vacant → **overlay** (bâti habité par famille, artistes, albums, titres).
Retirés du plan de ville : `curiosites` et `points-remarquables` (pastilles
brunes plus grosses qu'un bâtiment), `familles-etiquette`, et toutes les couches
du monde fictif sans tuile ici.

Le réseau apparaît dense dès z12 (`tuiles::classe_reelle_visible_des`) : c'est la
texture de mille rues fines qui fait l'aspect poster. Un **halo de petite
couronne** (voirie/eau/verts jusqu'à ~2 km au-delà du périph, jamais habité,
`crates/osm`) donne au **fondu de bordure** de la carte
(`apps/desktop/ui/app.js::poserVignetteCarte`, teinté avec le fond du thème, plein
en vue d'ensemble, éteint à l'échelle de la rue) de la matière à dissoudre.

**Reste à faire** (`docs/…` du plan « rendu par couche ») : étape 2 le bâti
(rendre la trame de Paris visible plus tôt), étape 3 l'overlay musical
sous-couche par sous-couche.

## Note

La carte **fonctionne** dans l'application : 14 essais sur 15, 20 couches,
3,8 s à l'apparition. Un rapport antérieur la disait cassée ; il testait une
interface périmée. Voir `carto-etapes.md`.
