// SPDX-License-Identifier: GPL-3.0-or-later
//! MuScriptor (Kyutai × Mirelo, 2026) porté en Burn : transcription de
//! qualité, restreinte à des groupes d'instruments (la basse ici).
//!
//! Code d'origine : <https://github.com/muscriptor/muscriptor> (MIT).
//! Poids : <https://huggingface.co/MuScriptor/muscriptor-medium>, CC BY-NC
//! 4.0 et conditions d'usage (transcrire seulement ce sur quoi on a les
//! droits) acceptées par l'utilisateur sur Hugging Face — chargés tels quels
//! depuis leur `model.safetensors`, jamais redistribués par nous.
//!
//! Chaîne, comme `TranscriptionModel.transcribe` :
//! 1. mono → 16 kHz, segments de 5 s (le dernier complété de silence) ;
//! 2. par segment : préfixe de conditionnement ([`reseau::Generateur::prefixe`]),
//!    puis décodage glouton jeton par jeton, logits hors vocabulaire et
//!    instruments non demandés masqués ;
//! 3. **prélude forcé** : un segment commence par les notes encore tenues à
//!    la fin du précédent ([`jetons::prologue`]), au lieu de les deviner ;
//! 4. [`jetons::Suivi`] tire les notes du flux.
//!
//! Écart voulu avec l'original : un segment qui n'émet jamais sa fin (EOS)
//! dans la limite de longueur boucle — même note répétée toutes les 10 à
//! 20 ms, surtout dans le silence (banc `experiments/partitions/`) ; ses
//! jetons sont écartés et les notes ouvertes fermées à sa frontière.

pub mod jetons;
pub mod reseau;
pub mod spectre;

use std::path::Path;

use burn::tensor::backend::Backend;
use burn::tensor::{Int, Tensor, TensorData};
use burn_store::{KeyRemapper, ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore};

use crate::{Error, Note, Result};
pub use reseau::Config;

/// Le backend de l'application : `wgpu` (Metal, Vulkan, DX12) ou le repli
/// CPU, selon les fonctionnalités du crate.
#[cfg(feature = "gpu")]
pub type Moteur = burn::backend::Wgpu;
#[cfg(all(not(feature = "gpu"), feature = "cpu"))]
pub type Moteur = burn::backend::NdArray;

/// Nom du fichier de poids dans le dossier des modèles de l'application.
pub const FICHIER: &str = "muscriptor-medium.safetensors";

pub const FREQUENCE: u32 = 16_000;
pub const SEGMENT: usize = 5 * FREQUENCE as usize;
/// Limite de longueur d'un segment, prologue compris. L'original va jusqu'à
/// 2 000 ; le segment le plus dense du banc (« Parallel Universe ») en
/// compte 338. Au-delà, c'est une boucle : la limite basse en abrège le coût
/// (tout le lot attend le segment qui boucle) et réduit le cache.
pub const LONGUEUR_MAX: usize = 1000;
/// Segments décodés ensemble par l'application (voir
/// [`Muscriptor::transcrire_16k`]).
pub const LOT: usize = 16;
/// Instruments demandés pour une ligne de basse.
pub const BASSES: &[&str] = &["electric_bass", "acoustic_bass"];
/// Jetons générés entre deux lectures (voir [`Muscriptor::segment`]).
const LECTURE: usize = 8;

/// Le modèle chargé, sur un périphérique Burn.
pub struct Muscriptor<B: Backend> {
    gen: reseau::Generateur<B>,
    /// Limite de jetons par segment, prologue compris.
    pub longueur_max: usize,
}

/// Bilan d'une transcription.
#[derive(Debug, Clone, Default)]
pub struct Bilan {
    pub segments: usize,
    pub jetons: usize,
    /// Le plus long segment fini, en jetons.
    pub jetons_max: usize,
    /// Segments écartés faute d'EOS.
    pub boucles: Vec<usize>,
}

