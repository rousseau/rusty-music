// SPDX-License-Identifier: GPL-3.0-or-later
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

pub mod amelioration;
pub mod multipiste;
pub mod spectre;
pub mod waveform;
pub use amelioration::{amelioration, enregistrer_taux_sortie, Amelioration};
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
    Ok(Some(tampon_traite(
        piste.echantillons,
        rusty_music_core::opus::SR,
        canaux,
    )))
}

/// Applique la chaîne d'amélioration (rééchantillonnage vers la sortie,
/// excitation si « E » est actif) au tampon décodé, puis le remet en source
/// `rodio`. Point de passage unique des deux branches de [`ouvrir`].
fn tampon_traite(
    mut echantillons: Vec<f32>,
    taux: u32,
    canaux: u16,
) -> rodio::buffer::SamplesBuffer {
    let taux = amelioration::traiter(&mut echantillons, taux, canaux);
    rodio::buffer::SamplesBuffer::new(
        canaux.try_into().expect("au moins un canal"),
        taux.try_into().expect("fréquence non nulle"),
        echantillons,
    )
}

/// Décode entièrement une source en mémoire.
///
/// **C'est le tampon anti-coupure.** `rodio` tirait jusqu'ici ses échantillons
/// d'un `Decoder` branché sur le fichier : toute lecture pendant la lecture —
/// un scan, une passe d'analyse, l'OS qui sollicite le disque pour autre
/// chose — pouvait retarder un paquet et couper le son. Une fois décodée ici,
/// la piste ne dépend plus du disque : elle vit en RAM, comme les fichiers
/// Opus le font déjà plus haut ([`opus_en_memoire`]) par nécessité (`rodio` ne
/// sait pas les décoder en flux). On applique maintenant la même recette à
/// tous les formats, mais par choix.
///
/// Le seek en profite aussi : sauter dans un `SamplesBuffer` est un calcul
/// direct sur l'index, pas une reprise de décodage depuis la dernière image
/// clé.
///
/// Le prix est en mémoire (une soixantaine de mégaoctets pour quatre minutes
/// en stéréo) et en temps de préchargement — mais `ouvrir` n'est appelée que
/// par le préchargement (voir [`Player::completer`] et son commentaire côté
/// appli desktop), déjà conçu pour tolérer un disque lent.
fn decoder_en_memoire(source: Box<dyn rodio::Source + Send>) -> rodio::buffer::SamplesBuffer {
    let canaux = source.channels().get();
    let taux = source.sample_rate().get();
    let echantillons: Vec<rodio::Sample> = source.collect();
    tampon_traite(echantillons, taux, canaux)
}

/// Ouvre `track` et rend une source jouable, entièrement décodée en mémoire —
/// voir [`decoder_en_memoire`].
///
/// Fonction libre plutôt que méthode : elle peut donc s'exécuter hors du
/// verrou qui protège `Player`, entre [`Player::a_precharger`] et
/// [`Player::charger_precharge`] — c'est tout l'intérêt de la séparation.
pub fn ouvrir(track: &Path) -> Result<Box<dyn rodio::Source + Send>> {
    if let Some(buf) = opus_en_memoire(track)? {
        debug!(path = %track.display(), "piste Opus ouverte");
        return Ok(Box::new(buf));
    }
    let file = std::fs::File::open(track).map_err(|source| Error::Open {
        path: track.to_path_buf(),
        source,
    })?;
    let decoder = Decoder::try_from(file).map_err(|source| Error::Decode {
        path: track.to_path_buf(),
        source,
    })?;
    let tampon = decoder_en_memoire(Box::new(decoder));
    debug!(path = %track.display(), "piste décodée en mémoire");
    Ok(Box::new(tampon))
}

/// Spectrogramme du fichier **tel qu'il sortirait de [`ouvrir`]** : avec
/// l'excitateur « E » appliqué s'il est actif. Sert à montrer dans l'interface
/// ce que « E » ajoute, sans avoir à écrire un fichier.
pub fn spectre_ameliore(chemin: &Path, largeur: usize, hauteur: usize) -> Result<Spectre> {
    let source = ouvrir(chemin)?;
    let mono: Vec<rodio::Sample> = rodio::source::UniformSourceIterator::new(
        source,
        1.try_into().expect("1 canal"),
        multipiste::SR.try_into().expect("fréquence valide"),
    )
    .collect();
    Ok(spectre::calculer_echantillons(
        &mono,
        multipiste::SR,
        largeur,
        hauteur,
    ))
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
    /// Traduit un chemin de la file en chemin réellement ouvert — l'identité
    /// par défaut, un aiguillage vers le cache HD (`crates/superres`) côté
    /// application. `queue` garde toujours les chemins d'origine : c'est eux
    /// que `current` rend, et l'interface s'y repère.
    resoudre: Box<dyn Fn(&Path) -> PathBuf + Send + Sync>,
}

