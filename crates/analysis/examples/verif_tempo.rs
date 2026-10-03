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
    // Un fichier texte (un chemin par ligne) à la place d'un dossier : pour rejouer
    // un échantillon d'une bibliothèque.
    if dossier.is_file() {
        v = std::fs::read_to_string(&dossier)?.lines().map(PathBuf::from).collect();
    } else {
        fichiers(&dossier, &mut v)?;
    }
    v.sort();
    println!("chemin,bpm");
    // Parallèle : le banc se rejoue en quelques secondes.
    let fils = std::thread::available_parallelism().map_or(4, |n| n.get());
    let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
    std::thread::scope(|s| {
        for k in 0..fils {
            let (tx, analyseur, dossier, v) = (tx.clone(), &analyseur, &dossier, &v);
            s.spawn(move || {
                for p in v.iter().skip(k).step_by(fils) {
                    let rel = if dossier.is_file() { p.display().to_string() } else { p.strip_prefix(dossier).unwrap_or(p).display().to_string() };
                    // symphonia panique sur certains fichiers tronqués : un fichier
                    // ne doit pas arrêter la passe.
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| analyser(p, analyseur)));
                    match r {
                        Ok(Ok(d)) => {
                            let _ = tx.send((rel, d.bpm.map_or(String::new(), |b| format!("{b:.3}"))));
                        }
                        Ok(Err(e)) => eprintln!("{rel} : {e}"),
                        Err(_) => eprintln!("{rel} : panique du décodeur"),
                    }
                }
            });
        }
        drop(tx);
        let mut sorties: Vec<(String, String)> = rx.iter().collect();
        sorties.sort();
        for (rel, bpm) in sorties {
            println!("{rel},{bpm}");
        }
    });
    Ok(())
}
