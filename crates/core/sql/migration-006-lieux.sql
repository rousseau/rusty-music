-- SPDX-License-Identifier: GPL-3.0-or-later
-- Des lieux à soi (`docs/plan-balade.md`, P8) : épingles et chaînes posées sur
-- la carte par l'utilisateur. Rien n'y désigne un sommet de voirie, une tuile
-- ou un identifiant de morceau : des coordonnées et des chemins de fichiers,
-- qui survivent à une régénération des tuiles, à une réanalyse et à un
-- rescan (`crate::lieux`).
CREATE TABLE epingle (
    id       INTEGER PRIMARY KEY,
    nom      TEXT    NOT NULL,
    couleur  TEXT    NOT NULL,              -- `#RRGGBB`
    emoji    TEXT,                          -- facultatif, un seul caractère affiché
    lon      REAL    NOT NULL,
    lat      REAL    NOT NULL,
    creee_le INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- Une chaîne **est** une playlist (ses morceaux, par chemin), avec en plus un
-- tracé et une couleur. Supprimer la playlist supprime la chaîne.
CREATE TABLE chaine (
    playlist_id INTEGER PRIMARY KEY REFERENCES playlist(id) ON DELETE CASCADE,
    couleur     TEXT    NOT NULL,
    -- Le tracé, `[[lon, lat], …]` (JSON) ; vide quand il n'y a pas de plan de
    -- ville : la carte relie alors les morceaux.
    trace       TEXT    NOT NULL DEFAULT '[]',
    -- 'balade' | 'itineraire'
    origine     TEXT    NOT NULL,
    creee_le    INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
