// SPDX-License-Identifier: GPL-3.0-or-later
//! Le retour de l'instrument pendant la pratique (E7) : l'entrée de la basse,
//! passée par la chaîne de son ([`crate::instrument::Chaine`]), jouée sur la
//! sortie du Mac en même temps que le morceau.
//!
//! **Deux horloges.** L'entrée (une Scarlett) et la sortie (le Mac) ont
//! chacune leur quartz : à 48 kHz nominal des deux côtés, l'écart réel est de
//! quelques dizaines de ppm, assez pour qu'un tampon partagé se vide ou
//! déborde en quelques minutes. Le rappel d'entrée pousse le canal retenu dans
//! un anneau sans verrou (`rtrb`, pas réécrit ici) ; le rappel de sortie le
//! lit par un rééchantillonneur polynomial à rapport réglable (`rubato`
//! `Async`, pas réécrit ici), dont le rapport suit le remplissage de l'anneau :
//! trop plein, il consomme plus vite ; trop vide, moins vite. Le remplissage
//! tient autour d'une cible ([`Moteur::cible`]) ; c'est l'essentiel de la
//! latence ajoutée.
//!
//! **Le rappel de sortie** ([`Moteur::remplir`]) n'alloue rien et ne prend
//! aucun verrou bloquant : les réglages passent par des atomiques, une
//! nouvelle chaîne (préréglage changé) par un second anneau ; le volume suit
//! sa consigne en 10 ms (pas de clic). Il ne bouge pas le morceau : la sortie
//! du retour est un flux à part, que CoreAudio mêle à celui du lecteur.
//!
//! Le coach et l'accordeur lisent le signal **brut** avant l'anneau : rien de
//! ce qui se passe ici ne les atteint.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Adjustable, Async, FixedAsync, PolynomialDegree, Resampler};

use crate::instrument::Chaine;

/// Remplissage visé de l'anneau au plus bas, en trames d'entrée : 2,7 ms à
/// 48 kHz. Le moteur vise plus quand les tampons des appareils l'exigent :
/// un rappel de sortie prend d'un coup son bloc entier, un rappel d'entrée
/// le rend d'un coup aussi ; la cible les couvre tous deux, plus un bloc de
/// rééchantillonnage ([`Moteur::cible`]).
pub const CIBLE_TRAMES: usize = 128;

/// Bloc d'entrée du rééchantillonneur, en trames.
pub const BLOC: usize = 32;

/// Tampon demandé aux appareils (entrée et sortie), en trames : 2,7 ms à
/// 48 kHz. Ramené dans ce que l'appareil accepte.
pub const TAMPON_APPAREIL: u32 = 128;

/// Capacité de l'anneau : 1/4 s à 192 kHz. Ce qui ne tient pas est jeté
/// (retour arrêté, ou sortie bloquée).
const CAPACITE: usize = 48_000;

/// Écart relatif maximal du rapport autour du nominal (±1 %) : bien plus que
/// la dérive de deux quartz, assez pour se recaler après un à-coup.
const ECART_MAX: f64 = 0.01;

/// Gain du correcteur de rapport : écart relatif de rapport par écart
/// relatif de remplissage. Constante de temps du recalage =
/// `CIBLE_TRAMES / (sr × GAIN)` ≈ 1,3 s à 48 kHz.
const GAIN: f64 = 0.002;

/// Lissage du remplissage mesuré (s) : la mesure saute d'un tampon à chaque
/// rappel.
const LISSAGE_S: f64 = 0.25;

/// Au-delà de ce remplissage (en cibles), l'anneau est vidé jusqu'à la
/// cible : retour qui reprend, rappel de sortie resté longtemps muet.
const RECALE_AU_DELA: usize = 4;

/// Temps de la rampe de volume (s).
const RAMPE_S: f32 = 0.010;

/// Ce que le rappel d'entrée, le rappel de sortie et les commandes se
/// partagent : réglages et compteurs, tous atomiques.
#[derive(Debug)]
pub struct Partage {
    volume: AtomicU32,
    coupe: AtomicBool,
    canal: AtomicUsize,
    /// Le plus grand bloc d'entrée vu, en trames.
    bloc_entree: AtomicUsize,
    /// Rappels de sortie qui ont manqué d'entrée (un trou : un craquement).
    sous_alimentations: AtomicU64,
    /// Échantillons d'entrée jetés, l'anneau plein.
    jetes: AtomicU64,
    /// Fois où l'anneau trop plein a été ramené à la cible.
    recales: AtomicU64,
    /// Remplissage lissé, en trames d'entrée (×1000).
    remplissage_milli: AtomicU64,
}

