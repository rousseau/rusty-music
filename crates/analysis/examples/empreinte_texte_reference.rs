// SPDX-License-Identifier: GPL-3.0-or-later
//! Garde-fou : l'encodeur texte Burn rend-il ce que rend PyTorch ?
//!
//! Même principe que `empreinte_reference` côté audio, mais la tokenisation
//! change de langage au passage (Python → `tokenizers`, la première fois
//! qu'un tokeniseur tourne côté Rust dans ce dépôt) : ce garde-fou couvre
//! aussi cette traduction, pas seulement le graphe.
//!
//! Les valeurs attendues **proviennent d'une exécution de
//! `experiments/clap-texte/sonder.py::tour_texte`** (le même chemin que
//! `ClapModel.get_text_features`), figées ici pour survivre au retrait de la
//! dépendance PyTorch.
//!
//!   cargo run --release -p rusty-music-analysis --example empreinte_texte_reference

use rusty_music_analysis::EmbedderTexte;

const PHRASE: &str = "a song with a strong focus on drums";

/// Six premières valeurs de l'empreinte, sous PyTorch (`tour_texte`,
/// padding dynamique — sans effet sur le résultat, le masque d'attention
/// exclut déjà le remplissage, qu'il s'arrête à la longueur naturelle de la
/// phrase ou à `LONGUEUR` fixe).
const ATTENDU: [f32; 6] = [
    -5.762_489e-3,
    1.464_647_7e-2,
    4.907_789e-2,
    -7.783_627e-3,
    3.202_761_3e-2,
    -3.745_482e-2,
];
const NORME_ATTENDUE: f32 = 1.0;
/// Marge large : cosinus/écart de la migration audio (1,4 × 10⁻⁶) plus l'écart
/// introduit par un tokeniseur indépendant de celui de `transformers` — le but
/// est d'attraper une vraie dérive, pas de coller à l'arrondi `f32` près.
const TOLERANCE: f32 = 1e-4;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debut = std::time::Instant::now();
    let enc = EmbedderTexte::charger(None)?;
    println!("chargé en {:.1} s", debut.elapsed().as_secs_f64());

    let debut = std::time::Instant::now();
    let v = enc.embed(PHRASE)?;
    println!("encodé en {:.0} ms · « {PHRASE} »", debut.elapsed().as_secs_f64() * 1000.0);

    let norme: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    println!("norme : {norme:.7} (attendue {NORME_ATTENDUE})");
    assert!(
        (norme - NORME_ATTENDUE).abs() < TOLERANCE,
        "norme inattendue : {norme} (attendue {NORME_ATTENDUE})"
    );

    let ecart = v[..6]
        .iter()
        .zip(&ATTENDU)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    println!("six premières valeurs : {:?}", &v[..6]);
    println!("écart absolu max sur ces six valeurs : {ecart:.2e}");
    assert!(
        ecart < TOLERANCE,
        "dérive détectée : écart {ecart:.2e} > tolérance {TOLERANCE:.2e}"
    );

    println!("\n✓ l'encodeur texte Burn s'accorde avec PyTorch");
    Ok(())
}
