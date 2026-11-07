# Contrat d'interface — transversal aux écrans du rail

> Document de cohérence, pas de design. Les règles ci-dessous ont presque
> toutes été décidées ailleurs — `ui-spec.md` (Explorer), `ui-spec-lecteur.md`
> (Écouter), `ui-spec-editeur.md` (Pratiquer, Créer), `carto-anneau.md` (Anneau),
> `frise-filiations.md` (Temps) — puis oubliées dès qu'on quittait le module
> où elles avaient été écrites. Ce document les rassemble en un seul contrat,
> **applicable à tous les écrans du rail** — les **six modes** (Écouter,
> Pratiquer, Explorer, Créer, Découvrir, Valise) plus **Bibliothèque**, point
> d'entrée posé au-dessus du sélecteur de mode plutôt que dedans — et vérifie
> ce qui existe déjà contre ce contrat. Ce qu'il a été avant : « Historique »,
> en fin de document.

## Le socle commun — ce qu'aucun écran ne redéfinit

Le modèle retenu (`ui-spec.md`, « Atelier ») n'appartient pas à Explorer. Il
tient en quatre zones, présentes et identiques dans tous les écrans du rail :

- **Rail gauche fixe** — identité, puis **Bibliothèque** en entrée de rail
  isolée (point de départ, pas un mode), puis le sélecteur de mode à six
  boutons (`Écouter / Pratiquer / Explorer / Créer / Découvrir / Valise`),
  puis des blocs propres
  à l'écran courant, montrés ou masqués par `basculerMode()`
  (`apps/desktop/ui/js/modes.js`).
- **Zone centrale propre à chaque mode** — pas le même composant partout : le
  nuage/la carte/l'anneau/la frise pour Explorer, la grille d'albums ou la
  liste d'artistes pour Écouter, et pour Pratiquer et Créer tant qu'on y
  choisit quoi ouvrir — puis l'établi : la partition en Pratiquer, la pile de
  stems en Créer —, le tableau de bord de la bibliothèque pour Bibliothèque,
  le fil d'actualité pour Découvrir, la composition de la valise pour Valise.
  C'est la formulation exacte à retenir — pas « la carte
  toujours visible » de la première version d'`ui-spec.md`, qui datait d'avant
  l'ajout de Bibliothèque et Découvrir au rail et que le code n'a jamais
  suivie à la lettre (voir l'audit de Créer, ligne « Écart »).
- **Inspecteur droite, unique** — `aside.inspecteur`,
  jamais recréé ni dupliqué par un mode. Détail en Règle 1.
- **Transport pleine largeur en bas** — `footer.transport`, en dehors de tout conteneur conditionné par le mode ;
  aucun code ne le masque (vérifié : aucune occurrence de `.transport` suivie
  de `.hidden` dans `ui/js/`). Persiste tel quel dans tous les écrans, y
  compris Bibliothèque et Découvrir où on ne s'y attend pas forcément.
- **Dock optionnel** — `#dock`, réservé à Pratiquer et Créer, pousse le
  centre vers le haut sans reproduire la mécanique du rail ou de l'inspecteur.
- **Champ d'intention — dans l'affichage « Lama » d'Explorer** (déplacé le
  1ᵉʳ octobre 2026). Posé le 14 septembre comme une ligne partagée entre les
  modes (`#bloc-intention`, en tête de `<main class="centre">`), il vivait sous
  les quatre affichages d'Explorer (Nuage, Carte, Temps, Anneau) **sans aucun
  lien avec eux** : on y décrivait une playlist, la route n'apparaissait que sur
  deux d'entre eux, et rien n'expliquait le résultat. Il vit désormais dans un
  cinquième affichage, **Lama** (`data-affichage="lama"`, `#lama-vue`), dont le
  panneau central est dédié à l'interprète : le champ, un accueil avec des
  exemples de prompts, puis — sans cadres, des filets — la carte (un mini-nuage
  où la demande et la bibliothèque se répondent) en tête à côté de ce que Lama
  a compris, et dessous ce que Rusty Music a cherché et ce qui en sort. Le rail
  n'y garde que le sélecteur d'Affichage (en colonne). Le lama reste **dans Explorer** :
  demander une playlist en mots est une façon d'explorer la bibliothèque, pas
  un sixième écran du rail. Le plan interprété quitte l'inspecteur (le dernier
  bloc qu'il avait de plus que « ce qui s'écoute ») pour la carte « Lama a
  compris » — même précédent que la bio d'artiste au centre (Règle 1). Les
  autres affichages n'ont plus de champ ; la route de la playlist y garde sa
  couleur par partie.

- **Palette de recherche — au-dessus de tous les écrans** (1ᵉʳ novembre 2026,
  idée reprise de Disc Player, `MODELES.md`). ⌘K ou « / » ouvrent une boîte
  flottante (`#palette`, `js/palette.js`) au-dessus de l'écran courant, qui
  reste visible derrière un voile léger : chercher ne remplace rien (Règle 2).
  Résultats groupés — meilleur résultat, artistes, albums, pistes —,
  insensibles à la casse et aux accents ; ↑/↓, Entrée (ouvre l'artiste ou
  l'album, lance la piste), Échap. Les commandes de l'application vivent dans
  la même boîte (aller à un mode ou un affichage, thème, transport, file,
  favori, E/HD/N, ajouter un dossier, tout rafraîchir) : elles appellent des
  gestes qui existent déjà, jamais une action propre à la palette. Le champ
  `#q` du rail redevient un **filtre** de ce qui est affiché, dans tous les
  écrans où il apparaît (voir l'audit d'Écouter, Règle 2).

- **Vue plein écran « En cours »** (1ᵉʳ novembre 2026, idée de Disc Player).
  Un clic sur la pochette du transport (ou ⌘K « en cours ») ouvre, par-dessus
  rail, centre et inspecteur, le morceau qui joue : sa pochette sur un disque
  qui tourne pendant la lecture, le spectre autour (la colonne du spectrogramme
  du transport à la tête de lecture, aux couleurs de `--rampe`), titre,
  artiste, album. Le transport et la file restent utilisables ; Échap ferme
  (`#en-cours`, `js/en-cours.js`). Les fonds prennent les deux couleurs de la
  pochette : la seule exception à la Règle 3 (voir là). Avec
  `prefers-reduced-motion`, ni rotation ni spectre.

- **Choisir l'édition, face à face** (1ᵉʳ novembre 2026, idée de Disc
  Player). Sur les pistes d'un album ouvert, « Choisir l'édition… » ouvre une
  boîte au même voile que la palette (`#editions`, `js/editions.js`) : une
  phrase dit ce qui part sur le réseau (titre, artiste, adresse de contact,
  vers musicbrainz.org) ; à gauche les éditions proposées, une coche pour un
  même nombre de pistes (une concordance, pas une décision) ; à droite
  l'album de la bibliothèque ; dessous, les pistes alignées rang à rang, les
  lignes qui diffèrent marquées (piste seule, écart ≥ 3 s, titre). Rien n'est
  appliqué sans « Retenir » ; « Oublier ce choix » rend aux pistes leurs tags.

## Règles universelles — dans tous les écrans du rail, sans exception

### 1. Un seul inspecteur, partagé
Toute sélection peuple `#insp` (`aside.inspecteur`). Aucun mode ne recrée son
propre composant pochette/métadonnées/bio/critiques : c'est le même arbre DOM
que la sélection vienne d'un morceau de la grille, d'un point du nuage ou
d'un album de l'anneau.

**Exception — bio d'artiste au centre, tranchée le 15 septembre 2026.**
L'inspecteur reste dédié à ce qui **s'écoute** (piste ou album en cours,
transport bas) : pochette, métadonnées, famille, crédits d'édition, label,
critiques, voisins soniques — rien de tout ça ne dépend de la sélection
centrale. La biographie (TheAudioDB) est différente : elle dépend de
**quel artiste est ouvert au centre** (`ouvrirAlbumsArtiste`), pas de quel
morceau joue, et alourdissait l'inspecteur sans rapport avec l'écoute en
cours. Elle quitte donc `#insp` pour la zone centrale d'Écouter, affichée
avec la grille d'albums de l'artiste ouvert. Ça ne rouvre pas la règle : la
bio reste un contenu unique à la fois, jamais dupliqué entre centre et
inspecteur — elle vit dans un seul arbre DOM selon le contexte (« quel
artiste j'explore » vs « quel morceau j'écoute »), exactement comme la
Règle « Sélectionner n'est pas écouter » sépare déjà le clic silencieux du
geste ▶. **Faite** : la bio vit dans le bandeau `#autour-artiste` (bloc
`#autour-bio-bloc`) au-dessus des albums de l'artiste ouvert, résolue par
MBID d'artiste (commande `bio_artiste`, `majAutourArtiste` dans
`js/ecouter.js`) et non plus par piste ; l'inspecteur n'a plus de bloc bio.

