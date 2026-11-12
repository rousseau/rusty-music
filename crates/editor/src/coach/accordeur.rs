// SPDX-License-Identifier: GPL-3.0-or-later
//! L'accordeur de basse de Pratiquer (V36) : la même entrée que le coach de
//! basse, la même estimation de hauteur ([`super::basse::hauteur`], pYIN puis
//! les contrôles d'octave), affinée pour viser le cent.
//!
//! **Affiner.** pYIN rend la période à une fraction d'échantillon près, sur
//! une trame de 24 ms : à 400 Hz (120 échantillons à 48 kHz), un cent vaut
//! 0,07 échantillon, plus fin que ce qu'il garantit. L'accordeur dispose de
//! plus de signal (une corde tenue) : il cherche le creux de la différence de
//! YIN non pas à une période mais à `k` périodes (≈ 25 ms de décalage), sur
//! toute la trame, et l'interpole en parabole. L'erreur sur le décalage se
//! divise par `k`.
//!
//! **La référence.** La 440 par défaut ; « sur le morceau » (décision de
//! François), l'accord réel du disque mesuré sur le stem de basse
//! ([`ecart_disque`] : médiane des écarts au demi-tempéré des notes sûres),
//! auquel s'ajoute la transposition en cours. La transposition de l'établi se
//! fait par demi-tons entiers : elle déplace la grille d'un nombre entier de
//! demi-tons, l'écart en cents reste celui du disque ([`reference_hz`]).

use super::basse::{hauteur, midi, synthetiser_note, AnalyseurDI, CONFIANCE_JEU, CONFIANCE_SURE};
use serde::{Deserialize, Serialize};

/// Les cordes à vide d'une basse, de la plus grave : si (cinq cordes), mi,
/// la, ré, sol.
pub const CORDES: [u8; 5] = [23, 28, 33, 38, 43];

/// Sous ce niveau (dBFS, RMS), aucune corde ne joue.
pub const SILENCE_DBFS: f32 = -50.0;

/// « Juste » à ± ce nombre de cents.
pub const JUSTE_CENTS: f32 = 3.0;

/// Décalage visé pour l'affinage : le creux cherché à `k` périodes, la plus
/// proche de ce nombre de secondes.
const DECALAGE_AFFINE_S: f32 = 0.025;

/// Notes sûres qu'il faut au disque pour que son accord se mesure.
pub const NOTES_DISQUE_MIN: usize = 12;

/// Ce que l'accordeur entend.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Accord {
    /// Fondamentale affinée.
    pub f0_hz: f32,
    /// La note la plus proche (MIDI), contre la référence.
    pub note: u8,
    /// Écart à cette note, en cents (positif : trop haut).
    pub cents: f32,
    /// La corde à vide la plus proche (MIDI, [`CORDES`]).
    pub corde: u8,
    /// Écart à la corde à vide, en cents.
    pub cents_corde: f32,
    /// Probabilité de voisement de pYIN.
    pub confiance: f32,
}

fn niveau_db(x: &[f32]) -> f32 {
    let e = x.iter().map(|v| (v * v) as f64).sum::<f64>() / x.len().max(1) as f64;
    10.0 * (e.max(1e-12)).log10() as f32
}

/// Différence de YIN de `x` au décalage `tau`, sur tout ce qui tient.
fn difference(x: &[f32], tau: usize) -> f64 {
    let n = x.len().saturating_sub(tau);
    let mut s = 0f64;
    for j in 0..n {
        let e = (x[j] - x[j + tau]) as f64;
        s += e * e;
    }
    s / n.max(1) as f64
}

/// La fondamentale `f0` affinée sur `x` : le creux de la différence cherché
/// à `k` périodes, à ±2 échantillons, puis interpolé en parabole.
pub fn affiner(x: &[f32], sr: f32, f0: f32) -> f32 {
    let p = sr / f0;
    let k = ((sr * DECALAGE_AFFINE_S) / p).round().max(1.0);
    let centre = (k * p).round() as usize;
    if centre < 3 || centre + 3 >= x.len() / 2 {
        return f0;
    }
    let mut meilleur = centre;
    let mut dmin = f64::INFINITY;
    for tau in centre - 2..=centre + 2 {
        let d = difference(x, tau);
        if d < dmin {
            dmin = d;
            meilleur = tau;
        }
    }
    let (a, b, c) = (
        difference(x, meilleur - 1),
        dmin,
        difference(x, meilleur + 1),
    );
    let den = a - 2.0 * b + c;
    let t = if den.abs() > 1e-18 {
        meilleur as f64 + 0.5 * (a - c) / den
    } else {
        meilleur as f64
    };
    (sr as f64 * k as f64 / t) as f32
}

