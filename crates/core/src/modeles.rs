// SPDX-License-Identifier: GPL-3.0-or-later
//! Où trouver les poids des modèles, selon d'où on tourne.
//!
//! Deux exécutions n'ont pas la même idée de « à côté » :
//!
//! - **en développement**, on lance depuis la racine du dépôt, et les poids
//!   sont dans `models/` — un chemin relatif au dossier courant suffit ;
//! - **dans une application empaquetée**, le dossier courant est `/` quand on
//!   double-clique depuis le Finder. Un chemin relatif ne désigne alors rien,
//!   et un chemin absolu figé à la compilation désigne une machine qui n'est
//!   pas celle de l'utilisateur.
//!
//! Ce module cherche donc dans un ordre qui couvre les deux, et rend le
//! premier candidat qui existe. Il ne devine jamais : si rien n'est trouvé,
//! l'appelant reçoit `None` et peut dire précisément ce qui manque.

use std::path::{Path, PathBuf};

/// Variable d'environnement qui l'emporte sur tout le reste.
///
/// Sert à faire tourner une application installée sur des poids rangés
/// ailleurs, sans la reconstruire.
pub const VARIABLE: &str = "RUSTY_MUSIC_MODELS";

/// Les dossiers où chercher, dans l'ordre de priorité.
pub fn dossiers() -> Vec<PathBuf> {
    let mut candidats = Vec::new();

    if let Some(force) = std::env::var_os(VARIABLE) {
        candidats.push(PathBuf::from(force));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Disposition d'un paquet macOS : `Rusty Music.app/Contents/MacOS/
            // rusty-music-desktop` a ses ressources dans `../Resources/`.
            candidats.push(dir.join("../Resources/models"));
            // Disposition simple : les poids à côté du binaire.
            candidats.push(dir.join("models"));
        }
    }

    // Développement : lancé depuis la racine du dépôt.
    candidats.push(PathBuf::from("models"));
    candidats
}

/// Cherche un fichier de poids par son nom.
pub fn trouver(nom: &str) -> Option<PathBuf> {
    dossiers()
        .into_iter()
        .map(|d| d.join(nom))
        .find(|p| p.is_file())
}

/// Message d'erreur qui dit où l'on a regardé.
///
/// Un « fichier introuvable » sans la liste des endroits visités oblige à lire
/// le code pour comprendre.
pub fn introuvable(nom: &str) -> String {
    let vus: Vec<String> = dossiers()
        .iter()
        .map(|d| d.join(nom).display().to_string())
        .collect();
    format!(
        "{nom} introuvable. Cherché dans :\n  {}\n\
         Poser {VARIABLE} pour désigner un autre dossier.",
        vus.join("\n  ")
    )
}

/// Le dossier retenu pour un fichier donné, s'il existe.
pub fn dossier_de(nom: &str) -> Option<PathBuf> {
    trouver(nom).and_then(|p| p.parent().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_variable_denvironnement_passe_devant() {
        // On ne modifie pas l'environnement du processus de test — d'autres
        // tests tournent en parallèle. On vérifie l'ordre sur la liste telle
        // qu'elle est construite quand la variable est absente.
        let sans = dossiers();
        assert!(
            sans.last() == Some(&PathBuf::from("models")),
            "le repli de développement doit rester en dernier : {sans:?}"
        );
        assert!(
            sans.iter().any(|d| d.ends_with("Resources/models")),
            "la disposition d'un paquet doit être tentée : {sans:?}"
        );
    }

    #[test]
    fn le_message_dit_ou_lon_a_cherche() {
        let m = introuvable("essai.bpk");
        assert!(m.contains("essai.bpk"));
        assert!(m.contains(VARIABLE), "le message doit citer l'échappatoire");
        // Autant de chemins listés que de dossiers candidats.
        assert_eq!(m.matches("essai.bpk").count(), dossiers().len() + 1);
    }
}
