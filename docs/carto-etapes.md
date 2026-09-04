# Ce qu'il reste pour que la carte soit une carte

> **Support remplacé le 23 août 2026** : la carte n'engendre plus son monde,
> elle emprunte le plan de Paris — voir `docs/carto-ville.md`. Tout ce qui suit
> décrit le chantier du monde généré ; il reste comme historique du
> raisonnement (les diagnostics sur la lisibilité, l'incident du disque, le
> piège des assets périmés restent valables) mais **la génération de terrain,
> de relief et de peuplement qu'il documente n'est plus le plan actif**. Le
> nouveau plan de travail (affectation familles→quartiers→rues→adresses) et son
> avancement sont en fin de document, section « Plan de ville — avancement ».

État au 23 août 2026 — **les cinq étapes sont faites**, voir « Ce qui a été
fait » en fin de document pour ce qui reste ouvert malgré tout. Ce document séquence le chantier cartographique ; les
décisions de conception vivent dans `carto-peuplement-architecture.md` et
`carto-google-maps.md`, les mesures dans `journal.md`.

## Où on en est

**Livré et mesuré** — la chaîne de rendu : projection, tuiles MVT, archive
PMTiles, ombrage du relief, style MapLibre avec révélation par échelle (4,5 s
de génération, 16,7 Mo). Et le réseau de circulation : hiérarchie routière en
quatre classes, arbre de crête, quatre profils d'itinéraire (22 s de
construction, routage en millisecondes).

**Deux choses qu'il faut dire franchement :**

- la carte **ne s'affiche pas dans le logiciel**. MapLibre ne s'initialise pas
  dans la webview du système ; elle ne se voit qu'en la servant à un navigateur
  ordinaire ;
- le réseau routier **n'est dessiné nulle part**. Il est calculé, classé,
  routable — et absent des tuiles.

## Pourquoi elle ne se lit pas comme une carte

Six causes, relevées sur le rendu réel plutôt que supposées.

1. **Pas de trait de côte.** Le monde n'a pas de bord : les territoires se
   fondent dans le fond. Une carte se lit d'abord parce que la terre s'arrête
   quelque part.
2. **Aucune route.** C'est ce qui rend Google Maps lisible avant tout le reste :
   une hiérarchie de traits qui structure le plan. On en calcule quatre classes
   et on n'en dessine aucune.
3. **Aucune ville.** Entre le nom de région et le morceau, il n'y a rien à quoi
   accrocher le regard. Aux zooms moyens la carte est vide.
4. **Les territoires sont des isobares.** Sept bandes translucides empilées par
   famille : cela se lit comme une carte météo, pas comme une carte politique.
   Un territoire veut un aplat et une bordure nette.
5. **Les zooms 3 à 5 sont désertés.** Les étiquettes d'artistes n'apparaissent
   qu'à partir de z5, les morceaux qu'à z6 : entre les deux, quelques points
   sans nom.
6. **Les contours ont des marches d'escalier**, visibles dès z4 : la nappe est
   calculée sur une grille de 1024 cellules pour le monde entier.

**Et la cause de fond** : on rend l'**ancienne** structure — nuage t-SNE,
familles k-means, nappe de densité — avec un habillage de carte. La structure
**documentée**, le peuplement, n'est pas implémentée. Une carte se lit par ses
lieux et ses liens ; celle-ci n'a que des densités.

## Les étapes

### 0. Débloquer l'affichage dans le logiciel — *prérequis de tout ce qui se voit*

MapLibre construit sa carte, puis plus rien : aucun `style.load`, aucune erreur,
même avec un style minimal sans source. Déjà écartés par l'expérience : notre
style, nos tuiles, la politique de sécurité, la version (v4 comme v5), le worker
issu d'un blob, et l'absence de WebGL — **WebGL2 fonctionne**, « Apple GPU ».

Trois pistes, de la moins chère à la plus coûteuse :

- **0a. Essayer dans la fenêtre principale** plutôt que dans la seconde. La
  fenêtre carte est créée par `WebviewWindowBuilder` ; rien ne dit qu'elle a la
  même configuration que celle du fichier de configuration. **Hypothèse jamais
  testée, et la moins chère de toutes.**
- **0b. Reproduire hors de Tauri**, dans une WKWebView nue, pour savoir qui de
  Tauri ou de WebKit refuse.
- **0c. Selon le verdict** : signalement en amont, ou repli assumé — la carte
  servie au navigateur du système, et l'intégration abandonnée.

### 1. Intégrer à la coquille « Atelier » — *la demande explicite*

Remplacer le canevas de `#carte-vue` par MapLibre **dans la fenêtre
principale**, et retirer `carte.html` et sa fenêtre séparée.

Ce qui existe et doit être rebranché (≈ 25 fonctions, ≈ 130 lignes liées au
canevas dans `app.js`) :

| existant | vers |
|---|---|
| sélection au lasso | `queryRenderedFeatures` sur un polygone |
| inspecteur au survol/clic | gestionnaires MapLibre, déjà écrits dans `carte.js` |
| les quatre modes de chemin | une couche GeoJSON par-dessus les tuiles |
| colorer par famille / année / tempo / énergie | expressions de style sur la couche `morceaux` |
| affichage nuage ↔ densité | visibilité de couches |

### 2. Rendre la carte lisible — *sans attendre le peuplement*

Ces cinq points portent sur la structure actuelle, et **aucun n'est perdu**
quand le peuplement arrivera : le littoral vient déjà d'un champ de densité, les
routes ne dépendent pas du placement, le travail de style survit tel quel.

- **2a. Le trait de côte.** La nappe globale est déjà calculée — et jetée : les
  tuiles ignorent les bandes dont la famille vaut `None`. Son palier le plus bas
  est un littoral. Le gain de lisibilité le plus grand pour le moins d'effort.
- **2b. Aplats au lieu d'isobares.** Un remplissage par territoire et une
  bordure nette, au lieu de sept bandes empilées.
- **2c. Les routes dans les tuiles.** Le réseau existe ; il manque une couche
  `routes` et quatre largeurs de trait — avec le liseré (`casing`) qui fait
  qu'une route se lit comme une route.
- **2d. Combler les zooms 3 à 5.** Étiquettes d'artistes plus tôt, symboles
  échelonnés par effectif — en attendant la vraie typologie de l'étape 3d.
- **2e. Lisser les contours.** Grille plus fine aux zooms proches, ou
  simplification des anneaux avant encodage.

### 3. La structure documentée : le peuplement

C'est ici que la carte cesse d'être une nappe colorée. Voir
`carto-peuplement-architecture.md` pour le détail ; l'ordre est déjà tranché.

- **3a. Corriger les dates** (objections O2 et O3) — décidé le 21 août, pas
  fait. Le placement chronologique lit ces dates, et les 720 morceaux d'avant
  1990 sont les plus mal datés. Ne se rattrape pas après coup.
- **3b. Générateur de monde** : axes, altitude, habitabilité, **niveau de la
  mer et îles**.
- **3c. Peuplement chronologique** : habitants, établissements, journal
  d'arrivées.
- **3d. La typologie dans les tuiles et le style** : six rangs, six symboles,
  six seuils de zoom. **C'est *la* chose qui donne l'allure IGN** — et qui
  remplit les zooms moyens que l'étape 2d ne fait que rapiécer.
- **3e. Toponymes** des établissements.

### 4. Le routage dans l'interface

Le moteur est là, il n'a aucune interface.

- Panneau d'itinéraire : départ, arrivée, profil, **durée cible**, alternatives.
- Tracé sur la carte, et le profil de popularité en dénivelé.
- Envoi du résultat vers la file de lecture.

### 5. Dettes

