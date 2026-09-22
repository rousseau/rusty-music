// SPDX-License-Identifier: GPL-3.0-or-later
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("erreur de base de données : {0}")]
    Db(#[from] rusqlite::Error),

    #[error("erreur d'entrée/sortie : {0}")]
    Io(#[from] std::io::Error),

    #[error("lecture des tags impossible pour {path} : {source}")]
    Tags {
        path: PathBuf,
        #[source]
        source: lofty::error::LoftyError,
    },

    #[error("surveillance du dossier impossible : {0}")]
    Watch(#[from] notify::Error),

    #[error("le chemin n'existe pas ou n'est pas un dossier : {0}")]
    NotADirectory(PathBuf),

    /// Interrogation d'une source distante (MusicBrainz, ListenBrainz).
    ///
    /// Toujours récupérable : l'enrichissement est additif, une bibliothèque
    /// sans réseau reste entièrement utilisable. Cette erreur interrompt une
    /// passe, jamais l'application.
    #[error("source distante injoignable : {0}")]
    Reseau(String),

    #[error("fichier Opus illisible : {0}")]
    Opus(String),

    /// Décodage pleine piste (`crate::decode`), tout format hors Opus — voir
    /// [`Error::Opus`] pour ce cas-là.
    #[error("format audio non décodable pour {path} : {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: rodio::decoder::DecoderError,
    },

    /// Donnée locale illisible — un fragment XML du dump Discogs, par
    /// exemple. Distinct de [`Error::Reseau`] : ce n'est pas une source
    /// distante injoignable, c'est un contenu déjà en main mais malformé.
    /// Toujours récupérable à l'échelle d'une seule entrée : voir
    /// `crate::discogs`.
    #[error("donnée illisible : {0}")]
    Parsing(String),

    /// Un fichier a fait paniquer son traitement (décodage hasardeux d'un
    /// format mal formé, typiquement) — voir `crate::panique::sans_panique`.
    /// Converti en échec de ce seul fichier plutôt que de laisser la panique
    /// dérouler et avorter toute une passe qui en traite des milliers
    /// d'autres.
    #[error("panique interne pendant le traitement de {path} : {message}")]
    Panique { path: PathBuf, message: String },
}
