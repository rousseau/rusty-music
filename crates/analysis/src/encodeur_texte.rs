// SPDX-License-Identifier: GPL-3.0-or-later
//! L'encodeur texte de CLAP, exécuté par Burn — le pendant de `encodeur.rs`
//! pour le champ d'intention d'Explorer (« texte → playlist »).
//!
//! Comme l'audio, le graphe est traduit en Rust natif par `burn-onnx` au
//! moment du build (`build.rs`) ; ce module l'habille et y ajoute le
//! tokeniseur RoBERTa qui manquait à `experiments/clap-texte` (qui
//! consommait des identifiants de jetons précalculés par Python, voir son
//! README).
//!
//! **Toujours en backend `NdArray` (CPU), jamais conditionné par le feature
//! `gpu`/`cpu` qui choisit le backend de la tour audio.** Une requête
//! utilisateur ponctuelle (une par prompt de playlist, jamais par fenêtre
//! audio) tourne en 91 ms sur CPU (`experiments/clap-texte/README.md`) —
//! largement sous le seuil où partager un device `wgpu` entre deux modèles
//! vaudrait la complexité. Le poids (478 Mo, quatre fois la tour audio) est
//! celui d'un RoBERTa-base complet.
//!
//! Modèle et tokeniseur : `laion/clap-htsat-unfused`, Apache-2.0 — le même
//! dépôt que l'encodeur audio (voir le module doc de `crate`), la tour texte
//! n'ayant pas de licence séparée de ses poids.

use std::path::Path;

use burn::tensor::{Int, Tensor, TensorData};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

use crate::DIMS;

/// Le tokeniseur embarqué dans le binaire, pas lu à l'exécution.
///
/// 2 Mo, committé (`crates/analysis/tokenizer/tokenizer.json`, contrairement
/// aux poids `.bpk`, gitignorés) : contrairement au `.bpk` de 478 Mo, rien ne
/// justifie une étape de préparation ni une déclaration dans les ressources
/// du paquet Tauri — `include_bytes!` le rend disponible partout où le
/// binaire l'est, sans chemin à résoudre entre poste de développement et
/// application installée.
const TOKENIZER_JSON: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tokenizer/tokenizer.json"));

/// Le code produit par `burn-onnx`. Généré, donc non conforme à nos usages de
/// nommage : on ne le relit pas, on le régénère.
#[allow(clippy::all, dead_code, unused_variables, non_snake_case)]
mod genere {
    include!(concat!(env!("OUT_DIR"), "/model/clap-text-encoder.rs"));
}

/// Toujours CPU — voir la documentation de module.
pub type Moteur = burn::backend::NdArray<f32>;

type Peripherique = burn::tensor::Device<Moteur>;

/// Longueur de séquence figée par l'export (`sonder.py export --longueur
/// 32`, voir `scripts/preparer-clap-texte.sh`) : c'est la forme sur laquelle
/// `burn-onnx` a généré le code, elle ne se change pas à l'exécution.
pub const LONGUEUR: usize = 32;

/// Préfixe de légende employé à l'entraînement de CLAP — un mot nu
/// (« drums ») n'est pas la forme sur laquelle le modèle a appris, une phrase
/// l'est (`docs/suite.md` §7, `experiments/clap-texte/sonder.py::PROMPT`).
pub const PROMPT: &str = "This is a sound of ";

/// Encodeur texte chargé en mémoire, prêt à projeter des phrases dans le même
/// espace à [`DIMS`] dimensions que les empreintes audio.
pub struct EmbedderTexte {
    modele: genere::Model<Moteur>,
    tokenizer: Tokenizer,
    device: Peripherique,
}

impl EmbedderTexte {
    /// Nom du fichier de poids, tel qu'il est embarqué dans une application.
    pub const POIDS: &'static str = "clap-text-encoder.bpk";

