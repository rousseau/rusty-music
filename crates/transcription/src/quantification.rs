// SPDX-License-Identifier: GPL-3.0-or-later
//! Des notes en secondes à une partition : chaque attaque est aimantée sur la
//! grille des temps **mesurés** (Beat This!), à la double croche, puis le tout
//! est découpé en mesures sur les premiers temps. Le tempo peut dériver : la
//! grille suit les temps détectés, pas un tempo moyen.
//!
//! Choix du prototype, à reprendre avec un vrai banc :
//! - grille de doubles croches seulement (pas encore de triolets) ;
//! - une note dure jusqu'à sa fin ou jusqu'à l'attaque suivante ;
//! - ce qui précède le premier premier temps (l'anacrouse) est ignoré ;
//! - une note qui franchit une barre est coupée et liée.

use serde::Serialize;

use crate::Note;

/// Doubles croches par temps.
pub const DIVISIONS: u32 = 4;

/// Une position sur le manche, ou un fût — ce que la partition dessine.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Jeu {
    pub hauteur: u8,
    pub corde: Option<u8>,
    pub frette: Option<u8>,
}

/// Un évènement de la mesure : une note (ou sa suite liée), ou un silence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Evenement {
    /// Durée, en doubles croches.
    pub seiziemes: u32,
    pub jeu: Option<Jeu>,
    /// Suite d'une note commencée avant (à lier).
    pub lie: bool,
    /// Activation du réseau, pour estomper ce qui est peu sûr.
    pub confiance: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mesure {
    pub debut_s: f32,
    /// Temps dans la mesure (la métrique, mesure par mesure).
    pub temps: u32,
    pub evenements: Vec<Evenement>,
}

/// Indice fractionnaire de temps à l'instant `t`, par interpolation linéaire
/// entre les temps détectés (et prolongement aux bords).
fn indice(temps: &[f32], t: f32) -> f32 {
    let n = temps.len();
    let i = temps.partition_point(|&x| x <= t);
    let k = i.saturating_sub(1).min(n - 2);
    let (a, b) = (temps[k], temps[k + 1]);
    k as f32 + (t - a) / (b - a).max(1e-4)
}

