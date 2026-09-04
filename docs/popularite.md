# Popularité générale des morceaux et des albums

## Intention

Donner à chaque morceau une **popularité générale** — sa notoriété dans le
monde, pas dans cette bibliothèque. Aujourd'hui le seul signal disponible est
`effectif` : le nombre de morceaux gardés d'un artiste (`crates/carto/src/source.rs`,
`crates/analysis/src/reseau.rs`). C'est une approximation locale et honnête,
mais elle ne distingue pas un tube d'une face B, ni un artiste culte d'un
artiste oublié dont on a tout gardé.

La popularité viendra donc de **sources externes agrégées**, récupérée pendant
l'analyse de la bibliothèque, mise en cache, rafraîchie de loin en loin. Elle
s'affiche comme une **jauge graduée** à côté de chaque morceau — dans la file
d'attente (panneau droit) et dans la liste des pistes d'un album (panneau
central).

**Additive, comme l'enrichissement des genres** (`crates/core/src/enrichir.rs`) :
une bibliothèque qui n'a jamais vu le réseau reste entièrement utilisable, la
jauge affiche simplement « — ». La popularité précise, elle ne conditionne rien.

## Les sources

MusicBrainz **ne porte aucune donnée de popularité** — c'est une base de faits.
Mais il est le **pivot** : chaque entité MB a un MBID, et nos morceaux portent
déjà `mb_recording_id`, `mb_artist_id`, `mb_album_artist_id` (table `tracks`).
Toutes les sources ci-dessous s'interrogent par MBID ou se rattachent à un MBID.

| Source | Ce qu'on obtient | Échelon | Accès | Licence / conditions |
|---|---|---|---|---|
| **ListenBrainz** `/1/popularity/{recording,release-group}` | `total_listen_count`, `total_user_count`, **par lot** (POST d'une liste de MBID) | recording · release-group | aucun | CC0, même fondation que MusicBrainz, auto-hébergeable |
| **Deezer** `api.deezer.com/search/track` | `rank` (piste, ~10 k – 1 M) | piste | aucun | API publique, ~50 req/5 s ; recherche par artiste + titre, **retenue seulement si artiste ET titre concordent** |
| ~~Last.fm~~ | — | — | clé d'API | **écarté** — on évite les clés pour l'instant |
| ~~Discogs, Spotify, YouTube~~ | — | — | jeton / OAuth / clé | **écartés** — même raison |

**Décision — deux sources, aucune clé.**

- **ListenBrainz** : le socle propre. Interrogation par MBID d'enregistrement et
  de release-group, en quelques requêtes groupées, sans compte ni clé. La sonde
  de phase 0 mesure **97 % de couverture** (enregistrement ou album) sur la
  bibliothèque de test, uniforme quel que soit l'effectif de l'artiste.
- **Deezer** : un second signal, public plus large, biais francophone assumé.
  Deezer n'indexe pas par MBID : on retrouve une piste par recherche
  « artiste + titre ». La sonde de phase 0 le valide — **80 % de couverture
  piste, 99 % des rapprochements justes** dès lors qu'on exige la concordance
  de l'artiste **et** du titre normalisé. Corrélation modérée avec ListenBrainz
  (ρ ≈ 0,55) : il apporte de l'information, pas de la redondance. Limité à
  l'échelon **piste** en phase 1 (le `rank` est dans la recherche ; l'album
  demanderait un appel de plus et ne couvre que 69 %).
- **Tout ce qui demande une clé ou un jeton est écarté** — Last.fm, Discogs,
  Spotify, YouTube. Décision de l'utilisateur ; et la sonde montre que ce n'est
  pas nécessaire (1,5 % de morceaux sans aucune source).

**Ce qui n'est accessible par aucune source ouverte** : ventes réelles
(Luminate — B2B payant), positions de charts (Billboard, SNEP — pas d'API),
certifications (RIAA, BPI).

## Couverture et repli

Tout morceau n'a pas de `mb_recording_id` dans ses tags. La popularité se
résout donc **du plus précis au plus large**, et s'arrête à l'album :

1. `mb_recording_id` → popularité de l'enregistrement (idéale : distingue les
   morceaux d'un même album) ;
2. sinon, l'album MusicBrainz du morceau (`mb_release_groups`, rattaché par
   titre normalisé — déjà en place pour les genres) → popularité du
   release-group ; tous les morceaux de l'album partagent alors la valeur ;
3. sinon, **grisé** — aucune jauge.

