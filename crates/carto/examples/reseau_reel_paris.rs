// SPDX-License-Identifier: GPL-3.0-or-later
//! Le graphe routable sur la vraie ville : combien de sommets, en combien de
//! temps, et un plus court chemin réel entre deux points pris au hasard.
//!
//! `cargo run --release -p rusty-music-carto --example reseau_reel_paris -- <ville-paris.db>`

use std::path::Path;

use rusty_music_carto::reseau_reel::Graphe;

fn main() -> anyhow::Result<()> {
    let ville = std::env::args().nth(1).unwrap_or_else(|| "ville-paris.db".into());
    let t = std::time::Instant::now();
    let extrait = rusty_music_osm::base::lire(Path::new(&ville))?;
    println!("lecture : {} tronçons, {:.2} s", extrait.troncons.len(), t.elapsed().as_secs_f64());

    let t = std::time::Instant::now();
    let graphe = Graphe::construire(&extrait);
    let (sommets, aretes) = graphe.taille();
    println!("graphe construit en {:.2} s — {sommets} sommets, {aretes} arêtes", t.elapsed().as_secs_f64());

    // Deux points pris aux deux bouts de l'extrait (Notre-Dame et l'Arc de
    // Triomphe, à peu près) pour un chemin qui traverse vraiment la ville.
    let depart = [2.3499, 48.8530];
    let arrivee = [2.2950, 48.8738];
    let t = std::time::Instant::now();
    let chemin = graphe.chemin(depart, arrivee);
    let duree = t.elapsed();
    match chemin {
        Some(pts) => {
            let mut longueur = 0.0;
            for f in pts.windows(2) {
                let lat_moy = (f[0][1] + f[1][1]).to_radians() / 2.0;
                let dx = (f[1][0] - f[0][0]).to_radians() * lat_moy.cos() * 6_371_000.0;
                let dy = (f[1][1] - f[0][1]).to_radians() * 6_371_000.0;
                longueur += (dx * dx + dy * dy).sqrt();
            }
            println!(
                "chemin Notre-Dame → Arc de Triomphe : {} points, {:.0} m, {:.3} s",
                pts.len(),
                longueur,
                duree.as_secs_f64()
            );
        }
        None => {
            println!("aucun chemin trouvé — {:.3} s", duree.as_secs_f64());
            let cd = graphe.taille_composante(depart);
            let ca = graphe.taille_composante(arrivee);
            println!("composante du départ : {cd:?} sommets, composante de l'arrivée : {ca:?} sommets (sur {sommets} au total)");
        }
    }
    Ok(())
}
