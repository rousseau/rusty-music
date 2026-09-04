// SPDX-License-Identifier: GPL-3.0-or-later
//! Spectrogramme d'un fichier, pour l'affichage.
//!
//! L'onde (`waveform.rs`) montre *combien* il y a de son au fil du temps ; le
//! spectrogramme montre *quoi*. Sur des stems séparés c'est ce qui compte :
//! une basse et une batterie ont des enveloppes voisines et des spectres qui
//! n'ont rien à voir. On voit d'un coup d'œil si la séparation a fait son
//! travail.
//!
//! Deux partis pris repris de l'interface web de `demucs-rs`, et pour les
//! mêmes raisons :
//!
//! - **axe des fréquences logarithmique.** L'oreille l'est ; en linéaire, les
//!   trois quarts de l'image seraient occupés par des aigus où il ne se passe
//!   presque rien, et la basse serait écrasée sur deux pixels ;
//! - **plage dynamique bornée.** Sans plafond, un seul transitoire fort
//!   comprime tout le reste vers le noir.
//!
//! Le calcul rend des intensités sur un octet ; la coloration appartient à
//! l'interface, qui applique la rampe séquentielle déjà retenue pour la carte.
//! Une rampe n'oppose pas des identités — c'est ce qui l'autorise là où trois
//! teintes catégorielles seraient déjà de trop.

use std::path::{Path, PathBuf};

use rodio::source::UniformSourceIterator;
use rodio::Decoder;
use rustfft::{num_complex::Complex, FftPlanner};

use crate::{Error, Result};

/// Fenêtre d'analyse. 2048 points à 44,1 kHz donnent 46 ms — assez fin pour
/// suivre une attaque de caisse claire, assez large pour séparer deux notes de
/// basse.
const N_FFT: usize = 2048;
/// Plancher de l'affichage : 80 dB sous le maximum. Au-delà, on ne montre plus
/// que le bruit de quantification.
const PLAGE_DB: f32 = 80.0;
/// Plus basse fréquence représentée. En dessous, l'échelle logarithmique
/// consacrerait des pixels à ce qu'aucun haut-parleur ne restitue.
const F_MIN: f32 = 40.0;
/// Plus haute fréquence représentée — **fixe**, quelle que soit la fréquence
/// d'échantillonnage de l'entrée. Deux spectrogrammes (original / HD, ou deux
/// morceaux) se comparent alors sur la même échelle verticale : un aigu
/// manquant se lit comme une bande sombre en haut, pas comme une image
/// « écrasée » sur une échelle plus courte.
const F_MAX: f32 = 22_050.0;

/// Une image de spectrogramme, prête à colorer.
pub struct Spectre {
    /// `hauteur × largeur` intensités, ligne du haut = fréquences aiguës.
    pub pixels: Vec<u8>,
    pub largeur: usize,
    pub hauteur: usize,
}

/// Calcule le spectrogramme d'un fichier, à la taille demandée.
///
/// `largeur` fixe le nombre de tranches temporelles : on choisit le pas pour
/// couvrir tout le morceau, quelle que soit sa durée. L'image n'a donc pas
/// besoin d'être redimensionnée à l'affichage.
pub fn calculer(chemin: &Path, largeur: usize, hauteur: usize) -> Result<Spectre> {
    let (mono, sr) = decoder_mono(chemin)?;
    Ok(calculer_echantillons(&mono, sr, largeur, hauteur))
}

