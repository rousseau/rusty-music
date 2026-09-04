// SPDX-License-Identifier: GPL-3.0-or-later
//! Mesure ce qu'un extrait OSM offre comme support à la bibliothèque.
//!
//!     cargo run --release -p rusty-music-osm --example mesure -- <fichier.osm.pbf>
//!
//! La question à laquelle il répond : le plan de Paris a-t-il de quoi loger
//! 27 000 morceaux, et sur combien de rues distinctes ?

use std::path::PathBuf;

use rusty_music_osm::{extraire, PARIS};

fn main() -> anyhow::Result<()> {
    let chemin: PathBuf = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage : mesure <fichier.osm.pbf>"))?
        .into();

    let depart = std::time::Instant::now();
    let extrait = extraire(&chemin, PARIS, Some("Paris"))?;
    let duree = depart.elapsed();
    let r = extrait.resume();

    match &extrait.frontiere {
        Some(f) => {
            let sommets: usize = f.anneaux.iter().map(|a| a.len()).sum();
            println!(
                "Paris — découpé sur la limite communale ({} anneau(x), {sommets} sommets), lu en {:.1} s\n",
                f.anneaux.len(),
                duree.as_secs_f64()
            );
        }
        None => println!("Paris — CADRE RECTANGULAIRE (frontière introuvable), lu en {:.1} s\n", duree.as_secs_f64()),
    }
    println!("  tronçons          {:>8}  ({:.0} km)", r.troncons, r.longueur_km);
    println!(
        "  dont nommés       {:>8}  ({:.0} km)",
        r.troncons_nommes, r.longueur_nommee_km
    );
    println!("  RUES DISTINCTES   {:>8}", r.rues_distinctes);
    println!("  ADRESSES          {:>8}", r.adresses);
    println!("  bâtiments         {:>8}", r.batis);
    println!("  plans d'eau       {:>8}", r.eaux);
    println!("  espaces verts     {:>8}", r.verts);
    println!("  toponymes place=* {:>8}", r.lieux);
    println!("  repères réels     {:>8}", r.points_remarquables);

    println!("\n  par classe :");
    for (classe, n, km) in &r.par_classe {
        println!("    {:<14} {:>7}  {:>7.0} km", classe.nom(), n, km);
    }

    if !extrait.points_remarquables.is_empty() {
        let mut par_genre: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for p in &extrait.points_remarquables {
            *par_genre.entry(p.genre.as_str()).or_default() += 1;
        }
        let mut par_genre: Vec<(&str, usize)> = par_genre.into_iter().collect();
        par_genre.sort_unstable_by_key(|(_, n)| std::cmp::Reverse(*n));
        println!("\n  repères réels, par genre :");
        for (genre, n) in &par_genre {
            println!("    {genre:<20} {n:>5}");
        }
    }

    // Ce qui décide de la faisabilité.
    const MORCEAUX: f64 = 27_042.0;
    let par_rue = MORCEAUX / r.rues_distinctes.max(1) as f64;
    let ecart_m = r.longueur_nommee_km * 1000.0 / MORCEAUX;
    println!("\n  Pour 27 042 morceaux :");
    println!("    {par_rue:.1} morceaux par rue distincte");
    println!("    {ecart_m:.1} m entre deux morceaux le long des rues nommées");
    println!(
        "    couverture par les adresses OSM : {:.0} %",
        100.0 * r.adresses as f64 / MORCEAUX
    );

    // La structure dont l'affectation a besoin : rue -> adresses.
    let mut par_rue: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut sans_rue = 0usize;
    for adresse in &extrait.adresses {
        match adresse.rue.as_deref() {
            Some(rue) => *par_rue.entry(rue).or_default() += 1,
            None => sans_rue += 1,
        }
    }
    let noms_de_rues: std::collections::HashSet<&str> =
        extrait.troncons.iter().filter_map(|t| t.nom.as_deref()).collect();
    let appariees = par_rue.keys().filter(|r| noms_de_rues.contains(**r)).count();
    println!("\n  adresses -> rue :");
    println!("    avec addr:street  {:>8} ({:.0} %)", extrait.adresses.len() - sans_rue,
             100.0 * (extrait.adresses.len() - sans_rue) as f64 / extrait.adresses.len() as f64);
    println!("    sans              {:>8}", sans_rue);
    println!("    noms de rue cités {:>8}", par_rue.len());
    println!("    dont retrouvés dans la géométrie {:>4} ({:.0} %)", appariees,
             100.0 * appariees as f64 / par_rue.len().max(1) as f64);
    let mut plus_fournies: Vec<(usize, &str)> = par_rue.iter().map(|(r, n)| (*n, *r)).collect();
    plus_fournies.sort_unstable_by(|a, b| b.cmp(a));
    println!("    rues les mieux dotées en adresses :");
    for (n, rue) in plus_fournies.iter().take(5) {
        println!("      {n:>5} adresses  {rue}");
    }

    let mut rues: Vec<(usize, &str)> = extrait
        .rues_par_nom()
        .into_iter()
        .map(|(nom, troncons)| {
            (
                troncons.iter().map(|t| t.longueur_m() as usize).sum::<usize>(),
                nom,
            )
        })
        .collect();
    rues.sort_unstable_by(|a, b| b.cmp(a));
    println!("\n  les dix plus longues rues (elles porteront les plus gros artistes) :");
    for (m, nom) in rues.iter().take(10) {
        println!("    {:>6} m  {nom}", m);
    }
    Ok(())
}
