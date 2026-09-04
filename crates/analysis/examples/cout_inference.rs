// SPDX-License-Identifier: GPL-3.0-or-later
//! Jalon 1 — combien coûte une empreinte.
//!
//! Mesure le seul poste que le plan du module 2 laissait chiffré à l'estime :
//! l'inférence du modèle. Le contenu des fenêtres n'influe pas sur la durée
//! d'un passage avant — c'est un calcul dense, à forme fixe — donc on mesure
//! sur des fenêtres synthétiques et le chiffre vaut pour de vraies.
//!
//!   cargo run --release -p rusty-music-analysis --example cout_inference
//!
//! Un chemin de modèle peut être passé en argument.

use std::time::Instant;

use rusty_music_analysis::{Embedder, DIMS, FENETRE_S, MELS, TRAMES};

/// Bibliothèque de référence, mesurée le 16 août 2026.
const MORCEAUX: f64 = 27_044.0;
const HEURES_AUDIO: f64 = 1843.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Poids du build en cours si aucun chemin n'est donné.
    let modele = std::env::args().nth(1);
    let modele = modele.as_deref().map(std::path::Path::new);
    let coeurs = std::thread::available_parallelism().map_or(1, |n| n.get());

    println!(
        "poids   : {}",
        modele.map_or(env!("RM_POIDS"), |p| p.to_str().unwrap_or("?"))
    );
    println!("backend : {}", rusty_music_analysis::encodeur::moteur());
    println!("entrée  : [n, 1, {TRAMES}, {MELS}]  ({FENETRE_S} s par fenêtre)");
    println!("sortie  : {DIMS} dimensions");
    println!("cœurs   : {coeurs}\n");

    let t = Instant::now();
    let mut enc = Embedder::charger(modele, coeurs)?;
    println!("chargement du modèle : {} ms", t.elapsed().as_millis());

    // Une fenêtre plausible : du bruit borné, pas des zéros — certains graphes
    // court-circuitent sur des entrées constantes.
    let fenetre: Vec<f32> = (0..TRAMES * MELS)
        .map(|i| ((i * 2654435761) % 1000) as f32 / 1000.0 - 0.5)
        .collect();

    // Chauffe : la première inférence paie l'allocation des tampons.
    enc.empreintes(&fenetre, 1)?;

    // Une durée reste « valide » même si le modèle rend toujours la même
    // chose : on vérifie d'abord qu'il réagit au contenu. Deux bruits
    // uniformes ne suffisent pas — CLAP les jugerait à juste titre
    // équivalents. Il faut deux timbres franchement distincts, dans des
    // valeurs plausibles pour du log-mel (plancher vers -80, crêtes vers 0).
    let grave = mel_bande(0..12);
    let aigu = mel_bande(50..64);
    let b = enc.empreintes(&grave, 1)?;
    let c = enc.empreintes(&aigu, 1)?;
    let (u, v) = (&b[0], &c[0]);
    let cos = produit(u, v) / (norme(u) * norme(v));
    println!(
        "contrôle : {} dimensions · grave vs aigu → cosinus {cos:.3}",
        u.len()
    );
    assert_eq!(u.len(), DIMS, "dimension inattendue");
    assert!(
        cos < 0.999,
        "le modèle ne distingue pas deux timbres opposés — entrée mal formée ?"
    );
    println!();

    // Deux façons d'occuper la machine : laisser ONNX Runtime répartir une
    // inférence sur tous les cœurs, ou lui en donner un seul et paralléliser
    // sur les morceaux. La seconde passe mieux à l'échelle si le débit par
    // cœur y est meilleur — c'est ce que cette comparaison tranche.
    println!(
        "{:>8}  {:>5}  {:>11}  {:>13}  {:>11}",
        "threads", "lot", "par lot", "par fenêtre", "fenêtres/s"
    );
    println!("{}", "─".repeat(58));

    let mut meilleur = (coeurs, 1usize, 0f64);
    for threads in [coeurs, 1] {
        let mut e = Embedder::charger(modele, threads)?;
        e.empreintes(&fenetre, 1)?; // chauffe

        for lot in [1usize, 8] {
            let entree: Vec<f32> = fenetre
                .iter()
                .cycle()
                .take(lot * TRAMES * MELS)
                .copied()
                .collect();
            let tours = if lot >= 8 { 5 } else { 10 };

            let t = Instant::now();
            for _ in 0..tours {
                e.empreintes(&entree, lot)?;
            }
            let par_lot = t.elapsed().as_secs_f64() / tours as f64;
            let par_fenetre = par_lot / lot as f64;
            let debit = 1.0 / par_fenetre;

            println!(
                "{threads:>8}  {lot:>5}  {:>9.0} ms  {:>11.1} ms  {debit:>11.1}",
                par_lot * 1000.0,
                par_fenetre * 1000.0
            );
            if threads == coeurs && debit > meilleur.2 {
                meilleur = (threads, lot, debit);
            }
        }
    }

    let (threads, lot, debit_interne) = meilleur;
    println!(
        "\nun seul processus : {debit_interne:.1} fenêtres/s ({threads} threads, lot de {lot})"
    );

    // Le parallélisme interne d'ONNX Runtime passe mal à l'échelle : autant
    // de cœurs pour à peine le double de débit. L'autre voie est d'ouvrir une
    // session mono-thread par cœur et de répartir les morceaux. On la mesure
    // au lieu de l'extrapoler — la bande passante mémoire pourrait tout aussi
    // bien annuler le gain.
    let debit = {
        let fenetre = std::sync::Arc::new(fenetre.clone());
        let tours = 10;
        // Chaque thread charge son modèle, puis attend les autres : douze
        // chargements simultanés de 112 Mo dureraient plusieurs secondes et
        // noieraient le temps d'inférence si on les chronométrait avec.
        let porte = std::sync::Barrier::new(coeurs + 1);

        std::thread::scope(|pool| -> Result<f64, Box<dyn std::error::Error>> {
            let mut mains = Vec::new();
            for _ in 0..coeurs {
                let fenetre = std::sync::Arc::clone(&fenetre);
                let porte = &porte;
                mains.push(pool.spawn(move || -> std::result::Result<(), String> {
                    let mut e = Embedder::charger(modele, 1).map_err(|e| e.to_string())?;
                    e.empreintes(&fenetre, 1).map_err(|e| e.to_string())?; // chauffe
                    porte.wait();
                    for _ in 0..tours {
                        e.empreintes(&fenetre, 1).map_err(|e| e.to_string())?;
                    }
                    Ok(())
                }));
            }
            porte.wait();
            let t = Instant::now();
            for m in mains {
                m.join().unwrap()?;
            }
            Ok((coeurs * tours) as f64 / t.elapsed().as_secs_f64())
        })?
    };

    println!(
        "{coeurs} sessions mono-thread en parallèle : {debit:.1} fenêtres/s  \
         (×{:.1} par rapport au processus unique)\n",
        debit / debit_interne
    );

    // Ce que ça donne sur la bibliothèque, selon la façon de découper.
    println!("Coût de l'inférence sur {MORCEAUX:.0} morceaux ({HEURES_AUDIO:.0} h) :");
    println!("{}", "─".repeat(64));
    for (nom, fenetres) in [
        ("1 fenêtre par morceau (10 s au centre)", MORCEAUX),
        ("3 fenêtres par morceau (début/milieu/fin)", MORCEAUX * 3.0),
        (
            "couverture intégrale",
            HEURES_AUDIO * 3600.0 / FENETRE_S as f64,
        ),
    ] {
        let s = fenetres / debit;
        println!("  {nom:<42} {fenetres:>9.0} fen.  {}", duree(s));
    }

    println!("\n  (débit retenu : {coeurs} sessions mono-thread en parallèle.)");
    Ok(())
}

/// Fenêtre log-mel synthétique dont l'énergie se concentre sur `bandes`.
fn mel_bande(bandes: std::ops::Range<usize>) -> Vec<f32> {
    let mut v = vec![-80.0f32; TRAMES * MELS];
    for t in 0..TRAMES {
        for m in bandes.clone() {
            // Léger relief temporel, pour ne pas présenter un plan parfait.
            v[t * MELS + m] = -8.0 + 6.0 * ((t as f32 / 40.0).sin());
        }
    }
    v
}

fn produit(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn norme(a: &[f32]) -> f32 {
    produit(a, a).sqrt()
}

fn duree(s: f64) -> String {
    if s < 90.0 {
        format!("{s:.0} s")
    } else if s < 5400.0 {
        format!("{:.0} min", s / 60.0)
    } else {
        format!("{:.1} h", s / 3600.0)
    }
}
