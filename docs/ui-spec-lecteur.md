# Brief d'interface — Module 1 (Lecteur)

> **Périmètre : ce document ne couvre que le Module 1 (Lecteur), le mode
> « Écouter » de l'interface.** Le Module 2 (Exploration) est dans `ui-spec.md`,
> le Module 3 dans `ui-spec-editeur.md`. Voir `modules.md` pour la
> décomposition d'ensemble.
>
> **État (v0.1.0) : tout ce que ce document demandait est livré** — les
> manques de transport, la file, les trois vues de parcours, les pochettes et
> leur repli réseau. Ce qui reste est dans « Questions ouvertes » en fin de
> document. Les sections ci-dessous gardent le raisonnement d'origine ; les
> points livrés y sont barrés ou marqués.

## Ce qui est déjà tranché ailleurs — à reprendre tel quel

- **Coquille « Atelier »** (`ui-spec.md`) : rail gauche, carte au centre,
  inspecteur à droite, dock bas, transport pleine largeur persistant dans les
  quatre modes. Le lecteur n'ouvre pas de fenêtre à lui : il vit dans le mode
  **Écouter** de cette coquille.
- **Transport** (`ui/prototype/maquette-navigation.html`) : bouton rond 34×34,
  vignette de pochette 38×38 (rayon 6), titre 13 px/600 avec ellipse, artiste
  12 px atténué, minutage `00:00 / 03:41`. La progression en barres verticales
  de la maquette a été remplacée par le **spectrogramme du son réellement
  joué** (`spectre_transport`, `main.rs`), avec en HD la teinte de ce que le
  modèle a ajouté.
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
- **Déplacement dans la piste.** — **livré.** Un clic sur le spectrogramme à
  la position *x* appelle `seek`.
- **Volume.** — **livré** (curseur dans le transport, `set_volume`, linéaire,
  1.0 = niveau d'origine), complété par le bouton « N » (normalisation EBU
  R128, `docs/amelioration-audio.md`).
- **Aléatoire / répétition.** — **livré.** Deux boutons dans le transport du
  panneau de file : aléatoire (mélange Fisher-Yates de ce qui n'a pas encore
  été confié à la sortie, l'ordre d'avant est rendu à la désactivation) et
  répétition cyclant aucune → toutes → une.
- **Rafraîchissement.** `Player::position()` se lit par sondage, il n'y a pas
  de flux d'évènements. 4 à 10 rafraîchissements par seconde suffisent pour la
  progression ; inutile de viser la fréquence d'écran.

## File d'attente

Livrée. Ce qui avait été proposé :

- Panneau ouvert depuis le transport, en superposition à droite — **pas** un
  quatrième volet permanent, la coquille est déjà dense.
- Liste ordonnée, la piste en cours mise en évidence. Le moteur donne
  `Player::current()` et `Player::remaining()`.
- Un album envoyé en lecture remplace la file (`Player::play`) ; « ajouter à la
  suite » l'allonge (`Player::enqueue`).
- **Réordonnancement par glisser-déposer — livré.** Seule la portion de file
  pas encore confiée à la sortie se laisse déplacer ; le moteur ignore en
  silence un déplacement qui déborderait sur ce qui joue ou est préchargé, et
  l'interface se recale sur la file qu'il renvoie.

## Vues de parcours

Le mode Écouter a besoin de trois vues. Livrées, plus une quatrième : l'**univers
de l'artiste** (albums, biographie, artistes similaires, playlist de l'artiste).
Les volumes réels de la bibliothèque de test les contraignent fortement :

- **Artistes.** Liste virtualisée avec index alphabétique. Le regroupement se
  fait par identifiant MusicBrainz : sans lui, 1 384 des 3 543 entrées sont des
  variantes « X feat. Y » et la liste devient inutilisable.
- **Albums — 1 986 entrées.** Grille de pochettes. Chaque pochette coûte 50 à
  210 ms de lecture disque : chargement paresseux à l'affichage et cache
  mémoire indispensables, sinon la grille se traîne.
- **Pistes d'un album — 8 à 34 entrées.** Liste simple : numéro, titre, durée.
  Déjà triée par le moteur.

## Pochettes

- Deux origines locales, transparentes pour l'interface : image embarquée,
  sinon fichier du dossier. `Cover::source` le dit si l'on veut l'afficher.
  Sans l'une ni l'autre sur la première piste (celle que la grille retient),
  on essaie huit pistes sœurs de l'album, puis le repli réseau (`cover_depuis_reseau`, `main.rs`) :
  Cover Art Archive par release, puis par release-group, puis Deezer. Cache
  disque par album ; un « pas de pochette » périme après 30 jours, une panne
  réseau n'est jamais mémorisée.
- Tailles utiles : 38 px (transport), ~140 px (grille d'albums — resserré
  depuis le 180 px initial pour en montrer plus à l'écran), ~320 px
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

Manques identifiés en écrivant ce document. Les trois premiers sont faits, les
deux derniers sont des raccourcis de code sans effet visible :

1. ~~**Piste précédente**~~ — fait. `rodio` ne sachant qu'avancer, `previous()`
   reconstruit la sortie à partir du rang visé, sans toucher à la file : sans
   quoi un second retour en arrière serait impossible. Au-delà de trois
   secondes écoulées, la piste en cours est reprise à zéro.
2. ~~**Recherche sans accents**~~ — fait. Index FTS5 à contenu externe,
   tokenizer `unicode61 remove_diacritics 2`, tenu à jour par déclencheurs.
   « bjork » trouve « Björk », « kanan » trouve « Kanañ a ri! ».
3. ~~**Aléatoire / répétition**~~ — fait, avec le réordonnancement de la file
   par glisser-déposer. `Player` porte `set_alea`, `set_repetition`,
   `deplacer`, `verrou` (rang déjà confié à la sortie) ; l'aléatoire retient
   l'ordre d'avant pour le rendre à la désactivation.
4. **Pistes d'un artiste** : `tracks_of_artist()` n'existe pas ; on passe
   par `albums_of_artist()` puis `tracks_of_album()`. Aucun défaut constaté,
   à ne créer que si un besoin de performance apparaît.
5. **Durée totale de la file** : à calculer côté interface à partir des
   `duration_ms` de la base. Non affichée aujourd'hui.

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
  déplacer : piste précédente et recherche sans accents. L'aléatoire, la
  répétition et le réordonnancement de la file, d'abord écartés de la v1, ont
  été ajoutés depuis lors d'une reprise du panneau de file.

## Questions ouvertes

- ~~**Marque affichée dans le rail**~~ — tranché : **Rusty Music** partout.
  Le rail, le titre de fenêtre, le binaire (`rusty-music`), les crates et la
  base (`rusty-music.db`) portent ce nom. Les deux maquettes de
  `ui/prototype/` gardent leur `carto.fm` d'origine : ce sont des documents
  datés, les réécrire falsifierait le compte rendu de la phase de design.
  Mention historique : la maquette portait `carto.fm v0.3`, le
  binaire s'appelle `rusty-music`, le projet s'appelle `rusty_music`. À unifier.
- ~~**Forme d'onde réelle.**~~ — **tranché : le transport montre un vrai
  spectrogramme**, calculé à la demande (`spectre_transport`), pas des barres
  décoratives. L'enveloppe crête/RMS (160 tranches, `docs/journal.md`) existe
  aussi, côté Éditeur. Le « même dessin, trois échelles » du document de
  directions visuelles (transport, inspecteur, stems) n'est pas réalisé : le
  transport et l'établi ont chacun le leur.
