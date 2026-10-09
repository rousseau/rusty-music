// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc de la pulsation (Beat This!) : temps de calcul et sorties, une ligne
//! JSON par fichier — `{"chemin", "ms", "duree_s", "temps", "premiers"}`. La
//! notation contre les annotations (GTZAN) se fait hors de Rust :
//! `experiments/pulsation/noter.py`.
//!
//!   cargo run --release -p rusty-music-editor --example verif_pulsation -- <fichier|dossier|liste> [petit|complet] [fils]
//!
//! Un fichier texte (un chemin par ligne) à la place d'un dossier rejoue un
//! échantillon de la bibliothèque, comme `verif_tempo`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rusty_music_editor::pulsation::{Modele, Pisteur};

fn fichiers(dossier: &Path, sortie: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dossier)? {
        let p = e?.path();
        if p.is_dir() {
            fichiers(&p, sortie)?;
        } else if p
            .extension()
            .is_some_and(|x| ["wav", "mp3", "flac", "ogg", "m4a", "au"].iter().any(|y| x.eq_ignore_ascii_case(y)))
        {
            sortie.push(p);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let entree = PathBuf::from(args.next().ok_or("usage : verif_pulsation <fichier|dossier|liste> [petit|complet] [fils]")?);
    let modele = match args.next().as_deref() {
        Some("petit") => Modele::Petit,
        _ => Modele::Complet,
    };
    let fils: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));

    let mut v = Vec::new();
    let liste = entree.is_file() && entree.extension().is_some_and(|x| x == "txt");
    if liste {
        v = std::fs::read_to_string(&entree)?.lines().map(PathBuf::from).collect();
    } else if entree.is_file() {
        v.push(entree.clone());
    } else {
        fichiers(&entree, &mut v)?;
    }
    v.sort();
    let fils = fils.clamp(1, v.len().max(1));
    eprintln!("{} fichier(s), modèle {modele:?}, {fils} fil(s)", v.len());

    std::thread::scope(|s| {
        for k in 0..fils {
            let (v, entree) = (&v, &entree);
            s.spawn(move || {
                let t0 = Instant::now();
                let mut pisteur = match Pisteur::charger(modele) {
                    Ok(p) => p,
                    Err(e) => return eprintln!("chargement : {e}"),
                };
                if k == 0 {
                    eprintln!("chargement du réseau : {} ms", t0.elapsed().as_millis());
                }
                for p in v.iter().skip(k).step_by(fils) {
                    let rel = if entree.is_dir() { p.strip_prefix(entree).unwrap_or(p) } else { p.as_path() };
                    let t = Instant::now();
                    // symphonia panique sur certains fichiers tronqués.
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pisteur.analyser_fichier(p)));
                    let ms = t.elapsed().as_millis();
                    match r {
                        Ok(Ok(a)) => println!(
                            "{}",
                            serde_json::json!({
                                "chemin": rel.display().to_string(),
                                "ms": ms,
                                "bpm": a.bpm(),
                                "metrique": a.metrique(),
                                "temps": a.temps,
                                "premiers": a.premiers_temps,
                            })
                        ),
                        Ok(Err(e)) => eprintln!("{} : {e}", rel.display()),
                        Err(_) => eprintln!("{} : panique du décodeur", rel.display()),
                    }
                }
            });
        }
    });
    Ok(())
}
