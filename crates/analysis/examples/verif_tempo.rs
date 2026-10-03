// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc d'essai du tempo contre une vérité terrain (GTZAN, Ballroom…).
//! Jetable. Parcourt un dossier (récursif) et écrit `chemin,bpm` — chemin relatif
//! au dossier — sur la sortie standard ; la comparaison aux annotations se fait
//! hors de Rust (`experiments/tempo/`).
//!
//!   cargo run --release -p rusty-music-analysis --example verif_tempo -- <dossier>

use std::path::{Path, PathBuf};

use rusty_music_analysis::descripteurs::{analyser, Analyseur};

fn fichiers(dossier: &Path, sortie: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dossier)? {
        let p = e?.path();
        if p.is_dir() {
            fichiers(&p, sortie)?;
        } else if p.extension().is_some_and(|x| ["wav", "mp3", "flac", "ogg", "m4a"].iter().any(|y| x.eq_ignore_ascii_case(y))) {
            sortie.push(p);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dossier = PathBuf::from(std::env::args().nth(1).expect("usage: verif_tempo <dossier>"));
    let analyseur = Analyseur::new();
    let mut v = Vec::new();
    fichiers(&dossier, &mut v)?;
    v.sort();
    println!("chemin,bpm");
    for p in v {
        let rel = p.strip_prefix(&dossier).unwrap_or(&p).display().to_string();
        // symphonia panique sur certains fichiers tronqués : un fichier ne doit
        // pas arrêter la passe.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| analyser(&p, &analyseur)));
        match r {
            Ok(Ok(d)) => println!("{rel},{}", d.bpm.map_or(String::new(), |b| format!("{b:.3}"))),
            Ok(Err(e)) => eprintln!("{rel} : {e}"),
            Err(_) => eprintln!("{rel} : panique du décodeur"),
        }
    }
    Ok(())
}
