// SPDX-License-Identifier: GPL-3.0-or-later
//! Les opérations d'une attention à plusieurs têtes, à des formes choisies,
//! contre une référence en f64 calculée à la main — pour isoler le noyau qui
//! rend htdemucs_6s faux en MSL (`experiments/burn-metal/README.md`).
//!
//! `couches_demucs` a situé l'écart dans la première auto-attention de la
//! branche temporelle de htdemucs_6s : 8 têtes, séquence 1 344, têtes de 48.
//! La même attention sur 2 688 positions (branche fréquentielle) est juste,
//! et le 4 stems (1 344 positions, têtes de 64) aussi.
//!
//!   cargo run --release -p rusty-music-editor --features burn/metal --example formes_attention

use burn::tensor::{activation, Distribution, Tensor, TensorData};
use rusty_music_editor::Moteur;

/// Écart relatif maximal (sur le max absolu de la référence).
fn ecart(obtenu: &[f32], reference: &[f64]) -> f64 {
    let echelle = reference
        .iter()
        .fold(0.0f64, |m, x| m.max(x.abs()))
        .max(1e-30);
    obtenu
        .iter()
        .zip(reference)
        .map(|(a, b)| (*a as f64 - b).abs())
        .fold(0.0, f64::max)
        / echelle
}

/// Produit [h, m, k] · [h, k, n] en f64.
fn produit(a: &[f32], b: &[f32], h: usize, m: usize, k: usize, n: usize) -> Vec<f64> {
    let mut c = vec![0.0f64; h * m * n];
    for t in 0..h {
        for i in 0..m {
            for p in 0..k {
                let x = a[t * m * k + i * k + p] as f64;
                let ligne = &b[t * k * n + p * n..t * k * n + p * n + n];
                let sortie = &mut c[t * m * n + i * n..t * m * n + i * n + n];
                for (s, y) in sortie.iter_mut().zip(ligne) {
                    *s += x * *y as f64;
                }
            }
        }
    }
    c
}

fn vers_vec<const D: usize>(t: Tensor<Moteur, D>) -> Vec<f32> {
    t.into_data().to_vec::<f32>().unwrap()
}

fn essai(h: usize, s: usize, d: usize) {
    let dev = Default::default();
    let q = Tensor::<Moteur, 4>::random([1, h, s, d], Distribution::Normal(0.0, 1.0), &dev);
    let k = Tensor::<Moteur, 4>::random([1, h, s, d], Distribution::Normal(0.0, 1.0), &dev);
    let v = Tensor::<Moteur, 4>::random([1, h, s, d], Distribution::Normal(0.0, 1.0), &dev);
    let (qv, kv, vv) = (
        vers_vec(q.clone()),
        vers_vec(k.clone()),
        vers_vec(v.clone()),
    );

    // Q · Kᵀ
    let kt = k.clone().swap_dims(2, 3);
    let ktv = vers_vec(kt.clone());
    let scores = q.matmul(kt) / (d as f64).sqrt();
    let ref_scores: Vec<f64> = produit(&qv, &ktv, h, s, d, s)
        .into_iter()
        .map(|x| x / (d as f64).sqrt())
        .collect();
    let e_scores = ecart(&vers_vec(scores.clone()), &ref_scores);

    // softmax
    let poids = activation::softmax(scores, 3);
    let mut ref_poids = ref_scores.clone();
    for ligne in ref_poids.chunks_mut(s) {
        let m = ligne.iter().cloned().fold(f64::MIN, f64::max);
        let z: f64 = ligne.iter().map(|x| (x - m).exp()).sum();
        for x in ligne.iter_mut() {
            *x = (*x - m).exp() / z;
        }
    }
    let pv = vers_vec(poids.clone());
    let e_poids = ecart(&pv, &ref_poids);

    // poids · V, sur les poids rendus par le backend (isole le produit)
    let sortie = poids.matmul(v);
    let ref_sortie = produit(&pv, &vv, h, s, s, d);
    let e_sortie = ecart(&vers_vec(sortie), &ref_sortie);

    // Le même produit sur des entrées fixées (poids de la référence) : la
    // forme seule, sans dépendre du softmax.
    let p_ref: Vec<f32> = ref_poids.iter().map(|x| *x as f32).collect();
    let p_t = Tensor::<Moteur, 4>::from_data(TensorData::new(p_ref.clone(), [1, h, s, s]), &dev);
    let v_t = Tensor::<Moteur, 4>::from_data(TensorData::new(vv.clone(), [1, h, s, d]), &dev);
    let e_sortie2 = ecart(
        &vers_vec(p_t.matmul(v_t)),
        &produit(&p_ref, &vv, h, s, s, d),
    );

    println!(
        "h {h} · s {s:>5} · d {d:>3}   QKᵀ {e_scores:.1e}   softmax {e_poids:.1e}   PV {e_sortie:.1e}   PV(fixé) {e_sortie2:.1e}"
    );
    let _ = (qv, kv);
}

