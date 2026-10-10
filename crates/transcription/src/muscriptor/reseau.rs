// SPDX-License-Identifier: GPL-3.0-or-later
//! Le réseau de MuScriptor en Burn — port de `models/lm.py`,
//! `modules/transformer.py` et `modules/conditioners.py` (MIT).
//!
//! Transformer décodeur seul, pré-normalisation, GELU, sans biais,
//! positions sinusoïdales, cache clé-valeur préalloué. Le préfixe de
//! conditionnement — log-mel du segment projeté, classe de jeu de données,
//! groupes d'instruments — précède le premier jeton.
//!
//! Les noms des champs reprennent les clés du `model.safetensors` d'origine :
//! `burn-store` les charge tels quels (adaptateur PyTorch pour les
//! `LayerNorm`), seuls `emb.0` et `linears.0` sont renommés.
//!
//! Vitesse (mesurée sur Metal, `examples/micro_burn.rs`) : un pas de décodage
//! multiplie un vecteur par chaque matrice. Gardées dans la disposition
//! PyTorch `[sortie, entrée]` et lues transposées, elles vont 5 à 28 fois
//! plus vite qu'en `[entrée, sortie]` (celle du `Linear` de Burn) — d'où
//! [`Lineaire`]. L'attention est écrite à la main sur la partie remplie du
//! cache : 7 fois plus rapide que `module::attention` pour une requête.

use burn::module::{Module, Param};
use burn::nn::{Embedding, EmbeddingConfig, LayerNorm, LayerNormConfig};
use burn::tensor::activation::{gelu, softmax};
use burn::tensor::backend::Backend;
use burn::tensor::{Int, Tensor, TensorData};

use super::spectre;

/// Architecture d'une taille publiée (`config.json` du dépôt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub dim: usize,
    pub tetes: usize,
    pub couches: usize,
    /// Taille de la tête de sortie (le vocabulaire utile s'arrête à 1 393).
    pub card: usize,
}

impl Config {
    pub const SMALL: Config = Config { dim: 768, tetes: 12, couches: 14, card: 1393 };
    pub const MEDIUM: Config = Config { dim: 1024, tetes: 16, couches: 24, card: 1395 };
    pub const LARGE: Config = Config { dim: 1536, tetes: 24, couches: 48, card: 1395 };
}

const MELS: usize = 512;
const EPS_LOG: f32 = 1e-6;
const PERIODE_MAX: f32 = 10_000.0;

/// Couche linéaire aux poids `[sortie, entrée]` (disposition PyTorch).
#[derive(Module, Debug)]
pub struct Lineaire<B: Backend> {
    pub weight: Param<Tensor<B, 2>>,
    pub bias: Option<Param<Tensor<B, 1>>>,
}

impl<B: Backend> Lineaire<B> {
    fn new(entree: usize, sortie: usize, biais: bool, device: &B::Device) -> Self {
        Self {
            weight: Param::from_tensor(Tensor::zeros([sortie, entree], device)),
            bias: biais.then(|| Param::from_tensor(Tensor::zeros([sortie], device))),
        }
    }

    /// Les dimensions de tête sont aplaties : un produit 2D `[n, entrée]` ×
    /// `[entrée, sortie]`, plutôt qu'un produit par lot qui diffuserait les
    /// poids.
    pub fn forward<const D: usize>(&self, x: Tensor<B, D>) -> Tensor<B, D> {
        let mut forme = x.dims();
        let entree = forme[D - 1];
        let n: usize = forme[..D - 1].iter().product();
        let mut y = x.reshape([n, entree]).matmul(self.weight.val().transpose());
        if let Some(b) = &self.bias {
            y = y + b.val().unsqueeze::<2>();
        }
        forme[D - 1] = y.dims()[1];
        y.reshape(forme)
    }
}

#[derive(Module, Debug)]
pub struct Fenetre<B: Backend> {
    pub window: Param<Tensor<B, 1>>,
}

