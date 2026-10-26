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
//! Écart voulu avec l'original : le décodage glouton part parfois en boucle
//! — même note répétée au même instant ou toutes les 10 à 20 ms (bancs
//! `experiments/partitions/` et `experiments/banc-transcription/`), avec ou
//! sans EOS. [`jetons::debut_de_boucle`] le repère : le décodage s'arrête,
//! on garde le flux d'avant la boucle. Un segment sans EOS ni boucle
//! repérée est écarté, ses notes ouvertes fermées à sa frontière.

pub mod jetons;
pub mod mince;
pub mod reseau;
pub mod spectre;

use std::path::Path;

use burn::module::{Module, ModuleMapper, Param};
use burn::tensor::backend::Backend;
use burn::tensor::{DType, Element, Int, Tensor, TensorData};
use burn_store::{KeyRemapper, ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore};

use crate::{Error, Note, Result};
pub use mince::ProduitMince;
pub use reseau::Config;

/// Le backend de l'application : `wgpu` (Metal, Vulkan, DX12) ou le repli
/// CPU, selon les fonctionnalités du crate. Sur Metal, en flottants 16 bits :
/// moitié de mémoire (6,3 Go au lieu de 11,7 en pointe, lot de 16), mêmes
/// notes à 10 ms près sur les morceaux du banc. Ailleurs, f32 : le f16 n'est
/// pas garanti par tous les pilotes Vulkan et DX12.
#[cfg(all(feature = "gpu", target_os = "macos"))]
pub type Moteur = burn::backend::Wgpu<burn::tensor::f16>;
#[cfg(all(feature = "gpu", not(target_os = "macos")))]
pub type Moteur = burn::backend::Wgpu;
#[cfg(all(not(feature = "gpu"), feature = "cpu"))]
pub type Moteur = burn::backend::NdArray;

/// L'appareil de MuScriptor. Burn 0.21 fixe le flottant par défaut **par
/// appareil**, une fois pour toutes, au premier tenseur créé : `Wgpu<f16>` et
/// le `Wgpu<f32>` du démixage partagent le type d'appareil, donc l'appareil
/// par défaut. Après une transcription, le démixage échouait (« expected F16,
/// got F32 », 24 oct.). Sur Metal, MuScriptor prend donc l'appareil
/// « intégré » — la même puce Apple, mais un appareil distinct pour Burn,
/// avec ses réglages et sa mémoire. Un Mac sans GPU intégré (Mac Pro Intel)
/// n'est pas couvert.
#[cfg(all(feature = "gpu", target_os = "macos"))]
pub fn appareil() -> burn::tensor::Device<Moteur> {
    burn::backend::wgpu::WgpuDevice::IntegratedGpu(0)
}
#[cfg(not(all(feature = "gpu", target_os = "macos")))]
pub fn appareil() -> burn::tensor::Device<Moteur> {
    Default::default()
}

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
    /// Segments coupés au début d'une boucle, le reste gardé.
    pub tronques: Vec<usize>,
}

