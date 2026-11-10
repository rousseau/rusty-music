// SPDX-License-Identifier: GPL-3.0-or-later
//! Dynamique et groove de la batterie MIDI (E5, étape 4 du coach,
//! `docs/recherche-coach-pratique.md`, « Timing, dynamique et groove »).
//!
//! Des **tendances**, jamais une note isolée : chaque mesure se fait sur
//! l'ensemble des notes sûres attrapées depuis l'armement, et rien n'est dit
//! sous un nombre minimal de notes ni sous un écart que l'oreille
//! n'entendrait pas.
//!
//! - **Micro-timing contre le stem**, pas contre la grille : la référence est
//!   la transcription du stem de batterie, ses instants gardent ce que le
//!   batteur du disque fait (caisse claire en arrière…). L'écart moyen par
//!   pièce dit « devant » ou « derrière le disque » ; quand le disque est
//!   lui-même décalé de la grille des temps (Beat This!), le conseil dit ce
//!   que le disque fait et ce que fait le jeu.
//! - **Swing** : rapport de durée entre la première et la seconde croche
//!   d'un temps, mesuré sur le disque et sur le jeu, sur les mêmes notes.
//! - **Dynamique** : la vélocité MIDI contre l'activation d'ADTOF au pic,
//!   par pièce, calculée sur le stem. Régularité des coups identiques (même
//!   pièce, même force sur le disque) ; corrélation des accents (seulement
//!   si le disque en a : sinon il n'y a rien à suivre).
//!
//! Les notes fantômes et les cymbales n'entrent pas : elles sont incertaines
//! ([`super::appariement::reference_sure`]), et la règle du coach est de ne
//! rien dire sur une référence incertaine.

use serde::Serialize;

use super::appariement::{Jugement, Reference, Verdict};
use super::batterie::{Classe, Frappe};

/// Une note sûre de la référence, attrapée par un coup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Echantillon {
    pub classe: Classe,
    /// Instant de la note sur le disque, en secondes du morceau.
    pub tau_disque_s: f64,
    /// Instant du coup, ramené au temps du morceau (latence ôtée).
    pub tau_jeu_s: f64,
    /// Activation d'ADTOF au pic de la note, sur le stem (0 à 1).
    pub force_disque: f32,
    pub velocite: u8,
}

/// Les échantillons d'une fenêtre jugée : chaque note sûre attrapée
/// (parfait, bon, en avance, en retard) et le coup qui l'a prise. `forces`
/// suit `refs` (activation d'ADTOF) ; une force absente vaut 1.
pub fn echantillons(
    refs: &[Reference],
    forces: &[f32],
    frappes: &[Frappe],
    jugements: &[Jugement],
) -> Vec<Echantillon> {
    jugements
        .iter()
        .filter(|j| {
            matches!(
                j.verdict,
                Verdict::Parfait | Verdict::Bon | Verdict::Avance | Verdict::Retard
            )
        })
        .filter_map(|j| {
            let i = j.reference?;
            let r = refs.get(i)?;
            let f = frappes.get(j.frappe?)?;
            let e = j.ecart_musical_ms?;
            (r.sure && e.is_finite()).then(|| Echantillon {
                classe: r.classe,
                tau_disque_s: r.tau_s,
                tau_jeu_s: r.tau_s + e / 1000.0,
                force_disque: forces.get(i).copied().unwrap_or(1.0),
                velocite: f.velocite,
            })
        })
        .collect()
}

/// Notes d'une pièce sous lesquelles rien n'est dit du timing ou de la
/// vélocité.
pub const N_MIN: usize = 8;
/// Notes d'une pièce sous lesquelles rien n'est dit des accents.
pub const N_ACCENTS: usize = 12;
/// Temps à croches sous lesquels rien n'est dit du swing.
pub const N_SWING: usize = 8;
/// Écart moyen au disque (ms, temps du morceau) au-delà duquel on dit
/// « devant » ou « derrière » : au-dessus du plancher de l'oreille
/// (10 ms), et l'écart doit dépasser deux erreurs types.
pub const DECALAGE_MS: f64 = 15.0;
/// Décalage du disque à la grille (ms) à partir duquel il « pose » une
/// pièce devant ou derrière le temps.
pub const DISQUE_DECALE_MS: f64 = 10.0;
/// Écart du rapport de swing (jeu contre disque) à partir duquel on le dit.
pub const SWING_ECART: f64 = 0.2;
/// Écart type de vélocité (sur 127) des coups identiques au-delà duquel ils
/// sont irréguliers.
pub const VELOCITE_ECART: f64 = 16.0;
/// Coefficient de variation des forces du disque sous lequel il n'a pas
/// d'accents à suivre.
pub const ACCENTS_CV_MIN: f64 = 0.15;
/// Corrélation des accents sous laquelle ils ne suivent pas le disque.
pub const ACCENTS_CORR_MIN: f64 = 0.2;