impl Default for Partage {
    fn default() -> Self {
        Partage {
            // Jamais fort au premier lancement : moitié, et coupé.
            volume: AtomicU32::new(0.5f32.to_bits()),
            coupe: AtomicBool::new(true),
            canal: AtomicUsize::new(0),
            bloc_entree: AtomicUsize::new(0),
            sous_alimentations: AtomicU64::new(0),
            jetes: AtomicU64::new(0),
            recales: AtomicU64::new(0),
            remplissage_milli: AtomicU64::new(0),
        }
    }
}

impl Partage {
    /// Volume de l'instrument, `0..=1` (borné).
    pub fn regler_volume(&self, v: f32) {
        let v = if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.volume.store(v.to_bits(), Ordering::Relaxed);
    }
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
    pub fn couper(&self, c: bool) {
        self.coupe.store(c, Ordering::Relaxed);
    }
    pub fn coupe(&self) -> bool {
        self.coupe.load(Ordering::Relaxed)
    }
    /// Le canal de l'entrée qui part au retour (compté depuis 0).
    pub fn choisir_canal(&self, c: usize) {
        self.canal.store(c, Ordering::Relaxed);
    }
    pub fn canal(&self) -> usize {
        self.canal.load(Ordering::Relaxed)
    }
    pub fn sous_alimentations(&self) -> u64 {
        self.sous_alimentations.load(Ordering::Relaxed)
    }
    pub fn jetes(&self) -> u64 {
        self.jetes.load(Ordering::Relaxed)
    }
    pub fn recales(&self) -> u64 {
        self.recales.load(Ordering::Relaxed)
    }
    /// Remplissage lissé de l'anneau, en trames d'entrée.
    pub fn remplissage(&self) -> f64 {
        self.remplissage_milli.load(Ordering::Relaxed) as f64 / 1000.0
    }
}

/// Le bout du rappel d'entrée.
pub struct Entree {
    anneau: rtrb::Producer<f32>,
    partage: Arc<Partage>,
}

impl Entree {
    /// Pousse un bloc mono. Sans allocation ni verrou ; ce qui ne tient pas
    /// est jeté et compté.
    pub fn pousser(&mut self, bloc: &[f32]) {
        self.partage
            .bloc_entree
            .fetch_max(bloc.len(), Ordering::Relaxed);
        let n = bloc.len().min(self.anneau.slots());
        if n < bloc.len() {
            self.partage
                .jetes
                .fetch_add((bloc.len() - n) as u64, Ordering::Relaxed);
        }
        if n == 0 {
            return;
        }
        if let Ok(morceau) = self.anneau.write_chunk_uninit(n) {
            morceau.fill_from_iter(bloc[..n].iter().copied());
        }
    }

    /// Le canal à pousser (réglage partagé).
    pub fn canal(&self) -> usize {
        self.partage.canal()
    }
}

/// Le bout du rappel de sortie : lit l'anneau, rééchantillonne, traite,
/// règle le volume.
pub struct Moteur {
    anneau: rtrb::Consumer<f32>,
    chaines: rtrb::Consumer<Chaine>,
    partage: Arc<Partage>,
    chaine: Chaine,
    rs: Async<f32>,
    /// Rapport nominal sortie / entrée.
    rapport: f64,
    /// Remplissage visé, en trames d'entrée (dernier calcul).
    cible: usize,
    sr_sortie: f32,
    entree: Vec<f32>,
    sortie: Vec<f32>,
    /// Trames prêtes dans `sortie[lu..pret]`.
    lu: usize,
    pret: usize,
    /// En attente que l'anneau atteigne la cible (départ, ou après un trou).
    amorce: bool,
    remplissage: f64,
    gain: f32,
    pas_gain: f32,
}

/// Le côté des commandes : réglages, changement de chaîne.
pub struct Commandes {
    pub partage: Arc<Partage>,
    chaines: rtrb::Producer<Chaine>,
}

