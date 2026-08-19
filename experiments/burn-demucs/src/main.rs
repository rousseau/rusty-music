//! Fait tourner HTDemucs importé par `burn-onnx` et vérifie ce qu'il rend.
//!
//! Deux questions, dans l'ordre :
//!
//! 1. **L'import passe-t-il ?** C'est `build.rs` qui répond ; si ce binaire
//!    compile, la réponse est oui.
//! 2. **Le résultat est-il le bon ?** L'entrée est un signal calculé, pas un
//!    fichier : la comparaison ne doit dépendre ni du disque ni de la carte.
//!    La référence se produit avec `reference_demucs.py`, qui fait passer
//!    exactement le même signal dans ONNX Runtime.

mod model {
    #[allow(clippy::all, dead_code, unused_variables, non_snake_case)]
    pub mod htdemucs {
        include!(concat!(env!("OUT_DIR"), "/model/htdemucs-fige.rs"));
    }
}

use burn::tensor::{Tensor, TensorData};

/// Entrée figée dans le graphe : 7,8 s de stéréo à 44,1 kHz.
const CANAUX: usize = 2;
const ECHANTILLONS: usize = 343_980;

#[cfg(any(feature = "metal", feature = "metal-nu"))]
type Moteur = burn::backend::Wgpu<f32, i32>;
#[cfg(not(any(feature = "metal", feature = "metal-nu")))]
type Moteur = burn::backend::NdArray<f32>;

/// Un mélange synthétique à quatre voix, chacune dans son registre : de quoi
/// donner au démixage quelque chose à séparer. Du bruit uniforme ne dirait
/// rien — la leçon a déjà été payée sur CLAP.
fn melange() -> Vec<f32> {
    let sr = 44_100.0f32;
    (0..CANAUX * ECHANTILLONS)
        .map(|i| {
            let c = (i / ECHANTILLONS) as f32;
            let t = (i % ECHANTILLONS) as f32 / sr;
            let basse = (t * 55.0 * std::f32::consts::TAU).sin() * 0.35;
            let voix = (t * 220.0 * std::f32::consts::TAU).sin() * 0.25;
            let autre = (t * 880.0 * std::f32::consts::TAU).sin() * 0.15;
            // Percussion : un clic toutes les demi-secondes.
            let phase = (t * 2.0).fract();
            let batterie = if phase < 0.01 { 0.4 * (1.0 - phase * 100.0) } else { 0.0 };
            (basse + voix + autre + batterie) * (1.0 + 0.1 * c)
        })
        .collect()
}

fn main() {
    let device = burn::tensor::Device::<Moteur>::default();
    println!(
        "backend : {}",
        if cfg!(feature = "metal") { "wgpu (Metal, fusion+autotune)" }
        else if cfg!(feature = "metal-nu") { "wgpu (Metal, sans fusion ni autotune)" }
        else { "ndarray (CPU)" }
    );

    let debut = std::time::Instant::now();
    let poids = std::env::var("RM_POIDS_DEMUCS")
        .unwrap_or_else(|_| concat!(env!("OUT_DIR"), "/model/htdemucs-fige.bpk").to_string());
    let modele = model::htdemucs::Model::<Moteur>::from_file(&poids, &device);
    println!("modèle chargé en {:.1} s", debut.elapsed().as_secs_f64());

    let brut = melange();
    let controle: f64 = brut.iter().map(|x| *x as f64).sum();
    println!("entrée : {} échantillons, somme {controle:.3}", brut.len());

    let entree = Tensor::<Moteur, 1>::from_data(
        TensorData::new(brut, [CANAUX * ECHANTILLONS]),
        &device,
    )
    .reshape([1, CANAUX, ECHANTILLONS]);

    // Deux passes : la première d'un backend GPU compile ses noyaux.
    let mut stems: Vec<f32> = Vec::new();
    for passe in 0..2 {
        let t = std::time::Instant::now();
        let sortie = modele.forward(entree.clone());
        let forme = sortie.dims();
        stems = sortie.into_data().to_vec().expect("sortie f32");
        println!(
            "  passe {} : {} ms — forme {forme:?}",
            passe + 1,
            t.elapsed().as_millis()
        );
    }

    // Quatre stems dans l'ordre de Demucs : batterie, basse, autre, voix.
    let noms = ["batterie", "basse", "autre", "voix"];
    let par_stem = stems.len() / noms.len();
    println!("\n{} valeurs, {par_stem} par stem :", stems.len());
    for (i, nom) in noms.iter().enumerate() {
        let t = &stems[i * par_stem..(i + 1) * par_stem];
        let energie = (t.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / t.len() as f64).sqrt();
        println!("  {nom:<10} RMS {energie:.6}");
    }

    if let Some(sortie) = std::env::args().nth(1) {
        let texte: Vec<String> = stems.iter().map(|x| format!("{x:e}")).collect();
        std::fs::write(&sortie, texte.join("\n")).expect("écriture");
        println!("\nstems écrits dans {sortie}");
    }
}
