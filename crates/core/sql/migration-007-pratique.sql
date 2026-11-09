-- SPDX-License-Identifier: GPL-3.0-or-later
-- La mémoire de Pratiquer, par morceau (`docs/ui-spec-editeur.md`, question
-- tranchée le 9 nov. 2026 ; `crate::db::pratique`). Créer n'y écrit rien : ses
-- stems sur le disque sont sa seule persistance (décision 3).
--
-- Clé : le chemin du fichier, comme le cache de stems (`dossier_stems`) et
-- l'édition retenue (`edition_piste`) : elle survit à un rescan et à une
-- réanalyse, que l'identifiant de piste ne traverse pas toujours.
CREATE TABLE pratique (
    path             TEXT PRIMARY KEY,
    -- 'bass' | 'drums' : la partie lue au centre.
    instrument       TEXT,
    -- 'score+tabs' | 'tabs' (basse).
    notation         TEXT,
    -- Mesures de décompte : 0, 1 ou 2.
    decompte         INTEGER,
    -- Variante de démixage des stems de ce morceau ('bsrofo_sw', 'htdemucs'…),
    -- quand elle est connue.
    variante         TEXT,
    -- Vitesse d'ensemble laissée (1 = tempo d'origine).
    vitesse          REAL,
    -- La plus haute vitesse à laquelle une boucle a été jouée jusqu'au bout.
    vitesse_atteinte REAL,
    -- Longueur des boucles posées au clavier, et « accélérer à chaque passage ».
    mesures_boucle   INTEGER,
    accel_boucle     INTEGER NOT NULL DEFAULT 0,
    -- Où l'on s'est arrêté, en secondes du morceau d'origine.
    position_s       REAL,
    maj              INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

-- Les boucles du morceau, bornes en secondes du morceau **d'origine** (ni
-- étirées ni décalées par la vitesse). `nom` NULL : la boucle posée quand on a
-- quitté le morceau, au plus une ; nommée : gardée pour y revenir.
CREATE TABLE pratique_boucle (
    path             TEXT    NOT NULL REFERENCES pratique(path) ON DELETE CASCADE,
    rang             INTEGER NOT NULL,
    nom              TEXT,
    debut_s          REAL    NOT NULL,
    fin_s            REAL    NOT NULL,
    -- Nombre de mesures ; NULL pour une boucle posée sur une section.
    mesures          INTEGER,
    vitesse_atteinte REAL,
    PRIMARY KEY (path, rang)
);

-- Place pour les corrections de transcription (E2, `plan-editer-pratique-
-- creation.md` 1.5) : corde imposée, octave, note supprimée ou ajoutée. Vide
-- tant que l'éditeur de correction n'existe pas ; la note visée se retrouve
-- par son instant et sa hauteur transcrits, le modèle ne la réécrit plus.
CREATE TABLE pratique_correction (
    id         INTEGER PRIMARY KEY,
    path       TEXT    NOT NULL REFERENCES pratique(path) ON DELETE CASCADE,
    instrument TEXT    NOT NULL,
    -- 'corde' | 'octave' | 'supprimer' | 'ajouter'
    genre      TEXT    NOT NULL,
    debut_s    REAL    NOT NULL,
    hauteur    INTEGER,
    duree_s    REAL,
    corde      INTEGER,
    frette     INTEGER,
    le         INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
CREATE INDEX pratique_correction_morceau ON pratique_correction (path, instrument);

-- Place pour la table par mesure du coach (E4, `recherche-coach-pratique.md`,
-- « une table par mesure ») : par mesure de la grille de Beat This!, ce que
-- les passages ont donné, et le score d'erreur lissé d'une séance à l'autre.
-- Vide tant que le coach n'existe pas.
CREATE TABLE pratique_mesure (
    path         TEXT    NOT NULL REFERENCES pratique(path) ON DELETE CASCADE,
    instrument   TEXT    NOT NULL,
    mesure       INTEGER NOT NULL,
    passages     INTEGER NOT NULL DEFAULT 0,
    attendues    INTEGER NOT NULL DEFAULT 0,
    attrapees    INTEGER NOT NULL DEFAULT 0,
    manquees     INTEGER NOT NULL DEFAULT 0,
    en_trop      INTEGER NOT NULL DEFAULT 0,
    fausses      INTEGER NOT NULL DEFAULT 0,
    ecart_moy_ms REAL,
    ecart_et_ms  REAL,
    score        REAL,
    maj          INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (path, instrument, mesure)
);
