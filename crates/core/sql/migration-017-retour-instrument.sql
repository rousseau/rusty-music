-- SPDX-License-Identifier: GPL-3.0-or-later
-- Le retour de l'instrument pendant la pratique (E7, `crate::db::latence`) :
-- le son choisi et les volumes, par instrument (« basse » pour l'instant).
CREATE TABLE retour_instrument (
    instrument TEXT PRIMARY KEY,
    -- Nom du préréglage (`brut`, `rond`, `funky`…).
    prereglage TEXT NOT NULL,
    -- Volume de l'instrument, 0 à 1.
    volume     REAL NOT NULL CHECK (volume >= 0 AND volume <= 1),
    -- Balance morceau / instrument, −1 (morceau seul) à 1 (instrument seul).
    balance    REAL NOT NULL DEFAULT 0 CHECK (balance >= -1 AND balance <= 1),
    maj        INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
