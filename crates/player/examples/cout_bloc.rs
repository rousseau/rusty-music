//! La pire salve du rappel audio, selon la taille de bloc.
//!
//! ```bash
//! cargo run --release -p rusty-music-player --example cout_bloc
//! ```
//!
//! **La question, et pourquoi elle décide.** `Melange::next()` est appelé par la
//! sortie audio, un échantillon à la fois, et doit rendre la main avant que le
//! périphérique ait épuisé son tampon. `Voix::remplir()` y pousse un bloc entier
//! dans l'étireur : **un appel sur quelques milliers fait tout le travail, les
//! autres ne font rien.** C'est ce pic-là qui craque, pas la moyenne.
//!
//! La moyenne, elle, va bien : WSOLA tient 3,2 fois le temps réel sur quatre
//! stems. Mesurer un débit moyen aurait donc conclu que tout allait bien.
//!
//! **L'unité de salve n'est pas le bloc, c'est le pas de l'étireur.** `wsola`
//! rend sa sortie par sauts de `hop_ms` (15 ms par défaut, soit 661 trames à
//! 44,1 kHz) : un bloc plus grand qu'un pas en calcule plusieurs d'un coup, un
//! bloc plus petit n'en calcule jamais moins d'un.

use std::time::Instant;

const SR: u32 = 44_100;
const CANAUX: u16 = 2;
const STEMS: usize = 4;
/// Tampon CoreAudio courant sur ce Mac : c'est l'échéance à tenir.
const TAMPON_TRAMES: f64 = 512.0;
/// Les bornes de l'interface, et le milieu. **Le pire cas est le plus lent** :
/// à tempo 0,25 un bloc d'entrée rend quatre fois plus de sortie, donc quatre
/// fois plus de pas d'étireur à calculer d'un coup.
const TEMPOS: [f32; 4] = [0.25, 0.5, 1.5, 4.0];

fn main() {
    let mut graine = 0x2545_F491_4F6C_DD1Du64;
    let mut alea = || {
        graine ^= graine >> 12;
        graine ^= graine << 25;
        graine ^= graine >> 27;
        (graine.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 8_388_608.0 - 1.0
    };
    let matiere: Vec<f32> = (0..SR as usize * 30 * CANAUX as usize)
        .map(|_| alea())
        .collect();

    let echeance = TAMPON_TRAMES / SR as f64 * 1000.0;
    println!(
        "quatre stems · échéance du tampon {echeance:.1} ms\n\n\
         {:>7} {:>7} {:>13} {:>13} {:>9}",
        "bloc", "tempo", "pire salve", "audio rendu", "verdict"
    );

    for (bloc, tempo) in [128usize, 256, 512, 1024, 2048, 4096]
        .into_iter()
        .flat_map(|b| TEMPOS.into_iter().map(move |t| (b, t)))
    {
        let mut e = wsola::TimeStretch::new(SR, CANAUX).expect("étireur");
        e.set_tempo(tempo);
        let mut i = 0;
        let n = bloc * CANAUX as usize;

        // Chauffe : la première poussée remplit les tampons internes, son coût
        // n'est pas représentatif du régime établi.
        for _ in 0..16 {
            e.push(&matiere[i..i + n]);
            let _ = e.pull(usize::MAX);
            i += n;
        }

        // **Une salve, c'est ce que fait un seul `next()`** : pousser des blocs
        // jusqu'à ce que l'étireur rende quelque chose. Un bloc trop court n'en
        // rend pas à chaque fois, et la boucle de `echantillon` recommence.
        let (mut pire, mut rendu_pire) = (0.0f64, 0usize);
        for _ in 0..400 {
            let t = Instant::now();
            let mut rendu = 0usize;
            while rendu == 0 {
                if i + n >= matiere.len() {
                    i = 0;
                }
                e.push(&matiere[i..i + n]);
                rendu = e.pull(usize::MAX).len();
                i += n;
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            if ms > pire {
                pire = ms;
                rendu_pire = rendu;
            }
        }

        let salve = pire * STEMS as f64;
        let audio = rendu_pire as f64 / CANAUX as f64 / SR as f64 * 1000.0;
        println!(
            "{bloc:>7} {tempo:>7.2} {salve:>10.2} ms {audio:>10.1} ms {:>9}",
            if salve < echeance { "tient" } else { "CRAQUE" }
        );
    }

    println!(
        "\nL'étireur rend sa sortie par pas de 661 trames (15 ms) : une salve ne\n\
         descend jamais sous un pas, et un bloc d'entrée qui en couvre plusieurs\n\
         les calcule tous d'un coup. Le pire cas est donc le tempo le plus lent,\n\
         où un bloc rend quatre fois sa durée."
    );
}
