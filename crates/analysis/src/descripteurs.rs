//! Tempo, tonalité, énergie : les grandeurs nommables d'un morceau.
//!
//! `ui-spec.md` promet de colorer et filtrer la carte par année, tempo et
//! énergie. L'année vient des tags, les deux autres d'ici.
//!
//! **Les algorithmes sont ceux des bibliothèques du domaine, pas des
//! inventions.** Tempo : flux spectral puis autocorrélation à peigne, comme
//! `onset/specflux` et `beattracking` d'aubio. Tonalité : chroma corrélé aux
//! profils de Krumhansl-Schmuckler, comme QM-DSP (celui de Mixxx). Les écrire
//! plutôt que les lier évite une dépendance C et un passage sous copyleft, pour
//! trois cents lignes — et le mixage DJ, seul usage qui exigerait une grille de
//! battements, est hors du périmètre du module 3.

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

const TPS: f32 = SR as f32 / HOP as f32;

/// En deçà de 60 on confond le tempo avec la mesure, au-delà de 200 avec les
/// doubles croches.
const BPM_MIN: f32 = 60.0;
const BPM_MAX: f32 = 200.0;

/// **On teste des tempos, pas des décalages entiers.** À 93,75 trames/s, un
/// morceau à 150 BPM a une période de 37,5 trames qu'aucun décalage entier
/// n'atteint : le double, exact, l'emportait et le morceau sortait à 75.
const CANDIDATS: usize = 240;

/// L'autocorrélation ne distingue pas un tempo de son double. Comme aubio, on
/// tranche par une préférence autour de 120 BPM — log-normale ici, Rayleigh
/// chez eux, même rôle.
const BPM_PREFERE: f32 = 120.0;
const ETALEMENT: f32 = 0.9;

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
        Self {
            fft: p.plan_fft_forward(N_FFT),
            fft_chroma: p.plan_fft_forward(N_FFT_CHROMA),
            hann: hann(N_FFT),
            hann_chroma: hann(N_FFT_CHROMA),
            classes,
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

    fn spectres(&self, s: &[f32]) -> Vec<Vec<f32>> {
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
fn flux(spectres: &[Vec<f32>]) -> Vec<f32> {
    spectres
        .windows(2)
        .map(|p| p[1].iter().zip(&p[0]).map(|(a, b)| (a - b).max(0.0)).sum())
        .collect()
}

/// Retire la tendance lente : sinon c'est la structure du morceau — couplet,
/// refrain — qui domine l'autocorrélation, pas sa pulsation.
fn centrer(env: &mut [f32]) {
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
fn correle(env: &[f32], d: f32) -> f32 {
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

/// Le tempo le plus vraisemblable, ou `None` si rien ne ressort — en pratique,
/// le silence seul.
fn tempo(env: &[f32]) -> Option<f32> {
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

    (0..CANDIDATS)
        .map(|i| {
            let bpm = BPM_MIN * (BPM_MAX / BPM_MIN).powf(i as f32 / (CANDIDATS - 1) as f32);
            let d = decalage(bpm);
            let peigne = (1..=H)
                .map(|h| correle(env, d * h as f32) / h as f32)
                .sum::<f32>()
                / poids_total;
            let prior = (-0.5 * ((bpm / BPM_PREFERE).ln() / ETALEMENT).powi(2)).exp();
            (bpm, peigne / reference * prior)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, s)| *s > 0.02)
        .map(|(bpm, _)| bpm)
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

    for f in fenetres {
        let mut env = flux(&a.spectres(f));
        centrer(&mut env);
        tempos.extend(tempo(&env));
        for (dst, src) in chroma.iter_mut().zip(a.chroma(&a.spectres_chroma(f))) {
            *dst += src;
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

    /// Le silence n'a pas de tempo. Rendre 120 par défaut colorerait la carte
    /// d'une valeur inventée.
    #[test]
    fn le_silence_na_ni_tempo_ni_tonalite() {
        let d = analyser_fenetres(&[vec![0.0; SR as usize * 10]], &Analyseur::new());
        assert_eq!(
            (d.bpm, d.tonalite, d.energie, d.sonie),
            (None, None, 0.0, -100.0)
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
