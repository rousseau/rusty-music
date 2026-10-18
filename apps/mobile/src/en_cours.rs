// SPDX-License-Identifier: GPL-3.0-or-later
//! « En cours de lecture » d'iOS (écran verrouillé, centre de contrôle) et
//! commandes à distance (boutons de l'écran verrouillé, AirPods, CarPlay).
//!
//! Le pendant de `apps/desktop/src/systeme_media.rs`, qui passe par
//! `souvlaki` — déclaré pour macOS seulement. Ici, directement par
//! `objc2-media-player` (`MPNowPlayingInfoCenter`, `MPRemoteCommandCenter`).
//! Pas encore de pochette : `MPMediaItemArtwork` demande une `UIImage`, donc
//! les liaisons UIKit en plus.

use std::time::{Duration, Instant};

use crate::EtatLecture;

/// Décide quand republier : à chaque changement de piste ou de pause, et
/// toutes les [`Suivi::RAFRAICHIR`] sinon. Entre deux, iOS extrapole la
/// position à partir de la vitesse de lecture publiée.
#[derive(Default)]
pub(crate) struct Suivi {
    dernier: Option<(Option<String>, Option<String>, bool)>,
    publie_le: Option<Instant>,
}

impl Suivi {
    const RAFRAICHIR: Duration = Duration::from_secs(10);

    pub(crate) fn battement(&mut self, lecture: Option<&EtatLecture>) {
        let cle = lecture.map(|l| (l.titre.clone(), l.album_id.clone(), l.pause));
        let perime = self.publie_le.is_none_or(|t| t.elapsed() >= Self::RAFRAICHIR);
        if cle == self.dernier && !(lecture.is_some() && perime) {
            return;
        }
        afficher(lecture);
        self.dernier = cle;
        self.publie_le = Some(Instant::now());
    }
}

/// Publie ce qui joue, ou efface l'affichage si rien ne joue.
pub(crate) fn afficher(lecture: Option<&EtatLecture>) {
    #[cfg(target_os = "ios")]
    ios::afficher(lecture);
    #[cfg(not(target_os = "ios"))]
    let _ = lecture;
}

/// Branche les commandes à distance sur le transport. À appeler une fois,
/// depuis le fil principal (`setup`).
pub(crate) fn brancher_commandes(app: tauri::AppHandle) {
    #[cfg(target_os = "ios")]
    ios::brancher(app);
    #[cfg(not(target_os = "ios"))]
    let _ = app;
}

#[cfg(target_os = "ios")]
mod ios {
    use std::ptr::NonNull;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::Message;
    use objc2_foundation::{NSDictionary, NSNumber, NSString};
    use objc2_media_player::{
        MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyAlbumTitle,
        MPMediaItemPropertyArtist, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
        MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
        MPNowPlayingInfoPropertyPlaybackRate, MPRemoteCommand, MPRemoteCommandCenter,
        MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
    };
    use tauri::Manager;

    use crate::{transport, Etat, EtatLecture, Geste};

    /// Tout objet Objective-C est un `AnyObject` : les valeurs du
    /// dictionnaire d'affichage sont de types mêlés (chaînes, nombres).
    fn objet<T: Message>(r: Retained<T>) -> Retained<AnyObject> {
        // SAFETY : conversion vers la racine de la hiérarchie d'objets.
        unsafe { Retained::cast_unchecked(r) }
    }

    pub(super) fn afficher(lecture: Option<&EtatLecture>) {
        // SAFETY : `MPNowPlayingInfoCenter` est utilisable depuis n'importe
        // quel fil ; les clés sont des constantes du framework.
        unsafe {
            let centre = MPNowPlayingInfoCenter::defaultCenter();
            let Some(l) = lecture else {
                centre.setNowPlayingInfo(None);
                return;
            };
            let mut cles: Vec<&NSString> = Vec::new();
            let mut valeurs: Vec<Retained<AnyObject>> = Vec::new();
            for (cle, texte) in [
                (MPMediaItemPropertyTitle, &l.titre),
                (MPMediaItemPropertyArtist, &l.artiste),
                (MPMediaItemPropertyAlbumTitle, &l.album),
            ] {
                if let Some(t) = texte {
                    cles.push(cle);
                    valeurs.push(objet(NSString::from_str(t)));
                }
            }
            if let Some(d) = l.duree_ms {
                cles.push(MPMediaItemPropertyPlaybackDuration);
                valeurs.push(objet(NSNumber::new_f64(d as f64 / 1000.0)));
            }
            cles.push(MPNowPlayingInfoPropertyElapsedPlaybackTime);
            valeurs.push(objet(NSNumber::new_f64(l.position_ms as f64 / 1000.0)));
            cles.push(MPNowPlayingInfoPropertyPlaybackRate);
            valeurs.push(objet(NSNumber::new_f64(if l.pause { 0.0 } else { 1.0 })));
            let infos = NSDictionary::from_retained_objects(&cles, &valeurs);
            centre.setNowPlayingInfo(Some(&infos));
        }
    }

    /// Relie une commande à un geste du transport.
    fn relier(
        commande: &MPRemoteCommand,
        app: &tauri::AppHandle,
        geste: impl Fn(NonNull<MPRemoteCommandEvent>) -> Geste + 'static,
    ) {
        let app = app.clone();
        let bloc = RcBlock::new(move |evenement: NonNull<MPRemoteCommandEvent>| {
            match transport(&app.state::<Etat>(), geste(evenement)) {
                Ok(()) => MPRemoteCommandHandlerStatus::Success,
                Err(e) => {
                    tracing::warn!(erreur = %e, "commande à distance refusée");
                    MPRemoteCommandHandlerStatus::CommandFailed
                }
            }
        });
        // SAFETY : appelée sur le fil principal ; le centre de commandes
        // retient le bloc aussi longtemps que la cible existe.
        unsafe {
            commande.setEnabled(true);
            let _cible = commande.addTargetWithHandler(&bloc);
        }
    }

    pub(super) fn brancher(app: tauri::AppHandle) {
        // SAFETY : fil principal, au démarrage.
        unsafe {
            let centre = MPRemoteCommandCenter::sharedCommandCenter();
            relier(&centre.playCommand(), &app, |_| Geste::Pause(Some(false)));
            relier(&centre.pauseCommand(), &app, |_| Geste::Pause(Some(true)));
            relier(&centre.togglePlayPauseCommand(), &app, |_| Geste::Pause(None));
            relier(&centre.nextTrackCommand(), &app, |_| Geste::Suivante);
            relier(&centre.previousTrackCommand(), &app, |_| Geste::Precedente);
            relier(&centre.changePlaybackPositionCommand(), &app, |evenement| {
                // SAFETY : cette commande n'émet que des événements de
                // changement de position.
                let position = evenement
                    .cast::<MPChangePlaybackPositionCommandEvent>()
                    .as_ref()
                    .positionTime();
                Geste::AllerA(Duration::from_secs_f64(position.max(0.0)))
            });
        }
    }
}
