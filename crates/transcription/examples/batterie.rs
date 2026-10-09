// SPDX-License-Identifier: GPL-3.0-or-later
//! Transcrit un stem de batterie (ADTOF) et, avec le `pulsation.json` du
//! morceau, écrit les mesures quantifiées.
//!
//!   cargo run --release -p rusty-music-transcription --example batterie -- <drums.wav> [pulsation.json mesures.json]

use std::path::PathBuf;
use std::time::Instant;

use rodio::{Decoder, Source};
use rusty_music_transcription::batterie::{self, Batteur, Piece};
use rusty_music_transcription::quantification;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dec = Decoder::try_from(std::fs::File::open(PathBuf::from(&a[0]))?)?;
    let canaux = dec.channels().get() as usize;
    let ech: Vec<f32> = dec.collect();
    let (g, d): (Vec<f32>, Vec<f32>) = ech.chunks(canaux).map(|c| (c[0], c[canaux - 1])).unzip();
    let t0 = Instant::now();
    let (carac, trames, bandes) = batterie::caracteristiques(&g, &d);
    let t_c = t0.elapsed();
    let mut b = Batteur::charger_installe()?;
    let act = b.activations(&carac, trames, bandes)?;
    let coups = batterie::coups(&act, trames);
    println!("{:.0} s : caractéristiques {} ms, total {} ms, {} coups", trames as f32 / 100.0, t_c.as_millis(), t0.elapsed().as_millis(), coups.len());
    for p in [Piece::GrosseCaisse, Piece::CaisseClaire, Piece::Toms, Piece::Charleston, Piece::Cymbales] {
        println!("  {p:?} : {}", coups.iter().filter(|c| c.piece == p).count());
    }
    if a.len() >= 3 {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[1])?)?;
        let f = |k: &str| -> Vec<f32> { v[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect() };
        let m = quantification::quantifier_coups(&coups, &f("temps"), &f("premiers_temps"), 4);
        std::fs::write(&a[2], serde_json::to_string(&m)?)?;
        println!("{} mesures → {}", m.len(), a[2]);
    }
    Ok(())
}
