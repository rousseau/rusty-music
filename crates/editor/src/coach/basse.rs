// SPDX-License-Identifier: GPL-3.0-or-later
//! La basse en DI (E5, étape 5 du coach, `docs/recherche-coach-pratique.md`,
//! « La basse en DI : onset immédiat, hauteur 50 à 65 ms plus tard »).
//!
//! Deux temps, parce que la physique l'impose : le Si grave d'une basse à
//! cinq cordes (B0, 30,9 Hz) a une période de 32 ms, et tout estimateur de
//! hauteur en veut deux.
//!
//! 1. **L'attaque**, tout de suite : un flux spectral à fenêtre courte
//!    (≈ 11 ms, pas de ≈ 3 ms) sur la DI, compressé en log, avec un seuil
//!    qui suit la médiane du passé et un pic local d'une trame d'avance.
//! 2. **La hauteur**, différée : pYIN sur une trame qui commence 12 ms après
//!    l'attaque (le transitoire passé) et couvre 24 ms d'intégration plus une
//!    période de 30 Hz ; elle arrive ≈ 70 ms après l'attaque. C'est la
//!    première étape de pYIN (Mauch & Dixon 2014 : distribution bêta des
//!    seuils, probabilité de chaque creux), sans le HMM qui lisserait vers
//!    le futur : causale, trame par trame.
//!
//! Les harmoniques d'une basse sont fortes, et l'erreur classique d'un
//! estimateur est l'octave. Deux contrôles par l'énergie, sur une fenêtre
//! rectangulaire d'un nombre entier de périodes (les harmoniques de f0 y sont
//! orthogonales à f0/2) : une fondamentale détectée sans énergie est un
//! sous-octave de l'estimateur, corrigé ; et une note jouée une octave
//! au-dessus de la référence n'est rapportée « mauvaise octave » que si
//! l'énergie à la fondamentale attendue est faible.
//!
//! La référence se vérifie **avec le même analyseur** sur le stem de basse
//! démixé ([`confiance_reference`]) : une note de la transcription que
//! l'analyseur ne retrouve pas dans le stem n'est pas notée (Benetos,
//! Klapuri et Dixon 2012). Le jugement ([`juger`]) reprend l'apparieur de la
//! batterie (une seule voix) et y ajoute la hauteur.

use std::collections::VecDeque;
use std::sync::Arc;

use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use serde::{Deserialize, Serialize};

use super::appariement::{apparier, Fenetres, Jugement, Reference, Verdict};
use super::batterie::{Classe, Frappe};
use super::Horloge;

/// Fréquence la plus grave cherchée : sous le Si grave (B0, 30,9 Hz).
pub const F_MIN_HZ: f32 = 30.0;
/// La plus aiguë : la 24ᵉ case de la corde de sol (G4, 392 Hz), avec marge.
pub const F_MAX_HZ: f32 = 440.0;
/// Début de la trame de hauteur après l'attaque : le transitoire passé.
pub const DECALAGE_HAUTEUR_S: f32 = 0.012;
/// Fenêtre d'intégration de YIN.
pub const FENETRE_HAUTEUR_S: f32 = 0.024;
/// Deux attaques plus proches sont la même note.
pub const ECART_MIN_S: f32 = 0.060;
/// Sous ce niveau (dBFS, RMS de la trame), aucune attaque.
pub const SILENCE_DBFS: f32 = -55.0;
/// Probabilité de voisement de pYIN sous laquelle une hauteur jouée est
/// inconnue : rien n'est dit sur elle.
pub const CONFIANCE_JEU: f32 = 0.3;
/// Confiance (même mesure, sur le stem) sous laquelle une note de la
/// référence n'est pas notée.
pub const CONFIANCE_SURE: f32 = 0.5;
/// Rapport d'énergie E(f0/2)/E(f0) au-dessus duquel la fondamentale une
/// octave plus bas est présente.
pub const SOUS_OCTAVE_PRESENTE: f32 = 0.1;

/// Le nombre de MIDI d'une fréquence (69 = la 440).
pub fn midi(f_hz: f32) -> f32 {
    69.0 + 12.0 * (f_hz / 440.0).log2()
}

fn hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

