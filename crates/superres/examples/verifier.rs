//! Vérifie le pipeline de super-résolution contre des références PyTorch.
//!
//!   # 1. exporter le modèle
//!   ./scripts/preparer-aero.sh --checkpoint musdb-hl256.th --segment 5
//!   # 2. générer les références (fichier entier + variante « chunké »)
//!   PYTHONPATH=/tmp/rusty-music-aero-src python3 \
//!       crates/superres/examples/reference_pipeline.py \
//!       --checkpoint musdb-hl256.th --wav /tmp/sr_in.wav --out /tmp/sr_ref
//!   # 3.
//!   cargo run -p rusty-music-superres --example verifier --release -- \
//!       models/aero-11025-44100.onnx /tmp/sr_in.wav /tmp/sr_ref
//!
//! Deux mesures :
//!   * pipeline complet (avec notre rééchantillonneur) vs PyTorch sur le
//!     fichier entier — l'écart inclut la différence `rubato` / `torchaudio` ;
//!   * pipeline à partir du `lr` de référence (`<out>.lr.f32`) vs PyTorch —
//!     isole le réseau + STFT + recouvrement, doit être < 1 %.

use std::path::Path;
use std::time::Instant;

fn lire(p: &str) -> Vec<f32> {
    std::fs::read(p)
        .unwrap_or_else(|e| panic!("{p} : {e}"))
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn decoder_mono(p: &Path) -> Vec<f32> {
    use rodio::Source;
    let s = rodio::Decoder::try_from(std::fs::File::open(p).unwrap()).unwrap();
    let ch = s.channels().get() as usize;
    s.collect::<Vec<f32>>().chunks(ch).map(|c| c.iter().sum::<f32>() / ch as f32).collect()
}

fn dit(quoi: &str, o: &[f32], a: &[f32]) {
    let n = o.len().min(a.len());
    let (mut num, mut da, mut db, mut mx) = (0.0f64, 0.0f64, 0.0f64, 0.0f32);
    for (x, y) in o[..n].iter().zip(&a[..n]) {
        num += (*x as f64) * (*y as f64);
        da += (*x as f64).powi(2);
        db += (*y as f64).powi(2);
        mx = mx.max((x - y).abs());
    }
    let err: f64 = o[..n].iter().zip(&a[..n]).map(|(x, y)| ((x - y) as f64).powi(2)).sum();
    println!("  {quoi:<40} cos {:.6}  rel {:.2e}  maxabs {:.2e}", num / (da.sqrt() * db.sqrt()), (err / db).sqrt(), mx);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let modele = Path::new(&a[1]);
    let entree = Path::new(&a[2]);
    let base = &a[3];

    let mut m = rusty_music_superres::Modele::charger(modele).unwrap();
    let hr_ref = lire(&format!("{base}.hr.f32"));

    // 1. pipeline complet — MÉLANGE la source, donc s'écarte volontairement de
    //    `model()` seul dans le bas du spectre (c'est le but). Sert surtout à
    //    mesurer le temps.
    let sortie = std::env::temp_dir().join("verif_full.wav");
    let t0 = Instant::now();
    let _ = rusty_music_superres::regenerer(entree, &sortie, &mut m, |_, _| {}).unwrap();
    let dt = t0.elapsed().as_secs_f32();
    let got = decoder_mono(&sortie);
    println!("pipeline complet (avec mélange source) :");
    dit("Rust vs PyTorch entier", &got, &hr_ref);

    // 2. sortie brute du modèle depuis le lr de référence — LA parité qui
    //    compte : STFT + réseau + iSTFT + recouvrement, sans mélange.
    let lr = lire(&format!("{base}.lr.f32"));
    let sortie = std::env::temp_dir().join("verif_lr.wav");
    rusty_music_superres::regenerer_depuis_lr(&[lr], &sortie, &mut m, |_, _| {}).unwrap();
    let got = decoder_mono(&sortie);
    println!("modèle seul depuis lr de référence :");
    dit("Rust vs PyTorch entier", &got, &hr_ref);

    let audio = hr_ref.len() as f32 / rusty_music_superres::HR_SR as f32;
    println!("\n{dt:.1}s pour {audio:.1}s d'audio  (×{:.1} le temps réel, mono)", audio / dt);
}