impl Player {
    /// Ouvre la sortie audio par défaut du système.
    pub fn new() -> Result<Self> {
        let mut output = DeviceSinkBuilder::open_default_sink()?;
        // On ferme la sortie sciemment en fin de processus : le message que
        // `rodio` émet alors sur stderr n'apprend rien et pollue la CLI.
        output.log_on_drop(false);
        // La fréquence de la carte son : `ouvrir` rééchantillonne le tampon
        // décodé vers elle (sinc propre), plutôt que de laisser `rodio` faire
        // son interpolation linéaire au mélangeur.
        amelioration::enregistrer_taux_sortie(output.config().sample_rate().get());
        let inner = rodio::Player::connect_new(output.mixer());
        Ok(Self {
            inner,
            _output: output,
            queue: Vec::new(),
            prochain: 0,
            charges: Vec::new(),
            resoudre: Box::new(|p| p.to_path_buf()),
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

    /// Remplace la file par `tracks`, sans couper la lecture en cours si le
    /// premier morceau ne change pas.
    ///
    /// Sert à la régénération d'un chemin sur la carte (curseur de bruit,
    /// « Autre tirage ») : le départ choisi reste le même, seule la suite
    /// change. Redémarrer à zéro à chaque ajustement du curseur coupait la
    /// piste en cours d'écoute — désagréable, et hors de propos puisque le
    /// départ n'a pas bougé. Ce qui est déjà confié à la sortie (`prochain`
    /// premiers rangs) ne peut de toute façon pas être retiré de `rodio` sans
    /// interrompre le son : on ne remplace donc que la suite pas encore
    /// chargée. Si le premier morceau diffère — un vrai autre départ, ou rien
    /// en cours de lecture — on retombe sur [`Self::play`], qui redémarre.
    pub fn set_queue(&mut self, tracks: &[PathBuf]) -> Result<()> {
        let meme_depart = matches!(
            (self.queue.first(), tracks.first()),
            (Some(a), Some(b)) if a == b
        );
        if !meme_depart || self.index().is_none() {
            return self.play(tracks);
        }
        let deja_confies = self.prochain.min(tracks.len());
        self.queue = self.queue[..deja_confies]
            .iter()
            .cloned()
            .chain(tracks[deja_confies..].iter().cloned())
            .collect();
        Ok(())
    }

    /// Remplace la file par `tracks` en gardant la piste en cours **sans
    /// coupure**, préchargement de l'ancienne file compris.
    ///
    /// Contrairement à [`Self::set_queue`], on ne conserve pas les `prochain`
    /// premiers rangs déjà confiés à la sortie : ce sont eux qui, sur le
    /// bouton ✦ « playlist dans l'esprit de ce morceau », faisaient jouer
    /// encore un ou deux morceaux de l'ancienne file avant que la nouvelle ne
    /// démarre. On vide la sortie et on y remet la seule piste en cours,
    /// rouverte à sa position (même procédé que [`Self::remplacer_courant`]) ;
    /// la suite est préparée par le sondage habituel, sur la nouvelle file.
    ///
    /// `tracks[0]` doit être la piste en cours et `source` en être une
    /// réouverture fraîche. Si rien ne joue, ou si la piste en cours a changé
    /// entre-temps, on retombe sur [`Self::play`] et `source` est ignorée.
    pub fn rebrancher_file(
        &mut self,
        tracks: &[PathBuf],
        source: Box<dyn rodio::Source + Send>,
    ) -> Result<()> {
        let joue_la_tete = self
            .current()
            .zip(tracks.first())
            .is_some_and(|(courant, tete)| courant == tete.as_path());
        if !joue_la_tete {
            return self.play(tracks);
        }
        let pos = self.inner.get_pos();
        let en_pause = self.inner.is_paused();
        self.inner.clear();
        self.inner.append(source);
        let _ = self.inner.try_seek(pos);
        if en_pause {
            self.inner.pause();
        } else {
            self.inner.play();
        }
        self.queue = tracks.to_vec();
        self.charges = vec![0];
        self.prochain = 1;
        Ok(())
    }

    /// Complète la réserve de pistes prêtes. À appeler régulièrement — c'est
    /// ce qui remplace le chargement intégral de la file.
    ///
    /// Enchaîne [`Self::a_precharger`], [`ouvrir`] et
    /// [`Self::charger_precharge`] verrou tenu tout du long : pratique pour
    /// un contexte à un seul fil (le CLI), mais **c'est cette I/O tenue sous
    /// verrou qui bloquait le bouton lecture/pause de l'appli desktop** —
    /// `toggle_pause` attend le même verrou que le sondage qui appelle cette
    /// fonction toutes les 200 ms. Là où plusieurs commandes se disputent le
    /// même verrou (`apps/desktop/src/main.rs`), appeler séparément les trois
    /// étapes permet de ne tenir le verrou que pour les deux qui ne touchent
    /// pas le disque.
    pub fn completer(&mut self) -> Result<()> {
        let Some((rang, piste)) = self.a_precharger() else {
            return Ok(());
        };
        let source = ouvrir(&piste)?;
        self.charger_precharge(rang, source);
        Ok(())
    }

    /// Installe l'aiguillage chemin de file → chemin réellement ouvert (cache
    /// HD). Voir le champ `resoudre`.
    pub fn set_resolveur(&mut self, f: impl Fn(&Path) -> PathBuf + Send + Sync + 'static) {
        self.resoudre = Box::new(f);
    }

    /// Piste à précharger, si la réserve n'est pas pleine — ou `None`. Ne
    /// fait aucune I/O, sûr à appeler verrou tenu. Le chemin rendu est déjà
    /// **résolu** (cache HD compris) : c'est celui à passer à [`ouvrir`].
    pub fn a_precharger(&mut self) -> Option<(usize, PathBuf)> {
        if self.inner.len() >= PRECHARGE || self.prochain >= self.queue.len() {
            return None;
        }
        let rang = self.prochain;
        let piste = (self.resoudre)(&self.queue[rang]);
        // On avance avant de tenter : une piste que `symphonia` ne sait pas
        // décoder — les fichiers Opus de la bibliothèque — bloquerait sinon la
        // file sur elle, réessayée à chaque passage.
        self.prochain += 1;
        Some((rang, piste))
    }

    /// Empile une source déjà ouverte par [`ouvrir`]. Ne fait aucune I/O, sûr
    /// à appeler verrou tenu.
    pub fn charger_precharge(&mut self, rang: usize, source: Box<dyn rodio::Source + Send>) {
        self.inner.append(source);
        self.charges.push(rang);
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

    /// Remplace la source de la piste **en cours** par `source` — typiquement
    /// une réouverture du même fichier avec l'amélioration (dés)activée — en
    /// reprenant à la position courante, sans coupure perceptible.
    ///
    /// `attendu` protège d'une course : si la piste a changé pendant que
    /// `source` se décodait en tâche de fond, on ne touche à rien. La file
    /// n'est pas modifiée ; le préchargement, vidé ici, est reconstruit par le
    /// sondage habituel (avec la nouvelle version).
    ///
    /// Ne tient le verrou que pour `clear`/`append`/`seek` — le décodage, lui,
    /// a eu lieu hors verrou dans [`ouvrir`].
    pub fn remplacer_courant(
        &mut self,
        attendu: &Path,
        source: Box<dyn rodio::Source + Send>,
    ) -> Result<()> {
        let Some(i) = self.index() else {
            return Ok(());
        };
        if self.queue.get(i).map(PathBuf::as_path) != Some(attendu) {
            return Ok(());
        }
        let pos = self.inner.get_pos();
        let en_pause = self.inner.is_paused();
        self.inner.clear();
        self.charges.clear();
        self.inner.append(source);
        self.charges.push(i);
        self.prochain = i + 1;
        // `SamplesBuffer` : le seek est un calcul d'index, pas une reprise de
        // décodage. Une piste plus courte que `pos` (ne devrait pas arriver)
        // laisse simplement le seek échouer sans conséquence.
        let _ = self.inner.try_seek(pos);
        if en_pause {
            self.inner.pause();
        } else {
            self.inner.play();
        }
        Ok(())
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
        // Une seule piste décodée ici, juste de quoi lancer le son : `ouvrir`
        // décode le fichier entier en mémoire (~0,3 à 1 s sur support lent) et
        // `charger` tient le verrou `Player`. En préparer `PRECHARGE` d'un coup
        // retardait le démarrage d'autant et gelait le transport (pause,
        // sondage) le temps du décodage. La réserve est complétée juste après,
        // hors verrou, par le sondage de `playback_state` (côté desktop) ou la
        // boucle du CLI — un fichier à la fois, toutes les 200 ms.
        self.completer()?;
        // `clear()` laisse le lecteur en pause : sans ça, rien ne sortirait.
        self.inner.play();
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
