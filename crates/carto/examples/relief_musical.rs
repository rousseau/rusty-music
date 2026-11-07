// SPDX-License-Identifier: GPL-3.0-or-later
//! P6 de la balade (`docs/plan-balade.md`) : le relief musical sur la vraie
//! bibliothèque, pour chaque grandeur — temps de calcul, courbes, tuiles,
//! poids des archives, et répartition des morceaux dans les dix bandes.
//!
//! `cargo run --release -p rusty-music-carto --example relief_musical -- [rusty-music.db] [positions.json] [dossier-de-sortie]`

use std::collections::HashMap;
use std::path::PathBuf;

use rusty_music_carto::altitude::{self, Grandeur, Relief, NIVEAUX};
use rusty_music_core::db::Library;

const MODELE: &str = "clap-htsat-unfused-5f";

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let support = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/fm.rustymusic.desktop");
    let lib_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| support.join("rusty-music.db"));
    let positions_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| support.join("tuiles/positions.json"));
    let sortie = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("relief-musical"));
    std::fs::create_dir_all(&sortie)?;

    let lib = Library::open(&lib_path)?;
    let vue = lib.map_view(MODELE)?;
    let ecoutes = lib.nombre_ecoutes()?;
    let positions: HashMap<i64, [f32; 2]> =
        serde_json::from_str(&std::fs::read_to_string(&positions_path)?)?;

    println!("| grandeur | morceaux | calcul | courbes | tuiles dem / courbes | archives | morceaux par bande (0 → 9) |");
    println!("|---|---:|---:|---:|---:|---:|---|");
    for g in Grandeur::TOUTES {
        let brutes: Vec<Option<f32>> = match g {
            Grandeur::Energie => vue.iter().map(|p| p.energy).collect(),
            Grandeur::Tempo => vue.iter().map(|p| p.bpm).collect(),
            Grandeur::Popularite => vue.iter().map(|p| p.popularite.map(|x| x as f32)).collect(),
            Grandeur::Familiarite => vue
                .iter()
                .map(|p| Some(ecoutes.get(&p.id).copied().unwrap_or(0) as f32))
                .collect(),
        };
        let points: Vec<([f64; 2], f32)> = vue
            .iter()
            .zip(altitude::normaliser(g, &brutes))
            .filter_map(|(p, v)| {
                let q = positions.get(&p.id)?;
                Some(([q[0] as f64, q[1] as f64], v?))
            })
            .collect();
        let t = std::time::Instant::now();
        let Some(relief) = Relief::calculer(&points) else {
            println!("| {} | 0 | — | — | — | — | — |", g.id());
            continue;
        };
        let courbes = relief.courbes();
        let calcul = t.elapsed();
        let dem = sortie.join(format!("dem-{}.pmtiles", g.id()));
        let crb = sortie.join(format!("courbes-{}.pmtiles", g.id()));
        let t2 = std::time::Instant::now();
        let n_dem = relief.ecrire_dem(&dem)?;
        let n_crb = relief.ecrire_courbes(&courbes, &crb)?;
        let ecriture = t2.elapsed();
        let mut bandes = [0usize; NIVEAUX as usize];
        for (p, _) in &points {
            bandes[relief.bande(*p) as usize] += 1;
        }
        let octets = std::fs::metadata(&dem)?.len() + std::fs::metadata(&crb)?.len();
        println!(
            "| {} | {} | {:.2} s + {:.2} s | {} | {} / {} | {:.1} Mo | {} |",
            g.id(),
            points.len(),
            calcul.as_secs_f64(),
            ecriture.as_secs_f64(),
            courbes.len(),
            n_dem,
            n_crb,
            octets as f64 / 1e6,
            bandes
                .iter()
                .map(|n| format!("{:.0} %", 100.0 * *n as f64 / points.len() as f64))
                .collect::<Vec<_>>()
                .join(" · ")
        );
        if g == Grandeur::Energie {
            // Le rail de la molette : combien de morceaux bordent une courbe.
            let morceaux: Vec<(i64, [f64; 2])> = vue
                .iter()
                .filter_map(|p| {
                    positions
                        .get(&p.id)
                        .map(|q| (p.id, [q[0] as f64, q[1] as f64]))
                })
                .collect();
            let mut le_long: Vec<usize> = courbes
                .iter()
                .map(|c| altitude::le_long(c, &morceaux, 40.0).len())
                .collect();
            le_long.sort_unstable();
            let mut ouverts: Vec<usize> = courbes
                .iter()
                .map(|c| altitude::ouvrir(&relief, c, &morceaux).len())
                .collect();
            ouverts.sort_unstable();
            let med = |v: &[usize]| v.get(v.len() / 2).copied().unwrap_or(0);
            eprintln!(
                "énergie : riverains par courbe (médiane {}, max {}), morceaux à l'ouverture (médiane {}, max {})",
                med(&le_long),
                le_long.last().copied().unwrap_or(0),
                med(&ouverts),
                ouverts.last().copied().unwrap_or(0)
            );
        }
    }
    Ok(())
}
