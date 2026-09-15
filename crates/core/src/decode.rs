// SPDX-License-Identifier: GPL-3.0-or-later
//! Décodage pleine piste, canaux et fréquence d'origine préservés.
//!
//! Distinct de `crates/analysis/src/decode.rs` (mono, 48 kHz, cinq fenêtres
//! de dix secondes pour l'encodeur CLAP) et de `crates/editor/src/decode.rs`
//! (stéréo, 44,1 kHz, pour Demucs) : la mesure de loudness EBU R128/BS.1770
//! (`crate::loudness`) veut le fichier **entier**, sans sous-mixage ni
//! rééchantillonnage — la norme pondère par canal réel et intègre sur tout
//! le programme, pas sur un extrait.
//!
//! Vit dans le cœur, comme `opus.rs`, parce que la passe de loudness doit
//! tourner depuis la CLI et le mode Bibliothèque sans dépendre d'`analysis`
//! ni d'`editor` — et que `crates/core` ne peut de toute façon pas dépendre
//! d'eux (c'est l'inverse).

use std::path::Path;
use std::time::Duration;

use rodio::Source;
use tracing::warn;

use crate::error::{Error, Result};

/// Taille de fichier au-delà de laquelle on refuse de le charger en mémoire.
///
/// Même plafond et même raison que `crates/analysis/src/decode.rs` : une
/// piste ne pèse jamais ça, c'est le signe d'un fichier corrompu ou d'autre
/// chose que de la musique rangé sous une extension audio.
const TAILLE_MAX: u64 = 1_000_000_000;

/// Durée au-delà de laquelle un flux décodé est tronqué, même s'il continue
/// d'en produire — un en-tête corrompu peut sinon épuiser la mémoire avant
/// qu'on s'en aperçoive. Quatre heures, largement au-dessus de tout morceau
/// réel.
const DUREE_MAX: Duration = Duration::from_secs(4 * 3600);

/// Pause imposée après un délai dépassé qui a survécu à toutes ses
/// tentatives — voir `crates/analysis/src/decode.rs`, même mésaventure
/// rencontrée en pratique (panique noyau `pcie-sdreader`).
const REPOS_APRES_TIMEOUT: Duration = Duration::from_secs(10);

/// Temps maximal accordé à une tentative de lecture avant de l'abandonner —
/// `std::fs::read` peut sinon rester bloqué des heures sur un support en
/// détresse, sans jamais renvoyer d'erreur.
const DELAI_LECTURE: Duration = Duration::from_secs(45);

/// Un morceau décodé intégralement, échantillons entrelacés à sa fréquence
/// et son nombre de canaux d'origine — ce que BS.1770 veut pour pondérer
/// correctement chaque canal.
pub struct PisteNative {
    pub echantillons: Vec<f32>,
    pub canaux: u16,
    pub taux: u32,
}

fn est_opus(chemin: &Path) -> bool {
    chemin
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("opus"))
}

/// Décode `chemin` entièrement, sans sous-mixage ni rééchantillonnage.
///
/// Aiguille vers [`crate::opus::decoder`] pour les `.opus` (symphonia, donc
/// `rodio`, ne les décode pas) ; le reste passe par `rodio::Decoder`, lu
/// d'un bloc en mémoire pour ne pas dépendre du disque pendant tout le
/// décodage (comme `crates/analysis/src/decode.rs`).
pub fn decoder_natif(chemin: &Path) -> Result<PisteNative> {
    if est_opus(chemin) {
        let piste = crate::opus::decoder(chemin)?;
        let canaux = u16::try_from(piste.canaux).unwrap_or(2).max(1);
        return Ok(tronquer(
            PisteNative {
                echantillons: piste.echantillons,
                canaux,
                taux: crate::opus::SR,
            },
            chemin,
        ));
    }
    let octets = lire_borne(chemin)?;
    let decodeur =
        rodio::Decoder::try_from(std::io::Cursor::new(octets)).map_err(|source| Error::Decode {
            path: chemin.to_path_buf(),
            source,
        })?;
    let canaux = decodeur.channels().get();
    let taux = decodeur.sample_rate().get();
    let echantillons: Vec<f32> = decodeur.collect();
    Ok(tronquer(
        PisteNative {
            echantillons,
            canaux,
            taux,
        },
        chemin,
    ))
}

fn tronquer(mut piste: PisteNative, chemin: &Path) -> PisteNative {
    let max = piste.taux as usize * piste.canaux as usize * DUREE_MAX.as_secs() as usize;
    if piste.echantillons.len() > max {
        warn!(
            path = %chemin.display(),
            echantillons = piste.echantillons.len(),
            "flux anormalement long, tronqué à 4 h"
        );
        piste.echantillons.truncate(max);
    }
    piste
}

/// Vérifie la taille avant de charger, puis lit — avec reprise sur un
/// support qui s'est fait attendre. Voir `crates/analysis/src/decode.rs`
/// pour la mésaventure qui justifie ce luxe.
fn lire_borne(chemin: &Path) -> Result<Vec<u8>> {
    let taille = std::fs::metadata(chemin)?.len();
    if taille > TAILLE_MAX {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{taille} octets, plafond {TAILLE_MAX}"),
        )));
    }

    const TENTATIVES: u32 = 3;
    for tentative in 0..TENTATIVES {
        match lire_avec_delai(chemin, DELAI_LECTURE) {
            Ok(octets) => return Ok(octets),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut && tentative + 1 < TENTATIVES => {
                warn!(path = %chemin.display(), tentative, "lecture en délai dépassé, nouvel essai");
                std::thread::sleep(Duration::from_millis(300 * (tentative as u64 + 1)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                warn!(
                    path = %chemin.display(),
                    "délai dépassé à bout de tentatives, pause avant de continuer"
                );
                std::thread::sleep(REPOS_APRES_TIMEOUT);
                return Err(Error::Io(e));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
    unreachable!("la boucle rend toujours avant d'épuiser ses tentatives")
}

/// Lit `chemin`, sans jamais attendre `std::fs::read` plus que
/// [`DELAI_LECTURE`] — voir `crates/analysis/src/decode.rs` pour la panique
/// noyau qui justifie ce fil à part.
fn lire_avec_delai(chemin: &Path, delai: Duration) -> std::io::Result<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let chemin = chemin.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(std::fs::read(&chemin));
    });
    rx.recv_timeout(delai)
        .unwrap_or_else(|_| Err(std::io::Error::from(std::io::ErrorKind::TimedOut)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fichier(nom: &str, octets: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rusty-music-decode-{}-{nom}",
            std::process::id()
        ));
        std::fs::write(&p, octets).expect("écriture");
        p
    }

    /// Un fichier illisible doit rendre une erreur, pas paniquer : la passe
    /// de loudness traverse la bibliothèque entière sans surveillance.
    #[test]
    fn un_fichier_illisible_echoue_proprement() {
        let p = fichier("pasaudio", b"ce n'est pas un fichier audio");
        assert!(decoder_natif(&p).is_err());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn un_fichier_absent_echoue_proprement() {
        let p = std::env::temp_dir().join(format!(
            "rusty-music-decode-{}-absent.flac",
            std::process::id()
        ));
        assert!(decoder_natif(&p).is_err());
    }
}
