// SPDX-License-Identifier: GPL-3.0-or-later
//! Décodage des fichiers Opus.
//!
//! symphonia — donc rodio, donc tout le reste du projet — ne décode pas Opus.
//! Un album entier de la bibliothèque de test restait hors de la carte.
//!
//! `opus-pure` (BSD-3) est un codec Opus **pur Rust**, conteneur Ogg compris,
//! sans FFI ni `build.rs`. Il remplace `ogg` + `opus-decoder`, une
//! soixantaine de fois plus lent et moins juste (`docs/journal.md`, 8 nov.
//! 2026).
//! Le crate `opus` officiel compile libopus depuis ses sources et aurait
//! imposé `cmake` à quiconque construit le projet.
//!
//! Vit dans le cœur parce que les trois modules en ont besoin — la carte pour
//! analyser, le lecteur pour jouer — et qu'ils ne partagent que lui.

use std::path::Path;

use crate::error::{Error, Result};

/// Le codec lui-même, réexporté pour les bancs qui encodent (l'encodeur
/// d'`extraits_aura`, dans `analysis`) sans le redéclarer en dépendance.
pub use opus_pure as codec;

/// Opus décode toujours à 48 kHz, quelle que soit la fréquence d'origine.
pub const SR: u32 = 48_000;

/// Un fichier Opus décodé, échantillons entrelacés à [`SR`].
pub struct Piste {
    pub echantillons: Vec<f32>,
    pub canaux: usize,
}

/// Lit `chemin` de façon bornée (plafond de taille, délai, reprise — voir
/// `crate::decode::lire_borne`) puis décode l'Opus qu'il contient.
pub fn decoder(chemin: &Path) -> Result<Piste> {
    decoder_depuis(&crate::decode::lire_borne(chemin)?)
}

fn erreur(e: opus_pure::Error) -> Error {
    Error::Opus(e.to_string())
}

/// Décode un flux Opus déjà en mémoire.
///
/// Séparée de [`decoder`] pour que la lecture du fichier passe par
/// `crate::decode::lire_borne` — sans quoi ce module n'avait ni plafond de
/// taille, ni délai, ni reprise sur un support qui se fait attendre, alors
/// que c'est justement la panique noyau rencontrée sur ce genre de support
/// qui a motivé ces garde-fous ailleurs (voir `crate::decode`).
///
/// `opus-pure` prend en charge ce que le module faisait à la main avec
/// `ogg` + `opus-decoder` : le gain de sortie de l'en-tête (appliqué par le
/// décodeur que rend [`opus_pure::OpusHead::decoder`]), le `pre-skip` — les
/// premiers échantillons amorcent le décodeur et ne font pas partie du
/// morceau — et la granule de fin, qui dit où s'arrête la dernière trame
/// complétée par l'encodeur (l'ignorer ajoutait quelques ms de blanc avant
/// la piste suivante, `examples/verif_gapless.rs`). Les deux derniers par
/// [`opus_pure::Trim`].
pub fn decoder_depuis(octets: &[u8]) -> Result<Piste> {
    let mut lecteur =
        opus_pure::OggOpusReader::new(std::io::Cursor::new(octets)).map_err(erreur)?;
    let entete = lecteur.head().clone();
    let canaux = entete.channel_count as usize;
    if entete.mapping_family != 0 {
        return Err(Error::Opus(format!(
            "canaux disposés en famille {}, seule la 0 est gérée",
            entete.mapping_family
        )));
    }
    let mut dec = entete.decoder(SR as i32).map_err(erreur)?;
    let mut rognage = opus_pure::Trim::new(&entete, SR as i32, canaux).map_err(erreur)?;
    let mut pcm = vec![0.0f32; opus_pure::MAX_PACKET_SAMPLES * canaux];
    let mut paquet = opus_pure::OggPacket::new(Vec::new(), 0, false);
    let mut echantillons = Vec::new();
    while lecteur.read_packet_into(&mut paquet).map_err(erreur)? {
        let n = dec
            .decode(&paquet.data, opus_pure::MAX_PACKET_SAMPLES, &mut pcm)
            .map_err(erreur)?;
        echantillons.extend_from_slice(rognage.keep(&paquet, &pcm[..n * canaux]));
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

    /// Aller-retour encodeur → décodeur : un flux qui déclare sa fin
    /// (granule finale, RFC 7845 §4.4) revient avec exactement ses
    /// échantillons, ni pre-skip devant ni bourrage derrière — la condition
    /// du gapless entre deux pistes d'album.
    #[test]
    fn aller_retour_a_l_echantillon_pres() {
        use codec::{Application, OggOpusWriter, OpusEncoder, OpusHead, MAX_PACKET_BYTES};
        let trame = 960;
        let total = 48_000 + 123; // pas un nombre entier de trames
        let mut enc = OpusEncoder::new(SR as i32, 1, Application::Audio).expect("encodeur");
        let entete = OpusHead::for_encoder(&enc, SR);
        let trames = (total + entete.pre_skip as usize).div_ceil(trame);
        let granule_finale = u64::from(entete.pre_skip) + total as u64;
        let mut w = OggOpusWriter::new(Vec::new(), entete).expect("écrivain");
        let mut paquet = vec![0u8; MAX_PACKET_BYTES];
        for i in 0..trames {
            let bloc: Vec<f32> = (i * trame..(i + 1) * trame)
                .map(|n| {
                    if n < total {
                        0.3 * (n as f32 * 0.05).sin()
                    } else {
                        0.0
                    }
                })
                .collect();
            let n = enc.encode(&bloc, trame, &mut paquet).expect("encodage");
            if i + 1 == trames {
                let reste = granule_finale - w.granule() as u64;
                w.write_packet_with_duration(&paquet[..n], reste as u32)
                    .expect("paquet");
            } else {
                w.write_packet(&paquet[..n]).expect("paquet");
            }
        }
        let octets = w.finish().expect("fin");
        let piste = decoder_depuis(&octets).expect("décodage");
        assert_eq!(piste.canaux, 1);
        assert_eq!(piste.echantillons.len(), total);
    }
}