impl<B: ProduitMince> Muscriptor<B> {
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
        // `burn-store` garde le type du fichier (f32) : on ramène les poids
        // au flottant du backend, sinon un `Wgpu<f16>` mêle les deux.
        let reseau = reseau.map(&mut VersFlottant(<B::FloatElem as Element>::dtype()));
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
                for (k, (mut genere, mut fini)) in self.segments_en_lot(&segs, &groupes, &masque).into_iter().enumerate() {
                    let i = debut_lot + k;
                    if let Some(b) = jetons::debut_de_boucle(&genere) {
                        genere.truncate(b);
                        fini = true;
                        bilan.tronques.push(i);
                    }
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
            let (mut genere, mut fini) = self.segment(&segment, &groupes, &prologue, &masque);
            if let Some(b) = jetons::debut_de_boucle(&genere) {
                genere.truncate(b);
                fini = true;
                bilan.tronques.push(i);
            }
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
                if jetons::debut_de_boucle(&genere).is_some() {
                    return (genere, false);
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
        let mut boucle = vec![false; b];
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
                        if *fini || boucle[r] {
                            break;
                        }
                        if j as u32 == jetons::EOS {
                            *fini = true;
                        } else {
                            genere.push(j as u32);
                        }
                    }
                    // Le segment continue de tourner avec le lot, mais on
                    // n'en garde plus rien.
                    if !*fini && !boucle[r] && jetons::debut_de_boucle(genere).is_some() {
                        boucle[r] = true;
                    }
                }
                if sortie.iter().zip(&boucle).all(|(s, &b)| s.1 || b) {
                    break;
                }
            }
        }
        sortie
    }

    /// Notes brutes (avec leur programme) d'un signal mono à n'importe quelle
    /// fréquence. `instruments` vide : aucune restriction.
    pub fn transcrire_brut(&self, mono: &[f32], frequence: u32, instruments: &[&str], lot: usize, avancer: impl FnMut(usize, usize)) -> Result<(Vec<jetons::NoteBrute>, Bilan)> {
        let mut x = if frequence == FREQUENCE { mono.to_vec() } else { crate::basic_pitch::reechantillonner(mono, frequence, FREQUENCE)? };
        let gain = gain_niveau(&x);
        if gain > 1.0 {
            x.iter_mut().for_each(|v| *v = (*v * gain).clamp(-1.0, 1.0));
        }
        Ok(self.transcrire_16k(&x, instruments, lot, avancer))
    }

    /// Transcrit du mono à n'importe quelle fréquence : notes des
    /// `instruments` demandés, en secondes du fichier, triées.
    pub fn transcrire(&self, mono: &[f32], frequence: u32, instruments: &[&str], lot: usize, avancer: impl FnMut(usize, usize)) -> Result<(Vec<Note>, Bilan)> {
        let (brutes, bilan) = self.transcrire_brut(mono, frequence, instruments, lot, avancer)?;
        let mut notes: Vec<Note> = brutes
            .into_iter()
            .filter(|n| n.programme != jetons::PROGRAMME_BATTERIE || instruments.contains(&"drums"))
            .map(|n| Note { debut_s: n.debut_s, fin_s: n.fin_s, hauteur: n.hauteur, amplitude: 1.0, corde: None, frette: None })
            .collect();
        notes.sort_by(|a, b| a.debut_s.total_cmp(&b.debut_s).then(a.hauteur.cmp(&b.hauteur)));
        Ok((sans_doublons(notes), bilan))
    }
}

/// Le programme est oublié dans [`Note`] : une même note émise pour deux
/// instruments du groupe (basse électrique et acoustique : 14 doublons sur
/// 391 notes d'un morceau de Slakh) n'en fait qu'une, la plus longue.
fn sans_doublons(notes: Vec<Note>) -> Vec<Note> {
    let mut gardees: Vec<Note> = Vec::with_capacity(notes.len());
    for n in notes {
        let double = gardees.iter_mut().rev().take_while(|g| n.debut_s - g.debut_s <= 0.02).find(|g| g.hauteur == n.hauteur);
        match double {
            Some(g) => g.fin_s = g.fin_s.max(n.fin_s),
            None => gardees.push(n),
        }
    }
    gardees
}

/// Convertit chaque paramètre flottant vers un type donné.
pub(crate) struct VersFlottant(pub(crate) DType);

impl<B: Backend> ModuleMapper<B> for VersFlottant {
    fn map_float<const D: usize>(&mut self, param: Param<Tensor<B, D>>) -> Param<Tensor<B, D>> {
        let (id, t, mapper) = param.consume();
        Param::from_mapped_value(id, t.cast(self.0), mapper)
    }
}

/// Niveau de jeu visé (RMS des passages joués). Le modèle se tait sur un
/// signal trop faible : le stem de basse de « Strawberry Fields Forever »
/// (RMS 0,008, la piste entière est enregistrée bas) ne donne aucune note,
/// le même amplifié 20 fois en donne 134. Un stem au niveau habituel (0,1 à
/// 0,3) n'est pas touché.
const NIVEAU_CIBLE: f32 = 0.1;