impl Commandes {
    /// Remplace la chaîne de son (construite hors du rappel : elle alloue).
    /// `false` si le rappel n'a pas encore pris les précédentes.
    pub fn changer_chaine(&mut self, c: Chaine) -> bool {
        self.chaines.push(c).is_ok()
    }
}

/// Crée un retour : `sr_entree` et `sr_sortie` sont les fréquences
/// nominales des deux appareils ; `chaine` le son de départ (construit à
/// `sr_sortie`).
pub fn retour(
    sr_entree: u32,
    sr_sortie: u32,
    chaine: Chaine,
) -> Result<(Entree, Moteur, Commandes), String> {
    let partage = Arc::new(Partage::default());
    let (prod, cons) = rtrb::RingBuffer::<f32>::new(CAPACITE);
    let (cprod, ccons) = rtrb::RingBuffer::<Chaine>::new(4);
    let rapport = f64::from(sr_sortie) / f64::from(sr_entree.max(1));
    let rs = Async::<f32>::new_poly(
        rapport,
        1.0 + 2.0 * ECART_MAX,
        PolynomialDegree::Cubic,
        BLOC,
        1,
        FixedAsync::Input,
    )
    .map_err(|e| e.to_string())?;
    let sortie = vec![0.0; rs.output_frames_max()];
    let sr = sr_sortie as f32;
    Ok((
        Entree {
            anneau: prod,
            partage: partage.clone(),
        },
        Moteur {
            anneau: cons,
            chaines: ccons,
            partage: partage.clone(),
            chaine,
            rs,
            rapport,
            cible: CIBLE_TRAMES,
            sr_sortie: sr,
            entree: vec![0.0; BLOC],
            sortie,
            lu: 0,
            pret: 0,
            amorce: true,
            remplissage: 0.0,
            gain: 0.0,
            pas_gain: 1.0 - (-1.0 / (RAMPE_S * sr)).exp(),
        },
        Commandes {
            partage,
            chaines: cprod,
        },
    ))
}

impl Moteur {
    /// Latence propre au moteur, en trames de sortie : remplissage visé,
    /// un bloc de rééchantillonnage, le retard du filtre. Sans les tampons
    /// des appareils.
    pub fn latence_trames(&self) -> f64 {
        (self.cible + BLOC) as f64 * self.rapport + self.rs.output_delay() as f64
    }

    /// Le remplissage visé pour un rappel de sortie de `trames` trames : le
    /// bloc qu'il prendra, en trames d'entrée, le plus grand bloc d'entrée,
    /// un bloc de rééchantillonnage ; [`CIBLE_TRAMES`] au moins.
    pub fn cible(&self, trames: usize) -> usize {
        let sortie = (trames as f64 / self.rapport).ceil() as usize;
        let entree = self.partage.bloc_entree.load(Ordering::Relaxed);
        (sortie + entree + BLOC).max(CIBLE_TRAMES)
    }

    /// Reconstruit le rééchantillonneur et la chaîne pour une autre sortie
    /// (appareil changé entre deux retours). Alloue : jamais pendant que le
    /// moteur joue. L'anneau reste, l'amorce recommence.
    pub fn preparer(
        &mut self,
        sr_entree: u32,
        sr_sortie: u32,
        chaine: Chaine,
    ) -> Result<(), String> {
        let rapport = f64::from(sr_sortie) / f64::from(sr_entree.max(1));
        self.rs = Async::<f32>::new_poly(
            rapport,
            1.0 + 2.0 * ECART_MAX,
            PolynomialDegree::Cubic,
            BLOC,
            1,
            FixedAsync::Input,
        )
        .map_err(|e| e.to_string())?;
        self.sortie = vec![0.0; self.rs.output_frames_max()];
        self.rapport = rapport;
        self.chaine = chaine;
        self.sr_sortie = sr_sortie as f32;
        self.pas_gain = 1.0 - (-1.0 / (RAMPE_S * self.sr_sortie)).exp();
        self.lu = 0;
        self.pret = 0;
        self.amorce = true;
        self.gain = 0.0;
        Ok(())
    }

    pub fn sr_sortie(&self) -> f32 {
        self.sr_sortie
    }