**Sélectionner n'est pas écouter — le même geste doit produire le même effet
dans les quatre visualisations d'Explorer.** Décidé le 14 septembre 2026 après
audit : le clic jouait un morceau directement sur Nuage/Carte, mais se
contentait de sélectionner sur Anneau/Temps (« marcher » dans le graphe
d'albums sans interrompre l'écoute en cours) — deux modes muets, deux modes
bruyants, pour le même geste. Choix retenu : le clic reste **silencieux
partout** (sélection, inspecteur, éventuellement départ de chemin sur
Nuage/Carte), et l'écoute passe par un geste à part, le bouton ▶ de
l'inspecteur (`insp-lecture`, joue le morceau ou l'album selon ce qui est
affiché — `inspectionAlbum`/`inspectionPiste`). Ce bouton et son voisin ✦
(`insp-alchimie`) restent **en permanence visibles** sur la pochette — plus
seulement au survol comme leur équivalent `.album__lecture`/`.album__alchimie`
de la grille — pour que le geste qui lance l'écoute se devine, pas seulement
depuis l'Anneau/la Frise où il est le seul recours.

**Étendu à Écouter et à Éditer (aujourd'hui Pratiquer et Créer) le 21
septembre 2026.** Dans une liste de
morceaux (résultats de recherche, pistes d'un album), le clic sur la ligne
**sélectionne** (`selectionner`, `pisteSelectionnee` dans `js/ecouter.js`) : filet
d'accent à gauche (`ligne--select`), inspecteur peuplé, rien ne sonne. La
lecture est un geste à part : le numéro (→ ▶ au survol) ou le double-clic. La
sélection est un état propre, **distinct de la lecture** : `inspecter` suit
aussi le morceau joué (`battement`), il ne la pose pas, et une file qui avance
ne l'écrase pas ; lancer une lecture explicitement (▶, double-clic, clic dans la
file, bouton ▶ de l'inspecteur) la pose. Deux conséquences :
- **Le transport** montre la sélection en *aperçu* (« sélectionné · titre »,
  atténué) **seulement si rien n'est chargé** — ni lecture, ni pause, ni stems.
  Un morceau en cours garde le transport : choisir autre chose ne coupe rien.
  ▶ (ou Espace) lit alors la sélection. Purement interface : le moteur ne charge
  rien tant qu'on ne lit pas. Le spectrogramme de la barre suit ce qui est
  affiché — morceau joué, aperçu (calculé après 400 ms, donc déjà en cache au
  départ de la lecture) ou, quand les stems ont la main, morceau édité
  (`estAffiche` dans `js/lecteur.js`).
- **Pratiquer et Créer** prennent la sélection pour source (`morceauAEditer`),
  sans passer par la file de lecture : sélectionner puis entrer dans l'un des
  deux ouvre directement l'état « séparer », sans lecture préalable.

### 2. Estomper, jamais masquer
Un filtre ou une recherche laisse le contexte visible, atténué — il ne
retire jamais des éléments du rendu. Vaut pour un filtre sur la carte comme
pour une recherche dans les listes ou un critère dans Découvrir.

### 3. Couleur = famille, palette unique
Une seule source de vérité pour les douze teintes de familles :
`--familles` dans `style.css`, lue via `getComputedStyle` partout où
l'interface (hors tuiles de carte) a besoin d'une couleur de famille.
Exception documentée et bornée : `crates/carto/src/palette.rs` recalibre les
douze teintes **par thème de fond de carte** — décision déjà actée dans
`CLAUDE.md` (« Chaque `Palette` porte fond et ses 12 teintes de familles sur
la carte, calées sur le fond »). Ce n'est pas une redéfinition sauvage : le
fichier documente lui-même la frontière (« La carte seulement. Le nuage de
points t-SNE, la légende et le reste de l'application gardent la palette de
`--familles` »).

**Seconde exception, tranchée le 1ᵉʳ novembre 2026 : les couleurs d'album,
dans la seule vue plein écran « En cours ».** Deux couleurs tirées de la
pochette (méthode de Disc Player, `plan-ecouter-v0.3.md`, point 2), recalées
en OKLCH pour chaque thème, y colorent **les fonds seulement** : jamais un
contrôle, un trait ni un point, jamais ailleurs dans Écouter, jamais dans
Explorer. La vue n'affiche aucune famille : rien n'y lit la couleur comme une
famille. Le contrôle de contraste devra couvrir le pire cas du recalage avant
que la vue ne soit livrée. Élargir l'exception (en-tête d'un album ouvert…)
demande une nouvelle décision. **Livrée le même jour** (`js/couleurs-album.js`) :
les bornes du recalage sont des jetons de chaque thème (`--album-clarte`,
`--album-chroma`), et `scripts/contraste-interface.py` vérifie `--txt` sur leur
pire cas, toutes teintes : 8,1:1 en sombre, 8,3:1 en clair. Le texte de la vue
est entièrement en `--txt` : `--mut` n'y tiendrait pas.

### 4. Stabilité des positions et de la mise en page
Sélectionner un élément ne réorganise jamais ce qui l'entoure. Une liste ne
saute pas au clic, l'anneau et la frise ne redistribuent pas leurs positions
selon le focal.

### 5. Sobriété stricte
AudioMuse-AI comme référence négative explicite. Densité maîtrisée,
hiérarchie typographique forte, pas d'empilement de contrôles visibles en
permanence — dans tous les écrans, pas seulement Explorer où la règle a été
écrite.

### 6. Les deux thèmes traités sérieusement
Sombre et clair sont **dessinés** séparément — pas l'un dérivé
mécaniquement de l'autre — dans les deux sens : le contrat porte sur la
conception **et** sur l'accessibilité réelle du second thème depuis
l'interface. Tout texte tient 4,5:1 sur ses fonds (3:1 pour un glyphe ou du
grand texte), dans chaque thème ; `scripts/contraste-interface.py` le vérifie.

### 7. Zoom et pan cohérents
Mêmes gestes, mêmes contrôles, partout où zoom et pan existent — y compris un
futur zoom temporel d'une forme d'onde dans Créer, pas seulement le zoom
géographique de la Carte.

### 8. Révélation par échelle et par contexte
Jamais tout affiché en permanence. Vaut pour les étiquettes de la carte comme
pour les options avancées d'un mode quel qu'il soit.

## Règle scopée — modes qui affichent des relations entre morceaux

### 9. Un seul langage graphique pour la force d'une relation
Épaisseur et opacité du trait, jamais des styles de trait différents
(plein/tiret/pointillé) pour coder une variable continue. S'applique
aujourd'hui à Explorer → Anneau et Explorer → Temps. Ne s'écarte pas par
défaut pour Créer : si la génération s'appuyant sur toute la bibliothèque en
vient à afficher ses propres sources un jour, la même règle s'applique.

## Règle de gabarit — mesurée, pas regardée

### 10. Un seul gabarit
Les largeurs, marges et hauteurs que plusieurs règles CSS doivent partager sont
des **jetons** de `:root` (`style.css`), jamais des nombres recopiés :
`--largeur-rail` (232), `--largeur-droite` (300, inspecteur **et** file),
`--hauteur-transport` (64), `--gouttiere` (26, marge du contenu du panneau
central), `--gouttiere-panneau` (18, marge intérieure des panneaux),
`--colonne-lecture` (760). Quatre conséquences :
- **Une surimpression recouvre exactement le panneau qu'elle recouvre.** La file
  d'attente (`position: fixed`) fait la largeur de l'inspecteur, pas 20 px de
  plus : elle empiétait sur le centre, dans **tous** les modes, et lui volait sa
  marge de droite dès qu'une playlist s'ouvrait (relevé le 1ᵉʳ octobre 2026,
  d'abord à l'œil — « ce n'est plus symétrique quand la playlist est générée » —
  puis à la mesure).
- **Les marges sont symétriques**, la droite mesurée jusqu'au panneau droit
  *visible le plus proche* (inspecteur, ou file si elle est ouverte), pas jusqu'au
  bord de la fenêtre ni jusqu'à l'inspecteur seul.
- **Une colonne de lecture à largeur limitée est centrée** (Bibliothèque, Valise ;
  accueil du Lama) : le conteneur reste pleine largeur — la barre de défilement reste au
  bord du panneau — et c'est le padding qui centre.
- **Deux colonnes seulement si chacune a la place** : `auto-fit` plutôt qu'un
  rapport fixe (le Lama s'empile dans une fenêtre étroite au lieu de déborder).

Cette règle se **vérifie par `scripts/audit-interface.sh`** (`apps/desktop/ui/js/audit.js`) :
l'application, lancée en mode d'essai sur une copie de la base, parcourt écrans
× états (de base, file ouverte, morceau sélectionné) × thèmes × tailles de
fenêtre (960×620, 1 400×900, 1 920×1080) et contrôle G1 jetons, G2 symétrie des
marges, G3 absence de débordement, G4 aucun contrôle recouvert, G5 repère A–Z
réservé à l'Écoute (et, depuis le 1ᵉʳ novembre 2026, G6 typographie et G7
écritures, Règle 11 ; le contraste des deux thèmes en tête, Règle 6 ; depuis
le 7 novembre, G8 hauteur du rail, Règle 13). À lancer
après toute modification de la mise en page.