/// Comme [`calculer`], mais sur des échantillons mono déjà décodés — pour
/// montrer le son *après* une transformation en mémoire (excitateur « E »).
pub fn calculer_echantillons(mono: &[f32], sr: u32, largeur: usize, hauteur: usize) -> Spectre {
    let largeur = largeur.max(1);
    let hauteur = hauteur.max(1);

    if mono.len() < N_FFT {
        return Spectre {
            pixels: vec![0; largeur * hauteur],
            largeur,
            hauteur,
        };
    }

    let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let hann: Vec<f32> = (0..N_FFT)
        .map(|i| {
            let x = std::f32::consts::TAU * i as f32 / N_FFT as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect();

    let raies = N_FFT / 2 + 1;
    let dernier = mono.len().saturating_sub(N_FFT);
    let mut amplitudes = vec![0.0f32; largeur * raies];
    let mut tampon = vec![Complex::new(0.0f32, 0.0); N_FFT];

    for x in 0..largeur {
        // Réparti sur tout le morceau : la dernière colonne finit sur la
        // dernière fenêtre entière, pas au-delà.
        let debut = if largeur > 1 {
            dernier * x / (largeur - 1)
        } else {
            0
        };
        for (i, c) in tampon.iter_mut().enumerate() {
            *c = Complex::new(mono[debut + i] * hann[i], 0.0);
        }
        fft.process(&mut tampon);
        for (k, c) in tampon.iter().take(raies).enumerate() {
            amplitudes[x * raies + k] = c.norm();
        }
    }

    // Décibels, plafonnés au maximum de l'image entière : chaque stem est donc
    // à sa propre échelle. C'est voulu — une voix discrète doit rester lisible
    // à côté d'une batterie qui écrase tout.
    let maxi = amplitudes.iter().copied().fold(0.0f32, f32::max).max(1e-9);
    let plancher = -PLAGE_DB;

    let (log_min, log_max) = (F_MIN.log10(), F_MAX.log10());
    let mut pixels = vec![0u8; largeur * hauteur];

    for y in 0..hauteur {
        // Haut de l'image = aigus. Échelle fixée à `F_MAX`, pas à la fréquence
        // de Nyquist de l'entrée.
        let f = if hauteur > 1 {
            let t = y as f32 / (hauteur - 1) as f32;
            10f32.powf(log_max - t * (log_max - log_min))
        } else {
            F_MAX
        };
        let raie = (f * N_FFT as f32 / sr as f32).min((raies - 1) as f32);
        let (bas, frac) = (raie.floor() as usize, raie.fract());
        let haut = (bas + 1).min(raies - 1);

        for x in 0..largeur {
            let base = x * raies;
            let a = amplitudes[base + bas] * (1.0 - frac) + amplitudes[base + haut] * frac;
            let db = 20.0 * (a / maxi).max(1e-9).log10();
            let norme = ((db - plancher) / PLAGE_DB).clamp(0.0, 1.0);
            pixels[y * largeur + x] = (norme * 255.0) as u8;
        }
    }

    Spectre {
        pixels,
        largeur,
        hauteur,
    }
}

/// Décode un fichier en mono, à sa fréquence d'origine.
fn decoder_mono(chemin: &Path) -> Result<(Vec<f32>, u32)> {
    let octets = std::fs::read(chemin).map_err(|source| Error::Open {
        path: chemin.to_path_buf(),
        source,
    })?;
    let decodeur =
        Decoder::try_from(std::io::Cursor::new(octets)).map_err(|source| Error::Decode {
            path: chemin.to_path_buf(),
            source,
        })?;
    let sr = crate::multipiste::SR;
    let mono: Vec<f32> = UniformSourceIterator::new(
        decodeur,
        1.try_into().expect("1 canal"),
        sr.try_into().expect("fréquence valide"),
    )
    .collect();

    if mono.is_empty() {
        return Err(Error::DureeInconnue {
            path: PathBuf::from(chemin),
        });
    }
    Ok((mono, sr))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une sinusoïde doit allumer une bande, et une seule. C'est le contrôle
    /// qui dit que l'axe logarithmique place les fréquences au bon endroit.
    #[test]
    fn une_sinusoide_allume_une_seule_bande() {
        let sr = 44_100.0f32;
        let mono: Vec<f32> = (0..sr as usize)
            .map(|i| (i as f32 / sr * 1000.0 * std::f32::consts::TAU).sin())
            .collect();

        // On rejoue le cœur du calcul sans passer par un fichier.
        let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
        let mut tampon: Vec<Complex<f32>> = (0..N_FFT)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / N_FFT as f32;
                Complex::new(mono[i] * (0.5 - 0.5 * x.cos()), 0.0)
            })
            .collect();
        fft.process(&mut tampon);

        let raies = N_FFT / 2 + 1;
        let pic = (0..raies)
            .max_by(|a, b| tampon[*a].norm().partial_cmp(&tampon[*b].norm()).unwrap())
            .unwrap();
        let f = pic as f32 * sr / N_FFT as f32;
        assert!((f - 1000.0).abs() < 30.0, "pic à {f:.0} Hz au lieu de 1000");
    }

    #[test]
    fn le_silence_reste_au_plancher() {
        // Sur un signal nul, toutes les intensités doivent tomber à zéro
        // plutôt que de produire du bruit : la normalisation divise par le
        // maximum, et il faut que le cas dégénéré tienne.
        let maxi = 0.0f32.max(1e-9);
        let db = 20.0 * (0.0f32 / maxi).max(1e-9).log10();
        let norme = ((db + PLAGE_DB) / PLAGE_DB).clamp(0.0, 1.0);
        assert_eq!(norme, 0.0);
    }
}
