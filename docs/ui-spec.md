# Brief d'interface — carte de bibliothèque musicale

> **Périmètre : ce document ne couvre que le Module 2 (Exploration).** Le Module 1 est dans `ui-spec-lecteur.md`, le Module 3 dans `ui-spec-editeur.md`. Voir `modules.md` pour la décomposition d'ensemble.

## Constat de départ
L'interface d'AudioMuse-AI est jugée trop lourde / orientée « panneau serveur ». Objectif : un outil d'exploration léger, visuel, centré sur le nuage de points.

## Vue principale : nuage de points 2D
- Chaque point = un morceau, positionné selon la similarité audio (embedding réduit en 2D).
- Interactions attendues : zoom/pan, survol = tooltip (titre/artiste), clic = sélection (peuple l'inspecteur, devient le départ proposé). **Révisé** : le clic ne joue plus directement — silencieux, comme sur l'Anneau et la Frise, pour que le même geste ait le même effet dans les quatre visualisations d'Explorer. Écouter est un geste à part, le bouton ▶ de l'inspecteur (toujours visible, pas seulement au survol).
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

## Tranché le 14 septembre

### Texte → playlist. **Décidé, implémenté.**
La question restée ouverte plus haut (« exporter la tour texte permettrait de
nommer les familles… ») trouve une seconde réponse, plus directe : décrire en
texte libre la playlist voulue, pas seulement nommer une famille.

- **Champ d'intention, pas un 7ᵉ bouton de chemin.** Une ligne partagée entre
  les modes (`#bloc-intention`), sous l'entête du panneau central — révélée
  pour l'instant seulement en Explorer. Un sous-panneau du rail (calqué sur
  `#bloc-itineraire`) a été écarté : trop étroit pour un prompt de plusieurs
  phrases, et l'idée d'un champ unique qui s'adapte au mode plutôt qu'un de
  plus par mode l'a emporté.
- **Un LLM local (Ollama) interprète le prompt**, jamais le texte brut : il en
  extrait un morceau ou artiste de départ (résolu dans la bibliothèque), une
  suite ordonnée de descripteurs anglais (CLAP a été entraîné sur des
  légendes, pas des mots nus) et un nombre de morceaux. Sans artiste ni
  morceau cité, le départ se choisit par similarité au premier descripteur —
  la recherche par description validée dans `experiments/clap-texte/`.
- **Le modèle Ollama se choisit, ne se devine pas.** Un nom fixe en repli
  (`qwen2.5:3b`, celui de `preparer_vocabulaire.py`) échoue dès que la machine
  ne l'a pas installé — Ollama rend alors un 404 sur `/api/generate`
  qu'aucun message ne distingue d'un serveur injoignable (bogue observé et
  corrigé lors du premier essai réel). Retenu : une icône 🦙 à gauche du champ
  déplie la liste des modèles déjà installés (`ollama::modeles`, `GET
  /api/tags`) ; le choix est mémorisé (`localStorage`) et, faute de choix,
  `ollama::modele_par_defaut` prend le premier modèle installé plutôt qu'un
  nom deviné. Un modèle « qui réfléchit » (capacité `thinking`, ex.
  qwen3.8:27b-mlx) répond dans son champ `thinking` plutôt que `response`
  quand `"think": false` n'est pas honoré : `ollama::interpreter` lit l'un ou
  l'autre.
- **Confirmation affichée, mais composition enchaînée sans second geste
  (révisé le 14 septembre).** Le premier jet demandait un clic sur
  « Composer » après l'interprétation, pour qu'une résolution erronée
  (artiste homonyme, par exemple) ne surprenne pas en pleine écoute. À
  l'usage, ce clic supplémentaire alourdissait plus qu'il ne protégeait — la
  résolution s'est révélée fiable, et l'attente d'Ollama (déjà longue,
  jusqu'à la minute sur un gros modèle) rendait le second geste d'autant plus
  sensible. Retenu : l'interprétation **s'affiche toujours** dans
  l'inspecteur (départ, étapes éditables, nombre de morceaux) — l'utilisateur
  voit ce qu'Ollama a compris — puis la composition s'enchaîne aussitôt,
  sans attendre de clic. Le bouton, renommé « Recomposer », reste disponible
  pour rejouer la composition seule après une modification du plan affiché
  (étape reformulée, départ effacé, nombre changé) — sans repasser par
  Ollama.
