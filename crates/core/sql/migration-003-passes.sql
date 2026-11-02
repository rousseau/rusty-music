-- SPDX-License-Identifier: GPL-3.0-or-later
-- Le journal des passes « Compléter » (`Library::ouvrir_passe`) : chaque
-- ligne qu'une passe ajoute, modifie ou retire dans les tables de cache de
-- l'enrichissement est notée avec sa valeur d'avant, pour qu'« Annuler cette
-- passe » rejoue tout à l'envers. Rien de ce journal ne touche aux fichiers :
-- tout ce qu'une passe écrit est en base.
CREATE TABLE passe (
    id         INTEGER PRIMARY KEY,
    nom        TEXT    NOT NULL,              -- « completer »
    debut      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    fin        INTEGER,                       -- NULL tant qu'elle tourne (ou si elle a été coupée)
    annulee_le INTEGER,
    bilan      TEXT
);

CREATE TABLE passe_ecriture (
    rang       INTEGER PRIMARY KEY,           -- l'ordre des écritures, rejoué à l'envers
    passe      INTEGER NOT NULL REFERENCES passe(id) ON DELETE CASCADE,
    table_nom  TEXT    NOT NULL,
    operation  TEXT    NOT NULL,              -- 'insert' | 'update' | 'delete'
    cle        TEXT    NOT NULL,              -- clé primaire de la ligne (JSON)
    avant      TEXT                           -- la ligne d'avant (JSON), NULL pour un ajout
);
CREATE INDEX idx_passe_ecriture ON passe_ecriture(passe, rang);
