// SPDX-License-Identifier: GPL-3.0-or-later
//! Combien coûte la construction du graphe, selon le profil de compilation.
use rusty_music_analysis::chemin::Graphe;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    // Vecteurs déterministes de 512 dimensions, comme les empreintes CLAP.
    let e: Vec<(i64, Vec<f32>)> = (0..n)
        .map(|i| {
            let v: Vec<f32> = (0..512)
                .map(|d| ((i * 7919 + d * 104729) as f32 * 0.000_37).sin())
                .collect();
            (i as i64, v)
        })
        .collect();
    let fils = std::thread::available_parallelism().map_or(4, |p| p.get());
    let t = std::time::Instant::now();
    let g = Graphe::construire(&e, 12, fils);
    let s = t.elapsed().as_secs_f64();
    let complet = s * (27031.0 / n as f64).powi(2);
    println!(
        "{n} points, {fils} fils — {s:.2} s  (taille {})\n  extrapolé à 27 031 : {:.0} s",
        g.taille(),
        complet
    );
}
