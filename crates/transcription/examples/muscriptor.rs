// SPDX-License-Identifier: GPL-3.0-or-later
//! Transcrit la basse d'un stem avec le portage Burn de MuScriptor et écrit
//! les notes brutes (format de `crate::Note`, JSON) — pour le banc
//! (`experiments/partitions/muscriptor_banc.py --rust`).
//!
//!   cargo run --release -p rusty-music-transcription --example muscriptor -- <bass.wav> <notes.json> [lot] [longueur_max]

use std::time::Instant;

use rodio::{Decoder, Source};
use rusty_music_transcription::muscriptor::{self, Muscriptor, BASSES};

#[cfg(feature = "gpu")]
type B = burn::backend::Wgpu;
#[cfg(not(feature = "gpu"))]
type B = burn::backend::NdArray;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let lot: usize = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(1);
    let dec = Decoder::try_from(std::fs::File::open(&a[0])?)?;
    let (sr, canaux) = (dec.sample_rate().get(), dec.channels().get() as usize);
    let brut: Vec<f32> = dec.collect();
    let mono: Vec<f32> = brut.chunks(canaux).map(|c| c.iter().sum::<f32>() / canaux as f32).collect();
    let mut m = Muscriptor::<B>::charger(&muscriptor::poids_hugging_face("medium").ok_or("poids MuScriptor introuvables")?, &Default::default())?;
    if let Some(l) = a.get(3).and_then(|x| x.parse().ok()) {
        m.longueur_max = l;
    }
    let t = Instant::now();
    let (notes, bilan) = m.transcrire(&mono, sr, BASSES, lot, |fait, total| eprint!("\r{fait}/{total}"))?;
    eprintln!();
    std::fs::write(&a[1], serde_json::to_string(&notes)?)?;
    println!(
        "{} notes, {} segments, {} jetons (max {} par segment) en {:.1} s ({:.2} ms/jeton), boucles {:?}",
        notes.len(),
        bilan.segments,
        bilan.jetons,
        bilan.jetons_max,
        t.elapsed().as_secs_f32(),
        t.elapsed().as_secs_f32() * 1000.0 / bilan.jetons.max(1) as f32,
        bilan.boucles
    );
    Ok(())
}
