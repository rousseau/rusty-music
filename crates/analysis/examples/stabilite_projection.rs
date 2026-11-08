// SPDX-License-Identifier: GPL-3.0-or-later
//! Le nuage reste-t-il en place quand la bibliothèque grandit ? (D5 de
//! `docs/synthese-novembre-2026.md`, O8.)
//!
//! On projette la bibliothèque privée de 5 % de ses morceaux (tirés avec une
//! graine fixe), puis on la reprojette entière :
//!
//!   - **zéro** : depuis zéro, comme `passe::projeter_tout` aujourd'hui ;
//!   - **suite** : depuis les coordonnées précédentes, les nouveaux posés au
//!     barycentre de leurs 12 plus proches voisins CLAP, sans exagération
//!     précoce (t-SNE reprend un dessin déjà convergé) ;
//!   - **suite courte** : la même, en 250 époques au lieu de toutes ;
//!   - **la passe** : ce que fait `passe::projeter_tout`, depuis les
//!     coordonnées cadrées (`projection::reprendre`, [`EPOQUES_REPRISE`]) ;
//!   - **courte ×¼, ×4** : la suite courte depuis un départ à une autre échelle ;
//!   - **amorce** : les coordonnées précédentes ramenées à un écart-type de
//!     1e-4, puis le calendrier complet, exagération comprise (l'amorçage
//!     d'openTSNE : garder la forme d'ensemble, laisser tout le reste bouger).
//!
//! `bhtsne` 0.7 accepte des coordonnées de départ (`initial_embedding`) mais
//! n'optimise pas un sous-ensemble seul : c'est toute la projection qui
//! repart, initialisée.
//!
//! Mesures, sur les anciens points, après `cadrer` (ce que voit l'interface,
//! carré `[-1, 1]²`, largeur 2) : déplacement médian et au 9ᵉ décile, en
//! pourcentage de la largeur. Puis la conservation des 12 voisins (512 d →
//! 2 d) sur tous les points. Critère de D5 : médiane < 2 %, conservation
//! inchangée (~13 %).
//!
//! Rien n'est écrit : tout est lu dans la base, `analyze` déjà passé.
//!
//!   cargo run --release -p rusty-music-analysis --example stabilite_projection -- <base.db>

use std::collections::HashSet;
use std::time::Instant;

use rusty_music_analysis::chemin::{Empreinte, Graphe, K_VOISINS};
use rusty_music_analysis::passe::MODELE;
use rusty_music_analysis::projection::{
    cadrer, normaliser, placer_nouveaux, projeter, reprendre, Point, EPOQUES_REPRISE,
};
use rusty_music_core::Library;

const PART_NOUVEAUX: f64 = 0.05;
const GRAINE: u64 = 0x005E_EDD5;

/// Une variante de projection, à mesurer.
type Essai<'a> = Box<dyn Fn() -> Vec<Point> + 'a>;

