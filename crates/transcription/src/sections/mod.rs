// SPDX-License-Identifier: GPL-3.0-or-later
//! Sections d'un morceau (intro, couplet, refrain…) : all-in-one (Kim et
//! Nam, « All-In-One Metrical and Functional Structure Analysis with
//! Neighborhood Attentions on Demixed Audio », WASPAA 2023 ; code et poids
//! MIT) porté en Burn. Plan : `docs/plan-editer-pratique-creation.md`, 2.4.
//!
//! Il lit les quatre stems de HTDemucs — les nôtres, sans second démixage :
//! 1. [`spectre`] : le spectrogramme de madmom de chaque stem (30 Hz–17 kHz,
//!    81 bandes, 100 trames par seconde) ;
//! 2. [`reseau`] : les 8 plis publiés, logits moyennés comme l'ensemble
//!    `harmonix-all` de l'original ;
//! 3. [`segments`] : le choix des frontières et l'étiquette de chaque
//!    section, port de `postprocessing/functional.py`.
//!
//! Les temps et premiers temps restent ceux de Beat This! (déjà porté) :
//! ceux d'all-in-one passent par le DBN de madmom, non porté ; ses
//! activations sont rendues pour comparaison.
//!
//! Poids : `experiments/structure/exporter_safetensors.py` convertit les
//! `.pth` de `taejunkim/allinone` en safetensors.

pub mod reseau;
pub mod voisinage;

use std::path::{Path, PathBuf};

use burn::module::Module;
use burn::tensor::backend::Backend;
use burn::tensor::{Element, Tensor, TensorData};
use burn_store::{KeyRemapper, ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore};
use serde::{Deserialize, Serialize};

use crate::muscriptor::VersFlottant;
use crate::{madmom, Error, Result};
use reseau::{AllInOne, BANDES, FONCTIONS, INSTRUMENTS};
use voisinage::Indices;

/// Les étiquettes de Harmonix, dans l'ordre des sorties du réseau.
pub const ETIQUETTES: [&str; FONCTIONS] = ["start", "end", "intro", "outro", "break", "bridge", "inst", "solo", "verse", "chorus"];
/// Trames par seconde.
pub const FPS: usize = madmom::FPS;
const FMIN: f64 = 30.0;
const FMAX: f64 = 17_000.0;
/// `min_hops_per_beat` de l'original : 60 / 240 BPM × 100 trames.
const TRAMES_PAR_TEMPS_MIN: usize = 24;
/// Fenêtres du choix des frontières : 12 s de part et d'autre.
const FENETRE: usize = 12 * FPS;

/// Une section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub debut_s: f32,
    pub fin_s: f32,
    pub etiquette: String,
}

/// Les sorties moyennées des plis, par trame.
#[derive(Debug, Clone, Default)]
pub struct Activations {
    pub trames: usize,
    /// Logits `[T]`.
    pub beat: Vec<f32>,
    pub downbeat: Vec<f32>,
    pub section: Vec<f32>,
    /// Logits `[10, T]`, étiquette par étiquette.
    pub function: Vec<f32>,
}

/// Le spectrogramme d'all-in-one : `[4, T, 81]`, stems dans l'ordre bass,
/// drums, other, vocals, chacun en stéréo `(gauche, droite)` à 44,1 kHz.
pub fn spectre(stems: [(&[f32], &[f32]); INSTRUMENTS]) -> (Vec<f32>, usize) {
    let mut tout = Vec::new();
    let mut trames = 0;
    for (g, d) in stems {
        let (s, t, b) = madmom::spectre_log(g, d, FMIN, FMAX);
        debug_assert_eq!(b, BANDES);
        trames = t;
        tout.extend(s);
    }
    (tout, trames)
}

/// L'ensemble des plis chargés.
pub struct Sections<B: Backend> {
    plis: Vec<AllInOne<B>>,
    device: B::Device,
}