- **La tour texte de CLAP tourne enfin en direct**, mais seulement pour cet
  usage ponctuel : un appel par prompt, en CPU, indépendant du backend choisi
  pour la tour audio. Voir `crates/analysis/src/encodeur_texte.rs` et
  `docs/rust-audio-stack.md`.
- **`Graphe::guidee`**, nouvelle marche dans `crates/analysis/src/chemin.rs` :
  comme l'errance, une marche auto-évitante pondérée par la proximité sonore
  locale, mais qui dérive vers chaque cible textuelle en séquence plutôt que
  sans direction.
- **Arrivée précise, ajoutée le 14 septembre après un essai réel.** « Partir
  de Shootyz Groove. Arriver à RATM. En passant par du hip hop » révèle un
  angle mort du premier jet : rien dans le schéma ne distinguait un départ
  d'une arrivée, si bien qu'un modèle mettait l'arrivée dans `seed_artiste`
  (le seul champ « artiste » qu'il connaissait) et un autre l'abandonnait —
  et même bien reconnue, une arrivée n'avait nulle part où aller : `guidee`
  ne fait que dériver vers des cibles textuelles, des régions de l'espace, pas
  des points précis. Deux correctifs : `arrivee_artiste`/`arrivee_morceau`
  dans le schéma Ollama, avec un exemple à départ **et** arrivée dans la
  consigne système (déterminant en pratique — sans lui, la confusion revenait
  y compris sur le modèle recommandé) ; et, côté moteur, un raccordement exact
  par `Graphe::sonique` depuis là où `guidee` s'arrête vers le morceau
  d'arrivée résolu, plutôt que de compter sur la dérive textuelle pour y
  atterrir seule. L'arrivée s'affiche et s'efface dans l'inspecteur comme le
  départ.

  **Round 2, même jour** : le correctif ci-dessus a presque marché — départ
  correct, mais arrivée sur un morceau sans rapport (« Tetra Hydro » au lieu
  de RATM). Cause distincte : le modèle rendait bien `arrivee_artiste`, mais
  tel quel — « RATM », pas « Rage Against The Machine ». Or
  `resoudre_piste_nommee` cherche un nom **littéral** dans la bibliothèque
  (préfixe FTS5, `crates/core/src/db.rs::requete_fts`) : un sigle n'y trouve
  jamais le nom complet, faute de mot commun. La recherche rendait donc zéro
  résultat, l'arrivée résolue restait `None` **sans que rien ne le signale** —
  la marche guidée dérivait alors normalement, sans arrivée forcée, jusqu'à un
  morceau quelconque. Deux correctifs : la consigne système demande
  maintenant explicitement le nom complet et usuel (« RATM » → "Rage Against
  The Machine", exemples à l'appui) — vérifié contre les deux modèles
  installés, les deux corrigent correctement une fois la consigne changée ;
  et, pour ne plus jamais rater un cas en silence, `PlanTexte` porte
  désormais aussi `arrivee_demandee` (le nom cru reconnu, trouvé ou non) —
  l'inspecteur affiche « introuvable dans la bibliothèque » en rouge plutôt
  que de faire disparaître l'arrivée sans explication.

  **Round 3, même jour** : une fois l'arrivée bien résolue, un même prompt
  ouvrait toujours sur le même morceau de l'artiste de départ. Cause : un
  artiste sans titre précis se résolvait via `chemin::parcours`, qui retient
  toujours le morceau le plus proche du centroïde — déterministe par
  construction, comme pour le pivot d'album de `path_album`. Or le but
  affiché de l'application est la **redécouverte** de la bibliothèque, pas un
  détour toujours identique par les mêmes pivots. `resoudre_piste_nommee`
  choisit désormais au hasard (`Alea::categorique`, poids uniformes) parmi
  les morceaux trouvés pour un artiste — plus de similarité audio ni de
  centroïde dans cette résolution-là. La graine vient d'un tirage frais côté
  interface à chaque **interprétation** (pas à chaque « Recomposer », qui
  rejoue le plan déjà résolu) : redemander le même prompt peut ouvrir sur un
  autre morceau du même artiste, mais recomposer un plan affiché garde son
  départ.

## Questions encore ouvertes
- Recherche unique avec autocomplétion multi-type vs filtres facettés séparés (la maquette utilise une recherche unique). **Tranché de fait** : recherche unique, qui filtre la carte en mode Explorer et pose une borne sur Entrée. L'autocomplétion multi-type reste à faire si le besoin s'en fait sentir.
- ~~Spec d'interface du module 3~~ — écrite : `docs/ui-spec-editeur.md`. Les trois modules ont désormais la leur.
