//! Où part le temps quand on se positionne : l'ouverture, ou la lecture ?
//!
//! Sur le SSD, le positionnement rend le décodage sept fois moins cher. Sur la
//! carte SD, la passe ne va pas plus vite du tout. Si le positionnement lit
//! bien moins d'octets, la seule explication est qu'on les lit **avant** :
//! `total_duration()` doit connaître la longueur du morceau, et un MPEG sans
//! en-tête Xing ne la déclare pas — il faut parcourir ses trames jusqu'au
//! bout pour la déduire.
//!
//! Cet exemple sépare les deux postes, fichier par fichier. À lancer sur des
//! fichiers **non encore lus** : une fois en cache, la mesure ne veut plus
//! rien dire.
//!
//!   cargo run --release -p rusty-music-analysis --example cout_ouverture -- <fichier>…

use std::path::PathBuf;
use std::time::Instant;

use rusty_music_analysis::decode::{fenetres, fenetres_integrales, FENETRES};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pistes: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if pistes.is_empty() {
        eprintln!("usage : … --example cout_ouverture -- <fichier audio>…");
        return Ok(());
    }

    println!(
        "{:<30} {:>8} {:>10} {:>12} {:>10}",
        "morceau", "taille", "variante", "durée", "Mo/s apparents"
    );
    println!("{}", "─".repeat(76));

    // Une seule variante par fichier, en alternance : dès qu'un fichier a été
    // lu une fois, il est en cache et toute mesure ultérieure ment.
    let (mut t_flux, mut n_flux) = (std::time::Duration::ZERO, 0u32);
    let (mut t_bloc, mut n_bloc) = (std::time::Duration::ZERO, 0u32);

    for p in &pistes {
        let nom: String = p
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(29)
            .collect();
        let taille = std::fs::metadata(p)?.len();

        let flux = n_flux + n_bloc; // alterne un fichier sur deux
        let t = Instant::now();
        if flux % 2 == 0 {
            fenetres(p, FENETRES)?;
        } else {
            fenetres_integrales(p, FENETRES)?;
        }
        let d = t.elapsed();
        if flux % 2 == 0 {
            t_flux += d;
            n_flux += 1;
        } else {
            t_bloc += d;
            n_bloc += 1;
        }

        println!(
            "{nom:<30} {:>6.1} Mo {:>10} {:>9} ms {:>10.1}",
            taille as f64 / 1_048_576.0,
            if flux % 2 == 0 {
                "position."
            } else {
                "intégral"
            },
            d.as_millis(),
            taille as f64 / 1_048_576.0 / d.as_secs_f64().max(1e-9),
        );
    }

    let moy = |t: std::time::Duration, n: u32| if n > 0 { t.as_millis() / n as u128 } else { 0 };
    println!("{}", "─".repeat(76));
    println!(
        "  positionnement : {} ms · lecture intégrale : {} ms",
        moy(t_flux, n_flux),
        moy(t_bloc, n_bloc)
    );

    println!(
        "\nSi « flux » est nettement plus rapide, le support sert bien des\n\
         lectures dispersées et il ne faut pas tout lire. Si les deux se valent,\n\
         c'est que le transfert est facturé au fichier et non à l'octet — et il\n\
         n'y a alors rien à gagner sans changer de support."
    );
    Ok(())
}