/// Une note de la DI : l'attaque, puis la hauteur.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoteDI {
    /// Instant de l'attaque, en secondes de l'horloge commune.
    pub t_s: f64,
    /// Fondamentale, si pYIN en a trouvé une.
    pub f0_hz: Option<f32>,
    /// Probabilité de voisement de pYIN (0 à 1).
    pub confiance: f32,
    /// E(f0/2)/E(f0) : la fondamentale une octave plus bas est-elle là ?
    pub sous_octave: f32,
    /// Niveau RMS de la trame d'attaque, en dBFS.
    pub niveau_db: f32,
}

impl NoteDI {
    pub fn hauteur(&self) -> Option<f32> {
        self.f0_hz.map(midi)
    }
}

/// Ce que l'analyseur rend : l'attaque dès qu'elle est sûre, la note
/// complète (même instant) quand sa hauteur est mesurée.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Evenement {
    Attaque { t_s: f64, niveau_db: f32 },
    Note(NoteDI),
}

/// pYIN, première étape, sur une trame : la fondamentale la plus probable et
/// la probabilité de voisement. `x` doit couvrir la fenêtre d'intégration
/// plus la plus longue période.
pub fn pyin_trame(x: &[f32], sr: f32) -> Option<(f32, f32)> {
    let w = (sr * FENETRE_HAUTEUR_S) as usize;
    let tau_max = (sr / F_MIN_HZ).ceil() as usize;
    let tau_min = ((sr / F_MAX_HZ).floor() as usize).max(2);
    if x.len() < w + tau_max + 1 {
        return None;
    }
    // Différence et différence normalisée cumulée (YIN, étapes 2 et 3).
    let mut d = vec![0f32; tau_max + 1];
    for (tau, dt) in d.iter_mut().enumerate().skip(1) {
        let mut s = 0f64;
        for j in 0..w {
            let e = (x[j] - x[j + tau]) as f64;
            s += e * e;
        }
        *dt = s as f32;
    }
    let mut c = vec![1f32; tau_max + 1];
    let mut cumul = 0f64;
    for tau in 1..=tau_max {
        cumul += d[tau] as f64;
        c[tau] = if cumul > 0.0 {
            (d[tau] as f64 * tau as f64 / cumul) as f32
        } else {
            1.0
        };
    }
    // Creux locaux, dans l'ordre des périodes.
    let minima: Vec<usize> = (tau_min..tau_max)
        .filter(|&t| c[t] < c[t - 1] && c[t] <= c[t + 1])
        .collect();
    let &global = minima.iter().min_by(|&&a, &&b| c[a].total_cmp(&c[b]))?;
    // Seuils de 0,01 à 1 sous une loi bêta de moyenne 0,15 (pYIN).
    let (alpha, beta) = (2.0f64, 2.0 * (1.0 - 0.15) / 0.15);
    let poids: Vec<f64> = (1..=100)
        .map(|i| {
            let s = i as f64 / 100.0 - 0.005;
            s.powf(alpha - 1.0) * (1.0 - s).powf(beta - 1.0)
        })
        .collect();
    let total: f64 = poids.iter().sum();
    let mut proba = vec![0f64; tau_max + 1];
    // Part voisée : les seuils qui ont trouvé un creux ; le repli n'en est
    // pas.
    let mut voisement = 0f64;
    for (i, p) in poids.iter().enumerate() {
        let seuil = (i + 1) as f32 / 100.0;
        match minima.iter().find(|&&t| c[t] < seuil) {
            Some(&t) => {
                proba[t] += p / total;
                voisement += p / total;
            }
            // Aucun creux sous ce seuil : le plus profond, à faible
            // probabilité (p_a = 0,01 dans pYIN).
            None => proba[global] += 0.01 * p / total,
        }
    }
    let (tau, _) = proba.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    if proba[tau] <= 0.0 {
        return None;
    }
    // Interpolation parabolique du creux.
    let t = if tau > 0 && tau < tau_max {
        let (a, b, cc) = (c[tau - 1], c[tau], c[tau + 1]);
        let den = a - 2.0 * b + cc;
        if den.abs() > 1e-12 {
            tau as f32 + 0.5 * (a - cc) / den
        } else {
            tau as f32
        }
    } else {
        tau as f32
    };
    Some((sr / t, (voisement as f32).min(1.0)))
}

