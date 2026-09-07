// SPDX-License-Identifier: GPL-3.0-or-later
//! CritiqueBrainz : le client, et rien d'autre.
//!
//! Comme [`crate::musicbrainz`], ce module parle au réseau et rend des
//! données ; il n'écrit pas en base. La passe qui l'emploie est dans
//! [`crate::critiques`].
//!
//! **Licence Creative Commons.** `docs/data-sources.md` affirmait qu'aucune
//! API libre n'existait pour les critiques d'albums — ce n'est plus exact.
//! CritiqueBrainz publie des critiques sous licence CC (BY-SA ou BY-NC-SA
//! selon la critique — jamais supposée uniforme, elle est stockée par ligne).
//! Une licence Creative Commons autorise explicitement à garder le texte
//! complet, à condition d'afficher l'attribution partout où il apparaît.
//!
//! **Aucune clé pour la lecture**, interrogation par identifiant MusicBrainz
//! de release-group — jamais de recherche approximative, nos morceaux le
//! portent déjà (`mb_release_groups`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};

/// Critiques par page. 50 est confortable ; peu d'albums en portent plus.
const PAR_PAGE: usize = 50;

/// Délai minimal entre deux requêtes — API publique, pas de limite annoncée,
/// mais la même politesse qu'envers les autres sources.
const CADENCE: Duration = Duration::from_millis(500);

/// Combien de fois réessayer avant d'abandonner un identifiant.
const ESSAIS: u32 = 4;

/// Une critique, telle que CritiqueBrainz la rend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Critique {
    pub id: String,
    pub auteur: Option<String>,
    pub licence_id: String,
    pub licence_nom: Option<String>,
    pub langue: Option<String>,
    pub texte: String,
    /// URL d'origine si la critique est hébergée ailleurs ; sinon la page
    /// CritiqueBrainz de la critique elle-même.
    pub url_originale: String,
}

/// Client CritiqueBrainz, cadencé.
pub struct Client {
    agent: ureq::Agent,
    dernier: Mutex<Option<Instant>>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!("rusty-music/{}", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self {
            agent,
            dernier: Mutex::new(None),
        }
    }

    fn cadencer(&self) {
        let mut dernier = self.dernier.lock().expect("horloge du débit");
        if let Some(precedent) = *dernier {
            let ecoule = precedent.elapsed();
            if ecoule < CADENCE {
                std::thread::sleep(CADENCE - ecoule);
            }
        }
        *dernier = Some(Instant::now());
    }

    fn json(&self, url: &str) -> Result<Option<Value>> {
        let mut derniere = String::new();
        for essai in 0..ESSAIS {
            self.cadencer();
            match self.agent.get(url).call() {
                Ok(mut r) => {
                    let corps = r
                        .body_mut()
                        .read_to_string()
                        .map_err(|e| Error::Reseau(format!("lecture du corps : {e}")))?;
                    return serde_json::from_str(&corps)
                        .map(Some)
                        .map_err(|e| Error::Reseau(format!("JSON illisible : {e}")));
                }
                Err(ureq::Error::StatusCode(404)) => return Ok(None),
                Err(e) => {
                    derniere = e.to_string();
                    std::thread::sleep(Duration::from_secs(1 << essai));
                }
            }
        }
        Err(Error::Reseau(format!(
            "{ESSAIS} tentatives sans succès sur {url} — {derniere}"
        )))
    }

    /// Toutes les critiques du release-group `mbid`. Une liste vide est une
    /// réponse valable : la plupart des albums n'en ont aucune.
    pub fn avis_pour_release_group(&self, mbid: &str) -> Result<Vec<Critique>> {
        let mut out = Vec::new();
        let mut offset = 0usize;
        loop {
            let url = format!(
                "https://critiquebrainz.org/ws/1/review/?entity_id={mbid}\
                 &entity_type=release_group&limit={PAR_PAGE}&offset={offset}"
            );
            let Some(v) = self.json(&url)? else { return Ok(out) };
            let page = v["reviews"].as_array().cloned().unwrap_or_default();
            let recus = page.len();
            out.extend(page.iter().filter_map(critique_de));
            let total = v["count"].as_u64().unwrap_or(0) as usize;
            offset += recus;
            if offset >= total || recus == 0 {
                return Ok(out);
            }
        }
    }
}

/// Extrait une critique d'un élément de la réponse `/ws/1/review/`.
///
/// `source_url` est absent pour une critique native de CritiqueBrainz : dans
/// ce cas l'URL d'origine est sa propre page de critique.
fn critique_de(r: &Value) -> Option<Critique> {
    let id = r["id"].as_str()?.to_string();
    let url_originale = r["source_url"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://critiquebrainz.org/review/{id}"));
    Some(Critique {
        id,
        auteur: r["user"]["display_name"].as_str().map(str::to_string),
        licence_id: r["license_id"].as_str().unwrap_or("?").to_string(),
        licence_nom: r["full_name"].as_str().map(str::to_string),
        langue: r["language"].as_str().map(str::to_string),
        texte: r["text"].as_str().unwrap_or_default().to_string(),
        url_originale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extrait d'une vraie réponse `/ws/1/review/`, gardé en dur — pas
    /// d'accès réseau dans les tests.
    #[test]
    fn critique_de_lit_lauteur_et_la_licence() {
        let v: Value = serde_json::from_str(
            r#"{"id":"2c2ed1da-22f2-4fe9-8b01-9a99227c1c15",
                "entity_id":"f5093c06-23e3-404f-aeaa-40f72885ee3a",
                "license_id":"CC BY-SA 3.0",
                "full_name":"Creative Commons Attribution-ShareAlike 3.0 Unported",
                "language":"en","text":"A very coherent whole...",
                "source_url":null,
                "user":{"display_name":"TudbuT"}}"#,
        )
        .expect("JSON de test");
        let c = critique_de(&v).expect("critique attendue");
        assert_eq!(c.auteur.as_deref(), Some("TudbuT"));
        assert_eq!(c.licence_id, "CC BY-SA 3.0");
        assert_eq!(c.url_originale, "https://critiquebrainz.org/review/2c2ed1da-22f2-4fe9-8b01-9a99227c1c15");
    }

    #[test]
    fn critique_de_garde_lurl_dorigine_quand_elle_existe() {
        let v: Value = serde_json::from_str(
            r#"{"id":"x","license_id":"CC BY-NC-SA 3.0","text":"...",
                "source_url":"https://exemple.org/critique"}"#,
        )
        .expect("JSON de test");
        let c = critique_de(&v).expect("critique attendue");
        assert_eq!(c.url_originale, "https://exemple.org/critique");
    }

    #[test]
    fn critique_de_sans_id_rend_rien() {
        let v: Value = serde_json::from_str(r#"{"text":"..."}"#).expect("JSON de test");
        assert!(critique_de(&v).is_none());
    }
}
