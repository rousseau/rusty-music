// SPDX-License-Identifier: GPL-3.0-or-later
//! Les opérations de l'attention dans le temps de BS-RoFormer, une à une,
//! aux formes d'un segment de SW (62 séquences de 1 151 trames, 8 têtes de 64).
//!
//!   cargo run --release -p rusty-music-editor --example vitesse_attention

use std::time::Instant;

use burn::tensor::activation::softmax;
use burn::tensor::backend::Backend;
use burn::tensor::{Distribution, Tensor};
use rusty_music_editor::Moteur;

fn main() {
    let device = Default::default();
    let synchro = || Moteur::sync(&device).unwrap();
    let (lot, tetes, n, d) = (62, 8, 1151, 64);
    let alea = |forme: [usize; 4]| Tensor::<Moteur, 4>::random(forme, Distribution::Normal(0.0, 1.0), &device);
    let (q, k, v) = (alea([lot, tetes, n, d]), alea([lot, tetes, n, d]), alea([lot, tetes, n, d]));
    let x = Tensor::<Moteur, 2>::random([lot * n, 256], Distribution::Normal(0.0, 1.0), &device);
    let w = Tensor::<Moteur, 2>::random([256, 1536], Distribution::Normal(0.0, 1.0), &device);
    for passe in 0..3 {
        println!("passe {passe}");
        let mesurer = |nom: &str, f: &dyn Fn()| {
            synchro();
            let t = Instant::now();
            f();
            synchro();
            println!("  {nom:34} {:7.1} ms", t.elapsed().as_secs_f64() * 1e3);
        };
        mesurer("projection qkv [71 362 × 256 × 1536]", &|| drop(x.clone().matmul(w.clone())));
        for tranche in [8, 62] {
            let (q, k, v) = (q.clone().slice_dim(0, 0..tranche), k.clone().slice_dim(0, 0..tranche), v.clone().slice_dim(0, 0..tranche));
            let facteur = lot as f64 / tranche as f64;
            println!("  tranche de {tranche} (× {facteur:.2} pour le segment)");
            mesurer("    q·kᵀ", &|| drop(q.clone().matmul(k.clone().swap_dims(2, 3))));
            let s = q.clone().matmul(k.clone().swap_dims(2, 3));
            mesurer("    softmax", &|| drop(softmax(s.clone() * 0.125, 3)));
            let p = softmax(s.clone(), 3);
            mesurer("    p·v", &|| drop(p.clone().matmul(v.clone())));
            mesurer("    attention fusionnée de Burn", &|| drop(burn::tensor::module::attention(q.clone(), k.clone(), v.clone(), None, None, Default::default())));
        }
    }
}
