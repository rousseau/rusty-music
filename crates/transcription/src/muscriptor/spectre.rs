// SPDX-License-Identifier: GPL-3.0-or-later
//! Module du spectre d'un segment, comme `torch.stft` dans le
//! `_MelSpectrogram` d'origine : n_fft 2048, pas 160 (100 trames/s à 16 kHz),
//! fenêtre de Hann périodique, centré avec réflexion, puissance 1. Le banc de
//! filtres mel et le logarithme sont appliqués par le réseau (`reseau.rs`),
//! sur le même périphérique que lui.

use rustfft::{num_complex::Complex, FftPlanner};

pub const N_FFT: usize = 2048;
pub const PAS: usize = 160;
pub const BINS: usize = N_FFT / 2 + 1;

/// `[trames, BINS]` à plat, trames = 1 + len / PAS.
pub fn module(x: &[f32], fenetre: &[f32]) -> (Vec<f32>, usize) {
    assert_eq!(fenetre.len(), N_FFT);
    let m = N_FFT / 2;
    let n = x.len();
    // Réflexion sans répéter le bord (`pad_mode="reflect"`).
    let reflet = |i: isize| -> f32 {
        let n = n as isize;
        let mut j = i;
        if j < 0 {
            j = -j;
        }
        if j >= n {
            j = 2 * (n - 1) - j;
        }
        x[j.clamp(0, n - 1) as usize]
    };
    let trames = 1 + n / PAS;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let mut tampon = vec![Complex::new(0.0, 0.0); N_FFT];
    let mut sortie = vec![0.0f32; trames * BINS];
    for t in 0..trames {
        let debut = (t * PAS) as isize - m as isize;
        for (k, c) in tampon.iter_mut().enumerate() {
            *c = Complex::new(reflet(debut + k as isize) * fenetre[k], 0.0);
        }
        fft.process(&mut tampon);
        for (k, c) in tampon[..BINS].iter().enumerate() {
            sortie[t * BINS + k] = c.norm();
        }
    }
    (sortie, trames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_sinusoide_tombe_dans_son_bin() {
        let sr = 16_000.0;
        let x: Vec<f32> = (0..16_000).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin()).collect();
        let fen: Vec<f32> = (0..N_FFT).map(|k| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * k as f32 / N_FFT as f32).cos()).collect();
        let (s, trames) = module(&x, &fen);
        assert_eq!(trames, 101);
        let t = 50;
        let pic = (0..BINS).max_by(|&a, &b| s[t * BINS + a].total_cmp(&s[t * BINS + b])).unwrap();
        assert_eq!(pic, (1000.0 * N_FFT as f32 / sr).round() as usize); // 128
    }
}
