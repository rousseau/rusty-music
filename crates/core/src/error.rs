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
}