/// Énergie de `x` à la fréquence `f`, sur une fenêtre rectangulaire de `n`
/// échantillons (Goertzel).
fn energie(x: &[f32], sr: f32, f: f32, n: usize) -> f32 {
    let w = 2.0 * std::f64::consts::PI * f as f64 / sr as f64;
    let (mut re, mut im) = (0f64, 0f64);
    for (j, &v) in x.iter().take(n).enumerate() {
        re += v as f64 * (w * j as f64).cos();
        im -= v as f64 * (w * j as f64).sin();
    }
    ((re * re + im * im) / (n as f64 * n as f64)) as f32
}

/// Longueur rectangulaire d'un nombre entier de périodes de `f`, la plus
/// longue qui tienne dans `dispo`.
fn periodes_entieres(sr: f32, f: f32, dispo: usize) -> Option<usize> {
    let p = sr / f;
    let k = (dispo as f32 / p).floor();
    (k >= 1.0).then(|| (k * p).round() as usize)
}

/// La hauteur d'une note dont `x` commence à la trame de hauteur : pYIN,
/// puis les contrôles d'octave par l'énergie. Rend (f0, voisement,
/// sous-octave).
pub fn hauteur(x: &[f32], sr: f32) -> Option<(f32, f32, f32)> {
    let (mut f0, conf) = pyin_trame(x, sr)?;
    // Sous-octave de l'estimateur : à f0, aucune énergie, tout est à 2·f0.
    while 2.0 * f0 <= F_MAX_HZ {
        let n = periodes_entieres(sr, f0, x.len())?;
        let (e1, e2) = (energie(x, sr, f0, n), energie(x, sr, 2.0 * f0, n));
        if e2 > 0.0 && e1 / e2 < 0.02 {
            f0 *= 2.0;
        } else {
            break;
        }
    }
    let sous = match periodes_entieres(sr, f0 / 2.0, x.len()) {
        Some(n) => {
            let (e_bas, e) = (energie(x, sr, f0 / 2.0, n), energie(x, sr, f0, n));
            if e > 0.0 {
                e_bas / e
            } else {
                0.0
            }
        }
        // Trop court pour une période de f0/2 : on ne sait pas.
        None => 0.0,
    };
    Some((f0, conf, sous))
}

/// Échantillons qu'il faut après l'attaque pour mesurer la hauteur.
fn besoin_hauteur(sr: f32) -> usize {
    (sr * DECALAGE_HAUTEUR_S) as usize
        + (sr * FENETRE_HAUTEUR_S) as usize
        + (sr / F_MIN_HZ).ceil() as usize
        + 1
}

/// L'analyseur de la DI, par blocs : ce que rend la capture (cpal) s'y
/// pousse tel quel, mono.
pub struct AnalyseurDI {
    sr: f32,
    /// Instant de l'horloge commune du premier échantillon.
    origine_s: f64,
    n_fft: usize,
    pas: usize,
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    tampon: VecDeque<f32>,
    /// Indice absolu de `tampon[0]`.
    debut: u64,
    /// Début absolu de la prochaine trame de flux.
    trame: u64,
    log_prec: Vec<f32>,
    passe: VecDeque<f32>,
    /// Les deux dernières trames : (début, flux, niveau dB).
    prec: [(u64, f32, f32); 2],
    derniere_attaque: Option<u64>,
    /// Attaques dont la hauteur attend ses échantillons : (indice, niveau).
    attente: VecDeque<(u64, f32)>,
}

impl AnalyseurDI {
    pub fn new(sr: f32, origine_s: f64) -> Self {
        let n_fft = ((sr * 0.0107) as usize).next_power_of_two().max(128);
        let hann = (0..n_fft)
            .map(|i| {
                let x = std::f32::consts::PI * i as f32 / n_fft as f32;
                x.sin() * x.sin()
            })
            .collect();
        AnalyseurDI {
            sr,
            origine_s,
            n_fft,
            pas: n_fft / 4,
            fft: FftPlanner::new().plan_fft_forward(n_fft),
            hann,
            tampon: VecDeque::new(),
            debut: 0,
            trame: 0,
            log_prec: vec![0.0; n_fft / 2 + 1],
            passe: VecDeque::new(),
            prec: [(0, 0.0, -120.0); 2],
            derniere_attaque: None,
            attente: VecDeque::new(),
        }
    }

    fn t(&self, i: u64) -> f64 {
        self.origine_s + i as f64 / self.sr as f64
    }

    fn fin(&self) -> u64 {
        self.debut + self.tampon.len() as u64
    }