#[derive(Module, Debug)]
pub struct Banc<B: Backend> {
    pub fb: Param<Tensor<B, 2>>,
}

#[derive(Module, Debug)]
pub struct TransformeeMel<B: Backend> {
    pub spectrogram: Fenetre<B>,
    pub mel_scale: Banc<B>,
}

#[derive(Module, Debug)]
pub struct CondMel<B: Backend> {
    pub mel_spec_transform: TransformeeMel<B>,
    pub output_proj: Lineaire<B>,
}

#[derive(Module, Debug)]
pub struct CondClasse<B: Backend> {
    pub embed: Embedding<B>,
}

#[derive(Module, Debug)]
pub struct Conditionneurs<B: Backend> {
    pub self_wav: CondMel<B>,
    pub instrument_group: CondClasse<B>,
    pub dataset_name: CondClasse<B>,
}

#[derive(Module, Debug)]
pub struct Fournisseur<B: Backend> {
    pub conditioners: Conditionneurs<B>,
}

#[derive(Module, Debug)]
pub struct Attention<B: Backend> {
    /// `[3·dim, dim]`, disposition PyTorch (q, k, v à la suite).
    pub in_proj_weight: Param<Tensor<B, 2>>,
    pub out_proj: Lineaire<B>,
}

#[derive(Module, Debug)]
pub struct Couche<B: Backend> {
    pub self_attn: Attention<B>,
    pub norm1: LayerNorm<B>,
    pub norm2: LayerNorm<B>,
    pub linear1: Lineaire<B>,
    pub linear2: Lineaire<B>,
}

#[derive(Module, Debug)]
pub struct Pile<B: Backend> {
    pub layers: Vec<Couche<B>>,
}

#[derive(Module, Debug)]
pub struct Reseau<B: Backend> {
    pub condition_provider: Fournisseur<B>,
    pub emb: Embedding<B>,
    pub transformer: Pile<B>,
    pub out_norm: LayerNorm<B>,
    pub linear: Lineaire<B>,
}

impl<B: Backend> Reseau<B> {
    /// Structure vide aux bonnes formes ; les poids viennent ensuite du
    /// fichier (`Muscriptor::charger`).
    pub fn new(c: Config, device: &B::Device) -> Self {
        let lin = |i, o, biais| Lineaire::new(i, o, biais, device);
        let norme = || LayerNormConfig::new(c.dim).with_epsilon(1e-5).init(device);
        Self {
            condition_provider: Fournisseur {
                conditioners: Conditionneurs {
                    self_wav: CondMel {
                        mel_spec_transform: TransformeeMel {
                            spectrogram: Fenetre { window: Param::from_tensor(Tensor::zeros([spectre::N_FFT], device)) },
                            mel_scale: Banc { fb: Param::from_tensor(Tensor::zeros([spectre::BINS, MELS], device)) },
                        },
                        output_proj: lin(MELS, c.dim, true),
                    },
                    instrument_group: CondClasse { embed: EmbeddingConfig::new(1001, c.dim).init(device) },
                    dataset_name: CondClasse { embed: EmbeddingConfig::new(5, c.dim).init(device) },
                },
            },
            emb: EmbeddingConfig::new(c.card + 1, c.dim).init(device),
            transformer: Pile {
                layers: (0..c.couches)
                    .map(|_| Couche {
                        self_attn: Attention {
                            in_proj_weight: Param::from_tensor(Tensor::zeros([3 * c.dim, c.dim], device)),
                            out_proj: lin(c.dim, c.dim, false),
                        },
                        norm1: norme(),
                        norm2: norme(),
                        linear1: lin(c.dim, 4 * c.dim, false),
                        linear2: lin(4 * c.dim, c.dim, false),
                    })
                    .collect(),
            },
            out_norm: norme(),
            linear: lin(c.dim, c.card, false),
        }
    }
}

