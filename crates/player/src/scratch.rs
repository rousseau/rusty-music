// SPDX-License-Identifier: GPL-3.0-or-later
//! Le mode « scratch » du vinyle : une main sur le disque.
//!
//! Le lecteur principal (`rodio::Player`) ne sait lire qu'en avant et à
//! vitesse fixe. Le temps du mode, il est mis en pause et une **source à
//! vitesse signée** prend sa place sur le mélangeur de la sortie, comme
//! l'aura ([`crate::aura`]) : même sortie, pas de second flux.
//!
//! - Le morceau en cours est décodé en mémoire **tel que la sortie le joue**
//!   (gain de normalisation, rééchantillonnage, « E », cache « HD » : on
//!   passe par [`crate::ouvrir`]). La tête de lecture y est un réel, en
//!   trames ; chaque trame est interpolée (Catmull-Rom, quatre points).
//! - La main dépose une vitesse cible (1 = vitesse du disque, négative =
//!   marche arrière) dans un atomique, à ~60 Hz ; le fil audio la lisse
//!   ([`TAU_MAIN`]) — les marches de 16 ms ne s'entendent pas.
//! - Main levée, le moteur ramène le disque à sa vitesse (1 en lecture, 0 en
//!   pause) à accélération bornée, comme une platine : de l'arrêt à 33 ⅓ en
//!   [`DEMARRAGE`].
//! - Un passe-haut à 10 Hz retire la composante continue qu'un disque arrêté
//!   sur un échantillon non nul laisserait — à l'arrêt, une cellule ne rend
//!   rien.
//! - L'entrée et la sortie se font en fondu court ([`FONDU`]) : le lecteur
//!   principal reprend à la tête du disque ([`crate::Player::sortir_scratch`]).
//!
//! Le fil audio ne bloque jamais : tout passe par des atomiques.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::Source;

/// Lissage de la vitesse de la main : constante de temps.
pub const TAU_MAIN: Duration = Duration::from_millis(12);
/// Durée du démarrage du moteur, de l'arrêt à la vitesse nominale, à
/// couple plein : elle fixe l'accélération maximale. L'asservissement
/// ([`TAU_ASSERV`], [`TAU_COUPLE`]) arrondit le départ et l'arrivée.
pub const DEMARRAGE: Duration = Duration::from_millis(350);
/// Asservissement de vitesse du moteur : constante de temps de la boucle.
pub const TAU_ASSERV: Duration = Duration::from_millis(120);
/// Le couple du moteur ne s'établit pas instantanément : pas de coude
/// audible au lâcher ni à l'arrivée à la vitesse nominale.
pub const TAU_COUPLE: Duration = Duration::from_millis(40);
/// Fondu d'entrée et de sortie du mode.
pub const FONDU: Duration = Duration::from_millis(6);
/// Vitesse de la main, en valeur absolue, au plus (36 fois la nominale).
pub const VITESSE_MAX: f32 = 36.0;
/// Trames entre deux relèves des atomiques.
const BLOC: usize = 32;
/// Passe-haut de la composante continue.
const COUPURE_HZ: f32 = 10.0;

/// Un morceau décodé pour le mode scratch.
pub struct Disque {
    pcm: Arc<[f32]>,
    canaux: usize,
    taux: u32,
}

impl Disque {
    /// Entrelacé, à `taux` Hz.
    pub fn new(pcm: Vec<f32>, canaux: u16, taux: u32) -> Self {
        Self {
            pcm: pcm.into(),
            canaux: usize::from(canaux.max(1)),
            taux: taux.max(1),
        }
    }

    /// Décode `chemin` comme le lecteur l'ouvrirait ([`crate::ouvrir`]).
    pub fn decoder(chemin: &std::path::Path, gain_lineaire: f32) -> crate::Result<Self> {
        let source = crate::ouvrir(chemin, gain_lineaire)?;
        let canaux = source.channels().get();
        let taux = source.sample_rate().get();
        Ok(Self::new(source.collect(), canaux, taux))
    }

    fn trames(&self) -> usize {
        self.pcm.len() / self.canaux
    }
}

