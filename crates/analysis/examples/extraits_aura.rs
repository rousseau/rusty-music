// SPDX-License-Identifier: GPL-3.0-or-later
//! Les extraits de l'aura (`docs/plan-balade.md`, P4, étape 1) : faut-il un
//! cache d'extraits Opus, ou suffit-il de décoder à la volée ?
//!
//! Sur 500 morceaux tirés au hasard (graine fixe) parmi ceux qui ont une
//! empreinte, le banc mesure :
//!
//! - **à froid**, avant toute autre lecture du fichier : la latence d'un
//!   décodage à la volée avec saut (`rusty_music_player::flux_extrait`),
//!   jusqu'au premier échantillon puis jusqu'aux 15 s entières, sur
//!   l'extrait de la fenêtre centrale ;
//! - la fenêtre représentative (`passe::fenetre_representative`) parmi les
//!   cinq de la passe d'empreintes, et à quel rang elle tombe ;
//! - le temps d'extraction d'un extrait de 15 s (décodage avec saut,
//!   puis encodage Opus mono 48 kbit/s), et sa taille ;
//! - la latence d'un extrait en cache : lire le `.opus`, le décoder
//!   (`rusty_music_core::opus`) ;
//! - à chaud, le décodage à la volée avec saut, et sans saut (`skip_duration`,
//!   ce que fait l'extrait du carrefour de P2).
//!
//! ```text
//! cargo run --release -p rusty-music-analysis --example extraits_aura -- \
//!     [rusty-music.db] [dossier des extraits] [morceaux]
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use opus_pure::{Application, OggOpusWriter, OpusEncoder, OpusHead, MAX_PACKET_BYTES};
use rodio::Source;
use rusty_music_analysis::alea::Alea;
use rusty_music_analysis::decode::{debut_extrait, fenetres, EXTRAIT_S, FENETRES};
use rusty_music_analysis::mel::Mel;
use rusty_music_analysis::passe::fenetre_representative;
use rusty_music_analysis::Embedder;
use rusty_music_core::Library;
use rusty_music_player::{flux_extrait, SR_EXTRAIT};

const GRAINE: u64 = 0xA0_8B;
const DEBIT: i32 = 48_000;

/// Encode `pcm` (mono, 48 kHz) en Ogg Opus, en mémoire.
fn encoder(pcm: &[f32]) -> anyhow::Result<Vec<u8>> {
    let trame = (SR_EXTRAIT / 50) as usize;
    let mut enc = OpusEncoder::new(SR_EXTRAIT as i32, 1, Application::Audio)?;
    enc.bitrate_bps = DEBIT;
    let tete = OpusHead::for_encoder(&enc, SR_EXTRAIT);
    let total = pcm.len();
    let avance = tete.pre_skip as usize;
    let trames = (total + avance).div_ceil(trame);
    let granule_final = u64::from(tete.pre_skip) + total as u64;
    let mut ecrivain = OggOpusWriter::new(Vec::new(), tete)?;
    let mut paquet = vec![0u8; MAX_PACKET_BYTES];
    let mut bloc = vec![0.0f32; trame];
    for i in 0..trames {
        let debut = (i * trame).min(total);
        let fin = (debut + trame).min(total);
        bloc[..fin - debut].copy_from_slice(&pcm[debut..fin]);
        bloc[fin - debut..].fill(0.0);
        let n = enc.encode(&bloc, trame, &mut paquet)?;
        if i + 1 == trames {
            let reste = granule_final - ecrivain.granule() as u64;
            ecrivain.write_packet_with_duration(&paquet[..n], reste as u32)?;
        } else {
            ecrivain.write_packet(&paquet[..n])?;
        }
    }
    Ok(ecrivain.finish()?)
}

