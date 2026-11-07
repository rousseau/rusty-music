// SPDX-License-Identifier: GPL-3.0-or-later
//! Téléchargement d'un gros fichier en flux : le dump Discogs (plusieurs Go) et
//! les poids des modèles (`crate::modeles`). Sans cadence ni réponse JSON —
//! voir `crate::http` pour les clients d'enrichissement.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use crate::error::{Error, Result};

/// Un agent HTTP pour le téléchargement du dump — pas de cadence particulière
/// (un seul très gros téléchargement, pas des milliers de petites requêtes),
/// mais un délai généreux : le fichier pèse plusieurs gigaoctets.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("rusty-music/{}", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(std::time::Duration::from_secs(3600)))
        .build()
        .into()
}

/// Télécharge `url` dans `dest`, en flux — jamais entièrement en mémoire.
pub fn telecharger(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<()> {
    telecharger_avec_avancement(agent, url, dest, |_, _| {})
}

/// Taille des lots de copie pendant le téléchargement — assez gros pour ne
/// pas multiplier les appels à `avancer`, assez petit pour qu'un avancement
/// affiché reste vivant sur un fichier de plusieurs gigaoctets.
const TAILLE_LOT_TELECHARGEMENT: usize = 4 * 1024 * 1024;

/// Comme [`telecharger`], en rapportant l'avancement en octets déjà écrits —
/// `avancer(vus, total)`, `total` étant `None` si le serveur n'a pas annoncé
/// de `Content-Length` (ne devrait pas arriver pour un fichier statique, mais
/// mieux vaut ne rien y supposer côté appelant).
pub fn telecharger_avec_avancement(
    agent: &ureq::Agent,
    url: &str,
    dest: &Path,
    avancer: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    telecharger_avec_jeton(agent, url, None, dest, avancer)
}

/// [`telecharger_avec_avancement`] avec un jeton d'accès (`Authorization:
/// Bearer`), pour un dépôt Hugging Face à accès restreint. ureq ne renvoie pas
/// l'en-tête à l'hôte d'une redirection (le CDN reçoit une URL déjà signée).
pub fn telecharger_avec_jeton(
    agent: &ureq::Agent,
    url: &str,
    jeton: Option<&str>,
    dest: &Path,
    mut avancer: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let mut requete = agent.get(url);
    if let Some(j) = jeton {
        requete = requete.header("Authorization", format!("Bearer {j}"));
    }
    let mut r = requete.call().map_err(|e| match e {
        ureq::Error::StatusCode(401 | 403) => Error::AccesRefuse(format!(
            "téléchargement {url} : accès refusé — conditions d'usage non acceptées sur Hugging Face, ou jeton absent ou invalide"
        )),
        e => Error::Reseau(format!("téléchargement {url} : {e}")),
    })?;
    let total = r
        .headers()
        .get(ureq::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let mut fichier = File::create(dest)?;
    let mut lecteur = r.body_mut().as_reader();
    let mut tampon = vec![0u8; TAILLE_LOT_TELECHARGEMENT];
    let mut vus = 0u64;
    loop {
        let n = lecteur.read(&mut tampon)?;
        if n == 0 {
            break;
        }
        fichier.write_all(&tampon[..n])?;
        vus += n as u64;
        avancer(vus, total);
    }
    Ok(())
}
