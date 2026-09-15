// SPDX-License-Identifier: GPL-3.0-or-later
//! Normalisation de volume par morceau — loudness EBU R128 / BS.1770.
//!
//! `docs/amelioration-audio.md` § « Hors périmètre — normalisation de
//! loudness » avait déjà tranché le principe : hors ligne, option du mode
//! Bibliothèque, crate `ebur128`. Distinct de `descriptors.loudness`
//! (`crates/analysis/src/descripteurs.rs`), un RMS perceptif sur 50 s de
//! fichier utilisé pour la couleur de la carte — sans pondération K ni
//! gating, impropre à égaliser un volume perçu.
//!
//! **On stocke la mesure brute (LUFS, pic vrai), pas un gain figé** : le
//! gain effectif se calcule à la lecture ([`gain_effectif_db`]). Changer
//! [`CIBLE_LUFS`] plus tard ne coûte donc aucune repasse de la bibliothèque.
//!
//! **Pas de péremption temporelle.** La loudness d'un fichier ne change
//! jamais ; seul un changement de méthode de mesure justifie un recalcul —
//! voir [`VERSION_LOUDNESS`], sur le patron de
//! `rusty_music_analysis::passe::VERSION_DESCRIPTEURS`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use tracing::warn;

use crate::db::Library;
use crate::error::{Error, Result};

/// Cible de loudness (LUFS) — convention ReplayGain 2.0 / la plupart des
/// lecteurs de bureau.
pub const CIBLE_LUFS: f64 = -18.0;

/// Version de la mesure — à incrémenter seulement si la méthode change
/// (canaux/fréquence de décodage, bug de calcul, montée majeure d'`ebur128`).
pub const VERSION_LOUDNESS: i32 = 1;

/// Référence du gain appliqué à la lecture : la piste seule, ou l'album dont
/// elle fait partie (préserve les écarts voulus entre ses pistes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeGain {
    Piste,
    Album,
}

/// Ce qu'une passe de mesure a produit.
#[derive(Debug, Default, Clone, Copy)]
pub struct Bilan {
    pub demandes: usize,
    pub mesures: usize,
    pub echecs: usize,
    /// Albums dont la loudness combinée a été (re)calculée.
    pub albums: usize,
}

/// Gain (dB) qui ramène `loudness_lufs` vers `cible`, plafonné pour que
/// `true_peak_dbtp + gain <= 0` — anti-écrêtage, pratique standard
/// ReplayGain (« prevent clipping »).
pub fn gain_effectif_db(loudness_lufs: f64, true_peak_dbtp: f64, cible: f64) -> f64 {
    (cible - loudness_lufs).min(-true_peak_dbtp)
}

/// Convertit un gain en décibels en facteur linéaire (1.0 = neutre).
pub fn db_vers_lineaire(db: f64) -> f32 {
    10f32.powf((db as f32) / 20.0)
}

fn erreur_ebur128(e: ebur128::Error) -> Error {
    Error::Parsing(format!("ebur128 : {e}"))
}

/// Décode `chemin` et lui fait traverser un analyseur `ebur128` neuf dans le
/// mode demandé — `Mode::I` seul pour la combinaison d'album (moins coûteux,
/// pas besoin du pic), `Mode::I | Mode::TRUE_PEAK` pour la mesure de piste.
fn analyser(chemin: &Path, mode: ebur128::Mode) -> Result<ebur128::EbuR128> {
    let piste = crate::decode::decoder_natif(chemin)?;
    if piste.canaux == 0 {
        return Err(Error::Parsing(format!("{} : aucun canal", chemin.display())));
    }
    let mut etat =
        ebur128::EbuR128::new(piste.canaux as u32, piste.taux, mode).map_err(erreur_ebur128)?;
    etat.add_frames_f32(&piste.echantillons)
        .map_err(erreur_ebur128)?;
    Ok(etat)
}

