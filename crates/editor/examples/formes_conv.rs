// SPDX-License-Identifier: GPL-3.0-or-later
//! Les convolutions 1×1 des couches DConv de HTDemucs, aux formes du modèle,
//! contre une référence en f64 — pour isoler le noyau qui rend htdemucs faux
//! en WGSL (`experiments/burn-metal/README.md`).
//!
//! `couches_demucs` (avec la trace de `conv.rs`) a situé l'écart dans le
//! `conv2` de la première couche DConv du 2ᵉ encodeur temporel : 24 → 384
//! canaux sur 5 375 échantillons.
//!
//!   cargo run --release -p rusty-music-editor --example formes_conv

use burn::nn::conv::Conv1dConfig;
use burn::tensor::{Distribution, Tensor};
use rusty_music_editor::Moteur;

fn vers_vec<const D: usize>(t: Tensor<Moteur, D>) -> Vec<f32> {
    t.into_data().to_vec::<f32>().unwrap()
}

/// Contournement : la convolution 1×1 en produit matriciel, canaux
/// d'entrée complétés de zéros jusqu'au multiple de 32 supérieur.
fn conv1x1_complete(
    conv: &burn::nn::conv::Conv1d<Moteur>,
    x: Tensor<Moteur, 3>,
) -> Tensor<Moteur, 3> {
    let [lot, entree, long] = x.dims();
    let w = conv.weight.val(); // [sortie, entree, 1]
    let sortie = w.dims()[0];
    let ep = entree.next_multiple_of(32);
    let dev = x.device();
    let mut w = w.reshape([1, sortie, entree]);
    let mut x = x;
    if ep > entree {
        w = Tensor::cat(vec![w, Tensor::zeros([1, sortie, ep - entree], &dev)], 2);
        x = Tensor::cat(vec![x, Tensor::zeros([lot, ep - entree, long], &dev)], 1);
    }
    let y = w.expand([lot, sortie, ep]).matmul(x);
    match &conv.bias {
        Some(b) => y + b.val().reshape([1, sortie, 1]),
        None => y,
    }
}

fn essai(lot: usize, entree: usize, sortie: usize, long: usize) {
    let dev = Default::default();
    let conv = Conv1dConfig::new(entree, sortie, 1).init::<Moteur>(&dev);
    let x = Tensor::<Moteur, 3>::random([lot, entree, long], Distribution::Normal(0.0, 1.0), &dev);
    let w = vers_vec(conv.weight.val()); // [sortie, entree, 1]
    let b = conv
        .bias
        .as_ref()
        .map(|b| vers_vec(b.val()))
        .unwrap_or(vec![0.0; sortie]);
    let xv = vers_vec(x.clone());
    let y = if std::env::var_os("COMPLETER").is_some() {
        vers_vec(conv1x1_complete(&conv, x))
    } else {
        vers_vec(conv.forward(x))
    };
    let mut err = 0.0f64;
    let mut echelle = 0.0f64;
    for n in 0..lot {
        for o in 0..sortie {
            for t in 0..long {
                let mut s = b[o] as f64;
                for i in 0..entree {
                    s += w[o * entree + i] as f64 * xv[n * entree * long + i * long + t] as f64;
                }
                echelle = echelle.max(s.abs());
                err = err.max((y[n * sortie * long + o * long + t] as f64 - s).abs());
            }
        }
    }
    let e = err / echelle;
    println!(
        "lot {lot:>3} · {entree:>3} → {sortie:>3} · longueur {long:>6}   écart {e:.1e}{}",
        if e > 1e-4 { "   ← FAUX" } else { "" }
    );
}

fn main() {
    println!("backend : {}", rusty_music_editor::moteur());
    for (lot, entree, sortie, long) in [
        (1, 24, 384, 5375), // enc time 2, DConv conv2 : faux en WGSL
        (32, 24, 384, 336), // enc freq 2, même conv : juste
        (1, 6, 96, 85995),
        (1, 12, 192, 21499),
        (1, 48, 768, 1344),
        (1, 24, 384, 5376),
        (1, 24, 384, 4096),
        (1, 32, 384, 5375),
        (1, 192, 384, 5375), // rewrite de enc time 2
    ] {
        essai(lot, entree, sortie, long);
    }
}
