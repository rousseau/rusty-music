// SPDX-License-Identifier: GPL-3.0-or-later
//! Sections (all-in-one porté) de dossiers de stems HTDemucs
//! (`bass.wav`, `drums.wav`, `other.wav`, `vocals.wav`). Écrit
//! `<sortie>/<nom du dossier>.json` au format d'all-in-one
//! (`{"segments": [{"start", "end", "label"}]}`), relu par
//! `experiments/structure/allinone_ref.py` pour la notation.
//!
//! Toujours en f32 (`sections::Moteur`, celui de l'application) : en f16
//! les frontières changent.
//!
//!   cargo run --release -p rusty-music-transcription --example sections -- <dossier_plis> <sortie> <stems>...

use std::path::{Path, PathBuf};
use std::time::Instant;

use burn::tensor::backend::Backend;
use rodio::{Decoder, Source};
use rusty_music_transcription::sections::{Moteur, Sections};

fn stereo(chemin: &Path) -> Result<(Vec<f32>, Vec<f32>), Box<dyn std::error::Error>> {
    let dec = Decoder::try_from(std::fs::File::open(chemin)?)?;
    let canaux = dec.channels().get() as usize;
    let ech: Vec<f32> = dec.collect();
    Ok(ech.chunks(canaux).map(|c| (c[0], c[canaux - 1])).unzip())
}

fn essai<B: Backend>() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (plis, sortie) = (PathBuf::from(&a[0]), PathBuf::from(&a[1]));
    std::fs::create_dir_all(&sortie).unwrap();
    let s = Sections::<B>::charger_dossier(&plis, &B::Device::default()).expect("plis");
    for d in &a[2..] {
        let d = Path::new(d);
        let nom = d.file_name().unwrap().to_string_lossy().to_string();
        let dest = sortie.join(format!("{nom}.json"));
        if dest.exists() {
            continue;
        }
        let t0 = Instant::now();
        // `bass.wav` (banc) ou `<titre> — bass.wav` (application).
        let stem = |x: &str| -> PathBuf {
            std::fs::read_dir(d)
                .ok()
                .and_then(|it| {
                    it.flatten().map(|e| e.path()).find(|p| {
                        p.file_name().is_some_and(|n| {
                            let n = n.to_string_lossy();
                            !n.starts_with("._") && n.ends_with(&format!("{x}.wav"))
                        })
                    })
                })
                .unwrap_or_else(|| d.join(format!("{x}.wav")))
        };
        let stems: Result<Vec<_>, _> = ["bass", "drums", "other", "vocals"]
            .iter()
            .map(|x| stereo(&stem(x)))
            .collect();
        let Ok(stems) = stems else {
            eprintln!("{nom} : stems illisibles");
            continue;
        };
        let lecture = t0.elapsed();
        let (sections, a) = s.analyser(std::array::from_fn(|i| {
            (stems[i].0.as_slice(), stems[i].1.as_slice())
        }));
        let segments: Vec<serde_json::Value> = sections
            .iter()
            .map(|x| serde_json::json!({"start": x.debut_s, "end": x.fin_s, "label": x.etiquette}))
            .collect();
        std::fs::write(
            &dest,
            serde_json::to_vec(&serde_json::json!({ "segments": segments })).unwrap(),
        )
        .unwrap();
        println!(
            "{nom} : {} sections, {:.0} s de musique en {:.2?} (lecture des stems {lecture:.2?})",
            sections.len(),
            a.trames as f32 / 100.0,
            t0.elapsed()
        );
    }
}

fn main() {
    essai::<Moteur>();
}
