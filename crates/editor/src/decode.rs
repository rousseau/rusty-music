// SPDX-License-Identifier: GPL-3.0-or-later
//! Décodage vers ce qu'attend le démixage : stéréo, 44,1 kHz, entier.
//!
//! Rien à voir avec `analysis::decode`, qui rend du mono 48 kHz par fenêtres
//! de dix secondes. Ici il faut **tout** le morceau, dans ses deux canaux, à
//! la fréquence sur laquelle Demucs a été entraîné — le modèle sait
//! rééchantillonner lui-même, mais rééchantillonner deux fois n'a jamais
//! amélioré personne.

use std::path::{Path, PathBuf};

use rodio::source::UniformSourceIterator;
use rodio::Decoder;

/// Fréquence d'entraînement de Demucs. Y arriver directement évite au modèle
/// de rééchantillonner à son tour.
pub const SR: u32 = 44_100;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ouverture impossible de {path} : {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("format non décodable pour {path} : {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: rodio::decoder::DecoderError,
    },
    #[error("{path} ne contient aucun échantillon")]
    Vide { path: PathBuf },
}

/// Un morceau décodé, canaux séparés — la forme que `demucs-core` attend.
pub struct Stereo {
    pub gauche: Vec<f32>,
    pub droite: Vec<f32>,
}

impl Stereo {
    /// Durée du morceau, en secondes.
    pub fn duree(&self) -> f64 {
        self.gauche.len() as f64 / SR as f64
    }
}

/// Décode un fichier entier en stéréo 44,1 kHz.
///
/// Le fichier est lu d'un bloc puis décodé depuis la mémoire, comme dans
/// `analysis::decode` et pour la même raison : sur un support lent, c'est le
/// seul motif d'accès servi au débit nominal.
pub fn stereo(path: &Path) -> Result<Stereo, Error> {
    let octets = std::fs::read(path).map_err(|source| Error::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let decodeur =
        Decoder::try_from(std::io::Cursor::new(octets)).map_err(|source| Error::Decode {
            path: path.to_path_buf(),
            source,
        })?;

    // Deux canaux, quelle que soit la source : un mono sera dupliqué, un
    // 5.1 replié. `rodio` s'en charge en flux.
    let entrelace: Vec<f32> = UniformSourceIterator::new(
        decodeur,
        2.try_into().expect("2 canaux"),
        SR.try_into().expect("44,1 kHz"),
    )
    .collect();

    if entrelace.is_empty() {
        return Err(Error::Vide {
            path: path.to_path_buf(),
        });
    }

    let n = entrelace.len() / 2;
    let mut gauche = Vec::with_capacity(n);
    let mut droite = Vec::with_capacity(n);
    for paire in entrelace.chunks_exact(2) {
        gauche.push(paire[0]);
        droite.push(paire[1]);
    }
    Ok(Stereo { gauche, droite })
}
