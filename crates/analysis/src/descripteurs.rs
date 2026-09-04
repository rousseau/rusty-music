// SPDX-License-Identifier: GPL-3.0-or-later
//! Tempo, tonalité, énergie : les grandeurs nommables d'un morceau.
//!
//! `ui-spec.md` promet de colorer et filtrer la carte par année, tempo et
//! énergie. L'année vient des tags, les deux autres d'ici.
//!
//! **Les algorithmes sont ceux des bibliothèques du domaine, pas des
//! inventions.** Tempo : flux spectral puis autocorrélation à peigne, comme
//! `onset/specflux` et `beattracking` d'aubio. Tonalité : chroma corrélé aux
//! profils de Krumhansl-Schmuckler, comme QM-DSP (celui de Mixxx).
//!
//! **Écrits plutôt que liés à l'origine pour éviter une dépendance C et un
//! passage sous copyleft — raison caduque depuis le passage du projet sous
//! GPL-3.0-or-later** (`CLAUDE.md`), qui débloque explicitement `aubio` et
//! `Essentia`. Réécrire ce module par-dessus une bibliothèque mûre reste une
//! évaluation à faire, pas encore faite (`docs/rust-audio-stack.md`) ; ce
//! qui suit n'est donc qu'un correctif de l'algorithme maison existant, pas
//! un jugement sur sa pérennité.
//!
//! **La grille de battements est dans `battements.rs`**, et elle part d'ici :
//! même flux spectral, même autocorrélation. Ce module rend un tempo, celui-là
//! y ajoute la phase — où les battements tombent. La note qui figurait ici
//! disait que la grille était hors périmètre parce que seul le mixage DJ
//! l'exigeait ; c'était faux, la greffe du module 3 en a besoin aussi pour
//! aligner ses temps forts.

use std::path::Path;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::decode;
use crate::mel::SR;

/// **Deux fenêtres : une attaque se situe dans le temps, une note en
/// fréquence.** Avec la seule fenêtre courte, un do₃ et le la♯ voisin tombaient
/// dans la même raie et le chroma d'un accord parfait s'étalait sur huit
/// classes.
const N_FFT: usize = 2048; // attaques : 43 ms, 93,75 trames/s
const HOP: usize = 512;
const N_FFT_CHROMA: usize = 8192; // chroma : 171 ms, 5,9 Hz par raie
const HOP_CHROMA: usize = 4096;

pub(crate) const TPS: f32 = SR as f32 / HOP as f32;

/// En deçà de 60 on confond le tempo avec la mesure, au-delà de 200 avec les
/// doubles croches.
pub(crate) const BPM_MIN: f32 = 60.0;
pub(crate) const BPM_MAX: f32 = 200.0;

/// **On teste des tempos, pas des décalages entiers.** À 93,75 trames/s, un
/// morceau à 150 BPM a une période de 37,5 trames qu'aucun décalage entier
/// n'atteint : le double, exact, l'emportait et le morceau sortait à 75.
const CANDIDATS: usize = 240;

/// L'autocorrélation ne distingue pas un tempo de son double. Comme aubio, on
/// tranche par une préférence autour de 120 BPM — log-normale ici, Rayleigh
/// chez eux, même rôle.
const BPM_PREFERE: f32 = 120.0;
const ETALEMENT: f32 = 0.9;

/// En dessous de ce rapport entre les temps faibles et les temps forts d'une
/// grille, les attaques alternent assez pour que le gagnant soit jugé une
/// subdivision d'un temps deux fois plus lent : on le divise par deux.
///
/// L'erreur d'octave la plus fréquente va dans un seul sens : une subdivision
/// régulière et bien marquée (guitare en « boom-chick », charleston discret)
/// dessine une attaque plus nette que le temps lui-même et double le tempo
/// rendu — jamais l'inverse. La préférence log-normale ci-dessus ne suffit
/// pas à la corriger : sur la plage retenue (60-200), elle est plus proche de
/// 120 pour un tempo doublé que pour l'original resté lent, et confirme donc
/// l'erreur au lieu de la rattraper. D'où cette seconde passe.
///
/// **Le tell est l'alternance fort/faible d'un temps sur deux**, pas
/// l'évidence brute au sous-multiple. Comparer `brut(bpm / 2)` à `brut(bpm)`,
/// ou même `correle(env, 2d)` à `correle(env, d)`, ne discrimine rien : ces
/// décalages tombent tous sur des pics réels d'un train périodique de
/// période T, et le seul écart mesuré vient de la quantification en trames
/// d'une période fractionnaire (mesuré : un train de clics parfaitement
/// régulier donnait déjà un rapport de 1,22, division à tort de 55 % de la
/// bibliothèque). On pose donc un peigne à la période du gagnant, on cherche
/// sa meilleure phase, et on compare l'énergie moyenne des dents de rang pair
/// à celle des dents de rang impair — même période, même interpolation pour
/// les deux, le biais de quantification s'annule. Un train régulier rend un
/// rapport de ~1 et n'est jamais touché.
///
/// Valeur de départ, à recaler sur `examples/octave_avant_apres.rs`.
const SEUIL_SOUS_OCTAVE: f32 = 0.60;

