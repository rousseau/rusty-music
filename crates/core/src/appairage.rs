// SPDX-License-Identifier: GPL-3.0-or-later
//! Le Mac et l'iPhone sur le Wi-Fi local (étape 4B de
//! `docs/plan-ios-compagnon.md`) : le passage de la lecture dans les deux sens
//! (option B), en attendant la synchronisation des valises.
//!
//! **Appairage au câble.** Le Mac tire un jeton au hasard et le dépose avec
//! [`FICHIER`] dans les Documents de l'app compagnon, par `devicectl`, au
//! prochain envoi ou passage par câble. Son serveur refuse toute requête qui
//! ne le porte pas ; « Oublier l'iPhone » l'efface. Le jeton circule en clair
//! sur le réseau local (HTTP, pas de TLS) : il protège d'un voisin de Wi-Fi
//! curieux, pas d'un attaquant qui écoute le réseau.
//!
//! **Pas de Bonjour.** Sur iOS, le multicast brut demande un droit qu'Apple
//! accorde au cas par cas ; le fichier porte donc les adresses où chercher le
//! Mac, dans l'ordre : son nom `.local` (résolu par le système), son adresse
//! sur le réseau local, son adresse Tailscale s'il en a une.
//!
//! Le protocole suit les règles du `handoff.ts` de Disc Player
//! (<https://github.com/eudj1n/snowsky-disc-player>, licence MIT, © eudj1n) :
//! chaque pas est confirmé par l'autre côté avant que la source ne se taise.
//! - Mac → iPhone : le Mac garde le passage en attente ([`route::PASSAGE`]) ;
//!   l'iPhone le prend à son retour au premier plan puis l'accuse
//!   ([`route::PASSAGE_RECU`]), et c'est seulement alors que le Mac se met en
//!   pause.
//! - iPhone → Mac : l'iPhone envoie sa lecture ; le Mac la charge en pause et
//!   répond un [`Accuse`], après quoi l'iPhone se met en pause.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::passage::Passage;

/// Nom du fichier d'appairage, à la racine des Documents de l'app et dans les
/// données de l'app de bureau.
pub const FICHIER: &str = "appairage.json";

/// Version du protocole, rendue par [`route::BONJOUR`].
pub const VERSION: u32 = 1;

/// Port d'écoute du Mac, hors des plages réservées.
pub const PORT: u16 = 47391;

/// Les chemins servis par le Mac.
pub mod route {
    /// `GET` : le Mac répond son nom ([`super::Bonjour`]).
    pub const BONJOUR: &str = "/v1/bonjour";
    /// `GET` : le passage que le Mac tient en attente (204 s'il n'y en a pas).
    /// `POST` : l'iPhone rend la lecture au Mac.
    pub const PASSAGE: &str = "/v1/passage";
    /// `POST` : l'iPhone accuse réception d'un passage ([`super::Recu`]).
    pub const PASSAGE_RECU: &str = "/v1/passage/recu";
}

/// Ce que le câble dépose sur l'iPhone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Appairage {
    pub version: u32,
    /// Nom du Mac, pour le dire à l'écran.
    pub mac: String,
    pub jeton: String,
    pub port: u16,
    /// Où chercher le Mac, dans l'ordre (nom `.local`, adresses IPv4).
    #[serde(default)]
    pub adresses: Vec<String>,
}

/// Réponse de [`route::BONJOUR`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bonjour {
    pub mac: String,
    pub version: u32,
}

/// Accusé de réception d'un passage Mac → iPhone : son instant d'émission,
/// pour que le Mac ne se taise pas sur un passage remplacé entre-temps.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Recu {
    pub emis_le: i64,
}

/// Ce que le Mac a repris d'une lecture rendue par l'iPhone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Accuse {
    pub pistes: usize,
    /// Morceaux de la file introuvables sur le Mac.
    pub absents: usize,
    pub courante_presente: bool,
}

