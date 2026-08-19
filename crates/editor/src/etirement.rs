//! Étirement temporel et transposition.
//!
//! **Vérifié avant d'écrire quoi que ce soit** : `wsola` fait déjà l'étirement
//! à hauteur préservée, en Rust pur et sans dépendance transitive. On s'en sert
//! plutôt que de maintenir un vocodeur de phase écrit à la main — c'était
//! cinq cents lignes de la partie la plus délicate du traitement du signal, et
//! ses artefacts de phase sur les transitoires n'étaient pas mesurables avec
//! les témoins qu'on savait calculer.
//!
//! La méthode de `wsola` — recouvrement-addition par similarité de forme
//! d'onde — est celle d'`atempo` chez ffmpeg et de VLC. Temporelle, donc sans
//! artefact de phase.
//!
//! Ce qui reste ici : la **transposition**, que `wsola` ne fait pas. Elle
//! s'obtient en étirant puis en rééchantillonnant du même rapport — la durée
//! revient à sa valeur d'origine, et c'est la hauteur qui a bougé.

use std::path::Path;

use crate::decode;

/// Fréquence de travail de l'éditeur.
const SR: u32 = 44_100;

/// Rééchantillonne d'un rapport donné, par interpolation cubique.
///
/// Catmull-Rom plutôt que linéaire : la linéaire est un filtre passe-bas très
/// doux qui ternit tout le haut du spectre, et ça s'entend sur des cymbales.
fn reechantillonner(signal: &[f32], rapport: f32) -> Vec<f32> {
    if signal.len() < 4 || (rapport - 1.0).abs() < 1e-6 {
        return signal.to_vec();
    }
    let taille = (signal.len() as f32 / rapport) as usize;
    (0..taille)
        .map(|i| {
            let x = i as f32 * rapport;
            let j = x.floor() as isize;
            let t = x - j as f32;
            let e = |d: isize| signal[(j + d).clamp(0, signal.len() as isize - 1) as usize];
            let (p0, p1, p2, p3) = (e(-1), e(0), e(1), e(2));
            p1 + 0.5
                * t
                * (p2 - p0
                    + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)))
        })
        .collect()
}

/// Étire un signal entrelacé sans changer sa hauteur.
///
/// `facteur` est un facteur de **durée** : 1,25 rend le morceau un quart plus
/// long. `wsola` raisonne en tempo, qui en est l'inverse.
pub fn etirer(signal: &[f32], canaux: usize, facteur: f32) -> Vec<f32> {
    if canaux == 0 || signal.is_empty() || (facteur - 1.0).abs() < 1e-6 {
        return signal.to_vec();
    }
    let tempo = (1.0 / facteur.clamp(0.25, 4.0)).clamp(0.25, 4.0);
    let out = wsola::stretch(signal, SR, canaux as u16, tempo).unwrap_or_default();
    // Un signal plus court que la fenêtre de l'étireur n'en ressort pas : il
    // n'y a pas de quoi chercher une similarité de forme d'onde. On rend alors
    // la matière telle quelle plutôt que le silence — la bibliothèque contient
    // des pistes d'une seconde.
    if out.is_empty() {
        return signal.to_vec();
    }
    out
}

/// Transpose de `demi_tons` sans changer la durée.
///
/// Étirer puis rééchantillonner du même rapport. L'inverse — rééchantillonner
/// puis étirer — donnerait le même résultat en théorie, mais ferait travailler
/// l'étireur sur un signal déjà interpolé.
pub fn transposer(signal: &[f32], canaux: usize, demi_tons: f32) -> Vec<f32> {
    if canaux == 0 || demi_tons.abs() < 1e-6 {
        return signal.to_vec();
    }
    let rapport = 2f32.powf(demi_tons / 12.0);
    let etire = etirer(signal, canaux, rapport);
    let par_canal: Vec<Vec<f32>> = separer(&etire, canaux)
        .iter()
        .map(|c| reechantillonner(c, rapport))
        .collect();
    entrelacer(&par_canal)
}

/// Sépare un signal entrelacé en canaux.
fn separer(signal: &[f32], canaux: usize) -> Vec<Vec<f32>> {
    (0..canaux)
        .map(|c| signal.iter().skip(c).step_by(canaux).copied().collect())
        .collect()
}

/// Réentrelace des canaux de même longueur.
fn entrelacer(canaux: &[Vec<f32>]) -> Vec<f32> {
    let n = canaux.iter().map(Vec::len).min().unwrap_or(0);
    let mut out = Vec::with_capacity(n * canaux.len());
    for i in 0..n {
        for c in canaux {
            out.push(c[i]);
        }
    }
    out
}

