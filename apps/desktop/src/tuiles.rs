// SPDX-License-Identifier: GPL-3.0-or-later
//! Les tuiles de la carte, engendrées puis servies à la webview.
//!
//! MapLibre veut des tuiles derrière une URL. Rust lit l'archive PMTiles et les
//! rend une par une — ni `pmtiles.js`, ni requêtes à plage d'octets, dont le
//! support en webview varie d'un système à l'autre.
//!
//! **Le passage se fait par `maplibregl.addProtocol`, pas par un schéma d'URI
//! personnalisé**, et c'est une correction. Un schéma enregistré auprès de
//! Tauri (`tuiles://localhost/...`) n'est jamais atteint : MapLibre charge ses
//! tuiles depuis un *worker* construit sur une URL `blob:`, dont l'origine est
//! opaque, et WKWebView refuse en silence ses requêtes vers un schéma
//! personnalisé — sans erreur, sans trace, la carte reste simplement noire.
//! `addProtocol` fait tourner le chargement sur le fil principal, où `invoke`
//! fonctionne : plus de question d'origine ni de CORS.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pmtiles::{AsyncPmTilesReader, MmapBackend, TileCoord};
use std::sync::RwLock;

use tauri::Manager;

/// Le protocole sous lequel la webview lit les tuiles.
pub const SCHEMA: &str = "tuiles";

/// Les archives, ouvertes à la demande et gardées en mémoire projetée : le
/// plan (`carte`), l'ombrage du monde fictif (`relief`), et sur le plan réel
/// le relief musical de chaque grandeur (`dem-<grandeur>`,
/// `courbes-<grandeur>`, `explorer::relief`).
///
/// Le verrou est **synchrone** et n'est jamais tenu au travers d'un `await` :
/// l'ouverture de l'archive se fait hors verrou, et le résultat n'est posé
/// qu'ensuite. La première version tenait un `tokio::sync::RwLock`, ce qui
/// obligeait `oublier()` à être `async` — et donc `engendrer_tuiles` à faire un
/// `block_on` depuis un fil du runtime, qui panique aussitôt.
#[derive(Default)]
pub struct Archives {
    ouvertes: RwLock<HashMap<String, Arc<AsyncPmTilesReader<MmapBackend>>>>,
}

/// Où vivent les archives, à côté de la base.
pub fn dossier(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let d = app.path().app_data_dir()?.join("tuiles");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn chemin_carte(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    Ok(dossier(app)?.join("carte.pmtiles"))
}

pub fn chemin_relief(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    Ok(dossier(app)?.join("carte-relief.pmtiles"))
}

/// L'archive d'un nom validé par [`archive_valide`].
pub fn chemin_archive(app: &tauri::AppHandle, quoi: &str) -> anyhow::Result<PathBuf> {
    Ok(match quoi {
        "carte" => chemin_carte(app)?,
        "relief" => chemin_relief(app)?,
        autre => dossier(app)?.join(format!("{autre}.pmtiles")),
    })
}

impl Archives {
    /// Oublie les archives ouvertes — à appeler après une régénération, sinon
    /// la webview continuerait de lire l'ancien fichier projeté en mémoire.
    pub fn oublier(&self) {
        if let Ok(mut o) = self.ouvertes.write() {
            o.clear();
        }
    }

    /// Oublie une seule archive, réécrite (le relief d'une grandeur).
    pub fn oublier_une(&self, quoi: &str) {
        if let Ok(mut o) = self.ouvertes.write() {
            o.remove(quoi);
        }
    }

    async fn lecteur(
        &self,
        quoi: &str,
        chemin: &Path,
    ) -> Option<Arc<AsyncPmTilesReader<MmapBackend>>> {
        if let Some(l) = self.ouvertes.read().ok()?.get(quoi) {
            return Some(l.clone());
        }
        // Ouverture hors verrou. Deux premières requêtes simultanées peuvent
        // ouvrir l'archive chacune de leur côté : la course est bénigne, un
        // `mmap` de plus ne coûte rien et la dernière écriture gagne.
        let backend = MmapBackend::try_from(chemin).await.ok()?;
        let lecteur = Arc::new(AsyncPmTilesReader::try_from_source(backend).await.ok()?);
        if let Ok(mut ecriture) = self.ouvertes.write() {
            ecriture.insert(quoi.to_string(), lecteur.clone());
        }
        Some(lecteur)
    }
}

/// Les octets d'une tuile, ou rien si elle n'existe pas.
///
/// Une tuile absente est le cas ordinaire, pas une erreur : un monde à peu
/// près vide n'a de tuiles que là où il y a des morceaux.
pub async fn lire(
    app: &tauri::AppHandle,
    quoi: &str,
    z: u8,
    x: u32,
    y: u32,
) -> anyhow::Result<Option<Vec<u8>>> {
    let fichier = chemin_archive(app, quoi)?;
    let archives = app.state::<Archives>();
    let Some(lecteur) = archives.lecteur(quoi, &fichier).await else {
        anyhow::bail!("archive illisible : {}", fichier.display());
    };
    let coord = TileCoord::new(z, x, y)?;
    // Le relief et le modèle de terrain sont en PNG, déjà compressés et rangés
    // tels quels ; les tuiles vectorielles sont gzippées dans l'archive.
    let donnees = if quoi == "relief" || quoi.starts_with("dem-") {
        lecteur.get_tile(coord).await?
    } else {
        lecteur.get_tile_decompressed(coord).await?
    };
    Ok(donnees.map(|o| o.to_vec()))
}

/// Le nom d'archive demandé, validé. Le JS passe une chaîne : elle ne doit
/// pouvoir désigner que l'une des archives connues, jamais un chemin.
pub fn archive_valide(quoi: &str) -> bool {
    if quoi == "carte" || quoi == "relief" {
        return true;
    }
    let Some((genre, grandeur)) = quoi.split_once('-') else {
        return false;
    };
    matches!(genre, "dem" | "courbes")
        && rusty_music_carto::altitude::Grandeur::par_id(grandeur).is_some()
}

/// La base d'URL des tuiles dans le style.
///
/// Ce n'est pas une vraie origine : `maplibregl.addProtocol` intercepte tout ce
/// qui commence par `tuiles://`, et le JS en tire le nom d'archive et les
/// coordonnées. L'hôte est arbitraire.
pub fn base() -> String {
    format!("{SCHEMA}://tuiles")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seules_les_archives_connues_sont_acceptees() {
        assert!(archive_valide("carte"));
        assert!(archive_valide("relief"));
        assert!(archive_valide("dem-energie"));
        assert!(archive_valide("courbes-familiarite"));
        assert!(!archive_valide("dem-autre"));
        assert!(!archive_valide("dem-../carte"));
        assert!(!archive_valide("autre"));
        assert!(!archive_valide("../../etc/passwd"));
        assert!(!archive_valide(""));
    }
}
