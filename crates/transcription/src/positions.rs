// SPDX-License-Identifier: GPL-3.0-or-later
//! A priori de position du doigté : le coût (− log de la fréquence, 0 pour
//! la plus courante) de jouer une note à telle frette sur telle corde, comptée
//! depuis l'aiguë. Appris sur 190850 notes de 331 tablatures de
//! référence (31 livres « Bass Recorded Versions », 300 parties de
//! basse Songsterr) — seuls ces comptes agrégés sont gardés.
//!
//! Écrit par `experiments/partitions/apprendre_positions.py` ; ne pas
//! modifier à la main.

/// Frettes de la table (0 à 20 ; au-delà, la dernière ligne vaut).
pub const FRETTES: usize = 21;
/// Cordes de la table, depuis l'aiguë (au-delà, la dernière vaut).
pub const CORDES: usize = 6;

/// `POSITIONS[corde depuis l'aiguë][frette]`.
#[allow(clippy::approx_constant)] // coûts appris : 6.28 n'est pas τ
pub const POSITIONS: [[f32; FRETTES]; CORDES] = [
    [3.88, 5.92, 3.42, 4.13, 3.55, 3.07, 3.91, 3.42, 4.75, 3.55, 4.37, 5.29, 4.95, 7.07, 4.53, 7.58, 5.39, 5.34, 8.45, 5.59, 8.27],
    [2.44, 3.0, 1.99, 2.3, 1.98, 1.68, 3.06, 1.87, 3.49, 2.87, 3.43, 4.74, 3.96, 5.62, 3.79, 5.72, 8.45, 4.84, 8.45, 5.67, 8.96],
    [1.22, 1.91, 1.19, 0.779, 1.5, 0.469, 2.26, 0.925, 2.28, 2.84, 3.19, 3.62, 2.97, 6.19, 5.83, 4.44, 8.68, 4.47, 6.84, 5.27, 7.02],
    [0.0, 1.4, 1.18, 0.368, 1.74, 0.653, 1.9, 1.31, 1.96, 3.45, 2.67, 3.81, 3.48, 5.18, 5.84, 4.25, 5.59, 5.43, 4.48, 6.48, 8.12],
    [3.71, 3.61, 5.07, 3.52, 4.47, 4.35, 4.58, 6.0, 6.28, 6.88, 8.27, 8.96, 8.12, 7.67, 6.11, 5.87, 7.42, 10.1, 8.68, 10.1, 10.1],
    [7.23, 3.97, 4.86, 4.85, 7.12, 7.07, 7.17, 9.37, 10.1, 7.98, 10.1, 10.1, 9.37, 10.1, 4.71, 5.3, 7.67, 10.1, 10.1, 10.1, 10.1],
];
