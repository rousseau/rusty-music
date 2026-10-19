// SPDX-License-Identifier: GPL-3.0-or-later
//! L'iPhone du compagnon « Rusty Valise », vu du Mac : qui est branché, quand
//! l'app expire, quelles valises il porte, et le transfert dans les deux sens.
//!
//! Tout passe par `xcrun devicectl` (Xcode) : c'est l'outil d'Apple, en USB
//! comme en Wi-Fi, et le seul qui écrive dans le conteneur d'une app sans
//! passer par le Finder. Sans Xcode, rien de ce module ne marche — le mode
//! Valise le dit et garde l'export de la valise dans un dossier. Usage
//! personnel seulement (`docs/plan-ios-compagnon.md`, « Distribution »).
//!
//! `devicectl` ne sait pas supprimer un fichier sur l'appareil : les valises
//! remplacées sont listées dans [`A_RETIRER`], que l'app supprime elle-même.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

/// Identifiant de l'app compagnon (`apps/mobile/tauri.conf.json`).
pub const BUNDLE: &str = "fr.rousseau.rustyvalise";

/// Fichier déposé à la racine des Documents de l'app : les noms des valises
/// qu'elle peut supprimer, leurs écoutes étant déjà reversées au Mac.
pub const A_RETIRER: &str = "a-retirer.json";

/// Un iPhone appairé avec ce Mac.
#[derive(Debug, Clone, Serialize)]
pub struct Appareil {
    pub udid: String,
    pub nom: String,
    pub modele: String,
    /// Branché en USB (sinon joignable en Wi-Fi, s'il est sur le même réseau
    /// et déverrouillé).
    pub filaire: bool,
}

/// Une valise présente sur l'iPhone.
#[derive(Debug, Clone, Serialize)]
pub struct ValiseTelephone {
    pub nom: String,
    /// Date de modification du dossier (ISO 8601, UTC).
    pub le: Option<String>,
    /// Le téléphone y a noté des écoutes (`journal.json`).
    pub journal: bool,
}

/// Un fichier JSON temporaire pour `--json-output`, effacé à la fin.
struct SortieJson(PathBuf);

impl SortieJson {
    fn nouvelle() -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "rusty-music-devicectl-{}-{n}.json",
            std::process::id()
        )))
    }
}

impl Drop for SortieJson {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Lance `xcrun devicectl <args>` et rend son résultat JSON (`result`).
fn devicectl(args: &[&str]) -> Result<Value, String> {
    let sortie = SortieJson::nouvelle();
    let res = Command::new("xcrun")
        .arg("devicectl")
        .args(args)
        .arg("--json-output")
        .arg(&sortie.0)
        .output()
        .map_err(|e| format!("xcrun introuvable (Xcode est-il installé ?) : {e}"))?;
    let json: Value = std::fs::read(&sortie.0)
        .ok()
        .and_then(|o| serde_json::from_slice(&o).ok())
        .unwrap_or_default();
    if !res.status.success() {
        // Le message utile est dans le JSON (`error.userInfo`), sinon sur stderr.
        let msg = json["error"]["userInfo"]["NSLocalizedDescription"]["string"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                String::from_utf8_lossy(&res.stderr)
                    .lines()
                    .rfind(|l| !l.trim().is_empty())
                    .unwrap_or("échec de devicectl")
                    .trim()
                    .to_owned()
            });
        return Err(msg);
    }
    Ok(json["result"].clone())
}

