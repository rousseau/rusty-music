-- SPDX-License-Identifier: GPL-3.0-or-later
-- La fenêtre la plus représentative d'un morceau, parmi celles que la passe
-- d'empreintes analyse (`rusty_music_analysis::passe::fenetre_representative`) :
-- son rang, de 0 (15 % du morceau) à 4 (85 %). L'aura de la carte en tire un
-- extrait de 15 s, décodé à la volée (`docs/plan-balade.md`, P4). NULL tant
-- qu'elle n'est pas connue : l'aura prend alors la fenêtre centrale.
ALTER TABLE features ADD COLUMN fenetre INTEGER;
