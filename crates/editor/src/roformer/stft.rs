// SPDX-License-Identifier: GPL-3.0-or-later
//! `torch.stft` / `torch.istft` tels que les appelle BS-RoFormer :
//! `center=True`, réflexion aux bords, fenêtre de Hann périodique, non
//! normalisée, un seul côté du spectre.
//!
//! En f64 : le réseau normalise chaque bande avant de la projeter, et dans
//! les bandes presque muettes l'arrondi d'une FFT en f32 n'est plus
//! négligeable devant le signal (constat de `UVR-rs`, qui fait de même).

use rustfft::FftPlanner;

pub type Complexe = rustfft::num_complex::Complex64;

/// `torch.hann_window(n)` (périodique).
pub fn hann(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
        .collect()
}

/// Spectre `[trames][n/2 + 1]` à plat.
pub fn directe(x: &[f32], n: usize, saut: usize, fenetre: &[f64]) -> Vec<Complexe> {
    let moitie = n / 2;
    let len = x.len();
    let trames = len / saut + 1;
    let cases = moitie + 1;
    let fft = FftPlanner::<f64>::new().plan_fft_forward(n);
    let mut tampon = vec![Complexe::default(); n];
    let mut sortie = Vec::with_capacity(trames * cases);
    for t in 0..trames {
        for (i, v) in tampon.iter_mut().enumerate() {
            // Position dans le signal prolongé de `moitie` par réflexion de chaque côté.
            let p = (t * saut + i) as isize - moitie as isize;
            let j = if p < 0 {
                (-p) as usize
            } else if (p as usize) < len {
                p as usize
            } else {
                2 * len - 2 - p as usize
            };
            *v = Complexe::new(x[j] as f64 * fenetre[i], 0.0);
        }
        fft.process(&mut tampon);
        sortie.extend_from_slice(&tampon[..cases]);
    }
    sortie
}

/// Inverse de [`directe`] : addition-recouvrement normalisée par l'enveloppe
/// des fenêtres au carré, rognée à `longueur`.
pub fn inverse(
    z: &[Complexe],
    cases: usize,
    n: usize,
    saut: usize,
    fenetre: &[f64],
    longueur: usize,
) -> Vec<f32> {
    let trames = z.len() / cases;
    let total = n + saut * (trames - 1);
    let ifft = FftPlanner::<f64>::new().plan_fft_inverse(n);
    let mut tampon = vec![Complexe::default(); n];
    let mut somme = vec![0.0f64; total];
    let mut enveloppe = vec![0.0f64; total];
    for t in 0..trames {
        let x = &z[t * cases..(t + 1) * cases];
        tampon[..cases].copy_from_slice(x);
        for k in cases..n {
            tampon[k] = x[n - k].conj();
        }
        ifft.process(&mut tampon);
        for i in 0..n {
            somme[t * saut + i] += tampon[i].re / n as f64 * fenetre[i];
            enveloppe[t * saut + i] += fenetre[i] * fenetre[i];
        }
    }
    let debut = n / 2;
    (0..longueur)
        .map(|i| {
            let j = debut + i;
            if j < total && enveloppe[j] > 1e-11 {
                (somme[j] / enveloppe[j]) as f32
            } else {
                0.0
            }
        })
        .collect()
}
