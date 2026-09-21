# Contrat d'interface — transversal aux cinq écrans

> Document de cohérence, pas de design. Les règles ci-dessous ont presque
> toutes été décidées ailleurs — `ui-spec.md` (Explorer), `ui-spec-lecteur.md`
> (Écouter), `ui-spec-editeur.md` (Éditer), `carto-anneau.md` (Anneau),
> `frise-filiations.md` (Temps) — puis oubliées dès qu'on quittait le module
> où elles avaient été écrites. Ce document les rassemble en un seul contrat,
> **applicable aux cinq écrans du rail** — les **quatre modes** (Écouter,
> Explorer, Éditer, Découvrir) plus **Bibliothèque**, point d'entrée posé
> au-dessus du sélecteur de mode plutôt que dedans — et vérifie ce qui existe
> déjà contre ce contrat.

> **Correction de périmètre au moment d'écrire ce document (7 septembre
> 2026)** : la commande qui a produit ce document supposait Éditer et
> Découvrir sans interface construite. C'est faux pour les deux —
> `apps/desktop/ui/app.js` porte ~15 fonctions dédiées à Découvrir (fil
> d'actualité, explorateur de collaborations) et une dizaine à Éditer (dock de
> stems, greffe, spectrogrammes), toutes branchées à un écran du rail. L'audit
> couvre donc les **sept lignes** (cinq écrans, dont les trois sous-modes
> d'Explorer), aucune n'étant écartée comme prématurée.

## Le socle commun — ce qu'aucun écran ne redéfinit

Le modèle retenu (`ui-spec.md`, « Atelier ») n'appartient pas à Explorer. Il
tient en quatre zones, présentes et identiques dans les cinq écrans :

- **Rail gauche fixe** — identité, puis **Bibliothèque** en entrée de rail
  isolée (point de départ, pas un mode), puis le sélecteur de mode à quatre
  boutons (`Écouter / Explorer / Éditer / Découvrir`), puis des blocs propres
  à l'écran courant, montrés ou masqués par `basculerMode()`
  (`apps/desktop/ui/app.js:5831`).
- **Zone centrale propre à chaque mode** — pas le même composant partout : le
  nuage/la carte/l'anneau/la frise pour Explorer, la grille d'albums ou la
  liste d'artistes pour Écouter et pour Éditer (on y choisit quoi ouvrir), le
  tableau de bord de la bibliothèque pour Bibliothèque, le fil d'actualité
  pour Découvrir. C'est la formulation exacte à retenir — pas « la carte
  toujours visible » de la première version d'`ui-spec.md`, qui datait d'avant
  l'ajout de Bibliothèque et Découvrir au rail et que le code n'a jamais
  suivie à la lettre (voir audit Éditer, § écart documentaire).
- **Inspecteur droite, unique** — `aside.inspecteur` (`index.html:604`),
  jamais recréé ni dupliqué par un mode. Détail en Règle 1.
- **Transport pleine largeur en bas** — `footer.transport`
  (`index.html:698`), en dehors de tout conteneur conditionné par le mode ;
  aucun code ne le masque (vérifié : aucune occurrence de `.transport` suivie
  de `.hidden` dans `app.js`). Persiste tel quel dans les cinq écrans, y
  compris Bibliothèque et Découvrir où on ne s'y attend pas forcément.
- **Dock optionnel** — `#dock` (`index.html:661`), réservé à Éditer, pousse le
  centre vers le haut sans reproduire la mécanique du rail ou de l'inspecteur.
- **Champ d'intention, partagé** (ajouté le 14 septembre 2026) —
  `#bloc-intention`, une ligne en tête de `<main class="centre">`, entre
  `.fil` et `.centre__corps` — donc hors de tout conteneur que
  `basculerMode()` bascule par ailleurs, sur le même principe que
  l'inspecteur unique (Règle 1) : un seul champ, jamais dupliqué par mode,
  dont `basculerMode()` ajuste seulement la visibilité et le placeholder
  (comme il le fait déjà pour `#fil-titre`). Interroge un LLM local (Ollama)
  pour traduire un prompt en texte libre en action dans le mode courant —
  aujourd'hui seulement en Explorer (« texte → playlist »,
  `docs/ui-spec.md`, § « Tranché le 14 septembre »), révélé ailleurs le jour
  où un autre mode y gagne un comportement. Ni généraliste ni permanent
  partout : révélé seulement là où un comportement est branché (Règle 8).

