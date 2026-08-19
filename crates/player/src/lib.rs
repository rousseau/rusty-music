//! Lecture audio (module 1) : sortie, transport, file d'attente.
//!
//! Crate séparé du cœur d'ingestion à dessein : la pile audio du système
//! (CoreAudio, ALSA, WASAPI) n'a rien à faire dans un binaire qui se contente
//! de lire la base. Le lecteur ne connaît que des chemins — c'est la base qui
//! les lui fournit, aucun module ne relit le disque de son côté.
//!
//! Le décodage passe par `rodio`, qui s'appuie sur `symphonia` — le décodeur
//! retenu dans `docs/architecture.md`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink};
use tracing::debug;

pub mod multipiste;
pub mod spectre;
pub mod waveform;
pub use multipiste::Multipiste;
pub use spectre::Spectre;
pub use waveform::Waveform;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("aucune sortie audio disponible : {0}")]
    Output(#[from] rodio::DeviceSinkError),

    #[error("ouverture impossible de {path} : {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("format non décodable pour {path} : {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: rodio::decoder::DecoderError,
    },

    #[error(transparent)]
    Opus(#[from] rusty_music_core::Error),

    #[error("étirement impossible : {0}")]
    Etirement(String),

    #[error("déplacement impossible dans la piste : {0}")]
    Seek(#[from] rodio::source::SeekError),

    #[error("durée inconnue pour {path} : enveloppe incalculable")]
    DureeInconnue { path: PathBuf },

    #[error("aucune piste à lire")]
    Vide,
}

/// Un fichier Opus chargé en mémoire, ou `None` si ce n'en est pas un.
///
/// rodio décode par symphonia, qui ne connaît pas Opus. Le cœur sait le faire ;
/// il rend des échantillons entrelacés que `SamplesBuffer` transforme en source
/// rodio ordinaire, jouable et mesurable comme les autres.
///
/// **Le morceau entier tient en mémoire** — une soixantaine de mégaoctets pour
/// quatre minutes en stéréo. C'est le prix d'un format que la chaîne de
/// décodage en flux ne sait pas ouvrir, et il ne concerne que ces fichiers.
pub(crate) fn opus_en_memoire(path: &Path) -> Result<Option<rodio::buffer::SamplesBuffer>> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("opus"))
    {
        return Ok(None);
    }
    let piste = rusty_music_core::opus::decoder(path)?;
    let canaux = u16::try_from(piste.canaux).unwrap_or(2).max(1);
    Ok(Some(rodio::buffer::SamplesBuffer::new(
        canaux.try_into().expect("au moins un canal"),
        rusty_music_core::opus::SR
            .try_into()
            .expect("48 kHz n'est pas nul"),
        piste.echantillons,
    )))
}

pub type Result<T> = std::result::Result<T, Error>;

/// Au-delà de ce temps écoulé, « précédent » reprend la piste en cours au lieu
/// de reculer d'un rang — convention partagée par tous les lecteurs.
pub const RETOUR_DEBUT: Duration = Duration::from_secs(3);

/// Nombre de pistes tenues prêtes dans la sortie.
///
/// Ouvrir un fichier et lire son en-tête coûte ~100 ms sur un support lent :
/// charger une file entière d'avance immobilisait le lecteur 17 s sur un album
/// de 157 pistes, et tout clic sur pause attendait d'autant. On n'en prépare
/// donc que quelques-unes, complétées au fil de la lecture.
const PRECHARGE: usize = 3;

/// Sortie audio et transport.
///
/// Une seule instance par processus : elle tient la sortie du système ouverte.
pub struct Player {
    // L'ordre de déclaration fixe l'ordre de destruction. Le lecteur doit
    // tomber avant la sortie à laquelle il est raccordé, pas l'inverse.
    inner: rodio::Player,
    _output: MixerDeviceSink,
    /// Chemins empilés, dans l'ordre. Sert à savoir ce qui joue : `rodio` ne
    /// retient que le nombre de sources restantes, pas leur provenance.
    queue: Vec<PathBuf>,
    /// Rang de la prochaine piste à confier à la sortie.
    prochain: usize,
    /// Rangs des pistes effectivement confiées à la sortie, dans l'ordre.
    ///
    /// On ne peut pas déduire la piste courante d'un simple calcul sur
    /// `prochain` : une piste illisible est sautée sans jamais entrer dans la
    /// sortie, et décalerait tout le repérage. Cette liste ne retient que ce
    /// qui y est réellement entré.
    charges: Vec<usize>,
}

impl Player {
    /// Ouvre la sortie audio par défaut du système.
    pub fn new() -> Result<Self> {
        let mut output = DeviceSinkBuilder::open_default_sink()?;
        // On ferme la sortie sciemment en fin de processus : le message que
        // `rodio` émet alors sur stderr n'apprend rien et pollue la CLI.
        output.log_on_drop(false);
        let inner = rodio::Player::connect_new(output.mixer());
        Ok(Self {
            inner,
            _output: output,
            queue: Vec::new(),
            prochain: 0,
            charges: Vec::new(),
        })
    }

    /// Remplace la file par `tracks` et lance la lecture.
    pub fn play(&mut self, tracks: &[PathBuf]) -> Result<()> {
        self.queue = tracks.to_vec();
        self.charger(0)
    }

    /// Ajoute une piste en fin de file, sans interrompre la lecture en cours.
    pub fn enqueue(&mut self, track: &Path) -> Result<()> {
        self.queue.push(track.to_path_buf());
        self.completer()
    }

