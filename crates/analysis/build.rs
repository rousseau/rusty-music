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

/// Dépose les poids dans `models/`, d'où l'empaqueteur les prendra.
///
/// **Seulement depuis un build `release`, et toujours en écrasant.** Deux
/// raisons, toutes deux payées d'un bogue :
///
/// - chaque profil de compilation régénère code *et* poids ; un emplacement
///   partagé entre profils est ambigu par construction. `cargo tauri build`
///   compile en release, donc c'est release qui a le droit d'écrire ;
/// - la version précédente comparait les **tailles** pour éviter une copie
///   inutile. Tous ces `.bpk` font exactement la même taille : la copie était
///   systématiquement sautée, et le paquet embarquait des poids venus d'un
///   build antérieur — ne correspondant au code d'aucun des deux profils.
///
/// Une copie de 117 Mo par build release coûte une fraction de seconde. Le
/// doute, lui, coûte une empreinte fausse sans le moindre message.
fn deposer_pour_le_paquet() {
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
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
