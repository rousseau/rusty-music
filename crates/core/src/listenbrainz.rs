// SPDX-License-Identifier: GPL-3.0-or-later
//! ListenBrainz : le client, et rien d'autre.
//!
//! Comme [`crate::musicbrainz`], ce module parle au réseau et rend des données ;
//! il n'écrit pas en base et ne décide de rien. La passe qui l'emploie est dans
//! [`crate::decouvrir`].
//!
//! **Deux points d'entrée, deux hôtes.**
//! - `labs.api.listenbrainz.org/similar-artists` : les artistes proches d'un
//!   artiste donné, calculés sur les écoutes agrégées de la communauté. C'est la
//!   source des « artistes voisins » du mode Découvrir. Aucun compte requis.
//! - `api.listenbrainz.org/1/explore/fresh-releases` : la liste éditoriale des
//!   sorties récentes, tous artistes confondus. Un seul appel par passe, recoupé
//!   ensuite avec les artistes de la bibliothèque.
//!
//! **Données sous CC0.** Rien à attribuer, mais le client s'identifie tout de
//! même dans son `User-Agent`, par courtoisie et pour être joignable en cas
//! d'abus — même politesse qu'envers MusicBrainz.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};

/// Un artiste proche, tel que ListenBrainz le classe.
#[derive(Debug, Clone, PartialEq)]
pub struct Voisin {
    pub mbid: String,
    pub nom: String,
    /// Score de similarité brut du jeu de données. Seul son ordre compte : on
    /// garde les mieux classés, la valeur n'est pas montrée telle quelle.
    pub score: f64,
}

/// Popularité agrégée d'une entité, telle que ListenBrainz la rend :
/// `/1/popularity/{recording,release-group}` cumule les écoutes et les
/// auditeurs distincts de toute la communauté. Aucun compte requis, données
/// CC0.
#[derive(Debug, Clone, PartialEq)]
pub struct PopulariteMb {
    pub mbid: String,
    pub ecoutes: i64,
    pub auditeurs: i64,
}

/// Une sortie récente vue par ListenBrainz.
#[derive(Debug, Clone, PartialEq)]
pub struct SortieFraiche {
    /// Identifiant de release-group, quand ListenBrainz le donne — c'est la clé
    /// qu'on partage avec [`crate::musicbrainz::Album`].
    pub rg_mbid: Option<String>,
    pub titre: String,
    /// Les artistes crédités : `(mbid, nom_du_credit)`. Le nom est le libellé
    /// de crédit (« X feat. Y »), faute de mieux — ListenBrainz ne détaille pas.
    pub artistes: Vec<String>,
    pub artistes_mbids: Vec<String>,
    pub date_sortie: Option<String>,
    pub type_primaire: Option<String>,
    pub types_secondaires: Vec<String>,
}

/// Algorithme de similarité demandé à ListenBrainz.
///
/// Le jeu de données en expose plusieurs, réglés différemment ; celui-ci est le
/// choix par défaut de leur propre interface — fenêtre de session courte, seuil
/// de bruit modéré, cent résultats au plus.
const ALGO_SIMILARITE: &str =
    "session_based_days_7500_session_300_contribution_5_threshold_10_limit_100_filter_True_skip_30";

/// Délai minimal entre deux requêtes. ListenBrainz limite le débit par en-têtes ;
/// une seconde de marge garde le client largement sous la barre.
const CADENCE: Duration = Duration::from_millis(1_100);

/// Combien de fois réessayer avant d'abandonner une requête.
const ESSAIS: u32 = 4;

/// Client ListenBrainz, cadencé. Un seul pour tout le processus : c'est lui qui
/// porte l'horloge du débit.
pub struct Client {
    agent: ureq::Agent,
    dernier: Mutex<Option<Instant>>,
}

