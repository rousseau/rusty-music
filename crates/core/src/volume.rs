//! Nature du support qui porte un chemin — amovible ou non.
//!
//! Sert à moduler la concurrence d'un balayage : une carte SD ou une clé USB
//! sature vite sous des lectures concurrentes, au point de déstabiliser son
//! pilote (rencontré en pratique — voir le journal). Un disque interne encaisse
//! sans broncher le nombre de fils habituel.

use std::path::Path;

/// Vrai si `path` est situé sur un support amovible.
///
/// **macOS seulement pour l'instant** : interroge `diskutil info`, qui
/// connaît la nature du support sans qu'on ait à relire les indicateurs bas
/// niveau du montage. Ailleurs, ou si `diskutil` échoue ou ne dit rien de
/// concluant, on répond `false` — la prudence n'a de sens que là où on sait
/// reconnaître le risque, pas au prix de brider tout le monde par défaut.
pub fn est_amovible(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Ok(sortie) = std::process::Command::new("diskutil")
            .arg("info")
            .arg(path)
            .output()
        else {
            return false;
        };
        if !sortie.status.success() {
            return false;
        }
        let texte = String::from_utf8_lossy(&sortie.stdout);
        // Valeurs observées : `Fixed` pour un disque interne, `Removable`
        // pour une carte SD ou une clé USB. Comparer la valeur après le `:`,
        // pas la ligne entière — le libellé lui-même contient « Removable ».
        texte
            .lines()
            .find_map(|l| l.trim_start().strip_prefix("Removable Media:"))
            .is_some_and(|valeur| valeur.trim() == "Removable")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn le_disque_racine_n_est_pas_amovible() {
        assert!(!est_amovible(Path::new("/")));
    }
}
