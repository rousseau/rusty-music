// SPDX-License-Identifier: GPL-3.0-or-later
//! Le produit `x · wᵀ` des couches de MuScriptor selon le nombre de lignes
//! de `x` : produit de Burn (gemv à une ligne, tuilé au-delà) contre le
//! produit mince (`muscriptor::mince`), écart et temps — minimum sur dix
//! tours, la machine pouvant être partagée. Backend choisi par `MOTEUR`.
//!
//!   MOTEUR=f16 cargo run --release -p rusty-music-transcription --example micro_matmul

#[macro_use]
mod commun;

use std::time::Instant;

use burn::tensor::{Distribution, Tensor, TensorPrimitive};
use rusty_music_transcription::muscriptor::ProduitMince;

fn mince<B: ProduitMince>(x: Tensor<B, 2>, w: Tensor<B, 2>) -> Tensor<B, 2> {
    Tensor::from_primitive(TensorPrimitive::Float(B::produit_mince(
        x.into_primitive().tensor(),
        w.into_primitive().tensor(),
    )))
}

fn executer<B: ProduitMince>() -> Result<(), Box<dyn std::error::Error>> {
    let dev = Default::default();
    let n = 50;
    // qkv, sortie de l'attention, linear1, linear2, logits.
    let formes = [
        (1024usize, 3072usize),
        (1024, 1024),
        (1024, 4096),
        (4096, 1024),
        (1024, 1395),
    ];
    let poids: Vec<Tensor<B, 2>> = formes
        .iter()
        .map(|&(e, s)| Tensor::random([s, e], Distribution::Normal(0.0, 0.02), &dev))
        .collect();
    // (lignes, produit mince ?)
    let essais = [
        (1usize, false),
        (16, false),
        (16, true),
        (8, true),
        (2, true),
    ];
    let mut meilleur = vec![f32::MAX; formes.len() * essais.len()];
    for (i, &(entree, _)) in formes.iter().enumerate() {
        let x = Tensor::<B, 2>::random([16, entree], Distribution::Normal(0.0, 1.0), &dev);
        let a: Vec<f32> = x
            .clone()
            .matmul(poids[i].clone().transpose())
            .into_data()
            .convert::<f32>()
            .into_vec()?;
        let b: Vec<f32> = mince(x, poids[i].clone())
            .into_data()
            .convert::<f32>()
            .into_vec()?;
        let ecart = a
            .iter()
            .zip(&b)
            .map(|(p, q)| (p - q).abs())
            .fold(0.0, f32::max);
        let echelle = a.iter().map(|p| p.abs()).fold(0.0, f32::max);
        println!(
            "{entree}→{} : écart max mince/Burn {ecart:.2e} (échelle {echelle:.2})",
            formes[i].1
        );
    }
    for _tour in 0..10 {
        for (i, &(entree, _)) in formes.iter().enumerate() {
            for (k, &(m, avec_mince)) in essais.iter().enumerate() {
                let x = Tensor::<B, 2>::random([m, entree], Distribution::Normal(0.0, 1.0), &dev);
                let w = &poids[i];
                let f = |x: Tensor<B, 2>| {
                    if avec_mince {
                        mince(x, w.clone())
                    } else {
                        x.matmul(w.clone().transpose())
                    }
                };
                let _ = f(x.clone()).into_data();
                let t = Instant::now();
                let mut acc = f(x.clone());
                for _ in 1..n {
                    acc = acc + f(x.clone());
                }
                let _ = acc.into_data();
                let c = &mut meilleur[i * essais.len() + k];
                *c = c.min(t.elapsed().as_secs_f32() * 1000.0 / n as f32);
            }
        }
    }
    for (i, &(e, s)) in formes.iter().enumerate() {
        let ligne: Vec<String> = essais
            .iter()
            .enumerate()
            .map(|(k, &(m, avec))| {
                format!(
                    "{} M={m} {:.3}",
                    if avec { "mince" } else { "Burn" },
                    meilleur[i * essais.len() + k]
                )
            })
            .collect();
        println!("{e}→{s} (ms) : {}", ligne.join(" | "));
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    selon_moteur!(executer)
}
