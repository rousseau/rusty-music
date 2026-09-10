// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostic du tempo, morceau par morceau — les grandeurs intermédiaires
//! de chacune des 5 fenêtres (gagnant de grille, alternance, rapport au
//! double, correction appliquée) plutôt que le seul BPM final, pour voir si
//! une erreur d'octave rapportée par l'écoute vient d'une correction qui ne
//! se déclenche pas, ou d'un gagnant de grille déjà faux avant toute
//! correction. Complète `octave_avant_apres.rs` (mesure sur un échantillon,
//! avant/après) par un examen fin d'un petit nombre de cas.
//!
//! Affiche deux médianes : « brute » (avant `stabiliser_corrections`, pour
//! voir le défaut) et « stabilisée » (ce que `passe::descripteurs` écrit
//! réellement en base, `analyser_fenetres` l'applique déjà).
//!
//!   cargo run --release -p rusty-music-analysis --example diagnostic_tempo -- <fichier…>

use std::path::PathBuf;

use rusty_music_analysis::descripteurs::{stabiliser_corrections, tempos_par_fenetre, Analyseur};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let chemins: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if chemins.is_empty() {
        return Err("usage : diagnostic_tempo <fichier…>".into());
    }

    let a = Analyseur::new();
    for chemin in &chemins {
        let texte = chemin.display().to_string();
        let court = texte
            .rsplit('/')
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/");
        println!("{court}");
        match tempos_par_fenetre(chemin, &a) {
            Ok(diagnostics) => {
                let mediane = |vals: &mut Vec<f32>| -> Option<f32> {
                    vals.sort_by(f32::total_cmp);
                    vals.get(vals.len() / 2).copied()
                };

                let mut bruts: Vec<f32> = diagnostics.iter().filter_map(|d| d.map(|d| d.bpm)).collect();
                let mediane_brute = mediane(&mut bruts);

                let mut stabilises: Vec<_> = diagnostics.iter().filter_map(|d| *d).collect();
                stabiliser_corrections(&mut stabilises);
                let mut apres: Vec<f32> = stabilises.iter().map(|d| d.bpm).collect();
                let mediane_stabilisee = mediane(&mut apres);

                for (i, d) in diagnostics.iter().enumerate() {
                    match d {
                        Some(d) => {
                            let correction = if d.corrige_vers_le_haut {
                                "×2"
                            } else if d.corrige_vers_le_bas {
                                "÷2"
                            } else if d.corrige_vers_le_bas_rapide {
                                "÷2r"
                            } else {
                                "—"
                            };
                            let apres = stabilises.get(i).map(|s| s.bpm).unwrap_or(d.bpm);
                            let stabilisee = if (apres - d.bpm).abs() > 0.1 {
                                format!("  (stabilisée -> {apres:.1})")
                            } else {
                                String::new()
                            };
                            println!(
                                "  fenêtre {i} : grille {:>6.1}  alt {:.2} (s.0,60)  brut(g) {:.2} (s.0,50/0,72)  2g/g {:.2} (s.1,02)  g÷2/g {:.2} (s.0,82)  corr {correction:<3}  -> {:>6.1} BPM{stabilisee}",
                                d.gagnant_grille, d.alternance, d.brut_gagnant, d.brut_double_sur_brut, d.brut_moitie_sur_brut, d.bpm,
                            );
                        }
                        None => println!("  fenêtre {i} : silencieuse"),
                    }
                }
                println!(
                    "  médiane brute : {}  —  médiane stabilisée (celle écrite en base) : {}",
                    mediane_brute.map_or("—".to_string(), |b| format!("{b:.1} BPM")),
                    mediane_stabilisee.map_or("—".to_string(), |b| format!("{b:.1} BPM")),
                );
            }
            Err(e) => println!("  échec : {e}"),
        }
    }
    Ok(())
}