/// Le cache clé-valeur d'une génération : par couche, `[lot, têtes, T, d]`.
pub struct Cache<B: Backend> {
    k: Vec<Option<Tensor<B, 4>>>,
    v: Vec<Option<Tensor<B, 4>>>,
    pub position: usize,
}

/// Le réseau prêt à générer : poids, positions précalculées, dimensions.
pub struct Generateur<B: Backend> {
    pub reseau: Reseau<B>,
    pub config: Config,
    /// Positions sinusoïdales `[longueur_max, dim]`.
    positions: Tensor<B, 2>,
    longueur_max: usize,
    /// Fenêtre du spectre, gardée côté CPU.
    pub fenetre: Vec<f32>,
    device: B::Device,
}

impl<B: Backend> Generateur<B> {
    pub fn new(reseau: Reseau<B>, config: Config, longueur_max: usize, device: &B::Device) -> Self {
        let fenetre = reseau.condition_provider.conditioners.self_wav.mel_spec_transform.spectrogram.window.val().into_data();
        let fenetre = fenetre.convert::<f32>().into_vec::<f32>().expect("fenêtre en f32");
        Self { positions: positions(longueur_max, config.dim, device), reseau, config, longueur_max, fenetre, device: device.clone() }
    }

    pub fn device(&self) -> &B::Device {
        &self.device
    }

    /// Cache pour `lot` séquences décodées ensemble, de longueur `longueur`
    /// au plus (préfixe compris).
    pub fn cache(&self, lot: usize, longueur: usize) -> Cache<B> {
        let (h, d) = (self.config.tetes, self.config.dim / self.config.tetes);
        let longueur = longueur.min(self.longueur_max);
        let vide = || Some(Tensor::zeros([lot, h, longueur, d], &self.device));
        Cache { k: (0..self.config.couches).map(|_| vide()).collect(), v: (0..self.config.couches).map(|_| vide()).collect(), position: 0 }
    }

    /// Préfixe de conditionnement d'un segment `[prefixe, dim]` : log-mel
    /// projeté (dernière trame mise à zéro, comme le masque de longueur
    /// d'origine), jeu de données « inconnu », groupes d'instruments.
    pub fn prefixe(&self, segment: &[f32], groupes: &[u32]) -> Tensor<B, 2> {
        let c = &self.reseau.condition_provider.conditioners;
        let (module, trames) = spectre::module(segment, &self.fenetre);
        let spec = Tensor::<B, 2>::from_data(TensorData::new(module, [trames, spectre::BINS]), &self.device);
        let mel = spec.matmul(c.self_wav.mel_spec_transform.mel_scale.fb.val()).add_scalar(EPS_LOG).log();
        let mel = c.self_wav.output_proj.forward(mel);
        let utiles = segment.len() / spectre::PAS;
        let masque: Vec<f32> = (0..trames).map(|t| if t < utiles { 1.0 } else { 0.0 }).collect();
        let mel = mel * Tensor::<B, 2>::from_data(TensorData::new(masque, [trames, 1]), &self.device);
        // `ClassConditioner` : indice = classe + 2 ; « aucune » = 1.
        let classes = |ids: Vec<i64>, e: &Embedding<B>| {
            let n = ids.len();
            let t = Tensor::<B, 2, Int>::from_data(TensorData::new(ids, [1, n]), &self.device);
            e.forward(t).squeeze_dim::<2>(0)
        };
        let jeu = classes(vec![1], &c.dataset_name.embed);
        let ids: Vec<i64> = if groupes.is_empty() { vec![1] } else { groupes.iter().map(|&g| g as i64 + 2).collect() };
        let inst = classes(ids, &c.instrument_group.embed);
        Tensor::cat(vec![mel, jeu, inst], 0)
    }

    /// Plonge des jetons : `[1, T, dim]`.
    pub fn plonger(&self, jetons: &[u32]) -> Tensor<B, 3> {
        let ids: Vec<i64> = jetons.iter().map(|&j| j as i64).collect();
        let t = Tensor::<B, 2, Int>::from_data(TensorData::new(ids, [1, jetons.len()]), &self.device);
        self.reseau.emb.forward(t)
    }