/// Comment repartir.
enum Depart<'a> {
    Suite(&'a [f32]),
    Amorce(&'a [f32]),
}

fn reprojeter(unites: &[Vec<f32>], perplexite: f32, epoques: usize, depart: Depart) -> Vec<Point> {
    let perplexite = perplexite.min((unites.len() as f32 - 1.0) / 3.0).max(2.0);
    let mut tsne: bhtsne::tSNE<f32, Vec<f32>, 2> = bhtsne::tSNE::new(unites);
    tsne.perplexity(perplexite).epochs(epoques);
    match depart {
        Depart::Suite(y) => {
            tsne.initial_embedding(y.to_vec())
                .stop_lying_epoch(0)
                .momentum_switch_epoch(0);
        }
        Depart::Amorce(y) => {
            tsne.initial_embedding(y.to_vec());
        }
    }
    tsne.barnes_hut(0.5, |a, b| {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt()
    });
    tsne.embedding()
        .chunks(2)
        .map(|c| Point { x: c[0], y: c[1] })
        .collect()
}

/// splitmix64 : assez pour un tirage reproductible, sans dépendance.
fn tirage(n: usize, part: f64, graine: u64) -> HashSet<usize> {
    let mut s = graine;
    let mut suivant = || {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut ordre: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        ordre.swap(i, (suivant() % (i as u64 + 1)) as usize);
    }
    ordre
        .into_iter()
        .take((n as f64 * part).round() as usize)
        .collect()
}

fn quantile(mut v: Vec<f32>, q: f64) -> f32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * q).round() as usize]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "rusty-music.db".into());
    let lib = Library::open(std::path::Path::new(&base))?;
    let params = lib.parametres_carte()?;
    let fils = std::thread::available_parallelism().map_or(1, |n| n.get());

    let empreintes: Vec<Empreinte> = lib.embeddings(MODELE)?;
    let n = empreintes.len();
    let unites: Vec<Vec<f32>> = empreintes.iter().map(|(_, v)| normaliser(v)).collect();
    let nouveaux = tirage(n, PART_NOUVEAUX, GRAINE);
    let anciens: Vec<usize> = (0..n).filter(|i| !nouveaux.contains(i)).collect();
    println!(
        "{n} empreintes · {} anciens · {} nouveaux · perplexité {} · {} époques\n",
        anciens.len(),
        nouveaux.len(),
        params.perplexite,
        params.epoques
    );

    // --- Le nuage d'avant : les anciens seuls --------------------------------
    let t = Instant::now();
    let vecs_anciens: Vec<Vec<f32>> = anciens.iter().map(|&i| unites[i].clone()).collect();
    let brut_avant = projeter(&vecs_anciens, params.perplexite, params.epoques);
    let mut avant = brut_avant.clone();
    cadrer(&mut avant);
    println!("avant (anciens seuls) : {:.1} s", t.elapsed().as_secs_f64());

    // --- Les nouveaux, au barycentre de leurs voisins CLAP parmi les anciens -
    let ecart_de = |pts: &[Point]| -> f32 {
        let m = pts.len() as f32;
        let (mx, my) = (
            pts.iter().map(|p| p.x).sum::<f32>() / m,
            pts.iter().map(|p| p.y).sum::<f32>() / m,
        );
        (pts.iter()
            .map(|p| (p.x - mx).powi(2) + (p.y - my).powi(2))
            .sum::<f32>()
            / m)
            .sqrt()
    };
    println!(
        "écart-type des coordonnées t-SNE brutes : {:.1} (cadrées : {:.3})",
        ecart_de(&brut_avant),
        ecart_de(&avant)
    );
    let mut depart = vec![0.0f32; 2 * n];
    for (k, &i) in anciens.iter().enumerate() {
        depart[2 * i] = brut_avant[k].x;
        depart[2 * i + 1] = brut_avant[k].y;
    }
    for &i in &nouveaux {
        let mut proches: Vec<(f32, usize)> = anciens
            .iter()
            .enumerate()
            .map(|(k, &j)| {
                let d: f32 = unites[i].iter().zip(&unites[j]).map(|(a, b)| a * b).sum();
                (-d, k)
            })
            .collect();
        proches.select_nth_unstable_by(K_VOISINS, |a, b| a.0.partial_cmp(&b.0).unwrap());
        let (mut x, mut y) = (0.0, 0.0);
        for &(_, k) in &proches[..K_VOISINS] {
            x += brut_avant[k].x;
            y += brut_avant[k].y;
        }
        depart[2 * i] = x / K_VOISINS as f32;
        depart[2 * i + 1] = y / K_VOISINS as f32;
    }
    // L'amorce d'openTSNE : même forme, écart-type 1e-4.
    let ecart = {
        let m = depart.len() as f32;
        let moy_x = depart.iter().step_by(2).sum::<f32>() / (m / 2.0);
        let moy_y = depart.iter().skip(1).step_by(2).sum::<f32>() / (m / 2.0);
        let var = depart
            .chunks(2)
            .map(|c| (c[0] - moy_x).powi(2) + (c[1] - moy_y).powi(2))
            .sum::<f32>()
            / m;
        var.sqrt()
    };
    let amorce: Vec<f32> = depart.iter().map(|v| v / ecart * 1e-4).collect();
    // Le même départ à d'autres échelles : la base ne garde que les
    // coordonnées cadrées, l'échelle brute est perdue.
    let quart: Vec<f32> = depart.iter().map(|v| v / 4.0).collect();
    let quadruple: Vec<f32> = depart.iter().map(|v| v * 4.0).collect();

    // --- Le graphe des voisins en 512 d, une fois -----------------------------
    let g512 = Graphe::construire(&empreintes, K_VOISINS, fils);
    let conservation = |pts: &[Point]| -> f64 {
        let plats: Vec<Empreinte> = empreintes
            .iter()
            .zip(pts)
            .map(|((id, _), p)| (*id, vec![p.x, p.y]))
            .collect();
        let g2d = Graphe::construire(&plats, K_VOISINS, fils);
        let mut communs = 0usize;
        for &id in g512.identifiants() {
            let a: HashSet<i64> = g512.voisins(id, K_VOISINS).into_iter().collect();
            communs += g2d
                .voisins(id, K_VOISINS)
                .into_iter()
                .filter(|v| a.contains(v))
                .count();
        }
        communs as f64 / (n * K_VOISINS) as f64 * 100.0
    };

    println!(
        "\n{:<14} {:>9} {:>10} {:>10} {:>14}",
        "variante", "temps", "médiane", "9e décile", "conservation"
    );
    println!("{}", "─".repeat(61));
    let essais: Vec<(&str, Essai)> = vec![
        (
            "zéro",
            Box::new(|| projeter(&unites, params.perplexite, params.epoques)),
        ),
        (
            "suite",
            Box::new(|| {
                reprojeter(
                    &unites,
                    params.perplexite,
                    params.epoques,
                    Depart::Suite(&depart),
                )
            }),
        ),
        (
            "suite courte",
            Box::new(|| {
                reprojeter(
                    &unites,
                    params.perplexite,
                    EPOQUES_REPRISE,
                    Depart::Suite(&depart),
                )
            }),
        ),
        (
            // Ce que fait la passe : depuis les coordonnées cadrées, seules
            // gardées en base.
            "la passe",
            Box::new(|| {
                let mut places: Vec<Option<Point>> = vec![None; n];
                for (k, &i) in anciens.iter().enumerate() {
                    places[i] = Some(avant[k]);
                }
                let vecs: Vec<Vec<f32>> = empreintes.iter().map(|(_, v)| v.clone()).collect();
                reprendre(
                    &vecs,
                    params.perplexite,
                    &placer_nouveaux(&vecs, &places, K_VOISINS),
                )
            }),
        ),
        (
            "courte ×¼",
            Box::new(|| {
                reprojeter(
                    &unites,
                    params.perplexite,
                    EPOQUES_REPRISE,
                    Depart::Suite(&quart),
                )
            }),
        ),
        (
            "courte ×4",
            Box::new(|| {
                reprojeter(
                    &unites,
                    params.perplexite,
                    EPOQUES_REPRISE,
                    Depart::Suite(&quadruple),
                )
            }),
        ),
        (
            "amorce",
            Box::new(|| {
                reprojeter(
                    &unites,
                    params.perplexite,
                    params.epoques,
                    Depart::Amorce(&amorce),
                )
            }),
        ),
    ];
    for (nom, projeter_ainsi) in essais {
        let t = Instant::now();
        let mut pts = projeter_ainsi();
        let duree = t.elapsed().as_secs_f64();
        cadrer(&mut pts);
        let deplacements: Vec<f32> = anciens
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                ((pts[i].x - avant[k].x).powi(2) + (pts[i].y - avant[k].y).powi(2)).sqrt()
            })
            .collect();
        // En % de la largeur du carré (2).
        let med = quantile(deplacements.clone(), 0.5) / 2.0 * 100.0;
        let d9 = quantile(deplacements, 0.9) / 2.0 * 100.0;
        println!(
            "{nom:<14} {duree:>7.1} s {med:>9.2} % {d9:>9.2} % {:>12.1} %",
            conservation(&pts)
        );
    }

    // --- La dérive : des passes répétées, sans rien ajouter ---------------------
    // Chaque passe reprend la précédente ; on mesure ce qui a bougé depuis la
    // première.
    println!("\ndérive, suite courte répétée sans ajout :");
    let mut brut = reprojeter(
        &unites,
        params.perplexite,
        EPOQUES_REPRISE,
        Depart::Suite(&depart),
    );
    let mut premiere = brut.clone();
    cadrer(&mut premiere);
    for passe in 1..=3 {
        let y: Vec<f32> = brut.iter().flat_map(|p| [p.x, p.y]).collect();
        brut = reprojeter(
            &unites,
            params.perplexite,
            EPOQUES_REPRISE,
            Depart::Suite(&y),
        );
        let mut pts = brut.clone();
        cadrer(&mut pts);
        let d: Vec<f32> = pts
            .iter()
            .zip(&premiere)
            .map(|(a, b)| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt())
            .collect();
        println!(
            "  passe {passe} : médiane {:.2} %, 9e décile {:.2} %, conservation {:.1} %",
            quantile(d.clone(), 0.5) / 2.0 * 100.0,
            quantile(d, 0.9) / 2.0 * 100.0,
            conservation(&pts)
        );
    }
    Ok(())
}