/// Mesure la loudness intégrée et le pic vrai (dBTP) d'un morceau.
///
/// `None` de fait (erreur) sur un morceau silencieux : BS.1770 ne trouve
/// alors aucun bloc au-dessus du seuil de gating absolu et rend une loudness
/// infinie — pas une mesure, au même titre qu'un tempo sur du silence
/// (`descripteurs::analyser_fenetres`). Le morceau joue alors sans
/// normalisation plutôt qu'avec une valeur inventée.
fn mesurer(chemin: &Path) -> Result<(f64, f64)> {
    let etat = analyser(chemin, ebur128::Mode::I | ebur128::Mode::TRUE_PEAK)?;
    let integrated_lufs = etat.loudness_global().map_err(erreur_ebur128)?;
    if !integrated_lufs.is_finite() {
        return Err(Error::Parsing(format!(
            "{} : silencieux, loudness non mesurable",
            chemin.display()
        )));
    }
    let mut pic = 0.0f64;
    for c in 0..etat.channels() {
        let p = etat.true_peak(c).map_err(erreur_ebur128)?;
        if p > pic {
            pic = p;
        }
    }
    // Le pic vrai d'`ebur128` est linéaire ; l'équation vers dBTP est celle
    // que la doc de la crate donne (`true_peak`) : 20·log10(pic).
    let true_peak_dbtp = if pic > 0.0 { 20.0 * pic.log10() } else { -100.0 };
    Ok((integrated_lufs, true_peak_dbtp))
}

/// Mesure la loudness des morceaux en attente, puis recalcule la loudness
/// d'album des albums touchés.
///
/// Même architecture que `rusty_music_analysis::passe::descripteurs` : un
/// curseur atomique et un bassin de travailleurs décodent et mesurent (aucun
/// n'ouvre la base), un fil unique écrit au fil de l'eau et se reprend —
/// `pending_loudness` ne rend que ce qui manque.
pub fn actualiser(
    lib: &Library,
    limite: i64,
    travailleurs: usize,
    mut avancement: impl FnMut(usize, usize) + Send,
) -> Result<Bilan> {
    let pistes = lib.pending_loudness(VERSION_LOUDNESS, limite)?;
    let mut bilan = Bilan {
        demandes: pistes.len(),
        ..Default::default()
    };
    if pistes.is_empty() {
        return Ok(bilan);
    }

    let total = pistes.len();
    let file: Vec<(i64, PathBuf)> = pistes
        .iter()
        .map(|p| (p.id, PathBuf::from(&p.path)))
        .collect();
    // Pour retrouver, une fois la mesure écrite, à quel album une piste
    // appartient (album vide = pas d'album, pas de gain d'album pour elle).
    let cles: HashMap<i64, (String, String)> = pistes
        .iter()
        .map(|p| (p.id, (p.album.clone(), p.album_artist.clone())))
        .collect();
    let curseur = AtomicUsize::new(0);
    let (tx, rx) = mpsc::sync_channel::<(i64, Option<(f64, f64)>)>(travailleurs.max(1) * 2);

    let mut albums_touches: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    std::thread::scope(|pool| {
        for _ in 0..travailleurs.max(1) {
            let tx = tx.clone();
            let (curseur, file) = (&curseur, &file);
            pool.spawn(move || loop {
                let i = curseur.fetch_add(1, Ordering::Relaxed);
                let Some((id, chemin)) = file.get(i) else {
                    break;
                };
                let mesure = match mesurer(chemin) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        warn!(path = %chemin.display(), error = %e, "loudness impossible");
                        None
                    }
                };
                if tx.send((*id, mesure)).is_err() {
                    break;
                }
            });
        }
        drop(tx);

        let mut vus = 0usize;
        for (id, mesure) in rx {
            match mesure {
                Some((integrated_lufs, true_peak_dbtp)) => {
                    match lib.save_loudness(id, integrated_lufs, true_peak_dbtp, VERSION_LOUDNESS) {
                        Ok(()) => {
                            bilan.mesures += 1;
                            if let Some((album, album_artist)) = cles.get(&id) {
                                if !album.is_empty() {
                                    albums_touches.insert((album.clone(), album_artist.clone()));
                                }
                            }
                        }
                        Err(e) => {
                            warn!(id, error = %e, "écriture loudness impossible");
                            bilan.echecs += 1;
                        }
                    }
                }
                None => {
                    bilan.echecs += 1;
                    if let Some((_, chemin)) = file.iter().find(|(i, _)| *i == id) {
                        let _ = lib.enregistrer_echec_scan(
                            chemin,
                            "décodage impossible pendant la mesure de loudness",
                        );
                    }
                }
            }
            vus += 1;
            avancement(vus, total);
        }
    });

    for (album, album_artist) in &albums_touches {
        match recalculer_album(lib, album, album_artist) {
            Ok(true) => bilan.albums += 1,
            Ok(false) => {}
            Err(e) => warn!(album, album_artist, error = %e, "loudness d'album impossible"),
        }
    }

    Ok(bilan)
}

