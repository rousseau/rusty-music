// SPDX-License-Identifier: GPL-3.0-or-later
//! Transposition en flux : Signalsmith Stretch, hauteur seule.
//!
//! Décision d'E3 (écoute à l'aveugle du 9 nov.) : `wsola` garde la vitesse,
//! Signalsmith prend la hauteur. Au banc, un stem de 184 s se transposait en
//! 1 s au lieu de 13 à 17 s, et la pire salve, à 4 stems par blocs de 128
//! trames, restait autour de 1 ms. C'est assez pour transposer **pendant la
//! lecture**, sans calcul préalable ni cache.
//!
//! Le transpositeur ne change pas la durée : il reçoit autant de trames qu'il
//! en rend, avec un retard fixe, [`Transpositeur::latence`]. Ce retard est ce
//! que l'appelant doit compenser pour rester calé sur les autres stems et sur
//! l'horloge de pratique.
//!
//! **Rien n'est alloué après la construction** : `process` travaille dans les
//! tampons que la configuration a dimensionnés. Le transpositeur peut donc
//! servir dans le rappel audio.

use signalsmith_stretch::Stretch;

use crate::multipiste::SR;

const CANAUX: usize = 2;

/// Correction de justesse, en demi-tons par demi-ton demandé (voir
/// [`Transpositeur::regler`]).
const CORRECTION: f32 = 0.0;

/// Un transpositeur stéréo, prêt pour le rappel audio.
pub struct Transpositeur {
    s: Stretch,
    demi_tons: f32,
}

impl Default for Transpositeur {
    fn default() -> Self {
        Self::new()
    }
}

impl Transpositeur {
    /// Préréglage par défaut de Signalsmith à 44,1 kHz : blocs de 120 ms,
    /// pas de 30 ms — celui que le banc E3 a fait écouter.
    pub fn new() -> Self {
        Self {
            s: Stretch::preset_default(CANAUX as u32, SR),
            demi_tons: 0.0,
        }
    }

    /// Bloc d'analyse et pas choisis, en trames (voir le module).
    pub fn avec(bloc: usize, pas: usize) -> Self {
        Self {
            s: Stretch::new(CANAUX as u32, bloc, pas),
            demi_tons: 0.0,
        }
    }

    /// Retard entre une trame reçue et la même trame rendue, en trames.
    pub fn latence(&self) -> usize {
        self.s.input_latency() + self.s.output_latency()
    }

    /// Les demi-tons en vigueur.
    pub fn demi_tons(&self) -> f32 {
        self.demi_tons
    }

    /// Change la hauteur. Prend effet au prochain pas d'analyse, sans remise à
    /// zéro : le vocodeur passe d'un rapport à l'autre sans trou.
    pub fn regler(&mut self, demi_tons: f32) {
        self.demi_tons = demi_tons;
        self.s
            .set_transpose_factor_semitones(demi_tons * (1.0 + CORRECTION), None);
    }

    /// Transpose un bloc entrelacé ; `sortie` a la longueur de `entree`.
    pub fn traiter(&mut self, entree: &[f32], sortie: &mut [f32]) {
        debug_assert_eq!(entree.len(), sortie.len());
        self.s.process(entree, sortie);
    }

