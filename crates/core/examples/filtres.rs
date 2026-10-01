// SPDX-License-Identifier: GPL-3.0-or-later
//! Diagnostic des filtres de playlist sur une vraie base : vocabulaire, temps
//! de chargement, tailles de sélection pour quelques demandes types.
//! `cargo run -p rusty-music-core --example filtres -- <base.db>` — ouvrir une
//! **copie** de la base, pas celle de l'application (ouverture = migrations).
use rusty_music_core::filtres_playlist::*;

fn main() -> anyhow::Result<()> {
    let base = std::env::args().nth(1).unwrap_or_else(|| "rusty-music.db".into());
    let lib = rusty_music_core::db::Library::open(std::path::Path::new(&base))?;

    let t = std::time::Instant::now();
    let vocab = lib.vocabulaire_genres(usize::MAX)?;
    println!("{} genres en {:.2} s ; les 15 premiers : {:?}", vocab.len(), t.elapsed().as_secs_f64(), &vocab[..vocab.len().min(15)]);

    let t = std::time::Instant::now();
    let c = lib.caracteristiques_pistes()?;
    println!("{} morceaux en {:.2} s", c.len(), t.elapsed().as_secs_f64());
    let avec = |f: fn(&CaracteristiquesPiste) -> bool| c.iter().filter(|p| f(p)).count();
    println!(
        "couverture : année {} · durée {} · bpm {} · énergie {} · popularité {} · genre {}",
        avec(|p| p.annee.is_some()), avec(|p| p.duree_ms.is_some()), avec(|p| p.bpm.is_some()),
        avec(|p| p.energie.is_some()), avec(|p| p.popularite.is_some()), avec(|p| !p.genres.is_empty()),
    );
    println!("durée moyenne : {} s", duree_moyenne_ms(&c, None) / 1000);

    let essais: Vec<(&str, FiltresPlaylist)> = vec![
        ("calme", FiltresPlaylist { energie: Some(NiveauEnergie::Calme), ..Default::default() }),
        ("intense", FiltresPlaylist { energie: Some(NiveauEnergie::Intense), ..Default::default() }),
        ("années 70", FiltresPlaylist { annee_min: Some(1970), annee_max: Some(1979), ..Default::default() }),
        ("calme + années 70 + sans rock", FiltresPlaylist {
            energie: Some(NiveauEnergie::Calme), annee_min: Some(1970), annee_max: Some(1979),
            exclure_genres: vec!["rock".into()], ..Default::default() }),
        ("peu connu", FiltresPlaylist { popularite: Some(NiveauPopularite::PeuConnu), ..Default::default() }),
        ("jazz peu connu bpm ≤ 90", FiltresPlaylist {
            genres: vec!["jazz".into()], popularite: Some(NiveauPopularite::PeuConnu),
            bpm_max: Some(90.0), ..Default::default() }),
    ];
    for (nom, f) in essais {
        let t = std::time::Instant::now();
        let s = selectionner(&c, &f, 20).unwrap();
        println!("{nom:<32} → {:>6} admissibles en {:.0} ms ; relâchés : {:?}", s.ids.len(), t.elapsed().as_secs_f64() * 1000.0, s.relaches);
    }
    Ok(())
}
