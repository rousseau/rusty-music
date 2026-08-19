//! Coût de la chaîne complète, sur de vrais morceaux.
//!
//! Complète le jalon 1, qui ne chiffrait que l'inférence : on mesure ici les
//! trois postes bout à bout — décodage, log-mel, empreinte — pour savoir
//! lequel commande la passe.
//!
//!   cargo run --release -p rusty-music-analysis --example cout_chaine -- <fichier>…
//!
//! Sans argument, prend quelques morceaux de la base locale.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusty_music_analysis::decode::fenetres;
use rusty_music_analysis::mel::Mel;
use rusty_music_analysis::Embedder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pistes: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if pistes.is_empty() {
        eprintln!("usage : … --example cout_chaine -- <fichier audio>…");
        return Ok(());
    }

    let mel = Mel::new();
    let mut enc = Embedder::charger(
        None,
        std::thread::available_parallelism().map_or(1, |n| n.get()),
    )?;

    println!(
        "{:<34} {:>9} {:>9} {:>9} {:>9}",
        "morceau", "décodage", "log-mel", "empreinte", "total"
    );
    println!("{}", "─".repeat(74));

    let (mut td, mut tm, mut te) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut empreintes = Vec::new();

    for p in &pistes {
        let t = Instant::now();
        let blocs = match fenetres(p, 1) {
            Ok(b) => b,
            Err(e) => {
                println!("{:<34} {e}", court(p));
                continue;
            }
        };
        let d = t.elapsed();

        let t = Instant::now();
        let spec: Vec<f32> = blocs.iter().flat_map(|b| mel.spectrogramme(b)).collect();
        let m = t.elapsed();

        let t = Instant::now();
        let v = enc.empreintes(&spec, blocs.len())?;
        let e = t.elapsed();

        println!(
            "{:<34} {:>7.0} ms {:>7.0} ms {:>7.0} ms {:>7.0} ms",
            court(p),
            d.as_secs_f64() * 1000.0,
            m.as_secs_f64() * 1000.0,
            e.as_secs_f64() * 1000.0,
            (d + m + e).as_secs_f64() * 1000.0
        );
        td += d;
        tm += m;
        te += e;
        empreintes.push((court(p), v.into_iter().next().unwrap()));
    }

    let n = empreintes.len().max(1) as f64;
    let total = (td + tm + te).as_secs_f64() / n;
    println!("{}", "─".repeat(74));
    println!(
        "{:<34} {:>7.0} ms {:>7.0} ms {:>7.0} ms {:>7.0} ms   (moyenne)",
        "",
        td.as_secs_f64() * 1000.0 / n,
        tm.as_secs_f64() * 1000.0 / n,
        te.as_secs_f64() * 1000.0 / n,
        total * 1000.0
    );
    println!(
        "\npart de chaque poste : décodage {:.0} %, log-mel {:.0} %, empreinte {:.0} %",
        100.0 * td.as_secs_f64() / (td + tm + te).as_secs_f64(),
        100.0 * tm.as_secs_f64() / (td + tm + te).as_secs_f64(),
        100.0 * te.as_secs_f64() / (td + tm + te).as_secs_f64(),
    );

    // Le vrai contrôle : deux morceaux proches doivent l'être aussi dans
    // l'espace des empreintes. Sans ça, tout le reste est du bruit bien
    // chronométré.
    if empreintes.len() >= 2 {
        println!("\nsimilarités (cosinus) :");
        for i in 0..empreintes.len() {
            for j in i + 1..empreintes.len() {
                let (a, b) = (&empreintes[i], &empreintes[j]);
                println!("  {:.3}   {} ↔ {}", cosinus(&a.1, &b.1), a.0, b.0);
            }
        }
    }
    Ok(())
}

fn court(p: &std::path::Path) -> String {
    let s = p
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    s.chars().take(32).collect()
}

fn cosinus(a: &[f32], b: &[f32]) -> f32 {
    let p: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    p / (na * nb)
}
