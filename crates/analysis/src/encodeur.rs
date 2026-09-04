// SPDX-License-Identifier: GPL-3.0-or-later
//! L'encodeur audio de CLAP, exécuté par Burn.
//!
//! Le modèle n'est pas interprété au vol depuis un fichier ONNX : `burn-onnx`
//! en a généré du Rust natif au moment du build (`build.rs`), et ce module ne
//! fait que l'habiller. Le graphe importé est numériquement celui d'ONNX
//! Runtime — cosinus 1,0000000000, écart absolu maximal 1,4 × 10⁻⁶, soit de
//! l'arrondi `f32` (`experiments/burn-clap/README.md`).
//!
//! **Le modèle est figé sur un lot de [`LOT`] fenêtres.** C'est le prix de
//! l'import : les blocs Swin calculaient leurs marges à partir de la forme
//! d'entrée, et il a fallu la rendre constante pour que la génération de code
//! aboutisse. Un appel plus court est complété de fenêtres nulles, dont les
//! empreintes sont écartées.

use std::path::Path;

use burn::tensor::{Tensor, TensorData};

use crate::{DIMS, MELS, TRAMES};

/// Le code produit par `burn-onnx`. Généré, donc non conforme à nos usages de
/// nommage : on ne le relit pas, on le régénère.
#[allow(clippy::all, dead_code, unused_variables, non_snake_case)]
mod genere {
    include!(concat!(env!("OUT_DIR"), "/model/clap-audio-encoder-b5.rs"));
}

/// Le backend, choisi à la compilation — c'est ainsi que Burn s'y prend.
///
/// `wgpu` couvre Metal, Vulkan et DX12 d'un même code ; `--no-default-features
/// --features cpu` retombe sur `ndarray` pour une machine sans accélérateur.
/// Compiler les deux doublerait la monomorphisation d'un modèle de 4 400
/// lignes, pour un repli qui ne sert pas sur les machines visées.
#[cfg(feature = "gpu")]
pub type Moteur = burn::backend::Wgpu<f32, i32>;
#[cfg(not(feature = "gpu"))]
pub type Moteur = burn::backend::NdArray<f32>;

/// Fenêtres par appel. Doit rester égal à `decode::FENETRES` : c'est sur ce
/// nombre que le modèle a été figé.
pub const LOT: usize = 5;

/// Nom du backend, pour les traces — savoir sur quoi on a tourné.
pub fn moteur() -> &'static str {
    if cfg!(feature = "gpu") {
        "wgpu"
    } else {
        "ndarray (CPU)"
    }
}

/// Le périphérique du backend : premier accélérateur disponible pour `wgpu`,
/// processeur pour `ndarray`. `burn::tensor::Device` est un alias qui masque
/// le type concret, différent d'un backend à l'autre.
type Peripherique = burn::tensor::Device<Moteur>;

/// Encodeur chargé en mémoire, prêt à produire des empreintes.
pub struct Embedder {
    modele: genere::Model<Moteur>,
    device: Peripherique,
}

impl Embedder {
    /// Nom du fichier de poids, tel qu'il est embarqué dans une application.
    pub const POIDS: &'static str = "clap-audio-encoder-b5.bpk";

    /// Charge les poids.
    ///
    /// Ordre de recherche, et **cet ordre compte** : le chemin explicite s'il
    /// est donné, puis les poids que *ce* build vient de produire (`RM_POIDS`),
    /// puis seulement les dossiers de `rusty_music_core::modeles`.
    ///
    /// Chercher `models/` en premier serait le bogue d'hier : chaque profil de
    /// compilation régénère code **et** poids, et charger ceux d'un autre
    /// profil ne provoque aucune erreur — seulement des empreintes fausses.
    /// `RM_POIDS` désigne toujours ceux qui vont avec le code exécuté ; il
    /// n'existe que sur la machine de build, donc une application installée
    /// tombe naturellement sur ses ressources.
    ///
    /// **Des poids venus d'un autre build ne provoquent aucune erreur** — Burn
    /// charge ce qu'il reconnaît et laisse le reste à l'initialisation, d'où
    /// des empreintes silencieusement fausses. C'est ce que vérifie l'exemple
    /// `empreinte_reference`.
    ///
    /// `threads` est ignoré : le parallélisme est celui du backend.
    pub fn charger(poids: Option<&Path>, _threads: usize) -> crate::Result<Self> {
        let trouve;
        let poids = match poids {
            Some(p) => p,
            None => {
                let du_build = std::path::PathBuf::from(env!("RM_POIDS"));
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
        let device = Peripherique::default();
        let modele = genere::Model::from_file(poids, &device);
        Ok(Self { modele, device })
    }

    /// Empreintes d'un lot de fenêtres log-mel.
    ///
    /// `fenetres` contient `n × TRAMES × MELS` valeurs, à plat. Renvoie `n`
    /// vecteurs de [`DIMS`] dimensions. `n` peut valoir n'importe quoi : les
    /// appels sont découpés en lots de [`LOT`], le dernier complété de zéros.
    pub fn empreintes(&mut self, fenetres: &[f32], n: usize) -> crate::Result<Vec<Vec<f32>>> {
        debug_assert_eq!(fenetres.len(), n * TRAMES * MELS);
        if n == 0 {
            return Ok(Vec::new());
        }

        let par_fenetre = TRAMES * MELS;
        let mut sorties = Vec::with_capacity(n);

        for debut in (0..n).step_by(LOT) {
            let reste = (n - debut).min(LOT);
            let tranche = &fenetres[debut * par_fenetre..(debut + reste) * par_fenetre];

            // Complétées à `LOT` : le modèle n'accepte pas d'autre forme. Les
            // fenêtres ajoutées sont calculées pour rien, mais un lot partiel
            // reste rare — seul le dernier d'un morceau peut l'être.
            let entree = if reste == LOT {
                tranche.to_vec()
            } else {
                let mut v = tranche.to_vec();
                v.resize(LOT * par_fenetre, 0.0);
                v
            };

            let tenseur = Tensor::<Moteur, 1>::from_data(
                TensorData::new(entree, [LOT * par_fenetre]),
                &self.device,
            )
            .reshape([LOT, 1, TRAMES, MELS]);

            let plat: Vec<f32> = self
                .modele
                .forward(tenseur)
                .into_data()
                .to_vec()
                .map_err(|e| crate::Error::Sortie(format!("{e:?}")))?;

            if plat.len() != LOT * DIMS {
                return Err(crate::Error::Sortie(format!(
                    "{} valeurs pour {LOT} × {DIMS} attendues",
                    plat.len()
                )));
            }
            sorties.extend(plat.chunks_exact(DIMS).take(reste).map(<[f32]>::to_vec));
        }
        Ok(sorties)
    }
}