/// Ce que l'on mesure d'une pièce.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroovePiece {
    pub classe: Classe,
    pub notes: u32,
    /// Écart moyen du jeu au disque, en ms du morceau (négatif : devant).
    pub ecart_moyen_ms: f64,
    /// Décalage moyen du disque aux temps de la grille, sur les notes
    /// posées sur un temps (négatif : devant le temps).
    pub disque_grille_ms: Option<f64>,
    /// Le même pour le jeu, sur les mêmes notes.
    pub jeu_grille_ms: Option<f64>,
    pub velocite_moyenne: f64,
    /// Écart type de la vélocité des coups identiques (même force sur le
    /// disque), s'il y en a assez.
    pub velocite_ecart: Option<f64>,
    /// Corrélation entre la force du disque et la vélocité, si le disque a
    /// des accents et qu'il y a assez de notes.
    pub accents: Option<f64>,
}

/// Le rapport de swing, durée de la première croche sur la seconde (1 :
/// droit, 2 : ternaire).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Swing {
    pub disque: f64,
    pub jeu: f64,
    /// Temps à croches mesurés.
    pub temps: u32,
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Motif {
    /// Le jeu est devant le disque.
    Devant,
    /// Le jeu est derrière le disque.
    Derriere,
    /// Le disque pose la pièce hors de la grille, le jeu non (ou l'inverse).
    Grille,
    Swing,
    Velocite,
    Accents,
}

/// Un conseil tiré d'un motif.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conseil {
    pub motif: Motif,
    pub classe: Option<Classe>,
    pub texte: String,
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Groove {
    pub pieces: Vec<GroovePiece>,
    pub swing: Option<Swing>,
    /// Dans l'ordre : timing, swing, accents, vélocité.
    pub conseils: Vec<Conseil>,
}

fn nom(c: Classe) -> &'static str {
    match c {
        Classe::GrosseCaisse => "la grosse caisse",
        Classe::CaisseClaire => "la caisse claire",
        Classe::Toms => "les toms",
        Classe::Charleston => "le charleston",
        Classe::Cymbales => "les cymbales",
    }
}

