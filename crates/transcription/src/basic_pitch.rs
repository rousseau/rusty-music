// SPDX-License-Identifier: GPL-3.0-or-later
//! Basic Pitch (Bittner et al., « A Lightweight Instrument-Agnostic Model for
//! Polyphonic Note Transcription and Multipitch Estimation », ICASSP 2022 ;
//! Spotify, code et poids Apache-2.0) : moins de 17 000 paramètres, CQT
//! comprise dans le graphe ONNX — l'entrée est l'audio brut à 22 050 Hz.
//!
//! Deux étages, portés de `basic_pitch/inference.py` et
//! `basic_pitch/note_creation.py` (révision `fa5997a`) :
//! - [`Transcripteur::activations`] : fenêtres de 2 s qui se recouvrent de 30
//!   trames, recollées en retirant la moitié du recouvrement de chaque côté ;
//! - [`notes`] : attaques (prédites et déduites des sauts d'énergie), suivi de
//!   chaque note tant que l'énergie tient, puis l'« astuce Melodia » qui
//!   ramasse l'énergie restante. Sans les inflexions de hauteur (pitch bends),
//!   dont la tablature n'a que faire.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;

use crate::{Error, Note, Result};

/// Fréquence d'entrée du modèle.
pub const SR: u32 = 22_050;
const FFT_HOP: usize = 256;
/// Échantillons par fenêtre : 2 s moins un pas.
const N_ECH: usize = SR as usize * 2 - FFT_HOP;
/// Trames rendues par fenêtre (86 par seconde × 2).
const TRAMES_FENETRE: usize = 172;
const RECOUVREMENT: usize = 30;
const PAS: usize = N_ECH - RECOUVREMENT * FFT_HOP;
pub const NOTES: usize = 88;
/// Hauteur MIDI de la première case (la0, 27,5 Hz).
const MIDI_BAS: usize = 21;
const ALIGNEMENT_MAGIQUE: f32 = 0.0018;
/// Fenêtres passées ensemble au réseau.
const LOT: usize = 16;

/// Le fichier de poids : le `nmp.onnx` du dépôt Spotify, tel quel.
pub const POIDS: crate::Poids = crate::Poids {
    nom: "basic_pitch_nmp.onnx",
    url: "https://raw.githubusercontent.com/spotify/basic-pitch/fa5997af0a8210982619003269994a1be25eddf3/basic_pitch/saved_models/icassp_2022/nmp.onnx",
    octets: 230_444,
    sha256: "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec",
};

/// Activations du réseau, recollées : `trames × 88`, ligne par ligne.
#[derive(Debug, Clone)]
pub struct Activations {
    pub trames: usize,
    pub note: Vec<f32>,
    pub attaque: Vec<f32>,
}

/// Réglages de la création de notes — les défauts de Basic Pitch.
#[derive(Debug, Clone, Copy)]
pub struct Reglages {
    pub seuil_attaque: f32,
    pub seuil_trame: f32,
    /// Durée minimale d'une note, en trames (11 ≈ 127,7 ms).
    pub duree_min: usize,
    pub freq_min: Option<f32>,
    pub freq_max: Option<f32>,
    pub deduire_attaques: bool,
    pub melodia: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Self {
            seuil_attaque: 0.5,
            seuil_trame: 0.3,
            duree_min: 11,
            freq_min: None,
            freq_max: None,
            deduire_attaques: true,
            melodia: true,
        }
    }
}

impl Reglages {
    /// Les réglages d'une basse : de si0 (30,9 Hz, la corde grave d'une cinq
    /// cordes) au sol4 (392 Hz, haut du manche).
    pub fn basse() -> Self {
        Self { freq_min: Some(30.0), freq_max: Some(400.0), ..Self::default() }
    }
}

pub struct Transcripteur {
    session: ort::session::Session,
}