/// Plancher de la moitié corrigée — distinct de `BPM_MIN`, plus bas.
///
/// `BPM_MIN` protège la grille de candidats : en dessous, un gagnant *de
/// départ* confondrait le tempo avec la mesure. La correction d'octave part
/// d'un gagnant déjà validé dans cette plage et se contente de le diviser
/// par deux — une lecture à 50 BPM reste plausible pour un morceau lent même
/// si la grille elle-même ne l'aurait pas proposée d'emblée. Sur « Give My
/// Love to Rose », c'est ce qui a permis à une quatrième fenêtre (117 BPM,
/// moitié 58,6) de rejoindre les trois autres autour de 65 plutôt que de
/// rester bloquée juste sous 60.
const BPM_MIN_CORRECTION: f32 = 45.0;

/// Nombre de phases essayées pour caler le peigne de la correction
/// descendante. Le pas vaut T/`PHASES_PEIGNE` — à 120 BPM, 46,9 trames de
/// période, soit ~1 trame, sous la largeur d'un pic de flux.
const PHASES_PEIGNE: usize = 48;

/// Sens montant : sous ce ratio, le double du tempo gagnant n'est qu'un
/// artefact et ne le détrône pas.
///
/// Jugé sur `brut`, qui discrimine ici là où il ne discrimine pas dans
/// l'autre sens : le double du gagnant échantillonne l'autocorrélation en
/// d/2 et 3d/2 — des creux, pas des pics — donc un faux double y perd
/// franchement, et le seuil peut être serré (1,02 : le double doit dépasser
/// l'évidence du gagnant lui-même, pas seulement en approcher une fraction).
/// Une subdivision en croches est quasi universelle dans la musique
/// populaire — charleston, guitare rythmique — si bien que presque tout
/// morceau montre une évidence non négligeable au double de son tempo réel ;
/// un seuil lâche suroctaverait la majorité de la bibliothèque.
///
/// Calé sur un cas réel : « Hard Core 100% Fluor » (Watcha, hardcore)
/// ressortait à 46 BPM alors que l'évidence brute, sur une grille élargie,
/// pointe nettement vers ~183 sur 4 de ses 5 fenêtres. Le gagnant de grille
/// (dans [60, 200]) y est 92,2 (évidence 0,711) contre 183,5 son double
/// (évidence 0,730, ratio 1,03) — largement au-dessus du seuil retenu.
const SEUIL_SUR_OCTAVE: f32 = 1.02;

/// Plafond du double corrigé — symétrique de `BPM_MIN_CORRECTION`, par le
/// même rapport à sa borne de grille (`BPM_MIN_CORRECTION / BPM_MIN` = 0,75,
/// donc `BPM_MAX / 0,75` ≈ 267).
///
/// `BPM_MAX` protège la grille de candidats : au-delà, un gagnant *de
/// départ* confondrait le tempo avec ses doubles croches (voir sa doc). La
/// correction part d'un gagnant déjà validé dans cette plage et se contente
/// de le doubler — une lecture à 267 BPM reste plausible pour un morceau
/// très rapide (hardcore, speedcore) même si la grille elle-même ne
/// l'aurait pas proposée d'emblée.
const BPM_MAX_CORRECTION: f32 = 266.7;

/// Chroma de la₂ à do₇. Plus bas, l'écart d'un demi-ton rejoint la largeur
/// d'une raie ; plus haut, les cymbales alimentent les douze classes.
const F_MIN: f32 = 110.0;
const F_MAX: f32 = 2093.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Descripteurs {
    /// `None` quand rien ne pulse. **Mesuré sur la bibliothèque : cela
    /// n'arrive qu'au silence.** Les seuils ci-dessous écartent l'absence de
    /// signal, pas la musique faiblement pulsée — un conte lu reçoit donc un
    /// tempo, qui ne veut rien dire. Les relever demanderait une vérité
    /// terrain qu'on n'a pas.
    pub bpm: Option<f32>,
    /// Notée à l'anglaise : « F min », « C maj ». Même réserve que `bpm`.
    pub tonalite: Option<String>,
    /// Valeur efficace, entre 0 et 1.
    pub energie: f32,
    /// La même en décibels pleine échelle, plancher à −100.
    pub sonie: f32,
    /// Taux de passage par zéro, moyenné sur les trames — élevé pour un son
    /// bruité/percussif, bas pour un son tonal grave. `None` au silence,
    /// même réserve que `bpm`.
    pub zcr: Option<f32>,
    /// Centroïde spectral (Hz) : moyenne, puis écart-type d'une trame à
    /// l'autre — un morceau au timbre constant a un faible écart-type, un
    /// morceau qui alterne voix et cymbales un fort.
    pub centroide_moy: Option<f32>,
    pub centroide_ecart: Option<f32>,
    /// Rolloff spectral (Hz, seuil 85 %), même paire moyenne/écart-type.
    pub rolloff_moy: Option<f32>,
    pub rolloff_ecart: Option<f32>,
    /// Aplatissement spectral (0..1, bruit vs tonal), même paire.
    pub flatness_moy: Option<f32>,
    pub flatness_ecart: Option<f32>,
}

