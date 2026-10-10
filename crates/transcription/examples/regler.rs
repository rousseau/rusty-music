// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc de réglage de la transcription de basse : calcule les activations de
//! Basic Pitch une fois, puis écrit les mesures quantifiées pour une grille de
//! réglages — à noter contre une tablature de référence
//! (`experiments/transcription/regler.py`).
//!
//!   cargo run --release -p rusty-music-transcription --example regler -- <bass.wav> <pulsation.json> <dossier> <bp|doigte>

use std::path::PathBuf;

use rodio::source::UniformSourceIterator;
use rodio::{Decoder, Source};
use rusty_music_transcription::basic_pitch::{self, Reglages, Transcripteur};
use rusty_music_transcription::tablature::{self, Accordage, Couts};
use rusty_music_transcription::{monophonie, quantification};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (wav, puls, dossier, mode) = (PathBuf::from(&a[0]), PathBuf::from(&a[1]), PathBuf::from(&a[2]), a[3].as_str());
    std::fs::create_dir_all(&dossier)?;
    let dec = Decoder::try_from(std::fs::File::open(&wav)?)?;
    let (sr, canaux) = (dec.sample_rate().get(), dec.channels().get() as usize);
    let brut: Vec<f32> = UniformSourceIterator::new(dec, (canaux as u16).try_into()?, sr.try_into()?).collect();
    let mono: Vec<f32> = brut.chunks(canaux).map(|c| c.iter().sum::<f32>() / canaux as f32).collect();
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(puls)?)?;
    let f = |k: &str| -> Vec<f32> { v[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect() };
    let (temps, premiers) = (f("temps"), f("premiers_temps"));
    let mut t = Transcripteur::charger_installe()?;
    let act = t.activations(&mono, sr)?;

    let ecrire = |nom: String, r: &Reglages, k: &Couts| -> Result<(), Box<dyn std::error::Error>> {
        let mut notes = monophonie::monophonique(basic_pitch::notes(&act, r));
        tablature::poser_avec(&mut notes, &Accordage::basse4(), k);
        let m = quantification::quantifier(&notes, &temps, &premiers, 4);
        std::fs::write(dossier.join(format!("{nom}.json")), serde_json::to_string(&m)?)?;
        Ok(())
    };
    let mut n = 0;
    // Réglages retenus après le premier banc (10 oct.) : base des suivants.
    let base = Reglages { seuil_attaque: 0.6, seuil_trame: 0.3, duree_min: 8, ..Reglages::basse() };
    let doigte = Couts { corde: 0.6, deplacement: 1.0, pente_bas: 0.0, debut_haut: 12, pente_haut: 0.1 };
    if mode == "bp" {
        for &sa in &[0.3f32, 0.4, 0.5, 0.6, 0.7] {
            for &st in &[0.2f32, 0.3, 0.4] {
                for &d in &[6usize, 8, 11] {
                    let r = Reglages { seuil_attaque: sa, seuil_trame: st, duree_min: d, ..Reglages::basse() };
                    ecrire(format!("bp_{sa}_{st}_{d}"), &r, &Couts::default())?;
                    n += 1;
                }
            }
        }
    } else if mode == "harm" {
        for &h in &[0.0f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8] {
            let r = Reglages { harmoniques: (h > 0.0).then_some(h), ..base };
            ecrire(format!("hm_{h}"), &r, &doigte)?;
            n += 1;
        }
    } else {
        for &c in &[0.05f32, 0.1, 0.3, 0.6, 1.0] {
            for &pb in &[0.0f32, 0.03] {
                for &dh in &[5u8, 9, 12] {
                    for &ph in &[0.0f32, 0.1, 0.3] {
                        let k = Couts { corde: c, deplacement: 1.0, pente_bas: pb, debut_haut: dh, pente_haut: ph };
                        ecrire(format!("dg_{c}_{pb}_{dh}_{ph}"), &Reglages::basse(), &k)?;
                        n += 1;
                    }
                }
            }
        }
    }
    println!("{n} réglages écrits dans {}", dossier.display());
    Ok(())
}
