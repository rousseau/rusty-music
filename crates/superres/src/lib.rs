//! Super-résolution audio hors ligne — « régénérer en HD ».
//!
//! Un fichier compressé a perdu le haut de son spectre. AERO (modèle appris,
//! `slp-rl/aero`, lignée HDemucs) le reconstruit : on ramène le signal à
//! 11 025 Hz, le modèle rend du 44 100 Hz plausible. Rendu **hors ligne** vers
//! un cache — ce n'est pas le bouton « E » (excitation temps réel), c'est un
//! « régénérer » qu'on lance et qu'on laisse tourner.
//!
//! Voie validée dans `experiments/burn-aero/` : STFT en Rust (`stft`), réseau
//! seul exécuté par ONNX Runtime (`ort` — `tract` calcule faux), iSTFT en
//! Rust. Un segment isolé reproduit PyTorch au `f32` près ; le pipeline
//! complet (segments de 5 s, recouvrement, fondu) s'en écarte de ~1 % — la
//! part de la segmentation, pas un défaut. ~5× le temps réel sur CPU par
//! canal. Mesures : `cargo run -p rusty-music-superres --example verifier`.

use std::path::Path;

use rodio::Source;
use tracing::debug;

pub mod stft;

/// Fréquences fixées par le modèle musique (`aero-11025-44100.onnx`).
pub const LR_SR: u32 = 11_025;
pub const HR_SR: u32 = 44_100;
const ECHELLE: usize = 4; // HR_SR / LR_SR

