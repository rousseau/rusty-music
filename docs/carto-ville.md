# La carte est un plan de ville

> Remplace la **génération** de monde (`carto-peuplement.md`,
> `carto-peuplement-architecture.md`, `carto-etat-de-lart.md`) comme support de
> la carte. Le modèle du peuplement n'est pas abandonné : il est *réalisé*. Voir
> « Ce que le peuplement devient », plus bas.

La carte ne fabrique plus son monde, elle emprunte celui d'une vraie ville.
**Paris.** Les morceaux, les artistes et les familles musicales viennent s'y
installer.

Raison de fond : la difficulté n'était jamais de placer les morceaux, c'était de
produire un terrain qui *ressemble à une carte*. Une ville existante résout ce
problème par construction — elle a des rues d'épaisseurs variées, un fleuve, des
parcs, des îlots bâtis, une silhouette reconnaissable. Rien de tout cela n'est à
inventer, et rien de tout cela n'était en train de réussir.

## Ce que Paris offre — mesuré

Extrait Geofabrik `europe/france/ile-de-france-latest.osm.pbf`, découpé sur la
limite communale (`admin_level=8`, un anneau de 992 sommets) :

| | |
|---|---|
| tronçons | 61 243 — 3 758 km |
| **rues distinctes nommées** | **7 047** — 2 373 km |
| bâtiments | 93 309 |
| plans d'eau | 482 (la Seine, les canaux, les lacs des bois) |
| espaces verts | 9 246 |
| adresses OSM | 136 661 |
| toponymes `place=*` | 175 |
| lecture + découpe | 9,1 s · base de 40,9 Mo |

Face à la bibliothèque : 27 044 morceaux, **1 082 artistes**, 2 101 albums,
12 familles.

**L'emboîtement est confortable** : 6,5 rues disponibles par artiste, 5 adresses
par morceau. On ne manquera pas de place — on aura au contraire à choisir quelles
rues laisser vides.

### Le découpage sur la commune n'est pas un détail

Le premier essai prenait un rectangle englobant. Il attrapait Boulogne, Ivry et
Saint-Denis — les plus longues « rues de Paris » étaient *Rue de Paris* et
*Autoroute de l'Est*, toutes deux en banlieue — et surtout il perdait la seule
chose qui rend un plan reconnaissable au premier coup d'œil : **la silhouette**.
Découpé sur la vraie limite, le classement des grandes voies devient Rivoli,
Voltaire, Vaugirard, Saint-Germain. C'est Paris.

### Un halo de petite couronne, dessiné mais pas habité