/// Recalcule la loudness combinée d'un album — le programme EBU R128
/// combiné (`EbuR128::loudness_global_multiple`, les pistes comme si elles
/// étaient concaténées), pas une moyenne : le gating relatif de BS.1770 se
/// calcule sur l'ensemble du programme.
///
/// **Redécode toutes les pistes de l'album**, y compris celles déjà mesurées
/// avant cette passe : la combinaison exacte demande des états `EbuR128`
/// vivants, qu'on ne conserve pas entre deux passes (voir la décision de ne
/// stocker que la mesure brute par piste, pas un gain figé). Le coût reste
/// proportionnel à la taille d'un album, pas à la bibliothèque — ne
/// s'exécute que pour les albums qu'`actualiser` vient de toucher.
fn recalculer_album(lib: &Library, album: &str, album_artist: &str) -> Result<bool> {
    let pistes = lib.pistes_de_lalbum(album, album_artist)?;
    let mut etats = Vec::with_capacity(pistes.len());
    for (_, chemin) in &pistes {
        match analyser(Path::new(chemin), ebur128::Mode::I) {
            Ok(etat) => etats.push(etat),
            Err(e) => warn!(chemin, error = %e, "piste ignorée pour le gain d'album"),
        }
    }
    if etats.is_empty() {
        return Ok(false);
    }
    let integrated_lufs =
        ebur128::EbuR128::loudness_global_multiple(etats.iter()).map_err(erreur_ebur128)?;
    if !integrated_lufs.is_finite() {
        return Ok(false);
    }
    lib.save_album_loudness(album, album_artist, integrated_lufs, etats.len(), VERSION_LOUDNESS)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_effectif_ramene_vers_la_cible() {
        // Piste plus forte que la cible (loudness haute) → gain négatif.
        assert!(gain_effectif_db(-10.0, -1.0, -18.0) < 0.0);
        // Piste plus faible que la cible → gain positif.
        assert!(gain_effectif_db(-25.0, -20.0, -18.0) > 0.0);
    }

    #[test]
    fn gain_effectif_plafonne_contre_lecretage() {
        // Naïf : cible(-18) - lufs(-25) = +7 dB. Mais le pic est à -0.5 dBTP :
        // amplifier de 7 dB écrêterait largement. Le plafond (+0.5 dB, celui
        // qui ramène le pic à 0 dBTP pile) doit l'emporter.
        let gain = gain_effectif_db(-25.0, -0.5, -18.0);
        assert!((gain - 0.5).abs() < 1e-9, "gain = {gain}, attendu 0.5");
    }

    #[test]
    fn db_vers_lineaire_est_neutre_a_zero() {
        assert!((db_vers_lineaire(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn db_vers_lineaire_six_db_double_environ() {
        assert!((db_vers_lineaire(6.0) - 2.0).abs() < 0.05);
        assert!((db_vers_lineaire(-6.0) - 0.5).abs() < 0.02);
    }

    /// Un WAV PCM16 mono minimal, écrit à la main — pas de dépendance
    /// supplémentaire pour ce seul besoin de test.
    fn wav_sinus(nom: &str, freq: f32, taux: u32, duree_s: f32, amplitude: f32) -> PathBuf {
        let n = (taux as f32 * duree_s) as usize;
        let mut donnees = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f32 / taux as f32;
            let s = amplitude * (2.0 * std::f32::consts::PI * freq * t).sin() * i16::MAX as f32;
            donnees.extend_from_slice(&(s as i16).to_le_bytes());
        }
        let octets_data = donnees.len() as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + octets_data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&taux.to_le_bytes());
        wav.extend_from_slice(&(taux * 2).to_le_bytes()); // octets/s
        wav.extend_from_slice(&2u16.to_le_bytes()); // alignement bloc
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits/échantillon
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&octets_data.to_le_bytes());
        wav.extend_from_slice(&donnees);

        let p = std::env::temp_dir().join(format!(
            "rusty-music-loudness-{}-{nom}",
            std::process::id()
        ));
        std::fs::write(&p, wav).expect("écriture wav");
        p
    }

    /// Bout en bout : décodage natif + `ebur128`, sur un ton pur d'amplitude
    /// connue. Sert de garde-fou plutôt que de référence exacte — la valeur
    /// LUFS d'un ton pur dépend du filtre K et du gating, pas d'une formule
    /// simple ; validé une fois à la main contre `ffmpeg -af loudnorm` (accord
    /// à 0,01 LU et 0,00 dBTP sur un cas réel, cf. journal d'implémentation).
    #[test]
    fn mesurer_un_ton_pur_rend_une_loudness_plausible() {
        let p = wav_sinus("ton.wav", 1000.0, 44_100, 3.0, 0.1);
        let (lufs, tp) = mesurer(&p).expect("mesure attendue");
        assert!(lufs > -35.0 && lufs < -10.0, "lufs = {lufs}");
        assert!(tp < 0.0 && tp > -25.0, "tp = {tp}");
        let _ = std::fs::remove_file(&p);
    }

    /// Un silence numérique ne franchit le seuil de gating absolu d'aucun
    /// bloc : `mesurer` doit échouer plutôt que rendre une valeur inventée.
    #[test]
    fn mesurer_un_silence_echoue() {
        let p = wav_sinus("silence.wav", 1000.0, 44_100, 2.0, 0.0);
        assert!(mesurer(&p).is_err());
        let _ = std::fs::remove_file(&p);
    }

    /// Bout en bout : deux pistes d'un même album, d'amplitudes différentes.
    /// `actualiser` doit mesurer les deux et recalculer la loudness d'album
    /// (programme combiné, pas une moyenne — sa valeur doit donc se situer
    /// entre les deux loudness individuelles, sans leur être égale).
    #[test]
    fn actualiser_mesure_les_pistes_et_lalbum() {
        use crate::db::Library;
        use crate::tags::TrackMeta;

        let p1 = wav_sinus("album-1.wav", 1000.0, 44_100, 2.0, 0.2);
        let p2 = wav_sinus("album-2.wav", 1000.0, 44_100, 2.0, 0.05);

        let lib = Library::open_in_memory().unwrap();
        for p in [&p1, &p2] {
            lib.upsert(&TrackMeta {
                path: p.clone(),
                album: Some("Album test".into()),
                album_artist: Some("Artiste test".into()),
                ..Default::default()
            })
            .unwrap();
        }

        let bilan = actualiser(&lib, i64::MAX, 2, |_, _| {}).unwrap();
        assert_eq!(bilan.demandes, 2);
        assert_eq!(bilan.mesures, 2);
        assert_eq!(bilan.echecs, 0);
        assert_eq!(bilan.albums, 1);

        let (faits, total) = lib.compter_loudness(VERSION_LOUDNESS).unwrap();
        assert_eq!((faits, total), (2, 2));

        let (lufs1, _) = mesurer(&p1).unwrap();
        let (lufs2, _) = mesurer(&p2).unwrap();
        let g1 = lib
            .gain_lecture(&p1, ModeGain::Album, CIBLE_LUFS)
            .unwrap()
            .expect("mesurée");
        let g2 = lib
            .gain_lecture(&p2, ModeGain::Album, CIBLE_LUFS)
            .unwrap()
            .expect("mesurée");
        // Loudness d'album combinée : même cible pour les deux pistes de
        // l'album, donc même gain — contrairement au mode piste, qui les
        // ferait converger l'une vers l'autre indépendamment.
        assert!((g1 - g2).abs() < 1e-6, "g1={g1} g2={g2}");
        // La combinaison n'est ni l'une ni l'autre valeur individuelle : le
        // gain d'album doit différer de ce qu'un gain par piste donnerait
        // pour au moins l'une des deux (sinon la combinaison n'a rien fait).
        let g_piste_1 = lib
            .gain_lecture(&p1, ModeGain::Piste, CIBLE_LUFS)
            .unwrap()
            .unwrap();
        assert!(lufs1 != lufs2 && (g_piste_1 - g1).abs() > 1e-6);

        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }
}
