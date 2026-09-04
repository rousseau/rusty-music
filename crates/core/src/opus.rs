// SPDX-License-Identifier: GPL-3.0-or-later
//! Décodage des fichiers Opus.
//!
//! symphonia — donc rodio, donc tout le reste du projet — ne décode pas Opus.
//! Un album entier de la bibliothèque de test restait hors de la carte.
//!
//! Deux crates, choisies pour ce qu'elles n'exigent pas : `ogg` démultiplexe le
//! conteneur, `opus-decoder` est un portage **pur Rust** de libopus, sans
//! `unsafe` ni FFI. Le crate `opus` officiel compile libopus depuis ses
//! sources et aurait imposé `cmake` à quiconque construit le projet.
//!
//! Vit dans le cœur parce que les trois modules en ont besoin — la carte pour
//! analyser, le lecteur pour jouer — et qu'ils ne partagent que lui.

use std::fs::File;
use std::path::Path;

use crate::error::{Error, Result};

/// Opus décode toujours à 48 kHz, quelle que soit la fréquence d'origine.
pub const SR: u32 = 48_000;

/// Taille maximale d'une trame Opus, par canal : 120 ms à 48 kHz.
const TRAME_MAX: usize = 5760;

/// Un fichier Opus décodé, échantillons entrelacés à [`SR`].
pub struct Piste {
    pub echantillons: Vec<f32>,
    pub canaux: usize,
}

/// Décode un fichier Opus entier.
pub fn decoder(chemin: &Path) -> Result<Piste> {
    let mut pages = ogg::PacketReader::new(File::open(chemin)?);
    let mut dec = None;
    let (mut canaux, mut a_sauter, mut gain) = (0usize, 0usize, 1.0f32);
    let mut pcm = Vec::new();
    let mut sortie = Vec::new();
    let mut entetes = 0;

    while let Some(p) = pages
        .read_packet()
        .map_err(|e| Error::Opus(format!("conteneur ogg : {e}")))?
    {
        // Les deux premiers paquets sont l'en-tête et les commentaires ; le
        // premier seul nous intéresse.
        if entetes < 2 {
            entetes += 1;
            if !p.data.starts_with(b"OpusHead") || p.data.len() < 19 {
                continue;
            }
            canaux = p.data[9] as usize;
            // Les premiers échantillons servent à amorcer le décodeur et ne
            // font pas partie du morceau : les garder ajouterait un blanc et
            // décalerait tout ce qui suit.
            a_sauter = u16::from_le_bytes([p.data[10], p.data[11]]) as usize;
            // Gain de sortie, en 1/256 de décibel.
            gain = 10f32.powf(i16::from_le_bytes([p.data[16], p.data[17]]) as f32 / 5120.0);
            if p.data[18] != 0 {
                return Err(Error::Opus(format!(
                    "canaux disposés en famille {}, seule la 0 est gérée",
                    p.data[18]
                )));
            }
            dec = Some(
                opus_decoder::OpusDecoder::new(SR, canaux)
                    .map_err(|e| Error::Opus(format!("{e:?}")))?,
            );
            pcm = vec![0.0f32; TRAME_MAX * canaux];
            continue;
        }

        let Some(d) = dec.as_mut() else {
            return Err(Error::Opus("en-tête OpusHead absent".into()));
        };
        let n = d
            .decode_float(&p.data, &mut pcm, false)
            .map_err(|e| Error::Opus(format!("{e:?}")))?;
        sortie.extend_from_slice(&pcm[..n * canaux]);
    }

    if canaux == 0 {
        return Err(Error::Opus("aucun flux Opus dans ce fichier".into()));
    }
    let debut = (a_sauter * canaux).min(sortie.len());
    let mut echantillons = sortie.split_off(debut);
    if gain != 1.0 {
        for e in &mut echantillons {
            *e *= gain;
        }
    }
    Ok(Piste {
        echantillons,
        canaux,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Même convention que les autres tests du cœur : un fichier dans le
    /// dossier temporaire, nommé d'après le processus.
    fn fichier(nom: &str, octets: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("rusty-music-opus-{}-{nom}", std::process::id()));
        std::fs::write(&p, octets).expect("écriture");
        p
    }

    /// Un fichier qui n'est pas de l'Ogg doit rendre une erreur, pas paniquer :
    /// l'extension ment parfois, et la passe d'analyse traverse la
    /// bibliothèque entière sans surveillance.
    #[test]
    fn un_fichier_illisible_echoue_proprement() {
        for (nom, octets) in [
            ("pasogg", &b"ce n'est pas un conteneur ogg"[..]),
            ("vide", &b""[..]),
        ] {
            let p = fichier(nom, octets);
            assert!(
                matches!(decoder(&p), Err(Error::Opus(_))),
                "{nom} aurait dû rendre une erreur Opus"
            );
            let _ = std::fs::remove_file(&p);
        }
    }
}
