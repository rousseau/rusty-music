// SPDX-License-Identifier: GPL-3.0-or-later
//! Combien de silence numérique reste en tête et en queue d'une piste décodée ?
//!
//! ```bash
//! cargo run --release -p rusty-music-player --example silences_bords -- <fichier de chemins>
//! ```
//!
//! Lit un chemin par ligne, ouvre chaque piste par
//! [`rusty_music_player::ouvrir`] (le chemin exact de la lecture) et imprime
//! `chemin<TAB>trames<TAB>taux<TAB>tete<TAB>queue` — `tete` et `queue` étant
//! le nombre de trames dont la valeur absolue reste sous −60 dB aux deux
//! bouts. Sert à chiffrer, sur une vraie bibliothèque, le blanc que le
//! décodage laisse entre deux pistes (point 1 de `docs/plan-ecouter-v0.2.md`).

use std::io::BufRead;

use rodio::Source;

const SEUIL: f32 = 0.001;

fn main() {
    let liste = std::env::args().nth(1).expect("un fichier de chemins");
    let fichier = std::fs::File::open(liste).expect("liste lisible");
    for ligne in std::io::BufReader::new(fichier)
        .lines()
        .map_while(Result::ok)
    {
        let chemin = std::path::Path::new(ligne.trim());
        let Ok(source) = rusty_music_player::ouvrir(chemin, 1.0) else {
            continue;
        };
        let taux = source.sample_rate().get();
        let canaux = source.channels().get() as usize;
        let tout: Vec<f32> = source.collect();
        let trames = tout.len() / canaux;
        let muet = |t: usize| (0..canaux).all(|c| tout[t * canaux + c].abs() < SEUIL);
        let tete = (0..trames).take_while(|&t| muet(t)).count();
        let queue = (0..trames).rev().take_while(|&t| muet(t)).count();
        println!("{}\t{trames}\t{taux}\t{tete}\t{queue}", chemin.display());
    }
}
