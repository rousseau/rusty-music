# Mode Anneau — pistes de visualisation

Complément à `carto-google-maps.md` et `carto-peuplement.md` : ceux-ci couvrent le mode Carte. Ce document couvre le mode Anneau (Explorer), inspiré du projet Eigenfactor de Moritz Stefaner, et la piste temporelle qui remplace le streamgraph insatisfaisant.

## Le vrai modèle : Eigenfactor est un système de 4 vues coordonnées, pas un anneau isolé

Projet de référence : *well-formed.eigenfactor* (Stefaner, en collaboration avec le Bergstrom Lab, 2009). Quatre vues sur le même jeu de données :

1. **Radial (l'anneau)** — vue d'ensemble statique.
2. **Alluvial** — évolution de la structure de clusters dans le temps, par colonnes discrètes (façon Sankey), **pas un flux continu**.
3. **Treemap** — la même hiérarchie de clusters, sous forme de rectangles emboîtés.
4. **Carte force-directed** — déjà couvert par notre mode Carte / Nuage.

**Piste pour le problème du streamgraph** : le streamgraph simule un flux continu là où la réalité est faite de changements discrets d'appartenance (un artiste change de genre dominant, un cluster se scinde). L'**alluvial** ne lisse pas ça : il montre des colonnes (années) reliées par des rubans dont la largeur = l'effectif, et l'ordre/la couleur reflète l'appartenance au cluster à cet instant. À essayer en remplacement du streamgraph avant d'abandonner l'idée d'une vue temporelle.

## Anatomie de l'anneau (radial + hierarchical edge bundling)

Structure du radial d'Eigenfactor, transposée :

| Eigenfactor | Notre projet |
|---|---|
| Anneau intérieur = revues, taille = score d'importance | Anneau intérieur = **artistes**, taille = **popularité** |
| Anneau extérieur = champs disciplinaires | Anneau extérieur = **genres** |
| Liens de citation, faisceaux de Bézier | Liens de **collaboration**, faisceaux de Bézier |

Technique clé : **hierarchical edge bundling** (Holten, 2006) — les liens suivent la structure hiérarchique des clusters et se regroupent en faisceaux, au lieu de tracer chaque lien en droite (illisible dès quelques centaines de liens). Sélectionner un artiste ou un genre met en évidence tout son flux entrant/sortant, le reste s'estompe (cohérent avec la règle déjà en place : filtrer estompe, jamais ne masque).

## Trois motifs de la taxonomie de Manuel Lima (Book of Circles) à exploiter

Taxonomie : 21 motifs circulaires en 7 familles. Principe transversal : le centre comme ancrage, les anneaux comme temps ou rang, les rayons comme catégories, le périmètre comme récit.

- **Diagramme en corde (chord diagram)** — matrice de collaborations entre artistes : chaque artiste occupe un arc, chaque collaboration un ruban traversant le cercle. Lisible jusqu'à quelques dizaines d'artistes ; au-delà, retomber sur l'edge bundling.
- **Sunburst** — hiérarchie de genres (genre → sous-genre → artiste → morceau) en anneaux concentriques emboîtés, anneaux de plus en plus fins vers l'extérieur. Équivalent radial d'une treemap.
- **Spirale chronologique** — rayon = temps (un tour complet par décennie, par exemple), plutôt qu'un axe linéaire. Fait apparaître les motifs périodiques (une résurgence de style tous les dix ans s'aligne radialement). Évoque directement le sillon d'un disque vinyle.

## Book of Trees — l'arbre radial pour la généalogie

Motif : arbre radial, centré sur un artiste choisi, déployant ses branches de collaboration/influence vers l'extérieur — se lit comme un arbre généalogique. Prolonge directement l'idée déjà posée d'assimiler un morceau/artiste à un humain avec son histoire.

## Ce que Bohnacker (Design génératif) apporte en plus : l'organique

Les trois familles ci-dessus produisent une géométrie propre (arcs nets, courbes lisses). Bohnacker apporte l'inverse : des règles algorithmiques (paramètres, bruit, systèmes d'agents) qui produisent des formes organiques, jamais identiques.

- **Portrait génératif par morceau** : forme dérivée déterministiquement de l'empreinte audio — la même empreinte produit toujours la même forme, deux morceaux proches produisent des formes visuellement proches. Pourrait devenir l'icône du morceau dans toute l'interface, pas seulement dans le mode Anneau. Prolonge la métaphore humaine : un visage généré par les caractéristiques de la personne.
- **Bruit sur les arcs et les faisceaux** plutôt que des Bézier parfaitement lisses — répond au même défaut que la première carte topographique (trop lisse).
- **Anneau par agents** : chaque morceau est un agent qui se stabilise à sa position d'équilibre plutôt qu'un point calculé une fois — frontières vivantes plutôt qu'arcs mécaniques.

## Prochaine étape suggérée
Prototyper séparément : (1) l'anneau statique (edge bundling artiste/genre/collaboration), (2) l'alluvial en remplacement du streamgraph, (3) une passe de bruit/génératif sur l'un des deux une fois la structure validée. Ne pas mélanger structure et style organique dans la même itération — le retour d'expérience de la carte topographique le confirme.

