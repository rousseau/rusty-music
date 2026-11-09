-- SPDX-License-Identifier: GPL-3.0-or-later
-- La latence calibrée du coach de Pratiquer (E4, `crate::db::latence`) : une
-- par périphérique et par chemin, hors du morceau. Le chemin MIDI (batterie
-- électronique) et le chemin audio (basse en DI) n'ont pas la même latence ;
-- un même nom de périphérique peut porter les deux.
CREATE TABLE latence (
    -- Nom du périphérique d'entrée, tel que le système le donne.
    peripherique  TEXT    NOT NULL,
    -- 'audio' | 'midi'.
    chemin        TEXT    NOT NULL CHECK (chemin IN ('audio', 'midi')),
    -- Médiane des écarts frappe − clic, en millisecondes de l'horloge du
    -- moteur : sortie, casque, geste et entrée compris.
    latence_ms    REAL    NOT NULL,
    -- Écart-type robuste (1,4826 × écart absolu médian) des frappes retenues.
    dispersion_ms REAL,
    -- Frappes retenues pour la médiane.
    frappes       INTEGER NOT NULL,
    maj           INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (peripherique, chemin)
);