/// Plans de FFT, fenêtres, et classe de hauteur de chaque raie. À construire
/// une fois par fil : planifier coûte plus cher que transformer.
pub struct Analyseur {
    fft: Arc<dyn Fft<f32>>,
    fft_chroma: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    hann_chroma: Vec<f32>,
    /// Classe et poids de chaque raie du spectre de chroma.
    classes: Vec<Option<(usize, f32)>>,
    /// Fréquence de chaque raie de `spectres()` — pour centroïde et rolloff,
    /// calculée une fois plutôt qu'à chaque trame.
    raies: Vec<f32>,
}

impl Default for Analyseur {
    fn default() -> Self {
        Self::new()
    }
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
        .collect()
}

impl Analyseur {
    pub fn new() -> Self {
        let mut p = FftPlanner::new();
        let classes = (0..N_FFT_CHROMA / 2 + 1)
            .map(|k| {
                let f = SR as f32 * k as f32 / N_FFT_CHROMA as f32;
                if !(F_MIN..=F_MAX).contains(&f) {
                    return None;
                }
                // Note MIDI : 69 = la₄ = 440 Hz, le reste modulo 12 donne la
                // classe. Le poids décroît avec la distance au demi-ton : une
                // sinusoïde déborde sur les raies voisines, et près du grave
                // l'une d'elles tombe à mi-chemin entre deux demi-tons — un do₃
                // seul versait ainsi 16 % de son énergie dans do♯.
                let note = 69.0 + 12.0 * (f / 440.0).log2();
                let poids = (1.0 - 2.0 * (note - note.round()).abs()).max(0.0);
                Some(((note.round() as i32).rem_euclid(12) as usize, poids))
            })
            .collect();
        let raies = (0..N_FFT / 2 + 1)
            .map(|k| SR as f32 * k as f32 / N_FFT as f32)
            .collect();
        Self {
            fft: p.plan_fft_forward(N_FFT),
            fft_chroma: p.plan_fft_forward(N_FFT_CHROMA),
            hann: hann(N_FFT),
            hann_chroma: hann(N_FFT_CHROMA),
            classes,
            raies,
        }
    }

    fn transformer(
        fft: &Arc<dyn Fft<f32>>,
        w: &[f32],
        signal: &[f32],
        hop: usize,
    ) -> Vec<Vec<f32>> {
        let n = w.len();
        if signal.len() < n {
            return Vec::new();
        }
        let mut buf = vec![Complex::new(0.0f32, 0.0); n];
        (0..=signal.len() - n)
            .step_by(hop)
            .map(|d| {
                for (i, t) in buf.iter_mut().enumerate() {
                    *t = Complex::new(signal[d + i] * w[i], 0.0);
                }
                fft.process(&mut buf);
                buf[..n / 2 + 1].iter().map(|c| c.norm()).collect()
            })
            .collect()
    }

    pub(crate) fn spectres(&self, s: &[f32]) -> Vec<Vec<f32>> {
        Self::transformer(&self.fft, &self.hann, s, HOP)
    }

    fn spectres_chroma(&self, s: &[f32]) -> Vec<Vec<f32>> {
        Self::transformer(&self.fft_chroma, &self.hann_chroma, s, HOP_CHROMA)
    }

    fn chroma(&self, spectres: &[Vec<f32>]) -> [f32; 12] {
        let mut c = [0.0f32; 12];
        for trame in spectres {
            for (k, m) in trame.iter().enumerate() {
                if let Some(Some((classe, poids))) = self.classes.get(k) {
                    c[*classe] += m * poids;
                }
            }
        }
        c
    }
}

/// Flux spectral : de combien le spectre a grandi. Seules les hausses comptent —
/// une note qui s'éteint n'est pas une attaque.
pub(crate) fn flux(spectres: &[Vec<f32>]) -> Vec<f32> {
    spectres
        .windows(2)
        .map(|p| p[1].iter().zip(&p[0]).map(|(a, b)| (a - b).max(0.0)).sum())
        .collect()
}

/// Fréquence moyenne du spectre, pondérée par l'amplitude — aigu si le son
/// est brillant (cymbale, sifflante), grave sinon. `0.0` sur une trame
/// muette, écartée par l'appelant via l'énergie totale.
pub(crate) fn centroide(trame: &[f32], raies: &[f32]) -> f32 {
    let total: f32 = trame.iter().sum();
    if total <= f32::EPSILON {
        return 0.0;
    }
    trame.iter().zip(raies).map(|(m, f)| m * f).sum::<f32>() / total
}

