// SPDX-License-Identifier: GPL-3.0-or-later
//! Combien de temps entre le clic et le premier son ?
//!
//! Un clic sur un morceau non préchargé passe par `Player::play` →
//! `charger` → `completer` → [`rusty_music_player::ouvrir`], verrou tenu :
//! rien ne sort avant que `ouvrir` ait rendu la source. On mesure donc,
//! fichier par fichier, `ouvrir` puis le premier échantillon tiré de la
//! source — ce que la sortie ferait. La latence de l'appareil s'y ajoute
//! (affichée en tête, `latence_sortie`) ; rien n'est joué.
//!
//! Trois passes par fichier :
//! - **1er contact** : la première lecture du fichier par ce processus
//!   (probablement hors du cache disque si le morceau n'a pas été écouté
//!   récemment ; pas garanti, `purge` demande les droits root) ;
//! - **en cache** : la même chose, fichier en cache ;
//! - **« E »** : en cache, excitation active.
//!
//! `5 s` estime une lecture progressive : ouvrir, décoder et traiter les
//! cinq premières secondes seulement (hors Opus).
//!
//! `décodage seul` isole la part du décodeur (symphonia, ou `opus-pure`) de
//! celle du traitement (gain, rééchantillonnage sinc vers la sortie, « E »).
//!
//!   cargo run --release -p rusty-music-player --example premier_son -- <fichier>…
//!
//! `RUSTY_TAUX_SORTIE` force la fréquence de sortie supposée (défaut : celle
//! de l'appareil par défaut, lue sans ouvrir de flux ; sinon 48 000).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rodio::Source;

fn taux_sortie() -> u32 {
    if let Some(t) = std::env::var("RUSTY_TAUX_SORTIE")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return t;
    }
    use rodio::cpal::traits::HostTrait;
    use rodio::DeviceTrait;
    rodio::cpal::default_host()
        .default_output_device()
        .and_then(|d| d.default_output_config().ok())
        .map(|c| c.sample_rate())
        .unwrap_or(48_000)
}

/// `ouvrir` puis premier échantillon. Rend (durée, durée de la piste).
fn premier_son(p: &Path) -> Result<(Duration, Option<Duration>), Box<dyn std::error::Error>> {
    let t = Instant::now();
    let mut source = rusty_music_player::ouvrir(p, 1.0)?;
    let duree = source.total_duration();
    let _ = source.next();
    Ok((t.elapsed(), duree))
}

/// Décodage seul, sans traitement : ce que coûte le décodeur.
fn decodage_seul(p: &Path) -> Result<Duration, Box<dyn std::error::Error>> {
    let t = Instant::now();
    if p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("opus"))
    {
        let _ = rusty_music_core::opus::decoder(p)?;
    } else {
        let d = rodio::Decoder::try_from(std::fs::File::open(p)?)?;
        let n = d.count();
        std::hint::black_box(n);
    }
    Ok(t.elapsed())
}

/// Ce que coûterait une lecture progressive : ouvrir, décoder et traiter
/// (gain, rééchantillonnage) les [`TRONCON`] premières secondes seulement.
/// `None` pour Opus, que `rusty_music_core::opus` ne décode qu'en entier.
fn premier_troncon(p: &Path) -> Result<Option<Duration>, Box<dyn std::error::Error>> {
    if p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("opus"))
    {
        return Ok(None);
    }
    let t = Instant::now();
    let d = rodio::Decoder::try_from(std::fs::File::open(p)?)?;
    let (taux, canaux) = (d.sample_rate().get(), d.channels().get());
    let mut ech: Vec<f32> = d.take_duration(TRONCON).collect();
    rusty_music_player::amelioration::traiter(&mut ech, taux, canaux, 1.0);
    Ok(Some(t.elapsed()))
}

const TRONCON: Duration = Duration::from_secs(5);

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pistes: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if pistes.is_empty() {
        eprintln!("usage : … --example premier_son -- <fichier audio>…");
        return Ok(());
    }
    let taux = taux_sortie();
    rusty_music_player::enregistrer_taux_sortie(taux);
    println!(
        "sortie supposée : {taux} Hz · latence de l'appareil : {} ms",
        ms(rusty_music_player::latence_sortie())
    );
    println!(
        "{:<6} {:>7} {:>11} {:>9} {:>9} {:>9} {:>9}  morceau",
        "format", "durée", "1er contact", "en cache", "décodage", "« E »", "5 s"
    );

    for p in &pistes {
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let nom: String = p
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(40)
            .collect();
        let (froid, duree) = match premier_son(p) {
            Ok(r) => r,
            Err(e) => {
                println!("{ext:<6} échec : {e}  {nom}");
                continue;
            }
        };
        let (chaud, _) = premier_son(p)?;
        let dec = decodage_seul(p)?;
        rusty_music_player::amelioration().set_actif(true);
        let (e, _) = premier_son(p)?;
        rusty_music_player::amelioration().set_actif(false);
        let troncon = premier_troncon(p)?.map_or("—".to_string(), |d| format!("{} ms", ms(d)));
        println!(
            "{ext:<6} {:>5} s {:>8} ms {:>6} ms {:>6} ms {:>6} ms {troncon:>9}  {nom}",
            duree.map_or(0, |d| d.as_secs()),
            ms(froid),
            ms(chaud),
            ms(dec),
            ms(e),
        );
    }
    Ok(())
}
