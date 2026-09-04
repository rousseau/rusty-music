// SPDX-License-Identifier: GPL-3.0-or-later
//! Le spectrogramme distingue-t-il vraiment les stems ?
//!
//! Un spectrogramme joli n'est pas un spectrogramme utile. Le contrôle porte
//! donc sur ce qu'on lui demande de montrer : l'énergie par bande. Une basse
//! doit s'écraser sur le grave, une batterie s'étaler, une voix culminer dans
//! le médium. Si les quatre se ressemblent, l'affichage n'apprend rien et il
//! faut revoir l'échelle ou la plage dynamique.
//!
//!   cargo run --release -p rusty-music-player --example verif_spectre -- <stems.wav>…

use std::path::Path;
use std::time::Instant;

fn main() {
    let fichiers: Vec<String> = std::env::args().skip(1).collect();
    if fichiers.is_empty() {
        eprintln!("usage : … --example verif_spectre -- <fichier.wav>…");
        return;
    }

    println!(
        "{:<10} {:>9} {:>9} {:>9} {:>9} {:>8}",
        "stem", "moyenne", "graves", "médium", "aigus", "calcul"
    );
    println!("{}", "─".repeat(60));

    for f in &fichiers {
        let t = Instant::now();
        let s = match rusty_music_player::spectre::calculer(Path::new(f), 400, 46) {
            Ok(s) => s,
            Err(e) => {
                println!("{f} : {e}");
                continue;
            }
        };
        let ms = t.elapsed().as_millis();

        // L'image a les aigus en haut : le premier tiers des lignes est donc
        // le haut du spectre, le dernier le grave.
        let tiers = s.hauteur / 3;
        let bande = |a: usize, b: usize| -> f64 {
            let mut somme = 0.0;
            for y in a..b {
                for x in 0..s.largeur {
                    somme += s.pixels[y * s.largeur + x] as f64;
                }
            }
            somme / ((b - a) * s.largeur) as f64
        };
        let moyenne = s.pixels.iter().map(|x| *x as f64).sum::<f64>() / s.pixels.len() as f64;

        let nom: String = Path::new(f)
            .file_stem()
            .map(|n| {
                n.to_string_lossy()
                    .rsplit('—')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();

        println!(
            "{nom:<10} {moyenne:>9.1} {:>9.1} {:>9.1} {:>9.1} {ms:>6} ms",
            bande(2 * tiers, s.hauteur),
            bande(tiers, 2 * tiers),
            bande(0, tiers),
        );
    }
}
