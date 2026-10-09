// SPDX-License-Identifier: GPL-3.0-or-later
//! Pulsation d'un morceau : ses temps et ses **premiers temps de mesure**.
//!
//! C'est l'ossature que partagent les deux usages du mode Éditer
//! (`docs/plan-editer-pratique-creation.md`, chantier 0.2) : boucler « ces
//! quatre mesures », poser une partition sur des barres, greffer un stem mesure
//! par mesure. `analysis::battements` ne donne qu'une grille à tempo constant,
//! sans mesure ; ici chaque temps est daté, et le tempo peut dériver.
//!
//! Le réseau est « Beat This! » (Foscarin, Schlüter, Widmer — CPJKU, ISMIR
//! 2024 ; code et poids MIT), par son portage Rust `beat-this` (danigb, MIT)
//! sur le runtime `rten`, pur Rust. Le post-traitement est le sien : un
//! simple choix de pics, sans DBN — les premiers temps sont recalés sur le
//! temps le plus proche.
//!
//! **Tout est en secondes du fichier d'origine.** L'étirement et les boucles
//! ne changent que la position lue ; la pulsation, elle, ne se recalcule pas.

use std::path::{Path, PathBuf};

use beat_this::{BeatThis, Runtime, RtenRuntime};
use serde::{Deserialize, Serialize};

use crate::{decode, Error, Result};

/// Version du format et du calcul. Une pulsation en cache d'une autre version
/// est recalculée.
pub const VERSION: u32 = 1;

/// Nom du fichier de cache, rangé à côté des stems du morceau.
pub const FICHIER_CACHE: &str = "pulsation.json";

/// Un fichier de poids : où le trouver, combien il pèse, son empreinte.
#[derive(Debug, Clone, Copy)]
pub struct Poids {
    pub nom: &'static str,
    pub url: &'static str,
    pub octets: u64,
    pub sha256: &'static str,
}

// Les deux petits modèles viennent de `danigb/beat-this-rs` à la révision
// 1ae768e — épinglée dans l'URL, pour qu'un dépôt qui bouge ne change pas ce
// qu'on télécharge ; le grand, de sa release `model-large`.

/// Le front-end log-mel, commun aux deux tailles.
pub const MEL: Poids = Poids {
    nom: "beat_this_mel.onnx",
    url: "https://raw.githubusercontent.com/danigb/beat-this-rs/1ae768e78f1ad83b0ed3886241dc29ffde853c40/models/mel_spectrogram.onnx",
    octets: 270_742,
    sha256: "fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9",
};

/// Les deux tailles du réseau publiées par CPJKU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modele {
    /// `final0`, ~20 M paramètres : F1 89,1 (temps) / 78,3 (premiers temps)
    /// sur GTZAN.
    Complet,
    /// `small1`, ~2 M paramètres : 88,8 / 77,2 — presque autant, dix fois
    /// plus léger.
    Petit,
}

impl Modele {
    pub fn poids(self) -> Poids {
        match self {
            Modele::Complet => Poids {
                nom: "beat_this.onnx",
                url: "https://github.com/danigb/beat-this-rs/releases/download/model-large/beat_this.onnx",
                octets: 83_162_650,
                sha256: "5f810debe53459b559127fb55bbad40035bb47cc567b20e501670f968c770f02",
            },
            Modele::Petit => Poids {
                nom: "beat_this_small.onnx",
                url: "https://raw.githubusercontent.com/danigb/beat-this-rs/1ae768e78f1ad83b0ed3886241dc29ffde853c40/models/beat_this_small.onnx",
                octets: 10_555_592,
                sha256: "a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f",
            },
        }
    }

    /// Les deux fichiers nécessaires sont-ils sur cette machine ?
    pub fn present(self) -> bool {
        [MEL, self.poids()]
            .iter()
            .all(|p| rusty_music_core::modeles::trouver(p.nom).is_some())
    }

