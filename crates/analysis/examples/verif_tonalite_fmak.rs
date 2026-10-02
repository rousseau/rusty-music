// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc d'essai de la tonalité contre une vérité terrain (FMAKv2, CC-BY-4.0 :
//! annotations d'experts sur des clips de la Free Music Archive). Jetable.
//!
//! Lit un dossier de clips `<id>.mp3` et écrit `id,tonalite` (notation
//! anglaise, « C# maj ») sur la sortie standard ; la comparaison aux
//! annotations se fait hors de Rust.
//!
//!   cargo run --release -p rusty-music-analysis --example verif_tonalite_fmak -- <dossier>

use std::path::PathBuf;

use rusty_music_analysis::descripteurs::{analyser, Analyseur};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dossier = PathBuf::from(std::env::args().nth(1).expect("usage: verif_tonalite_fmak <dossier>"));
    let analyseur = Analyseur::new();
    let mut fichiers: Vec<PathBuf> = std::fs::read_dir(&dossier)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "mp3"))
        .collect();
    fichiers.sort();
    println!("id,tonalite");
    for p in fichiers {
        let id = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        // symphonia panique sur certains MP3 tronqués : un clip ne doit pas
        // arrêter la passe.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| analyser(&p, &analyseur)));
        match r {
            Ok(Ok(d)) => println!("{},{}", id.trim_start_matches('0'), d.tonalite.unwrap_or_default()),
            Ok(Err(e)) => eprintln!("{id} : {e}"),
            Err(_) => eprintln!("{id} : panique du décodeur"),
        }
    }
    Ok(())
}
