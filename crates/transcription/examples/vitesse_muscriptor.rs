// SPDX-License-Identifier: GPL-3.0-or-later
//! Vitesse du décodage de MuScriptor : le même segment décodé plusieurs
//! fois (le premier passage compile et règle les noyaux), puis en lot (un
//! passage de chauffe, un mesuré), et la part du CPU dans un pas. Backend
//! choisi par `MOTEUR` (voir `commun/mod.rs`).
//!
//!   MOTEUR=f16 cargo run --release -p rusty-music-transcription --example vitesse_muscriptor -- <audio16k.f32> [passages]

#[macro_use]
mod commun;

use std::time::Instant;

use rusty_music_transcription::muscriptor::{self, ProduitMince, jetons, Muscriptor, BASSES, SEGMENT};

fn executer<B: ProduitMince>() -> Result<(), Box<dyn std::error::Error>> {
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
    for lot in [16usize] {
        // Segments différents : les cinq premiers de l'extrait, en boucle.
        let n = audio.len() / SEGMENT;
        let segs: Vec<Vec<f32>> = (0..lot).map(|i| audio[(i % n) * SEGMENT..(i % n + 1) * SEGMENT].to_vec()).collect();
        for passage in ["chauffe", "mesure"] {
            let t = Instant::now();
            let r = m.segments_en_lot(&segs, &groupes, &masque);
            let total: usize = r.iter().map(|x| x.0.len()).sum();
            let pas = r.iter().map(|x| x.0.len()).max().unwrap_or(0) + 1;
            let ms = t.elapsed().as_secs_f32() * 1000.0;
            println!("lot de {lot} ({passage}) : {total} jetons, {pas} pas, {:.2} ms/jeton, {:.1} ms/pas", ms / total as f32, ms / pas as f32);
        }
    }
    // Lot de 16 : le coût d'un pas selon la position (la part de
    // l'attention croît avec le cache, celle des poids non).
    let g = m.generateur();
    let b = 16;
    let prefixe = burn::tensor::Tensor::cat((0..b).map(|_| g.prefixe(&audio[..SEGMENT], &groupes).unsqueeze_dim::<3>(0)).collect(), 0);
    let mut cache = g.cache(b, prefixe.dims()[1] + 1 + 512);
    let initial = burn::tensor::Tensor::<B, 2, burn::tensor::Int>::from_data(burn::tensor::TensorData::new(vec![m.config().card as i64; b], [b, 1]), g.device());
    let mut l = g.avancer(burn::tensor::Tensor::cat(vec![prefixe, g.reseau.emb.forward(initial)], 1), &mut cache);
    let masque_b = masque.clone().expand([b, m.config().card]);
    let _ = l.clone().into_data();
    for bloc in 0..4 {
        let t = Instant::now();
        for _ in 0..128 {
            let j = (l + masque_b.clone()).argmax(1);
            l = g.avancer(g.reseau.emb.forward(j), &mut cache);
        }
        let _ = l.clone().into_data();
        println!("lot de {b}, pas {}..{} : {:.1} ms/pas", bloc * 128, (bloc + 1) * 128, t.elapsed().as_secs_f32() * 1000.0 / 128.0);
    }
    drop(cache);
    // CPU (construction des pas) contre GPU (exécution).
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    selon_moteur!(executer)
}
