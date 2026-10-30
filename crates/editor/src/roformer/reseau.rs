// SPDX-License-Identifier: GPL-3.0-or-later
//! Le réseau BS-RoFormer en Burn — port de `models/bs_roformer/bs_roformer.py`
//! de Music-Source-Separation-Training (ZFTurbo, MIT), lui-même repris de
//! `lucidrains/BS-RoFormer` (MIT).
//!
//! Le spectrogramme est découpé en bandes de fréquence ; chaque bande est
//! projetée en un vecteur, puis un Transformer alterne l'attention dans le
//! temps (une séquence par bande) et entre les bandes (une séquence par
//! trame), avec des positions rotatives (RoPE). Une tête par stem rend un
//! masque complexe par bande.
//!
//! Les champs reprennent les clés du `.ckpt` d'origine, renommées par
//! [`super::renommage`] là où PyTorch numérote (`nn.Sequential`).

use burn::module::{Module, Param};
use burn::nn::{Linear, LinearConfig};
use burn::tensor::activation::{gelu, sigmoid, softmax};
use burn::tensor::backend::Backend;
use burn::tensor::Tensor;

use super::Config;

/// `F.normalize(x, dim=-1) · √dim · γ`. Pas le `RmsNorm` de Burn : celui-ci
/// divise par la moyenne des carrés plus un ε, l'original par la norme
/// bornée à 10⁻¹² — et les bandes presque muettes y sont sensibles.
#[derive(Module, Debug)]
pub struct NormeRms<B: Backend> {
    pub gamma: Param<Tensor<B, 1>>,
}

impl<B: Backend> NormeRms<B> {
    fn new(dim: usize, device: &B::Device) -> Self {
        Self { gamma: Param::from_tensor(Tensor::ones([dim], device)) }
    }

    pub fn forward<const D: usize>(&self, x: Tensor<B, D>) -> Tensor<B, D> {
        let dim = x.dims()[D - 1];
        let norme = x.clone().powi_scalar(2).sum_dim(D - 1).sqrt().clamp_min(1e-12);
        x / norme * (dim as f32).sqrt() * self.gamma.val().unsqueeze::<D>()
    }
}

/// `Linear` sur `[lot, n, entrée]` aplati en un seul produit 2D :
/// `Linear::forward` diffuse les poids sur le lot, et pour l'attention
/// entre les bandes (1 151 séquences de 62) ce produit par lot coûte le
/// double.
fn appliquer<B: Backend>(l: &Linear<B>, x: Tensor<B, 3>) -> Tensor<B, 3> {
    let [lot, n, entree] = x.dims();
    let y = x.reshape([lot * n, entree]).matmul(l.weight.val());
    let y = match &l.bias {
        Some(b) => y + b.val().unsqueeze::<2>(),
        None => y,
    };
    let sortie = y.dims()[1];
    y.reshape([lot, n, sortie])
}

fn lineaire<B: Backend>(entree: usize, sortie: usize, biais: bool, device: &B::Device) -> Linear<B> {
    LinearConfig::new(entree, sortie).with_bias(biais).init(device)
}

/// Fréquences de RoPE (`rotary_embed.freqs`, une par paire de la tête).
#[derive(Module, Debug)]
pub struct Rotation<B: Backend> {
    pub freqs: Param<Tensor<B, 1>>,
}

impl<B: Backend> Rotation<B> {
    /// Applique la rotation à `x` `[lot, têtes, n, d]`, paires adjacentes
    /// `(x₀, x₁)` comme `rotary_embedding_torch` (`rotate_half` entrelacé).
    fn appliquer(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        let [lot, tetes, n, d] = x.dims();
        let device = x.device();
        let positions = Tensor::<B, 1, burn::tensor::Int>::arange(0..n as i64, &device).float();
        // [n, d/2]
        let angles = positions.unsqueeze_dim::<2>(1) * self.freqs.val().unsqueeze_dim::<2>(0);
        let (cos, sin) = (angles.clone().cos().reshape([1, 1, n, d / 2, 1]), angles.sin().reshape([1, 1, n, d / 2, 1]));
        let paires = x.reshape([lot, tetes, n, d / 2, 2]);
        let x0 = paires.clone().slice_dim(4, 0..1);
        let x1 = paires.slice_dim(4, 1..2);
        let y0 = x0.clone() * cos.clone() - x1.clone() * sin.clone();
        let y1 = x1 * cos + x0 * sin;
        Tensor::cat(vec![y0, y1], 4).reshape([lot, tetes, n, d])
    }
}

#[derive(Module, Debug)]
pub struct Attention<B: Backend> {
    pub norm: NormeRms<B>,
    pub to_qkv: Linear<B>,
    pub to_gates: Linear<B>,
    pub to_out: Linear<B>,
    pub rotary_embed: Rotation<B>,
}