Pas de repli sur l'artiste : décision de l'utilisateur. Une popularité
d'artiste ferait de tous ses morceaux une même valeur plate, ce qui n'apprend
rien à l'échelle d'une file d'attente ou d'une liste de pistes. (La popularité
d'artiste reste utile pour la carte — profil d'altitude, étagement des
étiquettes — mais la carte n'est pas touchée ici ; à récupérer avec ce
chantier-là.)

L'échelon retenu est mémorisé et affiché dans l'infobulle de la jauge : une
valeur d'album ne se lit pas tout à fait comme une valeur de morceau.

## Modèle de données

Trois tables neuves, sur le patron de `mb_genres` / `mb_fetched` /
`decouvrir_suivi` (`crates/core/sql/schema.sql`).

```sql
-- Popularité brute, telle qu'une source la rend. Une ligne par (entité, source).
CREATE TABLE IF NOT EXISTS popularite (
    mbid      TEXT NOT NULL,   -- MBID d'enregistrement ou de release-group
    kind      TEXT NOT NULL,   -- 'recording' | 'release-group'
    source    TEXT NOT NULL,   -- 'listenbrainz' | 'deezer'
    ecoutes   INTEGER,         -- écoutes / streams cumulés, si la source le donne
    auditeurs INTEGER,         -- auditeurs / fans distincts, si la source le donne
    at        INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (mbid, kind, source)
);

-- Ce qui a déjà été demandé, y compris quand la réponse était vide — sans quoi
-- une entité inconnue d'une source serait réinterrogée à chaque passe.
-- `at` sert aussi à la péremption : passé un délai, une actualité vieillit.
CREATE TABLE IF NOT EXISTS popularite_fetched (
    mbid   TEXT NOT NULL,
    kind   TEXT NOT NULL,
    source TEXT NOT NULL,
    at     INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (mbid, kind, source)
);

-- Popularité résolue et normalisée par morceau, prête pour l'affichage.
-- Réécrite en bloc à chaque passe (comme features.cluster) : ce n'est pas une
-- donnée d'entrée mais un calcul dérivé de `popularite` + la distribution.
CREATE TABLE IF NOT EXISTS track_popularite (
    track_id   INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    relative   REAL NOT NULL,   -- 0..1, rang dans la bibliothèque
    echelon    TEXT NOT NULL,   -- 'recording' | 'release-group'
    calcule_le INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
```

## La valeur affichée : un rang, pas un compte

Les comptes bruts s'étalent sur plusieurs ordres de grandeur et chaque source a
une couverture et une échelle propres. En linéaire, « tout le monde vaut zéro
sauf trois artistes » — le même écueil que `reseau.rs` résout déjà en logarithme
pour `effectif`.

`track_popularite.relative` se calcule donc ainsi, sur toute la bibliothèque en
une fois :

1. pour chaque morceau, prendre le meilleur échelon disponible (recording →
   release-group ; rien au-delà — voir « couverture et repli ») ;
2. pour **chaque source**, convertir le compte brut de cet échelon en **rang
   percentile** parmi les morceaux couverts par cette source (rang, pas valeur :
   insensible à l'échelle et aux distributions à longue traîne) ;
3. **mélanger** les rangs des sources disponibles par la **médiane** — robuste
   au désaccord d'une source, et ne suppose pas qu'on sache les pondérer. Avec
   une seule source (ListenBrainz seule si Deezer est écarté par la sonde), la
   médiane est cette source ;
4. le résultat est `relative` ∈ [0, 1] ; un morceau sans aucune source couverte
   n'a pas de ligne dans `track_popularite` (grisé à l'affichage).

Recalculé à chaque passe, comme la projection de la carte : ce n'est pas une
donnée gardée mais une lecture de l'état courant.

## La passe

