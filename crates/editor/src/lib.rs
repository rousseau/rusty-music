// SPDX-License-Identifier: GPL-3.0-or-later
//! Démixage en stems (module 3 — Éditeur / MAO).
//!
//! Sépare un morceau en batterie, basse, voix et « autre », par HTDemucs
//! exécuté sur Burn. Le modèle n'est pas importé depuis ONNX : `docs/module3-
//! demixage.md` raconte pourquoi cette voie a été écartée — 66 % du graphe
//! exporté n'est qu'une transformée de Fourier déroulée, et le backend GPU
//! s'y trompait. On s'appuie sur `demucs-core`, où la STFT reste en Rust et
//! où Burn ne reçoit que le réseau.

pub mod decode;
pub mod etirement;
pub mod greffe;
pub mod wav;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use demucs_core::model::metadata::{self, ModelInfo, StemId};
use demucs_core::{Demucs, ModelOptions};

pub use decode::{Stereo, SR};

/// Le backend, choisi à la compilation — même règle que le module 2.
///
/// Côté GPU, `fusion` et `autotune` ne sont pas facultatifs pour ce modèle :
/// mesuré 90 s par segment sans eux, 728 ms avec. Ils sont activés par la
/// feature `gpu` du crate.
#[cfg(feature = "gpu")]
pub type Moteur = burn::backend::Wgpu<f32, i32>;
#[cfg(not(feature = "gpu"))]
pub type Moteur = burn::backend::NdArray<f32>;

/// Nom du backend, pour les traces.
pub fn moteur() -> &'static str {
    if cfg!(feature = "gpu") {
        "wgpu"
    } else {
        "ndarray (CPU)"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("poids du modèle introuvables : {0} — voir scripts/preparer-demucs.sh")]
    PoidsAbsents(String),

    #[error("lecture des poids : {0}")]
    Poids(#[from] std::io::Error),

    #[error("modèle : {0}")]
    Modele(String),

    #[error("décodage : {0}")]
    Decodage(#[from] decode::Error),

    #[error("écriture : {0}")]
    Ecriture(#[from] wav::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Exécute un futur jusqu'à son terme, sur le fil courant.
///
/// Les futurs de `demucs-core` n'attendent que le GPU : ni ordonnanceur ni
/// entrées-sorties, seulement d'être relancés quand leur réveil sonne. Trente
/// lignes suffisent, et `CLAUDE.md` demande de ne pas ajouter de dépendance
/// sans raison — `pollster`, que `demucs-rs` emploie pour exactement cela,
/// ne fait rien d'autre.
fn attendre<F: Future>(futur: F) -> F::Output {
    struct Reveil(std::thread::Thread);
    impl std::task::Wake for Reveil {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Reveil(std::thread::current())));
    let mut ctx = std::task::Context::from_waker(&waker);
    let mut futur = std::pin::pin!(futur);
    loop {
        match futur.as_mut().poll(&mut ctx) {
            std::task::Poll::Ready(v) => return v,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

/// Les variantes de HTDemucs disponibles.
///
/// Le défaut est [`Variante::Standard`] : c'est le meilleur rapport
/// qualité/temps, et les deux autres ont chacune une contrepartie que la
/// documentation amont assume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variante {
    /// 84 Mo, un réseau généraliste, quatre stems. 7,8 × le temps réel.
    #[default]
    Standard,
    /// 84 Mo, six stems — ajoute guitare et piano. Même vitesse, mais la
    /// séparation des quatre stems de base y est un peu moins bonne.
    SixStems,
    /// 333 Mo, quatre réseaux spécialisés (un par stem). La variante que
    /// Demucs recommande pour la qualité, et environ quatre fois plus lente
    /// puisqu'il faut faire tourner les quatre.
    Affinee,
}

impl Variante {
    /// Nom en ligne de commande, et nom du fichier de poids.
    pub fn nom(self) -> &'static str {
        match self {
            Variante::Standard => metadata::HTDEMUCS_ID,
            Variante::SixStems => metadata::HTDEMUCS_6S_ID,
            Variante::Affinee => metadata::HTDEMUCS_FT_ID,
        }
    }

    pub fn analyser(nom: &str) -> Option<Self> {
        match nom {
            metadata::HTDEMUCS_ID => Some(Variante::Standard),
            metadata::HTDEMUCS_6S_ID => Some(Variante::SixStems),
            metadata::HTDEMUCS_FT_ID => Some(Variante::Affinee),
            _ => None,
        }
    }

    fn infos(self) -> &'static ModelInfo {
        match self {
            Variante::Standard => &metadata::HTDEMUCS,
            Variante::SixStems => &metadata::HTDEMUCS_6S,
            Variante::Affinee => &metadata::HTDEMUCS_FT,
        }
    }

    /// Nom du fichier de poids attendu dans `models/`.
    pub fn fichier(self) -> &'static str {
        self.infos().filename
    }

    /// Poids du téléchargement, en mégaoctets.
    pub fn megaoctets(self) -> u32 {
        self.infos().size_mb
    }

    /// Les stems que cette variante produit.
    pub fn stems(self) -> &'static [StemId] {
        self.infos().stems
    }

    fn options(self) -> ModelOptions {
        match self {
            Variante::Standard => ModelOptions::FourStem,
            Variante::SixStems => ModelOptions::SixStem,
            // Les quatre sous-modèles, donc les quatre stems : demander moins
            // n'économiserait rien puisqu'il faut de toute façon les charger.
            Variante::Affinee => ModelOptions::FineTuned(metadata::HTDEMUCS_FT.stems.to_vec()),
        }
    }
}

/// Un stem séparé, prêt à être écrit.
pub struct Piste {
    pub nom: &'static str,
    pub gauche: Vec<f32>,
    pub droite: Vec<f32>,
}

