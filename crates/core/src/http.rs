// SPDX-License-Identifier: GPL-3.0-or-later
//! Client HTTP cadencé et repris sur erreur temporaire — mécanique commune à
//! tous les enrichissements JSON (MusicBrainz, Deezer, ListenBrainz, Last.fm,
//! TheAudioDB, CritiqueBrainz).
//!
//! Les six modules partageaient jusqu'ici le même squelette recopié (`agent`,
//! `dernier: Mutex<Option<Instant>>`, `cadencer()`, boucle de reprise), avec
//! les mêmes défauts dans les six : une attente inutile après la *dernière*
//! tentative avant de renvoyer l'erreur, une reprise sur toute panne y
//! compris des codes qui ne valent pas la peine d'être retentés, et l'URL
//! complète — donc une éventuelle clé d'API — recopiée dans le message
//! d'erreur final, qui remonte jusqu'au journal persistant de l'application
//! et jusqu'à l'état affiché à l'utilisateur.
//!
//! Ce module ne fait que la partie réseau ; chaque appelant garde sa propre
//! extraction JSON et ses propres types métier. Il ne met **jamais** l'URL
//! dans un message d'erreur — seulement `ctx`, un libellé sans secret fourni
//! par l'appelant (ex. `"artist.gettoptags"`).
//!
//! `crate::discogs::agent`/`telecharger_avec_avancement` restent séparés :
//! c'est un téléchargement de fichier volumineux en flux (le dump mensuel,
//! plusieurs Go, réutilisé aussi par `crate::modeles` pour les poids de
//! modèles), sans cadence entre requêtes ni réponse JSON à parser —
//! `ClientCadence` ne lui apporterait rien.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{Error, Result};

/// Combien de fois réessayer avant d'abandonner — identique aux six clients
/// d'origine.
const ESSAIS: u32 = 4;

/// Issue d'une requête, avant que [`ClientCadence::get_json`]/[`ClientCadence::post_json`]
/// ne la simplifient en `Option<Value>` pour l'appelant courant.
///
/// Distincte d'un simple `Option<Value>` pour porter aussi le cas
/// [`Reponse::ArretImmediat`], que seul `Client` de `lastfm.rs` distingue
/// aujourd'hui (une clé refusée, HTTP 401/403, ne vaut pas la peine d'être
/// retentée — voir [`ClientCadence::get_json_avec`]).
pub enum Reponse {
    /// Reçue et décodée.
    Trouvee(Value),
    /// 404 : la ressource n'existe pas — pas une panne, une réponse.
    Absente,
    /// Un code que l'appelant a désigné via `arret_immediat` comme ne valant
    /// pas la peine d'être retenté. Le corps n'est pas lu.
    ArretImmediat(u16),
}

/// Client HTTP cadencé et repris sur erreur temporaire.
pub struct ClientCadence {
    agent: ureq::Agent,
    cadence: Duration,
    dernier: Mutex<Option<Instant>>,
}

impl ClientCadence {
    /// `agent` vient de l'appelant : chacun des six clients d'origine pose
    /// son propre `User-Agent` (certains y ajoutent une adresse de contact,
    /// comme MusicBrainz l'exige), ce que ce type n'a pas à savoir.
    pub fn new(agent: ureq::Agent, cadence: Duration) -> Self {
        Self { agent, cadence, dernier: Mutex::new(None) }
    }

    /// Patiente le temps qu'il faut pour ne pas dépasser la cadence.
    ///
    /// Publique : certains appelants (le téléchargement d'image de
    /// `deezer.rs`, qui ne passe pas par [`Self::get_json`]) doivent aussi
    /// respecter la cadence sur une requête qui n'en est pas une au sens de
    /// ce module.
    pub fn cadencer(&self) {
        let mut dernier = self.dernier.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(precedent) = *dernier {
            let ecoule = precedent.elapsed();
            if ecoule < self.cadence {
                std::thread::sleep(self.cadence - ecoule);
            }
        }
        *dernier = Some(Instant::now());
    }

