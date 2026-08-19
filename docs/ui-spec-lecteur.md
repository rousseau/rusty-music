# Brief d'interface — Module 1 (Lecteur)

> **Périmètre : ce document ne couvre que le Module 1 (Lecteur).** Le Module 2
> (Exploration) est dans `ui-spec.md`, le Module 3 reste à spécifier. Voir
> `modules.md` pour la décomposition d'ensemble.

## Ce qui est déjà tranché ailleurs — à reprendre tel quel

- **Coquille « Atelier »** (`ui-spec.md`) : rail gauche, carte au centre,
  inspecteur à droite, dock bas, transport pleine largeur persistant dans les
  trois modes. Le lecteur n'ouvre pas de fenêtre à lui : il vit dans le mode
  **Écoute** de cette coquille.
- **Transport** (`ui/prototype/maquette-navigation.html`) : bouton rond 34×34,
  vignette de pochette 38×38 (rayon 6), titre 13 px/600 avec ellipse, artiste
  12 px atténué, progression en barres verticales, minutage `00:00 / 03:41`.
  Les dimensions sont bonnes, seuls les **états** manquent (ci-dessous).
- **Direction visuelle** (`ui/prototype/Directions visuelles - carto.fm.html`) :
  **1a « Relief » retenue** — voir « Décisions » en fin de document. Les écrans
  ci-dessous restent décrits en termes de structure et de hiérarchie ; les
  valeurs exactes se prennent dans la maquette.

## Ce que le moteur fournit déjà

L'interface ne relit jamais le disque : tout vient de `rusty-music-core` et de
`rusty-music-player`. Correspondance écran → appel :

| Écran / élément | Appel |
|---|---|
| Liste d'artistes | `Library::artists()` → nom, nb de morceaux, nb d'albums |
| Liste d'albums (tous, ou d'un artiste) | `Library::albums(Option<&str>)` |
| Contenu d'un album | `Library::tracks_of_album(album, artist)`, déjà trié |
| Recherche | `Library::search(q, limit)` |
| Inspecteur d'un morceau | `Library::track(id)` |
| Pochette | `tags::read_cover(path)` → octets, MIME, origine |
| Transport | `Player` : `play`, `pause`, `resume`, `skip`, `seek`, `position`, `volume`, `current` |
| Réglages : source de la bibliothèque | `Library::roots()`, `Library::remove_root()` |

## Transport — les états qui manquent

La maquette ne montre que « ça joue ». À spécifier :

- **Bascule lecture/pause.** Le bouton porte un ▶ figé ; il lui faut ses deux
  états (`Player::is_paused()`). Même position, même taille, pas de déplacement
  au changement d'état.
- **Piste précédente / suivante.** Encadrent le bouton central. *Suivante* est
  disponible (`skip`) ; **précédente n'existe pas encore côté moteur** — voir
  « Ce que l'interface demandera au moteur ».
- **Déplacement dans la piste.** Les barres de progression sont aujourd'hui
  décoratives. Elles deviennent cliquables : un clic à la position *x* appelle
  `Player::seek()`. Survol = curseur fin + minutage de la position visée.
- **Volume.** Absent de la maquette. `Player` l'expose en linéaire (1.0 =
  niveau d'origine). Proposition : commande discrète à droite du minutage,
  repliée par défaut — la contrainte de sobriété stricte d'`ui-spec.md`
  s'applique ici aussi.
- **Aléatoire / répétition.** Ni l'un ni l'autre n'existe dans le moteur. À
  trancher : les spécifier maintenant, ou les remettre à plus tard.
- **Rafraîchissement.** `Player::position()` se lit par sondage, il n'y a pas
  de flux d'évènements. 4 à 10 rafraîchissements par seconde suffisent pour la
  progression ; inutile de viser la fréquence d'écran.

## File d'attente

Absente de tout ce qui existe. Proposition :

- Panneau ouvert depuis le transport, en superposition à droite — **pas** un
  quatrième volet permanent, la coquille est déjà dense.
- Liste ordonnée, la piste en cours mise en évidence. Le moteur donne
  `Player::current()` et `Player::remaining()`.
- Un album envoyé en lecture remplace la file (`Player::play`) ; « ajouter à la
  suite » l'allonge (`Player::enqueue`).
- À trancher : réordonnancement par glisser-déposer, ou file en lecture seule
  pour la première version.

## Vues de parcours

Le mode Écoute a besoin de trois vues que rien ne décrit aujourd'hui. Les
volumes réels de la bibliothèque de test les contraignent fortement :

- **Artistes.** Liste virtualisée avec index alphabétique. Le regroupement se
  fait par identifiant MusicBrainz : sans lui, 1 384 des 3 543 entrées sont des
  variantes « X feat. Y » et la liste devient inutilisable.
- **Albums — 1 986 entrées.** Grille de pochettes. Chaque pochette coûte 50 à
  210 ms de lecture disque : chargement paresseux à l'affichage et cache
  mémoire indispensables, sinon la grille se traîne.
- **Pistes d'un album — 8 à 34 entrées.** Liste simple : numéro, titre, durée.
  Déjà triée par le moteur.

## Pochettes

- Deux origines, transparentes pour l'interface : image embarquée, sinon
  fichier du dossier. `Cover::source` le dit si l'on veut l'afficher.
