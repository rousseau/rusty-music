// SPDX-License-Identifier: GPL-3.0-or-later
//! Statistiques de chaque étage de HTDemucs sur un seul segment, pour
//! localiser la couche où un backend s'écarte d'un autre
//! (`experiments/burn-metal/README.md`, « Reprendre », point 1).
//!
//! Le `DebugListener` de demucs-core donne, à chaque étage (entrée
//! normalisée, chaque encodeur, transformeur, chaque décodeur), le min, le
//! max, la moyenne et l'écart-type du tenseur. Une ligne par étage sur la
//! sortie standard, à comparer entre deux exécutions (processeur et MSL).
//!
//!   cargo run --release -p rusty-music-editor --no-default-features --features cpu \
//!       --example couches_demucs -- <extrait.wav> htdemucs_6s > cpu.txt
//!   cargo run --release -p rusty-music-editor --features burn/metal \
//!       --example couches_demucs -- <extrait.wav> htdemucs_6s > msl.txt

use std::sync::Arc;

use demucs_core::listener::{ForwardEvent, ForwardListener, TensorStats};
use demucs_core::Demucs;
use rusty_music_editor::{decode, Moteur, Variante, SR};

struct Releve(Vec<String>);

fn ligne(nom: &str, s: &Option<TensorStats>) -> String {
    match s {
        Some(s) => format!(
            "{nom:<22} {:<22} mean {:+.9e} std {:.9e} min {:+.9e} max {:+.9e}",
            format!("{:?}", s.shape),
            s.mean,
            s.std,
            s.min,
            s.max
        ),
        None => format!("{nom:<22} -"),
    }
}

impl ForwardListener for Releve {
    fn on_event(&mut self, e: ForwardEvent) {
        match e {
            ForwardEvent::NormalizedCac { stats } => self.0.push(ligne("cac", &stats)),
            ForwardEvent::EncoderDone {
                domain,
                layer,
                stats,
                ..
            } => self.0.push(ligne(&format!("enc {domain} {layer}"), &stats)),
            ForwardEvent::TransformerDone {
                freq_stats,
                time_stats,
            } => {
                self.0.push(ligne("transfo freq", &freq_stats));
                self.0.push(ligne("transfo time", &time_stats));
            }
            ForwardEvent::DecoderInput {
                freq_stats,
                time_stats,
            } => {
                self.0.push(ligne("dec_in freq", &freq_stats));
                self.0.push(ligne("dec_in time", &time_stats));
            }
            ForwardEvent::DecoderDone {
                domain,
                layer,
                stats,
                ..
            } => self.0.push(ligne(&format!("dec {domain} {layer}"), &stats)),
            _ => {}
        }
    }

    fn wants_stats(&self) -> bool {
        true
    }
}

fn attendre<F: std::future::Future>(futur: F) -> F::Output {
    struct Reveil(std::thread::Thread);
    impl std::task::Wake for Reveil {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Reveil(std::thread::current())));
    let mut ctx = std::task::Context::from_waker(&waker);
    let mut futur = std::pin::pin!(futur);
    loop {
        match futur.as_mut().poll(&mut ctx) {
            std::task::Poll::Ready(v) => return v,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let fichier = args.next().ok_or("usage : … <extrait.wav> [variante]")?;
    let variante = args
        .next()
        .and_then(|v| Variante::analyser(&v))
        .unwrap_or(Variante::SixStems);
    eprintln!(
        "backend : {} · {}",
        rusty_music_editor::moteur(),
        variante.nom()
    );
    let poids = rusty_music_core::modeles::trouver(variante.fichier()).ok_or("poids absents")?;
    let octets = std::fs::read(poids)?;
    let options = match variante {
        Variante::Standard => demucs_core::ModelOptions::FourStem,
        Variante::SixStems => demucs_core::ModelOptions::SixStem,
        _ => return Err("htdemucs ou htdemucs_6s seulement".into()),
    };
    let m = Demucs::<Moteur>::from_bytes(options, &octets, Default::default())?;
    let audio = decode::stereo(std::path::Path::new(&fichier))?;
    let mut releve = Releve(Vec::new());
    let stems = attendre(m.separate_with_listener(&audio.gauche, &audio.droite, SR, &mut releve))?;
    for l in &releve.0 {
        println!("{l}");
    }
    for s in &stems {
        let e: f64 = s
            .left
            .iter()
            .chain(&s.right)
            .map(|x| (*x as f64).powi(2))
            .sum();
        println!(
            "stem {:<16} rms {:.9e}",
            s.id.as_str(),
            (e / (2 * s.left.len()) as f64).sqrt()
        );
    }
    Ok(())
}
