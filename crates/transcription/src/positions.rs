// SPDX-License-Identifier: GPL-3.0-or-later
//! A priori de position du doigté : le coût (− log de la fréquence, 0 pour
//! la plus courante) de jouer une note à telle frette sur telle corde, comptée
//! depuis l'aiguë. Appris sur 88663 notes de 162 tablatures de
//! référence (31 livres « Bass Recorded Versions », 131 parties de
//! basse Songsterr) — seuls ces comptes agrégés sont gardés.
//!
//! Écrit par `experiments/partitions/apprendre_positions.py` ; ne pas
//! modifier à la main.

/// Frettes de la table (0 à 20 ; au-delà, la dernière ligne vaut).
pub const FRETTES: usize = 21;
/// Cordes de la table, depuis l'aiguë (au-delà, la dernière vaut).
pub const CORDES: usize = 6;

/// `POSITIONS[corde depuis l'aiguë][frette]`.
pub const POSITIONS: [[f32; FRETTES]; CORDES] = [
    [4.16, 6.61, 3.99, 4.91, 3.97, 3.69, 4.65, 3.5, 4.69, 3.51, 4.25, 4.95, 4.69, 6.54, 5.42, 7.93, 6.75, 7.01, 7.71, 5.6, 7.93],
    [2.62, 3.52, 1.97, 2.47, 1.88, 1.79, 3.38, 1.65, 3.71, 2.89, 3.15, 5.1, 3.91, 5.22, 4.36, 6.06, 7.71, 4.88, 7.71, 5.79, 8.22],
    [1.03, 1.62, 1.13, 0.888, 1.31, 0.404, 2.61, 1.01, 2.18, 2.61, 2.89, 3.1, 2.57, 5.85, 5.25, 4.38, 8.22, 5.0, 6.1, 4.53, 6.27],
    [0.0, 1.49, 1.17, 0.415, 1.65, 0.815, 2.12, 1.24, 1.81, 3.9, 3.07, 4.87, 3.09, 6.48, 7.24, 4.17, 6.32, 4.8, 5.98, 6.92, 8.62],
    [6.1, 6.83, 5.95, 4.49, 6.92, 4.5, 5.27, 5.35, 5.76, 6.27, 7.93, 9.32, 8.62, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32],
    [6.54, 3.24, 5.92, 5.27, 7.93, 6.37, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32, 9.32],
];