    /// Fait avancer le réseau de `x` (`[lot, T, dim]`, toutes les séquences à
    /// la même position) et rend les logits de la dernière, `[lot, card]`.
    pub fn avancer(&self, x: Tensor<B, 3>, cache: &mut Cache<B>) -> Tensor<B, 2> {
        let [b, t, dim] = x.dims();
        let p = cache.position;
        assert!(t == 1 || p == 0, "préremplissage seulement au premier pas");
        let mut x = x + self.positions.clone().slice([p..p + t, 0..dim]).unsqueeze_dim::<3>(0);
        for (i, couche) in self.reseau.transformer.layers.iter().enumerate() {
            let a = self.attention(&couche.self_attn, couche.norm1.forward(x.clone()), cache, i);
            x = x + a;
            let f = couche.linear2.forward(gelu(couche.linear1.forward(couche.norm2.forward(x.clone()))));
            x = x + f;
        }
        cache.position += t;
        let dernier = x.slice([0..b, t - 1..t, 0..dim]).reshape([b, dim]);
        self.reseau.linear.forward(self.reseau.out_norm.forward(dernier))
    }

    fn attention(&self, a: &Attention<B>, x: Tensor<B, 3>, cache: &mut Cache<B>, couche: usize) -> Tensor<B, 3> {
        let [b, t, dim] = x.dims();
        let (h, d) = (self.config.tetes, dim / self.config.tetes);
        let qkv = x.reshape([b * t, dim]).matmul(a.in_proj_weight.val().transpose()).reshape([b, t, 3 * dim]);
        let tete = |r: std::ops::Range<usize>| qkv.clone().slice([0..b, 0..t, r]).reshape([b, t, h, d]).swap_dims(1, 2);
        let (q, k, v) = (tete(0..dim), tete(dim..2 * dim), tete(2 * dim..3 * dim));
        let p = cache.position;
        let kc = cache.k[couche].take().unwrap();
        assert!(p + t <= kc.dims()[2], "séquence plus longue que le cache");
        let kc = kc.slice_assign([0..b, 0..h, p..p + t, 0..d], k);
        let vc = cache.v[couche].take().unwrap().slice_assign([0..b, 0..h, p..p + t, 0..d], v);
        let ka = kc.clone().slice([0..b, 0..h, 0..p + t, 0..d]);
        let va = vc.clone().slice([0..b, 0..h, 0..p + t, 0..d]);
        cache.k[couche] = Some(kc);
        cache.v[couche] = Some(vc);
        let mut s = q.matmul(ka.swap_dims(2, 3)).mul_scalar(1.0 / (d as f32).sqrt());
        if t > 1 {
            // Préremplissage (p = 0) : causal, carré. `tril_mask` vaut vrai
            // au-dessus de la diagonale (Burn nomme la partie gardée).
            s = s.mask_fill(Tensor::<B, 2, burn::tensor::Bool>::tril_mask([t, t], 0, &self.device).unsqueeze::<4>().expand([b, h, t, t]), f32::NEG_INFINITY);
        }
        let o = softmax(s, 3).matmul(va);
        a.out_proj.forward(o.swap_dims(1, 2).reshape([b, t, dim]))
    }
}

/// `create_sin_embedding` : phase = pos / 10000^(i / (dim/2 − 1)), puis
/// [cos, sin].
fn positions<B: Backend>(n: usize, dim: usize, device: &B::Device) -> Tensor<B, 2> {
    let demi = dim / 2;
    let mut v = vec![0.0f32; n * dim];
    for p in 0..n {
        for i in 0..demi {
            let phase = p as f32 / PERIODE_MAX.powf(i as f32 / (demi - 1) as f32);
            v[p * dim + i] = phase.cos();
            v[p * dim + demi + i] = phase.sin();
        }
    }
    Tensor::from_data(TensorData::new(v, [n, dim]), device)
}
