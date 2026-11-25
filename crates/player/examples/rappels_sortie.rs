// SPDX-License-Identifier: GPL-3.0-or-later
//! Régularité du rappel audio de la lecture normale selon la taille du
//! tampon : la chaîne de `Player` (morceau ouvert par `ouvrir`, « E » au
//! choix, aura) plus une sonde dans le mélangeur, qui date chaque salve
//! d'échantillons. Une salve qui part plus d'une période et demie après la
//! précédente, ou qui dure plus qu'une période, est une coupure probable.
//!
//! ```text
//! cargo run --release -p rusty-music-player --example rappels_sortie -- <morceau> [tampons…]
//! ```
//! Chaque tampon : 15 s au repos, puis 15 s avec tous les cœurs occupés à
//! décoder (le préchargement du morceau suivant, en pire).

use std::num::NonZero;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{DeviceSinkBuilder, Source};
use rusty_music_player::{amelioration, aura::Aura, ouvrir};

/// Silence qui note l'instant de chaque appel à `next`.
struct Sonde {
    instants: Arc<Mutex<Vec<Instant>>>,
    local: Vec<Instant>,
    taux: u32,
}

impl Iterator for Sonde {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.local.push(Instant::now());
        if self.local.len() >= 4096 {
            if let Ok(mut v) = self.instants.try_lock() {
                v.append(&mut self.local);
            }
        }
        Some(0.0)
    }
}

impl Source for Sonde {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> NonZero<u16> {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> NonZero<u32> {
        NonZero::new(self.taux).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Salves : groupes d'appels séparés de plus de 0,5 ms.
fn analyser(instants: &[Instant], periode: Duration) -> String {
    let mut debuts = Vec::new();
    let mut durees = Vec::new();
    let mut debut = instants[0];
    let mut prec = instants[0];
    for &t in &instants[1..] {
        if t - prec > Duration::from_micros(500) {
            debuts.push(debut);
            durees.push(prec - debut);
            debut = t;
        }
        prec = t;
    }
    let mut ecarts: Vec<f64> = debuts
        .windows(2)
        .map(|w| (w[1] - w[0]).as_secs_f64() * 1e3)
        .collect();
    let p = periode.as_secs_f64() * 1e3;
    let en_retard = ecarts.iter().filter(|&&e| e > 1.5 * p).count();
    let trop_longues = durees.iter().filter(|d| **d > periode).count();
    ecarts.sort_by(f64::total_cmp);
    let mut d: Vec<f64> = durees.iter().map(|d| d.as_secs_f64() * 1e3).collect();
    d.sort_by(f64::total_cmp);
    format!(
        "{} salves, écart médian {:.2} ms (attendu {:.2}), max {:.2} ; durée médiane {:.3} ms, max {:.3} ; en retard {}, trop longues {}",
        debuts.len(),
        ecarts[ecarts.len() / 2],
        p,
        ecarts.last().unwrap(),
        d[d.len() / 2],
        d.last().unwrap(),
        en_retard,
        trop_longues
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    let morceau = PathBuf::from(args.next().expect("chemin d'un morceau"));
    let tampons: Vec<u32> = args.map(|a| a.parse().expect("tampon")).collect();
    let tampons = if tampons.is_empty() {
        vec![2048, 512]
    } else {
        tampons
    };
    let e = std::env::var_os("E").is_some();
    amelioration::amelioration().set_actif(e);
    for tampon in tampons {
        let mut sortie = DeviceSinkBuilder::from_default_device()
            .expect("sortie")
            .with_buffer_size(cpal::BufferSize::Fixed(tampon))
            .open_stream()
            .expect("flux");
        sortie.log_on_drop(false);
        let taux = sortie.config().sample_rate().get();
        amelioration::enregistrer_taux_sortie(taux);
        let lecteur = rodio::Player::connect_new(sortie.mixer());
        lecteur.set_volume(0.05);
        lecteur.append(ouvrir(&morceau, 1.0).expect("morceau"));
        let aura = Aura::new();
        sortie.mixer().add(aura.source());
        let instants = Arc::new(Mutex::new(Vec::new()));
        sortie.mixer().add(Sonde {
            instants: Arc::clone(&instants),
            local: Vec::new(),
            taux,
        });
        let periode = Duration::from_secs_f64(tampon as f64 / taux as f64);
        for charge in [false, true] {
            std::thread::sleep(Duration::from_millis(500));
            instants.lock().unwrap().clear();
            let fin = Arc::new(AtomicBool::new(false));
            let fils: Vec<_> = (0..if charge {
                std::thread::available_parallelism().map_or(8, |n| n.get())
            } else {
                0
            })
                .map(|_| {
                    let fin = Arc::clone(&fin);
                    let m = morceau.clone();
                    std::thread::spawn(move || {
                        while !fin.load(Ordering::Relaxed) {
                            let _ = ouvrir(&m, 1.0);
                        }
                    })
                })
                .collect();
            std::thread::sleep(Duration::from_secs(15));
            fin.store(true, Ordering::Relaxed);
            for f in fils {
                let _ = f.join();
            }
            let v = instants.lock().unwrap().clone();
            println!(
                "tampon {tampon:>5} {} E={} : {}",
                if charge { "chargé" } else { "repos " },
                e as u8,
                analyser(&v, periode)
            );
        }
        drop(lecteur);
        drop(sortie);
    }
}
