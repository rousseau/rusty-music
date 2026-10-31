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

## La croissance — le placement retenu (1er oct. 2026)

> **Cette section remplace les étages 1-3 décrits plus bas dans l'application.**
> Ils sont archivés dans `archives/carto-ville-affectation.md` (et `carto quartiers`/`carto rues`
> en gardent le diagnostic), mais `ville::rassembler` ne les appelle plus.
> Raison, mesurée : le placement par familles et par artistes ne regardait
> **jamais la date** — le « centre ancien, périphérie récente » promis par
> `carto-peuplement.md` n'existait que pour la carte fictive de repli.

Le principe, tel que François l'a posé : *les morceaux sont des habitants qui
s'installent petit à petit ; les plus anciens fondent la partie la plus vieille
de la ville (l'île de la Cité), les artères accélèrent le développement et
portent les morceaux populaires, les petits quartiers abritent les morceaux
confidentiels, regroupés par style.* Implémenté par `crates/carto/src/
croissance.rs` (pur : parcelles + arrivants, sans OSM ni SQLite),
`facades.rs` (voie qui borde chaque bâtiment) et `ville::rassembler`.

**Le temps de la ville est le coût de voirie.** Chaque bâtiment de la zone
peuplée porte son coût de déplacement depuis l'île de la Cité
(`cout_voirie`, avenue ≈ 0,18 par mètre, rue ≈ 1,0) : c'est son « âge ». Une
avenue porte donc la ville très loin très tôt — l'accélération n'est pas
ajoutée, elle est dans la métrique.

**Les cellules.** La zone est découpée une fois en cellules par bissection
récursive équilibrée en bâtiments : des **couloirs** (~45 bâtiments qui bordent
un grand axe à moins de 35 m — primaire/secondaire) et des **petits quartiers**
(~90 bâtiments, le reste). ~500 cellules pour 27 000 morceaux.

**L'arrivée.** Les morceaux sont regroupés en *arrivées* — un album sorti une
année donnée, en bloc (une compilation de plusieurs décennies se scinde par
année) — triées par (année, hachage stable de l'album). Les morceaux sans date
fiable arrivent en dernier et vont là où la ville s'étend encore.

- **Front.** À chaque arrivée la ville ouvre, par coût croissant, assez de
  cellules pour loger les morceaux déjà arrivés × (1 + `marge_ouverture`) +
  `base_ouverture`. Le front ne recule jamais ; c'est lui qui fait que
  l'étendue suit la population.
- **Choix de la cellule** : un score sur les cellules ouvertes qui ont de la
  place — ressemblance de style (distance t-SNE au barycentre des habitants
  de la cellule, plus la part de la famille), appariement **popularité ↔
  artère** (`w_artere`), préférence pour les cellules proches du cœur
  (`w_proche`), bonus à la cellule qui loge déjà l'artiste, coût de couper un
  album en deux. Une cellule vierge emprunte le style de ses voisines
  habitées : c'est ce qui dessine des **gradients** plutôt que des frontières
  nettes. Fonder une cellule vierge coûte `w_fondation`.
- **Dans la cellule**, le premier morceau prend le bâtiment de plus bas coût
  (côté intérieur), chaque suivant le plus proche du précédent : un album est
  un bloc contigu.

Les rues sont nommées d'après l'artiste dominant des bâtiments qui les bordent
(plus une rue par artiste) ; les aplats de quartier sont contourés d'après la
famille majoritaire des habitants dans 130 m (`ville::territoires_des_habitants`,
plus de diagramme de puissance). L'étage 0 (monuments) est inchangé : les
artistes ancrés sont retirés de la croissance.

**Mesuré** (`cargo run --release -p rusty-music-carto --example
croissance_apercu`, 27 385 morceaux, 1,3 s ; l'exemple écrit `evaluation.md` et
des SVG année / popularité / famille / quartiers, et accepte `CR_MARGE=…
CR_W_ARTERE=…` pour comparer des réglages) :

| | résultat |
|---|---|
| année ↔ coût de voirie (Spearman) | **ρ = 0,83** (distance à vol d'oiseau : 0,73) |
| distance médiane au cœur, 1980s → 2020s | 600 m → 1 200 → 2 100 → 2 900 → 3 200 m |
| les 100 plus anciens | 99 à moins de 600 m de l'île de la Cité |
| popularité moyenne | artères 0,69, petits quartiers 0,41 |
| 10 % les plus populaires / 10 % les moins | 53 % / 6 % sur une artère |
| 8 voisins de même famille | **64 %** (hasard : 15 %) — l'ancien placement faisait du confetti |

**Limites connues.**
- Les artères restent une minorité (~24 % des bâtiments sont riverains d'un
  grand axe à 35 m) ; la popularité n'y est pas exclusive, elle y est
  *préférée*.
- Le millésime n'a que la granularité de l'année : l'ordre de deux albums de
  la même année est arbitraire mais stable.
- Les ~22 artistes ancrés sur monuments échappent à la chronologie (un Led
  Zeppelin de 1969 est à la Fondation Louis Vuitton, pas au centre). **Les
  monuments de l'île de la Cité (Notre-Dame, Sainte-Chapelle, Conciergerie)
  ne sont plus ancrables** (décision du 1er oct. 2026, après avoir vu Arctic
  Monkeys 2013 sur l'île) : elle est réservée aux morceaux les plus anciens
  (les 100 plus anciens : 99 à moins de 600 m).
- Les dernières arrivées (années 2020, non datés) comblent les trous
  restants, y compris dans le vieux centre, faute d'autre place.
- Rien de ce chantier n'est vérifié **dans l'application** : l'aperçu
  MapLibre (`rassembler_paris`) n'a pas pu être regardé ; seuls les SVG et les
  tuiles générées (10 710 tuiles) l'ont été.

## L'affectation en trois étages — remplacée

Le placement par familles → quartiers, artistes → rues, morceaux → adresses (sept. 2026) n'est plus appelé par `ville::rassembler` depuis la croissance du 1er oct. ; `ville::preparer` et les commandes `carto quartiers`/`carto rues` le gardent pour le diagnostic. Texte complet, mesures et objection V1 : [`archives/carto-ville-affectation.md`](archives/carto-ville-affectation.md).

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

> **Le peuplement rejouable en animation, sur le vrai plan (5 sept. 2026).**
> `annee: Option<i32>` devient une propriété de tuile : celle du morceau qui
> occupe un bâtiment (`source::BatimentReel`), la plus ancienne année de
> sortie d'un artiste (`source::Artiste` — un artiste ancré la porte aussi,
> c'est le même point que son monument), et donc du monument où il est ancré.
> Un curseur temporel du mode Carte (plan réel seulement) la compare : un
> bâtiment/artiste/monument dont l'année dépasse le curseur retombe dans le
> style « vacant »/masqué déjà en place — aucune apparence inventée pour
> l'occasion. Mécanique **`setFilter`/`setPaintProperty`**, pas
> `feature-state` : le codebase portait déjà ce geste pour isoler une famille
> (`app.js::majFiltreGL`/`CIBLES_FILTRE_GL`), une comparaison à un seuil
> global est un filtre, pas un état par entité, et `feature-state` aurait
> exigé de plomber un id stable par tuile qui n'existe pas aujourd'hui. Un
> morceau sans année fiable compte comme **toujours déjà là** (cohérent avec
> `BUCKET_INCERTAIN` du streamgraph, déjà hors de l'axe du temps plutôt que
> pincé à une extrémité) — un `coalesce` côté style retombe sur le seuil
> courant, jamais sur une valeur qui l'exclurait. Bornes du curseur = celles
> déjà calculées pour « Colorer par année » (`carte.bornes.annee`), pas une
> seconde lecture. Un bouton lecture anime le curseur à vitesse réglable.
> Répondent : bâti habité, `artistes-point`/`artistes-etiquette` (donc les
> monuments ancrés). Ne répondent pas, géographie pure : territoires, voirie,
> hydrographie, espaces verts.
>
> Invariant vérifié par construction (`ville::tests::
> le_batiment_habite_porte_lannee_de_son_occupant`, et affiché sur la vraie
> bibliothèque par `carto adresses`) : à toute année, le nombre de bâtiments
> montrés occupés égale le nombre de morceaux logés dont l'année est ≤ à
> celle-là (les non datés comptés des deux côtés).

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
| `crates/carto/src/ville.rs` | assemble une `Source` depuis `ville-paris.db` : `zone_peuplee` (étage 0 + zone), puis `rassembler` (croissance + `Source`) ; `preparer` ne sert plus qu'aux diagnostics `carto quartiers`/`rues` |
| `crates/carto/src/croissance.rs` | **le placement** : cellules, front, arrivées chronologiques (`examples/croissance_apercu.rs` pour l'évaluer) |
| `crates/carto/src/facades.rs` | voie la plus proche de chaque bâtiment, importance d'artère |
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
