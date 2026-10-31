// SPDX-License-Identifier: GPL-3.0-or-later
//! Greffe mesure par mesure sur deux morceaux déjà séparés par l'application
//! (dossiers de stems `<titre> — <stem>.wav`, avec `pulsation.json` et
//! `sections.json` s'ils y sont). Écrit le stem greffé et, à côté, le mélange
//! des autres stems de la source avec lui, pour l'écoute.
//!
//!   cargo run --release -p rusty-music-editor --example greffe_mesures -- <dossier source> <dossier greffon> <stem> <sortie.wav>
//!
//! `BLOC=1` : la greffe d'un bloc d'avant (tempo médian, calée sur le
//! premier temps), pour comparer.
//!
//! Sans `pulsation.json`, la pulsation est calculée sur la somme des stems
//! (l'application, elle, la calcule sur le fichier d'origine).

use std::path::{Path, PathBuf};

use rusty_music_editor::greffe::{self, Grille, Partie};
use rusty_music_editor::pulsation::{Modele, Pisteur, Pulsation, FICHIER_CACHE};
use rusty_music_editor::{decode, wav, SR};

fn stems(dossier: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dossier)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .filter_map(|p| {
            let n = p
                .file_stem()?
                .to_string_lossy()
                .rsplit('—')
                .next()?
                .trim()
                .to_string();
            Some((n, p))
        })
        .collect();
    v.sort();
    v
}

fn grille(dossier: &Path) -> Result<(Grille, Vec<f32>), Box<dyn std::error::Error>> {
    let p = match Pulsation::lire(&dossier.join(FICHIER_CACHE), Modele::Complet) {
        Some(p) => p,
        None => {
            let mut mono: Vec<f32> = Vec::new();
            for (_, w) in stems(dossier) {
                let s = decode::stereo(&w)?;
                mono.resize(mono.len().max(s.gauche.len()), 0.0);
                for (i, (g, d)) in s.gauche.iter().zip(&s.droite).enumerate() {
                    mono[i] += 0.5 * (g + d);
                }
            }
            Pisteur::charger(Modele::Complet)?.analyser(mono, SR)?
        }
    };
    let sections = std::fs::read(dossier.join("sections.json"))
        .ok()
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o).ok())
        .and_then(|v| {
            v["sections"].as_array().map(|a| {
                a.iter()
                    .map(|x| Partie {
                        debut_s: x["debut_s"].as_f64().unwrap_or(0.0) as f32,
                        fin_s: x["fin_s"].as_f64().unwrap_or(0.0) as f32,
                        etiquette: x["etiquette"].as_str().unwrap_or("").into(),
                    })
                    .collect()
            })
        })
        .unwrap_or_default();
    Ok((
        Grille {
            metrique: p.metrique(),
            premiers_temps: p.premiers_temps,
            sections,
        },
        p.temps,
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 4 {
        return Err(
            "usage : greffe_mesures <dossier source> <dossier greffon> <stem> <sortie.wav>".into(),
        );
    }
    let (ds, dg, stem, sortie) = (
        Path::new(&a[0]),
        Path::new(&a[1]),
        a[2].as_str(),
        PathBuf::from(&a[3]),
    );
    let trouver = |d: &Path| {
        stems(d)
            .into_iter()
            .find(|(n, _)| n == stem)
            .map(|(_, p)| p)
            .ok_or(format!("pas de stem {stem} dans {}", d.display()))
    };
    let (remplace, greffon) = (trouver(ds)?, trouver(dg)?);
    let ((gs, ts), (gg, tg)) = (grille(ds)?, grille(dg)?);
    println!(
        "source : {} mesures, {:?}/4, {} sections · greffon : {} mesures, {:?}/4, {} sections",
        gs.premiers_temps.len(),
        gs.metrique,
        gs.sections.len(),
        gg.premiers_temps.len(),
        gg.metrique,
        gg.sections.len()
    );
    let t0 = std::time::Instant::now();
    let plan = if std::env::var_os("BLOC").is_some() {
        let periode = |t: &[f32]| {
            let mut d: Vec<f32> = t.windows(2).map(|w| w[1] - w[0]).collect();
            d.sort_by(f32::total_cmp);
            d[d.len() / 2]
        };
        let cale = greffe::Cale {
            phase_remplace_s: ts[0],
            phase_greffon_s: tg[0],
            periode_greffon_s: periode(&tg),
        };
        greffe::greffer(
            &remplace,
            &greffon,
            Some(60.0 / periode(&ts)),
            Some(60.0 / periode(&tg)),
            Some(cale),
            &sortie,
        )?
    } else {
        greffe::mesure_a_mesure(&gs, &gg)?;
        greffe::greffer_par_mesures(&remplace, &greffon, &gs, &gg, &sortie)?
    };
    println!("{plan:?} en {:.1?}", t0.elapsed());

    // Le mélange : les autres stems de la source et le greffon.
    let mut g: Vec<f32> = Vec::new();
    let mut d: Vec<f32> = Vec::new();
    for p in stems(ds)
        .into_iter()
        .filter(|(n, _)| n != stem)
        .map(|(_, p)| p)
        .chain([sortie.clone()])
    {
        let s = decode::stereo(&p)?;
        g.resize(g.len().max(s.gauche.len()), 0.0);
        d.resize(d.len().max(s.droite.len()), 0.0);
        g.iter_mut().zip(&s.gauche).for_each(|(x, y)| *x += y);
        d.iter_mut().zip(&s.droite).for_each(|(x, y)| *x += y);
    }
    let crete = g
        .iter()
        .chain(&d)
        .fold(0.0f32, |m, x| m.max(x.abs()))
        .max(1.0);
    g.iter_mut().chain(d.iter_mut()).for_each(|x| *x /= crete);
    let mix = sortie.with_file_name(format!(
        "{}-mix.wav",
        sortie.file_stem().unwrap_or_default().to_string_lossy()
    ));
    wav::ecrire(&mix, &g, &d, SR)?;
    println!("écrit : {} et {}", sortie.display(), mix.display());
    Ok(())
}
