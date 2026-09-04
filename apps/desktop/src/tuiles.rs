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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pmtiles::{AsyncPmTilesReader, MmapBackend, TileCoord};
use std::sync::RwLock;

use tauri::Manager;

/// Le protocole sous lequel la webview lit les tuiles.
pub const SCHEMA: &str = "tuiles";

/// Les deux archives, ouvertes à la demande et gardées en mémoire projetée.
///
/// Le verrou est **synchrone** et n'est jamais tenu au travers d'un `await` :
/// l'ouverture de l'archive se fait hors verrou, et le résultat n'est posé
/// qu'ensuite. La première version tenait un `tokio::sync::RwLock`, ce qui
/// obligeait `oublier()` à être `async` — et donc `engendrer_tuiles` à faire un
/// `block_on` depuis un fil du runtime, qui panique aussitôt.
#[derive(Default)]
pub struct Archives {
    carte: RwLock<Option<Arc<AsyncPmTilesReader<MmapBackend>>>>,
    relief: RwLock<Option<Arc<AsyncPmTilesReader<MmapBackend>>>>,
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

impl Archives {
    /// Oublie les archives ouvertes — à appeler après une régénération, sinon
    /// la webview continuerait de lire l'ancien fichier projeté en mémoire.
    pub fn oublier(&self) {
        if let Ok(mut c) = self.carte.write() {
            *c = None;
        }
        if let Ok(mut r) = self.relief.write() {
            *r = None;
        }
    }

    fn cellule(&self, quoi: &str) -> &RwLock<Option<Arc<AsyncPmTilesReader<MmapBackend>>>> {
        if quoi == "relief" {
            &self.relief
        } else {
            &self.carte
        }
    }

    async fn lecteur(
        &self,
        quoi: &str,
        chemin: &Path,
    ) -> Option<Arc<AsyncPmTilesReader<MmapBackend>>> {
        if let Some(l) = self.cellule(quoi).read().ok()?.as_ref() {
            return Some(l.clone());
        }
        // Ouverture hors verrou. Deux premières requêtes simultanées peuvent
        // ouvrir l'archive chacune de leur côté : la course est bénigne, un
        // `mmap` de plus ne coûte rien et la dernière écriture gagne.
        let backend = MmapBackend::try_from(chemin).await.ok()?;
        let lecteur = Arc::new(AsyncPmTilesReader::try_from_source(backend).await.ok()?);
        if let Ok(mut ecriture) = self.cellule(quoi).write() {
            *ecriture = Some(lecteur.clone());
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
    let fichier = if quoi == "relief" {
        chemin_relief(app)?
    } else {
        chemin_carte(app)?
    };
    let archives = app.state::<Archives>();
    let Some(lecteur) = archives.lecteur(quoi, &fichier).await else {
        anyhow::bail!("archive illisible : {}", fichier.display());
    };
    let coord = TileCoord::new(z, x, y)?;
    // Le relief est en PNG, déjà compressé et rangé tel quel ; les tuiles
    // vectorielles sont gzippées dans l'archive.
    let donnees = if quoi == "relief" {
        lecteur.get_tile(coord).await?
    } else {
        lecteur.get_tile_decompressed(coord).await?
    };
    Ok(donnees.map(|o| o.to_vec()))
}

/// Le nom d'archive demandé, validé. Le JS passe une chaîne : elle ne doit
/// pouvoir désigner que l'une des deux archives, jamais un chemin.
pub fn archive_valide(quoi: &str) -> bool {
    quoi == "carte" || quoi == "relief"
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
    fn seules_les_deux_archives_sont_acceptees() {
        assert!(archive_valide("carte"));
        assert!(archive_valide("relief"));
        assert!(!archive_valide("autre"));
        assert!(!archive_valide("../../etc/passwd"));
        assert!(!archive_valide(""));
    }
}
