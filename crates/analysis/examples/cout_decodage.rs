//! Décoder tout le fichier, ou se placer aux trois fenêtres ?
//!
//! `cout_chaine` a montré que le décodage pèse 87 % de la chaîne. Or on ne
//! garde que trois fenêtres de dix secondes d'un morceau qui en dure deux
//! cent cinquante : 12 % de l'audio. La question est donc de savoir si un
//! positionnement (`try_seek`) évite vraiment de lire le reste, ou si le
//! décodeur relit tout de toute façon.
//!
//!   cargo run --release -p rusty-music-analysis --example cout_decodage -- <fichier>…
//!
//! Compare, pour chaque fichier : la durée des deux méthodes, et la
//! similarité des empreintes obtenues — un gain de temps qui changerait la
//! représentation ne serait pas un gain.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusty_music_analysis::decode::{fenetres, fenetres_integrales, FENETRES};
use rusty_music_analysis::mel::Mel;
use rusty_music_analysis::Embedder;

fn empreinte(mel: &Mel, enc: &mut Embedder, blocs: &[Vec<f32>]) -> Vec<f32> {
    let spec: Vec<f32> = blocs.iter().flat_map(|b| mel.spectrogramme(b)).collect();
    let vecteurs = enc.empreintes(&spec, blocs.len()).expect("inférence");
    let mut somme = vec![0.0f32; vecteurs[0].len()];
    for v in &vecteurs {
        for (s, x) in somme.iter_mut().zip(v) {
            *s += x / vecteurs.len() as f32;
        }
    }
    rusty_music_analysis::projection::normaliser(&somme)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Un premier argument numérique fixe le nombre de fenêtres : chaque
    // fenêtre supplémentaire est un positionnement de plus, et c'est
    // précisément là qu'un décodeur réutilisé pourrait dériver.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let n = match args.first().and_then(|a| a.parse::<usize>().ok()) {
        Some(n) => {
            args.remove(0);
            n
        }
        None => FENETRES,
    };
    let pistes: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
    if pistes.is_empty() {
        eprintln!("usage : … --example cout_decodage -- [fenêtres] <fichier audio>…");
        return Ok(());
    }
    println!("{n} fenêtres par morceau\n");

    let mel = Mel::new();
    let mut enc = Embedder::charger(
        None,
        std::thread::available_parallelism().map_or(1, |n| n.get()),
    )?;

    println!(
        "{:<32} {:>10} {:>10} {:>7} {:>9}",
        "morceau", "tout lire", "position.", "gain", "cosinus"
    );
    println!("{}", "─".repeat(73));

    let (mut tt, mut tp) = (Duration::ZERO, Duration::ZERO);
    let mut echecs = 0;

    for p in &pistes {
        let nom: String = p
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(31)
            .collect();

        let t = Instant::now();
        let complet = fenetres_integrales(p, n)?;
        let d_tout = t.elapsed();

        let t = Instant::now();
        let partiel = fenetres(p, n)?;
        if partiel.len() != complet.len() {
            println!(
                "{nom:<32} {:>8} ms  positionnement refusé, repli",
                d_tout.as_millis()
            );
            echecs += 1;
            continue;
        }
        let d_pos = t.elapsed();

        // La similarité dit si les deux méthodes décrivent le même morceau.
        let (a, b) = (
            empreinte(&mel, &mut enc, &complet),
            empreinte(&mel, &mut enc, &partiel),
        );
        let cos: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();

        println!(
            "{nom:<32} {:>8} ms {:>8} ms {:>6.1}× {:>9.3}",
            d_tout.as_millis(),
            d_pos.as_millis(),
            d_tout.as_secs_f64() / d_pos.as_secs_f64().max(1e-9),
            cos
        );
        tt += d_tout;
        tp += d_pos;
    }

    let n = (pistes.len() - echecs).max(1) as u32;
    println!("{}", "─".repeat(73));
    println!(
        "{:<32} {:>8} ms {:>8} ms {:>6.1}×   (moyenne)",
        "",
        (tt / n).as_millis(),
        (tp / n).as_millis(),
        tt.as_secs_f64() / tp.as_secs_f64().max(1e-9)
    );
    if echecs > 0 {
        println!("{echecs} fichier(s) non positionnables");
    }
    Ok(())
}
