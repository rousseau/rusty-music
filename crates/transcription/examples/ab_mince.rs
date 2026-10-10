// SPDX-License-Identifier: GPL-3.0-or-later
//! Essai temporaire : pas de décodage en lot de 16, produit mince contre
//! produit de Burn, alternés dans le même processus (minimum par variante).

#[macro_use]
mod commun;

use std::time::Instant;

use burn::tensor::{Int, Tensor, TensorData};
use rusty_music_transcription::muscriptor::{self, jetons, Muscriptor, ProduitMince, BASSES, SEGMENT};

fn executer<B: ProduitMince>() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let audio: Vec<f32> = std::fs::read(&a[0])?.chunks(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let b: usize = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(16);
    let m = Muscriptor::<B>::charger(&muscriptor::poids_hugging_face("medium").expect("poids"), &Default::default())?;
    let g = m.generateur();
    let groupes = jetons::groupes_de(BASSES);
    let masque = m.masque(BASSES).expand([b, m.config().card]);
    let prefixe = Tensor::cat((0..b).map(|_| g.prefixe(&audio[..SEGMENT], &groupes).unsqueeze_dim::<3>(0)).collect(), 0);
    let mut meilleur = [f32::MAX; 2];
    for tour in 0..8 {
        for (v, nom) in ["mince", "burn"].iter().enumerate() {
            if v == 1 {
                std::env::set_var("MUSCRIPTOR_SANS_MINCE", "1");
            } else {
                std::env::remove_var("MUSCRIPTOR_SANS_MINCE");
            }
            let mut cache = g.cache(b, prefixe.dims()[1] + 1 + 200);
            let initial = Tensor::<B, 2, Int>::from_data(TensorData::new(vec![m.config().card as i64; b], [b, 1]), g.device());
            let t = Instant::now();
            let mut l = g.avancer(Tensor::cat(vec![prefixe.clone(), g.reseau.emb.forward(initial)], 1), &mut cache);
            let _ = l.clone().into_data();
            let tp = t.elapsed().as_secs_f32() * 1000.0;
            let t = Instant::now();
            let segs: Vec<Vec<f32>> = (0..b).map(|_| audio[..SEGMENT].to_vec()).collect();
            let pre: Vec<_> = segs.iter().map(|s| g.prefixe(s, &groupes)).collect();
            let _ = Tensor::cat(pre, 0).into_data();
            println!("préremplissage {tp:.0} ms, préfixes {:.0} ms", t.elapsed().as_secs_f32() * 1000.0);
            for _ in 0..8 {
                let j = (l + masque.clone()).argmax(1);
                l = g.avancer(g.reseau.emb.forward(j), &mut cache);
            }
            let _ = l.clone().into_data();
            let t = Instant::now();
            for _ in 0..96 {
                let j = (l + masque.clone()).argmax(1);
                l = g.avancer(g.reseau.emb.forward(j), &mut cache);
            }
            let _ = l.into_data();
            let ms = t.elapsed().as_secs_f32() * 1000.0 / 96.0;
            meilleur[v] = meilleur[v].min(ms);
            println!("tour {tour} {nom} : {ms:.1} ms/pas");
        }
    }
    println!("lot {b} — minimum : mince {:.1} ms/pas, Burn {:.1} ms/pas", meilleur[0], meilleur[1]);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    selon_moteur!(executer)
}