/// Contournement : les têtes découpées sur la dernière dimension puis
/// empilées — `stack` écrit un tenseur neuf `[1, h, s, d]`, contigu.
fn empiler(x: &Tensor<Moteur, 3>, h: usize) -> Tensor<Moteur, 4> {
    Tensor::stack(x.clone().chunk(h, 2), 1)
}

/// Q · Kᵀ comme l'attention de Burn le calcule : Q et K sortent d'une
/// projection `[1, s, h·d]`, remise en `[1, s, h, d]` puis `swap_dims(1, 2)`
/// — des vues à pas non contigus, pas des tenseurs `[1, h, s, d]` neufs.
fn essai_vues(h: usize, s: usize, d: usize) {
    let dev = Default::default();
    let x = Tensor::<Moteur, 3>::random([1, s, h * d], Distribution::Normal(0.0, 1.0), &dev);
    let y = Tensor::<Moteur, 3>::random([1, s, h * d], Distribution::Normal(0.0, 1.0), &dev);
    let q = x.clone().reshape([1, s, h, d]).swap_dims(1, 2);
    let k = y.clone().reshape([1, s, h, d]).swap_dims(1, 2);
    // Référence : les mêmes valeurs, rendues contiguës par la lecture.
    let (qv, kv) = (vers_vec(q.clone()), vers_vec(k.clone()));
    let mut ktv = vec![0.0f32; h * d * s];
    for t in 0..h {
        for i in 0..s {
            for p in 0..d {
                ktv[t * d * s + p * s + i] = kv[t * s * d + i * d + p];
            }
        }
    }
    let reference = produit(&qv, &ktv, h, s, d, s);
    let neuf = |v: &Vec<f32>| {
        Tensor::<Moteur, 4>::from_data(TensorData::new(v.clone(), [1, h, s, d]), &dev)
    };
    let marque = |e: f64| if e > 1e-4 { " ← FAUX" } else { "" };
    let mut ligne = format!("vues : h {h} · s {s:>5} · d {d:>3}  ");
    for (nom, a, b) in [
        ("q vue, k vue", q.clone(), k.clone()),
        ("q neuf, k vue", neuf(&qv), k.clone()),
        ("q vue, k neuf", q.clone(), neuf(&kv)),
        (
            "+0 sur les vues",
            q.clone().add_scalar(0.0),
            k.clone().add_scalar(0.0),
        ),
        ("têtes empilées", empiler(&x, h), empiler(&y, h)),
    ] {
        let e = ecart(&vers_vec(a.matmul(b.swap_dims(2, 3))), &reference);
        ligne += &format!("  {nom} {e:.0e}{}", marque(e));
    }
    // Contournements, sur les vues telles que Burn les donne.
    let completer = |t: Tensor<Moteur, 4>| {
        let z = Tensor::<Moteur, 4>::zeros([1, h, s, 64 - d.min(64)], &dev);
        Tensor::cat(vec![t, z], 3)
    };
    let e = ecart(
        &vers_vec(completer(q.clone()).matmul(completer(k.clone()).swap_dims(2, 3))),
        &reference,
    );
    ligne += &format!("  | têtes à 64 {e:.0e}{}", marque(e));
    let e = ecart(
        &vers_vec(k.clone().matmul(q.clone().swap_dims(2, 3)).swap_dims(2, 3)),
        &reference,
    );
    ligne += &format!("  (k·qᵀ)ᵀ {e:.0e}{}", marque(e));
    println!("{ligne}");
}

fn main() {
    println!("backend : {}", rusty_music_editor::moteur());
    for (h, s, d) in [
        (8, 1344, 48), // htdemucs_6s, branche temporelle : faux en MSL
        (8, 2688, 48), // htdemucs_6s, branche fréquentielle : juste
        (8, 1344, 64), // htdemucs, branche temporelle : juste
        (8, 1344, 32),
        (8, 1000, 48),
        (8, 1024, 48),
        (8, 1536, 48),
        (1, 1344, 48),
    ] {
        essai(h, s, d);
    }
    for (h, s, d) in [
        (8, 1344, 48),
        (8, 2688, 48),
        (8, 1344, 64),
        (8, 1344, 32),
        (8, 1000, 48),
        (8, 1024, 48),
        (8, 1536, 48),
        (8, 1280, 48),
        (8, 1408, 48),
        (8, 672, 48),
        (1, 1344, 48),
        (4, 1344, 48),
    ] {
        essai_vues(h, s, d);
    }
}
