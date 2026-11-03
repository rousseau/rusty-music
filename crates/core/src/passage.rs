// SPDX-License-Identifier: GPL-3.0-or-later
//! Passage de la lecture du Mac à l'iPhone, par fichier (option A de
//! `docs/plan-ios-compagnon.md`, « Passage de la lecture Mac ↔ iPhone »).
//!
//! Le Mac dépose [`FICHIER`] dans les Documents de l'app compagnon, avec
//! `devicectl`, puis se met en pause ; l'app le lit, rouvre la valise qui
//! porte le plus de morceaux de la file et la reprend **en pause** à la
//! position. Idée et règles reprises du `handoff.ts` de Disc Player
//! (<https://github.com/eudj1n/snowsky-disc-player>, licence MIT, © eudj1n) :
//! jamais deux appareils qui jouent à la fois, jamais un morceau perdu sans
//! le dire.
//!
//! La file y est en **chemins d'origine** : la base d'une valise garde ceux du
//! Mac dans `tracks.path` (voir [`crate::valise`]), le téléphone n'a donc rien
//! à traduire. L'iPhone ne porte qu'une valise : la file se réduit à ce
//! qu'elle contient, et [`Reduction::absents`] dit combien en manquent.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::session::retenir;

/// Nom du fichier déposé à la racine des Documents de l'app.
pub const FICHIER: &str = "session-passage.json";

/// Version du format. Un téléphone plus ancien refuse un passage plus récent
/// plutôt que d'en lire la moitié.
pub const VERSION: u32 = 1;

/// Ce que le Mac jouait au moment du passage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Passage {
    pub version: u32,
    /// Instant du dépôt (millisecondes Unix) : identifie le passage, que
    /// l'iPhone accuse par le Wi-Fi ([`crate::appairage::Recu`]).
    pub emis_le: i64,
    /// File dans l'ordre courant, en chemins d'origine.
    pub file: Vec<String>,
    /// Ordre d'avant l'aléatoire, vide s'il n'a pas servi.
    #[serde(default)]
    pub avant_melange: Vec<String>,
    /// Rang de la piste en cours dans `file`.
    pub rang: usize,
    pub position_ms: u64,
    #[serde(default)]
    pub alea: bool,
    /// « aucune », « toutes » ou « une ».
    #[serde(default = "aucune")]
    pub repetition: String,
    #[serde(default = "un")]
    pub volume: f32,
}

fn aucune() -> String {
    "aucune".into()
}

fn un() -> f32 {
    1.0
}

/// La file du passage réduite à ce qu'une valise contient.
#[derive(Debug, Clone, PartialEq)]
pub struct Reduction {
    pub file: Vec<String>,
    pub avant_melange: Vec<String>,
    /// Rang recalé sur la file réduite (voir [`retenir`]).
    pub rang: usize,
    /// Zéro quand la piste en cours n'est pas dans la valise : on part alors
    /// du début de la suivante.
    pub position_ms: u64,
    /// Morceaux de la file absents de la valise.
    pub absents: usize,
    pub courante_presente: bool,
}

impl Passage {
    pub fn lire(chemin: &Path) -> Result<Self> {
        Self::depuis_json(&std::fs::read(chemin)?)
    }

    /// Le même contrôle que [`Self::lire`], pour un passage reçu par le Wi-Fi
    /// ([`crate::appairage`]).
    pub fn depuis_json(octets: &[u8]) -> Result<Self> {
        let p: Self = serde_json::from_slice(octets).map_err(|e| Error::Parsing(e.to_string()))?;
        if p.version > VERSION {
            return Err(Error::Parsing(format!(
                "passage au format {} (cet appareil lit le format {VERSION}) : mettre l'app à jour",
                p.version
            )));
        }
        Ok(p)
    }

    pub fn ecrire(&self, chemin: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| Error::Parsing(e.to_string()))?;
        std::fs::write(chemin, json)?;
        Ok(())
    }

    /// Combien de morceaux de la file `connu` reconnaît : sert à choisir la
    /// valise à rouvrir.
    pub fn presents(&self, mut connu: impl FnMut(&str) -> bool) -> usize {
        self.file.iter().filter(|c| connu(c)).count()
    }

    /// La file réduite aux morceaux que `connu` reconnaît.
    pub fn reduire(&self, connu: impl Fn(&str) -> bool) -> Reduction {
        let marques = self
            .file
            .iter()
            .map(|c| connu(c).then(|| c.clone()))
            .collect::<Vec<_>>();
        let absents = marques.iter().filter(|m| m.is_none()).count();
        let (file, rang, courante_presente) = retenir(marques, self.rang);
        Reduction {
            avant_melange: self
                .avant_melange
                .iter()
                .filter(|c| connu(c))
                .cloned()
                .collect(),
            file,
            rang,
            position_ms: if courante_presente {
                self.position_ms
            } else {
                0
            },
            absents,
            courante_presente,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passage() -> Passage {
        Passage {
            version: VERSION,
            emis_le: 1_793_600_000,
            file: ["/m/a.mp3", "/m/b.mp3", "/m/c.mp3", "/m/d.mp3"]
                .map(String::from)
                .to_vec(),
            avant_melange: Vec::new(),
            rang: 2,
            position_ms: 83_000,
            alea: false,
            repetition: "aucune".into(),
            volume: 0.8,
        }
    }

    #[test]
    fn la_file_se_reduit_a_la_valise_et_compte_les_absents() {
        // b manque : c, en cours, passe au rang 1 et garde sa position.
        let r = passage().reduire(|c| c != "/m/b.mp3");
        assert_eq!(r.file, ["/m/a.mp3", "/m/c.mp3", "/m/d.mp3"]);
        assert_eq!((r.rang, r.position_ms, r.absents), (1, 83_000, 1));
        assert!(r.courante_presente);
    }

    #[test]
    fn sans_la_piste_en_cours_on_part_du_debut_de_la_suivante() {
        let r = passage().reduire(|c| c != "/m/c.mp3");
        assert_eq!(r.file, ["/m/a.mp3", "/m/b.mp3", "/m/d.mp3"]);
        assert_eq!((r.rang, r.position_ms), (2, 0));
        assert!(!r.courante_presente);
    }

    #[test]
    fn la_valise_qui_porte_le_plus_de_morceaux_se_compte() {
        let p = passage();
        assert_eq!(p.presents(|c| c.ends_with("a.mp3")), 1);
        assert_eq!(p.presents(|_| true), 4);
        assert_eq!(p.presents(|_| false), 0);
    }

    #[test]
    fn le_fichier_fait_l_aller_retour() {
        let dossier = std::env::temp_dir().join(format!("rusty-passage-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join(FICHIER);
        passage().ecrire(&chemin).unwrap();
        assert_eq!(Passage::lire(&chemin).unwrap(), passage());

        // Un format plus récent est refusé, pas lu à moitié.
        let mut futur = passage();
        futur.version = VERSION + 1;
        futur.ecrire(&chemin).unwrap();
        assert!(Passage::lire(&chemin).is_err());
        std::fs::remove_dir_all(&dossier).unwrap();
    }

    #[test]
    fn les_champs_facultatifs_ont_des_valeurs_sures() {
        let p: Passage = serde_json::from_str(
            r#"{"version":1,"emis_le":0,"file":["/m/a.mp3"],"rang":0,"position_ms":5}"#,
        )
        .unwrap();
        assert_eq!(p.repetition, "aucune");
        assert!(!p.alea && p.avant_melange.is_empty());
        assert!((p.volume - 1.0).abs() < f32::EPSILON);
    }
}