## Diagnostic de rendu — première itération (capture comparée à la référence)

Comparaison entre notre premier rendu et http://eigenfactor.org/projects/well-formed/radial.html. Quatre écarts identifiés, par ordre d'impact :

1. **Pas de vrai bundling hiérarchique** — les liaisons partaient d'un point unique proche du centre (effet « éventail »), au lieu de naître à la position réelle du nœud sélectionné et de suivre des points de contrôle dérivés de l'arbre des genres (chemin commun jusqu'à l'ancêtre partagé). C'est ce cheminement par la hiérarchie qui produit le regroupement en faisceaux caractéristique de la technique de Holten (*Hierarchical Edge Bundles*), et non un simple rayonnement depuis le centre.
2. **Encodage de la force en 3 styles de trait** (plein/tiret/pointillé) au lieu d'un seul langage continu (épaisseur + opacité). À corriger : un seul style de trait, force = largeur + opacité.
3. **Pas de couleur de provenance** sur les liens — sur la référence, un lien prend la couleur du cluster d'origine. Aide à la lecture lors d'une sélection.
4. **Palette trop saturée** — même défaut que la première itération de la carte topographique (voir `carto-direction.md`).

Référence d'implémentation : algorithme de Holten (lien PDF sur la page eigenfactor), exemple D3.js du même algorithme pour vérifier la logique de points de contrôle. Paramètre de tension (beta ≈ 0.75-0.85) à garder réglable — l'effet se juge à l'œil.

## Deuxième itération — état de base vs sélection, et la playlist

### Pourquoi ça manque encore de richesse visuelle
Eigenfactor n'isole jamais un seul nœud : sa vue par défaut affiche déjà les 1000 premiers liens de citation, bundlés, ce qui donne la texture tissée caractéristique. La sélection **isole des flux par-dessus** ce fond déjà riche, elle ne le remplace pas. Chez nous, seuls les voisins du focal sont visibles → densité de traits dix fois plus faible, quel que soit le soin apporté au bundling.

**Correction, cohérente avec la règle déjà actée pour les filtres de carte (estomper, jamais masquer)** :
- État de base : un sous-ensemble représentatif du réseau entier, en liens très ténus (faible opacité).
- Sélection : les flux du focal ressortent par-dessus (opacité et largeur montent), le reste s'estompe encore davantage sans disparaître.

### Stabilité des positions — même principe que le peuplement
La position d'un album sur l'anneau ne dépend **jamais** du focal sélectionné. Seul l'éclairage des liens change au clic. Un anneau qui se réorganise à chaque sélection casse la mémorisation.

### Étiquettes — révélation par sélection, pas affichage permanent
Comme Eigenfactor : pas de nom en permanence sur chaque segment (illisible à l'échelle de la bibliothèque). Nom au survol ; pour les voisins directs du focal sélectionné, petit trait de rappel (leader line) vers une étiquette externe.

### Panneau d'information — réutiliser l'inspecteur existant
Ne pas créer de nouveau composant : le clic sur un album de l'anneau peuple le **même inspecteur** déjà spécifié pour le mode Carte (pochette, métadonnées, voisins soniques). Cohérent avec le principe de réutilisation de `ui-workflow.md`.

## Playlist et déambulation le long des connexions

Deux mécanismes, ancrés dans les références déjà établies.

### Le « pin magnétique » (technique de Stefaner, réappropriée)
Au-delà du bundling, Stefaner avait inventé pour Eigenfactor des indicateurs de flux qu'il appelait des « pins magnétiques » : un repère qui voyage le long du lien pour montrer le sens de circulation. Réutilisation directe : quand l'utilisateur choisit le voisin suivant, un point lumineux **voyage le long de la courbe de Bézier** du focal actuel vers le nouveau (~1-2 s), avant que celui-ci ne devienne le nouveau focal et rejoigne la playlist. C'est le mécanisme concret de la « balade le long des connexions ».

### La couronne qui grandit (inspiré du sunburst de Design Génératif)
Inspiration : le sunburst de fichiers/dossiers où chaque segment pousse une barre radiale vers l'extérieur, dont la longueur encode une valeur (ancienneté du fichier).
Transposition : à chaque ajout à la playlist, une barre pousse vers l'extérieur à la position de l'album sur l'anneau — longueur = ordre d'ajout, durée, ou énergie. Résultat : une couronne qui grandit au fil de l'exploration, comme les cernes d'un arbre. Fait écho, à l'échelle d'une session d'écoute, au peuplement chronologique de la carte à l'échelle de toute la bibliothèque.

### Distinction visuelle parcouru / potentiel
Le tracé déjà parcouru : texture organique, légèrement bruitée (cf. bruit sur les arcs déjà prévu). Le tracé potentiel mais non exploré : fin, propre, presque invisible. Le chemin réellement pris doit avoir plus de présence que ceux qui ne l'ont pas été.