    /// Télécharge ce qui manque, empreintes vérifiées.
    /// `avancer(octets_reçus, octets_attendus)` est rappelé à chaque lot.
    pub fn telecharger(self, mut avancer: impl FnMut(u64, Option<u64>)) -> Result<()> {
        for p in [MEL, self.poids()] {
            if rusty_music_core::modeles::trouver(p.nom).is_some() {
                continue;
            }
            rusty_music_core::modeles::telecharger(p.nom, p.url, p.octets, Some(p.sha256), &mut avancer)
                .map_err(|e| Error::Telechargement(e.to_string()))?;
        }
        Ok(())
    }
}

/// Les temps d'un morceau.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pulsation {
    pub version: u32,
    pub modele: Modele,
    /// Chaque temps, en secondes, croissants.
    pub temps: Vec<f32>,
    /// Les premiers temps de mesure, en secondes — chacun est aussi dans
    /// `temps`.
    pub premiers_temps: Vec<f32>,
}

/// Où tombe un instant dans la métrique.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Position {
    /// Indice de la mesure, 0 pour celle qui commence au premier premier
    /// temps ; −1 pour l'anacrouse, ce qui précède.
    pub mesure: i32,
    /// Temps écoulés depuis le début de la mesure, fraction comprise : 0,0
    /// sur le premier temps, 2,5 à mi-chemin du troisième et du quatrième.
    pub temps: f32,
}

impl Pulsation {
    /// Tempo médian, en BPM — la médiane plutôt que la moyenne, pour qu'un
    /// temps manqué ou un point d'orgue ne tire pas tout.
    pub fn bpm(&self) -> Option<f32> {
        let mut ecarts: Vec<f32> = self.temps.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).collect();
        if ecarts.is_empty() {
            return None;
        }
        ecarts.sort_by(f32::total_cmp);
        Some(60.0 / ecarts[ecarts.len() / 2])
    }

    /// Indices, dans `temps`, des premiers temps de mesure.
    fn indices_premiers(&self) -> Vec<usize> {
        self.premiers_temps
            .iter()
            .filter_map(|&p| {
                let i = self.temps.partition_point(|&t| t < p);
                // Le temps le plus proche : `p` y a été recalé par le réseau,
                // mais à l'arrondi près des flottants.
                [i.checked_sub(1), Some(i)]
                    .into_iter()
                    .flatten()
                    .filter(|&k| k < self.temps.len())
                    .min_by(|&a, &b| (self.temps[a] - p).abs().total_cmp(&(self.temps[b] - p).abs()))
            })
            .collect()
    }

    /// Temps par mesure : le nombre le plus fréquent entre deux premiers
    /// temps. `None` s'il y a moins de deux premiers temps.
    pub fn metrique(&self) -> Option<u32> {
        let idx = self.indices_premiers();
        let mut compte = [0usize; 17];
        for w in idx.windows(2) {
            let n = w[1].saturating_sub(w[0]);
            if (1..compte.len()).contains(&n) {
                compte[n] += 1;
            }
        }
        let (n, c) = compte.iter().enumerate().max_by_key(|&(_, c)| *c)?;
        (*c > 0).then_some(n as u32)
    }

    /// Indice fractionnaire de temps à l'instant `t` : 3,5 = à mi-chemin du
    /// quatrième et du cinquième. Hors des temps détectés, on prolonge avec
    /// l'écart le plus proche.
    pub fn indice_temps(&self, t: f32) -> Option<f32> {
        let n = self.temps.len();
        if n < 2 {
            return None;
        }
        let i = self.temps.partition_point(|&x| x <= t);
        let k = i.saturating_sub(1).min(n - 2);
        let (a, b) = (self.temps[k], self.temps[k + 1]);
        Some(k as f32 + (t - a) / (b - a))
    }

    /// Instant d'un indice fractionnaire de temps — l'inverse de
    /// [`indice_temps`](Self::indice_temps).
    pub fn instant(&self, indice: f32) -> Option<f32> {
        let n = self.temps.len();
        if n < 2 {
            return None;
        }
        let k = (indice.floor().max(0.0) as usize).min(n - 2);
        let (a, b) = (self.temps[k], self.temps[k + 1]);
        Some(a + (indice - k as f32) * (b - a))
    }

    /// Mesure et temps à l'instant `t`.
    pub fn position(&self, t: f32) -> Option<Position> {
        let x = self.indice_temps(t)?;
        let idx = self.indices_premiers();
        let premier = *idx.first()? as f32;
        // Dernier premier temps à ou avant x.
        let m = idx.partition_point(|&i| i as f32 <= x + 1e-4);
        if m == 0 {
            // Anacrouse : compter à rebours depuis la première mesure, avec la
            // métrique du morceau.
            let metrique = self.metrique().unwrap_or(4) as f32;
            return Some(Position { mesure: -1, temps: (x - premier).rem_euclid(metrique) });
        }
        Some(Position { mesure: m as i32 - 1, temps: x - idx[m - 1] as f32 })
    }

    /// Début et fin de la mesure qui contient `t`, en secondes. La dernière
    /// mesure finit sur le dernier temps détecté prolongé d'un temps.
    pub fn mesure_autour(&self, t: f32) -> Option<(f32, f32)> {
        let p = &self.premiers_temps;
        let m = p.partition_point(|&x| x <= t);
        if m == 0 {
            return None;
        }
        let debut = p[m - 1];
        let fin = match p.get(m) {
            Some(&f) => f,
            None => {
                let dernier = *self.temps.last()?;
                let ecart = 60.0 / self.bpm()?;
                dernier + ecart
            }
        };
        Some((debut, fin))
    }

    /// Le premier temps de mesure le plus proche de `t`.
    pub fn aimanter(&self, t: f32) -> Option<f32> {
        let p = &self.premiers_temps;
        let i = p.partition_point(|&x| x < t);
        [i.checked_sub(1), Some(i)]
            .into_iter()
            .flatten()
            .filter_map(|k| p.get(k).copied())
            .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
    }

    /// Lit une pulsation en cache ; `None` si absente, illisible ou d'une
    /// autre version ou d'un autre modèle.
    pub fn lire(chemin: &Path, modele: Modele) -> Option<Pulsation> {
        let texte = std::fs::read_to_string(chemin).ok()?;
        let p: Pulsation = serde_json::from_str(&texte).ok()?;
        (p.version == VERSION && p.modele == modele).then_some(p)
    }

    /// Écrit la pulsation en cache, par fichier temporaire renommé.
    pub fn ecrire(&self, chemin: &Path) -> std::io::Result<()> {
        let tmp = chemin.with_extension("json.partiel");
        std::fs::write(&tmp, serde_json::to_vec(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, chemin)
    }
}