## Règles universelles — dans les cinq écrans, sans exception

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
geste ▶. Chantier de code ouvert : `bio_piste` résout aujourd'hui la bio
par MBID de piste (`app.js:1993-2010`), pas par MBID d'artiste indépendant
d'une piste sélectionnée — il faut une route côté moteur qui prenne
directement l'artiste ouvert au centre.

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

**Étendu à Écouter et à Éditer le 21 septembre 2026.** Dans une liste de
morceaux (résultats de recherche, pistes d'un album), le clic sur la ligne
**sélectionne** (`selectionner`, `pisteSelectionnee` dans `app.js`) : filet
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
  (`estAffiche` dans `app.js`).
- **Éditer** prend la sélection pour source (`morceauAEditer`), sans passer par la
  file de lecture : sélectionner puis entrer en Éditer ouvre directement
  l'état « séparer », sans lecture préalable.

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

### 4. Stabilité des positions et de la mise en page
Sélectionner un élément ne réorganise jamais ce qui l'entoure. Une liste ne
saute pas au clic, l'anneau et la frise ne redistribuent pas leurs positions
selon le focal.

### 5. Sobriété stricte
AudioMuse-AI comme référence négative explicite. Densité maîtrisée,
hiérarchie typographique forte, pas d'empilement de contrôles visibles en
permanence — dans les cinq écrans, pas seulement Explorer où la règle a été
écrite.

### 6. Les deux thèmes traités sérieusement
Sombre et clair sont **dessinés** séparément — pas l'un dérivé
mécaniquement de l'autre — dans les deux sens : le contrat porte sur la
conception **et** sur l'accessibilité réelle du second thème depuis
l'interface.

### 7. Zoom et pan cohérents
Mêmes gestes, mêmes contrôles, partout où zoom et pan existent — y compris un
futur zoom temporel d'une forme d'onde dans Éditer, pas seulement le zoom
géographique de la Carte.

### 8. Révélation par échelle et par contexte
Jamais tout affiché en permanence. Vaut pour les étiquettes de la carte comme
pour les options avancées d'un mode quel qu'il soit.

## Règle scopée — modes qui affichent des relations entre morceaux

### 9. Un seul langage graphique pour la force d'une relation
Épaisseur et opacité du trait, jamais des styles de trait différents
(plein/tiret/pointillé) pour coder une variable continue. S'applique
aujourd'hui à Explorer → Anneau et Explorer → Temps. Ne s'écarte pas par
défaut pour Éditer : si la génération s'appuyant sur toute la bibliothèque en
vient à afficher ses propres sources un jour, la même règle s'applique.

## Principes généraux

Les huit règles ne couvrent pas tous les cas. Deux principes tranchent le
reste :

- **Cohérence et standards (Nielsen).** L'utilisateur ne doit jamais se
  demander si des mots, des situations ou des actions différentes signifient
  la même chose. Un bouton « régler » qui ouvre un tiroir dans Éditer et un
  bouton « régler » qui bascule un état ailleurs sont la même faute, même si
  aucune des huit règles ne la nomme explicitement. Face à un cas nouveau : la
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

Sept lignes : Écouter, les trois sous-modes d'Explorer, Bibliothèque, Éditer,
Découvrir. Verdicts : ✅ conforme · ❌ non conforme (raison précise) · —
non applicable · 🔧 tranché en doc, chantier de code ouvert.

### Écouter

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | 🔧 | `inspecter()` (`app.js:1017`) peuple `#insp` pour pochette/métadonnées/crédits/critiques — toujours conforme. Bio d'artiste : exception actée (voir Règle 1) pour la déplacer au centre quand un artiste est ouvert, mais pas encore codée — `bloc-bio` vit encore dans `#insp` (`index.html:797-807`), peuplé par `bio_piste` (résolution par piste, pas par artiste ouvert au centre). |
| 2 | Estomper, jamais masquer | ❌ | La recherche du rail (`#q`), hors mode Explorer, remplace la liste affichée par une liste de résultats plate (`invoke("search")` puis `poser("recherche", …)`) au lieu d'atténuer les albums/artistes qui ne correspondent pas dans la grille en cours. La vue `recherche` sépare en revanche ses gestes (`ligneRecherche` : clic sur la ligne → sélection silencieuse, numéro ou double-clic → lecture, album/artiste → navigation). Les pistes d'un album (`lignePiste`) suivent le même geste depuis le 21 septembre (clic = sélection, ▶ / double-clic = lecture, voir Règle 1) ; le nom d'artiste ne colore que lui et mène à ses albums (`lienLigne` + `ouvrirAlbumsArtiste`) — cohérent avec la recherche et l'inspecteur. Signalement par la couleur seule, jamais de soulignement. |
| 3 | Palette unique | ✅ | Aucune couleur de famille codée en dur dans `app.js` ; tout passe par `--familles`. |
| 4 | Stabilité des positions | ✅ | `inspecter()` ne touche ni `grille.scrollTop` ni `liste.scrollTop` ; le scroll n'est réinitialisé que par une vraie navigation (`poser(..., scroll=0)`, `app.js:814`), jamais par une sélection. |
| 5 | Sobriété stricte | ✅ | Les blocs du rail propres à d'autres modes restent masqués (`bloc-familles-ecoute`, etc., gérés par `basculerMode`) ; pas d'empilement visible. |
| 6 | Deux thèmes sérieux | ❌ | Voir constat transversal ci-dessous — s'applique à l'identique ici, Écouter n'a pas de bascule propre non plus. |
| 7 | Zoom/pan cohérents | — | Pas de zoom en Écouter (listes/grille classiques). |
| 8 | Révélation par échelle | ✅ | Bio/crédits/critiques restent des blocs `hidden` tant qu'ils n'ont pas de contenu (`bloc-bio`, `bloc-credits`, `bloc-critiques`, `index.html:627-649`), pas affichés vides en permanence. |
| 9 | Langage de force unique | — | Aucune relation entre morceaux affichée en Écouter. |

### Explorer → Carte

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Même `inspecter()` que partout ailleurs. **Corrigé** : le clic sur un point jouait le morceau directement (`cnv` → `click`), contrairement au clic sur l'Anneau/la Frise qui ne fait que sélectionner — même geste, deux effets différents. Le clic est maintenant silencieux ici aussi (sélectionne, peuple l'inspecteur, devient le départ de chemin), et le bouton ▶ de l'inspecteur (`insp-lecture`) reste en permanence visible sur la pochette, plus seulement au survol — c'est lui qui lance l'écoute, dans les quatre visualisations. |
| 2 | Estomper, jamais masquer | ✅ | Trois mécanismes distincts et tous conformes : isolement de famille (`app.js:3542-3563`, `globalAlpha` réduit), filtre texte (`app.js:1493-1503`, commentaire explicite « les morceaux qui ne correspondent pas s'estompent »), intervalle d'années (`app.js:3591-3599`, voile semi-opaque plutôt que masquage). |
| 3 | Palette unique | ✅ | Voir Règle 3 : `palette.rs` recalibre par thème de fond, exception documentée et actée dans `CLAUDE.md`. |
| 4 | Stabilité des positions | ✅ | La position d'un point ne dépend que de la projection t-SNE / des tuiles, jamais de la sélection courante. |
| 5 | Sobriété stricte | ✅ | Réglages avancés (perplexité, époques, k-means) déportés dans le mode Bibliothèque plutôt que dans le rail d'Explorer — conforme à la contrainte de `ui-spec.md` (« les réglages avancés restent dans un tiroir rétractable, pas dans le rail »), même si le rangement choisi (un autre mode entier plutôt qu'un tiroir) est plus lourd qu'un tiroir — voir Bibliothèque, Règle 8. |
| 6 | Deux thèmes sérieux | ❌ | Constat transversal ci-dessous. |
| 7 | Zoom/pan cohérents | ✅ | `zoomer()` (`app.js:4660-4698`) est la fonction unique appelée par la molette (`app.js:4707-4723`) et par les boutons `+`/`−`/réinitialiser du rail (`#bloc-zoom`, `index.html:135-139`) ; même geste, même lecture `zoom-val`, quel que soit le sous-mode. |
| 8 | Révélation par échelle | ✅ | Étiquettes de tuiles dépendantes du zoom (MapLibre), légende cliquable plutôt qu'affichage permanent de tous les noms de famille. |
| 9 | Langage de force unique | — | Le nuage/la carte n'affiche pas de relations entre morceaux (le chemin tracé est un itinéraire, pas une variable continue à encoder). |

