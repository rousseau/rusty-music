// SPDX-License-Identifier: GPL-3.0-or-later
//! Last.fm : le client, et rien d'autre.
//!
//! Comme [`crate::critiquebrainz`], ce module parle au réseau et rend des
//! données ; il n'écrit pas en base. La passe qui l'emploie est dans
//! [`crate::lastfm_pass`].
//!
//! **Sert le nommage des familles, pas la popularité.** `docs/popularite.md`
//! avait écarté Last.fm faute de clé gratuite disponible sans compte — cette
//! fois la clé est demandée à l'utilisateur (gratuite, sur last.fm/api), pour
//! un usage différent : les tags de genre communautaires (`artist.getTopTags`)
//! votent aux côtés de MusicBrainz et du vocabulaire CLAP-texte, ils ne
//! mesurent aucune écoute. Interrogation **exclusivement par MBID** — jamais
//! de recherche par nom, même discipline que TheAudioDB.

use std::time::Duration;

use serde_json::Value;

use crate::error::Error;
use crate::http::{ClientCadence, Reponse};

/// Délai minimal entre deux requêtes — API publique sans limite annoncée,
/// même politesse qu'envers les autres sources.
const CADENCE: Duration = Duration::from_millis(300);

/// Pourquoi la récupération des tags d'un artiste a échoué — la distinction
/// que [`crate::lastfm_pass`] a besoin de faire : réessayer un autre artiste
/// après un `Bloquant` échouerait exactement pareil.
#[derive(Debug, thiserror::Error)]
pub enum EchecTags {
    /// La requête elle-même est refusée — clé invalide ou suspendue, débit
    /// dépassé (HTTP 401/403, ou codes Last.fm 4/10/26/29). La passe s'arrête
    /// là : c'est un problème de clé, pas de cet artiste.
    #[error("{0}")]
    Bloquant(#[source] Error),
    /// Cet artiste précis n'a rien donné — 5xx ponctuel, réseau instable,
    /// JSON illisible. Un autre artiste peut réussir : la passe le note et
    /// continue.
    #[error("{0}")]
    Ponctuel(#[source] Error),
}

/// Un tag communautaire et son poids relatif (0-100, tel que Last.fm le rend).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub nom: String,
    pub poids: u32,
}

/// Client Last.fm, cadencé.
pub struct Client {
    http: ClientCadence,
    cle: String,
}

impl Client {
    pub fn new(cle: String) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!("rusty-music/{}", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self { http: ClientCadence::new(agent, CADENCE), cle }
    }

    /// Une clé refusée (bogue, révoquée) rend 401/403 — vérifié en direct sur
    /// `ws.audioscrobbler.com`. Insister ne changera rien : quatre tentatives
    /// avec attente croissante (15 s) pour échouer pareil au bout du compte
    /// ne feraient que ralentir le diagnostic, artiste après artiste — d'où
    /// `Bloquant`, qui arrête la passe et revient aussitôt, sans épuiser les
    /// tentatives comme le ferait un [`crate::error::Error::Reseau`] ordinaire.
    ///
    /// `ctx` (jamais l'url, qui porte la clé) est ce qu'un message d'erreur
    /// éventuel garde du site interrogé.
    fn json(&self, url: &str) -> std::result::Result<Value, EchecTags> {
        match self.http.get_json_avec(url, "artist.gettoptags", |code| matches!(code, 401 | 403)) {
            Ok(Reponse::Trouvee(v)) => Ok(v),
            Ok(Reponse::Absente) => Ok(Value::Null),
            Ok(Reponse::ArretImmediat(code)) => Err(EchecTags::Bloquant(Error::Reseau(format!(
                "clé Last.fm refusée (HTTP {code})"
            )))),
            Err(e) => Err(EchecTags::Ponctuel(e)),
        }
    }