/// Échantillons basse résolution par segment — `T = 862` trames, forme d'entrée
/// figée du modèle. `(862 - 1) * HOP_IN`.
pub const SEG_LR: usize = (862 - 1) * stft::HOP_IN; // 55 104
/// Échantillons haute résolution rendus par segment. `HOP_OUT * (T - 1)`.
pub const SEG_HR: usize = stft::HOP_OUT * (862 - 1); // 220 416
/// Recouvrement entre segments (~1,25 s) : le modèle a des effets de bord
/// (repli STFT, fenêtrage LSTM), un fondu-enchaîné les masque.
const CHEV_LR: usize = SEG_LR / 4;
const PAS_LR: usize = SEG_LR - CHEV_LR;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ouverture de {0} impossible : {1}")]
    Ouverture(std::path::PathBuf, #[source] std::io::Error),
    #[error("décodage impossible : {0}")]
    Decode(String),
    #[error("modèle ONNX : {0}")]
    Onnx(#[from] ort::Error),
    #[error("rééchantillonnage : {0}")]
    Reechantillonnage(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Le générateur AERO chargé une fois, réutilisé pour tous les segments et
/// toutes les pistes.
pub struct Modele {
    session: ort::session::Session,
}

impl Modele {
    pub fn charger(chemin_onnx: &Path) -> Result<Self> {
        let session = ort::session::Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)?
            .commit_from_file(chemin_onnx)?;
        Ok(Self { session })
    }

    /// Un segment : spectrogramme complexe `[2, 256, 862]` → même forme.
    fn segment(&mut self, spec: &[f32]) -> Result<Vec<f32>> {
        let arr = ndarray::Array::from_shape_vec((1, 2, stft::BINS, 862), spec.to_vec())
            .expect("forme du segment");
        let val = ort::value::Tensor::from_array(arr)?;
        let sortie = self.session.run(ort::inputs!["spec" => val.view()])?;
        let (_, donnees) = sortie["spec_hr"].try_extract_tensor::<f32>()?;
        Ok(donnees.to_vec())
    }
}

/// Régénère `entree` en haute résolution et écrit un WAV 44,1 kHz dans
/// `sortie`. `progres(faits, total)` est appelé après chaque segment.
///
/// **Le spectre de la source est conservé sous sa coupure** ; le modèle ne
/// fournit que ce qui est au-dessus. Un MP3 128 qui monte à 16 kHz garde donc
/// ses 16 kHz réels, le modèle n'ajoute que 16–22 kHz — le HD ne peut pas
/// rendre le son plus terne que l'original. Repartir d'un 11 kHz *pour tout le
/// spectre* étouffait les aigus réels ; c'est corrigé.
///
/// Rend la **coupure estimée de la source** : au-dessus de ~16 kHz, le modèle
/// n'apporte presque rien — à l'appelant d'en avertir.
pub fn regenerer(
    entree: &Path,
    sortie: &Path,
    modele: &mut Modele,
    progres: impl Fn(usize, usize),
) -> Result<f32> {
    let (canaux_src, sr_src, coupure) = decoder(entree)?;

    // 1. modèle : chaque canal ramené à 11 kHz, régénéré à 44,1 kHz.
    let canaux_lr: Vec<Vec<f32>> = if sr_src == LR_SR {
        canaux_src.clone()
    } else {
        canaux_src
            .iter()
            .map(|p| reechantillonner(p, sr_src, LR_SR))
            .collect::<Result<_>>()?
    };
    let mut canaux_hr = modele_sur_canaux(&canaux_lr, modele, &progres)?;

    // 2. source à 44,1 kHz pour le mélange.
    let source_44: Vec<Vec<f32>> = if sr_src == HR_SR {
        canaux_src
    } else {
        canaux_src
            .iter()
            .map(|p| reechantillonner(p, sr_src, HR_SR))
            .collect::<Result<_>>()?
    };

    // 3. mélange : source sous la coupure, modèle au-dessus.
    melanger_hf(&source_44, &mut canaux_hr, coupure);

    normaliser_et_ecrire(sortie, canaux_hr)?;
    Ok(coupure)
}

/// Comme [`regenerer`] mais à partir de canaux déjà à [`LR_SR`], **sans
/// mélange avec la source** : la sortie brute du modèle. Sert aux tests de
/// parité, où la référence est `model()` seul.
pub fn regenerer_depuis_lr(
    canaux_lr: &[Vec<f32>],
    sortie: &Path,
    modele: &mut Modele,
    progres: impl Fn(usize, usize),
) -> Result<()> {
    let canaux_hr = modele_sur_canaux(canaux_lr, modele, &progres)?;
    normaliser_et_ecrire(sortie, canaux_hr)
}

/// Passe chaque canal à 11 kHz dans le modèle par segments de 5 s avec
/// recouvrement, addition-recouvrement à 44,1 kHz. Rend les canaux HR
/// normalisés par le poids du fondu (mais pas encore limités en crête).
fn modele_sur_canaux(
    canaux_lr: &[Vec<f32>],
    modele: &mut Modele,
    progres: &impl Fn(usize, usize),
) -> Result<Vec<Vec<f32>>> {
    let n_canaux = canaux_lr.len();
    let n_lr = canaux_lr[0].len();
    let n_segments = if n_lr <= SEG_LR {
        1
    } else {
        (n_lr - CHEV_LR).div_ceil(PAS_LR)
    };
    let total = n_segments * n_canaux;
    debug!(n_lr, n_segments, n_canaux, "régénération");

    let n_hr = n_lr * ECHELLE;
    let mut canaux_hr = vec![vec![0.0f32; n_hr]; n_canaux];
    let mut poids = vec![0.0f32; n_hr]; // fenêtre identique pour tous les canaux

    let mut faits = 0;
    for (c, canal) in canaux_lr.iter().enumerate() {
        for s in 0..n_segments {
            let deb_lr = s * PAS_LR;
            let mut seg = vec![0.0f32; SEG_LR];
            let dispo = n_lr.saturating_sub(deb_lr).min(SEG_LR);
            seg[..dispo].copy_from_slice(&canal[deb_lr..deb_lr + dispo]);

            let (spec, t) = stft::spec(&seg);
            debug_assert_eq!(t, 862);
            let spec_hr = modele.segment(&spec)?;
            let hr = stft::ispec(&spec_hr, 862);

            let deb_hr = deb_lr * ECHELLE;
            let premier = s == 0;
            let dernier = s == n_segments - 1;
            for (i, &v) in hr.iter().enumerate() {
                let j = deb_hr + i;
                if j >= n_hr {
                    break;
                }
                let w = fondu(i, hr.len(), premier, dernier);
                canaux_hr[c][j] += v * w;
                if c == 0 {
                    poids[j] += w;
                }
            }
            faits += 1;
            progres(faits, total);
        }
    }

    for canal in &mut canaux_hr {
        for (v, &w) in canal.iter_mut().zip(&poids) {
            if w > 1e-6 {
                *v /= w;
            }
        }
    }
    Ok(canaux_hr)
}

/// Rabat la crête à −0,3 dBFS (le modèle n'est pas borné, on préfère réduire
/// qu'écrêter) puis écrit le WAV 44,1 kHz.
fn normaliser_et_ecrire(sortie: &Path, mut canaux: Vec<Vec<f32>>) -> Result<()> {
    let crete = canaux
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f32, |m, v| m.max(v.abs()));
    if crete > 0.966 {
        let g = 0.966 / crete;
        for canal in &mut canaux {
            for v in canal.iter_mut() {
                *v *= g;
            }
        }
    }
    ecrire_wav(sortie, &canaux, HR_SR)
}

/// Combine `source` et la sortie du modèle `hd` de façon à ce que le HD **ne
/// puisse qu'ajouter** de l'aigu, jamais en retirer.
///
/// - sous `fc` (la coupure estimée de la source) : le spectre de la source,
///   inchangé — c'est l'audible réel, il ne se touche pas ;
/// - au-dessus de `fc` : pour chaque raie, celle des deux (source, modèle) qui
///   porte **le plus d'énergie**. Une coupure sous-estimée ne fait alors
///   perdre aucun contenu réel : là où la source est encore présente, elle
///   gagne.
///
/// Domaine STFT (2048, recouvrement 3/4), en place sur `hd`.
fn melanger_hf(source: &[Vec<f32>], hd: &mut [Vec<f32>], fc: f32) {
    use rustfft::{num_complex::Complex, FftPlanner};
    const N: usize = 2048;
    const HOP: usize = N / 4;
    let fc = fc.clamp(2_000.0, HR_SR as f32 / 2.0 - 1_000.0);
    let df = HR_SR as f32 / N as f32;
    let raie_fc = (fc / df) as usize;

    let mut planner = FftPlanner::<f32>::new();
    let avant = planner.plan_fft_forward(N);
    let arriere = planner.plan_fft_inverse(N);
    let hann: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let mut norm_ola = 0.0f32;
    let mut i = 0;
    while i < N {
        norm_ola += hann[i] * hann[i];
        i += HOP;
    }
    let inv_ola = 1.0 / norm_ola.max(1e-6);
    let inv_fft = 1.0 / N as f32;

    let mut bs = vec![Complex::new(0.0f32, 0.0); N];
    let mut bh = vec![Complex::new(0.0f32, 0.0); N];

    for (cs, ch) in source.iter().zip(hd.iter_mut()) {
        let n = cs.len().min(ch.len());
        let mut sortie = vec![0.0f32; ch.len()];
        let mut pos = 0;
        while pos + N <= n {
            for k in 0..N {
                bs[k] = Complex::new(cs[pos + k] * hann[k], 0.0);
                bh[k] = Complex::new(ch[pos + k] * hann[k], 0.0);
            }
            avant.process(&mut bs);
            avant.process(&mut bh);
            for k in 0..=N / 2 {
                let v = if k <= raie_fc || bs[k].norm_sqr() >= bh[k].norm_sqr() {
                    bs[k]
                } else {
                    bh[k]
                };
                bh[k] = v;
                if k > 0 && k < N / 2 {
                    bh[N - k] = v.conj();
                }
            }
            arriere.process(&mut bh);
            for k in 0..N {
                sortie[pos + k] += bh[k].re * inv_fft * hann[k] * inv_ola;
            }
            pos += HOP;
        }
        // Le cœur (addition-recouvrement pleine) prend le mélange ; les N
        // premiers/derniers échantillons gardent la source brute — quelques
        // dizaines de millisecondes.
        let fin = n.saturating_sub(N);
        for j in 0..ch.len() {
            ch[j] = if j >= N && j < fin { sortie[j] } else { cs.get(j).copied().unwrap_or(0.0) };
        }
    }
}

/// Fenêtre trapézoïdale sur un segment de sortie : montée sur le recouvrement
/// d'entrée (sauf pour le tout premier segment), descente sur celui de sortie
/// (sauf pour le dernier), plat au milieu.
fn fondu(i: usize, len: usize, premier: bool, dernier: bool) -> f32 {
    let rampe = CHEV_LR * ECHELLE;
    if rampe == 0 || len <= 2 * rampe {
        return 1.0;
    }
    let montee = if premier || i >= rampe {
        1.0
    } else {
        i as f32 / rampe as f32
    };
    let descente = if dernier || i < len - rampe {
        1.0
    } else {
        (len - 1 - i) as f32 / rampe as f32
    };
    montee.min(descente)
}

/// Décode `entree` en canaux planaires, à sa fréquence d'origine. Rend
/// `(canaux, sr, coupure_estimée)`.
fn decoder(entree: &Path) -> Result<(Vec<Vec<f32>>, u32, f32)> {
    let fichier = std::fs::File::open(entree)
        .map_err(|e| Error::Ouverture(entree.to_path_buf(), e))?;
    let source = rodio::Decoder::try_from(fichier).map_err(|e| Error::Decode(e.to_string()))?;
    let sr = source.sample_rate().get();
    let n_canaux = source.channels().get() as usize;
    let entrelace: Vec<f32> = source.collect();
    if entrelace.is_empty() {
        return Err(Error::Decode("aucun échantillon".into()));
    }

    let trames = entrelace.len() / n_canaux;
    let mut plans: Vec<Vec<f32>> = vec![Vec::with_capacity(trames); n_canaux];
    for t in 0..trames {
        for (c, plan) in plans.iter_mut().enumerate() {
            plan.push(entrelace[t * n_canaux + c]);
        }
    }
    let coupure = coupure_estimee(&plans[0], sr);
    Ok((plans, sr, coupure))
}

/// Fréquence la plus haute où le spectre moyen est encore ~35 dB au-dessus du
/// bruit — grossier mais suffisant pour dire « déjà pleine bande » ou non.
fn coupure_estimee(x: &[f32], sr: u32) -> f32 {
    use rustfft::{num_complex::Complex, FftPlanner};
    const N: usize = 8192;
    if x.len() < N * 2 {
        return sr as f32 / 2.0;
    }
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
    let hann: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let raies = N / 2;
    let mut spectre = vec![0.0f32; raies];
    let pas = (x.len() - N) / 20;
    let mut d = 0;
    let mut prises = 0;
    while d + N <= x.len() && prises < 20 {
        let mut buf: Vec<Complex<f32>> =
            (0..N).map(|i| Complex::new(x[d + i] * hann[i], 0.0)).collect();
        fft.process(&mut buf);
        for (k, v) in spectre.iter_mut().enumerate() {
            *v += buf[k].norm_sqr();
        }
        prises += 1;
        d += pas.max(N);
    }
    // Niveau de référence : médiane des bins 300 Hz – 3 kHz.
    let hz = sr as f32 / N as f32;
    let (lo, hi) = ((300.0 / hz) as usize, ((3000.0 / hz) as usize).min(raies));
    let mut grave: Vec<f32> = spectre[lo..hi].to_vec();
    grave.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let seuil = grave[grave.len() / 2].max(1e-20) * 1e-4; // ~−40 dB
    for k in (1..raies).rev() {
        if spectre[k] > seuil {
            return k as f32 * hz;
        }
    }
    sr as f32 / 2.0
}

#[doc(hidden)]
pub fn reech_test(x: &[f32], de: u32, vers: u32) -> Vec<f32> {
    reechantillonner(x, de, vers).unwrap()
}

/// Rééchantillonnage sinc (FFT) d'un canal, `de` → `vers` Hz.
///
/// Deux réglages contre-intuitifs, tous deux payés d'un bug :
///
/// - **`new` (ou `new_custom` avec `sub_chunks` élevé) coupe beaucoup trop
///   bas.** Le défaut vise un sous-bloc de ~256 trames pour une faible
///   latence : à 44,1 → 11,025 kHz cela donne une FFT de sortie de 64 points,
///   dont la fenêtre anti-repliement mange près de 1 kHz sous Nyquist (coupure
///   à ~4,3 kHz au lieu de ~5,3). Sur un morceau plein bande, AERO voit alors
///   une entrée déjà sur-filtrée et rend un aigu étouffé — le cas « Trawalc'h »
///   qui a révélé le défaut. `sub_chunks = 1` avec un grand `chunk_size` rend
///   une FFT de sortie de milliers de points et la coupure remonte au bord.
/// - **`process_all` laisse une amorce fausse (~1 s)** — son retrait annoncé
///   du retard de démarrage ne suffit pas. On préfixe l'entrée d'un bloc
///   réfléchi et on jette l'amorce correspondante.
///
/// Résultat : coupure à ~5,3 kHz (comme `torchaudio.resample`), écart au
/// rééchantillonneur de référence 5 × 10⁻⁴.
fn reechantillonner(x: &[f32], de: u32, vers: u32) -> Result<Vec<f32>> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler, WindowFunction};

    const BLOC: usize = 16_384;
    let amorce = 4096.min(x.len());
    let mut rembourre = Vec::with_capacity(x.len() + amorce);
    rembourre.extend((1..=amorce).rev().map(|i| x[i.min(x.len() - 1)]));
    rembourre.extend_from_slice(x);

    let mut r = Fft::<f32>::new_custom(
        de as usize,
        vers as usize,
        BLOC,
        1, // un seul sous-bloc → grande FFT → coupure nette au bord
        1,
        WindowFunction::Hann,
        FixedSync::Input,
    )
    .map_err(|e| Error::Reechantillonnage(e.to_string()))?;
    let entree = InterleavedSlice::new(&rembourre, 1, rembourre.len())
        .map_err(|e| Error::Reechantillonnage(e.to_string()))?;
    let sortie = r
        .process_all(&entree, rembourre.len(), None)
        .map_err(|e| Error::Reechantillonnage(e.to_string()))?;

    let a_jeter = (amorce as u64 * vers as u64 / de as u64) as usize;
    let cible = (x.len() as u64 * vers as u64 / de as u64) as usize;
    let mut sortie = sortie.take_data();
    sortie.drain(..a_jeter.min(sortie.len()));
    sortie.truncate(cible);
    Ok(sortie)
}

