//! Empreintes audio (module 2 — Exploration).
//!
//! Un modèle pré-entraîné transforme un extrait en vecteur : c'est la distance
//! entre ces vecteurs qui place les morceaux sur la carte. Aucun
//! ré-entraînement — voir `docs/architecture.md`.
//!
//! Modèle de référence : encodeur audio de CLAP (`laion/clap-htsat-unfused`),
//! converti en ONNX sans modification des poids, Apache-2.0. Le graphe est
//! traduit en Rust natif par `burn-onnx` au moment du build et exécuté par
//! **Burn** — voir `encodeur.rs`, et `scripts/preparer-modele.sh` pour la
//! préparation du modèle qui rend cet import possible.

pub mod alea;
pub mod battements;
pub mod chemin;
pub mod cluster;
pub mod decode;
pub mod descripteurs;
pub mod encodeur;
pub mod mel;
pub mod passe;
pub mod projection;

pub use encodeur::{Embedder, LOT};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("poids du modèle introuvables : {0}")]
    PoidsAbsents(String),

    #[error("sortie du modèle inattendue : {0}")]
    Sortie(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Forme d'entrée attendue par l'encodeur audio de CLAP.
///
/// Une fenêtre de 10 s à 48 kHz : `n_fft` 1024, `hop` 480 → 1001 trames de
/// 64 bandes mel. Le modèle est figé sur cette taille.
pub const TRAMES: usize = 1001;
pub const MELS: usize = 64;
/// Durée d'audio couverte par une fenêtre.
pub const FENETRE_S: f32 = 10.0;
/// Dimensions de l'empreinte produite.
pub const DIMS: usize = 512;
