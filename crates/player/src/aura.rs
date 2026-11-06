// SPDX-License-Identifier: GPL-3.0-or-later
//! L'aura de la carte (`docs/plan-balade.md`, P4) : les deux ou trois morceaux
//! les plus proches d'un point jouent ensemble, très bas, le plus proche
//! dominant, chacun placé à gauche ou à droite selon sa direction.
//!
//! Même principe que [`crate::multipiste`] : **une seule source qui somme les
//! voix**, raccordée au mélangeur de la sortie du [`crate::Player`]. Pas de
//! seconde sortie `rodio`, donc pas de dérive entre l'aura et le morceau
//! principal, et pas de passage par la file du lecteur : l'aura est une
//! sortie parallèle.
//!
//! Le fil audio ne bloque jamais. L'interface dépose la demande dans un
//! [`Depot`] ; la source la relève en début de bloc par `try_lock`, et la
//! relèvera au bloc suivant si le verrou est pris. Gains et panoramiques ne
//! sautent jamais : chaque voix glisse vers sa cible en [`FONDU`], à l'entrée
//! comme à la sortie.
//!
//! **Reprise.** Une voix quittée puis revisitée dans les [`REPRISE`] reprend
//! là où on l'avait laissée (le « entrer / sortir d'une zone » de SonicMaps) ;
//! au-delà, elle repart du début de son extrait. La tête de lecture vit dans
//! un atomique partagé : le fil audio l'avance, la poignée la garde après la
//! sortie de la voix.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::Source;

use crate::SR_EXTRAIT;

/// Voix jouées ensemble, au plus.
pub const VOIX_MAX: usize = 3;
/// Fondu d'entrée et de sortie d'une voix, et de tout changement de gain.
pub const FONDU: Duration = Duration::from_millis(300);
/// Une voix revisitée avant ce délai reprend où elle en était.
pub const REPRISE: Duration = Duration::from_secs(30);
/// Extraits gardés en mémoire (mono, `i16`, 15 s : 1,4 Mo chacun).
const RETENUS_MAX: usize = 24;
/// Fondu aux deux bords d'un extrait, en trames : il boucle, et la reprise
/// au début ne doit pas claquer.
const BORD: usize = (SR_EXTRAIT as usize * 30) / 1000;
/// Trames entre deux relèves du dépôt (1,3 ms).
const BLOC: usize = 64;

/// Une voix demandée : quel morceau, à quel gain (0 à 1), où (−1 gauche,
/// +1 droite).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VoixAura {
    pub id: i64,
    pub gain: f32,
    pub pan: f32,
}

/// Une voix telle que la source la reçoit.
#[derive(Clone)]
struct Entree {
    id: i64,
    extrait: Arc<[i16]>,
    tete: Arc<AtomicUsize>,
    gain: f32,
    pan: f32,
}

/// Ce que la poignée dépose et que la source relève.
struct Depot {
    nouveau: AtomicBool,
    voix: Mutex<Vec<Entree>>,
    /// Niveau global, `f32` en bits : le plafond de l'aura.
    niveau: AtomicU32,
}

/// Un extrait chargé, et ce qu'on sait de son dernier passage.
struct Retenu {
    extrait: Arc<[i16]>,
    tete: Arc<AtomicUsize>,
    /// Quand la voix s'est tue ; `None` tant qu'elle joue.
    quittee: Option<Instant>,
}

/// La poignée, côté application : charge les extraits, dit qui joue.
pub struct Aura {
    depot: Arc<Depot>,
    retenus: HashMap<i64, Retenu>,
    actives: Vec<i64>,
}

impl Default for Aura {
    fn default() -> Self {
        Self::new()
    }
}

impl Aura {
    pub fn new() -> Self {
        Self {
            depot: Arc::new(Depot {
                nouveau: AtomicBool::new(false),
                voix: Mutex::new(Vec::with_capacity(VOIX_MAX)),
                niveau: AtomicU32::new(0.0f32.to_bits()),
            }),
            retenus: HashMap::new(),
            actives: Vec::new(),
        }
    }

    /// Une source à raccorder au mélangeur. Une nouvelle à chaque
    /// réouverture de la sortie : elle reprend la demande en cours.
    pub fn source(&self) -> SourceAura {
        self.depot.nouveau.store(true, Ordering::Release);
        SourceAura {
            depot: Arc::clone(&self.depot),
            voix: Vec::with_capacity(2 * VOIX_MAX),
            niveau: 0.0,
            trame: [0.0; 2],
            canal: 0,
            avant_releve: 0,
        }
    }