/// L'accord de la trame `x` (les dernières centaines de millisecondes de
/// l'entrée), contre la `la_hz` de référence. `None` : rien ne joue, ou pYIN
/// n'est pas sûr (« joue une corde »).
pub fn accorder(x: &[f32], sr: f32, la_hz: f32) -> Option<Accord> {
    accorder_vers(x, sr, la_hz, &CORDES)
}

/// [`accorder`] vers les cordes à vide `cordes` (MIDI) : l'accordage à
/// utiliser avec la hauteur en cours (V37 : ½ ton dessous, drop D…). Sans
/// corde, celles de [`CORDES`].
pub fn accorder_vers(x: &[f32], sr: f32, la_hz: f32, cordes: &[u8]) -> Option<Accord> {
    let cordes = if cordes.is_empty() {
        &CORDES[..]
    } else {
        cordes
    };
    if x.is_empty() || niveau_db(x) < SILENCE_DBFS {
        return None;
    }
    let (f0, confiance, _) = hauteur(x, sr)?;
    if confiance < CONFIANCE_JEU {
        return None;
    }
    let f0 = affiner(x, sr, f0);
    let m = midi(f0) - 12.0 * (la_hz / 440.0).log2();
    let note = m.round().clamp(0.0, 127.0);
    let corde = *cordes
        .iter()
        .min_by(|&&a, &&b| (m - a as f32).abs().total_cmp(&(m - b as f32).abs()))
        .expect("au moins une corde");
    Some(Accord {
        f0_hz: f0,
        note: note as u8,
        cents: 100.0 * (m - note),
        corde,
        cents_corde: 100.0 * (m - corde as f32),
        confiance,
    })
}

/// L'accord du disque, mesuré sur le stem de basse : (écart en cents au
/// La 440, notes retenues). Chaque note que l'analyseur de la DI y trouve
/// sûre ([`CONFIANCE_SURE`]) est affinée sur 150 ms après son attaque ;
/// l'écart est la médiane des écarts au demi-ton le plus proche. `None` sous
/// [`NOTES_DISQUE_MIN`] notes.
pub fn ecart_disque(stem: &[f32], sr: f32) -> Option<(f32, usize)> {
    let debut = (0.015 * sr) as usize;
    let long = (0.150 * sr) as usize;
    let mut ecarts: Vec<f32> = AnalyseurDI::analyser(stem, sr, 0.0)
        .iter()
        .filter(|n| n.confiance >= CONFIANCE_SURE)
        .filter_map(|n| {
            let f0 = n.f0_hz?;
            let a = (n.t_s * sr as f64) as usize + debut;
            let x = stem.get(a..(a + long).min(stem.len()))?;
            if x.len() < long / 2 {
                return None;
            }
            let m = midi(affiner(x, sr, f0));
            Some(100.0 * (m - m.round()))
        })
        .collect();
    if ecarts.len() < NOTES_DISQUE_MIN {
        return None;
    }
    ecarts.sort_by(f32::total_cmp);
    Some((ecarts[ecarts.len() / 2], ecarts.len()))
}

/// La référence de l'accordeur : La 440, ou « sur le morceau » l'accord du
/// disque (`ecart_cents`) plus la transposition en cours (`demi_tons`). La
/// transposition entière déplace la grille d'autant de demi-tons : ramené au
/// demi-ton le plus proche, l'écart reste celui du disque.
pub fn reference_hz(ecart_cents: Option<f32>, demi_tons: i32) -> f32 {
    match ecart_cents {
        None => 440.0,
        Some(e) => {
            let total = e + 100.0 * demi_tons as f32;
            let reduit = total - 100.0 * (total / 100.0).round();
            440.0 * 2f32.powf(reduit / 1200.0)
        }
    }
}

