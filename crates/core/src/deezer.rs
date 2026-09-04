// SPDX-License-Identifier: GPL-3.0-or-later
//! Deezer : le client, et rien d'autre.
//!
//! Comme [`crate::musicbrainz`] et [`crate::listenbrainz`], ce module parle au
//! réseau et rend des données ; il n'écrit pas en base. La passe qui l'emploie
//! est dans [`crate::popularite`].
//!
//! **API publique, sans compte ni clé.** Deezer sert le second signal de
//! popularité du chantier `docs/popularite.md` : le `rank` d'une piste, entre
//! ~10 000 et ~1 000 000.
//!
//! **Deezer n'indexe pas par MBID.** On retrouve une piste par recherche
//! `artist:"…" track:"…"`, et on ne retient un résultat que si **l'artiste et
//! le titre** concordent tous les deux. Sans cette double vérification, la
//! sonde de phase 0 a mesuré ~1 rapprochement sur 40 tombant sur un autre
//! morceau du même artiste.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::musicbrainz::{cle_artiste, normaliser_titre};

/// Délai minimal entre deux requêtes. Deezer limite à ~50 requêtes par tranche
/// de 5 secondes ; 150 ms tient largement sous la barre.
const CADENCE: Duration = Duration::from_millis(150);

/// Combien de fois réessayer avant d'abandonner une recherche.
const ESSAIS: u32 = 4;

/// Client Deezer, cadencé. Un seul pour tout le processus.
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

    /// Une recherche GET rendant du JSON, réessayée sur échec temporaire.
    fn chercher(&self, kind: &str, q: &str) -> Result<Option<Value>> {
        let url = format!("https://api.deezer.com/search/{kind}");
        let mut derniere = String::new();
        for essai in 0..ESSAIS {
            self.cadencer();
            match self
                .agent
                .get(&url)
                .query("q", q)
                .query("limit", "5")
                .call()
            {
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

    /// Le `rank` Deezer de la piste `titre` de `artiste`, si la recherche rend
    /// un résultat dont **l'artiste et le titre** concordent avec la demande.
    /// `None` si rien ne concorde — l'appelant marque quand même l'entité
    /// comme « demandée », pour ne pas y revenir.
    pub fn rang_piste(&self, artiste: &str, titre: &str) -> Result<Option<i64>> {
        let q = format!(
            "artist:\"{}\" track:\"{}\"",
            echapper(artiste),
            echapper(titre)
        );
        let Some(v) = self.chercher("track", &q)? else {
            return Ok(None);
        };
        let art_attendu = cle_artiste(artiste);
        let tit_attendu = normaliser_titre(titre);
        for d in v["data"].as_array().into_iter().flatten() {
            let art = cle_artiste(d["artist"]["name"].as_str().unwrap_or(""));
            let tit = normaliser_titre(d["title"].as_str().unwrap_or(""));
            if concorde(&art, &art_attendu) && concorde(&tit, &tit_attendu) {
                return Ok(Some(d["rank"].as_i64().unwrap_or(0)));
            }
        }
        Ok(None)
    }
}

/// Deux chaînes normalisées concordent si elles sont égales ou si l'une
/// contient l'autre — « télépopmusik » ↔ « télépopmusik feat maud », « never
/// forget » ↔ « never forget instrumental version ».
fn concorde(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || a.contains(b) || b.contains(a))
}

/// Retire les guillemets d'un terme de recherche : ils fermeraient la chaîne
/// `artist:"…"` de la requête Deezer au mauvais endroit.
fn echapper(s: &str) -> String {
    s.replace('"', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concorde_egalite_et_inclusion() {
        assert!(concorde("nirvana", "nirvana"));
        assert!(concorde("telepopmusikfeatmaud", "telepopmusik"));
        assert!(concorde("neverforget", "neverforgetinstrumentalversion"));
        assert!(!concorde("nirvana", "nirwana"));
        assert!(!concorde("", "nirvana"));
    }
}