Sur le patron de `crate::enrichir` (client séparé du réseau, `db` séparé du
rangement, la passe est la seule à connaître l'ordre des opérations).

**Clients** (`crates/core/src/`) — cadencés, `User-Agent` identifiant
l'application comme `musicbrainz.rs` / `listenbrainz.rs` le font déjà :

- `listenbrainz.rs` : ajouter `popularite_recordings(&[&str])` et
  `popularite_release_groups(&[&str])` — POST par lots (60 MBID, comme la sonde)
  sur `api.listenbrainz.org/1/popularity/{recording,release-group}`. Réponse :
  `[{recording_mbid|release_group_mbid, total_listen_count, total_user_count}]`,
  seuls les MBID connus figurent. Aucun compte, aucune clé.
- `deezer.rs` (neuf) : `chercher_piste(artiste, titre) -> Option<u64>` sur
  `api.deezer.com/search/track?q=artist:"…" track:"…"`, qui rend le `rank` du
  premier résultat **dont l'artiste ET le titre normalisés concordent** avec la
  demande (`normaliser_titre` pour le titre, une réduction équivalente pour
  l'artiste — retirer « feat. », accents, ponctuation). Sans cette double
  vérification, ~1 rapprochement sur 40 est un autre morceau du même artiste
  (mesuré phase 0). Pas d'auth, cadence ~7 req/s.

**Orchestration** — `crates/core/src/popularite.rs` (neuf) :

```
pub fn actualiser(
    lib: &mut Library,
    clients: &Clients,          // listenbrainz + deezer
    limite: usize,
    peremption_jours: i64,
    mut avancer: impl FnMut(&Bilan),
) -> Result<Bilan>
```

1. rassembler les MBID en attente, par échelon et par source : ceux jamais
   demandés (`NOT EXISTS` dans `popularite_fetched`) **ou** demandés il y a plus
   de `peremption_jours` — comme `decouvrir_suivi`, pas comme `mb_fetched` qui
   ne périme jamais ;
2. interroger par lots, ranger dans `popularite` + marquer dans
   `popularite_fetched`, **dans la même transaction** (une passe coupée ne perd
   ni ne refait) ;
3. un échec réseau n'interrompt pas la passe : l'entité fautive n'est pas
   marquée et revient au prochain passage (idem `enrichir`) ;
4. en fin de passe, **recalculer `track_popularite` en entier** (voir « la
   valeur affichée »).

**Place dans la chaîne** — `apps/desktop/ui/app.js`, `analyserRacine` et
`lancerChaineComplete` : nouvelle étape après les genres.

- `passeScan` → `passeEmpreintes` → `passeDescripteurs` → `passeGenres` →
  **`passePopularite`** (« Étape 5/5 »).
- La passe popularité **tourne toujours** : ni ListenBrainz ni Deezer ne
  demandent de compte. L'adresse de contact MusicBrainz, si elle est
  renseignée, sert au `User-Agent` de ListenBrainz par courtoisie (comme le
  fait déjà `listenbrainz.rs`), mais son absence ne saute pas l'étape —
  contrairement aux genres.
- Après elle : rien à rafraîchir côté carte ou familles. Seuls
  `chargerStatsBibliotheque` et une éventuelle file ouverte se redessinent.

## Commandes

**Tauri** (`apps/desktop/src/main.rs`), sur le patron de
`start_enrichment` / `enrichment_state` :

- `start_popularite { rafraichir: bool }` — lance le fil de fond ;
- `popularite_state -> EtatPopularite { en_cours, faits, total, resultat }` —
  sondé par `attendreFin`, comme les autres passes ;
- `popularites { ids: Vec<i64> } -> Vec<(i64, Option<f64>, Option<String>)>` —
  lot `(track_id, relative, echelon)` pour les morceaux visibles. **Commande
  séparée**, comme `descripteurs` et `neighbours` : on ne charge pas la
  popularité dans `TrackRow` (7 requêtes à toucher), l'interface la demande
  pour ce qu'elle affiche.

**CLI** (`crates/cli/src/main.rs`), sur le patron de `Decouvrir` :

```
rusty-music popularite [--limite <n>] [--rafraichir-des <jours>]
```

## Rafraîchissement périodique — fait (phase 3)

Une popularité vieillit — mais lentement, et relancer la passe coûte des
minutes (jusqu'à ~2 h côté Deezer). Convention du projet : **ne jamais prendre
l'utilisateur en otage** (cf. l'analyse déclenchée à la main).

- **Pas d'exécution automatique.** À l'entrée du mode Bibliothèque,
  `popularite_fraicheur` rend `(couverts, plus_ancienne_at, perimes)` ;
  `chargerPopulariteFraicheur` n'affiche la ligne d'alerte que si `couverts > 0`
  **et** `perimes > 0` — « Popularité : N entités de plus de 90 jours (la plus
  ancienne remonte à …). [Rafraîchir] ». Sinon, silence.
- **Case « Rafraîchir aussi la popularité de plus de 90 jours »** dans le rail,
  à côté de « Refaire ce qui est déjà mesuré ». Décochée par défaut : la passe
  ne comble que les trous. Cochée : `start_popularite { rafraichir: true }` →
  `depuis = now − 90 j`, et `pop_recordings_candidats` / `pop_deja_fait`
  reprennent tout ce qui dépasse ce délai.
- Le bouton « Rafraîchir » de la ligne d'alerte coche la case et lance la passe
  seule.
- CLI : `--rafraichir-des <jours>` (0 = ne rafraîchit rien).

## L'affichage : la jauge graduée

Un composant `.jauge-pop` — **cinq segments**, `round(relative * 5)` remplis,
teinte muette → accent. Vide (aucun segment, contour seul) quand la popularité
est inconnue : un morceau qui n'est pas encore passé dans la passe n'a pas de
jauge, **jamais une valeur inventée** — même règle que les descripteurs de
l'inspecteur (`montrerDescripteurs`, `app.js`).

Infobulle : le rang en clair (« popularité : élevée »), l'échelon (« mesurée
sur le morceau » / « sur l'album ») et les sources ayant répondu.

**Deux emplacements**, une seule fabrique de composant :

1. **File d'attente** — `dessinerFile` (`app.js:1156`). La ligne passe de
   `rang · texte · durée` à `rang · texte · jauge · durée`. Au rendu de la
   file, un seul appel `popularites` avec tous les `id` visibles, puis on
   distribue.
2. **Liste des pistes d'un album** — `ligne` (`app.js:140`), utilisée par
   `poser("pistes", …)`. Même cellule `.ligne__pop`, même appel groupé à
   l'ouverture de l'album.

CSS dans `apps/desktop/ui/style.css`, près de `.barre-genre` (déjà une barre
graduée) et `.file__ligne`.

## Plan par phases

**Phase 0 — sonde — FAITE (2 septembre 2026).** `experiments/popularite/`
(`aspirer.py` + `rapport.py` + `README.md`, cache versionné). 200 morceaux au
hasard, ListenBrainz par MBID et Deezer par recherche. Résultats :

- **couverture** : ListenBrainz 97 % (enregistrement ou album), uniforme ;
  Deezer piste 80 % ; au moins une source **98,5 %**, donc 1,5 % de morceaux
  grisés ;
- **fiabilité Deezer** : 99 % des rapprochements justes **si artiste + titre
  concordent** (contre ~2 % d'artiste faux + quelques % de mauvais morceau si
  l'on ne vérifie que l'artiste) ;
- **accord** ListenBrainz ↔ Deezer : ρ ≈ 0,55 — modéré, donc complémentaire et
  non redondant. Repli enregistrement → album cohérent (ρ ≈ 0,61).

**Verdict : les deux sources entrent en phase 1.** ListenBrainz comme socle
(suffirait seule) ; Deezer **à l'échelon piste seulement**, match artiste +
titre. Spotify / YouTube non nécessaires. Détail : `experiments/popularite/README.md`.

**Phase 1 — socle — FAITE (2 septembre 2026).**

- Schéma : `popularite`, `popularite_fetched`, `track_popularite` (trois
  `CREATE TABLE IF NOT EXISTS`, aucune migration de colonne).
- `crates/core/src/listenbrainz.rs` : `post_json` + `popularite_enregistrements`
  / `popularite_albums` (POST par lots de 60, `total_listen_count` /
  `total_user_count`).
- `crates/core/src/deezer.rs` (neuf) : `Client::rang_piste(artiste, titre)` —
  `/search/track`, ne retient un résultat que si `cle_artiste` **et**
  `normaliser_titre` concordent (`musicbrainz::cle_artiste` ajouté pour ça).
- `crates/core/src/popularite.rs` (neuf) : `actualiser(...)` sur le patron de
  `enrichir` — reprenable (`popularite_fetched` marqué dans la même
  transaction), tolérante aux coupures, puis `recalculer_track_popularite` en
  fin de passe.
- `crates/core/src/db.rs` : `pop_recordings_candidats`, `pop_rg_candidats`
  (lien morceau → release-group en Rust, comme les genres), `pop_deja_fait`,
  `pop_poser`, `recalculer_track_popularite` (rang percentile par source,
  médiane) ; `PisteAPopulariser`, `PopulariteBrute`, `rangs_percentiles`.
- Tauri : `start_popularite { contact }` / `popularite_state`
  (`EtatPopularite`) — tourne sans clé, `contact` seulement pour le
  `User-Agent`.
- CLI : `rusty-music popularite [--contact] [--limite] [--rafraichir-des]`.
- `app.js` : `passePopularite`, étape **5/5** de `lancerChaineComplete` et
  suite de `analyserRacine` ; `reprendreActualisationEnCours` la reprend.
- Tests : `rangs_percentiles`, `recalcul_track_popularite`,
  `pop_candidats_excluent_ce_qui_est_deja_fait` ; `deezer::concorde`.

Mesuré : ~1 requête/s côté ListenBrainz (lots de 60), ~7/s côté Deezer. La
passe complète sur 27 000 morceaux ≈ 10 min pour ListenBrainz + ~2 h pour
Deezer (une recherche par enregistrement) — du même ordre que la passe de
genres, et reprenable. `--limite` la borne.

**Phase 2 — la jauge — FAITE (2 septembre 2026).**

- `Library::popularites(&[i64]) -> Vec<(id, relative, echelon)>` +
  commande Tauri `popularites { ids }` — lot pour ce qui est visible, séparée
  comme `descripteurs` (rien dans `TrackRow`).
- `app.js` : cache `popParPiste` (`{relative, echelon}` / `null` = pas de
  popularité / absent = pas demandé) ; `chargerPopularites(ids, apres)` —
  une requête pour les ids inconnus, ne repeint que s'il y avait à charger ;
  `jaugePop(pop)` — cinq segments façon indicateur de signal, `round(relative
  × 5)` pleins, **au moins un dès qu'une mesure existe** (sinon « connu et peu
  populaire » = « inconnu »), contour seul si inconnu, infobulle « popularité :
  {mot} · mesurée sur {le morceau|l'album} ».
- Câblé dans `dessinerFile` (file d'attente) et `ligne` (listes de morceaux —
  pistes d'un album *et* résultats de recherche, `vue.quoi === "pistes"`) ;
  `poser` déclenche le chargement, la liste virtualisée relit le cache au
  défilement. `popARecalculee` vide le cache et recharge après une passe.
- CSS `.jauge-pop` près de `.barre-genre` — teintes par variables, thème clair
  et sombre.

Pas de vérification visuelle dans la webview Tauri (extension navigateur non
connectée, et WKWebView ne se pilote pas de l'extérieur) : logique de
`jaugePop` et du cache testée hors interface, tout compile, les tests core
passent. À regarder à l'œil au prochain lancement — jauge et ligne d'alerte.

**Phase 3 — rafraîchissement — FAITE (2 septembre 2026).** (Last.fm ayant été
écarté, la phase Last.fm du plan initial saute ; ceci est l'ex-phase 4.)

- Le paramètre `depuis` de `popularite::actualiser` était déjà en place depuis
  la phase 1 (`0` = combler les trous, `now − N j` = réinterroger le périmé) ;
  `start_popularite` gagne un booléen `rafraichir` qui calcule
  `now − POP_PEREMPTION_JOURS` (90 j).
- `Library::popularite_fraicheur(90) -> (couverts, plus_ancienne_at, perimes)`
  + commande `popularite_fraicheur`.
- `app.js` : case « Rafraîchir aussi la popularité de plus de 90 jours » dans le
  rail (à côté de « Refaire ce qui est déjà mesuré ») — lue par `analyserRacine`
  et `lancerChaineComplete`. `chargerPopulariteFraicheur` : ligne d'alerte à
  l'entrée du mode Bibliothèque, visible **seulement** si de la popularité
  existe et qu'une partie dépasse 90 j, avec un bouton « Rafraîchir » qui coche
  la case et relance la passe. Recalée après chaque passe.
- Tests : `popularite_fraicheur_compte_le_perime`.

Rien d'automatique : la passe reste déclenchée à la main (convention du
projet — cf. l'analyse). La case décochée par défaut ; l'utilisateur choisit de
payer les ~2 h de Deezer.

**Plus tard, hors de ce chantier** :

- Popularité **d'artiste** (échelon `artist`) et son branchement dans la carte —
  profil d'altitude des itinéraires (`reseau.rs`), étagement des étiquettes
  (`carto::tuiles::rang_artiste`) — en remplacement du proxy `effectif`. Touche
  le rendu des tuiles.
- Sources à clé (Last.fm, Discogs, Spotify, YouTube) — seulement si les sources
  ouvertes se révèlent trop lacunaires.

## Réserves

- **Biais de couverture.** ListenBrainz penche prog / metal / indie ; Deezer,
  francophone. Le mélange atténue, il n'efface pas. Phase 0 : ListenBrainz
  reste uniforme (96–98 %) même sur la longue traîne ; Deezer y descend à
  72 %.
- **Rapprochement Deezer.** Sans MBID, le garde-fou est la double concordance
  artiste + titre normalisé — 99 % de justes sur la sonde, mais une erreur
  résiduelle reste possible (un autre morceau du même artiste au titre proche).
- **Pas de compte absolu de streams** : la jauge dit un rang dans la
  bibliothèque, pas un nombre d'écoutes.
- **Granularité limitée à l'album** quand le morceau n'a pas de MBID
  d'enregistrement (8 % des morceaux) : tous les morceaux du disque affichent
  alors la même jauge.
- **Popularité ≠ qualité ni pertinence.** C'est un axe d'affichage et de tri, à
  côté du tempo, de l'année, de la famille — pas un jugement.