- Tailles utiles : 38 px (transport), ~180 px (grille d'albums), ~320 px
  (inspecteur). Carrées, recadrage centré — les pochettes réelles vont de
  350×350 à 1200×1200.
- **Cache côté interface obligatoire.** Le cœur ne stocke rien : les 4,9 Go
  d'images ne sont volontairement pas en base.
- Sans pochette : réserver la place, ne pas faire sauter la mise en page.
  L'encadré rayé des directions visuelles fait office de substitut.

## États limites — mesurés, pas hypothétiques

Chiffres relevés sur la bibliothèque réelle (27 044 morceaux) :

- **55 morceaux sans artiste.** Ils n'apparaissent pas dans la liste
  d'artistes ; ils restent atteignables par album et par recherche. Prévoir un
  affichage pour l'artiste vide dans l'inspecteur et le transport.
- **2 714 sans genre, 504 sans année.** Les champs vides sont la norme, pas
  l'exception : aucune grille de métadonnées ne doit se déformer.
- **10 fichiers illisibles** (Opus, non décodé par symphonia). La lecture
  échoue à l'ouverture : message clair, passage à la piste suivante plutôt
  qu'un blocage silencieux.
- **Fichier disparu.** La surveillance retire le morceau de la base, mais il
  peut être dans la file au moment où il s'évapore. Même traitement.
- **Titres très longs et écritures non latines** (la bibliothèque contient
  芸能山城組, `Kanañ a ri!`, `(həd) p.e.`) : ellipse partout, et une police de
  repli qui couvre le CJK.

## Ce que l'interface demandera au moteur

Manques identifiés en écrivant ce document. Les deux premiers sont faits :

1. ~~**Piste précédente**~~ — fait. `rodio` ne sachant qu'avancer, `previous()`
   reconstruit la sortie à partir du rang visé, sans toucher à la file : sans
   quoi un second retour en arrière serait impossible. Au-delà de trois
   secondes écoulées, la piste en cours est reprise à zéro.
2. ~~**Recherche sans accents**~~ — fait. Index FTS5 à contenu externe,
   tokenizer `unicode61 remove_diacritics 2`, tenu à jour par déclencheurs.
   « bjork » trouve « Björk », « kanan » trouve « Kanañ a ri! ».
3. **Aléatoire / répétition** : rien côté moteur. Hors périmètre v1.
4. **Pistes d'un artiste** : `tracks_of_artist()` n'existe pas ; on passe
   aujourd'hui par `albums_of_artist()` puis `tracks_of_album()`.
5. **Durée totale de la file** : à calculer côté interface à partir des
   `duration_ms` de la base.

## Regroupement des artistes — une subtilité à connaître

La couverture MusicBrainz n'est pas totale : 25 030 morceaux sur 27 044 portent
un identifiant d'artiste d'album. Un même artiste peut donc avoir des pistes
étiquetées et d'autres non.

`Library::artists()` rattache les secondes aux premières quand le nom ne
désigne qu'un seul identifiant — sans ce rattrapage, l'artiste apparaît **deux
fois**, avec des comptes d'albums qui se contredisent. `albums_of_artist()`
prend pour cette raison l'identifiant **et** le nom : filtrer sur le seul
identifiant ferait ouvrir moins d'albums que la ligne n'en annonce.

## Décisions

- **Direction visuelle : 1a « Relief ».** La carte comme terrain — encre chaude
  sur papier, îlots en courbes de niveau, noms de styles posés à plat comme des
  toponymes. Newsreader pour les titres, IBM Plex Mono pour les données.
  Densité aérée, panneaux séparés par des filets d'un pixel. Fond `#1B1813`,
  accent ambre `#C07C4A`. Les thèmes sombre et clair sont tous deux dessinés,
  pas dérivés l'un de l'autre.
- **Regroupement des artistes : par identifiant MusicBrainz.** Repli sur le
  texte quand l'identifiant manque.
- **Périmètre de la première version**, au-delà de lire / mettre en pause / se
  déplacer : piste précédente et recherche sans accents. Sont écartés de la v1
  l'aléatoire, la répétition, et le réordonnancement de la file (qui reste donc
  en lecture seule).

## Questions ouvertes

- ~~**Marque affichée dans le rail**~~ — tranché : **Rusty Music** partout.
  Le rail, le titre de fenêtre, le binaire (`rusty-music`), les crates et la
  base (`rusty-music.db`) portent ce nom. Les deux maquettes de
  `ui/prototype/` gardent leur `carto.fm` d'origine : ce sont des documents
  datés, les réécrire falsifierait le compte rendu de la phase de design.
  Mention historique : la maquette portait `carto.fm v0.3`, le
  binaire s'appelle `rusty-music`, le projet s'appelle `rusty_music`. À unifier.
- **Forme d'onde réelle.** Le document de directions visuelles pousse 1a plus
  loin : l'onde y cesse d'être un motif décoratif pour devenir une enveloppe
  crête avec noyau RMS, lue à l'identique dans le transport, l'inspecteur et
  les pistes de stems — même dessin, trois échelles. Cela suppose de décoder
  chaque piste et de stocker l'enveloppe réduite ; c'est une brique commune
  avec le module 3. À trancher : barres décoratives en v1, ou vraie onde.
