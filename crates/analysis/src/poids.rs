// SPDX-License-Identifier: GPL-3.0-or-later
//! Où télécharger les poids de CLAP quand l'application ne les a pas.
//!
//! Les `.bpk` sont produits au build par `burn-onnx` depuis l'ONNX
//! (`build.rs`) : aucun dépôt d'origine ne les sert. Ils sont donc publiés,
//! sous la licence de CLAP (Apache-2.0), sur
//! [`rousseau/clap-htsat-unfused-burn`](https://huggingface.co/rousseau/clap-htsat-unfused-burn),
//! à une révision épinglée, et téléchargés au premier lancement de
//! l'application plutôt que livrés dans son paquet (590 Mo de moins dans le
//! `.dmg`).
//!
//! **Le fichier publié doit aller avec le code exécuté.** Deux builds du même
//! ONNX avec la même version de `burn-onnx` produisent des `.bpk` dont seule
//! l'en-tête diffère (des `param_id` tirés au hasard) : la section des
//! données, elle, est identique octet pour octet. `sha256_donnees` est
//! l'empreinte de cette section ; `scripts/release.sh` refuse de construire un
//! paquet dont les `.bpk` fraîchement produits ne la retrouvent pas — il faut
//! alors republier (`scripts/publier-clap.sh`) et mettre à jour ce fichier.
//! Un `.bpk` dont les noms de tenseurs ne correspondent plus échoue au
//! chargement (tenseurs manquants), il ne donne pas d'empreintes fausses.

/// Un fichier de poids publié : son adresse, sa taille, ses empreintes.
#[derive(Debug, Clone, Copy)]
pub struct PoidsEnLigne {
    pub nom: &'static str,
    pub url: &'static str,
    pub octets: u64,
    /// Du fichier entier, tel que servi : vérifié au téléchargement.
    pub sha256: &'static str,
    /// De la seule section des données (voir la documentation du module).
    pub sha256_donnees: &'static str,
}

/// Révision du dépôt Hugging Face d'où viennent les deux fichiers.
pub const REVISION: &str = "63b85b680d4db732a9acdce18f2dda0f9b718ea1";

pub const AUDIO: PoidsEnLigne = PoidsEnLigne {
    nom: crate::Embedder::POIDS,
    url: "https://huggingface.co/rousseau/clap-htsat-unfused-burn/resolve/63b85b680d4db732a9acdce18f2dda0f9b718ea1/clap-audio-encoder-b5.bpk",
    octets: 117_145_344,
    sha256: "7f352f3167c0432aff0a68a6a8b0edcda84bfa55f29111d8afc14684e40899b0",
    sha256_donnees: "d6881ae97509b165429fbb8746ba5c890315c5a5a22468fd572d0cb7744264b4",
};

pub const TEXTE: PoidsEnLigne = PoidsEnLigne {
    nom: crate::EmbedderTexte::POIDS,
    url:
        "https://huggingface.co/rousseau/clap-htsat-unfused-burn/resolve/63b85b680d4db732a9acdce18f2dda0f9b718ea1/clap-text-encoder.bpk",
    octets: 501_233_664,
    sha256: "d739ab9e0d4001736a7c38a2cebc4f6ffae439fbfc3657e88d905d847e1a2546",
    sha256_donnees: "efd7a2d9561e76a7cbe04f1c24ce592ab8aa3a632dcbef478ce9bee28b2cf5a0",
};

impl PoidsEnLigne {
    pub fn present(&self) -> bool {
        rusty_music_core::modeles::trouver(self.nom).is_some()
    }

    /// Télécharge le fichier s'il manque, empreinte vérifiée.
    pub fn telecharger(
        &self,
        avancer: impl FnMut(u64, Option<u64>),
    ) -> rusty_music_core::Result<std::path::PathBuf> {
        if let Some(p) = rusty_music_core::modeles::trouver(self.nom) {
            return Ok(p);
        }
        rusty_music_core::modeles::telecharger(
            self.nom,
            self.url,
            self.octets,
            Some(self.sha256),
            avancer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_adresses_designent_les_fichiers_attendus() {
        for p in [AUDIO, TEXTE] {
            assert!(p.url.starts_with("https://huggingface.co/"), "{}", p.url);
            assert!(p.url.ends_with(&format!("/{}", p.nom)), "{}", p.url);
            assert!(
                p.url.contains(&format!("/resolve/{REVISION}/")),
                "{}",
                p.url
            );
            assert_eq!(p.sha256.len(), 64);
            assert_eq!(p.sha256_donnees.len(), 64);
        }
    }
}
