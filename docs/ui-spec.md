# Brief d'interface — carte de bibliothèque musicale

> **Périmètre : ce document ne couvre que le Module 2 (Exploration).** Le Module 1 est dans `ui-spec-lecteur.md`, le Module 3 dans `ui-spec-editeur.md`. Voir `modules.md` pour la décomposition d'ensemble.

## Constat de départ
L'interface d'AudioMuse-AI est jugée trop lourde / orientée « panneau serveur ». Objectif : un outil d'exploration léger, visuel, centré sur le nuage de points.

## Vue principale : nuage de points 2D
- Chaque point = un morceau, positionné selon la similarité audio (embedding réduit en 2D).
- Interactions attendues : zoom/pan, survol = tooltip (titre/artiste), clic = sélection/lecture.
- Sélection multiple au lasso pour générer une playlist à partir d'une zone. **Décidé et implémenté** — voir « Tranché le 17 août » plus bas.

## Code couleur
- Variables catégorielles (artiste, style/genre) : palette discrète + légende cliquable ; règle à définir pour la haute cardinalité (top N + gris pour le reste, ou mode « isoler une catégorie »).
- Variables continues (année, tempo, énergie) : dégradé + légende en gradient.

## Recherche & filtres
- Reste à trancher : barre de recherche unique avec autocomplétion multi-type (artiste/titre/style/année), vs filtres facettés séparés (menus/sliders).
- Comportement des filtres actifs : les points non concernés s'estompent (restent visibles en fond, contexte de la bibliothèque conservé). **Décidé.**

## Fonctionnalité signature : le chemin
- Mode d'activation : les deux options disponibles — clic direct sur 2 points du nuage, et choix des 2 morceaux via la barre de recherche. **Décidé.**
- **Quatre façons de fabriquer un chemin. Décidé.** Un seul mode ne suffit pas :
  fournir deux points répond à « emmène-moi de là à là », pas à « promène-moi ».
  Le mode se choisit dans le rail ; **Maj est le modificateur de chemin dans les
  quatre**, seul le geste change.

  | mode | geste | ce qu'il produit |
  |---|---|---|
  | **Direct** | clic, puis maj+clic | une droite entre les deux points **sur la carte** ; à chaque pas le morceau le plus proche du point visé. Ce qu'on voit est ce qu'on obtient. |
  | **Lisse** | clic, puis maj+clic | plus court chemin dans le graphe des 12 plus proches voisins. Chaque saut est par construction une transition entre proches : plus long, sans à-coup. |
  | **Errance** | maj+clic | marche aléatoire auto-évitante dans ce même graphe. C'est l'auto-évitement qui produit la dérive — une marche brownienne libre tournerait autour de son point de départ. |
  | **Dessiné** | maj+glisser | le trait est rééchantillonné à pas d'arc constant, chaque échantillon cueille le point le plus proche à l'écran. |

- **Direct et lisse ne sont pas deux réglages du même calcul** : le direct tire
  une droite à l'écran et cueille au plus près ; le lisse suit le graphe des
  voisins, où chaque saut est une transition entre proches.
- **Deux modes calculent sur la carte plutôt que sur les empreintes : le
  dessiné et le direct.** Le principe reste « on calcule sur les empreintes »,
  et l'exception a une frontière nette : elle vaut quand l'utilisateur désigne
  un geste à l'écran. Le dessiné pointe un trait ; le direct pointe une droite
  entre deux points visibles.
- **Le direct a d'abord été écrit sur les empreintes** (interpolation sphérique
  entre les deux). Le trajet était juste, mais il zigzaguait — une droite dans
  l'espace des empreintes n'en est plus une après t-SNE — et un mode nommé
  « direct » qui serpente ne tient pas sa promesse. **Tranché le 17 août : le
  geste l'emporte sur le calcul**, c'est un outil de pointage. Le mode lisse
  reste là pour qui veut la vérité sonore du trajet.
- Les deux modes de carte se distinguent sur un point : **le dessiné borne sa
  cueillette à un rayon**, si bien que ce que le trait traverse à vide reste
  vide ; **le direct ne borne rien**, parce que l'utilisateur a désigné deux
  morceaux et veut aller de l'un à l'autre — chaque pas doit rendre quelque
  chose.
- L'errance est reproductible à graine égale ; « Autre tirage » la relance.

## Panneau réglages
- Contenu : choix du modèle d'embedding, méthode de projection (t-SNE/UMAP/PCA), paramètres de clustering.
- Format envisagé : tiroir rétractable (drawer), pas une colonne permanente, pour ne pas polluer la vue principale.

## Style visuel
- Thème : bascule sombre/clair disponible (les deux implémentés). **Décidé.**
- Densité générale et typographie : à définir lors du sketch.

## Usage
Ce document sert de brief de départ pour Claude Design (mockup/prototype) et de spec fonctionnelle pour l'implémentation finale (HTML/WebGL + Tauri).

## Modèle de navigation — DÉCIDÉ : « Atelier »
Retenu après comparaison de trois maquettes cliquables (variantes Immersion / Atelier / Établi — voir `maquette-navigation.html`).

