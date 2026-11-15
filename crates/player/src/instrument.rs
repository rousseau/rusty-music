// SPDX-License-Identifier: GPL-3.0-or-later
//! Le son de l'instrument joué pendant la pratique (E7) : la chaîne qui
//! traite l'entrée de la basse avant de la mêler au morceau.
//!
//! Une chaîne = jusqu'à [`MAX_FILTRES`] filtres d'égalisation (biquads de la
//! crate `biquad`, pas réécrits ici), un compresseur à détection de crête, une
//! saturation douce (`tanh`, décalée pour l'asymétrie d'un ampli à lampes),
//! un filtre après saturation (le baffle), un gain de sortie qui **compense le
//! volume** (même niveau efficace que le signal brut sur un signal de basse de
//! référence), puis un limiteur doux : jamais au-delà de ±1, quel que soit
//! l'entrée.
//!
//! [`Chaine::traiter`] tourne dans le rappel audio : ni allocation, ni verrou
//! (`tests/instrument_sans_allocation.rs` le contrôle). Tout ce qui alloue ou
//! coûte (coefficients, compensation) se fait dans [`Chaine::new`].
//!
//! Le coach et l'accordeur analysent le signal **brut**, jamais celui-ci.
//! La structure prévoit d'autres instruments ([`Instrument`]) ; seule la basse
//! a ses préréglages aujourd'hui.

use biquad::{Biquad, Coefficients, DirectForm2Transposed, Type};

/// Filtres d'égalisation au plus, par chaîne.
pub const MAX_FILTRES: usize = 5;

/// L'instrument branché.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Instrument {
    Basse,
}

impl Instrument {
    pub fn nom(self) -> &'static str {
        match self {
            Instrument::Basse => "basse",
        }
    }
    pub fn analyser(s: &str) -> Option<Self> {
        (s == "basse").then_some(Instrument::Basse)
    }
    /// Ses préréglages, dans l'ordre où l'interface les montre.
    pub fn prereglages(self) -> &'static [Prereglage] {
        match self {
            Instrument::Basse => &Prereglage::BASSE,
        }
    }
}

/// Un préréglage de son.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prereglage {
    Brut,
    Rond,
    Funky,
    Slap,
    Crunch,
    Overdrive,
    Vintage,
    /// L'égaliseur approché d'après le stem de basse du morceau
    /// ([`Reglage::comme_le_disque`]) ; sans stem, comme [`Prereglage::Brut`].
    CommeLeDisque,
}

impl Prereglage {
    pub const BASSE: [Prereglage; 8] = [
        Prereglage::Brut,
        Prereglage::Rond,
        Prereglage::Funky,
        Prereglage::Slap,
        Prereglage::Crunch,
        Prereglage::Overdrive,
        Prereglage::Vintage,
        Prereglage::CommeLeDisque,
    ];

    /// Identifiant stable (base, commandes).
    pub fn nom(self) -> &'static str {
        match self {
            Prereglage::Brut => "brut",
            Prereglage::Rond => "rond",
            Prereglage::Funky => "funky",
            Prereglage::Slap => "slap",
            Prereglage::Crunch => "crunch",
            Prereglage::Overdrive => "overdrive",
            Prereglage::Vintage => "vintage",
            Prereglage::CommeLeDisque => "comme-le-disque",
        }
    }

    /// Ce que l'interface affiche.
    pub fn libelle(self) -> &'static str {
        match self {
            Prereglage::Brut => "Brut",
            Prereglage::Rond => "Rond",
            Prereglage::Funky => "Funky",
            Prereglage::Slap => "Slap",
            Prereglage::Crunch => "Crunch",
            Prereglage::Overdrive => "Overdrive",
            Prereglage::Vintage => "Vintage",
            Prereglage::CommeLeDisque => "Comme le disque",
        }
    }

    pub fn analyser(s: &str) -> Option<Self> {
        Prereglage::BASSE.into_iter().find(|p| p.nom() == s)
    }
}

