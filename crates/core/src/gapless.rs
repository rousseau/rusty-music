// SPDX-License-Identifier: GPL-3.0-or-later
//! Silences d'amorçage des fichiers MP4/M4A (AAC), pour enchaîner sans blanc.
//!
//! Un encodeur AAC ajoute en tête de quoi amorcer son filtre (1 024 trames, 2 112
//! pour iTunes) et complète la dernière trame en queue. Le fichier le dit, mais
//! `symphonia` 0.5 lit la liste d'édition sans l'appliquer : sans correction,
//! chaque piste ajoute de 20 à 45 ms de blanc au raccord (mesuré par
//! `crates/player/examples/verif_gapless.rs` : +41,8 ms).
//!
//! Deux sources, la première prime quand elle existe :
//!
//! 1. **`iTunSMPB`**, l'étiquette d'iTunes/Apple Music (aussi écrite par
//!    fdkaac, Nero) : retard, bourrage et nombre exact de trames utiles ;
//! 2. **la liste d'édition** (`edts/elst`), que ffmpeg et la plupart des
//!    encodeurs écrivent : début et durée du segment joué.
//!
//! Les MP3 n'ont pas besoin de ce module (`symphonia` lit l'en-tête LAME) ;
//! l'Opus a le sien dans [`crate::opus`].

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Ce qu'il faut retirer d'une piste décodée, en **trames** à sa fréquence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rognage {
    /// Trames à retirer en tête.
    pub debut: usize,
    /// Nombre de trames à garder à partir de `debut`, si le fichier le dit.
    pub longueur: Option<usize>,
}

/// Au-delà, la valeur lue est jugée absurde (0,5 s à 48 kHz) : on ne rogne pas.
const DEBUT_MAX: usize = 24_000;
/// `moov` plus gros que ça : fichier suspect, on s'abstient.
const MOOV_MAX: u64 = 32 * 1024 * 1024;

/// Rognage du fichier MP4 `chemin` décodé à `taux` Hz, ou `None` s'il n'a pas
/// de conteneur MP4, pas d'information, ou une information incohérente.
pub fn mp4(chemin: &Path, taux: u32) -> Option<Rognage> {
    let ext = chemin.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "m4a" | "mp4" | "m4b" | "m4p") {
        return None;
    }
    let moov = lire_moov(chemin)?;
    depuis_moov(&moov, taux)
}

/// Octets de la boîte `moov` (sans son en-tête), en parcourant les boîtes de
/// premier niveau sans charger le fichier : `moov` est souvent en queue.
fn lire_moov(chemin: &Path) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(chemin).ok()?;
    let total = f.metadata().ok()?.len();
    let mut pos = 0u64;
    while pos + 8 <= total {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut en = [0u8; 16];
        f.read_exact(&mut en[..8]).ok()?;
        let mut taille = u64::from(u32::from_be_bytes(en[..4].try_into().ok()?));
        let mut entete = 8u64;
        if taille == 1 {
            f.read_exact(&mut en[8..16]).ok()?;
            taille = u64::from_be_bytes(en[8..16].try_into().ok()?);
            entete = 16;
        } else if taille == 0 {
            taille = total - pos;
        }
        if taille < entete {
            return None;
        }
        if &en[4..8] == b"moov" {
            let n = taille - entete;
            if n > MOOV_MAX {
                return None;
            }
            let mut buf = vec![0u8; n as usize];
            f.read_exact(&mut buf).ok()?;
            return Some(buf);
        }
        pos += taille;
    }
    None
}

/// Boîtes consécutives d'un tampon : `(type, contenu)`. S'arrête au premier
/// en-tête incohérent plutôt que de lire hors du tampon.
fn boites(mut buf: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut v = Vec::new();
    while buf.len() >= 8 {
        let mut taille = u64::from(u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]));
        let type_: [u8; 4] = [buf[4], buf[5], buf[6], buf[7]];
        let mut entete = 8usize;
        if taille == 1 && buf.len() >= 16 {
            taille = u64::from_be_bytes(buf[8..16].try_into().unwrap_or([0; 8]));
            entete = 16;
        } else if taille == 0 {
            taille = buf.len() as u64;
        }
        let Ok(taille) = usize::try_from(taille) else {
            break;
        };
        if taille < entete || taille > buf.len() {
            break;
        }
        v.push((type_, &buf[entete..taille]));
        buf = &buf[taille..];
    }
    v
}

