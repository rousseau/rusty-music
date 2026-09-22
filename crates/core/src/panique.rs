// SPDX-License-Identifier: GPL-3.0-or-later
//! Convertit une panique en erreur plutôt qu'en déroulement.
//!
//! Les passes de fond (scan, loudness, empreintes CLAP…) traitent la
//! bibliothèque entière fichier par fichier, dans un pool de threads
//! (`std::thread::scope`). Un fichier mal formé peut faire paniquer son
//! décodeur (symphonia, lofty, un modèle d'inférence) — sans protection, cette
//! panique remonte hors du `scope` et **avorte toute la passe**, souvent
//! après avoir déjà traité des milliers de fichiers sans panne : un seul
//! fichier fautif ferait recommencer toute une passe de plusieurs heures.
//!
//! Générique et sans opinion sur le type d'erreur du domaine appelant :
//! [`sans_panique`] ne rend qu'un message, à l'appelant de l'associer au
//! chemin ou au contexte qui l'a produit — voir `crate::error::Error::Panique`
//! pour la variante qu'en fait le cœur, et `rusty_music_analysis::decode::Error`
//! pour celle qu'en fait le décodage du module d'analyse.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Exécute `f`, rattrapant une panique éventuelle en `Err(message)` plutôt que
/// de la laisser dérouler et emporter tout ce qui l'entoure.
///
/// **`AssertUnwindSafe` interne, pas laissé à la charge de l'appelant.** Le
/// compilateur refuse `catch_unwind` sur une fermeture qui emprunte un état
/// mutable partagé (par précaution : la panique a pu l'interrompre à moitié
/// modifié) — c'est le cas ici dès qu'un appelant capture, par exemple, un
/// plan de FFT mis en cache derrière un `Arc<dyn Trait>` (`rustfft`, entre
/// autres, n'implémente pas `RefUnwindSafe` sur ses plans). Mais `f` est
/// systématiquement jetée dès son retour ou sa panique, sans qu'aucun de ses
/// emprunts ne survive à cet appel : rien de potentiellement incohérent
/// n'est jamais relu, ce qui rend l'affirmation sûre en pratique — c'est
/// exactement l'usage que la documentation de `AssertUnwindSafe` recommande.
pub fn sans_panique<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(message_de)
}

fn message_de(cause: Box<dyn std::any::Any + Send>) -> String {
    match cause.downcast::<&str>() {
        Ok(s) => return s.to_string(),
        Err(cause) => cause,
    }
    .downcast::<String>()
    .map(|s| *s)
    .unwrap_or_else(|_| "panique sans message".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_panique_devient_un_message() {
        let r: Result<(), String> = sans_panique(|| panic!("décodage impossible"));
        assert_eq!(r, Err("décodage impossible".to_string()));
    }

    #[test]
    fn une_panique_formatee_devient_aussi_un_message() {
        let n = 3;
        let r: Result<(), String> = sans_panique(|| panic!("échec sur la piste {n}"));
        assert_eq!(r, Err("échec sur la piste 3".to_string()));
    }

    #[test]
    fn le_travail_normal_traverse_sans_toucher_au_resultat() {
        assert_eq!(sans_panique(|| 42), Ok(42));
        assert_eq!(sans_panique(|| Err::<i32, _>("échec ordinaire")), Ok(Err("échec ordinaire")));
    }
}