impl<B: Backend> Sections<B> {
    /// Charge des plis convertis (`harmonix-fold<i>.safetensors`).
    pub fn charger(fichiers: &[PathBuf], device: &B::Device) -> Result<Self> {
        let remap = KeyRemapper::new().add_pattern(r"\.self\.", ".soi.").map_err(|e| Error::Sortie(format!("renommage des clés : {e}")))?;
        let mut plis = Vec::new();
        for f in fichiers {
            let mut pli = AllInOne::<B>::new(device);
            let mut store = SafetensorsStore::from_file(f).with_from_adapter(PyTorchToBurnAdapter).remap(remap.clone());
            let r = pli.load_from(&mut store).map_err(|e| Error::PoidsAbsents(format!("{} : {e}", f.display())))?;
            if !r.missing.is_empty() || !r.errors.is_empty() {
                return Err(Error::Sortie(format!("{} : poids manquants {:?}, erreurs {:?}", f.display(), r.missing, r.errors)));
            }
            plis.push(pli.map(&mut VersFlottant(<B::FloatElem as Element>::dtype())));
        }
        if plis.is_empty() {
            return Err(Error::PoidsAbsents("aucun pli d'all-in-one".into()));
        }
        Ok(Self { plis, device: device.clone() })
    }

    /// Tous les plis d'un dossier, dans l'ordre de leur nom.
    pub fn charger_dossier(dossier: &Path, device: &B::Device) -> Result<Self> {
        let mut f: Vec<PathBuf> = std::fs::read_dir(dossier)
            .map_err(|e| Error::PoidsAbsents(format!("{} : {e}", dossier.display())))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("harmonix-fold") && n.ends_with(".safetensors")))
            .collect();
        f.sort();
        Self::charger(&f, device)
    }

    pub fn plis(&self) -> usize {
        self.plis.len()
    }

    /// Logits moyennés sur les `n` premiers plis (tous si `None`).
    pub fn activations(&self, spec: &[f32], trames: usize, n: Option<usize>) -> Activations {
        let x = Tensor::<B, 3>::from_data(TensorData::new(spec.to_vec(), [INSTRUMENTS, trames, BANDES]), &self.device);
        let mut indices = Indices::new(&self.device);
        let plis = &self.plis[..n.unwrap_or(self.plis.len()).min(self.plis.len())];
        let mut somme: Option<[Tensor<B, 2>; 4]> = None;
        for pli in plis {
            let l = pli.forward(x.clone(), &mut indices);
            let l = [l.beat, l.downbeat, l.section, l.function];
            somme = Some(match somme {
                None => l,
                Some(s) => {
                    let [a, b, c, d] = s;
                    let [e, f, g, h] = l;
                    [a + e, b + f, c + g, d + h]
                }
            });
        }
        let [beat, downbeat, section, function] = somme.expect("au moins un pli").map(|t| t / plis.len() as f64);
        let vers = |t: Tensor<B, 2>| t.into_data().convert::<f32>().into_vec::<f32>().expect("f32");
        Activations {
            trames,
            beat: vers(beat),
            downbeat: vers(downbeat),
            section: vers(section),
            function: vers(function.swap_dims(0, 1)),
        }
    }

    /// Les sections des quatre stems.
    pub fn analyser(&self, stems: [(&[f32], &[f32]); INSTRUMENTS]) -> (Vec<Section>, Activations) {
        let (spec, trames) = spectre(stems);
        let a = self.activations(&spec, trames, None);
        (segments(&a), a)
    }
}

