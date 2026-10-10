// SPDX-License-Identifier: GPL-3.0-or-later
//! Transcription automatique d'un stem, pour l'usage « pratiquer » du mode
//! Éditer (`docs/plan-editer-pratique-creation.md`, chantier 1.2).
//!
//! Le crate ne décode rien : il prend du mono et une fréquence, et rend des
//! notes. Le chemin de la basse :
//!
//! 1. [`basic_pitch`] — le réseau de Spotify (Apache-2.0) par ONNX Runtime,
//!    puis le port de sa création de notes ;
//! 2. [`porte`] — pas de note là où le stem se tait (fuites d'autres
//!    instruments) ;
//! 3. [`monophonie`] — une basse joue une note à la fois ;
//! 4. [`tablature`] — corde et frette de chaque note, par plus court chemin ;
//! 5. [`quantification`] — sur les temps et les mesures de la pulsation.
//!
//! La batterie : [`batterie`] — ADTOF (Zehren et al.), poids convertis en ONNX,
//! spectrogramme et choix des pics réécrits d'après madmom.

pub mod basic_pitch;
pub mod batterie;
pub mod monophonie;
pub mod porte;
pub mod quantification;
pub mod tablature;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("poids du modèle introuvables : {0}")]
    PoidsAbsents(String),

    #[error("téléchargement des poids : {0}")]
    Telechargement(String),

    #[error("ONNX Runtime : {0}")]
    Onnx(#[from] ort::Error),

    #[error("rééchantillonnage : {0}")]
    Reechantillonnage(String),

    #[error("sortie du modèle inattendue : {0}")]
    Sortie(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Un fichier de poids : où le trouver, combien il pèse, son empreinte.
#[derive(Debug, Clone, Copy)]
pub struct Poids {
    pub nom: &'static str,
    pub url: &'static str,
    pub octets: u64,
    pub sha256: &'static str,
}

impl Poids {
    pub fn present(&self) -> bool {
        rusty_music_core::modeles::trouver(self.nom).is_some()
    }

    /// Télécharge le fichier s'il manque, empreinte vérifiée.
    pub fn telecharger(&self, avancer: impl FnMut(u64, Option<u64>)) -> Result<()> {
        if self.present() {
            return Ok(());
        }
        rusty_music_core::modeles::telecharger(self.nom, self.url, self.octets, Some(self.sha256), avancer)
            .map(|_| ())
            .map_err(|e| Error::Telechargement(e.to_string()))
    }
}

/// Une note transcrite, en secondes du fichier d'origine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub debut_s: f32,
    pub fin_s: f32,
    /// Hauteur MIDI (69 = la 440).
    pub hauteur: u8,
    /// Activation moyenne du réseau sur la durée de la note, de 0 à 1 — ce
    /// qui tient lieu de confiance et de vélocité.
    pub amplitude: f32,
    /// Corde (0 = la plus grave) et frette, une fois la tablature posée.
    pub corde: Option<u8>,
    pub frette: Option<u8>,
}

impl Note {
    pub fn duree(&self) -> f32 {
        self.fin_s - self.debut_s
    }
}
