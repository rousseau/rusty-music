-- SPDX-License-Identifier: GPL-3.0-or-later
-- Le canal d'une interface audio où joue la basse (V34, `crate::db::latence`) :
-- une Scarlett Solo a le micro sur l'entrée 1 et l'instrument (Hi-Z) sur la 2.
-- Gardé par interface, comme la latence : choisi à la main, ou trouvé par le
-- coach (calibration, ou basse entendue sur un autre canal).
CREATE TABLE canal_entree (
    -- Nom de l'interface, tel que le système le donne.
    peripherique TEXT    PRIMARY KEY,
    -- Le canal retenu, compté depuis 0 (l'entrée 1 de l'interface).
    canal        INTEGER NOT NULL CHECK (canal >= 0),
    -- 1 : choisi à la main ; 0 : trouvé par le coach (« auto »).
    manuel       INTEGER NOT NULL DEFAULT 0 CHECK (manuel IN (0, 1)),
    maj          INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
