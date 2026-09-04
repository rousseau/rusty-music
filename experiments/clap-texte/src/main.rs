// SPDX-License-Identifier: GPL-3.0-or-later
//! La tour texte de CLAP, exécutée par Burn — et comparée à PyTorch.
//!
//! Pas de tokeniseur ici : `sonder.py reference` a écrit les identifiants de
//! jetons et les vecteurs attendus. C'est délibéré — on veut savoir si
//! `burn-onnx` importe ce graphe, question qui ne doit pas dépendre d'une
//! seconde inconnue.

use burn::tensor::{Tensor, TensorData};

#[allow(clippy::all, dead_code, unused_variables, non_snake_case)]
mod genere {
    include!(concat!(env!("OUT_DIR"), "/model/clap-text-encoder.rs"));
}

#[cfg(feature = "metal")]
type Moteur = burn::backend::Wgpu<f32, i32>;
#[cfg(not(feature = "metal"))]
type Moteur = burn::backend::NdArray<f32>;

#[derive(serde::Deserialize)]
struct Reference {
    longueur: usize,
    phrases: Vec<String>,
    input_ids: Vec<Vec<i64>>,
    attention_mask: Vec<Vec<i64>>,
    vecteurs: Vec<Vec<f32>>,
}

fn main() {
    let brut = std::fs::read_to_string("reference.json").expect("sonder.py reference d'abord");
    let r: Reference = serde_json::from_str(&brut).unwrap();

    let device = Default::default();
    let modele = genere::Model::<Moteur>::default();

    let mut pire_cos = 1.0f32;
    let mut pire_ecart = 0.0f32;
    let debut = std::time::Instant::now();
    for (i, phrase) in r.phrases.iter().enumerate() {
        let ids = Tensor::<Moteur, 1, burn::tensor::Int>::from_data(
            TensorData::new(r.input_ids[i].clone(), [r.longueur]),
            &device,
        )
        .reshape([1, r.longueur]);
        let masque = Tensor::<Moteur, 1, burn::tensor::Int>::from_data(
            TensorData::new(r.attention_mask[i].clone(), [r.longueur]),
            &device,
        )
        .reshape([1, r.longueur]);

        let sortie: Vec<f32> = modele.forward(ids, masque).into_data().to_vec().unwrap();
        let attendu = &r.vecteurs[i];
        let cos: f32 = sortie.iter().zip(attendu).map(|(a, b)| a * b).sum();
        let ecart = sortie
            .iter()
            .zip(attendu)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        println!("  cos {cos:.10}  écart {ecart:.2e}  « {phrase} »");
        pire_cos = pire_cos.min(cos);
        pire_ecart = pire_ecart.max(ecart);
    }
    let ms = debut.elapsed().as_secs_f64() * 1000.0 / r.phrases.len() as f64;
    println!("\npire cosinus {pire_cos:.10} · pire écart {pire_ecart:.2e}");
    println!("{ms:.0} ms par phrase, une par une");
}
