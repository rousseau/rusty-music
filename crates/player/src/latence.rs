// SPDX-License-Identifier: GPL-3.0-or-later
//! Latence de la sortie audio : le temps entre l'échantillon que le moteur
//! rend et celui qu'on entend.
//!
//! La position du moteur compte ce que la sortie lui a **pris**, pas ce
//! qu'elle a joué. Sur les haut-parleurs du MacBook Pro l'écart est de
//! 28,5 ms, dont 11,6 de tampon (512 trames, `TAMPON_SORTIE` ; 63 ms avec
//! le défaut de rodio, 25 nov. 2026) ; en Bluetooth, il dépasse 200 ms (Mustang Micro Plus : 230 ms
//! d'appareil plus le tampon, 29 oct. 2026) — un curseur de partition calé
//! sur la position brute court alors devant la musique. cpal ne compte que
//! son tampon : on lit le reste à CoreAudio.

use std::time::Duration;

/// Latence de l'appareil de sortie par défaut : celle qu'il déclare, plus sa
/// marge de sécurité, son tampon et celle de son flux. Zéro hors macOS, ou si
/// CoreAudio ne répond pas.
pub fn latence_sortie() -> Duration {
    detail_latence_sortie()
        .map(|d| d.totale())
        .unwrap_or(Duration::ZERO)
}

/// Les termes de [`latence_sortie`], en trames de l'appareil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailLatence {
    /// Fréquence nominale de l'appareil, en Hz.
    pub taux: f64,
    /// Latence déclarée par l'appareil (convertisseur, traitement interne).
    pub appareil: u32,
    /// Marge de sécurité du pilote.
    pub marge: u32,
    /// Tampon d'entrée-sortie de l'appareil : commun à tous ses clients,
    /// imposé par le dernier flux ouvert.
    pub tampon: u32,
    /// Latence du premier flux de sortie.
    pub flux: u32,
}

impl DetailLatence {
    /// Durée de `trames` à la fréquence de l'appareil.
    pub fn duree(&self, trames: u32) -> Duration {
        Duration::from_secs_f64(trames as f64 / self.taux)
    }

    /// La somme des quatre termes.
    pub fn totale(&self) -> Duration {
        self.duree(self.appareil + self.marge + self.tampon + self.flux)
    }
}

/// Le détail de [`latence_sortie`] ; `None` hors macOS, ou si CoreAudio ne
/// répond pas.
pub fn detail_latence_sortie() -> Option<DetailLatence> {
    #[cfg(target_os = "macos")]
    {
        macos::detail()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::mem::size_of;
    use std::ptr::{null, NonNull};

    use objc2_core_audio::{
        kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyLatency,
        kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertySafetyOffset,
        kAudioDevicePropertyStreams, kAudioHardwareNoError,
        kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
        kAudioStreamPropertyLatency, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
        AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertyScope,
        AudioObjectPropertySelector,
    };

    fn adresse(
        selecteur: AudioObjectPropertySelector,
        portee: AudioObjectPropertyScope,
    ) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selecteur,
            mScope: portee,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    /// Une propriété de taille fixe (entier, flottant).
    fn lire<T: Copy + Default>(
        objet: AudioObjectID,
        selecteur: AudioObjectPropertySelector,
        portee: AudioObjectPropertyScope,
    ) -> Option<T> {
        let a = adresse(selecteur, portee);
        let mut valeur = T::default();
        let mut taille = size_of::<T>() as u32;
        // SAFETY : `valeur` occupe `taille` octets, et CoreAudio n'écrit pas
        // au-delà de la taille annoncée.
        let statut = unsafe {
            AudioObjectGetPropertyData(
                objet,
                NonNull::from(&a),
                0,
                null(),
                NonNull::from(&mut taille),
                NonNull::from(&mut valeur).cast(),
            )
        };
        (statut == kAudioHardwareNoError).then_some(valeur)
    }

    /// Le premier flux de sortie de l'appareil, qui a sa propre latence.
    fn premier_flux(appareil: AudioObjectID) -> Option<AudioObjectID> {
        let a = adresse(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput);
        let mut taille = 0u32;
        // SAFETY : on ne lit que la taille.
        let statut = unsafe {
            AudioObjectGetPropertyDataSize(
                appareil,
                NonNull::from(&a),
                0,
                null(),
                NonNull::from(&mut taille),
            )
        };
        if statut != kAudioHardwareNoError {
            return None;
        }
        let mut flux = vec![0 as AudioObjectID; taille as usize / size_of::<AudioObjectID>()];
        if flux.is_empty() {
            return None;
        }
        // SAFETY : `flux` a la taille que CoreAudio vient d'annoncer.
        let statut = unsafe {
            AudioObjectGetPropertyData(
                appareil,
                NonNull::from(&a),
                0,
                null(),
                NonNull::from(&mut taille),
                NonNull::new(flux.as_mut_ptr())?.cast(),
            )
        };
        (statut == kAudioHardwareNoError).then(|| flux[0])
    }

    pub(super) fn detail() -> Option<super::DetailLatence> {
        let global = kAudioObjectPropertyScopeGlobal;
        let sortie = kAudioObjectPropertyScopeOutput;
        let appareil: AudioObjectID = lire(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyDefaultOutputDevice,
            global,
        )?;
        let taux: f64 = lire(appareil, kAudioDevicePropertyNominalSampleRate, global)?;
        if taux <= 0.0 {
            return None;
        }
        Some(super::DetailLatence {
            taux,
            appareil: lire(appareil, kAudioDevicePropertyLatency, sortie).unwrap_or(0),
            marge: lire(appareil, kAudioDevicePropertySafetyOffset, sortie).unwrap_or(0),
            tampon: lire(appareil, kAudioDevicePropertyBufferFrameSize, global).unwrap_or(0),
            flux: premier_flux(appareil)
                .and_then(|f| lire(f, kAudioStreamPropertyLatency, global))
                .unwrap_or(0),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn la_latence_est_plausible() {
        // Sans sortie (machine de build), zéro ; avec, moins d'une seconde
        // même en Bluetooth.
        let l = super::latence_sortie();
        eprintln!("latence de sortie : {l:?}");
        assert!(l < std::time::Duration::from_secs(1));
    }
}
