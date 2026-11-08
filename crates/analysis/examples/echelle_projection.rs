// SPDX-License-Identifier: GPL-3.0-or-later
//! Le « plafond » de la projection tient-il à l'empreinte ou à la descente ?
//!
//! `projection.rs` notait qu'environ 13 % des douze plus proches voisins
//! survivent à t-SNE, quels que soient l'amorçage et la normalisation, et en
//! concluait que le plafond est dans l'empreinte. En D5, une reprise partie
//! d'une échelle quatre fois plus grande a porté la conservation à 24,7 % :
//! la descente de gradient y est peut-être pour quelque chose.
//!
//! On refait la projection depuis zéro en faisant varier, un axe à la fois
//! autour du réglage de l'application (départ gaussien d'écart-type 1e-4,
//! 1 000 époques, exagération 12 pendant 250) :
//!
//!   - l'**échelle de départ** (écart-type du tirage gaussien) ;
//!   - le nombre d'**époques** ;
//!   - l'**exagération** précoce ;
//!   - une **suite** : le réglage de l'application, puis 250 ou 1 000 époques
//!     de plus sans exagération, depuis ses coordonnées brutes à l'échelle
//!     ×1, ×4 ou ×8 (le cas vu en D5, mais sur une projection faite depuis
//!     zéro) ;
//!   - le **pas d'apprentissage**, multiplié par 4 (`n / 12`, le choix
//!     d'openTSNE), 8, 16 (`n / 3`, avec 500, 1 000 ou 2 000 époques), 32
//!     ou 64 (V10).
//!
//! Chaque variante tourne avec trois graines (le tirage du départ ; le
//! Barnes-Hut parallèle ajoute son propre hasard). Mesures : conservation des
//! 12 voisins (512 d → 2 d), pureté de genre exact et de famille des 12
//! voisins projetés (la famille est celle en base), temps de la descente. Les
//! affinités, communes à toutes les variantes, sont calculées une fois ; le
//! pas d'apprentissage vaut, sauf sur son propre axe, celui que `bhtsne`
//! choisit pour une exagération de 12 (`n / 48`), pour que l'axe
//! « exagération » ne change que l'exagération.
//!
//! Rien n'est écrit : tout est lu dans la base, `analyze` et `project` passés.
//!
//!   cargo run --release -p rusty-music-analysis --example echelle_projection -- <base.db>
//!   RUSTY_VARIANTES=pas cargo run … : l'application et l'axe du pas seulement

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rusty_music_analysis::chemin::{Empreinte, Graphe, K_VOISINS};
use rusty_music_analysis::passe::MODELE;
use rusty_music_analysis::projection::normaliser;
use rusty_music_core::Library;

type Affinites = bhtsne::SparseAffinities<f32>;

const GRAINES: [u64; 3] = [0x0E5C_A1E1, 0x0E5C_A1E2, 0x0E5C_A1E3];

/// Une variante : d'où partir, combien d'époques, quelle exagération.
#[derive(Clone, Copy)]
struct Reglage {
    nom: &'static str,
    ecart: f32,
    epoques: usize,
    exageration: f32,
    /// Une suite : époques de plus, sans exagération, depuis le résultat du
    /// réglage de l'application multiplié par ce facteur.
    suite: Option<(usize, f32)>,
    /// Facteur sur le pas d'apprentissage (`n / 48`).
    pas: f32,
}

const APP: Reglage = Reglage {
    nom: "app (1e-4, 1000, ×12)",
    ecart: 1e-4,
    epoques: 1000,
    exageration: 12.0,
    suite: None,
    pas: 1.0,
};

/// splitmix64 puis Box-Muller : un départ gaussien reproductible.
fn depart_gaussien(n: usize, ecart: f32, graine: u64) -> Vec<f32> {
    let mut s = graine;
    let mut uniforme = || {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| {
            let (u, v) = (uniforme().max(1e-300), uniforme());
            ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()) as f32 * ecart
        })
        .collect()
}

fn descendre(
    unites: &[Vec<f32>],
    affinites: &Affinites,
    depart: Vec<f32>,
    epoques: usize,
    exageration: f32,
    pas: f32,
) -> Vec<f32> {
    let mut tsne: bhtsne::tSNE<f32, Vec<f32>, 2> = bhtsne::tSNE::new(unites);
    tsne.with_affinities(affinites.clone())
        .learning_rate(pas)
        .epochs(epoques)
        .initial_embedding(depart);
    if exageration > 1.0 {
        tsne.early_exaggeration(exageration);
    } else {
        tsne.stop_lying_epoch(0).momentum_switch_epoch(0);
    }
    tsne.barnes_hut(0.5, |a, b| {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt()
    });
    tsne.embedding()
}