impl Client {
    pub fn new(contact: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!(
                "rusty-music/{} ( {contact} )",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self {
            agent,
            dernier: Mutex::new(None),
        }
    }

    /// Patiente le temps qu'il faut pour ne pas dépasser la cadence.
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

    /// Une requête GET rendant du JSON, réessayée sur échec temporaire.
    ///
    /// `404` est une réponse : la ressource n'existe pas, on rend `None`. Tout
    /// le reste — coupure réseau, `429`, `503` — est traité comme temporaire,
    /// l'attente doublant à chaque tentative.
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

    /// Une requête POST rendant du JSON, réessayée sur échec temporaire —
    /// même politique que [`Self::json`], mais avec un corps.
    fn post_json(&self, url: &str, corps: &Value) -> Result<Option<Value>> {
        let corps = serde_json::to_string(corps)
            .map_err(|e| Error::Reseau(format!("encodage JSON : {e}")))?;
        let mut derniere = String::new();
        for essai in 0..ESSAIS {
            self.cadencer();
            match self
                .agent
                .post(url)
                .content_type("application/json")
                .send(corps.as_str())
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

    /// Popularité agrégée d'une liste d'enregistrements. Seuls les MBID connus
    /// de ListenBrainz figurent dans la réponse ; un MBID absent n'a pas
    /// (encore) d'écoute enregistrée — à l'appelant d'en tenir compte.
    pub fn popularite_enregistrements(&self, mbids: &[String]) -> Result<Vec<PopulariteMb>> {
        self.popularite("recording", "recording_mbids", "recording_mbid", mbids)
    }

    /// Idem pour une liste de release-groups (l'album au sens MusicBrainz).
    pub fn popularite_albums(&self, mbids: &[String]) -> Result<Vec<PopulariteMb>> {
        self.popularite(
            "release-group",
            "release_group_mbids",
            "release_group_mbid",
            mbids,
        )
    }

    fn popularite(
        &self,
        chemin: &str,
        champ_requete: &str,
        champ_reponse: &str,
        mbids: &[String],
    ) -> Result<Vec<PopulariteMb>> {
        if mbids.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("https://api.listenbrainz.org/1/popularity/{chemin}");
        let v = self
            .post_json(&url, &serde_json::json!({ champ_requete: mbids }))?
            .unwrap_or(Value::Null);
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        Some(PopulariteMb {
                            mbid: e[champ_reponse].as_str()?.to_string(),
                            ecoutes: e["total_listen_count"].as_i64().unwrap_or(0),
                            auditeurs: e["total_user_count"].as_i64().unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Les artistes proches de `mbid`, du mieux classé au moins bien classé.
    ///
    /// Une liste vide est une réponse valable : le jeu de données ne couvre pas
    /// tous les artistes, et un artiste peu écouté n'a pas de voisin calculé.
    pub fn artistes_similaires(&self, mbid: &str) -> Result<Vec<Voisin>> {
        let url = format!(
            "https://labs.api.listenbrainz.org/similar-artists/json\
             ?artist_mbids={mbid}&algorithm={ALGO_SIMILARITE}"
        );
        Ok(self
            .json(&url)?
            .map(|v| voisins_de(&v))
            .unwrap_or_default())
    }

    /// Les sorties récentes vues par ListenBrainz, sur `jours` en arrière.
    ///
    /// Un seul appel, non filtré : c'est à l'appelant de croiser avec les
    /// artistes qu'il connaît. La réponse est large (des milliers de lignes),
    /// mais tient en une requête.
    pub fn sorties_fraiches(&self, jours: u32) -> Result<Vec<SortieFraiche>> {
        let url = format!(
            "https://api.listenbrainz.org/1/explore/fresh-releases/\
             ?days={jours}&past=true&future=false&sort=release_date"
        );
        Ok(self
            .json(&url)?
            .map(|v| sorties_de(&v))
            .unwrap_or_default())
    }
}

/// Extrait les voisins d'une réponse `similar-artists`.
///
/// Le jeu de données rend une liste plate d'objets. On accepte deux noms de
/// champ pour l'identité (`artist_mbid` / `artist_mbids`) et pour le nom
/// (`name` / `artist_name`) : les jeux ListenBrainz ne sont pas tout à fait
/// uniformes d'une version à l'autre.
fn voisins_de(v: &Value) -> Vec<Voisin> {
    let lignes = v
        .as_array()
        .cloned()
        // Certaines réponses labs enveloppent la liste dans un second élément.
        .or_else(|| v.get(1).and_then(|x| x.as_array()).cloned())
        .unwrap_or_default();
    lignes
        .iter()
        .filter_map(|r| {
            let mbid = r["artist_mbid"]
                .as_str()
                .or_else(|| r["artist_mbids"].as_array()?.first()?.as_str())?
                .to_string();
            let nom = r["name"]
                .as_str()
                .or_else(|| r["artist_name"].as_str())
                .unwrap_or("?")
                .to_string();
            let score = r["score"].as_f64().unwrap_or(0.0);
            Some(Voisin { mbid, nom, score })
        })
        .collect()
}

/// Extrait les sorties d'une réponse `fresh-releases`.
fn sorties_de(v: &Value) -> Vec<SortieFraiche> {
    v["payload"]["releases"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let titre = r["release_name"].as_str()?.to_string();
                    let artistes_mbids: Vec<String> = r["artist_mbids"]
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                        .unwrap_or_default();
                    let artistes: Vec<String> = r["artist_credit_name"]
                        .as_str()
                        .map(|s| vec![s.to_string()])
                        .unwrap_or_default();
                    Some(SortieFraiche {
                        rg_mbid: r["release_group_mbid"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                        titre,
                        artistes,
                        artistes_mbids,
                        date_sortie: r["release_date"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                        type_primaire: r["release_group_primary_type"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                        types_secondaires: r["release_group_secondary_type"]
                            .as_array()
                            .map(|a| {
                                a.iter().filter_map(Value::as_str).map(str::to_string).collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extrait d'une vraie réponse `similar-artists` (Radiohead), tronquée.
    #[test]
    fn voisins_de_lit_une_liste_plate() {
        let v: Value = serde_json::from_str(
            r#"[
                {"artist_mbid":"8bfac288-ccc5-448d-9573-c33ea2aa5c30","name":"Red Hot Chili Peppers","score":941},
                {"artist_mbid":"cc197bad-dc9c-440d-a5b5-d52ba2e14234","name":"Coldplay","score":812}
            ]"#,
        )
        .expect("JSON de test");
        let g = voisins_de(&v);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].nom, "Red Hot Chili Peppers");
        assert!(g[0].score > g[1].score);
    }

    #[test]
    fn voisins_de_une_reponse_vide_rend_une_liste_vide() {
        assert!(voisins_de(&serde_json::json!([])).is_empty());
        assert!(voisins_de(&serde_json::json!({"error": "x"})).is_empty());
    }

    #[test]
    fn sorties_de_lit_le_payload() {
        let v: Value = serde_json::from_str(
            r#"{"payload":{"releases":[
                {"release_name":"New Album","artist_credit_name":"Some Artist",
                 "artist_mbids":["aaa"],"release_group_mbid":"rg-1",
                 "release_date":"2026-08-20","release_group_primary_type":"Album",
                 "release_group_secondary_type":[]}
            ],"total_count":1}}"#,
        )
        .expect("JSON de test");
        let s = sorties_de(&v);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].rg_mbid.as_deref(), Some("rg-1"));
        assert_eq!(s[0].artistes_mbids, vec!["aaa".to_string()]);
        assert_eq!(s[0].date_sortie.as_deref(), Some("2026-08-20"));
    }
}