/// Xcode (et donc `devicectl`) est-il là ?
pub fn xcode_present() -> bool {
    Command::new("xcrun")
        .args(["--find", "devicectl"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Les iPhone physiques appairés (pas les simulateurs).
pub fn appareils() -> Result<Vec<Appareil>, String> {
    let r = devicectl(&["list", "devices"])?;
    Ok(r["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| {
            d["hardwareProperties"]["reality"] == "physical"
                && d["hardwareProperties"]["platform"] == "iOS"
                && d["connectionProperties"]["pairingState"] == "paired"
        })
        .filter_map(|d| {
            Some(Appareil {
                udid: d["hardwareProperties"]["udid"].as_str()?.to_owned(),
                nom: d["deviceProperties"]["name"].as_str().unwrap_or("iPhone").to_owned(),
                modele: d["hardwareProperties"]["marketingName"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                filaire: d["connectionProperties"]["transportType"] == "wired",
            })
        })
        .collect())
}

/// Date d'expiration (ISO 8601) du profil de signature de l'app sur
/// l'appareil — la plus lointaine s'il y en a plusieurs. `None` : l'app n'y
/// est pas installée par Xcode.
pub fn echeance(udid: &str) -> Result<Option<String>, String> {
    let r = devicectl(&["device", "profile", "list", "--device", udid])?;
    Ok(r["provisioningProfiles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["name"].as_str().is_some_and(|n| n.ends_with(BUNDLE)))
        .filter_map(|p| p["expirationDate"].as_str())
        .max()
        .map(str::to_owned))
}

/// L'app compagnon est-elle installée ?
pub fn app_installee(udid: &str) -> Result<bool, String> {
    let r = devicectl(&["device", "info", "apps", "--device", udid, "--bundle-id", BUNDLE])?;
    Ok(r["apps"].as_array().is_some_and(|a| !a.is_empty()))
}

/// Les valises dans les Documents de l'app : les dossiers de premier niveau
/// qui portent un `valise.json`.
pub fn valises(udid: &str) -> Result<Vec<ValiseTelephone>, String> {
    let r = devicectl(&[
        "device", "info", "files", "--device", udid,
        "--domain-type", "appDataContainer", "--domain-identifier", BUNDLE,
        "--subdirectory", "Documents",
    ])?;
    let fichiers = r["files"].as_array().cloned().unwrap_or_default();
    let mut out: Vec<ValiseTelephone> = fichiers
        .iter()
        .filter_map(|f| {
            let chemin = f["relativePath"].as_str()?;
            let nom = chemin.strip_suffix("/valise.json")?;
            (!nom.contains('/')).then(|| ValiseTelephone {
                nom: nom.to_owned(),
                le: fichiers
                    .iter()
                    .find(|d| d["relativePath"] == nom)
                    .and_then(|d| d["metadata"]["lastModDate"].as_str())
                    .map(str::to_owned),
                journal: fichiers
                    .iter()
                    .any(|d| d["relativePath"] == format!("{nom}/{}", rusty_music_core::valise::JOURNAL)),
            })
        })
        .collect();
    out.sort_by(|a, b| b.le.cmp(&a.le));
    Ok(out)
}

/// Rapatrie `Documents/<chemin>` de l'app dans le fichier `destination`.
pub fn copier_depuis(udid: &str, chemin: &str, destination: &Path) -> Result<(), String> {
    let source = format!("Documents/{chemin}");
    devicectl(&[
        "device", "copy", "from", "--device", udid,
        "--domain-type", "appDataContainer", "--domain-identifier", BUNDLE,
        "--source", &source,
        "--destination", &destination.to_string_lossy(),
    ])
    .map(|_| ())
}

/// Dépose `source` (fichier ou dossier) en `Documents/<chemin>` dans l'app.
pub fn copier_vers(udid: &str, source: &Path, chemin: &str) -> Result<(), String> {
    let destination = format!("Documents/{chemin}");
    devicectl(&[
        "device", "copy", "to", "--device", udid,
        "--domain-type", "appDataContainer", "--domain-identifier", BUNDLE,
        "--source", &source.to_string_lossy(),
        "--destination", &destination,
    ])
    .map(|_| ())
}

/// Le script de réinstallation, s'il est là : il faut le dépôt (sources de
/// l'app mobile) et Xcode — une machine de développement, pas une app
/// empaquetée.
pub fn script_reinstaller() -> Option<PathBuf> {
    let s = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/reinstaller-valise.sh");
    s.is_file().then(|| s.canonicalize().unwrap_or(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contre un vrai iPhone : `cargo test -p rusty-music-desktop iphone -- --ignored --nocapture`.
    #[test]
    #[ignore = "demande un iPhone appairé et Xcode"]
    fn lit_l_iphone() {
        assert!(xcode_present());
        let appareils = appareils().unwrap();
        println!("{appareils:?}");
        let Some(a) = appareils.first() else { return };
        assert!(app_installee(&a.udid).unwrap());
        println!("échéance : {:?}", echeance(&a.udid).unwrap());
        println!("valises : {:?}", valises(&a.udid).unwrap());
        println!("script : {:?}", script_reinstaller());
    }
}