## Règle de typographie — mesurée, pas regardée

### 11. Un jeu réduit de tailles, rien sous 12 px, des noms dans toutes les écritures
Décidée le 1ᵉʳ novembre 2026, sur le modèle de l'audit typographique de Disc
Player (`MODELES.md`).

**Mesure de départ** (`style.css`, avant la règle) : **21 tailles** distinctes,
de 8,5 à 34 px, souvent à un demi-pixel l'une de l'autre (9, 9,5, 10, 10,5, 11,
11,5, 12, 12,5…) ; **168 déclarations**, dont **97 sous 12 px** — 49 d'entre
elles les étiquettes en capitales monospace (titres de blocs du rail à 9,5 px,
compteurs, durées) ; quatre graisses (400 ×2, 500 ×12, 600 ×8, 700 ×2) ; sur
le canevas, des noms à 10 et 11 px ; sur la carte, des étiquettes de 9 à 11 px.
Les polices déclarées (Newsreader, IBM Plex) ne sont pas embarquées : ce qui
s'affiche sur macOS est Georgia (titres), SF (texte) et SF Mono (données).
Aucun nom en écriture de droite à gauche n'était isolé, aucun nom japonais ou
coréen balisé.

**La règle.**
- **Six tailles, chacune un rôle** — des jetons de `:root`, jamais un nombre :
  `--t-legende` 12 (étiquettes en capitales, compteurs, durées, aides),
  `--t-second` 13 (texte secondaire : artiste d'une ligne, liens, notes),
  `--t-corps` 14 (racine, titres en liste, champs, navigation),
  `--t-intertitre` 16 (titre de l'inspecteur, champ de la palette),
  `--t-titre` 20 (titres de panneau), `--t-fil` 26 (titre du fil, serif) ;
  plus `--t-decor` 34, les initiales d'un artiste sur leur aplat (un décor).
- **Trois graisses** : 400 courant, 500 noms et navigation, 600 titres et
  boutons forts (700 fondu dans 600).
- **Aucun texte sous 12 px, sans exception** : étiquettes en capitales (leur
  interlettrage plafonné à 0,1 em), glyphes, repère A–Z, noms dessinés sur le
  canevas, étiquettes de la carte (`crates/carto/src/style.rs` ; un style
  écrit avant la règle est remonté à 12 px quand l'application le sert,
  `plancher_etiquettes`, sans régénérer les tuiles).
  Seule la gravure de la partition, rendue par alphaTab à l'échelle de la
  portée, n'est pas du texte d'interface.
- **Les noms propres** (titres, artistes, albums) **sont isolés pour le bidi** :
  tout élément en ligne est un isolat (`unicode-bidi: isolate`), et un nom
  joint à d'autres textes dans une même chaîne passe par `isoler()` (isolats
  U+2068 … U+2069 quand il porte une lettre de droite à gauche). **`lang` quand
  l'écriture le dit** : `ja` pour des kana, `ko` pour du hangul, posé par
  `observerEcritures()` (`js/ecritures.js`) sur l'élément qui porte le texte ;
  des idéogrammes seuls gardent la langue du document, rien ne distinguant
  alors le chinois du japonais.

**Vérifiée** par `scripts/audit-interface.sh` : en tête,
`scripts/typographie-interface.py` (toute taille de `style.css` passe par un
jeton, les jetons valent l'échelle, graisses 400/500/600, aucune police de
canevas sous 12 px, aucune taille en style en ligne) ; dans l'application, G6
(chaque texte visible dans l'une des sept tailles et l'une des trois graisses)
et G7 (un texte en kana ou en hangul porte son `lang`, un nom de droite à
gauche joint à d'autres textes est isolé) ; pour la carte, le test
`aucune_etiquette_sous_douze_pixels` (`cargo test`). Après application : 7
tailles (119 × 12, 30 × 13, 5 × 14, 8 × 16, 3 × 20, 4 × 26, 1 × 34). Le premier
audit complet a relevé deux défauts que les maquettes ne montraient pas : un
`<b>` du Lama au gras du navigateur (700 ; `b, strong` valent désormais 600) et
l'en-tête d'Écouter qui débordait de 5 px à 960 px sous WebKit (il passe à la
ligne). Ensuite : **198 cellules, 0 violation**.

## Règles de place — à quoi sert chaque zone, et combien elle tient

Proposées par `synthese-novembre-2026.md` (§ 2.4), écrites et mises en œuvre
le 7 novembre 2026 (B3, B4 ; `docs/journal.md`).

### 12. Une zone, un rôle

- **Le rail dit où je suis et ce que je regarde.** On y trouve la navigation,
  puis les réglages de la *vue* : filtrer, affichage, années, couleurs,
  familles. On n'y met pas d'outil qui agit sur un objet, pas de réglage
  d'application, pas d'aide de plus d'une ligne, pas de mention légale.
- **Le centre porte l'objet, et les commandes qui s'y posent** : zoom,
  calques, aura, chemin, bandeau de balade, carrefour. C'est la convention de
  toute carte en ligne, et c'est déjà celle de l'aura et de la balade.
- **L'inspecteur porte ce qui est choisi ou ce qui joue**, et ce qu'on peut
  en faire.
- **Le bas porte le temps** : le transport, et la barre de l'établi quand des
  stems ont la main.
- **Une aide de plus d'une ligne** vit dans l'état vide du centre, ou derrière
  un « ? ». **Une mention de licence** vit dans « À propos » et au moment du
  téléchargement.

Ce que chaque écran montre de ces zones (blocs du rail, contrôles de l'en-tête,
vues du centre, bloc du stem dans l'inspecteur, barre de l'établi) est déclaré
en un seul endroit : la table de `apps/desktop/ui/js/ecrans.js`, que
`basculerMode` applique. Un élément y est soit toujours là dans un écran, soit
là selon l'état de l'écran (la fonction qui gère cet état le *demande*), et
masqué partout ailleurs. Ajouter un bloc, c'est d'abord lui donner sa ligne
dans la table ; le test de fumée « chaque écran montre ce que dit sa table »
la vérifie écran par écran.

### 13. Le rail tient dans la fenêtre

À 1 400 × 900, avec les douze familles, ce que le rail montre sans geste tient
dans sa hauteur ; un `<details>` fermé ne compte que pour son titre.

**Vérifiée** par G8 dans `scripts/audit-interface.sh` (`audit.js`) : à la
fenêtre de 1 400 × 900, le bas du dernier élément visible du rail, compté
depuis le haut de son contenu, ne dépasse pas la hauteur visible du rail.
L'audit tourne sur une copie de la vraie bibliothèque, qui a ses douze
familles. `apps/desktop/ui-tests/rails.spec.js` donne la même mesure bloc par
bloc, et des captures (`rails.py` les assemble), sur une bibliothèque fictive à
douze familles et avec l'établi chargé, que l'audit ne voit pas.

**Ce qu'il a fallu** (7 novembre 2026, 8 écrans sur 13 dépassaient, la Carte
de 1 409 px) : les outils de la carte posés sur elle (zoom, « Calques »,
« Chemin » et son panneau) ; les réglages de la balade dans son bandeau ; les
balades passées et les chaînes repliées ; le Démixage sur une ligne l'établi
ouvert ; les aides de plus d'une ligne derrière un « ? » ou dans l'état vide du
centre ; l'adresse de contact dans Bibliothèque seulement. Puis, pour tous les
écrans : l'Affichage sur deux rangées, les six modes sur deux colonnes, la
légende des familles sur deux colonnes (nom coupé, nom entier et compte au
survol), le compte de la bibliothèque dans son bouton. Tout tient ; l'Anneau,
au plus juste (803 px pour 804) : un bloc de plus dans son rail le ferait
déborder.

## Principes généraux

Les treize règles ne couvrent pas tous les cas. Deux principes tranchent le
reste :

- **Cohérence et standards (Nielsen).** L'utilisateur ne doit jamais se
  demander si des mots, des situations ou des actions différentes signifient
  la même chose. Un bouton « régler » qui ouvre un tiroir dans Créer et un
  bouton « régler » qui bascule un état ailleurs sont la même faute, même si
  aucune des treize règles ne la nomme explicitement. Face à un cas nouveau : la
  question n'est pas « qu'est-ce qui est joli ici » mais « qu'est-ce que ce
  mot, ce geste, cette couleur signifient déjà ailleurs dans l'application ».
- **Divulgation progressive.** Montrer seulement ce qui est nécessaire à
  l'instant présent ; le reste attend un geste (survol, clic, expansion). Ce
  n'est pas seulement la Règle 8 (révélation par échelle) — c'est le principe
  qui la sous-tend et qui s'applique aussi à l'ordre d'apparition des
  contrôles, pas seulement à leur visibilité binaire.

Quand une règle et un principe semblent s'opposer sur un cas précis, le
principe l'emporte : les règles sont des applications déjà tranchées de ces
deux principes, pas des lois indépendantes d'eux.

## Audit de conformité

Une section par écran : Écouter, les affichages d'Explorer (Carte, Lama,
Anneau, Temps), Bibliothèque, Pratiquer, Créer, Découvrir, Valise ; puis la vue
« En cours » et les surimpressions de la carte. Verdicts : ✅ conforme · ❌ non
conforme (raison précise) · ⚠️ conforme sous réserve (laquelle) · — non
applicable · 🔧 tranché en doc, chantier de code ouvert. Les défauts I1 à I10
cités sont ceux de `synthese-novembre-2026.md`, § 2.3.