    fn morceau(&self, i: u64, n: usize) -> Vec<f32> {
        let a = (i - self.debut) as usize;
        self.tampon.range(a..a + n).copied().collect()
    }

    /// Une trame de flux : flux spectral log, niveau RMS.
    fn flux(&mut self, i: u64) -> (f32, f32) {
        let x = self.morceau(i, self.n_fft);
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let mut buf: Vec<Complex32> = x
            .iter()
            .zip(&self.hann)
            .map(|(v, h)| Complex32::new(v * h, 0.0))
            .collect();
        self.fft.process(&mut buf);
        let mut f = 0f32;
        for (k, prec) in self.log_prec.iter_mut().enumerate() {
            let l = (1.0 + 100.0 * buf[k].norm()).ln();
            f += (l - *prec).max(0.0);
            *prec = l;
        }
        (f / self.log_prec.len() as f32, 20.0 * rms.max(1e-9).log10())
    }

    /// Pousse un bloc ; rend les attaques et les notes que ce bloc complète.
    pub fn pousser(&mut self, bloc: &[f32]) -> Vec<Evenement> {
        self.tampon.extend(bloc.iter().copied());
        let mut sortie = Vec::new();
        let ecart_min = (self.sr * ECART_MIN_S) as u64;
        let historique = (0.25 * self.sr / self.pas as f32) as usize;
        while self.trame + self.n_fft as u64 <= self.fin() {
            let i = self.trame;
            let (f, db) = self.flux(i);
            // La trame d'avant est-elle un pic au-dessus du seuil ?
            let (i1, f1, db1) = self.prec[1];
            let (_, f0, _) = self.prec[0];
            let mut tri: Vec<f32> = self.passe.iter().copied().collect();
            tri.sort_by(f32::total_cmp);
            let mediane = tri.get(tri.len() / 2).copied().unwrap_or(0.0);
            let seuil = 1.5 * mediane + 0.05;
            let loin = self.derniere_attaque.is_none_or(|a| i1 >= a + ecart_min);
            if f1 > seuil && f1 >= f0 && f1 > f && db.max(db1) > SILENCE_DBFS && loin {
                // L'attaque au milieu de la trame du pic.
                let a = i1 + self.n_fft as u64 / 2;
                self.derniere_attaque = Some(i1);
                let niveau = db.max(db1);
                sortie.push(Evenement::Attaque {
                    t_s: self.t(a),
                    niveau_db: niveau,
                });
                self.attente.push_back((a, niveau));
            }
            self.passe.push_back(f1);
            if self.passe.len() > historique {
                self.passe.pop_front();
            }
            self.prec = [self.prec[1], (i, f, db)];
            self.trame += self.pas as u64;
        }
        // Les hauteurs dont les échantillons sont là.
        let besoin = besoin_hauteur(self.sr) as u64;
        let decalage = (self.sr * DECALAGE_HAUTEUR_S) as u64;
        while let Some(&(a, niveau)) = self.attente.front() {
            if a + besoin > self.fin() {
                break;
            }
            self.attente.pop_front();
            let x = self.morceau(a + decalage, (besoin - decalage) as usize);
            let (f0_hz, confiance, sous_octave) = match hauteur(&x, self.sr) {
                Some((f, c, s)) => (Some(f), c, s),
                None => (None, 0.0, 0.0),
            };
            sortie.push(Evenement::Note(NoteDI {
                t_s: self.t(a),
                f0_hz,
                confiance,
                sous_octave,
                niveau_db: niveau,
            }));
        }
        // On garde ce qu'il faut pour la prochaine trame et les hauteurs en
        // attente.
        let garder_depuis = self
            .attente
            .front()
            .map(|&(a, _)| a)
            .unwrap_or(u64::MAX)
            .min(self.trame);
        while self.debut < garder_depuis && !self.tampon.is_empty() {
            self.tampon.pop_front();
            self.debut += 1;
        }
        sortie
    }

    /// Toutes les notes d'une prise entière (un fichier).
    pub fn analyser(x: &[f32], sr: f32, origine_s: f64) -> Vec<NoteDI> {
        let mut a = AnalyseurDI::new(sr, origine_s);
        let mut v: Vec<NoteDI> = a
            .pousser(x)
            .into_iter()
            .chain(a.pousser(&vec![0.0; besoin_hauteur(sr) + a.n_fft]))
            .filter_map(|e| match e {
                Evenement::Note(n) => Some(n),
                Evenement::Attaque { .. } => None,
            })
            .collect();
        v.sort_by(|p, q| p.t_s.total_cmp(&q.t_s));
        v
    }
}

