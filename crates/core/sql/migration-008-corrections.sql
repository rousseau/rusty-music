-- SPDX-License-Identifier: GPL-3.0-or-later
-- Les corrections de transcription (E2, `plan-editer-pratique-creation.md`
-- 1.5 ; `crate::db::pratique`). La table vient de la migration n° 7 ; une
-- octave corrigée dit de combien d'octaves la note bouge (−2 à +2), ce que ses
-- colonnes ne portaient pas. `hauteur` reste la hauteur **transcrite** de la
-- note visée (son identité, avec `debut_s`) ; pour une note ajoutée, la sienne.
ALTER TABLE pratique_correction ADD COLUMN octaves INTEGER;
