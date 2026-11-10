// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc F2 (`experiments/compat-stems/`) : prépare les paires « morceau
//! source × batterie candidate » que COCOLA, CLAP seul et le groove de F1
//! classeront ensuite.
//!
//! Lit `plan.json` (écrit par `experiments/compat-stems/choisir.py`) et, sous
//! `<racine>` :
//!
//! - `stems/<id>/` : les stems de chaque morceau (HTDemucs, la variante de
//!   Créer) et sa pulsation (`pulsation.json`, Beat This! sur le fichier
//!   d'origine, comme l'application) ;
//! - `greffes/<source>/contexte.wav` : la source sans sa batterie ;
//! - `greffes/<source>/<candidat>.wav` : la batterie du candidat posée
//!   mesure par mesure sur la grille de la source (`greffer_par_mesures`,
//!   sans sections), ou d'un bloc si la métrique l'interdit ;
//! - `greffes.jsonl` : une ligne par paire (mode, facteur, mesures, durée).
//!
//! Reprend où il s'est arrêté : ce qui existe déjà n'est pas refait.
//!
//!   cargo run --release -p rusty-music-editor --example banc_compat -- <plan.json> <racine>

use std::io::Write;
use std::path::{Path, PathBuf};

use rusty_music_editor::greffe::{self, Grille};
use rusty_music_editor::pulsation::{Modele, Pisteur, FICHIER_CACHE};
use rusty_music_editor::{decode, wav, Demixeur, Variante, SR};

/// Les stems d'un dossier, `(nom, chemin)`, sans les fichiers cachés
/// qu'un disque non APFS sème à côté (`._… — drums.wav`, journal V23).
fn stems(dossier: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dossier)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
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

