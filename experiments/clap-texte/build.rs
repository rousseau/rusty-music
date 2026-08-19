//! Génère du Rust natif depuis la tour texte de CLAP, au moment du build.
//!
//! **La question de l'essai.** La tour audio n'a pas été importable telle que
//! publiée : ses blocs Swin calculaient leurs marges à l'exécution, et il a
//! fallu figer la forme d'entrée puis replier les constantes. Rien ne disait
//! que la tour texte se comporterait mieux — c'est un RoBERTa, pas un Swin,
//! mais `burn-onnx` génère du Rust par inférence *statique* de formes, et un
//! transformeur de texte a lui aussi de quoi calculer des formes en chemin
//! (`CumSum` sur le masque pour les positions, `Expand` du masque d'attention).
//!
//! Le modèle est exporté par `sonder.py export`, à longueur de séquence figée.

use burn_onnx::ModelGen;

fn main() {
    ModelGen::new()
        .input("modeles/clap-text-encoder.onnx")
        .out_dir("model/")
        .run_from_script();
}