/// Premier échantillon, puis l'extrait entier : (ms, ms, échantillons).
fn a_la_volee(chemin: &Path, debut: Duration, sauter: bool) -> anyhow::Result<(f64, f64, usize)> {
    let n = (EXTRAIT_S * f64::from(SR_EXTRAIT)) as usize;
    let t = Instant::now();
    let mut flux: Box<dyn Source<Item = f32> + Send> = if sauter {
        flux_extrait(chemin, debut)?
    } else {
        // Ce que fait `ouvrir_extrait` (P2) : décoder jusqu'au départ.
        let mut f = flux_extrait(chemin, Duration::ZERO)?;
        let jusqua = (debut.as_secs_f64() * f64::from(SR_EXTRAIT)) as usize;
        for _ in 0..jusqua {
            f.next();
        }
        f
    };
    let premier = flux.next();
    let t_premier = t.elapsed().as_secs_f64() * 1e3;
    let reste = flux.take(n.saturating_sub(1)).count() + usize::from(premier.is_some());
    Ok((t_premier, t.elapsed().as_secs_f64() * 1e3, reste))
}

fn quantiles(nom: &str, unite: &str, v: &mut [f64]) {
    if v.is_empty() {
        println!("| {nom} | — | — | — | — |");
        return;
    }
    v.sort_by(f64::total_cmp);
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    println!(
        "| {nom} | {:.1} | {:.1} | {:.1} | {:.1} {unite} |",
        q(0.5),
        q(0.9),
        q(0.99),
        q(1.0)
    );
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let support = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/fm.rustymusic.desktop");
    let base = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| support.join("rusty-music.db"));
    let dossier = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("extraits-aura"));
    let combien: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(500);
    std::fs::create_dir_all(&dossier)?;

    let lib = Library::open(&base)?;
    let mut stmt = lib.conn.prepare(
        "SELECT t.id, t.path, t.duration_ms FROM tracks t
           JOIN features f ON f.track_id = t.id AND f.model = 'clap-htsat-unfused-5f'
          WHERE t.duration_ms IS NOT NULL ORDER BY t.id",
    )?;
    let tous: Vec<(i64, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut alea = Alea::depuis(GRAINE);
    let mut pris = std::collections::BTreeSet::new();
    while pris.len() < combien.min(tous.len()) {
        pris.insert(alea.borne(tous.len()));
    }
    let pistes: Vec<&(i64, String, i64)> = pris.iter().map(|&i| &tous[i]).collect();

    let mel = Mel::new();
    let mut enc = Embedder::charger(None, 1)?;

    let mut froid_premier = Vec::new();
    let mut froid_entier = Vec::new();
    let mut fenetre_t = Vec::new();
    let mut extraction = Vec::new();
    let mut encodage = Vec::new();
    let mut tailles = Vec::new();
    let mut cache_entier = Vec::new();
    let mut chaud_premier = Vec::new();
    let mut chaud_entier = Vec::new();
    let mut sans_saut_premier = Vec::new();
    let mut rangs = [0usize; FENETRES];
    let mut ecart_au_centre = Vec::new();
    let mut par_format: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    let mut echecs = 0usize;

    for (k, (id, chemin, duree_ms)) in pistes.iter().enumerate() {
        eprint!("\r  {k}/{}", pistes.len());
        let chemin = Path::new(chemin);
        let duree = *duree_ms as f64 / 1000.0;
        let ext = chemin
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        // À froid : rien n'a encore lu ce fichier dans ce banc.
        let centre = Duration::from_secs_f64(debut_extrait(duree, FENETRES, FENETRES / 2));
        match a_la_volee(chemin, centre, true) {
            Ok((p, e, _)) => {
                froid_premier.push(p);
                froid_entier.push(e);
                par_format.entry(ext.clone()).or_default().push(p);
            }
            Err(e) => {
                eprintln!("\n{id} : {e}");
                echecs += 1;
                continue;
            }
        }

        // La fenêtre représentative, comme la passe d'empreintes la verrait.
        let t = Instant::now();
        let rang = match fenetres(chemin, FENETRES)
            .map_err(anyhow::Error::from)
            .and_then(|b| {
                let spec: Vec<f32> = b.iter().flat_map(|x| mel.spectrogramme(x)).collect();
                Ok(enc.empreintes(&spec, b.len())?)
            }) {
            Ok(v) => fenetre_representative(&v).unwrap_or(FENETRES / 2),
            Err(e) => {
                eprintln!("\n{id} : {e}");
                echecs += 1;
                continue;
            }
        };
        fenetre_t.push(t.elapsed().as_secs_f64() * 1e3);
        rangs[rang] += 1;
        let debut = Duration::from_secs_f64(debut_extrait(duree, FENETRES, rang));
        ecart_au_centre.push((debut.as_secs_f64() - centre.as_secs_f64()).abs());

        // Extraction pour le cache : décoder 15 s avec saut, encoder.
        let t = Instant::now();
        let pcm =
            rusty_music_player::extrait_mono(chemin, debut, Duration::from_secs_f64(EXTRAIT_S))?;
        if pcm.len() < SR_EXTRAIT as usize {
            eprintln!("\n{id} : extrait vide après le saut");
            echecs += 1;
            continue;
        }
        let t_enc = Instant::now();
        let opus = encoder(&pcm)?;
        encodage.push(t_enc.elapsed().as_secs_f64() * 1e3);
        extraction.push(t.elapsed().as_secs_f64() * 1e3);
        tailles.push(opus.len() as f64 / 1024.0);
        let fichier = dossier.join(format!("{id}.opus"));
        std::fs::write(&fichier, &opus)?;

        // (a) En cache : lire le fichier et le décoder entier.
        let t = Instant::now();
        let piste = rusty_music_core::opus::decoder(&fichier)?;
        cache_entier.push(t.elapsed().as_secs_f64() * 1e3);
        anyhow::ensure!(!piste.echantillons.is_empty(), "{id} : extrait illisible");

        // (b) À la volée, à chaud, avec puis sans saut.
        let (p, e, _) = a_la_volee(chemin, debut, true)?;
        chaud_premier.push(p);
        chaud_entier.push(e);
        let (p, _, _) = a_la_volee(chemin, debut, false)?;
        sans_saut_premier.push(p);
    }
    eprintln!();

    let n = froid_premier.len();
    println!("## Extraits de l'aura, {n} morceaux ({echecs} échecs)\n");
    println!("| mesure | médiane | p90 | p99 | max |\n|---|---:|---:|---:|---:|");
    quantiles(
        "à la volée, à froid : 1ᵉʳ échantillon",
        "ms",
        &mut froid_premier,
    );
    quantiles(
        "à la volée, à froid : 15 s décodées",
        "ms",
        &mut froid_entier,
    );
    quantiles(
        "à la volée, à chaud : 1ᵉʳ échantillon",
        "ms",
        &mut chaud_premier,
    );
    quantiles(
        "à la volée, à chaud : 15 s décodées",
        "ms",
        &mut chaud_entier,
    );
    quantiles(
        "sans saut (`skip_duration`), à chaud : 1ᵉʳ échantillon",
        "ms",
        &mut sans_saut_premier,
    );
    quantiles(
        "en cache : lire et décoder 15 s d'Opus",
        "ms",
        &mut cache_entier,
    );
    quantiles("extraction : décoder 15 s + encoder", "ms", &mut extraction);
    quantiles("dont l'encodage Opus", "ms", &mut encodage);
    quantiles("taille d'un extrait Opus 48 kbit/s", "Kio", &mut tailles);
    quantiles(
        "fenêtre représentative (5 fenêtres + CLAP)",
        "ms",
        &mut fenetre_t,
    );
    quantiles(
        "écart au centre de l'extrait choisi",
        "s",
        &mut ecart_au_centre,
    );

    let moy = tailles.iter().sum::<f64>() / tailles.len().max(1) as f64;
    let ext_moy = extraction.iter().sum::<f64>() / extraction.len().max(1) as f64;
    println!(
        "\nRang de la fenêtre représentative (0 = 15 %, 4 = 85 %) : {rangs:?}\n\
         Cache pour 27 845 morceaux : {:.2} Go ; extraction séquentielle : {:.1} h.",
        moy * 27_845.0 / 1024.0 / 1024.0,
        ext_moy * 27_845.0 / 3.6e6
    );
    println!("\nÀ froid, premier échantillon par format :\n\n| format | n | médiane | p90 |\n|---|---:|---:|---:|");
    for (f, v) in &mut par_format {
        v.sort_by(f64::total_cmp);
        let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        println!("| {f} | {} | {:.1} | {:.1} |", v.len(), q(0.5), q(0.9));
    }
    Ok(())
}
