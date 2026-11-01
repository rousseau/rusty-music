-- SPDX-License-Identifier: GPL-3.0-or-later
-- L'édition MusicBrainz retenue pour un album (« Choisir l'édition… »), et ce
-- qu'elle donne à chaque piste. Les tags ne sont jamais réécrits : `upsert`
-- préfère ces valeurs à celles des tags tant que le choix existe.
CREATE TABLE edition_choisie (
    album        TEXT    NOT NULL,
    artiste      TEXT    NOT NULL DEFAULT '',
    release_id   TEXT    NOT NULL,
    titre        TEXT    NOT NULL,
    choisie_le   INTEGER NOT NULL,
    PRIMARY KEY (album, artiste)
);

CREATE TABLE edition_piste (
    path                TEXT PRIMARY KEY,
    album               TEXT NOT NULL,
    artiste             TEXT NOT NULL DEFAULT '',
    release_id          TEXT NOT NULL,
    recording_id        TEXT,
    artist_mbid         TEXT,
    album_artist_mbid   TEXT
);
CREATE INDEX edition_piste_album ON edition_piste (album, artiste);
