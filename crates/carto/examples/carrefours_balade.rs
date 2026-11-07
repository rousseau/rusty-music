// SPDX-License-Identifier: GPL-3.0-or-later
//! Les carrefours de la balade (`docs/plan-balade.md`, P2), mesurés sur la
//! bibliothèque : la marche de l'application (`balade::suivants`, la pente
//! `pente` à l'allure *dériver*, les voies montrées `proposer`), sans gestes, depuis 500
//! départs de 20 pas (graine fixe, mêmes départs que `continuite_balade`).
//!
//! Ce que ça dit : combien de pas débouchent sur un carrefour (plus d'une
//! voie), combien de voies le panneau montre, et si le choix en vaut la
//! peine — l'écart à l'oreille entre la voie la plus proche et la plus
//! lointaine, la part de carrefours où une voie mène à une autre famille.
//!
//! ```text
//! cargo run --release -p rusty-music-carto --example carrefours_balade -- \
//!     [rusty-music.db] [ville-paris.db] [positions.json]
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use rusty_music_analysis::alea::Alea;
use rusty_music_carto::accrochage::AccrochageVoirie;
use rusty_music_carto::balade::{
    deja_vu, pente, proposer, suivants, Allure, Contexte, EtatBalade, Memoire,
};
use rusty_music_carto::reseau_reel::Graphe;
use rusty_music_core::db::Library;