### Explorer → Anneau

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | `inspecterAlbum()` (`app.js:1058-1094`) réutilise explicitement le même composant : commentaire « le même composant que pour un morceau (`inspecter`), généralisé plutôt que dupliqué » (`app.js:1051-1053`). Cas exemplaire de la règle bien appliquée. Manquait un geste pour être *utile*, pas seulement unique : cliquer un album de l'anneau le rend focal (`chargerAnneau`) sans le jouer, et le panneau n'offrait aucun moyen de l'écouter — corrigé par un bouton ▶ (`insp-lecture`) posé sur la pochette, symétrique du ✦ existant, qui appelle `lireAlbum()` pour un album ou joue directement le morceau affiché sinon (même composant, même geste, dans les deux cas). |
| 2 | Estomper, jamais masquer | ✅ | `app.js:3691-3717` : fond permanent de liens en trace ténue (`globalAlpha` 0,15) et sélection qui fait ressortir sans jamais faire disparaître le reste — corrige explicitement le premier défaut relevé dans `carto-anneau.md` (« seuls les voisins du focal sont visibles »). |
| 3 | Palette unique | ✅ | Même lecture de `--familles`. |
| 4 | Stabilité des positions | ✅ | Décidé et implémenté conformément à `carto-anneau.md` (« la position d'un album sur l'anneau ne dépend jamais du focal sélectionné »). |
| 5 | Sobriété stricte | ✅ | Pas de contrôle superflu ; `k`/`beta` du bundling sont deux réglettes, pas un panneau. |
| 6 | Deux thèmes sérieux | ❌ | Constat transversal. |
| 7 | Zoom/pan cohérents | ✅ | Corrigé : l'exception (« un cercle de rayon fixe n'a rien à agrandir ») ne tenait plus dès lors que l'anneau grandit à l'usage — familles denses, noms serrés — sans que rien n'empêchait de l'agrandir à l'écran. `zoomerAnneau()` reprend le principe du repli sans MapLibre du nuage (un seul facteur `anneau.vue.k`, `dx`/`dy` pour glisser), branché sur `zoomer()` comme les autres sous-modes ; `#bloc-zoom` n'est plus masqué en Anneau. |
| 8 | Révélation par échelle | ✅ | Noms au survol, pas en permanence sur chaque segment — conforme à `carto-anneau.md`. |
| 9 | Langage de force unique | ✅ | `setLineDash` n'est jamais utilisé pour coder la force d'un lien (seulement pour le lasso, le tracé en cours, la colonne « incertain ») ; force = largeur + opacité par sélection (`app.js:3626-3629`), exactement la correction actée dans `carto-anneau.md` (« un seul style de trait, force = largeur + opacité »). |

