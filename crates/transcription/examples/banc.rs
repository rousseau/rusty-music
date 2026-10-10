// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc contre des partitions publiées (`experiments/partitions/`) : transcrit
//! un stem de basse avec la chaîne de l'éditeur et écrit notes et mesures
//! quantifiées.
//!
//!   cargo run --release -p rusty-music-transcription --example banc -- <bass.wav> <pulsation.json> <sortie.json> [seuil_porte_db]
//!
//! `pulsation.json` : `{"temps": [...], "premiers_temps": [...]}` (cache de
//! l'éditeur, ou sortie de `verif_pulsation`).
//!
//! Grille de réglages : `banc -- reglages <bass.wav> <pulsation.json> <dossier>`
//! calcule les activations une fois et écrit un fichier par réglage (Basic
//! Pitch et porte).
//!
//! Grille de doigtés : `banc -- doigtes <transcription.json> <pulsation.json> <dossier>`
//! repose les notes d'une transcription avec plusieurs jeux de [`Couts`] et
//! écrit un fichier par jeu.

use std::path::PathBuf;

use rodio::source::UniformSourceIterator;
use rodio::{Decoder, Source};
use rusty_music_transcription::basic_pitch::{self, Reglages, Transcripteur};
use rusty_music_transcription::tablature::{self, Accordage, Couts};
use rusty_music_transcription::Note;
use rusty_music_transcription::{monophonie, porte, quantification};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a[0] == "doigtes" {
        return grille_doigtes(&a[1..]);
    }
    if a[0] == "reglages" {
        return grille_reglages(&a[1..]);
    }
    let (wav, puls, sortie) = (PathBuf::from(&a[0]), PathBuf::from(&a[1]), PathBuf::from(&a[2]));
    let dec = Decoder::try_from(std::fs::File::open(&wav)?)?;
    let (sr, canaux) = (dec.sample_rate().get(), dec.channels().get() as usize);
    let brut: Vec<f32> = UniformSourceIterator::new(dec, (canaux as u16).try_into()?, sr.try_into()?).collect();
    let mono: Vec<f32> = brut.chunks(canaux).map(|c| c.iter().sum::<f32>() / canaux as f32).collect();
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(puls)?)?;
    let f = |k: &str| -> Vec<f32> { v[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect() };
    let (temps, premiers) = (f("temps"), f("premiers_temps"));
    let mut t = Transcripteur::charger_installe()?;
    let act = t.activations(&mono, sr)?;
    // Seuil de la porte en dB (4ᵉ argument) ; 0 = sans porte.
    let seuil: f32 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(porte::SEUIL_DB);
    let brutes = basic_pitch::notes(&act, &Reglages::basse());
    let brutes = if seuil > 0.0 { porte::filtrer(brutes, &mono, sr, seuil) } else { brutes };
    let mut notes = monophonie::monophonique(brutes);
    let accordage = Accordage::choisir(&notes);
    tablature::poser(&mut notes, &accordage);
    let mesures = quantification::quantifier(&notes, &temps, &premiers, 4);
    std::fs::write(&sortie, serde_json::to_string(&serde_json::json!({ "notes": notes, "mesures": mesures, "accordage": accordage.cordes }))?)?;
    println!("{} notes, {} mesures → {}", notes.len(), mesures.len(), sortie.display());
    Ok(())
}

fn lire_pulsation(chemin: &str) -> Result<(Vec<f32>, Vec<f32>), Box<dyn std::error::Error>> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(chemin)?)?;
    let f = |k: &str| -> Vec<f32> { v[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect() };
    Ok((f("temps"), f("premiers_temps")))
}

fn grille_doigtes(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let notes: Vec<Note> = serde_json::from_value(v["notes"].clone())?;
    let (temps, premiers) = lire_pulsation(&a[1])?;
    let dossier = PathBuf::from(&a[2]);
    std::fs::create_dir_all(&dossier)?;
    let mut n = 0;
    for &corde in &[0.3f32, 0.6, 1.0] {
        for &vide in &[0.0f32, 0.1, 0.3] {
            for &aigue in &[0.0f32, 0.1, 0.2, 0.4] {
                for &ph in &[0.0f32, 0.1] {
                    let k = Couts { corde, vide_haut: vide, corde_aigue: aigue, pente_haut: ph, ..Couts::default() };
                    let mut x = notes.clone();
                    let accordage = Accordage::choisir(&x);
                    tablature::poser_avec(&mut x, &accordage, &k);
                    let mesures = quantification::quantifier(&x, &temps, &premiers, 4);
                    let nom = format!("dg_{corde}_{vide}_{aigue}_{ph}.json");
                    std::fs::write(dossier.join(nom), serde_json::to_string(&serde_json::json!({ "notes": x, "mesures": mesures }))?)?;
                    n += 1;
                }
            }
        }
    }
    println!("{n} jeux de coûts écrits dans {}", dossier.display());
    Ok(())
}

fn lire_mono(chemin: &str) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>> {
    let dec = Decoder::try_from(std::fs::File::open(chemin)?)?;
    let (sr, canaux) = (dec.sample_rate().get(), dec.channels().get() as usize);
    let brut: Vec<f32> = UniformSourceIterator::new(dec, (canaux as u16).try_into()?, sr.try_into()?).collect();
    Ok((brut.chunks(canaux).map(|c| c.iter().sum::<f32>() / canaux as f32).collect(), sr))
}

fn grille_reglages(a: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (mono, sr) = lire_mono(&a[0])?;
    let (temps, premiers) = lire_pulsation(&a[1])?;
    let dossier = PathBuf::from(&a[2]);
    std::fs::create_dir_all(&dossier)?;
    let mut t = Transcripteur::charger_installe()?;
    let act = t.activations(&mono, sr)?;
    let mut n = 0;
    for &sa in &[0.3f32, 0.4, 0.5, 0.6] {
        for &st in &[0.2f32, 0.3, 0.4] {
            for &d in &[4usize, 6, 8] {
                for &h in &[0.0f32, 0.6] {
                    for &porte_db in &[0.0f32, 30.0, 40.0] {
                        let r = Reglages { seuil_attaque: sa, seuil_trame: st, duree_min: d, harmoniques: (h > 0.0).then_some(h), ..Reglages::basse() };
                        let brutes = basic_pitch::notes(&act, &r);
                        let brutes = if porte_db > 0.0 { porte::filtrer(brutes, &mono, sr, porte_db) } else { brutes };
                        let mut notes = monophonie::monophonique(brutes);
                        let accordage = Accordage::choisir(&notes);
                        tablature::poser(&mut notes, &accordage);
                        let mesures = quantification::quantifier(&notes, &temps, &premiers, 4);
                        let nom = format!("bp_{sa}_{st}_{d}_{h}_{porte_db}.json");
                        std::fs::write(dossier.join(nom), serde_json::to_string(&serde_json::json!({ "notes": notes, "mesures": mesures }))?)?;
                        n += 1;
                    }
                }
            }
        }
    }
    println!("{n} réglages écrits dans {}", dossier.display());
    Ok(())
}
