// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc des doigtés : repose les notes de tablatures de référence (les
//! hauteurs de la tablature elle-même, sans erreur de transcription) avec une
//! grille de [`Couts`], et compte les notes jouées à la même corde et la même
//! frette que la référence. Le corpus est écrit par
//! `experiments/partitions/corpus_doigtes.py`.
//!
//!   cargo run --release -p rusty-music-transcription --example doigtes -- <corpus.json> <grille.json> <sortie.json> [detail.json]
//!
//! `corpus.json` : `[{"nom", "cordes": [grave → aiguë], "notes": [Note], "verite": [[corde, frette]]}]`.
//! `grille.json` : `[{"corde", "deplacement", ...}]` — les champs absents
//! gardent la valeur de [`Couts::default`].
//! Sortie : `[{"couts": {...}, "par_morceau": {nom: [identiques, total]}}]`.

use std::collections::BTreeMap;

use rusty_music_transcription::tablature::{self, Accordage, Couts};
use rusty_music_transcription::Note;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Morceau {
    nom: String,
    cordes: Vec<u8>,
    notes: Vec<Note>,
    verite: Vec<(u8, u8)>,
}

fn couts(v: &Value) -> Couts {
    let d = Couts::default();
    let f = |k: &str, x: f32| v[k].as_f64().map_or(x, |y| y as f32);
    Couts {
        corde: f("corde", d.corde),
        deplacement: f("deplacement", d.deplacement),
        pente_bas: f("pente_bas", d.pente_bas),
        debut_haut: v["debut_haut"].as_u64().map_or(d.debut_haut, |y| y as u8),
        pente_haut: f("pente_haut", d.pente_haut),
        vide_haut: f("vide_haut", d.vide_haut),
        corde_aigue: f("corde_aigue", d.corde_aigue),
        position: f("position", d.position),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let corpus: Vec<Morceau> = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let grille: Vec<Value> = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let fils = std::thread::available_parallelism().map_or(4, |n| n.get());
    let resultats: Vec<Value> = std::thread::scope(|s| {
        let morceaux = &corpus;
        let taches: Vec<_> = grille
            .chunks(grille.len().div_ceil(fils).max(1))
            .map(|lot| {
                s.spawn(move || {
                    lot.iter()
                        .map(|g| {
                            let k = couts(g);
                            let par: BTreeMap<&str, (usize, usize)> = morceaux
                                .iter()
                                .map(|m| {
                                    let mut x = m.notes.clone();
                                    tablature::poser_avec(&mut x, &Accordage { cordes: m.cordes.clone(), frettes: 24 }, &k);
                                    let ok = x.iter().zip(&m.verite).filter(|(n, v)| (n.corde, n.frette) == (Some(v.0), Some(v.1))).count();
                                    (m.nom.as_str(), (ok, m.verite.len()))
                                })
                                .collect();
                            json!({ "couts": g, "par_morceau": par })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        taches.into_iter().flat_map(|t| t.join().unwrap()).collect()
    });
    std::fs::write(&a[2], serde_json::to_string(&resultats)?)?;
    // Détail du premier réglage : (corde, frette) posées et de référence, note à note.
    if let (Some(chemin), Some(g)) = (a.get(3), grille.first()) {
        let detail: Vec<Value> = corpus
            .iter()
            .map(|m| {
                let mut x = m.notes.clone();
                tablature::poser_avec(&mut x, &Accordage { cordes: m.cordes.clone(), frettes: 24 }, &couts(g));
                let poses: Vec<(Option<u8>, Option<u8>)> = x.iter().map(|n| (n.corde, n.frette)).collect();
                json!({ "nom": m.nom, "cordes": m.cordes, "poses": poses, "verite": m.verite, "hauteurs": x.iter().map(|n| n.hauteur).collect::<Vec<_>>(), "debuts": x.iter().map(|n| n.debut_s).collect::<Vec<_>>() })
            })
            .collect();
        std::fs::write(chemin, serde_json::to_string(&detail)?)?;
    }
    println!("{} réglages × {} morceaux → {}", grille.len(), corpus.len(), a[2]);
    Ok(())
}
