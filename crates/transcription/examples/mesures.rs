// SPDX-License-Identifier: GPL-3.0-or-later
//! Les mesures d'une transcription de basse en cache (notes déjà doigtées),
//! sur la pulsation du morceau, en texte : corde et frette, durée en doubles
//! croches, `~` pour une note liée à la suivante.
//!
//!   cargo run --release -p rusty-music-transcription --example mesures -- \
//!     <transcription.json> <pulsation.json> [première mesure] [dernière]
use rusty_music_transcription::{quantification, Note};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let t: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let p: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let (de, a_) = (
        a.get(2).map_or(Ok(1), |x| x.parse())?,
        a.get(3).map_or(Ok(usize::MAX), |x| x.parse())?,
    );
    let notes: Vec<Note> = serde_json::from_value(t["notes"].clone())?;
    let f = |k: &str| -> Vec<f32> {
        p[k].as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() as f32)
            .collect()
    };
    let m = quantification::quantifier(&notes, &f("temps"), &f("premiers_temps"), 4);
    const CORDES: [&str; 5] = ["B", "E", "A", "D", "G"];
    let decal = if notes.iter().any(|n| n.corde == Some(4)) {
        0
    } else {
        1
    };
    for (i, m) in m
        .iter()
        .enumerate()
        .skip(de - 1)
        .take(a_.saturating_sub(de) + 1)
    {
        let ev: Vec<String> = m
            .evenements
            .iter()
            .map(|e| {
                let j = e.jeu.map_or("-".into(), |j| match (j.corde, j.frette) {
                    (Some(c), Some(f)) => format!("{}{f}", CORDES[c as usize + decal]),
                    _ => j.hauteur.to_string(),
                });
                format!("{j}/{}{}", e.seiziemes, if e.lie { "~" } else { "" })
            })
            .collect();
        println!("m{:<3} {:6.2}  {}", i + 1, m.debut_s, ev.join(" "));
    }
    Ok(())
}