impl Transcripteur {
    pub fn charger(chemin: &Path) -> Result<Self> {
        let session = ort::session::Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)?
            .commit_from_file(chemin)?;
        Ok(Self { session })
    }

    /// Le modèle, trouvé parmi les poids de la machine.
    pub fn charger_installe() -> Result<Self> {
        let chemin = rusty_music_core::modeles::trouver(POIDS.nom).ok_or_else(|| {
            Error::PoidsAbsents(rusty_music_core::modeles::introuvable(POIDS.nom).to_string())
        })?;
        Self::charger(&chemin)
    }

    /// Les activations d'un signal mono à `frequence` Hz.
    pub fn activations(&mut self, mono: &[f32], frequence: u32) -> Result<Activations> {
        let audio = if frequence == SR { mono.to_vec() } else { reechantillonner(mono, frequence, SR)? };
        let longueur = audio.len();
        // Demi-recouvrement de silence devant, comme `get_audio_input`.
        let mut rembourre = vec![0.0f32; RECOUVREMENT * FFT_HOP / 2];
        rembourre.extend_from_slice(&audio);
        let departs: Vec<usize> = (0..rembourre.len()).step_by(PAS).collect();

        let utiles = TRAMES_FENETRE - RECOUVREMENT;
        let mut note = Vec::with_capacity(departs.len() * utiles * NOTES);
        let mut attaque = Vec::with_capacity(departs.len() * utiles * NOTES);
        for lot in departs.chunks(LOT) {
            let mut entree = vec![0.0f32; lot.len() * N_ECH];
            for (k, &d) in lot.iter().enumerate() {
                let fin = (d + N_ECH).min(rembourre.len());
                entree[k * N_ECH..k * N_ECH + (fin - d)].copy_from_slice(&rembourre[d..fin]);
            }
            let tenseur = ort::value::Tensor::from_array(
                ndarray::Array::from_shape_vec((lot.len(), N_ECH, 1), entree).expect("forme d'entrée"),
            )?;
            let sortie = self.session.run(ort::inputs!["serving_default_input_2:0" => tenseur.view()])?;
            let (_, n) = sortie["StatefulPartitionedCall:1"].try_extract_tensor::<f32>()?;
            let (_, o) = sortie["StatefulPartitionedCall:2"].try_extract_tensor::<f32>()?;
            if n.len() != lot.len() * TRAMES_FENETRE * NOTES || o.len() != n.len() {
                return Err(Error::Sortie(format!("{} valeurs pour {} fenêtres", n.len(), lot.len())));
            }
            // On retire la moitié du recouvrement de chaque côté de chaque fenêtre.
            for k in 0..lot.len() {
                let base = k * TRAMES_FENETRE * NOTES;
                let garde = base + RECOUVREMENT / 2 * NOTES..base + (TRAMES_FENETRE - RECOUVREMENT / 2) * NOTES;
                note.extend_from_slice(&n[garde.clone()]);
                attaque.extend_from_slice(&o[garde]);
            }
        }
        // Autant de trames que le signal d'origine en appelle.
        let trames = ((longueur as f64 / PAS as f64) * utiles as f64) as usize;
        let trames = trames.min(note.len() / NOTES);
        note.truncate(trames * NOTES);
        attaque.truncate(trames * NOTES);
        Ok(Activations { trames, note, attaque })
    }
}

/// Instant de début de chaque trame, en secondes — `model_frames_to_time`,
/// alignement par fenêtre compris.
pub fn instant_trame(i: usize) -> f32 {
    let decalage = (FFT_HOP as f32 / SR as f32) * (TRAMES_FENETRE as f32 - N_ECH as f32 / FFT_HOP as f32)
        + ALIGNEMENT_MAGIQUE;
    i as f32 * FFT_HOP as f32 / SR as f32 - decalage * (i / TRAMES_FENETRE) as f32
}

fn hz_vers_midi(f: f32) -> f32 {
    12.0 * (f / 440.0).log2() + 69.0
}

