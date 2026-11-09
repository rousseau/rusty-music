// SPDX-License-Identifier: GPL-3.0-or-later
//! Le groove (densité, syncope, swing) des batteries d'un cache de stems.
//!
//! F1, chercher le greffon : vérifie sur de vraies batteries démixées que les
//! trois nombres de `groove.rs` rangent les morceaux comme l'oreille. Lit, ne
//! modifie rien.
//!
//! ```bash
//! cargo run --release -p rusty-music-analysis --features sans-modeles \
//!   --example groove_stems -- "$HOME/Library/Application Support/fm.rustymusic.desktop/stems"
//! ```

use std::path::Path;

fn main() -> anyhow::Result<()> {
    let racine = std::env::args().nth(1).expect("dossier de stems");
    let analyseur = rusty_music_analysis::descripteurs::Analyseur::new();
    let mut lignes = Vec::new();
    for morceau in std::fs::read_dir(&racine)?.flatten() {
        let dossier = morceau.path();
        let Some(drums) = std::fs::read_dir(&dossier)?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.to_string_lossy().ends_with("drums.wav"))
        else {
            continue;
        };
        let Ok(pulsation) = std::fs::read_to_string(dossier.join("pulsation.json")) else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(&pulsation)?;
        let temps: Vec<f32> = v["temps"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_f64())
                    .map(|x| x as f32)
                    .collect()
            })
            .unwrap_or_default();
        let debut = std::time::Instant::now();
        let mono = rusty_music_analysis::decode::mono_complet(Path::new(&drums))?;
        let g = rusty_music_analysis::groove::groove(&mono, 48_000, &analyseur, &temps);
        let nom = dossier.file_name().unwrap().to_string_lossy().to_string();
        lignes.push((nom, g, debut.elapsed().as_millis()));
    }
    lignes.sort_by(|a, b| {
        let s = |g: &Option<rusty_music_analysis::groove::Groove>| g.map_or(-1.0, |g| g.syncope);
        s(&b.1).total_cmp(&s(&a.1))
    });
    println!("| morceau | syncope | attaques/temps | swing | temps | ms |");
    println!("|---|---|---|---|---|---|");
    for (nom, g, ms) in lignes {
        match g {
            Some(g) => println!(
                "| {nom} | {:.2} | {:.1} | {} | {} | {ms} |",
                g.syncope,
                g.densite,
                g.swing.map_or("—".into(), |s| format!("{s:.2}")),
                g.temps
            ),
            None => println!("| {nom} | — | — | — | — | {ms} |"),
        }
    }
    Ok(())
}