/// Écrit des canaux planaires en WAV PCM 16 bits — même format que le cache de
/// stems (`crates/editor/src/wav.rs`), lu partout, `rodio` compris. Écrit à la
/// main : le projet n'ajoute pas de dépendance pour un en-tête RIFF.
fn ecrire_wav(chemin: &Path, canaux: &[Vec<f32>], sr: u32) -> Result<()> {
    use std::io::Write;

    let n_canaux = canaux.len() as u16;
    let trames = canaux.iter().map(Vec::len).min().unwrap_or(0);
    let bloc = n_canaux * 2; // 16 bits
    let octets_donnees = (trames * bloc as usize) as u32;

    let mut f = std::io::BufWriter::new(std::fs::File::create(chemin)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + octets_donnees).to_le_bytes())?;
    f.write_all(b"WAVE")?;
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM entier
    f.write_all(&n_canaux.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * bloc as u32).to_le_bytes())?;
    f.write_all(&bloc.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&octets_donnees.to_le_bytes())?;

    for t in 0..trames {
        for canal in canaux {
            let v = (canal[t].clamp(-1.0, 1.0) * i16::MAX as f32).round();
            f.write_all(&(v as i16).to_le_bytes())?;
        }
    }
    f.flush()?;
    Ok(())
}

// -------------------------------------------------------- cache & aiguillage

