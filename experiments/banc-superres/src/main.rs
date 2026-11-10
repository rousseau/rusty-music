// SPDX-License-Identifier: GPL-3.0-or-later
//! `banc-superres <modele.onnx> <entree> <sortie.wav> [...]` : AERO par le
//! chemin de l'application (`rusty_music_superres::regenerer`), une ligne
//! JSON par fichier : secondes de calcul, coupure estimée de la source.
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        args.len() >= 3 && args.len() % 2 == 1,
        "modele.onnx (entree sortie)+"
    );
    let t = Instant::now();
    let mut modele = rusty_music_superres::Modele::charger(Path::new(&args[0])).expect("modèle");
    eprintln!("chargé en {:.2} s", t.elapsed().as_secs_f64());
    for paire in args[1..].chunks(2) {
        let t = Instant::now();
        let coupure = rusty_music_superres::regenerer(
            Path::new(&paire[0]),
            Path::new(&paire[1]),
            &mut modele,
            |_, _| {},
        )
        .expect("régénérer");
        println!(
            "{{\"entree\": {:?}, \"secondes\": {:.3}, \"coupure_hz\": {:.0}}}",
            paire[0],
            t.elapsed().as_secs_f64(),
            coupure
        );
    }
}