#[derive(Default, Clone, Copy)]
struct Mesure {
    conservation: f64,
    genre: f64,
    famille: f64,
    secondes: f64,
    ecart: f64,
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
    let genres = lib.genres_resolus(MODELE)?;
    let famille: HashMap<i64, i64> = lib
        .map_points(MODELE)?
        .into_iter()
        .filter(|p| p.3 >= 0)
        .map(|(id, _, _, c)| (id, c))
        .collect();
    let perplexite = params.perplexite.min((n as f32 - 1.0) / 3.0).max(2.0);
    let pas = (n as f32 / 48.0).max(50.0);
    println!(
        "{n} empreintes · {} à genre résolu · {} en famille · perplexité {perplexite} · pas {pas:.0}",
        genres.len(),
        famille.len()
    );

    // --- Les voisins en 512 d et les affinités, une fois -----------------------
    let g512 = Graphe::construire(&empreintes, K_VOISINS, fils);
    let t = Instant::now();
    let affinites = {
        let mut tsne: bhtsne::tSNE<f32, Vec<f32>, 2> = bhtsne::tSNE::new(&unites);
        tsne.perplexity(perplexite)
            .epochs(0)
            .barnes_hut(0.5, |a, b| {
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| (x - y) * (x - y))
                    .sum::<f32>()
                    .sqrt()
            });
        tsne.affinities().expect("affinités")
    };
    println!("affinités : {:.1} s", t.elapsed().as_secs_f64());

    // Pureté d'un graphe : part des paires (morceau, voisin) étiquetées des deux
    // côtés qui partagent l'étiquette.
    let purete = |g: &Graphe| -> (f64, f64) {
        let (mut pg, mut mg, mut pf, mut mf) = (0usize, 0usize, 0usize, 0usize);
        for &id in g.identifiants() {
            for v in g.voisins(id, K_VOISINS) {
                if let (Some(a), Some(b)) = (genres.get(&id), genres.get(&v)) {
                    pg += 1;
                    mg += usize::from(a == b);
                }
                if let (Some(a), Some(b)) = (famille.get(&id), famille.get(&v)) {
                    pf += 1;
                    mf += usize::from(a == b);
                }
            }
        }
        (
            mg as f64 / pg.max(1) as f64 * 100.0,
            mf as f64 / pf.max(1) as f64 * 100.0,
        )
    };
    let (g_512, f_512) = purete(&g512);
    println!("pureté en 512 d : genre {g_512:.1} %, famille {f_512:.1} %\n");

    let mesurer = |y: &[f32], secondes: f64| -> Mesure {
        let plats: Vec<Empreinte> = empreintes
            .iter()
            .zip(y.chunks(2))
            .map(|((id, _), c)| (*id, c.to_vec()))
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
        let (genre, famille) = purete(&g2d);
        let m = n as f64;
        let (mx, my) = y.chunks(2).fold((0.0, 0.0), |(a, b), c| {
            (a + c[0] as f64 / m, b + c[1] as f64 / m)
        });
        let ecart = (y
            .chunks(2)
            .map(|c| (c[0] as f64 - mx).powi(2) + (c[1] as f64 - my).powi(2))
            .sum::<f64>()
            / m)
            .sqrt();
        Mesure {
            ecart,
            conservation: communs as f64 / (n * K_VOISINS) as f64 * 100.0,
            genre,
            famille,
            secondes,
        }
    };

    let r = |nom, ecart, epoques, exageration| Reglage {
        nom,
        ecart,
        epoques,
        exageration,
        suite: None,
        pas: 1.0,
    };
    let s = |nom, epoques, facteur| Reglage {
        suite: Some((epoques, facteur)),
        nom,
        ..APP
    };
    let reglages = [
        APP,
        // Échelle de départ.
        r("départ 1e-2", 1e-2, 1000, 12.0),
        r("départ 1", 1.0, 1000, 12.0),
        r("départ 10", 10.0, 1000, 12.0),
        r("départ 30", 30.0, 1000, 12.0),
        // Époques.
        r("500 époques", 1e-4, 500, 12.0),
        r("2000 époques", 1e-4, 2000, 12.0),
        r("4000 époques", 1e-4, 4000, 12.0),
        // Exagération.
        r("sans exagération", 1e-4, 1000, 1.0),
        r("exagération 4", 1e-4, 1000, 4.0),
        r("exagération 24", 1e-4, 1000, 24.0),
        // Suites du réglage de l'application.
        s("app + 250 ×1", 250, 1.0),
        s("app + 250 ×4", 250, 4.0),
        s("app + 1000 ×1", 1000, 1.0),
        s("app + 1000 ×4", 1000, 4.0),
        // Départ large et longue descente ensemble.
        r("départ 10, 4000 ép.", 10.0, 4000, 12.0),
        s("app + 250 ×8", 250, 8.0),
        s("app + 1000 ×8", 1000, 8.0),
        // Pas d'apprentissage : ×4, c'est `n / 12`, le choix d'openTSNE.
        Reglage {
            nom: "pas ×4",
            pas: 4.0,
            ..APP
        },
        Reglage {
            nom: "pas ×4, 2000 ép.",
            pas: 4.0,
            epoques: 2000,
            ..APP
        },
        Reglage {
            nom: "pas ×16",
            pas: 16.0,
            ..APP
        },
        // Suites de V3 (V10) : autour du pas ×16, qui vaut `n / 3` (le pas
        // `n / 48` de `bhtsne` × 16).
        Reglage {
            nom: "pas ×8",
            pas: 8.0,
            ..APP
        },
        Reglage {
            nom: "pas ×16, 500 ép.",
            pas: 16.0,
            epoques: 500,
            ..APP
        },
        Reglage {
            nom: "pas ×16, 2000 ép.",
            pas: 16.0,
            epoques: 2000,
            ..APP
        },
        Reglage {
            nom: "pas ×32",
            pas: 32.0,
            ..APP
        },
        Reglage {
            nom: "pas ×64",
            pas: 64.0,
            ..APP
        },
    ];
    // `RUSTY_VARIANTES=app,pas` : seulement les variantes dont le nom commence
    // par l'un de ces préfixes (le réglage de l'application tourne toujours,
    // les suites en dépendent).
    let filtre: Option<Vec<String>> = std::env::var("RUSTY_VARIANTES")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let retenue = |r: &Reglage| {
        r.nom == APP.nom
            || filtre
                .as_ref()
                .is_none_or(|f| f.iter().any(|p| r.nom.starts_with(p.as_str())))
    };

    // Les résultats du réglage de l'application, par graine, pour les suites.
    let mut app_par_graine: Vec<Vec<f32>> = Vec::new();
    println!(
        "{:<24} {:>13} {:>13} {:>13} {:>10} {:>7}",
        "variante", "conservation", "pureté genre", "pureté fam.", "temps", "écart"
    );
    println!("{}", "─".repeat(85));
    for reglage in reglages.into_iter().filter(retenue) {
        let mut mesures = Vec::new();
        for (k, &graine) in GRAINES.iter().enumerate() {
            let t = Instant::now();
            let y = match reglage.suite {
                None => descendre(
                    &unites,
                    &affinites,
                    depart_gaussien(2 * n, reglage.ecart, graine),
                    reglage.epoques,
                    reglage.exageration,
                    pas * reglage.pas,
                ),
                Some((epoques, facteur)) => {
                    let depart = app_par_graine[k].iter().map(|v| v * facteur).collect();
                    descendre(&unites, &affinites, depart, epoques, 1.0, pas)
                }
            };
            let secondes = t.elapsed().as_secs_f64();
            if reglage.nom == APP.nom {
                app_par_graine.push(y.clone());
            }
            mesures.push(mesurer(&y, secondes));
        }
        let etendue = |f: fn(&Mesure) -> f64| {
            let v: Vec<f64> = mesures.iter().map(f).collect();
            let moy = v.iter().sum::<f64>() / v.len() as f64;
            let ecart = v.iter().cloned().fold(f64::MIN, f64::max)
                - v.iter().cloned().fold(f64::MAX, f64::min);
            (moy, ecart)
        };
        let (c, dc) = etendue(|m| m.conservation);
        let (g, dg) = etendue(|m| m.genre);
        let (f, df) = etendue(|m| m.famille);
        let (t, _) = etendue(|m| m.secondes);
        let (e, _) = etendue(|m| m.ecart);
        println!(
            "{:<24} {c:>6.1} ±{:<4.1} {g:>6.1} ±{:<4.1} {f:>6.1} ±{:<4.1} {t:>8.1} s {e:>7.1}",
            reglage.nom,
            dc / 2.0,
            dg / 2.0,
            df / 2.0
        );
    }
    println!(
        "\n± : demi-étendue sur {} graines. Temps : descente seule, mesures exclues.\n\
         Écart : écart-type des coordonnées brutes (avant `cadrer`).",
        GRAINES.len()
    );
    Ok(())
}