### Explorer → Temps (frise des filiations)

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Même mécanisme que l'Anneau (le clic sur un album de la frise appelle la même voie d'inspection) — hérite donc aussi du bouton ▶ ajouté là-bas (`insp-lecture`), même geste, même composant. |
| 2 | Estomper, jamais masquer | ✅ | Colonne « dates incertaines » en trait pointillé atténué plutôt que morceaux exclus (`app.js:3601-3618`) ; intervalle d'années : voile, pas masquage (même mécanisme que Carte). |
| 3 | Palette unique | ✅ | Bandes colorées par la même famille/`--familles`. |
| 4 | Stabilité des positions | ✅ | Par construction : `frise-filiations.md` § 1 retient les bandes par famille comme mode par défaut précisément parce qu'« une bande est une structure stable [qui] accueille les nouveaux albums sans redistribuer l'existant » — writen avant l'implémentation, respecté dedans. |
| 5 | Sobriété stricte | ✅ | Un seuil de connexions (réglette), rien de plus dans le rail. |
| 6 | Deux thèmes sérieux | ❌ | Constat transversal. |
| 7 | Zoom/pan cohérents | ✅ | `zoomerTemps()` (`app.js:4646-4658`) est décrit dans son propre commentaire comme suivant « exactement » le même principe que le zoom du nuage — un seul facteur `k`, même geste molette, même lecture `zoom-val`. |
| 8 | Révélation par échelle | ✅ | Année affichée au survol seulement (`frise-filiations.md` § 5), pas d'empilement de labels sur l'axe. |
| 9 | Langage de force unique | ✅ | Même mécanique que l'Anneau : fond permanent en trace ténue, force encodée par largeur/opacité à la sélection — aucun `setLineDash` sur les arcs eux-mêmes. |

