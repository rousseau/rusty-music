// SPDX-License-Identifier: GPL-3.0-or-later
//! La passe Last.fm : interroge `artist.getTopTags`, remplit la base.
//!
//! Séparée de [`crate::lastfm`] (qui ne fait que parler au réseau) et de
//! [`crate::db`] (qui ne fait que ranger) — même discipline que
//! [`crate::critiques`]/[`crate::popularite`].
//!
//! **Additive et best-effort.** Un artiste sans tag Last.fm est un état
//! normal, marqué dans `lastfm_fetched` comme pour les autres sources, pour
//! ne pas repasser dessus à chaque fois.

use crate::db::Library;
use crate::error::Result;
use crate::lastfm;

/// Ce qu'une passe a produit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bilan {
    pub artistes_interroges: usize,
    pub artistes_avec_tags: usize,
    /// Artistes qu'un échec ponctuel (5xx, réseau) a laissés de côté — ils
    /// reviendront au prochain passage. Un échec bloquant (clé refusée)
    /// n'est pas compté ici : il fait remonter une erreur, la passe s'arrête.
    pub echecs: usize,
    pub faits: usize,
    pub total: usize,
}

/// Interroge Last.fm pour au plus `limite` artistes.
///
/// `depuis` : instant (epoch s) à partir duquel un artiste déjà interrogé
/// compte comme frais.
pub fn actualiser(
    lib: &mut Library,
    client: &lastfm::Client,
    depuis: i64,
    limite: usize,
    mut avancer: impl FnMut(&Bilan),
) -> Result<Bilan> {
    let mut bilan = Bilan::default();
    let a_faire = lib.lastfm_candidats(depuis, limite)?;
    bilan.total = a_faire.len();
    avancer(&bilan);

    for mbid in a_faire {
        match client.tags_artiste(&mbid) {
            Ok(tags) => {
                if !tags.is_empty() {
                    bilan.artistes_avec_tags += 1;
                }
                lib.lastfm_tags_poser(&mbid, &tags)?;
            }
            Err(lastfm::EchecTags::Bloquant(e)) => {
                // Clé refusée, suspendue, débit dépassé : réessayer un autre
                // artiste échouerait à l'identique. On arrête là et on
                // remonte l'erreur — sans quoi le bilan afficherait « 0 tag »,
                // indiscernable d'une bibliothèque dont personne n'a de tag.
                return Err(e);
            }
            Err(lastfm::EchecTags::Ponctuel(e)) => {
                // Pas marqué : l'artiste reviendra au prochain passage.
                tracing::warn!(erreur = %e, %mbid, "tags Last.fm non interrogés");
                bilan.artistes_interroges += 1;
                bilan.echecs += 1;
                bilan.faits += 1;
                avancer(&bilan);
                continue;
            }
        }
        bilan.artistes_interroges += 1;
        bilan.faits += 1;
        avancer(&bilan);
    }
    Ok(bilan)
}