/// Étire un fichier et rend le résultat entrelacé. Utilitaire pour la ligne de
/// commande, qui décode puis traite.
pub fn etirer_fichier(
    chemin: &Path,
    facteur: f32,
    demi_tons: f32,
) -> Result<Vec<f32>, decode::Error> {
    let s = decode::stereo(chemin)?;
    let entrelace: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .flat_map(|(g, d)| [*g, *d])
        .collect();
    let mut out = etirer(&entrelace, 2, facteur);
    if demi_tons.abs() > 1e-6 {
        out = transposer(&out, 2, demi_tons);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::{num_complex::Complex, FftPlanner};

    const TAU: f32 = std::f32::consts::TAU;
    const HZ: f32 = SR as f32;

    fn sinus(hz: f32, secondes: f32) -> Vec<f32> {
        let n = (HZ * secondes) as usize;
        (0..n).map(|i| (TAU * hz * i as f32 / HZ).sin()).collect()
    }

    /// Fréquence dominante, par transformée sur la partie centrale.
    fn dominante(signal: &[f32]) -> f32 {
        let n = 8192;
        assert!(signal.len() >= n, "signal trop court pour être mesuré");
        let debut = (signal.len() - n) / 2;
        let mut buf: Vec<Complex<f32>> = (0..n)
            .map(|i| {
                let w = 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos();
                Complex::new(signal[debut + i] * w, 0.0)
            })
            .collect();
        FftPlanner::new().plan_fft_forward(n).process(&mut buf);
        let k = (1..n / 2)
            .max_by(|a, b| buf[*a].norm().total_cmp(&buf[*b].norm()))
            .unwrap_or(0);
        k as f32 * HZ / n as f32
    }

    /// Ce que l'étirement promet : la durée change, la hauteur non.
    #[test]
    fn letirement_change_la_duree_pas_la_hauteur() {
        let s = sinus(440.0, 2.0);
        for facteur in [0.5f32, 1.5, 2.0] {
            let out = etirer(&s, 1, facteur);
            let attendu = s.len() as f32 * facteur;
            let ecart = (out.len() as f32 - attendu).abs() / attendu;
            assert!(ecart < 0.05, "durée ×{facteur} : {ecart:.3} d'écart");

            let hz = dominante(&out);
            assert!(
                (hz - 440.0).abs() < 12.0,
                "hauteur déplacée à {hz:.0} Hz pour un étirement ×{facteur}"
            );
        }
    }

    /// Et la transposition l'inverse : la hauteur change, la durée non.
    #[test]
    fn la_transposition_change_la_hauteur_pas_la_duree() {
        let s = sinus(220.0, 2.0);
        for (demi_tons, attendu) in [(12.0f32, 440.0f32), (-12.0, 110.0), (7.0, 329.6)] {
            let out = transposer(&s, 1, demi_tons);
            let hz = dominante(&out);
            let ecart = (hz - attendu).abs() / attendu;
            assert!(
                ecart < 0.04,
                "{demi_tons} demi-tons : {hz:.0} Hz au lieu de {attendu:.0}"
            );

            let duree = (out.len() as f32 - s.len() as f32).abs() / s.len() as f32;
            assert!(duree < 0.08, "durée déplacée de {:.1} %", duree * 100.0);
        }
    }

    /// Un facteur neutre rend le signal tel quel : on ne fait rien passer dans
    /// l'étireur quand il n'y a rien à étirer.
    #[test]
    fn un_facteur_neutre_rend_le_signal() {
        let s = sinus(440.0, 0.5);
        assert_eq!(etirer(&s, 1, 1.0), s);
        assert_eq!(transposer(&s, 1, 0.0), s);
    }

    /// La bibliothèque contient des pistes plus courtes qu'une fenêtre.
    #[test]
    fn un_signal_trop_court_ne_panique_pas() {
        assert!(!etirer(&[0.1; 100], 1, 2.0).is_empty());
        assert!(etirer(&[], 0, 2.0).is_empty());
        assert_eq!(transposer(&[0.1; 100], 1, 0.0).len(), 100);
    }

    /// Le stéréo doit rester du stéréo, et de la même longueur des deux côtés.
    #[test]
    fn le_stereo_reste_aligne() {
        let g = sinus(440.0, 1.0);
        let d = sinus(660.0, 1.0);
        let entrelace: Vec<f32> = g.iter().zip(&d).flat_map(|(a, b)| [*a, *b]).collect();
        let out = etirer(&entrelace, 2, 1.5);
        assert_eq!(out.len() % 2, 0, "sortie stéréo impaire");
        let canaux = separer(&out, 2);
        assert_eq!(canaux[0].len(), canaux[1].len());
        assert!((dominante(&canaux[0]) - 440.0).abs() < 12.0);
        assert!((dominante(&canaux[1]) - 660.0).abs() < 12.0);
    }
}