/// Un filtre d'égalisation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filtre {
    /// Étagère grave : `db` sous `hz`.
    Grave {
        hz: f32,
        db: f32,
    },
    /// Étagère aiguë : `db` au-dessus de `hz`.
    Aigu {
        hz: f32,
        db: f32,
    },
    /// Cloche centrée sur `hz`.
    Cloche {
        hz: f32,
        db: f32,
        q: f32,
    },
    PasseBas {
        hz: f32,
    },
    PasseHaut {
        hz: f32,
    },
}

impl Filtre {
    fn coefficients(self, sr: f32) -> Coefficients<f32> {
        let nyquist = sr * 0.49;
        let (t, hz, q) = match self {
            Filtre::Grave { hz, db } => (Type::LowShelf(db), hz, 0.707),
            Filtre::Aigu { hz, db } => (Type::HighShelf(db), hz, 0.707),
            Filtre::Cloche { hz, db, q } => (Type::PeakingEQ(db), hz, q),
            Filtre::PasseBas { hz } => (Type::LowPass, hz, 0.707),
            Filtre::PasseHaut { hz } => (Type::HighPass, hz, 0.707),
        };
        let normalise = (hz.clamp(10.0, nyquist)) / (sr / 2.0);
        Coefficients::from_normalized_params(t, normalise, q.max(0.1))
            .expect("fréquence bornée sous Nyquist, Q positif")
    }
}

/// Compresseur à détection de crête.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compresseur {
    pub seuil_db: f32,
    pub ratio: f32,
    pub attaque_ms: f32,
    pub relache_ms: f32,
}

/// Saturation douce : `drive` 0 = aucune ; `asymetrie` > 0 ajoute des
/// harmoniques paires (lampes).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Saturation {
    pub drive: f32,
    pub asymetrie: f32,
}

/// Les réglages d'une chaîne, sans état.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Reglage {
    pub filtres: Vec<Filtre>,
    pub compresseur: Option<Compresseur>,
    pub saturation: Saturation,
    /// Après la saturation : le baffle, qui ôte le grésillement.
    pub apres: Option<Filtre>,
}

impl Reglage {
    /// Le réglage d'un préréglage. [`Prereglage::CommeLeDisque`] rend ici le
    /// son brut : il lui faut le stem ([`Reglage::comme_le_disque`]).
    pub fn de(p: Prereglage) -> Self {
        use Filtre::*;
        let comp = |seuil_db, ratio, attaque_ms, relache_ms| {
            Some(Compresseur {
                seuil_db,
                ratio,
                attaque_ms,
                relache_ms,
            })
        };
        match p {
            Prereglage::Brut | Prereglage::CommeLeDisque => Reglage::default(),
            Prereglage::Rond => Reglage {
                filtres: vec![
                    Grave { hz: 120.0, db: 4.0 },
                    Cloche {
                        hz: 800.0,
                        db: -2.0,
                        q: 0.8,
                    },
                    PasseBas { hz: 2200.0 },
                ],
                compresseur: comp(-24.0, 2.5, 15.0, 200.0),
                ..Default::default()
            },
            Prereglage::Funky => Reglage {
                filtres: vec![
                    PasseHaut { hz: 35.0 },
                    Grave { hz: 90.0, db: 2.0 },
                    Cloche {
                        hz: 1500.0,
                        db: 4.0,
                        q: 1.0,
                    },
                    Aigu {
                        hz: 4000.0,
                        db: 2.0,
                    },
                ],
                compresseur: comp(-26.0, 4.0, 5.0, 120.0),
                ..Default::default()
            },
            Prereglage::Slap => Reglage {
                filtres: vec![
                    PasseHaut { hz: 35.0 },
                    Grave { hz: 80.0, db: 4.0 },
                    Cloche {
                        hz: 500.0,
                        db: -7.0,
                        q: 0.9,
                    },
                    Aigu {
                        hz: 3000.0,
                        db: 6.0,
                    },
                ],
                compresseur: comp(-22.0, 4.0, 2.0, 100.0),
                ..Default::default()
            },
            Prereglage::Crunch => Reglage {
                filtres: vec![
                    PasseHaut { hz: 40.0 },
                    Cloche {
                        hz: 900.0,
                        db: 3.0,
                        q: 0.8,
                    },
                ],
                compresseur: comp(-20.0, 2.0, 10.0, 150.0),
                saturation: Saturation {
                    drive: 3.0,
                    asymetrie: 0.1,
                },
                apres: Some(PasseBas { hz: 5000.0 }),
            },
            Prereglage::Overdrive => Reglage {
                filtres: vec![
                    PasseHaut { hz: 50.0 },
                    Cloche {
                        hz: 700.0,
                        db: 5.0,
                        q: 0.7,
                    },
                ],
                compresseur: comp(-18.0, 3.0, 8.0, 150.0),
                saturation: Saturation {
                    drive: 10.0,
                    asymetrie: 0.15,
                },
                apres: Some(PasseBas { hz: 3500.0 }),
            },
            Prereglage::Vintage => Reglage {
                filtres: vec![
                    Grave { hz: 100.0, db: 3.0 },
                    Cloche {
                        hz: 400.0,
                        db: 2.0,
                        q: 0.7,
                    },
                    PasseBas { hz: 1800.0 },
                ],
                compresseur: comp(-20.0, 3.0, 20.0, 300.0),
                saturation: Saturation {
                    drive: 1.8,
                    asymetrie: 0.3,
                },
                apres: Some(PasseBas { hz: 2500.0 }),
            },
        }
    }

