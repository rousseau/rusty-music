// SPDX-License-Identifier: GPL-3.0-or-later
//! Parité de la batterie (ADTOF) avec la référence Python
//! (`experiments/batterie/exporter_adtof.py`, même extrait audio) :
//! caractéristiques madmom, sorties du réseau, coups détectés.
//!
//!   cargo run --release -p rusty-music-transcription --example batterie_parite -- <essai.wav> <dossier_reference> <adtof.onnx>

use std::path::PathBuf;

use rodio::{Decoder, Source};
use rusty_music_transcription::batterie::{self, Batteur, CLASSES};

fn lire_f32(chemin: PathBuf) -> Vec<f32> {
    std::fs::read(chemin).unwrap().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (wav, refdir, onnx) = (PathBuf::from(&a[0]), PathBuf::from(&a[1]), PathBuf::from(&a[2]));
    let dec = Decoder::try_from(std::fs::File::open(&wav)?)?;
    assert_eq!(dec.channels().get(), 2);
    let ech: Vec<f32> = dec.collect();
    let (g, d): (Vec<f32>, Vec<f32>) = ech.chunks(2).map(|c| (c[0], c[1])).unzip();

    let (carac, trames, bandes) = batterie::caracteristiques(&g, &d);
    let ref_c = lire_f32(refdir.join("caracteristiques.f32"));
    let ecart_c = carac.iter().zip(&ref_c).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
    println!("caractéristiques : {trames} trames × {bandes} bandes (référence {}) — écart max {ecart_c:.2e}", ref_c.len() / bandes);

    let mut b = Batteur::charger(&onnx)?;
    let act = b.activations(&carac, trames, bandes)?;
    let ref_p = lire_f32(refdir.join("predictions.f32"));
    let ecart_p = act.iter().zip(&ref_p).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
    println!("prédictions : écart max {ecart_p:.2e}");

    let coups = batterie::coups(&act, trames);
    let r: serde_json::Value = serde_json::from_slice(&std::fs::read(refdir.join("reference.json"))?)?;
    for (k, nom) in ["BD", "SD", "TT", "HH", "CY+RD"].iter().enumerate() {
        let nous: Vec<f32> = coups.iter().filter(|c| c.piece == CLASSES[k]).map(|c| c.instant_s).collect();
        let eux: Vec<f32> = r["coups"][nom].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect();
        let communs = nous.iter().filter(|t| eux.iter().any(|u| (*t - u).abs() < 1e-3)).count();
        println!("  {nom:6} nous {:3}  référence {:3}  communs {communs}", nous.len(), eux.len());
    }
    Ok(())
}