    /// Complète la réserve de pistes prêtes. À appeler régulièrement — c'est
    /// ce qui remplace le chargement intégral de la file.
    ///
    /// N'ouvre qu'un fichier par appel : le verrou du lecteur n'est jamais
    /// retenu plus longtemps que la lecture d'un en-tête.
    pub fn completer(&mut self) -> Result<()> {
        if self.inner.len() >= PRECHARGE || self.prochain >= self.queue.len() {
            return Ok(());
        }
        let rang = self.prochain;
        let piste = self.queue[rang].clone();
        // On avance avant de tenter : une piste que `symphonia` ne sait pas
        // décoder — les fichiers Opus de la bibliothèque — bloquerait sinon la
        // file sur elle, réessayée à chaque passage.
        self.prochain += 1;
        self.appendre(&piste)?;
        self.charges.push(rang);
        Ok(())
    }

    /// Revient à la piste précédente.
    ///
    /// Comme sur n'importe quel lecteur, une piste déjà entamée depuis plus de
    /// [`RETOUR_DEBUT`] est reprise à zéro plutôt que de reculer d'un rang.
    pub fn previous(&mut self) -> Result<()> {
        let Some(i) = self.index() else { return Ok(()) };
        let cible = if self.position() > RETOUR_DEBUT || i == 0 {
            i
        } else {
            i - 1
        };
        self.charger(cible)
    }

    /// Saute directement à une piste de la file.
    ///
    /// Les pistes qui précèdent restent dans la file : on peut revenir en
    /// arrière ensuite.
    pub fn jump_to(&mut self, index: usize) -> Result<()> {
        if index >= self.queue.len() {
            return Ok(());
        }
        self.charger(index)
    }

    /// File complète, dans l'ordre.
    pub fn queue(&self) -> &[PathBuf] {
        &self.queue
    }

    /// (Re)remplit la sortie à partir de `depart` dans la file.
    ///
    /// `rodio` ne sait qu'avancer : reculer impose de vider la sortie et de la
    /// regarnir. `self.queue` n'est pas touchée — sans quoi on perdrait
    /// l'historique et un second retour en arrière serait impossible.
    fn charger(&mut self, depart: usize) -> Result<()> {
        self.inner.clear();
        self.prochain = depart;
        self.charges.clear();
        // Seulement de quoi démarrer : la suite viendra par `completer()`.
        for _ in 0..PRECHARGE {
            self.completer()?;
        }
        // `clear()` laisse le lecteur en pause : sans ça, rien ne sortirait.
        self.inner.play();
        Ok(())
    }

    /// Ajoute une source à la sortie sans toucher à la file.
    fn appendre(&self, track: &Path) -> Result<()> {
        if let Some(buf) = crate::opus_en_memoire(track)? {
            self.inner.append(buf);
            debug!(path = %track.display(), "piste Opus empilée");
            return Ok(());
        }
        let file = std::fs::File::open(track).map_err(|source| Error::Open {
            path: track.to_path_buf(),
            source,
        })?;
        let decoder = Decoder::try_from(file).map_err(|source| Error::Decode {
            path: track.to_path_buf(),
            source,
        })?;
        self.inner.append(decoder);
        debug!(path = %track.display(), "piste empilée");
        Ok(())
    }

    /// Rang de la piste en cours dans la file.
    ///
    /// La sortie se vide par l'avant : les `restantes` dernières entrées de
    /// `charges` sont donc celles qui restent, et la première d'entre elles est
    /// la piste courante.
    fn index(&self) -> Option<usize> {
        let restantes = self.inner.len();
        if restantes == 0 {
            return None;
        }
        let rang = self.charges.len().checked_sub(restantes)?;
        self.charges.get(rang).copied()
    }

    /// Piste en cours de lecture, si la file n'est pas épuisée.
    pub fn current(&self) -> Option<&Path> {
        self.queue.get(self.index()?).map(PathBuf::as_path)
    }

    /// Pistes encore en file, celle en cours comprise.
    pub fn remaining(&self) -> usize {
        self.inner.len()
    }

    pub fn pause(&self) {
        self.inner.pause();
    }

    pub fn resume(&self) {
        self.inner.play();
    }

    /// Vide la file et arrête la sortie.
    pub fn stop(&mut self) {
        self.inner.stop();
        self.queue.clear();
        self.prochain = 0;
    }

    /// Passe à la piste suivante de la file.
    pub fn skip(&self) {
        self.inner.skip_one();
    }

    pub fn is_paused(&self) -> bool {
        self.inner.is_paused()
    }

    /// Vrai quand il n'y a plus rien à jouer.
    pub fn is_finished(&self) -> bool {
        self.inner.empty()
    }

    /// Position dans la piste en cours.
    pub fn position(&self) -> Duration {
        self.inner.get_pos()
    }

    /// Déplace la tête de lecture dans la piste en cours.
    pub fn seek(&self, pos: Duration) -> Result<()> {
        self.inner.try_seek(pos)?;
        Ok(())
    }

    /// Volume linéaire : 1.0 = niveau d'origine.
    ///
    /// `rodio::Float` vaut `f32` tant que la feature `f64` de `rodio` reste
    /// désactivée — c'est le cas, la liste des features est fixée dans le
    /// `Cargo.toml` racine. L'activer ferait échouer la compilation ici, ce
    /// qui vaut mieux qu'une conversion silencieuse.
    pub fn volume(&self) -> f32 {
        self.inner.volume()
    }

    pub fn set_volume(&self, volume: f32) {
        self.inner.set_volume(volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ouvre réellement la sortie audio : ignoré par défaut, une machine sans
    /// périphérique de sortie (CI) échouerait.
    /// À lancer avec `cargo test -p rusty-music-player -- --ignored`.
    #[test]
    #[ignore]
    fn ouvre_la_sortie_par_defaut() {
        let player = Player::new().expect("sortie audio indisponible");
        assert!(player.is_finished());
        assert!(player.current().is_none());
        assert_eq!(player.remaining(), 0);
    }
}
