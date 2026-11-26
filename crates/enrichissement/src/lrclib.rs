// SPDX-License-Identifier: GPL-3.0-or-later
//! LRCLIB (<https://lrclib.net>) : paroles, souvent datées ligne par ligne
//! (format LRC), pour la partition de voix de Pratiquer. Service libre, sans
//! clé ; appariement par artiste, titre, album et durée.
//!
//! Décision du 26 nov. 2026 (la piste notée le 4 oct., `plan-ecouter-v0.2.md`
//! § 9) : demandées seulement quand on calcule une partition de voix, mises
//! en cache à côté des stems. Le texte reste celui des ayants droit : on
//! l'affiche pour soi, on ne le redistribue pas.

use std::time::Duration;

use rusty_music_core::error::Result;
use rusty_music_core::http::ClientCadence;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Une requête par seconde au plus : un usage à la main, pas une passe.
const CADENCE: Duration = Duration::from_secs(1);
/// Écart de durée toléré entre le fichier et l'enregistrement de LRCLIB.
const ECART_DUREE_S: f64 = 4.0;

/// Ce que LRCLIB sait d'un morceau.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paroles {
    /// Les paroles datées (LRC), si LRCLIB les a.
    pub datees: Option<String>,
    /// Le texte seul.
    pub texte: Option<String>,
    /// Morceau instrumental selon LRCLIB.
    pub instrumental: bool,
}

pub struct Client {
    http: ClientCadence,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!(
                "rusty-music/{} (https://github.com/rousseau/rusty-music)",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self {
            http: ClientCadence::new(agent, CADENCE),
        }
    }

    /// Les paroles d'un morceau : la correspondance exacte (`/api/get`),
    /// sinon la recherche (`/api/search`), dont on garde l'enregistrement de
    /// durée la plus proche, daté d'abord. `None` : LRCLIB ne connaît pas le
    /// morceau.
    pub fn chercher(
        &self,
        artiste: &str,
        titre: &str,
        album: Option<&str>,
        duree_s: Option<f64>,
    ) -> Result<Option<Paroles>> {
        let duree = duree_s.map(|d| format!("{}", d.round() as i64));
        let mut q: Vec<(&str, &str)> = vec![("artist_name", artiste), ("track_name", titre)];
        if let Some(a) = album {
            q.push(("album_name", a));
        }
        if let Some(d) = &duree {
            q.push(("duration", d));
        }
        if let Some(v) =
            self.http
                .get_json_requete("https://lrclib.net/api/get", &q, "lrclib get")?
        {
            if let Some(p) = paroles_de(&v) {
                return Ok(Some(p));
            }
        }
        let v = self.http.get_json_requete(
            "https://lrclib.net/api/search",
            &[("artist_name", artiste), ("track_name", titre)],
            "lrclib search",
        )?;
        Ok(v.as_ref().and_then(|v| meilleur(v, duree_s)))
    }
}

fn paroles_de(v: &Value) -> Option<Paroles> {
    let texte = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let p = Paroles {
        datees: texte("syncedLyrics"),
        texte: texte("plainLyrics"),
        instrumental: v.get("instrumental").and_then(Value::as_bool) == Some(true),
    };
    (p.datees.is_some() || p.texte.is_some() || p.instrumental).then_some(p)
}

/// L'enregistrement d'une recherche qui va le mieux : durée dans
/// [`ECART_DUREE_S`] si on la connaît, daté avant non daté, puis le plus
/// proche en durée.
fn meilleur(v: &Value, duree_s: Option<f64>) -> Option<Paroles> {
    v.as_array()?
        .iter()
        .filter_map(|e| {
            let p = paroles_de(e)?;
            let ecart = match (duree_s, e.get("duration").and_then(Value::as_f64)) {
                (Some(a), Some(b)) => (a - b).abs(),
                _ => 0.0,
            };
            (ecart <= ECART_DUREE_S).then_some((p, ecart))
        })
        .min_by(|a, b| {
            b.0.datees
                .is_some()
                .cmp(&a.0.datees.is_some())
                .then(a.1.total_cmp(&b.1))
        })
        .map(|(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn une_reponse_se_lit() {
        let v =
            json!({"syncedLyrics": "[00:01.00] Hey", "plainLyrics": "Hey", "instrumental": false});
        let p = paroles_de(&v).unwrap();
        assert_eq!(p.datees.as_deref(), Some("[00:01.00] Hey"));
        assert!(paroles_de(&json!({"syncedLyrics": null, "plainLyrics": " "})).is_none());
    }

    #[test]
    fn la_recherche_prefere_le_date_de_bonne_duree() {
        let v = json!([
            {"duration": 255.0, "plainLyrics": "a", "syncedLyrics": null},
            {"duration": 300.0, "plainLyrics": "b", "syncedLyrics": "[00:01.00] b"},
            {"duration": 256.0, "plainLyrics": "c", "syncedLyrics": "[00:01.00] c"},
        ]);
        let p = meilleur(&v, Some(255.0)).unwrap();
        assert_eq!(p.texte.as_deref(), Some("c"));
    }
}