    /// L'extrait de `id` est déjà en mémoire.
    pub fn connait(&self, id: i64) -> bool {
        self.retenus.contains_key(&id)
    }

    /// Garde l'extrait de `id` (mono à [`SR_EXTRAIT`]), bords adoucis.
    pub fn retenir(&mut self, id: i64, mono: &[f32]) {
        let n = mono.len();
        let bord = BORD.min(n / 2).max(1);
        let extrait: Arc<[i16]> = mono
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let g = (i.min(n - 1 - i) as f32 / bord as f32).min(1.0);
                (x * g * f32::from(i16::MAX)).clamp(-32767.0, 32767.0) as i16
            })
            .collect();
        self.retenus.insert(
            id,
            Retenu {
                extrait,
                tete: Arc::new(AtomicUsize::new(0)),
                quittee: Some(Instant::now()),
            },
        );
        self.oublier_les_plus_anciens();
    }

    /// Les voix qui doivent jouer, à la place des précédentes. Une voix dont
    /// l'extrait n'est pas retenu est ignorée ; au-delà de [`VOIX_MAX`], les
    /// suivantes aussi. Les voix absentes de la demande s'éteignent en fondu.
    pub fn jouer(&mut self, demande: &[VoixAura]) {
        let maintenant = Instant::now();
        let mut entrees = Vec::with_capacity(VOIX_MAX);
        let mut actives = Vec::with_capacity(VOIX_MAX);
        for v in demande {
            if entrees.len() == VOIX_MAX || actives.contains(&v.id) {
                continue;
            }
            let Some(r) = self.retenus.get_mut(&v.id) else {
                continue;
            };
            if !self.actives.contains(&v.id) {
                // Elle revient : où en était-elle ?
                if r.quittee.is_some_and(|q| maintenant - q > REPRISE) {
                    r.tete.store(0, Ordering::Relaxed);
                }
                r.quittee = None;
            }
            actives.push(v.id);
            entrees.push(Entree {
                id: v.id,
                extrait: Arc::clone(&r.extrait),
                tete: Arc::clone(&r.tete),
                gain: v.gain.clamp(0.0, 1.0),
                pan: v.pan.clamp(-1.0, 1.0),
            });
        }
        for id in &self.actives {
            if !actives.contains(id) {
                if let Some(r) = self.retenus.get_mut(id) {
                    r.quittee = Some(maintenant);
                }
            }
        }
        self.actives = actives;
        *self.depot.voix.lock().unwrap_or_else(|e| e.into_inner()) = entrees;
        self.depot.nouveau.store(true, Ordering::Release);
    }

    /// Tout se tait, en fondu.
    pub fn taire(&mut self) {
        self.jouer(&[]);
    }

    /// Les morceaux qui jouent, dans l'ordre de la demande.
    pub fn actives(&self) -> &[i64] {
        &self.actives
    }

    /// Plafond global de l'aura (0 à 1), atteint lui aussi en fondu.
    pub fn regler_niveau(&self, niveau: f32) {
        self.depot
            .niveau
            .store(niveau.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn oublier_les_plus_anciens(&mut self) {
        while self.retenus.len() > RETENUS_MAX {
            let Some(id) = self
                .retenus
                .iter()
                .filter_map(|(id, r)| r.quittee.map(|q| (*id, q)))
                .min_by_key(|(_, q)| *q)
                .map(|(id, _)| id)
            else {
                break;
            };
            self.retenus.remove(&id);
        }
    }
}

/// Une voix dans le fil audio : sa cible, et où en sont ses glissements.
struct Voix {
    id: i64,
    extrait: Arc<[i16]>,
    tete: Arc<AtomicUsize>,
    gain_cible: f32,
    pan_cible: f32,
    gain: f32,
    pan: f32,
}

/// Pas d'un glissement, par trame : de 0 à 1 en [`FONDU`].
fn pas_de_fondu() -> f32 {
    1.0 / (FONDU.as_secs_f32() * SR_EXTRAIT as f32)
}

fn glisser(x: &mut f32, cible: f32, pas: f32) {
    if (*x - cible).abs() <= pas {
        *x = cible;
    } else if *x < cible {
        *x += pas;
    } else {
        *x -= pas;
    }
}

/// La source qui somme les voix, en stéréo à [`SR_EXTRAIT`]. Elle ne finit
/// jamais : sans voix, elle rend du silence, et le mélangeur la garde.
pub struct SourceAura {
    depot: Arc<Depot>,
    voix: Vec<Voix>,
    niveau: f32,
    trame: [f32; 2],
    canal: usize,
    avant_releve: usize,
}

impl SourceAura {
    /// Relève la demande, si elle a changé et que le verrou est libre.
    fn relever(&mut self) {
        if !self.depot.nouveau.swap(false, Ordering::Acquire) {
            return;
        }
        let Ok(demande) = self.depot.voix.try_lock() else {
            self.depot.nouveau.store(true, Ordering::Release);
            return;
        };
        for v in &mut self.voix {
            v.gain_cible = 0.0;
        }
        for e in demande.iter() {
            match self.voix.iter_mut().find(|v| v.id == e.id) {
                Some(v) => {
                    v.gain_cible = e.gain;
                    v.pan_cible = e.pan;
                    // Un extrait rechargé entre-temps : on suit le nouveau.
                    if !Arc::ptr_eq(&v.extrait, &e.extrait) {
                        v.extrait = Arc::clone(&e.extrait);
                        v.tete = Arc::clone(&e.tete);
                    }
                }
                None => self.voix.push(Voix {
                    id: e.id,
                    extrait: Arc::clone(&e.extrait),
                    tete: Arc::clone(&e.tete),
                    gain_cible: e.gain,
                    pan_cible: e.pan,
                    gain: 0.0,
                    pan: e.pan,
                }),
            }
        }
    }

    /// Calcule la trame suivante (gauche, droite).
    fn trame_suivante(&mut self) {
        if self.avant_releve == 0 {
            self.relever();
            self.avant_releve = BLOC;
        }
        self.avant_releve -= 1;
        let pas = pas_de_fondu();
        glisser(
            &mut self.niveau,
            f32::from_bits(self.depot.niveau.load(Ordering::Relaxed)),
            pas,
        );
        let (mut g, mut d) = (0.0f32, 0.0f32);
        for v in &mut self.voix {
            glisser(&mut v.gain, v.gain_cible, pas);
            glisser(&mut v.pan, v.pan_cible, pas);
            let n = v.extrait.len();
            if n == 0 {
                continue;
            }
            let i = v.tete.load(Ordering::Relaxed) % n;
            let x = f32::from(v.extrait[i]) / f32::from(i16::MAX) * v.gain;
            v.tete.store(i + 1, Ordering::Relaxed);
            // Panoramique à puissance constante.
            let angle = (v.pan + 1.0) * std::f32::consts::FRAC_PI_4;
            g += x * angle.cos();
            d += x * angle.sin();
        }
        // Une voix éteinte quitte la somme ; sa tête reste dans la poignée.
        self.voix.retain(|v| v.gain > 0.0 || v.gain_cible > 0.0);
        self.trame = [g * self.niveau, d * self.niveau];
    }
}

impl Iterator for SourceAura {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.canal == 0 {
            self.trame_suivante();
        }
        let x = self.trame[self.canal];
        self.canal = (self.canal + 1) % 2;
        Some(x)
    }
}

