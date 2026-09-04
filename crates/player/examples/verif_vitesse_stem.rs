// SPDX-License-Identifier: GPL-3.0-or-later
//! La vitesse d'un seul stem agit-elle vraiment ?
//!
//! ```bash
//! cargo run --release -p rusty-music-player --example verif_vitesse_stem -- <dossier de stems>
//! ```
//!
//! Reproduit **la séquence exacte de l'interface** : vitesse d'ensemble
//! d'abord, qui écrit sur tous les stems, puis la vitesse du stem écarté. C'est
//! cet ordre-là qui est en cause si le réglage par stem n'a pas d'effet — un
//! test qui n'écrirait que la seconde ne verrait rien.

use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let dossier = PathBuf::from(std::env::args().nth(1).expect("un dossier de stems"));
    let mut stems: Vec<(String, PathBuf)> = std::fs::read_dir(&dossier)
        .expect("dossier lisible")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wav"))
        .map(|p| {
            let nom = p
                .file_stem()
                .map(|s| s.to_string_lossy().rsplit('—').next().unwrap_or("").trim().to_string())
                .unwrap_or_default();
            (nom, p)
        })
        .collect();
    stems.sort();
    println!("{} stems : {:?}", stems.len(), stems.iter().map(|(n, _)| n).collect::<Vec<_>>());

    let m = rusty_music_player::Multipiste::charger(&stems).expect("chargement");

    // La séquence de `appliquerVitesses()`, à la lettre.
    m.vitesse(1.0);
    m.vitesse_stem(1, 1.5);
    println!("après la séquence de l'interface : {:?}", m.vitesses());

    m.reprendre();
    std::thread::sleep(Duration::from_millis(1500));
    m.pause();

    let pos = m.position();
    let derive = m.derive();
    println!(
        "après 1,5 s de lecture : position {:.2} s, dérive {:.3} s",
        pos.as_secs_f64(),
        derive.as_secs_f64()
    );
    if derive.as_millis() < 100 {
        println!("\n✗ le stem accéléré n'a PAS pris d'avance — le réglage n'agit pas");
    } else {
        println!("\n✓ le stem accéléré a pris de l'avance : le moteur fait son travail");
    }
}
