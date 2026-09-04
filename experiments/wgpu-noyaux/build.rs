// SPDX-License-Identifier: GPL-3.0-or-later
//! Traduit les neuf modèles d'un opérateur en Rust natif.
//!
//! Chacun ne contient qu'un nœud : si la génération échoue sur l'un d'eux,
//! c'est déjà une réponse — un opérateur que `burn-onnx` ne sait pas importer
//! ne peut pas être celui qui calcule faux à l'exécution.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=modeles");
    let dossier = Path::new("modeles");
    let mut noms: Vec<String> = std::fs::read_dir(dossier)
        .expect("dossier modeles — lancer generer.py d'abord")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "onnx"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .collect();
    noms.sort();

    let mut gen = burn_onnx::ModelGen::new();
    for n in &noms {
        gen.input(&format!("modeles/{n}.onnx"));
    }
    gen.out_dir("model/").run_from_script();

    // La liste part au binaire : il doit savoir ce qui a été généré.
    println!("cargo:rustc-env=NOYAUX={}", noms.join(","));
}
