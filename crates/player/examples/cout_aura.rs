// SPDX-License-Identifier: GPL-3.0-or-later
//! Ce que coûte l'aura de la carte (`docs/plan-balade.md`, P4) au fil audio :
//! trois voix sommées, fondues et placées, puis converties à la fréquence de
//! la sortie comme le fait le mélangeur de `rodio`. Rend le temps de calcul
//! d'une minute de son, en part d'un cœur.
//!
//! ```text
//! cargo run --release -p rusty-music-player --example cout_aura [fréquence de sortie]
//! ```

use std::time::Instant;

use rodio::source::UniformSourceIterator;
use rodio::Source;
use rusty_music_player::aura::{Aura, VoixAura};
use rusty_music_player::SR_EXTRAIT;

fn main() {
    let sortie: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(44_100);
    // Trois extraits de 15 s, un bruit quelconque : le coût ne dépend pas de
    // ce qu'ils contiennent.
    let mut aura = Aura::new();
    let mut graine = 0x2545_F491u32;
    for id in 1..=3 {
        let mono: Vec<f32> = (0..15 * SR_EXTRAIT as usize)
            .map(|_| {
                graine ^= graine << 13;
                graine ^= graine >> 17;
                graine ^= graine << 5;
                (graine as f32 / u32::MAX as f32 - 0.5) * 0.5
            })
            .collect();
        aura.retenir(id, &mono);
    }
    aura.regler_niveau(0.5);
    let voix = |a: f32| {
        [
            VoixAura {
                id: 1,
                gain: 1.0,
                pan: -a,
            },
            VoixAura {
                id: 2,
                gain: 0.4,
                pan: a,
            },
            VoixAura {
                id: 3,
                gain: 0.2,
                pan: 0.0,
            },
        ]
    };

    let secondes = 60usize;
    for (nom, sr) in [
        ("source seule, 48 kHz", None),
        ("convertie vers la sortie", Some(sortie)),
    ] {
        let mut source: Box<dyn Source<Item = f32>> = match sr {
            None => Box::new(aura.source()),
            Some(sr) => Box::new(UniformSourceIterator::new(
                aura.source(),
                2.try_into().unwrap(),
                sr.try_into().unwrap(),
            )),
        };
        let taux = sr.unwrap_or(SR_EXTRAIT) as usize;
        let t = Instant::now();
        let mut somme = 0.0f32;
        // Une nouvelle demande toutes les secondes : les voix glissent sans
        // cesse, le pire cas.
        for s in 0..secondes {
            aura.jouer(&voix(if s % 2 == 0 { 0.8 } else { -0.8 }));
            for x in source.by_ref().take(taux * 2) {
                somme += x.abs();
            }
        }
        let dt = t.elapsed().as_secs_f64();
        println!(
            "{nom} : {:.1} ms pour {secondes} s de son, {:.3} % d'un cœur (contrôle {somme:.0})",
            dt * 1e3,
            100.0 * dt / secondes as f64
        );
    }
}
