// SPDX-License-Identifier: GPL-3.0-or-later
//! La projection préserve-t-elle le genre, ou l'empreinte ne le porte-t-elle
//! pas ? Deux hypothèses pour un même symptôme — une famille de genre étalée
//! sur presque toute la carte — et ce contrôle les sépare.
//!
//! On mesure la **pureté du voisinage** : pour chaque morceau à genre résolu,
//! la part de ses douze plus proches voisins qui partagent son genre exact,
//! puis sa famille. Deux fois : dans l'espace des empreintes (512 d) et sur la
//! carte (2 d).
//!
//!   - pureté haute en 512 d, effondrée en 2 d → **c'est la projection** qui
//!     disperse. Piste : `spectral_init` de `bhtsne`, z-score avant t-SNE,
//!     perplexité plus haute — tout est bon marché et dans l'arbre actuel.
//!   - pureté déjà basse en 512 d → **l'empreinte ne sépare pas le genre** et
//!     aucun réglage de t-SNE n'y changera rien. Piste : un modèle spécifique
//!     musique (MERT, MAEST, Discogs-EffNet), chantier de l'ampleur de
//!     `experiments/burn-clap/`.
//!
//! Le repère « grunge » en fin de sortie tranche une troisième cause,
//! indépendante des deux autres : la famille « Rock » du vocabulaire réunit
//! treize genres (`crates/core/src/db.rs`, `VOCABULAIRE_DEFAUT`). Si le tag
//! *littéral* « grunge » se regroupe alors que la famille « Rock » entière non,
//! le problème est la largeur du seau, pas l'espace ni la projection.
//!
//! Rien n'est recalculé ni écrit : tout vient de la base, `analyze` et
//! `project` déjà passés.
//!
//!   cargo run --release -p rusty-music-analysis --example genre_projection -- <base.db>

use std::collections::HashMap;

use rusty_music_analysis::chemin::{Empreinte, Graphe, K_VOISINS};
use rusty_music_analysis::passe::MODELE;
use rusty_music_core::Library;

