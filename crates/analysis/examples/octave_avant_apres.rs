//! Concentration anormale dans [40, 90] BPM, avant/après la correction
//! montante — la mesure de validation prévue par le plan de recherche sur
//! l'octave (voir le fichier de plan, section « Vérification »).
//!
//! `avant` est relu directement de `descriptors.bpm` (calculé par l'ancien
//! algorithme, à sens unique). `après` est recalculé ici sur les mêmes
//! fichiers, avec le code actuel (bidirectionnel), via [`analyser`] — le
//! même chemin de production que `passe::descripteurs`. Aucune écriture en
//! base : purement une mesure, sur un échantillon, pas une repasse.
//!
//!   cargo run --release -p rusty-music-analysis --example octave_avant_apres -- <échantillon.tsv>
//!
//! Le fichier d'échantillon est `chemin<séparateur>bpm_ancien`, une ligne
//! par morceau (tabulation ou `|`, le mode par défaut de `sqlite3`) —
//! produit par une requête SQL sur `descriptors.bpm`, pas régénéré ici : ce
//! script n'a pas besoin de savoir ouvrir la base.

use std::path::PathBuf;
use std::time::Instant;

use rusty_music_analysis::descripteurs::{analyser, Analyseur};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fichier = std::env::args()
        .nth(1)
        .ok_or("usage : octave_avant_apres <échantillon.tsv>")?;
    let lignes = std::fs::read_to_string(&fichier)?;

    let chemins: Vec<(PathBuf, f32)> = lignes
        .lines()
        .filter_map(|l| {
            let (chemin, bpm) = l.split_once(['\t', '|'])?;
            Some((PathBuf::from(chemin), bpm.parse().ok()?))
        })
        .collect();

    let mut avant: Vec<f32> = Vec::new();
    let mut apres: Vec<f32> = Vec::new();
    let mut deplaces: Vec<(String, f32, f32)> = Vec::new();
    let mut echecs = 0usize;

    let a = Analyseur::new();
    let t = Instant::now();
    for (i, (chemin, bpm_ancien)) in chemins.iter().enumerate() {
        if i % 25 == 0 {
            eprint!("\r  {i}/{}", chemins.len());
        }
        let Ok(d) = analyser(chemin, &a) else {
            echecs += 1;
            continue;
        };
        let Some(bpm_nouveau) = d.bpm else {
            echecs += 1;
            continue;
        };

        avant.push(*bpm_ancien);
        apres.push(bpm_nouveau);
        if (bpm_nouveau - bpm_ancien).abs() / bpm_ancien > 0.03 {
            deplaces.push((chemin.display().to_string(), *bpm_ancien, bpm_nouveau));
        }
    }
    eprintln!(
        "\r  {} morceaux mesurés, {echecs} en échec — {:.0} s ({:.1} s/morceau)",
        avant.len(),
        t.elapsed().as_secs_f64(),
        t.elapsed().as_secs_f64() / avant.len().max(1) as f64
    );

    let part = |v: &[f32], bas: f32, haut: f32| -> f64 {
        100.0 * v.iter().filter(|&&b| (bas..haut).contains(&b)).count() as f64 / v.len().max(1) as f64
    };
    println!(
        "\nConcentration dans [40, 90] BPM — avant : {:.1} %, après : {:.1} %",
        part(&avant, 40.0, 90.0),
        part(&apres, 40.0, 90.0)
    );
    println!(
        "Concentration dans [150, 267] BPM — avant : {:.1} %, après : {:.1} %",
        part(&avant, 150.0, 267.0),
        part(&apres, 150.0, 267.0)
    );

    println!(
        "\n{} morceaux déplacés de plus de 3 % ({:.1} % de l'échantillon) :",
        deplaces.len(),
        100.0 * deplaces.len() as f64 / avant.len().max(1) as f64
    );
    let mut tries = deplaces.clone();
    tries.sort_by(|a, b| (b.2 / b.1).abs().total_cmp(&(a.2 / a.1).abs()));
    for (chemin, av, ap) in tries.iter().take(20) {
        let court: String = chemin
            .rsplit('/')
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/");
        println!("  {av:>6.1} -> {ap:>6.1}  {:.2}x  {court}", ap / av);
    }
    Ok(())
}
