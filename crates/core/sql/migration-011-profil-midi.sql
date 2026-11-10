-- SPDX-License-Identifier: GPL-3.0-or-later
-- Le profil de correspondance d'une batterie électronique (E4, tranche 2,
-- `crate::db::latence`) : quelle note MIDI est quelle pièce, gardé par
-- périphérique pour ne pas refaire l'affectation des pads à chaque séance.
-- Le contenu (`rusty_music_editor::coach::batterie::Profil`, en JSON) est
-- l'affaire du coach ; la base le garde tel quel.
CREATE TABLE profil_midi (
    -- Nom du périphérique MIDI, tel que le système le donne.
    peripherique TEXT    PRIMARY KEY,
    profil       TEXT    NOT NULL CHECK (json_valid(profil)),
    maj          INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