fn moyenne(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

fn ecart_type(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    let m = moyenne(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt()
}

fn mediane(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn correlation(x: &[f64], y: &[f64]) -> Option<f64> {
    let (mx, my) = (moyenne(x), moyenne(y));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx).powi(2);
        syy += (b - my).powi(2);
    }
    (sxx > 1e-12 && syy > 1e-12).then(|| sxy / (sxx * syy).sqrt())
}

/// Le temps de la grille qui contient `tau`, et sa position dans le temps
/// (0 à 1).
fn dans_le_temps(temps: &[f64], tau: f64) -> Option<(usize, f64)> {
    let i = temps.partition_point(|&b| b <= tau).checked_sub(1)?;
    let (a, b) = (temps[i], *temps.get(i + 1)?);
    (b > a).then(|| (i, (tau - a) / (b - a)))
}

/// Une note est « sur le temps » à moins de 12 % du temps d'un battement.
const SUR_LE_TEMPS: f64 = 0.12;

/// Le décalage signé de `tau` au battement le plus proche, en ms, si la note
/// en est assez près.
fn au_battement(temps: &[f64], tau: f64) -> Option<f64> {
    let (i, p) = dans_le_temps(temps, tau)?;
    let duree = temps[i + 1] - temps[i];
    if p <= SUR_LE_TEMPS {
        Some(p * duree * 1000.0)
    } else if p >= 1.0 - SUR_LE_TEMPS {
        Some((p - 1.0) * duree * 1000.0)
    } else {
        None
    }
}

fn mesurer_piece(classe: Classe, e: &[&Echantillon], temps: &[f64]) -> GroovePiece {
    let ecarts: Vec<f64> = e
        .iter()
        .map(|x| (x.tau_jeu_s - x.tau_disque_s) * 1000.0)
        .collect();
    // Sur le temps : le décalage du disque et celui du jeu au même battement.
    let mut disque = Vec::new();
    let mut jeu = Vec::new();
    for x in e {
        if let Some(d) = au_battement(temps, x.tau_disque_s) {
            disque.push(d);
            jeu.push(d + (x.tau_jeu_s - x.tau_disque_s) * 1000.0);
        }
    }
    let assez = |v: &Vec<f64>| (v.len() >= N_MIN).then(|| moyenne(v));
    let velocites: Vec<f64> = e.iter().map(|x| x.velocite as f64).collect();
    let forces: Vec<f64> = e.iter().map(|x| x.force_disque as f64).collect();
    // Coups identiques : même pièce, force du disque à ±15 % de sa médiane.
    let force_med = mediane(&mut forces.clone());
    let identiques: Vec<f64> = e
        .iter()
        .filter(|x| ((x.force_disque as f64) - force_med).abs() <= 0.15 * force_med)
        .map(|x| x.velocite as f64)
        .collect();
    let cv = ecart_type(&forces) / moyenne(&forces).max(1e-6);
    let accents = (e.len() >= N_ACCENTS && cv >= ACCENTS_CV_MIN)
        .then(|| correlation(&forces, &velocites))
        .flatten();
    GroovePiece {
        classe,
        notes: e.len() as u32,
        ecart_moyen_ms: moyenne(&ecarts),
        disque_grille_ms: assez(&disque),
        jeu_grille_ms: assez(&jeu),
        velocite_moyenne: moyenne(&velocites),
        velocite_ecart: (identiques.len() >= N_MIN).then(|| ecart_type(&identiques)),
        accents,
    }
}

/// Le rapport de swing du disque et du jeu, sur les croches de second
/// temps (position 0,4 à 0,8 dans le battement) des battements sans double
/// croche avant (rien entre 0,15 et 0,4 : un seize-temps droit ferait
/// prendre sa quatrième double pour une croche swinguée).
fn mesurer_swing(e: &[Echantillon], temps: &[f64]) -> Option<Swing> {
    use std::collections::BTreeMap;
    let mut par_temps: BTreeMap<usize, Vec<(f64, f64)>> = BTreeMap::new();
    for x in e {
        if let Some((i, p)) = dans_le_temps(temps, x.tau_disque_s) {
            let q = dans_le_temps(temps, x.tau_jeu_s)
                .filter(|&(j, _)| j == i)
                .map(|(_, q)| q);
            if let Some(q) = q {
                par_temps.entry(i).or_default().push((p, q));
            }
        }
    }
    let mut disque = Vec::new();
    let mut jeu = Vec::new();
    for notes in par_temps.values() {
        if notes.iter().any(|&(p, _)| (0.15..0.4).contains(&p)) {
            continue;
        }
        // Une croche par temps : la première dans la plage.
        if let Some(&(p, q)) = notes.iter().find(|&&(p, _)| (0.4..=0.8).contains(&p)) {
            if q > 0.0 && q < 1.0 {
                disque.push(p / (1.0 - p));
                jeu.push(q / (1.0 - q));
            }
        }
    }
    (disque.len() >= N_SWING).then(|| Swing {
        temps: disque.len() as u32,
        disque: mediane(&mut disque),
        jeu: mediane(&mut jeu),
    })
}

fn ms(v: f64) -> String {
    format!("{:.0} ms", v.abs())
}

fn rapport(v: f64) -> String {
    format!("{v:.2}").replace('.', ",")
}

/// Le groove du jeu depuis l'armement : par pièce, le swing et les
/// conseils. `temps` est la grille des battements, en secondes du morceau
/// (vide : ni swing ni décalage à la grille).
pub fn analyser(echantillons: &[Echantillon], temps: &[f64]) -> Groove {
    let mut pieces = Vec::new();
    for c in Classe::TOUTES {
        let e: Vec<&Echantillon> = echantillons.iter().filter(|x| x.classe == c).collect();
        if !e.is_empty() {
            pieces.push(mesurer_piece(c, &e, temps));
        }
    }
    let swing = mesurer_swing(echantillons, temps);

    let mut timing = Vec::new();
    let mut accents = Vec::new();
    let mut velocite = Vec::new();
    for p in &pieces {
        let e: Vec<f64> = echantillons
            .iter()
            .filter(|x| x.classe == p.classe)
            .map(|x| (x.tau_jeu_s - x.tau_disque_s) * 1000.0)
            .collect();
        let n = e.len();
        let se = ecart_type(&e) / (n as f64).sqrt();
        let net = n >= N_MIN
            && p.ecart_moyen_ms.abs() >= DECALAGE_MS
            && p.ecart_moyen_ms.abs() > 2.0 * se;
        if net {
            let piece = nom(p.classe);
            let texte = match (p.disque_grille_ms, p.jeu_grille_ms) {
                (Some(d), Some(j)) if d.abs() >= DISQUE_DECALE_MS && j.abs() < d.abs() / 2.0 => {
                    let ou = if d > 0.0 { "derrière" } else { "devant" };
                    format!(
                        "Le disque pose {piece} {} {ou} le temps ; toi, à {}, tu joues sur la grille.",
                        ms(d),
                        ms(j)
                    )
                }
                _ => {
                    let ou = if p.ecart_moyen_ms < 0.0 {
                        "devant"
                    } else {
                        "derrière"
                    };
                    format!("Tu joues {piece} {} {ou} le disque.", ms(p.ecart_moyen_ms))
                }
            };
            let grille = matches!(
                (p.disque_grille_ms, p.jeu_grille_ms),
                (Some(d), Some(j)) if d.abs() >= DISQUE_DECALE_MS && j.abs() < d.abs() / 2.0
            );
            timing.push(Conseil {
                motif: if grille {
                    Motif::Grille
                } else if p.ecart_moyen_ms < 0.0 {
                    Motif::Devant
                } else {
                    Motif::Derriere
                },
                classe: Some(p.classe),
                texte,
            });
        }
        if let Some(r) = p.accents.filter(|&r| r < ACCENTS_CORR_MIN) {
            accents.push(Conseil {
                motif: Motif::Accents,
                classe: Some(p.classe),
                texte: format!(
                    "Tes accents sur {} ne suivent pas ceux du disque (corrélation {}).",
                    nom(p.classe),
                    rapport(r)
                ),
            });
        }
        if let Some(s) = p.velocite_ecart.filter(|&s| s > VELOCITE_ECART) {
            velocite.push(Conseil {
                motif: Motif::Velocite,
                classe: Some(p.classe),
                texte: format!(
                    "Tes coups de {} varient (± {:.0} de vélocité) là où le disque les joue égaux.",
                    nom(p.classe)
                        .trim_start_matches("la ")
                        .trim_start_matches("le ")
                        .trim_start_matches("les "),
                    s
                ),
            });
        }
    }
    let mut conseils = timing;
    if let Some(s) = swing.filter(|s| (s.jeu - s.disque).abs() >= SWING_ECART) {
        let sens = if s.jeu < s.disque {
            "plus droites"
        } else {
            "plus swinguées"
        };
        conseils.push(Conseil {
            motif: Motif::Swing,
            classe: None,
            texte: format!(
                "Tes croches sont {sens} que le disque ({} contre {}).",
                rapport(s.jeu),
                rapport(s.disque)
            ),
        });
    }
    conseils.extend(accents);
    conseils.extend(velocite);
    Groove {
        pieces,
        swing,
        conseils,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 32 temps à 120 bpm (0,5 s).
    fn grille() -> Vec<f64> {
        (0..=32).map(|i| i as f64 * 0.5).collect()
    }

    fn ech(classe: Classe, disque: f64, decalage_ms: f64, force: f32, velocite: u8) -> Echantillon {
        Echantillon {
            classe,
            tau_disque_s: disque,
            tau_jeu_s: disque + decalage_ms / 1000.0,
            force_disque: force,
            velocite,
        }
    }

    /// Un rock : grosse caisse sur 1 et 3, caisse claire sur 2 et 4,
    /// charleston en croches (seconde croche à la position `swing` du temps).
    /// `jeu(classe, rang)` rend (décalage en ms, vélocité).
    fn rock(
        retard_cc_disque_ms: f64,
        swing: f64,
        mut jeu: impl FnMut(Classe, usize, f32) -> (f64, u8),
        force_cc: impl Fn(usize) -> f32,
    ) -> Vec<Echantillon> {
        let mut v = Vec::new();
        for t in 0..32 {
            let b = t as f64 * 0.5;
            let (classe, force, tau) = if t % 2 == 0 {
                (Classe::GrosseCaisse, 0.9, b)
            } else {
                (
                    Classe::CaisseClaire,
                    force_cc(t),
                    b + retard_cc_disque_ms / 1000.0,
                )
            };
            let (d, vel) = jeu(classe, t, force);
            v.push(ech(classe, tau, d, force, vel));
            for (k, pos) in [0.0, swing].into_iter().enumerate() {
                let (d, vel) = jeu(Classe::Charleston, 2 * t + k, 0.8);
                v.push(ech(Classe::Charleston, b + pos * 0.5, d, 0.8, vel));
            }
        }
        v
    }

    #[test]
    fn un_jeu_fidele_ne_recoit_aucun_conseil() {
        let mut bruit = 0u32;
        let mut aleas = || {
            bruit = bruit.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            ((bruit >> 16) % 7) as f64 - 3.0
        };
        let e = rock(
            0.0,
            0.5,
            |_, _, f| (aleas(), (f * 110.0) as u8),
            |t| if t % 4 == 1 { 1.0 } else { 0.6 },
        );
        let g = analyser(&e, &grille());
        assert!(g.conseils.is_empty(), "{:?}", g.conseils);
        let cc = g
            .pieces
            .iter()
            .find(|p| p.classe == Classe::CaisseClaire)
            .unwrap();
        assert!(
            cc.accents.unwrap() > 0.9,
            "accents suivis : {:?}",
            cc.accents
        );
        let s = g.swing.unwrap();
        assert!(
            (s.disque - 1.0).abs() < 1e-9 && (s.jeu - 1.0).abs() < 0.05,
            "{s:?}"
        );
    }

    #[test]
    fn une_caisse_claire_pressee_est_dite_devant_le_disque() {
        let e = rock(
            0.0,
            0.5,
            |c, _, _| {
                (
                    if c == Classe::CaisseClaire {
                        -22.0
                    } else {
                        0.0
                    },
                    90,
                )
            },
            |_| 0.8,
        );
        let g = analyser(&e, &grille());
        assert_eq!(g.conseils.len(), 1, "{:?}", g.conseils);
        let c = &g.conseils[0];
        assert_eq!(
            (c.motif, c.classe),
            (Motif::Devant, Some(Classe::CaisseClaire))
        );
        assert_eq!(c.texte, "Tu joues la caisse claire 22 ms devant le disque.");
        // Trop peu de notes : rien.
        let peu: Vec<Echantillon> = e
            .iter()
            .filter(|x| x.classe == Classe::CaisseClaire)
            .take(5)
            .copied()
            .collect();
        assert!(analyser(&peu, &grille()).conseils.is_empty());
        // Sous le seuil de l'oreille : rien.
        let fin = rock(
            0.0,
            0.5,
            |c, _, _| (if c == Classe::CaisseClaire { -9.0 } else { 0.0 }, 90),
            |_| 0.8,
        );
        assert!(analyser(&fin, &grille()).conseils.is_empty());
    }

    #[test]
    fn le_disque_derriere_le_temps_et_le_jeu_sur_la_grille() {
        // Le batteur du disque pose sa caisse claire 20 ms en arrière ; le
        // jeu la met sur le temps (20 ms devant le disque).
        let e = rock(
            20.0,
            0.5,
            |c, _, _| {
                (
                    if c == Classe::CaisseClaire {
                        -20.0
                    } else {
                        0.0
                    },
                    90,
                )
            },
            |_| 0.8,
        );
        let g = analyser(&e, &grille());
        let cc = g
            .pieces
            .iter()
            .find(|p| p.classe == Classe::CaisseClaire)
            .unwrap();
        assert!((cc.disque_grille_ms.unwrap() - 20.0).abs() < 1e-6);
        assert!(cc.jeu_grille_ms.unwrap().abs() < 1e-6);
        assert_eq!(g.conseils.len(), 1, "{:?}", g.conseils);
        assert_eq!(g.conseils[0].motif, Motif::Grille);
        assert_eq!(
            g.conseils[0].texte,
            "Le disque pose la caisse claire 20 ms derrière le temps ; toi, à 0 ms, tu joues sur la grille."
        );
    }

    #[test]
    fn des_croches_droites_sur_un_disque_swingue() {
        // Disque à 0,64 du temps (rapport 1,78) ; le jeu tombe à la moitié.
        let e = rock(
            0.0,
            0.64,
            |c, rang, _| {
                let d = if c == Classe::Charleston && rang % 2 == 1 {
                    -0.14 * 500.0
                } else {
                    0.0
                };
                (d, 90)
            },
            |_| 0.8,
        );
        let g = analyser(&e, &grille());
        let s = g.swing.unwrap();
        assert_eq!(s.temps, 32);
        assert!(
            (s.disque - 0.64 / 0.36).abs() < 1e-6 && (s.jeu - 1.0).abs() < 1e-6,
            "{s:?}"
        );
        // Le décalage de 70 ms des secondes croches est dit aussi (derrière
        // ou devant le disque) ; le swing en plus.
        let swing: Vec<_> = g
            .conseils
            .iter()
            .filter(|c| c.motif == Motif::Swing)
            .collect();
        assert_eq!(swing.len(), 1);
        assert_eq!(
            swing[0].texte,
            "Tes croches sont plus droites que le disque (1,00 contre 1,78)."
        );
        // Des doubles croches droites ne passent pas pour du swing.
        let mut seize = Vec::new();
        for t in 0..16 {
            for k in 0..4 {
                seize.push(ech(
                    Classe::Charleston,
                    t as f64 * 0.5 + k as f64 * 0.125,
                    0.0,
                    0.8,
                    90,
                ));
            }
        }
        assert!(analyser(&seize, &grille()).swing.is_none());
        // Sans grille, ni swing ni décalage à la grille.
        let g = analyser(&e, &[]);
        assert!(g.swing.is_none() && g.pieces.iter().all(|p| p.disque_grille_ms.is_none()));
    }

    #[test]
    fn les_accents_et_la_velocite() {
        let accents = |t: usize| if t % 4 == 1 { 1.0 } else { 0.6 };
        // Accents à contretemps du disque : forts sur ses notes faibles.
        let e = rock(
            0.0,
            0.5,
            |c, _, f| {
                (
                    0.0,
                    if c == Classe::CaisseClaire {
                        if f > 0.9 {
                            60
                        } else {
                            110
                        }
                    } else {
                        90
                    },
                )
            },
            accents,
        );
        let g = analyser(&e, &grille());
        let a: Vec<_> = g
            .conseils
            .iter()
            .filter(|c| c.motif == Motif::Accents)
            .collect();
        assert_eq!(a.len(), 1, "{:?}", g.conseils);
        assert_eq!(a[0].classe, Some(Classe::CaisseClaire));
        // Un disque sans accents : rien à suivre, même avec des vélocités
        // au hasard ; mais les coups identiques irréguliers se disent.
        let e = rock(
            0.0,
            0.5,
            |c, rang, _| {
                (
                    0.0,
                    if c == Classe::Charleston {
                        [60, 120][rang % 2]
                    } else {
                        90
                    },
                )
            },
            |_| 0.8,
        );
        let g = analyser(&e, &grille());
        assert!(g.pieces.iter().all(|p| p.accents.is_none()));
        assert_eq!(g.conseils.len(), 1, "{:?}", g.conseils);
        assert_eq!(g.conseils[0].motif, Motif::Velocite);
        assert_eq!(
            g.conseils[0].texte,
            "Tes coups de charleston varient (± 30 de vélocité) là où le disque les joue égaux."
        );
    }

    #[test]
    fn seules_les_notes_sures_attrapees_font_un_echantillon() {
        use super::super::appariement::Jugement;
        let refs = [
            Reference {
                tau_s: 1.0,
                classe: Classe::CaisseClaire,
                sure: true,
            },
            Reference {
                tau_s: 1.5,
                classe: Classe::CaisseClaire,
                sure: false,
            },
            Reference {
                tau_s: 2.0,
                classe: Classe::GrosseCaisse,
                sure: true,
            },
        ];
        let frappe = |t_s, velocite| Frappe {
            t_s,
            classe: Classe::CaisseClaire,
            note: 38,
            velocite,
            ouverture: None,
        };
        let frappes = [frappe(10.0, 100), frappe(10.5, 40)];
        let j = |reference, frappe, ecart: Option<f64>, verdict| Jugement {
            reference,
            passage: 0,
            frappe,
            attendu_s: Some(0.0),
            ecart_ms: ecart,
            ecart_musical_ms: ecart,
            verdict,
        };
        let jugements = [
            j(Some(0), Some(0), Some(-12.0), Verdict::Bon),
            j(Some(1), Some(1), Some(5.0), Verdict::NonNotee),
            j(Some(2), None, None, Verdict::Manquee),
            j(None, Some(1), None, Verdict::EnTrop),
        ];
        let e = echantillons(&refs, &[0.9, 0.3, 0.8], &frappes, &jugements);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0], ech(Classe::CaisseClaire, 1.0, -12.0, 0.9, 100));
    }
}
