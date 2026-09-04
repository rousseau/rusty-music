// SPDX-License-Identifier: GPL-3.0-or-later
//! Spectrogrammes log-mel, au format attendu par l'encodeur audio de CLAP.
//!
//! Les valeurs ci-dessous ne sont pas négociables : elles reproduisent le
//! prétraitement de `ClapFeatureExtractor` pour `laion/clap-htsat-unfused`.
//! Un banc de filtres ou une échelle qui s'en écarteraient donneraient des
//! empreintes silencieusement fausses — le modèle ne s'en plaindrait pas, il
//! rendrait simplement des vecteurs qui ne veulent rien dire.
//!
//! Fenêtre de Hann de 1024, pas de 480, 64 bandes mel entre 50 Hz et 14 kHz,
//! échelle HTK, filtres non normalisés, spectre de puissance, puis décibels.

use std::sync::Arc;

use rustfft::{num_complex::Complex32, Fft, FftPlanner};

/// Fréquence d'échantillonnage attendue par le modèle.
pub const SR: u32 = 48_000;
pub const N_FFT: usize = 1024;
pub const HOP: usize = 480;
pub const N_MELS: usize = 64;
const FMIN: f32 = 50.0;
const FMAX: f32 = 14_000.0;
/// Nombre de raies utiles d'une FFT réelle de `N_FFT` points.
const N_RAIES: usize = N_FFT / 2 + 1;

/// Durée d'une fenêtre d'analyse, en secondes.
pub const FENETRE_S: usize = 10;
/// Échantillons couverts par une fenêtre.
pub const FENETRE_N: usize = SR as usize * FENETRE_S;
/// Trames produites. Le rembourrage centré ajoute une demi-fenêtre de chaque
/// côté, d'où `1 + n / hop` et non `1 + (n - n_fft) / hop`.
pub const TRAMES: usize = FENETRE_N / HOP + 1;

fn hz_vers_mel(f: f32) -> f32 {
    2595.0 * (1.0 + f / 700.0).log10()
}
fn mel_vers_hz(m: f32) -> f32 {
    700.0 * (10f32.powf(m / 2595.0) - 1.0)
}

/// Calculateur réutilisable : le plan FFT et le banc de filtres ne dépendent
/// pas du signal, les construire à chaque fenêtre serait du gaspillage.
pub struct Mel {
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    /// Banc triangulaire, à plat : `N_MELS × N_RAIES`.
    filtres: Vec<f32>,
}

impl Default for Mel {
    fn default() -> Self {
        Self::new()
    }
}

impl Mel {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(N_FFT);

        // Hann périodique — celle qu'utilise `window_function(..., "hann")`,
        // divisée par (N) et non (N-1) comme la variante symétrique.
        let hann = (0..N_FFT)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / N_FFT as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();

        // Filtres triangulaires régulièrement espacés sur l'échelle mel.
        let bornes: Vec<f32> = (0..N_MELS + 2)
            .map(|i| {
                let m = hz_vers_mel(FMIN)
                    + (hz_vers_mel(FMAX) - hz_vers_mel(FMIN)) * i as f32 / (N_MELS + 1) as f32;
                mel_vers_hz(m)
            })
            .collect();

        let mut filtres = vec![0.0f32; N_MELS * N_RAIES];
        for m in 0..N_MELS {
            let (gauche, centre, droite) = (bornes[m], bornes[m + 1], bornes[m + 2]);
            for k in 0..N_RAIES {
                let f = SR as f32 * 0.5 * k as f32 / (N_RAIES - 1) as f32;
                let poids = if f > gauche && f < centre {
                    (f - gauche) / (centre - gauche)
                } else if f >= centre && f < droite {
                    (droite - f) / (droite - centre)
                } else {
                    0.0
                };
                filtres[m * N_RAIES + k] = poids;
            }
        }

