//! Diagnostic ponctuel : décode un seul fichier et rapporte sa taille en
//! mémoire, sans passer par l'encodeur GPU. Jetable — sert à isoler un
//! fichier suspect sans relancer la passe complète.
//!
//!   cargo run -p rusty-music-analysis --example diag_fichier -- <fichier>

use std::path::PathBuf;
use std::time::Instant;

use rusty_music_analysis::decode::{fenetres, FENETRES};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args().nth(1).expect("usage: diag_fichier <fichier>"));
    println!("décodage de {} ...", path.display());
    let debut = Instant::now();
    let blocs = fenetres(&path, FENETRES)?;
    let total: usize = blocs.iter().map(Vec::len).sum();
    println!(
        "ok : {} fenêtre(s), {total} échantillons, {:.1} Mo, {:?}",
        blocs.len(),
        total as f64 * 4.0 / 1_048_576.0,
        debut.elapsed()
    );
    Ok(())
}