### Structure
- **Rail gauche fixe** : identité, sélecteur de mode (Écoute / Explorer / Éditer), recherche, colorer par, légende des styles, contrôles de chemin.
- **Carte au centre** : toujours visible, jamais masquée par un changement de mode.
- **Inspecteur droite** : morceau courant (pochette, métadonnées, voisins soniques, infos artiste).
- **Éditeur en dock bas** : s'ouvre en mode Édition et pousse la carte vers le haut sans la faire disparaître (la carte reste la réserve de matière).
- **Transport pleine largeur en bas** : persiste dans les trois modes.

### Justification
Modèle le plus lisible et le plus extensible : c'est celui qui encaissera la montée en puissance du module 3 (MAO) sans réorganisation. La carte reste la colonne vertébrale du continuum écoute → exploration → édition.

### Risque identifié à surveiller
Ce modèle est celui qui peut le plus facilement retomber dans le côté « panneau d'administration » reproché à AudioMuse-AI. **Contrainte de design : sobriété stricte** — densité maîtrisée, hiérarchie typographique forte, pas d'empilement de contrôles visibles en permanence. Les réglages avancés (modèle d'embedding, projection, clustering) restent dans un tiroir rétractable, pas dans le rail.

## Tranché le 17 août

### Sélection au lasso → playlist. **Décidé, implémenté.**
- **Geste** : `alt` + glisser sur la carte, dans tous les modes de chemin. Le lasso est une sélection, pas un chemin : rien ne justifiait de le cacher derrière un mode. Contour fermé et zone assombrie pendant le tracé, pour voir ce qu'on attrape avant de lâcher.
- **Forme du contour** : quelconque, y compris concave — lancer de rayon en règle pair-impair, pas d'enveloppe convexe. Un lasso tracé à la main est presque toujours concave.
- **Ordre de la playlist** : **parcours de proche en proche** dans l'espace des empreintes, pas l'ordre de la base. Une zone donne des dizaines de morceaux ; les enchaîner tels quels produirait une lecture qui saute d'un bout à l'autre. Départ au morceau le plus central, puis glouton du plus proche. Ce n'est pas l'optimal — le trouver serait un voyageur de commerce — mais aucune transition n'est brutale.

### Chemin depuis la barre de recherche. **Décidé, implémenté.**
La spec retenait « le choix des 2 morceaux via la barre de recherche » sans dire comment. Retenu :
- **La barre garde son rôle de filtre** en mode Explorer, et gagne un second usage : **Entrée pose une borne**. Un second champ dans le rail aurait alourdi une colonne déjà dense.
- Le morceau posé est le **plus proche du centre de la carte** parmi ceux que le filtre retient — sur une recherche large, prendre le premier de la liste tomberait n'importe où.
- **Départ et arrivée sont affichés** dans le bloc Chemin, avec un `×` pour les effacer. Ils étaient jusqu'ici mémorisés sans être montrés : rien ne disait ce qui était choisi ni comment le corriger.
- Le chemin se trace **dès que les deux bornes sont posées**, quelle que soit la voie — clic ou recherche.

### Nommage des familles. **Décidé, implémenté.**
La légende affichait « Famille 1 … Famille 12 », ce qui ne dit rien de ce que la
couleur désigne. Deux règles évidentes échouent, chacune essayée :

- **le genre le plus fréquent** ne distingue rien — « Rock » domine six des
  douze familles ;
- **le genre le plus caractéristique** (le plus sur-représenté par rapport à la
  bibliothèque) désigne une poche marginale : il nommait « Ska Rock · Latin »
  une famille de 4 321 morceaux menée par Bob Marley, Femi Kuti et James Brown,
  sur la foi de 52 morceaux.

Retenu : **`part dans la famille × log₂(sur-représentation)`**, qui exige les
deux — peser dans la famille *et* y être plus présent qu'ailleurs. La même
famille devient « Reggae · Pop ». Deux garde-fous complètent la règle : pas de
quasi-synonymes dans un libellé (« Electronic · Electro »), et pas deux familles
homonymes (deux sortaient « Metal · Rock » ; la seconde descend son classement
et devient « Metal · Grunge »).

Source : les étiquettes de genre des fichiers, présentes sur 90 % des morceaux
analysés. **Limite mesurée, pas seulement redoutée.** La comparaison avec
AudioMuse-AI sur la même bibliothèque
(`experiments/audiomuse-comparaison/`) valide dix libellés sur douze et en
réfute un : la famille de Regina Spektor, Agnes Obel, Nina Simone et Jeff
Buckley sort « Children's · Pop » parce que 121 de ses fichiers portent cette
étiquette, rare ailleurs. Aucun classement ne rattrape une étiquette fausse —
le défaut est dans la donnée, pas dans la règle, qui ne dépend pas de la source
des genres.

La sortie possible ne demande aucune dépendance nouvelle : **CLAP est un modèle
texte-vers-audio et nous n'en avons exporté que la tour audio.** Exporter la
tour texte permettrait de nommer les familles en comparant leurs empreintes à
des mots, sans passer par les tags. Voir `docs/suite.md`.

## Questions encore ouvertes
- Recherche unique avec autocomplétion multi-type vs filtres facettés séparés (la maquette utilise une recherche unique). **Tranché de fait** : recherche unique, qui filtre la carte en mode Explorer et pose une borne sur Entrée. L'autocomplétion multi-type reste à faire si le besoin s'en fait sentir.
- ~~Spec d'interface du module 3~~ — écrite : `docs/ui-spec-editeur.md`. Les trois modules ont désormais la leur.