**Révisé.** L'**affectation** reste bornée à la commune (c'est elle qui fait la
silhouette). Mais la **voirie, l'eau et les espaces verts** sont désormais
retenus jusqu'à ~2 km au-delà (Boulogne, Neuilly, Montreuil…), sans jamais
recevoir de morceau. Raison : le rendu de fond façon *maptoposter* fond ses
bords dans le papier, et un réseau qui s'arrête net sur le périphérique n'a rien
à dissoudre — le fondu bute sur un aplat vide. Le halo lui donne de la matière.
`crates/osm::extraire` ne découpe plus `troncons`/`eaux`/`verts` sur la
frontière ; `affectation::rassembler_rues` et `ville::rassembler` écartent de
l'assignation toute rue dont l'essentiel du tracé est hors commune (la règle
« appartient à la ville où elle passe le plus » d'avant). Le **bâti** reste,
lui, borné à Paris — une couronne de bâti alourdirait les tuiles pour rien.

### Les adresses d'OSM ne servent pas

**13 % seulement** portent `addr:street` (18 170 sur 136 661). Impossible donc de
grouper les adresses par rue à partir d'elles. Sans importance : puisqu'on
invente les noms, les vrais numéros n'avaient rien à nous apprendre. **Les
emplacements viennent de la géométrie des rues**, qui est complète, et les
numéros sont semés dessus. Les adresses OSM restent en base pour mémoire.

### Les trottoirs sont écartés

Ils comptaient pour 142 000 des 182 000 tronçons du premier essai. Ce ne sont
pas des rues, ce sont les bords des rues, et les dessiner double chaque voie
d'un liseré parasite.

## Les familles — par genre, pas par k-means

Les familles musicales (les quartiers de l'étage 1) venaient d'un k-means sur
les empreintes CLAP : purement acoustique, aveugle aux genres déclarés.
Mesuré sur la bibliothèque réelle, ça a produit des familles que rien de
commun ne nomme — « Reggae · Rock », deux genres que rien ne rapproche sinon
le hasard de l'entraînement du modèle.

**Remplacé par un vocabulaire fixe et humain**
(`crates/analysis/src/cluster.rs::VOCABULAIRE`) : douze familles reconnaissables
— Rock, Metal, Hip Hop, Électronique, Jazz, Reggae, Soul · Funk, Folk,
Chanson, Classique, Pop, Monde — chacune définie par une liste de genres
MusicBrainz qui s'y rattachent. La liste est **mesurée, pas devinée** : les
genres les mieux votés d'au moins cinq artistes de la bibliothèque réelle
(`hip hop` 130 artistes, `jazz` 81, `alternative rock` 67…).

Deux passes :
1. **Ancrage.** Un morceau dont le genre résolu (même hiérarchie
   qu'auparavant : album MusicBrainz → artiste → tag du fichier,
   `Library::genres_resolus`) figure dans le vocabulaire rejoint directement
   sa famille — aucun calcul de distance.
2. **Repli acoustique.** Pour le reste (genre absent ou hors vocabulaire),
   chaque famille ancrée a un centroïde (moyenne des empreintes de ses
   membres ancrés) ; le morceau rejoint le plus proche — un k-means à une
   itération dont les centres de départ viennent des genres, pas du hasard.

**Repli k-means conservé pour un seul cas** : aucun morceau n'a de genre
résolu du tout (bibliothèque jamais passée par `enrich`). Sans ancre nulle
part, le vocabulaire ne peut rien faire ; `projeter_tout` retombe alors sur
l'ancien k-means, pour ne jamais laisser la carte sans familles.

**Mesuré sur la bibliothèque réelle** : 12 familles, toutes peuplées (534 à
8 038 morceaux), 27 042 morceaux comptés — aucun perdu. Noms obtenus (le
nommage par genre dominant, inchangé, s'applique maintenant à des familles
qui ont déjà un genre cohérent) :

```
8038  Rock · Grunge                 1122  Reggae · Dub
3903  Electronic · Trip Hop          708  Chanson Française · Children's
3541  Hip Hop · Rap                  691  Soul · Funk
2458  Folk · Celtic                  663  Classical · Country & Folk
2182  Alternative Metal · Industrial 534  Afrobeat · Latin
1817  Jazz · Fusion
1385  Pop · Children's
```

Plus de mélange arbitraire : chaque nom associe deux genres du même monde.
Conséquence mesurée sur l'étage 1 : l'erreur de capacité (cible vs obtenue)
tombe de 6 % (8 familles k-means) à **2 %** (12 familles par genre, mieux
réparties en effectif). La mesure de voisinage (objection V1 plus bas) ne
bouge en revanche pas — 7 % contre 8 % — ce qui confirme que ce nombre ne
dépendait pas de la façon dont les familles étaient choisies.

## L'affectation — le vrai travail

> **Peuplement dense du centre vers l'extérieur + ancrage aux monuments
> (3 sept. 2026).** Deux corrections sur retour d'usage (« les morceaux sont
> trop éparpillés dans la ville », « la popularité n'est pas exploitée ») :
>
> 1. **La zone peuplée n'est plus toute la commune.** L'étage 1 mettait le
>    nuage t-SNE à l'échelle de l'étalement de *toute* la voirie (~105 km²,
>    ~70 000 bâtiments) pour ~27 000 morceaux — densité effective 1 bâtiment
>    sur 2,6, et un troisième cercle de repli « n'importe où dans Paris » qui
>    dispersait le reste. Désormais : `ville::preparer` prend le **centre**
>    (`ILE_DE_LA_CITE`, en dur, le crate reste agnostique à la commune) et
>    retient l'**ensemble des `N` bâtiments habitables les plus proches** —
>    `N` = nombre de morceaux à peupler. `affectation::semer_centre` translate
>    le nuage sur ce centre et le met à l'échelle `p95(dist bâtiment→centre) /
>    p95(‖coord t-SNE − barycentre familles‖)` — les deux mesurées sur les
>    points *réellement placés*, pas sur l'étalement des centroïdes de famille.
>    Le `partitionner` et `loger_artistes` ne voient que les rues qui bordent
>    un bâtiment de l'ensemble. À l'étage 3, `batiments_pris` démarre avec
>    **tout ce qui n'est pas dans l'ensemble** : les trois cercles de
>    `loger_dans_batiments` (dernier recours compris) ne peuvent loger que dans
>    l'ensemble, et comme il y a autant de bâtiments que de morceaux, l'ensemble
>    se remplit à 100 % **sans trou par construction**. Conséquence assumée :
>    avec ~27 000 morceaux la zone peuplée couvre le Paris central ; les
>    arrondissements extérieurs et les deux bois restent non bâtis, sauf les
>    monuments-ancres. La zone grandit avec la bibliothèque.
>    `territoires` (aplat de quartier) est clippé à l'enveloppe du tissu peuplé,
>    pas à un cercle.
>
> **Zone peuplée par coût de voirie, pas par disque (mis à jour).** Premier
> jet : « les N bâtiments les plus proches à vol d'oiseau » — un disque, la
> voirie ne contraignait rien. Corrigé : `ville::preparer` garde désormais les
> `N` bâtiments au plus faible **coût de déplacement sur la voirie** depuis
> l'île de la Cité (`cout_voirie::couts_batiments` → `reseau_reel::Graphe`
> pondéré par `cout_voirie::friction`). Une avenue « rapproche » ~3× plus qu'une
> rue résidentielle : la frontière prend une forme d'étoile qui suit les axes
> (Grands Boulevards, Rivoli, Sébastopol, avenues de l'Étoile / Nation /
> Bastille). Repli euclidien (`batiments::n_plus_proches`) si le graphe n'est
> pas exploitable. Un terme organique lissé (`cout_voirie::AMPLITUDE_ORGANIQUE`,
> ±35 %, hachage déterministe de la position) fait **serpenter** la frontière au
> lieu de suivre une courbe de niveau propre. **`cout_voirie::friction` +
> `AMPLITUDE_ORGANIQUE` sont les boutons de réglage de la forme** ;
> `cargo run --example cout_voirie` la visualise et compare les deux zones.
>
> Limite qui reste : le **cœur** de la zone est encore assez rond — le centre
> historique de Paris est une trame dense de voies toutes comparables, un coût
> depuis un seul point s'y propage à peu près uniformément. Les *bras* suivent
> bien les radiales (p95 du rayon +33 %, max +70 % vs disque, ~24 % des
> bâtiments changés). Pour casser aussi le cœur : partir de plusieurs foyers, ou
> passer au découpage en corridors le long des boulevards (points 2-3, non
> faits).
>
> 2. **Étage 0 — les artistes les plus populaires filent aux monuments.**
>    `crate::ancrage` : `track_popularite.relative` (jusque-là jamais lu par la
>    carte) donne une popularité par artiste (médiane des rangs connus moins une
>    pénalité de couverture). Une liste curatée d'une trentaine de monuments
>    iconiques (le tag OSM `wikidata` est trop universel pour hiérarchiser) est
>    résolue contre `points_remarquables` et appariée **par rang pur** — pas de
>    contrainte géographique ni musicale, les lieux importants sont partout dans
>    Paris. Un artiste ancré **déménage entièrement** : ses morceaux se logent
>    autour du monument, sa rue synthétique est le tronçon réel le plus proche
>    (retiré du bassin de l'étage 2), il est retiré des étages 1-3 **avant**
>    l'agrégation des familles. Marqueurs fixes, aucune déformation du reste.

C'est là que tout se joue, et c'est le seul endroit. Aujourd'hui « proche »
signifie « musicalement semblable », par construction du t-SNE ; c'est cette
propriété qui donne un sens au lasso, aux quatre modes de chemin et aux profils
d'itinéraire. Sur un plan imposé, elle doit être **gagnée**.

Elle est traitable parce que les deux côtés sont des nuages 2D — notre
projection d'un côté, la voirie de l'autre. Ce n'est pas une affectation
quadratique générale, c'est une déformation d'un nuage sur l'autre.

Trois étages, du gros au fin. L'étagement de `carto-google-maps.md` §1, abandonné
alors parce qu'un force layout contraint était instable, **redevient valable :
sur un plan figé, la stabilité est gratuite, la géographie ne bouge jamais.**

### Étage 1 — familles → quartiers

**12 familles**, une partition de Paris en 12 zones. Les centroïdes des familles
dans l'espace t-SNE forment une constellation ; on l'ajuste sur la forme de Paris
par **Procruste** (rotation, échelle, translation — pas de déformation libre, la
disposition relative des genres doit survivre), puis Voronoï depuis ces
12 germes, découpé sur la frontière.

**Implémenté et mesuré** (`crates/carto/src/affectation.rs`, commande
`quartiers`) : Procruste sans correspondance point à point (rotation qui
aligne l'axe principal du nuage de familles sur celui du réseau de rues,
échelle isotrope, translation), puis diagramme de puissance dont les poids
sont ajustés par rétroaction — la même idée que Sinkhorn pour le transport
optimal, réduite au cas où une seule masse (les rues) doit être distribuée.

Résultat sur la bibliothèque réelle, 7 047 rues (2 373 km), avec les 12
familles par genre (voir plus bas) : **erreur relative maximale de 2 %**
entre longueur cible et longueur obtenue, en 0,12 s. Meilleur qu'avec les 8
familles k-means d'origine (6 %) — des familles plus nombreuses et mieux
réparties en effectif partitionnent plus finement.

`voronoice` reste en dépendance mais n'a finalement pas servi ici : le
diagramme de puissance par rétroaction s'est avéré suffisant et plus simple
à contraindre en capacité qu'un Voronoï classique suivi d'une relaxation de
Lloyd.

La voirie parisienne est très inégale — le centre est dense, les deux bois
sont vides. Un Voronoï brut donnerait à une famille le bois de Boulogne et
trois rues. C'est exactement ce que corrige la rétroaction de capacité
ci-dessus : les poids absorbent le déséquilibre géométrique, pas une
relaxation de Lloyd.

> **L'aplat de quartier (1ᵉʳ sept. 2026).** En dézoomant pour voir Paris
> entier, la carte n'avait plus **aucune** information musicale : les seuls
> éléments colorés par famille sont les bâtiments habités, révélés à partir du
> zoom 14 seulement. Le diagramme de puissance convergé (`Quartiers::poids` +
> les germes) définit pourtant la zone de chaque famille *en tout point* du
> plan, pas seulement aux centres de rue — `affectation::territoires`
> l'évalue sur une grille du territoire parisien (restreinte à la limite
> communale) et `contour` en tire un polygone par famille (`Source::
> territoires_reels`, couche `territoires-reels`, `FAMILLE_TERRITOIRE_REEL`).
> Rendu comme un lavis un peu plus soutenu que celui du monde fictif, qui
> s'efface quand les bâtiments prennent le relais (`Paliers::ville().
> territoires_jusqu_a = 13`). C'est enfin l'expression visuelle des
> « quartiers » que ce document décrit (« Le Marais électronique »).
>
> Conséquence : la **pastille** grise d'artiste (`artistes-point`) ne se
> révèle plus dès qu'on voit Paris entier (zoom 11) mais seulement une fois
> entré dans un quartier (`art_b3` ≈ 13, `style::couches`) — l'aplat suffit au
> repérage de loin, accompagné du **nom** seul (`artistes-etiquette`,
> inchangé, dès le zoom 12).

### Étage 2 — artistes → rues : **fait**

`crates/carto/src/affectation.rs` (`loger_artistes`), commande `carto rues`.

Dans la zone de sa famille, chaque artiste vise le point que lui donne le
**même Procruste** que l'étage 1, appliqué à son propre centroïde plutôt qu'à
celui de sa famille — cohérent par construction avec le semis des quartiers.
Attribution **gloutonne, par nombre de morceaux décroissant** : le plus gros
artiste choisit le premier, et prend la rue libre la plus proche capable de le
loger (longueur ÷ espacement ≥ nombre de morceaux) ; s'il ne tient pas sur une
seule, il prend la suivante la plus proche jusqu'à tenir. **Une rue appartient
à un seul artiste** — pas de partage, pour qu'un nom de rue reste sans
ambiguïté à l'affichage.

Repli si la zone d'une famille est épuisée : l'artiste emprunte la rue libre
la plus proche, où qu'elle soit, et c'est compté (« débordement ») plutôt que
caché.

**Correction en cours de route** — et elle a payé. Grouper les artistes par
`tracks.artist` donnait 3 543 « artistes » et 67 débordements, presque tous
des crédits de featuring hip-hop (`Xzibit, B‐Real & Demrick feat. Busta
Rhymes`, `... feat. DJ Quik`, etc. — chacun une chaîne distincte, donc un
« artiste » à lui seul, épuisant les rues de sa zone). Regrouper par
`tracks.album_artist` (déjà la convention documentée pour `mb_album_artist_id`)
ramène le compte à **1 082 artistes, 0 débordement**.

Mesuré sur la bibliothèque réelle, espacement 4 m : 1 082 artistes logés, 853
sur une seule rue, 229 sur plusieurs (jusqu'à 19 — *Various Artists*, la
compilation à 805 morceaux de l'objection O3). 142 355 adresses offertes pour
26 987 morceaux à loger (427 % de marge : la plupart des artistes sont petits
et 5 637 rues sur 7 047 ne servent à personne). 159 ms.

Conséquence heureuse et non forcée : **les gros artistes héritent des grandes
voies**, parce que les rues longues sont rares et partent en premier —
Metallica sur un boulevard, un artiste à morceau unique dans une impasse.

> **Capacité en bâtiments réels, pas en longueur (1ᵉʳ sept. 2026).** La marge
> de 427 % était une illusion : elle comptait une adresse tous les 4 m, alors
> qu'un vrai bâtiment fait 15-30 m de façade. Un artiste « tenait » sur une
> rue qui n'avait physiquement qu'un quart des bâtiments nécessaires, et
> l'étage 3 éparpillait le reste ailleurs (52 % de repli quartier mesuré —
> voir plus bas). `affectation::capacites_par_rue` compte désormais, une fois,
> les bâtiments logeables le long de chaque rue (chacun rattaché à sa rue la
> plus proche, pas de double comptage) ; `loger_artistes` s'en sert à la place
> de `longueur / espacement`, et **saute une rue sans bâtiment** plutôt que de
> l'accumuler. Conséquence : plus de rues par artiste, mais les bâtiments
> colorés apparaissent enfin le long de la rue qui porte son nom.
>
> **Le point d'artiste sur sa rue (même date).** `ville::rassembler` remplit
> `Source::artistes_places` : chaque artiste posé au centre (pondéré par
> longueur) de ses rues attribuées, pas au barycentre de ses morceaux logés
> (`Source::artistes`), qui après un repli tombait dans un vide entre deux
> amas. `tuiles` préfère `artistes_places` dès qu'il est rempli (plan de ville
> réel), retombe sur `Source::artistes` sur le chemin fictif.

### Étage 3 — morceaux → adresses : **fait, dans de vrais bâtiments**

`crates/carto/src/batiments.rs` (`GrilleBatiments`), `crates/carto/src/
affectation.rs` (`Trace`, `loger_dans_batiments`), commande `carto adresses`.

Un morceau habite un vrai bâtiment OSM, jamais partagé avec un autre morceau
— pas une adresse de trottoir, une maison. `GrilleBatiments` indexe tous les
bâtiments de l'extrait (aire ≥ `AIRE_MIN_M2` = 15 m², pour écarter cabanons et
kiosques) sur une grille uniforme en mètres locaux. Pour chaque artiste, dans
l'ordre (album, puis numéro de piste — la même clé que `CleArrivee`), et les
artistes eux-mêmes traités **par effectif décroissant** : on échantillonne la
`Trace` de chacune de ses rues pour réunir un bassin de bâtiments libres, puis
**chaque morceau prend celui du bassin le plus proche de sa cible** — la
position que lui donne le Procruste des étages 1-2 appliqué à **sa propre**
coordonnée t-SNE, pas seulement à celle de son artiste.

> **Cible par morceau (1ᵉʳ sept. 2026).** Auparavant les bâtiments d'un cercle
> étaient triés par **aire décroissante** et distribués dans l'ordre des
> pistes : l'artiste prolifique héritait des plus grands bâtiments (proxy de
> popularité), mais deux morceaux voisins dans l'embedding pouvaient atterrir
> aux deux bouts d'un quartier — c'est le défaut qu'un retour d'usage a
> signalé (playlist cohérente dans le nuage, éparpillée sur la carte). Le tri
> par cible remplace le tri par aire : les pistes d'un album (cibles quasi
> identiques) se groupent toujours, et un morceau tombe près de ses voisins
> sonores. On perd le proxy « plus grand bâtiment » — il n'était pas central.
> `source::Artiste::effectif` sert encore à l'ordre de traitement des
> artistes (le prolifique ancre ses morceaux avant que ses voisins ne
> prennent les bâtiments proches).

Trois cercles de recherche, dans l'ordre : les rues de l'artiste, puis le
reste du quartier de sa famille, puis n'importe où dans Paris en dernier
recours (là aussi, le bâtiment libre le plus proche de la cible). **Le
deuxième cercle s'est avéré nécessaire, pas optionnel** — voir « La mesure qui
compte » ci-dessous.

**Sur la carte, le bâtiment habité est coloré, pas pointé.** Une première
version posait un point de morceau au centre du bâtiment — retour de
l'utilisateur : à 1-3 px, ce point ne se voyait quasiment pas, et faisait
double emploi avec le bâtiment qui l'accueille désormais. `source::
BatimentReel` porte la famille de son occupant jusqu'à la tuile
(`tuiles::Anneau::palier`, réutilisé) ; `style::couches` remplit le bâtiment
entier de la couleur de cette famille (`batiments-morceaux`), et le révèle
dès `paliers.morceaux_des` plutôt que d'attendre le zoom de l'îlot (15) comme
un bâtiment vacant. Le point de morceau (`morceaux-point`) reste, mais
seulement sur le chemin fictif — voir `docs/carto-etapes.md`.

Un tronçon OSM n'est pas garanti bout à bout avec le suivant du même nom :
`assembler_trace` les ordonne par projection sur l'axe principal de leurs
points (une régression, pas une topologie), et retourne un tronçon si son
début est plus proche de la fin de la trace en cours que sa fin. Approximatif
sur une rue en épingle à cheveux (objection V7) ; largement suffisant pour
échantillonner des positions dans un ordre cohérent.

Mesuré sur la bibliothèque réelle *(avant le tri par cible du 1ᵉʳ sept.)* :
**26 987 adresses posées, 0 morceau sans adresse, 0 % hors zone**, 1,07 s pour
les trois étages (contre 165 ms avant le logement en bâtiments — le coût de
l'interrogation répétée de la grille). Chiffres à réactualiser sur la machine
qui a la bibliothèque analysée + `ville-paris.db` (`carto adresses
ville-paris.db --echantillon 2000`).

### La mesure qui compte — objection V1, enfin vérifiable

**Une régression mesurée, puis corrigée par un deuxième cercle de recherche.**
Le premier logement en bâtiments (un seul cercle : les rues de l'artiste, puis
repli direct n'importe où dans Paris) a fait chuter le recouvrement k=12 de 8 %
à ~1 % de moyenne, avec **51 % des morceaux hors zone** (13 610 / 26 987). Le
diagnostic : la capacité en longueur d'une rue (étage 2, une adresse tous les
`espacement` = 4 m) suppose une densité d'adresses de trottoir ; de vrais
bâtiments occupent 15-30 m de façade chacun, bien plus rares au mètre linéaire
— un artiste épuise donc couramment ses propres rues avant ses morceaux, et le
repli direct « Paris entier » dispersait ces morceaux sans aucun rapport avec
le voisinage musical de leur famille.

**Correction : replier d'abord sur le reste du quartier de la famille**
(`loger_dans_batiments`, deuxième cercle) avant Paris entier. Mesuré après
correction : **0 % hors zone** (tout le monde reste dans le bon quartier),
mais **52 % de repli quartier** (14 055 / 26 987) — la moitié des morceaux
n'habite pas la rue de son propre artiste, seulement le quartier de sa
famille. Le recouvrement k=12 remonte à **3 % de moyenne, 0 % de médiane** —
mieux que le ~1 % du repli citywide, mais toujours en dessous des 8 % mesurés
avant le passage aux bâtiments réels. C'est le prix, mesuré et non supposé, du
passage d'une adresse de trottoir (dense, arbitrairement extensible) à un vrai
bâtiment (rare, fixe).

Diagnostic complémentaire, inchangé par ce chantier : **seuls 16 % des
voisins musicaux d'un morceau sont d'un autre morceau du même artiste.**
Autrement dit, la ressemblance sonore d'un morceau tient à 84 % à des morceaux
d'artistes *différents* — la proximité géographique d'adresse à adresse ne
peut porter, par construction, que la proximité *entre artistes* que l'étage 1
(au niveau des familles) et l'étage 2 (le placement glouton par taille
décroissante) ont capturée, et cette proximité inter-artiste reste grossière
quel que soit le nombre de familles (le passage de 8 à 12 familles mieux
réparties n'avait rien changé à ce chiffre).

**Ce que ça veut dire pour la suite.** La carte, telle qu'affectée, est un
échafaudage lisible (famille → quartier, artiste → rue) plus qu'un moteur de
similarité fine. `carto-ville.md` prévoyait déjà que le chemin *musical* se
calcule sur le graphe des k plus proches voisins, pas par lecture de
coordonnées — cette mesure confirme que ce n'était pas une prudence
superflue : la promenade sonore continue de dépendre du graphe et du routage,
pas de la proximité visuelle sur le plan. Reste à trancher, et ce n'est pas
tranché ici : est-ce suffisant (le plan pour se repérer, le graphe pour
explorer), ou faut-il investir dans un placement plus fin (plus de familles,
un sous-classement des artistes à l'intérieur de leur quartier, plus de rues
par artiste pour réduire le repli quartier) pour que la carte elle-même porte
davantage de similarité ? Voir objection V8.

> **Rouvert le 1ᵉʳ sept. 2026, sur retour d'usage.** Une playlist automatique
> cohérente dans le nuage de points apparaissait éparpillée sur la carte —
> exactement le cas que le choix du 23 août réservait à une réouverture. On ne
> refait pas l'affectation (pas de transport optimal global, la structure en
> 3 étages tient) : l'étage 3 vise désormais, pour **chaque morceau**, le point
> du Procruste appliqué à sa propre coordonnée t-SNE et prend le bâtiment libre
> le plus proche (au lieu du plus grand), et l'étage 1 lève la réflexion. Le
> reste des leviers de V8 (plus de familles, sous-classement inter-artistes)
> reste au frigo ; à rouvrir si le recouvrement k=12 réactualisé reste bas.

### Pourquoi le voisinage survit

L'étage 1 fixe la disposition d'ensemble, l'étage 2 place chaque artiste près du
centre de sa zone à proportion de son écart t-SNE, l'étage 3 garde les morceaux
d'un artiste contigus. La distorsion est bornée et, à l'intérieur d'une zone,
essentiellement affine.

## Les noms — inventés, dérivés de la bibliothèque

Aucun toponyme réel n'est affiché. La géométrie est parisienne, la nomenclature
est musicale.

| Support | Nom | Exemple |
|---|---|---|
| quartier | famille musicale | *Le Marais électronique* |
| rue | artiste | *Rue Nina Simone* |
| adresse | morceau | *12, rue Nina Simone* |

Le **type de voie suit la classe OSM**, ce qui fait porter la popularité par la
nomenclature elle-même — exactement le rôle que tenaient les six rangs
d'établissement du peuplement :

| Classe OSM | Type de voie |
|---|---|
| autoroute | Boulevard Périphérique |
| primaire | Avenue, Boulevard |
| secondaire, tertiaire | Rue, Cours |
| résidentielle | Rue |
| piétonne | Passage, Allée, Villa |
| service | Impasse |

## Ce que le peuplement devient

Le modèle n'est pas jeté — il trouve son support.

- « Centre ancien dense, périphérie récente » : Paris **l'a vraiment**. On y
  ajoute l'ordre chronologique en faisant décroître l'ancienneté avec la
  distance au centre, et le récit se raconte tout seul.
- La typologie à six rangs devient la hiérarchie des voies (ci-dessus).
- La stabilité reste une propriété, pour une raison plus forte encore : le plan
  ne bouge pas.
- Ce qui tombe : la génération de relief, les biomes de Whittaker, l'érosion,
  les rivières par D8 — Paris a la Seine. Autant de travail **économisé**, et
  c'est le principal gain de la bascule.

## Le réseau et les itinéraires

Le réseau routier devient la voirie réelle. La distance routière ne vaut donc la
distance sonique que si l'affectation est bonne : **le routage devient un test de
la qualité de l'affectation.**

Les modes *direct* et *dessiné* de la Carte gardent ce partage : le choix des
morceaux se fait à l'écran (ou sur le graphe des k plus proches voisins), le
trait *dessiné* suit ensuite les rues entre deux adresses consécutives
(`trace_rues`, purement cosmétique).

**Le mode *itinéraire* de la Carte, lui, route directement sur la voirie
réelle** (décision revue en septembre 2026, à la demande de l'utilisateur —
elle renverse le « habille le trait, ne remplace pas la sélection » de
`carto-etapes.md`). Un plus court chemin (Dijkstra) est calculé sur le graphe
des rues OSM entre l'adresse de départ et celle d'arrivée — ou, sans arrivée,
jusqu'à un morceau assez loin pour qu'il y ait la durée cible de musique en
chemin. **La playlist est faite des morceaux qui bordent les rues traversées**,
dans l'ordre du parcours ; **la durée prime toujours** (avec ou sans arrivée).
Les trois profils sont trois pondérations d'arête *propres au routage piéton*
(`cout_itineraire.rs`, **pas** `cout_voirie::friction` — qui rend l'autoroute
bon marché pour la forme de la ville) : *par le connu* suit avenues et
boulevards, *redécouvrir* les petites rues, *panoramique* les petites rues le
long des parcs et de l'eau ; l'autoroute est chère dans les trois. **Un seul
trajet est rendu — le choix, c'est le profil** (pas de variantes, pas de case
« éviter les autoroutes »). L'itinéraire *musical* (graphe des voisins, commande
`itineraire`) reste le repli : pas de ville importée, morceau sans adresse, ou
vue Points.

Code : `crates/carto/src/reseau_reel.rs` (graphe, couloir),
`crates/carto/src/cout_itineraire.rs` (profils, proximité d'agrément),
`itineraire_voirie` dans `apps/desktop/src/main.rs`.
Exemple de réglage : `cargo run --release -p rusty-music-carto --example
itineraire_voirie_paris -- ville-paris.db`.

## Licence

Les données OSM sont sous **ODbL**. Deux obligations : afficher
« © les contributeurs OpenStreetMap » sur la carte, et partager à l'identique
toute base dérivée. Compatible avec la GPL-3.0 du projet. **Fait** :
attribution affichée (`apps/desktop/ui/app.js`, `attributionControl`) et
documentée dans le README.

## Où c'est dans le dépôt

| | |
|---|---|
| `crates/osm/` | lecture du `.osm.pbf`, découpe sur la commune, persistance |
| `carto ville <fichier.osm.pbf> --commune Paris` | l'import, une fois |
| `ville-paris.db` (à côté de la base de la bibliothèque) | le plan, 40,9 Mo |
| `crates/carto/src/ville.rs` | assemble une `Source` depuis `ville-paris.db` + l'affectation ; `preparer` (étages 0-2, partagé avec le CLI) puis `rassembler` (étage 3 + `Source`) |
| `crates/carto/src/ancrage.rs` | étage 0 : artistes les plus populaires → monuments iconiques |

`osmpbf` lui-même ne sert qu'à l'import, une fois — mais le crate `osm` **est**
lié à l'application depuis que `apps/desktop/src/main.rs` lit `ville-paris.db`
directement (`osm::base::lire`) pour peupler la carte au lancement : la note
précédente (« jamais lié ») décrivait un état antérieur au branchement.

## Objections

**V1 — La distorsion est réelle et non mesurée.** Contraindre 27 000 points sur
une voirie donnée déforme forcément les distances. Il faut une mesure, pas une
intuition : **part des k plus proches voisins musicaux qui restent parmi les k
plus proches voisins géographiques**, avant et après. À produire avant de
déclarer l'affectation réussie ; sans elle, on aura une jolie carte qui ment.

**V2 — Paris n'a pas la forme du nuage.** La silhouette est un ovale ; un nuage
t-SNE a des bras et des vides. Le Procruste de l'étage 1 fait ce qu'il peut,
mais certaines familles seront serrées et d'autres étirées. La contrainte de
capacité corrige la population, pas la forme.

> **Réflexion levée (1ᵉʳ sept. 2026).** L'ambiguïté du miroir à 180° n'est plus
> « assumée » : `semer` engendre les deux Procrustes (avec et sans réflexion de
> l'axe transverse) et garde celui dont le `partitionner` qui suit a le plus
> petit coût de transport (somme des distances² rue → germe de sa famille). Un
> `partitionner` de plus, ~0,12 s. La forme, elle, n'est toujours pas corrigée.

**V3 — Les bois sont un problème.** Boulogne et Vincennes font un cinquième de
la surface et n'ont presque pas de rues. Soit on les laisse vides — ce sont des
parcs, c'est cohérent — soit on y met ce qui est isolé musicalement. La seconde
lecture est plus jolie : **les morceaux orphelins habitent les bois**.
Tranché par le peuplement dense (3 sept. 2026) : les bois restent **vides**,
comme tout ce qui est hors de la zone peuplée — sauf si un monument-ancre y
tombe (la Fondation Louis Vuitton est dans le bois de Boulogne).

**V4 — Un artiste = une rue rompt avec le morceau comme habitant.** Le modèle du
peuplement laissait les morceaux d'un artiste se disperser selon leur son
(objection O9). Ici ils sont regroupés d'office. C'est un choix contraire, et il
faut l'assumer : on gagne « où est Bowie ? », on perd « la période berlinoise
est ailleurs ». Compromis possible : un artiste très étalé reçoit **plusieurs
rues, dans des quartiers différents**.

**V5 — 40,9 Mo par ville.** Acceptable pour une, à surveiller si l'on en propose
plusieurs. Le rappel de l'incident des 199 Go vaut ici.

**V6 — La carte parle français.** Une géographie parisienne impose un vocabulaire
(rue, impasse, quai, périphérique) et une culture. C'est un parti pris, pas un
défaut, mais il ferme la porte à une internationalisation simple.

**V7 — `assembler_trace` se replierait sur une rue en épingle à cheveux.**
L'ordre par projection PCA suppose une rue globalement rectiligne. Aucune rue
parisienne mesurée n'a posé le cas à ce jour, mais ce n'est pas vérifié
systématiquement.

**V8 bis — recouvrement k=12 à ré-actualiser (3 sept. 2026).** Le peuplement
dense (nuage compacté sur l'île de la Cité, plus de repli « n'importe où »)
devrait resserrer la localité intra-famille ; l'ancrage d'une trentaine
d'artistes hors de leur quartier la desserre un peu. Chiffre à refaire :
`carto adresses ville-paris.db --echantillon 2000` sur la machine qui a la
bibliothèque analysée + les passes `enrich` et `popularité`.

**V8 — Le recouvrement k=12 mesuré est de 8 % (médiane 0 %), pas les 100 % d'un
plan naïf.** Voir la section « La mesure qui compte » ci-dessus. Ce n'est pas
un échec caché : c'est la mesure que l'objection V1 réclamait, et le
diagnostic (84 % des voisins musicaux sont chez un autre artiste) montre que
le déficit vient de la granularité du placement inter-artiste (par
familles), pas de l'ordre intra-rue — et il ne bouge pas quand le nombre de
familles change (7 % à 12 familles bien réparties, 8 % à 8 familles k-means).

**Tranché le 23 août 2026** : on accepte le partage des rôles plutôt que
d'affiner le placement. Le plan sert à se repérer — familles, quartiers,
rues, adresses lisibles et stables. L'exploration fine par similarité reste
au graphe des k plus proches voisins et au routage (`carto-google-maps.md`
§2-3), déjà conçus pour ça et non affectés par cette mesure. On n'investit
donc pas dans une affectation plus fine (plus de familles, sous-classement
intra-quartier) pour l'instant ; à rouvrir si l'usage réel montre que la
lecture visuelle de la similarité manque.
