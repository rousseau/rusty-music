// SPDX-License-Identifier: GPL-3.0-or-later
//! Le réseau d'all-in-one en Burn — port de `models/allinone.py` et
//! `models/dinat.py` (MIT), configuration `harmonix` des 8 plis publiés.
//!
//! Spectrogramme de chaque stem → trois convolutions → 11 blocs, chacun une
//! attention de voisinage dans le temps (deux dilatations, `2^i` et
//! `2^(i+1)`) puis une attention sur la grille instruments × temps → têtes
//! temps, premiers temps, frontières, fonctions.
//!
//! Les noms des champs reprennent les clés du `state_dict` d'origine ;
//! seul `self` (mot réservé) devient `soi` au chargement.
//!
//! Deux détails de l'original, gardés tels quels :
//! - quand la séquence est plus courte que la fenêtre (`k·d`, doublée pour
//!   la double attention), elle est complétée de zéros **après** la
//!   normalisation ; ces zéros servent de clés et de valeurs ;
//! - la grille des instruments (4 lignes) est toujours complétée d'une
//!   cinquième ligne nulle, la fenêtre faisant 5.

use burn::module::{Module, Param};
use burn::nn::conv::{Conv2d, Conv2dConfig};
use burn::nn::pool::{MaxPool2d, MaxPool2dConfig};
use burn::nn::{LayerNorm, LayerNormConfig, Linear, LinearConfig, PaddingConfig2d};
use burn::tensor::activation::{elu, gelu};
use burn::tensor::backend::Backend;
use burn::tensor::Tensor;

use super::voisinage::{attention, Indices};

pub const INSTRUMENTS: usize = 4;
pub const BANDES: usize = 81;
pub const DIM: usize = 24;
pub const TETES: usize = 2;
pub const NOYAU: usize = 5;
pub const PROFONDEUR: usize = 11;
pub const FONCTIONS: usize = 10;
const EPS: f64 = 1e-5;

/// Une couche linéaire nommée `dense` (`intermediate`, `output`…).
#[derive(Module, Debug)]
pub struct Dense<B: Backend> {
    pub dense: Linear<B>,
}

impl<B: Backend> Dense<B> {
    fn new(entree: usize, sortie: usize, device: &B::Device) -> Self {
        Self {
            dense: LinearConfig::new(entree, sortie).init(device),
        }
    }
}

/// Projections et biais relatif d'une attention ; `rpb` `[H, 2k−1]` en 1D,
/// `[H, 2k−1, 2k−1]` en 2D.
#[derive(Module, Debug)]
pub struct Attention<B: Backend, const R: usize> {
    pub query: Linear<B>,
    pub key: Linear<B>,
    pub value: Linear<B>,
    pub rpb: Param<Tensor<B, R>>,
}

#[derive(Module, Debug)]
pub struct ModuleAttention<B: Backend, const R: usize> {
    pub soi: Attention<B, R>,
    pub output: Dense<B>,
}

impl<B: Backend, const R: usize> ModuleAttention<B, R> {
    fn new(device: &B::Device) -> Self {
        let large = 2 * NOYAU - 1;
        let forme: [usize; R] = std::array::from_fn(|i| if i == 0 { TETES } else { large });
        let lin = || LinearConfig::new(DIM, DIM).init(device);
        Self {
            soi: Attention {
                query: lin(),
                key: lin(),
                value: lin(),
                rpb: Param::from_tensor(Tensor::zeros(forme, device)),
            },
            output: Dense::new(DIM, DIM, device),
        }
    }

    /// `h` `[B, X, Y, C]`, grille complète → `[B, X, Y, C]`.
    fn forward(
        &self,
        h: Tensor<B, 4>,
        dilatation: usize,
        indices: &mut Indices<B>,
    ) -> Tensor<B, 4> {
        let [b, x, y, c] = h.dims();
        let dh = c / TETES;
        let tetes = |t: Tensor<B, 4>| t.reshape([b, x * y, TETES, dh]).swap_dims(1, 2);
        let q = tetes(self.soi.query.forward(h.clone())) / (dh as f64).sqrt();
        let k = tetes(self.soi.key.forward(h.clone()));
        let v = tetes(self.soi.value.forward(h));
        let (idx, bidx) = indices.de(x, y, NOYAU, dilatation);
        let n = if x == 1 { NOYAU } else { NOYAU * NOYAU };
        let rpb = self.soi.rpb.val().reshape([TETES as i32, -1]);
        let ctx = attention(q, k, v, rpb, idx, bidx, n)
            .swap_dims(1, 2)
            .reshape([b, x, y, c]);
        self.output.dense.forward(ctx)
    }
}