### Bibliothèque

> Rappel de vocabulaire : « Bibliothèque » désigne ici l'écran atteint par
> l'entrée de rail du même nom (réglages, statistiques, vérifications) — pas
> un mode au sens du sélecteur à quatre boutons, et pas le fait de parcourir
> ses artistes/albums, qui se passe en mode **Écouter**. La Règle 2 sur « une
> recherche dans Bibliothèque » de la commande d'origine vise en réalité la
> recherche du rail, auditée sous Écouter.

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | — | Aucune sélection de morceau dans ce mode : les listes de vérification (genres suspects, éditions multiples, doublons, points isolés) n'ont pas de gestionnaire de clic vers l'inspecteur (`chargerVerifications`, `app.js:6889-6929` — chaque ligne n'a qu'un texte et parfois un bouton d'action, jamais un `onClick` de sélection). |
| 2 | Estomper, jamais masquer | — | Rien à filtrer dans ce mode dans son état actuel. |
| 3 | Palette unique | — | Pas de rendu coloré par famille ici. |
| 4 | Stabilité des positions | ✅ | Page de réglages classique, rien qui se réorganise au clic. |
| 5 | Sobriété stricte | ❌ | `index.html:426-548` empile, toujours visibles simultanément dès l'entrée dans le mode : Paramètres de la carte (perplexité, époques, familles, itérations), Vocabulaire des familles, Nappe de densité (résolution, noyau, bandes, force d'ombre), et quatre blocs de Vérifications (genres suspects, éditions multiples, doublons/points isolés, échecs de scan) — aucun de ces blocs n'est masqué par défaut ni replié derrière un geste. C'est la définition même du « panneau d'administration » qu'`ui-spec.md` cite comme repoussoir. |
| 6 | Deux thèmes sérieux | ❌ | Constat transversal. |
| 7 | Zoom/pan cohérents | — | Pas de zoom dans ce mode. |
| 8 | Révélation par échelle | ❌ | Même constat qu'à la Règle 5 : ce sont exactement les « options avancées » que la Règle 8 vise nommément, et elles sont toutes à l'écran en permanence plutôt que rétractables. Contraste net avec le panneau par stem de l'Éditeur (Règle 8 là-bas), qui applique la même idée correctement. |
| 9 | Langage de force unique | — | Pas de relations entre morceaux affichées. |