/// Ce que la poignée et la source partagent.
struct Partage {
    /// Vitesse voulue par la main (`f32` en bits), valable si `main`.
    cible: AtomicU32,
    main: AtomicBool,
    /// Vitesse du moteur, main levée : 1 en lecture, 0 en pause.
    moteur: AtomicU32,
    /// Volume du lecteur (`f32` en bits).
    volume: AtomicU32,
    /// Saut demandé, en trames (`f64` en bits) ; `u64::MAX` : aucun.
    saut: AtomicU64,
    arret: AtomicBool,
    /// Écrits par le fil audio : tête (trames, `f64`) et vitesse (`f32`).
    tete: AtomicU64,
    vitesse: AtomicU32,
    /// Le moteur a mené le disque au bout du morceau.
    fini: AtomicBool,
    /// Fondu de sortie achevé : la source s'est tue.
    tue: AtomicBool,
    /// Latence geste → fil audio : instant (µs depuis `origine`) de la
    /// dernière cible déposée, et délai mesuré à sa relève (µs).
    depose_us: AtomicU64,
    releve_us: AtomicU64,
    origine: Instant,
}

const AUCUN_SAUT: u64 = u64::MAX;

fn f32_de(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// La poignée du mode, côté [`crate::Player`].
pub struct Platine {
    partage: Arc<Partage>,
    taux: u32,
}

/// L'état du disque, pour l'interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EtatDisque {
    pub position: Duration,
    pub vitesse: f32,
    /// Délai entre la dernière vitesse déposée et sa prise en compte par le
    /// fil audio ; le tampon de la sortie s'y ajoute (`latence_sortie`).
    pub latence_main: Duration,
}

impl Platine {
    /// La poignée et la source à raccorder au mélangeur. `depart` : la
    /// position du lecteur principal ; `en_lecture` : le moteur tourne.
    pub fn new(
        disque: Disque,
        depart: Duration,
        en_lecture: bool,
        volume: f32,
    ) -> (Self, SourceScratch) {
        let moteur = if en_lecture { 1.0f32 } else { 0.0 };
        let tete = (depart.as_secs_f64() * f64::from(disque.taux))
            .clamp(0.0, disque.trames().saturating_sub(1) as f64);
        let partage = Arc::new(Partage {
            cible: AtomicU32::new(moteur.to_bits()),
            main: AtomicBool::new(false),
            moteur: AtomicU32::new(moteur.to_bits()),
            volume: AtomicU32::new(volume.to_bits()),
            saut: AtomicU64::new(AUCUN_SAUT),
            arret: AtomicBool::new(false),
            tete: AtomicU64::new(tete.to_bits()),
            vitesse: AtomicU32::new(moteur.to_bits()),
            fini: AtomicBool::new(false),
            tue: AtomicBool::new(false),
            depose_us: AtomicU64::new(0),
            releve_us: AtomicU64::new(0),
            origine: Instant::now(),
        });
        let taux = disque.taux;
        let source = SourceScratch::new(disque, Arc::clone(&partage), tete, moteur, volume);
        (Self { partage, taux }, source)
    }

    /// La main sur le disque, à `vitesse` (1 = nominale, négative = arrière),
    /// ou levée (`None`) : le moteur reprend.
    pub fn main(&self, vitesse: Option<f32>) {
        let p = &self.partage;
        match vitesse {
            Some(v) if v.is_finite() => {
                let v = v.clamp(-VITESSE_MAX, VITESSE_MAX);
                p.cible.store(v.to_bits(), Ordering::Relaxed);
                let t = p.origine.elapsed().as_micros() as u64;
                p.depose_us.store(t.max(1), Ordering::Relaxed);
                p.main.store(true, Ordering::Release);
            }
            _ => p.main.store(false, Ordering::Release),
        }
    }

