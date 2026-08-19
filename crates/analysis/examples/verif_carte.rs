//! La carte veut-elle dire quelque chose ?
//!
//! Contrôle indépendant du modèle : on compare les distances sur la carte à
//! des regroupements que le réseau n'a jamais vus — l'album et l'artiste, qui
//! viennent des tags. Si deux morceaux d'un même album ne sont pas plus
//! proches que deux morceaux tirés au hasard, la carte ne dit rien.
//!
//!   cargo run --release -p rusty-music-analysis --example verif_carte -- <base.db>

use std::collections::HashMap;

use rusty_music_analysis::passe::MODELE;
use rusty_music_core::Library;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "rusty-music.db".into());
    let lib = Library::open(std::path::Path::new(&base))?;

    let points = lib.map_points(MODELE)?;
    if points.len() < 20 {
        eprintln!("{} points seulement : pas de quoi juger", points.len());
        return Ok(());
    }

    // Album et artiste de chaque morceau placé.
    let mut stmt = lib.conn.prepare(
        "SELECT id, COALESCE(album,''), COALESCE(COALESCE(album_artist,artist),'') FROM tracks",
    )?;
    let meta: HashMap<i64, (String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect::<Result<_, _>>()?;

    let d = |a: &(i64, f32, f32, i64), b: &(i64, f32, f32, i64)| {
        ((a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
    };

    let (mut alb, mut nalb) = (0.0f64, 0usize);
    let (mut art, mut nart) = (0.0f64, 0usize);
    let (mut fam, mut nfam) = (0.0f64, 0usize);
    let (mut tout, mut ntout) = (0.0f64, 0usize);

    for i in 0..points.len() {
        for j in i + 1..points.len() {
            let dist = d(&points[i], &points[j]) as f64;
            tout += dist;
            ntout += 1;

            let (Some(a), Some(b)) = (meta.get(&points[i].0), meta.get(&points[j].0)) else {
                continue;
            };
            if !a.0.is_empty() && a.0 == b.0 {
                alb += dist;
                nalb += 1;
            }
            if !a.1.is_empty() && a.1 == b.1 {
                art += dist;
                nart += 1;
            }
            if points[i].3 == points[j].3 {
                fam += dist;
                nfam += 1;
            }
        }
    }

    let moy = |s: f64, n: usize| if n == 0 { f64::NAN } else { s / n as f64 };
    let hasard = moy(tout, ntout);

    println!("{} morceaux placés\n", points.len());
    println!(
        "{:<28} {:>9} {:>9} {:>10}",
        "paires", "distance", "paires", "vs hasard"
    );
    println!("{}", "─".repeat(60));
    for (nom, s, n) in [
        ("même album", alb, nalb),
        ("même artiste", art, nart),
        ("même famille (k-means)", fam, nfam),
        ("au hasard", tout, ntout),
    ] {
        let m = moy(s, n);
        println!("{nom:<28} {m:>9.3} {n:>9} {:>9.2}×", m / hasard);
    }

    let ratio = moy(alb, nalb) / hasard;
    println!("\n{}", "─".repeat(60));
    println!(
        "{}",
        if ratio < 0.6 {
            "La carte tient : deux morceaux d'un même album y sont nettement \n\
             plus proches que deux morceaux quelconques."
        } else if ratio < 0.85 {
            "Signal présent mais modeste : la carte regroupe, sans trancher."
        } else {
            "La carte ne dit rien : les albums y sont dispersés comme au hasard."
        }
    );
    Ok(())
}