    /// Une requête GET rendant du JSON, réessayée sur échec temporaire.
    /// `404` devient `None` ; `ctx` (jamais l'url) identifie la requête dans
    /// un message d'erreur éventuel.
    pub fn get_json(&self, url: &str, ctx: &str) -> Result<Option<Value>> {
        match self.get_json_avec(url, ctx, |_| false)? {
            Reponse::Trouvee(v) => Ok(Some(v)),
            Reponse::Absente => Ok(None),
            Reponse::ArretImmediat(_) => unreachable!("arret_immediat toujours faux ici"),
        }
    }

    /// Comme [`Self::get_json`], mais `arret_immediat(code)` peut désigner un
    /// code qui ne vaut pas la peine d'être retenté (ex. 401/403 chez
    /// Last.fm : une clé refusée le reste à la tentative suivante) — la
    /// requête revient alors aussitôt en [`Reponse::ArretImmediat`], sans
    /// attendre ni épuiser [`ESSAIS`].
    pub fn get_json_avec(
        &self,
        url: &str,
        ctx: &str,
        arret_immediat: impl Fn(u16) -> bool,
    ) -> Result<Reponse> {
        self.requete(ctx, arret_immediat, |agent| agent.get(url).call())
    }

    /// Comme [`Self::get_json`], avec des paramètres de requête ajoutés et
    /// encodés par `ureq` (`?query("q", "artist:\"a b\"")` → `%22a+b%22`,
    /// entre autres) — nécessaire dès qu'un paramètre porte du texte libre
    /// (une recherche Deezer), jamais sûr à interpoler tel quel dans l'URL.
    pub fn get_json_requete(&self, url: &str, query: &[(&str, &str)], ctx: &str) -> Result<Option<Value>> {
        match self.requete(ctx, |_| false, |agent| {
            let mut req = agent.get(url);
            for (cle, valeur) in query {
                req = req.query(*cle, *valeur);
            }
            req.call()
        })? {
            Reponse::Trouvee(v) => Ok(Some(v)),
            Reponse::Absente => Ok(None),
            Reponse::ArretImmediat(_) => unreachable!("arret_immediat toujours faux ici"),
        }
    }

    /// Une requête POST rendant du JSON, réessayée sur échec temporaire —
    /// même politique que [`Self::get_json`], mais avec un corps.
    pub fn post_json(&self, url: &str, corps: &Value, ctx: &str) -> Result<Option<Value>> {
        let texte = serde_json::to_string(corps)
            .map_err(|e| Error::Reseau(format!("{ctx} : encodage JSON : {e}")))?;
        match self.requete(ctx, |_| false, |agent| {
            agent.post(url).content_type("application/json").send(texte.as_str())
        })? {
            Reponse::Trouvee(v) => Ok(Some(v)),
            Reponse::Absente => Ok(None),
            Reponse::ArretImmediat(_) => unreachable!("arret_immediat toujours faux ici"),
        }
    }

    fn requete(
        &self,
        ctx: &str,
        arret_immediat: impl Fn(u16) -> bool,
        mut appel: impl FnMut(
            &ureq::Agent,
        ) -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Reponse> {
        let mut derniere = String::new();
        for essai in 0..ESSAIS {
            self.cadencer();
            match appel(&self.agent) {
                Ok(mut r) => {
                    let corps = r
                        .body_mut()
                        .read_to_string()
                        .map_err(|e| Error::Reseau(format!("{ctx} : lecture du corps : {e}")))?;
                    return serde_json::from_str(&corps).map(Reponse::Trouvee).map_err(|e| {
                        Error::Reseau(format!("{ctx} : JSON illisible : {e}"))
                    });
                }
                Err(ureq::Error::StatusCode(404)) => return Ok(Reponse::Absente),
                Err(ureq::Error::StatusCode(code)) if arret_immediat(code) => {
                    return Ok(Reponse::ArretImmediat(code));
                }
                Err(e) => {
                    // Le message de `ureq::Error` ne porte pas l'URL de la
                    // requête (vérifié sur l'énumération de la 3.4) : rien
                    // ici ne peut donc faire fuiter une clé d'API, même à la
                    // dernière tentative.
                    derniere = e.to_string();
                    if essai + 1 < ESSAIS {
                        std::thread::sleep(Duration::from_secs(1 << essai));
                    }
                }
            }
        }
        Err(Error::Reseau(format!("{ctx} : {ESSAIS} tentatives sans succès — {derniere}")))
    }
}
