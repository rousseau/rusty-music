// SPDX-License-Identifier: GPL-3.0-or-later
//! Traitement du tampon audio décodé, avant remise à `rodio`.
//!
//! Deux étages, appliqués dans [`crate::ouvrir`] — donc hors du verrou
//! `Player`, sur le morceau déjà entièrement en RAM :
//!
//! 1. **Rééchantillonnage vers la fréquence de la carte son** — toujours actif.
//!    `rodio` 0.22 ne fait qu'une interpolation linéaire pour monter en
//!    fréquence et jette des échantillons pour descendre
//!    (`conversions::SampleRateConverter`), ce qui s'entend sur les rapports
//!    non entiers. `rubato` fait un sinc propre ; on rend alors à `rodio` un
//!    tampon déjà à la bonne fréquence, son convertisseur devient un
//!    passe-plat.
//!
//! 2. **Excitation psychoacoustique** (bouton « E ») — optionnelle, dosée par
//!    une intensité `0`..`1`. Synthétise les 2ᵉ et 3ᵉ harmoniques de la bande
//!    `[2,5 kHz, coupure]` et les ajoute — dans la région audible juste sous
//!    la coupure (présence, « air ») comme au-dessus d'elle. C'est l'excitateur
//!    par non-linéarité classique (Aphex, Larsen & Aarts 2004), transposé dans
//!    le domaine STFT pour rester sans repliement. Passe-plat si le morceau est
//!    déjà pleine bande.
//!
//! L'état est **global au processus** (`OnceLock` / `Atomic*`) : une seule
//! sortie audio, un seul auditeur, et cela évite de faire transiter le réglage
//! par la signature de `ouvrir` — qui rejaillirait sur `completer`,
//! `playback_state` et la CLI. Le code fait déjà ce choix pour les paramètres
//! audio en direct des stems (`multipiste.rs`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;

use rustfft::{num_complex::Complex, FftPlanner};

// ------------------------------------------------------------ sortie audio

/// Fréquence de la carte son, `0` tant que `Player::new` ne l'a pas posée.
static TAUX_SORTIE: AtomicU32 = AtomicU32::new(0);

/// Mémorise la fréquence de la sortie — appelé une fois à l'ouverture du lecteur.
pub fn enregistrer_taux_sortie(hz: u32) {
    TAUX_SORTIE.store(hz, Ordering::Relaxed);
}

fn taux_sortie() -> Option<u32> {
    match TAUX_SORTIE.load(Ordering::Relaxed) {
        0 => None,
        hz => Some(hz),
    }
}

// -------------------------------------------------------- état du bouton « E »

/// Intensité par défaut du bouton « E », si l'interface n'en fixe pas.
pub const INTENSITE_DEFAUT: f32 = 0.6;

/// Interrupteur de l'amélioration, et son intensité (`0.0`..=`1.0`). Une seule
/// instance, rendue par [`amelioration`].
pub struct Amelioration {
    actif: AtomicBool,
    /// `f32` rangé en bits — pas d'`AtomicF32` en std.
    intensite: AtomicU32,
}

static AMELIORATION: OnceLock<Amelioration> = OnceLock::new();

/// L'instance unique du processus.
pub fn amelioration() -> &'static Amelioration {
    AMELIORATION.get_or_init(|| Amelioration {
        actif: AtomicBool::new(false),
        intensite: AtomicU32::new(INTENSITE_DEFAUT.to_bits()),
    })
}

