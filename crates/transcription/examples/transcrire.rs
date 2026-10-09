// SPDX-License-Identifier: GPL-3.0-or-later
//! Transcrit un fichier (un stem de basse, typiquement) et affiche les notes
//! posées en tablature. Avec un dossier en second argument, y vide l'audio à
//! 22 050 Hz, les activations et les notes brutes — de quoi comparer au code
//! Python d'origine (`experiments/transcription/parite_basic_pitch.py`).
//!
//! Avec un troisième argument, le `pulsation.json` du morceau (cache de
//! l'éditeur) : y écrit aussi `mesures.json`, la partition quantifiée.
//!
//!   cargo run --release -p rusty-music-transcription --example transcrire -- <fichier> [dossier] [pulsation.json]

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use rodio::source::UniformSourceIterator;
use rodio::{Decoder, Source};
use rusty_music_transcription::basic_pitch::{self, Reglages, Transcripteur};
use rusty_music_transcription::{monophonie, quantification, tablature};

fn ecrire_f32(chemin: PathBuf, v: &[f32]) -> std::io::Result<()> {
    let mut f = std::fs::File::create(chemin)?;
    for x in v {
        f.write_all(&x.to_le_bytes())?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let fichier = PathBuf::from(args.next().ok_or("usage : transcrire <fichier> [dossier]")?);
    let vidage = args.next().map(PathBuf::from);
    let pulsation = args.next().map(PathBuf::from);

    let dec = Decoder::try_from(std::fs::File::open(&fichier)?)?;
    let sr = dec.sample_rate().get();
    let canaux = dec.channels().get() as usize;
    let brut: Vec<f32> = UniformSourceIterator::new(dec, dec_canaux(canaux), dec_sr(sr)).collect();
    let mono: Vec<f32> = brut.chunks(canaux).map(|c| c.iter().sum::<f32>() / canaux as f32).collect();
    println!("{} : {:.1} s à {sr} Hz", fichier.display(), mono.len() as f32 / sr as f32);

    basic_pitch::POIDS.telecharger(|_, _| {})?;
    let mut t = Transcripteur::charger_installe()?;
    let t0 = Instant::now();
    let a = t.activations(&mono, sr)?;
    let t_reseau = t0.elapsed();
    let brutes = basic_pitch::notes(&a, &Reglages::basse());
    let t_notes = t0.elapsed() - t_reseau;
    let mut notes = monophonie::monophonique(brutes.clone());
    tablature::poser(&mut notes, &tablature::Accordage::basse4());
    println!(
        "{} trames · réseau {} ms · notes {} ms · {} notes brutes → {} monophoniques",
        a.trames,
        t_reseau.as_millis(),
        t_notes.as_millis(),
        brutes.len(),
        notes.len()
    );
    for n in notes.iter().take(24) {
        println!(
            "  {:7.3}–{:7.3} s  midi {:3}  corde {}  frette {:2}  a={:.2}",
            n.debut_s,
            n.fin_s,
            n.hauteur,
            n.corde.map_or("-".into(), |c| c.to_string()),
            n.frette.unwrap_or(99),
            n.amplitude
        );
    }

    if let Some(d) = vidage {
        std::fs::create_dir_all(&d)?;
        let audio = if sr == basic_pitch::SR { mono.clone() } else { reech(&mono, sr) };
        ecrire_f32(d.join("audio22k.f32"), &audio)?;
        ecrire_f32(d.join("note.f32"), &a.note)?;
        ecrire_f32(d.join("onset.f32"), &a.attaque)?;
        std::fs::write(d.join("notes.json"), serde_json::to_string(&brutes)?)?;
        if let Some(p) = pulsation {
            let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p)?)?;
            let f = |k: &str| -> Vec<f32> { v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default() };
            let mesures = quantification::quantifier(&notes, &f("temps"), &f("premiers_temps"), 4);
            println!("{} mesures quantifiées", mesures.len());
            std::fs::write(d.join("mesures.json"), serde_json::to_string(&mesures)?)?;
        }
        println!("vidé dans {}", d.display());
    }
    Ok(())
}

fn dec_canaux(c: usize) -> rodio::ChannelCount {
    (c as u16).try_into().expect("canaux")
}
fn dec_sr(sr: u32) -> rodio::SampleRate {
    sr.try_into().expect("fréquence")
}

/// Le même rééchantillonnage que le crate, pour vider exactement l'entrée du
/// réseau : on repasse par `activations` sur un signal déjà à 22 050 Hz.
fn reech(mono: &[f32], sr: u32) -> Vec<f32> {
    rusty_music_transcription::basic_pitch::reechantillonner_pour_essai(mono, sr).expect("rééchantillonnage")
}
