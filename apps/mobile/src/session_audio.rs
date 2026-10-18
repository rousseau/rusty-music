// SPDX-License-Identifier: GPL-3.0-or-later
//! Session audio d'iOS.
//!
//! Sans catégorie `Playback`, iOS traite l'app comme un son d'ambiance : le
//! bouton de sourdine la coupe, et la lecture s'arrête dès que l'écran se
//! verrouille — même avec `UIBackgroundModes = audio` dans `Info.ios.plist`.
//! `cpal` ouvre la sortie mais ne touche pas à la session : c'est à nous de
//! la régler, **avant** d'ouvrir la sortie.
//!
//! En Rust par `objc2-avf-audio` (déjà tiré par `cpal`) plutôt qu'en plugin
//! Swift : voir `docs/plan-ios-compagnon.md`, étape 0.

/// Règle la session en lecture et l'active. Sans effet hors iOS.
pub fn configurer() {
    #[cfg(target_os = "ios")]
    {
        use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryPlayback};

        // SAFETY : appelée depuis le fil principal au démarrage, avant que la
        // sortie audio n'existe ; `AVAudioSessionCategoryPlayback` est une
        // constante du framework, toujours présente sur iOS.
        unsafe {
            let session = AVAudioSession::sharedInstance();
            match AVAudioSessionCategoryPlayback {
                Some(categorie) => {
                    if let Err(e) = session.setCategory_error(categorie) {
                        tracing::error!(erreur = %e.localizedDescription(), "catégorie de session audio refusée");
                    }
                }
                None => tracing::error!("AVAudioSessionCategoryPlayback absente"),
            }
            if let Err(e) = session.setActive_error(true) {
                tracing::error!(erreur = %e.localizedDescription(), "activation de la session audio refusée");
            }
        }
    }
}
