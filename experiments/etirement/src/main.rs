// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc d'écoute de l'étirement (E3).
//!
//! Trois méthodes, les mêmes réglages :
//!
//! - **wsola** : ce que fait l'application aujourd'hui. L'étirement est
//!   `etirement::etirer`, la transposition `etirement::transposer` (étirer puis
//!   rééchantillonner en Catmull-Rom). Le module est **inclus tel quel** depuis
//!   `crates/editor/src/etirement.rs` : on mesure le code de l'application, pas
//!   une copie qui pourrait dériver.
//! - **Signalsmith Stretch** (`signalsmith-stretch`, MIT) : vocodeur de phase,
//!   temps et hauteur ensemble, préréglage par défaut.
//! - **Rubber Band R3** (`rubberband`, GPL) : moteur « Finer », hors ligne pour
//!   les rendus, temps réel pour le coût par bloc.
//!
//! ```bash
//! # Coûts (aucun fichier écrit) : salve par bloc de 128 trames, transposition
//! # d'un stem entier, et contrôle de hauteur sur une sinusoïde.
//! cargo run --release -- mesurer "<stem de 184 s>.wav"
//!
//! # Rendus pour l'écoute à l'aveugle. La liste est un TSV
//! # « nom<TAB>chemin[<TAB>début en s] » ; la clé s'écrit à part.
//! cargo run --release -- ecoute <dossier d'écoute> <fichier clé> <liste.tsv>
//! ```

#[path = "../../../crates/editor/src/etirement.rs"]
#[allow(dead_code)]
mod etirement;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rubberband::{Options as RbOptions, Stretcher};
use signalsmith_stretch::Stretch;

const SR: u32 = 44_100;
const CANAUX: usize = 2;
/// Le bloc retenu par l'application pour le rappel audio (journal, 19 août).
const BLOC: usize = 128;
/// Comme `cout_bloc` : quatre stems tournent ensemble dans l'établi.
const STEMS: f64 = 4.0;
/// Tampon CoreAudio courant sur ce Mac : l'échéance à tenir.
const ECHEANCE_MS: f64 = 512.0 / SR as f64 * 1000.0;
const DUREE_EXTRAIT_S: f64 = 20.0;

/// Les réglages du banc : facteur de vitesse (tempo) ou demi-tons.
#[derive(Clone, Copy)]
enum Reglage {
    Vitesse(f32),
    Hauteur(f32),
}

