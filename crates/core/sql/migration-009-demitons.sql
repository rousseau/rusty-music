-- SPDX-License-Identifier: GPL-3.0-or-later
-- Une note transcrite se corrige aussi au demi-ton (V18, `crate::db::pratique`) :
-- genre 'demiton', de combien de demi-tons la note bouge (−11 à +11), en plus
-- d'une éventuelle octave corrigée. Avant, seule une note ajoutée changeait de
-- hauteur au demi-ton (reposée à la nouvelle hauteur).
ALTER TABLE pratique_correction ADD COLUMN demitons INTEGER;