fn batterie(dossier: &Path) -> Option<PathBuf> {
    stems(dossier)
        .into_iter()
        .find(|(n, _)| n == "drums")
        .map(|(_, p)| p)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [plan, racine] = a.as_slice() else {
        return Err("usage : banc_compat <plan.json> <racine>".into());
    };
    let racine = Path::new(racine);
    let plan: serde_json::Value = serde_json::from_slice(&std::fs::read(plan)?)?;
    let morceaux: Vec<(i64, String)> = plan["morceaux"]
        .as_array()
        .ok_or("plan : pas de « morceaux »")?
        .iter()
        .filter_map(|m| Some((m["id"].as_i64()?, m["path"].as_str()?.to_string())))
        .collect();
    let paires: Vec<(i64, i64)> = plan["paires"]
        .as_array()
        .ok_or("plan : pas de « paires »")?
        .iter()
        .filter_map(|p| Some((p["source"].as_i64()?, p["candidat"].as_i64()?)))
        .collect();
    let dossier = |id: i64| racine.join("stems").join(id.to_string());

    // 1. Démixage et pulsation de chaque morceau.
    let mut demixeur: Option<Demixeur> = None;
    let mut pisteur = Pisteur::charger(Modele::Complet)?;
    let debut = std::time::Instant::now();
    for (i, (id, chemin)) in morceaux.iter().enumerate() {
        let d = dossier(*id);
        let t = std::time::Instant::now();
        let mut fait = Vec::new();
        if batterie(&d).is_none() {
            let m = match demixeur.as_mut() {
                Some(m) => m,
                None => {
                    let m = Demixeur::charger(None, Variante::Standard)?;
                    m.chauffer();
                    demixeur.insert(m)
                }
            };
            match m.separer_fichier_suivi(Path::new(chemin), &d, |_| {}) {
                Ok(_) => fait.push("démixé"),
                Err(e) => {
                    println!("[{}/{}] {id} : démixage raté : {e}", i + 1, morceaux.len());
                    continue;
                }
            }
        }
        let cache = d.join(FICHIER_CACHE);
        if !cache.is_file() {
            pisteur.analyser_avec_cache(Path::new(chemin), &cache)?;
            fait.push("pulsation");
        }
        println!(
            "[{}/{}] {id} : {} en {:.1} s (total {:.0} s)",
            i + 1,
            morceaux.len(),
            if fait.is_empty() {
                "déjà prêt".to_string()
            } else {
                fait.join(", ")
            },
            t.elapsed().as_secs_f64(),
            debut.elapsed().as_secs_f64()
        );
        std::io::stdout().flush()?;
    }
    drop(demixeur);

    // 2. Contexte de chaque source et greffes.
    let grille = |id: i64| -> Option<Grille> {
        let p = rusty_music_editor::pulsation::Pulsation::lire(
            &dossier(id).join(FICHIER_CACHE),
            Modele::Complet,
        )?;
        Some(Grille {
            metrique: p.metrique(),
            premiers_temps: p.premiers_temps,
            sections: Vec::new(),
        })
    };
    let mut journal = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(racine.join("greffes.jsonl"))?;
    for (k, (source, candidat)) in paires.iter().enumerate() {
        let sortie_dossier = racine.join("greffes").join(source.to_string());
        let contexte = sortie_dossier.join("contexte.wav");
        if !contexte.is_file() {
            let mut g: Vec<f32> = Vec::new();
            let mut d: Vec<f32> = Vec::new();
            for (_, p) in stems(&dossier(*source))
                .into_iter()
                .filter(|(n, _)| n != "drums")
            {
                let s = decode::stereo(&p)?;
                g.resize(g.len().max(s.gauche.len()), 0.0);
                d.resize(d.len().max(s.droite.len()), 0.0);
                g.iter_mut().zip(&s.gauche).for_each(|(x, y)| *x += y);
                d.iter_mut().zip(&s.droite).for_each(|(x, y)| *x += y);
            }
            std::fs::create_dir_all(&sortie_dossier)?;
            wav::ecrire(&contexte, &g, &d, SR)?;
        }
        let sortie = sortie_dossier.join(format!("{candidat}.wav"));
        if sortie.is_file() {
            continue;
        }
        let (Some(remplace), Some(greffon)) =
            (batterie(&dossier(*source)), batterie(&dossier(*candidat)))
        else {
            println!("paire {source} × {candidat} : stems manquants");
            continue;
        };
        let t = std::time::Instant::now();
        let (gs, gg) = (grille(*source), grille(*candidat));
        let par_mesures = match (&gs, &gg) {
            (Some(a), Some(b)) => greffe::mesure_a_mesure(a, b),
            _ => Err("pulsation absente".into()),
        };
        let (mode, plan) = match par_mesures {
            Ok(()) => (
                "mesures".to_string(),
                greffe::greffer_par_mesures(
                    &remplace,
                    &greffon,
                    gs.as_ref().ok_or("grille")?,
                    gg.as_ref().ok_or("grille")?,
                    &sortie,
                ),
            ),
            Err(raison) => {
                let bpm = |g: &Option<Grille>| {
                    g.as_ref().and_then(|g| {
                        let mut d: Vec<f32> =
                            g.premiers_temps.windows(2).map(|w| w[1] - w[0]).collect();
                        d.sort_by(f32::total_cmp);
                        let m = d.get(d.len() / 2)?;
                        Some(60.0 * g.metrique.unwrap_or(4) as f32 / m)
                    })
                };
                (
                    format!("bloc ({raison})"),
                    greffe::greffer(&remplace, &greffon, bpm(&gs), bpm(&gg), None, &sortie),
                )
            }
        };
        let ligne = match plan {
            Ok(p) => serde_json::json!({
                "source": source, "candidat": candidat, "mode": mode,
                "facteur": p.facteur, "octaves": p.octaves, "mesures": p.mesures,
                "ms": t.elapsed().as_millis() as u64,
            }),
            Err(e) => serde_json::json!({
                "source": source, "candidat": candidat, "mode": mode,
                "erreur": e.to_string(),
            }),
        };
        writeln!(journal, "{ligne}")?;
        println!("greffe {}/{} : {ligne}", k + 1, paires.len());
        std::io::stdout().flush()?;
    }
    Ok(())
}
