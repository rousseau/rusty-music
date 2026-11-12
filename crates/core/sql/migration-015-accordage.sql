-- SPDX-License-Identifier: GPL-3.0-or-later
-- L'accordage de la tablature de basse choisi à la main pour un morceau (V37,
-- `crate::db::pratique`) : 'standard', 'demi-ton', 'ton', 'drop-d', 'cinq'
-- (`rusty_music_transcription::tablature::Accord`). NULL : celui que les notes
-- transcrites réclament. La mémoire de Pratiquer (`garder_pratique`) ne le
-- touche pas : il a sa méthode.
ALTER TABLE pratique ADD COLUMN accordage TEXT;
