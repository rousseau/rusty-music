# Direction cartographique

> **Support remplacé le 25 août 2026** : la carte n'engendre plus son monde,
> elle emprunte le plan de Paris — voir `docs/carto-ville.md`. Ce document
> décrit la direction artistique du **monde fictif engendré** (littoral par
> nappe de densité, biomes de Whittaker non décidés, six rangs
> d'établissement) : une partie de ce raisonnement survit sous une autre
> forme sur le plan réel (la hiérarchie des voies remplace les six rangs,
> `carto-ville.md` le documente), le reste (littoral, relief, biomes) tombe
> avec la bascule. Reste comme historique du raisonnement et comme direction
> active du chemin de repli (sans ville importée).

Ce que la carte doit donner à voir, et par quels moyens. `CLAUDE.md` y renvoyait
depuis longtemps sans qu'il existe — c'était l'objection O12. Il décrit
maintenant ce qui est fait, pas ce qui est souhaité ; la mécanique est dans
`carto-peuplement-architecture.md`, les mesures dans `journal.md`, la suite dans
`carto-etapes.md`.

## Deux visualisations, et deux seulement

Le mode Explorer offre **le nuage et la carte**, pas davantage.

- **Nuage** — la projection t-SNE dessinée au canevas, un point par morceau.
  C'est la vue analytique : on y voit les 27 000 morceaux tels quels, colorés
  par famille, année, tempo ou énergie.
- **Carte** — les tuiles vectorielles. C'est la vue cartographique : la même
  projection, mais lue comme un pays.

Les deux partagent le même canevas et les mêmes gestes ; `carteGL()` décide
lequel des deux repères gouverne les coordonnées. L'ancien mode « densité »,
qui dessinait la nappe au canevas, a disparu : la carte le fait mieux, et son
code — un millier de lignes — est retiré.

## Le parti pris

**Une carte se lit par ses lieux et ses liens, pas par sa densité.** C'est la
leçon qui a coûté le plus cher : une nappe de densité habillée en carte reste
une carte météo. Il a fallu lui donner un bord, des lieux et un réseau pour
qu'on puisse s'y repérer.

Quatre couches, dans cet ordre, et chacune répond à une question du regard :

| couche | question |
|---|---|
| la mer et le littoral | *où s'arrête le monde ?* |
| le relief | *où est-ce dense, où est-ce vide ?* |
| les territoires | *de quoi cette région est-elle faite ?* |
| les établissements et les routes | *où suis-je, et où puis-je aller ?* |

## L'apparence : claire, comme les cartes qu'on sait lire

Une première version reprenait le thème sombre de l'application : fond presque
noir, territoires en aplats saturés, routes orange. Le résultat restait une
**visualisation de données habillée en carte**.

Les cartes qu'on sait lire — OpenStreetMap, Plans, Google Maps — partagent trois
traits, et ce sont eux qu'on reprend :

1. **un fond clair** : terre crème `#F2EFE9`, eau bleu pâle `#AAD3DF` ;
2. **la couleur est rare et porte un sens** : les régions ne sont plus que des
   lavis à 18 %, pas des aplats francs. Un territoire se distingue de son voisin
   sans crier ;
3. **le réseau domine le graphisme** — c'est lui qu'on suit du regard, et c'est
   pour cela qu'il est le seul élément vraiment coloré : rose pour les
   autoroutes, sable pour les nationales, aux teintes d'OSM.

Deux conséquences moins évidentes : l'ombrage du relief tombe à 30 % d'opacité
— au-delà, il grise la carte et mange les routes — et les lieux deviennent des
**cercles blancs cernés de sombre**, le symbole de lieu le plus universel et le
seul qui reste lisible sur tous les fonds.

## La mer et le littoral

La nappe de densité globale — celle qui n'appartient à aucune famille — dessine
la terre. Elle était calculée depuis longtemps et **jetée** : les tuiles ne
gardaient que les nappes par famille.

Deux pièges rencontrés, tous deux invisibles au raisonnement :

- **les bandes d'isovaleur sont des anneaux emboîtés, pas des disques.** N'en
  garder qu'un laissait la mer transparaître au milieu des continents. Empilées
  d'une seule teinte, elles pavent la terre sans se recouvrir ;
- **la grille de densité fait 1 024 cellules pour le monde entier.** Dès le zoom
  4 une tuile n'en couvre plus que 64, et les marches d'escalier se voient. Deux
  passes de Chaikin en font une côte — mais seulement à partir du zoom 3 :
  au-delà, le lissage ne fait que quadrupler le poids de la tuile.

## Le relief

Ombrage de Horn, calculé par nous et non par MapLibre. Les raisons sont dans le
journal ; la principale : le `hillshade` de MapLibre déduit la pente de la
taille d'un pixel **en mètres**, et notre monde n'en a pas.

Le noyau du relief n'est pas celui des territoires : 0,05 contre 0,02. À la
valeur des contours, l'ombrage ressemble à du papier froissé — la nappe y porte
tout le détail des 27 000 morceaux.

## Les toponymes

Trois échelles de noms, et **c'est leur hiérarchie qui fait la lisibilité** :

| échelle | nom | source |
|---|---|---|
| région | « TRIP HOP · POP » | familles nommées par les genres MusicBrainz |
| établissement | « LED ZEPPELIN », « Cypress Hill » | l'artiste **fondateur** du lieu |
| habitant | le titre | les tags |

Un lieu porte le nom de qui l'a fondé : c'est la seule source dont on dispose,
et elle a du sens. Les métropoles sont en capitales et en gras, comme les
capitales d'une carte routière ; le reste en romain, décroissant avec le rang.

**MapLibre arbitre seul les collisions.** On ne lui donne qu'un ordre de
priorité (`symbol-sort-key`) : les grands d'abord, les autres s'effacent quand
la place manque. Ne rien écrire de plus est un choix, pas une paresse — un
évitement de collisions fait à la main serait pire et coûterait cher.

## La révélation par échelle

Le cœur du rendu, et non un détail. Six rangs d'établissement, six seuils :

| zoom | ce qui apparaît |
|---|---|
| 0-2 | littoral, territoires, noms de régions, métropoles, autoroutes |
| 3-4 | villes |
| 5 | bourgs, nationales |
| 7 | villages |
| 8-9 | hameaux, morceaux |
| 10+ | fermes isolées, titres |

Le style est **engendré depuis Rust**, à partir des mêmes paliers que les
tuiles : ce qui n'est pas dans la tuile ne peut plus être déclaré dans le style.
Un test refuse toute expression de zoom imbriquée — la faute qui fait rejeter le
style entier sans une ligne d'erreur.

## Le réseau

Quatre classes, mais **deux seulement sont dessinées** : autoroutes et
nationales, 8 030 tronçons sur 261 270. Les secondaires et les sentiers restent
dans le moteur de routage, où ils servent. Une carte routière dessine une
hiérarchie, pas chaque voie : avec elles, chaque morceau tirait ses douze liens
et le résultat était une pelote.

Et une règle qui n'allait pas de soi : **une route n'est une route que si elle
est locale sur la carte.** Le réseau relie des morceaux proches *à l'oreille* ;
la projection ne préserve que les voisinages locaux, et un lien parfaitement
justifié peut traverser tout le planisphère. Un quart des tronçons est écarté du
dessin pour cette raison — sans quoi la carte disparaissait sous les rayures.

## La balade

Le routage, côté interface : un départ, une arrivée facultative, un profil, et
surtout **une durée cible**. « Un itinéraire de 40 minutes » est la demande la
plus naturelle, et c'est celle qu'on sert en premier.

Le trajet s'affiche sur la carte et rend son **dénivelé** — la popularité le
long du chemin, en barres. C'est le profil d'altitude que le document prévoyait :
un trajet par autoroute monte (les morceaux connus), un petit sentier reste bas.

## Ce que la carte ne fait pas, et pourquoi

- **Pas de rotation ni d'inclinaison.** Une carte inventée n'a ni nord ni
  horizon, et l'inclinaison casse la lecture du relief.
- **Pas de répétition du monde.** C'est une île, pas une planète.
- **Pas de biomes de Whittaker.** Le quatrième axe — l'humidité — reste à
  décider ; la palette est hypsométrique en attendant, ce qui suffit à lire.
- **L'artiste n'est pas un lieu.** Une discographie de quarante ans se répartit
  sur des établissements fondés à des décennies d'écart. C'est musicalement
  juste et cela rompt avec l'ancien modèle : « où est Bowie ? » n'a plus de
  réponse géographique, c'est un calque à surligner.
