-- SPDX-License-Identifier: GPL-3.0-or-later
-- La pente des écarts dans la table par mesure du coach (E4, tranche 3,
-- `crate::db::pratique`) : la régression de l'écart signé sur la position
-- dans la mesure, en millisecondes par mesure. Négative, le musicien presse
-- (il finit la mesure plus en avance qu'il ne l'a commencée) ; positive, il
-- traîne. Lissée d'un passage à l'autre comme l'écart moyen et l'écart-type.
ALTER TABLE pratique_mesure ADD COLUMN pente_ms REAL;
