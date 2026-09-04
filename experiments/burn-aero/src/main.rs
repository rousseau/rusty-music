//! Essai burn-aero : valide le pipeline « STFT Rust → réseau ONNX → iSTFT Rust »
//! contre les références PyTorch de `reference_aero.py`.
//!
//!   ./scripts/preparer-aero.sh --checkpoint musdb-hl256.th --segment 5
//!   PYTHONPATH=/tmp/rusty-music-aero-src python3 reference_aero.py \
//!       --checkpoint musdb-hl256.th --out /tmp/aero-ref
//!   cargo run --release -- ../../models/aero-11025-44100.onnx /tmp/aero-ref

mod stft;

use std::collections::HashMap;
use std::time::Instant;

fn lire(chemin: &str) -> Vec<f32> {
    std::fs::read(chemin)
        .unwrap_or_else(|e| panic!("{chemin} : {e}"))
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn meta(base: &str) -> HashMap<String, usize> {
    std::fs::read_to_string(format!("{base}.meta.txt"))
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(' ')?;
            Some((k.to_string(), v.trim().parse().ok()?))
        })
        .collect()
}

/// cosinus, erreur L2 relative, écart absolu max.
fn ecart(obtenu: &[f32], attendu: &[f32]) -> (f64, f64, f32) {
    assert_eq!(obtenu.len(), attendu.len(), "tailles : {} vs {}", obtenu.len(), attendu.len());
    let (mut num, mut da, mut db, mut mx) = (0.0f64, 0.0f64, 0.0f64, 0.0f32);
    for (a, b) in obtenu.iter().zip(attendu) {
        num += (*a as f64) * (*b as f64);
        da += (*a as f64).powi(2);
        db += (*b as f64).powi(2);
        mx = mx.max((a - b).abs());
    }
    let err: f64 = obtenu.iter().zip(attendu).map(|(a, b)| ((a - b) as f64).powi(2)).sum();
    (num / (da.sqrt() * db.sqrt()), (err / db).sqrt(), mx)
}

fn dit(quoi: &str, o: &[f32], a: &[f32]) {
    let (cos, rel, mx) = ecart(o, a);
    println!("  {quoi:<28} cos {cos:.7}  rel {rel:.2e}  maxabs {mx:.2e}");
}

fn ort_reseau(modele: &str, spec: &[f32], t: usize) -> (Vec<f32>, f32) {
    let mut sess = ort::session::Session::builder()
        .unwrap()
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
        .unwrap()
        .commit_from_file(modele)
        .unwrap();
    let arr = ndarray::Array::from_shape_vec((1, 2, stft::BINS, t), spec.to_vec()).unwrap();
    let val = ort::value::Tensor::from_array(arr).unwrap();
    let _ = sess.run(ort::inputs!["spec" => val.view()]).unwrap();
    let t0 = Instant::now();
    let out = sess.run(ort::inputs!["spec" => val.view()]).unwrap();
    let dt = t0.elapsed().as_secs_f32();
    let (_, o) = out["spec_hr"].try_extract_tensor::<f32>().unwrap();
    (o.to_vec(), dt)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let modele = args.get(1).map(String::as_str).unwrap_or("../../models/aero-11025-44100.onnx");
    let base = args.get(2).map(String::as_str).unwrap_or("/tmp/aero-ref");

    let m = meta(base);
    let (t_ref, nhr) = (m["T"], m["Nhr"]);
    let lr = lire(&format!("{base}.lr.f32"));
    let spec_ref = lire(&format!("{base}.spec.f32"));
    let spec_hr_ref = lire(&format!("{base}.spec_hr.f32"));
    let hr_ref = lire(&format!("{base}.hr.f32"));
    println!("T={t_ref}  Nlr={}  Nhr={nhr}\n", lr.len());

    // 1. STFT Rust vs _spec PyTorch
    let (spec, t) = stft::spec(&lr);
    assert_eq!(t, t_ref, "nombre de trames");
    println!("STFT :");
    dit("spec Rust vs PyTorch", &spec, &spec_ref);

    // 2. iSTFT Rust : partant du spectre HR de référence, retrouver l'audio HR
    println!("iSTFT :");
    let hr_from_ref = stft::ispec(&spec_hr_ref, t);
    let n = hr_from_ref.len().min(hr_ref.len());
    dit("ispec(spec_hr_ref) vs hr", &hr_from_ref[..n], &hr_ref[..n]);

    // 3. réseau ONNX (ort) sur le spectre de référence
    println!("réseau :");
    let (spec_hr, dt) = ort_reseau(modele, &spec_ref, t_ref);
    dit("ort(spec_ref) vs spec_hr", &spec_hr, &spec_hr_ref);

    // 4. pipeline complet : STFT Rust → ort → iSTFT Rust vs model() PyTorch
    println!("pipeline complet :");
    let (spec_hr2, _) = ort_reseau(modele, &spec, t);
    let hr = stft::ispec(&spec_hr2, t);
    let n = hr.len().min(hr_ref.len());
    dit("Rust bout-à-bout vs hr", &hr[..n], &hr_ref[..n]);

    let audio_s = nhr as f32 / m["hr_sr"] as f32;
    println!("\nort : {dt:.2}s pour {audio_s:.1}s d'audio  (×{:.1} le temps réel)", audio_s / dt);
}