impl<B: Backend> Muscriptor<B> {
    /// Charge un `model.safetensors` d'origine. L'architecture se lit dans le
    /// `config.json` voisin s'il existe, sinon `medium`.
    pub fn charger(poids: &Path, device: &B::Device) -> Result<Self> {
        let config = lire_config(&poids.with_file_name("config.json")).unwrap_or(Config::MEDIUM);
        let mut reseau = reseau::Reseau::<B>::new(config, device);
        let remap = KeyRemapper::new()
            .add_pattern(r"^emb\.0\.", "emb.")
            .and_then(|r| r.add_pattern(r"^linears\.0\.", "linear."))
            .map_err(|e| Error::Sortie(format!("renommage des clés : {e}")))?;
        let mut store = SafetensorsStore::from_file(poids).with_from_adapter(PyTorchToBurnAdapter).remap(remap);
        let r = reseau.load_from(&mut store).map_err(|e| Error::PoidsAbsents(format!("{} : {e}", poids.display())))?;
        if !r.missing.is_empty() || !r.errors.is_empty() {
            return Err(Error::Sortie(format!("poids manquants {:?}, erreurs {:?}", r.missing, r.errors)));
        }
        let longueur = 504 + 32 + 2000; // préfixe (mel + classes) + jetons, limite de l'original
        Ok(Self { gen: reseau::Generateur::new(reseau, config, longueur, device), longueur_max: LONGUEUR_MAX })
    }

    /// Le réseau, pour les essais de parité.
    pub fn generateur(&self) -> &reseau::Generateur<B> {
        &self.gen
    }

    pub fn config(&self) -> Config {
        self.gen.config
    }

    /// Transcrit du mono à 16 kHz ; seuls `instruments` peuvent apparaître
    /// (noms de [`jetons::GROUPES`]). `avancer(fait, total)` après chaque
    /// segment.
    ///
    /// `lot` : segments décodés ensemble. 1 = l'un après l'autre avec
    /// prélude forcé (le réglage de qualité de l'original) ; au-delà, en
    /// parallèle, chaque segment devinant lui-même ses notes tenues — un pas
    /// coûte presque autant pour 8 segments que pour un (le coût est dans
    /// le lancement des opérations, pas dans le calcul).
    pub fn transcrire_16k(&self, x: &[f32], instruments: &[&str], lot: usize, mut avancer: impl FnMut(usize, usize)) -> (Vec<jetons::NoteBrute>, Bilan) {
        let groupes = jetons::groupes_de(instruments);
        let masque = self.masque(instruments);
        let n = x.len().div_ceil(SEGMENT).max(1);
        let mut suivi = jetons::Suivi::new();
        let mut bilan = Bilan { segments: n, ..Bilan::default() };
        let decoupe = |i: usize| {
            let mut s = x[(i * SEGMENT).min(x.len())..((i + 1) * SEGMENT).min(x.len())].to_vec();
            s.resize(SEGMENT, 0.0);
            s
        };
        if lot > 1 {
            for debut_lot in (0..n).step_by(lot) {
                let fin_lot = (debut_lot + lot).min(n);
                let segs: Vec<Vec<f32>> = (debut_lot..fin_lot).map(decoupe).collect();
                for (k, (genere, fini)) in self.segments_en_lot(&segs, &groupes, &masque).into_iter().enumerate() {
                    let i = debut_lot + k;
                    let debut = (i * 5) as f32;
                    suivi.frontiere(debut, (i + 1 < n).then_some(debut + 5.0));
                    bilan.jetons += genere.len();
                    if fini {
                        bilan.jetons_max = bilan.jetons_max.max(genere.len());
                        genere.iter().for_each(|&j| suivi.jeton(j));
                    } else {
                        bilan.boucles.push(i);
                        suivi.jeton(jetons::TIE);
                    }
                }
                avancer(fin_lot, n);
            }
            return (suivi.finir(), bilan);
        }
        for i in 0..n {
            let segment = decoupe(i);
            let debut = (i * 5) as f32;
            let fin = (i + 1 < n).then_some(debut + 5.0);
            suivi.frontiere(debut, fin);
            let prologue = if i > 0 { jetons::prologue(&suivi.ouvertes()) } else { Vec::new() };
            let (genere, fini) = self.segment(&segment, &groupes, &prologue, &masque);
            bilan.jetons += prologue.len() + genere.len();
            if fini {
                bilan.jetons_max = bilan.jetons_max.max(prologue.len() + genere.len());
                for &j in prologue.iter().chain(&genere) {
                    suivi.jeton(j);
                }
            } else {
                // Boucle : rien de ce segment, les notes tenues s'y arrêtent.
                bilan.boucles.push(i);
                suivi.jeton(jetons::TIE);
            }
            avancer(i + 1, n);
        }
        (suivi.finir(), bilan)
    }