/// Au-delà de cette taille, la dispersion d'une famille est estimée sur un
/// sous-échantillon régulier — la moyenne des distances de paires converge
/// bien avant d'avoir à en former des dizaines de millions.
const PLAFOND_PAIRES: usize = 4000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "rusty-music.db".into());
    let lib = Library::open(std::path::Path::new(&base))?;
    let fils = std::thread::available_parallelism().map_or(1, |n| n.get());

    let empreintes: Vec<Empreinte> = lib.embeddings(MODELE)?;
    let points = lib.map_points(MODELE)?; // (id, x, y, cluster)
    let genres = lib.genres_resolus(MODELE)?; // id -> genre résolu, minuscules
    let noms_familles: HashMap<i64, String> = lib
        .familles(MODELE)?
        .into_iter()
        .map(|(id, nom, _)| (id, nom))
        .collect();

    if empreintes.len() < 100 || points.len() < 100 {
        eprintln!(
            "{} empreintes, {} points placés : pas de quoi juger",
            empreintes.len(),
            points.len()
        );
        return Ok(());
    }

    let cluster: HashMap<i64, i64> = points.iter().map(|(id, _, _, c)| (*id, *c)).collect();
    let avec_genre = genres.len();
    let familles_vues: std::collections::HashSet<i64> =
        cluster.values().copied().filter(|c| *c >= 0).collect();

    println!(
        "{} empreintes · {} placées · {} à genre résolu · {} familles\n",
        empreintes.len(),
        points.len(),
        avec_genre,
        familles_vues.len()
    );

    // --- Deux graphes des K_VOISINS plus proches, mêmes identifiants ---
    // 512 d : l'espace des empreintes.
    let g512 = Graphe::construire(&empreintes, K_VOISINS, fils);
    // 2 d : la carte. La même machinerie, l'« empreinte » étant ici (x, y).
    let plats: Vec<Empreinte> = points
        .iter()
        .map(|(id, x, y, _)| (*id, vec![*x, *y]))
        .collect();
    let g2d = Graphe::construire(&plats, K_VOISINS, fils);

    // --- Pureté du voisinage ---------------------------------------------------
    // Pour un graphe donné : part des paires (morceau, voisin) qui partagent le
    // genre exact, puis la famille. Le dénominateur ne compte que les paires où
    // les deux bouts portent l'étiquette — un voisin sans genre n'est pas un
    // échec, il est hors mesure.
    let purete = |g: &Graphe| -> Purete {
        let mut p = Purete::default();
        for &id in g.identifiants() {
            let voisins = g.voisins(id, K_VOISINS);
            let g_ici = genres.get(&id);
            let f_ici = cluster.get(&id).filter(|c| **c >= 0);
            for vid in voisins {
                if let (Some(a), Some(b)) = (g_ici, genres.get(&vid)) {
                    p.paires_genre += 1;
                    if a == b {
                        p.meme_genre += 1;
                    }
                }
                if let (Some(a), Some(b)) = (f_ici, cluster.get(&vid).filter(|c| **c >= 0)) {
                    p.paires_famille += 1;
                    if a == b {
                        p.meme_famille += 1;
                    }
                }
            }
        }
        p
    };
    let p512 = purete(&g512);
    let p2d = purete(&g2d);

    // Repère : la pureté d'un voisinage tiré au hasard, c'est la probabilité
    // que deux morceaux pris au sort partagent l'étiquette — somme des carrés
    // des parts. C'est le plancher à battre.
    let hasard_genre = collision(genres.values());
    let hasard_famille = collision(
        cluster
            .values()
            .filter(|c| **c >= 0)
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .iter(),
    );

    println!("── Pureté du voisinage (k = {K_VOISINS})");
    println!(
        "{:<22} {:>12} {:>12}",
        "", "genre exact", "famille"
    );
    println!("{}", "─".repeat(48));
    println!(
        "{:<22} {:>11.1}% {:>11.1}%",
        "empreinte (512 d)",
        p512.genre() * 100.0,
        p512.famille() * 100.0
    );
    println!(
        "{:<22} {:>11.1}% {:>11.1}%",
        "carte (2 d)",
        p2d.genre() * 100.0,
        p2d.famille() * 100.0
    );
    println!(
        "{:<22} {:>+11.1}  {:>+11.1} ",
        "perte à la projection",
        (p2d.genre() - p512.genre()) * 100.0,
        (p2d.famille() - p512.famille()) * 100.0
    );
    println!(
        "{:<22} {:>11.1}% {:>11.1}%   ← plancher",
        "au hasard",
        hasard_genre * 100.0,
        hasard_famille * 100.0
    );

    // --- Conservation du voisinage à la projection --------------------------
    // Le vrai travail d'une projection : garder voisins ceux qui l'étaient.
    // Part des 12 plus proches voisins en 512 d qui restent parmi les 12 plus
    // proches sur la carte. C'est ce qu'améliorerait une meilleure projection —
    // t-SNE mieux initialisé, ou UMAP.
    let mut recouvre = 0usize;
    let mut base_n = 0usize;
    for &id in g512.identifiants() {
        if g2d.rang_de(id).is_none() {
            continue;
        }
        let a: std::collections::HashSet<i64> = g512.voisins(id, K_VOISINS).into_iter().collect();
        let b: std::collections::HashSet<i64> = g2d.voisins(id, K_VOISINS).into_iter().collect();
        recouvre += a.intersection(&b).count();
        base_n += K_VOISINS;
    }
    println!("\n── Conservation du voisinage (512 d → carte)");
    println!(
        "  voisins communs aux deux : {:.1}% des 12\n\
         \x20 (100 % = projection parfaite ; <0,1 % = hasard sur 27 k)",
        recouvre as f64 / base_n.max(1) as f64 * 100.0
    );

    // --- Dispersion par famille : en 512 d ET sur la carte ------------------
    // Si une famille est déjà étalée dans l'empreinte, aucune projection 2 d ne
    // la resserrera — le plafond est mis par l'empreinte, pas par t-SNE.
    let coords: HashMap<i64, Vec<f32>> =
        points.iter().map(|(id, x, y, _)| (*id, vec![*x, *y])).collect();
    let emb: HashMap<i64, Vec<f32>> = empreintes.iter().map(|(id, v)| (*id, v.clone())).collect();
    let tous: Vec<i64> = points.iter().map(|(id, ..)| *id).collect();
    let hasard_2d = dist_moyenne(&tous, &coords);
    let hasard_512 = dist_moyenne(&tous, &emb);

    let mut par_famille: HashMap<i64, Vec<i64>> = HashMap::new();
    for (id, _, _, c) in &points {
        if *c >= 0 {
            par_famille.entry(*c).or_default().push(*id);
        }
    }
    let mut lignes: Vec<(i64, usize, f64, f64)> = par_famille
        .iter()
        .map(|(c, ids)| {
            (
                *c,
                ids.len(),
                dist_moyenne(ids, &emb) / hasard_512,
                dist_moyenne(ids, &coords) / hasard_2d,
            )
        })
        .collect();
    lignes.sort_by_key(|(_, n, _, _)| std::cmp::Reverse(*n));

    println!("\n── Dispersion par famille (÷ hasard ; plus bas = plus resserré)");
    println!(
        "{:<24} {:>9} {:>12} {:>12}",
        "famille", "morceaux", "en 512 d", "sur la carte"
    );
    println!("{}", "─".repeat(60));
    for (c, n, d512, d2d) in &lignes {
        let brut = noms_familles
            .get(c)
            .cloned()
            .unwrap_or_else(|| format!("cluster {c}"));
        let nom: String = brut.chars().take(24).collect();
        println!("{nom:<24} {n:>9} {d512:>11.2}× {d2d:>11.2}×");
    }
    println!("{:<24} {:>9} {:>11.2}× {:>11.2}×", "au hasard", points.len(), 1.0, 1.0);

    // --- Repère « grunge » littéral -----------------------------------------
    let grunge: Vec<i64> = genres
        .iter()
        .filter(|(_, g)| g.as_str() == "grunge")
        .map(|(id, _)| *id)
        .filter(|id| coords.contains_key(id))
        .collect();

    println!("\n── Repère « grunge » (tag résolu littéral, pas la famille Rock)");
    if grunge.len() < 20 {
        println!(
            "{} morceaux seulement au genre résolu « grunge » — repère non concluant",
            grunge.len()
        );
    } else {
        let set: std::collections::HashSet<i64> = grunge.iter().copied().collect();
        let purete_sous = |g: &Graphe| -> f64 {
            let (mut oui, mut tot) = (0usize, 0usize);
            for &id in &grunge {
                for vid in g.voisins(id, K_VOISINS) {
                    tot += 1;
                    if set.contains(&vid) {
                        oui += 1;
                    }
                }
            }
            oui as f64 / tot.max(1) as f64
        };
        let d512 = dist_moyenne(&grunge, &emb) / hasard_512;
        let d2d = dist_moyenne(&grunge, &coords) / hasard_2d;
        println!("{} morceaux au genre résolu « grunge »", grunge.len());
        println!(
            "  voisins aussi « grunge »  — empreinte {:.1}%   carte {:.1}%",
            purete_sous(&g512) * 100.0,
            purete_sous(&g2d) * 100.0
        );
        println!("  dispersion  — {d512:.2}× le hasard en 512 d, {d2d:.2}× sur la carte");
        println!(
            "  (à comparer à la ligne « Rock · Grunge » ci-dessus : un « grunge »\n\
             \x20  resserré sous une famille « Rock » étalée = c'est la largeur du seau.)"
        );
    }

    Ok(())
}