/// Une note de la référence de basse (la transcription du stem).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoteRef {
    /// Instant dans le morceau d'origine, en secondes.
    pub tau_s: f64,
    /// Hauteur MIDI.
    pub hauteur: u8,
    /// Fausse quand le stem ne confirme pas la note : rien n'est dit sur
    /// elle.
    pub sure: bool,
}

/// La hauteur jouée face à celle de la référence.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Hauteur {
    Juste,
    /// Mauvaise note, à `demi_tons` de la référence (positif : trop haut),
    /// hors octaves.
    Fausse {
        demi_tons: i32,
    },
    /// La bonne note à une ou plusieurs octaves (positif : trop haut).
    Octave {
        octaves: i32,
    },
    /// pYIN n'est pas sûr de la hauteur jouée : rien n'est dit.
    Inconnue,
}

/// Compare une note jouée à la hauteur attendue.
pub fn comparer(attendue: u8, jouee: &NoteDI) -> Hauteur {
    let Some(m) = jouee.hauteur() else {
        return Hauteur::Inconnue;
    };
    if jouee.confiance < CONFIANCE_JEU {
        return Hauteur::Inconnue;
    }
    let ecart = m - attendue as f32;
    let demi = ecart.round() as i32;
    if (ecart - demi as f32).abs() > 0.5 {
        return Hauteur::Inconnue;
    }
    if demi == 0 {
        return Hauteur::Juste;
    }
    if demi % 12 == 0 {
        // Une octave au-dessus, avec l'énergie de la fondamentale attendue :
        // l'estimateur a pris l'harmonique 2, la note est juste.
        if demi == 12 && jouee.sous_octave >= SOUS_OCTAVE_PRESENTE {
            return Hauteur::Juste;
        }
        return Hauteur::Octave { octaves: demi / 12 };
    }
    Hauteur::Fausse { demi_tons: demi }
}

/// La confiance de chaque note de la référence, mesurée par le même
/// analyseur sur le stem de basse : la probabilité de voisement de pYIN
/// quand sa hauteur confirme celle de la transcription (à l'octave près si
/// l'énergie de la fondamentale le permet), 0 sinon. Plusieurs trames après
/// l'instant de la note (le stem démixé étale les attaques) ; on garde la
/// meilleure.
pub fn confiance_reference(stem: &[f32], sr: f32, notes: &[(f64, u8)]) -> Vec<f32> {
    let besoin = besoin_hauteur(sr) - (sr * DECALAGE_HAUTEUR_S) as usize;
    notes
        .iter()
        .map(|&(tau, h)| {
            let mut meilleure = 0f32;
            for decal in [0.0f32, 0.010, 0.020, 0.035] {
                let a = ((tau as f32 + DECALAGE_HAUTEUR_S + decal) * sr).round();
                if a < 0.0 {
                    continue;
                }
                let a = a as usize;
                if a + besoin > stem.len() {
                    continue;
                }
                let Some((f0, conf, sous)) = hauteur(&stem[a..a + besoin], sr) else {
                    continue;
                };
                let note = NoteDI {
                    t_s: tau,
                    f0_hz: Some(f0),
                    confiance: conf,
                    sous_octave: sous,
                    niveau_db: 0.0,
                };
                if comparer(h, &note) == Hauteur::Juste {
                    meilleure = meilleure.max(conf);
                }
            }
            meilleure
        })
        .collect()
}

/// Les notes de la référence, chacune sûre si le stem la confirme
/// ([`CONFIANCE_SURE`]).
pub fn references(notes: &[(f64, u8)], confiances: &[f32]) -> Vec<NoteRef> {
    notes
        .iter()
        .zip(confiances.iter().chain(std::iter::repeat(&0.0)))
        .map(|(&(tau_s, hauteur), &c)| NoteRef {
            tau_s,
            hauteur,
            sure: c >= CONFIANCE_SURE,
        })
        .collect()
}

/// Le jugement d'une note de basse : celui de l'apparieur (timing), plus la
/// hauteur quand la note est attrapée et la référence sûre.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JugementBasse {
    #[serde(flatten)]
    pub jugement: Jugement,
    pub hauteur: Option<Hauteur>,
}

