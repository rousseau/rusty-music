//! Traduit HTDemucs en Rust natif, au moment du build.
//!
//! Contrairement à CLAP, ce modèle déclare déjà son entrée en dur —
//! `[1, 2, 343980]`, soit 7,8 s de stéréo à 44,1 kHz. Ça n'a pas suffi : le
//! graphe publié calcule tant de formes à l'exécution que la génération
//! s'arrête sur `ReduceProd` appliqué à une forme et non à un tenseur
//! (« expected Tensor, got Shape(3) »).
//!
//! On lui donne donc le graphe replié par ONNX Runtime, même recette que pour
//! CLAP : `ORT_ENABLE_BASIC` évalue tout ce calcul de formes une fois pour
//! toutes. 24 765 nœuds tombent à 1 453, et `ReduceProd`, `ScatterND`,
//! `Range`, `Expand`, `Shape` disparaissent complètement. Sortie vérifiée
//! identique sous ORT — RMS des quatre stems inchangés.

fn main() {
    println!("cargo:rerun-if-changed=../../models/htdemucs-fige.onnx");
    burn_onnx::ModelGen::new()
        .input("../../models/htdemucs-fige.onnx")
        .out_dir("model/")
        .run_from_script();
}
