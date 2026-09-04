// SPDX-License-Identifier: GPL-3.0-or-later
//! Écrit la tuile de relief du zoom 0 en PNG, pour juger un réglage à l'œil.
//!
//! `cargo run --release -p rusty-music-carto --example apercu_relief -- <base> <sortie.png>`

use rusty_music_carto::relief;
use rusty_music_core::{db::Library, density};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| "rusty-music.db".into());
    let sortie = args.next().unwrap_or_else(|| "relief.png".into());
    let noyau: f64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.02);
    let exageration: f64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.35);

    let lib = Library::open(std::path::Path::new(&base))?;
    let mut parametres = lib.parametres_carte()?.parametres_densite();
    parametres.noyau = noyau;
    let points = lib.map_points(rusty_music_analysis_modele())?;
    let champ = density::champ_global(&points, &parametres);

    let o = relief::Ombrage {
        exageration,
        ..Default::default()
    };
    let png = relief::apercu(&champ, parametres.resolution, &o, 0, 0, 0)?;
    std::fs::write(&sortie, &png)?;
    println!(
        "{sortie} — {} morceaux, noyau {noyau}, couverture {:.1} %, exagération {}",
        points.len(),
        relief::couverture(&champ, parametres.resolution, &o) * 100.0,
        o.exageration
    );
    Ok(())
}

/// Le nom du modèle vit dans `analysis`, que ce crate ne veut pas tirer pour
/// une chaîne de caractères.
fn rusty_music_analysis_modele() -> &'static str {
    "clap-htsat-unfused-5f"
}