    /// Charge les poids et le tokeniseur.
    ///
    /// Même ordre de recherche que [`crate::Embedder::charger`] pour les
    /// poids : chemin explicite, puis ceux que ce build vient de produire
    /// (`RM_POIDS_CLAP_TEXT_ENCODER`), puis les dossiers de
    /// `rusty_music_core::modeles`. Le tokeniseur, lui, est embarqué dans le
    /// binaire ([`TOKENIZER_JSON`]), rien à en charger séparément.
    pub fn charger(poids: Option<&Path>) -> crate::Result<Self> {
        let trouve;
        let poids = match poids {
            Some(p) => p,
            None => {
                let du_build = std::path::PathBuf::from(env!("RM_POIDS_CLAP_TEXT_ENCODER"));
                trouve = if du_build.is_file() {
                    du_build
                } else {
                    rusty_music_core::modeles::trouver(Self::POIDS).unwrap_or_default()
                };
                &trouve
            }
        };
        if !poids.exists() {
            return Err(crate::Error::PoidsAbsents(
                rusty_music_core::modeles::introuvable(Self::POIDS),
            ));
        }

        let mut tokenizer = Tokenizer::from_bytes(TOKENIZER_JSON)
            .map_err(|e| crate::Error::Sortie(format!("tokeniseur illisible : {e}")))?;
        // `padding="max_length", max_length=LONGUEUR, truncation=True` côté
        // Python (`sonder.py::cmd_export`) — le graphe généré est figé sur
        // cette forme, une longueur variable ne s'y présenterait pas.
        let pad_id = tokenizer
            .token_to_id("<pad>")
            .ok_or_else(|| crate::Error::Sortie("jeton <pad> absent du tokeniseur".into()))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(LONGUEUR),
            pad_id,
            pad_token: "<pad>".to_string(),
            ..Default::default()
        }));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: LONGUEUR,
                ..Default::default()
            }))
            .map_err(|e| crate::Error::Sortie(format!("réglage de troncature : {e}")))?;

        let device = Peripherique::default();
        let modele = genere::Model::from_file(poids, &device);
        Ok(Self { modele, tokenizer, device })
    }

    /// Empreinte d'une phrase descriptive, dans le même espace à [`DIMS`]
    /// dimensions que les empreintes audio — déjà normalisée : le graphe
    /// exporté inclut la même division par la norme que la tour audio
    /// embarque déjà sa projection (`experiments/clap-texte/
    /// sonder.py::cmd_export`).
    ///
    /// `phrase` est préfixée par [`PROMPT`] avant tokenisation, comme à
    /// l'entraînement — ne pas la préfixer deux fois côté appelant.
    pub fn embed(&self, phrase: &str) -> crate::Result<Vec<f32>> {
        let texte = format!("{PROMPT}{phrase}");
        let encodage = self
            .tokenizer
            .encode(texte, true)
            .map_err(|e| crate::Error::Sortie(format!("tokenisation : {e}")))?;

        // `i64`, pas `u32` : c'est le type que `TensorData` attend côté Int,
        // même choix que la référence Python de l'essai
        // (`experiments/clap-texte/src/main.rs`).
        let ids: Vec<i64> = encodage.get_ids().iter().map(|&i| i64::from(i)).collect();
        let masque: Vec<i64> = encodage
            .get_attention_mask()
            .iter()
            .map(|&i| i64::from(i))
            .collect();
        debug_assert_eq!(ids.len(), LONGUEUR);

        let ids =
            Tensor::<Moteur, 1, Int>::from_data(TensorData::new(ids, [LONGUEUR]), &self.device)
                .reshape([1, LONGUEUR]);
        let masque =
            Tensor::<Moteur, 1, Int>::from_data(TensorData::new(masque, [LONGUEUR]), &self.device)
                .reshape([1, LONGUEUR]);

        let sortie: Vec<f32> = self
            .modele
            .forward(ids, masque)
            .into_data()
            .to_vec()
            .map_err(|e| crate::Error::Sortie(format!("{e:?}")))?;

        if sortie.len() != DIMS {
            return Err(crate::Error::Sortie(format!(
                "{} valeurs pour {DIMS} attendues",
                sortie.len()
            )));
        }
        Ok(sortie)
    }
}
