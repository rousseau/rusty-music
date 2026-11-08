// SPDX-License-Identifier: GPL-3.0-or-later
//! Surveillance continue de la racine (notify).
//!
//! Les éditeurs de tags écrivent souvent en plusieurs opérations rapprochées :
//! on regroupe donc les évènements sur une courte fenêtre avant d'agir, plutôt
//! que de relire les tags à chaque notification.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tracing::{debug, info, warn};

use crate::error::Result;
use crate::{db::Library, is_audio, tags};

/// Fenêtre de regroupement des évènements.
pub const DEBOUNCE: Duration = Duration::from_millis(750);

/// Ce qu'un regroupement d'évènements a changé en base — rendu au rappel de
/// [`boucle`], pour que l'application prévienne son interface (le cœur ne
/// connaît pas Tauri).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Changements {
    pub ajoutes: usize,
    pub mis_a_jour: usize,
    pub retires: usize,
}

impl Changements {
    /// Vrai si rien n'a bougé en base (évènements sans effet, échecs seuls).
    pub fn est_vide(&self) -> bool {
        self.ajoutes == 0 && self.mis_a_jour == 0 && self.retires == 0
    }
}

/// Handle d'une surveillance active — opaque : ce qu'il y a dedans (un
/// `notify::RecommendedWatcher`) n'a pas besoin d'être connu de qui
/// l'appelle. L'abandonner (`drop`) arrête la surveillance du filesystem et
/// fait sortir la [`boucle`] qui lui est associée.
pub struct Surveillance(#[allow(dead_code)] notify::RecommendedWatcher);

/// Pose la surveillance de `root` et rend le handle à conserver, avec le
/// récepteur des évènements à passer à [`boucle`].
///
/// Ne bloque pas : poser un watcher est immédiat, seule [`boucle`] tourne
/// indéfiniment. Séparé de `watch_root` pour qui doit garder la main sur
/// plusieurs racines à la fois (l'application de bureau, une par racine) —
/// [`watch_root`] combine les deux pour l'usage à une seule racine du CLI.
pub fn demarrer(root: &Path) -> Result<(Surveillance, mpsc::Receiver<notify::Result<Event>>)> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok((Surveillance(watcher), rx))
}