impl Appairage {
    /// Un appairage neuf, au jeton tiré au hasard.
    pub fn nouveau(mac: String, port: u16, adresses: Vec<String>) -> Result<Self> {
        Ok(Self {
            version: VERSION,
            mac,
            jeton: nouveau_jeton()?,
            port,
            adresses,
        })
    }

    pub fn lire(chemin: &Path) -> Result<Self> {
        serde_json::from_slice(&std::fs::read(chemin)?).map_err(|e| Error::Parsing(e.to_string()))
    }

    /// Écrit le fichier, lisible par son seul propriétaire : il porte le jeton.
    pub fn ecrire(&self, chemin: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| Error::Parsing(e.to_string()))?;
        std::fs::write(chemin, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(chemin, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// L'en-tête `Authorization` qu'attend le Mac.
    pub fn autorisation(&self) -> String {
        format!("Bearer {}", self.jeton)
    }

    /// `entete` porte-t-il le jeton ? Comparaison en temps constant : la
    /// durée de la réponse ne dit pas combien de caractères étaient justes.
    pub fn autorise(&self, entete: Option<&str>) -> bool {
        let attendu = self.autorisation();
        let Some(recu) = entete else { return false };
        recu.len() == attendu.len()
            && recu
                .bytes()
                .zip(attendu.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }
}

/// 24 octets du générateur du système, en hexadécimal.
fn nouveau_jeton() -> Result<String> {
    let mut octets = [0u8; 24];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut octets)?;
    Ok(octets.iter().map(|o| format!("{o:02x}")).collect())
}

/// Le client de l'iPhone. Les délais sont courts : il est interrogé au retour
/// au premier plan, et un Mac absent ne doit pas faire attendre l'écran.
pub struct Client {
    agent: ureq::Agent,
    base: String,
    autorisation: String,
}

impl Client {
    /// Essaie `preferee` puis chaque adresse de l'appairage ; rend le client
    /// de la première qui répond, et le nom du Mac. Un 401 arrête tout : le
    /// Mac a oublié cet iPhone ([`Error::AccesRefuse`]).
    pub fn joindre(a: &Appairage, preferee: Option<&str>) -> Result<(Self, Bonjour)> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_millis(1500)))
            .timeout_global(Some(Duration::from_secs(5)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut derniere = Error::Reseau("aucune adresse où chercher le Mac".into());
        let adresses = preferee.into_iter().chain(
            a.adresses
                .iter()
                .map(String::as_str)
                .filter(|x| Some(*x) != preferee),
        );
        for adresse in adresses {
            let client = Self {
                agent: agent.clone(),
                base: format!("http://{adresse}:{}", a.port),
                autorisation: a.autorisation(),
            };
            match client.bonjour() {
                Ok(b) => return Ok((client, b)),
                Err(e @ Error::AccesRefuse(_)) => return Err(e),
                Err(e) => derniere = e,
            }
        }
        Err(derniere)
    }

    /// L'adresse qui a répondu, à essayer d'abord la fois suivante.
    pub fn adresse(&self) -> &str {
        let sans_schema = self.base.trim_start_matches("http://");
        sans_schema.rsplit_once(':').map_or(sans_schema, |(h, _)| h)
    }

    fn bonjour(&self) -> Result<Bonjour> {
        let (code, corps) = self.envoyer("GET", route::BONJOUR, None)?;
        attendre(code, 200, route::BONJOUR)?;
        json(&corps)
    }

    /// Le passage que le Mac tient en attente, s'il y en a un.
    pub fn passage_en_attente(&self) -> Result<Option<Passage>> {
        let (code, corps) = self.envoyer("GET", route::PASSAGE, None)?;
        if code == 204 {
            return Ok(None);
        }
        attendre(code, 200, route::PASSAGE)?;
        Passage::depuis_json(&corps).map(Some)
    }

    /// Accuse réception d'un passage : le Mac se met alors en pause.
    pub fn accuser(&self, emis_le: i64) -> Result<()> {
        let corps =
            serde_json::to_vec(&Recu { emis_le }).map_err(|e| Error::Parsing(e.to_string()))?;
        let (code, _) = self.envoyer("POST", route::PASSAGE_RECU, Some(&corps))?;
        attendre(code, 204, route::PASSAGE_RECU)
    }

    /// Rend la lecture au Mac, qui la charge en pause.
    pub fn rendre(&self, passage: &Passage) -> Result<Accuse> {
        let corps = serde_json::to_vec(passage).map_err(|e| Error::Parsing(e.to_string()))?;
        let (code, reponse) = self.envoyer("POST", route::PASSAGE, Some(&corps))?;
        if code == 422 {
            return Err(Error::Parsing(
                String::from_utf8_lossy(&reponse).into_owned(),
            ));
        }
        attendre(code, 200, route::PASSAGE)?;
        json(&reponse)
    }

    fn envoyer(&self, methode: &str, route: &str, corps: Option<&[u8]>) -> Result<(u16, Vec<u8>)> {
        let url = format!("{}{route}", self.base);
        let reseau = |e: ureq::Error| Error::Reseau(format!("{route} : {e}"));
        let mut reponse = match (methode, corps) {
            ("POST", Some(c)) => self
                .agent
                .post(&url)
                .header("Authorization", &self.autorisation)
                .header("Content-Type", "application/json")
                .send(c),
            _ => self
                .agent
                .get(&url)
                .header("Authorization", &self.autorisation)
                .call(),
        }
        .map_err(reseau)?;
        let code = reponse.status().as_u16();
        if code == 401 {
            return Err(Error::AccesRefuse(
                "le Mac ne reconnaît plus cet iPhone : rebrancher le câble pour l'appairer".into(),
            ));
        }
        let corps = reponse.body_mut().read_to_vec().map_err(reseau)?;
        Ok((code, corps))
    }
}

fn attendre(code: u16, attendu: u16, route: &str) -> Result<()> {
    if code == attendu {
        Ok(())
    } else {
        Err(Error::Reseau(format!("{route} : réponse {code}")))
    }
}

fn json<T: serde::de::DeserializeOwned>(octets: &[u8]) -> Result<T> {
    serde_json::from_slice(octets).map_err(|e| Error::Parsing(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_jeton_est_neuf_et_seul_le_bon_passe() {
        let a = Appairage::nouveau("Mac".into(), PORT, vec!["mac.local".into()]).unwrap();
        let b = Appairage::nouveau("Mac".into(), PORT, Vec::new()).unwrap();
        assert_eq!(a.jeton.len(), 48);
        assert_ne!(a.jeton, b.jeton);
        assert!(a.autorise(Some(&a.autorisation())));
        assert!(!a.autorise(Some(&b.autorisation())));
        assert!(!a.autorise(Some("Bearer ")));
        assert!(!a.autorise(None));
    }

    #[test]
    fn le_fichier_fait_l_aller_retour_et_reste_prive() {
        let dossier = std::env::temp_dir().join(format!("rusty-appairage-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join(FICHIER);
        let a = Appairage::nouveau("Mac".into(), PORT, vec!["10.0.0.2".into()]).unwrap();
        a.ecrire(&chemin).unwrap();
        assert_eq!(Appairage::lire(&chemin).unwrap(), a);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&chemin).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dossier).unwrap();
    }

    #[test]
    fn sans_mac_joignable_le_client_le_dit() {
        // Port 9 (discard) sur la boucle locale : rien n'y écoute.
        let a = Appairage {
            version: VERSION,
            mac: "Mac".into(),
            jeton: "x".into(),
            port: 9,
            adresses: vec!["127.0.0.1".into()],
        };
        assert!(matches!(Client::joindre(&a, None), Err(Error::Reseau(_))));
    }
}
