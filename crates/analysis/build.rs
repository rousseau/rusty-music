// SPDX-License-Identifier: GPL-3.0-or-later
//! Génère les encodeurs CLAP en Rust natif, depuis l'ONNX, au moment du build.
//!
//! `burn-onnx` produit deux fichiers dans `OUT_DIR` par modèle : le code du
//! modèle et ses poids (`.bpk`), ces derniers chargés à l'exécution et non
//! embarqués dans le binaire.
//!
//! Deux modèles, même mécanique : l'encodeur **audio** (`encodeur.rs`), et
//! l'encodeur **texte** (`encodeur_texte.rs`) qui projette une description en
//! langage naturel dans le même espace à 512 dimensions — c'est lui qui rend
//! possible le champ d'intention d'Explorer (« texte → playlist »).
//!
//! Aucun des deux n'est le modèle publié tel quel : les formes dynamiques
//! (marges des blocs Swin côté audio, masque d'attention côté texte)
//! empêchent `burn-onnx` de générer du code. Voir
//! `scripts/preparer-modele.sh` / `scripts/preparer-clap-texte.sh` pour la
//! préparation qui fige ces formes, et `experiments/burn-clap/README.md` /
//! `experiments/clap-texte/README.md` pour la mesure qui l'a établi.

use std::path::Path;

struct Modele {
    /// Nom de base, partagé par l'ONNX d'entrée et les poids produits.
    poids: &'static str,
    onnx: &'static str,
    script_manquant: &'static str,
}

const AUDIO: Modele = Modele {
    poids: "clap-audio-encoder-b5",
    onnx: "../../models/clap-audio-encoder-b5.onnx",
    script_manquant: "./scripts/preparer-modele.sh",
};

const TEXTE: Modele = Modele {
    poids: "clap-text-encoder",
    onnx: "../../models/clap-text-encoder.onnx",
    script_manquant: "./scripts/preparer-clap-texte.sh",
};

fn main() {
    println!("cargo:rerun-if-changed={}", AUDIO.onnx);
    println!("cargo:rerun-if-changed={}", TEXTE.onnx);
    println!("cargo:rerun-if-changed=build.rs");

    generer(&AUDIO);
    generer(&TEXTE);
}

fn generer(modele: &Modele) {
    if !Path::new(modele.onnx).exists() {
        // Un message qui dit quoi faire : sans lui, l'échec est une trace de
        // `burn-onnx` incompréhensible pour qui n'a pas suivi la migration.
        panic!(
            "\n\n  Modèle absent : {}\n\
             \n  Le préparer une fois :\n\
             \n      {}\n\
             \n  (le script rappelle comment récupérer le modèle d'origine)\n\n",
            modele.onnx, modele.script_manquant
        );
    }

    burn_onnx::ModelGen::new()
        .input(modele.onnx)
        .out_dir("model/")
        .run_from_script();

    signaler_les_poids(modele);
    deposer_pour_le_paquet(modele);
}

/// Publie le chemin des poids générés, pour que le binaire les retrouve.
///
/// `burn-onnx` les laisse dans `OUT_DIR` et code en dur ce chemin absolu dans
/// le `Default` du modèle — inutilisable dès que le binaire quitte la machine
/// de build. `Embedder::charger`/`EmbedderTexte::charger` lisent donc une
/// variable d'environnement `RM_POIDS_<NOM>` en priorité : c'est le seul
/// chemin qui désigne à coup sûr les poids allant avec le code exécuté.
///
/// **Des poids venus d'un autre build ne provoquent aucune erreur** — Burn
/// charge ce qu'il reconnaît et laisse le reste à l'initialisation, d'où des
/// empreintes silencieusement fausses. C'est ce que vérifie l'exemple
/// `empreinte_reference` côté audio.
fn signaler_les_poids(modele: &Modele) {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let poids = Path::new(&out)
        .join("model")
        .join(format!("{}.bpk", modele.poids));
    println!(
        "cargo:rustc-env=RM_POIDS_{}={}",
        variable(modele.poids),
        poids.display()
    );
}

fn variable(poids: &str) -> String {
    poids.to_uppercase().replace('-', "_")
}

/// Dépose les poids frais dans `models/`, où `apps/desktop/tauri.conf.json` les
/// déclare comme ressource du paquet.
///
/// **Seulement en `release`, et toujours en écrasant.** Voir la documentation
/// de l'ancienne version mono-modèle de cette fonction pour le pourquoi
/// (bogue passé : comparer les tailles pour éviter la copie la sautait
/// toujours, tous les `.bpk` faisant la même taille).
fn deposer_pour_le_paquet(modele: &Modele) {
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let source = Path::new(&out)
        .join("model")
        .join(format!("{}.bpk", modele.poids));
    let dossier = Path::new("../../models");
    if !dossier.is_dir() {
        return;
    }
    if let Err(e) = std::fs::copy(&source, dossier.join(format!("{}.bpk", modele.poids))) {
        println!("cargo:warning=poids non recopiés ({}) : {e}", modele.poids);
    }
}
