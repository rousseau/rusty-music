# Architecture du peuplement

Mécanique du modèle posé par [`carto-peuplement.md`](carto-peuplement.md) :
traits, structures, schéma, réglages. Ce document est une **proposition à
relire**, pas un état des lieux — rien de ce qui suit n'est implémenté.

Trois briques, dans l'ordre où elles se construisent :

1. **le générateur de monde** — un terrain à partir de propriétés musicales ;
2. **le peuplement chronologique** — les habitants s'installent par date ;
3. **la typologie** — ferme, hameau, village, bourg, ville, métropole.

Deux décisions prises avant rédaction, parce qu'elles commandent tout le reste :

- **insertion rétroactive → éditions figées.** Un morceau ancien acquis demain
  ne se réinsère pas dans le passé (voir [O8](#o8--linsertion-rétroactive-casse-le-théorème)) ;
- **monde nº 1 → le t-SNE existant, gelé.** Les 27 042 positions déjà en base
  fournissent les ancres ; plus rien n'est reprojeté.

---

## 0. Ce qui a été mesuré

Toute la conception qui suit s'appuie sur ces chiffres, relevés sur la base
réelle (`~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db`).
Les requêtes sont en [annexe](#annexe--les-requêtes-de-mesure) pour qu'on puisse
les rejouer. **Ce qui n'est pas mesuré est signalé comme estimé.**

| Fait | Chiffre | Ce que ça commande |
|---|---|---|
| Morceaux | 27 044 | — |
| Sans année exploitable | 551 (2,0 %) | une politique explicite, §2.2 |
| Années distinctes | **64** | granularité annuelle, jamais mieux |
| Pire année | **2019 : 1 341 arrivées, 98 albums** | l'ordre intra-année décide qui fonde |
| Avant 1990 | **720 (2,7 %)**, dont 65 dans les années 1960 | le « centre ancien » repose sur 65 fondateurs |
| 1990-2019 | 22 968 (87 %) | bibliothèque très concentrée |
| Empreintes CLAP (512 d) | **27 042 (99,99 %)** | seul générateur à couverture complète |
| `descriptors` | 23 819 (88 %) — bpm 23 807, énergie 23 819 | |
| centroïde / flatness | **13 948 (52 %)** | un axe « brillance » ne couvre que la moitié |
| énergie ~ sonie | **r = 0,92** | c'est **un seul** axe |
| centroïde ~ flatness | **r = 0,893** | idem |
| centroïde ~ zcr | **r = 0,878** | idem |
| bpm ~ énergie | r = −0,156 | indépendants |
| bpm ~ centroïde | r = −0,107 | indépendants |
| énergie ~ centroïde | r = 0,242 | indépendants |
| BPM | **87 % entre 40 et 90** | erreurs d'octave : axe inutilisable brut |
| Various Artists | 805 morceaux, 45 albums | datés par la compilation, pas par l'œuvre |
| `mb_recording_id` | 25 075 (93 %) | mais **504 des 551 sans date n'ont aucun MBID** |
| Albums à années multiples | 23 | les tags datent l'édition, pas l'œuvre |
| Artistes (≥ 5 morceaux) couvrant 28-47 ans | une trentaine | discographies éclatées sur toute la frise |

Trois de ces lignes valent une lecture attentive, parce qu'elles contredisent le
document de concept : **il n'y a que trois directions descriptives
indépendantes** (tempo, énergie, brillance) et non six ; **le BPM brut est
inexploitable** ; et **la bibliothèque n'a pas d'antiquité**. Voir les
objections [O5](#o5--les-axes-candidats-ne-sont-pas-indépendants),
[O1](#o1--la-bibliothèque-na-pas-dantiquité).

---

## 1. Le générateur de monde

### 1.1 Le contrat, et il porte toute la conception

> `position(h)` doit être une **fonction pure de l'habitant seul.** Toute
> dépendance au corpus passe par un `Ancrage`, calculé une fois et figé avec le
> monde.

C'est ce qui rend le peuplement incrémental **au niveau de la position**, et pas
seulement au niveau de l'établissement. Sans cette clause, ajouter un morceau
demanderait de reprojeter, donc de tout redéplacer — exactement ce que
`carto-google-maps.md` reprochait à t-SNE et UMAP.

### 1.2 Les traits

Le trait est objet-sûr : seulement les évaluateurs purs. La préparation et la
reconstruction sont deux fonctions libres qui font le registre — un
`const NOM` ou un `where Self: Sized` casserait `dyn Generateur`.

```rust
/// Ce qu'un générateur voit d'un morceau. Rien d'autre : ni voisinage, ni
/// corpus, ni base. C'est cette pauvreté délibérée qui garantit la pureté de
/// `position`.
pub struct Habitant<'a> {
    pub track_id: i64,
    pub empreinte: Option<&'a [f32]>,   // CLAP, 512 dimensions
    pub bpm: Option<f32>,               // brut ; le repliement regarde le générateur
    pub energie: Option<f32>,
    pub brillance: Option<f32>,         // descriptors.centroid_mean
    pub annee: Option<i32>,
    pub genres: &'a [String],
}

/// Le seul état global d'un monde. Calculé une fois par `preparer`, sérialisé
/// en base, jamais recalculé tant que l'édition vit.
pub struct Ancrage {
    pub generateur: String,
    pub parametres: serde_json::Value,
    pub graine: u64,
    pub modele: Option<String>,              // « clap-htsat-unfused-5f »
    /// Ancres de projection : (empreinte, position figée). Vide pour les
    /// générateurs dont les axes sont des scalaires par morceau.
    pub ancres: Vec<(Vec<f32>, [f32; 2])>,
    /// Bornes de normalisation, un couple (min, max) par axe.
    pub bornes: Vec<[f32; 2]>,
}

/// L'état d'un établissement, tel qu'un générateur a le droit de le voir pour
/// juger d'une affinité.
pub struct EtatEtablissement {
    pub centre: [f32; 2],
    pub population: u32,
    pub centroide: Option<Vec<f32>>,
    pub genres: Vec<String>,
}

pub trait Generateur: Send + Sync {
    fn nom(&self) -> &str;

    /// Position dans le domaine [-1, 1]². `None` = ce morceau n'a pas sa place
    /// dans ce monde (propriété manquante) — il est **compté et rapporté**,
    /// jamais abandonné en silence. Voir O6.
    fn position(&self, h: &Habitant) -> Option<[f32; 2]>;

    /// La propriété que porte l'altitude, ramenée dans [0, 1]. `None` = ce
    /// morceau n'informe pas le relief ; il le subira quand même.
    fn altitude(&self, h: &Habitant) -> Option<f32>;

    /// Le second axe du diagramme de Whittaker (« précipitations »). `None`
    /// pour tout le générateur = relief hypsométrique seul, sans biomes.
    fn humidite(&self, _h: &Habitant) -> Option<f32> { None }

    /// Affinité d'un arrivant avec un établissement, dans [0, 1].
    fn affinite(&self, h: &Habitant, e: &EtatEtablissement) -> Option<f32>;
}

/// Le calcul global, fait une fois. C'est le seul endroit du système qui a le
/// droit de regarder tout le corpus.
pub fn preparer(nom: &str, corpus: &[Habitant], graine: u64) -> Result<Ancrage>;

/// La reconstruction, à partir de ce qui est en base. Doit être exacte : un
/// `Ancrage` relu donne le même générateur, donc les mêmes positions.
pub fn charger(a: &Ancrage) -> Result<Box<dyn Generateur>>;
```

### 1.3 Les quatre générateurs, et leur couverture mesurée

| Générateur | Position | Altitude | Humidité | Affinité | Couverture |
|---|---|---|---|---|---|
| `similarite-audio` | ancres CLAP (Nyström) | énergie | brillance | cos(empreinte, centroïde) | **27 042** |
| `tempo-energie` | (tempo replié, brillance) | énergie | — | 1 − distance sur les 3 descripteurs | **13 948** |
| `epoque` | (année, énergie) | brillance | — | cos(empreinte, centroïde) | 23 819 |
| `genre` | table genre → 2D + gigue déterministe | énergie | — | Jaccard des genres | 24 330 |

**`similarite-audio` — les ancres.** 512 ancres échantillonnées parmi les
27 042 positions t-SNE déjà en base, par **point le plus éloigné** pour couvrir
le nuage plutôt que sa partie dense. Position d'un morceau = barycentre des
ancres, pondéré par `exp(−d²/2σ²)` sur les 16 ancres les plus proches en espace
d'empreinte. C'est une extension de Nyström (landmark MDS), technique connue —
on ne l'invente pas. O(512) par morceau, déterministe, aucun recalcul global.
**Coût de démarrage nul : la projection existe déjà.** L'instabilité de t-SNE
disparaît parce que plus rien n'est reprojeté ; sa qualité locale est conservée
parce qu'on garde son résultat.

**`tempo-energie` — le repliement du BPM est obligatoire.** `tempo_replié` se
double ou se divise jusqu'à tomber dans [70, 140). Sans lui, **87 % de la
bibliothèque s'écrase sur un cinquième de l'axe** — séquelle des erreurs
d'octave déjà documentées dans `suite.md` (« 73 % d'accord à 6 % près, 80 % à
l'octave près »). Le repliement à l'octave est déjà la méthode retenue par
`greffe.rs` pour l'étirement ; c'est la même idée au même endroit du problème.

**`genre` — l'étage 1 ressuscité, à sa juste place.** La table genre → position
est le vieil étage 1 de `carto-google-maps.md`, et c'est la seule part de
l'ancien modèle qui reste juste : elle opère à l'échelle où un force layout est
trivial (30 à 80 nœuds), instantané et **corrigeable à la main**. Elle cesse
d'être une hiérarchie imposée d'en haut pour devenir *un générateur parmi
d'autres*. Un morceau se pose à la position de son genre dominant, plus une
gigue déterministe dérivée de son empreinte — sinon tous les morceaux d'un genre
occuperaient le même point.

### 1.4 Le relief

Réutilise `crates/core/src/density.rs` tel quel. C'est le point de réemploi qui
compte : la nappe de densité, le flou gaussien en trois passes de flou en boîte
et l'extraction d'isobandes sont déjà écrits, mesurés et testés.

```rust
pub struct Relief {
    pub resolution: usize,
    /// KDE des positions d'habitants, normalisée. Dessine la côte.
    pub habitabilite: Vec<f32>,      // resolution²
    /// Dans [0, 1]. Dessine le relief.
    pub altitude: Vec<f32>,          // resolution²
    pub humidite: Option<Vec<f32>>,
    pub niveau_mer: f32,
}
```

Construction, six étapes :

1. deux accumulateurs sur la grille : `poids[c] += 1` et `somme[c] += altitude(h)` ;
2. `density::flouter_gaussien` sur les deux. **L'estimateur de Nadaraya-Watson
   s'obtient en divisant deux champs floutés** — il n'y a pas d'interpolateur à
   écrire ;
3. `habitabilite = poids / max(poids)` ;
4. `relief = somme / poids` là où le poids est non nul, `0,5` ailleurs ;
5. bruit fractal (fBm, 4 octaves, persistance 0,5, amplitude 0,08) semé par
   `graine` — c'est le « bruit pour les côtes » d'Amit Patel, sans lui la ligne
   de rivage est un ovale ;
6. `altitude = niveau_mer + (1 − niveau_mer) · relief` sur la terre ;
   bathymétrie décroissante sous la mer, pour l'ombrage.

`ContourBuilder::isobands` — déjà en dépendance, déjà utilisé — rend les courbes
de niveau et les bandes hypsométriques. Le **diagramme de Whittaker** devient
une table `const` à deux entrées, indexée par (bande d'altitude, bande
d'humidité) → biome. Sans humidité, la palette est purement hypsométrique — mer,
plage, plaine, colline, montagne, neige — ce qui est déjà un rendu IGN correct
et suffit pour livrer.

### 1.5 Le niveau de la mer — correction au document de concept

`carto-peuplement.md` fait du niveau de la mer un **seuil de densité** ; la
consigne de conception en fait un **seuil d'altitude**. Les deux ne peuvent pas
être la même grandeur : si l'altitude porte l'énergie, seuiller l'altitude noie
les morceaux calmes — c'est-à-dire ses propres habitants. Réconciliation
retenue :

> **L'habitabilité (la densité) dessine la côte ; la troisième propriété dessine
> le relief au-dessus.**
>
> `niveau_mer` = 1ᵉʳ centile de l'habitabilité **évaluée aux positions des
> habitants**, et non sur les cellules de grille. La part de terre émergée
> devient alors une *mesure de sortie*, rapportée à la génération, plutôt qu'un
> réglage qu'on tâtonne.

Et une garantie plutôt qu'un quantile. Le masque terrestre est :

```
terre = (habitabilité ≥ niveau_mer) ∪ (disques de rayon_ile autour des habitants sous le seuil)
```

**Aucun habitant ne se noie ; les plus isolés deviennent des îles.** Ce qui
tombe juste : la typologie a déjà « ferme isolée » pour eux.

---

## 2. Le peuplement chronologique

### 2.1 L'ordre d'arrivée

Une clé, et elle est la seule source de vérité. Son ordre lexicographique **est**
l'ordre du peuplement.

```rust
pub struct CleArrivee {
    pub date: u32,      // AAAAMMJJ ; mois et jour à 00 si inconnus
    pub album: u64,     // hachage stable de (album_artist, album)
    pub disque: u16,    // 0 faute de mieux : voir ci-dessous
    pub piste: u16,     // tracks.track_no
    pub track_id: i64,  // départage final : jamais d'ex æquo
}
```

**`disque` n'a pas de source aujourd'hui.** `tracks` porte `track_no` mais aucun
numéro de disque ; un coffret de trois disques voit donc ses trois pistes nº 1
départagées par `track_id`. `lofty` lit l'étiquette `DISCNUMBER` — il faut une
colonne, ou accepter le désordre à l'intérieur d'un coffret. Peu grave, mais à
ne pas découvrir à l'implémentation.

**`album` avant `piste`, et c'est structurant.** Sur les 1 341 arrivées de 2019,
un tri par (année, id) éparpillerait les treize pistes d'un album parmi 98
albums, et chacune irait fonder ailleurs. Groupées, **un album arrive en bloc et
fonde un hameau** — ce qui est musicalement ce qu'on veut, et ce qui donne du
sens à la strate.

L'ordre entre deux albums de la même année est celui de leur hachage :
arbitraire, mais **déterministe et stable**. On ne peut pas faire mieux avec des
données au millésime (voir [O4](#o4--le-millésime-na-pas-la-granularité-annoncée)).

### 2.2 La datation — cinq échelons, provenance conservée

`arrivees.date_source` garde d'où vient la date. Ce n'est pas une commodité de
débogage : c'est ce qui permet de rendre un habitant mal daté différemment, et
de rejouer le peuplement quand une source s'améliore.

| Échelon | Source | Portée | Note |
|---|---|---|---|
| `musicbrainz` | `mb_release_groups.first_release_date` (**colonne à ajouter**) | jusqu'à 25 017 | **corrige la réédition** : date de l'œuvre, pas du pressage |
| `tag` | `tracks.year` | 26 493 | repli |
| `album` | médiane des frères datés du même album | **+56** | mesuré |
| `artiste` | médiane des morceaux datés de l'artiste | **+23** (79 en tout) | mesuré |
| `ingestion` | `tracks.added_at` | **472 (1,7 %)** | dernier recours |

**Les morceaux sans date : ce qui est proposé, et pourquoi.** Sur les 551,
**504 n'ont aucun MBID** — ni enregistrement, ni artiste. MusicBrainz ne les
sauvera pas ; ce n'est pas une question d'effort ou de patience, il n'y a rien
à interroger. L'échelle locale en récupère 79 (mesuré : 56 par l'album, 23 de
plus par l'artiste).

Les 472 restants prennent **leur date d'entrée dans la bibliothèque**, avec
`date_source = 'ingestion'`, et sont **rendus distinctement** sur la carte : pas
de date de fondation affichée, symbole nuancé. Ni écartés, ni maquillés en 1998.
C'est vrai de la seule chose qu'on sache d'eux : ils sont arrivés récemment
*chez toi*. Et le jour où ils reçoivent une vraie date, `date_source` dit
exactement lesquels rejouer.

### 2.3 La boucle

```
index = Grille::neuve(rayon_max)              // 20×20 cellules sur [-1, 1]²

pour (rang, h) dans arrivants                 // déjà triés par CleArrivee
    pos = generateur.position(h)              // None → hors_monde, compté et rapporté
    pos = relief.ancrer(pos)                  // île si sous le niveau de la mer
    candidats = index.dans_disque(pos)        // 3×3 cellules, puis |pos − centre| ≤ rayon(pop)
    meilleur  = argmax affinite(h, e) sur candidats, retenu si ≥ seuil_affinite
    si meilleur : e.accueillir(h, rang)
    sinon       : index.fonder(pos, h, rang)
```

**Rayon de recrutement, et non « l'établissement le plus proche ».**
`rayon(n) = rayon_base · √n`, plafonné à `rayon_max`. Un arrivant ne regarde que
les établissements dont le disque de recrutement le contient. C'est le rayon de
travail d'une ville en 4X ; ça rend la géographie lisible — une métropole a un
large bassin, une ferme n'en a presque pas — **et c'est le garde-fou contre
l'emballement préférentiel** ([O7](#o7--lattachement-préférentiel-semballe)).

**Index spatial : grille uniforme, pas de R-tree.** Domaine borné, requête à
rayon borné : cellule = `rayon_max` = 0,10, soit 20×20 = 400 cellules, balayage
de 3×3. Avec ~3 000 établissements, ~68 candidats par requête, ~1,8 M cosinus
512 d pour tout le corpus — **estimé** de l'ordre du GFLOP, donc sous la
seconde. `rstar` reste le repli si le profil dément l'estimation.

### 2.4 Le théorème de stabilité

C'est la clause centrale du modèle, et elle se démontre au lieu de se vérifier.
La parcelle d'un habitant est :

```
parcelle(e, place) = centre(e) + pas_parcelle·√place · (cos(place·φ), sin(place·φ))
                     φ = angle d'or, 2,399963 rad
```

- `centre(e)` est la position du fondateur, **figée à la fondation** ;
- `place` est la population de l'établissement **au moment de l'arrivée**.

Aucun des deux ne dépend de quoi que ce soit qui arrive ensuite. La position
d'un habitant est écrite une fois et n'est plus jamais recalculée.

> **La stabilité n'est pas une contrainte vérifiée après coup : c'est la forme
> de la formule.**

Effet de bord heureux : la spirale phyllotaxique croît naturellement vers
l'extérieur. Un village s'étend depuis son centre, pour la même raison qu'en
vrai — la place libre est au bord.

Cohérence des deux rayons : `rayon_base ≈ 5 × pas_parcelle`, donc le bâti d'un
établissement occupe toujours environ un cinquième de son bassin de recrutement.
Deux établissements peuvent être voisins sans que leurs parcelles se chevauchent.

### 2.5 L'affinité : centroïde courant

Le centroïde de l'établissement, mis à jour à chaque arrivée, plutôt que
l'empreinte figée du fondateur. Raison : le centroïde donne des établissements
musicalement cohérents, là où le fondateur fige une identité que ses habitants
ne partagent pas forcément. Le risque de dérive sémantique — le « blob » qui
finit par tout avaler — est tenu par le **plafond de rayon**, pas par un gel du
centroïde.

Mise à jour en ligne, O(dim) par arrivée : un ajout tardif reste O(log n).
Repli documenté si la calibration montre des établissements incohérents :
l'empreinte du fondateur, figée.

### 2.6 Rejeu et éditions

`arrivees.rang` et `etablissements.fondation_rang` sont conservés, donc :

- **« la carte en 1975 »** = `WHERE date_cle <= 19751231` ;
- **l'animation** itère sur `rang` ;
- **le rang typologique se calcule à la date affichée**, il n'est donc pas
  stocké. Un hameau de 1975 est une métropole en 2026, et la carte doit le
  montrer. `etablissements.population` n'est qu'une dénormalisation de confort
  pour la vue par défaut.

**Éditions.** Le peuplement d'une édition est un fait consigné. Un arrivant
tardif rejoint un établissement existant ou en fonde un nouveau à sa position ;
il **ne se réinsère pas dans le passé** — il porte sa date de sortie comme
étiquette, non comme rang. `mondes.fige_jusqu_a` marque la frontière. Une
« nouvelle édition de la carte », déclenchée à la main, rejoue tout par date de
sortie. C'est la mécanique d'éditions déjà admise par `carto-google-maps.md`,
et c'est la réponse à [O8](#o8--linsertion-rétroactive-casse-le-théorème).

---

## 3. La typologie des établissements

Le rang est **calculé, jamais stocké** — il dépend de la date affichée (§2.6).

| Rang | Population | Symbole | Étiquette | Zoom d'apparition |
|---|---|---|---|---|
| Ferme isolée | 1 | carré 2 px | 8 px italique gris | 13 |
| Hameau | 2-5 | point 3 px | 9 px italique | 11 |
| Village | 6-20 | cercle vide 4 px | 10 px | 9 |
| Bourg | 21-60 | cercle plein 5 px | 11 px | 7 |
| Ville | 61-200 | cercle cerclé 7 px | 13 px demi-gras | 5 |
| Métropole | 200+ | étoile cerclée 9 px | 16 px gras capitales | 3 |
| *(habitants)* | — | point 1,5 px | titre 9 px | 14 |

```rust
pub enum Rang { Ferme, Hameau, Village, Bourg, Ville, Metropole }

impl Rang {
    pub fn depuis_population(n: u32) -> Rang;
    pub fn symbole(self) -> Symbole;   // forme + rayon en pixels
    pub fn taille_etiquette(self) -> f32;
    pub fn zoom_apparition(self) -> f32;
}
```

**MapLibre fait le reste, et il ne faut rien réécrire** : `symbol-sort-key =
−population` donne la priorité de collision aux grands, `minzoom` par couche
gère l'apparition, l'évitement de collisions est natif. C'est précisément la
raison pour laquelle `CLAUDE.md` a retenu MapLibre plutôt qu'un rendu maison.

---

## 4. Schéma SQL

Cinq tables neuves, deux colonnes sur une table existante. **Aucune modification
destructive** : `features.x`, `features.y` et `features.cluster` restent en
place, la carte actuelle continue de fonctionner pendant toute la bascule.

```sql
-- Un monde = un générateur, ses paramètres, sa graine. Figé une fois créé :
-- c'est ce qui fait qu'une carte se mémorise.
CREATE TABLE mondes (
    id           INTEGER PRIMARY KEY,
    nom          TEXT    NOT NULL,
    generateur   TEXT    NOT NULL,        -- 'similarite-audio' | 'genre' | 'epoque' | 'tempo-energie'
    parametres   TEXT    NOT NULL,        -- JSON
    graine       INTEGER NOT NULL,
    modele       TEXT,                    -- 'clap-htsat-unfused-5f', si le générateur en dépend
    niveau_mer   REAL    NOT NULL,        -- mesuré à la génération, pas réglé
    cree_le      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    -- Rang au-delà duquel on ajoute sans rejouer. En deçà, le peuplement est
    -- un fait consigné : voir « Éditions ».
    fige_jusqu_a INTEGER NOT NULL DEFAULT 0
);

-- Ancres de projection : le seul état global du générateur, figé avec le monde.
-- Sans elles, la position d'un morceau ne serait pas une fonction de lui seul.
CREATE TABLE monde_ancres (
    monde_id INTEGER NOT NULL REFERENCES mondes(id) ON DELETE CASCADE,
    rang     INTEGER NOT NULL,
    vecteur  BLOB    NOT NULL,            -- f32 little-endian
    ax       REAL    NOT NULL,            -- position 2D figée de l'ancre
    ay       REAL    NOT NULL,
    PRIMARY KEY (monde_id, rang)
);

-- Le relief échantillonné. Stocké et non recalculé : « la carte en 1975 » doit
-- lire le même terrain qu'aujourd'hui, et le champ de densité bouge quand le
-- corpus grossit (voir O9).
CREATE TABLE monde_relief (
    monde_id     INTEGER PRIMARY KEY REFERENCES mondes(id) ON DELETE CASCADE,
    resolution   INTEGER NOT NULL,
    habitabilite BLOB NOT NULL,           -- f32 LE, resolution²
    altitude     BLOB NOT NULL,           -- f32 LE, resolution²
    humidite     BLOB                     -- f32 LE, optionnel (biomes Whittaker)
);

CREATE TABLE etablissements (
    id             INTEGER PRIMARY KEY,
    monde_id       INTEGER NOT NULL REFERENCES mondes(id) ON DELETE CASCADE,
    cx             REAL    NOT NULL,      -- centre, figé à la fondation, jamais mis à jour
    cy             REAL    NOT NULL,
    fondation_rang INTEGER NOT NULL,
    fondation_date INTEGER NOT NULL,      -- AAAAMMJJ
    fondateur_id   INTEGER NOT NULL REFERENCES tracks(id),
    -- Dénormalisation de confort pour la vue par défaut. La population à une
    -- date donnée se compte dans `arrivees`.
    population     INTEGER NOT NULL DEFAULT 1,
    centroide      BLOB,                  -- f32 LE, centroïde courant de l'affinité
    nom            TEXT,                  -- toponyme, généré ou saisi
    ile            INTEGER NOT NULL DEFAULT 0   -- fondé hors du masque terrestre
);
CREATE INDEX idx_etab_monde ON etablissements(monde_id, population DESC);

-- Le journal du peuplement. Une ligne par habitant placé. C'est cette table qui
-- rend la croissance rejouable.
CREATE TABLE arrivees (
    monde_id         INTEGER NOT NULL REFERENCES mondes(id) ON DELETE CASCADE,
    track_id         INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    rang             INTEGER NOT NULL,    -- 0..n-1, l'ordre total
    date_cle         INTEGER NOT NULL,    -- AAAAMMJJ
    date_source      TEXT    NOT NULL,    -- 'tag'|'musicbrainz'|'album'|'artiste'|'ingestion'
    etablissement_id INTEGER NOT NULL REFERENCES etablissements(id) ON DELETE CASCADE,
    place            INTEGER NOT NULL,    -- rang d'arrivée DANS l'établissement
    x                REAL    NOT NULL,    -- parcelle = phyllotaxie(centre, place)
    y                REAL    NOT NULL,
    PRIMARY KEY (monde_id, track_id)
);
CREATE INDEX idx_arrivees_rang ON arrivees(monde_id, rang);
CREATE INDEX idx_arrivees_etab ON arrivees(etablissement_id, place);
CREATE INDEX idx_arrivees_date ON arrivees(monde_id, date_cle);

-- Deux colonnes sur une table existante. `first-release-date` est DÉJÀ dans la
-- réponse MusicBrainz que `mb_poser_albums` reçoit : aucun appel de plus, et
-- c'est ce qui corrige les rééditions (O2). `secondary_types` porte
-- « Compilation », qui signale une date à ne pas croire (O3).
ALTER TABLE mb_release_groups ADD COLUMN first_release_date TEXT;
ALTER TABLE mb_release_groups ADD COLUMN secondary_types    TEXT;
```

---

## 5. Paramètres et valeurs de départ

| Paramètre | Départ | D'où elle sort |
|---|---|---|
| `seuil_affinite` | **0,62** (cosinus CLAP) | **à calibrer** — procédure ci-dessous |
| `rayon_base` | 0,012 | ≈ 5 × `pas_parcelle` |
| `exposant_rayon` | 0,5 | aire de recrutement ∝ population |
| `rayon_max` | 0,10 | 5 % de la largeur de carte ; fixe la maille de la grille |
| `pas_parcelle` | 0,0025 | 200 habitants tiennent dans un disque de rayon 0,035 |
| `rayon_ile` | 0,0075 | 3 × `pas_parcelle` |
| `noyau_habitabilite` | 0,02 | la valeur déjà retenue et justifiée dans `density.rs` |
| `resolution_relief` | 1024 | idem `density.rs` |
| `amplitude_bruit` | 0,08 | à juger à l'œil |
| `octaves_bruit` | 4, persistance 0,5 | fBm standard |
| `niveau_mer` | 1ᵉʳ centile de l'habitabilité aux positions d'habitants | garantit ≥ 99 % sur le continent |
| `graine` | 20260821 | n'importe laquelle, mais consignée |

### La calibration du seuil d'affinité

C'est la seule valeur qu'on ne peut pas déduire. Deux mesures préalables, toutes
deux quasi gratuites parce que le graphe des k plus proches voisins existe déjà
(`crates/analysis/src/chemin.rs`, `Graphe::construire`) :

1. **distribution du cosinus** entre un morceau et ses 1ᵉʳ, 8ᵉ et 50ᵉ voisins.
   Point de départ = médiane du 8ᵉ voisin ;
2. **balayage** de 0,50 à 0,80 par pas de 0,02, en rapportant à chaque fois :
   nombre d'établissements, population médiane, plus grande population, part de
   fermes isolées.

**Critères d'acceptation, annoncés avant de mesurer** — sinon on ajuste les
critères au résultat :

- 5 à 15 métropoles ;
- population médiane entre 4 et 8 ;
- fermes isolées < 15 % ;
- le plus gros établissement < 10 % de la bibliothèque.

Une valeur qui ne les tient pas est **rejetée**, pas rattrapée par un correctif.

---

## Objections

Classées par poids. Les six premières sont des problèmes de données ou de
modèle : **aucune ne se résout en codant mieux.**

### O1 — La bibliothèque n'a pas d'antiquité

**720 morceaux avant 1990 (2,7 %), dont 65 dans les années 1960.**

Le récit promis par `carto-peuplement.md` — « centre ancien dense, périphérie
récente, comme une vraie ville, pour la même raison » — suppose une fondation
lente. Ici, 65 habitants fondent le monde, puis 6 715 débarquent dans les années
1990 et 9 027 dans les années 2000. Ce ne sera pas un centre ancien : ce sera
une poignée de hameaux, puis un raz-de-marée.

**Ce modèle raconte une histoire que cette bibliothèque n'a pas.** C'est
l'objection la plus grave, parce qu'elle porte sur ce qui rend le modèle
séduisant.

Atténuation : frise en **rang** et non en calendrier, pour que l'animation ne
passe pas 40 % de son temps sur 2,7 % des morceaux. Mais l'atténuation est
cosmétique — le fait reste.

### O2 — `year` date l'édition, pas l'œuvre

Une réédition 2010 d'un disque de 1969 arrive en 2010. Et ce sont précisément
les 720 morceaux anciens, ceux dont tout dépend (O1), qui sont les plus
exposés : le vieux fonds est ce qu'on rachète en remaster.

Indice mesuré : **23 albums portent déjà plusieurs années distinctes** en
interne — les tags ne sont même pas cohérents à l'intérieur d'un disque.

Correctif : `first-release-date` du release-group MusicBrainz, **déjà présent
dans la réponse que le client reçoit**. Il suffit d'une colonne. C'est le
meilleur rapport valeur/effort de tout le chantier, et il est à faire avant
toute autre chose.

### O3 — Les compilations arrivent au mauvais siècle

805 morceaux sur 45 albums « Various Artists ». Une compilation soul de 2003
fait fonder en 2003 vingt établissements de musique de 1965.

Le release-group ne suffit pas : c'est la compilation. Il faut la date au niveau
de l'**enregistrement**. Coût : 805 morceaux à une requête par seconde =
**≈ 13 minutes**, une seule fois. Tractable.

À ne pas généraliser : les 27 000 morceaux prendraient 7 h 30. `secondary_types`
= « Compilation » sert précisément à cibler les 805.

### O4 — Le millésime n'a pas la granularité annoncée

**64 valeurs distinctes, 1 341 arrivées dans la pire année.** Le placement
chronologique suppose une suite ; on a des paquets annuels.

Qui fonde et qui rejoint, à l'intérieur d'une année, est décidé par le
départage — donc par le hachage d'album. Déterministe et stable, mais
**arbitraire**. Le mois et le jour de MusicBrainz réduisent le problème sans le
supprimer : beaucoup de release-groups n'ont eux-mêmes que l'année.

### O5 — Les axes candidats ne sont pas indépendants

Mesuré : **énergie ~ sonie 0,92**, **centroïde ~ flatness 0,893**,
**centroïde ~ zcr 0,878**. Il n'y a **trois directions indépendantes**, pas six :
tempo, énergie, brillance. Choisir deux axes du même faisceau écrase le monde
sur une diagonale.

Bonne nouvelle : l'exemple du document de concept survit à la mesure —
« acoustique ↔ synthétique » (la brillance) et « calme ↔ intense » (l'énergie)
sont bien indépendants (r = 0,242).

Mauvaise : **le BPM brut est inutilisable.** 87 % de la bibliothèque tombe entre
40 et 90 BPM, séquelle des erreurs d'octave documentées dans `suite.md`.
Repliement obligatoire avant tout usage comme axe.

### O6 — Un seul générateur couvre la bibliothèque

CLAP : 27 042 (99,99 %). Descripteurs : 23 819 (88 %). **Brillance : 13 948
(52 %)** — les colonnes `centroid_*`, `rolloff_*` et `flatness_*` ont été
ajoutées après coup et la passe n'a jamais été rejouée sur tout le corpus.

Un monde `tempo-energie` laisserait donc **la moitié de la bibliothèque hors
carte**. D'où `position() -> Option` et un compte de « morceaux hors de ce
monde » affiché — jamais un abandon silencieux.

Prérequis explicite : rejouer la passe descripteurs avant de livrer ce
générateur.

### O7 — L'attachement préférentiel s'emballe

Chronologique + « rejoindre le plus proche » est littéralement le modèle
Barabási-Albert : le premier gros établissement grossit d'autant plus vite qu'il
est déjà gros. Sans plafond, un seul établissement avale la bibliothèque.

Le plafond `rayon(n) = rayon_base·√n` borné par `rayon_max` est là pour ça, et
le critère d'acceptation « plus gros établissement < 10 % » est le test qui le
vérifie. **À surveiller comme le premier mode de défaillance à
l'implémentation.**

### O8 — L'insertion rétroactive casse le théorème

Le placement est stable quand on **ajoute du récent**, pas quand on **ajoute du
vieux**. Un morceau de 1973 acquis demain s'insère au rang 300 et décale tout ce
qui suit. Or une discothèque grossit précisément par des disques anciens.

C'est le seul endroit où le théorème de §2.4 demande une clause. Réponse
retenue : **les éditions figées** (§2.6). Les strates par date de sortie sont
une propriété d'une édition ; la croissance quotidienne est purement
additive. Le prix est honnête et il faut le dire : *à l'intérieur d'une édition,
un morceau ancien acquis tard n'est pas à sa place chronologique.*

### O9 — Le terrain n'est pas stable, seul le peuplement l'est

La côte vient d'un KDE sur tout le corpus : ajouter 3 000 morceaux la déplace,
alors même qu'aucun habitant n'a bougé. C'est pourquoi `monde_relief` **stocke
la grille** plutôt que de la recalculer.

Conséquence à assumer : un monde vieillissant voit apparaître des habitants sur
une mer qui ne les connaît pas — d'où la règle des îles (§1.5). Lecture
inattendue et plutôt heureuse : **l'archipel des acquisitions récentes.**

### O10 — L'artiste cesse d'être un lieu

Une trentaine d'artistes de cinq morceaux ou plus couvrent 28 à 47 ans. Leur
œuvre se répartira sur des établissements fondés à des décennies d'écart, et
potentiellement loin les uns des autres si leur son a changé.

Musicalement défendable — la période berlinoise *est* ailleurs. Mais **c'est une
rupture avec l'ancien modèle**, où l'étage 2 faisait explicitement de l'artiste
une ville. « Où est Bowie ? » n'a plus de réponse géographique.

À traiter comme un **calque** — surligner tous les habitants d'un artiste — et
non comme un lieu. Et à écrire dans la spec d'interface, sinon l'utilisateur
cherchera la ville.

### O11 — Le monde `epoque` est une tautologie

Si l'axe des abscisses est l'année et que le peuplement est chronologique, la
colonisation balaie littéralement la carte de gauche à droite. C'est joli une
fois, et ça n'apprend rien. À garder comme démonstration du principe, pas comme
monde de travail.

### O12 — `docs/carto-direction.md` n'existe pas

`CLAUDE.md` y renvoie deux fois, `carto-google-maps.md` une, pour le relief, les
toponymes et la navigation « balade » — c'est-à-dire pour la moitié du rendu de
ce chantier. À écrire ou à déréférencer, mais pas à laisser pendre.

---

## Suite — décidé, pas encore fait

**Le code ira dans un nouveau crate `crates/carto`.** Le peuplement ne tire ni
Burn ni rodio : à part, il compile en quelques secondes et se teste sans GPU,
là où `analysis` traîne un modèle de 4 400 lignes générées. `core` garde ainsi
le périmètre d'ingestion que lui donne `CLAUDE.md`. `density.rs` a vocation à
déménager avec — c'est déjà du calcul de carte logé dans le cœur.

**Premier chantier : corriger les dates (O2 + O3), avant d'implémenter.**
Colonnes `first_release_date` et `secondary_types` sur `mb_release_groups`, puis
la passe au niveau enregistrement sur les 805 morceaux de compilation
(≈ 13 minutes). Raison : le placement chronologique lit ces dates, et les 720
morceaux d'avant 1990 — ceux dont tout le récit dépend (O1) — sont précisément
les plus mal datés. Un peuplement bâti sur des dates de pressage ne se rattrape
pas sans tout rejouer.

Contre-argument enregistré : la calibration de `seuil_affinite` (§5) ne dépend
pas des dates et pourrait donc se mesurer dès maintenant, sur le graphe des
voisins qui existe déjà.

**Restent ouverts, hors périmètre de ce document :**

- **les toponymes** — `etablissements.nom` existe, rien ne le remplit. L'essai
  `experiments/clap-texte/` a mesuré le nommage (« 7 mieux, 3 égales, 2
  fausses ») et `suite.md` §7 bis dit que le choix n'est pas tranché ;
- **le réseau routier** — `carto-peuplement.md` affirme qu'il se construit sur
  les établissements, mais `carto-google-maps.md` §2 le construit sur un kNN de
  morceaux. La hiérarchie routière tombe pourtant sur les rangs sans effort :
  autoroute entre métropoles, sentier vers les fermes isolées ;
- **la cohabitation avec la carte actuelle** — le schéma est non destructif,
  l'interface n'est pas spécifiée : deux modes, un basculement, une bascule
  définitive ? Question pour `ui-spec.md`, pas pour ce document ;
- **`docs/carto-direction.md`** — O12, toujours pendant.

---

## Annexe — les requêtes de mesure

Sur `~/Library/Application Support/fm.rustymusic.desktop/rusty-music.db`, en
lecture seule.

```sql
-- Dates : couverture et concentration
SELECT COUNT(*) FROM tracks;
SELECT COUNT(*) FROM tracks WHERE year IS NULL OR year < 1900 OR year > 2026;
SELECT COUNT(DISTINCT year) FROM tracks WHERE year BETWEEN 1900 AND 2026;
SELECT (year/10)*10 AS dec, COUNT(*) FROM tracks
  WHERE year BETWEEN 1900 AND 2026 GROUP BY dec ORDER BY dec;
SELECT year, COUNT(*) n, COUNT(DISTINCT album) alb FROM tracks
  WHERE year BETWEEN 1900 AND 2026 GROUP BY year ORDER BY n DESC LIMIT 5;

-- Ce que l'échelle de datation récupère
WITH nd AS (SELECT * FROM tracks WHERE year IS NULL OR year<1900 OR year>2026)
SELECT COUNT(*) FROM nd WHERE album IN
  (SELECT album FROM tracks WHERE year BETWEEN 1900 AND 2026 AND album IS NOT NULL);
WITH nd AS (SELECT * FROM tracks WHERE year IS NULL OR year<1900 OR year>2026)
SELECT COUNT(*) FROM nd WHERE COALESCE(mb_album_artist_id, album_artist, artist) IN
  (SELECT COALESCE(mb_album_artist_id, album_artist, artist) FROM tracks
   WHERE year BETWEEN 1900 AND 2026);
SELECT COUNT(*) FROM tracks
  WHERE (year IS NULL OR year<1900 OR year>2026)
    AND mb_recording_id IS NULL AND mb_album_artist_id IS NULL;

-- Couverture des générateurs
SELECT model, dim, COUNT(*) FROM features GROUP BY model, dim;
SELECT COUNT(*), COUNT(bpm), COUNT(energy), COUNT(centroid_mean) FROM descriptors;

-- Corrélations entre axes candidats (Pearson)
WITH s AS (SELECT bpm b, energy e, loudness l, centroid_mean c, flatness_mean f, zcr z
           FROM descriptors WHERE bpm IS NOT NULL AND energy IS NOT NULL
             AND centroid_mean IS NOT NULL)
SELECT
  ROUND((AVG(e*l)-AVG(e)*AVG(l))/(SQRT(AVG(e*e)-AVG(e)*AVG(e))*SQRT(AVG(l*l)-AVG(l)*AVG(l))),3) AS energie_sonie,
  ROUND((AVG(c*f)-AVG(c)*AVG(f))/(SQRT(AVG(c*c)-AVG(c)*AVG(c))*SQRT(AVG(f*f)-AVG(f)*AVG(f))),3) AS centroide_flatness,
  ROUND((AVG(b*e)-AVG(b)*AVG(e))/(SQRT(AVG(b*b)-AVG(b)*AVG(b))*SQRT(AVG(e*e)-AVG(e)*AVG(e))),3) AS bpm_energie,
  ROUND((AVG(e*c)-AVG(e)*AVG(c))/(SQRT(AVG(e*e)-AVG(e)*AVG(e))*SQRT(AVG(c*c)-AVG(c)*AVG(c))),3) AS energie_centroide
FROM s;

-- Le BPM s'écrase-t-il ?
SELECT CAST(bpm/10 AS INT)*10 d, COUNT(*) FROM descriptors
  WHERE bpm IS NOT NULL GROUP BY d ORDER BY d;

-- Compilations et rééditions
SELECT COUNT(*) FROM tracks WHERE album_artist LIKE '%Various%';
SELECT COUNT(*) FROM (SELECT album FROM tracks
  WHERE album IS NOT NULL AND year IS NOT NULL
  GROUP BY album HAVING COUNT(DISTINCT year) > 1);

-- Étalement des discographies
SELECT etendue, COUNT(*) FROM (
  SELECT COALESCE(mb_album_artist_id, album_artist, artist) a,
         MAX(year)-MIN(year) etendue, COUNT(*) n
  FROM tracks WHERE year BETWEEN 1900 AND 2026 GROUP BY a HAVING n >= 5
) GROUP BY etendue ORDER BY etendue DESC LIMIT 15;
```
