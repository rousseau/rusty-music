//! Fait tourner l'encodeur CLAP importé par `burn-onnx`, et vérifie qu'il dit
//! la même chose que la chaîne de production (ONNX Runtime).
//!
//! Deux questions, dans l'ordre :
//!
//! 1. **L'import passe-t-il ?** C'est `build.rs` qui répond ; si ce binaire
//!    compile, la réponse est oui.
//! 2. **Le résultat est-il le bon ?** Un modèle qui s'importe mais rend autre
//!    chose ne sert à rien. On lui donne une entrée déterministe et on compare
//!    l'empreinte à celle qu'`ort` produit sur la même entrée.

mod model {
    pub mod clap {
        include!(concat!(env!("OUT_DIR"), "/model/clap-audio-encoder-b5.rs"));
    }
}

use burn::tensor::Tensor;

/// Le backend, choisi à la compilation. C'est tout l'intérêt de Burn : le même
/// modèle généré tourne sur l'un ou l'autre sans que le code change.
#[cfg(feature = "cpu")]
type B = burn::backend::NdArray<f32>;
#[cfg(feature = "metal")]
type B = burn::backend::Wgpu<f32, i32>;

/// Mêmes constantes que `crates/analysis` : une fenêtre de 10 s à 48 kHz
/// donne 1001 trames de 64 bandes mel.
const TRAMES: usize = 1001;
const MELS: usize = 64;

/// Fenêtres par appel — le lot que la passe soumet réellement pour un morceau.
const LOT: usize = 5;

/// Entrée déterministe, indépendante de tout fichier audio : deux passes
/// doivent pouvoir se comparer sans dépendre de la carte SD.
///
/// Les `LOT` fenêtres sont décalées les unes des autres : un lot de cinq
/// copies du même signal ne dirait rien d'un lot réel.
fn entree_test() -> Vec<f32> {
    (0..LOT * TRAMES * MELS)
        .map(|i| {
            let fenetre = (i / (TRAMES * MELS)) as f32;
            let r = i % (TRAMES * MELS);
            let (t, m) = ((r / MELS) as f32, (r % MELS) as f32);
            // Quelque chose de structuré plutôt que du bruit : une empreinte de
            // bruit uniforme ne distingue rien, la leçon a déjà été payée.
            -40.0 + 20.0 * (((t + 37.0 * fenetre) / 97.0).sin() * (m / 11.0).cos())
        })
        .collect()
}

fn main() {
    let device = Default::default();
    println!(
        "backend : {}",
        if cfg!(feature = "metal") { "wgpu (Metal)" } else { "ndarray (CPU)" }
    );

    let debut = std::time::Instant::now();
    let modele: model::clap::Model<B> = model::clap::Model::default();
    println!("modèle chargé en {:.1} s", debut.elapsed().as_secs_f64());

    let brut = entree_test();
    let controle: f64 = brut.iter().map(|x| *x as f64).sum();
    println!("entrée : {} valeurs, somme {controle:.3}", brut.len());
    let entree = Tensor::<B, 1>::from_floats(brut.as_slice(), &device)
        .reshape([LOT, 1, TRAMES, MELS]);

    // Trois passes : la première d'un backend GPU compile ses noyaux, et la
    // chronométrer avec le calcul donnerait un chiffre qui ne se reproduit
    // jamais. La leçon a déjà été payée sur le jalon 1.
    let mut ms = 0;
    let mut v: Vec<f32> = Vec::new();
    for passe in 0..3 {
        let debut = std::time::Instant::now();
        let sortie = modele.forward(entree.clone());
        v = sortie.into_data().to_vec().expect("sortie f32");
        let t = debut.elapsed().as_millis();
        println!("  passe {} : {t} ms", passe + 1);
        ms = t;
    }
    let norme = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    println!(
        "{LOT} empreintes : {} valeurs, norme {norme:.4} — {ms} ms le lot, {:.1} ms/fenêtre",
        v.len(),
        ms as f64 / LOT as f64
    );
    if let Some(sortie) = std::env::args().nth(1) {
        let texte: Vec<String> = v.iter().map(|x| format!("{x:e}")).collect();
        std::fs::write(&sortie, texte.join("\n")).expect("écriture");
        println!("  écrite dans {sortie}");
    } else {
        println!("  premières valeurs : {:?}", &v[..v.len().min(6)]);
        println!("\nDonner un chemin en argument pour l'écrire et la comparer.");
    }
}
