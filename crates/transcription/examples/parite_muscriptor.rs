// SPDX-License-Identifier: GPL-3.0-or-later
//! Parité du portage de MuScriptor avec le code d'origine : lit les
//! références écrites par `experiments/muscriptor/reference.py` et compare
//! préfixe de conditionnement, logits du premier pas, jetons de chaque
//! segment (prologue forcé repris de la référence) et notes.
//!
//!   cargo run --release -p rusty-music-transcription --example parite_muscriptor -- <dossier_ref> [model.safetensors]

use std::path::PathBuf;
use std::time::Instant;

use burn::tensor::Tensor;
use rusty_music_transcription::muscriptor::{self, jetons, Muscriptor, BASSES, SEGMENT};

#[cfg(feature = "gpu")]
type B = burn::backend::Wgpu;
#[cfg(not(feature = "gpu"))]
type B = burn::backend::NdArray;

fn lire_f32(chemin: PathBuf) -> Vec<f32> {
    std::fs::read(&chemin).unwrap_or_else(|e| panic!("{} : {e}", chemin.display())).chunks(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn ecart(a: &[f32], b: &[f32]) -> (f32, f32) {
    assert_eq!(a.len(), b.len(), "tailles différentes");
    let max = a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    let echelle = b.iter().map(|x| x.abs()).fold(0.0, f32::max);
    (max, echelle)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dossier = PathBuf::from(&a[0]);
    let poids = a.get(1).map(PathBuf::from).or_else(|| muscriptor::poids_hugging_face("medium")).expect("poids introuvables");
    let device = Default::default();
    let t0 = Instant::now();
    let m = Muscriptor::<B>::charger(&poids, &device)?;
    println!("chargé en {:.1} s ({:?})", t0.elapsed().as_secs_f32(), m.config());
    let g = m.generateur();
    let audio = lire_f32(dossier.join("audio16k.f32"));
    let groupes = jetons::groupes_de(BASSES);

    let prefixe = g.prefixe(&audio[..SEGMENT], &groupes);
    let p: Vec<f32> = prefixe.clone().into_data().convert::<f32>().into_vec()?;
    let (e, ech) = ecart(&p, &lire_f32(dossier.join("cond.f32")));
    println!("préfixe {:?} : écart max {e:.2e} (échelle {ech:.2})", prefixe.dims());

    let mut cache = g.cache(1, 2600);
    let x = Tensor::cat(vec![prefixe.unsqueeze_dim::<3>(0), g.plonger(&[m.config().card as u32])], 1);
    let l: Vec<f32> = g.avancer(x, &mut cache).into_data().convert::<f32>().into_vec()?;
    let ref_l = lire_f32(dossier.join("logits.f32"));
    let (e, ech) = ecart(&l, &ref_l);
    let am = |v: &[f32]| (0..1393).max_by(|&i, &j| v[i].total_cmp(&v[j])).unwrap();
    println!("logits du premier pas : écart max {e:.2e} (échelle {ech:.2}), argmax {} / réf. {}", am(&l), am(&ref_l));

    let ref_jetons: Vec<Vec<u32>> = serde_json::from_slice(&std::fs::read(dossier.join("jetons.json"))?)?;
    let masque = m.masque(BASSES);
    for (i, r) in ref_jetons.iter().enumerate() {
        let mut seg = audio[(i * SEGMENT).min(audio.len())..((i + 1) * SEGMENT).min(audio.len())].to_vec();
        seg.resize(SEGMENT, 0.0);
        let k = if i == 0 { 0 } else { r.iter().position(|&j| j == jetons::TIE).map_or(0, |p| p + 1) };
        let t = Instant::now();
        let (gen, fini) = m.segment(&seg, &groupes, &r[..k], &masque);
        let commun = gen.iter().zip(&r[k..]).take_while(|(x, y)| x == y).count();
        println!(
            "segment {i} : {} jetons (réf. {}), {commun} identiques en tête, fini {fini}, {:.1} ms/jeton",
            gen.len(),
            r.len() - k,
            t.elapsed().as_secs_f32() * 1000.0 / gen.len().max(1) as f32
        );
    }

    let t = Instant::now();
    let (notes, bilan) = m.transcrire_16k(&audio, BASSES, 1, |_, _| {});
    let ref_notes: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(dossier.join("notes.json"))?)?;
    let identiques = notes
        .iter()
        .filter(|n| ref_notes.iter().any(|r| r["hauteur"].as_u64() == Some(n.hauteur as u64) && (r["debut_s"].as_f64().unwrap() as f32 - n.debut_s).abs() < 0.005 && (r["fin_s"].as_f64().unwrap() as f32 - n.fin_s).abs() < 0.005))
        .count();
    println!(
        "transcription : {} notes (réf. {}), {identiques} identiques ; {} jetons en {:.1} s, boucles {:?}",
        notes.len(),
        ref_notes.len(),
        bilan.jetons,
        t.elapsed().as_secs_f32(),
        bilan.boucles
    );
    Ok(())
}
