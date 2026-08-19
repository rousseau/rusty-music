//! Ce que coûte la projection à l'échelle de la bibliothèque.
//!
//! La projection ne se découpe pas : les coordonnées t-SNE n'ont de sens que
//! relativement à l'ensemble projeté d'un bloc. Deux lots donneraient deux
//! repères sans rapport. Il faut donc savoir ce que coûte une passe unique sur
//! la totalité — c'est ce que ce banc mesure, sur des vecteurs synthétiques.
//!
//!   cargo run --release -p rusty-music-analysis --example cout_projection

use std::time::Instant;

use rusty_music_analysis::cluster::kmeans;
use rusty_music_analysis::projection::{cadrer, projeter};

fn main() {
    // Structure plausible : des amas, pas du bruit uniforme — t-SNE converge
    // différemment selon que les données ont une structure ou non.
    let faux = |n: usize| -> Vec<Vec<f32>> {
        (0..n)
            .map(|i| {
                let amas = i % 12;
                (0..512)
                    .map(|d| {
                        let base = if d % 12 == amas { 1.0 } else { 0.0 };
                        base + (((i * 2654435761 + d * 40503) % 1000) as f32 / 1000.0 - 0.5) * 0.3
                    })
                    .collect()
            })
            .collect()
    };

    println!("{:>8}  {:>12}  {:>12}", "points", "projection", "familles");
    println!("{}", "─".repeat(38));

    for n in [1_000usize, 5_000, 27_044] {
        let v = faux(n);

        let t = Instant::now();
        let mut pts = projeter(&v, 30.0, 1000);
        cadrer(&mut pts);
        let tp = t.elapsed().as_secs_f64();

        let t = Instant::now();
        let _ = kmeans(&v, 12, 50);
        let tk = t.elapsed().as_secs_f64();

        println!("{n:>8}  {:>10.1} s  {:>10.1} s", tp, tk);
    }

    // Les chemins entre deux morceaux enchaînent des recherches de plus
    // proches voisins. En force brute c'est 27 044 × 512 produits par requête :
    // assez pour justifier un index approché, ou pas ? On mesure avant de
    // compliquer.
    let v = faux(27_044);
    let cible = v[1234].clone();
    let t = Instant::now();
    let tours = 20;
    let mut garde = 0usize;
    for _ in 0..tours {
        garde = v
            .iter()
            .enumerate()
            .map(|(i, u)| {
                let d: f32 = u.iter().zip(&cible).map(|(a, b)| (a - b) * (a - b)).sum();
                (i, d)
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map_or(0, |(i, _)| i);
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / tours as f64;
    println!("\nplus proche voisin parmi 27 044, force brute : {ms:.1} ms  (voisin {garde})");
}
