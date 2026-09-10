# Nommer les familles : vocabulaire Wikipédia + vote à trois sources

## Contexte

`docs/suite.md` §7 documente un essai (`experiments/clap-texte/`) : nommer
les 12 familles acoustiques par un vocabulaire CLAP-texte de ~74 phrases
choisies à la main marche mieux que les genres MusicBrainz sur 7 familles sur
12, égale sur 3, mais se trompe franchement sur 2 — ska français → « an
african percussion ensemble », chant breton a cappella → « a man rapping in
french ». Cause : **une famille que rien dans le vocabulaire ne décrit ne
reste pas sans nom, elle en reçoit un faux**, et élargir le vocabulaire à la
main ne suffit pas — chaque ajout redistribue les scores (`part ×
log₂(sur-représentation)` récompense la phrase rare).

Le plan cité par `docs/suite.md` (miner des citations dans les critiques
CritiqueBrainz pour élargir le vocabulaire) est abandonné : couverture trop
faible dans la bibliothèque, et surtout mauvais niveau de granularité — les
critiques décrivent des albums, le nommage a besoin de décrire des styles.

Ce chantier le remplace par trois volets qui se renforcent, plutôt qu'une
seule autorité :

1. un vocabulaire CLAP construit **depuis les genres MusicBrainz réels de la
   bibliothèque**, chacun enrichi par son article Wikipédia, plutôt que
   choisi à la main ;
2. un **vote entre trois sources indépendantes** (MusicBrainz, CLAP-texte,
   Last.fm) plutôt qu'une autorité unique ;
3. un **affinage local, suggéré jamais automatique**, pour les familles où
   les trois sources divergent fortement.

**Contrainte de portée** : `features.cluster` (0-11) reste un identifiant
stable et fixe — lu par `crates/carto/src/palette.rs`
(`Palette.familles: [&str; 12]`), le partitionnement en quartiers de la
carte, les bandes de l'anneau et de la frise. Ce chantier ne touche ni la
tour audio ni ces identifiants : seul le **libellé** d'une famille change,
jamais son périmètre de morceaux ni son numéro.

## 1. Le vocabulaire, construit depuis Wikipédia

Trois décisions prises pendant la conception (le seuil et l'approche de
réécriture, confirmées avec l'utilisateur) :

- **genres candidats** : ceux de `mb_genres` portés par au moins **200**
  entités MusicBrainz distinctes (artiste ou release-group) — ~87 genres
  mesurés sur la bibliothèque réelle (832 genres distincts au total, une
  longue traîne très concentrée : `rock` sur 18 245 entités). La commande
  `rusty-music genres-candidats --seuil 200` les liste, un par ligne.
- **réécriture** : un modèle Ollama local (`qwen2.5:3b` par défaut), prompté
  avec les exemples de l'essai — un mot nu (« celtic ») est un mauvais
  prompt CLAP, une phrase descriptive (« traditional celtic music with
  fiddle and flute ») réussit. Pas de repli mécanique : un genre sans
  réécriture exploitable est omis, pas mal nommé.
- **paragraphe source** : le paragraphe de tête (`exintro`) de l'article
  Wikipédia le plus probable, résolu par recherche (pas un titre deviné) —
  c'est là qu'un article de genre résume texture, instrumentation, rythme.

Pipeline complet : `experiments/clap-texte/preparer_vocabulaire.py`. Il
réutilise `sonder.py` (`tour_texte`, `PROMPT`) pour l'embarquement des
phrases — la tour texte de CLAP (RoBERTa, 501 Mo) ne sert **qu'ici, hors
ligne** ; rien de ce script ne tourne à l'exécution de l'application.

Sortie : `crates/analysis/vocabulaire/vocabulaire.bin` (N × 512 `f32`
petit-boutien) + `vocabulaire.txt` (`genre\tphrase` par ligne) — commités,
quelques centaines de Ko, pas les modèles lourds de `models/`.

**Rien ne sort d'`experiments/clap-texte/` vers le workspace.** C'est la
coupe retenue : seul le fichier produit (la table) entre dans
`crates/analysis/`, jamais le code Python ni un modèle. Le calibrage
(centrage par colonne, pas de réduction par écart-type — la réduction
sur-corrige, mesuré sur Metallica dans l'essai) ne se fait **pas** à la
génération de la table : il se fait à l'usage, contre le score réel des
morceaux (`crates/analysis/src/vocabulaire_texte.rs::voter`), exactement
comme `sonder.py calibrer` le fait dans l'essai.

## 2. Le vote à trois sources

`crates/core/src/db.rs::nommer_les_familles` (score `part ×
log₂(sur-représentation)`, déjà en place) est appelé **trois fois**, une
par source, sans y toucher :

- **MusicBrainz** — comme avant ce chantier (`genres_du_morceau` : album,
  puis artiste, puis tag de fichier).
- **CLAP-texte** — calculé en lot par `crates/analysis` (le calibrage a
  besoin du score de tous les morceaux contre tout le vocabulaire avant de
  centrer chaque colonne), persisté dans `features.texte_label` au même
  endroit que `cluster`, lors de `passe::projeter_tout`. Absent tant que le
  vocabulaire n'a pas été généré (MusicBrainz et Last.fm votent alors
  seuls).
- **Last.fm** — `artist.getTopTags` par MBID d'artiste, jamais par nom.
  Nouvelle source d'enrichissement, même patron « données + déjà-demandé »
  que les trois autres (`docs/enrichissement-lecteur.md`) :
  `lastfm_tags`/`lastfm_fetched`, client `crates/core/src/lastfm.rs`, passe
  `crates/core/src/lastfm_pass.rs`. **Sert le nommage des familles, pas la
  popularité** — `docs/popularite.md` avait écarté Last.fm pour cet autre
  usage, faute de clé gratuite ; ici la clé est demandée à l'utilisateur
  (gratuite, `last.fm/api`), sans clé de test partagée.

**Règle de combinaison** (`crates/core/src/db.rs::arbitrer_vote`),
volontairement simple :

- **au moins deux des trois libellés de tête s'accordent** (comparaison
  normalisée : minuscules, ponctuation ignorée) → fiable, ce libellé est
  affiché (casse MusicBrainz quand elle est du bon côté de l'accord) ;
- **les trois divergent** → une critique CritiqueBrainz déjà disponible
  pour un album de la famille peut confirmer l'une des trois propositions
  (sous-chaîne insensible à la casse) — signal complémentaire, jamais une
  source à égalité : une famille doit pouvoir être bien nommée sans
  qu'aucune critique n'existe pour elle ;
- sinon, repli sur le nom MusicBrainz seul — le filet de sécurité déjà en
  place avant ce chantier, pour que la carte garde toujours un nom.

`Library::familles(model)` (signature et forme du triplet inchangées — les
cinq points de lecture de `app.js`, tous alimentés par `invoke("families")`)
reste un mince projeté de `Library::familles_votees(model)`, qui rend le
détail complet (`VoteFamille` : `fiable`, `nom_affiche`, et les trois
propositions).

## 3. L'inspecteur : « nom incertain », et l'affinage suggéré

Nouveau bloc « Famille » dans l'inspecteur (`apps/desktop/ui/app.js`,
`montrerNomFamille`) — pas selon le patron piste-par-piste de
`montrerBio`/`montrerCredits`/`montrerCritiques` : le vote est **par
famille**, chargé une fois par `chargerFamilles()` et mis en cache
(`carte.familleVotes`), consulté par numéro de famille plutôt que redemandé
à chaque piste inspectée.

