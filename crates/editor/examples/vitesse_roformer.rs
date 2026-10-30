// SPDX-License-Identifier: GPL-3.0-or-later
//! Où passe le temps de BS-RoFormer : chaque étage d'un segment, mesuré
//! après synchronisation du backend.
//!
//!   cargo run --release -p rusty-music-editor --example vitesse_roformer -- <BS-Rofo-SW-Fixed.ckpt> [répétitions]

use std::time::Instant;

use burn::tensor::backend::Backend;
use burn::tensor::{Distribution, Tensor};
use rusty_music_editor::roformer::{Config, Roformer};
use rusty_music_editor::Moteur;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let repetitions: usize = a.get(1).map_or(2, |s| s.parse().unwrap());
    let mut config = Config::sw();
    if let Ok(t) = std::env::var("TRANCHE") {
        config.tranche_temps = t.parse().unwrap();
    }
    let device = Default::default();
    let m = Roformer::<Moteur>::charger(a[0].as_ref(), config.clone(), &device).expect("chargement");
    let r = m.reseau();
    let trames = config.segment / config.saut + 1;
    let bandes = config.bandes.len();
    let largeur = 4 * config.bandes.iter().sum::<usize>();
    let synchro = || Moteur::sync(&device).unwrap();
    let chrono = |nom: &str, f: &dyn Fn() -> Tensor<Moteur, 3>| {
        synchro();
        let t = Instant::now();
        let y = f();
        synchro();
        println!("  {nom:28} {:7.1} ms", t.elapsed().as_secs_f64() * 1e3);
        y
    };
    for rep in 0..repetitions {
        println!("passe {rep}");
        let x = Tensor::<Moteur, 2>::random([trames, largeur], Distribution::Normal(0.0, 1.0), &device);
        let y = chrono("découpe", &|| r.decouper(x.clone(), &config.bandes));
        let b = &r.layers[0];
        let yt = y.clone().swap_dims(0, 1);
        let temps = &b.temps.layers[0];
        let frequence = &b.frequence.layers[0];
        chrono(&format!("temps : attention ({bandes} × {trames})"), &|| temps.attn.forward(yt.clone(), config.tetes, config.tranche_temps));
        chrono("temps : anticipation", &|| temps.ff.forward(yt.clone()));
        chrono(&format!("fréq. : attention ({trames} × {bandes})"), &|| frequence.attn.forward(y.clone(), config.tetes, config.tranche_frequence));
        chrono("fréq. : anticipation", &|| frequence.ff.forward(y.clone()));
        chrono("un bloc entier", &|| r.bloc(0, y.clone(), &config));
        chrono("masques", &|| m.tetes(y.clone()));
    }
}