    /// Masque additif des logits `[1, card]` : −∞ hors vocabulaire et sur les
    /// instruments non demandés.
    pub fn masque(&self, instruments: &[&str]) -> Tensor<B, 2> {
        let card = self.gen.config.card;
        let mut m = vec![0.0f32; card];
        for v in m.iter_mut().skip(jetons::VOCABULAIRE as usize) {
            *v = f32::NEG_INFINITY;
        }
        if !instruments.is_empty() {
            for j in jetons::interdits(instruments) {
                m[j as usize] = f32::NEG_INFINITY;
            }
        }
        Tensor::from_data(TensorData::new(m, [1, card]), self.gen.device())
    }

    /// Décode un segment : jetons générés (sans EOS) et vrai s'il a fini.
    pub fn segment(&self, segment: &[f32], groupes: &[u32], prologue: &[u32], masque: &Tensor<B, 2>) -> (Vec<u32>, bool) {
        let g = &self.gen;
        let prefixe = g.prefixe(segment, groupes);
        let mut cache = g.cache(1, prefixe.dims()[0] + 1 + self.longueur_max);
        let mut entree: Vec<u32> = vec![g.config.card as u32]; // jeton initial
        entree.extend_from_slice(prologue);
        let x = Tensor::cat(vec![prefixe.unsqueeze_dim::<3>(0), g.plonger(&entree)], 1);
        let mut logits = g.avancer(x, &mut cache);
        // Les jetons restent sur le périphérique : l'argmax d'un pas nourrit
        // directement le suivant, et on ne les relit que par lots — une
        // lecture coûte un aller-retour (≈ 1,8 ms sur Metal). Les pas faits
        // après l'EOS d'un lot sont perdus, rien de plus.
        let mut genere = Vec::new();
        let mut lot: Vec<Tensor<B, 2, Int>> = Vec::with_capacity(LECTURE);
        let mut n = prologue.len();
        while n < self.longueur_max {
            let j = (logits + masque.clone()).argmax(1);
            lot.push(j.clone());
            n += 1;
            logits = g.avancer(g.reseau.emb.forward(j), &mut cache);
            if lot.len() == LECTURE || n == self.longueur_max {
                let lus = Tensor::cat(std::mem::take(&mut lot), 1).into_data().convert::<i64>().into_vec::<i64>().expect("jetons");
                for j in lus {
                    if j as u32 == jetons::EOS {
                        return (genere, true);
                    }
                    genere.push(j as u32);
                }
            }
        }
        (genere, false)
    }