/// Sous ce seuil (85 %, la valeur par défaut du domaine — Essentia et
/// librosa la partagent) tombe l'essentiel de l'énergie spectrale. Distingue
/// un son à large bande (bruit blanc, cymbale) d'un son concentré en
/// fréquence (voix, basse).
const SEUIL_ROLLOFF: f32 = 0.85;

pub(crate) fn rolloff(trame: &[f32], raies: &[f32], seuil: f32) -> f32 {
    let total: f32 = trame.iter().map(|m| m * m).sum();
    if total <= f32::EPSILON {
        return 0.0;
    }
    let cible = total * seuil;
    let mut cumul = 0.0;
    for (m, f) in trame.iter().zip(raies) {
        cumul += m * m;
        if cumul >= cible {
            return *f;
        }
    }
    raies.last().copied().unwrap_or(0.0)
}

/// Moyenne géométrique sur moyenne arithmétique du spectre — proche de 1
/// pour du bruit (spectre plat), proche de 0 pour un son tonal (quelques
/// raies dominantes). Le petit epsilon dans le logarithme évite `ln(0)` sur
/// une raie éteinte, sans biaiser un spectre par ailleurs riche.
pub(crate) fn aplatissement(trame: &[f32]) -> f32 {
    let n = trame.len() as f32;
    if n == 0.0 {
        return 0.0;
    }
    let arithmetique = trame.iter().sum::<f32>() / n;
    if arithmetique <= f32::EPSILON {
        return 0.0;
    }
    let log_geometrique = trame.iter().map(|m| (m + 1e-10).ln()).sum::<f32>() / n;
    log_geometrique.exp() / arithmetique
}

/// Fraction de changements de signe entre échantillons consécutifs, sur le
/// signal temporel brut — élevé pour un son bruité/percussif, bas pour un
/// son tonal grave. Pas de FFT ici, contrairement aux trois précédents.
pub(crate) fn zcr(signal: &[f32]) -> f32 {
    if signal.len() < 2 {
        return 0.0;
    }
    let croisements = signal
        .windows(2)
        .filter(|p| (p[0] >= 0.0) != (p[1] >= 0.0))
        .count();
    croisements as f32 / (signal.len() - 1) as f32
}

/// Moyenne et écart-type d'une série — accumulée au fil des trames plutôt
/// que gardée en mémoire, pour ne pas dupliquer les milliers de valeurs
/// d'une analyse complète.
#[derive(Default)]
pub(crate) struct Moyenne {
    n: usize,
    somme: f64,
    somme_carres: f64,
}

impl Moyenne {
    fn pousser(&mut self, v: f32) {
        self.n += 1;
        self.somme += v as f64;
        self.somme_carres += (v as f64).powi(2);
    }

    /// `None` si rien n'a été poussé — le silence, où aucune trame n'a
    /// d'énergie à décrire.
    fn reduire(&self) -> Option<(f32, f32)> {
        if self.n == 0 {
            return None;
        }
        let n = self.n as f64;
        let moyenne = self.somme / n;
        let variance = (self.somme_carres / n - moyenne.powi(2)).max(0.0);
        Some((moyenne as f32, variance.sqrt() as f32))
    }
}

/// Retire la tendance lente : sinon c'est la structure du morceau — couplet,
/// refrain — qui domine l'autocorrélation, pas sa pulsation.
pub(crate) fn centrer(env: &mut [f32]) {
    let l = (TPS * 0.4) as usize;
    if env.len() <= l || l == 0 {
        return;
    }
    let moyennes: Vec<f32> = (0..env.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(l / 2), (i + l / 2).min(env.len() - 1));
            env[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
        })
        .collect();
    for (v, m) in env.iter_mut().zip(moyennes) {
        *v = (*v - m).max(0.0);
    }
}

/// Autocorrélation à décalage fractionnaire, interpolée linéairement. Produit
/// moyen et non somme : un décalage long recouvre moins de trames.
pub(crate) fn correle(env: &[f32], d: f32) -> f32 {
    let n = env.len() as f32 - d - 1.0;
    if n <= 0.0 {
        return 0.0;
    }
    let n = n as usize;
    let somme: f32 = env
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, ici)| {
            let x = i as f32 + d;
            let (j, t) = (x.floor() as usize, x.fract());
            ici * (env[j] * (1.0 - t) + env[j + 1] * t)
        })
        .sum();
    somme / n as f32
}

