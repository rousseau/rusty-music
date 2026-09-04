// SPDX-License-Identifier: GPL-3.0-or-later
//! Ce que l'étirement coûte, et ce qu'il fait aux transitoires.
use rustfft::{num_complex::Complex, FftPlanner};

/// Netteté des attaques : rapport crête sur moyenne du flux spectral. Un
/// vocodeur qui étale les transitoires fait baisser ce rapport.
fn nettete(signal: &[f32]) -> f32 {
    const N: usize = 1024;
    let fft = FftPlanner::new().plan_fft_forward(N);
    let hann: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let mut precedent = vec![0.0f32; N / 2 + 1];
    let mut flux = Vec::new();
    for d in (0..signal.len().saturating_sub(N)).step_by(256) {
        let mut buf: Vec<Complex<f32>> = (0..N)
            .map(|i| Complex::new(signal[d + i] * hann[i], 0.0))
            .collect();
        fft.process(&mut buf);
        let mag: Vec<f32> = buf[..N / 2 + 1].iter().map(|c| c.norm()).collect();
        flux.push(
            mag.iter()
                .zip(&precedent)
                .map(|(a, b)| (a - b).max(0.0))
                .sum::<f32>(),
        );
        precedent = mag;
    }
    let moyenne = flux.iter().sum::<f32>() / flux.len().max(1) as f32;
    let crete = flux.iter().cloned().fold(0.0f32, f32::max);
    if moyenne > 0.0 {
        crete / moyenne
    } else {
        0.0
    }
}

fn main() {
    let chemin = std::env::args().nth(1).expect("chemin d'un stem");
    let s = rusty_music_editor::decode::stereo(std::path::Path::new(&chemin)).expect("décodage");
    let mono: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .map(|(a, b)| (a + b) / 2.0)
        .collect();
    let secondes = mono.len() as f32 / 44100.0;
    println!(
        "{} — {secondes:.0} s\n",
        chemin.rsplit('/').next().unwrap_or("")
    );
    println!(
        "{:>10} {:>10} {:>12} {:>10}",
        "facteur", "netteté", "durée", "temps"
    );
    println!("{}", "─".repeat(46));
    println!(
        "{:>10} {:>10.1} {:>11.0} s {:>9}",
        "original",
        nettete(&mono),
        secondes,
        "—"
    );
    for f in [0.8f32, 1.0, 1.25, 1.5] {
        let t = std::time::Instant::now();
        let out = rusty_music_editor::etirement::etirer(&mono, 1, f);
        let ms = t.elapsed().as_secs_f32();
        println!(
            "{:>10.2} {:>10.1} {:>11.0} s {:>8.2} s",
            f,
            nettete(&out),
            out.len() as f32 / 44100.0,
            ms
        );
    }
    let t = std::time::Instant::now();
    let out = rusty_music_editor::etirement::transposer(&mono, 1, 2.0);
    println!(
        "\ntransposition +2 demi-tons : netteté {:.1}, {:.0} s, {:.2} s de calcul",
        nettete(&out),
        out.len() as f32 / 44100.0,
        t.elapsed().as_secs_f32()
    );
}