/// Juge les notes attendues entre `t0_s` et `t1_s` de l'horloge (mêmes
/// règles que [`apparier`] : une voix, glouton sur l'écart, rien de dit sur
/// une référence incertaine), puis la hauteur de chaque note attrapée.
pub fn juger(
    horloge: &Horloge,
    refs: &[NoteRef],
    jeu: &[NoteDI],
    t0_s: f64,
    t1_s: f64,
    fenetres: &Fenetres,
) -> Vec<JugementBasse> {
    // Une seule classe : la basse est une voix.
    let voix = Classe::Toms;
    let r: Vec<Reference> = refs
        .iter()
        .map(|n| Reference {
            tau_s: n.tau_s,
            classe: voix,
            sure: n.sure,
        })
        .collect();
    let frappes: Vec<Frappe> = jeu
        .iter()
        .map(|n| Frappe {
            t_s: n.t_s,
            classe: voix,
            note: n.hauteur().map_or(0, |m| m.round().clamp(0.0, 127.0) as u8),
            velocite: 100,
            ouverture: None,
        })
        .collect();
    apparier(horloge, &r, &frappes, t0_s, t1_s, fenetres)
        .into_iter()
        .map(|jugement| {
            let attrapee = matches!(
                jugement.verdict,
                Verdict::Parfait | Verdict::Bon | Verdict::Avance | Verdict::Retard
            );
            let hauteur = match (attrapee, jugement.reference, jugement.frappe) {
                (true, Some(i), Some(k)) => Some(comparer(refs[i].hauteur, &jeu[k])),
                _ => None,
            };
            JugementBasse { jugement, hauteur }
        })
        .collect()
}

/// Les hauteurs d'un ensemble de jugements.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BilanHauteur {
    pub justes: u32,
    pub fausses: u32,
    pub octaves: u32,
    pub inconnues: u32,
}

impl BilanHauteur {
    pub fn de(jugements: &[JugementBasse]) -> BilanHauteur {
        let mut b = BilanHauteur::default();
        for j in jugements {
            match j.hauteur {
                Some(Hauteur::Juste) => b.justes += 1,
                Some(Hauteur::Fausse { .. }) => b.fausses += 1,
                Some(Hauteur::Octave { .. }) => b.octaves += 1,
                Some(Hauteur::Inconnue) => b.inconnues += 1,
                None => {}
            }
        }
        b
    }
}