    /// Le rappel de sortie : remplit `sortie` (entrelacée à `canaux`
    /// canaux, la même valeur sur chacun). Sans allocation ni verrou.
    pub fn remplir(&mut self, sortie: &mut [f32], canaux: usize) {
        let canaux = canaux.max(1);
        let trames = sortie.len() / canaux;
        while let Ok(c) = self.chaines.pop() {
            self.chaine = c; // l'ancienne n'a rien sur le tas
        }
        self.regler_rapport(trames);
        let cible = if self.partage.coupe() {
            0.0
        } else {
            self.partage.volume()
        };
        for trame in sortie.chunks_exact_mut(canaux) {
            let x = self.suivant();
            self.gain += (cible - self.gain) * self.pas_gain;
            let y = self.gain * x;
            for s in trame.iter_mut() {
                *s = y;
            }
        }
        for s in sortie[trames * canaux..].iter_mut() {
            *s = 0.0;
        }
    }

    /// Mesure le remplissage, recale ou amorce, et ajuste le rapport.
    fn regler_rapport(&mut self, trames: usize) {
        let cible = self.cible(trames);
        self.cible = cible;
        let n = self.anneau.slots();
        if n > RECALE_AU_DELA * cible {
            if let Ok(m) = self.anneau.read_chunk(n - cible) {
                m.commit_all();
            }
            self.partage.recales.fetch_add(1, Ordering::Relaxed);
            self.remplissage = cible as f64;
        }
        let n = self.anneau.slots();
        if self.amorce {
            if n >= cible {
                self.amorce = false;
                self.remplissage = n as f64;
            } else {
                return;
            }
        }
        let a = 1.0 - (-(trames as f64) / (LISSAGE_S * f64::from(self.sr_sortie))).exp();
        self.remplissage += (n as f64 - self.remplissage) * a;
        self.partage
            .remplissage_milli
            .store((self.remplissage * 1000.0) as u64, Ordering::Relaxed);
        let ecart = (self.remplissage - cible as f64) / cible as f64;
        let rel = (1.0 - GAIN * ecart).clamp(1.0 - ECART_MAX, 1.0 + ECART_MAX);
        let _ = self.rs.set_resample_ratio_relative(rel, true);
    }

    /// L'échantillon suivant, rééchantillonné et traité ; 0 pendant
    /// l'amorce.
    fn suivant(&mut self) -> f32 {
        if self.lu == self.pret && !self.amorce && !self.produire() {
            // Trou : l'entrée n'a pas suivi. On repart en amorce.
            self.amorce = true;
            self.partage
                .sous_alimentations
                .fetch_add(1, Ordering::Relaxed);
        }
        if self.lu < self.pret {
            let x = self.sortie[self.lu];
            self.lu += 1;
            x
        } else {
            0.0
        }
    }

    /// Un bloc de l'anneau rééchantillonné et traité dans `sortie`.
    fn produire(&mut self) -> bool {
        let besoin = self.rs.input_frames_next();
        if besoin > self.entree.len() || self.anneau.slots() < besoin {
            return false;
        }
        let Ok(m) = self.anneau.read_chunk(besoin) else {
            return false;
        };
        let (a, b) = m.as_slices();
        self.entree[..a.len()].copy_from_slice(a);
        self.entree[a.len()..a.len() + b.len()].copy_from_slice(b);
        m.commit_all();
        let Ok(e) = InterleavedSlice::new(&self.entree[..besoin], 1, besoin) else {
            return false;
        };
        let max = self.sortie.len();
        let Ok(mut s) = InterleavedSlice::new_mut(&mut self.sortie[..], 1, max) else {
            return false;
        };
        match self.rs.process_into_buffer(&e, &mut s, None) {
            Ok((_, n)) => {
                self.chaine.traiter(&mut self.sortie[..n]);
                self.lu = 0;
                self.pret = n;
                n > 0
            }
            Err(_) => false,
        }
    }
}

/// Un retour joué : le flux de sortie vit dans un fil, lâché avec cette
/// valeur.
pub struct Sortie {
    _arret: mpsc::Sender<()>,
    pub sr: u32,
    pub canaux: u16,
    /// Tampon de l'appareil de sortie, en trames (s'il a été fixé).
    pub tampon: Option<u32>,
}

