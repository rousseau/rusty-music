// SPDX-License-Identifier: GPL-3.0-or-later
//! Toute la carte refaite sur une copie des données, avec le pas de t-SNE de
//! l'application ou un pas plus grand (V10) : nuage, familles, plan de Paris.
//!
//! `projection.rs` (V3) a montré qu'un pas seize fois plus grand double la
//! conservation des voisins à temps égal. La projection nourrit aussi le plan
//! de Paris (affinité de style de `croissance`) : changer le pas le redessine.
//! Cet exemple fait le redessin **hors de l'application**, sur un dossier de
//! données copié, pour le regarder avant de décider :
//!
//!   1. `passe::projeter_tout(…, Refaite)` : projection et familles, comme le
//!      bouton « Recalculer la carte » ;
//!   2. si `facteur ≠ 1`, la projection seule est refaite avec le pas
//!      `max(n / 48, 50) × facteur` (le pas automatique de `bhtsne` pour une
//!      exagération de 12, multiplié), mêmes perplexité et époques, puis
//!      cadrée ; les familles, calculées sur les empreintes, restent ;
//!   3. les mesures du nuage : conservation des 12 voisins (512 d → 2 d),
//!      pureté de genre exact et de famille des voisins projetés ;
//!   4. le plan de Paris, comme `engendrer_tuiles` de l'application :
//!      `ville::rassembler`, `carte.pmtiles`, un `style-<palette>.json` par
//!      palette, `positions.json` ; le relief musical (`dem-*`, `courbes-*`),
//!      calculé d'après les anciennes adresses, est retiré, et celui de
//!      l'énergie refait comme `relief_musical` de l'application.
//!
//! Écrit dans `<dossier>` (la copie : `rusty-music.db`, `ville-paris.db`,
//! `tuiles/`), jamais ailleurs.
//!
//!   cargo run --release -p rusty-music-carto --example carte_avant_apres -- <dossier> <facteur>

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use rusty_music_analysis::chemin::{Empreinte, Graphe, K_VOISINS};
use rusty_music_analysis::passe::{self, Projection, MODELE};
use rusty_music_analysis::projection::{cadrer, normaliser, Point};
use rusty_music_carto::altitude::{self, Grandeur};
use rusty_music_carto::{style, tuiles, ville, Palette};
use rusty_music_core::Library;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dossier = PathBuf::from(args.next().expect("<dossier> <facteur>"));
    let facteur: f32 = args.next().map_or(Ok(1.0), |f| f.parse())?;
    let lib = Library::open(&dossier.join("rusty-music.db"))?;
    let fils = std::thread::available_parallelism().map_or(1, |n| n.get());

    // --- 1. La passe de l'application -----------------------------------------
    let familles_avant: HashMap<i64, i64> = lib
        .map_points(MODELE)?
        .into_iter()
        .map(|(id, _, _, f)| (id, f))
        .collect();
    let t = Instant::now();
    let r = passe::projeter_tout(&lib, None, Projection::Refaite)?;
    let t_passe = t.elapsed().as_secs_f64();
    println!(
        "projeter_tout : {:?}, {} empreintes, {} familles, {t_passe:.1} s",
        r.projection, r.empreintes, r.familles
    );

    let empreintes: Vec<Empreinte> = lib.embeddings(MODELE)?;
    let n = empreintes.len();
    let points = lib.map_points(MODELE)?;
    let famille: HashMap<i64, i64> = points.iter().map(|p| (p.0, p.3)).collect();
    let changees = famille
        .iter()
        .filter(|(id, f)| familles_avant.get(id).is_some_and(|g| g != *f))
        .count();
    println!("familles changées par la passe : {changees} morceaux sur {n}");

    // --- 2. Le pas plus grand -------------------------------------------------
    let params = lib.parametres_carte()?;
    let mut t_projection = None;
    if (facteur - 1.0).abs() > f32::EPSILON {
        let unites: Vec<Vec<f32>> = empreintes.iter().map(|(_, v)| normaliser(v)).collect();
        let perplexite = params.perplexite.min((n as f32 - 1.0) / 3.0).max(2.0);
        let pas = (n as f32 / 48.0).max(50.0) * facteur;
        let t = Instant::now();
        let mut tsne: bhtsne::tSNE<f32, Vec<f32>, 2> = bhtsne::tSNE::new(&unites);
        tsne.perplexity(perplexite)
            .epochs(params.epoques)
            .learning_rate(pas)
            .barnes_hut(0.5, |a, b| {
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| (x - y) * (x - y))
                    .sum::<f32>()
                    .sqrt()
            });
        let mut p: Vec<Point> = tsne
            .embedding()
            .chunks(2)
            .map(|c| Point { x: c[0], y: c[1] })
            .collect();
        cadrer(&mut p);
        t_projection = Some(t.elapsed().as_secs_f64());
        let maj: Vec<(i64, f32, f32, i64)> = empreintes
            .iter()
            .zip(&p)
            .map(|((id, _), q)| (*id, q.x, q.y, famille[id]))
            .collect();
        lib.update_map(MODELE, &maj)?;
        println!(
            "projection au pas {pas:.0} (×{facteur}) : {:.1} s",
            t_projection.unwrap()
        );
    }

    // --- 3. Le nuage mesuré ---------------------------------------------------
    let points = lib.map_points(MODELE)?;
    let genres = lib.genres_resolus(MODELE)?;
    let xy: HashMap<i64, Vec<f32>> = points.iter().map(|p| (p.0, vec![p.1, p.2])).collect();
    let plats: Vec<Empreinte> = empreintes
        .iter()
        .map(|(id, _)| (*id, xy[id].clone()))
        .collect();
    let g512 = Graphe::construire(&empreintes, K_VOISINS, fils);
    let g2d = Graphe::construire(&plats, K_VOISINS, fils);
    let mut communs = 0usize;
    let (mut pg, mut mg, mut pf, mut mf) = (0usize, 0usize, 0usize, 0usize);
    for &id in g2d.identifiants() {
        let a: HashSet<i64> = g512.voisins(id, K_VOISINS).into_iter().collect();
        for v in g2d.voisins(id, K_VOISINS) {
            communs += usize::from(a.contains(&v));
            if let (Some(a), Some(b)) = (genres.get(&id), genres.get(&v)) {
                pg += 1;
                mg += usize::from(a == b);
            }
            let (a, b) = (famille[&id], famille[&v]);
            if a >= 0 && b >= 0 {
                pf += 1;
                mf += usize::from(a == b);
            }
        }
    }
    println!(
        "MESURE conservation {:.1} % · pureté genre {:.1} % · pureté famille {:.1} % · projection {:.1} s",
        communs as f64 / (n * K_VOISINS) as f64 * 100.0,
        mg as f64 / pg.max(1) as f64 * 100.0,
        mf as f64 / pf.max(1) as f64 * 100.0,
        t_projection.unwrap_or(t_passe)
    );

    // --- 4. Le plan de Paris --------------------------------------------------
    let t = Instant::now();
    let extrait = rusty_music_osm::base::lire(&dossier.join("ville-paris.db"))?;
    let vue = lib.map_view(MODELE)?;
    let noms: HashMap<i64, String> = lib
        .familles(MODELE)?
        .into_iter()
        .map(|(id, nom, _)| (id, nom))
        .collect();
    let vecteurs: HashMap<i64, Vec<f32>> = empreintes.into_iter().collect();
    let r = ville::rassembler(
        &extrait,
        &vue,
        &noms,
        Some(ville::ILE_DE_LA_CITE),
        Some(&vecteurs),
    );
    let paliers = tuiles::Paliers::ville();
    let dossier_tuiles = dossier.join("tuiles");
    std::fs::create_dir_all(&dossier_tuiles)?;
    tuiles::ecrire(&r.source, &paliers, &dossier_tuiles.join("carte.pmtiles"))?;
    // `tuiles::base()` de l'application : `maplibregl.addProtocol` intercepte
    // tout ce qui commence par `tuiles://`.
    for palette in Palette::toutes() {
        let s = style::construire(&r.source, &paliers, "tuiles://tuiles", palette);
        let nom = if palette.id == "osm-clair" {
            "style.json".to_string()
        } else {
            format!("style-{}.json", palette.id)
        };
        std::fs::write(dossier_tuiles.join(nom), serde_json::to_vec_pretty(&s)?)?;
    }
    let positions: HashMap<i64, [f32; 2]> = r
        .source
        .morceaux
        .iter()
        .map(|m| (m.id, [m.x, m.y]))
        .collect();
    std::fs::write(
        dossier_tuiles.join("positions.json"),
        serde_json::to_vec(&positions)?,
    )?;
    // Ce que fait `engendrer_tuiles` s'arrête ici (V16 : sa durée).
    println!(
        "plan de Paris sans relief (comme engendrer_tuiles) : {:.1} s",
        t.elapsed().as_secs_f64()
    );
    for entree in std::fs::read_dir(&dossier_tuiles)? {
        let p = entree?.path();
        let nom = p.file_name().unwrap_or_default().to_string_lossy();
        if nom.starts_with("dem-") || nom.starts_with("courbes-") {
            std::fs::remove_file(&p)?;
        }
    }
    // Le relief de l'énergie (la grandeur par défaut), comme `relief_musical`
    // de l'application, sur les nouvelles adresses.
    let energie: Vec<Option<f32>> = vue.iter().map(|p| p.energy).collect();
    let points_relief: Vec<([f64; 2], f32)> = vue
        .iter()
        .zip(altitude::normaliser(Grandeur::Energie, &energie))
        .filter_map(|(p, v)| Some((positions.get(&p.id)?, v?)))
        .map(|(q, v)| ([q[0] as f64, q[1] as f64], v))
        .collect();
    if let Some(relief) = altitude::Relief::calculer(&points_relief) {
        let courbes = relief.courbes();
        relief.ecrire_dem(&dossier_tuiles.join("dem-energie.pmtiles"))?;
        relief.ecrire_courbes(&courbes, &dossier_tuiles.join("courbes-energie.pmtiles"))?;
    }
    println!(
        "plan de Paris : {} adresses, {} sans adresse, {:.1} s",
        r.adresses_posees,
        r.morceaux_sans_adresse,
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
