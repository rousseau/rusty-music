//! Quel noyau wgpu calcule faux ?
//!
//! HTDemucs importé depuis ONNX rend des stems justes sur processeur et faux
//! sur Metal (`docs/module3-demixage.md`). CLAP, lui, passe sur Metal au
//! cosinus 1,0000000000. Le défaut est donc dans un opérateur que l'un emploie
//! et l'autre pas — neuf candidats.
//!
//! Chaque modèle ici ne contient qu'un nœud. On le fait tourner sur le backend
//! choisi et on compare à la référence ONNX Runtime produite par `generer.py`.
//! Comparer les deux backends l'un à l'autre aurait dit qu'ils divergent, pas
//! lequel a tort.
//!
//!   cargo run --release                                        # Metal
//!   cargo run --release --no-default-features --features cpu   # processeur

use burn::tensor::{Tensor, TensorData};

#[cfg(feature = "metal")]
type Moteur = burn::backend::Wgpu<f32, i32>;
#[cfg(not(feature = "metal"))]
type Moteur = burn::backend::NdArray<f32>;

#[allow(clippy::all, dead_code, unused_variables, non_snake_case)]
mod noyaux {
    macro_rules! modele {
        ($nom:ident, $fichier:literal) => {
            pub mod $nom {
                include!(concat!(env!("OUT_DIR"), "/model/", $fichier, ".rs"));
            }
        };
    }
    modele!(clip, "clip");
    modele!(convtranspose, "convtranspose");
    modele!(cos, "cos");
    modele!(instancenormalization, "instancenormalization");
    modele!(layernormalization, "layernormalization");
    modele!(sigmoid, "sigmoid");
    modele!(sin, "sin");
    modele!(split, "split");
    modele!(tile, "tile");
    modele!(gather, "gather");
    modele!(unsqueeze, "unsqueeze");
    modele!(instancenormalization_grand, "instancenormalization_grand");
    modele!(convtranspose_grand, "convtranspose_grand");
    modele!(layernormalization_grand, "layernormalization_grand");
}

/// Doit rester rigoureusement identique à `entree()` de `generer.py`.
fn entree(taille: usize) -> Vec<f32> {
    (0..taille).map(|i| (i as f32 * 0.017).sin() * 0.8).collect()
}

/// Chiffres relevés sous ONNX Runtime, écrits par `generer.py`.
///
/// Inclus tel quel plutôt que lu au démarrage : la référence doit être figée
/// dans le binaire, sinon on compare un résultat à un fichier qui a pu bouger
/// entre-temps.
include!("../modeles/reference.rs");

fn main() {
    println!(
        "backend : {}\n",
        if cfg!(feature = "metal") { "wgpu (Metal)" } else { "ndarray (processeur)" }
    );
    let device = burn::tensor::Device::<Moteur>::default();

    let mut resultats: Vec<(&str, f64, f64)> = Vec::new();

    macro_rules! essai {
        ($nom:literal, $module:ident, $forme:expr) => {{
            let forme = $forme;
            let n: usize = forme.iter().product();
            let x = Tensor::<Moteur, 1>::from_data(TensorData::new(entree(n), [n]), &device);
            let m = noyaux::$module::Model::<Moteur>::default();
            let y = m.forward(x.reshape(forme));
            let v: Vec<f32> = y.into_data().to_vec().expect("sortie f32");
            let somme: f64 = v.iter().map(|x| *x as f64).sum();
            let norme: f64 = v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
            resultats.push(($nom, somme, norme));
        }};
    }

    essai!("clip", clip, [1, 32, 32, 64]);
    essai!("convtranspose", convtranspose, [1, 16, 16, 32]);
    essai!("cos", cos, [1, 64, 128]);
    essai!("instancenormalization", instancenormalization, [1, 32, 32, 64]);
    essai!("layernormalization", layernormalization, [1, 64, 128]);
    essai!("sigmoid", sigmoid, [1, 32, 32, 64]);
    essai!("sin", sin, [1, 64, 128]);
    // `Split` rend deux sorties : son `forward` produit un tuple. On juge la
    // première — si la découpe est fausse, elle le sera.
    {
        let n: usize = 1 * 64 * 128;
        let x = Tensor::<Moteur, 1>::from_data(TensorData::new(entree(n), [n]), &device);
        let m = noyaux::split::Model::<Moteur>::default();
        let (y, _autre) = m.forward(x.reshape([1, 64, 128]));
        let v: Vec<f32> = y.into_data().to_vec().expect("sortie f32");
        let somme: f64 = v.iter().map(|x| *x as f64).sum();
        let norme: f64 = v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
        resultats.push(("split", somme, norme));
    }
    essai!("tile", tile, [1, 8, 16]);
    essai!("gather", gather, [1, 64, 128]);
    essai!("unsqueeze", unsqueeze, [1, 64, 128]);
    // À l'échelle de HTDemucs : le découpage en tuiles d'un noyau GPU change
    // avec la taille, et c'est là que vivent les erreurs de bord.
    essai!("instancenormalization_grand", instancenormalization_grand, [1, 48, 512, 336]);
    essai!("convtranspose_grand", convtranspose_grand, [1, 48, 256, 168]);
    essai!("layernormalization_grand", layernormalization_grand, [1, 336, 512]);

    println!("{:<24} {:>14} {:>14} {:>10}", "opérateur", "norme obtenue", "norme ORT", "écart");
    println!("{}", "─".repeat(66));

    let mut coupables = Vec::new();
    for (nom, _somme, norme) in &resultats {
        let Some((_, _, ref_norme)) = REFERENCE.iter().find(|(n, _, _)| n == nom) else {
            continue;
        };
        // Écart relatif : les normes vont de l'unité au millier selon
        // l'opérateur, un seuil absolu n'aurait pas de sens.
        let ecart = (norme - ref_norme).abs() / ref_norme.abs().max(1e-9);
        let verdict = if ecart > 1e-4 { "  ← FAUX" } else { "" };
        if ecart > 1e-4 {
            coupables.push(*nom);
        }
        println!("{nom:<24} {norme:>14.6} {ref_norme:>14.6} {ecart:>9.2e}{verdict}");
    }

    println!();
    if coupables.is_empty() {
        println!("Aucun écart : sur ces formes, ce backend calcule juste partout.");
    } else {
        println!("Opérateur(s) fautif(s) : {}", coupables.join(", "));
    }
}