/// Rapport entre les temps faibles et les temps forts d'une grille de période
/// `periode` (en trames) posée sur l'enveloppe d'attaques.
///
/// Un peigne de Dirac de cette période est glissé sur `PHASES_PEIGNE` phases ;
/// à celle qui ramasse le plus d'énergie, les dents de rang pair et de rang
/// impair sont moyennées séparément, et l'on rend `faible / fort` — le plus
/// petit des deux sur le plus grand.
///
/// **1,0 = aucune alternance**, tous les temps se valent, la période est le
/// vrai battement. **Vers 0 = un temps sur deux est nettement plus faible**,
/// la période n'est qu'une subdivision d'un temps deux fois plus lent (guitare
/// en boom-chick, charleston sur les croches).
///
/// Le peigne travaille à période constante et même interpolation pour les deux
/// familles de dents : contrairement à une comparaison `correle(d)` contre
/// `correle(2d)`, le biais de quantification d'une période fractionnaire en
/// trames s'annule dans le rapport.
fn alternance_dents(env: &[f32], periode: f32) -> f32 {
    if periode < 1.0 || env.len() < 2 {
        return 1.0;
    }
    let dents = |phi: f32| -> (f32, f32) {
        let (mut pair, mut impair) = (0.0f32, 0.0f32);
        let (mut n_pair, mut n_impair) = (0usize, 0usize);
        let mut x = phi;
        let mut rang = 0usize;
        while (x as usize) + 1 < env.len() {
            let (j, t) = (x.floor() as usize, x.fract());
            let v = env[j] * (1.0 - t) + env[j + 1] * t;
            if rang % 2 == 0 {
                pair += v;
                n_pair += 1;
            } else {
                impair += v;
                n_impair += 1;
            }
            rang += 1;
            x += periode;
        }
        let moy = |s: f32, n: usize| if n > 0 { s / n as f32 } else { 0.0 };
        (moy(pair, n_pair), moy(impair, n_impair))
    };

    let mut meilleur = (f32::MIN, 1.0f32);
    for k in 0..PHASES_PEIGNE {
        let phi = periode * k as f32 / PHASES_PEIGNE as f32;
        let (p, i) = dents(phi);
        let total = p + i;
        let (fort, faible) = if p >= i { (p, i) } else { (i, p) };
        let rapport = if fort > f32::EPSILON { faible / fort } else { 1.0 };
        if total > meilleur.0 {
            meilleur = (total, rapport);
        }
    }
    meilleur.1
}

/// Le tempo le plus vraisemblable, ou `None` si rien ne ressort — en pratique,
/// le silence seul.
pub(crate) fn tempo(env: &[f32]) -> Option<f32> {
    let decalage = |bpm: f32| 60.0 * TPS / bpm;
    let reference = correle(env, 0.0);
    if (env.len() as f32) < decalage(BPM_MIN) * 3.0 || reference <= f32::EPSILON {
        return None;
    }
    // Peigne sur trois harmoniques : une pulsation de période T résonne aussi à
    // 2T et 3T, l'inverse est faux. C'est ce qui sépare la fondamentale de ses
    // sous-multiples.
    const H: usize = 3;
    let poids_total: f32 = (1..=H).map(|h| 1.0 / h as f32).sum();
    let brut = |bpm: f32| -> f32 {
        let d = decalage(bpm);
        (1..=H)
            .map(|h| correle(env, d * h as f32) / h as f32)
            .sum::<f32>()
            / poids_total
            / reference
    };

    let (bpm, _) = (0..CANDIDATS)
        .map(|i| {
            let bpm = BPM_MIN * (BPM_MAX / BPM_MIN).powf(i as f32 / (CANDIDATS - 1) as f32);
            let prior = (-0.5 * ((bpm / BPM_PREFERE).ln() / ETALEMENT).powi(2)).exp();
            (bpm, brut(bpm) * prior)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, s)| *s > 0.02)?;

    // Correction d'octave. Les deux sens sont mutuellement exclusifs, jamais
    // chaînés : un gagnant ne descend ou ne monte qu'une fois.
    //
    // Ils ne se jugent pas sur la même grandeur, et c'est voulu — voir
    // `SEUIL_SOUS_OCTAVE` et `SEUIL_SUR_OCTAVE`. Le sens descendant regarde
    // l'alternance fort/faible d'un temps sur deux ([`alternance_dents`]) ;
    // `brut(bpm / 2)` ne discriminerait rien, il n'échantillonne que des
    // pics réels. Le sens montant, lui, peut s'appuyer sur `brut` : le
    // double échantillonne des creux, un faux double y perd.
    let d = decalage(bpm);
    let corrige_vers_le_bas =
        bpm / 2.0 >= BPM_MIN_CORRECTION && alternance_dents(env, d) < SEUIL_SOUS_OCTAVE;
    let corrige_vers_le_haut =
        bpm * 2.0 <= BPM_MAX_CORRECTION && brut(bpm * 2.0) >= brut(bpm) * SEUIL_SUR_OCTAVE;
    // Le sens montant l'emporte quand les deux qualifient : son test est le
    // plus rare et le plus spécifique.
    let bpm = if corrige_vers_le_haut {
        bpm * 2.0
    } else if corrige_vers_le_bas {
        bpm / 2.0
    } else {
        bpm
    };
    Some(bpm)
}