impl Amelioration {
    pub fn actif(&self) -> bool {
        self.actif.load(Ordering::Relaxed)
    }
    pub fn set_actif(&self, v: bool) {
        self.actif.store(v, Ordering::Relaxed);
    }
    /// Intensité courante, bornée à `[0, 1]`.
    pub fn intensite(&self) -> f32 {
        f32::from_bits(self.intensite.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }
    pub fn set_intensite(&self, v: f32) {
        self.intensite
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

// --------------------------------------------------------------- point d'entrée

/// Coupure estimée au-delà de laquelle le morceau est jugé pleine bande :
/// l'excitateur ne fait rien.
const FC_MAX_HZ: f32 = 18_000.0;
/// En deçà, pas de bande source exploitable — on s'abstient plutôt que de
/// fabriquer du spectre à partir de presque rien.
const FC_MIN_HZ: f32 = 3_000.0;
/// Bas de la bande source dont on tire les harmoniques. En dessous, on
/// toucherait au corps du son (voix, cuivres) au lieu de l'air.
const F_SOURCE_MIN_HZ: f32 = 2_500.0;
/// Gains des 2ᵉ et 3ᵉ harmoniques à intensité 1 (linéaires). L'intensité du
/// bouton les module de 0 à ces valeurs.
const GAIN_H2_MAX: f32 = 0.60;
const GAIN_H3_MAX: f32 = 0.28;

/// Traite le tampon entrelacé `ech` (`canaux` canaux entrelacés, `taux` Hz).
///
/// Peut réallouer `ech` (rééchantillonnage) ; renvoie le taux effectif du
/// tampon après traitement — c'est lui qu'il faut donner à `SamplesBuffer`.
pub fn traiter(ech: &mut Vec<f32>, taux: u32, canaux: u16) -> u32 {
    let mut taux = taux;

    if canaux == 0 || ech.is_empty() {
        return taux;
    }

    // 1. Rééchantillonnage vers la sortie.
    if let Some(cible) = taux_sortie() {
        if cible != taux {
            if let Some(sortie) = reechantillonner(ech, taux, cible, canaux) {
                *ech = sortie;
                taux = cible;
            }
        }
    }

    // 2. Excitation psychoacoustique (bouton « E »).
    let ame = amelioration();
    if ame.actif() {
        exciter(ech, taux, canaux, ame.intensite());
    }

    taux
}

// ------------------------------------------------------------ rééchantillonnage

/// Rééchantillonne un tampon entrelacé de `de` vers `vers` Hz. `None` si le
/// rééchantillonneur refuse la configuration — l'appelant garde alors le
/// tampon d'origine et laisse `rodio` faire sa conversion linéaire.
fn reechantillonner(ech: &[f32], de: u32, vers: u32, canaux: u16) -> Option<Vec<f32>> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler};

    let canaux = canaux as usize;
    let trames = ech.len() / canaux;
    if trames == 0 || canaux == 0 {
        return None;
    }

    // `process_all` réinitialise le rééchantillonneur, traite tout le clip
    // (dernier bloc partiel compris) et retire le retard de démarrage : la
    // sortie tient exactement les trames rééchantillonnées.
    let mut r = Fft::<f32>::new(de as usize, vers as usize, 4096, canaux, FixedSync::Input).ok()?;
    let entree = InterleavedSlice::new(ech, canaux, trames).ok()?;
    let sortie = r.process_all(&entree, trames, None).ok()?;
    Some(sortie.take_data())
}

// -------------------------------------------------------------------- exciter

/// Fenêtre STFT. 2048 à 44,1 kHz = 46 ms — comme le spectrogramme.
const N_FFT: usize = 2048;
/// Pas d'analyse : recouvrement 3/4, la fenêtre de Hann y respecte la
/// contrainte de reconstruction (COLA).
const HOP: usize = N_FFT / 4;

/// Synthétise des harmoniques (2ᵉ et 3ᵉ) de la bande
/// `[F_SOURCE_MIN_HZ, fc]` et les ajoute au tampon — dans la région audible
/// juste sous la coupure (présence, « air ») **et** au-dessus d'elle. `intensite`
/// (`0`..=`1`, celle du bouton « E ») dose les gains d'harmoniques.
///
/// N'ajoute rien si la coupure estimée sort de `[FC_MIN_HZ, FC_MAX_HZ)` ou si
/// l'intensité est nulle : un fichier déjà pleine bande ressort intact.
fn exciter(ech: &mut [f32], taux: u32, canaux: u16, intensite: f32) {
    let intensite = intensite.clamp(0.0, 1.0);
    if intensite <= 0.0 {
        return;
    }
    let canaux = canaux as usize;
    let trames = ech.len() / canaux;
    if trames < N_FFT {
        return;
    }

    let fc = estimer_coupure(ech, taux, canaux);
    if !(FC_MIN_HZ..FC_MAX_HZ).contains(&fc) {
        return;
    }

    let hz_par_raie = taux as f32 / N_FFT as f32;
    let nyquist = N_FFT / 2;
    let raie_fc = ((fc / hz_par_raie).round() as usize).min(nyquist - 1);
    let raie_src0 = ((F_SOURCE_MIN_HZ / hz_par_raie).round() as usize).max(1);
    if raie_src0 >= raie_fc {
        return;
    }

    // Courbe douce : perceptible dès le quart, généreuse en fin de course.
    let g2 = GAIN_H2_MAX * intensite * intensite;
    let g3 = GAIN_H3_MAX * intensite * intensite;

    let mut planner = FftPlanner::<f32>::new();
    let avant = planner.plan_fft_forward(N_FFT);
    let arriere = planner.plan_fft_inverse(N_FFT);
    let hann: Vec<f32> = fenetre_hann(N_FFT);

    // Somme des fenêtres² sur une période de recouvrement — constante loin des
    // bords, sert à normaliser l'addition-recouvrement.
    let mut norm_ola = 0.0f32;
    let mut i = 0;
    while i < N_FFT {
        norm_ola += hann[i] * hann[i];
        i += HOP;
    }
    let inv_ola = 1.0 / norm_ola.max(1e-6);
    let inv_fft = 1.0 / N_FFT as f32;

    let mut spectre = vec![Complex::new(0.0f32, 0.0); N_FFT];
    let mut synth = vec![Complex::new(0.0f32, 0.0); N_FFT];

    for c in 0..canaux {
        let sec: Vec<f32> = (0..trames).map(|t| ech[t * canaux + c]).collect();
        let mut humide = vec![0.0f32; trames];

        let mut pos = 0;
        while pos + N_FFT <= trames {
            for (k, s) in spectre.iter_mut().enumerate() {
                *s = Complex::new(sec[pos + k] * hann[k], 0.0);
            }
            avant.process(&mut spectre);

            synth.iter_mut().for_each(|s| *s = Complex::new(0.0, 0.0));
            let bande = raie_fc - raie_src0;
            for (offset, src) in spectre[raie_src0..raie_fc].iter().enumerate() {
                let k = raie_src0 + offset;
                let (ampl, phase) = (src.norm(), src.arg());
                // Décroissance vers l'aigu de la bande source : le haut du
                // spectre existant est déjà ténu, ses harmoniques ne doivent
                // pas dominer.
                let tilt = 1.0 - 0.6 * offset as f32 / bande as f32;

                // 2ᵉ harmonique : fréquence et phase doublées.
                let d2 = 2 * k;
                if d2 < nyquist {
                    let v = Complex::from_polar(ampl * g2 * tilt, 2.0 * phase);
                    synth[d2] += v;
                    synth[N_FFT - d2] += v.conj();
                }
                // 3ᵉ harmonique.
                let d3 = 3 * k;
                if d3 < nyquist {
                    let v = Complex::from_polar(ampl * g3 * tilt, 3.0 * phase);
                    synth[d3] += v;
                    synth[N_FFT - d3] += v.conj();
                }
            }

            arriere.process(&mut synth);
            for k in 0..N_FFT {
                humide[pos + k] += synth[k].re * inv_fft * hann[k];
            }
            pos += HOP;
        }

        for t in 0..trames {
            let out = sec[t] + humide[t] * inv_ola;
            ech[t * canaux + c] = out.clamp(-1.0, 1.0);
        }
    }
}

/// Estime la fréquence de coupure d'un signal : la plus haute fréquence où le
/// spectre lissé est encore ~30 dB sous le niveau moyen du bas médium.
/// `FC_MAX_HZ` si l'estimation est ambiguë (traité alors comme pleine bande).
fn estimer_coupure(ech: &[f32], taux: u32, canaux: usize) -> f32 {
    const N: usize = 4096;
    let trames = ech.len() / canaux;
    if trames < N {
        return FC_MAX_HZ;
    }

    let mut planner = FftPlanner::<f32>::new();
    let avant = planner.plan_fft_forward(N);
    let hann = fenetre_hann(N);
    let raies = N / 2;

    let mut spectre = vec![0.0f32; raies];
    let mut buf = vec![Complex::new(0.0f32, 0.0); N];
    let cibles = 24usize;
    let pas = ((trames - N) / cibles).max(N);
    let mut d = 0;
    let mut prises = 0;
    while d + N <= trames && prises < cibles {
        for (i, b) in buf.iter_mut().enumerate() {
            let mut s = 0.0f32;
            for c in 0..canaux {
                s += ech[(d + i) * canaux + c];
            }
            *b = Complex::new(s / canaux as f32 * hann[i], 0.0);
        }
        avant.process(&mut buf);
        for (k, v) in spectre.iter_mut().enumerate() {
            *v += buf[k].norm_sqr();
        }
        prises += 1;
        d += pas;
    }
    if prises == 0 {
        return FC_MAX_HZ;
    }

    // Lissage sur ~9 raies : un spectre brut est trop dentelé pour qu'un seuil
    // par raie isolée tienne.
    let demi = 4usize;
    let lisse: Vec<f32> = (0..raies)
        .map(|k| {
            let a = k.saturating_sub(demi);
            let b = (k + demi + 1).min(raies);
            spectre[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect();

    let hz_par_raie = taux as f32 / N as f32;
    let b_lo = ((300.0 / hz_par_raie) as usize).max(1);
    let b_hi = ((3_000.0 / hz_par_raie) as usize).min(raies).max(b_lo + 1);
    let reference = lisse[b_lo..b_hi].iter().sum::<f32>() / (b_hi - b_lo) as f32;
    if reference <= 1e-20 {
        return FC_MAX_HZ;
    }
    let seuil = reference * 1e-3; // −30 dB en puissance

    // On repart de l'aigu : la coupure est la première raie (en descendant)
    // dont la moyenne locale repasse au-dessus du seuil.
    for k in (b_hi..raies).rev() {
        if lisse[k] > seuil {
            let f = k as f32 * hz_par_raie;
            return if f >= FC_MAX_HZ { FC_MAX_HZ } else { f.max(FC_MIN_HZ) };
        }
    }
    FC_MAX_HZ
}

fn fenetre_hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = std::f32::consts::TAU * i as f32 / n as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sinus(freq: f32, sr: u32, secondes: f32) -> Vec<f32> {
        let n = (sr as f32 * secondes) as usize;
        (0..n)
            .map(|i| (i as f32 / sr as f32 * freq * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    /// Énergie moyenne d'une bande, par périodogramme grossier.
    fn energie_bande(x: &[f32], sr: u32, f0: f32, f1: f32) -> f32 {
        let n = 4096.min(x.len());
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        let hann = fenetre_hann(n);
        let mut buf: Vec<Complex<f32>> = (0..n)
            .map(|i| Complex::new(x[i] * hann[i], 0.0))
            .collect();
        fft.process(&mut buf);
        let hz = sr as f32 / n as f32;
        let (k0, k1) = ((f0 / hz) as usize, ((f1 / hz) as usize).min(n / 2));
        buf[k0..k1].iter().map(|c| c.norm_sqr()).sum::<f32>() / (k1 - k0).max(1) as f32
    }

    /// Bruit large bande passé-bas grossièrement à `fc` par somme de sinus à
    /// pas serré et phases variées — spectre dense, comme de la musique.
    fn bruit_passe_bas(fc: f32, sr: u32, secondes: f32) -> Vec<f32> {
        let n = (sr as f32 * secondes) as usize;
        let mut x = vec![0.0f32; n];
        let mut f = 40.0f32;
        let mut phase = 0.0f32;
        while f < fc {
            phase += 1.3;
            for (i, v) in x.iter_mut().enumerate() {
                *v += (i as f32 / sr as f32 * f * std::f32::consts::TAU + phase).sin();
            }
            f += 25.0;
        }
        let crete = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
        for v in &mut x {
            *v /= crete * 1.4;
        }
        x
    }

    #[test]
    fn exciter_cree_du_haut_sur_un_signal_bande_limite() {
        let sr = 44_100;
        let mut x = bruit_passe_bas(8_000.0, sr, 2.0);
        let avant = energie_bande(&x, sr, 9_000.0, 15_000.0);
        exciter(&mut x, sr, 1, INTENSITE_DEFAUT);
        let apres = energie_bande(&x, sr, 9_000.0, 15_000.0);
        assert!(
            apres > avant * 4.0,
            "la bande 9–15 kHz devrait gagner de l'énergie : {avant:.3e} -> {apres:.3e}"
        );
    }

    #[test]
    fn lintensite_dose_leffet() {
        let sr = 44_100;
        let base = bruit_passe_bas(8_000.0, sr, 2.0);

        let mut faible = base.clone();
        exciter(&mut faible, sr, 1, 0.3);
        let e_faible = energie_bande(&faible, sr, 9_000.0, 15_000.0);

        let mut fort = base.clone();
        exciter(&mut fort, sr, 1, 1.0);
        let e_fort = energie_bande(&fort, sr, 9_000.0, 15_000.0);

        assert!(
            e_fort > e_faible * 2.0,
            "intensité 1.0 doit dépasser 0.3 : {e_faible:.3e} vs {e_fort:.3e}"
        );
    }

    #[test]
    fn exciter_laisse_un_signal_pleine_bande_intact() {
        let sr = 44_100;
        let mut x = sinus(19_000.0, sr, 2.0); // au-dessus de FC_MAX
        let copie = x.clone();
        exciter(&mut x, sr, 1, INTENSITE_DEFAUT);
        let ecart: f32 = x
            .iter()
            .zip(&copie)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(ecart < 1e-6, "pleine bande : le tampon doit ressortir intact");
    }

    #[test]
    fn reechantillonner_44_vers_48_allonge_le_tampon() {
        let sr = 44_100;
        let x = sinus(1_000.0, sr, 1.0); // 1 s mono
        let y = reechantillonner(&x, 44_100, 48_000, 1).expect("rééchantillonnage");
        let attendu = 48_000f32;
        assert!(
            (y.len() as f32 - attendu).abs() < attendu * 0.02,
            "≈ 48000 échantillons attendus, {} obtenus",
            y.len()
        );
    }
}
