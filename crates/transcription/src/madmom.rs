// SPDX-License-Identifier: GPL-3.0-or-later
//! Le spectrogramme de madmom (BSD), réécrit : `FramedSignalProcessor`
//! (trames de 2048 centrées, 100 par seconde), `ShortTimeFourierTransform`
//! (fenêtre de Hann), `FilteredSpectrogram` (12 bandes par octave, filtres
//! triangulaires normalisés), `LogarithmicSpectrogram` (`log10(1 + x)`).
//!
//! Partagé par ADTOF ([`crate::batterie`], 20 Hz–20 kHz, 84 bandes) et
//! all-in-one ([`crate::sections`], 30 Hz–17 kHz, 81 bandes).

use std::sync::Arc;

/// Fréquence attendue du signal.
pub const SR: u32 = 44_100;
pub const TRAME: usize = 2048;
pub const FPS: usize = 100;
const SAUT: f64 = SR as f64 / FPS as f64;
const BANDES_OCTAVE: f64 = 12.0;

/// Le banc de filtres de madmom (`LogarithmicFilterbank`, filtres normalisés,
/// bandes uniques) : `bins × bandes`, colonne par colonne.
pub fn banc_de_filtres(fmin: f64, fmax: f64) -> (Vec<f32>, usize) {
    let n_bins = TRAME / 2;
    let freq_bin = |k: usize| k as f64 * SR as f64 / TRAME as f64;
    // log_frequencies(12, fmin, fmax, 440)
    let gauche = ((fmin / 440.0).log2() * BANDES_OCTAVE).floor() as i64;
    let droite = ((fmax / 440.0).log2() * BANDES_OCTAVE).ceil() as i64;
    let freqs: Vec<f64> = (gauche..droite)
        .map(|k| 440.0 * 2f64.powf(k as f64 / BANDES_OCTAVE))
        .filter(|f| (fmin..=fmax).contains(f))
        .collect();
    // frequencies2bins(..., unique_bins=True) : le bin le plus proche.
    let mut bins: Vec<usize> = freqs
        .iter()
        .map(|&f| {
            let i = (0..n_bins).position(|k| freq_bin(k) >= f).unwrap_or(n_bins).clamp(1, n_bins - 1);
            let (g, d) = (freq_bin(i - 1), freq_bin(i));
            if f - g < d - f { i - 1 } else { i }
        })
        .collect();
    bins.dedup();
    // TriangularFilter.band_bins(overlap=True) puis filtres normalisés.
    let bandes = bins.len().saturating_sub(2);
    let mut fb = vec![0.0f32; n_bins * bandes];
    for b in 0..bandes {
        let (debut, mut centre, mut fin) = (bins[b], bins[b + 1], bins[b + 2]);
        if fin - debut < 2 {
            centre = debut;
            fin = debut + 1;
        }
        let (c, n) = (centre - debut, fin - debut);
        let mut donnees = vec![0.0f64; n];
        for (k, v) in donnees.iter_mut().enumerate().take(c) {
            *v = k as f64 / c as f64;
        }
        for (k, v) in donnees.iter_mut().enumerate().skip(c) {
            *v = 1.0 - (k - c) as f64 / (n - c) as f64;
        }
        let somme: f64 = donnees.iter().sum();
        for (k, v) in donnees.iter().enumerate() {
            let x = (*v / somme) as f32;
            let case = &mut fb[(debut + k) * bandes + b];
            *case = case.max(x);
        }
    }
    (fb, bandes)
}

/// Spectrogramme log filtré de madmom : `trames × bandes`, ligne par ligne.
///
/// `gauche`/`droite` : le signal stéréo à 44,1 kHz, en `f32` dans [−1, 1]
/// (tel que le rendent nos décodeurs depuis un WAV 16 bits). Comme madmom, on
/// repasse en entiers 16 bits et on moyenne les canaux **en tronquant**.
pub fn spectre_log(gauche: &[f32], droite: &[f32], fmin: f64, fmax: f64) -> (Vec<f32>, usize, usize) {
    let mono: Vec<f64> = gauche
        .iter()
        .zip(droite)
        .map(|(g, d)| {
            let g = (*g as f64 * 32768.0).round().clamp(-32768.0, 32767.0);
            let d = (*d as f64 * 32768.0).round().clamp(-32768.0, 32767.0);
            ((g + d) / 2.0).trunc()
        })
        .collect();
    let n = mono.len();
    let trames = (n as f64 / SAUT).ceil() as usize;
    let (fb, bandes) = banc_de_filtres(fmin, fmax);
    let fenetre: Vec<f64> = (0..TRAME)
        .map(|k| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / (TRAME - 1) as f64).cos()) / 32767.0)
        .collect();
    let fft: Arc<dyn rustfft::Fft<f64>> = rustfft::FftPlanner::new().plan_fft_forward(TRAME);
    let mut tampon = vec![rustfft::num_complex::Complex::new(0.0f64, 0.0); TRAME];
    let mut sortie = vec![0.0f32; trames * bandes];
    let mut spectre = vec![0.0f32; TRAME / 2];
    // Chaque filtre ne couvre que quelques cases : on ne garde que celles-là,
    // dans l'ordre (même somme, au bit près).
    let creux: Vec<Vec<(usize, f32)>> =
        (0..bandes).map(|b| (0..TRAME / 2).map(|k| (k, fb[k * bandes + b])).filter(|&(_, w)| w != 0.0).collect()).collect();
    for t in 0..trames {
        let debut = (t as f64 * SAUT) as i64 - (TRAME / 2) as i64;
        for (k, c) in tampon.iter_mut().enumerate() {
            let i = debut + k as i64;
            let x = if i >= 0 && (i as usize) < n { mono[i as usize] } else { 0.0 };
            *c = rustfft::num_complex::Complex::new(x * fenetre[k], 0.0);
        }
        fft.process(&mut tampon);
        for (k, s) in spectre.iter_mut().enumerate() {
            *s = tampon[k].norm() as f32;
        }
        for (b, filtre) in creux.iter().enumerate() {
            let acc: f32 = filtre.iter().map(|&(k, w)| spectre[k] * w).sum();
            sortie[t * bandes + b] = (1.0 + acc).log10();
        }
    }
    (sortie, trames, bandes)
}