/// Le réseau chargé, prêt à servir plusieurs morceaux.
pub struct Pisteur {
    modele: Modele,
    reseau: BeatThis<<RtenRuntime as Runtime>::Model>,
}

fn chemin_poids(p: Poids) -> Result<PathBuf> {
    rusty_music_core::modeles::trouver(p.nom).ok_or_else(|| {
        Error::PoidsAbsents(format!(
            "{}\n  ./scripts/preparer-beat-this.sh",
            rusty_music_core::modeles::introuvable(p.nom)
        ))
    })
}

impl Pisteur {
    /// Charge les deux réseaux (front-end mel et pisteur).
    pub fn charger(modele: Modele) -> Result<Self> {
        let mel = chemin_poids(MEL)?;
        let reseau = chemin_poids(modele.poids())?;
        let reseau = BeatThis::new(&RtenRuntime, &mel, &reseau).map_err(|e| Error::Modele(e.to_string()))?;
        Ok(Self { modele, reseau })
    }

    pub fn modele(&self) -> Modele {
        self.modele
    }

    /// Analyse un signal mono.
    pub fn analyser(&mut self, mono: Vec<f32>, frequence: u32) -> Result<Pulsation> {
        let a = self
            .reseau
            .analyze_owned(mono, frequence)
            .map_err(|e| Error::Modele(e.to_string()))?;
        Ok(Pulsation { version: VERSION, modele: self.modele, temps: a.beats, premiers_temps: a.downbeats })
    }