        Self { fft, hann, filtres }
    }

    /// Spectrogramme log-mel d'une fenêtre, en `TRAMES × N_MELS` valeurs.
    ///
    /// `signal` doit être mono à [`SR`]. Plus court qu'une fenêtre, il est
    /// répété pour la remplir — c'est le `repeatpad` de CLAP, qui évite
    /// d'injecter un long silence que le modèle interpréterait.
    pub fn spectrogramme(&self, signal: &[f32]) -> Vec<f32> {
        let mut x = vec![0.0f32; FENETRE_N];
        if signal.is_empty() {
            // Rien à analyser : plancher partout.
            return vec![-100.0; TRAMES * N_MELS];
        }
        for (i, v) in x.iter_mut().enumerate() {
            *v = signal[i % signal.len()];
        }

        let demi = N_FFT / 2;
        let mut sortie = vec![0.0f32; TRAMES * N_MELS];
        let mut tampon = vec![Complex32::new(0.0, 0.0); N_FFT];
        let mut puissance = vec![0.0f32; N_RAIES];

        for t in 0..TRAMES {
            // Rembourrage centré par réflexion, appliqué à la volée : recopier
            // tout le signal rembourré coûterait une allocation par fenêtre.
            let debut = t * HOP;
            for (i, c) in tampon.iter_mut().enumerate() {
                let idx = debut + i;
                let j = if idx < demi {
                    demi - idx
                } else if idx - demi < FENETRE_N {
                    idx - demi
                } else {
                    // Réflexion en fin de signal.
                    2 * FENETRE_N - 2 - (idx - demi)
                };
                *c = Complex32::new(x[j.min(FENETRE_N - 1)] * self.hann[i], 0.0);
            }
            self.fft.process(&mut tampon);

            for (k, p) in puissance.iter_mut().enumerate() {
                *p = tampon[k].norm_sqr();
            }

            for m in 0..N_MELS {
                let bande = &self.filtres[m * N_RAIES..(m + 1) * N_RAIES];
                let energie: f32 = bande.iter().zip(&puissance).map(|(w, p)| w * p).sum();
                // Décibels, avec le même plancher que `power_to_db`.
                sortie[t * N_MELS + m] = 10.0 * energie.max(1e-10).log10();
            }
        }
        sortie
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une sinusoïde pure doit concentrer son énergie sur la bande mel qui
    /// contient sa fréquence. C'est ce qui distingue un banc de filtres juste
    /// d'un banc mal échelonné — lequel produirait des empreintes fausses
    /// sans jamais lever d'erreur.
    #[test]
    fn une_sinusoide_tombe_dans_la_bonne_bande() {
        let mel = Mel::new();
        for hz in [200.0f32, 1000.0, 5000.0] {
            let signal: Vec<f32> = (0..SR as usize)
                .map(|i| (std::f32::consts::TAU * hz * i as f32 / SR as f32).sin())
                .collect();
            let spec = mel.spectrogramme(&signal);

            // Bande la plus énergique sur une trame du milieu.
            let t = TRAMES / 2;
            let trame = &spec[t * N_MELS..(t + 1) * N_MELS];
            let pic = trame
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .unwrap()
                .0;

            // Bande attendue : celle dont le centre encadre la fréquence.
            let attendue = (0..N_MELS)
                .min_by(|&a, &b| {
                    let c = |m: usize| {
                        let mm = hz_vers_mel(FMIN)
                            + (hz_vers_mel(FMAX) - hz_vers_mel(FMIN)) * (m + 1) as f32
                                / (N_MELS + 1) as f32;
                        (mel_vers_hz(mm) - hz).abs()
                    };
                    c(a).partial_cmp(&c(b)).unwrap()
                })
                .unwrap();

            assert!(
                pic.abs_diff(attendue) <= 1,
                "{hz} Hz : pic en bande {pic}, attendu {attendue}"
            );
        }
    }

    #[test]
    fn la_forme_correspond_a_lentree_du_modele() {
        let mel = Mel::new();
        let spec = mel.spectrogramme(&vec![0.1; 1000]);
        assert_eq!(spec.len(), TRAMES * N_MELS);
        assert_eq!(TRAMES, 1001, "le modèle est figé sur 1001 trames");
        assert_eq!(N_MELS, 64);
    }

    #[test]
    fn le_silence_reste_au_plancher() {
        let mel = Mel::new();
        let spec = mel.spectrogramme(&vec![0.0; FENETRE_N]);
        assert!(spec.iter().all(|&v| v <= -99.0), "le silence doit plancher");
    }
}