### Écouter

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | `inspecter()` (`js/ecouter.js`) peuple `#insp` pour pochette/métadonnées/crédits/critiques. Bio d'artiste : au centre, comme l'exception de la Règle 1 le veut — `#autour-artiste` au-dessus des albums de l'artiste ouvert, `bio_artiste` par MBID d'artiste ; plus de bloc bio dans `#insp`. |
| 2 | Estomper, jamais masquer | ✅ | **Le champ du rail : corrigé le 1ᵉʳ novembre 2026.** `#q` remplaçait la grille par une liste de résultats (`invoke("search")`, vue `recherche`). Deux voies étaient possibles : qu'il ouvre la palette, ou qu'il estompe la grille. **Retenu : il estompe** (`filtrerEcoute`, `repondAuFiltre`) — albums, artistes, pistes d'un album ou playlists qui ne répondent pas passent à 25 % d'opacité (`.album--estompe`, `.ligne--estompe`, pleine au survol), à leur place ; le fil dit « 2 sur 3 albums ». Raison : c'est déjà ce que le même champ fait en Explorer (il estompe la carte) — un champ qui filtrerait ici et ouvrirait une boîte là serait la faute de cohérence que le principe de Nielsen interdit ; et « chercher partout » a désormais son geste à soi, la palette, qui ne remplace rien non plus. Les deux se rejoignent : Entrée dans `#q` ouvre la palette avec le texte tapé, et le titre « Filtrer » porte son raccourci ⌘K. La vue `recherche` et `ligneRecherche` sont retirées. **Le filtre par famille d'Écouter, corrigé le même jour** : il retirait de la grille les albums des familles non cochées (`albumsAffiches`) ; ils s'estompent désormais, comme dans Découvrir (`passeFamille`, `estRetenu`, `js/ecouter-familles.js`), et le compte dit « n sur N ». Pistes d'un album (`lignePiste`) : clic = sélection, ▶ / double-clic = lecture (Règle 1) ; le nom d'artiste ne colore que lui et mène à ses albums. Signalement par la couleur seule, jamais de soulignement. |
| 3 | Palette unique | ✅ | Aucune couleur de famille codée en dur dans `ui/js/` ; tout passe par `--familles`. |
| 4 | Stabilité des positions | ✅ | `inspecter()` ne touche ni `grille.scrollTop` ni `liste.scrollTop` ; le scroll n'est réinitialisé que par une vraie navigation (`poser(..., scroll=0)`, `js/ecouter.js`), jamais par une sélection. |
| 5 | Sobriété stricte | ✅ | Les blocs du rail propres à d'autres modes restent masqués (`bloc-familles-ecoute`, etc., gérés par `basculerMode`) ; pas d'empilement visible. |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ci-dessous (résolu le 1ᵉʳ novembre 2026). Grille, liste, filtre et palette vérifiés dans les deux thèmes. |
| 7 | Zoom/pan cohérents | — | Pas de zoom en Écouter (listes/grille classiques). |
| 8 | Révélation par échelle | ✅ | Bio/crédits/critiques restent des blocs `hidden` tant qu'ils n'ont pas de contenu (`bloc-credits`, `bloc-critiques` ; au centre, les sections de `#autour-artiste`), pas affichés vides en permanence. |
| 9 | Langage de force unique | — | Aucune relation entre morceaux affichée en Écouter. |

### Explorer → Carte

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Même `inspecter()` que partout ailleurs. **Corrigé** : le clic sur un point jouait le morceau directement (`cnv` → `click`), contrairement au clic sur l'Anneau/la Frise qui ne fait que sélectionner — même geste, deux effets différents. Le clic est maintenant silencieux ici aussi (sélectionne, peuple l'inspecteur, devient le départ de chemin), et le bouton ▶ de l'inspecteur (`insp-lecture`) reste en permanence visible sur la pochette, plus seulement au survol — c'est lui qui lance l'écoute, dans les quatre visualisations. |
| 2 | Estomper, jamais masquer | ✅ | Trois mécanismes distincts et tous conformes : isolement de famille (`ui/js/`, `globalAlpha` réduit), filtre texte (`ui/js/`, commentaire explicite « les morceaux qui ne correspondent pas s'estompent »), intervalle d'années (`ui/js/`, voile semi-opaque plutôt que masquage). |
| 3 | Palette unique | ✅ | Voir Règle 3 : `palette.rs` recalibre par thème de fond, exception documentée et actée dans `CLAUDE.md`. |
| 4 | Stabilité des positions | ✅ | La position d'un point ne dépend que de la projection t-SNE / des tuiles, jamais de la sélection courante. |
| 5 | Sobriété stricte | ✅ | Réglages avancés (perplexité, époques, k-means) déportés dans le mode Bibliothèque plutôt que dans le rail d'Explorer — conforme à la contrainte de `ui-spec.md` (« les réglages avancés restent dans un tiroir rétractable, pas dans le rail »), même si le rangement choisi (un autre mode entier plutôt qu'un tiroir) est plus lourd qu'un tiroir — voir Bibliothèque, Règle 8. |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ci-dessous. Le canevas se redessine au changement de thème ; les tuiles gardent leur propre fond de plan (Règle 3). |
| 7 | Zoom/pan cohérents | ✅ | `zoomer()` (`js/maplibre.js`) est la fonction unique appelée par la molette (`js/maplibre.js`) et par les boutons `+`/`−`/réinitialiser posés sur la carte (`#bloc-zoom`, sous les outils, Règle 12) ; même geste, même lecture `zoom-val`, quel que soit le sous-mode. |
| 8 | Révélation par échelle | ✅ | Étiquettes de tuiles dépendantes du zoom (MapLibre), légende cliquable plutôt qu'affichage permanent de tous les noms de famille. |
| 9 | Langage de force unique | — | Le nuage/la carte n'affiche pas de relations entre morceaux (le chemin tracé est un itinéraire, pas une variable continue à encoder). |