    /// Décode un fichier entier et l'analyse. Le mélange des deux canaux :
    /// c'est ce sur quoi le réseau a été entraîné.
    pub fn analyser_fichier(&mut self, chemin: &Path) -> Result<Pulsation> {
        let s = decode::stereo(chemin)?;
        let mono: Vec<f32> = s.gauche.iter().zip(&s.droite).map(|(g, d)| 0.5 * (g + d)).collect();
        drop(s);
        self.analyser(mono, decode::SR)
    }

    /// La pulsation en cache dans `cache` si elle y est et vaut pour ce
    /// modèle ; sinon l'analyse de `source`, écrite en cache. Une écriture
    /// ratée n'empêche pas de rendre le résultat.
    pub fn analyser_avec_cache(&mut self, source: &Path, cache: &Path) -> Result<Pulsation> {
        if let Some(p) = Pulsation::lire(cache, self.modele) {
            return Ok(p);
        }
        let p = self.analyser_fichier(source)?;
        if let Err(e) = p.ecrire(cache) {
            tracing::warn!("pulsation : cache non écrit ({}) : {e}", cache.display());
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4/4 à 120 BPM, une anacrouse d'un temps : temps à 0,5 s d'écart à
    /// partir de 0,5 s, premiers temps à partir de 1,0 s.
    fn quatre_quatre() -> Pulsation {
        let temps: Vec<f32> = (1..=17).map(|k| k as f32 * 0.5).collect();
        let premiers_temps = vec![1.0, 3.0, 5.0, 7.0];
        Pulsation { version: VERSION, modele: Modele::Petit, temps, premiers_temps }
    }

    #[test]
    fn tempo_et_metrique() {
        let p = quatre_quatre();
        assert!((p.bpm().unwrap() - 120.0).abs() < 1e-3);
        assert_eq!(p.metrique(), Some(4));
    }

    #[test]
    fn position_dans_la_mesure() {
        let p = quatre_quatre();
        assert_eq!(p.position(1.0), Some(Position { mesure: 0, temps: 0.0 }));
        let q = p.position(4.25).unwrap();
        assert_eq!(q.mesure, 1);
        assert!((q.temps - 2.5).abs() < 1e-4);
        // L'anacrouse : un temps avant la première mesure, donc le quatrième.
        let a = p.position(0.5).unwrap();
        assert_eq!(a.mesure, -1);
        assert!((a.temps - 3.0).abs() < 1e-4);
    }

    #[test]
    fn indice_et_instant_sont_inverses() {
        let p = quatre_quatre();
        for t in [0.2_f32, 0.75, 3.3, 8.4, 9.0] {
            let i = p.indice_temps(t).unwrap();
            assert!((p.instant(i).unwrap() - t).abs() < 1e-4, "{t}");
        }
    }

    #[test]
    fn tempo_qui_derive() {
        // Les temps s'écartent : 0,5 s puis 0,6 s. La position suit les temps,
        // pas un tempo moyen.
        let temps = vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.6, 3.2, 3.8, 4.4];
        let p = Pulsation { version: VERSION, modele: Modele::Petit, temps, premiers_temps: vec![0.0, 2.0, 4.4] };
        let q = p.position(2.9).unwrap();
        assert_eq!(q.mesure, 1);
        assert!((q.temps - 1.5).abs() < 1e-4);
    }

    #[test]
    fn mesure_et_aimant() {
        let p = quatre_quatre();
        assert_eq!(p.mesure_autour(3.4), Some((3.0, 5.0)));
        assert_eq!(p.mesure_autour(0.7), None);
        // La dernière mesure finit un temps après le dernier temps.
        assert_eq!(p.mesure_autour(7.5), Some((7.0, 9.0)));
        assert_eq!(p.aimanter(3.9), Some(3.0));
        assert_eq!(p.aimanter(4.1), Some(5.0));
    }

    #[test]
    fn cache_de_version() {
        let dossier = std::env::temp_dir().join(format!("pulsation-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join(FICHIER_CACHE);
        let p = quatre_quatre();
        p.ecrire(&chemin).unwrap();
        assert_eq!(Pulsation::lire(&chemin, Modele::Petit), Some(p));
        assert_eq!(Pulsation::lire(&chemin, Modele::Complet), None);
        std::fs::remove_dir_all(dossier).unwrap();
    }
}
