// SPDX-License-Identifier: GPL-3.0-or-later
//! Transcrit la basse d'un stem avec le portage Burn de MuScriptor et écrit
//! les notes brutes (format de `crate::Note`, JSON) — pour le banc
//! (`experiments/partitions/muscriptor_banc.py --rust`). Avec une liste
//! d'instruments (`voice,acoustic_piano`… ou `tous` pour aucune restriction),
//! les notes gardent leur programme (`NoteBrute`) — banc multi-instrument
//! (`experiments/partitions/banc_pvg.py`).
//!
//! `MUSCRIPTOR_POIDS` désigne un autre `model.safetensors` (taille large :
//! son `config.json` voisin donne l'architecture).
//!
//!   [MOTEUR=f32] [MUSCRIPTOR_POIDS=…] cargo run --release -p rusty-music-transcription --example muscriptor -- <audio> <notes.json> [lot] [longueur_max] [instruments]

#[macro_use]
mod commun;

use std::time::Instant;

use rodio::{Decoder, Source};
use rusty_music_transcription::muscriptor::{self, ProduitMince, Muscriptor, BASSES};

fn executer<B: ProduitMince>() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let lot: usize = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(1);
    let dec = Decoder::try_from(std::fs::File::open(&a[0])?)?;
    let (sr, canaux) = (dec.sample_rate().get(), dec.channels().get() as usize);
    let brut: Vec<f32> = dec.collect();
    let mono: Vec<f32> = brut.chunks(canaux).map(|c| c.iter().sum::<f32>() / canaux as f32).collect();
    let poids = std::env::var_os("MUSCRIPTOR_POIDS").map(std::path::PathBuf::from).or_else(|| muscriptor::poids_hugging_face("medium"));
    let mut m = Muscriptor::<B>::charger(&poids.ok_or("poids MuScriptor introuvables")?, &Default::default())?;
    if let Some(l) = a.get(3).and_then(|x| x.parse().ok()) {
        m.longueur_max = l;
    }
    let t = Instant::now();
    let progres = |fait, total| eprint!("\r{fait}/{total}");
    let (n, bilan) = match a.get(4).map(String::as_str) {
        None => {
            let (notes, bilan) = m.transcrire(&mono, sr, BASSES, lot, progres)?;
            std::fs::write(&a[1], serde_json::to_string(&notes)?)?;
            (notes.len(), bilan)
        }
        Some(liste) => {
            let instruments: Vec<&str> = if liste == "tous" { vec![] } else { liste.split(',').collect() };
            let (notes, bilan) = m.transcrire_brut(&mono, sr, &instruments, lot, progres)?;
            std::fs::write(&a[1], serde_json::to_string(&notes)?)?;
            (notes.len(), bilan)
        }
    };
    eprintln!();
    println!(
        "{} notes, {} segments, {} jetons (max {} par segment) en {:.1} s ({:.2} ms/jeton), boucles {:?}",
        n,
        bilan.segments,
        bilan.jetons,
        bilan.jetons_max,
        t.elapsed().as_secs_f32(),
        t.elapsed().as_secs_f32() * 1000.0 / bilan.jetons.max(1) as f32,
        bilan.boucles
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    selon_moteur!(executer)
}
