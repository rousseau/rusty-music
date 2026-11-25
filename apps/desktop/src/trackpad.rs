// SPDX-License-Identifier: GPL-3.0-or-later
//! Les phases du trackpad pour le mode scratch du vinyle.
//!
//! WebKit ne donne à la webview que les deltas de la molette : ni quand les
//! doigts se posent ou se lèvent, ni quelle part du défilement est l'inertie
//! de macOS. `scratch.js` le devinait (main levée après 60 ms de silence,
//! inertie reconnue à une suite géométrique). AppKit le dit sur chaque
//! `scrollWheel` : `phase` (doigts posés sans bouger, début, fin, annulation)
//! et `momentumPhase` (l'inertie).
//!
//! Un moniteur local d'AppKit voit ces événements avant la fenêtre, sans les
//! consommer : la webview défile comme avant. Tant que le scratch est armé
//! ([`armer`]), il émet vers l'interface l'événement `scratch-phase` à chaque
//! changement de phase — quelques-uns par geste, pas un par delta.

use std::sync::atomic::{AtomicBool, Ordering};

/// Le nom de l'événement Tauri.
pub(crate) const EVENEMENT: &str = "scratch-phase";

/// Vrai quand le scratch est armé côté moteur : sinon, le moniteur se tait.
static ARME: AtomicBool = AtomicBool::new(false);

pub(crate) fn armer(oui: bool) {
    ARME.store(oui, Ordering::Relaxed);
}

/// Les doigts sur le trackpad (`NSEvent.phase`).
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhaseDoigts {
    /// Posés sans bouger (`MayBegin`).
    Poses,
    /// Le geste commence (`Began`).
    Debut,
    /// Les doigts bougent (`Changed`) : l'événement porte leur déplacement.
    Change,
    /// Levés (`Ended`).
    Fin,
    /// Le système a repris le geste (`Cancelled`) : comme levés.
    Annule,
}

/// L'inertie de macOS après le geste (`NSEvent.momentumPhase`).
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhaseInertie {
    Debut,
    Change,
    Fin,
}

/// La charge de `scratch-phase`.
#[derive(serde::Serialize, Clone, Debug)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct PhaseTrackpad {
    pub(crate) doigts: Option<PhaseDoigts>,
    pub(crate) inertie: Option<PhaseInertie>,
    /// Déplacement de l'événement, en points (`scrollingDeltaX/Y`). Il passe
    /// par ici et non par l'événement `wheel` de la page : les deux chemins
    /// ne sont pas ordonnés entre eux, et un delta d'inertie arrivé avant
    /// l'annonce de l'inertie reprenait le disque (« il repart deux fois »).
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    /// Défilement naturel de macOS (`isDirectionInvertedFromDevice`) : les
    /// deltas suivent alors les doigts ; sinon, ils vont à l'opposé.
    pub(crate) naturel: bool,
    /// L'instant de l'événement, en millisecondes depuis l'époque Unix :
    /// l'interface le compare à `performance.timeOrigin + performance.now()`
    /// pour compter le trajet jusqu'à elle.
    pub(crate) t_ms: f64,
}

/// Traduit les phases d'un `scrollWheel`. `None` pour ce qui n'intéresse pas
/// l'interface : un delta au milieu du geste (`Changed`), ou une molette de
/// souris (sans phase).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn traduire(phase: usize, inertie: usize) -> Option<(Option<PhaseDoigts>, Option<PhaseInertie>)> {
    let doigts = match phase {
        0x20 => Some(PhaseDoigts::Poses),
        0x1 => Some(PhaseDoigts::Debut),
        0x4 => Some(PhaseDoigts::Change),
        0x8 => Some(PhaseDoigts::Fin),
        0x10 => Some(PhaseDoigts::Annule),
        _ => None,
    };
    let inertie = match inertie {
        0x1 => Some(PhaseInertie::Debut),
        0x4 => Some(PhaseInertie::Change),
        0x8 | 0x10 => Some(PhaseInertie::Fin),
        _ => None,
    };
    (doigts.is_some() || inertie.is_some()).then_some((doigts, inertie))
}

/// Installe le moniteur, une fois, au démarrage (fil principal). Sans effet
/// hors macOS : l'interface garde son heuristique.
pub(crate) fn installer(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    macos::installer(app.clone());
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ptr::NonNull;
    use std::time::{SystemTime, UNIX_EPOCH};

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSEventMask};
    use objc2_foundation::NSProcessInfo;
    use tauri::Emitter;

    use super::{traduire, PhaseTrackpad, ARME, EVENEMENT};

    pub(super) fn installer(app: tauri::AppHandle) {
        if MainThreadMarker::new().is_none() {
            tracing::warn!("phases du trackpad : pas sur le fil principal, moniteur non installé");
            return;
        }
        let bloc = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
            if ARME.load(std::sync::atomic::Ordering::Relaxed) {
                // SAFETY : AppKit passe un événement valide pour la durée du rappel.
                let e = unsafe { ev.as_ref() };
                if let Some((doigts, inertie)) = traduire(e.phase().0, e.momentumPhase().0) {
                    let t_ms = epoque_ms(e.timestamp());
                    let _ = app.emit(
                        EVENEMENT,
                        PhaseTrackpad {
                            doigts,
                            inertie,
                            dx: e.scrollingDeltaX(),
                            dy: e.scrollingDeltaY(),
                            naturel: e.isDirectionInvertedFromDevice(),
                            t_ms,
                        },
                    );
                }
            }
            // Rendu tel quel : l'événement poursuit vers la webview.
            ev.as_ptr()
        });
        // SAFETY : le bloc rend l'événement reçu, un pointeur valide.
        let moniteur = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::ScrollWheel, &bloc)
        };
        match moniteur {
            // Il vit autant que l'application.
            Some(m) => std::mem::forget(m),
            None => tracing::warn!("phases du trackpad : moniteur refusé par AppKit"),
        }
    }

    /// `NSEvent.timestamp` compte depuis le démarrage de la machine : ramené à
    /// l'époque Unix par l'écart avec l'instant présent.
    fn epoque_ms(horodatage: f64) -> f64 {
        let maintenant = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let age = NSProcessInfo::processInfo().systemUptime() - horodatage;
        (maintenant - age.max(0.0)) * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_traduites() {
        assert_eq!(traduire(0x20, 0), Some((Some(PhaseDoigts::Poses), None)));
        assert_eq!(traduire(0x8, 0), Some((Some(PhaseDoigts::Fin), None)));
        assert_eq!(traduire(0, 0x1), Some((None, Some(PhaseInertie::Debut))));
        // Un delta au milieu du geste, une molette de souris : rien.
        assert_eq!(traduire(0x4, 0), Some((Some(PhaseDoigts::Change), None)));
        assert_eq!(traduire(0, 0x4), Some((None, Some(PhaseInertie::Change))));
        assert_eq!(traduire(0x2, 0), None); // Stationary
        assert_eq!(traduire(0, 0), None);
    }
}
