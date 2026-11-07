-- SPDX-License-Identifier: GPL-3.0-or-later
-- Les balades gardées (`docs/plan-balade.md`, P5) : de quoi tracer les
-- sentiers battus sur la carte, lister les balades passées et les rejouer.
--
-- Un pas est noté quand son morceau commence (`explorer::balade`). Le rang 0
-- est le départ : ni sommets ni chemin. Les sommets sont ceux du graphe de
-- voirie (`rusty_music_carto::reseau_reel::Graphe`), stables tant que le plan
-- de ville n'est pas réimporté ; une balade dans le graphe des voisins (sans
-- ville) n'en a pas.
CREATE TABLE balade (
    id     INTEGER PRIMARY KEY,
    debut  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    -- 'ville' | 'voisins'
    terrain TEXT   NOT NULL
);

CREATE TABLE balade_pas (
    balade         INTEGER NOT NULL REFERENCES balade(id) ON DELETE CASCADE,
    rang           INTEGER NOT NULL,
    morceau        INTEGER NOT NULL,
    sommet_depart  INTEGER,
    sommet_arrivee INTEGER,
    -- Les sommets parcourus, du départ à l'arrivée inclus, séparés par des
    -- virgules : les tronçons des sentiers battus.
    chemin         TEXT,
    -- 1 : la voie a été choisie d'un geste au carrefour ; 0 : la pente.
    choisi         INTEGER NOT NULL DEFAULT 0,
    -- Voies offertes à ce pas (1 : pas de carrefour), pour lire `choisi`.
    voies          INTEGER NOT NULL DEFAULT 1,
    le             INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (balade, rang)
);
