// SPDX-License-Identifier: GPL-3.0-or-later
//! Décompose la latence de la sortie par défaut, avant et après l'ouverture
//! d'un flux `rodio` à plusieurs tailles de tampon.
//!
//! `cargo run --release -p rusty-music-player --example latence_sortie`

use std::time::Duration;

use rodio::DeviceSinkBuilder;
use rusty_music_player::detail_latence_sortie;

fn afficher(quand: &str) {
    let Some(d) = detail_latence_sortie() else {
        println!("{quand:<24} pas de CoreAudio");
        return;
    };
    let ms = |t| d.duree(t).as_secs_f64() * 1e3;
    println!(
        "{quand:<24} {:>6.0} Hz  appareil {:>4} ({:>5.1} ms)  marge {:>4} ({:>5.1} ms)  \
         tampon {:>5} ({:>5.1} ms)  flux {:>4} ({:>5.1} ms)  = {:>5.1} ms",
        d.taux,
        d.appareil,
        ms(d.appareil),
        d.marge,
        ms(d.marge),
        d.tampon,
        ms(d.tampon),
        d.flux,
        ms(d.flux),
        d.totale().as_secs_f64() * 1e3
    );
}

fn main() {
    afficher("sans flux");
    for tampon in [
        None,
        Some(2048),
        Some(1024),
        Some(512),
        Some(256),
        Some(128),
    ] {
        let mut b = DeviceSinkBuilder::from_default_device().expect("sortie");
        if let Some(t) = tampon {
            b = b.with_buffer_size(cpal::BufferSize::Fixed(t));
        }
        let mut sortie = b.open_stream().expect("flux");
        sortie.log_on_drop(false);
        std::thread::sleep(Duration::from_millis(300));
        let nom = match tampon {
            None => "rodio par défaut".to_string(),
            Some(t) => format!("Fixed({t})"),
        };
        afficher(&nom);
        drop(sortie);
        std::thread::sleep(Duration::from_millis(300));
        afficher("  après fermeture");
    }
}
