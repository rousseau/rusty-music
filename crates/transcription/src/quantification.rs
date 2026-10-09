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
//! - une note qui franchit une barre est coupée et liée ;
//! - le **décalage de jeu** est retranché avant d'aimanter (voir
//!   [`decalage_de_jeu`]).

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

/// Le décalage de jeu d'un morceau, en secondes : ce qu'il faut retrancher aux
/// attaques pour qu'elles tombent au mieux sur la grille de doubles croches.
///
/// Mesuré sur « Love Foolosophy » (9 oct.) : batterie et basse jouent toutes
/// deux ~50 ms derrière les temps détectés — près d'une demi-double croche à
/// 130 BPM. Aimantées telles quelles, la moitié des notes tombait sur la case
/// d'à côté. On cherche le décalage qui minimise l'écart moyen à la grille,
/// dans ± une demi-double croche ; à égalité (0,5 % près) on préfère jouer
/// **derrière** le temps, de loin le plus courant (et Basic Pitch date une
/// note plutôt tard que tôt).
pub fn decalage_de_jeu(notes: &[Note], temps: &[f32]) -> f32 {
    let attaques: Vec<f32> = notes.iter().map(|n| n.debut_s).collect();
    decalage_des_attaques(&attaques, temps)
}

/// [`decalage_de_jeu`] sur de simples instants d'attaque (les coups d'une
/// batterie, par exemple).
pub fn decalage_des_attaques(attaques: &[f32], temps: &[f32]) -> f32 {
    if attaques.len() < 8 || temps.len() < 2 {
        return 0.0;
    }
    let mut ecarts: Vec<f32> = temps.windows(2).map(|w| w[1] - w[0]).collect();
    ecarts.sort_by(f32::total_cmp);
    let demi = ecarts[ecarts.len() / 2] / DIVISIONS as f32 / 2.0;
    let ecart_moyen = |d: f32| -> f32 {
        let s: f32 = attaques
            .iter()
            .map(|&t| {
                let x = indice(temps, t - d) * DIVISIONS as f32;
                (x - x.round()).abs()
            })
            .sum();
        s / attaques.len() as f32
    };
    let pas = 0.005;
    let n = (demi / pas).floor() as i32;
    let candidats: Vec<(f32, f32)> = (-n..=n).map(|k| k as f32 * pas).map(|d| (ecart_moyen(d), d)).collect();
    let meilleur = candidats.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
    candidats
        .iter()
        .filter(|c| c.0 <= meilleur + 0.005)
        // Derrière le temps d'abord, puis le plus petit décalage.
        .max_by(|a, b| (a.1 > 0.0).cmp(&(b.1 > 0.0)).then(b.1.abs().total_cmp(&a.1.abs())))
        .map_or(0.0, |c| c.1)
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
    let decalage = decalage_de_jeu(notes, temps);
    for n in triees {
        let d = (indice(temps, n.debut_s - decalage) * DIVISIONS as f32).round() as i64;
        let f = ((indice(temps, n.fin_s - decalage) * DIVISIONS as f32).round() as i64).max(d + 1);
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

/// Un temps de batterie : les pièces frappées ensemble sur une double
/// croche, ou un silence (`pieces` vide).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frappe {
    pub seiziemes: u32,
    pub pieces: Vec<crate::batterie::Piece>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MesureBatterie {
    pub debut_s: f32,
    pub temps: u32,
    pub frappes: Vec<Frappe>,
}

/// Les coups d'une batterie, en mesures : chaque coup sur la double croche la
/// plus proche (décalage de jeu retranché), les coups d'une même case
/// réunis ; une frappe dure jusqu'à la case frappée suivante — la convention
/// de la notation de batterie, où seule l'attaque compte.
pub fn quantifier_coups(coups: &[crate::batterie::Coup], temps: &[f32], premiers: &[f32], metrique: u32) -> Vec<MesureBatterie> {
    if temps.len() < 2 || premiers.is_empty() {
        return Vec::new();
    }
    let attaques: Vec<f32> = coups.iter().map(|c| c.instant_s).collect();
    let decalage = decalage_des_attaques(&attaques, temps);
    let case = |t: f32| (indice(temps, t - decalage) * DIVISIONS as f32).round() as i64;
    let bornes: Vec<i64> = premiers.iter().map(|&p| (indice(temps, p) * DIVISIONS as f32).round() as i64).collect();
    let fin_morceau = bornes.last().copied().unwrap_or(0) + (metrique.max(1) * DIVISIONS) as i64;

    let mut cases: std::collections::BTreeMap<i64, Vec<crate::batterie::Piece>> = Default::default();
    for c in coups {
        let k = case(c.instant_s);
        if k >= bornes[0] && k < fin_morceau {
            let v = cases.entry(k).or_default();
            if !v.contains(&c.piece) {
                v.push(c.piece);
            }
        }
    }
    let mut mesures = Vec::with_capacity(premiers.len());
    for (m, &debut) in bornes.iter().enumerate() {
        let fin = bornes.get(m + 1).copied().unwrap_or(fin_morceau);
        if fin <= debut {
            continue;
        }
        let dans: Vec<(i64, Vec<crate::batterie::Piece>)> =
            cases.range(debut..fin).map(|(k, v)| (*k, { let mut v = v.clone(); v.sort(); v })).collect();
        let mut frappes = Vec::new();
        let premier = dans.first().map_or(fin, |x| x.0);
        if premier > debut {
            frappes.push(Frappe { seiziemes: (premier - debut) as u32, pieces: Vec::new() });
        }
        for (i, (k, pieces)) in dans.iter().enumerate() {
            let suivante = dans.get(i + 1).map_or(fin, |x| x.0);
            frappes.push(Frappe { seiziemes: (suivante - k) as u32, pieces: pieces.clone() });
        }
        let temps_mesure = ((fin - debut) as f32 / DIVISIONS as f32).round().max(1.0) as u32;
        mesures.push(MesureBatterie { debut_s: premiers[m], temps: temps_mesure, frappes });
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
    fn un_jeu_en_retard_est_recale() {
        // Doubles croches jouées 50 ms derrière une grille à 120 BPM (une
        // double croche = 125 ms) : sans recalage, la moitié tomberait à côté.
        let (t, p) = grille();
        let n: Vec<Note> = (0..40).map(|k| note(k as f32 * 0.25 + 0.05, k as f32 * 0.25 + 0.2, 28)).collect();
        let d = decalage_de_jeu(&n, &t);
        assert!((d - 0.05).abs() <= 0.006, "décalage {d}");
        let m = quantifier(&n, &t, &p, 4);
        // Une note toutes les deux doubles croches, chacune sur sa case.
        assert!(m[0].evenements.iter().filter(|e| e.jeu.is_some()).count() == 8, "{:?}", m[0].evenements);
    }

    #[test]
    fn un_jeu_sur_la_grille_n_est_pas_decale() {
        let (t, _) = grille();
        let n: Vec<Note> = (0..40).map(|k| note(k as f32 * 0.25, k as f32 * 0.25 + 0.2, 28)).collect();
        assert!(decalage_de_jeu(&n, &t).abs() < 0.006);
    }

    #[test]
    fn un_rythme_rock_en_batterie() {
        use crate::batterie::{Coup, Piece};
        let (t, p) = grille();
        // Charleston en croches, grosse caisse sur 1 et 3, caisse claire sur 2 et 4.
        let mut coups = Vec::new();
        for k in 0..8 {
            coups.push(Coup { instant_s: k as f32 * 0.25, piece: Piece::Charleston, force: 0.8 });
        }
        for &(t0, piece) in &[(0.0, Piece::GrosseCaisse), (0.5, Piece::CaisseClaire), (1.0, Piece::GrosseCaisse), (1.5, Piece::CaisseClaire)] {
            coups.push(Coup { instant_s: t0, piece, force: 0.8 });
        }
        let m = quantifier_coups(&coups, &t, &p, 4);
        let f = &m[0].frappes;
        assert_eq!(f.len(), 8, "{f:?}");
        assert!(f.iter().all(|x| x.seiziemes == 2));
        assert_eq!(f[0].pieces, vec![Piece::GrosseCaisse, Piece::Charleston]);
        assert_eq!(f[2].pieces, vec![Piece::CaisseClaire, Piece::Charleston]);
        assert_eq!(f[1].pieces, vec![Piece::Charleston]);
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
