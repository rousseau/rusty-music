// SPDX-License-Identifier: GPL-3.0-or-later
//! Démixage par BS-RoFormer (Lu et al., arXiv 2309.02612), réseau sur Burn,
//! STFT en Rust. Choix et banc : `docs/module3-roformer.md`,
//! `experiments/roformer/README.md`.
//!
//! Les poids sont lus tels que publiés (`.ckpt` PyTorch, par `burn-store`) :
//! rien à convertir, donc rien à republier — la licence du modèle « SW »
//! n'est pas déclarée.
//!
//! Le découpage en segments reprend `demix` de Music-Source-Separation-
//! Training (MIT) : segments recouvrants, fondu linéaire d'un dixième du
//! segment, bords du morceau prolongés par réflexion.

pub mod reseau;
mod stft;

use std::path::Path;

use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};
use burn_store::{KeyRemapper, ModuleSnapshot, PytorchStore};

use crate::{Error, Result};

/// Les 62 bandes de BS-RoFormer (cases de fréquence par bande, 1 025 en tout).
pub const BANDES: [usize; 62] = [
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, //
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, //
    12, 12, 12, 12, 12, 12, 12, 12, //
    24, 24, 24, 24, 24, 24, 24, 24, //
    48, 48, 48, 48, 48, 48, 48, 48, //
    128, 129,
];

/// Nom du fichier de poids de SW, tel que publié.
pub const FICHIER_SW: &str = "BS-Rofo-SW-Fixed.ckpt";
/// Son empreinte (`scripts/preparer-roformer.sh`, `FractalSurfer/BS-RoFormer-SW-6stem`).
pub const SHA256_SW: &str = "24e7d35ee9c64415673d3fd33e06a67cac2c103c5df6267ba1576459c775916e";

/// Architecture et réglages d'inférence d'un modèle publié (son `.yaml`).
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub dim: usize,
    pub profondeur: usize,
    pub tetes: usize,
    pub dim_tete: usize,
    pub bandes: Vec<usize>,
    /// Noms des stems, dans l'ordre des têtes de masque.
    pub stems: &'static [&'static str],
    /// `mlp_expansion_factor` des têtes de masque.
    pub expansion_masque: usize,
    pub n_fft: usize,
    pub saut: usize,
    /// Longueur d'un segment, en échantillons (`chunk_size`).
    pub segment: usize,
    /// Recouvrement (`num_overlap`) : un segment avance de `segment / recouvrement`.
    pub recouvrement: usize,
    /// Séquences d'attention calculées ensemble — ne change pas le résultat,
    /// seulement la mémoire.
    pub tranche_temps: usize,
    pub tranche_frequence: usize,
}

impl Config {
    /// BS-RoFormer « SW », 6 stems (`BS-Rofo-SW-Fixed.yaml`).
    pub fn sw() -> Self {
        Self {
            dim: 256,
            profondeur: 12,
            tetes: 8,
            dim_tete: 64,
            bandes: BANDES.to_vec(),
            stems: &["bass", "drums", "other", "vocals", "guitar", "piano"],
            expansion_masque: 4,
            n_fft: 2048,
            saut: 512,
            segment: 588_800,
            recouvrement: 2,
            tranche_temps: 8,
            tranche_frequence: 2048,
        }
    }
}

/// Clés du `.ckpt` → champs de [`reseau::Reseau`]. PyTorch numérote les
/// `nn.Sequential` et les `ModuleList` de modules hétérogènes ; on nomme.
fn renommage() -> std::result::Result<KeyRemapper, String> {
    let motifs = [
        (r"^layers\.(\d+)\.0\.", "layers.$1.temps."),
        (r"^layers\.(\d+)\.1\.", "layers.$1.frequence."),
        (r"\.layers\.(\d+)\.0\.", ".layers.$1.attn."),
        (r"\.layers\.(\d+)\.1\.net\.0\.", ".layers.$1.ff.norme."),
        (r"\.layers\.(\d+)\.1\.net\.1\.", ".layers.$1.ff.entree."),
        (r"\.layers\.(\d+)\.1\.net\.4\.", ".layers.$1.ff.sortie."),
        (r"\.to_out\.0\.", ".to_out."),
        (r"^band_split\.to_features\.(\d+)\.0\.", "band_split.to_features.$1.norme."),
        (r"^band_split\.to_features\.(\d+)\.1\.", "band_split.to_features.$1.lineaire."),
        (r"\.to_freqs\.(\d+)\.0\.0\.", ".to_freqs.$1.couches.0."),
        (r"\.to_freqs\.(\d+)\.0\.2\.", ".to_freqs.$1.couches.1."),
    ];
    let mut r = KeyRemapper::new();
    for (de, vers) in motifs {
        r = r.add_pattern(de, vers).map_err(|e| e.to_string())?;
    }
    Ok(r)
}

