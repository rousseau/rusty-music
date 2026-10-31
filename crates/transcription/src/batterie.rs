// SPDX-License-Identifier: GPL-3.0-or-later
//! Transcription de la batterie : ADTOF « Frame_RNN » (Zehren, Alunno,
//! Bientinesi, « High-Quality and Reproducible Automatic Drum Transcription
//! from Crowdsourced Data », Signals 2023), entraîné sur 359 h de vraie
//! musique, 5 classes : grosse caisse, caisse claire, toms, charleston,
//! cymbales (crash et ride).
//!
//! Les poids (CC BY-NC-SA 4.0, comme tout le dépôt ADTOF) sont convertis en
//! ONNX par `experiments/batterie/exporter_adtof.py` ; **le code d'ADTOF n'est
//! ni copié ni lié**. Ce qui l'entoure est réécrit ici d'après madmom (BSD),
//! que ADTOF emploie :
//! - [`caracteristiques`] : mono 16 bits, trames de 2048 centrées à 100 par
//!   seconde, fenêtre de Hann, spectre filtré en 84 bandes logarithmiques
//!   (12 par octave, 20 Hz–20 kHz), `log10(1 + x)` ;
//! - [`coups`] : le choix de pics de `NotePeakPickingProcessor`, avec les
//!   seuils par classe d'ADTOF.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{madmom, Error, Result};

/// Fréquence attendue du signal.
pub const SR: u32 = 44_100;
const FMIN: f64 = 20.0;
const FMAX: f64 = 20_000.0;

/// Les classes, dans l'ordre des sorties du réseau.
pub const CLASSES: [Piece; 5] = [
    Piece::GrosseCaisse,
    Piece::CaisseClaire,
    Piece::Toms,
    Piece::Charleston,
    Piece::Cymbales,
];
/// Seuils de détection par classe, réglés par ADTOF sur sa validation.
pub const SEUILS: [f32; 5] = [0.22, 0.24, 0.32, 0.22, 0.30];

/// Le fichier ONNX produit par la préparation.
pub const FICHIER: &str = "adtof_frame_rnn.onnx";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Piece {
    GrosseCaisse,
    CaisseClaire,
    Toms,
    Charleston,
    Cymbales,
}

/// Un coup détecté.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coup {
    pub instant_s: f32,
    pub piece: Piece,
    /// Activation du réseau au pic, de 0 à 1.
    pub force: f32,
}

/// Les caractéristiques d'ADTOF : `trames × bandes`, ligne par ligne.
///
/// `gauche`/`droite` : le signal stéréo à 44,1 kHz, en `f32` dans [−1, 1]
/// (tel que le rendent nos décodeurs depuis un WAV 16 bits). Voir
/// [`madmom::spectre_log`].
pub fn caracteristiques(gauche: &[f32], droite: &[f32]) -> (Vec<f32>, usize, usize) {
    madmom::spectre_log(gauche, droite, FMIN, FMAX)
}

pub struct Batteur {
    session: ort::session::Session,
}

impl Batteur {
    pub fn charger(chemin: &Path) -> Result<Self> {
        let session = ort::session::Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)?
            .commit_from_file(chemin)?;
        Ok(Self { session })
    }

    pub fn charger_installe() -> Result<Self> {
        let chemin = rusty_music_core::modeles::trouver(FICHIER).ok_or_else(|| {
            Error::PoidsAbsents(format!(
                "{}\n  ./scripts/preparer-adtof.sh",
                rusty_music_core::modeles::introuvable(FICHIER)
            ))
        })?;
        Self::charger(&chemin)
    }

    /// Activations par trame, `trames × 5`.
    pub fn activations(&mut self, carac: &[f32], trames: usize, bandes: usize) -> Result<Vec<f32>> {
        let entree =
            ndarray::Array::from_shape_vec((1, trames, bandes, 1), carac.to_vec()).expect("forme");
        let tenseur = ort::value::Tensor::from_array(entree)?;
        let sortie = self.session.run(ort::inputs!["x" => tenseur.view()])?;
        let (_, a) = sortie[0].try_extract_tensor::<f32>()?;
        if a.len() != trames * CLASSES.len() {
            return Err(Error::Sortie(format!(
                "{} valeurs pour {trames} trames",
                a.len()
            )));
        }
        Ok(a.to_vec())
    }
}

