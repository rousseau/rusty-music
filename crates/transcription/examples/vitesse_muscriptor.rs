// SPDX-License-Identifier: GPL-3.0-or-later
//! Vitesse du décodage de MuScriptor : le même segment décodé plusieurs
//! fois (le premier passage compile les noyaux), puis en lot, et la part du
//! CPU dans un pas.
//!
//!   cargo run --release -p rusty-music-transcription --example vitesse_muscriptor -- <audio16k.f32> [passages]

use std::time::Instant;

use rusty_music_transcription::muscriptor::{self, jetons, Muscriptor, BASSES, SEGMENT};

#[cfg(feature = "gpu")]
type B = burn::backend::Wgpu;
#[cfg(not(feature = "gpu"))]
type B = burn::backend::NdArray;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let audio: Vec<f32> = std::fs::read(&a[0])?.chunks(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let passages: usize = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(3);
    let m = Muscriptor::<B>::charger(&muscriptor::poids_hugging_face("medium").expect("poids"), &Default::default())?;
    let groupes = jetons::groupes_de(BASSES);
    let masque = m.masque(BASSES);
    for k in 0..passages {
        let t = Instant::now();
        let (gen, _) = m.segment(&audio[..SEGMENT], &groupes, &[], &masque);
        println!("passage {k} : {} jetons, {:.1} ms/jeton", gen.len(), t.elapsed().as_secs_f32() * 1000.0 / gen.len() as f32);
    }
    for lot in [8usize, 16] {
        let segs: Vec<Vec<f32>> = (0..lot).map(|_| audio[..SEGMENT].to_vec()).collect();
        let t = Instant::now();
        let r = m.segments_en_lot(&segs, &groupes, &masque);
        let total: usize = r.iter().map(|x| x.0.len()).sum();
        println!("lot de {lot} : {total} jetons, {:.2} ms/jeton", t.elapsed().as_secs_f32() * 1000.0 / total as f32);
    }
    // CPU (construction des pas) contre GPU (exécution).
    let g = m.generateur();
    let mut cache = g.cache(1, 2600);
    let x = burn::tensor::Tensor::cat(vec![g.prefixe(&audio[..SEGMENT], &groupes).unsqueeze_dim::<3>(0), g.plonger(&[m.config().card as u32])], 1);
    let mut l = g.avancer(x, &mut cache);
    let _ = l.clone().into_data();
    let t = Instant::now();
    for _ in 0..64 {
        let j = (l + masque.clone()).argmax(1);
        l = g.avancer(g.reseau.emb.forward(j), &mut cache);
    }
    let cpu = t.elapsed().as_secs_f32();
    let _ = l.into_data();
    let tout = t.elapsed().as_secs_f32();
    println!("64 pas : construction {:.1} ms/pas, total {:.1} ms/pas", cpu * 1000.0 / 64.0, tout * 1000.0 / 64.0);
    Ok(())
}
