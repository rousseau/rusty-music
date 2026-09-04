// SPDX-License-Identifier: GPL-3.0-or-later
//! Le peuplement sur la bibliothèque réelle : combien d'établissements, de
//! quels rangs, en combien de temps.
//!
//! `cargo run --release -p rusty-music-carto --example cout_peuplement -- <base>`

use std::collections::HashMap;

use rusty_music_carto::peuplement::{self, Arrivant, Parametres, Rang};
use rusty_music_core::db::Library;

fn main() -> anyhow::Result<()> {
    let base = std::env::args().nth(1).unwrap_or_else(|| "rusty-music.db".into());
    let seuil: f32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(0.62);
    let lib = Library::open(std::path::Path::new(&base))?;
    let modele = "clap-htsat-unfused-5f";

    let t = std::time::Instant::now();
    let ordre = lib.ordre_darrivee()?;
    let vue = lib.map_view(modele)?;
    let empreintes: HashMap<i64, Vec<f32>> = lib.embeddings(modele)?.into_iter().collect();
    let par_id: HashMap<i64, &rusty_music_core::db::MapPoint> =
        vue.iter().map(|p| (p.id, p)).collect();
    println!("lecture : {:.2} s", t.elapsed().as_secs_f64());

    // L'ordre décide ; les morceaux hors carte ne peuvent pas s'installer.
    let arrivants: Vec<Arrivant> = ordre
        .iter()
        .filter_map(|a| {
            let p = par_id.get(&a.track_id)?;
            Some(Arrivant {
                track_id: a.track_id,
                x: p.x,
                y: p.y,
                empreinte: empreintes.get(&a.track_id).cloned().unwrap_or_default(),
                famille: p.cluster,
                date: a.date,
                artiste: p.artist.clone().unwrap_or_default(),
            })
        })
        .collect();
    println!("{} arrivants sur {} morceaux", arrivants.len(), ordre.len());

    let rayon: f32 = std::env::args().nth(3).and_then(|v| v.parse().ok()).unwrap_or(0.012);
    let p = Parametres {
        seuil_affinite: seuil,
        rayon_base: rayon,
        // Le bâti garde son cinquième du bassin.
        pas_parcelle: rayon / 5.0,
        ..Default::default()
    };
    let r = peuplement::peupler(&arrivants, &p);
    let rap = &r.rapport;

    println!("\nseuil {seuil}, rayon de base {rayon} — {:.2} s", rap.ms as f64 / 1000.0);
    println!("  {} habitants, {} établissements, {} îles", rap.habitants, rap.etablissements, rap.iles);
    println!("  niveau de la mer : {:.4}", rap.niveau_mer);
    println!("  plus grand établissement : {} habitants", rap.plus_grand);
    for (nom, n) in &rap.par_rang {
        println!("  {nom:<12} {n:>6}  {:>5.1} %", 100.0 * *n as f64 / rap.etablissements as f64);
    }

    // Les critères d'acceptation annoncés dans le document de conception.
    let cherche = |n: &str| rap.par_rang.iter().find(|(x, _)| x == n).map(|(_, c)| *c).unwrap_or(0);
    let metropoles = cherche("métropole");
    let fermes = cherche("ferme");
    let mut tailles: Vec<u32> = r.etablissements.iter().map(|e| e.population).collect();
    tailles.sort_unstable();
    let mediane = tailles.get(tailles.len() / 2).copied().unwrap_or(0);
    println!("\ncritères d'acceptation :");
    let dire = |nom: &str, ok: bool, valeur: String| {
        println!("  {} {nom} : {valeur}", if ok { "✓" } else { "✗" });
    };
    dire("5 à 15 métropoles", (5..=15).contains(&metropoles), metropoles.to_string());
    dire("population médiane 4-8", (4..=8).contains(&mediane), mediane.to_string());
    dire(
        "fermes isolées < 15 %",
        (fermes as f64) < 0.15 * rap.etablissements as f64,
        format!("{:.1} %", 100.0 * fermes as f64 / rap.etablissements as f64),
    );
    dire(
        "plus gros < 10 % de la bibliothèque",
        (rap.plus_grand as f64) < 0.10 * rap.habitants as f64,
        format!("{:.1} %", 100.0 * rap.plus_grand as f64 / rap.habitants as f64),
    );

    // Un aperçu des plus grandes villes.
    let mut grands: Vec<_> = r.etablissements.iter().collect();
    grands.sort_by_key(|e| std::cmp::Reverse(e.population));
    println!("\nles plus grandes :");
    for e in grands.iter().take(8) {
        println!(
            "  {:<12} {:>5} habitants  fondée en {}  {}",
            Rang::depuis_population(e.population).nom(),
            e.population,
            e.fondation_date / 10_000,
            e.nom
        );
    }
    Ok(())
}