- `fiable` → une ligne, le nom.
- sinon → « Nom incertain », les trois propositions listées séparément,
  jamais fondues en une ligne, et un bouton « Affiner cette famille ? ».

**Affinage, suggéré jamais automatique.** Au clic, `subdiviser_famille`
(commande Tauri) :

- restreint les empreintes à la seule famille concernée
  (`Library::embeddings_du_cluster`) ;
- les redécoupe par un k-means simple (`crates/analysis/src/
  cluster.rs::subdiviser`, k = 3 par défaut) ;
- nomme chaque sous-groupe par le même vote à trois sources
  (`Library::sous_groupes_votes`, qui partage tout le cœur du vote avec
  `familles_votees` via `nommer_par_groupe`).

**Rien n'est écrit dans `features.cluster`.** Les identifiants de
sous-groupe rendus n'ont de sens que pour cet appel — jamais un territoire
de carte, une bande d'anneau ou de frise. Les onze autres familles et tout
ce qui dépend déjà du découpage à 12 restent strictement intacts.

## Fichiers

- `experiments/clap-texte/preparer_vocabulaire.py` — pipeline hors ligne
  (Wikipédia + Ollama + embarquement), réutilise `sonder.py`.
- `crates/analysis/vocabulaire/` — table produite, commitée.
- `crates/analysis/src/vocabulaire_texte.rs` — chargement + vote en lot
  (calibrage centré).
- `crates/analysis/src/passe.rs::projeter_tout` — appelle le vote CLAP-texte
  et persiste `features.texte_label`.
- `crates/analysis/src/cluster.rs::subdiviser` — k-means restreint, pour
  l'affinage local.
- `crates/core/src/lastfm.rs`, `src/lastfm_pass.rs` — client et passe
  Last.fm.
- `crates/core/src/db.rs` — `VoteFamille`, `familles_votees`,
  `sous_groupes_votes`, `nommer_par_groupe`, `arbitrer_vote`,
  `genres_candidats`, `lastfm_tags`, migration `features.texte_label`.
- `crates/cli/src/main.rs` — `rusty-music genres-candidats`,
  `rusty-music lastfm`.
- `apps/desktop/src/main.rs` — `start_lastfm`/`lastfm_state`,
  `families_detail`, `famille_piste`, `subdiviser_famille`.
- `apps/desktop/ui/index.html`/`app.js` — case + clé Last.fm dans le rail
  d'analyse, bloc « Famille » de l'inspecteur.

## Ce qui reste à faire, à la main

Le vocabulaire vise « quelques dizaines à une centaine de phrases » — pas un
problème de volume. Si Wikipédia décrit mal un style après ce pipeline, la
réponse n'est pas un algorithme plus élaboré : `preparer_vocabulaire.py`
imprime en fin d'exécution les genres omis (Wikipédia ou Ollama n'ont rien
donné d'exploitable) — ce sont les phrases à écrire à la main.
