// SPDX-License-Identifier: GPL-3.0-or-later
//! TheAudioDB : le client, et rien d'autre.
//!
//! Comme [`crate::musicbrainz`] et [`crate::listenbrainz`], ce module parle au
//! réseau et rend des données ; il n'écrit pas en base. La passe qui l'emploie
//! est dans [`crate::biographies`].
//!
//! **Résolution exclusivement par MBID.** `artist-mb.php?i={mbid}` retrouve un
//! artiste directement par son identifiant MusicBrainz — vérifié en direct sur
//! plusieurs artistes, y compris de taille modeste. Comme `tracks.mb_artist_id`
//! est déjà rempli à l'ingestion, l'appariement approximatif par nom (risqué —
//! TheAudioDB porte des doublons documentés, plusieurs artistes identiques pour
//! un même nom) ne se pose jamais ici : un artiste sans MBID est ignoré, jamais
//! cherché par nom (`docs/enrichissement-lecteur.md`).
//!
//! **Aucune clé requise.** La clé de test partagée `123` suffit (30 requêtes
//! par minute) ; une clé personnelle, si l'utilisateur en a une, l'accélère.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};

/// Clé de test partagée, publique — documentée par TheAudioDB pour un usage
/// à faible volume.
const CLE_PARTAGEE: &str = "123";

/// Délai minimal entre deux requêtes. La clé partagée est limitée à 30/min
/// (documentation TheAudioDB) et partagée entre tous ses utilisateurs : plus
/// prudent qu'avec une clé personnelle.
const CADENCE: Duration = Duration::from_millis(2_100);

/// Combien de fois réessayer avant d'abandonner un identifiant.
const ESSAIS: u32 = 4;

/// Une biographie d'artiste, telle que TheAudioDB la rend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artiste {
    pub id_theaudiodb: String,
    pub biographie_en: Option<String>,
    pub biographie_fr: Option<String>,
}

/// Client TheAudioDB, cadencé.
pub struct Client {
    agent: ureq::Agent,
    cle: String,
    dernier: Mutex<Option<Instant>>,
}

impl Client {
    /// `cle` : une clé personnelle si l'utilisateur en a une, sinon la clé de
    /// test partagée.
    pub fn new(cle: Option<&str>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!("rusty-music/{}", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self {
            agent,
            cle: cle
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(CLE_PARTAGEE)
                .to_string(),
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

    /// L'artiste MusicBrainz `mbid`, s'il est connu de TheAudioDB. `None` est
    /// une réponse valable : beaucoup d'artistes, surtout confidentiels,
    /// n'y figurent pas.
    ///
    /// `strMusicBrainzID` de la réponse est vérifié contre `mbid` demandé —
    /// défense bon marché contre une réponse mal alignée.
    pub fn artiste_par_mbid(&self, mbid: &str) -> Result<Option<Artiste>> {
        let url = format!("https://www.theaudiodb.com/api/v1/json/{}/artist-mb.php?i={mbid}", self.cle);
        let Some(v) = self.json(&url)? else { return Ok(None) };
        Ok(artiste_de(&v, mbid))
    }
}

/// Extrait le premier artiste d'une réponse `artist-mb.php`, en vérifiant que
/// son `strMusicBrainzID` correspond bien au MBID demandé.
fn artiste_de(v: &Value, mbid_demande: &str) -> Option<Artiste> {
    let a = v["artists"].as_array()?.first()?;
    if a["strMusicBrainzID"].as_str() != Some(mbid_demande) {
        return None;
    }
    let texte = |champ: &str| a[champ].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    Some(Artiste {
        id_theaudiodb: a["idArtist"].as_str().unwrap_or_default().to_string(),
        biographie_en: texte("strBiography"),
        biographie_fr: texte("strBiographyFR"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extrait d'une vraie réponse `artist-mb.php` (Radiohead), gardé en dur —
    /// pas d'accès réseau dans les tests.
    #[test]
    fn artiste_de_lit_les_biographies_en_et_fr() {
        let v: Value = serde_json::from_str(
            r#"{"artists":[{
                "idArtist":"111418",
                "strArtist":"Radiohead",
                "strMusicBrainzID":"a74b1b7f-71a5-4011-9441-d0b5e4122711",
                "strBiography":"Radiohead are an English rock band...",
                "strBiographyFR":"Radiohead est un groupe de rock anglais...",
                "strBiographyDE":null
            }]}"#,
        )
        .expect("JSON de test");
        let a = artiste_de(&v, "a74b1b7f-71a5-4011-9441-d0b5e4122711").expect("artiste attendu");
        assert_eq!(a.id_theaudiodb, "111418");
        assert!(a.biographie_en.unwrap().starts_with("Radiohead are"));
        assert!(a.biographie_fr.unwrap().starts_with("Radiohead est"));
    }

    #[test]
    fn artiste_de_rejette_un_mbid_qui_ne_correspond_pas() {
        let v: Value = serde_json::from_str(
            r#"{"artists":[{"idArtist":"1","strMusicBrainzID":"autre-mbid"}]}"#,
        )
        .expect("JSON de test");
        assert!(artiste_de(&v, "mbid-demande").is_none());
    }

    #[test]
    fn artiste_de_sans_resultat_rend_rien() {
        let v: Value = serde_json::from_str(r#"{"artists":null}"#).expect("JSON de test");
        assert!(artiste_de(&v, "x").is_none());
    }
}