/// Les mesures d'un morceau, notes posées. `temps` et `premiers` sont ceux de
/// la pulsation ; les notes, triées ou non.
pub fn quantifier(notes: &[Note], temps: &[f32], premiers: &[f32], metrique: u32) -> Vec<Mesure> {
    if temps.len() < 2 || premiers.is_empty() {
        return Vec::new();
    }
    // Chaque premier temps, en doubles croches depuis le premier temps détecté.
    let bornes: Vec<i64> = premiers
        .iter()
        .map(|&p| (indice(temps, p) * DIVISIONS as f32).round() as i64)
        .collect();
    let fin_morceau = bornes.last().copied().unwrap_or(0) + (metrique.max(1) * DIVISIONS) as i64;

    // Attaques et fins sur la grille ; deux attaques sur la même case : la plus sûre.
    let mut grille: Vec<(i64, i64, Jeu, f32)> = Vec::new();
    let mut triees: Vec<&Note> = notes.iter().collect();
    triees.sort_by(|a, b| a.debut_s.total_cmp(&b.debut_s));
    for n in triees {
        let d = (indice(temps, n.debut_s) * DIVISIONS as f32).round() as i64;
        let f = ((indice(temps, n.fin_s) * DIVISIONS as f32).round() as i64).max(d + 1);
        if d < bornes[0] || d >= fin_morceau {
            continue;
        }
        let jeu = Jeu { hauteur: n.hauteur, corde: n.corde, frette: n.frette };
        match grille.last_mut() {
            Some(der) if der.0 == d => {
                if n.amplitude > der.3 {
                    *der = (d, f, jeu, n.amplitude);
                }
            }
            _ => grille.push((d, f, jeu, n.amplitude)),
        }
    }
    // Une note s'arrête au plus tard à l'attaque suivante.
    for i in 0..grille.len().saturating_sub(1) {
        let suivante = grille[i + 1].0;
        grille[i].1 = grille[i].1.min(suivante);
    }

    let mut mesures = Vec::with_capacity(premiers.len());
    let mut k = 0; // prochaine note de la grille à poser
    for (m, &debut) in bornes.iter().enumerate() {
        let fin = bornes.get(m + 1).copied().unwrap_or(fin_morceau);
        if fin <= debut {
            continue;
        }
        let mut evenements = Vec::new();
        let mut curseur = debut;
        // Une note commencée dans une mesure précédente qui déborde ici.
        if k > 0 {
            let (_, f, jeu, a) = grille[k - 1];
            if f > debut {
                let fin_note = f.min(fin);
                evenements.push(Evenement { seiziemes: (fin_note - debut) as u32, jeu: Some(jeu), lie: true, confiance: a });
                curseur = fin_note;
            }
        }
        while k < grille.len() && grille[k].0 < fin {
            let (d, f, jeu, a) = grille[k];
            if d > curseur {
                evenements.push(Evenement { seiziemes: (d - curseur) as u32, jeu: None, lie: false, confiance: 1.0 });
            }
            let d = d.max(curseur);
            let fin_note = f.min(fin);
            if fin_note > d {
                evenements.push(Evenement { seiziemes: (fin_note - d) as u32, jeu: Some(jeu), lie: false, confiance: a });
                curseur = fin_note;
            }
            k += 1;
        }
        if curseur < fin {
            evenements.push(Evenement { seiziemes: (fin - curseur) as u32, jeu: None, lie: false, confiance: 1.0 });
        }
        let temps_mesure = ((fin - debut) as f32 / DIVISIONS as f32).round().max(1.0) as u32;
        mesures.push(Mesure { debut_s: premiers[m], temps: temps_mesure, evenements });
    }
    mesures
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(debut: f32, fin: f32, hauteur: u8) -> Note {
        Note { debut_s: debut, fin_s: fin, hauteur, amplitude: 0.8, corde: Some(0), frette: Some(hauteur - 28) }
    }

    /// 120 BPM, 4/4 : un temps toutes les 0,5 s, une mesure toutes les 2 s.
    fn grille() -> (Vec<f32>, Vec<f32>) {
        ((0..32).map(|k| k as f32 * 0.5).collect(), (0..8).map(|m| m as f32 * 2.0).collect())
    }

    #[test]
    fn chaque_mesure_fait_seize_doubles_croches() {
        let (t, p) = grille();
        let n = vec![note(0.02, 0.48, 28), note(0.51, 0.74, 31), note(1.0, 1.9, 33)];
        let m = quantifier(&n, &t, &p, 4);
        assert_eq!(m.len(), 8);
        for x in &m {
            assert_eq!(x.evenements.iter().map(|e| e.seiziemes).sum::<u32>(), 16);
            assert_eq!(x.temps, 4);
        }
        let e = &m[0].evenements;
        assert_eq!(e[0].seiziemes, 4); // noire
        assert_eq!(e[1].seiziemes, 2); // croche
        assert!(e[2].jeu.is_none()); // croche de silence
        // De 1,0 s (case 8) à 1,9 s (15,2 → case 15) : 7 doubles croches, puis
        // une de silence.
        assert_eq!(e[3].seiziemes, 7);
        assert!(e[4].jeu.is_none() && e[4].seiziemes == 1);
    }

    #[test]
    fn une_note_qui_franchit_la_barre_est_liee() {
        let (t, p) = grille();
        let m = quantifier(&[note(1.5, 3.0, 28)], &t, &p, 4);
        let fin_m0 = m[0].evenements.last().unwrap();
        assert!(fin_m0.jeu.is_some() && !fin_m0.lie);
        let debut_m1 = &m[1].evenements[0];
        assert!(debut_m1.lie);
        // De 2,0 s à 3,0 s : la moitié de la seconde mesure.
        assert_eq!(debut_m1.seiziemes, 8);
    }

    #[test]
    fn le_tempo_qui_derive_est_suivi() {
        // Les temps s'écartent : la note sur le troisième temps reste sur le
        // troisième temps, quel que soit l'instant absolu.
        let t: Vec<f32> = (0..16).map(|k| k as f32 * 0.5 + (k * k) as f32 * 0.01).collect();
        let p: Vec<f32> = (0..4).map(|m| t[m * 4]).collect();
        let m = quantifier(&[note(t[2] + 0.01, t[3], 28)], &t, &p, 4);
        assert!(m[0].evenements[0].jeu.is_none());
        assert_eq!(m[0].evenements[0].seiziemes, 8);
    }
}
