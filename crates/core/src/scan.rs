// SPDX-License-Identifier: GPL-3.0-or-later
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

/// Parcourt `root` et ingère tous les fichiers musicaux, sur `jobs` threads
/// de lecture.
///
/// Les fichiers déjà en base et inchangés (même taille, même mtime) sont
/// sautés sans relire les tags. Un fichier illisible n'interrompt pas le
/// scan : il est compté dans `failed` et journalisé.
///
/// En fin de parcours, les morceaux de `root` dont le fichier a disparu sont
/// retirés : c'est ce qui rattrape les suppressions faites pendant que rien ne
/// surveillait le dossier.
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

    // Aucun fichier vu du tout : un montage vide mais toujours là (volume
    // débranché, partage réseau qui répond mais ne sert plus rien) est
    // indiscernable d'une racine réellement vidée par l'utilisateur, et
    // `WalkDir` (`filter_map(|e| e.ok())` ci-dessus) avale ses propres erreurs
    // sans les compter. Élaguer ici prendrait un incident de montage pour un
    // grand ménage et viderait toute la racine d'un coup. On saute l'élagage
    // par prudence plutôt que de risquer ça — au prix, en échange, de ne
    // rattraper une racine réellement vidée qu'au prochain fichier qui y
    // réapparaît.
    rep.removed = if rep.seen == 0 {
        debug!(root = %root.display(), "aucun fichier vu : élagage sauté par prudence");
        0
    } else {
        lib.prune_missing(root)?
    };
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
                // Un fichier dont la lecture des tags panique (lofty sur un
                // conteneur malformé) ne doit pas emporter tout le scan avec
                // lui — voir `crate::panique`.
                let resultat = match crate::panique::sans_panique(|| tags::read(path)) {
                    Ok(r) => r,
                    Err(message) => Err(Error::Panique { path: path.clone(), message }),
                };
                // Le récepteur est parti (impossible ici, mais évite de tourner
                // dans le vide si la boucle d'écriture s'arrêtait un jour).
                if tx.send((path.clone(), resultat)).is_err() {
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
                        // Un fichier qui échouait avant et se lit maintenant
                        // — tags corrigés, remplacé — n'a plus sa place dans
                        // la liste des échecs.
                        let _ = lib.effacer_echec_scan(&path);
                    }
                    Err(e) => {
                        rep.failed += 1;
                        warn!(path = %path.display(), error = %e, "insertion impossible");
                        let _ = lib.enregistrer_echec_scan(&path, &e.to_string());
                    }
                },
                Err(e) => {
                    rep.failed += 1;
                    warn!(path = %path.display(), error = %e, "tags illisibles");
                    let _ = lib.enregistrer_echec_scan(&path, &e.to_string());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un WAV PCM16 mono minimal, écrit à la main — même patron que
    /// `loudness::tests::wav_sinus`, sans le sinus : un scan ne lit que les
    /// tags, pas le contenu.
    fn wav_minimal(chemin: &Path) {
        let donnees = vec![0u8; 4410 * 2];
        let octets_data = donnees.len() as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + octets_data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&44_100u32.to_le_bytes());
        wav.extend_from_slice(&88_200u32.to_le_bytes()); // octets/s
        wav.extend_from_slice(&2u16.to_le_bytes()); // alignement bloc
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits/échantillon
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&octets_data.to_le_bytes());
        wav.extend_from_slice(&donnees);
        std::fs::write(chemin, wav).expect("écriture wav");
    }

    /// Une racine vidée mais toujours là (montage débranché, partage réseau
    /// en panne) ne doit pas faire disparaître les pistes qu'on y connaissait
    /// — voir la garde de `scan_root_jobs` : sans fichier vu du tout, on ne
    /// tente pas l'élagage plutôt que de risquer de le confondre avec un vrai
    /// grand ménage.
    #[test]
    fn scan_root_jobs_ne_purge_pas_si_rien_vu() {
        let racine = std::env::temp_dir().join(format!(
            "rusty-music-scan-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&racine);
        std::fs::create_dir_all(&racine).expect("dossier de test");
        let fichier = racine.join("piste.wav");
        wav_minimal(&fichier);

        let lib = crate::db::Library::open_in_memory().expect("base en mémoire");
        let rep = scan_root_jobs(&lib, &racine, 1, false).expect("premier scan");
        assert_eq!(rep.seen, 1);
        assert_eq!(rep.inserted, 1);
        assert_eq!(lib.count().unwrap(), 1);

        // La racine « disparaît » : vidée sans que le dossier lui-même ne
        // s'efface — le cas d'un support débranché en cours de route.
        std::fs::remove_file(&fichier).expect("retrait du fichier");
        let rep2 = scan_root_jobs(&lib, &racine, 1, false).expect("second scan");
        assert_eq!(rep2.seen, 0, "aucun fichier audio vu cette fois");
        assert_eq!(rep2.removed, 0, "l'élagage est sauté, pas déclenché");
        assert_eq!(lib.count().unwrap(), 1, "la piste reste en base par prudence");

        let _ = std::fs::remove_dir_all(&racine);
    }
}
