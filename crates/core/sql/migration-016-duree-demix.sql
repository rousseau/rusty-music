-- SPDX-License-Identifier: GPL-3.0-or-later
-- Le temps de démixage sur cette machine, par variante (V40,
-- `crate::db::latence`) : secondes de calcul par seconde de morceau, moyenne
-- glissante des démixages faits. L'estimation affichée au lancement en vient.
CREATE TABLE duree_demix (
    variante TEXT    PRIMARY KEY,
    -- Secondes de calcul (chargement, chauffe, séparation, écriture) par
    -- seconde de morceau.
    facteur  REAL    NOT NULL CHECK (facteur > 0),
    -- Démixages comptés.
    mesures  INTEGER NOT NULL DEFAULT 1,
    maj      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
