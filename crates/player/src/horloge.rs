// SPDX-License-Identifier: GPL-3.0-or-later
//! L'horloge de pratique : quel instant du morceau le moteur rend, et quand.
//!
//! Le coach de Pratiquer (E4, `docs/recherche-coach-pratique.md`) compare
//! chaque coup joué à l'instant où la référence l'attendait. Les coups
//! arrivent horodatés par le système (MIDI, entrée audio) ; la musique, elle,
//! avance au pas du fil audio. Ce module les met sur une même horloge,
//! [`maintenant_s`], et publie une **ancre** : tel instant du morceau a été
//! rendu à tel instant de cette horloge, à telle vitesse. Le reste
//! (projection des notes, boucle, latence) est du calcul pur, dans
//! `rusty_music_editor::coach`.
//!
//! **Ce que vaut l'ancre.** Le fil audio calcule les trames par paquets, en
//! avance sur la sortie : l'instant où une trame est calculée flotte d'un
//! tampon. La sortie, elle, rend les trames à cadence fixe, trame n à
//! T₀ + n/SR. Une trame n'est jamais calculée après sa sortie ; celles qui le
//! sont « juste à temps » donnent le plus grand `instant − n/SR`. On en garde
//! donc le **maximum** sur une fenêtre glissante de une à deux secondes :
//! l'enveloppe haute se cale sur la cadence de la sortie, sans la gigue des
//! paquets, et le renouvellement de la fenêtre suit la dérive entre l'horloge
//! de la carte son et celle du système (50 ppm font 3 ms par minute). Le
//! décalage constant qui reste (tampon, appareil, casque, geste) est
//! précisément ce que la calibration mesure : la latence L du coach.

use std::sync::atomic::{fence, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Secondes écoulées depuis la première lecture de l'horloge, dans ce
/// processus. Monotone ; commune au fil audio, aux entrées MIDI et aux
/// commandes.
pub fn maintenant_s() -> f64 {
    static ORIGINE: OnceLock<Instant> = OnceLock::new();
    ORIGINE.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Tel instant du morceau a été rendu à tel instant de [`maintenant_s`].
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Ancre {
    /// Instant de l'horloge, en secondes.
    pub t_s: f64,
    /// Instant du morceau d'origine, en secondes. Pendant un décompte, il est
    /// avant le point de départ (négatif avant le début du morceau) : les
    /// clics se placent sur la même échelle que la musique.
    pub morceau_s: f64,
    /// Vitesse globale de lecture (1 = tempo d'origine).
    pub vitesse: f64,
}

/// L'ancre, partagée entre le fil audio qui l'écrit et qui la lit, sans
/// verrou : un compteur de séquence (impair pendant l'écriture) dit au
/// lecteur s'il a lu une ancre entière.
#[derive(Default)]
pub(crate) struct AncrePartagee {
    seq: AtomicU64,
    t: AtomicU64,
    morceau: AtomicU64,
    vitesse: AtomicU64,
}

impl AncrePartagee {
    fn publier(&self, a: Ancre) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.t.store(a.t_s.to_bits(), Ordering::Relaxed);
        self.morceau.store(a.morceau_s.to_bits(), Ordering::Relaxed);
        self.vitesse.store(a.vitesse.to_bits(), Ordering::Relaxed);
        self.seq.store(s + 2, Ordering::Release);
    }

    /// La dernière ancre publiée, `None` avant la première.
    pub(crate) fn lire(&self) -> Option<Ancre> {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 == 0 {
                return None;
            }
            if s1 % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let a = Ancre {
                t_s: f64::from_bits(self.t.load(Ordering::Relaxed)),
                morceau_s: f64::from_bits(self.morceau.load(Ordering::Relaxed)),
                vitesse: f64::from_bits(self.vitesse.load(Ordering::Relaxed)),
            };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return Some(a);
            }
        }
    }
}

/// Toutes les combien de trames le fil audio lit l'horloge : 64 trames font
/// 1,45 ms à 44,1 kHz, moins qu'un paquet de sortie.
const PAS: u64 = 64;

/// Côté fil audio : compte les trames rendues et publie l'ancre.
pub(crate) struct Chronometre {
    sr: f64,
    /// Trames rendues depuis la création.
    rendues: u64,
    fenetre_debut: u64,
    /// Maximum de `instant − rendues/SR` sur la fenêtre en cours, et sur la
    /// précédente.
    haut: f64,
    haut_avant: f64,
    pub(crate) ancre: std::sync::Arc<AncrePartagee>,
}

impl Chronometre {
    pub(crate) fn nouveau(sr: u32) -> Self {
        Self {
            sr: sr as f64,
            rendues: 0,
            fenetre_debut: 0,
            haut: f64::NEG_INFINITY,
            haut_avant: f64::NEG_INFINITY,
            ancre: Default::default(),
        }
    }

    /// Une trame est rendue : elle porte l'instant `morceau_s` du morceau, à
    /// la vitesse `vitesse`. `instant` est [`maintenant_s`] (injecté pour les
    /// tests) ; il n'est lu qu'une trame sur [`PAS`].
    pub(crate) fn trame(&mut self, morceau_s: f64, vitesse: f64, instant: impl FnOnce() -> f64) {
        if self.rendues.is_multiple_of(PAS) {
            if self.rendues - self.fenetre_debut >= self.sr as u64 {
                self.haut_avant = self.haut;
                self.haut = f64::NEG_INFINITY;
                self.fenetre_debut = self.rendues;
            }
            let n_s = self.rendues as f64 / self.sr;
            self.haut = self.haut.max(instant() - n_s);
            self.ancre.publier(Ancre {
                t_s: self.haut.max(self.haut_avant) + n_s,
                morceau_s,
                vitesse,
            });
        }
        self.rendues += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une sortie qui prend ses trames par paquets de 512, en avance d'un
    /// paquet : l'ancre retrouve la cadence de la sortie à moins d'un pas
    /// près, sans la gigue des paquets.
    #[test]
    fn l_ancre_suit_la_sortie_et_non_les_paquets() {
        let sr = 44_100u32;
        let mut c = Chronometre::nouveau(sr);
        let paquet = 512u64;
        let t0 = 10.0; // instant de sortie de la trame 0
        let mut pire: f64 = 0.0;
        for n in 0..(sr as u64 * 5) {
            // Le paquet k est calculé d'un coup, un paquet avant sa sortie,
            // avec une gigue de 0 à 3 ms selon le paquet.
            let k = n / paquet;
            let gigue = ((k * 7919) % 4) as f64 * 0.001;
            let calcul = t0 + ((k - k.min(1)) * paquet) as f64 / sr as f64 + gigue;
            c.trame(n as f64 / sr as f64, 1.0, || calcul);
            if n > 2 * sr as u64 + 1000 && n % PAS == 0 {
                let a = c.ancre.lire().unwrap();
                // La trame n sort à t0 + n/SR ; l'ancre la donne à un
                // décalage constant près (un paquet moins la gigue maximale).
                let attendu = t0 + n as f64 / sr as f64;
                pire = pire.max((a.t_s - attendu + (paquet as f64 / sr as f64 - 0.003)).abs());
                assert_eq!(a.morceau_s, n as f64 / sr as f64);
            }
        }
        assert!(pire < 0.0015, "écart à la cadence de sortie : {pire} s");
    }

    #[test]
    fn rien_avant_la_premiere_trame() {
        let c = Chronometre::nouveau(44_100);
        assert_eq!(c.ancre.lire(), None);
        assert!(maintenant_s() >= 0.0);
    }
}