/// La taille de tampon à demander à un appareil : [`TAMPON_APPAREIL`]
/// ramené dans ce qu'il accepte ; `None` s'il ne dit rien.
pub fn tampon_pour(sb: &cpal::SupportedBufferSize) -> Option<u32> {
    match sb {
        cpal::SupportedBufferSize::Range { min, max } => {
            Some(TAMPON_APPAREIL.clamp(*min, (*max).max(*min)))
        }
        cpal::SupportedBufferSize::Unknown => None,
    }
}

/// Fréquence de la sortie par défaut, pour construire la chaîne et le
/// moteur avant d'ouvrir le flux.
pub fn sr_sortie_par_defaut() -> Result<u32, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let hote = cpal::default_host();
    let appareil = hote.default_output_device().ok_or("aucune sortie audio")?;
    let config = appareil
        .default_output_config()
        .map_err(|e| e.to_string())?;
    Ok(config.sample_rate())
}

/// Ouvre la sortie par défaut et y joue `moteur` (partagé : un retour
/// arrêté puis repris garde son anneau). Le rappel prend le moteur par
/// `try_lock`, que personne d'autre ne tient pendant qu'il joue.
pub fn jouer(moteur: Arc<Mutex<Moteur>>) -> Result<Sortie, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let (pret_tx, pret_rx) = mpsc::channel::<Result<(u32, u16, Option<u32>), String>>();
    let (arret_tx, arret_rx) = mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("retour-instrument".into())
        .spawn(move || {
            let flux = (|| -> Result<(cpal::Stream, u32, u16, Option<u32>), String> {
                let hote = cpal::default_host();
                let appareil = hote.default_output_device().ok_or("aucune sortie audio")?;
                let supporte = appareil
                    .default_output_config()
                    .map_err(|e| e.to_string())?;
                if supporte.sample_format() != cpal::SampleFormat::F32 {
                    return Err(format!(
                        "format de sortie non pris en charge : {}",
                        supporte.sample_format()
                    ));
                }
                let tampon = tampon_pour(supporte.buffer_size());
                let sr = supporte.sample_rate();
                let canaux = supporte.channels();
                let mut config: cpal::StreamConfig = supporte.into();
                if let Some(t) = tampon {
                    config.buffer_size = cpal::BufferSize::Fixed(t);
                }
                let m = moteur.clone();
                let rappel =
                    move |donnees: &mut [f32], _: &cpal::OutputCallbackInfo| match m.try_lock() {
                        Ok(mut m) => m.remplir(donnees, canaux as usize),
                        Err(_) => donnees.fill(0.0),
                    };
                let flux = appareil
                    .build_output_stream(&config, rappel, |_| {}, None)
                    .map_err(|e| e.to_string())?;
                flux.play().map_err(|e| e.to_string())?;
                Ok((flux, sr, canaux, tampon))
            })();
            match flux {
                Ok((flux, sr, canaux, tampon)) => {
                    let _ = pret_tx.send(Ok((sr, canaux, tampon)));
                    let _ = arret_rx.recv();
                    drop(flux);
                }
                Err(e) => {
                    let _ = pret_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let (sr, canaux, tampon) = pret_rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "la sortie audio ne répond pas".to_string())??;
    Ok(Sortie {
        _arret: arret_tx,
        sr,
        canaux,
        tampon,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::{Prereglage, Reglage};

    fn brut(sr: u32) -> Chaine {
        Chaine::new(&Reglage::de(Prereglage::Brut), sr as f32)
    }

    /// Une source simulée : l'entrée pousse des blocs de `bloc_e` trames à
    /// `sr_e × (1 + derive)`, la sortie en tire `bloc_s` à `sr_s`, chacune à
    /// son heure. Rend la sortie (mono) et le moteur.
    struct Simulation {
        entree: Entree,
        moteur: Moteur,
        commandes: Commandes,
        sr_e: f64,
        sr_s: f64,
        bloc_e: usize,
        bloc_s: usize,
        t_e: f64,
        t_s: f64,
        n_e: usize,
    }

    impl Simulation {
        fn new(sr_e: u32, sr_s: u32, derive: f64, bloc_e: usize, bloc_s: usize) -> Self {
            let (entree, moteur, commandes) = retour(sr_e, sr_s, brut(sr_s)).unwrap();
            commandes.partage.couper(false);
            commandes.partage.regler_volume(1.0);
            Simulation {
                entree,
                moteur,
                commandes,
                sr_e: f64::from(sr_e) * (1.0 + derive),
                sr_s: f64::from(sr_s),
                bloc_e,
                bloc_s,
                t_e: 0.0,
                t_s: 0.0,
                n_e: 0,
            }
        }

        /// Fait tourner jusqu'à `duree_s`, `source(i)` donnant l'échantillon
        /// d'entrée n° i ; `puits(t, bloc)` reçoit chaque bloc de sortie.
        fn tourner(
            &mut self,
            duree_s: f64,
            source: impl Fn(usize) -> f32,
            mut puits: impl FnMut(f64, &[f32]),
        ) {
            let mut be = vec![0.0f32; self.bloc_e];
            let mut bs = vec![0.0f32; self.bloc_s * 2];
            while self.t_s < duree_s {
                if self.t_e <= self.t_s {
                    for x in be.iter_mut() {
                        *x = source(self.n_e);
                        self.n_e += 1;
                    }
                    self.entree.pousser(&be);
                    self.t_e += self.bloc_e as f64 / self.sr_e;
                } else {
                    self.moteur.remplir(&mut bs, 2);
                    let mono: Vec<f32> = bs.chunks(2).map(|c| c[0]).collect();
                    puits(self.t_s, &mono);
                    self.t_s += self.bloc_s as f64 / self.sr_s;
                }
            }
        }
    }

    #[test]
    fn dix_minutes_sans_trou_malgre_la_derive() {
        // 48 kHz des deux côtés, l'entrée 80 ppm plus rapide, puis plus
        // lente : ni trou, ni recalage, ni échantillon jeté.
        for derive in [80e-6, -80e-6] {
            let mut sim = Simulation::new(48_000, 48_000, derive, 64, 128);
            let f = 110.0 / 48_000.0;
            let mut crete = 0.0f32;
            let mut saut = 0.0f32;
            let mut prec = 0.0f32;
            let mut total = 0usize;
            sim.tourner(
                600.0,
                |i| 0.5 * (std::f64::consts::TAU * f * i as f64).sin() as f32,
                |t, b| {
                    total += b.len();
                    if t > 1.0 {
                        for &x in b {
                            crete = crete.max(x.abs());
                            saut = saut.max((x - prec).abs());
                            prec = x;
                        }
                    } else if let Some(&x) = b.last() {
                        prec = x;
                    }
                },
            );
            let p = &sim.commandes.partage;
            assert!(total >= 600 * 48_000, "{total} échantillons rendus");
            eprintln!(
                "dérive {derive} : {total} échantillons, remplissage {:.1} / cible {}, saut {saut:.5}",
                p.remplissage(),
                sim.moteur.cible
            );
            assert_eq!(p.sous_alimentations(), 0, "dérive {derive}");
            assert_eq!(p.recales(), 0, "dérive {derive}");
            assert_eq!(p.jetes(), 0, "dérive {derive}");
            // Le remplissage reste près de la cible.
            assert!(
                (p.remplissage() - sim.moteur.cible as f64).abs() < 0.5 * sim.moteur.cible as f64,
                "remplissage {} pour {}",
                p.remplissage(),
                sim.moteur.cible
            );
            // Une sinusoïde à 110 Hz, crête 0,5 : jamais de saut plus grand
            // que sa pente maximale (un trou ou un recalage en ferait un).
            let pente = 0.5 * std::f32::consts::TAU * 110.0 / 48_000.0;
            assert!(saut < 1.5 * pente, "saut {saut} > pente {pente}");
            assert!((crete - 0.5).abs() < 0.02, "crête {crete}");
        }
    }

    #[test]
    fn des_frequences_differentes_se_rencontrent() {
        // Scarlett à 44,1 kHz, Mac à 48 kHz : même chose sur une minute.
        let mut sim = Simulation::new(44_100, 48_000, 30e-6, 64, 256);
        sim.tourner(60.0, |i| (i as f32 * 0.01).sin() * 0.3, |_, _| {});
        let p = &sim.commandes.partage;
        assert_eq!(p.sous_alimentations(), 0);
        assert_eq!(p.recales(), 0);
    }

    #[test]
    fn la_latence_du_moteur_est_courte() {
        // Une impulsion après 2 s de régime : mesurée de son entrée dans
        // l'anneau à sa sortie du moteur (la sortie compte le bloc qu'elle
        // remplit, comme un appareil).
        let mut sim = Simulation::new(48_000, 48_000, 0.0, 64, 128);
        let i0 = 96_000usize;
        let mut vue = None;
        let t_impulsion = std::cell::Cell::new(None::<f64>);
        let sr_e = sim.sr_e;
        sim.tourner(
            3.0,
            |i| if i == i0 { 1.0 } else { 0.0 },
            |t, b| {
                if vue.is_none() {
                    if let Some(k) = b.iter().position(|x| x.abs() > 0.3) {
                        vue = Some(t + k as f64 / 48_000.0);
                    }
                }
                let _ = &t_impulsion;
            },
        );
        // L'impulsion entre dans l'anneau à la fin de son bloc d'entrée.
        let bloc = (i0 / 64 + 1) * 64;
        let arrivee = bloc as f64 / sr_e;
        let latence_ms = (vue.expect("impulsion perdue") - arrivee) * 1000.0;
        let annoncee_ms = sim.moteur.latence_trames() / 48.0;
        eprintln!("latence du moteur : mesurée {latence_ms:.2} ms, annoncée {annoncee_ms:.2} ms");
        assert!(
            latence_ms > 0.0 && latence_ms < 8.0,
            "latence du moteur {latence_ms:.2} ms"
        );
        // L'annonce compte la cible entière : elle majore la mesure.
        assert!(
            latence_ms <= annoncee_ms + 0.5,
            "mesurée {latence_ms:.2} ms, annoncée {annoncee_ms:.2} ms"
        );
    }

    #[test]
    fn coupe_au_depart_et_volume_sans_clic() {
        let (mut e, mut m, c) = retour(48_000, 48_000, brut(48_000)).unwrap();
        // Départ : coupé, rien ne sort.
        let bloc = vec![0.5f32; 64];
        let mut s = vec![0.0f32; 128];
        for _ in 0..40 {
            e.pousser(&bloc);
            e.pousser(&bloc);
            m.remplir(&mut s, 1);
            assert!(s.iter().all(|x| *x == 0.0));
        }
        // Le volume monte par une rampe : jamais plus de 1/100 de la
        // consigne d'un échantillon au suivant (10 ms à 48 kHz).
        c.partage.couper(false);
        c.partage.regler_volume(1.0);
        let mut prec = 0.0f32;
        let mut pas = 0.0f32;
        for _ in 0..40 {
            e.pousser(&bloc);
            e.pousser(&bloc);
            m.remplir(&mut s, 1);
            for &x in &s {
                pas = pas.max((x - prec).abs());
                prec = x;
            }
        }
        assert!(pas < 0.5 * 0.01, "pas {pas}");
        assert!((prec - 0.5).abs() < 0.01, "niveau atteint {prec}");
        // Volume hors bornes ramené.
        c.partage.regler_volume(7.0);
        assert_eq!(c.partage.volume(), 1.0);
        c.partage.regler_volume(f32::NAN);
        assert_eq!(c.partage.volume(), 0.0);
    }

    #[test]
    fn la_chaine_change_dans_le_rappel() {
        let (mut e, mut m, mut c) = retour(48_000, 48_000, brut(48_000)).unwrap();
        c.partage.couper(false);
        c.partage.regler_volume(1.0);
        assert!(c.changer_chaine(Chaine::new(&Reglage::de(Prereglage::Overdrive), 48_000.0)));
        let bloc: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin() * 0.9).collect();
        let mut s = vec![0.0f32; 128];
        for _ in 0..20 {
            e.pousser(&bloc);
            e.pousser(&bloc);
            m.remplir(&mut s, 1);
        }
        assert!(c.chaines.slots() == 4, "chaîne non prise");
        assert!(s.iter().all(|x| x.abs() < 1.0));
    }

    #[test]
    fn un_retour_repris_vide_l_exces() {
        // Arrêté pendant 1 s, l'entrée a continué : à la reprise l'anneau
        // revient à la cible, une seule fois.
        let (mut e, mut m, c) = retour(48_000, 48_000, brut(48_000)).unwrap();
        let bloc = vec![0.1f32; 64];
        for _ in 0..750 {
            e.pousser(&bloc);
        }
        let mut s = vec![0.0f32; 128];
        m.remplir(&mut s, 1);
        assert_eq!(c.partage.recales(), 1);
        assert!(m.anneau.slots() <= m.cible);
    }
}
