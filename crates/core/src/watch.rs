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
pub fn boucle(lib: &Library, rx: mpsc::Receiver<notify::Result<Event>>) {
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
            flush(lib, &mut pending);
        }
    }
}

/// Surveille `root` indéfiniment et met la base à jour. Bloquant : à lancer
/// dans un thread dédié.
pub fn watch_root(lib: &Library, root: &Path) -> Result<()> {
    let (_surveillance, rx) = demarrer(root)?;
    info!(root = %root.display(), "surveillance active");
    boucle(lib, rx);
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
fn flush(lib: &Library, pending: &mut HashSet<PathBuf>) {
    for path in pending.drain() {
        if path.exists() {
            match tags::read(&path).and_then(|m| lib.upsert(&m)) {
                Ok(_) => info!(path = %path.display(), "ajouté ou mis à jour"),
                Err(e) => warn!(path = %path.display(), error = %e, "ingestion impossible"),
            }
        } else {
            match lib.remove_path(&path) {
                Ok(n) if n > 0 => info!(path = %path.display(), "retiré de la bibliothèque"),
                Ok(_) => {}
                Err(e) => warn!(path = %path.display(), error = %e, "suppression impossible"),
            }
        }
    }
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
        flush(&lib, &mut pending);

        assert_eq!(lib.count().unwrap(), 0);
        assert!(pending.is_empty());
    }
}