/// Gain qui amène le niveau des passages joués (90ᵉ centile du RMS par
/// tranches de 100 ms) à [`NIVEAU_CIBLE`] ; jamais d'atténuation, au plus
/// ×100 (le silence reste du silence).
fn gain_niveau(x: &[f32]) -> f32 {
    let mut rms: Vec<f32> = x.chunks(FREQUENCE as usize / 10).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect();
    if rms.is_empty() {
        return 1.0;
    }
    rms.sort_by(f32::total_cmp);
    let niveau = rms[(rms.len() * 9 / 10).min(rms.len() - 1)];
    if niveau <= 1e-5 {
        return 1.0;
    }
    (NIVEAU_CIBLE / niveau).clamp(1.0, 100.0)
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

/// Révision épinglée des poids medium sur Hugging Face : l'empreinte et la
/// taille ci-dessous sont celles de ce fichier-là.
pub const REVISION: &str = "f32236969308476e01fd3aae67357de5feb05a2d";
pub const OCTETS: u64 = 1_228_144_472;
const SHA256: &str = "ac80adbdf85d87231735fd948af7013441c0afced316c4e9067fd5d8a7fb97ec";

/// Télécharge les poids medium dans le dossier des modèles de l'application,
/// avec le jeton Hugging Face de l'utilisateur : le dépôt est à accès
/// restreint (conditions d'usage à accepter sur huggingface.co, compte
/// requis). Le jeton n'est envoyé qu'à Hugging Face et n'est écrit nulle part.
pub fn telecharger(jeton: Option<&str>, avancer: impl FnMut(u64, Option<u64>)) -> Result<std::path::PathBuf> {
    let url = format!("https://huggingface.co/MuScriptor/muscriptor-medium/resolve/{REVISION}/model.safetensors");
    rusty_music_core::modeles::telecharger_avec_jeton(FICHIER, &url, jeton, OCTETS, Some(SHA256), avancer).map_err(|e| Error::Telechargement(e.to_string()))
}

/// Le jeton Hugging Face déjà posé sur la machine : `HF_TOKEN`, sinon le
/// fichier qu'écrit `hf auth login` (`$HF_HOME/token`, par défaut
/// `~/.cache/huggingface/token`).
pub fn jeton_hugging_face() -> Option<String> {
    let jeton = std::env::var("HF_TOKEN").ok().or_else(|| {
        let base = std::env::var_os("HF_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache/huggingface")))?;
        std::fs::read_to_string(base.join("token")).ok()
    })?;
    let jeton = jeton.trim().to_string();
    (!jeton.is_empty()).then_some(jeton)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_note_emise_pour_deux_basses_n_en_fait_qu_une() {
        let n = |d: f32, f: f32, h: u8| Note { debut_s: d, fin_s: f, hauteur: h, amplitude: 1.0, corde: None, frette: None };
        let r = sans_doublons(vec![n(3.01, 4.51, 38), n(3.01, 4.0, 43), n(3.02, 4.6, 38), n(4.51, 5.0, 38)]);
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].hauteur, r[0].fin_s), (38, 4.6));
    }

    #[test]
    fn le_gain_amplifie_un_signal_faible_et_jamais_un_fort() {
        let sinus = |a: f32| -> Vec<f32> { (0..FREQUENCE as usize * 2).map(|i| a * (i as f32 * 0.05).sin()).collect() };
        // RMS d'un sinus = a / √2
        assert!((gain_niveau(&sinus(0.01)) - NIVEAU_CIBLE / (0.01 / 2f32.sqrt())).abs() < 0.5);
        assert_eq!(gain_niveau(&sinus(0.5)), 1.0);
        assert_eq!(gain_niveau(&vec![0.0; 16_000]), 1.0);
    }
}