    /// « Comme le disque » : un égaliseur à [`BANDES_HZ`].len() cloches qui
    /// rapproche le spectre de la basse jouée de celui du stem de basse
    /// `stem` (mono, `sr` Hz). Le spectre de départ est celui d'une basse
    /// directe (le signal de référence) ; gains bornés à ±9 dB.
    pub fn comme_le_disque(stem: &[f32], sr: f32) -> Self {
        let cible = profil_bandes(stem, sr);
        let reference = profil_bandes(&signal_reference(sr, 4.0), sr);
        let filtres = BANDES_HZ
            .iter()
            .zip(cible.iter().zip(reference))
            .map(|(&hz, (&c, r))| Filtre::Cloche {
                hz,
                db: (c - r).clamp(-9.0, 9.0),
                q: 1.0,
            })
            .collect();
        Reglage {
            filtres,
            ..Default::default()
        }
    }
}

/// Centres des bandes de « comme le disque ».
pub const BANDES_HZ: [f32; 5] = [60.0, 150.0, 400.0, 1000.0, 2500.0];

/// L'énergie de `signal` dans chaque bande de [`BANDES_HZ`], en dB relatifs
/// à leur moyenne (le niveau d'ensemble ne compte pas).
pub fn profil_bandes(signal: &[f32], sr: f32) -> [f32; BANDES_HZ.len()] {
    let mut db = [0.0f32; BANDES_HZ.len()];
    for (sortie, &hz) in db.iter_mut().zip(&BANDES_HZ) {
        let c =
            Coefficients::from_normalized_params(Type::BandPass, (hz / (sr / 2.0)).min(0.98), 1.4)
                .expect("bande sous Nyquist");
        let mut f = DirectForm2Transposed::<f32>::new(c);
        let e: f64 = signal
            .iter()
            .map(|&x| f64::from(f.run(x)).powi(2))
            .sum::<f64>()
            / signal.len().max(1) as f64;
        *sortie = 10.0 * (e.max(1e-20)).log10() as f32;
    }
    let moyenne = db.iter().sum::<f32>() / db.len() as f32;
    db.map(|d| d - moyenne)
}

/// Un signal de basse directe synthétique, déterministe : des notes pincées
/// (fondamentale de 41 à 98 Hz, harmoniques décroissantes, attaque brève et
/// extinction), autour de −18 dBFS efficaces. Il sert à compenser le volume
/// des préréglages et de spectre de départ à « comme le disque ».
pub fn signal_reference(sr: f32, secondes: f32) -> Vec<f32> {
    let n = (sr * secondes) as usize;
    (0..n).map(|i| echantillon_reference(i, sr)).collect()
}