### Éditer

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | Le morceau source passe par le même inspecteur ; **le détail d'un stem sélectionné y va aussi** (`#bloc-stem`, `majInspecteurStem`), plus de panneau déplié sous la ligne dans la pile. |
| 2 | Estomper, jamais masquer | — | Pas de filtre/recherche dans la pile ni la barre d'outils ; les candidats de greffe (`zoneGreffe`) sont une liste de résultats, pas un filtre sur un ensemble déjà visible. |
| 3 | Palette unique | — | La pile de stems ne colore rien par famille (le mini-nuage de greffe, quand il existera, devra lire `--familles`). |
| 4 | Stabilité des positions | ✅ | Sélectionner un stem (`edition.stemSel`) souligne sa ligne et peuple l'inspecteur, sans déplacer les lignes de la pile ni les réordonner. |
| 5 | Sobriété stricte | ✅ | Barre d'outils : deux pas-à-pas d'ensemble (vitesse, hauteur), la dérive quand elle existe, Exporter. La pile : nom, solo, muet, badge, niveau, spectrogramme — le reste attend la sélection, dans l'inspecteur. |
| 6 | Deux thèmes sérieux | ❌ | Constat transversal. |
| 7 | Zoom/pan cohérents | — | Pas encore de zoom temporel de la pile. Le contrat de la Règle 7 s'appliquera dès qu'il existera : mêmes gestes que `zoomer()`/`zoomerTemps()`, et le playhead unique (`positionnerPlayhead`) devra suivre le même facteur. |
| 8 | Révélation par échelle | ✅ | Le détail vitesse/hauteur/greffe d'un stem n'apparaît qu'à la sélection de sa ligne (`majInspecteurStem`), dans l'inspecteur commun — divulgation progressive que Bibliothèque n'applique pas à ses propres réglages avancés. |
| 9 | Langage de force unique | — | Pas de relations entre morceaux affichées ; la liste de voisins pour la greffe (`zoneGreffe`) est une liste classée. À surveiller si le mini-nuage de greffe (prévu, `ui-spec-editeur.md`) affiche un jour des liens. |
| — | Écart, hors des neuf règles | ✅ | **Résolu le 10 septembre 2026 — doc puis code.** `ui-spec-editeur.md` a retranché « le dock pousse la carte vers le haut » ; le centre d'Éditer est **l'établi à trois états** (`majEtatEditer` : `#grille`/`#liste` → `#editer-separer` → `#editer-etabli`), `#bloc-demix` est descendu du rail au centre, le détail d'un stem peuple `#bloc-stem` dans l'inspecteur, et `#dock` est une barre d'outils pleine largeur (plus de `max-height: 34vh`). Reste une passe de vérification visuelle dans l'app en fonctionnement (alignement du playhead au pixel, largeur de la barre sur fenêtre étroite). |

### Découvrir

