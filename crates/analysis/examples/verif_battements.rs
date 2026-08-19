//! Ce que la grille de battements atteint, sur une vérité terrain fabriquée.
//!
//! ```bash
//! cargo run --release -p rusty-music-analysis --example verif_battements
//! ```
//!
//! **Pourquoi des clics et pas de la musique.** Il n'existe pas ici de jeu
//! annoté : personne n'a marqué à la main où tombent les temps de vingt-sept
//! mille morceaux. Des clics à une position connue sont la seule référence
//! fabricable, et ils suffisent à répondre à la question posée — le détecteur
//! place-t-il les battements là où ils sont ?
//!
//! **Ce que cet exemple a servi à trouver**, et qui n'était pas prévu : l'écart
//! initial n'était pas une constante. Il valait −31 ms à 150 BPM et −67 ms à
//! 120, et la colonne « dérive » a expliqué pourquoi — la grille de tempo de
//! `descripteurs.rs` a un pas de 0,5 %, ce qui suffit à décaler la phase de
//! quarante millisecondes au bout de seize secondes. D'où l'affinage conjoint
//! de la période et de la phase, sans lequel une grille ne tient pas la durée
//! d'un morceau.

use rusty_music_analysis::battements;
use rusty_music_analysis::descripteurs::Analyseur;
use rusty_music_analysis::mel::SR;

/// Clics à une position connue. Le retard est ce qui éprouve la phase : un
/// signal commençant sur un clic aurait toujours la phase zéro, et n'importe
/// quel détecteur cassé passerait.
fn clics(bpm: f32, secondes: f32, retard_s: f32) -> Vec<f32> {
    let n = (SR as f32 * secondes) as usize;
    let pas = (SR as f32 * 60.0 / bpm) as usize;
    let mut s = vec![0.0f32; n];
    for d in ((SR as f32 * retard_s) as usize..n).step_by(pas) {
        let duree = (SR as usize / 50).min(n - d);
        for i in 0..duree {
            let t = i as f32 / duree as f32;
            s[d + i] = (1.0 - t) * (((i * 7919) % 2001) as f32 / 1000.0 - 1.0);
        }
    }
    s
}

fn main() {
    let a = Analyseur::new();
    println!(
        "{:>5} {:>7} {:>9} {:>10} {:>10} {:>9}",
        "bpm", "retard", "bpm lu", "phase", "écart(ms)", "netteté"
    );

    let mut pire = 0.0f32;
    for bpm in [90.0f32, 120.0, 150.0, 174.0] {
        let periode = 60.0 / bpm;
        for retard in [0.0f32, 0.05, 0.12, 0.2, 0.31] {
            let g = battements::grille(&clics(bpm, 16.0, retard), &a).expect("clics réguliers");
            let attendue = retard % periode;
            // **Replier avant de comparer, sinon on mesure sa propre erreur.**
            // À 174 BPM le tempo sort à l'octave inférieure — 87 — et c'est
            // une grille juste : un battement sur deux. Une phase de 0,689 s y
            // est la phase 0 à un demi-millième près, mais une soustraction
            // brute la comptait à 344 ms.
            let d = (g.phase_s - attendue).rem_euclid(periode);
            let ecart = d.min(periode - d);
            pire = pire.max(ecart);
            println!(
                "{bpm:>5.0} {retard:>7.2} {:>9.2} {:>10.4} {:>10.1} {:>9.2}",
                g.bpm,
                g.phase_s,
                ecart * 1000.0,
                g.nettete
            );
        }
    }

    // Le balayage pose `PHASES` décalages par période : à 120 BPM, un pas vaut
    // 7,8 ms. C'est le plancher, et le chiffre ci-dessous s'y compare.
    println!("\npire écart de phase : {:.1} ms", pire * 1000.0);
    println!(
        "plancher de la méthode à 120 BPM : {:.1} ms (un pas de balayage)",
        500.0 / 64.0
    );
    println!(
        "\nÀ 174 BPM le tempo sort à 87 : l'ambiguïté d'octave de `descripteurs.rs`,\n\
         que la préférence log-normale autour de 120 tranche vers le bas. La grille\n\
         reste juste — un battement sur deux — mais c'est à savoir avant de caler\n\
         deux morceaux dont l'un serait lu à demi-tempo.\n\
         \n\
         Les netteté ci-dessus (18 à 35) sont celles de clics sur du silence, un\n\
         peigne parfait. Sur une vraie batterie, compter 2 à 3."
    );
}