#[derive(Default)]
struct Purete {
    meme_genre: usize,
    paires_genre: usize,
    meme_famille: usize,
    paires_famille: usize,
}

impl Purete {
    fn genre(&self) -> f64 {
        self.meme_genre as f64 / self.paires_genre.max(1) as f64
    }
    fn famille(&self) -> f64 {
        self.meme_famille as f64 / self.paires_famille.max(1) as f64
    }
}

/// Probabilité que deux étiquettes tirées au hasard dans la série coïncident —
/// somme des carrés des fréquences. C'est la pureté qu'un voisinage aléatoire
/// atteindrait, donc le plancher à battre.
fn collision<'a, S: AsRef<str> + 'a>(labels: impl Iterator<Item = &'a S>) -> f64 {
    let mut comptes: HashMap<&str, usize> = HashMap::new();
    let mut total = 0usize;
    for l in labels {
        *comptes.entry(l.as_ref()).or_default() += 1;
        total += 1;
    }
    if total == 0 {
        return f64::NAN;
    }
    comptes
        .values()
        .map(|&n| {
            let p = n as f64 / total as f64;
            p * p
        })
        .sum()
}

/// Distance euclidienne moyenne des paires d'un ensemble de morceaux, dans
/// l'espace que porte `pos` — 2 d pour la carte, 512 d pour l'empreinte.
/// Au-delà de [`PLAFOND_PAIRES`] morceaux, un sous-échantillon régulier : la
/// moyenne se stabilise bien avant l'exhaustivité.
fn dist_moyenne(ids: &[i64], pos: &HashMap<i64, Vec<f32>>) -> f64 {
    let pts: Vec<&Vec<f32>> = ids.iter().filter_map(|id| pos.get(id)).collect();
    if pts.len() < 2 {
        return f64::NAN;
    }
    let pas = pts.len().div_ceil(PLAFOND_PAIRES).max(1);
    let ech: Vec<&Vec<f32>> = pts.iter().step_by(pas).copied().collect();
    let (mut somme, mut n) = (0.0f64, 0usize);
    for i in 0..ech.len() {
        for j in i + 1..ech.len() {
            let d2: f32 = ech[i]
                .iter()
                .zip(ech[j].iter())
                .map(|(a, b)| (a - b) * (a - b))
                .sum();
            somme += d2.sqrt() as f64;
            n += 1;
        }
    }
    somme / n.max(1) as f64
}
