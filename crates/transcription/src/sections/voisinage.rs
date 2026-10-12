// SPDX-License-Identifier: GPL-3.0-or-later
//! Attention de voisinage dilatée, à la NATTEN 0.14, par gather + softmax :
//! pas de noyau dédié. Référence : `experiments/structure/natten_pur`, qui
//! fait tourner all-in-one en Python à l'identique.
//!
//! Avec une dilatation `d`, la séquence se lit comme `d` sous-suites
//! entrelacées (`i ≡ r mod d`). Dans sa sous-suite, chaque position voit `k`
//! voisins consécutifs : fenêtre centrée, puis repoussée contre les bords,
//! jamais tronquée. Le biais de position relative vaut
//! `rpb[(j − i) / d + k − 1]`.

use std::collections::HashMap;

use burn::tensor::activation::softmax;
use burn::tensor::backend::Backend;
use burn::tensor::{Int, Tensor, TensorData};

/// Voisins `[longueur × k]`, ligne par ligne, et indice du biais relatif de
/// chacun (de 0 à 2k − 2).
pub fn voisins(longueur: usize, k: usize, d: usize) -> (Vec<i64>, Vec<i64>) {
    let (mut j, mut b) = (Vec::with_capacity(longueur * k), Vec::with_capacity(longueur * k));
    for i in 0..longueur {
        let (r, u) = (i % d, i / d);
        let long_r = (longueur - r).div_ceil(d) as i64;
        let debut = (u as i64 - (k / 2) as i64).max(0).min((long_r - k as i64).max(0));
        for ki in 0..k {
            let jj = (debut + ki as i64) * d as i64 + r as i64;
            j.push(jj);
            b.push((jj - i as i64) / d as i64 + k as i64 - 1);
        }
    }
    (j, b)
}

/// La même chose sur une grille `x × y` aplatie ligne par ligne : `k²`
/// voisins par case, dans l'ordre `(ki, kj) → ki·k + kj` ; biais aplati
/// `bx·(2k − 1) + by`.
pub fn voisins_2d(x: usize, y: usize, k: usize, d: usize) -> (Vec<i64>, Vec<i64>) {
    let (jx, bx) = voisins(x, k, d);
    let (jy, by) = voisins(y, k, d);
    let large = (2 * k - 1) as i64;
    let (mut j, mut b) = (Vec::with_capacity(x * y * k * k), Vec::with_capacity(x * y * k * k));
    for a in 0..x {
        for c in 0..y {
            for ki in 0..k {
                for kj in 0..k {
                    j.push(jx[a * k + ki] * y as i64 + jy[c * k + kj]);
                    b.push(bx[a * k + ki] * large + by[c * k + kj]);
                }
            }
        }
    }
    (j, b)
}

/// Indices calculés une fois par forme, partagés par les couches et les
/// plis d'une même analyse.
pub struct Indices<B: Backend> {
    device: B::Device,
    cache: HashMap<(usize, usize, usize, usize), (Tensor<B, 1, Int>, Tensor<B, 1, Int>)>,
}

impl<B: Backend> Indices<B> {
    pub fn new(device: &B::Device) -> Self {
        Self { device: device.clone(), cache: HashMap::new() }
    }

    /// `(voisins, biais)` d'une grille `x × y` (`x = 1` en 1D).
    pub fn de(&mut self, x: usize, y: usize, k: usize, d: usize) -> (Tensor<B, 1, Int>, Tensor<B, 1, Int>) {
        let device = &self.device;
        self.cache
            .entry((x, y, k, d))
            .or_insert_with(|| {
                let (j, b) = if x == 1 { voisins(y, k, d) } else { voisins_2d(x, y, k, d) };
                let n = j.len();
                (Tensor::from_data(TensorData::new(j, [n]), device), Tensor::from_data(TensorData::new(b, [n]), device))
            })
            .clone()
    }
}

/// Éléments au plus par tenseur intermédiaire `[B, H, l, n, D]` : on
/// découpe les requêtes en tranches pour borner la mémoire (la grille des
/// instruments d'un morceau de 7 min compte 210 000 cases × 25 voisins).
const ELEMENTS_MAX: usize = 1 << 25;

/// `q`, `k`, `v` `[B, H, L, D]` (`q` déjà mis à l'échelle), `rpb` `[H, nb]`
/// aplati, `idx` et `bidx` `[L·n]` → contexte `[B, H, L, D]`.
pub fn attention<B: Backend>(
    q: Tensor<B, 4>,
    k: Tensor<B, 4>,
    v: Tensor<B, 4>,
    rpb: Tensor<B, 2>,
    idx: Tensor<B, 1, Int>,
    bidx: Tensor<B, 1, Int>,
    n: usize,
) -> Tensor<B, 4> {
    let [b, h, l, d] = q.dims();
    let tranche = (ELEMENTS_MAX / (b * h * n * d)).max(1);
    let mut sorties = Vec::with_capacity(l.div_ceil(tranche));
    let mut a = 0;
    while a < l {
        let z = (a + tranche).min(l);
        let m = z - a;
        let ix = idx.clone().slice([a * n..z * n]);
        let kv = k.clone().select(2, ix.clone()).reshape([b, h, m, n, d]);
        let qs = q.clone().slice([0..b, 0..h, a..z, 0..d]);
        let scores = (qs.unsqueeze_dim::<5>(3) * kv).sum_dim(4).reshape([b, h, m, n]);
        let biais = rpb.clone().select(1, bidx.clone().slice([a * n..z * n])).reshape([1, h, m, n]);
        let p = softmax(scores + biais, 3);
        let vv = v.clone().select(2, ix).reshape([b, h, m, n, d]);
        sorties.push((p.unsqueeze_dim::<5>(4) * vv).sum_dim(3).reshape([b, h, m, d]));
        a = z;
    }
    Tensor::cat(sorties, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenetre_centree_puis_repoussee_contre_les_bords() {
        let (j, b) = voisins(8, 3, 1);
        assert_eq!(&j[0..3], &[0, 1, 2]); // bord gauche : repoussée
        assert_eq!(&j[9..12], &[2, 3, 4]); // centrée sur 3
        assert_eq!(&j[21..24], &[5, 6, 7]); // bord droit
        assert_eq!(&b[0..3], &[2, 3, 4]); // j − i = 0, 1, 2
        assert_eq!(&b[9..12], &[1, 2, 3]);
    }

    #[test]
    fn dilatation_par_sous_suites() {
        // d = 2 : les positions paires ne voient que des paires.
        let (j, b) = voisins(10, 3, 2);
        assert_eq!(&j[4 * 3..5 * 3], &[2, 4, 6]);
        assert_eq!(&j[5 * 3..6 * 3], &[3, 5, 7]);
        assert_eq!(&j[9 * 3..10 * 3], &[5, 7, 9]);
        assert_eq!(&b[9 * 3..10 * 3], &[0, 1, 2]);
    }

    #[test]
    fn grille_deux_dimensions() {
        let (j, b) = voisins_2d(5, 6, 5, 1);
        // Case (0, 0) : toutes les lignes, colonnes 0 à 4.
        assert_eq!(&j[0..5], &[0, 1, 2, 3, 4]);
        assert_eq!(j[5], 6);
        assert_eq!(b[0], 4 * 9 + 4);
        assert_eq!(j.len(), 5 * 6 * 25);
    }
}