fn enfant<'a>(buf: &'a [u8], type_: &[u8; 4]) -> Option<&'a [u8]> {
    boites(buf)
        .into_iter()
        .find(|(t, _)| t == type_)
        .map(|(_, c)| c)
}

fn u32_be(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u64_be(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// Échelle de temps d'une boîte `mvhd` ou `mdhd` (même disposition).
fn echelle(b: &[u8]) -> Option<u32> {
    match *b.first()? {
        0 => u32_be(b, 12),
        1 => u32_be(b, 20),
        _ => None,
    }
}

fn depuis_moov(moov: &[u8], taux: u32) -> Option<Rognage> {
    if taux == 0 {
        return None;
    }
    let r = itunsmpb(moov).or_else(|| liste_edition(moov, taux))?;
    // Un fichier qui annonce un retard énorme ment, ou nous avons mal lu.
    (r.debut <= DEBUT_MAX).then_some(r)
}

/// `iTunSMPB` : texte hexadécimal « 00000000 DELAY PADDING LONGUEUR ... ».
/// Cherché dans les octets bruts de `moov` : l'étiquette est enfouie dans
/// `udta/meta/ilst/----` et ce parcours n'a pas à connaître la forme de `meta`.
fn itunsmpb(moov: &[u8]) -> Option<Rognage> {
    let cle = b"iTunSMPB";
    let i = moov.windows(cle.len()).position(|w| w == cle)? + cle.len();
    // La boîte `data` suit : « data » + 4 (type) + 4 (locale), puis le texte.
    let reste = moov.get(i..(i + 256).min(moov.len()))?;
    let d = reste.windows(4).position(|w| w == b"data")? + 4 + 8;
    let texte: String = reste
        .get(d..)?
        .iter()
        .take_while(|&&c| c.is_ascii_hexdigit() || c == b' ')
        .map(|&c| c as char)
        .collect();
    let champs: Vec<&str> = texte.split_whitespace().collect();
    let debut = usize::from_str_radix(champs.get(1)?, 16).ok()?;
    let longueur = usize::try_from(u64::from_str_radix(champs.get(3)?, 16).ok()?).ok()?;
    // Pas de trames utiles annoncées : étiquette vide ou mal remplie.
    (longueur > 0).then_some(Rognage {
        debut,
        longueur: Some(longueur),
    })
}

/// Liste d'édition de la piste audio : début (`media_time`) et durée du
/// premier segment non vide. Plusieurs segments : on s'abstient.
fn liste_edition(moov: &[u8], taux: u32) -> Option<Rognage> {
    let echelle_film = echelle(enfant(moov, b"mvhd")?)?;
    for (t, trak) in boites(moov) {
        if &t != b"trak" {
            continue;
        }
        let mdia = enfant(trak, b"mdia")?;
        // `hdlr` : version/flags (4), prédéfini (4), type de gestionnaire.
        if enfant(mdia, b"hdlr")?.get(8..12)? != b"soun" {
            continue;
        }
        let echelle_media = echelle(enfant(mdia, b"mdhd")?)?;
        let elst = enfant(enfant(trak, b"edts")?, b"elst")?;
        let version = *elst.first()?;
        let n = u32_be(elst, 4)? as usize;
        let taille = if version == 1 { 20 } else { 12 };
        let mut segments = Vec::new();
        for k in 0..n {
            let o = 8 + k * taille;
            let (duree, debut) = if version == 1 {
                (u64_be(elst, o)?, u64_be(elst, o + 8)? as i64)
            } else {
                (
                    u64::from(u32_be(elst, o)?),
                    i64::from(u32_be(elst, o + 4)? as i32),
                )
            };
            // media_time = -1 : segment vide (silence en tête), pas du contenu.
            if debut >= 0 {
                segments.push((duree, debut as u64));
            }
        }
        let [(duree, debut)] = segments[..] else {
            return None;
        };
        if echelle_media == 0 || echelle_film == 0 {
            return None;
        }
        let en_trames = |valeur: u64, echelle: u32| {
            usize::try_from(valeur * u64::from(taux) / u64::from(echelle)).ok()
        };
        return Some(Rognage {
            debut: en_trames(debut, echelle_media)?,
            // Durée du segment : 0 = « jusqu'à la fin ».
            longueur: (duree > 0)
                .then(|| en_trames(duree, echelle_film))
                .flatten(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boite(type_: &[u8; 4], contenu: &[u8]) -> Vec<u8> {
        let mut v = ((contenu.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(type_);
        v.extend_from_slice(contenu);
        v
    }

    /// `mvhd`/`mdhd` de version 0 : version+flags, 2 dates, échelle, durée.
    fn entete_temps(echelle: u32) -> Vec<u8> {
        let mut v = vec![0u8; 4 + 8];
        v.extend_from_slice(&echelle.to_be_bytes());
        v.extend_from_slice(&[0; 4]);
        v
    }

    fn elst_v0(segments: &[(u32, i32)]) -> Vec<u8> {
        let mut v = vec![0u8; 4];
        v.extend_from_slice(&(segments.len() as u32).to_be_bytes());
        for (duree, debut) in segments {
            v.extend_from_slice(&duree.to_be_bytes());
            v.extend_from_slice(&debut.to_be_bytes());
            v.extend_from_slice(&[0, 1, 0, 0]);
        }
        v
    }

    fn moov_avec(segments: &[(u32, i32)], gestionnaire: &[u8; 4]) -> Vec<u8> {
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(gestionnaire);
        let mdia = [boite(b"mdhd", &entete_temps(44_100)), boite(b"hdlr", &hdlr)].concat();
        let trak = [
            boite(b"mdia", &mdia),
            boite(b"edts", &boite(b"elst", &elst_v0(segments))),
        ]
        .concat();
        [boite(b"mvhd", &entete_temps(1000)), boite(b"trak", &trak)].concat()
    }

    #[test]
    fn la_liste_d_edition_donne_debut_et_longueur() {
        // 1 024 trames d'amorçage ; segment de 3 s dans une échelle de 1 000.
        let moov = moov_avec(&[(3000, 1024)], b"soun");
        let r = depuis_moov(&moov, 44_100).expect("rognage");
        assert_eq!(
            r,
            Rognage {
                debut: 1024,
                longueur: Some(132_300)
            }
        );
    }

    #[test]
    fn un_segment_vide_en_tete_ou_plusieurs_segments_font_s_abstenir() {
        // Un segment vide est ignoré, mais deux segments de contenu : on renonce.
        assert!(depuis_moov(&moov_avec(&[(3000, 1024), (1000, 5000)], b"soun"), 44_100).is_none());
        let seul = depuis_moov(&moov_avec(&[(500, -1), (3000, 1024)], b"soun"), 44_100);
        assert_eq!(seul.map(|r| r.debut), Some(1024));
    }

    #[test]
    fn une_piste_qui_n_est_pas_audio_est_ignoree() {
        assert!(depuis_moov(&moov_avec(&[(3000, 1024)], b"vide"), 44_100).is_none());
    }

    #[test]
    fn un_retard_absurde_n_est_pas_applique() {
        assert!(depuis_moov(&moov_avec(&[(3000, 900_000)], b"soun"), 44_100).is_none());
    }

    #[test]
    fn itunsmpb_prime_sur_la_liste_d_edition() {
        let texte = b" 00000000 00000840 000001C0 000000000002B6C0 00000000";
        let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
        data.extend_from_slice(texte);
        let tag = [
            boite(b"mean", b"\0\0\0\0com.apple.iTunes"),
            boite(b"name", b"\0\0\0\0iTunSMPB"),
            boite(b"data", &data),
        ]
        .concat();
        let moov = [moov_avec(&[(3000, 1024)], b"soun"), boite(b"----", &tag)].concat();
        let r = depuis_moov(&moov, 44_100).expect("rognage");
        // 0x840 = 2 112 de retard ; 0x2B6C0 = 177 856 trames utiles.
        assert_eq!(
            r,
            Rognage {
                debut: 2112,
                longueur: Some(177_856)
            }
        );
    }

    #[test]
    fn un_tampon_tronque_ne_fait_pas_paniquer() {
        let moov = moov_avec(&[(3000, 1024)], b"soun");
        for n in 0..moov.len() {
            let _ = depuis_moov(&moov[..n], 44_100);
        }
    }
}
