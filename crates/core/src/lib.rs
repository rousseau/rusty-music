//! Cœur d'ingestion.
//!
//! Point d'entrée unique du logiciel : un répertoire de musique, scanné puis
//! surveillé en continu. Produit une base SQLite consommée par les trois
//! modules (lecteur, exploration, éditeur). Aucun module ne relit le disque
//! directement.

pub mod db;
pub mod enrichir;
pub mod error;
pub mod modeles;
pub mod musicbrainz;
pub mod opus;
pub mod scan;
pub mod tags;
pub mod watch;

pub use db::Library;
pub use error::{Error, Result};
pub use tags::{Cover, CoverSource, TrackMeta};

/// Extensions traitées comme des fichiers musicaux.
///
/// `mp4` est inclus : le conteneur sert aussi bien à l'audio seul (des albums
/// entiers de la bibliothèque de test sont étiquetés ainsi plutôt qu'en `m4a`).
/// Contrepartie : une vidéo rangée dans le dossier surveillé serait ingérée.
pub const AUDIO_EXTS: &[&str] = &[
    "flac", "mp3", "m4a", "mp4", "aac", "ogg", "opus", "wav", "aiff", "aif", "wv", "ape",
];

/// Vrai si le chemin porte une extension musicale connue (comparaison insensible à la casse).
pub fn is_audio(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}
