-- SPDX-License-Identifier: GPL-3.0-or-later
-- Le tempo que Deezer donne d'un enregistrement (`/track/{id}`, champ `bpm`),
-- gardé pour le comparer à notre mesure (`descriptors.bpm`) :
-- `docs/recherche-enrichissement-croise.md`, étape 1. Deezer le rend souvent
-- doublé (Karma Police à 151) et parfois à 0 : 0 est rangé NULL.
--
-- Une ligne = déjà demandé, même sans réponse : un tempo ne change pas, la
-- demande n'est jamais refaite (contrairement à `popularite_fetched`).
-- `deezer_id` NULL : la recherche n'a rien rendu qui concorde.
CREATE TABLE IF NOT EXISTS bpm_deezer (
    mbid      TEXT    PRIMARY KEY,   -- MBID d'enregistrement
    deezer_id INTEGER,
    bpm       REAL,
    at        INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