    /// Le moteur tourne (lecture) ou s'arrête (pause).
    pub fn regler_moteur(&self, en_lecture: bool) {
        let v = if en_lecture { 1.0f32 } else { 0.0 };
        self.partage.moteur.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn moteur(&self) -> bool {
        f32_de(&self.partage.moteur) > 0.0
    }

    pub fn regler_volume(&self, volume: f32) {
        self.partage
            .volume
            .store(volume.to_bits(), Ordering::Relaxed);
    }

    /// Pose la tête à `pos` (seek pendant le mode).
    pub fn sauter(&self, pos: Duration) {
        let t = pos.as_secs_f64() * f64::from(self.taux);
        self.partage.saut.store(t.to_bits(), Ordering::Release);
    }

    /// Tête du disque.
    pub fn position(&self) -> Duration {
        let saut = self.partage.saut.load(Ordering::Acquire);
        let t = if saut == AUCUN_SAUT {
            f64::from_bits(self.partage.tete.load(Ordering::Acquire))
        } else {
            f64::from_bits(saut)
        };
        Duration::from_secs_f64((t / f64::from(self.taux)).max(0.0))
    }

    pub fn etat(&self) -> EtatDisque {
        EtatDisque {
            position: self.position(),
            vitesse: f32_de(&self.partage.vitesse),
            latence_main: Duration::from_micros(self.partage.releve_us.load(Ordering::Relaxed)),
        }
    }

    /// Le moteur a mené le disque au bout : le lecteur principal doit
    /// reprendre la main (fin du morceau, enchaînement).
    pub fn fini(&self) -> bool {
        self.partage.fini.load(Ordering::Acquire)
    }

    /// Fait taire la source (fondu de [`FONDU`]).
    pub fn arreter(&self) {
        self.partage.arret.store(true, Ordering::Release);
    }
}

impl Drop for Platine {
    fn drop(&mut self) {
        self.arreter();
    }
}

/// La source raccordée au mélangeur.
pub struct SourceScratch {
    disque: Disque,
    partage: Arc<Partage>,
    /// Tête, en trames.
    pos: f64,
    /// Vitesse lissée.
    v: f32,
    cible: f32,
    main: bool,
    volume: f32,
    vol: f32,
    /// Enveloppe de fondu, 0..1, et son pas par trame.
    env: f32,
    pas_env: f32,
    arret: bool,
    k_main: f32,
    pas_moteur: f32,
    /// Accélération du moteur, en vitesse par trame, et ses deux gains.
    acc: f32,
    k_asserv: f32,
    k_couple: f32,
    r_hp: f32,
    /// État du passe-haut, par canal : entrée et sortie précédentes.
    hp_x: Vec<f32>,
    hp_y: Vec<f32>,
    trame: Vec<f32>,
    canal: usize,
    compte: usize,
    fini: bool,
}

impl SourceScratch {
    fn new(disque: Disque, partage: Arc<Partage>, pos: f64, v: f32, volume: f32) -> Self {
        let fs = disque.taux as f32;
        let canaux = disque.canaux;
        Self {
            partage,
            pos,
            v,
            cible: v,
            main: false,
            volume,
            vol: volume,
            env: 0.0,
            pas_env: 1.0 / (FONDU.as_secs_f32() * fs).max(1.0),
            arret: false,
            k_main: 1.0 - (-1.0 / (TAU_MAIN.as_secs_f32() * fs)).exp(),
            pas_moteur: 1.0 / (DEMARRAGE.as_secs_f32() * fs),
            acc: 0.0,
            k_asserv: 1.0 / (TAU_ASSERV.as_secs_f32() * fs),
            k_couple: 1.0 - (-1.0 / (TAU_COUPLE.as_secs_f32() * fs)).exp(),
            r_hp: 1.0 - 2.0 * std::f32::consts::PI * COUPURE_HZ / fs,
            hp_x: vec![0.0; canaux],
            hp_y: vec![0.0; canaux],
            trame: vec![0.0; canaux],
            canal: 0,
            compte: 0,
            fini: false,
            disque,
        }
    }

    fn relever(&mut self) {
        let p = &self.partage;
        let saut = p.saut.swap(AUCUN_SAUT, Ordering::Acquire);
        if saut != AUCUN_SAUT {
            self.pos = f64::from_bits(saut);
        }
        self.main = p.main.load(Ordering::Acquire);
        self.cible = if self.main {
            let depose = p.depose_us.swap(0, Ordering::Relaxed);
            if depose != 0 {
                let maintenant = p.origine.elapsed().as_micros() as u64;
                p.releve_us
                    .store(maintenant.saturating_sub(depose), Ordering::Relaxed);
            }
            f32_de(&p.cible)
        } else {
            f32_de(&p.moteur)
        };
        self.volume = f32_de(&p.volume);
        self.arret |= p.arret.load(Ordering::Acquire);
        p.tete.store(self.pos.to_bits(), Ordering::Release);
        p.vitesse.store(self.v.to_bits(), Ordering::Relaxed);
    }