fn echantillon_reference(i: usize, sr: f32) -> f32 {
    const NOTES: [f32; 4] = [41.2, 55.0, 73.4, 98.0];
    let duree_note = (sr * 0.5) as usize;
    let k = i / duree_note;
    let t = (i % duree_note) as f32 / sr;
    let f0 = NOTES[k % NOTES.len()];
    // La deuxième note de chaque mesure est jouée plus doucement : de la
    // dynamique pour le compresseur.
    let force = if k.is_multiple_of(2) { 1.0 } else { 0.4 };
    let enveloppe = (1.0 - (-t / 0.004).exp()) * (-t / 0.35).exp();
    let mut x = 0.0;
    for h in 1..=8 {
        let a = 1.0 / (h as f32).powf(1.2);
        x += a * (std::f32::consts::TAU * f0 * h as f32 * t).sin();
    }
    0.18 * force * enveloppe * x
}

/// Une chaîne prête à tourner dans le rappel audio.
#[derive(Clone)]
pub struct Chaine {
    filtres: [DirectForm2Transposed<f32>; MAX_FILTRES],
    n_filtres: usize,
    compresseur: Option<EtatCompresseur>,
    saturation: Saturation,
    /// Gain de petit signal de la saturation, pour la ramener à 1.
    norme_saturation: f32,
    decalage_saturation: f32,
    apres: Option<DirectForm2Transposed<f32>>,
    gain_sortie: f32,
}

#[derive(Clone, Copy)]
struct EtatCompresseur {
    seuil_db: f32,
    pente: f32,
    attaque: f32,
    relache: f32,
    enveloppe: f32,
}

impl Chaine {
    /// Construit la chaîne de `reglage` à `sr` Hz, volume compensé sur le
    /// signal de référence. Alloue : hors du rappel audio.
    pub fn new(reglage: &Reglage, sr: f32) -> Self {
        let mut c = Self::sans_compensation(reglage, sr);
        let mut essai = c.clone();
        let (mut e_in, mut e_out) = (0.0f64, 0.0f64);
        for i in 0..(sr * 4.0) as usize {
            let x = echantillon_reference(i, sr);
            let y = essai.avant_limiteur(x);
            e_in += f64::from(x).powi(2);
            e_out += f64::from(y).powi(2);
        }
        c.gain_sortie = if e_out > 1e-12 {
            ((e_in / e_out).sqrt() as f32).clamp(0.05, 20.0)
        } else {
            1.0
        };
        c
    }

    fn sans_compensation(reglage: &Reglage, sr: f32) -> Self {
        let neutre = Coefficients {
            a1: 0.0,
            a2: 0.0,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
        };
        let mut filtres = [DirectForm2Transposed::<f32>::new(neutre); MAX_FILTRES];
        let n_filtres = reglage.filtres.len().min(MAX_FILTRES);
        for (f, r) in filtres.iter_mut().zip(&reglage.filtres) {
            *f = DirectForm2Transposed::new(r.coefficients(sr));
        }
        let coef = |ms: f32| (-1.0 / (ms.max(0.1) * 1e-3 * sr)).exp();
        let compresseur = reglage.compresseur.map(|c| EtatCompresseur {
            seuil_db: c.seuil_db,
            pente: 1.0 - 1.0 / c.ratio.max(1.0),
            attaque: coef(c.attaque_ms),
            relache: coef(c.relache_ms),
            enveloppe: 0.0,
        });
        let s = reglage.saturation;
        let decalage = s.asymetrie;
        let norme = if s.drive > 0.0 {
            let th = (s.drive * decalage).tanh();
            1.0 / (s.drive * (1.0 - th * th))
        } else {
            1.0
        };
        Chaine {
            filtres,
            n_filtres,
            compresseur,
            saturation: s,
            norme_saturation: norme,
            decalage_saturation: decalage,
            apres: reglage
                .apres
                .map(|f| DirectForm2Transposed::new(f.coefficients(sr))),
            gain_sortie: 1.0,
        }
    }

    /// Le gain de sortie qui compense le volume (linéaire).
    pub fn gain_sortie(&self) -> f32 {
        self.gain_sortie
    }

    /// Traite `tampon` (mono) sur place. Sans allocation : pour le rappel
    /// audio.
    pub fn traiter(&mut self, tampon: &mut [f32]) {
        for x in tampon.iter_mut() {
            *x = limiteur(self.gain_sortie * self.avant_limiteur(*x));
        }
    }

    /// Remet les états à zéro (changement de préréglage, reprise).
    pub fn vider(&mut self) {
        for f in &mut self.filtres {
            f.reset_state();
        }
        if let Some(f) = &mut self.apres {
            f.reset_state();
        }
        if let Some(c) = &mut self.compresseur {
            c.enveloppe = 0.0;
        }
    }