| # | Règle | Verdict | Raison |
|---|---|---|---|
| 1 | Inspecteur unique | ✅ | `#decouvrir-centre` (nom d'artiste, fil de collaborations) n'est pas un inspecteur de morceau redondant : c'est le contenu central propre à ce mode, au même titre que la grille d'albums l'est pour Écouter. Aucune métadonnée de morceau (pochette, tempo, durée…) n'y est dupliquée. |
| 2 | Estomper, jamais masquer | ✅ | **Résolu le 20 septembre 2026** avec le passage du fil en grille de pochettes : `rendreFilDecouvrir()` construit toutes les cartes, et celles hors du filtre par famille (`sortiePasseFamille` / `voisinPasseFamille`) reçoivent `.album--estompe` (opacité .25, pleine au survol) au lieu d'être retirées. La grille ne se recompose donc pas ; seuls les compteurs d'onglets suivent le filtre. |
| 3 | Palette unique | ✅ | Légende de familles de Découvrir tirée de la même passe que celle de la carte (`chargerFamilles`, `app.js:6266`). |
| 4 | Stabilité des positions | ✅ | Naviguer entre artistes pousse sur un fil d'Ariane (`decouvrirFil`) sans réordonner la liste de collaborations déjà affichée ; le filtre par famille estompe sans recomposer la grille. |
| 5 | Sobriété stricte | ✅ | Trois onglets (Sorties/Collaborations/À écouter ailleurs), un fil, une légende de familles repliable — pas d'empilement. |
| 6 | Deux thèmes sérieux | ❌ | Constat transversal. |
| 7 | Zoom/pan cohérents | — | Pas de zoom dans ce mode. |
| 8 | Révélation par échelle | ✅ | Le bloc Familles ne s'affiche que quand il a un sens (`majBlocFamillesDecouvrir`, carte déjà calculée) plutôt qu'en permanence. |
| 9 | Langage de force unique | — | Pas de relations graphiques dans ce mode (des listes, pas un anneau ou une frise). |

### Constat transversal — Règle 6, les deux thèmes

Le même verdict traverse les sept lignes, il est donc écrit une seule fois
plutôt que répété : la conception est sérieuse des deux côtés — `style.css`
dessine `:root` (sombre) et `:root[data-theme="clair"]` séparément, avec des
rampes de couleur recalibrées indépendamment (contraste vérifié, paliers
OKLCH propres à chaque fond, commentaire explicite : « Les deux thèmes sont
dessinés, pas dérivés l'un de l'autre »). **Mais rien dans `app.js` ne pose
jamais `data-theme="clair"`** : aucune occurrence de
`document.documentElement.dataset.theme`, d'un `setAttribute("data-theme", …)`
visant la racine, ni d'une détection `matchMedia("(prefers-color-scheme)")`.
Le seul usage de `data-theme` dans `app.js` cible `#carte-theme
[data-theme]` (`app.js:5316-5321`) — les cinq **fonds de plan de la carte**
(`osm-clair`/`sepia`/`encre`/`nuit`/`bleu-plan`), un concept différent et sans
rapport avec le thème sombre/clair de l'interface (voir Règle 3). Résultat :
le thème clair de `style.css` est un code mort du point de vue de
l'utilisateur — personne ne peut l'atteindre depuis l'application. Non
conforme, sur la moitié « accessible » de la règle plutôt que sur la moitié
« dessinée ».

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
terminé — dans n'importe lequel des cinq écrans :

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
   geste ? Si la réponse ressemble à la page Bibliothèque actuelle
   (dix blocs de réglages ouverts d'un coup), reculer.
6. **Thème** — l'écran a-t-il été vérifié en clair **et** en sombre ? (Note à
   ce jour : le thème clair n'est atteignable par aucun geste dans
   l'application — le vérifier veut dire forcer `data-theme="clair"` à la
   main tant que ce manque n'est pas comblé.)
7. **Zoom/pan** — si l'écran en a un, appelle-t-il la même fonction/le même
   geste que la carte et la frise, ou une troisième mécanique ?
8. **Divulgation progressive** — un réglage avancé est-il caché derrière un
   geste, ou affiché d'emblée « au cas où » ?
9. **Force d'une relation** (si l'écran affiche des relations entre
   morceaux) — épaisseur + opacité seulement, jamais un style de trait
   différent par valeur ?

Et le principe de dernier recours : si aucune des neuf règles ne tranche,
qu'est-ce que ce mot, ce geste ou cette couleur signifient déjà ailleurs dans
l'application — et l'écran en cours le respecte-t-il ?
