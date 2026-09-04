// SPDX-License-Identifier: GPL-3.0-or-later
//! Vérifie que l'enveloppe suit vraiment le signal, sur un fichier réel.
//! Ignoré par défaut : dépend d'un fichier de la bibliothèque locale, passé
//! par la variable d'environnement `ONDE_FICHIER`.

use std::path::Path;

#[test]
#[ignore]
fn enveloppe_dun_fichier_reel() {
    let p = std::env::var("ONDE_FICHIER").expect("ONDE_FICHIER");
    let path = Path::new(&p);
    let t = std::time::Instant::now();
    let w = rusty_music_player::waveform::compute(path, 160, None).expect("calcul");
    let ms = t.elapsed().as_millis();

    assert_eq!(w.peak.len(), 160);
    assert_eq!(w.rms.len(), 160);

    let crete_max = w.peak.iter().cloned().fold(0f32, f32::max);
    let crete_min = w.peak.iter().cloned().fold(1f32, f32::min);
    let rms_sous_crete = w.peak.iter().zip(&w.rms).all(|(p, r)| r <= p);

    println!("  calcul en {ms} ms");
    println!("  crête min {crete_min:.3} / max {crete_max:.3}");
    let pct = |v: &[f32]| {
        v.iter()
            .take(12)
            .map(|x| (x * 100.0) as i32)
            .collect::<Vec<_>>()
    };
    println!("  crêtes : {:?}", pct(&w.peak));
    println!("  RMS    : {:?}", pct(&w.rms));

    assert!(crete_max > 0.05, "signal muet ? crête max {crete_max}");
    assert!(
        crete_min < crete_max * 0.8,
        "enveloppe plate : elle ne suit pas le signal"
    );
    assert!(rms_sous_crete, "le RMS doit rester sous la crête");
}
