// SPDX-License-Identifier: GPL-3.0-or-later
//! Le groove d'une batterie en trois nombres : densité, syncope, swing.
//!
//! **Ce que la recherche du greffon en fait** (plan Éditer 2.2) : « une
//! batterie plus funk », c'est une batterie plus syncopée à densité voisine.
//! Les empreintes CLAP ne savent comparer que des mélanges entiers ; ces trois
//! nombres se mesurent sur le stem batterie d'un candidat une fois démixé, et
//! disent à l'utilisateur pourquoi il est classé là.
//!
//! **La méthode.** Les attaques sont les maxima locaux du flux spectral, sur
//! des amplitudes compressées ([`enveloppe_log`]), au-dessus d'un seuil
//! adaptatif. Chacune est placée dans son temps — l'intervalle entre deux
//! battements de Beat This!, pas une grille à tempo constant : un morceau
//! joué sans clic dérive — et arrondie à la double croche :
//!
//! - **densité** : attaques par temps ;
//! - **syncope** : part de l'énergie d'attaque hors des temps, la double
//!   croche (« e », « a ») comptant pleinement, la croche du milieu (« et »)
//!   pour moitié. 0 : tout sur les temps ; 1 : tout sur les doubles croches
//!   faibles. C'est la pondération métrique de Longuet-Higgins et Lee réduite
//!   à un temps, et c'est volontairement simple : on compare des batteries
//!   entre elles, on ne mesure pas une syncope absolue ;
//! - **swing** : où tombe la croche du milieu, en rapport de durées (1 :
//!   droit ; 2 : ternaire, croche pointée sur double). `None` quand trop peu
//!   d'attaques s'y trouvent pour le dire.

use crate::battements;
use crate::descripteurs::{self, Analyseur, TPS};

/// Attaques minimales sur la croche du milieu pour oser un swing.
const MIN_SWING: usize = 8;

/// Fenêtre où l'on cherche la croche du milieu, en fraction de temps : de
/// l'exact milieu (droit, 0,5) au triolet (0,667) et un peu au-delà, mais
/// avant la double croche « a » (0,75) qu'un charleston en doubles mettrait
/// sinon dans le compte.
const FENETRE_SWING: (f32, f32) = (0.42, 0.72);

/// Le groove mesuré d'une batterie.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Groove {
    /// Attaques par temps.
    pub densite: f32,
    /// 0 à 1 — voir le module.
    pub syncope: f32,
    /// Rapport de durées des deux croches d'un temps, s'il se mesure.
    pub swing: Option<f32>,
    /// Temps couverts par la mesure.
    pub temps: usize,
}

/// Le flux spectral sur des amplitudes **compressées** (`ln(1 + x)`).
///
/// Mesuré sur de vraies batteries démixées (`examples/groove_stems.rs`) : sur
/// les amplitudes brutes du tempo, grosse caisse et caisse claire écrasent
/// tout, et les charlestons comme les notes fantômes — ce qui fait justement
/// un groove funk — passent sous le seuil. La compression logarithmique, celle
/// des détecteurs d'attaques usuels (librosa, madmom), les rend visibles.
fn enveloppe_log(signal: &[f32], a: &Analyseur) -> Vec<f32> {
    let mut spectres = a.spectres(signal);
    for trame in &mut spectres {
        for x in trame.iter_mut() {
            *x = x.ln_1p();
        }
    }
    let mut env = descripteurs::flux(&spectres);
    descripteurs::centrer(&mut env);
    env
}

/// Attaques d'une enveloppe : (instant en secondes, force).
fn attaques(env: &[f32]) -> Vec<(f32, f32)> {
    if env.len() < 3 {
        return Vec::new();
    }
    let n = env.len() as f32;
    let moyenne = env.iter().sum::<f32>() / n;
    let ecart = (env.iter().map(|v| (v - moyenne).powi(2)).sum::<f32>() / n).sqrt();
    let seuil = moyenne + 0.5 * ecart;
    // Deux trames de part et d'autre (±21 ms) : un fla reste une attaque, un
    // roulement serré aussi par bouts, et c'est ce qu'on entend.
    (2..env.len().saturating_sub(2))
        .filter(|&i| {
            let v = env[i];
            v > seuil && v > env[i - 1] && v >= env[i + 1] && v >= env[i - 2] && v >= env[i + 2]
        })
        .map(|i| (i as f32 / TPS + battements::LATENCE_S, env[i]))
        .collect()
}

