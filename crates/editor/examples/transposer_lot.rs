// SPDX-License-Identifier: GPL-3.0-or-later
//! V28 : transposer un jeu de stems en file ou en parallèle, et comparer.
//!
//! ```bash
//! cargo run --release -p rusty-music-editor --example transposer_lot -- <sortie> <fils> <demi-tons> <stem.wav>…
//! ```
//!
//! Le même chemin que l'établi (`transposer_un` du bureau) : décoder,
//! `etirement::transposer`, écrire le WAV. `<fils>` = 1 : l'un après
//! l'autre ; plus : autant de fils qui prennent le stem suivant dans une
//! file commune, comme `start_etirer`. Les fichiers vont dans `<sortie>`,
//! pour être comparés octet pour octet d'un mode à l'autre. Aucun son.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rusty_music_editor::{decode, etirement, wav};

fn un(source: &Path, sortie: &Path, demi_tons: f32) -> f64 {
    let t = Instant::now();
    let s = decode::stereo(source).expect("décodage");
    let entrelace: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .flat_map(|(g, d)| [*g, *d])
        .collect();
    let out = etirement::transposer(&entrelace, 2, demi_tons);
    let (g, d): (Vec<f32>, Vec<f32>) = out.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).unzip();
    let cible = sortie.join(source.file_name().expect("nom de fichier"));
    wav::ecrire(&cible, &g, &d, 44_100).expect("écriture");
    t.elapsed().as_secs_f64()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let sortie = PathBuf::from(args.next().expect("dossier de sortie"));
    let fils: usize = args.next().and_then(|v| v.parse().ok()).expect("fils");
    let demi_tons: f32 = args.next().and_then(|v| v.parse().ok()).expect("demi-tons");
    let stems: Vec<PathBuf> = args.map(PathBuf::from).collect();
    std::fs::create_dir_all(&sortie).expect("dossier de sortie");

    let t = Instant::now();
    let suivant = AtomicUsize::new(0);
    let durees = std::sync::Mutex::new(vec![0.0; stems.len()]);
    std::thread::scope(|portee| {
        for _ in 0..fils.max(1) {
            portee.spawn(|| loop {
                let i = suivant.fetch_add(1, Ordering::Relaxed);
                let Some(s) = stems.get(i) else { break };
                let d = un(s, &sortie, demi_tons);
                durees.lock().expect("verrou")[i] = d;
            });
        }
    });
    let total = t.elapsed().as_secs_f64();
    let durees = durees.into_inner().expect("verrou");
    println!(
        "{} stems, {} fil(s), {demi_tons:+} : {total:.1} s en tout ; par stem {}",
        stems.len(),
        fils,
        durees
            .iter()
            .map(|d| format!("{d:.1}"))
            .collect::<Vec<_>>()
            .join(" / ")
    );
}