impl Source for SourceAura {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        2.try_into().expect("2 canaux")
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        SR_EXTRAIT.try_into().expect("48 kHz")
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNE_SECONDE: usize = SR_EXTRAIT as usize;

    fn trames(s: &mut SourceAura, n: usize) -> Vec<[f32; 2]> {
        (0..n)
            .map(|_| [s.next().unwrap(), s.next().unwrap()])
            .collect()
    }

    /// Un extrait constant : ce qui sort est le gain, à lire directement.
    fn aura_constante(ids: &[i64]) -> (Aura, SourceAura) {
        let mut a = Aura::new();
        for &id in ids {
            a.retenir(id, &vec![0.5; 2 * UNE_SECONDE]);
        }
        a.regler_niveau(1.0);
        let s = a.source();
        (a, s)
    }

    #[test]
    fn une_voix_entre_en_fondu_sans_sauter() {
        let (mut a, mut s) = aura_constante(&[1]);
        a.jouer(&[VoixAura {
            id: 1,
            gain: 1.0,
            pan: 0.0,
        }]);
        let t = trames(&mut s, UNE_SECONDE / 2);
        // Ni saut d'une trame à l'autre, ni plein niveau avant 300 ms.
        let max_saut = t
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_saut < 0.001, "saut {max_saut}");
        let a_100ms = t[UNE_SECONDE / 10][0];
        let a_400ms = t[UNE_SECONDE * 4 / 10][0];
        assert!(a_100ms < a_400ms * 0.5, "{a_100ms} puis {a_400ms}");
        // Au centre, puissance constante : 0,5 × cos(π/4) de chaque côté.
        assert!((a_400ms - 0.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((t[UNE_SECONDE * 4 / 10][1] - a_400ms).abs() < 1e-6);
    }

    #[test]
    fn le_panoramique_place_la_voix_et_le_gain_la_dose() {
        let (mut a, mut s) = aura_constante(&[1, 2]);
        a.jouer(&[
            VoixAura {
                id: 1,
                gain: 1.0,
                pan: -1.0,
            },
            VoixAura {
                id: 2,
                gain: 0.25,
                pan: 1.0,
            },
        ]);
        let t = trames(&mut s, UNE_SECONDE / 2);
        let [g, d] = t[t.len() - 1];
        assert!((g - 0.5).abs() < 1e-3, "toute la première à gauche : {g}");
        assert!(
            (d - 0.125).abs() < 1e-3,
            "la seconde, au quart, à droite : {d}"
        );
    }

    #[test]
    fn une_voix_quittee_s_eteint_en_fondu_puis_quitte_la_somme() {
        let (mut a, mut s) = aura_constante(&[1, 2]);
        let un = VoixAura {
            id: 1,
            gain: 1.0,
            pan: 0.0,
        };
        a.jouer(&[un]);
        trames(&mut s, UNE_SECONDE / 2);
        a.jouer(&[VoixAura { id: 2, ..un }]);
        let t = trames(&mut s, UNE_SECONDE / 2);
        let max_saut = t
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_saut < 0.001, "le croisement ne claque pas : {max_saut}");
        assert_eq!(s.voix.len(), 1, "la voix éteinte est sortie de la somme");
        assert_eq!(a.actives(), &[2]);
    }

