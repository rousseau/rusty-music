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
//!
//! [`lire_borne`] et les constantes de garde-fou sont `pub` : `analysis` et
//! `editor` les réutilisent plutôt que de les recopier (voir leurs modules
//! `decode` respectifs) — la copie avait déjà divergé, la troncature d'ici ne
//! s'appliquant qu'après avoir tout chargé, alors que celle d'`analysis`
//! borne avant de collecter.

use std::path::Path;
use std::time::Duration;

use rodio::Source;
use tracing::warn;

use crate::error::{Error, Result};

/// Taille de fichier au-delà de laquelle on refuse de le charger en mémoire.
///
/// Une piste ne pèse jamais ça : c'est le signe d'un fichier corrompu, ou
/// d'autre chose que de la musique rangé sous une extension audio.
pub const TAILLE_MAX: u64 = 1_000_000_000;

/// Durée au-delà de laquelle un flux décodé est tronqué, même s'il continue
/// d'en produire — un en-tête corrompu peut sinon épuiser la mémoire avant
/// qu'on s'en aperçoive. Quatre heures, largement au-dessus de tout morceau
/// réel.
const DUREE_MAX: Duration = Duration::from_secs(4 * 3600);

/// Pause imposée après un délai dépassé qui a survécu à toutes ses
/// tentatives — rencontré en pratique sous la forme d'une véritable panique
/// noyau (`pcie-sdreader`, timeout de complétion PCIe) plutôt que d'une
/// simple erreur applicative.
pub const REPOS_APRES_TIMEOUT: Duration = Duration::from_secs(10);

/// Temps maximal accordé à une tentative de lecture avant de l'abandonner —
/// `std::fs::read` peut sinon rester bloqué des heures sur un support en
/// détresse, sans jamais renvoyer d'erreur.
pub const DELAI_LECTURE: Duration = Duration::from_secs(45);

/// Un morceau décodé intégralement, échantillons entrelacés à sa fréquence
/// et son nombre de canaux d'origine — ce que BS.1770 veut pour pondérer
/// correctement chaque canal.
pub struct PisteNative {
    pub echantillons: Vec<f32>,
    pub canaux: u16,
    pub taux: u32,
}

pub fn est_opus(chemin: &Path) -> bool {
    chemin
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("opus"))
}

/// Échantillons entrelacés au-delà desquels un flux est tronqué — voir
/// [`DUREE_MAX`].
fn max_echantillons(taux: u32, canaux: u16) -> usize {
    taux as usize * canaux as usize * DUREE_MAX.as_secs() as usize
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
        let taux = crate::opus::SR;
        let echantillons = tronquer_si_demesure(piste.echantillons, max_echantillons(taux, canaux), chemin);
        return Ok(PisteNative { echantillons, canaux, taux });
    }
    let octets = lire_borne(chemin)?;
    let decodeur =
        rodio::Decoder::try_from(std::io::Cursor::new(octets)).map_err(|source| Error::Decode {
            path: chemin.to_path_buf(),
            source,
        })?;
    let canaux = decodeur.channels().get();
    let taux = decodeur.sample_rate().get();
    let max = max_echantillons(taux, canaux);
    // `+1` pour distinguer un flux qui s'arrête pile au plafond de celui qui
    // continuait — seul ce dernier mérite l'avertissement. Le `.take()` borne
    // la collecte elle-même : contrairement à une troncature après coup, un
    // en-tête corrompu ne peut pas faire grossir `echantillons` au-delà de
    // `max + 1` avant qu'on s'en aperçoive.
    let mut echantillons: Vec<f32> = decodeur.take(max + 1).collect();
    if echantillons.len() > max {
        warn!(
            path = %chemin.display(),
            echantillons = echantillons.len(),
            "flux anormalement long, tronqué à 4 h"
        );
        echantillons.truncate(max);
    }
    Ok(PisteNative { echantillons, canaux, taux })
}

/// Coupe `echantillons` à `max` en le signalant, si besoin — utilisé pour
/// Opus, dont [`crate::opus::decoder`] rend déjà tout le flux d'un bloc (le
/// décodeur ne se prête pas facilement au streaming incrémental) : la
/// troncature n'a donc lieu qu'après coup, comme avant ce module. Un fichier
/// Opus réel reste de toute façon borné en taille par [`lire_borne`] et
/// nettement plus léger à décoder que les formats PCM.
fn tronquer_si_demesure(mut echantillons: Vec<f32>, max: usize, chemin: &Path) -> Vec<f32> {
    if echantillons.len() > max {
        warn!(
            path = %chemin.display(),
            echantillons = echantillons.len(),
            "flux anormalement long, tronqué à 4 h"
        );
        echantillons.truncate(max);
    }
    echantillons
}

