//! Ce que coûte un maillage de Voronoï sur la bibliothèque réelle.
//!
//! Mesure préalable à la refonte des formes : les contours par isolignes
//! rendent des taches arrondies, un maillage rend des polygones irréguliers.
//! Encore faut-il qu'il tienne à 27 000 sites.

use rusty_music_core::db::Library;
use voronoice::{BoundingBox, Point, VoronoiBuilder};

fn main() -> anyhow::Result<()> {
    let base = std::env::args().nth(1).unwrap_or_else(|| "rusty-music.db".into());
    let lib = Library::open(std::path::Path::new(&base))?;
    let points = lib.map_points("clap-htsat-unfused-5f")?;
    println!("{} sites", points.len());

    for relaxations in [0usize, 1, 2] {
        let sites: Vec<Point> = points
            .iter()
            .map(|&(_, x, y, _)| Point {
                x: x as f64,
                y: y as f64,
            })
            .collect();
        let t = std::time::Instant::now();
        let v = VoronoiBuilder::default()
            .set_sites(sites)
            .set_bounding_box(BoundingBox::new_centered_square(2.16))
            .set_lloyd_relaxation_iterations(relaxations)
            .build();
        let duree = t.elapsed();
        match v {
            Some(v) => {
                let cellules = v.iter_cells().count();
                let sommets: usize = v.iter_cells().map(|c| c.iter_vertices().count()).sum();
                println!(
                    "  {relaxations} relaxation(s) : {cellules} cellules, {sommets} sommets, {:.2} s",
                    duree.as_secs_f64()
                );
            }
            None => println!("  {relaxations} relaxation(s) : échec en {:.2} s", duree.as_secs_f64()),
        }
    }
    Ok(())
}