impl Reglage {
    fn tous() -> [Reglage; 5] {
        [
            Reglage::Vitesse(0.5),
            Reglage::Vitesse(0.7),
            Reglage::Vitesse(0.8),
            Reglage::Hauteur(2.0),
            Reglage::Hauteur(-2.0),
        ]
    }
    fn nom(self) -> String {
        match self {
            Reglage::Vitesse(v) => format!("vitesse-{:02}", (v * 100.0).round() as i32),
            Reglage::Hauteur(d) if d > 0.0 => format!("hauteur-plus{}", d as i32),
            Reglage::Hauteur(d) => format!("hauteur-moins{}", -d as i32),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Methode {
    Wsola,
    Signalsmith,
    RubberBand,
}

impl Methode {
    const TOUTES: [Methode; 3] = [Methode::Wsola, Methode::Signalsmith, Methode::RubberBand];
    fn nom(self) -> &'static str {
        match self {
            Methode::Wsola => "wsola",
            Methode::Signalsmith => "signalsmith",
            Methode::RubberBand => "rubberband-r3",
        }
    }
}

// ---------------------------------------------------------------------------
// Rendu hors ligne d'un signal entrelacé stéréo.

fn rendre(m: Methode, x: &[f32], r: Reglage) -> Vec<f32> {
    match m {
        Methode::Wsola => match r {
            Reglage::Vitesse(v) => etirement::etirer(x, CANAUX, 1.0 / v),
            Reglage::Hauteur(d) => etirement::transposer(x, CANAUX, d),
        },
        Methode::Signalsmith => {
            let (tempo, demi_tons) = match r {
                Reglage::Vitesse(v) => (v, 0.0),
                Reglage::Hauteur(d) => (1.0, d),
            };
            let mut s = Stretch::preset_default(CANAUX as u32, SR);
            if demi_tons != 0.0 {
                s.set_transpose_factor_semitones(demi_tons, None);
            }
            let trames = x.len() / CANAUX;
            let sortie = (trames as f64 / tempo as f64).round() as usize;
            let mut out = vec![0.0f32; sortie * CANAUX];
            assert!(s.exact(x, &mut out), "signalsmith : signal trop court");
            out
        }
        Methode::RubberBand => {
            let (ratio, pitch) = match r {
                Reglage::Vitesse(v) => (1.0 / v as f64, 1.0),
                Reglage::Hauteur(d) => (1.0, 2f64.powf(d as f64 / 12.0)),
            };
            rubberband_hors_ligne(x, ratio, pitch)
        }
    }
}

fn desentrelacer(x: &[f32]) -> [Vec<f32>; 2] {
    [
        x.iter().step_by(2).copied().collect(),
        x.iter().skip(1).step_by(2).copied().collect(),
    ]
}

fn rubberband_hors_ligne(x: &[f32], ratio: f64, pitch: f64) -> Vec<f32> {
    let [g, d] = desentrelacer(x);
    let n = g.len();
    let mut s = Stretcher::new(
        SR,
        CANAUX as u32,
        RbOptions::PROCESS_OFFLINE | RbOptions::ENGINE_FINER | RbOptions::THREADING_NEVER,
        ratio,
        pitch,
    );
    s.set_expected_input_duraction(n as u32);
    const PAS: usize = 8192;
    s.set_max_process_size(PAS as u32);
    let mut i = 0;
    while i < n {
        let f = (i + PAS).min(n);
        s.study(&[&g[i..f], &d[i..f]], f == n);
        i = f;
    }
    let mut out = Vec::with_capacity((n as f64 * ratio) as usize * CANAUX + 8192);
    let mut bg = vec![0.0f32; PAS * 4];
    let mut bd = vec![0.0f32; PAS * 4];
    let mut vider = |s: &mut Stretcher, out: &mut Vec<f32>| {
        while let Some(dispo) = s.available() {
            if dispo == 0 {
                break;
            }
            let k = (dispo as usize).min(bg.len());
            let lu = s.retrieve(&mut [&mut bg[..k], &mut bd[..k]]) as usize;
            for j in 0..lu {
                out.push(bg[j]);
                out.push(bd[j]);
            }
        }
    };
    let mut i = 0;
    while i < n {
        let f = (i + PAS).min(n);
        s.process(&[&g[i..f], &d[i..f]], f == n);
        vider(&mut s, &mut out);
        i = f;
    }
    // Hors ligne, la fin arrive après le dernier `process` : on vide jusqu'à
    // ce que l'étireur se déclare fini (`available` rend None).
    while s.available().is_some() {
        vider(&mut s, &mut out);
        if s.available() == Some(0) {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Coût par bloc : la pire salve, comme `crates/player/examples/cout_bloc.rs`.

/// Un étireur en flux, tel que le rappel audio s'en servirait.
trait Flux {
    /// Pousse un bloc d'entrée et rend le nombre de trames sorties.
    fn salve(&mut self, bloc: &[f32]) -> usize;
}

struct FluxWsola(wsola::TimeStretch);
impl Flux for FluxWsola {
    fn salve(&mut self, bloc: &[f32]) -> usize {
        self.0.push(bloc);
        self.0.pull(usize::MAX).len() / CANAUX
    }
}

struct FluxSignalsmith {
    s: Stretch,
    tempo: f64,
    reste: f64,
    out: Vec<f32>,
}
impl Flux for FluxSignalsmith {
    fn salve(&mut self, bloc: &[f32]) -> usize {
        // La sortie d'un appel doit être un nombre entier de trames : on
        // reporte la fraction d'un appel au suivant.
        let voulu = bloc.len() as f64 / CANAUX as f64 / self.tempo + self.reste;
        let k = voulu.floor() as usize;
        self.reste = voulu - k as f64;
        self.s.process(bloc, &mut self.out[..k * CANAUX]);
        k
    }
}

struct FluxRubberBand {
    s: Stretcher,
    g: Vec<f32>,
    d: Vec<f32>,
    og: Vec<f32>,
    od: Vec<f32>,
}
impl Flux for FluxRubberBand {
    fn salve(&mut self, bloc: &[f32]) -> usize {
        let n = bloc.len() / CANAUX;
        for (j, c) in bloc.chunks_exact(2).enumerate() {
            self.g[j] = c[0];
            self.d[j] = c[1];
        }
        self.s.process(&[&self.g[..n], &self.d[..n]], false);
        let mut rendu = 0;
        while let Some(k) = self.s.available() {
            if k == 0 {
                break;
            }
            let k = (k as usize).min(self.og.len());
            rendu += self.s.retrieve(&mut [&mut self.og[..k], &mut self.od[..k]]) as usize;
        }
        rendu
    }
}

fn flux(m: Methode, tempo: f32, demi_tons: f32) -> Box<dyn Flux> {
    match m {
        Methode::Wsola => {
            assert!(demi_tons == 0.0, "wsola ne transpose pas en flux");
            let mut e = wsola::TimeStretch::new(SR, CANAUX as u16).expect("étireur");
            e.set_tempo(tempo);
            Box::new(FluxWsola(e))
        }
        Methode::Signalsmith => {
            let mut s = Stretch::preset_default(CANAUX as u32, SR);
            if demi_tons != 0.0 {
                s.set_transpose_factor_semitones(demi_tons, None);
            }
            Box::new(FluxSignalsmith {
                s,
                tempo: tempo as f64,
                reste: 0.0,
                out: vec![0.0; BLOC * 8 * CANAUX],
            })
        }
        Methode::RubberBand => {
            let mut s = Stretcher::new(
                SR,
                CANAUX as u32,
                RbOptions::PROCESS_REALTIME
                    | RbOptions::ENGINE_FINER
                    | RbOptions::THREADING_NEVER
                    | RbOptions::PITCH_HIGH_CONSISTENCY,
                1.0 / tempo as f64,
                2f64.powf(demi_tons as f64 / 12.0),
            );
            s.set_max_process_size(BLOC as u32);
            Box::new(FluxRubberBand {
                s,
                g: vec![0.0; BLOC],
                d: vec![0.0; BLOC],
                og: vec![0.0; 1 << 16],
                od: vec![0.0; 1 << 16],
            })
        }
    }
}

fn bruit(secondes: usize) -> Vec<f32> {
    let mut graine = 0x2545_F491_4F6C_DD1Du64;
    (0..SR as usize * secondes * CANAUX)
        .map(|_| {
            graine ^= graine >> 12;
            graine ^= graine << 25;
            graine ^= graine >> 27;
            (graine.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 8_388_608.0 - 1.0
        })
        .collect()
}

/// Ce que rend la mesure d'un réglage, en ms pour quatre stems.
struct Salves {
    /// Le plus petit des maxima de [`PASSES`] passes : une préemption du
    /// système tombe au hasard dans une passe, pas dans toutes, alors qu'un pic
    /// propre à l'étireur revient à chaque passe.
    pire: f64,
    /// Le plus grand des maxima : ce qu'a vu la passe la plus malchanceuse.
    pire_brut: f64,
    p99: f64,
    mediane: f64,
    /// Audio rendu par la pire salve retenue, en ms.
    audio: f64,
}

const PASSES: usize = 5;

fn pire_salve(m: Methode, tempo: f32, demi_tons: f32, matiere: &[f32]) -> Salves {
    let mut e = flux(m, tempo, demi_tons);
    let n = BLOC * CANAUX;
    let mut i = 0;
    let suivant = |i: &mut usize| {
        if *i + n >= matiere.len() {
            *i = 0;
        }
        let b = &matiere[*i..*i + n];
        *i += n;
        b
    };
    // Chauffe : remplir les tampons internes (latence de Rubber Band et de
    // Signalsmith comprise, plusieurs milliers de trames).
    for _ in 0..400 {
        e.salve(suivant(&mut i));
    }
    let mut durees = Vec::with_capacity(2000 * PASSES);
    let mut maxima = Vec::with_capacity(PASSES);
    for _ in 0..PASSES {
        let (mut pire, mut rendu_pire) = (0.0f64, 0usize);
        for _ in 0..2000 {
            let t = Instant::now();
            let mut rendu = 0;
            // Une salve, c'est ce que fait un `next()` : pousser jusqu'à ce que
            // l'étireur rende quelque chose.
            let mut garde = 0;
            while rendu == 0 && garde < 64 {
                rendu = e.salve(suivant(&mut i));
                garde += 1;
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            durees.push(ms);
            if ms > pire {
                pire = ms;
                rendu_pire = rendu;
            }
        }
        maxima.push((pire, rendu_pire));
    }
    durees.sort_by(|a, b| a.partial_cmp(b).unwrap());
    maxima.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let (pire, rendu) = maxima[0];
    Salves {
        pire: pire * STEMS,
        pire_brut: maxima[PASSES - 1].0 * STEMS,
        p99: durees[durees.len() * 99 / 100] * STEMS,
        mediane: durees[durees.len() / 2] * STEMS,
        audio: rendu as f64 / SR as f64 * 1000.0,
    }
}

// ---------------------------------------------------------------------------
// Contrôle de hauteur : une sinusoïde de 441 Hz, compter ses passages par zéro.

fn sinus(hz: f32, secondes: f32) -> Vec<f32> {
    let n = (SR as f32 * secondes) as usize;
    (0..n)
        .flat_map(|i| {
            let v = 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / SR as f32).sin();
            [v, v]
        })
        .collect()
}

fn frequence(x: &[f32]) -> f64 {
    let g: Vec<f32> = x.iter().step_by(2).copied().collect();
    // Le milieu seulement : les bords portent les fondus des étireurs.
    let a = g.len() / 4;
    let b = 3 * g.len() / 4;
    let mut passages = 0usize;
    let (mut premier, mut dernier) = (None, 0usize);
    for i in a + 1..b {
        if g[i - 1] <= 0.0 && g[i] > 0.0 {
            passages += 1;
            premier.get_or_insert(i);
            dernier = i;
        }
    }
    match premier {
        Some(p) if passages > 1 => (passages - 1) as f64 * SR as f64 / (dernier - p) as f64,
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Décodage, écriture.

fn decoder(chemin: &Path) -> Vec<f32> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let f = fs::File::open(chemin).unwrap_or_else(|e| panic!("{}: {e}", chemin.display()));
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = chemin.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let sonde = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .expect("format");
    let mut format = sonde.format;
    let piste = format.default_track().expect("piste").clone();
    let sr = piste.codec_params.sample_rate.unwrap_or(SR);
    assert_eq!(
        sr,
        SR,
        "{} : {sr} Hz, le banc attend 44,1 kHz",
        chemin.display()
    );
    let mut dec = symphonia::default::get_codecs()
        .make(&piste.codec_params, &DecoderOptions::default())
        .expect("décodeur");
    let mut out = Vec::new();
    while let Ok(paquet) = format.next_packet() {
        if paquet.track_id() != piste.id {
            continue;
        }
        let Ok(buf) = dec.decode(&paquet) else {
            continue;
        };
        let spec = *buf.spec();
        let canaux = spec.channels.count();
        let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
        sb.copy_interleaved_ref(buf);
        let s = sb.samples();
        if canaux == 1 {
            out.extend(s.iter().flat_map(|v| [*v, *v]));
        } else {
            out.extend(s.chunks_exact(canaux).flat_map(|c| [c[0], c[1]]));
        }
    }
    out
}

fn ecrire(chemin: &Path, x: &[f32]) {
    let spec = hound::WavSpec {
        channels: CANAUX as u16,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(chemin, spec).expect("wav");
    for v in x {
        w.write_sample((v.clamp(-1.0, 1.0) * 32767.0).round() as i16)
            .expect("écriture");
    }
    w.finalize().expect("wav");
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// La fenêtre de 20 s la plus pleine entre 15 % et 85 % du morceau : la plus
/// forte en énergie, et sans trou (on prend le minimum sur des tranches de 2 s,
/// pour qu'un stem de batterie à moitié muet ne gagne pas sur un pic isolé).
fn choisir_debut(x: &[f32]) -> f64 {
    let trames = x.len() / CANAUX;
    let duree = trames as f64 / SR as f64;
    let (a, b) = (
        duree * 0.15,
        (duree * 0.85 - DUREE_EXTRAIT_S).max(duree * 0.15),
    );
    let mut meilleur = (a, -1.0f64);
    let mut t = a;
    while t <= b {
        let i = (t * SR as f64) as usize * CANAUX;
        let n = (DUREE_EXTRAIT_S * SR as f64) as usize * CANAUX;
        if i + n > x.len() {
            break;
        }
        let f = &x[i..i + n];
        let tranche = 2 * SR as usize * CANAUX;
        let plancher = f.chunks(tranche).map(rms).fold(f64::INFINITY, f64::min);
        if plancher > meilleur.1 {
            meilleur = (t, plancher);
        }
        t += 1.0;
    }
    meilleur.0
}

fn fondus(x: &mut [f32]) {
    let n = (0.02 * SR as f64) as usize;
    let trames = x.len() / CANAUX;
    for i in 0..n.min(trames / 2) {
        let g = i as f32 / n as f32;
        for c in 0..CANAUX {
            x[i * CANAUX + c] *= g;
            x[(trames - 1 - i) * CANAUX + c] *= g;
        }
    }
}

/// Même niveau efficace que l'extrait d'origine : une version plus forte
/// paraît meilleure, et l'écoute doit juger l'étirement, pas le volume.
fn egaliser(x: &mut [f32], cible: f64) {
    let r = rms(x);
    if r > 0.0 {
        let mut g = (cible / r) as f32;
        let crete = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if crete * g > 0.98 {
            g = 0.98 / crete;
        }
        x.iter_mut().for_each(|v| *v *= g);
    }
}

/// Permutation déterministe des trois méthodes, différente par fichier.
fn permutation(graine: u64) -> [Methode; 3] {
    let mut h = graine.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    const P: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let p = P[(h % 6) as usize];
    [
        Methode::TOUTES[p[0]],
        Methode::TOUTES[p[1]],
        Methode::TOUTES[p[2]],
    ]
}

// ---------------------------------------------------------------------------

fn mesurer(stem: &Path) {
    println!("# Contrôle de hauteur — sinusoïde de 441 Hz, 6 s\n");
    println!("| méthode | réglage | attendu (Hz) | mesuré (Hz) | durée attendue | durée rendue |");
    println!("|---|---|---|---|---|---|");
    let s = sinus(441.0, 6.0);
    for m in Methode::TOUTES {
        for r in Reglage::tous() {
            let out = rendre(m, &s, r);
            let (hz, duree) = match r {
                Reglage::Vitesse(v) => (441.0, 6.0 / v as f64),
                Reglage::Hauteur(d) => (441.0 * 2f64.powf(d as f64 / 12.0), 6.0),
            };
            println!(
                "| {} | {} | {hz:.1} | {:.1} | {duree:.2} s | {:.2} s |",
                m.nom(),
                r.nom(),
                frequence(&out),
                out.len() as f64 / CANAUX as f64 / SR as f64
            );
        }
    }

    println!(
        "\n# Pire salve par bloc de {BLOC} trames, quatre stems \
         (échéance du tampon : {ECHEANCE_MS:.1} ms)\n"
    );
    println!(
        "| méthode | tempo | demi-tons | pire salve (min. de {PASSES} passes) | pire brute | 99e centile | médiane | audio rendu par la pire | verdict |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    let matiere = bruit(30);
    let cas: [(f32, f32); 6] = [
        (0.25, 0.0),
        (0.5, 0.0),
        (0.7, 0.0),
        (0.8, 0.0),
        (1.0, 2.0),
        (1.0, -2.0),
    ];
    for m in Methode::TOUTES {
        for (tempo, dt) in cas {
            if m == Methode::Wsola && dt != 0.0 {
                println!(
                    "| {} | {tempo} | {dt:+} | — | — | — | — | — | pas de transposition en flux |",
                    m.nom()
                );
                continue;
            }
            let r = pire_salve(m, tempo, dt, &matiere);
            println!(
                "| {} | {tempo} | {dt:+} | {:.2} ms | {:.2} ms | {:.2} ms | {:.3} ms | {:.1} ms | {} |",
                m.nom(),
                r.pire,
                r.pire_brut,
                r.p99,
                r.mediane,
                r.audio,
                if r.pire < ECHEANCE_MS { "tient" } else { "CRAQUE" }
            );
        }
    }

    // Le journal du 19 août mesurait un stem de 184 s ; les stems présents
    // durent 191 s et plus : on coupe à 184 s pour comparer à durée égale.
    let mut x = decoder(stem);
    x.truncate((184 * SR as usize * CANAUX).min(x.len()));
    let duree = x.len() as f64 / CANAUX as f64 / SR as f64;
    println!(
        "\n# Transposition d'un stem entier — {} ({duree:.0} s), un fil\n",
        stem.file_name().unwrap().to_string_lossy()
    );
    println!("| méthode | +2 demi-tons | −2 demi-tons | durée rendue |");
    println!("|---|---|---|---|");
    for m in Methode::TOUTES {
        let mut cols = Vec::new();
        let mut rendue = 0.0;
        for d in [2.0f32, -2.0] {
            let t = Instant::now();
            let out = rendre(m, &x, Reglage::Hauteur(d));
            cols.push(t.elapsed().as_secs_f64());
            rendue = out.len() as f64 / CANAUX as f64 / SR as f64;
        }
        println!(
            "| {} | {:.2} s | {:.2} s | {rendue:.1} s |",
            m.nom(),
            cols[0],
            cols[1]
        );
    }
}

fn ecoute(dossier: &Path, cle: &Path, liste: &Path) {
    assert!(
        !cle.starts_with(dossier),
        "la clé doit vivre hors du dossier d'écoute"
    );
    fs::create_dir_all(dossier).expect("dossier d'écoute");
    let mut lignes_cle = vec!["extrait\treglage\tfichier\tmethode".to_string()];
    let mut sources = vec!["extrait\tsource\tdebut_s".to_string()];
    let texte = fs::read_to_string(liste).expect("liste");
    for (k, ligne) in texte
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .enumerate()
    {
        let champs: Vec<&str> = ligne.split('\t').collect();
        let (nom, chemin) = (champs[0], PathBuf::from(champs[1]));
        let x = decoder(&chemin);
        let debut = champs
            .get(2)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or_else(|| choisir_debut(&x));
        let i = (debut * SR as f64) as usize * CANAUX;
        let n = (DUREE_EXTRAIT_S * SR as f64) as usize * CANAUX;
        let mut extrait = x[i..(i + n).min(x.len())].to_vec();
        fondus(&mut extrait);
        let rep = dossier.join(format!("{}-{nom}", k + 1));
        fs::create_dir_all(&rep).unwrap();
        sources.push(format!("{nom}\t{}\t{debut:.1}", chemin.display()));
        eprintln!("{nom} : {} à {debut:.0} s", chemin.display());

        let mut rendus: Vec<(String, Vec<f32>)> = Vec::new();
        for (j, r) in Reglage::tous().into_iter().enumerate() {
            let ordre = permutation((k * 16 + j) as u64 + 0xE3);
            for (lettre, m) in ["A", "B", "C"].into_iter().zip(ordre) {
                let mut out = rendre(m, &extrait, r);
                // Même longueur pour les trois : une taille de fichier qui
                // diffère de quelques octets trahirait la méthode.
                let tempo = match r {
                    Reglage::Vitesse(v) => v as f64,
                    Reglage::Hauteur(_) => 1.0,
                };
                let voulu = (extrait.len() / CANAUX) as f64 / tempo;
                out.resize(voulu.round() as usize * CANAUX, 0.0);
                fondus(&mut out);
                let fichier = format!("{}-{lettre}.wav", r.nom());
                lignes_cle.push(format!(
                    "{}-{nom}\t{}\t{fichier}\t{}",
                    k + 1,
                    r.nom(),
                    m.nom()
                ));
                rendus.push((fichier, out));
            }
        }

        // **Un seul niveau pour tout l'extrait.** Les vocodeurs de phase
        // montent le facteur de crête : réglés chacun au niveau de l'original
        // sous un plafond de crête, ils sortaient jusqu'à 2 dB plus bas que
        // wsola — et à l'écoute, plus fort paraît meilleur. On prend donc le
        // plus haut niveau efficace que **tous** les fichiers tiennent sans
        // dépasser le plafond, original compris.
        rendus.push(("0-original.wav".into(), extrait));
        let cible = rendus.iter().fold(f64::INFINITY, |acc, (_, x)| {
            let crete = x.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
            acc.min(if crete > 0.0 {
                0.98 * rms(x) / crete
            } else {
                acc
            })
        });
        let cible = cible.min(rendus.last().map(|(_, x)| rms(x)).unwrap_or(cible));
        for (fichier, mut x) in rendus {
            egaliser(&mut x, cible);
            ecrire(&rep.join(&fichier), &x);
        }
    }
    fs::write(cle, lignes_cle.join("\n") + "\n").expect("clé");
    fs::write(cle.with_file_name("sources.tsv"), sources.join("\n") + "\n").expect("sources");
    eprintln!("clé : {}", cle.display());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("mesurer") => mesurer(Path::new(&args[2])),
        Some("ecoute") => ecoute(
            Path::new(&args[2]),
            Path::new(&args[3]),
            Path::new(&args[4]),
        ),
        _ => eprintln!(
            "usage : banc-etirement mesurer <stem.wav> | ecoute <dossier> <clé> <liste.tsv>"
        ),
    }
}