/// Une note de basse synthétique (corde pincée : harmoniques décroissantes,
/// déclin exponentiel), pour les tests et les bancs sans instrument.
/// `fondamentale` : part de la fondamentale (1 : normale ; proche de 0 :
/// corde attaquée près du chevalet, harmonique 2 dominante).
pub fn synthetiser_note(
    sortie: &mut [f32],
    sr: f32,
    debut_s: f32,
    duree_s: f32,
    midi_note: f32,
    fondamentale: f32,
) {
    let f = hz(midi_note);
    let a = (debut_s * sr) as usize;
    let n = ((duree_s * sr) as usize).min(sortie.len().saturating_sub(a));
    let attaque = (0.003 * sr) as usize;
    for j in 0..n {
        let t = j as f32 / sr;
        let env = (-t * 3.0).exp()
            * if j < attaque {
                j as f32 / attaque as f32
            } else {
                1.0
            };
        let fin = if n - j < (0.01 * sr) as usize {
            (n - j) as f32 / (0.01 * sr)
        } else {
            1.0
        };
        let mut s = 0f32;
        for h in 1..=8 {
            let fh = f * h as f32;
            if fh > sr / 2.0 {
                break;
            }
            let amp =
                if h == 1 { fondamentale } else { 0.8 / h as f32 } * (-t * 2.0 * h as f32).exp();
            s += amp * (2.0 * std::f32::consts::PI * fh * t).sin();
        }
        sortie[a + j] += 0.3 * env * fin * s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    /// Une ligne de basse : (début, durée, MIDI), du Si grave au sol aigu.
    fn ligne() -> Vec<(f32, f32, f32)> {
        vec![
            (0.20, 0.45, 28.0), // E1, 41,2 Hz
            (0.70, 0.45, 23.0), // B0, 30,9 Hz
            (1.20, 0.25, 33.0), // A1
            (1.50, 0.25, 33.0), // A1 répété
            (1.80, 0.45, 38.0), // D2
            (2.30, 0.45, 43.0), // G2
            (2.80, 0.30, 55.0), // G3
            (3.15, 0.40, 31.0), // G1
        ]
    }

    fn prise(notes: &[(f32, f32, f32)], fondamentale: f32) -> Vec<f32> {
        let fin = notes.iter().map(|n| n.0 + n.1).fold(0.0, f32::max) + 0.3;
        let mut x = vec![0f32; (fin * SR) as usize];
        for &(d, l, m) in notes {
            synthetiser_note(&mut x, SR, d, l, m, fondamentale);
        }
        // Un souffle de fond, déterministe.
        let mut graine = 12345u32;
        for v in &mut x {
            graine = graine.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *v += ((graine >> 16) as f32 / 65536.0 - 0.5) * 2e-4;
        }
        x
    }

    #[test]
    fn attaques_et_hauteurs_d_une_ligne_de_basse() {
        let notes = ligne();
        let jeu = AnalyseurDI::analyser(&prise(&notes, 1.0), SR, 0.0);
        assert_eq!(jeu.len(), notes.len(), "{jeu:#?}");
        for (n, &(d, _, m)) in jeu.iter().zip(&notes) {
            let ecart = (n.t_s - d as f64) * 1000.0;
            assert!(ecart.abs() < 10.0, "attaque {d} : écart {ecart:.1} ms");
            let h = n.hauteur().expect("hauteur");
            assert!((h - m).abs() < 0.5, "note {m} lue {h:.2}");
            assert!(n.confiance > 0.7, "note {m} : confiance {}", n.confiance);
        }
    }

    #[test]
    fn la_hauteur_arrive_moins_de_75_ms_apres_l_attaque() {
        // Par blocs de 64 : on note le premier bloc qui rend la note.
        let notes = [(0.30f32, 0.6f32, 23.0f32)];
        let x = prise(&notes, 1.0);
        let mut a = AnalyseurDI::new(SR, 0.0);
        let (mut t_attaque, mut t_note) = (None, None);
        for (k, bloc) in x.chunks(64).enumerate() {
            let fin_bloc = ((k + 1) * 64) as f64 / SR as f64;
            for e in a.pousser(bloc) {
                match e {
                    Evenement::Attaque { .. } if t_attaque.is_none() => t_attaque = Some(fin_bloc),
                    Evenement::Note(_) if t_note.is_none() => t_note = Some(fin_bloc),
                    _ => {}
                }
            }
        }
        let attaque_ms = (t_attaque.unwrap() - 0.30) * 1000.0;
        let note_ms = (t_note.unwrap() - 0.30) * 1000.0;
        assert!(attaque_ms < 20.0, "attaque rendue {attaque_ms:.1} ms après");
        assert!(note_ms < 75.0, "hauteur rendue {note_ms:.1} ms après");
    }

    #[test]
    fn par_blocs_comme_d_un_seul_tenant() {
        let x = prise(&ligne(), 1.0);
        let entier = AnalyseurDI::analyser(&x, SR, 2.0);
        let mut a = AnalyseurDI::new(SR, 2.0);
        let mut blocs = Vec::new();
        for bloc in x
            .chunks(300)
            .chain(std::iter::once(&vec![0f32; 10_000][..]))
        {
            for e in a.pousser(bloc) {
                if let Evenement::Note(n) = e {
                    blocs.push(n);
                }
            }
        }
        assert_eq!(entier, blocs);
        assert!((entier[0].t_s - 2.2).abs() < 0.01);
    }

    #[test]
    fn une_note_tenue_ne_donne_qu_une_attaque_et_le_silence_aucune() {
        let x = prise(&[(0.2, 1.5, 28.0)], 1.0);
        assert_eq!(AnalyseurDI::analyser(&x, SR, 0.0).len(), 1);
        let souffle = prise(&[], 1.0);
        assert!(AnalyseurDI::analyser(&souffle, SR, 0.0).is_empty());
    }

    fn note(m: f32, fondamentale: f32) -> NoteDI {
        let x = prise(&[(0.1, 0.5, m)], fondamentale);
        let v = AnalyseurDI::analyser(&x, SR, 0.0);
        assert_eq!(v.len(), 1);
        v[0]
    }

    #[test]
    fn fausses_notes_et_octaves() {
        assert_eq!(comparer(28, &note(28.0, 1.0)), Hauteur::Juste);
        assert_eq!(
            comparer(28, &note(29.0, 1.0)),
            Hauteur::Fausse { demi_tons: 1 }
        );
        assert_eq!(
            comparer(33, &note(31.0, 1.0)),
            Hauteur::Fausse { demi_tons: -2 }
        );
        // Le mi grave joué une octave au-dessus : sans fondamentale à 41 Hz,
        // c'est la mauvaise octave.
        assert_eq!(
            comparer(28, &note(40.0, 1.0)),
            Hauteur::Octave { octaves: 1 }
        );
        assert_eq!(
            comparer(40, &note(28.0, 1.0)),
            Hauteur::Octave { octaves: -1 }
        );
    }

    #[test]
    fn une_fondamentale_faible_n_est_pas_une_faute_d_octave() {
        // Corde attaquée près du chevalet : l'harmonique 2 domine. Qu'importe
        // ce que lit pYIN, l'énergie à 41 Hz dit que c'est le mi grave.
        let n = note(28.0, 0.15);
        assert_eq!(comparer(28, &n), Hauteur::Juste, "{n:?}");
    }

    #[test]
    fn la_reference_se_verifie_sur_le_stem() {
        let stem = prise(&ligne(), 1.0);
        let mut notes: Vec<(f64, u8)> = ligne().iter().map(|n| (n.0 as f64, n.2 as u8)).collect();
        // La transcription s'est trompée sur la 3ᵉ note, a inventé une note
        // dans un silence, et a vu la 5ᵉ une octave trop haut.
        notes[2].1 = 35;
        notes.push((3.9, 30));
        notes[4].1 += 12;
        let c = confiance_reference(&stem, SR, &notes);
        let refs = references(&notes, &c);
        let sures: Vec<bool> = refs.iter().map(|r| r.sure).collect();
        assert_eq!(
            sures,
            [true, true, false, true, false, true, true, true, false],
            "{c:?}"
        );
    }

    fn horloge() -> Horloge {
        Horloge {
            t_s: 100.0,
            morceau_s: 0.0,
            vitesse: 1.0,
            boucle: None,
            latence_s: 0.0,
        }
    }

    #[test]
    fn juger_une_prise_contre_la_reference() {
        let reference = ligne();
        let notes: Vec<(f64, u8)> = reference.iter().map(|n| (n.0 as f64, n.2 as u8)).collect();
        let stem = prise(&reference, 1.0);
        let mut refs = references(&notes, &confiance_reference(&stem, SR, &notes));
        // Une note que le stem ne confirme pas (posée à la main ici).
        refs[6].sure = false;
        // Le jeu : 3ᵉ note un demi-ton trop haut, 4ᵉ oubliée, 6ᵉ une octave
        // au-dessus, 7ᵉ (incertaine) fausse, le reste juste, 20 ms en retard.
        let mut jouees = reference.clone();
        jouees[2].2 += 1.0;
        jouees[5].2 += 12.0;
        jouees[6].2 += 3.0;
        jouees.remove(3);
        for n in &mut jouees {
            n.0 += 0.020;
        }
        let jeu = AnalyseurDI::analyser(&prise(&jouees, 1.0), SR, 100.0);
        let js = juger(&horloge(), &refs, &jeu, 100.0, 104.0, &Fenetres::default());
        let b = BilanHauteur::de(&js);
        assert_eq!(
            (b.justes, b.fausses, b.octaves, b.inconnues),
            (4, 1, 1, 0),
            "{js:#?}"
        );
        let manquees = js
            .iter()
            .filter(|j| j.jugement.verdict == Verdict::Manquee)
            .count();
        assert_eq!(manquees, 1);
        // Rien n'est dit sur la note incertaine, ni timing ni hauteur.
        let incertaine = js.iter().find(|j| j.jugement.reference == Some(6)).unwrap();
        assert_eq!(incertaine.jugement.verdict, Verdict::NonNotee);
        assert_eq!(incertaine.hauteur, None);
        // Le retard de 20 ms se lit sur les notes justes.
        for j in js.iter().filter(|j| j.hauteur == Some(Hauteur::Juste)) {
            let e = j.jugement.ecart_ms.unwrap();
            assert!((e - 20.0).abs() < 10.0, "écart {e:.1} ms");
        }
    }
}
