// SPDX-License-Identifier: GPL-3.0-or-later
//! Ce que coûte le réseau de circulation, sur la bibliothèque réelle.
//!
//! `cargo run --release -p rusty-music-analysis --example cout_reseau -- <base>`

use std::collections::HashMap;

use rusty_music_analysis::reseau::{Echelle, Options, Parametres, Profil, Reseau};
use rusty_music_core::{db::Library, density};

fn main() -> anyhow::Result<()> {
    let base = std::env::args().nth(1).unwrap_or_else(|| "rusty-music.db".into());
    let k: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(rusty_music_analysis::chemin::K_VOISINS);
    let lib = Library::open(std::path::Path::new(&base))?;
    let modele = rusty_music_analysis::passe::MODELE;

    let t = std::time::Instant::now();
    let empreintes = lib.embeddings(modele)?;
    let vue = lib.map_view(modele)?;
    println!(
        "{} empreintes et {} points lus en {:.2} s",
        empreintes.len(),
        vue.len(),
        t.elapsed().as_secs_f64()
    );

    // La popularité dont on dispose : le nombre de morceaux par artiste.
    let mut par_artiste: HashMap<String, u32> = HashMap::new();
    for p in &vue {
        *par_artiste
            .entry(p.artist.clone().unwrap_or_default())
            .or_default() += 1;
    }
    let mut index: HashMap<&str, u32> = HashMap::new();
    for (i, nom) in par_artiste.keys().enumerate() {
        index.insert(nom.as_str(), i as u32);
    }

    let morceaux: Vec<rusty_music_analysis::reseau::Morceau> = vue
        .iter()
        .map(|p| {
            let artiste = p.artist.clone().unwrap_or_default();
            rusty_music_analysis::reseau::Morceau {
                id: p.id,
                duree_ms: p.duration_ms.unwrap_or(0).max(0) as u64,
                artiste: index[artiste.as_str()],
                famille: p.cluster,
                x: p.x,
                y: p.y,
                morceaux_de_lartiste: par_artiste[&artiste],
            }
        })
        .collect();

    let points: Vec<(i64, f32, f32, i64)> = lib.map_points(modele)?;
    let mut parametres = lib.parametres_carte()?.parametres_densite();
    parametres.noyau = 0.05;
    let champ = density::champ_global(&points, &parametres);

    let echelle = match std::env::args().nth(3).as_deref() {
        Some("morceaux") => Echelle::Morceaux,
        _ => Echelle::Artistes,
    };
    let p = Parametres {
        k,
        fils: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8),
        echelle,
        ..Default::default()
    };
    println!("\nconstruction (k = {k}, {} pôles, centralité sur {echelle:?})…", p.poles);
    let (reseau, r) = Reseau::construire_mesure(empreintes, &morceaux, &champ, parametres.resolution, &p);

    println!("  graphe des voisins : {:>8.2} s", r.ms_graphe as f64 / 1000.0);
    println!("  centralité (Brandes) : {:>6.2} s", r.ms_centralite as f64 / 1000.0);
    println!("  arbre de crête     : {:>8.2} s", r.ms_crete as f64 / 1000.0);
    println!("  total              : {:>8.2} s", r.ms_total as f64 / 1000.0);
    println!("\n  {} morceaux, {} arêtes, {} refuges isolés", r.morceaux, r.aretes, r.refuges);
    for (nom, n) in &r.par_classe {
        println!("  {nom:<12} {n:>7}  {:>5.1} %", 100.0 * *n as f64 / r.aretes as f64);
    }

    // Les quatre profils, entre deux morceaux éloignés.
    let ids = reseau.identifiants();
    let (a, b) = (ids[0], ids[ids.len() / 2]);
    println!("\nprofils, de {a} à {b} :");
    for (nom, profil) in [
        ("autoroute", Profil::Autoroute),
        ("sentier", Profil::Sentier),
        ("panoramique", Profil::Panoramique),
    ] {
        let t = std::time::Instant::now();
        match reseau.itineraires(&Options::nouveau(a, profil).vers(b)) {
            Ok(v) => {
                let i = &v[0];
                println!(
                    "  {nom:<12} {:>3} morceaux  {:>5.1} min  distance {:>5.2}  \
                     popularité moy. {:.3}  en {:>6.1} ms",
                    i.morceaux.len(),
                    i.duree_ms as f64 / 60_000.0,
                    i.distance_sonique,
                    i.popularite.iter().sum::<f32>() / i.popularite.len() as f32,
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
            Err(e) => println!("  {nom:<12} {e}"),
        }
    }

    // La contrainte prioritaire.
    for minutes in [20u64, 40, 60] {
        let t = std::time::Instant::now();
        let o = Options::nouveau(a, Profil::Sentier).duree(minutes * 60_000);
        match reseau.itineraires(&o) {
            Ok(v) => println!(
                "\n« {minutes} minutes » → {} morceaux, {:.1} min réelles, \
                 distance {:.2}, en {:.0} ms",
                v[0].morceaux.len(),
                v[0].duree_ms as f64 / 60_000.0,
                v[0].distance_sonique,
                t.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => println!("\n« {minutes} minutes » → {e} ({:.0} ms)", t.elapsed().as_secs_f64() * 1000.0),
        }
    }

    // Trois itinéraires, comme Google Maps.
    let t = std::time::Instant::now();
    match reseau.itineraires(&Options::nouveau(a, Profil::Autoroute).vers(b).alternatives(3)) {
        Ok(v) => {
            println!("\n{} itinéraires alternatifs en {:.0} ms :", v.len(), t.elapsed().as_secs_f64() * 1000.0);
            for (n, i) in v.iter().enumerate() {
                println!(
                    "  {}. {:>3} morceaux, {:>5.1} min, distance {:.2}",
                    n + 1,
                    i.morceaux.len(),
                    i.duree_ms as f64 / 60_000.0,
                    i.distance_sonique
                );
            }
        }
        Err(e) => println!("\nalternatives : {e}"),
    }
    Ok(())
}