### Explorer → Lama

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Cliquer un morceau du résultat (liste, point de la route) le sélectionne et peuple le même `#insp` (`selectionner`) ; le plan, lui, vit dans la carte « Lama a compris » au centre — l'inspecteur ne garde que ce qui s'écoute. |
| 2 | Estomper, jamais masquer | ✅ | Le mini-nuage dessine toute la bibliothèque, estompée, et colore par-dessus ce que les filtres retiennent ; la partie non survolée s'atténue (`globalAlpha`), jamais ne disparaît. |
| 3 | Palette unique | ⚠️ | Six teintes de **parties** (`--partie-1..6`), prises dans la même encre que `--familles` mais distinctes d'elles : les parties et les familles ne se lisent pas ensemble (le mini-nuage montre les premières, pas les secondes). Dessinées pour les deux thèmes. |
| 4 | Stabilité des positions | ✅ | Les points du mini-nuage ne bougent jamais (t-SNE) ; survoler une partie ne réordonne rien. |
| 5 | Sobriété stricte | ✅ | Le rail ne garde que le sélecteur d'Affichage : Années, Colorer par, Familles et Chercher y sont masqués en bloc (le zoom et les outils sont sur la carte, qui n'est pas affichée) (`majRailExplorer`) — ils réglaient une carte qu'on ne voit pas. Accueil et exemples effacés dès la première demande (« Exemples » les rouvre), réponse brute du modèle repliée, sections séparées par des filets plutôt qu'encadrées. Le choix du modèle (intégré ou d'Ollama) reste derrière l'icône 🦙 ; sans modèle utilisable, une note de l'accueil dit quoi faire. |
| 6 | Deux thèmes sérieux | ✅ | Vérifié sombre et clair le 1ᵉʳ octobre (maquette navigateur, voir ci-dessous) ; atteignable depuis le 1ᵉʳ novembre (constat transversal). |
| 7 | Zoom/pan cohérents | — | Le mini-nuage n'a ni zoom ni pan : c'est un aperçu, le Nuage et la Carte portent les gestes. |
| 8 | Révélation par échelle | ✅ | Détails à la demande : survol d'une partie (barre, liste, résultat, route), bulle du morceau au survol d'un point de la route, dépliants. |
| 9 | Langage de force unique | — | Aucune relation entre morceaux n'est encodée par un trait (la route est un ordre, pas une force). |

Vérification de l'interface : la webview du système ne se pilote pas et ne se
peint pas hors de son Espace macOS ; le panneau a donc été vérifié dans un
navigateur, servi avec une fausse API Tauri alimentée de **vraies** données
exportées du moteur (`exporter_pour_maquette`), puis l'application réelle
a été lancée (`RUSTY_MUSIC_AFFICHAGE=lama RUSTY_MUSIC_PROMPT=…`) pour contrôler
dans son journal que la composition va au bout.

### Explorer → Anneau

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | `inspecterAlbum()` (`js/ecouter.js`) réutilise explicitement le même composant : commentaire « le même composant que pour un morceau (`inspecter`), généralisé plutôt que dupliqué » (`js/ecouter.js`). Cas exemplaire de la règle bien appliquée. Manquait un geste pour être *utile*, pas seulement unique : cliquer un album de l'anneau le rend focal (`chargerAnneau`) sans le jouer, et le panneau n'offrait aucun moyen de l'écouter — corrigé par un bouton ▶ (`insp-lecture`) posé sur la pochette, symétrique du ✦ existant, qui appelle `lireAlbum()` pour un album ou joue directement le morceau affiché sinon (même composant, même geste, dans les deux cas). |
| 2 | Estomper, jamais masquer | ✅ | `ui/js/` : fond permanent de liens en trace ténue (`globalAlpha` 0,15) et sélection qui fait ressortir sans jamais faire disparaître le reste — corrige explicitement le premier défaut relevé dans `carto-anneau.md` (« seuls les voisins du focal sont visibles »). |
| 3 | Palette unique | ✅ | Même lecture de `--familles`. |
| 4 | Stabilité des positions | ✅ | Décidé et implémenté conformément à `carto-anneau.md` (« la position d'un album sur l'anneau ne dépend jamais du focal sélectionné »). |
| 5 | Sobriété stricte | ✅ | Pas de contrôle superflu ; `k`/`beta` du bundling sont deux réglettes, pas un panneau. |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ; vérifié en clair le 1ᵉʳ novembre (maquette). |
| 7 | Zoom/pan cohérents | ✅ | Corrigé : l'exception (« un cercle de rayon fixe n'a rien à agrandir ») ne tenait plus dès lors que l'anneau grandit à l'usage — familles denses, noms serrés — sans que rien n'empêchait de l'agrandir à l'écran. `zoomerAnneau()` reprend le principe du repli sans MapLibre du nuage (un seul facteur `anneau.vue.k`, `dx`/`dy` pour glisser), branché sur `zoomer()` comme les autres sous-modes ; `#bloc-zoom` n'est plus masqué en Anneau. |
| 8 | Révélation par échelle | ✅ | Noms au survol, pas en permanence sur chaque segment — conforme à `carto-anneau.md`. |
| 9 | Langage de force unique | ✅ | `setLineDash` n'est jamais utilisé pour coder la force d'un lien (seulement pour le lasso, le tracé en cours, la colonne « incertain ») ; force = largeur + opacité par sélection (`ui/js/`), exactement la correction actée dans `carto-anneau.md` (« un seul style de trait, force = largeur + opacité »). |

### Explorer → Temps (frise des filiations)

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Même mécanisme que l'Anneau (le clic sur un album de la frise appelle la même voie d'inspection) — hérite donc aussi du bouton ▶ ajouté là-bas (`insp-lecture`), même geste, même composant. |
| 2 | Estomper, jamais masquer | ✅ | Colonne « dates incertaines » en trait pointillé atténué plutôt que morceaux exclus (`ui/js/`) ; intervalle d'années : voile, pas masquage (même mécanisme que Carte). |
| 3 | Palette unique | ✅ | Bandes colorées par la même famille/`--familles`. |
| 4 | Stabilité des positions | ✅ | Par construction : `frise-filiations.md` § 1 retient les bandes par famille comme mode par défaut précisément parce qu'« une bande est une structure stable [qui] accueille les nouveaux albums sans redistribuer l'existant » — writen avant l'implémentation, respecté dedans. |
| 5 | Sobriété stricte | ✅ | Un seuil de connexions (réglette), rien de plus dans le rail. |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ; vérifié en clair le 1ᵉʳ novembre (maquette). |
| 7 | Zoom/pan cohérents | ✅ | `zoomerTemps()` (`js/maplibre.js`) est décrit dans son propre commentaire comme suivant « exactement » le même principe que le zoom du nuage — un seul facteur `k`, même geste molette, même lecture `zoom-val`. |
| 8 | Révélation par échelle | ✅ | Année affichée au survol seulement (`frise-filiations.md` § 5), pas d'empilement de labels sur l'axe. |
| 9 | Langage de force unique | ✅ | Même mécanique que l'Anneau : fond permanent en trace ténue, force encodée par largeur/opacité à la sélection — aucun `setLineDash` sur les arcs eux-mêmes. |

### Bibliothèque