/// Des activations aux notes — `output_to_notes_polyphonic`.
pub fn notes(a: &Activations, r: &Reglages) -> Vec<Note> {
    let n = a.trames;
    if n < 3 {
        return Vec::new();
    }
    let mut trames = a.note.clone();
    let mut attaques = a.attaque.clone();

    // Hors de la plage demandée, tout à zéro.
    let bas = r.freq_min.map_or(0, |f| (hz_vers_midi(f) - MIDI_BAS as f32).round().max(0.0) as usize);
    let haut = r.freq_max.map_or(NOTES, |f| ((hz_vers_midi(f) - MIDI_BAS as f32).round().max(0.0) as usize).min(NOTES));
    for t in 0..n {
        for f in (0..bas.min(NOTES)).chain(haut..NOTES) {
            trames[t * NOTES + f] = 0.0;
            attaques[t * NOTES + f] = 0.0;
        }
    }

    if r.deduire_attaques {
        attaques = attaques_deduites(&attaques, &trames, n);
    }

    // Les pics d'attaque dans le temps, au-dessus du seuil, du plus tard au
    // plus tôt (l'ordre de `np.where` renversé).
    let mut pics = Vec::new();
    for t in 1..n - 1 {
        for f in 0..NOTES {
            let v = attaques[t * NOTES + f];
            if v >= r.seuil_attaque && v > attaques[(t - 1) * NOTES + f] && v > attaques[(t + 1) * NOTES + f] {
                pics.push((t, f));
            }
        }
    }
    pics.reverse();

    const TOLERANCE: usize = 11;
    let mut reste = trames.clone();
    let mut sortie = Vec::new();
    let moyenne = |debut: usize, fin: usize, f: usize| -> f32 {
        let s: f32 = (debut..fin).map(|t| trames[t * NOTES + f]).sum();
        s / (fin - debut).max(1) as f32
    };

    for (debut, f) in pics {
        if debut >= n - 1 {
            continue;
        }
        let mut i = debut + 1;
        let mut k = 0;
        while i < n - 1 && k < TOLERANCE {
            if reste[i * NOTES + f] < r.seuil_trame {
                k += 1;
            } else {
                k = 0;
            }
            i += 1;
        }
        i -= k;
        if i - debut <= r.duree_min {
            continue;
        }
        for t in debut..i {
            reste[t * NOTES + f] = 0.0;
            if f + 1 < NOTES {
                reste[t * NOTES + f + 1] = 0.0;
            }
            if f > 0 {
                reste[t * NOTES + f - 1] = 0.0;
            }
        }
        sortie.push((debut, i, f, moyenne(debut, i, f)));
    }

    if r.melodia {
        // `np.argmax` à chaque tour, sans rebalayer : un tas trié une fois.
        // Les cases ne font que tomber à zéro, une entrée périmée se reconnaît.
        let mut tas: BinaryHeap<(Ordonne, Reverse<usize>)> = reste
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > r.seuil_trame)
            .map(|(i, v)| (Ordonne(*v), Reverse(i)))
            .collect();
        while let Some((Ordonne(v), Reverse(idx))) = tas.pop() {
            if reste[idx] != v || v <= r.seuil_trame {
                continue;
            }
            let (milieu, f) = (idx / NOTES, idx % NOTES);
            reste[idx] = 0.0;
            let effacer = |reste: &mut Vec<f32>, t: usize| {
                reste[t * NOTES + f] = 0.0;
                if f + 1 < NOTES {
                    reste[t * NOTES + f + 1] = 0.0;
                }
                if f > 0 {
                    reste[t * NOTES + f - 1] = 0.0;
                }
            };
            // En avant.
            let mut i = milieu + 1;
            let mut k = 0;
            while i < n - 1 && k < TOLERANCE {
                if reste[i * NOTES + f] < r.seuil_trame {
                    k += 1;
                } else {
                    k = 0;
                }
                effacer(&mut reste, i);
                i += 1;
            }
            let fin = i - 1 - k;
            // En arrière.
            let mut i = milieu as isize - 1;
            let mut k = 0;
            while i > 0 && k < TOLERANCE {
                if reste[i as usize * NOTES + f] < r.seuil_trame {
                    k += 1;
                } else {
                    k = 0;
                }
                effacer(&mut reste, i as usize);
                i -= 1;
            }
            let debut = (i + 1 + k as isize) as usize;
            if fin <= debut || fin - debut <= r.duree_min {
                continue;
            }
            sortie.push((debut, fin, f, moyenne(debut, fin, f)));
        }
    }

    let mut notes: Vec<Note> = sortie
        .into_iter()
        .map(|(d, fin, f, amplitude)| Note {
            debut_s: instant_trame(d),
            fin_s: instant_trame(fin),
            hauteur: (f + MIDI_BAS) as u8,
            amplitude,
            corde: None,
            frette: None,
        })
        .collect();
    notes.sort_by(|a, b| a.debut_s.total_cmp(&b.debut_s).then(a.hauteur.cmp(&b.hauteur)));
    notes
}

/// Un flottant ordonné pour le tas — les activations ne sont jamais NaN.
#[derive(Clone, Copy)]
struct Ordonne(f32);
impl PartialEq for Ordonne {
    fn eq(&self, autre: &Self) -> bool {
        self.cmp(autre).is_eq()
    }
}
impl Eq for Ordonne {}
impl PartialOrd for Ordonne {
    fn partial_cmp(&self, autre: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(autre))
    }
}
impl Ord for Ordonne {
    fn cmp(&self, autre: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&autre.0)
    }
}

/// Attaques déduites des sauts d'énergie — `get_infered_onsets`, sur deux
/// trames d'écart.
fn attaques_deduites(attaques: &[f32], trames: &[f32], n: usize) -> Vec<f32> {
    const N_DIFF: usize = 2;
    let mut ecart = vec![0.0f32; n * NOTES];
    for t in 0..n {
        for f in 0..NOTES {
            let v = trames[t * NOTES + f];
            let d = (1..=N_DIFF)
                .map(|m| v - if t >= m { trames[(t - m) * NOTES + f] } else { 0.0 })
                .fold(f32::INFINITY, f32::min);
            ecart[t * NOTES + f] = if t < N_DIFF { 0.0 } else { d.max(0.0) };
        }
    }
    let max_attaque = attaques.iter().copied().fold(0.0f32, f32::max);
    let max_ecart = ecart.iter().copied().fold(0.0f32, f32::max);
    attaques
        .iter()
        .zip(&ecart)
        .map(|(a, e)| {
            let e = if max_ecart > 0.0 { max_attaque * e / max_ecart } else { 0.0 };
            a.max(e)
        })
        .collect()
}