    /// Décode plusieurs segments ensemble, sans prologue imposé : pour
    /// chacun, ses jetons (sans EOS) et vrai s'il a fini.
    pub fn segments_en_lot(&self, segments: &[Vec<f32>], groupes: &[u32], masque: &Tensor<B, 2>) -> Vec<(Vec<u32>, bool)> {
        let g = &self.gen;
        let b = segments.len();
        let card = g.config.card;
        let prefixes: Vec<Tensor<B, 3>> = segments.iter().map(|s| g.prefixe(s, groupes).unsqueeze_dim::<3>(0)).collect();
        let prefixe = Tensor::cat(prefixes, 0);
        let p = prefixe.dims()[1];
        let initial = Tensor::<B, 2, Int>::from_data(TensorData::new(vec![card as i64; b], [b, 1]), g.device());
        let mut cache = g.cache(b, p + 1 + self.longueur_max);
        let mut logits = g.avancer(Tensor::cat(vec![prefixe, g.reseau.emb.forward(initial)], 1), &mut cache);
        let masque = masque.clone().expand([b, card]);
        let mut sortie: Vec<(Vec<u32>, bool)> = vec![(Vec::new(), false); b];
        let mut lot: Vec<Tensor<B, 2, Int>> = Vec::with_capacity(LECTURE);
        let mut n = 0;
        while n < self.longueur_max {
            let j = (logits + masque.clone()).argmax(1);
            lot.push(j.clone());
            n += 1;
            logits = g.avancer(g.reseau.emb.forward(j), &mut cache);
            if lot.len() == LECTURE || n == self.longueur_max {
                let k = lot.len();
                let lus = Tensor::cat(std::mem::take(&mut lot), 1).into_data().convert::<i64>().into_vec::<i64>().expect("jetons");
                for (r, (genere, fini)) in sortie.iter_mut().enumerate() {
                    for &j in &lus[r * k..(r + 1) * k] {
                        if *fini {
                            break;
                        }
                        if j as u32 == jetons::EOS {
                            *fini = true;
                        } else {
                            genere.push(j as u32);
                        }
                    }
                }
                if sortie.iter().all(|s| s.1) {
                    break;
                }
            }
        }
        sortie
    }

    /// Transcrit du mono à n'importe quelle fréquence : notes des
    /// `instruments` demandés, en secondes du fichier, triées.
    pub fn transcrire(&self, mono: &[f32], frequence: u32, instruments: &[&str], lot: usize, avancer: impl FnMut(usize, usize)) -> Result<(Vec<Note>, Bilan)> {
        let x = if frequence == FREQUENCE { mono.to_vec() } else { crate::basic_pitch::reechantillonner(mono, frequence, FREQUENCE)? };
        let (brutes, bilan) = self.transcrire_16k(&x, instruments, lot, avancer);
        let mut notes: Vec<Note> = brutes
            .into_iter()
            .filter(|n| n.programme != jetons::PROGRAMME_BATTERIE || instruments.contains(&"drums"))
            .map(|n| Note { debut_s: n.debut_s, fin_s: n.fin_s, hauteur: n.hauteur, amplitude: 1.0, corde: None, frette: None })
            .collect();
        notes.sort_by(|a, b| a.debut_s.total_cmp(&b.debut_s).then(a.hauteur.cmp(&b.hauteur)));
        Ok((notes, bilan))
    }
}

fn lire_config(chemin: &Path) -> Option<Config> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(chemin).ok()?).ok()?;
    let n = |k: &str| v.get(k)?.as_u64().map(|x| x as usize);
    Some(Config { dim: n("dim")?, tetes: n("num_heads")?, couches: n("num_layers")?, card: n("card")? })
}

/// Les poids, où qu'ils soient : dossier des modèles de l'application
/// ([`FICHIER`]), sinon le cache Hugging Face (`hf download
/// MuScriptor/muscriptor-medium`).
pub fn poids() -> Option<std::path::PathBuf> {
    rusty_music_core::modeles::trouver(FICHIER).or_else(|| poids_hugging_face("medium"))
}

/// Le `model.safetensors` d'une taille dans le cache Hugging Face local
/// (`~/.cache/huggingface/hub`), s'il y est.
pub fn poids_hugging_face(taille: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("HF_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache/huggingface")))?;
    let snaps = base.join(format!("hub/models--MuScriptor--muscriptor-{taille}/snapshots"));
    std::fs::read_dir(snaps).ok()?.flatten().map(|e| e.path().join("model.safetensors")).find(|p| p.exists())
}