    #[test]
    fn une_voix_revisitee_reprend_ou_elle_en_etait() {
        let (mut a, mut s) = aura_constante(&[1, 2]);
        let un = VoixAura {
            id: 1,
            gain: 1.0,
            pan: 0.0,
        };
        a.jouer(&[un]);
        trames(&mut s, UNE_SECONDE);
        a.jouer(&[VoixAura { id: 2, ..un }]);
        trames(&mut s, UNE_SECONDE / 2);
        let tete = a.retenus[&1].tete.load(Ordering::Relaxed);
        assert!(
            tete > UNE_SECONDE,
            "elle a joué plus d'une seconde : {tete}"
        );
        a.jouer(&[un]);
        assert_eq!(a.retenus[&1].tete.load(Ordering::Relaxed), tete);

        // Passé le délai de reprise, elle repart du début.
        a.jouer(&[VoixAura { id: 2, ..un }]);
        a.retenus.get_mut(&1).unwrap().quittee = Some(Instant::now() - REPRISE * 2);
        a.jouer(&[un]);
        assert_eq!(a.retenus[&1].tete.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn trois_voix_au_plus_et_jamais_un_inconnu() {
        let (mut a, _s) = aura_constante(&[1, 2, 3, 4]);
        let v = |id| VoixAura {
            id,
            gain: 0.5,
            pan: 0.0,
        };
        a.jouer(&[v(9), v(1), v(1), v(2), v(3), v(4)]);
        assert_eq!(a.actives(), &[1, 2, 3]);
    }

    #[test]
    fn la_memoire_oublie_d_abord_ce_qui_s_est_tu_le_plus_tot() {
        let mut a = Aura::new();
        for id in 0..RETENUS_MAX as i64 + 4 {
            a.retenir(id, &[0.1; 64]);
        }
        assert_eq!(a.retenus.len(), RETENUS_MAX);
        assert!(!a.connait(0) && a.connait(RETENUS_MAX as i64 + 3));
    }
}