    /// Oublie tout ce qui a été reçu : après un saut dans la matière.
    pub fn remettre(&mut self) {
        self.s.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOC: usize = 128;

    fn sinus(hz: f64, secondes: f64) -> Vec<f32> {
        let n = (SR as f64 * secondes) as usize;
        (0..n)
            .flat_map(|i| {
                let v =
                    (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / SR as f64).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    /// Fréquence par passages par zéro (interpolés), sur la moitié centrale.
    fn frequence(x: &[f32]) -> f64 {
        let g: Vec<f32> = x.iter().step_by(CANAUX).copied().collect();
        let (a, b) = (g.len() / 4, 3 * g.len() / 4);
        let mut passages = Vec::new();
        for i in a + 1..b {
            if g[i - 1] <= 0.0 && g[i] > 0.0 {
                let t = (i - 1) as f64 + (-g[i - 1] / (g[i] - g[i - 1])) as f64;
                passages.push(t);
            }
        }
        let n = passages.len();
        (n - 1) as f64 * SR as f64 / (passages[n - 1] - passages[0])
    }

    fn en_flux(t: &mut Transpositeur, x: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; x.len()];
        for (e, s) in x.chunks(BLOC * CANAUX).zip(out.chunks_mut(BLOC * CANAUX)) {
            t.traiter(e, s);
        }
        out
    }

    /// Le retard annoncé est celui qu'on mesure : du bruit à zéro demi-ton,
    /// corrélé à lui-même décalé.
    #[test]
    fn la_latence_annoncee_est_la_vraie() {
        let mut graine = 0x2545_F491_4F6C_DD1Du64;
        let x: Vec<f32> = (0..SR as usize * 2)
            .flat_map(|_| {
                graine ^= graine >> 12;
                graine ^= graine << 25;
                graine ^= graine >> 27;
                let v =
                    (graine.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 8_388_608.0 - 1.0;
                [0.5 * v, 0.5 * v]
            })
            .collect();
        let mut t = Transpositeur::new();
        t.regler(0.0);
        let y = en_flux(&mut t, &x);
        let l = t.latence();
        let g: Vec<f32> = x.iter().step_by(CANAUX).copied().collect();
        let h: Vec<f32> = y.iter().step_by(CANAUX).copied().collect();
        let correlation = |d: usize| -> f64 {
            (SR as usize / 2..SR as usize)
                .map(|i| g[i] as f64 * h[i + d] as f64)
                .sum()
        };
        let meilleur = (0..2 * l).max_by(|a, b| correlation(*a).total_cmp(&correlation(*b)));
        assert_eq!(meilleur, Some(l), "latence annoncée {l}");
    }

    fn cents(mesure: f64, attendu: f64) -> f64 {
        1200.0 * (mesure / attendu).log2()
    }

    /// Le critère d'E6 : |erreur| ≤ 1 cent. **Pas tenu par le préréglage**
    /// (mesuré le 9 nov. : jusqu'à +8 cents à 110 Hz, +4 à 441 Hz, ≤ 1,6 à
    /// 1 kHz ; +22 à 82 Hz) — le biais vient de l'estimation des pics de
    /// Signalsmith (centroïde d'énergie), il dépend de la fréquence et ne se
    /// compense pas par un facteur. Ignoré tant que le réglage n'est pas
    /// choisi : voir [`exploration`] et le journal (E6).
    #[test]
    #[ignore]
    fn justesse_sur_sinusoide() {
        let mut ecarts = Vec::new();
        for hz in [110.0, 441.0, 1000.0] {
            let x = sinus(hz, 4.0);
            for d in [-5.0f32, -2.0, -1.0, 1.0, 2.0, 5.0] {
                let mut t = Transpositeur::new();
                t.regler(d);
                let y = en_flux(&mut t, &x);
                let attendu = hz * 2f64.powf(d as f64 / 12.0);
                ecarts.push((hz, d, cents(frequence(&y), attendu)));
            }
        }
        for e in &ecarts {
            eprintln!("{} Hz, {:+} demi-tons : {:+.2} cents", e.0, e.1, e.2);
        }
        assert!(ecarts.iter().all(|e| e.2.abs() <= 1.0), "{ecarts:?}");
    }
}

/// Justesse selon le bloc d'analyse et le pas, sur sinusoïdes (82 Hz à 1 kHz)
/// et sur une « basse » à 8 harmoniques. Les chiffres du 9 nov. sont au
/// journal (E6).
///
/// ```bash
/// cargo test --release -p rusty-music-player --features hauteur --lib exploration -- --ignored --nocapture
/// ```
#[cfg(test)]
mod exploration {
    use super::*;
    #[test]
    #[ignore]
    fn reglages() {
        let sin = |hz: f64| -> Vec<f32> {
            (0..(SR as usize * 4))
                .flat_map(|i| {
                    let v = (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / SR as f64).sin())
                        as f32;
                    [v, v]
                })
                .collect()
        };
        let freq = |x: &[f32]| -> f64 {
            let g: Vec<f32> = x.iter().step_by(2).copied().collect();
            let (a, b) = (g.len() / 4, 3 * g.len() / 4);
            let mut p = Vec::new();
            for i in a + 1..b {
                if g[i - 1] <= 0.0 && g[i] > 0.0 {
                    p.push((i - 1) as f64 + (-g[i - 1] / (g[i] - g[i - 1])) as f64);
                }
            }
            (p.len() - 1) as f64 * SR as f64 / (p[p.len() - 1] - p[0])
        };
        // Pic spectral : DFT fenêtrée (Hann) sur la moitié centrale, balayée
        // à 0,05 cent autour de la fréquence attendue.
        let pic = |x: &[f32], attendu: f64| -> f64 {
            let g: Vec<f64> = x.iter().step_by(2).map(|v| *v as f64).collect();
            let (a, b) = (g.len() / 4, 3 * g.len() / 4);
            let n = b - a;
            let w: Vec<f64> = (0..n)
                .map(|i| {
                    g[a + i]
                        * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
                })
                .collect();
            let mut meilleur = (0.0, attendu);
            let mut c = -40.0;
            while c <= 40.0 {
                let f = attendu * 2f64.powf(c / 1200.0);
                let (mut re, mut im) = (0.0, 0.0);
                let dphi = 2.0 * std::f64::consts::PI * f / SR as f64;
                for (i, v) in w.iter().enumerate() {
                    let ph = dphi * i as f64;
                    re += v * ph.cos();
                    im += v * ph.sin();
                }
                let m = re * re + im * im;
                if m > meilleur.0 {
                    meilleur = (m, f);
                }
                c += 0.25;
            }
            meilleur.1
        };
        {
            let x = sin(110.0);
            eprintln!(
                "contrôle entrée 110 Hz : zéros {:+.2} c, pic {:+.2} c",
                1200.0 * (freq(&x) / 110.0).log2(),
                1200.0 * (pic(&x, 110.0) / 110.0).log2()
            );
        }
        // Une « basse » : dents de scie à 8 harmoniques (amplitude 1/k).
        let scie = |hz: f64| -> Vec<f32> {
            (0..(SR as usize * 4))
                .flat_map(|i| {
                    let t = i as f64 / SR as f64;
                    let v: f64 = (1..=8)
                        .map(|k| {
                            0.3 / k as f64 * (2.0 * std::f64::consts::PI * hz * k as f64 * t).sin()
                        })
                        .sum();
                    [v as f32, v as f32]
                })
                .collect()
        };
        for (bloc, pas) in [(5292usize, 1323usize), (8192, 512)] {
            let mut ligne = String::new();
            for hz in [41.2, 55.0, 82.4, 110.0] {
                let x = scie(hz);
                for d in [-2.0f32, 2.0] {
                    let mut s = Stretch::new(2, bloc, pas);
                    s.set_transpose_factor_semitones(d, None);
                    let mut y = vec![0.0; x.len()];
                    for (e, o) in x.chunks(256).zip(y.chunks_mut(256)) {
                        s.process(e, o);
                    }
                    let att = hz * 2f64.powf(d as f64 / 12.0);
                    let h: Vec<String> = (1..=4)
                        .map(|k| {
                            let a = att * k as f64;
                            format!("{:+.1}", 1200.0 * (pic(&y, a) / a).log2())
                        })
                        .collect();
                    ligne += &format!("[{hz} {d:+}: {}] ", h.join(" "));
                }
            }
            eprintln!("scie, bloc {bloc} pas {pas} (harmoniques 1-4, cents) : {ligne}");
        }
        for (bloc, pas) in [
            (5292usize, 1323usize),
            (8192, 512),
            (8192, 256),
            (16384, 512),
            (16384, 1024),
        ] {
            let mut pire: f64 = 0.0;
            let mut ligne = String::new();
            for hz in [82.4, 110.0, 220.0, 441.0, 1000.0] {
                let x = sin(hz);
                for d in [-5.0f32, -2.0, -1.0, 1.0, 2.0, 5.0] {
                    let mut s = Stretch::new(2, bloc, pas);
                    s.set_transpose_factor_semitones(d, None);
                    let mut y = vec![0.0; x.len()];
                    for (e, o) in x.chunks(256).zip(y.chunks_mut(256)) {
                        s.process(e, o);
                    }
                    let att = hz * 2f64.powf(d as f64 / 12.0);
                    let c = 1200.0 * (pic(&y, att) / att).log2();
                    let _ = freq(&y);
                    pire = pire.max(c.abs());
                    ligne += &format!("{c:+.1} ");
                }
            }
            eprintln!(
                "bloc {bloc} pas {pas} latence {} : pire {pire:.2} | {ligne}",
                Stretch::new(2, bloc, pas).input_latency()
                    + Stretch::new(2, bloc, pas).output_latency()
            );
        }
    }
}