/// Profils de Krumhansl-Schmuckler : la place de chaque degré dans une
/// tonalité, mesurée sur des auditeurs.
const MAJEUR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINEUR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut haut, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        haut += (x - ma) * (y - mb);
        va += (x - ma).powi(2);
        vb += (y - mb).powi(2);
    }
    let bas = (va * vb).sqrt();
    if bas <= f32::EPSILON {
        0.0
    } else {
        haut / bas
    }
}

/// La meilleure des vingt-quatre tonalités. `None` sur un chroma plat : des
/// percussions seules ou de la parole n'ont pas de tonalité, et leur en donner
/// une serait un mensonge commode.
fn tonalite(chroma: &[f32; 12]) -> Option<String> {
    if chroma.iter().sum::<f32>() <= f32::EPSILON {
        return None;
    }
    (0..12)
        .flat_map(|f| {
            [(&MAJEUR, "maj"), (&MINEUR, "min")].map(|(profil, mode)| {
                // C'est le profil qu'on tourne, pas le chroma : tourner le
                // chroma changerait aussi le nom qu'on lui associe.
                let tourne: Vec<f32> = (0..12).map(|i| profil[(i + 12 - f) % 12]).collect();
                (format!("{} {mode}", NOTES[f]), correlation(chroma, &tourne))
            })
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, r)| *r > 0.2)
        .map(|(nom, _)| nom)
}

/// Analyse un fichier. Réutilise le fenêtrage du module 2 — cinq extraits de
/// dix secondes — dont le coût de décodage est déjà éprouvé.
pub fn analyser(path: &Path, a: &Analyseur) -> Result<Descripteurs, decode::Error> {
    Ok(analyser_fenetres(
        &decode::fenetres(path, decode::FENETRES)?,
        a,
    ))
}

