//! Où passe le temps d'une transposition, phase par phase.
//!
//! ```bash
//! cargo run --release -p rusty-music-editor --example cout_transposition -- <stem.wav>
//! ```
//!
//! **La question.** La transposition d'un jeu de stems est signalée lente au
//! point de figer l'interface, alors qu'elle tourne dans son propre fil. Avant
//! de la paralléliser ou de la déplacer, il faut savoir ce qu'elle coûte
//! vraiment et où : décoder, étirer, rééchantillonner, écrire.

use std::time::Instant;

use rusty_music_editor::{decode, etirement, wav};

fn main() {
    let chemin = std::env::args().nth(1).expect("un chemin de stem");
    let demi_tons: f32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);

    let t = Instant::now();
    let s = decode::stereo(std::path::Path::new(&chemin)).expect("décodage");
    let decodage = t.elapsed();
    let duree = s.gauche.len() as f64 / 44_100.0;

    let t = Instant::now();
    let entrelace: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .flat_map(|(g, d)| [*g, *d])
        .collect();
    let entrelacement = t.elapsed();

    let rapport = 2f32.powf(demi_tons / 12.0);
    let t = Instant::now();
    let etire = etirement::etirer(&entrelace, 2, rapport);
    let etirement_ = t.elapsed();

    let t = Instant::now();
    let out = etirement::transposer(&entrelace, 2, demi_tons);
    let transposition = t.elapsed();

    let t = Instant::now();
    let (g, d): (Vec<f32>, Vec<f32>) = out.chunks_exact(2).map(|c| (c[0], c[1])).unzip();
    let cible = std::env::temp_dir().join("cout_transposition.wav");
    wav::ecrire(&cible, &g, &d, 44_100).expect("écriture");
    let ecriture = t.elapsed();
    let _ = std::fs::remove_file(&cible);

    println!("stem de {duree:.0} s, {demi_tons:+} demi-ton(s)\n");
    println!("  décodage         {:>7.2} s", decodage.as_secs_f64());
    println!("  entrelacement    {:>7.2} s", entrelacement.as_secs_f64());
    println!(
        "  étirement seul   {:>7.2} s   ({} → {} échantillons)",
        etirement_.as_secs_f64(),
        entrelace.len(),
        etire.len()
    );
    println!(
        "  transposition    {:>7.2} s   (= étirement + rééchantillonnage)",
        transposition.as_secs_f64()
    );
    println!("  écriture         {:>7.2} s", ecriture.as_secs_f64());

    let total = decodage + entrelacement + transposition + ecriture;
    println!("\n  un stem          {:>7.2} s", total.as_secs_f64());
    println!(
        "  quatre, en file  {:>7.2} s   ← ce que fait le code aujourd'hui",
        total.as_secs_f64() * 4.0
    );
    println!(
        "\nMémoire vive du seul étirement : {:.0} Mo d'entrée, {:.0} Mo de sortie.",
        entrelace.len() as f64 * 4.0 / 1e6,
        etire.len() as f64 * 4.0 / 1e6
    );

    // **Quatre en parallèle, et ce qu'il en coûte en mémoire.** Le gain est
    // évident ; le risque ne l'est pas. Chaque transposition tient plusieurs
    // copies du signal en vol, et quatre à la fois les multiplient d'autant.
    let t = Instant::now();
    std::thread::scope(|portee| {
        for _ in 0..4 {
            let e = &entrelace;
            portee.spawn(move || {
                let _ = etirement::transposer(e, 2, demi_tons);
            });
        }
    });
    let parallele = t.elapsed();
    println!(
        "\n  quatre en parallèle {:>6.2} s   (contre {:.2} s en file)",
        parallele.as_secs_f64(),
        transposition.as_secs_f64() * 4.0
    );
    // La pointe mémoire se lit de l'extérieur — `/usr/bin/time -l` sur macOS —
    // plutôt qu'en ajoutant `libc` aux dépendances pour un exemple.
}