/// Le choix de pics de madmom (`NotePeakPickingProcessor`, lissage nul,
/// moyenne sur 10 trames avant et 1 après, maximum sur 2 avant et 1 après,
/// coups d'une même classe à moins de 20 ms fusionnés sur le premier).
pub fn coups(activations: &[f32], trames: usize) -> Vec<Coup> {
    coups_avec(activations, trames, &SEUILS)
}

/// [`coups`] avec d'autres seuils (bancs).
pub fn coups_avec(activations: &[f32], trames: usize, seuils: &[f32; 5]) -> Vec<Coup> {
    const AVANT_MOY: usize = 10;
    const APRES_MOY: usize = 1;
    const AVANT_MAX: usize = 2;
    const APRES_MAX: usize = 1;
    const FUSION_S: f32 = 0.02 + 1e-12;
    let n_classes = CLASSES.len();
    let mut tous = Vec::new();
    for (c, piece) in CLASSES.iter().enumerate() {
        let a: Vec<f32> = (0..trames)
            .map(|t| activations[t * n_classes + c])
            .collect();
        let lire = |v: &[f32], i: i64| {
            if i >= 0 && (i as usize) < trames {
                v[i as usize]
            } else {
                0.0
            }
        };
        let detections: Vec<f32> = (0..trames as i64)
            .map(|i| {
                let moy: f32 = (i - AVANT_MOY as i64..=i + APRES_MOY as i64)
                    .map(|j| lire(&a, j))
                    .sum::<f32>()
                    / (AVANT_MOY + APRES_MOY + 1) as f32;
                let x = a[i as usize];
                if x >= moy + seuils[c] {
                    x
                } else {
                    0.0
                }
            })
            .collect();
        let mut instants: Vec<(f32, f32)> = Vec::new();
        for i in 0..trames as i64 {
            let d = detections[i as usize];
            if d == 0.0 {
                continue;
            }
            let max = (i - AVANT_MAX as i64..=i + APRES_MAX as i64)
                .map(|j| lire(&detections, j))
                .fold(0.0, f32::max);
            if d == max {
                instants.push((i as f32 / madmom::FPS as f32, d));
            }
        }
        // combine_events(…, 'left')
        let mut garde: Vec<(f32, f32)> = Vec::new();
        for (t, f) in instants {
            match garde.last() {
                Some(&(g, _)) if t - g <= FUSION_S => {}
                _ => garde.push((t, f)),
            }
        }
        tous.extend(garde.into_iter().map(|(t, f)| Coup {
            instant_s: t,
            piece: *piece,
            force: f,
        }));
    }
    tous.sort_by(|a, b| {
        a.instant_s
            .total_cmp(&b.instant_s)
            .then(a.piece.cmp(&b.piece))
    });
    tous
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quatre_vingt_quatre_bandes_comme_madmom() {
        let (fb, bandes) = madmom::banc_de_filtres(FMIN, FMAX);
        assert_eq!(bandes, 84);
        // Chaque filtre est normalisé.
        for b in 0..bandes {
            let s: f32 = (0..madmom::TRAME / 2).map(|k| fb[k * bandes + b]).sum();
            assert!((s - 1.0).abs() < 1e-4, "bande {b} : {s}");
        }
    }

    #[test]
    fn un_pic_isole_est_detecte_une_fois() {
        let trames = 50;
        let mut a = vec![0.0f32; trames * 5];
        a[20 * 5 + 1] = 0.9; // caisse claire à 0,20 s
        a[21 * 5 + 1] = 0.5;
        a[22 * 5 + 1] = 0.6; // second pic à 10 ms : fusionné
        let c = coups(&a, trames);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].piece, Piece::CaisseClaire);
        assert!((c[0].instant_s - 0.2).abs() < 1e-6);
    }
}
