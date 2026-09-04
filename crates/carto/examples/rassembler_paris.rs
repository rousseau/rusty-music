// SPDX-License-Identifier: GPL-3.0-or-later
//! Le plan de ville réel sur la bibliothèque réelle : combien d'adresses, de
//! tuiles, en combien de temps — et **la boucle de validation du rendu**. Sort
//! dans `$TMPDIR/rusty-music-paris/` : les tuiles en clair, les 5 styles
//! (un par palette), et `index.html` (aperçu MapLibre avec sélecteur de thème
//! et de niveau de rendu). Servir ce dossier en HTTP et ouvrir la page.
//!
//! ```
//! cargo run --release -p rusty-music-carto --example rassembler_paris -- \
//!     <bibliothèque.db> <ville-paris.db> [--styles-seulement]
//! python3 -m http.server -d "$TMPDIR/rusty-music-paris" 8099
//! ```
//!
//! `--styles-seulement` : ne réécrit que les 5 `style-*.json` (instantané),
//! saute la génération de tuiles si `carte.pmtiles` est déjà là — pour itérer
//! sur les couleurs de `palette.rs` sans attendre le pavage.

use std::collections::HashMap;
use std::path::Path;

use rusty_music_core::db::Library;

fn main() -> anyhow::Result<()> {
    let styles_seulement = std::env::args().any(|a| a == "--styles-seulement");
    let mut positionnels = std::env::args().skip(1).filter(|a| !a.starts_with("--"));
    let base = positionnels.next().unwrap_or_else(|| "rusty-music.db".into());
    let ville = positionnels.next().unwrap_or_else(|| "ville-paris.db".into());
    let modele = "clap-htsat-unfused-5f";

    let t = std::time::Instant::now();
    let lib = Library::open(Path::new(&base))?;
    let vue = lib.map_view(modele)?;
    let noms_famille: HashMap<i64, String> = lib
        .familles(modele)?
        .into_iter()
        .map(|(id, nom, _)| (id, nom))
        .collect();
    println!(
        "bibliothèque : {} morceaux positionnés, {} familles nommées, {:.2} s",
        vue.len(),
        noms_famille.len(),
        t.elapsed().as_secs_f64()
    );
    if vue.is_empty() {
        anyhow::bail!("aucun morceau sur la carte — lancer `analyser` puis `carte` d'abord");
    }

    let t = std::time::Instant::now();
    let extrait = rusty_music_osm::base::lire(Path::new(&ville))?;
    println!(
        "ville : {} tronçons, frontière {}, {:.2} s",
        extrait.troncons.len(),
        extrait.frontiere.is_some(),
        t.elapsed().as_secs_f64()
    );

    let repere = rusty_music_carto::affectation::Repere::centre_de(&extrait);
    let grille = rusty_music_carto::batiments::GrilleBatiments::nouvelle(&extrait, &repere);
    println!(
        "bâtiments : {} au total, {} éligibles (aire ≥ {} m²)",
        extrait.batis.len(),
        grille.tous().len(),
        rusty_music_carto::batiments::AIRE_MIN_M2,
    );

    let t = std::time::Instant::now();
    let r = rusty_music_carto::ville::rassembler(
        &extrait,
        &vue,
        &noms_famille,
        rusty_music_carto::ville::ESPACEMENT_PAR_DEFAUT,
        Some(rusty_music_carto::ville::ILE_DE_LA_CITE),
    );
    // Pas de `curiosites` : `style::couches_ville` ne les rend pas (pastilles
    // brunes sur un plan de ville, cf. plan « rendu par couche »).
    println!(
        "affectation : {:.2} s — {} adresses posées, {} sans adresse, {} repli quartier ({:.0} %), {} hors zone ({:.0} %), {} débordements, erreur quartiers {:.1} %",
        t.elapsed().as_secs_f64(),
        r.adresses_posees,
        r.morceaux_sans_adresse,
        r.repli_quartier,
        100.0 * r.repli_quartier as f64 / r.adresses_posees.max(1) as f64,
        r.hors_zone,
        100.0 * r.hors_zone as f64 / r.adresses_posees.max(1) as f64,
        r.debordements,
        100.0 * r.quartiers_erreur_relative,
    );
    println!(
        "             {} artistes ancrés aux monuments, {} bâtiments peuplés",
        r.artistes_ancres, r.batiments_peuples,
    );
    println!(
        "source : {} morceaux, {} familles, {} albums, {} tronçons réels, {} bâtiments, {} eaux, {} verts, frontière {}",
        r.source.morceaux.len(),
        r.source.familles.len(),
        r.source.albums.len(),
        r.source.troncons_reels.len(),
        r.source.batiments.len(),
        r.source.eaux.len(),
        r.source.verts.len(),
        r.source.frontiere.is_some(),
    );
    assert!(r.source.est_ville_reelle(), "la source doit basculer sur le rendu réel");

    let sortie = std::env::temp_dir().join("rusty-music-paris");
    std::fs::create_dir_all(&sortie)?;
    let chemin_tuiles = sortie.join("carte.pmtiles");
    let paliers = rusty_music_carto::tuiles::Paliers::ville();

    if styles_seulement && chemin_tuiles.is_file() {
        println!("tuiles : conservées ({})", chemin_tuiles.display());
    } else {
        std::fs::remove_file(&chemin_tuiles).ok();
        let t = std::time::Instant::now();
        // Tuiles en clair (`carte/{z}/{x}/{y}.mvt`) à côté de l'archive : c'est
        // ce que sert `index.html` dans un navigateur ordinaire.
        let rapport = rusty_music_carto::tuiles::ecrire_avec(
            &r.source,
            &paliers,
            &chemin_tuiles,
            Some(&sortie.join("carte")),
        )?;
        println!(
            "tuiles : {} tuiles, {:.1} Mo, {:.2} s — écrites dans {}",
            rapport.tuiles,
            rapport.octets as f64 / 1e6,
            t.elapsed().as_secs_f64(),
            chemin_tuiles.display(),
        );
        for (z, n, octets) in &rapport.par_zoom {
            println!("  zoom {z:>2} : {n:>6} tuiles, {:>8.1} Ko", *octets as f64 / 1e3);
        }
    }

    // Un style par palette : `style.json` (osm-clair) + `style-<id>.json`.
    for palette in rusty_music_carto::Palette::toutes() {
        let style = rusty_music_carto::style::construire(
            &r.source,
            &paliers,
            "tuiles://localhost",
            palette,
        );
        let nom = if palette.id == "osm-clair" {
            "style.json".to_string()
        } else {
            format!("style-{}.json", palette.id)
        };
        std::fs::write(sortie.join(&nom), serde_json::to_vec_pretty(&style)?)?;
        if palette.id == "osm-clair" {
            println!(
                "styles : {} couches, source « relief » {}",
                style["layers"].as_array().map(|l| l.len()).unwrap_or(0),
                if style["sources"].get("relief").is_some() { "présente (inattendu)" } else { "absente (attendu)" },
            );
        }
    }
    std::fs::write(sortie.join("index.html"), include_str!("apercu-ville.html"))?;

    println!("\n  python3 -m http.server -d {} 8099", sortie.display());
    println!("  http://localhost:8099/?style=style-encre.json&rendu=fond&zoom=13\n");

    Ok(())
}
