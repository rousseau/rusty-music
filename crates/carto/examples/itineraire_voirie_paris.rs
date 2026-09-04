//! L'itinéraire du mode Explorer, routé sur la vraie voirie : pour chaque
//! profil, la longueur du trajet, les rues traversées et la taille du couloir
//! (les sommets d'où proviendront les morceaux de la playlist).
//!
//! Sert à régler à l'œil les tables de `cout_itineraire` et le rayon de
//! couloir de `reseau_reel::Graphe::couloir` avant de câbler l'interface.
//!
//! `cargo run --release -p rusty-music-carto --example itineraire_voirie_paris -- <ville-paris.db>`

use std::collections::HashSet;
use std::path::Path;

use rusty_music_carto::cout_itineraire::{friction_itineraire, ProfilVoirie, ProximiteAgrement};
use rusty_music_carto::reseau_reel::Graphe;

fn longueur_m(pts: &[[f64; 2]]) -> f64 {
    pts.windows(2)
        .map(|f| {
            let lat_moy = (f[0][1] + f[1][1]).to_radians() / 2.0;
            let dx = (f[1][0] - f[0][0]).to_radians() * lat_moy.cos() * 6_371_000.0;
            let dy = (f[1][1] - f[0][1]).to_radians() * 6_371_000.0;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

fn main() -> anyhow::Result<()> {
    let ville = std::env::args().nth(1).unwrap_or_else(|| "ville-paris.db".into());
    let extrait = rusty_music_osm::base::lire(Path::new(&ville))?;
    println!("lecture : {} tronçons", extrait.troncons.len());

    let t = std::time::Instant::now();
    let agrement = ProximiteAgrement::nouvelle(&extrait, 120.0);
    println!("proximité d'agrément en {:.2} s", t.elapsed().as_secs_f64());

    // Notre-Dame → Arc de Triomphe, comme `reseau_reel_paris`.
    let depart = [2.3499, 48.8530];
    let arrivee = [2.2950, 48.8738];
    let vol = longueur_m(&[depart, arrivee]);
    println!("\ndépart {depart:?} → arrivée {arrivee:?} ({vol:.0} m à vol d'oiseau)\n");

    for profil in [ProfilVoirie::ParLeConnu, ProfilVoirie::Redecouvrir, ProfilVoirie::Panoramique] {
        let t = std::time::Instant::now();
        let graphe =
            Graphe::construire_pondere(&extrait, friction_itineraire(profil, Some(&agrement)));
        let construction = t.elapsed();

        let t = std::time::Instant::now();
        let Some((sommets, cout)) = graphe.chemin_sommets(depart, arrivee) else {
            println!("{profil:?} : aucun chemin");
            continue;
        };
        let calcul = t.elapsed();
        let pts: Vec<[f64; 2]> = sommets.iter().map(|&s| graphe.point(s)).collect();
        let rues = graphe.troncons_traverses(&sommets);
        let couloir = graphe.couloir(&sommets, 25.0);
        let noms: HashSet<&str> = rues
            .iter()
            .filter_map(|id| extrait.troncons.iter().find(|t| t.id == *id))
            .filter_map(|t| t.nom.as_deref())
            .collect();

        // Répartition des mètres parcourus par classe de voie — le vrai test du
        // profil : « par le connu » doit être surtout primaire/secondaire,
        // « redécouvrir » surtout résidentiel/piéton, ni l'un ni l'autre sur
        // l'autoroute.
        let classe_de: std::collections::HashMap<i64, rusty_music_osm::Classe> =
            extrait.troncons.iter().map(|t| (t.id, t.classe)).collect();
        let mut metres: std::collections::HashMap<rusty_music_osm::Classe, f64> =
            std::collections::HashMap::new();
        for paire in sommets.windows(2) {
            let seg = longueur_m(&[graphe.point(paire[0]), graphe.point(paire[1])]);
            // La classe de l'arête = celle d'un tronçon commun aux deux sommets.
            if let Some(id) = rues.iter().find(|id| {
                extrait
                    .troncons
                    .iter()
                    .find(|t| t.id == **id)
                    .map(|t| t.points.contains(&graphe.point(paire[0])) && t.points.contains(&graphe.point(paire[1])))
                    .unwrap_or(false)
            }) {
                if let Some(c) = classe_de.get(id) {
                    *metres.entry(*c).or_default() += seg;
                }
            }
        }
        let total: f64 = metres.values().sum::<f64>().max(1.0);
        let mut part: Vec<(rusty_music_osm::Classe, f64)> =
            metres.iter().map(|(c, m)| (*c, m / total * 100.0)).collect();
        part.sort_by(|a, b| b.1.total_cmp(&a.1));
        let repartition: String = part
            .iter()
            .map(|(c, p)| format!("{} {p:.0}%", c.nom()))
            .collect::<Vec<_>>()
            .join(", ");

        println!(
            "{profil:?} : {:.0} m ({:.1}× le vol d'oiseau), coût {cout}, \
             {} tronçons / {} rues nommées, couloir {} sommets \
             — construit {:.2} s, routé {:.3} s\n    voies : {repartition}",
            longueur_m(&pts),
            longueur_m(&pts) / vol,
            rues.len(),
            noms.len(),
            couloir.len(),
            construction.as_secs_f64(),
            calcul.as_secs_f64(),
        );
    }

    Ok(())
}
