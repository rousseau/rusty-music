// SPDX-License-Identifier: GPL-3.0-or-later
//! Une basse joue une note à la fois. Basic Pitch, lui, est polyphonique : sur
//! un stem de basse, il rend souvent l'harmonique à l'octave par-dessus la
//! fondamentale, ou une note fantôme laissée par la grosse caisse.
//!
//! Règle : deux notes qui commencent ensemble (moins de 50 ms d'écart) — on
//! garde la plus forte, et à force égale (à 20 % près) la plus grave, puisque
//! l'erreur courante est l'harmonique d'octave. Une note qui commence pendant
//! une autre coupe la précédente : le bassiste a quitté la première pour
//! jouer la seconde.

use crate::Note;

/// Écart sous lequel deux attaques sont simultanées, en secondes.
const SIMULTANE_S: f32 = 0.05;

pub fn monophonique(mut notes: Vec<Note>) -> Vec<Note> {
    notes.sort_by(|a, b| a.debut_s.total_cmp(&b.debut_s));
    let mut sortie: Vec<Note> = Vec::with_capacity(notes.len());
    for n in notes {
        let Some(derniere) = sortie.last_mut() else {
            sortie.push(n);
            continue;
        };
        if n.debut_s - derniere.debut_s < SIMULTANE_S {
            if preferer(&n, derniere) {
                *derniere = n;
            }
            continue;
        }
        if derniere.fin_s > n.debut_s {
            derniere.fin_s = n.debut_s;
        }
        sortie.push(n);
    }
    sortie
}

/// `a` l'emporte-t-elle sur `b`, attaquées ensemble ?
fn preferer(a: &Note, b: &Note) -> bool {
    let proches = (a.amplitude - b.amplitude).abs() <= 0.2 * a.amplitude.max(b.amplitude);
    if proches {
        a.hauteur < b.hauteur
    } else {
        a.amplitude > b.amplitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(debut: f32, fin: f32, hauteur: u8, amplitude: f32) -> Note {
        Note { debut_s: debut, fin_s: fin, hauteur, amplitude, corde: None, frette: None }
    }

    #[test]
    fn l_harmonique_d_octave_cede_a_la_fondamentale() {
        let m = monophonique(vec![n(1.0, 2.0, 52, 0.6), n(1.01, 2.0, 40, 0.55)]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].hauteur, 40);
    }

    #[test]
    fn la_plus_forte_l_emporte_quand_l_ecart_est_net() {
        let m = monophonique(vec![n(1.0, 2.0, 40, 0.3), n(1.02, 2.0, 45, 0.8)]);
        assert_eq!(m[0].hauteur, 45);
    }

    #[test]
    fn une_note_coupe_la_precedente() {
        let m = monophonique(vec![n(1.0, 2.0, 40, 0.6), n(1.5, 2.5, 43, 0.6)]);
        assert_eq!(m.len(), 2);
        assert!((m[0].fin_s - 1.5).abs() < 1e-6);
    }
}
