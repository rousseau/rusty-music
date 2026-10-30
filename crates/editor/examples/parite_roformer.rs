// SPDX-License-Identifier: GPL-3.0-or-later
//! Parité du port de BS-RoFormer avec le code d'origine : lit les références
//! de `experiments/roformer/reference.py` et compare, étage par étage,
//! spectre, bandes projetées, chaque bloc, masques, segment et morceau
//! découpé (cosinus et écart relatif). Le premier étage qui décroche dit où
//! chercher.
//!
//!   cargo run --release -p rusty-music-editor --example parite_roformer -- <dossier_ref> <BS-Rofo-SW-Fixed.ckpt>
//!   (processeur : --no-default-features --features cpu)

use std::path::{Path, PathBuf};
use std::time::Instant;

use burn::tensor::{Tensor, TensorData};
use rusty_music_editor::roformer::{Config, Roformer};
use rusty_music_editor::{moteur, Moteur};

fn lire(dossier: &Path, nom: &str) -> Vec<f32> {
    let octets = std::fs::read(dossier.join(nom)).unwrap_or_else(|e| panic!("{nom} : {e}"));
    octets.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn vers_vec<const D: usize>(t: Tensor<Moteur, D>) -> Vec<f32> {
    t.into_data().convert::<f32>().into_vec().unwrap()
}

/// Cosinus et écart relatif (norme de la différence / norme de la référence).
fn comparer(nom: &str, a: &[f32], r: &[f32]) -> f64 {
    assert_eq!(a.len(), r.len(), "{nom} : tailles {} et {}", a.len(), r.len());
    let (mut ar, mut aa, mut rr, mut dd) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.iter().zip(r) {
        let (x, y) = (x as f64, y as f64);
        ar += x * y;
        aa += x * x;
        rr += y * y;
        dd += (x - y) * (x - y);
    }
    let cos = ar / (aa.sqrt() * rr.sqrt()).max(1e-30);
    println!("{nom:12} cosinus {cos:.7}  écart relatif {:.2e}", (dd / rr.max(1e-30)).sqrt());
    cos
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dossier = PathBuf::from(&a[0]);
    let poids = PathBuf::from(&a[1]);
    let config = Config::sw();
    let device = Default::default();
    let t0 = Instant::now();
    let m = Roformer::<Moteur>::charger(&poids, config.clone(), &device).expect("chargement");
    println!("{} : chargé en {:.1} s", moteur(), t0.elapsed().as_secs_f32());

    let audio = lire(&dossier, "audio.f32");
    let n = audio.len() / 2;
    let (g, d) = audio.split_at(n);
    let seg = config.segment.min(n);

    let (spectre, trames) = m.spectre(&g[..seg], &d[..seg]);
    comparer("spectre", &spectre, &lire(&dossier, "spectre.f32"));

    // Étages du réseau, chacun repris de la référence précédente : une
    // erreur ne se propage pas, on voit l'étage fautif.
    let r = m.reseau();
    let largeur = spectre.len() / trames;
    let x = Tensor::<Moteur, 2>::from_data(TensorData::new(spectre.clone(), [trames, largeur]), &device);
    let t = Instant::now();
    comparer("découpe", &vers_vec(r.decouper(x, &config.bandes)), &lire(&dossier, "decoupe.f32"));
    let forme = [trames, config.bandes.len(), config.dim];
    let mut entree = lire(&dossier, "decoupe.f32");
    for i in 0..config.profondeur {
        let x = Tensor::<Moteur, 3>::from_data(TensorData::new(entree, forme), &device);
        let reference = lire(&dossier, &format!("bloc{i}.f32"));
        comparer(&format!("bloc {i}"), &vers_vec(r.bloc(i, x, &config)), &reference);
        entree = reference;
    }
    let x = Tensor::<Moteur, 3>::from_data(TensorData::new(entree, forme), &device);
    comparer("masques", &vers_vec(m.tetes(x)), &lire(&dossier, "masques.f32"));
    println!("réseau étage par étage : {:.1} s", t.elapsed().as_secs_f32());

    // De bout en bout, sans reprise.
    let t = Instant::now();
    let sortie = m.segment(&g[..seg], &d[..seg]).expect("segment");
    let duree = t.elapsed().as_secs_f32();
    let plat: Vec<f32> = sortie.iter().flat_map(|[a, b]| a.iter().chain(b).copied()).collect();
    let reference = lire(&dossier, "segment.f32");
    let par_stem = reference.len() / config.stems.len();
    println!("segment de {:.1} s en {duree:.1} s", seg as f32 / 44100.0);
    for (s, nom) in config.stems.iter().enumerate() {
        comparer(&format!("segment {nom}"), &plat[s * par_stem..(s + 1) * par_stem], &reference[s * par_stem..(s + 1) * par_stem]);
    }

    let t = Instant::now();
    let tout = m.separer(g, d, |_, _| {}).expect("séparation");
    println!("morceau de {:.1} s en {:.1} s", n as f32 / 44100.0, t.elapsed().as_secs_f32());
    let plat: Vec<f32> = tout.iter().flat_map(|[a, b]| a.iter().chain(b).copied()).collect();
    let reference = lire(&dossier, "demix.f32");
    let par_stem = reference.len() / config.stems.len();
    let mut pire = 1.0f64;
    for (s, nom) in config.stems.iter().enumerate() {
        pire = pire.min(comparer(&format!("demix {nom}"), &plat[s * par_stem..(s + 1) * par_stem], &reference[s * par_stem..(s + 1) * par_stem]));
    }
    println!("pire cosinus du morceau : {pire:.7} ({})", if pire >= 0.9999 { "parité" } else { "ÉCART" });
}
