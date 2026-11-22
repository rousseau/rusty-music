# Plan : la page d'accueil d'Écouter

> Écrit le 22 nov. 2026, après une discussion sur une présentation « à la
> Netflix / Apple TV » du mode Écouter. Chaque étape (A0 à A4) porte un
> **prompt prêt à coller** dans une nouvelle session. Rien n'est implémenté au
> moment de l'écriture.

## Le principe : faire remonter ce qui dort

Netflix et Apple TV mettent en vitrine un catalogue immense pour retenir
l'abonné. Le problème de Rusty Music est l'inverse : une bibliothèque
possédée, finie (1 986 albums sur la bibliothèque de test), qu'on parcourt
dans une grille de A à Z ou par date d'ajout. Les albums qui ne sont ni en
tête d'alphabet ni récents ne remontent jamais. La page d'accueil sert à
**redécouvrir ce qu'on a déjà**, pas à découvrir ce qu'on n'a pas : ce rôle-là
reste au mode Découvrir.

Elle prend la forme d'**un album à la une** en haut, puis de **rangées
horizontales de pochettes**, chacune titrée par la raison qui la fait
exister.

| règle | pourquoi |
|---|---|
| **Cinq à sept rangées, pas plus.** | Au-delà, on fait défiler sans lire ; c'est la grille A–Z sous un autre nom. |
| **Chaque rangée dit sa raison** dans son titre (« Pas écoutés depuis un an », « Sortis il y a 30 ans cette semaine »). | Une recommandation sans raison ressemble à un tirage ; avec, elle invite. |
| **Stable sur la journée.** Le tirage est fait avec une graine tirée de la date locale ; un bouton « ↻ » retire une seule rangée. | Une page qui change à chaque visite empêche de retrouver ce qu'on avait repéré. |
| **Une rangée vide ne s'affiche pas** ; une rangée de moins de 4 albums non plus. | Pas d'état vide décoratif. |
| **Pas de son au survol, pas de lecture automatique.** | « Jamais de son surprise » (`plan-ecouter-v0.2.md`). |
| **Rien n'exige le réseau.** Les rangées locales suffisent à remplir la page ; l'actualité s'ajoute quand elle est là. | Le lecteur doit marcher hors ligne. |
| **L'unité est l'album**, pas le morceau. | La lecture aléatoire de morceaux existe déjà dans la file. |

## Ce qui existe

| besoin | où |
|---|---|
| Journal d'écoute, favoris | `crates/core/src/memoire.rs` (tables `ecoute(chemin, le)`, `favori`), `dernieres_ecoutes`, `nombre_ecoutes`, listes intelligentes de la vue Playlists |
| Albums, clé d'album, pochettes | `Library::albums`, `cleAlbum` (`js/vues.js`), cache de pochettes et repli réseau (`ui-spec-lecteur.md` § Pochettes) |
| Année de l'œuvre (pas du pressage) | `first_release_date` des release-groups MusicBrainz (`mb_release_groups`), logique de `carte.rs` (~l. 976 et 1044) |
| Familles de genres | `familles.rs`, `artist_families`, `docs/nommage-familles.md` |
| Proximité sonore | empreintes CLAP (`crates/analysis`) |
| Biographie, vignette d'artiste | TheAudioDB (`crates/enrichissement/src/theaudiodb.rs`, `biographies.rs`) |
| Sorties récentes de vos artistes | fil Découvrir (`db/decouvrir.rs::decouvrir_fil`, sorties fraîches de ListenBrainz) |
| Popularité | `docs/popularite.md` (ListenBrainz + Deezer, cumuls) |
| Nommage en langue naturelle | Lama (`crates/lama`) |

## Les rangées

Un album est **écouté** à une date si au moins trois de ses pistes, ou la
moitié, y ont une écoute dans `ecoute` (seuil à confirmer en A0). Sa
**dernière écoute** est la plus récente de ces dates ; son **nombre
d'écoutes**, le nombre de journées distinctes où il a été écouté.

