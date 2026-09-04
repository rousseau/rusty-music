//! Cover Art Archive : la pochette d'un album, par identifiant de release-group.
//!
//! Comme [`crate::musicbrainz`] et [`crate::listenbrainz`], ce module ne fait
//! que parler au réseau. La mise en cache et l'encodage en `data:` URI sont du
//! ressort de l'appelant (côté application, `decouvrir_pochette`).
//!
//! Cover Art Archive répond `307` vers `archive.org` — `ureq` suit la
//! redirection — ou `404` quand l'album n'a pas de pochette, ce qui est
//! fréquent pour une sortie du mois même.

use std::sync::OnceLock;
use std::time::Duration;

use crate::error::{Error, Result};

/// Un agent partagé : inutile de rétablir TLS à chaque vignette.
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .user_agent(concat!("rusty-music/", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into()
    })
}

/// La pochette (face avant, 250 px) d'un release-group, en octets JPEG.
///
/// `None` si l'album n'a pas de pochette connue (`404`). Une panne réseau rend
/// une erreur — l'appelant l'avale, une vignette manquante n'est pas grave.
pub fn release_group(rg_mbid: &str) -> Result<Option<Vec<u8>>> {
    let url = format!("https://coverartarchive.org/release-group/{rg_mbid}/front-250");
    match agent().get(&url).call() {
        Ok(mut r) => r
            .body_mut()
            .read_to_vec()
            .map(Some)
            .map_err(|e| Error::Reseau(format!("lecture de la pochette : {e}"))),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(Error::Reseau(format!("Cover Art Archive : {e}"))),
    }
}