/// Arrondit `taille_bloc` au plus grand multiple de `canaux` qui ne le
/// dépasse pas, avec un plancher d'une trame — un bloc qui coupe une trame en
/// deux désalignerait les canaux du bloc suivant, et un analyseur comme
/// `ebur128` (`Interleaved::new`) refuse d'ailleurs tout bloc dont la
/// longueur n'est pas un multiple du nombre de canaux.
fn aligner_sur_les_trames(taille_bloc: usize, canaux: usize) -> usize {
    let canaux = canaux.max(1);
    (taille_bloc / canaux).max(1) * canaux
}

/// Comme [`decoder_natif`], mais en flux : `f` reçoit `(canaux, taux)` puis
/// chaque bloc d'au plus `taille_bloc` échantillons entrelacés (toujours un
/// nombre entier de trames, voir [`aligner_sur_les_trames`]), plutôt que
/// l'appelant ne reçoive le morceau entier d'un coup.
///
/// `(canaux, taux)` accompagnent **chaque** appel plutôt que d'être rendus à
/// part en fin de décodage : l'appelant en a besoin dès le premier bloc pour
/// construire son propre état (`ebur128::EbuR128::new`, notamment), qui ne
/// se connaît qu'une fois le décodeur ouvert — soit avant que `f` ne
/// commence à recevoir quoi que ce soit.
///
/// Sert `crate::loudness::analyser`, qui n'a besoin de voir chaque bloc
/// qu'une fois pour l'accumuler dans son analyseur EBU R128 — ce dernier
/// est justement conçu pour un usage incrémental (`add_frames_f32` peut être
/// appelée autant de fois qu'on veut). Le fichier compressé reste chargé
/// d'un bloc par [`lire_borne`] (le disque, pas la mémoire, est la ressource
/// à ménager ici) ; seuls les échantillons **décodés**, potentiellement bien
/// plus volumineux une fois développés, ne sont plus tous gardés à la fois.
pub fn decoder_natif_par_blocs(
    chemin: &Path,
    taille_bloc: usize,
    mut f: impl FnMut(u16, u32, &[f32]),
) -> Result<()> {
    if est_opus(chemin) {
        let piste = crate::opus::decoder(chemin)?;
        let canaux = u16::try_from(piste.canaux).unwrap_or(2).max(1);
        let taux = crate::opus::SR;
        let echantillons = tronquer_si_demesure(piste.echantillons, max_echantillons(taux, canaux), chemin);
        let pas = aligner_sur_les_trames(taille_bloc, canaux as usize);
        for bloc in echantillons.chunks(pas) {
            f(canaux, taux, bloc);
        }
        return Ok(());
    }
    let octets = lire_borne(chemin)?;
    let decodeur =
        rodio::Decoder::try_from(std::io::Cursor::new(octets)).map_err(|source| Error::Decode {
            path: chemin.to_path_buf(),
            source,
        })?;
    let canaux = decodeur.channels().get();
    let taux = decodeur.sample_rate().get();
    let max = max_echantillons(taux, canaux);
    let pas = aligner_sur_les_trames(taille_bloc, canaux as usize);

    let mut tampon = Vec::with_capacity(pas);
    let mut tronque = false;
    for (vus, echantillon) in decodeur.enumerate() {
        if vus >= max {
            tronque = true;
            break;
        }
        tampon.push(echantillon);
        if tampon.len() == tampon.capacity() {
            f(canaux, taux, &tampon);
            tampon.clear();
        }
    }
    if !tampon.is_empty() {
        f(canaux, taux, &tampon);
    }
    if tronque {
        warn!(path = %chemin.display(), echantillons = max, "flux anormalement long, tronqué à 4 h");
    }
    Ok(())
}