/// Le rééchantillonnage vers 22 050 Hz que fait [`Transcripteur::activations`],
/// exposé pour les essais de parité (l'entrée exacte du réseau).
pub fn reechantillonner_pour_essai(mono: &[f32], frequence: u32) -> Result<Vec<f32>> {
    reechantillonner(mono, frequence, SR)
}

/// Rééchantillonnage sinc par FFT (`rubato`), même méthode que
/// `crates/superres` : amorce réfléchie, puis retirée.
fn reechantillonner(x: &[f32], de: u32, vers: u32) -> Result<Vec<f32>> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler, WindowFunction};

    if x.is_empty() {
        return Ok(Vec::new());
    }
    const BLOC: usize = 16_384;
    let amorce = 4096.min(x.len());
    let mut rembourre = Vec::with_capacity(x.len() + amorce);
    rembourre.extend((1..=amorce).rev().map(|i| x[i.min(x.len() - 1)]));
    rembourre.extend_from_slice(x);
    let mut r = Fft::<f32>::new_custom(de as usize, vers as usize, BLOC, 1, 1, WindowFunction::Hann, FixedSync::Input)
        .map_err(|e| Error::Reechantillonnage(e.to_string()))?;
    let entree = InterleavedSlice::new(&rembourre, 1, rembourre.len())
        .map_err(|e| Error::Reechantillonnage(e.to_string()))?;
    let sortie = r
        .process_all(&entree, rembourre.len(), None)
        .map_err(|e| Error::Reechantillonnage(e.to_string()))?;
    let a_jeter = (amorce as u64 * vers as u64 / de as u64) as usize;
    let cible = (x.len() as u64 * vers as u64 / de as u64) as usize;
    let mut sortie = sortie.take_data();
    sortie.drain(..a_jeter.min(sortie.len()));
    sortie.truncate(cible);
    Ok(sortie)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activations_vides(trames: usize) -> Activations {
        Activations { trames, note: vec![0.0; trames * NOTES], attaque: vec![0.0; trames * NOTES] }
    }

    #[test]
    fn une_note_tenue_devient_une_note() {
        let mut a = activations_vides(100);
        let f = 40 - MIDI_BAS; // mi2
        for t in 10..60 {
            a.note[t * NOTES + f] = 0.8;
        }
        a.attaque[10 * NOTES + f] = 0.9;
        let r = Reglages { deduire_attaques: false, melodia: false, ..Reglages::default() };
        let n = notes(&a, &r);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].hauteur, 40);
        assert!((n[0].debut_s - instant_trame(10)).abs() < 1e-6);
        assert!((n[0].fin_s - instant_trame(60)).abs() < 1e-6);
        assert!((n[0].amplitude - 0.8).abs() < 1e-5);
    }

    #[test]
    fn trop_courte_elle_disparait() {
        let mut a = activations_vides(100);
        let f = 30;
        for t in 10..18 {
            a.note[t * NOTES + f] = 0.8;
        }
        a.attaque[10 * NOTES + f] = 0.9;
        let r = Reglages { deduire_attaques: false, melodia: false, ..Reglages::default() };
        assert!(notes(&a, &r).is_empty());
    }

    #[test]
    fn melodia_ramasse_une_note_sans_attaque() {
        let mut a = activations_vides(100);
        let f = 30;
        for t in 20..70 {
            a.note[t * NOTES + f] = 0.6;
        }
        let r = Reglages { deduire_attaques: false, ..Reglages::default() };
        let n = notes(&a, &r);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].hauteur as usize, f + MIDI_BAS);
    }

    #[test]
    fn la_plage_de_frequence_filtre() {
        let mut a = activations_vides(100);
        let f = 80; // très aigu, hors de la plage d'une basse
        for t in 10..60 {
            a.note[t * NOTES + f] = 0.8;
        }
        a.attaque[10 * NOTES + f] = 0.9;
        assert!(notes(&a, &Reglages::basse()).is_empty());
    }

    #[test]
    fn instants_des_trames() {
        assert_eq!(instant_trame(0), 0.0);
        // 86 trames par seconde environ, et le recalage par fenêtre.
        assert!((instant_trame(86) - 86.0 * 256.0 / 22050.0).abs() < 1e-5);
        assert!(instant_trame(172) < 172.0 * 256.0 / 22050.0);
    }
}
