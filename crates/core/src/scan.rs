//! Scan initial d'une racine : parcours récursif, lecture des tags, insertion.
//!
//! La lecture des tags passe par un petit pool de threads : elle est dominée
//! par l'attente disque, et les faire patienter en parallèle tient le support
//! occupé. Les écritures, elles, restent sur le thread appelant —
//! `rusqlite::Connection` n'est pas partageable entre threads, et sérialiser
//! les insertions évite d'avoir à verrouiller la base.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

use tracing::{debug, warn};
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::tags::TrackMeta;
use crate::{db::Library, is_audio, tags};

/// Nombre de threads de lecture par défaut.
///
/// Calé sur le nombre de cœurs : au-delà, on ne fait qu'allonger la file du
/// périphérique. `--jobs` permet d'ajuster selon le support (voir README).
pub fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ScanReport {
    pub seen: usize,
    pub inserted: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Morceaux retirés parce que le fichier a disparu depuis le dernier scan.
    pub removed: usize,
}

/// Parcourt `root` et ingère tous les fichiers musicaux.
///
/// Les fichiers déjà en base et inchangés (même taille, même mtime) sont
/// sautés sans relire les tags. Un fichier illisible n'interrompt pas le
/// scan : il est compté dans `failed` et journalisé.
///
/// En fin de parcours, les morceaux de `root` dont le fichier a disparu sont
/// retirés : c'est ce qui rattrape les suppressions faites pendant que rien ne
/// surveillait le dossier.
pub fn scan_root(lib: &Library, root: &Path) -> Result<ScanReport> {
    scan_root_jobs(lib, root, default_jobs(), false)
}

/// Comme [`scan_root`], en choisissant le nombre de threads de lecture.
///
/// `force` relit les tags de tous les fichiers, y compris ceux que la taille et
/// la mtime disent inchangés. C'est ce qu'il faut après avoir enrichi ce que
/// l'on extrait des tags : les fichiers, eux, n'ont pas bougé, donc le chemin
/// incrémental les sauterait tous et les nouvelles colonnes resteraient vides.
pub fn scan_root_jobs(lib: &Library, root: &Path, jobs: usize, force: bool) -> Result<ScanReport> {
    if !root.is_dir() {
        return Err(Error::NotADirectory(root.to_path_buf()));
    }
    lib.add_root(root)?;

    let mut rep = ScanReport::default();

    // 1er passage : parcours et tri. Le test « inchangé » interroge la base,
    // il reste donc ici ; seuls les fichiers à relire partent au pool.
    let mut a_lire: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file() || !is_audio(path) {
            continue;
        }
        rep.seen += 1;

        if let Ok(fs_meta) = entry.metadata() {
            if force {
                a_lire.push(path.to_path_buf());
                continue;
            }
            let size = fs_meta.len() as i64;
            let mtime = fs_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if lib.is_unchanged(path, size, mtime).unwrap_or(false) {
                rep.skipped += 1;
                continue;
            }
        }

        a_lire.push(path.to_path_buf());
    }

    // 2e passage : lecture des tags en parallèle, insertions sérialisées.
    lire_et_ingerer(lib, a_lire, jobs, &mut rep);

    rep.removed = lib.prune_missing(root)?;
    if rep.removed > 0 {
        debug!(count = rep.removed, "morceaux disparus retirés de la base");
    }

    lib.conn.execute(
        "UPDATE roots SET last_scan = strftime('%s','now') WHERE path = ?1",
        rusqlite::params![root.to_string_lossy()],
    )?;

    Ok(rep)
}

/// Lit les tags de `a_lire` sur `jobs` threads et insère les résultats.
///
/// Les threads se servent eux-mêmes dans la liste via un curseur atomique
/// plutôt que de se partager une file : pas de verrou, et un fichier lent ne
/// bloque pas ses voisins. Le canal est borné pour que les lecteurs ne
/// prennent pas trop d'avance sur les écritures.
fn lire_et_ingerer(lib: &Library, a_lire: Vec<PathBuf>, jobs: usize, rep: &mut ScanReport) {
    if a_lire.is_empty() {
        return;
    }
    let jobs = jobs.clamp(1, 64).min(a_lire.len());

    let a_lire = Arc::new(a_lire);
    let curseur = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::sync_channel::<(PathBuf, Result<TrackMeta>)>(jobs * 4);

    std::thread::scope(|pool| {
        for _ in 0..jobs {
            let a_lire = Arc::clone(&a_lire);
            let curseur = Arc::clone(&curseur);
            let tx = tx.clone();
            pool.spawn(move || loop {
                let i = curseur.fetch_add(1, Ordering::Relaxed);
                let Some(path) = a_lire.get(i) else { break };
                // Le récepteur est parti (impossible ici, mais évite de tourner
                // dans le vide si la boucle d'écriture s'arrêtait un jour).
                if tx.send((path.clone(), tags::read(path))).is_err() {
                    break;
                }
            });
        }
        // Sans cela, le canal ne se fermerait jamais : il resterait cet
        // émetteur-ci en vie et la boucle ci-dessous ne rendrait pas la main.
        drop(tx);

        for (path, res) in rx {
            match res {
                Ok(meta) => match lib.upsert(&meta) {
                    Ok(_) => {
                        rep.inserted += 1;
                        debug!(path = %path.display(), "ingéré");
                    }
                    Err(e) => {
                        rep.failed += 1;
                        warn!(path = %path.display(), error = %e, "insertion impossible");
                    }
                },
                Err(e) => {
                    rep.failed += 1;
                    warn!(path = %path.display(), error = %e, "tags illisibles");
                }
            }
        }
    });
}
