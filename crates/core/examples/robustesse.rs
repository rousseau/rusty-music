// SPDX-License-Identifier: GPL-3.0-or-later
//! Robustesse de l'interprétation (`ollama::interpreter`) sur un jeu de
//! prompts annotés — `experiments/prompts-playlist/prompts.json`.
//!
//! `cargo run --release -p rusty-music-core --example robustesse -- \
//!    <base.db> <modele> [sortie.json]`
//!
//! Ouvrir une **copie** de la base. Écrit, pour chaque prompt, la spec rendue,
//! la latence et le verdict ; `composition_sur_la_vraie_bibliotheque`
//! (`apps/desktop`) relit ce fichier pour tester la composition.
use std::collections::BTreeMap;

use rusty_music_core::ollama::{self, InterpretationLlm};
use serde_json::{json, Value};

const FILTRES: &[&str] = &[
    "seed_artiste", "seed_morceau", "arrivee_artiste", "arrivee_morceau", "n", "duree_minutes",
    "genres", "exclure_genres", "exclure_artistes", "annee_min", "annee_max", "bpm_min",
    "bpm_max", "energie", "popularite", "plafond_par_artiste",
];

fn est_vide(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

/// Un élément attendu (`a|b` = alternatives) se retrouve parmi les valeurs
/// rendues, par inclusion dans un sens ou dans l'autre, casse ignorée.
fn trouve(attendu: &str, rendus: &[String]) -> bool {
    attendu.split('|').any(|alt| {
        let alt = alt.trim().to_lowercase();
        rendus.iter().any(|r| {
            let r = r.to_lowercase();
            r.contains(&alt) || alt.contains(&r)
        })
    })
}

fn verifier(cas: &Value, spec: &Value) -> Vec<String> {
    let mut ecarts = Vec::new();
    let attendu = cas["attendu"].as_object().cloned().unwrap_or_default();
    let libre: Vec<&str> = cas["libre"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let strict = cas.get("strict").and_then(Value::as_bool).unwrap_or(true);

    for champ in FILTRES {
        if libre.contains(champ) {
            continue;
        }
        let rendu = &spec[*champ];
        match attendu.get(*champ) {
            None if strict => {
                if !est_vide(rendu) {
                    ecarts.push(format!("{champ} inventé : {rendu}"));
                }
            }
            None => {}
            Some(Value::Null) => {
                if !est_vide(rendu) {
                    ecarts.push(format!("{champ} devait être vide : {rendu}"));
                }
            }
            Some(Value::Array(voulus)) => {
                let rendus: Vec<String> = rendu
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                if voulus.is_empty() {
                    if !rendus.is_empty() {
                        ecarts.push(format!("{champ} devait être vide : {rendus:?}"));
                    }
                }
                for v in voulus.iter().filter_map(Value::as_str) {
                    if !trouve(v, &rendus) {
                        ecarts.push(format!("{champ} sans « {v} » : {rendus:?}"));
                    }
                }
            }
            Some(Value::String(v)) => {
                let r = rendu.as_str().unwrap_or("");
                // Un nom : inclusion ; une énumération : égalité.
                let ok = if matches!(*champ, "energie" | "popularite") {
                    r.eq_ignore_ascii_case(v)
                } else {
                    trouve(v, &[r.to_string()])
                };
                if !ok {
                    ecarts.push(format!("{champ} = {rendu}, attendu « {v} »"));
                }
            }
            Some(v) => {
                // Nombre : égal, ou à 10 % près pour un tempo.
                let (a, r) = (v.as_f64().unwrap_or(f64::NAN), rendu.as_f64());
                let tol = if champ.starts_with("bpm") { a * 0.1 } else { 0.0 };
                if !r.is_some_and(|r| (r - a).abs() <= tol) {
                    ecarts.push(format!("{champ} = {rendu}, attendu {v}"));
                }
            }
        }
    }
    ecarts
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().expect("base.db");
    let modele = args.next().expect("modèle Ollama");
    let sortie = args.next();

    let lib = rusty_music_core::db::Library::open(std::path::Path::new(&base))?;
    let vocab = lib.vocabulaire_genres(usize::MAX)?;
    let jeu: Value = serde_json::from_str(&std::fs::read_to_string(
        "experiments/prompts-playlist/prompts.json",
    )?)?;
    let cas = jeu["cas"].as_array().expect("cas");

    println!("modèle {modele} — {} prompts, {} genres connus\n", cas.len(), vocab.len());
    let mut resultats = Vec::new();
    let mut par_categorie: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut par_champ: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut echecs_json, mut total_s) = (0usize, 0f64);
    let mut latences = Vec::new();

    // `ROB_IDS=a,b` : ne rejouer que ces cas.
    let seuls: Option<Vec<String>> =
        std::env::var("ROB_IDS").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    for c in cas {
        let id = c["id"].as_str().unwrap_or("?");
        if seuls.as_ref().is_some_and(|l| !l.iter().any(|x| x == id)) {
            continue;
        }
        let cat = c["categorie"].as_str().unwrap_or("?").to_string();
        let prompt = c["prompt"].as_str().unwrap_or("");
        let t = std::time::Instant::now();
        let r: Result<InterpretationLlm, _> =
            ollama::interpreter(ollama::HOTE_DEFAUT, &modele, prompt, &vocab);
        let s = t.elapsed().as_secs_f64();
        total_s += s;
        latences.push(s);
        let entree = par_categorie.entry(cat.clone()).or_default();
        entree.1 += 1;
        match r {
            Err(e) => {
                echecs_json += 1;
                println!("✗ {id:<22} ERREUR ({s:.0} s) : {e}");
                resultats.push(json!({"id": id, "prompt": prompt, "erreur": e.to_string(), "s": s}));
            }
            Ok(p) => {
                let spec = serde_json::to_value(&p)?;
                let ecarts = verifier(c, &spec);
                for e in &ecarts {
                    if let Some(champ) = FILTRES.iter().find(|f| e.starts_with(*f)) {
                        *par_champ.entry(champ).or_default() += 1;
                    }
                }
                if ecarts.is_empty() {
                    entree.0 += 1;
                    println!("✓ {id:<22} ({s:.0} s)");
                } else {
                    println!("✗ {id:<22} ({s:.0} s) « {prompt} »");
                    for e in &ecarts {
                        println!("      {e}");
                    }
                }
                resultats.push(json!({"id": id, "prompt": prompt, "spec": spec, "ecarts": ecarts, "s": s,
                                      "exclusions_dures": c["exclusions_dures"]}));
            }
        }
    }

    let ok: usize = par_categorie.values().map(|v| v.0).sum();
    println!("\n== {modele} : {ok}/{} prompts conformes, {echecs_json} erreurs d'interprétation", cas.len());
    for (cat, (o, n)) in &par_categorie {
        println!("   {cat:<12} {o}/{n}");
    }
    if !par_champ.is_empty() {
        println!("   écarts par champ : {par_champ:?}");
    }
    latences.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "   latence : médiane {:.1} s, max {:.1} s, total {:.0} s",
        latences[latences.len() / 2],
        latences.last().unwrap(),
        total_s
    );
    if let Some(f) = sortie {
        std::fs::write(f, serde_json::to_string_pretty(&json!({"modele": modele, "resultats": resultats}))?)?;
    }
    Ok(())
}