/// Regroupe et applique les évènements du récepteur rendu par [`demarrer`].
///
/// Bloquant : à lancer dans un thread dédié. Sort quand la [`Surveillance`]
/// associée est abandonnée — c'est le seul signal d'arrêt, il n'y en a pas
/// d'autre à envoyer explicitement.
///
/// `rappel` reçoit, après chaque regroupement qui a changé la base, ce qui a
/// été ajouté, mis à jour et retiré ; il n'est pas appelé pour un
/// regroupement sans effet.
pub fn boucle(
    lib: &Library,
    rx: mpsc::Receiver<notify::Result<Event>>,
    mut rappel: impl FnMut(Changements),
) {
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut last_event = Instant::now();

    loop {
        match rx.recv_timeout(DEBOUNCE) {
            Ok(Ok(event)) => {
                last_event = Instant::now();
                debug!(kind = ?event.kind, paths = ?event.paths, "évènement brut");
                // Une simple lecture ne change rien : inutile de relire les tags.
                if !matches!(event.kind, EventKind::Access(_)) {
                    pending.extend(event.paths.iter().filter(|p| is_audio(p)).cloned());
                }
            }
            Ok(Err(e)) => warn!(error = %e, "évènement de surveillance invalide"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if !pending.is_empty() && last_event.elapsed() >= DEBOUNCE {
            let changements = flush(lib, &mut pending);
            if !changements.est_vide() {
                rappel(changements);
            }
        }
    }
}

/// Surveille `root` indéfiniment et met la base à jour. Bloquant : à lancer
/// dans un thread dédié.
pub fn watch_root(lib: &Library, root: &Path) -> Result<()> {
    let (_surveillance, rx) = demarrer(root)?;
    info!(root = %root.display(), "surveillance active");
    boucle(lib, rx, |_| {});
    Ok(())
}

/// Applique les changements accumulés pendant la fenêtre de regroupement.
///
/// Le type d'évènement ne dit pas de façon fiable s'il faut ajouter ou retirer :
/// FSEvents (macOS) livre en un seul lot l'historique cumulé d'un chemin — un
/// fichier supprimé arrive en `Create` + `Remove` + `Modify`, sans ordre
/// exploitable. Seul l'état du disque au moment du traitement fait foi : le
/// chemin existe encore ⇒ ajout ou mise à jour, il a disparu ⇒ retrait. Cela
/// couvre du même coup le renommage, qui n'est qu'une disparition suivie d'une
/// apparition.
fn flush(lib: &Library, pending: &mut HashSet<PathBuf>) -> Changements {
    let mut changements = Changements::default();
    for path in pending.drain() {
        // `Path::exists` renvoie faux aussi bien sur une absence avérée que
        // sur une erreur de permission ou d'E/S transitoire (support qui se
        // fait attendre) — la distinction compte : la première seule justifie
        // un retrait, dont la cascade emporterait des heures d'analyse sur un
        // simple incident de lecture. Même réflexe que `db::prune_missing`.
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                let connu = lib.contient_chemin(&path).unwrap_or(false);
                match tags::read(&path).and_then(|m| lib.upsert(&m)) {
                    Ok(_) if connu => {
                        changements.mis_a_jour += 1;
                        info!(path = %path.display(), "mis à jour");
                    }
                    Ok(_) => {
                        changements.ajoutes += 1;
                        info!(path = %path.display(), "ajouté");
                    }
                    Err(e) => warn!(path = %path.display(), error = %e, "ingestion impossible"),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match lib.remove_path(&path) {
                Ok(n) if n > 0 => {
                    changements.retires += 1;
                    info!(path = %path.display(), "retiré de la bibliothèque");
                }
                Ok(_) => {}
                Err(e) => warn!(path = %path.display(), error = %e, "suppression impossible"),
            },
            Err(e) => {
                warn!(path = %path.display(), error = %e, "métadonnées illisibles, morceau conservé par prudence");
            }
        }
    }
    changements
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::TrackMeta;

    /// Un chemin disparu doit sortir de la base quel que soit le type
    /// d'évènement reçu : FSEvents signale une suppression avec, entre autres,
    /// un `Modify` postérieur au `Remove`.
    #[test]
    fn flush_retire_les_chemins_disparus() {
        let lib = Library::open_in_memory().unwrap();
        let absent = PathBuf::from("/musique/disparu.flac");
        lib.upsert(&TrackMeta {
            path: absent.clone(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(lib.count().unwrap(), 1);

        let mut pending = HashSet::from([absent]);
        let changements = flush(&lib, &mut pending);

        assert_eq!(lib.count().unwrap(), 0);
        assert!(pending.is_empty());
        assert_eq!(
            changements,
            Changements {
                retires: 1,
                ..Default::default()
            }
        );
    }

    /// Un fichier copié dans une racine surveillée déclenche le rappel avec un
    /// ajout — c'est ce qui permet à l'application de recharger Écouter sans
    /// relance.
    #[test]
    fn boucle_signale_un_fichier_copie() {
        let racine =
            std::env::temp_dir().join(format!("rusty-music-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&racine);
        std::fs::create_dir_all(&racine).unwrap();
        // FSEvents rend des chemins canoniques (/private/var/… sur macOS).
        let racine = racine.canonicalize().unwrap();

        let (surveillance, rx) = demarrer(&racine).unwrap();
        let (tx_chg, rx_chg) = mpsc::channel::<Changements>();
        let fil = std::thread::spawn(move || {
            let lib = Library::open_in_memory().unwrap();
            boucle(&lib, rx, |c| {
                let _ = tx_chg.send(c);
            });
        });

        // Laisser le watcher s'installer avant d'écrire.
        std::thread::sleep(Duration::from_millis(300));
        crate::scan::tests::wav_minimal(&racine.join("nouveau.wav"));

        let recu = rx_chg.recv_timeout(Duration::from_secs(15));
        drop(surveillance);
        fil.join().unwrap();
        let _ = std::fs::remove_dir_all(&racine);

        let c = recu.expect("le rappel n'a pas été appelé");
        assert_eq!(c.ajoutes, 1, "{c:?}");
        assert_eq!(c.retires, 0, "{c:?}");
    }
}
