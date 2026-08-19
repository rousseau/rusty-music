//! Garde-fou : l'encodeur Burn rend-il toujours ce que rendait ONNX Runtime ?
//!
//! La migration a été validée par comparaison directe — cosinus 1,0000000000
//! sur les cinq fenêtres d'un lot, écart absolu maximal 1,4 × 10⁻⁶. Les
//! valeurs attendues ci-dessous **proviennent de cette exécution d'ONNX
//! Runtime** et sont figées ici : elles survivent au retrait de la dépendance.
//! Toute dérive ultérieure — changement de backend, de version de Burn, de
//! préparation du modèle — se verra donc tout de suite.
//!
//! L'entrée est calculée, pas lue : le contrôle ne doit dépendre ni du disque
//! ni de la carte SD.
//!
//!   cargo run --release -p rusty-music-analysis --example empreinte_reference

use rusty_music_analysis::{encodeur, Embedder, DIMS, LOT, MELS, TRAMES};

/// Six premières valeurs de la première empreinte, sous ONNX Runtime.
const ATTENDU: [f32; 6] = [
    -7.843_837e-3,
    2.789_993_6e-2,
    4.900_250_4e-1,
    4.332_510_2e-1,
    -1.382_394_7e-1,
    6.868_381e-2,
];
/// Norme euclidienne du lot entier, même source.
const NORME_ATTENDUE: f64 = 10.220_166;
/// Les deux moteurs divergeaient de 1,4 × 10⁻⁶ : pur arrondi `f32`. On se
/// donne un ordre de grandeur de marge, pas davantage — le but est d'attraper
/// une vraie dérive, pas de tolérer un modèle différent.
const TOLERANCE: f32 = 1e-5;

/// Doit rester rigoureusement identique à `entree_test` de
/// `experiments/burn-clap`.
fn entree_test() -> Vec<f32> {
    (0..LOT * TRAMES * MELS)
        .map(|i| {
            let fenetre = (i / (TRAMES * MELS)) as f32;
            let r = i % (TRAMES * MELS);
            let (t, m) = ((r / MELS) as f32, (r % MELS) as f32);
            -40.0 + 20.0 * (((t + 37.0 * fenetre) / 97.0).sin() * (m / 11.0).cos())
        })
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Sans argument : les poids du build en cours, les seuls qui aillent avec
    // son code généré.
    let poids = std::env::args().nth(1);
    let poids = poids.as_deref().map(std::path::Path::new);
    println!(
        "poids   : {}",
        poids.map_or(env!("RM_POIDS"), |p| p.to_str().unwrap_or("?"))
    );
    println!("backend : {}", encodeur::moteur());

    let debut = std::time::Instant::now();
    let mut enc = Embedder::charger(poids, 1)?;
    println!("chargé en {:.1} s", debut.elapsed().as_secs_f64());

    let entree = entree_test();
    let controle: f64 = entree.iter().map(|x| *x as f64).sum();
    println!("entrée : {} valeurs, somme {controle:.3}", entree.len());
    // Trois passes : la première d'un backend GPU compile ses noyaux, et la
    // chronométrer avec le calcul donnerait un chiffre irreproductible.
    let mut lot = Vec::new();
    let mut ms = 0;
    for passe in 0..3 {
        let t = std::time::Instant::now();
        lot = enc.empreintes(&entree, LOT)?;
        ms = t.elapsed().as_millis();
        println!("  passe {} : {ms} ms", passe + 1);
    }

    let plat: Vec<f32> = lot.concat();
    let norme = (plat.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>()).sqrt();
    println!(
        "\n{LOT} empreintes de {DIMS} dimensions — {ms} ms le lot, {:.1} ms/fenêtre",
        ms as f64 / LOT as f64
    );

    let ecart = ATTENDU
        .iter()
        .zip(&plat)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    println!("écart maximal sur les six premières valeurs : {ecart:.2e}");
    println!("norme : {norme:.6} (attendue {NORME_ATTENDUE:.6})");

    if ecart > TOLERANCE || (norme - NORME_ATTENDUE).abs() > 1e-3 {
        eprintln!("\n✗ DÉRIVE : l'encodeur ne rend plus ce que rendait ONNX Runtime.");
        eprintln!("  obtenu : {:?}", &plat[..6]);
        eprintln!("  attendu: {ATTENDU:?}");
        std::process::exit(1);
    }
    println!("\n✓ conforme à la référence ONNX Runtime");
    Ok(())
}
