# Frise des filiations — spécification de départ

Remplace le streamgraph du mode Explorer → Temps, jugé insatisfaisant (voir
`docs/carto-anneau.md`, qui penchait vers un alluvial). La frise reprend
l'idée d'un axe chronologique par album, mais résout d'abord quatre
ambiguïtés de conception avant toute implémentation, plus deux améliorations
d'affichage mineures.

## Ce qui existe déjà et se réutilise tel quel

Point central : la frise **ne calcule aucune similarité neuve**. Le réseau
d'albums du mode Explorer → Anneau (`docs/carto-anneau.md`) est déjà le
réseau dont elle a besoin — seule la position à l'écran change (chronologique
+ bande, au lieu de radial).

- **Réseau d'albums** — `apps/desktop/src/main.rs::reseau_albums` (ligne 2371)
  construit déjà les nœuds et les arcs :
  - `charger_centroides_albums` (main.rs:2467) → `AlbumNoeud{id, name, artist,
    famille: Option<i64>, date: u32 (AAAAMMJJ), duree_ms}` par album.
  - `charger_graphe_albums` (main.rs:2517, caché par taille d'empreintes) →
    un `Graphe` k-NN (`crates/analysis/src/chemin.rs`), `Graphe::plus_proches()`
    rend `(id, voisin, distance²)` pour chaque album.
  - Une fois ce réseau album-à-album en main, l'agrégat par paire de familles
    (poids inter-familles, § Ordre des bandes) n'est qu'une passe de somme
    dessus — O(nombre d'arcs), pas un calcul neuf.
- **Familles** — `Famille{id, nom, effectif}` (`crates/carto/src/source.rs`),
  12 par défaut (`VOCABULAIRE_DEFAUT`, `crates/core/src/db.rs:120-155`,
  personnalisable par l'utilisateur). Couleurs : `FAMILLES_VIVES`
  (`crates/carto/src/palette.rs:29-32`), déjà les couleurs de `--familles`
  utilisées par le nuage de points et la légende — reprises sans changement
  pour les bandes.
- **Année d'album fiable** — `annee_fiable_album`
  (`crates/core/src/db.rs:336-343`) : date de l'œuvre MusicBrainz sauf
  compilation, sinon le tag. C'est la date qui positionne un album sur l'axe
  horizontal.
- **Compression chronologique existante** — `bucket_annee`
  (`crates/core/src/db.rs:383-391`) : granularité annuelle dès 1990, tranches
  de 5 ans 1950-89, décennies avant, avec `BUCKET_INCERTAIN` pour les
  morceaux sans date fiable. C'est le mécanisme utilisé par l'ancien
  streamgraph (`Library::flux_temporel`). La frise elle-même garde un axe
  continu (la graduation à 6 mois l'exige, § Graduation) ; c'est plutôt
  l'« année d'apparition » par famille (§ 4) qui est le genre de valeur que
  cette compression pourrait consommer ailleurs, sans que la frise en dépende.
- **Position par embedding** — le mode Position (§ 1) réutilise la coordonnée
  2D déjà produite par la projection t-SNE du mode Explorer → Carte
  (`crates/analysis`), pas une réduction 1D dédiée.
- **Choix de la dominante par majorité** — le mécanisme de vote majoritaire
  par famille, égalité tranchée par le plus petit id, existe déjà à trois
  endroits : `Source::artistes` (`crates/carto/src/source.rs`),
  `crates/carto/src/ville.rs`, et la fermeture `dominante` de
  `Library::flux_temporel` (`crates/core/src/db.rs:3115-3124`). C'est le même
  mécanisme que réutilise le § 3 ci-dessous.

## 1. Axe par défaut : Famille, Position en option

**Bandes par famille = mode par défaut.** Une bande est une structure stable :
elle accueille les nouveaux albums sans redistribuer l'existant. La position
par embedding, elle, peut dériver légèrement à chaque recalcul de la
projection — un album qui bouge verticalement d'une session à l'autre casse
la mémorisation, au même titre que ce qui a déjà été tranché pour la
stabilité des positions sur la carte et sur l'anneau.

**Position (embedding) = option secondaire**, activable par l'utilisateur,
plus fidèle à la proximité sonique réelle mais moins stable. Réutilise la
coordonnée déjà calculée pour le mode Carte plutôt qu'une nouvelle réduction
dimensionnelle.

## 2. Ordre vertical des bandes — calculé, pas choisi à l'œil

### Poids inter-familles
Pour chaque arc `(album_a, album_b, distance)` du réseau existant (§ ci-dessus),
on prend la famille maîtresse de chaque album (§ 3) et on accumule dans une
matrice `W[i][j]` :
- le **nombre** de connexions entre familles `i` et `j` ;
- l'**intensité**, par exemple `Σ 1 / (1 + distance)` sur ces connexions.

Les arcs intra-famille (`i == j`) ne comptent pas pour l'ordre des bandes —
ils ne changent rien à une distance verticale qui est nulle par construction.

### Le problème : minimum linear arrangement
Ordonner les bandes pour minimiser `Σ W[i][j] · |pos(i) − pos(j)|` est le
problème classique du *minimum linear arrangement* (NP-difficile en général,
mais trivial à la taille d'un vocabulaire de familles).

**Algorithme exact retenu — DP par masque de bits.** Soit `cut(mask)` le poids
total des arcs entre les familles de `mask` et celles hors de `mask`. Alors :

```
dp[∅] = 0
dp[mask] = cut(mask) + min_{v ∈ mask} dp[mask \ {v}]
```

`dp[univers]` donne le coût optimal ; la rétropropagation des choix de `v`
reconstruit l'ordre. Complexité `O(2ⁿ · n)` : pour n = 12 (le vocabulaire par
défaut), environ 50 000 opérations — sous la milliseconde. Reste praticable
jusqu'à une vingtaine de familles ; au-delà, repli documenté sur une
recherche locale (2-opt à partir d'un ordre glouton), toujours rapide quelle
que soit la taille.

**Coût réel pour le chargement : nul.** L'ordre n'est recalculé que lorsque
le réseau d'arcs change (nouvelle passe d'analyse ou vocabulaire de familles
édité), mis en cache exactement comme `album_graphe`/`album_centroides` dans
`apps/desktop/src/main.rs` (clé = taille des empreintes). La frise ne relance
jamais ce calcul à l'ouverture.

### Repli circulaire — étudié, pas écarté a priori
La question posée : une bande du haut adjacente à celle du bas réduit-elle
encore la longueur moyenne des arcs ? Méthode : reprendre l'ordre linéaire
optimal, appliquer une courte recherche locale 2-opt sous distance
**circulaire** (`min(|pos(i) − pos(j)|, n − |pos(i) − pos(j)|)`), et comparer
le coût obtenu à celui de l'ordre linéaire pur.

**Décision, calculée et non esthétique** : adopter le repli circulaire
seulement si le gain dépasse une marge nette (proposition : ~15 % de longueur
totale pondérée en moins) ; sinon garder l'ordre linéaire, plus simple à
rendre (aucun arc qui doit boucler par le bord de l'écran, aucune bande dont
le voisinage change selon qu'on lit la légende du haut ou du bas).

## 3. Albums hybrides (Fusion) — famille maîtresse et secondaire

**Famille maîtresse = majorité des morceaux de l'album**, même mécanisme que
la fermeture `dominante` (`crates/core/src/db.rs:3115-3124`) : compte par
famille, égalité tranchée par le plus petit id. Décision retenue plutôt
qu'une pondération par similarité, pour deux raisons : c'est déjà la
convention du reste du code (trois usages existants, § ci-dessus) et elle ne
demande aucun calcul neuf (pas de centroïde de famille à maintenir). Détermine
la bande **et** la position de l'album — jamais la famille secondaire.

**Famille secondaire** = la deuxième famille la plus représentée parmi les
morceaux restants de l'album, retenue seulement si elle atteint un seuil
minimal (proposition : au moins 2 morceaux) — sans ce garde-fou, un seul
morceau mal étiqueté suffirait à fabriquer une fausse hybridation. Sert
**uniquement** à colorer l'arc entrant depuis cette famille-là ; si aucune
famille secondaire ne franchit le seuil, l'album n'est pas marqué hybride.

## 4. Année d'apparition d'une famille — garde-fou à deux artistes

Calculée pour un usage futur (indicateur, ou réutilisation par la compression
chronologique existante ailleurs, § ci-dessus) — pas consommée par la frise
elle-même dans cette spécification.

**Règle** : la date d'apparition d'une famille est la première date (échelle
de fiabilité déjà en place — MusicBrainz > tag > médiane album > médiane
artiste > ingestion, cf. `Library::ordre_darrivee`) à laquelle **au moins deux
artistes distincts** ont déjà un morceau dans cette famille. L'identité d'un
artiste suit la même logique que `mb_id_de` dans `flux_temporel` :
`mb_artist_id` si disponible, repli sur le nom sinon.

Une famille qu'un seul artiste isolé inaugure tôt n'a **pas** de date
d'apparition tant qu'un deuxième artiste distinct n'y est pas entré — la
valeur reste non définie jusque-là, plutôt que fixée sur un album unique qui
pourrait n'être qu'un accident de classification.

## 5. Graduation temporelle

Trait fin tous les 6 mois, trait épais tous les 10 ans. L'année ne s'affiche
qu'au survol — pas d'empilement de labels en continu sur l'axe, cohérent avec
la révélation par sélection déjà retenue pour les étiquettes de l'anneau
(`docs/carto-anneau.md`).

## 6. Marquage des hubs

Un **hub** est un album dont le nombre d'arcs sortants (vers des albums plus
tardifs) dépasse un seuil statistique sur la distribution des degrés sortants
du réseau de la frise (percentile ou z-score — à calibrer sur les données
réelles). Calcul dérivé du même passage O(arcs) que l'agrégat de familles du
§ 2, aucune structure neuve.

**Rendu** : un signal explicite sur le point lui-même (par exemple un
anneau/halo autour du marqueur d'album), pas seulement une densité de traits
environnante — un hub doit se voir même quand ses arcs sont estompés (état de
base, cf. la règle « estomper, jamais masquer » de `docs/carto-anneau.md`).

## Hors périmètre — piste ouverte

Distinguer filiation, influence secondaire et rupture stylistique demanderait
une donnée **documentée** (relation MusicBrainz typée — voir
`crates/core/src/musicbrainz.rs` pour le type de relation déjà modélisé côté
artistes — ou une annotation manuelle), pas une déduction depuis les
empreintes audio seules. Non traité ici ; à reprendre dans une session future
une fois qu'une telle source de vérité existe.