/// Le groove d'attaques placées dans des temps.
///
/// `temps` : les battements, en secondes, croissants (Beat This!).
/// `None` s'il n'y a pas deux temps ou aucune attaque entre eux.
pub fn groove_des_attaques(attaques: &[(f32, f32)], temps: &[f32]) -> Option<Groove> {
    if temps.len() < 2 {
        return None;
    }
    let (premier, dernier) = (temps[0], temps[temps.len() - 1]);
    let (mut n, mut force, mut hors) = (0usize, 0.0f32, 0.0f32);
    let mut milieux: Vec<f32> = Vec::new();
    for &(t, f) in attaques {
        // Un temps d'avance pour arrondir : une attaque un rien en avance sur
        // le premier temps lui appartient.
        if t < premier - 0.05 || t >= dernier {
            continue;
        }
        let k = temps.partition_point(|&b| b <= t).saturating_sub(1);
        let (a, b) = (temps[k], temps[(k + 1).min(temps.len() - 1)]);
        if b <= a {
            continue;
        }
        let p = ((t - a) / (b - a)).clamp(0.0, 1.0);
        let poids = match (p * 4.0).round() as u32 % 4 {
            0 => 0.0,
            2 => 0.5,
            _ => 1.0,
        };
        n += 1;
        force += f;
        hors += f * poids;
        if (FENETRE_SWING.0..FENETRE_SWING.1).contains(&p) {
            milieux.push(p);
        }
    }
    if n == 0 || force <= 0.0 {
        return None;
    }
    let nb_temps = temps.len() - 1;
    let swing = (milieux.len() >= MIN_SWING).then(|| {
        milieux.sort_by(f32::total_cmp);
        let m = milieux[milieux.len() / 2];
        m / (1.0 - m)
    });
    Some(Groove {
        densite: n as f32 / nb_temps as f32,
        syncope: hors / force,
        swing,
        temps: nb_temps,
    })
}

/// Le groove d'un signal mono, à n'importe quelle fréquence.
pub fn groove(signal: &[f32], sr: u32, a: &Analyseur, temps: &[f32]) -> Option<Groove> {
    if signal.is_empty() {
        return None;
    }
    let env = enveloppe_log(&battements::vers_48k(signal, sr), a);
    groove_des_attaques(&attaques(&env), temps)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// Une frappe : un bruit bref qui s'éteint en quelques millisecondes.
    fn frappe(signal: &mut [f32], t: f32, graine: &mut u32) {
        let debut = (t * SR as f32) as usize;
        for i in 0..(SR as usize / 50) {
            if debut + i >= signal.len() {
                break;
            }
            *graine = graine.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let bruit = (*graine >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            signal[debut + i] += bruit * (-(i as f32) / 120.0).exp();
        }
    }

    /// Seize secondes à 120 BPM, une frappe à chaque position (en fraction de
    /// temps) donnée, dans chaque temps.
    fn batterie(positions: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let periode = 0.5;
        let temps: Vec<f32> = (0..32).map(|k| 0.5 + k as f32 * periode).collect();
        let mut s = vec![0.0f32; SR as usize * 17];
        let mut g = 7;
        for &b in &temps[..temps.len() - 1] {
            for p in positions {
                frappe(&mut s, b + p * periode, &mut g);
            }
        }
        (s, temps)
    }

    fn mesure(positions: &[f32]) -> Groove {
        let (s, temps) = batterie(positions);
        groove(&s, SR, &Analyseur::new(), &temps).expect("un groove")
    }

    #[test]
    fn des_noires_ne_sont_pas_syncopees() {
        let g = mesure(&[0.0]);
        assert!((g.densite - 1.0).abs() < 0.1, "densité {}", g.densite);
        assert!(g.syncope < 0.05, "syncope {}", g.syncope);
        assert_eq!(g.swing, None);
    }

    #[test]
    fn des_doubles_faibles_syncopent_plus_que_des_croches() {
        let croches = mesure(&[0.0, 0.5]);
        let funk = mesure(&[0.0, 0.25, 0.75]);
        assert!((croches.densite - 2.0).abs() < 0.15, "{croches:?}");
        assert!((funk.densite - 3.0).abs() < 0.2, "{funk:?}");
        assert!(
            funk.syncope > croches.syncope + 0.2,
            "funk {} contre croches {}",
            funk.syncope,
            croches.syncope
        );
    }

    #[test]
    fn le_swing_se_lit_sur_la_croche_du_milieu() {
        let droit = mesure(&[0.0, 0.5]);
        let ternaire = mesure(&[0.0, 2.0 / 3.0]);
        let d = droit.swing.expect("swing droit");
        let t = ternaire.swing.expect("swing ternaire");
        assert!((d - 1.0).abs() < 0.15, "droit {d}");
        assert!((t - 2.0).abs() < 0.3, "ternaire {t}");
    }

    #[test]
    fn sans_temps_pas_de_groove() {
        assert_eq!(groove_des_attaques(&[(1.0, 1.0)], &[0.5]), None);
        assert_eq!(groove(&[], SR, &Analyseur::new(), &[0.0, 1.0]), None);
    }
}
