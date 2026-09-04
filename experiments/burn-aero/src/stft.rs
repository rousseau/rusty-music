//! STFT / iSTFT reproduisant `aero/src/models/spec.py` (qui appelle
//! `torch.stft` / `torch.istft`), pour la voie « réseau seul + STFT en Rust ».
//!
//! `_spec`  : n_fft 512, hop 64, win_length 128 (fenêtre de Hann périodique,
//!            zéro-complétée à 512, centrée), `normalized` (÷ √512), `center`
//!            (repli de 256 de part et d'autre), bin de Nyquist retiré → 256.
//! `_ispec` : n_fft 512, hop 256, win_length 512 (Hann périodique),
//!            `normalized` (× √512), `center` (rognage de 256 de part et
//!            d'autre), bin de Nyquist remis à zéro.

use rustfft::{num_complex::Complex, FftPlanner};

pub const NFFT: usize = 512;
pub const BINS: usize = 256; // NFFT/2, Nyquist retiré
pub const HOP_IN: usize = 64;
pub const WIN_IN: usize = 128;
pub const HOP_OUT: usize = 256;

/// Fenêtre de Hann **périodique** de longueur `n` : `0.5 - 0.5·cos(2πk/n)`.
fn hann_periodique(n: usize) -> Vec<f32> {
    (0..n)
        .map(|k| 0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / n as f32).cos())
        .collect()
}

/// Repli type `pad_mode='reflect'` de `p` échantillons de chaque côté
/// (numpy/torch : le bord n'est pas répété — `abcd` → `dcb|abcd|cba`).
fn reflect_pad(x: &[f32], p: usize) -> Vec<f32> {
    let n = x.len();
    let mut out = Vec::with_capacity(n + 2 * p);
    for i in 0..p {
        out.push(x[p - i]);
    }
    out.extend_from_slice(x);
    for i in 0..p {
        out.push(x[n - 2 - i]);
    }
    out
}

/// `_spec` : `x` (mono, `lr_sr`) → `[2, BINS, T]` entrelacé `(réel, imag)`
/// canal-major, comme `_move_complex_to_channels_dim`.
pub fn spec(x: &[f32]) -> (Vec<f32>, usize) {
    // AERO complète d'abord à un multiple du hop.
    let mut x = x.to_vec();
    let reste = x.len() % HOP_IN;
    if reste != 0 {
        x.extend(std::iter::repeat(0.0).take(HOP_IN - reste));
    }
    let n = x.len();
    let t_frames = n / HOP_IN + 1; // center=True

    // Fenêtre 128 centrée dans 512.
    let hann = hann_periodique(WIN_IN);
    let mut fenetre = vec![0.0f32; NFFT];
    let deb = (NFFT - WIN_IN) / 2;
    fenetre[deb..deb + WIN_IN].copy_from_slice(&hann);

    let pad = reflect_pad(&x, NFFT / 2);
    let fft = FftPlanner::<f32>::new().plan_fft_forward(NFFT);
    let norm = 1.0 / (NFFT as f32).sqrt();

    let mut reel = vec![0.0f32; BINS * t_frames];
    let mut imag = vec![0.0f32; BINS * t_frames];
    let mut buf = vec![Complex::new(0.0f32, 0.0); NFFT];

    for t in 0..t_frames {
        let base = t * HOP_IN;
        for i in 0..NFFT {
            buf[i] = Complex::new(pad[base + i] * fenetre[i], 0.0);
        }
        fft.process(&mut buf);
        for k in 0..BINS {
            reel[k * t_frames + t] = buf[k].re * norm;
            imag[k * t_frames + t] = buf[k].im * norm;
        }
    }

    let mut out = Vec::with_capacity(2 * BINS * t_frames);
    out.extend_from_slice(&reel);
    out.extend_from_slice(&imag);
    (out, t_frames)
}

/// `_ispec` : `[2, BINS, T]` → forme d'onde à `hr_sr`, longueur naturelle
/// `HOP_OUT·(T-1) + NFFT - NFFT` (rognage center) = `HOP_OUT·(T-1)`... en
/// pratique on rend `HOP_OUT·(T-1) + NFFT - 2·(NFFT/2)`.
pub fn ispec(spec: &[f32], t_frames: usize) -> Vec<f32> {
    let reel = &spec[..BINS * t_frames];
    let imag = &spec[BINS * t_frames..];

    let hann = hann_periodique(NFFT); // win_out = 512
    let ifft = FftPlanner::<f32>::new().plan_fft_inverse(NFFT);
    let denorm = (NFFT as f32).sqrt() / NFFT as f32; // normalized inverse × √N, et irfft ÷ N

    let total = NFFT + HOP_OUT * (t_frames - 1);
    let mut y = vec![0.0f32; total];
    let mut env = vec![0.0f32; total];
    let mut buf = vec![Complex::new(0.0f32, 0.0); NFFT];

    for t in 0..t_frames {
        // Reconstruire le spectre complet 512 par symétrie hermitienne.
        // bin 256 (Nyquist) = 0 (F.pad(z,(0,0,0,1))).
        buf[0] = Complex::new(reel[0 * t_frames + t], imag[0 * t_frames + t]);
        for k in 1..BINS {
            let c = Complex::new(reel[k * t_frames + t], imag[k * t_frames + t]);
            buf[k] = c;
            buf[NFFT - k] = c.conj();
        }
        buf[BINS] = Complex::new(0.0, 0.0); // Nyquist

        ifft.process(&mut buf);
        let base = t * HOP_OUT;
        for i in 0..NFFT {
            let v = buf[i].re * denorm * hann[i];
            y[base + i] += v;
            env[base + i] += hann[i] * hann[i];
        }
    }

    for i in 0..total {
        if env[i] > 1e-8 {
            y[i] /= env[i];
        }
    }
    // center=True : rogner NFFT/2 de chaque côté.
    y[NFFT / 2..total - NFFT / 2].to_vec()
}