use std::sync::atomic::{AtomicBool, Ordering};

/// Lecture en HD quand un cache existe — drapeau global au processus, posé par
/// l'interface (bouton « HD »). Comme l'amélioration « E » du lecteur.
static LECTURE_HD: AtomicBool = AtomicBool::new(false);

pub fn lecture_hd() -> bool {
    LECTURE_HD.load(Ordering::Relaxed)
}
pub fn set_lecture_hd(v: bool) {
    LECTURE_HD.store(v, Ordering::Relaxed);
}

/// Version du pipeline de régénération. **À incrémenter dès que `regenerer`
/// change** (modèle, rééchantillonnage, mélange…) : le nom de fichier la
/// porte, si bien qu'un cache produit par une version antérieure n'est plus
/// trouvé et [`purger_anciens`] le supprime. Sans cela, une correction du son
/// laissait les anciens fichiers étouffés en place, joués tels quels.
pub const VERSION_CACHE: u32 = 3;

/// Nom du fichier cache HD d'une piste, dans `racine`. Hachage du chemin
/// d'origine (deux pistes de même nom dans des dossiers différents ne se
/// marchent pas dessus) + la version du pipeline.
pub fn chemin_cache(racine: &Path, source: &Path) -> std::path::PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for o in source.to_string_lossy().as_bytes() {
        h ^= *o as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    racine.join(format!("{h:016x}-v{VERSION_CACHE}.wav"))
}

/// Supprime du cache tout WAV qui n'est pas de la version courante. À appeler
/// au démarrage et avant chaque régénération.
pub fn purger_anciens(racine: &Path) {
    let suffixe = format!("-v{VERSION_CACHE}.wav");
    let Ok(entrees) = std::fs::read_dir(racine) else {
        return;
    };
    for e in entrees.flatten() {
        let nom = e.file_name();
        let nom = nom.to_string_lossy();
        if nom.ends_with(".wav") && !nom.ends_with(&suffixe) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Chemin à ouvrir pour `source` : la version HD si la lecture HD est active
/// **et** que le cache existe, sinon `source` inchangé.
pub fn resoudre(racine: &Path, source: &Path) -> std::path::PathBuf {
    if lecture_hd() {
        let hd = chemin_cache(racine, source);
        if hd.is_file() {
            return hd;
        }
    }
    source.to_path_buf()
}
