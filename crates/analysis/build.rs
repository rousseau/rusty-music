// SPDX-License-Identifier: GPL-3.0-or-later
//! Génère l'encodeur CLAP en Rust natif, depuis l'ONNX, au moment du build.
//!
//! `burn-onnx` produit deux fichiers dans `OUT_DIR` : le code du modèle
//! (~4 400 lignes) et ses poids (`.bpk`, 117 Mo), ces derniers chargés à
//! l'exécution et non embarqués dans le binaire.
//!
//! L'entrée n'est **pas** le modèle publié mais sa version à formes figées,
//! produite par `scripts/preparer-modele.sh` — voir ce script pour le pourquoi,
//! et `experiments/burn-clap/README.md` pour la mesure qui l'a établi.

use std::path::Path;

/// Nom de base, partagé par l'ONNX d'entrée et les poids produits.
const POIDS: &str = "clap-audio-encoder-b5";
const MODELE: &str = "../../models/clap-audio-encoder-b5.onnx";

fn main() {
    println!("cargo:rerun-if-changed={MODELE}");
    println!("cargo:rerun-if-changed=build.rs");

    if !Path::new(MODELE).exists() {
        // Un message qui dit quoi faire : sans lui, l'échec est une trace de
        // `burn-onnx` incompréhensible pour qui n'a pas suivi la migration.
        panic!(
            "\n\n  Modèle absent : {MODELE}\n\
             \n  Le préparer une fois :\n\
             \n      ./scripts/preparer-modele.sh\n\
             \n  (le script rappelle comment récupérer le modèle d'origine)\n\n"
        );
    }

    burn_onnx::ModelGen::new()
        .input(MODELE)
        .out_dir("model/")
        .run_from_script();

    signaler_les_poids();
    deposer_pour_le_paquet();
}

/// Publie le chemin des poids générés, pour que le binaire les retrouve.
///
/// `burn-onnx` les laisse dans `OUT_DIR` et code en dur ce chemin absolu dans
/// le `Default` du modèle — inutilisable dès que le binaire quitte la machine
/// de build. `Embedder::charger` lit donc `RM_POIDS` en priorité : c'est le
/// seul chemin qui désigne à coup sûr les poids allant avec le code exécuté.
fn signaler_les_poids() {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let poids = Path::new(&out).join("model").join(format!("{POIDS}.bpk"));
    println!("cargo:rustc-env=RM_POIDS={}", poids.display());
}

/// Dépose les poids dans `models/`, où `apps/desktop/tauri.conf.json` les
/// déclare comme ressource du paquet.
///
/// **À chaque build, en écrasant.** Le `.bpk` est déterministe : `burn-onnx`
/// génère les mêmes octets depuis le même `.onnx`, quel que soit l'`opt-level`.
/// Le faire aussi en debug règle deux choses : `cargo build`/`clippy` de
/// `apps/desktop` échouait tant que le fichier n'existait pas (tauri-build
/// vérifie l'existence des ressources), et un `cargo tauri build` n'est plus
/// le seul à savoir produire un paquet cohérent.
///
/// Deux bogues passés, pour mémoire : comparer les **tailles** pour éviter une
/// copie sautait toujours (tous les `.bpk` font la même taille), et le paquet
/// embarquait alors des poids d'un build antérieur. D'où la copie
/// inconditionnelle — 117 Mo, une fraction de seconde.
fn deposer_pour_le_paquet() {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let source = Path::new(&out).join("model").join(format!("{POIDS}.bpk"));
    let dossier = Path::new("../../models");
    if !dossier.is_dir() {
        return;
    }
    if let Err(e) = std::fs::copy(&source, dossier.join(format!("{POIDS}.bpk"))) {
        println!("cargo:warning=poids non recopiés : {e}");
    }
}
