// SPDX-License-Identifier: GPL-3.0-or-later
//! Parité d'all-in-one avec la référence Python
//! (`experiments/structure/parite_ref.py`, mêmes stems) : spectrogramme,
//! logits du pli 0 et de l'ensemble, sections.
//!
//!   MOTEUR=f32 cargo run --release -p rusty-music-transcription --example sections_parite -- <dossier_reference> <dossier_plis>

#[macro_use]
mod commun;

use std::path::{Path, PathBuf};
use std::time::Instant;

use burn::tensor::backend::Backend;
use rodio::{Decoder, Source};
use rusty_music_transcription::sections::{self, Activations, Sections};

fn lire_f32(chemin: PathBuf) -> Vec<f32> {
    std::fs::read(&chemin)
        .unwrap_or_else(|e| panic!("{} : {e}", chemin.display()))
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

fn ecart(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "longueurs");
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

fn stereo(chemin: &Path) -> (Vec<f32>, Vec<f32>) {
    let dec = Decoder::try_from(std::fs::File::open(chemin).unwrap()).unwrap();
    assert_eq!(dec.channels().get(), 2);
    let ech: Vec<f32> = dec.collect();
    ech.chunks(2).map(|c| (c[0], c[1])).unzip()
}

fn comparer(nom: &str, a: &Activations, refdir: &Path) {
    let champs = [
        ("beat", &a.beat),
        ("downbeat", &a.downbeat),
        ("section", &a.section),
        ("function", &a.function),
    ];
    let e: Vec<String> = champs
        .iter()
        .map(|(c, v)| {
            format!(
                "{c} {:.2e}",
                ecart(v, &lire_f32(refdir.join(format!("{nom}-{c}.f32"))))
            )
        })
        .collect();
    println!("{nom:9} écart max des logits : {}", e.join(" · "));
}

fn essai<B: Backend>() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (refdir, plis) = (PathBuf::from(&a[0]), PathBuf::from(&a[1]));
    let stems: Vec<(Vec<f32>, Vec<f32>)> = ["bass", "drums", "other", "vocals"]
        .iter()
        .map(|s| stereo(&refdir.join("stems").join(format!("{s}.wav"))))
        .collect();
    let t0 = Instant::now();
    let (spec, trames) = sections::spectre(std::array::from_fn(|i| {
        (stems[i].0.as_slice(), stems[i].1.as_slice())
    }));
    let ref_spec = lire_f32(refdir.join("spec.f32"));
    println!(
        "spectre : {trames} trames ({:.2?}) — écart max {:.2e}",
        t0.elapsed(),
        ecart(&spec, &ref_spec)
    );

    let device = B::Device::default();
    let s = Sections::<B>::charger_dossier(&plis, &device).expect("plis");
    // Logits sur le spectre de référence : isole le réseau du spectrogramme.
    let t0 = Instant::now();
    let pli0 = s.activations(&ref_spec, trames, Some(1));
    println!("pli 0 : {:.2?}", t0.elapsed());
    comparer("pli0", &pli0, &refdir);
    let t0 = Instant::now();
    let ens = s.activations(&ref_spec, trames, None);
    println!("ensemble ({} plis) : {:.2?}", s.plis(), t0.elapsed());
    comparer("ensemble", &ens, &refdir);

    let nous = sections::segments(&ens);
    let r: serde_json::Value =
        serde_json::from_slice(&std::fs::read(refdir.join("reference.json")).unwrap()).unwrap();
    let eux = r["segments"].as_array().unwrap();
    println!("sections : {} (référence {})", nous.len(), eux.len());
    for (i, n) in nous.iter().enumerate() {
        let e = eux
            .get(i)
            .map(|e| {
                format!(
                    "{:7.2} {:7.2} {}",
                    e["debut"].as_f64().unwrap(),
                    e["fin"].as_f64().unwrap(),
                    e["etiquette"].as_str().unwrap()
                )
            })
            .unwrap_or_default();
        println!(
            "  {:7.2} {:7.2} {:8} | {e}",
            n.debut_s, n.fin_s, n.etiquette
        );
    }
}

fn main() {
    selon_moteur!(essai);
}