/// Une couche DiNAT : attention(s), puis MLP, en résiduel.
#[derive(Module, Debug)]
pub struct Couche<B: Backend, const R: usize> {
    pub layernorm_before: LayerNorm<B>,
    pub layernorm_after: LayerNorm<B>,
    pub intermediate: Dense<B>,
    pub output: Dense<B>,
    pub attention: ModuleAttention<B, R>,
    /// Seconde attention, dilatation double (temps seulement).
    pub attention2: Option<ModuleAttention<B, R>>,
}

impl<B: Backend, const R: usize> Couche<B, R> {
    fn new(double: bool, device: &B::Device) -> Self {
        let apres = if double { 2 * DIM } else { DIM };
        Self {
            layernorm_before: LayerNormConfig::new(DIM).with_epsilon(EPS).init(device),
            layernorm_after: LayerNormConfig::new(apres).with_epsilon(EPS).init(device),
            intermediate: Dense::new(apres, 4 * apres, device),
            output: Dense::new(4 * apres, DIM, device),
            attention: ModuleAttention::new(device),
            attention2: double.then(|| ModuleAttention::new(device)),
        }
    }

    /// `x` `[B, X, Y, C]` ; la grille est complétée de zéros jusqu'à
    /// `x_min × y_min` après la normalisation, puis recoupée.
    fn forward(
        &self,
        x: Tensor<B, 4>,
        dilatation: usize,
        x_min: usize,
        y_min: usize,
        indices: &mut Indices<B>,
    ) -> Tensor<B, 4> {
        let [b, gx, gy, c] = x.dims();
        let device = x.device();
        let mut h = self.layernorm_before.forward(x.clone());
        if gy < y_min {
            h = Tensor::cat(vec![h, Tensor::zeros([b, gx, y_min - gy, c], &device)], 2);
        }
        if gx < x_min {
            let y = h.dims()[2];
            h = Tensor::cat(vec![h, Tensor::zeros([b, x_min - gx, y, c], &device)], 1);
        }
        let recouper = |t: Tensor<B, 4>| t.slice([0..b, 0..gx, 0..gy, 0..c]);
        let h1 = x.clone() + recouper(self.attention.forward(h.clone(), dilatation, indices));
        let (milieu, raccourci) = match &self.attention2 {
            Some(a2) => {
                let h2 = x + recouper(a2.forward(h, 2 * dilatation, indices));
                (
                    Tensor::cat(vec![h1.clone(), h2.clone()], 3),
                    (h1 + h2) / 2.0,
                )
            }
            None => (h1.clone(), h1),
        };
        let m = self.output.dense.forward(gelu(
            self.intermediate
                .dense
                .forward(self.layernorm_after.forward(milieu)),
        ));
        raccourci + m
    }
}

#[derive(Module, Debug)]
pub struct Bloc<B: Backend> {
    pub timelayer: Couche<B, 2>,
    pub instlayer: Couche<B, 3>,
}

#[derive(Module, Debug)]
pub struct Encodeur<B: Backend> {
    pub layers: Vec<Bloc<B>>,
}

#[derive(Module, Debug)]
pub struct Plongement<B: Backend> {
    pub conv0: Conv2d<B>,
    pub pool0: MaxPool2d,
    pub conv1: Conv2d<B>,
    pub pool1: MaxPool2d,
    pub conv2: Conv2d<B>,
    pub pool2: MaxPool2d,
    pub norm: LayerNorm<B>,
}

#[derive(Module, Debug)]
pub struct Tete<B: Backend> {
    pub classifier: Linear<B>,
}

impl<B: Backend> Tete<B> {
    fn new(classes: usize, device: &B::Device) -> Self {
        Self {
            classifier: LinearConfig::new(INSTRUMENTS * DIM, classes).init(device),
        }
    }