    /// Avance la vitesse d'une trame vers sa cible.
    ///
    /// Main posée : la vitesse suit la main (lissage de [`TAU_MAIN`]). Main
    /// levée : un moteur asservi, du second ordre — l'accélération visée est
    /// proportionnelle à l'écart, bornée par le couple (plus fort pour
    /// freiner : une main qui jette le disque en arrière est vite reprise),
    /// et l'accélération réelle la rejoint en [`TAU_COUPLE`]. La vitesse
    /// glisse ainsi sans coude, au lâcher comme à l'arrivée.
    fn vitesse_suivante(&mut self) {
        let ecart = self.cible - self.v;
        if self.main {
            self.v += ecart * self.k_main;
            self.acc = 0.0;
            return;
        }
        if ecart.abs() < 1e-5 && self.acc.abs() < 1e-8 {
            self.v = self.cible;
            self.acc = 0.0;
            return;
        }
        let freine = ecart * self.v < 0.0 || self.v.abs() > 1.0;
        let borne = if freine {
            self.pas_moteur * 2.0
        } else {
            self.pas_moteur
        };
        let visee = (ecart * self.k_asserv).clamp(-borne, borne);
        self.acc += (visee - self.acc) * self.k_couple;
        self.v += self.acc;
    }

    /// Échantillon du canal `c` à la position `x` (trames), Catmull-Rom.
    fn interpoler(&self, x: f64, c: usize) -> f32 {
        let n = self.disque.trames() as isize;
        let i = x.floor() as isize;
        let t = (x - i as f64) as f32;
        let at = |k: isize| -> f32 {
            let k = k.clamp(0, n - 1) as usize;
            self.disque.pcm[k * self.disque.canaux + c]
        };
        let (y0, y1, y2, y3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
        let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let cc = -0.5 * y0 + 0.5 * y2;
        ((a * t + b) * t + cc) * t + y1
    }

    /// Calcule la trame suivante ; `false` quand la source s'est tue.
    fn trame_suivante(&mut self) -> bool {
        if self.compte.is_multiple_of(BLOC) {
            self.relever();
        }
        self.compte = self.compte.wrapping_add(1);

        if self.arret {
            self.env -= self.pas_env;
            if self.env <= 0.0 {
                self.partage
                    .tete
                    .store(self.pos.to_bits(), Ordering::Release);
                self.partage.tue.store(true, Ordering::Release);
                return false;
            }
        } else if self.env < 1.0 {
            self.env = (self.env + self.pas_env).min(1.0);
        }
        self.vol += (self.volume - self.vol) * 0.002;

        self.vitesse_suivante();
        let dernier = self.disque.trames().saturating_sub(1) as f64;
        self.pos += f64::from(self.v);
        if self.pos <= 0.0 {
            self.pos = 0.0;
            self.v = self.v.max(0.0);
        } else if self.pos >= dernier {
            self.pos = dernier;
            self.v = self.v.min(0.0);
            if !self.main && self.cible > 0.0 && !self.fini {
                self.fini = true;
                self.partage.fini.store(true, Ordering::Release);
            }
        }

        let gain = self.env * self.vol;
        for c in 0..self.disque.canaux {
            let x = self.interpoler(self.pos, c);
            let y = x - self.hp_x[c] + self.r_hp * self.hp_y[c];
            self.hp_x[c] = x;
            self.hp_y[c] = y;
            self.trame[c] = y * gain;
        }
        true
    }
}

impl Iterator for SourceScratch {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.canal == 0 && !self.trame_suivante() {
            return None;
        }
        let x = self.trame[self.canal];
        self.canal = (self.canal + 1) % self.disque.canaux;
        Some(x)
    }
}

impl Source for SourceScratch {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        u16::try_from(self.disque.canaux)
            .ok()
            .and_then(|c| c.try_into().ok())
            .expect("au moins un canal")
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.disque.taux.try_into().expect("fréquence non nulle")
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// Une rampe mono : l'échantillon vaut son rang, on lit la tête dedans.
    fn rampe(n: usize) -> Disque {
        Disque::new((0..n).map(|i| i as f32).collect(), 1, SR)
    }

    fn tirer(s: &mut SourceScratch, n: usize) -> Vec<f32> {
        s.by_ref().take(n).collect()
    }