impl<B: Backend> Attention<B> {
    /// `x` `[lot, n, dim]` ; l'attention est calculée par tranches de
    /// `tranche` séquences pour borner la matrice des scores
    /// (`lot · têtes · n²` flottants : 2,6 Go pour l'attention dans le temps
    /// d'un segment entier).
    pub fn forward(&self, x: Tensor<B, 3>, tetes: usize, tranche: usize) -> Tensor<B, 3> {
        let [lot, n, _] = x.dims();
        let x = self.norm.forward(x);
        let qkv = appliquer(&self.to_qkv, x.clone());
        let interne = qkv.dims()[2] / 3;
        let d = interne / tetes;
        // 'b n (qkv h d) -> qkv b h n d'
        let part = |i: usize| qkv.clone().slice_dim(2, i * interne..(i + 1) * interne).reshape([lot, n, tetes, d]).swap_dims(1, 2);
        let q = self.rotary_embed.appliquer(part(0));
        let k = self.rotary_embed.appliquer(part(1));
        let v = part(2);
        let echelle = (d as f32).powf(-0.5);
        let mut sorties = Vec::with_capacity(lot.div_ceil(tranche));
        for debut in (0..lot).step_by(tranche) {
            let fin = (debut + tranche).min(lot);
            let (q, k, v) = (q.clone().slice_dim(0, debut..fin), k.clone().slice_dim(0, debut..fin), v.clone().slice_dim(0, debut..fin));
            let scores = softmax(q.matmul(k.swap_dims(2, 3)) * echelle, 3);
            sorties.push(scores.matmul(v));
        }
        let sortie = if sorties.len() == 1 { sorties.pop().unwrap() } else { Tensor::cat(sorties, 0) };
        let portes = sigmoid(appliquer(&self.to_gates, x)).swap_dims(1, 2).unsqueeze_dim::<4>(3);
        let sortie = (sortie * portes).swap_dims(1, 2).reshape([lot, n, interne]);
        appliquer(&self.to_out, sortie)
    }
}

#[derive(Module, Debug)]
pub struct Anticipation<B: Backend> {
    pub norme: NormeRms<B>,
    pub entree: Linear<B>,
    pub sortie: Linear<B>,
}

impl<B: Backend> Anticipation<B> {
    pub fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        appliquer(&self.sortie, gelu(appliquer(&self.entree, self.norme.forward(x))))
    }
}

#[derive(Module, Debug)]
pub struct Couche<B: Backend> {
    pub attn: Attention<B>,
    pub ff: Anticipation<B>,
}

#[derive(Module, Debug)]
pub struct Transformer<B: Backend> {
    pub layers: Vec<Couche<B>>,
}

impl<B: Backend> Transformer<B> {
    fn forward(&self, mut x: Tensor<B, 3>, tetes: usize, tranche: usize) -> Tensor<B, 3> {
        for c in &self.layers {
            x = c.attn.forward(x.clone(), tetes, tranche) + x;
            x = c.ff.forward(x.clone()) + x;
        }
        x
    }
}

#[derive(Module, Debug)]
pub struct Bloc<B: Backend> {
    pub temps: Transformer<B>,
    pub frequence: Transformer<B>,
}

#[derive(Module, Debug)]
pub struct Projection<B: Backend> {
    pub norme: NormeRms<B>,
    pub lineaire: Linear<B>,
}

#[derive(Module, Debug)]
pub struct Decoupe<B: Backend> {
    pub to_features: Vec<Projection<B>>,
}

/// Perceptron d'une bande : linéaires séparés par des `tanh`, puis GLU.
#[derive(Module, Debug)]
pub struct Perceptron<B: Backend> {
    pub couches: Vec<Linear<B>>,
}

#[derive(Module, Debug)]
pub struct Masque<B: Backend> {
    pub to_freqs: Vec<Perceptron<B>>,
}

#[derive(Module, Debug)]
pub struct Reseau<B: Backend> {
    pub band_split: Decoupe<B>,
    pub layers: Vec<Bloc<B>>,
    pub final_norm: NormeRms<B>,
    pub mask_estimators: Vec<Masque<B>>,
}

