// SPDX-License-Identifier: GPL-3.0-or-later
//! Le recalage des attaques de la basse sur le stem, sur une transcription en
//! cache de l'application : combien de notes bougent, de combien, et ce que
//! devient une fenêtre de temps.
//!
//!   cargo run --release -p rusty-music-editor --example recaler_basse -- \
//!     "<dossier du morceau dans stems/>" [début_s fin_s [sortie.json]]
//!
//! Avec `sortie.json`, écrit la transcription recalée (même forme que le
//! cache) : `cargo run -p rusty-music-transcription --example mesures` la
//! remet en mesures.
use rusty_music_editor::coach::basse;
use rusty_music_editor::{decode, SR};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dossier = std::path::Path::new(&args[1]);
    let stem = std::fs::read_dir(dossier)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.to_string_lossy().ends_with("— bass.wav"))
        .ok_or("pas de stem de basse")?;
    let audio = decode::stereo(&stem)?;
    let mono: Vec<f32> = audio
        .gauche
        .iter()
        .zip(&audio.droite)
        .map(|(g, d)| 0.5 * (g + d))
        .collect();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(
        dossier.join("transcription-bass-muscriptor.json"),
    )?)?;
    let notes: Vec<(f64, u8)> = json["notes"]
        .as_array()
        .ok_or("pas de notes")?
        .iter()
        .map(|n| {
            (
                n["debut_s"].as_f64().unwrap(),
                n["hauteur"].as_u64().unwrap() as u8,
            )
        })
        .collect();
    let t0 = std::time::Instant::now();
    let du_stem = basse::notes_du_stem(&mono, SR as f32);
    let nouveaux = basse::recaler(&notes, &du_stem);
    let bouges: Vec<f64> = notes
        .iter()
        .zip(&nouveaux)
        .filter_map(|(n, x)| x.map(|x| (x - n.0) * 1000.0))
        .collect();
    let mut tri = bouges.clone();
    tri.sort_by(f64::total_cmp);
    let q = |p: f64| {
        tri.get(((tri.len() as f64 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    println!(
        "{} notes, {} attaques au stem ({} ms) ; {} recalées : écart (ms) quartiles {:.0} / {:.0} / {:.0}, plus tôt {} plus tard {}",
        notes.len(),
        du_stem.len(),
        t0.elapsed().as_millis(),
        bouges.len(),
        q(0.25),
        q(0.5),
        q(0.75),
        bouges.iter().filter(|&&x| x < 0.0).count(),
        bouges.iter().filter(|&&x| x > 0.0).count()
    );
    if args.len() >= 4 {
        let (a, b): (f64, f64) = (args[2].parse()?, args[3].parse()?);
        const N: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        for (n, x) in notes.iter().zip(&nouveaux) {
            if n.0 >= a && n.0 <= b {
                let nom = format!("{}{}", N[n.1 as usize % 12], n.1 as i32 / 12 - 1);
                match x {
                    Some(x) => println!("  {:7.3} {nom:4} → {x:7.3}", n.0),
                    None => println!("  {:7.3} {nom:4}", n.0),
                }
            }
        }
        let st: Vec<String> = du_stem
            .iter()
            .filter(|n| n.t_s >= a && n.t_s <= b)
            .map(|n| {
                format!(
                    "{:.3}{}c{:.2}",
                    n.t_s,
                    n.hauteur().map_or(String::new(), |m| format!("({m:.0})")),
                    n.confiance
                )
            })
            .collect();
        println!("  stem : {}", st.join(" "));
    }
    if let Some(sortie) = args.get(4) {
        let mut json = json.clone();
        for (n, x) in json["notes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(&nouveaux)
        {
            if let Some(x) = x {
                let (d, f) = (n["debut_s"].as_f64().unwrap(), n["fin_s"].as_f64().unwrap());
                n["debut_s"] = (*x).into();
                n["fin_s"] = f.max(x + 0.05).min(f + (x - d).max(0.0)).into();
            }
        }
        std::fs::write(sortie, serde_json::to_vec(&json)?)?;
    }
    Ok(())
}