/// Le démixeur, modèle chargé en mémoire.
pub struct Demixeur {
    modele: Demucs<Moteur>,
}

impl Demixeur {
    /// Charge les poids d'une variante.
    ///
    /// `dossier` vaut `None` dans le cas courant : les poids sont cherchés là
    /// où `rusty_music_core::modeles` les attend, ce qui couvre aussi bien le
    /// dépôt que l'application empaquetée. Un chemin explicite ne sert qu'à en
    /// désigner d'autres.
    pub fn charger(dossier: Option<&Path>, variante: Variante) -> Result<Self> {
        let poids = match dossier {
            Some(d) => d.join(variante.fichier()),
            None => rusty_music_core::modeles::trouver(variante.fichier()).unwrap_or_default(),
        };
        if !poids.is_file() {
            return Err(Error::PoidsAbsents(format!(
                "{}\n  ./scripts/preparer-demucs.sh {}",
                rusty_music_core::modeles::introuvable(variante.fichier()),
                variante.nom()
            )));
        }
        let octets = std::fs::read(&poids)?;
        let modele = Demucs::<Moteur>::from_bytes(variante.options(), &octets, Default::default())
            .map_err(|e| Error::Modele(e.to_string()))?;
        Ok(Self { modele })
    }

    /// Fait tourner le modèle une fois à vide.
    ///
    /// Sur GPU, la première inférence compile ses noyaux et fait tourner
    /// l'autotune : 4,5 s mesurées. Les payer ici, plutôt que sur le premier
    /// morceau, rend les mesures suivantes lisibles.
    pub fn chauffer(&self) {
        attendre(self.modele.warmup());
    }

    /// Sépare un morceau déjà décodé.
    pub fn separer(&self, audio: &Stereo) -> Result<Vec<Piste>> {
        let stems = attendre(self.modele.separate(&audio.gauche, &audio.droite, SR))
            .map_err(|e| Error::Modele(e.to_string()))?;
        Ok(stems
            .into_iter()
            .map(|s| Piste {
                nom: s.id.as_str(),
                gauche: s.left,
                droite: s.right,
            })
            .collect())
    }

    /// Sépare un fichier et écrit les stems dans `dossier`.
    ///
    /// Les fichiers produits portent le nom du morceau suivi du stem, pour
    /// qu'un dossier contenant plusieurs séparations reste lisible.
    pub fn separer_fichier(&self, entree: &Path, dossier: &Path) -> Result<Vec<PathBuf>> {
        let audio = decode::stereo(entree)?;
        tracing::info!(
            secondes = audio.duree(),
            "décodé, séparation sur {}",
            moteur()
        );
        let pistes = self.separer(&audio)?;

        std::fs::create_dir_all(dossier)?;
        let base = entree
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "morceau".into());

        let mut ecrits = Vec::with_capacity(pistes.len());
        for p in &pistes {
            let sortie = dossier.join(format!("{base} — {}.wav", p.nom));
            wav::ecrire(&sortie, &p.gauche, &p.droite, SR)?;
            ecrits.push(sortie);
        }
        Ok(ecrits)
    }
}

/// Rapport signal/distorsion, en décibels.
///
/// Sert au contrôle de bout en bout : la somme des stems doit reconstituer le
/// mélange. `demucs-core` exige 20 dB dans son propre test ; en dessous, la
/// séparation a perdu de la matière en route.
pub fn sdr(reference: &[f32], estimation: &[f32]) -> f64 {
    let signal: f64 = reference.iter().map(|x| (*x as f64).powi(2)).sum();
    let bruit: f64 = reference
        .iter()
        .zip(estimation)
        .map(|(a, b)| ((*a - *b) as f64).powi(2))
        .sum();
    if bruit <= f64::MIN_POSITIVE {
        return f64::INFINITY;
    }
    10.0 * (signal / bruit).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_sdr_recompense_la_ressemblance() {
        let r: Vec<f32> = (0..1000).map(|i| (i as f32 / 50.0).sin()).collect();
        assert!(sdr(&r, &r).is_infinite(), "identique = pas de distorsion");

        // Un bruit à 1 % doit donner environ 40 dB.
        let bruite: Vec<f32> = r
            .iter()
            .enumerate()
            .map(|(i, x)| x + 0.01 * (i as f32).sin())
            .collect();
        let d = sdr(&r, &bruite);
        assert!((30.0..50.0).contains(&d), "SDR inattendu : {d:.1} dB");

        // Un signal sans rapport doit tomber vers zéro, voire en dessous.
        let autre: Vec<f32> = (0..1000).map(|i| (i as f32 / 7.0).cos()).collect();
        assert!(sdr(&r, &autre) < 6.0);
    }

    #[test]
    fn attendre_execute_un_futur_immediat() {
        assert_eq!(attendre(async { 21 * 2 }), 42);
    }

    #[test]
    fn les_variantes_se_nomment_et_se_relisent() {
        for v in [Variante::Standard, Variante::SixStems, Variante::Affinee] {
            assert_eq!(
                Variante::analyser(v.nom()),
                Some(v),
                "aller-retour sur {v:?}"
            );
            assert!(v.fichier().ends_with(".safetensors"));
        }
        assert_eq!(Variante::default(), Variante::Standard);
        assert_eq!(Variante::analyser("htdemucs_v9"), None);
        // Six stems veut dire six, et l'affinée reste sur quatre.
        assert_eq!(Variante::SixStems.stems().len(), 6);
        assert_eq!(Variante::Standard.stems().len(), 4);
        assert!(Variante::Affinee.megaoctets() > Variante::Standard.megaoctets());
    }
}