    /// `h` `[1, K, T, C]` → `[T, classes]`.
    fn forward(&self, h: Tensor<B, 4>) -> Tensor<B, 2> {
        let [_, k, t, c] = h.dims();
        self.classifier
            .forward(h.swap_dims(1, 2).reshape([t, k * c]))
    }
}

/// Un pli d'all-in-one.
#[derive(Module, Debug)]
pub struct AllInOne<B: Backend> {
    pub embeddings: Plongement<B>,
    pub encoder: Encodeur<B>,
    pub norm: LayerNorm<B>,
    pub beat_classifier: Tete<B>,
    pub downbeat_classifier: Tete<B>,
    pub section_classifier: Tete<B>,
    pub function_classifier: Tete<B>,
}

/// Logits d'un pli : `[T, 1]` ×3 et `[T, 10]`.
pub struct Logits<B: Backend> {
    pub beat: Tensor<B, 2>,
    pub downbeat: Tensor<B, 2>,
    pub section: Tensor<B, 2>,
    pub function: Tensor<B, 2>,
}

impl<B: Backend> AllInOne<B> {
    pub fn new(device: &B::Device) -> Self {
        let pool = || MaxPool2dConfig::new([1, 3]).with_strides([1, 3]).init();
        let conv = |e, s, noyau: [usize; 2], pad: usize| {
            Conv2dConfig::new([e, s], noyau)
                .with_padding(PaddingConfig2d::Explicit(pad, 0, pad, 0))
                .init(device)
        };
        Self {
            embeddings: Plongement {
                conv0: conv(1, DIM / 2, [3, 3], 1),
                pool0: pool(),
                conv1: conv(DIM / 2, DIM, [1, 12], 0),
                pool1: pool(),
                conv2: conv(DIM, DIM, [3, 3], 1),
                pool2: pool(),
                norm: LayerNormConfig::new(DIM).init(device),
            },
            encoder: Encodeur {
                layers: (0..PROFONDEUR)
                    .map(|_| Bloc {
                        timelayer: Couche::new(true, device),
                        instlayer: Couche::new(false, device),
                    })
                    .collect(),
            },
            norm: LayerNormConfig::new(DIM).with_epsilon(EPS).init(device),
            beat_classifier: Tete::new(1, device),
            downbeat_classifier: Tete::new(1, device),
            section_classifier: Tete::new(1, device),
            function_classifier: Tete::new(FONCTIONS, device),
        }
    }

    /// `spec` `[K = 4, T, 81]` (bass, drums, other, vocals).
    pub fn forward(&self, spec: Tensor<B, 3>, indices: &mut Indices<B>) -> Logits<B> {
        let [k, t, f] = spec.dims();
        let e = &self.embeddings;
        let x = spec.reshape([k, 1, t, f]);
        let x = elu(e.pool0.forward(e.conv0.forward(x)), 1.0);
        let x = elu(e.pool1.forward(e.conv1.forward(x)), 1.0);
        let x = elu(e.pool2.forward(e.conv2.forward(x)), 1.0); // [K, C, T, 1]
        let mut h = e.norm.forward(x.reshape([k, DIM, t]).swap_dims(1, 2)); // [K, T, C]
        for (i, bloc) in self.encoder.layers.iter().enumerate() {
            let d = 1usize << i;
            // Temps : chaque stem est une séquence (grille 1 × T).
            let ht =
                bloc.timelayer
                    .forward(h.reshape([k, 1, t, DIM]), d, 1, NOYAU * d * 2, indices);
            // Instruments × temps : une grille K × T.
            let hi = bloc
                .instlayer
                .forward(ht.reshape([1, k, t, DIM]), 1, NOYAU, NOYAU, indices);
            h = hi.reshape([k, t, DIM]);
        }
        let h = self.norm.forward(h.reshape([1, k, t, DIM]));
        Logits {
            beat: self.beat_classifier.forward(h.clone()),
            downbeat: self.downbeat_classifier.forward(h.clone()),
            section: self.section_classifier.forward(h.clone()),
            function: self.function_classifier.forward(h),
        }
    }
}