/// Tempo à la **médiane** des fenêtres : une fenêtre tombée sur une intro libre
/// rendrait une valeur aberrante que la moyenne propagerait. Le chroma, lui, se
/// cumule — la tonalité se lit d'autant mieux qu'on a entendu plus de notes.
pub fn analyser_fenetres(fenetres: &[Vec<f32>], a: &Analyseur) -> Descripteurs {
    let mut tempos = Vec::new();
    let mut chroma = [0.0f32; 12];
    let (mut carres, mut n) = (0.0f64, 0usize);
    let mut centroide_stat = Moyenne::default();
    let mut rolloff_stat = Moyenne::default();
    let mut flatness_stat = Moyenne::default();
    let mut zcr_stat = Moyenne::default();

    for f in fenetres {
        // Un seul calcul de spectres, réutilisé pour le flux (tempo) et les
        // trois descripteurs de forme spectrale — pas de FFT en plus.
        let spectres = a.spectres(f);
        let mut env = flux(&spectres);
        centrer(&mut env);
        tempos.extend(tempo(&env));
        for (dst, src) in chroma.iter_mut().zip(a.chroma(&a.spectres_chroma(f))) {
            *dst += src;
        }
        for trame in &spectres {
            centroide_stat.pousser(centroide(trame, &a.raies));
            rolloff_stat.pousser(rolloff(trame, &a.raies, SEUIL_ROLLOFF));
            flatness_stat.pousser(aplatissement(trame));
        }
        // Le ZCR se lit sur le signal brut, pas sur un spectre — mêmes
        // bornes de trame (`N_FFT`/`HOP`) que `spectres()` pour rester à la
        // même résolution temporelle que les trois précédents.
        if f.len() >= N_FFT {
            for d in (0..=f.len() - N_FFT).step_by(HOP) {
                zcr_stat.pousser(zcr(&f[d..d + N_FFT]));
            }
        }
        carres += f.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
        n += f.len();
    }

    tempos.sort_by(f32::total_cmp);
    let energie = if n == 0 {
        0.0
    } else {
        (carres / n as f64).sqrt() as f32
    };
    // Silencieux : aucune des grandeurs spectrales n'a de sens — une trame
    // muette a rendu 0.0 partout dans la boucle ci-dessus, ce n'est pas une
    // mesure. Même réserve que `bpm`/`tonalite`.
    let silencieux = energie <= 1e-5;
    let paire = |stat: &Moyenne| -> (Option<f32>, Option<f32>) {
        if silencieux {
            (None, None)
        } else {
            let (m, e) = stat.reduire().unwrap_or((0.0, 0.0));
            (Some(m), Some(e))
        }
    };
    let (centroide_moy, centroide_ecart) = paire(&centroide_stat);
    let (rolloff_moy, rolloff_ecart) = paire(&rolloff_stat);
    let (flatness_moy, flatness_ecart) = paire(&flatness_stat);
    let zcr = if silencieux {
        None
    } else {
        zcr_stat.reduire().map(|(m, _)| m)
    };

    Descripteurs {
        bpm: tempos.get(tempos.len() / 2).copied(),
        tonalite: tonalite(&chroma),
        energie,
        // Plancher : le silence numérique donnerait −∞, que SQLite ne range pas.
        sonie: if energie > 1e-5 {
            20.0 * energie.log10()
        } else {
            -100.0
        },
        zcr,
        centroide_moy,
        centroide_ecart,
        rolloff_moy,
        rolloff_ecart,
        flatness_moy,
        flatness_ecart,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clics réguliers : la seule vérité terrain fabricable sans jeu annoté.
    fn clics(bpm: f32, secondes: f32) -> Vec<f32> {
        let n = (SR as f32 * secondes) as usize;
        let periode = (SR as f32 * 60.0 / bpm) as usize;
        let mut s = vec![0.0f32; n];
        for debut in (0..n).step_by(periode) {
            let duree = (SR as usize / 50).min(n - debut);
            for i in 0..duree {
                let t = i as f32 / duree as f32;
                s[debut + i] = (1.0 - t) * (((i * 7919) % 2001) as f32 / 1000.0 - 1.0);
            }
        }
        s
    }

    fn accord(freqs: &[f32], secondes: f32) -> Vec<f32> {
        let n = (SR as f32 * secondes) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                freqs
                    .iter()
                    .map(|f| (std::f32::consts::TAU * f * t).sin())
                    .sum::<f32>()
                    / freqs.len() as f32
            })
            .collect()
    }

    #[test]
    fn le_tempo_retrouve_une_pulsation_connue() {
        let a = Analyseur::new();
        for attendu in [90.0f32, 120.0, 150.0] {
            let mut env = flux(&a.spectres(&clics(attendu, 12.0)));
            centrer(&mut env);
            let trouve = tempo(&env).expect("pulsation régulière");
            let ecart = (trouve - attendu).abs() / attendu;
            assert!(
                ecart < 0.03,
                "{attendu} lus {trouve:.1} ({:.1} %)",
                ecart * 100.0
            );
        }
    }

    /// Un seul train de clics à `bpm_rapide`, mais un clic sur deux plus
    /// fort — le motif d'accent (grosse caisse en fin de mesure) qui, sur un
    /// morceau réel très rapide, peut faire préférer au peigne le rythme de
    /// la mesure plutôt que celui du battement. **Un seul train, pas deux
    /// superposés** : ajouter un second train complet au taux de la mesure
    /// injecterait une périodicité qui n'existe pas dans le mécanisme réel
    /// et biaiserait le test dans le mauvais sens (constaté en écrivant ce
    /// test : ça fait au contraire *descendre* un gagnant déjà juste).
    fn clics_accentues(bpm_rapide: f32, secondes: f32, gain_fort: f32, gain_faible: f32) -> Vec<f32> {
        let n = (SR as f32 * secondes) as usize;
        let periode = (SR as f32 * 60.0 / bpm_rapide) as usize;
        let mut s = vec![0.0f32; n];
        for (indice, debut) in (0..n).step_by(periode).enumerate() {
            let gain = if indice % 2 == 0 { gain_fort } else { gain_faible };
            let duree = (SR as usize / 50).min(n - debut);
            for i in 0..duree {
                let t = i as f32 / duree as f32;
                s[debut + i] = gain * (1.0 - t) * (((i * 7919) % 2001) as f32 / 1000.0 - 1.0);
            }
        }
        s
    }

    #[test]
    fn le_tempo_remonte_un_rythme_accentue_a_la_mesure() {
        // Calé sur le mécanisme réel diagnostiqué sur « Hard Core 100% Fluor »
        // (Watcha, hardcore ~183 BPM) : le gagnant de grille s'arrête sur le
        // rythme de la mesure (moitié du vrai tempo) parce que son accent
        // l'emporte sur le battement régulier, et la correction descendante
        // seule ne peut alors que l'enfoncer. `facteur_accent = 3.0` : assez
        // pour que la mesure domine nettement l'évidence brute, sans quoi le
        // peigne retrouve le battement directement et ne teste rien.
        let a = Analyseur::new();
        let attendu = 176.0f32; // sa moitié, 88, reste dans la grille [60,200]
        // Contraste mesuré, pas deviné : en dessous de 0,8 le rythme de la
        // mesure domine trop pour laisser une évidence testable au
        // battement ; 0,9 donne une marge confortable au-dessus du seuil.
        let mut env = flux(&a.spectres(&clics_accentues(attendu, 12.0, 1.0, 0.9)));
        centrer(&mut env);
        let trouve = tempo(&env).expect("pulsation régulière");
        let ecart = (trouve - attendu).abs() / attendu;
        assert!(
            ecart < 0.03,
            "attendu {attendu}, lu {trouve:.1} ({:.1} %) — la correction montante n'a pas \
             remonté le rythme de la mesure vers le battement",
            ecart * 100.0
        );
    }

    /// Le silence n'a pas de tempo. Rendre 120 par défaut colorerait la carte
    /// d'une valeur inventée.
    #[test]
    fn le_silence_na_ni_tempo_ni_tonalite() {
        let d = analyser_fenetres(&[vec![0.0; SR as usize * 10]], &Analyseur::new());
        assert_eq!(
            (d.bpm, d.tonalite, d.energie, d.sonie),
            (None, None, 0.0, -100.0)
        );
        // Même réserve pour les descripteurs timbraux : une trame muette
        // rendrait 0.0 partout dans la boucle interne, mais ce n'est pas une
        // mesure — le silence ne doit rien affirmer sur le timbre.
        assert_eq!(
            (
                d.zcr,
                d.centroide_moy,
                d.centroide_ecart,
                d.rolloff_moy,
                d.rolloff_ecart,
                d.flatness_moy,
                d.flatness_ecart,
            ),
            (None, None, None, None, None, None, None)
        );
    }

    /// Bruit blanc synthétique — xorshift32, pas le compteur modulaire de
    /// `clics` : un premier essai en `i.wrapping_mul(P) % M` s'est avéré
    /// être une suite de Weyl, pas du bruit — spectre en peigne (pics à
    /// 1148, 2320, 3468 Hz...) et ZCR de 0,05 au lieu de plus de 0,3.
    /// Le décalage-XOR retrouve un spectre plat et un ZCR élevé.
    fn bruit(secondes: f32) -> Vec<f32> {
        let n = (SR as f32 * secondes) as usize;
        let mut etat: u32 = 0x9E37_79B9;
        (0..n)
            .map(|_| {
                etat ^= etat << 13;
                etat ^= etat >> 17;
                etat ^= etat << 5;
                (etat as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }

    /// Le bruit est spectralement plat (aplatissement proche de 1) et
    /// change de signe sans arrêt (ZCR élevé) — l'inverse d'un ton pur.
    #[test]
    fn le_bruit_est_plat_et_traverse_le_zero_souvent() {
        let d = analyser_fenetres(&[bruit(10.0)], &Analyseur::new());
        let aplat = d.flatness_moy.expect("bruit non silencieux");
        let zcr = d.zcr.expect("bruit non silencieux");
        assert!(aplat > 0.5, "aplatissement {aplat} attendu > 0,5 sur du bruit");
        assert!(zcr > 0.3, "ZCR {zcr} attendu > 0,3 sur du bruit");
    }

    /// Un ton pur concentre son énergie sur une seule raie : aplatissement
    /// bas, centroïde proche de sa fréquence.
    #[test]
    fn un_ton_pur_est_concentre_et_situe_sa_frequence() {
        let d = analyser_fenetres(&[accord(&[880.0], 10.0)], &Analyseur::new());
        let aplat = d.flatness_moy.expect("ton non silencieux");
        let centroide = d.centroide_moy.expect("ton non silencieux");
        assert!(aplat < 0.3, "aplatissement {aplat} attendu < 0,3 sur un ton pur");
        // Tolérance large : le bruit de calcul de la FFT en f32 (environ
        // 1e-4 du pic, mesuré) se répartit sur les ~1000 raies jusqu'à
        // 24 kHz, et le centroïde le pondère par une fréquence bien plus
        // grande que celle du ton — quelques dix-millièmes d'amplitude
        // suffisent à déplacer la moyenne de plusieurs dizaines de Hz.
        let ecart = (centroide - 880.0).abs() / 880.0;
        assert!(
            ecart < 0.15,
            "centroïde {centroide:.0} Hz attendu proche de 880 Hz ({:.1} % d'écart)",
            ecart * 100.0
        );
    }

    #[test]
    fn la_tonalite_reconnait_un_accord_parfait() {
        let a = Analyseur::new();
        // Do majeur : do₄ mi₄ sol₄ do₅, fondamentale doublée comme une basse.
        let c = a.chroma(&a.spectres_chroma(&accord(&[261.63, 329.63, 392.00, 523.25], 4.0)));
        assert_eq!(tonalite(&c).as_deref(), Some("C maj"));
        // La mineur : les mêmes classes à une note près — le cas qui sépare
        // vraiment les deux profils.
        let c = a.chroma(&a.spectres_chroma(&accord(&[220.0, 261.63, 329.63, 440.0], 4.0)));
        assert_eq!(tonalite(&c).as_deref(), Some("A min"));
    }

    /// La bibliothèque contient des pistes d'une seconde.
    #[test]
    fn une_fenetre_trop_courte_ne_panique_pas() {
        let a = Analyseur::new();
        assert!(a.spectres(&vec![0.1; 100]).is_empty());
        assert!(a.spectres_chroma(&vec![0.1; 100]).is_empty());
        assert_eq!(analyser_fenetres(&[vec![0.1; 100]], &a).bpm, None);
    }
}
