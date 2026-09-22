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

use std::time::Duration;

use serde_json::Value;

use crate::error::Result;
use crate::http::ClientCadence;

/// Clé de test partagée, publique — documentée par TheAudioDB pour un usage
/// à faible volume.
const CLE_PARTAGEE: &str = "123";

/// Délai minimal entre deux requêtes. La clé partagée est limitée à 30/min
/// (documentation TheAudioDB) et partagée entre tous ses utilisateurs : plus
/// prudent qu'avec une clé personnelle.
const CADENCE: Duration = Duration::from_millis(2_100);

/// Une biographie d'artiste, telle que TheAudioDB la rend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artiste {
    pub id_theaudiodb: String,
    pub biographie_en: Option<String>,
    pub biographie_fr: Option<String>,
}

/// Client TheAudioDB, cadencé.
pub struct Client {
    http: ClientCadence,
    cle: String,
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
            http: ClientCadence::new(agent, CADENCE),
            cle: cle
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(CLE_PARTAGEE)
                .to_string(),
        }
    }

    /// `ctx` ne porte jamais `self.cle` : contrairement à `url`, qui la
    /// contient (`.../json/{cle}/artist-mb.php`), un message d'erreur ne doit
    /// jamais la répéter — voir `crate::http::ClientCadence`.
    fn json(&self, url: &str) -> Result<Option<Value>> {
        self.http.get_json(url, "artist-mb.php")
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

    /// La vignette d'artiste (`strArtistThumb`, JPEG) de `mbid`, si TheAudioDB
    /// connaît l'artiste et en a une. Même résolution par MBID, même
    /// vérification de l'identifiant que [`Client::artiste_par_mbid`] : jamais
    /// la photo d'un homonyme.
    ///
    /// `Ok(None)` : pas de photo (réponse définitive). `Err` : panne réseau ou
    /// image illisible — à ne pas mettre en cache.
    pub fn vignette_par_mbid(&self, mbid: &str) -> Result<Option<Vec<u8>>> {
        let url = format!("https://www.theaudiodb.com/api/v1/json/{}/artist-mb.php?i={mbid}", self.cle);
        let Some(v) = self.json(&url)? else { return Ok(None) };
        match vignette_de(&v, mbid) {
            Some(image) => crate::pochette::telecharger(&image),
            None => Ok(None),
        }
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

/// L'URL de `strArtistThumb` d'une réponse `artist-mb.php`, sous la même
/// condition que [`artiste_de`] : le `strMusicBrainzID` doit être celui demandé.
fn vignette_de(v: &Value, mbid_demande: &str) -> Option<String> {
    let a = v["artists"].as_array()?.first()?;
    if a["strMusicBrainzID"].as_str() != Some(mbid_demande) {
        return None;
    }
    a["strArtistThumb"].as_str().filter(|s| !s.is_empty()).map(str::to_string)
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

    #[test]
    fn vignette_de_lit_la_vignette_et_verifie_le_mbid() {
        let v: Value = serde_json::from_str(
            r#"{"artists":[{
                "strMusicBrainzID":"a74b1b7f-71a5-4011-9441-d0b5e4122711",
                "strArtistThumb":"https://r2.theaudiodb.com/images/media/artist/thumb/x.jpg"
            }]}"#,
        )
        .expect("JSON de test");
        assert_eq!(
            vignette_de(&v, "a74b1b7f-71a5-4011-9441-d0b5e4122711").as_deref(),
            Some("https://r2.theaudiodb.com/images/media/artist/thumb/x.jpg")
        );
        assert_eq!(vignette_de(&v, "autre-mbid"), None);
    }

    #[test]
    fn vignette_de_sans_image_ou_sans_resultat_rend_rien() {
        let vide: Value = serde_json::from_str(
            r#"{"artists":[{"strMusicBrainzID":"m","strArtistThumb":""}]}"#,
        )
        .expect("JSON de test");
        assert_eq!(vignette_de(&vide, "m"), None);
        let nul: Value = serde_json::from_str(r#"{"artists":null}"#).expect("JSON de test");
        assert_eq!(vignette_de(&nul, "m"), None);
    }
}