const MODELE: &str = "clap-htsat-unfused-5f";
const GRAINE: u64 = 0xBA1ADE;
const DEPARTS: usize = 500;
const LONGUEUR: usize = 20;

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let support = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/fm.rustymusic.desktop");
    let mut chemin = |defaut: &str| {
        args.next()
            .map(PathBuf::from)
            .unwrap_or_else(|| support.join(defaut))
    };
    let lib_path = chemin("rusty-music.db");
    let ville_path = chemin("ville-paris.db");
    let positions_path = chemin("tuiles/positions.json");

    let lib = Library::open(&lib_path)?;
    let memoire = Memoire::lire(&lib);
    let vue = lib.map_view(MODELE)?;
    let empreinte: HashMap<i64, Vec<f32>> = lib
        .embeddings(MODELE)?
        .into_iter()
        .map(|(id, v)| {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            (id, v.iter().map(|x| x / n).collect())
        })
        .collect();
    let famille: HashMap<i64, i64> = vue.iter().map(|p| (p.id, p.cluster)).collect();
    let distance = |a: i64, b: i64| -> f32 {
        match (empreinte.get(&a), empreinte.get(&b)) {
            (Some(x), Some(y)) => 1.0 - x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>(),
            _ => 2.0,
        }
    };

    let extrait = rusty_music_osm::base::lire(&ville_path)?;
    let reelles: HashMap<i64, [f32; 2]> =
        serde_json::from_str(&std::fs::read_to_string(&positions_path)?)?;
    let graphe = Graphe::construire(&extrait);
    let accrochage =
        AccrochageVoirie::construire(&graphe, reelles.iter().map(|(&id, &[x, y])| (id, x, y)));

    let mut candidats: Vec<i64> = accrochage
        .sommet_de
        .keys()
        .copied()
        .filter(|id| empreinte.contains_key(id))
        .collect();
    candidats.sort_unstable();
    let mut alea = Alea::depuis(GRAINE);
    let departs: Vec<i64> = (0..DEPARTS)
        .map(|_| candidats[alea.borne(candidats.len())])
        .collect();

    let (mut pas, mut carrefours, mut au_dela_de_trois) = (0usize, 0usize, 0usize);
    let mut voies_montrees: Vec<f64> = Vec::new();
    let mut voies_offertes: Vec<f64> = Vec::new();
    let (mut autre_famille, mut pente_proche) = (0usize, 0usize);
    let (mut d_proche, mut d_loin, mut ecart) = (Vec::new(), Vec::new(), Vec::new());
    let mut entre_carrefours: Vec<f64> = Vec::new();
    for &depart in &departs {
        let mut etat = EtatBalade::depuis(accrochage.sommet_de[&depart], depart);
        let mut courant = depart;
        let mut depuis_dernier = 0usize;
        for _ in 0..LONGUEUR {
            let branches = suivants(&etat, &graphe, &accrochage);
            let ctx = Contexte {
                courant,
                memoire: &memoire,
                distance: |id| distance(courant, id),
                famille: |id| famille.get(&id).copied(),
                energie: None,
                reference: None,
            };
            let Some(defaut) = pente(
                &etat,
                &branches,
                &graphe,
                &accrochage,
                Allure::Deriver,
                &ctx,
            ) else {
                break;
            };
            pas += 1;
            depuis_dernier += 1;
            if branches.len() > 1 {
                carrefours += 1;
                entre_carrefours.push(depuis_dernier as f64);
                depuis_dernier = 0;
                au_dela_de_trois += usize::from(branches.len() > 3);
                voies_offertes.push(branches.len() as f64);
                let montrees = proposer(
                    &branches,
                    defaut,
                    |id| distance(courant, id),
                    |id| famille.get(&id).copied(),
                    famille.get(&courant).copied(),
                    |b| deja_vu(&etat, &graphe, b),
                );
                voies_montrees.push(montrees.len() as f64);
                let d: Vec<f64> = montrees
                    .iter()
                    .map(|&i| distance(courant, branches[i].morceau) as f64)
                    .collect();
                let (min, max) = d
                    .iter()
                    .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
                d_proche.push(min);
                d_loin.push(max);
                ecart.push(max - min);
                pente_proche += usize::from(d[0] <= min);
                autre_famille += usize::from(
                    montrees
                        .iter()
                        .any(|&i| famille.get(&branches[i].morceau) != famille.get(&courant)),
                );
            }
            if branches.len() > 1 {
                etat.carrefours += 1;
            }
            let b = &branches[defaut];
            etat.avancer(b);
            courant = b.morceau;
        }
    }

    let part = |n: usize, d: usize| 100.0 * n as f64 / d.max(1) as f64;
    println!("## Carrefours de la balade (P2), {DEPARTS} départs × {LONGUEUR} pas\n");
    println!("| mesure | valeur |\n|---|---|");
    println!("| pas joués | {pas} |");
    println!(
        "| pas qui débouchent sur un carrefour | {carrefours} ({:.1} %) |",
        part(carrefours, pas)
    );
    println!(
        "| pas entre deux carrefours, médiane (p90) | {:.0} ({:.0}) |",
        quantile(&mut entre_carrefours.clone(), 0.5),
        quantile(&mut entre_carrefours, 0.9)
    );
    println!(
        "| voies offertes, médiane (max) | {:.0} ({:.0}) |",
        quantile(&mut voies_offertes.clone(), 0.5),
        quantile(&mut voies_offertes, 1.0)
    );
    println!(
        "| carrefours à plus de trois voies (sélection) | {:.1} % |",
        part(au_dela_de_trois, carrefours)
    );
    println!(
        "| voies montrées, moyenne | {:.2} |",
        voies_montrees.iter().sum::<f64>() / voies_montrees.len().max(1) as f64
    );
    println!(
        "| la pente est la plus proche à l'oreille | {:.1} % |",
        part(pente_proche, carrefours)
    );
    println!(
        "| une voie mène à une autre famille | {:.1} % |",
        part(autre_famille, carrefours)
    );
    println!(
        "| distance de la voie la plus proche, médiane | {:.3} |",
        quantile(&mut d_proche, 0.5)
    );
    println!(
        "| distance de la voie la plus lointaine, médiane | {:.3} |",
        quantile(&mut d_loin, 0.5)
    );
    println!(
        "| écart proche ↔ ailleurs, médiane (p10) | {:.3} ({:.3}) |",
        quantile(&mut ecart.clone(), 0.5),
        quantile(&mut ecart, 0.1)
    );
    Ok(())
}
