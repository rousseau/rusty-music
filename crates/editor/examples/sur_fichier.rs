// SPDX-License-Identifier: GPL-3.0-or-later
//! Démixe un fichier et rend les RMS par stem — pour comparer deux backends
//! sur la même entrée réelle.
//!
//! Le mélange synthétique tient en une seule fenêtre ; un vrai morceau en
//! demande plusieurs, avec recouvrement. C'est là que les chemins divergent.
//!
//!   cargo run --release -p rusty-music-editor --example sur_fichier -- <fichier> [variante] [secondes]
//!   … --no-default-features --features cpu --example sur_fichier -- …

use rusty_music_editor::{decode, sdr, Demixeur, Variante, SR};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let fichier = args
        .next()
        .ok_or("usage : … <fichier> [variante] [secondes]")?;
    let variante = args
        .next()
        .and_then(|v| Variante::analyser(&v))
        .unwrap_or_default();
    let secondes: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);

    // Le fichier est nommé, et ce n'est pas décoratif : trois « Karma
    // Police » cohabitent dans la bibliothèque de test, dont une reprise en
    // trio jazz. Comparer deux exécutions sans afficher le chemin, c'est se
    // préparer à conclure au bogue sur deux morceaux différents.
    println!("fichier : {fichier}");
    println!(
        "backend : {} · variante : {}",
        rusty_music_editor::moteur(),
        variante.nom()
    );
    let demixeur = Demixeur::charger(None, variante)?;

    let mut audio = decode::stereo(std::path::Path::new(&fichier))?;
    let n = (secondes * SR as usize).min(audio.gauche.len());
    audio.gauche.truncate(n);
    audio.droite.truncate(n);
    println!(
        "{:.1} s — {} fenêtre(s)",
        audio.duree(),
        n.div_ceil(343_980).max(1)
    );

    demixeur.chauffer();
    let t = std::time::Instant::now();
    let pistes = demixeur.separer(&audio)?;
    println!("séparation : {:.1} s\n", t.elapsed().as_secs_f64());

    let mut somme = vec![0.0f32; audio.gauche.len()];
    for p in &pistes {
        let total = (p.gauche.len() + p.droite.len()) as f64;
        let e: f64 = p
            .gauche
            .iter()
            .chain(&p.droite)
            .map(|x| (*x as f64).powi(2))
            .sum();
        println!("  {:<8} RMS {:.6}", p.nom, (e / total).sqrt());
        for (a, b) in somme.iter_mut().zip(&p.gauche) {
            *a += b;
        }
    }
    println!(
        "\nsomme des stems : SDR {:.1} dB",
        sdr(&audio.gauche, &somme)
    );
    Ok(())
}