> Rappel de vocabulaire : « Bibliothèque » désigne ici l'écran atteint par
> l'entrée de rail du même nom (réglages, statistiques, vérifications) — pas
> un mode au sens du sélecteur de modes, et pas le fait de parcourir
> ses artistes/albums, qui se passe en mode **Écouter**. La Règle 2 sur « une
> recherche dans Bibliothèque » de la commande d'origine vise en réalité la
> recherche du rail, auditée sous Écouter.

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | — | Aucune sélection de morceau dans ce mode : les listes de vérification (genres suspects, éditions multiples, doublons, points isolés) n'ont pas de gestionnaire de clic vers l'inspecteur (`chargerVerifications`, `js/bibliotheque.js` — chaque ligne n'a qu'un texte et parfois un bouton d'action, jamais un `onClick` de sélection). |
| 2 | Estomper, jamais masquer | — | Rien à filtrer dans ce mode dans son état actuel. |
| 3 | Palette unique | — | Pas de rendu coloré par famille ici. |
| 4 | Stabilité des positions | ✅ | Page de réglages classique, rien qui se réorganise au clic. |
| 5 | Sobriété stricte | ✅ | **Corrigé le 1ᵉʳ novembre 2026.** La page empilait, ouverts d'emblée, une dizaine de blocs de réglages et de vérifications — le « panneau d'administration » qu'`ui-spec.md` cite comme repoussoir. Ne restent ouverts que la source (dossiers, « Ajouter », « Tout rafraîchir » ; ses options repliées sous « Options de la chaîne ») et l'**état de la bibliothèque** : six lignes (identités, pochettes suspectes, genres, éditions multiples, doublons probables, fichiers en échec), chacune un compte, suivies de « Compléter » (2 nov. : une phrase dit ce qui partira sur le réseau, un bouton ; « Arrêter » et « Annuler cette passe » n'apparaissent que quand ils servent). Tout le reste — Discogs, stockage, apparence, aperçu, paramètres de la carte, vocabulaire, nappe, à propos — est un `<details>` fermé derrière son titre. |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ; vérifié en clair le 1ᵉʳ novembre (maquette). |
| 7 | Zoom/pan cohérents | — | Pas de zoom dans ce mode. |
| 8 | Révélation par échelle | ✅ | Chaque ligne de l'état déplie sa liste sur place (les listes des anciennes « Vérifications », mêmes identifiants) ; un compte nul se lit atténué, un compte à examiner en `--txt`. Les réglages avancés attendent l'ouverture de leur section. |
| 9 | Langage de force unique | — | Pas de relations entre morceaux affichées. |

### Pratiquer

Les deux modes partagent l'établi (`#editer-separer`, `#editer-etabli`),
l'état `edition` et les états « choisir » et « séparer » ; `enStems()`
(`socle.js`) dit si l'on est dans l'un des deux. En Pratiquer, l'établi prend
la classe `etabli--pratiquer` : la partition au centre, les stems réduits à une
rangée de niveaux sans spectrogramme (`majUsage`, `js/pratiquer.js`).

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Le morceau source passe par le même `#insp` ; le détail d'un stem sélectionné aussi (`#bloc-stem`, `majInspecteurStem`). La partition au centre n'est pas un second inspecteur : c'est la vue propre au mode. |
| 2 | Estomper, jamais masquer | ✅ | Dans l'état « choisir », la grille ou la liste suit le filtre du rail comme en Écouter (`filtrerEcoute`, `enStems()`) : estompée, rien de retiré. Pas de filtre sur la partition. |
| 3 | Palette unique | — | Rien n'y est coloré par famille. La partition a ses jetons de feuille (`--feuille-*`), la section et la mesure courantes prennent `--accent`. |
| 4 | Stabilité des positions | ✅ | La partition défile avec la lecture, mais une molette de l'utilisateur suspend ce suivi 3 s (`partition.moletteA`) : on ne lui reprend pas la page. Changer d'instrument (Basse, Batterie) remplace la partie, sans déplacer le reste. |
| 5 | Sobriété stricte | ❌ | Quatre écarts relevés le 7 nov., trois corrigés le jour même : « Filtrer » ne paraît plus que dans l'état « choisir » (I4) ; le décompte dit « Sans · 1 mesure · 2 mesures » et tient sur une ligne (I10) ; la mention de licence de BS-RoFormer « SW » (`#demix-provenance`) a quitté le rail pour le centre de l'état « séparer », quand ses poids sont à télécharger, et « À propos » (I9, B4). L'établi ouvert, le Démixage tient sur une ligne (« changer » le redéplie) et l'aide de la partie est dans l'état vide de la partition. Reste : le panneau d'un stem dans l'inspecteur porte la greffe (`zoneGreffe`, dans `panneauStem` quel que soit le mode), qui relève de Créer. |
| 6 | Deux thèmes sérieux | ✅ | La partition passe par des jetons dessinés pour chaque thème (aplat clair et encre sombre en clair), vérifiés par `scripts/contraste-interface.py`. |
| 7 | Zoom/pan cohérents | — | Pas de zoom : la molette fait défiler la partition, elle ne l'agrandit pas. |
| 8 | Révélation par échelle | ✅ | Le bloc « Votre partie » (`#bloc-usage`) n'apparaît qu'une fois les stems là ; la notation seulement pour la basse, Songsterr seulement si le morceau a un titre, la boucle (`#boucle-reglage`) seulement une fois la pulsation du morceau connue ; rien ne se transcrit sans « Calculer ». |
| 9 | Langage de force unique | — | Pas de relations entre morceaux affichées. |
| — | Écart, hors des neuf règles | ✅ | **Corrigé le 7 nov. (I5).** Les stems s'appelaient Drums, Bass, Other, Vocals… (le nom du moteur) quand le rail dit Basse et Batterie. Une table côté interface (`libelleStem`, `js/editer.js`) les dit Batterie, Basse, Voix, Autres, Guitare, Piano dans la pile et l'inspecteur ; le nom du moteur reste la clé (`data-stem`, commandes). Vaut aussi pour Créer. |

### Créer

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Le morceau source passe par le même inspecteur ; **le détail d'un stem sélectionné y va aussi** (`#bloc-stem`, `majInspecteurStem`), avec sa vitesse, sa hauteur et d'où le remplacer — plus de panneau déplié sous la ligne dans la pile. |
| 2 | Estomper, jamais masquer | ✅ | Le sélecteur (grille ou liste, avant « séparer ») suit le filtre du rail comme en Écouter : estompé, rien de retiré. Pas de filtre dans la pile ni la barre d'outils ; les candidats de greffe (`zoneGreffe`) sont une liste de résultats, pas un filtre sur un ensemble déjà visible. |
| 3 | Palette unique | — | La pile de stems ne colore rien par famille (le mini-nuage de greffe, quand il existera, devra lire `--familles`). |
| 4 | Stabilité des positions | ✅ | Sélectionner un stem (`edition.stemSel`) souligne sa ligne et peuple l'inspecteur, sans déplacer les lignes de la pile ni les réordonner. Passer de Pratiquer à Créer garde les stems chargés, sans rien relancer (test de fumée « Pratiquer puis Créer »). |
| 5 | Sobriété stricte | ✅ | La barre d'outils et la pile restent sobres (deux pas-à-pas d'ensemble, la dérive quand elle existe, Exporter ; par stem : nom, solo, muet, badge, niveau, spectrogramme). « Filtrer » ne paraît que dans l'état « choisir » (I4, corrigé le 7 nov.). |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ; vérifié en clair le 1ᵉʳ novembre (maquette). |
| 7 | Zoom/pan cohérents | — | Pas encore de zoom temporel de la pile. Le contrat de la Règle 7 s'appliquera dès qu'il existera : mêmes gestes que `zoomer()`/`zoomerTemps()`, et le playhead unique (`positionnerPlayhead`) devra suivre le même facteur. |
| 8 | Révélation par échelle | ✅ | Le détail vitesse/hauteur/greffe d'un stem n'apparaît qu'à la sélection de sa ligne (`majInspecteurStem`), dans l'inspecteur commun. |
| 9 | Langage de force unique | — | Pas de relations entre morceaux affichées ; la liste de voisins pour la greffe (`zoneGreffe`) est une liste classée. À surveiller si le mini-nuage de greffe (prévu, `ui-spec-editeur.md`) affiche un jour des liens. |
| — | Écart, hors des neuf règles | ✅ | **Résolu le 10 septembre 2026 — doc puis code.** `ui-spec-editeur.md` a retranché « le dock pousse la carte vers le haut » ; le centre d'Éditer (aujourd'hui de Pratiquer et de Créer) est **l'établi à trois états** (`majEtatEditer` : `#grille`/`#liste` → `#editer-separer` → `#editer-etabli`), `#bloc-demix` est descendu du rail au centre, le détail d'un stem peuple `#bloc-stem` dans l'inspecteur, et `#dock` est une barre d'outils pleine largeur (plus de `max-height: 34vh`). Reste une passe de vérification visuelle dans l'app en fonctionnement (alignement du playhead au pixel, largeur de la barre sur fenêtre étroite). |

