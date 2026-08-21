-- Schéma de la base locale. Exécuté à chaque ouverture (idempotent).
-- Une seule base alimentée par le dossier surveillé, consommée par les
-- trois modules (lecteur, exploration, éditeur).

PRAGMA journal_mode = WAL;
-- Plusieurs processus partagent la base : l'application, la CLI, une passe
-- d'analyse qui écrit pendant des heures. Sans délai d'attente, la moindre
-- contention rend un échec immédiat au lieu de patienter quelques
-- millisecondes. WAL autorise un rédacteur et des lecteurs simultanés ; ce
-- délai couvre le cas où deux rédacteurs se croisent.
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tracks (
    id            INTEGER PRIMARY KEY,
    path          TEXT    NOT NULL UNIQUE,   -- chemin absolu, clé d'identité
    size_bytes    INTEGER,
    mtime         INTEGER,                   -- epoch s, sert à détecter les modifications
    title         TEXT,
    artist        TEXT,
    album         TEXT,
    album_artist  TEXT,
    genre         TEXT,
    year          INTEGER,
    track_no      INTEGER,
    duration_ms   INTEGER,
    sample_rate   INTEGER,
    channels      INTEGER,
    mb_recording_id TEXT,                    -- MusicBrainz, si présent dans les tags
    -- Identité MusicBrainz des artistes. `mb_artist_id` peut porter plusieurs
    -- valeurs sur une piste « X feat. Y » : c'est `mb_album_artist_id`, unique,
    -- qui sert de clé de regroupement des artistes.
    mb_artist_id       TEXT,
    mb_album_artist_id TEXT,
    added_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    analyzed_at   INTEGER                    -- NULL = pas encore passé dans le pipeline d'analyse
);

CREATE INDEX IF NOT EXISTS idx_tracks_artist   ON tracks(artist);
CREATE INDEX IF NOT EXISTS idx_tracks_album    ON tracks(album);
-- L'index sur mb_album_artist_id est créé par `Library::migrate` et non ici :
-- sur une base antérieure, la colonne n'existe pas encore au moment où ce
-- fichier s'exécute, et `CREATE INDEX` échouerait.
CREATE INDEX IF NOT EXISTS idx_tracks_analyzed ON tracks(analyzed_at);

-- Embeddings et projection 2D (module 2). Un morceau peut avoir plusieurs
-- représentations si on change de modèle : la clé inclut le nom du modèle.
CREATE TABLE IF NOT EXISTS features (
    track_id  INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    model     TEXT    NOT NULL,              -- ex. "musicnn-msd", "clap-2023"
    dim       INTEGER NOT NULL,
    vector    BLOB    NOT NULL,              -- f32 little-endian, dim * 4 octets
    x         REAL,                          -- projection 2D (t-SNE / UMAP)
    y         REAL,
    cluster   INTEGER,
    computed_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (track_id, model)
);

-- Caractéristiques musicales lisibles (filtres de l'interface).
CREATE TABLE IF NOT EXISTS descriptors (
    track_id  INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    bpm       REAL,
    musical_key TEXT,
    energy    REAL,
    loudness  REAL,
    zcr             REAL,
    centroid_mean   REAL,
    centroid_std    REAL,
    rolloff_mean    REAL,
    rolloff_std     REAL,
    flatness_mean   REAL,
    flatness_std    REAL
);

-- Graphe des collaborations entre artistes (MusicBrainz), mode Découvrir.
CREATE TABLE IF NOT EXISTS artist_links (
    src_mbid  TEXT NOT NULL,
    dst_mbid  TEXT NOT NULL,
    -- Un collaborateur externe (pas dans la bibliothèque) n'a pas de ligne
    -- dans `tracks` pour donner son nom — la réponse `artist-rels` de
    -- MusicBrainz le fournit dans le même appel, autant le garder ici.
    dst_name  TEXT,
    relation  TEXT NOT NULL,                 -- "collaborator", "member of", "founder"…
    PRIMARY KEY (src_mbid, dst_mbid, relation)
);

-- Racines surveillées.
CREATE TABLE IF NOT EXISTS roots (
    path        TEXT PRIMARY KEY,
    added_at    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    last_scan   INTEGER
);

-- Fichiers qu'un scan n'a pas su lire (tags illisibles, insertion en échec).
-- Le compte existait déjà dans ScanReport.failed, mais seulement en mémoire
-- le temps de la passe ; sans cette table le détail se perdait dans les logs.
CREATE TABLE IF NOT EXISTS scan_failures (
    path   TEXT PRIMARY KEY,
    reason TEXT NOT NULL,
    at     INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- Paramètres du calcul de la carte (projection t-SNE, clustering k-means) —
-- clé/valeur plutôt que des colonnes : ce sont quatre nombres, pas de quoi
-- justifier un schéma rigide. Une clé absente vaut la valeur par défaut
-- ([`Library::parametres_carte`]) — pas de migration à écrire quand on en
-- ajoute une.
CREATE TABLE IF NOT EXISTS parametres_carte (
    cle    TEXT PRIMARY KEY,
    valeur REAL NOT NULL
);

-- Genres MusicBrainz, aspirés par identifiant.
--
-- Pourquoi une source de plus alors que les fichiers portent déjà un genre :
-- les tags sont saisis à la main, à l'échelle de l'album, et grossiers — ils
-- nommaient « Children's · Pop » la famille de Regina Spektor et Nina Simone.
-- Mesuré sur la bibliothèque de test, MusicBrainz améliore neuf familles sur
-- douze (`experiments/musicbrainz-genres/`).
--
-- Deux échelons. L'artiste couvre le plus de morceaux ; le « release-group »
-- — l'album au sens de MusicBrainz, indépendamment de ses rééditions —
-- l'emporte quand il est connu, pour qu'un disque acoustique d'un groupe
-- électrique soit étiqueté pour ce qu'il est.
CREATE TABLE IF NOT EXISTS mb_genres (
    mbid   TEXT    NOT NULL,
    kind   TEXT    NOT NULL,                 -- 'artist' | 'release-group'
    genre  TEXT    NOT NULL,
    -- MusicBrainz est contributif, donc bruité : un `amapiano` erroné sur un
    -- seul artiste suffisait à nommer une famille. Le nombre de votes sépare
    -- le consensus de l'accident.
    votes  INTEGER NOT NULL,
    PRIMARY KEY (mbid, kind, genre)
);

-- Les albums d'un artiste tels que MusicBrainz les nomme. Nos fichiers ne
-- portent pas d'identifiant d'album : c'est le titre, normalisé, qui fait le
-- lien.
CREATE TABLE IF NOT EXISTS mb_release_groups (
    mbid        TEXT PRIMARY KEY,
    artist_mbid TEXT NOT NULL,
    title       TEXT NOT NULL,
    title_norm  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mb_rg_norm ON mb_release_groups(artist_mbid, title_norm);

-- Ce qui a déjà été demandé, y compris quand la réponse était vide. Sans
-- cette trace, un artiste sans genre serait réinterrogé à chaque passe — et
-- MusicBrainz n'accorde qu'une requête par seconde.
CREATE TABLE IF NOT EXISTS mb_fetched (
    mbid TEXT    NOT NULL,
    kind TEXT    NOT NULL,                   -- 'artist' | 'albums'
    at   INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (mbid, kind)
);