pub struct Roformer<B: Backend> {
    reseau: reseau::Reseau<B>,
    tetes: reseau::Tetes<B>,
    config: Config,
    fenetre: Vec<f64>,
    device: B::Device,
}

impl<B: Backend> Roformer<B> {
    /// Charge le `.ckpt` d'origine.
    pub fn charger(poids: &Path, config: Config, device: &B::Device) -> Result<Self> {
        let mut reseau = reseau::Reseau::<B>::new(&config, device);
        let mut store = PytorchStore::from_file(poids).remap(renommage().map_err(Error::Modele)?);
        let r = reseau.load_from(&mut store).map_err(|e| Error::PoidsAbsents(format!("{} : {e}", poids.display())))?;
        if !r.missing.is_empty() || !r.errors.is_empty() {
            return Err(Error::Modele(format!("poids manquants {:?}, erreurs {:?}", r.missing, r.errors)));
        }
        let tetes = reseau::Tetes::depuis(std::mem::take(&mut reseau.mask_estimators), &config.bandes);
        let fenetre = stft::hann(config.n_fft);
        Ok(Self { reseau, tetes, config, fenetre, device: device.clone() })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn reseau(&self) -> &reseau::Reseau<B> {
        &self.reseau
    }

    /// Spectre d'un segment stéréo, à plat comme l'attend le réseau :
    /// `[trames, (fréquence, canal, réel/imaginaire)]`.
    pub fn spectre(&self, gauche: &[f32], droite: &[f32]) -> (Vec<f32>, usize) {
        let c = &self.config;
        let g = stft::directe(gauche, c.n_fft, c.saut, &self.fenetre);
        let d = stft::directe(droite, c.n_fft, c.saut, &self.fenetre);
        let cases = c.n_fft / 2 + 1;
        let trames = g.len() / cases;
        let mut x = Vec::with_capacity(trames * cases * 4);
        for t in 0..trames {
            for f in 0..cases {
                for canal in [&g, &d] {
                    let v = canal[t * cases + f];
                    x.extend_from_slice(&[v.re as f32, v.im as f32]);
                }
            }
        }
        (x, trames)
    }

    /// Le réseau seul, sur un spectre de [`Self::spectre`] : masques
    /// `[stems, trames, (fréquence, canal, réel/imaginaire)]`.
    pub fn masques(&self, spectre: Vec<f32>, trames: usize) -> Tensor<B, 3> {
        let largeur = spectre.len() / trames;
        let x = Tensor::<B, 2>::from_data(TensorData::new(spectre, [trames, largeur]), &self.device);
        let mut x = self.reseau.decouper(x, &self.config.bandes);
        for i in 0..self.config.profondeur {
            x = self.reseau.bloc(i, x, &self.config);
        }
        self.tetes(x)
    }

    /// Normalisation finale et têtes de masque, sur la sortie du dernier
    /// bloc `[trames, bandes, dim]`.
    pub fn tetes(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        self.tetes.forward(self.reseau.final_norm.forward(x))
    }

    /// Un segment, comme `BSRoformer.forward` : `[stems][canal][échantillon]`,
    /// de la longueur de l'entrée.
    pub fn segment(&self, gauche: &[f32], droite: &[f32]) -> Result<Vec<[Vec<f32>; 2]>> {
        let c = &self.config;
        let (spectre, trames) = self.spectre(gauche, droite);
        let masques: Vec<f32> = self.masques(spectre.clone(), trames).into_data().convert::<f32>().into_vec().map_err(|e| Error::Modele(format!("{e:?}")))?;
        let cases = c.n_fft / 2 + 1;
        let largeur = cases * 4;
        let mut sorties = Vec::with_capacity(c.stems.len());
        for s in 0..c.stems.len() {
            let m = &masques[s * trames * largeur..(s + 1) * trames * largeur];
            let canal = |k: usize| {
                let mut z = vec![stft::Complexe::default(); trames * cases];
                for t in 0..trames {
                    // `zero_dc` : la case 0 reste nulle.
                    for f in 1..cases {
                        let i = t * largeur + (f * 2 + k) * 2;
                        let x = stft::Complexe::new(spectre[i] as f64, spectre[i + 1] as f64);
                        let y = stft::Complexe::new(m[i] as f64, m[i + 1] as f64);
                        z[t * cases + f] = x * y;
                    }
                }
                stft::inverse(&z, cases, c.n_fft, c.saut, &self.fenetre, gauche.len())
            };
            sorties.push([canal(0), canal(1)]);
        }
        Ok(sorties)
    }

    /// Un morceau entier, comme `demix` de MSST : `[stems][canal][échantillon]`.
    /// `avancer(fait, total)` après chaque segment.
    pub fn separer(&self, gauche: &[f32], droite: &[f32], mut avancer: impl FnMut(usize, usize)) -> Result<Vec<[Vec<f32>; 2]>> {
        let c = &self.config;
        let n = gauche.len();
        let taille = c.segment;
        let fondu = taille / 10;
        let pas = taille / c.recouvrement;
        let bord = taille - pas;
        // Prolonge les deux bords par réflexion quand le morceau est assez long.
        let rembourre = n > 2 * bord && bord > 0;
        let etendre = |x: &[f32]| if rembourre { reflechir(x, bord, bord) } else { x.to_vec() };
        let (g, d) = (etendre(gauche), etendre(droite));
        let longueur = g.len();
        let mut fenetre = vec![1.0f32; taille];
        for i in 0..fondu {
            fenetre[i] = i as f32 / (fondu - 1) as f32;
            fenetre[taille - fondu + i] = 1.0 - i as f32 / (fondu - 1) as f32;
        }
        let mut resultat = vec![[vec![0.0f32; longueur], vec![0.0f32; longueur]]; c.stems.len()];
        let mut compte = vec![0.0f32; longueur];
        let total = longueur.div_ceil(pas);
        let mut i = 0;
        let mut fait = 0;
        avancer(0, total);
        while i < longueur {
            let fin = (i + taille).min(longueur);
            let lu = fin - i;
            // Segment incomplet : réflexion s'il dépasse la moitié, zéros sinon.
            let completer = |x: &[f32]| if lu > taille / 2 { reflechir(&x[i..fin], 0, taille - lu) } else { let mut v = x[i..fin].to_vec(); v.resize(taille, 0.0); v };
            let sortie = self.segment(&completer(&g), &completer(&d))?;
            let mut f = fenetre.clone();
            if i == 0 {
                f[..fondu].fill(1.0);
            } else if i + pas >= longueur {
                f[taille - fondu..].fill(1.0);
            }
            for (r, s) in resultat.iter_mut().zip(&sortie) {
                for k in 0..2 {
                    for j in 0..lu {
                        r[k][i + j] += s[k][j] * f[j];
                    }
                }
            }
            for j in 0..lu {
                compte[i + j] += f[j];
            }
            i += pas;
            fait += 1;
            avancer(fait, total);
        }
        let debut = if rembourre { bord } else { 0 };
        Ok(resultat
            .into_iter()
            .map(|[a, b]| {
                let couper = |v: Vec<f32>| v[debut..debut + n].iter().zip(&compte[debut..debut + n]).map(|(x, c)| x / c).collect();
                [couper(a), couper(b)]
            })
            .collect())
    }
}

/// `F.pad(x, (avant, apres), mode="reflect")` : réflexion sans répéter le bord.
fn reflechir(x: &[f32], avant: usize, apres: usize) -> Vec<f32> {
    let n = x.len();
    let mut v = Vec::with_capacity(avant + n + apres);
    v.extend((1..=avant).rev().map(|k| x[k]));
    v.extend_from_slice(x);
    v.extend((1..=apres).map(|k| x[n - 1 - k]));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Moteur;

    fn cosinus(a: &[f32], b: &[f32]) -> f64 {
        let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
        for (&x, &y) in a.iter().zip(b) {
            ab += x as f64 * y as f64;
            aa += x as f64 * x as f64;
            bb += y as f64 * y as f64;
        }
        ab / (aa.sqrt() * bb.sqrt())
    }

    #[test]
    fn la_reflexion_ne_repete_pas_le_bord() {
        // torch : F.pad(tensor([0, 1, 2, 3]), (2, 3), mode="reflect")
        assert_eq!(reflechir(&[0.0, 1.0, 2.0, 3.0], 2, 3), vec![2.0, 1.0, 0.0, 1.0, 2.0, 3.0, 2.0, 1.0, 0.0]);
    }

    #[test]
    fn la_stft_se_defait() {
        let (n, saut) = (2048, 512);
        let fenetre = stft::hann(n);
        let x: Vec<f32> = (0..20_000).map(|i| ((i as f32 * 0.013).sin() + (i as f32 * 0.0007).cos()) * 0.5).collect();
        let z = stft::directe(&x, n, saut, &fenetre);
        let y = stft::inverse(&z, n / 2 + 1, n, saut, &fenetre, x.len());
        let ecart = x.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(ecart < 1e-5, "écart {ecart}");
    }

    /// Les têtes regroupées rendent ce que rend un perceptron par bande,
    /// tel que l'écrit l'original.
    #[test]
    fn les_tetes_regroupees_rendent_les_masques_bande_par_bande() {
        let device = Default::default();
        let config = Config { dim: 8, profondeur: 1, tetes: 2, dim_tete: 4, bandes: vec![2, 2, 3, 3, 3, 5], stems: &["a", "b"], expansion_masque: 2, ..Config::sw() };
        let reseau = reseau::Reseau::<Moteur>::new(&config, &device);
        let forme = [7, config.bandes.len(), config.dim];
        let valeurs: Vec<f32> = (0..forme.iter().product::<usize>()).map(|i| (i as f32 * 0.37).sin()).collect();
        let x = Tensor::<Moteur, 3>::from_data(TensorData::new(valeurs, forme), &device);
        let mut attendu = Vec::new();
        for m in &reseau.mask_estimators {
            let parties: Vec<Tensor<Moteur, 2>> = m
                .to_freqs
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let xb = x.clone().slice_dim(1, i..i + 1).squeeze_dim::<2>(1);
                    let y = p.couches[1].forward(p.couches[0].forward(xb).tanh());
                    let moitie = y.dims()[1] / 2;
                    y.clone().slice_dim(1, 0..moitie) * burn::tensor::activation::sigmoid(y.slice_dim(1, moitie..2 * moitie))
                })
                .collect();
            attendu.extend(Tensor::cat(parties, 1).into_data().into_vec::<f32>().unwrap());
        }
        // Après le calcul de référence : les paramètres d'un `Linear` neuf
        // ne sont tirés qu'au premier accès, et un clone pris avant tirerait
        // les siens.
        let tetes = reseau::Tetes::depuis(reseau.mask_estimators.clone(), &config.bandes);
        let groupees: Vec<f32> = tetes.forward(x).into_data().into_vec().unwrap();
        assert_eq!(groupees.len(), attendu.len());
        assert!(cosinus(&groupees, &attendu) > 0.999_999, "{}", cosinus(&groupees, &attendu));
    }

    /// Parité avec le code d'origine sur un segment de SW : références de
    /// `experiments/roformer/reference.py` (dossier `ROFORMER_REF`), poids
    /// `BS-Rofo-SW-Fixed.ckpt` (`ROFORMER_POIDS`, sinon `models/`).
    ///
    ///   ROFORMER_REF=… cargo test --release -p rusty-music-editor -- --ignored parite_sw
    #[test]
    #[ignore = "poids de 700 Mo et références à préparer"]
    fn parite_sw_avec_msst() {
        let dossier = std::path::PathBuf::from(std::env::var("ROFORMER_REF").expect("ROFORMER_REF"));
        let poids = std::env::var("ROFORMER_POIDS").map(std::path::PathBuf::from).ok().or_else(|| rusty_music_core::modeles::trouver(FICHIER_SW)).expect("poids de SW");
        let lire = |nom: &str| -> Vec<f32> { std::fs::read(dossier.join(nom)).unwrap().chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
        let m = Roformer::<Moteur>::charger(&poids, Config::sw(), &Default::default()).unwrap();
        let audio = lire("audio.f32");
        let (g, d) = audio.split_at(audio.len() / 2);
        let seg = m.config().segment.min(g.len());
        let sortie = m.segment(&g[..seg], &d[..seg]).unwrap();
        let reference = lire("segment.f32");
        let par_stem = reference.len() / sortie.len();
        for (s, [a, b]) in sortie.iter().enumerate() {
            let plat: Vec<f32> = a.iter().chain(b).copied().collect();
            let c = cosinus(&plat, &reference[s * par_stem..(s + 1) * par_stem]);
            assert!(c >= 0.9999, "{} : cosinus {c}", m.config().stems[s]);
        }
    }
}
