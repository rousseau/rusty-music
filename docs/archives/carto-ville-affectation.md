# L'affectation en trois étages — familles, artistes, adresses (remplacée)

> Archive : section retirée de `carto-ville.md` le 31 oct. 2026, historique seulement.
> Le texte est tel qu'il était, liens relatifs compris (ils visent `docs/`).

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
