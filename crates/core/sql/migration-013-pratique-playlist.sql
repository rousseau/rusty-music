-- SPDX-License-Identifier: GPL-3.0-or-later
-- La playlist que Pratiquer enchaîne (P2, `crate::playlists`) : une seule à
-- la fois, gardée d'une session à l'autre. Le morceau en cours se retient
-- par son **chemin**, comme les pistes d'une playlist : le rang se déduit de
-- la liste telle qu'elle est à la reprise, retouchée ou non entre-temps.
CREATE TABLE pratique_playlist (
    -- Une ligne au plus.
    un           INTEGER PRIMARY KEY CHECK (un = 1),
    playlist_id  INTEGER NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
    -- Le morceau travaillé ; NULL : le premier de la liste.
    chemin       TEXT,
    maj          INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
