//! L'espace d'empreintes veut-il dire quelque chose ?
//!
//! Un cosinus élevé entre deux morceaux ne prouve rien tout seul : les
//! empreintes CLAP vivent dans un cône étroit, où *toutes* les similarités
//! sont hautes. Ce qui compte est l'écart entre deux mesures :
//!
//!   - **intra** : deux fenêtres du *même* morceau ;
//!   - **inter** : deux fenêtres de morceaux *différents*.
//!
//! Si intra ne dépasse pas inter, la carte ne dira rien — inutile d'aller
//! plus loin. C'est le contrôle qui décide si le jalon 2 est concluant.
//!
//!   cargo run --release -p rusty-music-analysis --example verif_empreintes -- <fichier>…

use std::path::PathBuf;

use rusty_music_analysis::decode::{fenetres, FENETRES};
use rusty_music_analysis::mel::Mel;
use rusty_music_analysis::Embedder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pistes: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if pistes.len() < 2 {
        eprintln!("il faut au moins deux morceaux");
        return Ok(());
    }

    let mel = Mel::new();
    let mut enc = Embedder::charger(
        None,
        std::thread::available_parallelism().map_or(1, |n| n.get()),
    )?;

    // Trois fenêtres par morceau : début, milieu, fin.
    let mut par_piste: Vec<(String, Vec<Vec<f32>>)> = Vec::new();
    for p in &pistes {
        let blocs = match fenetres(p, FENETRES) {
            Ok(b) if b.len() == 3 => b,
            Ok(_) => {
                eprintln!("  {} : trop court, ignoré", court(p));
                continue;
            }
            Err(e) => {
                eprintln!("  {} : {e}", court(p));
                continue;
            }
        };
        let spec: Vec<f32> = blocs.iter().flat_map(|b| mel.spectrogramme(b)).collect();
        par_piste.push((court(p), enc.empreintes(&spec, 3)?));
        println!("  analysé : {}", par_piste.last().unwrap().0);
    }

    if par_piste.len() < 2 {
        eprintln!("pas assez de morceaux exploitables");
        return Ok(());
    }

    let mut intra = Vec::new();
    for (_, v) in &par_piste {
        for i in 0..v.len() {
            for j in i + 1..v.len() {
                intra.push(cosinus(&v[i], &v[j]));
            }
        }
    }

    let mut inter = Vec::new();
    for a in 0..par_piste.len() {
        for b in a + 1..par_piste.len() {
            for u in &par_piste[a].1 {
                for v in &par_piste[b].1 {
                    inter.push(cosinus(u, v));
                }
            }
        }
    }

    let (mi, ma) = (moyenne(&intra), moyenne(&inter));
    println!("\n{}", "─".repeat(56));
    println!("intra-morceau : {mi:.3}   ({} paires)", intra.len());
    println!("inter-morceaux: {ma:.3}   ({} paires)", inter.len());
    println!("écart         : {:+.3}", mi - ma);
    println!("{}", "─".repeat(56));

    println!(
        "\n{}",
        if mi - ma > 0.05 {
            "L'espace discrimine : deux fenêtres d'un même morceau se \
             ressemblent nettement plus que deux morceaux distincts."
        } else {
            "L'espace NE discrimine PAS : les empreintes ne dépendent guère du \
             morceau. Prétraitement à revoir avant d'aller plus loin."
        }
    );
    Ok(())
}

fn court(p: &std::path::Path) -> String {
    p.file_stem().map_or_else(String::new, |s| {
        s.to_string_lossy().chars().take(30).collect()
    })
}

fn moyenne(v: &[f32]) -> f32 {
    v.iter().sum::<f32>() / v.len().max(1) as f32
}

fn cosinus(a: &[f32], b: &[f32]) -> f32 {
    let p: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    p / (na * nb)
}