/// Une corde tenue, pour les tests et les bancs : une note de basse
/// synthétique (harmoniques, déclin) de `duree_s` à la fréquence `f_hz`.
pub fn corde_synthetique(sr: f32, f_hz: f32, duree_s: f32) -> Vec<f32> {
    let mut x = vec![0f32; (duree_s * sr) as usize];
    synthetiser_note(&mut x, sr, 0.0, duree_s, midi(f_hz), 1.0);
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Une sinusoïde de `f` Hz, 0,3 s.
    fn sinus(f: f32, gain: f32) -> Vec<f32> {
        (0..(0.3 * SR) as usize)
            .map(|j| gain * (2.0 * std::f32::consts::PI * f * j as f32 / SR).sin())
            .collect()
    }

    #[test]
    fn sinusoides_de_41_a_400_hz_au_cent_pres() {
        let mut pire = 0f32;
        let mut f = 41.0f32;
        while f <= 400.0 {
            // Pas tout à fait sur une note : 0 à 37 cents d'écart.
            let x = sinus(f, 0.3);
            let a = accorder(&x, SR, 440.0).unwrap_or_else(|| panic!("{f} Hz : rien"));
            let attendu = midi(f);
            let mesure = a.note as f32 + a.cents / 100.0;
            let err = 100.0 * (mesure - attendu).abs();
            pire = pire.max(err);
            assert!(err <= 1.0, "{f} Hz : {a:?}, écart {err:.2} cents");
            assert_eq!(a.note, attendu.round() as u8, "{f} Hz");
            f *= 1.037;
        }
        eprintln!("pire écart de 41 à 400 Hz : {pire:.3} cent");
    }

    #[test]
    fn les_cordes_d_une_basse_et_un_accord_bas() {
        // Une corde de la, 8 cents trop bas, avec ses harmoniques.
        let x = corde_synthetique(SR, 55.0 * 2f32.powf(-8.0 / 1200.0), 0.4);
        let a = accorder(&x[(0.05 * SR) as usize..], SR, 440.0).unwrap();
        assert_eq!((a.note, a.corde), (33, 33), "{a:?}");
        assert!((a.cents + 8.0).abs() <= 1.0, "{a:?}");
        // Le si grave d'une cinq cordes.
        let x = corde_synthetique(SR, 30.87, 0.5);
        let a = accorder(&x[(0.05 * SR) as usize..], SR, 440.0).unwrap();
        assert_eq!(a.corde, 23, "{a:?}");
        // Le mi grave, entre deux cordes : la corde visée est la plus proche.
        let a = accorder(&sinus(43.0, 0.3), SR, 440.0).unwrap();
        assert_eq!(a.corde, 28);
        assert!(a.cents_corde > 60.0, "{a:?}");
    }

    #[test]
    fn l_accordage_a_utiliser_change_la_corde_visee() {
        // V37 : un la♭ (51,9 Hz) vise la corde de la♭ d'une basse en ½ ton
        // dessous, juste ; en standard, il serait la corde de la, trop bas.
        let x = corde_synthetique(SR, 51.91, 0.4);
        let x = &x[(0.05 * SR) as usize..];
        let a = accorder_vers(x, SR, 440.0, &[27, 32, 37, 42]).unwrap();
        assert_eq!(a.corde, 32, "{a:?}");
        assert!(a.cents_corde.abs() < 5.0, "{a:?}");
        let b = accorder(x, SR, 440.0).unwrap();
        assert_eq!(b.corde, 33, "{b:?}");
        assert!(b.cents_corde < -90.0, "{b:?}");
    }

    #[test]
    fn la_reference_deplace_les_cents() {
        // La 435 : un la à 110 Hz y est ≈ +19,8 cents.
        let a = accorder(&sinus(110.0, 0.3), SR, 435.0).unwrap();
        assert_eq!(a.note, 45);
        assert!((a.cents - 19.78).abs() < 1.0, "{a:?}");
    }

    #[test]
    fn un_signal_faible_ou_du_bruit_ne_dit_rien() {
        assert!(accorder(&sinus(55.0, 0.001), SR, 440.0).is_none());
        assert!(accorder(&[], SR, 440.0).is_none());
        let mut graine = 7u32;
        let bruit: Vec<f32> = (0..(0.3 * SR) as usize)
            .map(|_| {
                graine = graine.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((graine >> 16) as f32 / 65536.0 - 0.5) * 0.4
            })
            .collect();
        assert!(accorder(&bruit, SR, 440.0).is_none());
    }

    #[test]
    fn l_accord_du_disque_sur_un_stem() {
        // Une ligne de basse jouée 12 cents trop bas : vingt notes.
        let notes = [28.0, 33.0, 38.0, 40.0, 35.0, 31.0, 43.0, 36.0, 30.0, 45.0];
        let mut x = vec![0f32; (11.0 * SR) as usize];
        for k in 0..20 {
            let m = notes[k % notes.len()] - 0.12;
            synthetiser_note(&mut x, SR, 0.3 + 0.5 * k as f32, 0.45, m, 1.0);
        }
        let (e, n) = ecart_disque(&x, SR).expect("assez de notes sûres");
        assert!(n >= NOTES_DISQUE_MIN, "{n}");
        assert!((e + 12.0).abs() <= 1.5, "{e} cents sur {n} notes");
        // Trop peu de notes : rien.
        let mut court = vec![0f32; SR as usize];
        synthetiser_note(&mut court, SR, 0.2, 0.5, 33.0, 1.0);
        assert!(ecart_disque(&court, SR).is_none());
    }

    #[test]
    fn la_reference_sur_le_morceau() {
        assert_eq!(reference_hz(None, 3), 440.0);
        let r = |e, d| 1200.0 * (reference_hz(Some(e), d) / 440.0).log2();
        assert!((r(-12.0, 0) + 12.0).abs() < 0.01);
        // Transposé d'un demi-ton : l'écart en cents ne bouge pas.
        assert!((r(-12.0, 1) + 12.0).abs() < 0.01);
        assert!((r(30.0, -2) - 30.0).abs() < 0.01);
    }
}