### Découvrir

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | `#decouvrir-centre` (nom d'artiste, fil de collaborations) n'est pas un inspecteur de morceau redondant : c'est le contenu central propre à ce mode, au même titre que la grille d'albums l'est pour Écouter. Aucune métadonnée de morceau (pochette, tempo, durée…) n'y est dupliquée. |
| 2 | Estomper, jamais masquer | ✅ | **Résolu le 20 septembre 2026** avec le passage du fil en grille de pochettes : `rendreFilDecouvrir()` construit toutes les cartes, et celles hors du filtre par famille (`sortiePasseFamille` / `voisinPasseFamille`) reçoivent `.album--estompe` (opacité .25, pleine au survol) au lieu d'être retirées. La grille ne se recompose donc pas ; seuls les compteurs d'onglets suivent le filtre. |
| 3 | Palette unique | ✅ | Légende de familles de Découvrir tirée de la même passe que celle de la carte (`chargerFamilles`, `js/maplibre.js`). |
| 4 | Stabilité des positions | ✅ | Naviguer entre artistes pousse sur un fil d'Ariane (`decouvrirFil`) sans réordonner la liste de collaborations déjà affichée ; le filtre par famille estompe sans recomposer la grille. |
| 5 | Sobriété stricte | ✅ | Trois onglets (Sorties/Collaborations/À écouter ailleurs), un fil, une légende de familles repliable — pas d'empilement. |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ; vérifié en clair le 1ᵉʳ novembre (maquette). |
| 7 | Zoom/pan cohérents | — | Pas de zoom dans ce mode. |
| 8 | Révélation par échelle | ✅ | Le bloc Familles ne s'affiche que quand il a un sens (`majBlocFamillesDecouvrir`, carte déjà calculée) plutôt qu'en permanence. |
| 9 | Langage de force unique | — | Pas de relations graphiques dans ce mode (des listes, pas un anneau ou une frise). |

### Valise (19 octobre 2026)

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Rien ne s'y écoute : la liste des albums de la valise ne peuple pas l'inspecteur et n'en recrée pas un ; l'inspecteur garde ce qui joue. |
| 2 | Estomper, jamais masquer | — | Pas de filtre : la liste est le résultat des réglages. Un album écarté sort de la valise (c'est le geste demandé), son retour se fait par « Tout rétablir ». |
| 3 | Palette unique | ✅ | Pastille de famille par `couleursFamilles()` (`--familles`). |
| 4 | Stabilité des positions | ✅ | Garder un album ne réordonne pas la liste au-delà de ce que la sélection change (l'album gardé passe en tête : il est pris avant les règles). |
| 5 | Sobriété stricte | ✅ | Trois blocs (composition, limites, contenu) ; l'iPhone et ses gestes dans le rail ; « Renouveler l'app » n'apparaît que sur une machine qui a le dépôt et Xcode ; « Emporter la lecture » (2 nov.) seulement quand l'app est sur l'iPhone ou que l'iPhone est appairé pour le Wi-Fi (3 nov.), et sa phrase de résultat sous le bouton, pas en bandeau ; l'état Wi-Fi (lecture ou valise qui attend l'iPhone) tient en une ligne d'aide, « Envoyer sur l'iPhone » sert aussi sans câble, « Oublier l'iPhone » est un lien qui n'apparaît qu'une fois appairé. Côté compagnon, « Reprendre sur le Mac » est une icône du lecteur, montrée seulement quand le Mac répond. |
| 6 | Deux thèmes sérieux | ✅ | Constat transversal ; vérifié en clair le 1ᵉʳ novembre (maquette). |
| 7 | Zoom/pan cohérents | — | Pas de zoom. |
| 8 | Révélation par échelle | ✅ | Le choix de famille n'apparaît que sur une ligne « Famille » ; les albums écartés ne se comptent que s'il y en a ; l'avancement de l'envoi seulement pendant l'envoi. |
| 9 | Langage de force unique | — | Pas de relations. |

### Vue « En cours » (1ᵉʳ novembre 2026)

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | La vue montre ce que montre le transport (pochette, titre, artiste) et l'album, en grand ; rien du détail de l'inspecteur (crédits, critiques, famille, voisins), qui reste le seul à les porter. |
| 2 | Estomper, jamais masquer | — | Pas de filtre. |
| 3 | Palette unique | ✅ | Exception écrite et bornée (Règle 3) : couleurs d'album sur les fonds seulement, dans cette vue seulement ; le spectre suit `--rampe`, aucune famille n'est affichée. |
| 4 | Stabilité des positions | ✅ | Rien ne se réorganise : le disque tourne sur place, les barres du spectre ne bougent qu'en longueur. |
| 5 | Sobriété stricte | ✅ | Un seul contrôle propre (×) ; le transport en dessous garde les siens. |
| 6 | Deux thèmes sérieux | ✅ | Bornes de recalage dessinées pour chaque thème, contraste vérifié sur leur pire cas ; captures dans les deux thèmes (`docs/images/en-cours-*.jpg`). |
| 7 | Zoom/pan cohérents | — | Pas de zoom. |
| 8 | Révélation par échelle | ✅ | La vue attend un geste (pochette du transport, ⌘K) ; elle n'est jamais affichée d'office. |
| 9 | Langage de force unique | — | Le spectre est une rampe d'énergie, pas une relation entre morceaux. |
| 11 | Typographie | ✅ | Titre `--t-fil` (serif), artiste `--t-intertitre`, album `--t-second`. |

L'audit la parcourt comme un douzième écran (`en-cours`), la vue recouvrant
par construction ce qu'elle recouvre (exception G4 ; G5 ne compte plus un
repère recouvert). Le premier passage a relevé un vrai défaut : la file,
ouverte par-dessus, cachait le × de la vue ; la vue s'arrête désormais au bord
de la file (Règle 10). Ensuite : **216 cellules, 0 violation**.

### Surimpressions de la carte (balade, 7 novembre 2026)

Ce qui se pose sur la carte d'Explorer (Nuage, Carte) sans en être un
affichage : le **bandeau de balade** (`#carte-balade`, en haut à gauche : où en
est la balade, « ← Carrefour précédent », « Arrêter la balade », et
« Réglages », qui déplie l'intervention et l'allure) ; les **outils** (en haut à
droite : « Chemin » et « Calques », chacun son panneau sous le zoom, un seul
ouvert à la fois ; le premier Maj+clic ouvre celui du chemin) ; le
**carrefour** (`#carte-carrefour`, deux ou trois voies à côté du promeneur à la
fin d'un morceau, `js/balade.js`) ; l'**aura** (`#carte-aura`, au bout de la ligne des outils, `js/aura.js`) ; le **brouillard** et les sentiers battus
(couches du style, options du fond de plan, `js/traces.js`) ; le **menu du clic
droit** (`#carte-menu` : « Flâner d'ici », « Épingler ici », « Retirer
l'épingle », et le formulaire d'épingle, `js/lieux.js`).

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Aucune ne recrée l'inspecteur. Une voie du carrefour montre titre, artiste et famille comme une ligne de liste, pour choisir ; la prendre décide de la suite de la balade, elle ne sélectionne pas. Le menu ne porte que des gestes. Survoler une voie en fait entendre 5 s par l'aura, très bas : le son principal ne baisse pas, ce n'est pas une lecture. |
| 2 | Estomper, jamais masquer | ✅ | Le brouillard voile, il ne retire pas : papier à 0,72 sur le bâti jamais écouté, la teinte de la famille se devine dessous, et le bâtiment reste cliquable. Au carrefour, une voie déjà écoutée et les voies d'un carrefour figé passent en retrait (opacité 0,45), à leur place. |
| 3 | Palette unique | ⚠️ | Les pastilles des voies prennent la teinte de la famille sur le fond où l'on est (`couleursFamillesCarte` sur le plan, `couleursFamilles` ailleurs) ; promeneur, tracé et sentiers prennent `--accent` (la teinte posée par le style est remplacée par l'interface) ; le voile prend le papier de la palette. Réserve : le nuancier des épingles (six teintes codées dans `lieux.js`) n'est pas une famille, mais rien ne l'en distingue sur la carte. |
| 4 | Stabilité des positions | ✅ | La carte ne se recadre jamais d'elle-même pendant une balade ; seuls un clic sur une balade passée ou une chaîne la cadrent (`fitBounds`). Le carrefour se place à côté du promeneur, jamais dessus (`placerPanneau`). Lever le brouillard d'un bâtiment est un `setFeatureState`, rien ne se redessine ailleurs. |
| 5 | Sobriété stricte | ✅ | Une seule surimpression permanente, la ligne des outils (« Chemin », « Calques » sur la Carte, l'aura) et le zoom, des boutons nommés ; leurs panneaux sont fermés par défaut, un seul ouvert à la fois. Le bandeau n'existe qu'en balade, le carrefour qu'à la fin d'un morceau, et « ← Carrefour précédent » que s'il y a où revenir. Le menu ne montre que les entrées qui valent à l'endroit cliqué. Brouillard et sentiers sont des options éteintes par défaut (98,5 % de la ville serait voilée). |
| 6 | Deux thèmes sérieux | ⚠️ | Les cartouches (bandeau, carrefour, aura, menu) sont en jetons (`--panel`, `--line`, `--txt`, `--mut`, `--accent`) ; le voile suit la palette du fond. Réserve : les ombres portées sont des `rgba` noirs codés, et les teintes du nuancier sont les mêmes dans les deux thèmes. |
| 7 | Zoom/pan cohérents | ✅ | Aucune n'a de zoom propre : la carte garde ses gestes. Le carrefour se replace près du promeneur à chaque mise à jour de la balade (le sondage du lecteur) : après un glisser, il peut traîner jusqu'à la suivante. |
| 8 | Révélation par échelle | ✅ | Le compteur de quartier n'apparaît qu'au survol d'un territoire ; le carrefour attend la fin du morceau ; le formulaire d'épingle attend « Épingler ici » ; Échap ferme le carrefour, le menu et « Garder ». |
| 9 | Langage de force unique | ✅ | Les sentiers battus codent le nombre de passages par l'épaisseur et l'opacité seulement, jamais par un style de trait ; les pointillés d'une balade passée redessinée ne codent pas une valeur. |
| 10 | Gabarit | ⚠️ | Bandeau et carrefour posés sur `--gouttiere`, mais l'audit (`audit.js`) ne parcourt aucun état de balade : bandeau, carrefour, menu et cartouche « Garder » ne sont pas mesurés. Le relief, lui, a son écran (`explorer:carte+relief`). |

### Constat transversal — Règle 10, le gabarit (1ᵉʳ octobre 2026)

Première exécution de `scripts/audit-interface.sh` sur l'état d'avant les jetons :
**290 violations sur 180 cellules.**

| Constat | Écrans | Correction |
|---|---|---|
| La file d'attente fait **320 px** pour un inspecteur de **300** : elle empiète de 20 px sur le centre, marge de droite **6 px au lieu de 26** (G1, G2) | tous | `--largeur-droite` partagé par l'inspecteur et la file |
| Lettres du repère A–Z **recouvertes** par la file ouverte (G4) | Écoute | disparaît avec l'empiétement |
| Gouttière de **28 px** au lieu de 26 | Bibliothèque | `--gouttiere` |
| Gouttière de **38 px** au lieu de 26 | Éditer (« séparer ») | `38px var(--gouttiere)` |
| Colonne de lecture collée à gauche : marge de droite jusqu'à **656 px** (G2) | Bibliothèque | padding centré (`max(--gouttiere, (100 % − --colonne-lecture)/2)`) |
| Boutons de la carte « Lama a compris » (alors « Ollama a compris ») qui **débordent** du panneau de 7 à 10 px dans une fenêtre de 960 px (G3) | Lama | grille `auto-fit minmax(300px, 1fr)` : la carte et le récit s'empilent |
| Marges intérieures des panneaux : 16 px (rail) contre 18 (inspecteur, file, transport, dock) | rail | `--gouttiere-panneau` |

Après correction : **0 violation sur 180 cellules** (10 écrans — Nuage, Carte,
Temps, Anneau, Lama vide, Lama après playlist, Écoute, Éditer, Découvrir,
Bibliothèque — × 3 états × 2 thèmes × 3 tailles). **Non couvert** : l'établi
d'Éditer avec des stems chargés (il faut une séparation faite), l'esthétique et
les polices — l'audit ne mesure que la géométrie, le débordement et
l'occultation. (Les contrastes : `scripts/contraste-interface.py`, Règle 6,
depuis le 1ᵉʳ novembre 2026.)

### Constat transversal — Règle 6, les deux thèmes (résolu le 1ᵉʳ novembre 2026)

Le verdict traversait les sept lignes : `style.css` dessinait bien `:root`
(sombre) et `:root[data-theme="clair"]` séparément, mais rien dans `ui/js/` ne
posait jamais `data-theme="clair"` hors du mode d'essai — le thème clair était
du code mort pour l'utilisateur. Corrigé, sur les deux moitiés de la règle :

- **Atteignable.** Bibliothèque → Apparence : *Sombre*, *Clair* ou *Système*
  (`prefers-color-scheme`, suivi en direct), mémorisé (`localStorage`,
  `js/theme.js`) ; la palette (⌘K) bascule aussi. Sombre reste le défaut.
  `theme-avant.js`, script classique chargé dans `<head>` (la politique de
  Tauri interdit les scripts en ligne), pose le thème **avant le premier
  affichage** : pas de flash sombre (test de fumée). Les canevas, qui lisent
  leurs couleurs dans les jetons en dessinant, se redessinent sur l'évènement
  `theme` ; un spectrogramme déjà composé garde ses couleurs jusqu'au morceau
  suivant.
- **Contrasté, mesuré.** `scripts/contraste-interface.py`, appelé en tête de
  `scripts/audit-interface.sh`, lit les deux blocs de jetons dans la feuille
  et vérifie chaque texte (`--txt`, `--mut`, `--accent`, `--alerte`) sur chaque
  fond (`--bg`, `--panel`, `--panel2`) à 4,5:1, les libellés de partie du Lama,
  la partition, les glyphes posés sur les boutons (3:1) et les initiales
  d'artiste sur leurs 360 teintes ; il échoue aussi si un jeton du sombre n'est
  pas redessiné dans le clair, ou si une couleur de texte est codée en dur hors
  des jetons. Première exécution : **23 violations**. Corrigé thème par thème —
  `--accent` clair #A85F2C → #9A5426 (4,0:1 → 4,8:1 sur `--panel2`) ; le rouge
  des erreurs, codé en dur (#C0503A, 3,5:1 en sombre, 3,9:1 en clair), devient
  `--alerte`, dessiné pour chaque thème ; `--partie-3` clair #00895D → #007A53 ;
  la feuille de partition et les initiales d'artiste passent par des jetons
  (aplat clair et encre sombre en thème clair, plutôt que l'aplat sombre du
  thème sombre posé sur du papier). Après : **0 violation**.

Pas de palettes multiples par thème (Règle 5).

### Constat transversal — légende des familles, résolu le 13 septembre 2026

Aucune des neuf règles numérotées ne le nommait (la Règle 2 d'Explorer → Carte
jugeait seulement l'isolement lui-même, pas son mode de sélection), mais le
principe de cohérence (Nielsen) tranchait : `rendreFamilles()` sert la **même**
légende aux trois modes (Écouter, Explorer, Découvrir), et jusqu'ici seuls
Écouter et Découvrir permettaient de cocher plusieurs familles à la fois
(`filtreFamilles`/`filtreFamillesDecouvrir`, deux `Set`) — Explorer n'isolait
qu'une seule famille en bascule exclusive (`carte.isolee`, une valeur
unique). Le même bouton, dans le même composant, ne se comportait pas pareil
selon l'écran. Unifié en multi-sélection partout : `carte.isolee` devient
`carte.isolees` (un `Set`, comme les deux autres modes), avec son propre lien
« Toutes les familles » (`#familles-tout`) à côté de la légende d'Explorer,
et les commandes du moteur qui bornent un calcul par famille (`path`,
`path_drawn`, `selection`, `itineraire_voirie`) prennent maintenant
`familles: Option<Vec<i64>>` plutôt qu'un `famille: Option<i64>` unique
(`morceaux_des_familles`, `apps/desktop/src/main.rs`). Le mécanisme
d'estompage d'Explorer (Règle 2, dimming plutôt que masquage) est inchangé —
seul le nombre de familles qu'on peut isoler à la fois a changé.

## Checklist de fin de tâche

À revérifier explicitement, règle par règle, avant de considérer un écran
terminé — dans n'importe lequel des écrans du rail :

1. **Inspecteur** — la sélection peuple-t-elle `#insp` existant, ou ce nouvel
   écran recrée-t-il pochette/métadonnées/bio ailleurs ?
2. **Filtre/recherche** — un élément écarté est-il toujours dans le DOM,
   atténué, ou a-t-il disparu ?
3. **Couleur de famille** — vient-elle de `--familles` (ou de `palette.rs`
   pour les tuiles de carte, seule exception actée), ou un tableau de
   couleurs a-t-il été recopié à la main ?
4. **Sélection → mise en page** — cliquer un élément déplace-t-il, réordonne-
   t-il ou fait-il défiler autre chose que ce qui était prévu ?
5. **Sobriété** — combien de contrôles sont visibles en même temps sans
   geste ? Si la réponse ressemble à l'ancienne page Bibliothèque (dix blocs
   de réglages ouverts d'un coup, avant le 1ᵉʳ novembre 2026), reculer.
6. **Thème** — l'écran a-t-il été vérifié en clair **et** en sombre
   (Bibliothèque → Apparence, ou ⌘K « thème ») ? Une couleur nouvelle est-elle
   un jeton dessiné dans les deux blocs, et `scripts/contraste-interface.py`
   rend-il 0 violation ?
7. **Zoom/pan** — si l'écran en a un, appelle-t-il la même fonction/le même
   geste que la carte et la frise, ou une troisième mécanique ?
8. **Divulgation progressive** — un réglage avancé est-il caché derrière un
   geste, ou affiché d'emblée « au cas où » ?
9. **Force d'une relation** (si l'écran affiche des relations entre
   morceaux) — épaisseur + opacité seulement, jamais un style de trait
   différent par valeur ?
10. **Gabarit** — les dimensions partagées viennent-elles des jetons de `:root`,
    et `scripts/audit-interface.sh` rend-il 0 violation ?
11. **Typographie** — une taille nouvelle est-elle l'un des jetons `--t-*`
    (rien sous 12 px), une graisse 400/500/600 ? Un nom joint dans une chaîne
    passe-t-il par `isoler()` ?
12. **Place** — chaque nouvel élément est-il dans la zone de son rôle (le rail
    pour la vue, le centre pour l'objet et ses outils, l'inspecteur pour ce
    qui est choisi, le bas pour le temps), avec sa ligne dans la table de
    `js/ecrans.js` ? Le rail tient-il encore à 1 400 × 900 (G8 de
    `scripts/audit-interface.sh`) ?

Et le principe de dernier recours : si aucune des treize règles ne tranche,
qu'est-ce que ce mot, ce geste ou cette couleur signifient déjà ailleurs dans
l'application — et l'écran en cours le respecte-t-il ?

## Historique

Les bandeaux qui ouvraient ce document, dans l'ordre où ils avaient été posés
(le plus récent d'abord). Les nombres d'écrans qu'ils citent sont ceux de leur
date.

> **Sept écrans (5 novembre 2026) : Éditer se divise en Pratiquer et Créer**
> (`ui-spec-editeur.md`, décision 13). Le sélecteur porte six boutons :
> Écouter, Pratiquer, Explorer, Créer, Découvrir, Valise. Les deux nouveaux
> écrans partagent l'établi (`#editer-separer`, `#editer-etabli`) et l'état
> `edition` ; `enStems()` (`socle.js`) dit si l'on est dans l'un des deux.
> L'audit (`audit.js`) les capture séparément. Là où ce document dit
> « Éditer », lire « Pratiquer et Créer ».

> **Sixième écran (19 octobre 2026) : Valise**, cinquième bouton du
> sélecteur, sous Découvrir — la synchronisation du compagnon iOS. Panneau
> central `#valise-vue` (colonne de lecture `.bibliotheque`) : la composition
> de la valise (lignes règle + nombre d'albums, limites), puis la liste des
> albums qui partiront, où l'on garde ou écarte un album. Rail `#bloc-valise` :
> l'iPhone (appareil, échéance de l'app, valises qu'il porte, « Renouveler
> l'app »). Là où ce document dit « cinq écrans », lire « les écrans du rail » :
> Valise y est soumis aux mêmes règles (audit ci-dessous).

> **Correction de périmètre au moment d'écrire ce document (7 septembre
> 2026)** : la commande qui a produit ce document supposait Éditer et
> Découvrir sans interface construite. C'est faux pour les deux —
> `apps/desktop/ui/js/` porte ~15 fonctions dédiées à Découvrir (fil
> d'actualité, explorateur de collaborations) et une dizaine à Éditer (dock de
> stems, greffe, spectrogrammes), toutes branchées à un écran du rail. L'audit
> couvre donc les **sept lignes** (cinq écrans, dont les trois sous-modes
> d'Explorer), aucune n'étant écartée comme prématurée.