/// Probabilité de frontière → frontières, puis étiquette moyenne de chaque
/// section (port de `postprocess_functional_structure`).
pub fn segments(a: &Activations) -> Vec<Section> {
    let t = a.trames;
    let prob: Vec<f32> = a.section.iter().map(|x| 1.0 / (1.0 + (-x).exp())).collect();
    // Maxima locaux sur ±2 temps au plus rapide (96 trames).
    let r = 2 * TRAMES_PAR_TEMPS_MIN;
    let maxima: Vec<f32> = (0..t)
        .map(|i| {
            let m = prob[i.saturating_sub(r)..(i + r + 1).min(t)].iter().copied().fold(f32::NEG_INFINITY, f32::max);
            if prob[i] == m { prob[i] } else { 0.0 }
        })
        .collect();
    let frontieres = choisir_pics(&maxima, FENETRE, FENETRE);
    // Étiquettes : softmax par trame, moyenne par section.
    let mut fonctions = a.function.clone();
    for i in 0..t {
        let m = (0..FONCTIONS).map(|e| fonctions[e * t + i]).fold(f32::NEG_INFINITY, f32::max);
        let z: f32 = (0..FONCTIONS).map(|e| (fonctions[e * t + i] - m).exp()).sum();
        for e in 0..FONCTIONS {
            fonctions[e * t + i] = (fonctions[e * t + i] - m).exp() / z;
        }
    }
    let mut bornes: Vec<usize> = vec![0];
    bornes.extend(frontieres.iter().copied().filter(|&i| i > 0));
    bornes.push(t);
    let duree = t as f32 / FPS as f32;
    bornes
        .windows(2)
        .filter(|w| w[1] > w[0])
        .map(|w| {
            let moy = |e: usize| fonctions[e * t + w[0]..e * t + w[1]].iter().sum::<f32>();
            let e = (0..FONCTIONS).max_by(|&x, &y| moy(x).total_cmp(&moy(y))).unwrap_or(0);
            Section { debut_s: w[0] as f32 / FPS as f32, fin_s: (w[1] as f32 / FPS as f32).min(duree), etiquette: ETIQUETTES[e].into() }
        })
        .collect()
}

/// `peak_picking` de l'original : maximum de sa fenêtre, et au-dessus de la
/// moyenne des fenêtres passée et future (signal complété de zéros).
fn choisir_pics(act: &[f32], passe: usize, futur: usize) -> Vec<usize> {
    let t = act.len();
    let val = |i: i64| if i >= 0 && (i as usize) < t { act[i as usize] } else { 0.0 };
    let mut pics = Vec::new();
    for i in 0..t {
        let x = act[i];
        if x <= 0.0 {
            continue;
        }
        let (a, z) = (i as i64 - passe as i64, i as i64 + futur as i64);
        if (a..=z).any(|j| val(j) > x) {
            continue;
        }
        let m_passe = (a..i as i64).map(val).sum::<f32>() / passe as f32;
        let m_futur = (i as i64 + 1..=z).map(val).sum::<f32>() / futur as f32;
        if x - (m_passe + m_futur) / 2.0 > 0.0 {
            pics.push(i);
        }
    }
    pics
}

/// Probabilités des temps et premiers temps (sigmoïde des logits), pour
/// comparaison avec Beat This!.
pub fn probabilites(logits: &[f32]) -> Vec<f32> {
    logits.iter().map(|x| 1.0 / (1.0 + (-x).exp())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_pic_net_est_une_frontiere() {
        let mut a = vec![0.0f32; 5000];
        a[2000] = 0.9;
        a[2001] = 0.3;
        assert_eq!(choisir_pics(&a, FENETRE, FENETRE), vec![2000]);
    }

    #[test]
    fn segments_couvrent_le_morceau() {
        let t = 3000;
        let mut section = vec![-100.0f32; t]; // probabilité nulle : aucun candidat
        section[1500] = 5.0;
        let mut function = vec![0.0f32; FONCTIONS * t];
        for i in 0..1500 {
            function[8 * t + i] = 5.0; // verse
        }
        for i in 1500..t {
            function[9 * t + i] = 5.0; // chorus
        }
        let s = segments(&Activations { trames: t, section, function, ..Default::default() });
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].debut_s, s[0].fin_s, s[0].etiquette.as_str()), (0.0, 15.0, "verse"));
        assert_eq!((s[1].debut_s, s[1].fin_s, s[1].etiquette.as_str()), (15.0, 30.0, "chorus"));
    }
}