    #[inline]
    fn avant_limiteur(&mut self, x: f32) -> f32 {
        let mut y = if x.is_finite() { x } else { 0.0 };
        for f in &mut self.filtres[..self.n_filtres] {
            y = f.run(y);
        }
        if let Some(c) = &mut self.compresseur {
            let niveau = y.abs();
            let k = if niveau > c.enveloppe {
                c.attaque
            } else {
                c.relache
            };
            c.enveloppe = k * c.enveloppe + (1.0 - k) * niveau;
            let env_db = 20.0 * c.enveloppe.max(1e-6).log10();
            if env_db > c.seuil_db {
                y *= 10f32.powf(-(env_db - c.seuil_db) * c.pente / 20.0);
            }
        }
        let s = self.saturation;
        if s.drive > 0.0 {
            let b = self.decalage_saturation;
            y = ((s.drive * (y + b)).tanh() - (s.drive * b).tanh()) * self.norme_saturation;
        }
        if let Some(f) = &mut self.apres {
            y = f.run(y);
        }
        y
    }
}

/// Limiteur doux : linéaire jusqu'à 0,8, puis courbe vers 1 sans l'atteindre.
#[inline]
fn limiteur(y: f32) -> f32 {
    const GENOU: f32 = 0.8;
    let a = y.abs();
    if a <= GENOU {
        y
    } else {
        y.signum() * (GENOU + (1.0 - GENOU) * ((a - GENOU) / (1.0 - GENOU)).tanh()).min(0.999)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }
    fn db(x: f32) -> f32 {
        20.0 * x.max(1e-12).log10()
    }
    fn sinus(hz: f32, ampl: f32, secondes: f32) -> Vec<f32> {
        (0..(SR * secondes) as usize)
            .map(|i| ampl * (std::f32::consts::TAU * hz * i as f32 / SR).sin())
            .collect()
    }
    fn traiter(p: Prereglage, x: &[f32]) -> Vec<f32> {
        let mut c = Chaine::new(&Reglage::de(p), SR);
        let mut y = x.to_vec();
        c.traiter(&mut y);
        y
    }
    /// Gain d'un préréglage à `hz`, sinus faible (hors compresseur et
    /// saturation), en dB, régime établi.
    fn gain_a(p: Prereglage, hz: f32) -> f32 {
        let x = sinus(hz, 0.01, 1.0);
        let y = traiter(p, &x);
        let moitie = x.len() / 2;
        db(rms(&y[moitie..]) / rms(&x[moitie..]))
    }

    #[test]
    fn les_noms_font_l_aller_retour() {
        assert_eq!(Instrument::Basse.prereglages().len(), 8);
        for p in Prereglage::BASSE {
            assert_eq!(Prereglage::analyser(p.nom()), Some(p));
        }
        assert_eq!(Instrument::analyser("basse"), Some(Instrument::Basse));
        assert_eq!(Prereglage::analyser("inconnu"), None);
    }

    #[test]
    fn brut_rend_le_signal_tel_quel() {
        let x = signal_reference(SR, 2.0);
        let y = traiter(Prereglage::Brut, &x);
        let ecart = x
            .iter()
            .zip(&y)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(ecart < 1e-6, "{ecart}");
    }

    #[test]
    fn chaque_prereglage_garde_le_volume_du_brut() {
        let x = signal_reference(SR, 4.0);
        for p in Prereglage::BASSE {
            let y = traiter(p, &x);
            let ecart = db(rms(&y) / rms(&x));
            assert!(ecart.abs() < 1.0, "{} : {ecart:.2} dB", p.nom());
        }
    }

    #[test]
    fn jamais_de_saturation_numerique() {
        // Pleine échelle, accord grave et aigu : le pire cas.
        let x: Vec<f32> = sinus(55.0, 0.7, 1.0)
            .iter()
            .zip(sinus(3000.0, 0.3, 1.0))
            .map(|(a, b)| a + b)
            .collect();
        for p in Prereglage::BASSE {
            let y = traiter(p, &x);
            let crete = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(crete < 1.0, "{} : crête {crete}", p.nom());
            assert!(y.iter().all(|v| v.is_finite()));
        }
        // Une entrée qui n'est pas un nombre ne contamine pas la chaîne.
        let mut c = Chaine::new(&Reglage::de(Prereglage::Funky), SR);
        let mut t = vec![0.1, f32::NAN, f32::INFINITY, 0.1];
        c.traiter(&mut t);
        assert!(t.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn le_silence_reste_silencieux() {
        for p in Prereglage::BASSE {
            let y = traiter(p, &[0.0; 4800]);
            assert!(y.iter().all(|v| v.abs() < 1e-6), "{}", p.nom());
        }
    }

    #[test]
    fn rond_coupe_les_aigus_et_slap_creuse_les_mediums() {
        let pente = |p| gain_a(p, 3000.0) - gain_a(p, 80.0);
        assert!(
            pente(Prereglage::Rond) < -8.0,
            "{}",
            pente(Prereglage::Rond)
        );
        assert!(
            pente(Prereglage::Vintage) < -8.0,
            "{}",
            pente(Prereglage::Vintage)
        );
        let creux = gain_a(Prereglage::Slap, 500.0);
        assert!(creux < gain_a(Prereglage::Slap, 80.0) - 6.0);
        assert!(creux < gain_a(Prereglage::Slap, 5000.0) - 6.0);
        assert!(gain_a(Prereglage::Funky, 1500.0) > gain_a(Prereglage::Funky, 400.0) + 2.0);
    }

    /// Part d'énergie hors de la fondamentale (sinus de 110 Hz à −12 dBFS).
    fn distorsion(p: Prereglage) -> f32 {
        let hz = 110.0;
        let x = sinus(hz, 0.25, 1.0);
        let y = traiter(p, &x);
        let y = &y[y.len() / 2..];
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in y.iter().enumerate() {
            let w = std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(SR);
            re += f64::from(*v) * w.cos();
            im += f64::from(*v) * w.sin();
        }
        let fond = 2.0 * (re * re + im * im) / (y.len() as f64).powi(2);
        let total = y.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / y.len() as f64;
        ((total - fond).max(0.0) / total) as f32
    }

    #[test]
    fn la_saturation_croit_de_brut_a_overdrive() {
        let (b, c, o) = (
            distorsion(Prereglage::Brut),
            distorsion(Prereglage::Crunch),
            distorsion(Prereglage::Overdrive),
        );
        assert!(b < 1e-4, "brut {b}");
        assert!(c > 0.005 && o > 2.0 * c, "crunch {c}, overdrive {o}");
    }

    #[test]
    fn funky_resserre_la_dynamique() {
        // Notes fortes et douces du signal de référence : l'écart se réduit.
        let x = signal_reference(SR, 2.0);
        let note = (SR * 0.5) as usize;
        let ecart = |s: &[f32]| db(rms(&s[..note]) / rms(&s[note..2 * note]));
        let y = traiter(Prereglage::Funky, &x);
        assert!(
            ecart(&y) < ecart(&x) - 2.0,
            "{} contre {}",
            ecart(&y),
            ecart(&x)
        );
    }

    #[test]
    fn comme_le_disque_suit_le_stem() {
        // Un « stem » : la référence, graves relevés de 8 dB.
        let mut f = DirectForm2Transposed::<f32>::new(
            Filtre::Grave { hz: 100.0, db: 8.0 }.coefficients(SR),
        );
        let stem: Vec<f32> = signal_reference(SR, 4.0)
            .into_iter()
            .map(|x| f.run(x))
            .collect();
        let r = Reglage::comme_le_disque(&stem, SR);
        let gains: Vec<f32> = r
            .filtres
            .iter()
            .map(|f| match f {
                Filtre::Cloche { db, .. } => *db,
                _ => unreachable!(),
            })
            .collect();
        assert!(gains[0] > gains[4] + 4.0, "{gains:?}");
        // Le stem identique à la référence : à plat.
        let plat = Reglage::comme_le_disque(&signal_reference(SR, 4.0), SR);
        for f in &plat.filtres {
            if let Filtre::Cloche { db, .. } = f {
                assert!(db.abs() < 0.1, "{db}");
            }
        }
    }
}