    #[test]
    fn vitesse_nominale_avance_dune_trame_par_trame() {
        let (p, mut s) = Platine::new(rampe(SR as usize), Duration::from_millis(100), true, 1.0);
        tirer(&mut s, 4800);
        let pos = p.position().as_secs_f64();
        assert!((pos - 0.2).abs() < 0.002, "{pos}");
    }

    #[test]
    fn la_main_fait_reculer_le_disque() {
        let (p, mut s) = Platine::new(rampe(SR as usize), Duration::from_millis(500), true, 1.0);
        p.main(Some(-1.0));
        tirer(&mut s, 9600); // 200 ms
        let pos = p.position().as_secs_f64();
        assert!(pos < 0.36, "le disque n'a pas reculé : {pos}");
        assert!(p.etat().vitesse < -0.9);
    }

    #[test]
    fn main_levee_le_moteur_reprend_en_quelques_centaines_de_ms() {
        let (p, mut s) = Platine::new(rampe(SR as usize * 4), Duration::from_secs(1), true, 1.0);
        p.main(Some(0.0));
        tirer(&mut s, 4800);
        assert!(p.etat().vitesse.abs() < 0.01);
        p.main(None);
        // Toutes les 2 ms : la vitesse monte sans dépasser, et sans coude
        // (l'accélération ne saute jamais d'une mesure à l'autre).
        let mut vitesses = Vec::new();
        for _ in 0..500 {
            tirer(&mut s, 96);
            vitesses.push(s.v);
        }
        let a_500ms = vitesses[249];
        assert!(a_500ms > 0.85, "trop lent : {a_500ms} à 500 ms");
        let max = vitesses.iter().copied().fold(0.0f32, f32::max);
        assert!(max < 1.02, "dépassement : {max}");
        let acc: Vec<f32> = vitesses.windows(2).map(|w| w[1] - w[0]).collect();
        let saut = acc
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(saut < 1e-3, "coude : {saut}");
        assert!((vitesses[499] - 1.0).abs() < 1e-3, "{}", vitesses[499]);
    }

    #[test]
    fn lance_vite_puis_lache_ne_retombe_pas_a_zero() {
        let (p, mut s) = Platine::new(rampe(SR as usize * 8), Duration::from_secs(1), true, 1.0);
        p.main(Some(3.0));
        tirer(&mut s, 4800);
        p.main(None);
        let mut min = f32::MAX;
        for _ in 0..200 {
            tirer(&mut s, 480);
            min = min.min(s.v);
        }
        assert!(min > 0.98, "le disque est retombé à {min}");
        assert!((s.v - 1.0).abs() < 1e-3);
    }

    #[test]
    fn en_pause_le_moteur_arrete_le_disque() {
        let (p, mut s) = Platine::new(rampe(SR as usize), Duration::from_millis(100), false, 1.0);
        tirer(&mut s, 4800);
        assert!((p.position().as_secs_f64() - 0.1).abs() < 1e-3);
    }

    #[test]
    fn arret_en_fondu_puis_silence() {
        let (p, mut s) = Platine::new(rampe(SR as usize), Duration::ZERO, true, 1.0);
        tirer(&mut s, 1000);
        p.arreter();
        let reste = s.by_ref().count();
        assert!(
            reste <= (FONDU.as_secs_f32() * SR as f32) as usize + BLOC + 1,
            "{reste}"
        );
    }

    #[test]
    fn le_bout_du_morceau_est_signale() {
        let (p, mut s) = Platine::new(rampe(4800), Duration::from_millis(90), true, 1.0);
        tirer(&mut s, 2000);
        assert!(p.fini());
    }

    #[test]
    fn catmull_rom_passe_par_les_points_et_suit_une_droite() {
        let (_p, s) = Platine::new(rampe(100), Duration::ZERO, true, 1.0);
        assert_eq!(s.interpoler(10.0, 0), 10.0);
        assert!((s.interpoler(10.25, 0) - 10.25).abs() < 1e-5);
    }

    #[test]
    fn pas_de_composante_continue_disque_arrete() {
        // Un signal constant : à l'arrêt, la sortie retombe vers zéro.
        let d = Disque::new(vec![0.5; SR as usize], 1, SR);
        let (p, mut s) = Platine::new(d, Duration::from_millis(500), true, 1.0);
        p.main(Some(0.0));
        let x = tirer(&mut s, SR as usize / 2);
        assert!(x.last().unwrap().abs() < 0.01, "{}", x.last().unwrap());
    }
}