- **`docs/carto-direction.md` n'existe toujours pas** (objection O12), alors que
  `CLAUDE.md` y renvoie deux fois pour le relief, les toponymes et la balade.
- Biomes de Whittaker (le quatrième axe, l'humidité).

## Deux décisions à prendre

**L'ordre.** L'étape 2 transforme l'allure de la carte sur la structure
actuelle, sans rien perdre pour la suite. L'étape 3 va droit au résultat
documenté mais ne montre rien avant longtemps. Recommandation : **0 → 1 → 2 →
3 → 4**, parce que voir la carte dans le logiciel conditionne tout jugement sur
le reste.

**Si la webview ne cède pas.** Accepte-t-on la carte servie au navigateur du
système — auquel cas l'étape 1 disparaît et le reste tient — ou faut-il chercher
un autre moteur de rendu ? La seconde voie est lourde : `maplibre-rs` est
archivé, et écrire un rendu de tuiles vectorielles n'est pas un chantier
raisonnable.


---

# Ce qui a été fait

## 0. Le blocage — levé, et c'était l'hypothèse la moins chère

**MapLibre s'initialise parfaitement dans la fenêtre principale.** Il ne
démarrait jamais dans la seconde, celle que créait `WebviewWindowBuilder` —
sans une erreur, sans un événement. Toutes les autres pistes avaient été
écartées (style, tuiles, CSP, version, worker, WebGL) ; celle-là n'avait pas
été essayée.

La leçon vaut d'être notée : **la piste la moins chère se teste en premier**,
même quand elle paraît la moins probable.

## 1. Intégration — faite

La carte vit dans le mode Explorer, dans la fenêtre principale. `carte.html`,
`carte.js` et la commande `ouvrir_carte` sont supprimés.

La bascule a tenu à une observation : `versEcran` et `versCarte` sont les
**deux seules** transformations de coordonnées du fichier. En les faisant
déléguer à MapLibre, le lasso, les quatre modes de chemin, le pointage et les
bornes ont continué de fonctionner sans être réécrits. Le canevas reste
au-dessus, transparent, garde tous les gestionnaires et les relaie à MapLibre
qui n'écoute rien (`interactive: false`).

## 2. Lisibilité — faite

| | |
|---|---|
| trait de côte | la nappe globale, jusque-là **calculée et jetée** |
| aplats | un remplissage par territoire au lieu de sept bandes |
| routes | autoroutes et nationales dans les tuiles |
| zooms 3-5 | établissements et toponymes les remplissent |
| contours | lissage de Chaikin, à partir du zoom 3 |

Trois défauts trouvés en regardant le rendu, pas en raisonnant :

- **les bandes d'isovaleur sont des anneaux, pas des disques.** Filtrer sur le
  palier 0 ne gardait que la couronne : les continents étaient creux et la mer
  transparaissait au milieu ;
- **une route n'est une route que si elle est locale sur la carte.** Le réseau
  relie des morceaux proches à l'oreille ; la projection ne préserve que les
  voisinages locaux. Dessinés tels quels, les 7 833 nationales faisaient une
  pelote. Un quart des tronçons est écarté du dessin — ils restent dans le
  moteur de routage, où leur longueur à l'écran n'a aucune importance ;
- **le liseré des routes ne suivait pas l'opacité de la chaussée.** Il dessinait
  les nationales en noir aux zooms où la route était masquée.

Et une décision : **les tuiles ne portent que la hiérarchie**, 8 030 tronçons
sur 261 270. Avec les secondaires et les sentiers, l'archive passait de 17 à
72 Mo et le zoom 9 de 29 000 à 123 000 tuiles, pour une pelote.

## 3. Le peuplement — fait

**L'ordre d'arrivée** (`core::db::ordre_darrivee`) : 27 044 arrivées en 0,13 s,
26 493 par les tags, 212 par l'artiste, 56 par l'album, 283 par la date
d'ingestion. Aucun morceau écarté. Les colonnes `first_release_date` et
`secondary_types` existent ; elles se rempliront à la prochaine passe
MusicBrainz et corrigeront alors les rééditions.

**Le peuplement** (`carto::peuplement`) : 27 042 habitants, **757
établissements**, 87 îles, en 0,2 s. Un test vérifie le théorème de stabilité —
ajouter quarante arrivants ne déplace aucun habitant déjà installé.

**La calibration a démenti le document de conception.** Le seuil d'affinité,
balayé de 0,30 à 0,55, fait varier le nombre d'établissements de 3 126 à
3 142 : rien. Ce qui lie, c'est la géométrie — le rayon de recrutement. À 0,012
(la valeur proposée), 43 % des établissements étaient des fermes isolées et il
n'y avait qu'une métropole. À **0,024**, les six rangs sont tous peuplés :
196 fermes, 145 hameaux, 104 villages, 126 bourgs, 178 villes, 8 métropoles.

Deux des quatre critères d'acceptation que le document annonçait restent en
échec — médiane 10 au lieu de 4-8, fermes 25,9 % au lieu de moins de 15 %.
**Ces deux critères étaient des suppositions, pas des mesures.** La hiérarchie
complète des six rangs est ce qui compte pour la carte, et elle est là.

Les toponymes viennent de l'artiste fondateur : LED ZEPPELIN, THE BEATLES et
ARRESTED DEVELOPMENT sont des métropoles ; Cypress Hill, Dire Straits et Terry
Callier des villes.

## 4. Le routage dans l'interface — fait

Un mode de chemin « Itinéraire » dans le rail Explorer : profil (par le connu,
redécouvrir, panoramique) et **durée cible**. Le trajet part dans la file de
lecture et rend son dénivelé en barres — la popularité le long du chemin.
(Historique : il y a eu aussi une case « éviter les autoroutes » et un curseur
« variantes », retirés — voir plus bas.)

Le réseau se construit à la première demande, une fois par session.

## 5. Dettes — faites

`docs/carto-direction.md` existe enfin (objection O12).

## Ce qui reste ouvert

- **Les biomes de Whittaker** : le quatrième axe, l'humidité, n'est pas décidé.
  La palette reste hypsométrique.
- **Les dates MusicBrainz** : les colonnes sont là, la passe reste à lancer.
  C'est elle qui corrigera les rééditions, et donc les 720 morceaux d'avant 1990
  dont tout le récit chronologique dépend.
- **Les nationales dessinent encore des triangles** par endroits aux zooms
  moyens : le filtre de longueur les raccourcit sans les rendre sinueuses. Les
  faire épouser le relief — l'astuce de crête du document — reste à faire pour
  les nationales comme elle l'est pour les autoroutes.
- **Le peuplement n'est pas persisté** : il se recalcule à chaque génération de
  tuiles (0,2 s, donc sans douleur). Les tables `mondes`, `etablissements` et
  `arrivees` du document de conception n'existent pas encore — et avec elles,
  « la carte en 1975 » et l'animation de la croissance.
- **L'incrémentalité n'est pas éprouvée** : le générateur lit les positions
  t-SNE figées, ce qui suffit tant que le corpus ne bouge pas. Les ancres de
  Nyström, qui placent un morceau nouveau sans reprojeter, restent à écrire.


---

# Audit du 23 août, et ce qu'il a révélé

## Le défaut de fond : les morceaux n'habitaient pas la carte

Le peuplement était calculé, puis **ses positions étaient jetées**. La couche
des morceaux gardait les coordonnées t-SNE : la carte montrait un nuage avec des
épingles posées dessus. Zoomer sur « LED ZEPPELIN » ne montrait pas les morceaux
de Led Zeppelin groupés là, mais ceux qui traînaient dans le coin.

**Corrigé.** La chaîne est réordonnée : peuplement → parcelles → densité,
relief, territoires et réseau, tous calculés sur ces parcelles. La preuve se
voit à l'œil — des **lacs** apparaissent entre les établissements, là où la
densité tombe sous le niveau de la mer maintenant que les morceaux sont groupés.

## Un banc d'essai dans l'application

Une webview du système ne se pilote pas de l'extérieur : « est-ce que le lasso
marche encore ? » ne se vérifiait qu'à la main, donc ne se vérifiait pas.
`RUSTY_MUSIC_AUTOTEST=1` exerce les chemins que l'arrivée de MapLibre a touchés
et rend son verdict au journal : **15 sur 15**, dans les deux modes, carte
affichée en 0,5 s.

Un échec intermittent a été observé — « délai dépassé » à 15 s — et c'était la
contention : une compilation tournait en même temps. Le seuil du banc est à 3 s,
parce qu'une carte plus lente que cela passe pour cassée.

## Ce qui reste, et qui explique l'écart persistant

- **Le monde n'est pas généré.** Le trait `Generateur`, l'`Ancrage`, les quatre
  mondes interchangeables n'existent pas ; l'altitude porte la densité au lieu
  d'une propriété musicale. C'est un modèle différent de celui qui est écrit
  dans `carto-peuplement-architecture.md`.
- **Le réseau relie des morceaux, pas des lieux.** Depuis que les morceaux sont
  groupés, **5 926 tronçons sur 8 030** sont trop longs pour être dessinés : les
  arêtes sonores relient désormais des établissements distants. Le bon modèle
  est un réseau **d'établissement à établissement**, pondéré par le nombre
  d'arêtes qui les traversent — c'est d'ailleurs ce que dit le document,
  « autoroute : relie les grands pôles ».
- **Pas de persistance**, donc pas de rejeu ni d'animation.
- **Pas de biomes.**
- **L'apparence** ne satisfait toujours pas : la palette est claire et le réseau
  domine, mais les formes restent bulbeuses et les courbes de niveau
  omniprésentes.


---

# Les repères d'une carte, et ce qu'ils ont coûté

Réponse à une remarque précise : ce qui accroche l'œil sur un plan, c'est
**l'épaisseur des routes**, **la couleur des agglomérations, différente de la
campagne**, **les noms de lieux** et **les points remarquables**. Il en manquait
deux, et l'un des deux est le premier repère de tous.

## Les agglomérations

Un point, si gros soit-il, ne dit pas « ici, c'est la ville ». Il faut une
**tache d'une couleur qui n'est pas celle de la campagne**. Chaque établissement
porte désormais son contour bâti — le disque de ses parcelles, perturbé par un
bruit déterministe tiré de son identifiant, parce qu'un cercle parfait se lit
comme un symbole et que deux villes voisines auraient la même silhouette.

**Trois calibrations successives, toutes par la mesure :**

| pas | ce qu'on obtenait |
|---|---|
| 0,0048 | les 757 agglomérations se touchaient : une nappe grise continue, plus de campagne |
| 0,0015 | une seule ville couvrait le tiers de l'écran au zoom 5 |
| **0,0005** | la plus grande fait 0,011 — l'ordre de grandeur d'un pays habité |

Le raisonnement qui a tranché : 757 établissements sur un disque de rayon 1
donnent à chacun une part de rayon 0,036 ; le bâti doit en occuper quelques pour
cent, pas la totalité.

## Le réseau relie des lieux, pas des morceaux

Depuis que les morceaux habitent leurs établissements, une arête sonore va d'un
lieu à un autre. Les 261 270 arêtes sont donc **agrégées par couple
d'établissements** : 14 802 couloirs, dont 193 autoroutes et 646 nationales. Un
couloir emprunté cent cinquante fois devient une autoroute quelle que soit la
classe de chaque brin — c'est le trafic qui fait le rang.

L'épaisseur porte la hiérarchie avant la couleur : de 1 à 4 entre une autoroute
et une route secondaire, liseré compris.

## Deux géographies, et il faut les distinguer

**La terre ne se définit pas par où sont les maisons.** Tout calculer sur les
parcelles a été essayé : les morceaux groupés font du champ de densité une série
de pics, la terre se fragmente en un archipel d'îles à une ville, et il ne reste
aucun continent.

Donc : littoral, relief et territoires viennent de la distribution d'origine —
lisse et continue ; agglomérations, morceaux et routes viennent des parcelles.

## Un piège de construction, à connaître

**`touch build.rs` ne suffit pas à ré-embarquer l'interface.** Le binaire
continue de servir un `app.js` antérieur, sans le moindre avertissement : on
croit tester ce qu'on vient d'écrire et l'on teste la version d'avant.
`cargo clean -p rusty-music-desktop` avant reconstruction est le seul moyen sûr.

Ce piège a coûté une heure de diagnostic sur une carte qui « ne répondait
plus » : elle répondait très bien, mais avec le code de la veille.


---

# Les routes épousent le relief

`carto-google-maps.md` : « faire suivre aux autoroutes la ligne de crête de
densité — le réseau épouse alors le relief, comme une vraie carte routière ».
C'était resté lettre morte, et cela se voyait : les routes rayonnaient en étoile
depuis chaque agglomération, segments droits entre des points.

Chaque route est désormais une polyligne de neuf points. Pour chacun, on cherche
le décalage perpendiculaire qui passe par le sol le plus haut, dans une fenêtre
bornée ; le détour s'annule aux extrémités — **une route arrive dans la ville,
elle ne la contourne pas** — et la ligne brisée est adoucie, sans quoi elle
zigzaguerait d'un échantillon à l'autre.

L'agrégation par couple d'établissements est extraite dans
`carto::source::reseau_entre_lieux` : le CLI et l'application l'appellent tous
deux, et ne peuvent plus diverger. Le bouton « refaire les tuiles » de
l'application produisait jusque-là une carte dégradée, sans routes ni
peuplement.

**Coût mesuré, et correctif.** Neuf points par tracé ont fait passer la première
image de 0,5 à 3,5 s : les tuiles des premiers zooms — celles qu'on attend à
l'ouverture — portaient tout le détail. Sous le zoom 4, le détour ne se voit pas
et l'on n'envoie plus que les deux bouts : 2,8 s. Toujours plus lent qu'avant les
routes, et c'est le prix à payer.

# Le banc d'essai ne bloquait pas

Il ne démarrait pas. `app.js` s'exécutait de bout en bout, mais le binaire
servait une version antérieure — **le piège de `touch build.rs`**. Après
`cargo clean -p rusty-music-desktop` : 15 sur 15, dans les deux modes.

Le banc rend maintenant compte **au fil de l'eau** et non à la fin : grouper les
résultats rendait tout arrêt opaque, au point qu'on ne savait pas s'il avait
démarré.

# Incident : 199 Go

Le dossier avait atteint 199 Go et le disque était plein à 98 %. La cause :
`target/debug` à 162 Go, dont **53 Go de compilation incrémentale**, plus 10 Go
dans les `target/` des expériences.

Retiré : les artefacts de compilation seulement — ni source, ni base, ni
modèles. 189 Go → 17 Go, et 188 Go rendus au disque.

**Prévention** : `[profile.dev] incremental = false` dans le `Cargo.toml` de
l'espace de travail. L'incrémental n'apportait presque rien ici — les modèles
ONNX traduits en Rust (4 400 lignes pour CLAP, 8 600 pour HTDemucs) et
l'`opt-level = 3` du profil de développement font de toute façon recompiler
largement — tout en gardant tout. À surveiller avec `du -sh target`.


---

# Rivières et points remarquables

## Les rivières — ce qui manquait le plus au réalisme

Une carte se reconnaît à ses rivières autant qu'à ses routes. Elles donnent au
relief une lecture immédiate — l'eau descend, donc voir où elle va, c'est voir
la forme du terrain — et elles cassent la régularité des nappes de densité, qui
sans elles se lisent comme des isobares.

`carto::hydro` : direction d'écoulement en huit voisins, accumulation de flux,
extraction au-dessus d'un seuil. Technique classique du modelage de terrain, pas
une invention. **218 rivières** sur la bibliothèque, en trois épaisseurs —
ruisseau, rivière, fleuve.

Deux précautions que le code porte : la pente se mesure **par unité de
distance**, sans quoi les diagonales l'emporteraient et l'eau descendrait en
zigzag ; et un jeu de cellules déjà vues empêche de tourner en rond dans une
cuvette numérique. Un test vérifie que l'eau ne remonte jamais.

## Les points remarquables

Trois espèces, et rien de plus — une carte couverte de symboles ne signale plus
rien :

| espèce | ce que c'est |
|---|---|
| monument | le morceau le plus ancien d'un territoire, celui par qui il a commencé |
| refuge | un morceau dont même le plus proche voisin est loin |
| fondation | le morceau qui a fondé une métropole |

80 au total, bornés par espèce.

## Ce que ça a coûté, et le remède

Chaque enrichissement alourdit les **premières** tuiles — celles qu'on attend à
l'ouverture. La première image est passée de 0,5 s (avant les routes) à 2,8 s
puis 4,1 s. Le remède est chaque fois le même et vaut d'être noté comme règle :
**ne pas envoyer de loin ce qui ne s'y voit pas.** Sous le zoom 4 une route est
droite ; sous le zoom 3 un ruisseau n'existe pas ; les points remarquables
n'entrent qu'à partir du zoom 3.

## La régression n'existait pas — et le piège est refermé

J'ai signalé que le mode carte ne chargeait plus. **C'était faux.** Vérifié à
froid : 14 sur 15, la carte s'affiche avec ses 20 couches, le lasso fonctionne
dans les deux modes, l'itinéraire aussi. Le seul échec est mon propre seuil —
3,8 s à l'apparition contre 3 s visés.

La cause est encore le piège des assets embarqués : chaque reconstruction sans
`cargo clean -p` testait l'interface de la veille. Il a coûté plusieurs heures
et, plus grave, **un rapport faux**.

`apps/desktop/build.rs` déclare désormais à Cargo tout ce qui, sous `ui/`, doit
déclencher une reconstruction — récursivement, car `rerun-if-changed` sur un
dossier ne couvre que son contenu immédiat. Vérifié : modifier `app.js` suffit
maintenant à faire recompiler. Le `cargo clean` n'est plus nécessaire.

**La leçon dépasse ce projet** : un banc d'essai qui teste un artefact périmé est
pire qu'une absence de banc, parce qu'il donne confiance. Avant de conclure
qu'une chose est cassée, vérifier qu'on teste bien ce qu'on vient d'écrire.

---

## Plan de ville — avancement

Suit `docs/carto-ville.md`. Trois étages à faire ; le premier est fait et
mesuré.

### Étage 1 — familles → quartiers : **fait**

`crates/carto/src/affectation.rs`, commande `carto quartiers`. Procruste sans
correspondance (rotation + échelle isotrope + translation) pour semer les
familles sur la ville, puis diagramme de puissance ajusté par rétroaction de
capacité (l'idée de Sinkhorn, réduite à une masse à distribuer).

Mesuré sur la bibliothèque réelle, 12 familles (par genre, voir plus bas),
7 047 rues (2 373 km) : **erreur relative maximale 2 %**, 0,12 s. 6 tests.

**Les familles sont passées d'un k-means (8, mélanges arbitraires du type
« Reggae · Rock ») à un vocabulaire de genre fixe (12, mesuré sur la
bibliothèque réelle) — voir `carto-ville.md`, section « Les familles — par
genre, pas par k-means ». Détail : `crates/analysis/src/cluster.rs`,
`Library::genres_resolus`.

### Étage 2 — artistes → rues : **fait**

`crates/carto/src/affectation.rs` (`loger_artistes`), commande `carto rues`.
Glouton par effectif décroissant, rue libre la plus proche, plusieurs rues si
besoin. 1 082 artistes logés (regroupés par `album_artist`, pas `artist` —
voir ci-dessous), 0 débordement, 159 ms. 9 tests.

**Trouvaille en cours de route** : grouper par `tracks.artist` (le champ brut,
qui liste un featuring entier) donnait 3 543 « artistes » et 67 débordements —
presque tous des crédits hip-hop en featuring, chacun une chaîne unique. Le
regroupement par `album_artist` était déjà la convention documentée pour
`mb_album_artist_id` ; il n'était simplement pas appliqué ici. Corrigé, et
`MapPoint` porte désormais aussi `album_artist` (champ additif, n'affecte
aucun consommateur existant).

Corrigé en passant : `Library::map_view` recopiait `bpm` dans `energy` par un
décalage d'indice de colonne SQL introduit par l'ajout du nouveau champ —
repéré avant tout usage, jamais publié.

### Étage 3 — morceaux → adresses : **fait**

`crates/carto/src/affectation.rs` (`Trace`, `assembler_trace`, `placer_adresses`),
commande `carto adresses`. 26 987 adresses posées, 0 morceau sans adresse, 165
ms pour les trois étages. 15 tests dans le module (204 au total sur le
dépôt).

### Objection V1 — mesurée, et le résultat est bas

**Recouvrement k=12 entre voisins musicaux et voisins géographiques : 8 % de
moyenne, 0 % de médiane.** Diagnostic : 84 % des voisins musicaux d'un
morceau sont chez un *autre* artiste — la proximité fine tient donc à un
placement inter-artiste que 8 familles seulement peuvent difficilement
porter, pas à l'ordre des pistes dans une rue. Détail dans
`carto-ville.md`, section « La mesure qui compte » et objection V8.

**Tranché** : on accepte le partage des rôles — le plan pour se repérer, le
graphe des k plus proches voisins et le routage pour l'exploration fine par
similarité (déjà conçus pour ça, non affectés par cette mesure). Pas
d'investissement dans une affectation plus fine pour l'instant. Détail dans
`carto-ville.md`, objection V8.

## Trois étages faits — la suite

L'affectation (familles → quartiers → rues → adresses) est posée et mesurée.

**Fait, le 28 août 2026** (code écrit et testé unitairement — **pas encore
vu tourner dans l'application**, voir la réserve en fin de section) :

- `crates/carto/src/tuiles.rs`/`style.rs` branchés sur le plan de ville
  (`crates/carto/src/ville.rs` assemble une `Source` depuis `ville-paris.db` +
  l'affectation) plutôt que sur le monde engendré — nouvelles couches
  `routes-reelles`/`batiments`/`eaux`/`verts`/`frontiere`, les adresses
  remplacent les positions t-SNE brutes pour les morceaux ;
- `apps/desktop/src/main.rs` : `engendrer_tuiles` bascule automatiquement sur
  le plan réel si `ville-paris.db` existe à côté de la bibliothèque, avec
  repli sur le monde fictif sinon (rien à faire pour continuer à explorer sans
  ville importée) ;
- nomenclature tranchée : noms inventés uniquement (type de voie OSM +
  artiste), le nom OSM réel reste un attribut caché de traçabilité — voir
  `carto-ville.md` ;
- attribution ODbL affichée (`app.js`, `attributionControl`) et documentée
  dans le README.

**Fait le 29 août 2026** — le routage sur voirie réelle, initialement reporté
(voir plus bas), a finalement été construit dans la même session : un graphe
routable (`crates/carto/src/reseau_reel.rs`) depuis les tronçons OSM,
l'accrochage restreint à la plus grande composante connexe (un bug réel
trouvé sur la vraie carte : Notre-Dame → Arc de Triomphe échouait, l'un des
deux points accrochant sur un fragment isolé de 20 sommets, hors de la
composante principale de 188 188), le plus court chemin par `dijkstra`
(`pathfinding`). Mesuré sur Paris : 235 218 sommets construits en 0,03 s,
chemin réel de 4 984 m (4,6 km à vol d'oiseau) en 10 ms.

**Habille le trait, ne remplace pas la sélection** — décision d'août 2026,
**renversée en septembre 2026** (voir ci-dessous). À l'époque : le choix des
morceaux d'un itinéraire restait celui du réseau **sonique** ; seule la ligne
dessinée suivait les vraies rues (`trace_rues`). Cela reste vrai pour les
modes *direct* et *dessiné* de la Carte, mais plus pour le mode *itinéraire*.

**Renversement — l'itinéraire route sur la voirie (septembre 2026)** — à la
demande de l'utilisateur : sur le plan de ville réel, le mode *itinéraire*
calcule un plus court chemin sur le graphe des rues OSM et **compose la
playlist des morceaux qui bordent les rues traversées**, dans l'ordre du
parcours. Sans arrivée, il va jusqu'à un morceau assez loin pour la durée
cible ; **la durée prime toujours** (avec ou sans arrivée — l'arrivée devient
alors une simple direction). Nouvelles briques dans `reseau_reel.rs` :
`chemin_sommets`, `troncons_traverses`, `couloir` (tronçons traversés + rayon
de 25 m), `IndexSommets` (accrochage des ~27 000 morceaux en masse). Commande
`itineraire_voirie` + `morceaux_le_long` dans `main.rs`, caches
`accrochage_voirie` / `graphes_voirie` / `agrement_voirie` dans `Etat`,
invalidés à la régénération des tuiles. L'interface appelle `itineraire_voirie`
en vue Carte + ville, retombe sur `itineraire` (musical) sinon ou sur `repli`.
La polyligne routée est livrée avec la réponse — plus de `trace_rues` pour ce
mode.

**Les 3 profils** (`crates/carto/src/cout_itineraire.rs`) sont des tables de
friction *propres au routage piéton*, **pas** `cout_voirie::friction` (qui
donne sa forme à la zone peuplée et rend l'autoroute *bon marché* : la ville
s'étire le long du périph — désastreux pour router un piéton) :
- *par le connu* : secondaire/primaire les moins chères → suit avenues et
  boulevards ;
- *redécouvrir* : résidentiel/piéton les moins chers → petites rues ;
- *panoramique* : comme redécouvrir + `× 0,65` aux abords d'un parc/de l'eau.
L'**autoroute reste chère dans tous les profils** (`≥ 2,0`) — d'où la
suppression de la case « éviter les autoroutes », devenue redondante.
Un seul trajet est rendu : **le choix, c'est le profil**, pas des variantes.
Le curseur « variantes » et la case « éviter les autoroutes » sont retirés de
l'interface. `itineraires_disperses` (méthode de pénalité) reste dans
`reseau_reel.rs` comme utilitaire mais n'est plus branché.
Réglage à l'œil : `cargo run --release -p rusty-music-carto --example
itineraire_voirie_paris -- ville-paris.db` (imprime la répartition des mètres
parcourus par classe de voie).

**Cohérence des chemins sur la carte de Paris (septembre 2026, suite)** — trois
correctifs après retour d'usage :

1. **`direct` et `dessiné` calculaient en degrés lon/lat bruts.** À la latitude
   de Paris un degré de longitude vaut ~0,66 degré de latitude : « le morceau
   le plus proche » tirait vers le nord-sud et le rayon de cueillette du
   dessiné était une ellipse plus haute que large — d'où un trajet qui ne
   suivait pas la ligne visée et un dessiné qui ne collait pas au trait.
   `main.rs::RepereLocal` reprojette maintenant positions **et** tracé en mètres
   locaux avant `chemin::direct`/`dessine` quand le plan de ville est actif ;
   `path_drawn` reçoit son rayon en mètres (`metresParPixels` côté `app.js`).
   Le nuage t-SNE, déjà isotrope, n'est pas touché.
2. **Le dessiné se faisait ré-habiller par `trace_rues`** — le trait freehand
   était remplacé par un routage de rues entre les morceaux cueillis, perdant
   la forme dessinée. `tracerDessin` passe désormais son propre tracé à
   `poserChemin` (comme `itineraire_voirie` passe sa polyligne), donc pas
   d'habillage : le trait dessiné s'affiche tel quel. `direct` garde
   l'habillage (aller d'un point à un autre → les rues ont un sens).
3. **L'itinéraire exigeait un clic sur « Tracer », pas les autres modes.** Sur
   le plan de ville il se trace maintenant tout seul dès le départ posé (comme
   `direct`) et se rejoue quand profil ou durée changent (`tracerItineraire`,
   `retracerItineraireSiPret`). Le bouton « Tracer » ne reste que hors plan de
   ville, où le calcul musical (~30 s la première fois) est trop lent pour se
   déclencher seul. Les réglettes « morceaux » et « bruit » sont masquées en
   mode itinéraire (il a ses propres réglages : profil, durée) — la longueur de
   la playlist se règle par la **durée**.

Un tracé dessiné dans un repère (t-SNE / lon-lat) est aussi effacé en changeant
d'affichage : `carte.refaire`/`route`/`routeTrace` remis à `null`, plus de
« Autre tirage » qui rejouerait des coordonnées étrangères au repère courant.

**Itinéraire, correctifs de plus (retour d'usage) :**

4. **Durée vs arrivée.** Deux réglages qui se contredisaient. Modèle retenu :
   **une arrivée posée l'emporte** — le trajet va jusqu'à elle, playlist =
   tous les morceaux du parcours (plafond 120), la durée est *ignorée* et son
   curseur grisé (« → arrivée »). La durée ne sert qu'**sans arrivée** : « une
   balade de 40 min par là », la playlist s'arrête au cumul voulu. Sans arrivée
   ni durée : message d'invite. (`morceaux_le_long` : `terminus = arrivee_id` ;
   `duree_cible_ms` neutralisé si `terminus.is_some()`.)

5. **Profils qui donnaient des chemins étranges + « variantes » sans effet.**
   Deux causes. (a) `par le connu` réutilisait `cout_voirie::friction` où
   l'autoroute est la voie *la moins chère* — un « chemin par le connu » à
   travers Paris filait volontiers par le périphérique. Les 3 profils ont
   maintenant des tables de friction *propres au routage piéton*
   (`cout_itineraire.rs`), autoroute chère partout, contraste net
   avenues ↔ petites rues. (b) Les variantes (Yen puis pénalité) ne donnaient
   que des versions dégradées du même profil. **Un seul trajet est rendu — le
   choix, c'est le profil.** Le curseur « variantes » et la case « éviter les
   autoroutes » (redondante : l'autoroute est déjà dissuasive partout) sont
   **retirés de l'interface**. `itineraire_voirie` n'a plus les paramètres
   `alternatives` / `eviter_autoroutes` ; `itineraires_disperses` reste comme
   utilitaire dans `reseau_reel.rs`.

6. **Itinéraires « en boucle » / retours en arrière.** Cause principale :
   `couloir` élargissait chaque tronçon *traversé* à **tous ses sommets**, or un
   « way » OSM fait souvent des kilomètres. Longer 200 m d'une avenue ramassait
   les adresses à son autre bout, toutes au **même rang** → playlist en
   allers-retours, et `fin_de_trace` (rang du dernier morceau) tronquait le
   tracé n'importe où → grand segment de retour à l'écran. `couloir` ne garde
   plus que **les sommets du tracé lui-même** + un rayon de 25 m ; le rang de
   chaque sommet est sa vraie position le long du parcours. En complément :
   `fin_de_trace` tronque au dernier morceau (sans arrivée seulement) ; la
   destination provisoire vise ~2× la durée ; l'interface ne rappelle jamais
   `trace_rues` pour un itinéraire (il router-ait entre les morceaux dans
   l'ordre de la playlist, source d'autres boucles).

**Deux bugs préexistants trouvés et corrigés en marge**, sans rapport avec ce
chantier : `AIDE_CHEMIN` n'avait pas d'entrée pour `itineraire` (le mode
plantait au clic) ; poser les deux bornes en mode Itinéraire appelait quand
même la commande `path` (qui ne connaît pas ce mode et retombe sur une
droite) avant que le bouton « Tracer » dédié ne prenne la main.

**Modes de chemin désormais propres à chaque affichage** (`MODES_CHEMIN`
dans `app.js`, à la demande de l'utilisateur) : Nuage garde
direct/sonique/errance/dessiné (sonique et errance zigzaguent sans égard
pour la géographie — hors de propos sur une carte) ; Carte garde
direct/dessiné/itinéraire (itinéraire suit de vraies rues — hors de propos
sur un nuage t-SNE). Bascule automatique sur « direct » si le mode actif
devient indisponible en changeant d'affichage.

**Vérifié le 28 août 2026, hors application** — un `ville-paris.db` existait
déjà (importé le 23 août, jamais branché jusqu'ici). `crates/carto/examples/
rassembler_paris.rs` (nouveau, même discipline que `cout_peuplement.rs`) a
rejoué `ville::rassembler` → `tuiles::ecrire_avec` → `style::construire` sur
la **vraie bibliothèque** (27 042 morceaux) et le **vrai** `ville-paris.db` :

- 26 987 adresses posées, 0 sans adresse, 0 débordement, erreur de quartiers
  0,8 % — conforme aux mesures déjà publiées plus haut dans ce document ;
- 3 850 tuiles, 45,6 Mo, 2,2 s ; style à 26 couches, source `relief` absente
  comme attendu ;
- tuiles + style rechargés dans une page MapLibre autonome (variante de
  `crates/cli/src/essai.html`) et **rendus dans un vrai navigateur** (Chrome
  headless, WebGL logiciel) : périphérique rose, hiérarchie des rues,
  espaces verts, frontière communale en bleu — la silhouette est bien celle
  de Paris, pas un aplat ni une pelote.

**Fait le 29 août 2026** — le lasso, le survol, le tracé dessiné et le mode
direct comparaient une adresse réelle (lon/lat) à une position t-SNE : sans
correction, ils visaient un point sans rapport avec ce que les tuiles
montrent une fois le plan de ville réel actif. Corrigé des deux côtés à la
fois, par la même preuve (`positions.json`, écrit par `engendrer_tuiles`) :
côté Rust, `points_de_carte_effectifs` bascule `path`/`path_drawn`/
`selection` sur les vraies adresses quand ce fichier existe ; côté JS,
`versEcran`/`versCarte` (par `geoDepuisCarte`/`carteDepuisGeo`) deviennent
l'identité plutôt que la projection du monde fictif. **Piège évité de
justesse** : une première version écrasait `p.x`/`p.y` en place, ce qui
aurait cassé le mode Nuage (t-SNE) en même temps — revu pour garder les deux
repères séparés (`carte.positionsReelles`), le Nuage reste correct que la
ville soit active ou non.

**Reste à vérifier dans l'application elle-même** (pas fait — capture d'écran
de l'app impossible dans cette session, écran noir sans erreur apparente,
cause non identifiée) :

- que `engendrer_tuiles` produit bien le même résultat une fois déclenché
  depuis l'interface plutôt que par le harnais de mesure ci-dessus ;
- ~~aucun bouton de l'interface actuelle n'appelle `engendrer_tuiles`~~ —
  **corrigé le 29 août 2026** : bouton « Régénérer les tuiles » ajouté au
  panneau « Réglages carte » (`apps/desktop/ui/index.html`/`app.js`), à côté
  de « Recalculer la carte » (qui, lui, ne touche que les positions t-SNE).
  Détruit et relance l'instance MapLibre après coup, pour ne pas laisser
  affiché un rendu périmé sous un onglet qui pense n'avoir rien à refaire ;
- le banc d'essai `RUSTY_MUSIC_AUTOTEST=1` (fluidité, allers-retours de
  coordonnées) sur le contenu réel, maintenant que ce bouton existe ;
- calibrer à l'œil les zooms de révélation (`tuiles::classe_reelle_visible_des`,
  `tuiles::anneau_visible_a`, `Paliers::ville`), posés sans mesure ;
- ~~le recentrage initial de la caméra MapLibre n'est pas câblé~~ —
  **corrigé le 29 août 2026**, trouvé en repassant derrière le bouton
  ci-dessus : sans lui, la carte réelle se générait mais MapLibre s'ouvrait
  toujours sur `center:[0,0], zoom:1.6` (le centre du monde fictif), à des
  milliers de kilomètres de Paris — et à cette échelle, rien du plan réel
  n'est visible avant le zoom d'une avenue. `style::construire` pose
  maintenant `center`/`zoom` dans le style quand `est_ville_reelle()`
  (`tuiles::bbox_reelle`, déjà utilisé pour cadrer l'archive PMTiles) ;
  `app.js` les reprend au lieu des valeurs fictives quand ils existent.
  `maxZoom` du constructeur MapLibre suit aussi désormais le maximum réel
  des tuiles (17, contre 9 pour le monde fictif) — sans ça le bâti, qui
  n'apparaît qu'au zoom 15, restait hors de portée de la caméra.
- ~~**reste ouvert** : la conversion canevas↔géographie de `app.js`
  (`DEMI_ETENDUE`, autour de la ligne 1139) suppose toujours la projection du
  monde fictif.~~ — **corrigé le 29 août 2026**, voir plus haut (« le lasso,
  le survol, le tracé dessiné et le mode direct comparaient... »).

## Les morceaux habitent de vrais bâtiments — fait, avec une régression trouvée et corrigée

**Demande de l'utilisateur** : les morceaux apparaissaient en bordure de rue
(décalage fixe `LARGEUR_VOIE`), pas dans de vrais bâtiments — « un morceau =
un habitant de la ville → un logement ». Question posée en même temps :
existe-t-il une popularité par morceau, et ListenBrainz (compte créé par
l'utilisateur) serait-il utile ? Réponse vérifiée dans le code : non, aucune
popularité par morceau n'existe ni n'est câblée ; `effectif` (nombre de
morceaux gardés d'un artiste) reste le seul proxy. ListenBrainz est déjà
documenté comme la source prévue (`carto-google-maps.md`, `/1/popularity/*`)
mais rien n'est implémenté — chantier séparé, explicitement hors de celui-ci,
décidé avec l'utilisateur.

**Deux décisions prises avec l'utilisateur avant d'écrire le code** : un
bâtiment par morceau, jamais partagé (pas un immeuble collectif) ; l'effectif
de l'artiste dirige l'attribution (les plus gros artistes réclament les plus
grands bâtiments en premier).

**Construit** : `crates/carto/src/batiments.rs` (nouveau — `GrilleBatiments`,
grille spatiale uniforme en mètres locaux, aire minimale 15 m² pour écarter
cabanons/kiosques) ; `affectation::loger_dans_batiments` remplace
`placer_adresses` — échantillonne chaque rue, interroge la grille, trie les
bâtiments non pris par aire décroissante, attribue un par un ; `ville.rs`
traite les artistes par effectif décroissant avec un `HashSet` de bâtiments
pris partagé sur tout l'extrait, pour qu'aucun artiste n'en réclame un déjà
logé. Repères réels ajoutés en même temps, fonctionnalité indépendante :
`PointRemarquable` (Tour Eiffel, musées, monuments, lieux de culte —
`crates/osm/src/lib.rs`) extrait des tags `tourism`/`historic`/`amenity`,
stocké dans une nouvelle table `points_remarquables`, rendu comme une couche
`points-remarquables` dédiée (cercle + étiquette) — voir `carto-ville.md`
pour le détail des trois. Non vérifié sur données réelles dans cette session
(le `.osm.pbf` source n'était pas présent, seulement des fixtures de test) —
seule la mécanique (extraction, aller-retour SQLite, rendu) l'est.

**Régression trouvée par la mesure, pas supposée** : la première version (un
seul cercle de recherche — les rues de l'artiste, puis repli direct n'importe
où dans Paris) donnait **51 % de morceaux hors zone** (13 610 / 26 987) et
faisait chuter le recouvrement k=12 de 8 % (mesure précédente) à ~1 % de
moyenne. Diagnostic : la capacité en longueur d'une rue (étage 2) suppose une
adresse tous les 4 m ; un vrai bâtiment occupe 15-30 m de façade, donc bien
moins dense — un artiste épuise couramment ses rues avant ses morceaux, et le
repli direct dispersait alors le morceau n'importe où, sans rapport avec le
voisinage musical.

**Corrigé** : un deuxième cercle de recherche, choisi avec l'utilisateur
(« repli au quartier de la famille ») — avant de replier sur Paris entier, on
cherche d'abord sur les autres rues du quartier de la famille de l'artiste.
Mesuré après correction : **0 % hors zone**, mais **52 % de repli quartier**
(14 055 / 26 987 — la moitié des morceaux n'habite pas la rue de son propre
artiste, seulement le quartier de sa famille) ; recouvrement k=12 remonté à
3 % de moyenne, 0 % de médiane — mieux que le ~1 % du repli direct, toujours
sous les 8 % d'avant le passage aux bâtiments réels. Détail et discussion
dans `carto-ville.md`, section « La mesure qui compte ».

Tests ajoutés ou réécrits dans `crates/carto` (`batiments.rs` : 4 nouveaux ;
`affectation.rs` : les tests de `placer_adresses` remplacés par ceux de
`loger_dans_batiments`, dont un pour le repli quartier) et `crates/osm`
(un test de non-régression sur une base SQLite antérieure à la table
`points_remarquables`) ; tous passent, `cargo test -p rusty-music-carto -p
rusty-music-osm` (69 tests au total dans les deux crates).

## Le point de morceau se voyait à peine — remplacé par le bâtiment coloré

**Retour de l'utilisateur, juste après le chantier précédent** : « Les
morceaux sont maintenant bien sur des logements mais il ne faut pas afficher
un point pour un morceau. [...] Actuellement, on ne voit pas grand chose sur
la carte. » Un vrai défaut, pas une préférence cosmétique : `morceaux-point`
dessinait un disque de 1,1 px à `p.morceaux_des` (zoom 14, la vue d'ouverture
du plan réel) — sous le seuil de perception, et de toute façon redondant
maintenant qu'un vrai bâtiment loge chaque morceau.

**Le bâtiment porte le morceau, pas un point à côté.** `source::BatimentReel`
remplace `ContourReel` pour `Source.batiments` : chaque contour garde
maintenant `morceau_id`/`famille` de son occupant (`None` si vacant),
renseignés dans `ville::rassembler` depuis l'occupation déjà connue de
`affectation::loger_dans_batiments`. `tuiles::Anneau` **réutilise son champ
`palier`** (déjà réutilisé pour le rang des agglomérations fictives) pour
porter cette famille jusqu'à la tuile — `-1` pour un bâtiment vacant.

**Révélation avancée pour les bâtiments habités.** `anneau_visible_a` gagne
un paramètre `palier` : un bâtiment vacant reste un détail de près (zoom 15,
comme avant), mais un bâtiment **habité** se révèle dès
`paliers.morceaux_des` (14) — le seuil auquel les morceaux se révélaient déjà,
pour ne rien retarder de ce qui se voyait avant ce chantier. Sur la vraie
bibliothèque, ça veut dire ~27 000 bâtiments colorés dès l'ouverture, contre
93 309 en tout à partir de 15 seulement — le bâti vacant reste un détail de
rue, le bâti habité est un repère de carte.

`style.rs` scinde l'ancienne couche `batiments-reels` en trois : `batiments-
morceaux` (remplie par famille, `couleur_famille_champ(source, "palier",
BATI)` — une généralisation de `couleur_famille` qui accepte n'importe quel
champ MVT), `batiments-morceaux-bord` et `batiments-reels` (le bâti vacant,
inchangé, toujours gris et à 15). `morceaux-point`/`morceaux-etiquette`
restent, mais `morceaux-point` devient **strictement chemin fictif** — sur le
plan réel, plus aucune couche ne dessine un disque par morceau.

Côté JS, `majCouleurGL` (le sélecteur « colorer par famille/année/tempo/
énergie ») ne visait que `morceaux-point` ; il gagne un second bras pour
`batiments-morceaux`. Seule la coloration **par famille** s'y transpose — les
modes continus (année, tempo, énergie) n'ont pas d'attribut correspondant sur
un bâtiment (`palier` n'y porte que la famille), donc un bâtiment garde sa
couleur de famille par défaut dans ces modes-là plutôt que de virer à une
teinte plate. Limite connue, pas creusée ici.

**Vérifié sur la vraie bibliothèque et le vrai `ville-paris.db`** — tuiles et
style régénérés (`rassembler_paris`, 3 850 tuiles, 49,0 Mo, 2,81 s) et
**rendus dans un vrai navigateur** (Chrome headless, WebGL logiciel, la même
méthode que la vérification du chantier précédent) : à la vue d'ouverture
(zoom 14), des bâtiments de tailles et de couleurs variées apparaissent le
long des rues nommées, chacun dans la teinte de la famille musicale qui
l'habite — plus aucun point invisible.

Un nouveau test direct sur `anneau_visible_a`
(`un_batiment_habite_se_revele_plus_tot_quun_batiment_vacant`), les fixtures
existantes mises à jour pour `BatimentReel` ; tous les tests passent,
`cargo test -p rusty-music-carto` (64 tests) et `cargo build --workspace`.

## Filtre, échelle, clic, parité — la carte devient navigable

**Retour de l'utilisateur, après avoir relancé l'application** (« Il a fallu
relancer le calcul des tuiles » — attendu, pas un bug : les tuiles sont un
artefact dérivé du code, rien ne les régénère tout seul) : les bâtiments
colorés se voyaient, mais la carte restait insuffisante pour s'y déplacer —
artistes et albums mal repérés, filtre par famille sans effet sur la carte,
échelle de révélation à revoir (styles → artistes → albums/morceaux en
zoomant), clic sans effet sur l'écoute, parité et cohérence de l'interface à
vérifier.

**Le clic visait un rayon de 14 px autour d'un centroïde, pas la forme du
bâtiment.** Depuis que le morceau est un bâtiment entier (chantier
précédent), cliquer près du bord d'un grand bâtiment manquait le morceau.
`tuiles::Anneau` gagne un champ `morceau` (réutilisant le même idiome additif
que `palier`), écrit dans le tag MVT du bâti ; `pointSous` (app.js) interroge
d'abord `queryRenderedFeatures` sur `batiments-morceaux` avant de retomber
sur la recherche par rayon — survol et clic se corrigent du même coup, un
seul point d'entrée pour les deux.

**Le filtre par famille (panneau « Familles ») ne touchait que le canevas
2D**, jamais les tuiles MapLibre — cliquer une pastille n'avait donc aucun
effet visible en mode Carte. `majFiltreGL` (nouveau, sœur de `majCouleurGL`)
assombrit (`× 0.08`, la même conversion que `dessinerCarte()` sur le canevas)
tout ce qui n'appartient pas à la famille isolée, sur `territoires`,
`morceaux-point`, `artistes-point`/`-etiquette`, `albums-point`/`-etiquette`,
`batiments-morceaux`/`-bord` — capturant l'opacité d'origine de chaque couche
au premier appel pour pouvoir y revenir sans la recalculer côté JS.

**Échelle recalibrée pour l'étendue réelle de Paris, pas celle du monde
fictif.** `artistes_des` (13 → 11) et l'ancien décalage `+4`/`+5` (hérité
d'un monde à 9 zooms, absurde sur une pyramide à 17 — un artiste ne se
voyait qu'au zoom 17/18, quasiment jamais) sont remplacés par une bande
`[artistes_des, morceaux_des]` divisée en tiers, la même formule pour les
deux mondes. Les artistes s'étagent en plus par **rang de popularité**
(`tuiles::rang_artiste`, par quantile d'effectif — 5 %/20 %/50 % — plutôt que
par seuil de population fixe comme `peuplement::Rang`, qui n'a pas de sens
hors du monde engendré) : les plus prolifiques se révèlent en premier, même
principe que les six rangs d'établissement du monde fictif.

**Nouvel échelon : les albums.** Absent des deux mondes jusqu'ici.
`source::AlbumReel` (réel seulement) ; `ville::rassembler` regroupe les
pistes déjà logées par (artiste, titre d'album) et ancre chaque groupe sur
le morceau le plus proche du barycentre — la même idée que
`Source::ancres_de_familles`. Les pistes d'un album se logent contiguës le
long d'une rue par construction (`loger_dans_batiments` respecte l'ordre
album/piste), donc l'ancre tombe naturellement sur ce tronçon. Nouveau
palier `Paliers::albums_des` (13, entre `artistes_des` et `morceaux_des`) ;
mesuré sur la vraie bibliothèque : **2 101 albums**. `morceaux-etiquette`
(titres) recule d'un cran avant le zoom maximal plutôt que de n'apparaître
qu'à lui seul.

**Parité : le monument sur Paris.** Décidé avec l'utilisateur (monument
seul — voir les options écartées ci-dessous) : `apps/desktop/src/
main.rs::rassembler_ville` pose `Source.curiosites` après `ville::
rassembler`, via `source::curiosites(&morceaux, &[], &[], 60)` — établissements
et refuges vides, seul le monument (le morceau le plus ancien de chaque
famille, ne dépend que de `annee`) se calcule. Mesuré : **12 monuments** (un
par famille). Refuge (réseau kNN sonique + nappe de densité fictive,
coûteux pour un badge secondaire) et fondation (concept du peuplement, sans
équivalent réel) restent propres au monde fictif.

**Cohérence : « Colorer par » n'offre plus une option qui ne fait rien.**
`majSegmentsCouleur` désactive `année`/`tempo`/`énergie` en mode Carte sur le
plan réel (un bâtiment ne sait se colorer que par famille), avec un repli
automatique sur « Famille » si l'un d'eux était actif au moment de la
bascule — plutôt que de laisser un bouton actif sans aucun effet visible.

**Vérifié sur la vraie bibliothèque et le vrai `ville-paris.db`** — tuiles et
style régénérés (`rassembler_paris` : 3 852 tuiles, 50,5 Mo, 3,51 s ; 26 987
morceaux, 2 101 albums, 12 monuments) et **rendus dans un vrai navigateur**
(Chrome headless, WebGL logiciel) aux zooms 11/13/15/17 :

- **zoom 11** (styles) : le littoral et deux titres de monuments visibles,
  peu de détail — cohérent avec une ville de ~13 km qui n'occupe qu'un point
  à cette échelle.
- **zoom 13** (artistes) : réseau de rues, parcs, Seine, et une bonne
  vingtaine de noms d'artistes lisibles (Björk, DJ Shadow, Bruce
  Springsteen…) sans se marcher dessus, plus plusieurs monuments.
- **zoom 15** (albums) : bâtiments colorés, rues nommées par artiste, et des
  titres d'albums lisibles au-dessus des bâtiments correspondants
  (« Nevermind (Deluxe Edition, CD1) », « OK Computer: OKNOTOK 1997 2017 »…).
- **zoom 17** (morceaux) : bâtiments individuels, titres de morceaux lisibles
  un cran avant le tout dernier niveau.

Le filtre par famille (`majFiltreGL`) a été vérifié **isolément** : la
capture d'un instantané headless figé, prise immédiatement après le
`setPaintProperty`, attrapait parfois l'image d'avant le changement (le
rendu MapLibre tourne en continu par `requestAnimationFrame` ; un script à
instantané unique n'a qu'une chance de le rattraper, contrairement à
l'application réelle où la carte reste vivante). Avec un `triggerRepaint()`
explicite dans le harnais de vérification, la même expression
(`["*", base, ["case", ["==", ["get", champ], isolee], 1.0, 0.08]]`) montre
exactement le comportement voulu sur un style de test minimal : la famille
isolée reste pleinement visible, les deux autres tombent à une teinte à
peine perceptible. La logique est donc vérifiée correcte ; son effet sur les
tuiles réelles de Paris n'a pas pu être capturé en image dans cette session
(flakiness du harnais headless, pas de l'application).

**Non vérifié en image dans cette session, à confirmer à l'usage** : le clic
sur un bâtiment déclenchant l'écoute (`pointSous` + `queryRenderedFeatures`)
demande un vrai geste de souris et l'IPC Tauri (`invoke("play", …)`), hors de
portée du harnais MapLibre autonome utilisé ici — code relu avec soin,
suit le patron déjà en place pour le survol/lasso, mais un clic réel dans
l'application reste le seul essai qui compte.

**Le filtre par famille borne aussi le calcul d'un chemin.** Jusqu'ici,
isoler une famille dans le panneau « Familles » ne changeait que
l'affichage ; la playlist d'un chemin pouvait traverser toutes les
familles. Désormais, quand `carte.isolee` n'est pas `null`, `app.js` le
passe (`famille`) aux commandes `path`, `path_drawn` et `selection`. Côté
Rust, `morceaux_de_famille` liste les identifiants du cluster ; le nuage
passé à `chemin::direct`/`dessine` en est amputé, et le graphe sonique est
remplacé par `Graphe::restreint(&permis)` — mêmes arêtes, mais seules
celles dont les deux extrémités restent dans la famille. Un sous-graphe
disjoint fait retomber le sonique sur le direct (lui aussi filtré).
Changer de famille recalcule le chemin déjà tracé (`rejouerChemin`, même
graine). L'itinéraire *musical* (`reseau.rs`) reste non couvert ; l'itinéraire
*sur voirie* (`itineraire_voirie`), lui, l'est — `famille` est passé à la
commande et `morceaux_le_long` écarte les morceaux hors famille du couloir (le
départ et l'arrivée passent toujours). Test :
`le_sous_graphe_restreint_ne_traverse_que_les_permis` et
`morceaux_le_long_ordonne_dedoublonne_et_borne_la_famille`.

Tests ajoutés : `rang_artiste_est_monotone_et_couvre_les_quatre_paliers`
(quantiles, cas vide). Tous les tests passent, `cargo test -p
rusty-music-carto -p rusty-music-osm` (71 tests) et `cargo build
--workspace`.