/// Vérifie la taille avant de charger, puis lit — avec reprise sur un
/// support qui s'est fait attendre. Voir `crates/analysis/src/decode.rs`
/// pour la mésaventure qui justifie ce luxe.
pub fn lire_borne(chemin: &Path) -> Result<Vec<u8>> {
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

    /// Un WAV PCM16 mono minimal, écrit à la main — même patron que
    /// `loudness::tests::wav_sinus`.
    fn wav_sinus(nom: &str, freq: f32, taux: u32, duree_s: f32) -> std::path::PathBuf {
        let n = (taux as f32 * duree_s) as usize;
        let mut donnees = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f32 / taux as f32;
            let s = 0.5 * (2.0 * std::f32::consts::PI * freq * t).sin() * i16::MAX as f32;
            donnees.extend_from_slice(&(s as i16).to_le_bytes());
        }
        let octets_data = donnees.len() as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + octets_data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&taux.to_le_bytes());
        wav.extend_from_slice(&(taux * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&octets_data.to_le_bytes());
        wav.extend_from_slice(&donnees);
        fichier(nom, &wav)
    }

    /// `decoder_natif_par_blocs` doit rendre exactement les mêmes
    /// échantillons que `decoder_natif`, seulement répartis en blocs — c'est
    /// tout l'enjeu de la bascule en flux dans `loudness::analyser` : ne rien
    /// changer au résultat, seulement à l'empreinte mémoire.
    #[test]
    fn le_decodage_par_blocs_rend_les_memes_echantillons_quen_un_bloc() {
        let p = wav_sinus("blocs", 441.0, 44_100, 0.25);
        let entier = decoder_natif(&p).expect("décodage entier");

        let mut par_blocs = Vec::new();
        let (mut canaux_vus, mut taux_vus) = (None, None);
        decoder_natif_par_blocs(&p, 1024, |canaux, taux, bloc| {
            canaux_vus = Some(canaux);
            taux_vus = Some(taux);
            par_blocs.extend_from_slice(bloc);
        })
        .expect("décodage par blocs");

        assert_eq!(canaux_vus, Some(entier.canaux));
        assert_eq!(taux_vus, Some(entier.taux));
        assert_eq!(par_blocs, entier.echantillons);
        let _ = std::fs::remove_file(&p);
    }

    /// `aligner_sur_les_trames` : chaque bloc rendu doit rester un multiple
    /// du nombre de canaux, même quand la taille demandée n'en est pas un —
    /// sans quoi un bloc coupé en plein milieu d'une trame stéréo décalerait
    /// les canaux du bloc suivant, et `ebur128::Interleaved::new` refuserait
    /// le bloc.
    #[test]
    fn aligner_sur_les_trames_reste_un_multiple_des_canaux() {
        assert_eq!(aligner_sur_les_trames(1000, 2), 1000);
        assert_eq!(aligner_sur_les_trames(1001, 2), 1000);
        assert_eq!(aligner_sur_les_trames(1, 2), 2, "plancher d'une trame complète");
        assert_eq!(aligner_sur_les_trames(1001, 1), 1001);
    }

    /// Même vérification que le test mono ci-dessus, mais en stéréo avec une
    /// taille de bloc volontairement non multiple de 2 : c'est le cas que
    /// `aligner_sur_les_trames` doit corriger, et un mono seul ne l'aurait
    /// jamais fait échouer.
    #[test]
    fn le_decodage_par_blocs_reste_aligne_en_stereo() {
        let n = (44_100f32 * 0.2) as usize;
        let mut donnees = Vec::with_capacity(n * 2 * 2);
        for i in 0..n {
            let t = i as f32 / 44_100.0;
            let g = (0.5 * (2.0 * std::f32::consts::PI * 300.0 * t).sin() * i16::MAX as f32) as i16;
            let d = (0.5 * (2.0 * std::f32::consts::PI * 500.0 * t).sin() * i16::MAX as f32) as i16;
            donnees.extend_from_slice(&g.to_le_bytes());
            donnees.extend_from_slice(&d.to_le_bytes());
        }
        let octets_data = donnees.len() as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + octets_data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes()); // stéréo
        wav.extend_from_slice(&44_100u32.to_le_bytes());
        wav.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&octets_data.to_le_bytes());
        wav.extend_from_slice(&donnees);
        let p = fichier("blocs-stereo", &wav);

        let entier = decoder_natif(&p).expect("décodage entier");
        assert_eq!(entier.canaux, 2);

        let mut par_blocs = Vec::new();
        let mut canaux_vus = None;
        // 999, volontairement impair : jamais un multiple de 2.
        decoder_natif_par_blocs(&p, 999, |canaux, _taux, bloc| {
            canaux_vus = Some(canaux);
            assert_eq!(bloc.len() % 2, 0, "bloc désaligné sur les canaux");
            par_blocs.extend_from_slice(bloc);
        })
        .expect("décodage par blocs");

        assert_eq!(canaux_vus, Some(2));
        assert_eq!(par_blocs, entier.echantillons);
        let _ = std::fs::remove_file(&p);
    }
}