| id | titre affiché | définition | réseau |
|---|---|---|---|
| R1 `oublies` | Pas écoutés depuis longtemps | écoutés au moins 3 journées, dernière écoute il y a plus de 12 mois ; tri par nombre d'écoutes | non |
| R2 `jamais` | Jamais écoutés | aucune écoute ; ajoutés il y a plus de 30 jours (sinon c'est R6) ; tirage pondéré par popularité | non |
| R3 `anniversaire` | Sortis il y a *N* ans cette semaine | `first_release_date` connue au jour près, à ± 3 jours de la date ; *N* rond (10, 20, 25, 30, 40, 50…) d'abord | non |
| R4 `epoque` | *Famille* · années *2000* (une à trois rangées) | croisement famille × décennie (ou demi-décennie si la case est grosse) ; seulement les cases ≥ 8 albums ; tirage du jour parmi elles | non |
| R5 `hasard` | Au hasard | tirage pondéré : poids fort pour jamais écouté ou dernière écoute ancienne, faible pour écouté ce mois-ci | non |
| R6 `nouveaux` | Ajoutés récemment, pas encore écoutés | `added_at` < 30 jours, aucune écoute | non |
| R7 `parce-que` | Parce que vous avez écouté *X* | *X* = album le plus écouté des 14 derniers jours ; voisins CLAP (centroïde d'album), hors même artiste, hors écoutés ce mois-ci | non |
| R8 `actualite` | Nouvel album de *X* · ses anciens chez vous | artistes présents dans les sorties du fil Découvrir ; leurs albums possédés | oui (fil existant) |

R1 et R2 sont la raison d'être de la page. R7 et R8 viennent après.

**L'album à la une** : un seul, choisi dans l'ordre R8 → R3 → R1 → R5
(première rangée non vide). Grande pochette, fond large de l'artiste
(TheAudioDB, voir A3), deux lignes de biographie, bouton Lecture, raison en
surtitre.

**Ordre des rangées** : à la une, puis R1, R3, R4 (×1 à 3), R2, R7, R8, R5,
R6, en ne gardant que les sept premières non vides.

## Contraintes communes à tous les prompts

- Lire avant tout : `CLAUDE.md`, ce document, `docs/ui-spec-lecteur.md`,
  `docs/interface-guidelines.md`, puis les documents cités par l'étape.
- Rust uniquement à l'exécution ; ne rien réécrire qui existe ; toute nouvelle
  dépendance passe par `docs/rust-audio-stack.md` et `cargo deny check`.
- Une commande Tauri qui change de forme → `apps/desktop/ui-tests/fixtures.js`
  (et `ui-tests/types/commandes.ts`). Un changement de schéma → une migration
  (`db::schema::migrations`).
- Interface : `scripts/audit-interface.sh` après toute modification de mise en
  page ; attention à la Règle 13 (le rail tient dans la fenêtre).
- Fin d'étape : `./scripts/verifier.sh` vert, une entrée datée dans
  `docs/journal.md` (ce qui a été fait **et mesuré**), l'état dans
  `docs/suite.md` et dans le tableau d'avancement en bas de ce document.
- Ne pas toucher à `livre/`.

---

## A0 — Mesurer avant de construire

**Objectif.** Savoir quelles rangées seront pleines sur la vraie
bibliothèque. Le journal `ecoute` est récent : si R1 est vide parce que
l'historique a moins d'un an, il faut le savoir avant de dessiner la page
autour d'elle.

**Mesures.** Pour chaque rangée R1 à R7 (R8 dépend du fil) : nombre
d'albums candidats, et pour R3 et R4 la distribution par jour de l'année et
par case famille × décennie. Plus : ancienneté du journal (première écoute),
part des albums dont la date est connue au jour près, part des albums sans
famille, effet du seuil « écouté » (1, 3 pistes, moitié).

**Décision** (dans le journal, avec les chiffres) : seuils retenus ; si
l'historique est trop court pour R1, R1 utilise un seuil de 3 mois en
attendant, et le titre le dit (« Pas écoutés depuis trois mois »).

### Prompt A0

```text
Lis CLAUDE.md, docs/plan-ecouter-accueil.md (en entier, puis la section A0) et
crates/core/src/memoire.rs.

Écris un exemple de mesure `crates/core/examples/accueil_mesure.rs` qui lit la base de
ma bibliothèque et calcule, pour les rangées R1 à R7 telles que définies dans
plan-ecouter-accueil.md, le nombre d'albums candidats, ainsi que les mesures listées en A0
(ancienneté du journal `ecoute`, dates connues au jour près, cases famille × décennie
≥ 8 albums, albums sans famille, sensibilité au seuil « album écouté »). Réutilise les
requêtes existantes (memoire.rs, la logique d'année de carte.rs, les familles) ; ne les
duplique pas : si une partie n'est accessible que dans apps/desktop, déplace-la dans
crates/core dans un commit séparé, sans changer son comportement.

Sortie : un `accueil-mesure.md` (tableaux). Lance-le sur ma bibliothèque, reporte les
chiffres et les seuils retenus dans docs/journal.md et dans le tableau d'avancement de
docs/plan-ecouter-accueil.md. Pas d'interface dans cette étape.
```

---

## A1 — Le moteur des rangées

**Objectif.** Un module `crates/core/src/accueil.rs` qui rend la page
entière en une requête : l'album à la une et les rangées, chacune avec son
id, son titre, sa raison et ses albums (clé d'album, artiste, titre, année).
Rangées locales seulement (R1 à R7) ; R8 est ajoutée en A4.

**Points de conception.**
- Graine = date locale (AAAA-MM-JJ) + id de rangée ; le « ↻ » d'une rangée
  passe un décalage de graine, sans toucher aux autres.
- Un album n'apparaît que dans une rangée par page (priorité dans l'ordre
  des rangées), et jamais l'album à la une dans une rangée.
- R7 : centroïde CLAP par album, calculé à la demande et mis en cache en
  mémoire ; rien de neuf en base si `crates/analysis` le donne déjà.
- R4 : titre par défaut `Famille · années 2000` ; un nom Lama est un bonus
  optionnel, jamais bloquant.
- Commande Tauri `accueil { decalages }` → `PageAccueil` ; elle entre dans
  `types_commandes.rs`.

**Tests.** Une base de test avec un journal d'écoute fabriqué : chaque
rangée a son cas plein, son cas vide, le dédoublonnage entre rangées, la
stabilité sur une même date et le changement d'un jour à l'autre.

### Prompt A1

```text
Lis CLAUDE.md, docs/plan-ecouter-accueil.md (sections « Les rangées » et A1) et l'entrée
A0 de docs/journal.md (seuils retenus).

Crée crates/core/src/accueil.rs : `Library::page_accueil(date_locale, decalages)` qui rend
l'album à la une et les rangées R1 à R7 (id, titre, raison, albums), avec les règles du
document : graine par jour et par rangée, un album dans une seule rangée, rangées de moins
de 4 albums omises, sept rangées au plus. Réutilise memoire.rs, la logique d'année de
carte.rs, les familles et les empreintes CLAP existantes. Ajoute la commande Tauri
`accueil` (types_commandes.rs, fixtures.js, ui-tests/types/commandes.ts) et une
sous-commande CLI `rusty-music accueil` qui imprime la page du jour en texte, pour
vérifier sans interface. Tests unitaires sur une base fabriquée (cas plein, vide,
dédoublonnage, stabilité dans la journée). Pas d'interface dans cette étape.
```

---

## A2 — La vue Accueil

**Objectif.** Une quatrième vue dans « Parcourir » du rail d'Écouter :
**Accueil · Artistes · Albums · Playlists**, et Accueil devient la vue par
défaut à l'ouverture (la dernière vue choisie reste retenue ensuite).

**Mise en page.**
- En tête, l'album à la une : pochette ~320 px, surtitre (la raison), titre,
  artiste, année, bouton Lecture et « Ouvrir l'album ». Le fond large arrive
  en A3 ; en attendant, un dégradé tiré des couleurs de la pochette
  (`js/couleurs-album.js`).
- Puis les rangées : titre à gauche, « ↻ » à droite, pochettes ~140 px
  (même taille que la grille Albums), défilement horizontal au trackpad et
  aux flèches, chargement paresseux des pochettes comme dans la grille.
- Un clic ouvre l'album dans la vue existante, avec retour à l'Accueil
  (`snapVue`). Lecture directe au survol par un bouton ▶ sur la pochette,
  sans son tant qu'on ne clique pas.
- Le filtre par famille du rail (`bloc-familles-ecoute`) n'a pas de sens ici :
  le masquer dans cette vue (Règle 12).

**Vérification.** Audit d'interface sur la nouvelle vue (rapide pendant le
travail, complet à la fin), G8 vert, un test de fumée qui charge la vue
avec une `PageAccueil` de fixture.

### Prompt A2

```text
Lis CLAUDE.md, docs/plan-ecouter-accueil.md (section A2), docs/interface-guidelines.md et
docs/ui-spec-lecteur.md.

Ajoute la vue « Accueil » au mode Écouter : segment en tête de « Parcourir » dans le rail,
vue par défaut à l'ouverture, rendue à partir de la commande `accueil` (A1). Un nouveau
module js/accueil.js ; réutilise le chargement paresseux et le cache des pochettes de la
grille Albums, cleAlbum, snapVue pour le retour, couleurs-album.js pour le fond provisoire
de l'album à la une. Masque le filtre par famille dans cette vue. Ajoute la vue à la table
des écrans (js/ecrans.js), un test de fumée dans apps/desktop/ui-tests/ avec une page de
fixture, puis lance scripts/audit-interface.sh (rapide pendant le travail, complet à la
fin) et corrige ce qu'il signale. Envoie-moi une capture de la vue sur ma bibliothèque.
```

---

## A3 — Le fond « à la une »

**Objectif.** L'effet Apple TV : derrière l'album à la une, une image large
de l'artiste. TheAudioDB en fournit dans sa réponse `artist-mb.php`, déjà
interrogée pour la biographie et la vignette (champs de fanart et de
vignette large : **noms exacts à vérifier sur une réponse réelle**, et la
licence d'usage des images à relire dans `docs/data-sources.md`).

**Points de conception.** Même chemin que la vignette (`vignette_par_mbid`) :
une URL lue dans la réponse déjà reçue, un téléchargement à la demande, un
cache disque par artiste, un « pas d'image » qui périme après 30 jours, une
panne jamais mémorisée. Repli : le dégradé de A2. L'image est assombrie sous
le texte pour garder le contraste exigé par les guidelines.

### Prompt A3

```text
Lis CLAUDE.md, docs/plan-ecouter-accueil.md (section A3), docs/data-sources.md et
crates/enrichissement/src/theaudiodb.rs.

Vérifie sur une réponse réelle d'artist-mb.php les champs d'image large (fanart, vignette
large) et leur licence d'usage ; consigne-le dans docs/data-sources.md. Ajoute, sur le
modèle de `vignette_par_mbid` et de son cache, la récupération de l'image large d'un
artiste, puis son affichage derrière l'album à la une de la vue Accueil (assombrie sous le
texte, repli sur le dégradé existant). Mets à jour l'écran « À propos » si une source ou une
mention change. Audit d'interface sur la vue, puis une capture.
```

---

## A4 — L'actualité

**Objectif.** La rangée R8 et le premier choix de l'album à la une : quand
un artiste de la bibliothèque sort un album, proposer ses anciens qu'on
possède (« Nouvel album de X · réécoutez *Y* (2004) »).

**Ce qui existe.** Le fil Découvrir sait déjà repérer les sorties récentes
de vos artistes (ListenBrainz, `decouvrir_fil`). R8 se contente de le lire :
aucune requête nouvelle, et si la passe Découvrir n'a jamais tourné, R8 est
absente sans bruit.

**Plus tard, à étudier seulement.** Les classements de la semaine
(statistiques globales ListenBrainz, classement Deezer) croisés avec la
bibliothèque : « Ce qu'on écoute en ce moment, chez vous ». À n'engager que
si une source sans clé se confirme (`docs/popularite.md` écarte les clés).
Les vraies actualités (décès, tournées) n'ont pas de source ouverte propre :
hors périmètre.

### Prompt A4

```text
Lis CLAUDE.md, docs/plan-ecouter-accueil.md (section A4) et crates/core/src/db/decouvrir.rs.

Ajoute la rangée R8 `actualite` à page_accueil : pour chaque artiste présent dans les
sorties récentes du fil Découvrir (fenêtre de 30 jours), ses albums possédés, la sortie
nommée dans le titre de la rangée. Elle passe en tête du choix de l'album à la une. Aucune
requête réseau nouvelle : R8 lit ce que la passe Découvrir a déjà rangé, et disparaît sans
message si elle n'a pas tourné. Tests sur une base fabriquée, mise à jour de la fixture
`accueil`, capture de la vue.
```

---

## Avancement

| étape | état | date | note |
|---|---|---|---|
| A0 mesurer | fait | 22 nov. 2026 | `crates/core/examples/accueil_mesure.rs` ; journal de 49 j → R1 vide (0, même à 3 mois) : R1 à ≥ 1 journée et > 1 mois (22 albums) tant que le journal a moins de 6 mois ; écouté = 3 pistes ou la moitié (56 albums) ; R2 2 099, R3 25 (351 j/365 ≥ 4), R4 42 cases ≥ 8, R6 7, R7 2 129 ; 2 albums sans famille, 60,6 % datés au jour près (`docs/journal.md`) |
| A1 moteur | à faire | | |
| A2 vue | à faire | | |
| A3 fond à la une | à faire | | |
| A4 actualité | à faire | | |
