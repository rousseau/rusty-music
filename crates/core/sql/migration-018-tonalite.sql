-- SPDX-License-Identifier: GPL-3.0-or-later
-- La tonalité d'ensemble de Pratiquer, en demi-tons, par morceau (V42,
-- `crate::db::pratique`) : un morceau enregistré un demi-ton sous le
-- diapason se joue à −0,5 sans que le suivant le soit. NULL : 0.
ALTER TABLE pratique ADD COLUMN tonalite REAL;
