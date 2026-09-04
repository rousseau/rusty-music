// SPDX-License-Identifier: GPL-3.0-or-later
//! Génère du Rust natif depuis le modèle ONNX, au moment du build.
//!
//! C'est tout l'essai : `burn-onnx` déclare supporter les 34 opérateurs que ce
//! graphe emploie, mais un opérateur supporté n'est pas un graphe importé —
//! la génération travaille par inférence statique de formes.
//!
//! **Le modèle tel que publié ne passe pas** : ses douze blocs Swin ajustent
//! leurs marges (`Pad`) à partir de `height` et `width`, déclarées dynamiques
//! dans l'export, et `burn-onnx` rend « Runtime pads are not supported ».
//!
//! On lui donne donc le modèle **figé** sur la seule forme qu'on lui présente
//! jamais — 1 × 1 × 1001 × 64 —, replié par `onnxsim`. Les marges deviennent
//! alors des constantes, et avec elles disparaissent 45 `ScatterND`, 190
//! `Where`, 225 `Expand`, 180 `Range` : 8 031 nœuds tombent à ~1 275. Voir le
//! README pour la commande de génération.

use burn_onnx::ModelGen;

fn main() {
    ModelGen::new()
        .input("../../models/clap-audio-encoder-b5.onnx")
        .out_dir("model/")
        .run_from_script();
}