    /// Les tags de genre les mieux votés pour l'artiste `mbid`. Une liste vide
    /// est une réponse valable : Last.fm ne connaît pas tous les artistes.
    pub fn tags_artiste(&self, mbid: &str) -> std::result::Result<Vec<Tag>, EchecTags> {
        let url = format!(
            "https://ws.audioscrobbler.com/2.0/?method=artist.gettoptags\
             &mbid={mbid}&api_key={}&format=json",
            self.cle
        );
        let v = self.json(&url)?;
        tags_de(&v)
    }
}

/// Codes Last.fm qui disent quelque chose de la requête, pas de l'artiste —
/// clé invalide (10), suspendue (26), authentification refusée (4), débit
/// dépassé (29). Rendus en 401/403 dans les cas observés (voir `Client::
/// json`) ; vérifiés ici aussi au cas où Last.fm les rendrait un jour en
/// 200, comme sa documentation le laisse ouvert.
const CODES_BLOQUANTS: [u64; 4] = [4, 10, 26, 29];

/// Décode la réponse `artist.getTopTags` — extrait pour se tester sans
/// réseau, même patron que [`tag_de`].
fn tags_de(v: &Value) -> std::result::Result<Vec<Tag>, EchecTags> {
    if let Some(code) = v["error"].as_u64() {
        if CODES_BLOQUANTS.contains(&code) {
            // Ce n'est pas une absence de donnée : il ne faut pas la
            // masquer en silence comme l'artiste introuvable (6) ci-dessous —
            // et c'est un problème de clé, donc `Bloquant`.
            let message = v["message"].as_str().unwrap_or("sans détail");
            return Err(EchecTags::Bloquant(Error::Reseau(format!(
                "Last.fm : {message} (code {code})"
            ))));
        }
        // Un artiste inconnu de Last.fm (6) ou une autre requête sans objet
        // rend `{"error":N,...}` plutôt qu'un tableau vide — pas une
        // erreur, juste rien à en tirer.
        return Ok(Vec::new());
    }
    let tags = v["toptags"]["tag"].as_array().cloned().unwrap_or_default();
    Ok(tags.iter().filter_map(tag_de).collect())
}

fn tag_de(t: &Value) -> Option<Tag> {
    let nom = t["name"].as_str()?.to_string();
    if nom.is_empty() {
        return None;
    }
    // Le poids arrive tantôt en nombre, tantôt en chaîne selon les artistes —
    // observé en direct, pas documenté par Last.fm.
    let poids = t["count"]
        .as_u64()
        .or_else(|| t["count"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0) as u32;
    Some(Tag { nom, poids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn tag_de_lit_nom_et_poids() {
        let v: Value =
            serde_json::from_str(r#"{"name":"celtic","count":45,"url":"https://..."}"#)
                .expect("JSON de test");
        let t = tag_de(&v).expect("tag attendu");
        assert_eq!(t.nom, "celtic");
        assert_eq!(t.poids, 45);
    }

    #[test]
    fn tag_de_accepte_un_compte_en_chaine() {
        let v: Value = serde_json::from_str(r#"{"name":"folk","count":"12"}"#)
            .expect("JSON de test");
        let t = tag_de(&v).expect("tag attendu");
        assert_eq!(t.poids, 12);
    }

    #[test]
    fn tag_de_sans_nom_rend_rien() {
        let v: Value = serde_json::from_str(r#"{"count":10}"#).expect("JSON de test");
        assert!(tag_de(&v).is_none());
    }

    #[test]
    fn tags_de_lit_la_liste_de_tags() {
        let v: Value = serde_json::from_str(
            r#"{"toptags":{"tag":[{"name":"celtic","count":45}],"@attr":{"artist":"x"}}}"#,
        )
        .expect("JSON de test");
        let tags = tags_de(&v).expect("tags attendus");
        assert_eq!(tags, vec![Tag { nom: "celtic".into(), poids: 45 }]);
    }

    /// Régression : un artiste introuvable (code 6) est un état normal,
    /// jamais une erreur — sans quoi la passe le retenterait indéfiniment.
    #[test]
    fn tags_de_rend_une_liste_vide_pour_un_artiste_introuvable() {
        let v: Value = serde_json::from_str(r#"{"error":6,"message":"Artist not found"}"#)
            .expect("JSON de test");
        assert_eq!(tags_de(&v).unwrap(), Vec::new());
    }

    /// Une clé invalide ne doit jamais ressembler à « aucun tag » — sans
    /// cette distinction, `rusty-music lastfm --cle <mauvaise clé>` rend un
    /// bilan à 0 tag indiscernable d'une bibliothèque dont personne n'a de
    /// tag Last.fm, au lieu de dire clairement que la clé est en cause.
    #[test]
    fn tags_de_signale_une_cle_invalide_comme_une_erreur() {
        let v: Value = serde_json::from_str(r#"{"error":10,"message":"Invalid API key"}"#)
            .expect("JSON de test");
        let e = tags_de(&v).expect_err("une clé invalide doit remonter en erreur");
        assert!(matches!(e, EchecTags::Bloquant(_)), "doit arrêter la passe : {e}");
        assert!(e.to_string().contains("Invalid API key"), "message perdu : {e}");
        assert!(e.to_string().contains("10"), "code perdu : {e}");
    }

    #[test]
    fn tags_de_signale_une_cle_suspendue_et_une_limite_de_debit() {
        for code in [4, 26, 29] {
            let v: Value =
                serde_json::from_str(&format!(r#"{{"error":{code},"message":"m"}}"#))
                    .expect("JSON de test");
            assert!(
                matches!(tags_de(&v), Err(EchecTags::Bloquant(_))),
                "code {code} devrait arrêter la passe"
            );
        }
    }

    /// Un serveur d'une seule réponse HTTP, sur un port libre de la boucle
    /// locale — même patron que `modeles::tests::serveur`.
    fn serveur(reponse: &'static [u8]) -> String {
        use std::io::{Read, Write};
        let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = ecoute.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut flux, _)) = ecoute.accept() {
                let mut requete = [0u8; 2048];
                let _ = flux.read(&mut requete);
                let _ = flux.write_all(reponse);
            }
        });
        format!("http://127.0.0.1:{port}/2.0/")
    }

    /// Régression : c'est `Client::json`, pas `tags_de`, qui doit traduire un
    /// vrai 401/403 HTTP en `Bloquant` sans épuiser les tentatives — le test
    /// ci-dessus ne couvre que le cas où Last.fm rend le code d'erreur dans
    /// un corps 200, jamais le chemin HTTP réel emprunté par
    /// `ClientCadence::get_json_avec`.
    #[test]
    fn json_traduit_un_401_http_en_bloquant_sans_epuiser_les_tentatives() {
        let url = serveur(b"HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n");
        let client = Client::new("peu-importe".to_string());
        let debut = Instant::now();
        let e = client.json(&url).expect_err("401 doit remonter en erreur");
        assert!(matches!(e, EchecTags::Bloquant(_)), "doit arrêter la passe : {e}");
        assert!(
            debut.elapsed() < Duration::from_secs(2),
            "ne doit pas épuiser les tentatives (attente exponentielle) sur un arrêt immédiat"
        );
    }
}