impl<B: Backend> Reseau<B> {
    /// Structure aux bonnes formes ; les poids viennent ensuite du fichier.
    pub fn new(c: &Config, device: &B::Device) -> Self {
        let interne = c.tetes * c.dim_tete;
        let transformer = || Transformer {
            layers: vec![Couche {
                attn: Attention {
                    norm: NormeRms::new(c.dim, device),
                    to_qkv: lineaire(c.dim, 3 * interne, false, device),
                    to_gates: lineaire(c.dim, c.tetes, true, device),
                    to_out: lineaire(interne, c.dim, false, device),
                    rotary_embed: Rotation { freqs: Param::from_tensor(Tensor::zeros([c.dim_tete / 2], device)) },
                },
                ff: Anticipation {
                    norme: NormeRms::new(c.dim, device),
                    entree: lineaire(c.dim, 4 * c.dim, true, device),
                    sortie: lineaire(4 * c.dim, c.dim, true, device),
                },
            }],
        };
        let cache = c.dim * c.expansion_masque;
        Self {
            band_split: Decoupe {
                to_features: c.bandes.iter().map(|&b| Projection { norme: NormeRms::new(4 * b, device), lineaire: lineaire(4 * b, c.dim, true, device) }).collect(),
            },
            layers: (0..c.profondeur).map(|_| Bloc { temps: transformer(), frequence: transformer() }).collect(),
            final_norm: NormeRms::new(c.dim, device),
            mask_estimators: (0..c.stems.len())
                .map(|_| Masque {
                    to_freqs: c
                        .bandes
                        .iter()
                        .map(|&b| Perceptron { couches: vec![lineaire(c.dim, cache, true, device), lineaire(cache, 2 * 4 * b, true, device)] })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Les bandes projetées : `x` `[trames, Σ 4·bandes]` (fréquence, canal,
    /// réel/imaginaire, dans cet ordre) → `[trames, bandes, dim]`.
    pub fn decouper(&self, x: Tensor<B, 2>, bandes: &[usize]) -> Tensor<B, 3> {
        let mut debut = 0;
        let mut sorties = Vec::with_capacity(bandes.len());
        for (p, &b) in self.band_split.to_features.iter().zip(bandes) {
            let partie = x.clone().slice_dim(1, debut..debut + 4 * b);
            sorties.push(p.lineaire.forward(p.norme.forward(partie)));
            debut += 4 * b;
        }
        Tensor::stack(sorties, 1)
    }

    /// Un bloc : attention dans le temps puis entre les bandes, sur
    /// `x` `[trames, bandes, dim]`.
    pub fn bloc(&self, i: usize, x: Tensor<B, 3>, c: &Config) -> Tensor<B, 3> {
        let b = &self.layers[i];
        let x = b.temps.forward(x.swap_dims(0, 1), c.tetes, c.tranche_temps).swap_dims(0, 1);
        b.frequence.forward(x, c.tetes, c.tranche_frequence)
    }
}

/// Têtes de masque regroupées : PyTorch applique un perceptron par bande
/// et par stem, soit 744 petits produits pour SW, que le GPU paie chacun.
/// Les premières couches ont la même forme pour toutes les bandes (un seul
/// produit par lot) ; les secondes, la même par largeur de bande (sept
/// produits). Mêmes poids, mêmes opérations, dans un autre ordre.
pub struct Tetes<B: Backend> {
    stems: Vec<TeteStem<B>>,
}

struct TeteStem<B: Backend> {
    /// `[bandes, dim, caché]` et `[bandes, 1, caché]`.
    entree: (Tensor<B, 3>, Tensor<B, 3>),
    /// Bandes consécutives de même largeur : `[n, caché, 8·largeur]`.
    sorties: Vec<(std::ops::Range<usize>, Tensor<B, 3>, Tensor<B, 3>)>,
}

impl<B: Backend> Tetes<B> {
    /// Empile les poids chargés ; `masques` est consommé (pas de double copie).
    pub fn depuis(masques: Vec<Masque<B>>, bandes: &[usize]) -> Self {
        let mut groupes: Vec<std::ops::Range<usize>> = Vec::new();
        for (i, &b) in bandes.iter().enumerate() {
            match groupes.last_mut() {
                Some(g) if bandes[g.start] == b => g.end = i + 1,
                _ => groupes.push(i..i + 1),
            }
        }
        let empiler = |l: Vec<&Linear<B>>| {
            let poids = Tensor::stack::<3>(l.iter().map(|l| l.weight.val()).collect(), 0);
            let biais = Tensor::stack::<2>(l.iter().map(|l| l.bias.as_ref().expect("biais").val()).collect(), 0).unsqueeze_dim::<3>(1);
            (poids, biais)
        };
        let stems = masques
            .iter()
            .map(|m| {
                assert!(m.to_freqs.iter().all(|p| p.couches.len() == 2), "tête de masque de profondeur 2 seulement");
                let entree = empiler(m.to_freqs.iter().map(|p| &p.couches[0]).collect());
                let sorties = groupes
                    .iter()
                    .map(|g| {
                        let (w, b) = empiler(m.to_freqs[g.clone()].iter().map(|p| &p.couches[1]).collect());
                        (g.clone(), w, b)
                    })
                    .collect();
                TeteStem { entree, sorties }
            })
            .collect();
        Self { stems }
    }

    /// `x` `[trames, bandes, dim]` (après `final_norm`) → masques
    /// `[stems, trames, Σ 4·bandes]`, dans l'ordre du spectre d'entrée.
    pub fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        let trames = x.dims()[0];
        let x = x.swap_dims(0, 1);
        let par_stem = self
            .stems
            .iter()
            .map(|s| {
                let h = (x.clone().matmul(s.entree.0.clone()) + s.entree.1.clone()).tanh();
                let parties = s
                    .sorties
                    .iter()
                    .map(|(g, w, b)| {
                        let y = h.clone().slice_dim(0, g.clone()).matmul(w.clone()) + b.clone();
                        let moitie = y.dims()[2] / 2;
                        let y = y.clone().slice_dim(2, 0..moitie) * sigmoid(y.slice_dim(2, moitie..2 * moitie));
                        y.swap_dims(0, 1).reshape([trames as i32, -1])
                    })
                    .collect::<Vec<Tensor<B, 2>>>();
                Tensor::cat(parties, 1)
            })
            .collect::<Vec<_>>();
        Tensor::stack(par_stem, 0)
    }
}
