// SPDX-License-Identifier: GPL-3.0-or-later
//! La passe de biographies : interroge TheAudioDB, remplit la base.
//!
//! Séparée de [`crate::theaudiodb`] (qui ne fait que parler au réseau) et de
//! [`crate::db`] (qui ne fait que ranger) — même discipline que
//! [`crate::popularite`].
//!
//! **Additive et best-effort.** Une bibliothèque qui n'a jamais vu le réseau
//! reste utilisable — l'inspecteur affiche « — », rien de plus.
//!
//! **Reprenable.** Chaque MBID interrogé est marqué dans la même transaction
//! que sa donnée ([`crate::db::Library::theaudiodb_poser`]). Un échec réseau
//! n'interrompt pas la passe : l'artiste fautif n'est pas marqué et revient au
//! prochain passage.

use crate::db::{BioBrute, Library};
use crate::error::Result;
use crate::theaudiodb;

/// Ce qu'une passe a produit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bilan {
    pub interroges: usize,
    pub trouves: usize,
    pub faits: usize,
    pub total: usize,
}

/// Interroge TheAudioDB pour au plus `limite` artistes.
///
/// `depuis` : instant (epoch s) à partir duquel un artiste déjà interrogé
/// compte comme frais — `0` ne rafraîchit rien. `avancer` est rappelé
/// régulièrement, pour qu'une interface montre où on en est.
pub fn actualiser(
    lib: &mut Library,
    client: &theaudiodb::Client,
    depuis: i64,
    limite: usize,
    mut avancer: impl FnMut(&Bilan),
) -> Result<Bilan> {
    let mut bilan = Bilan::default();
    let a_faire = lib.theaudiodb_candidats(depuis, limite)?;
    bilan.total = a_faire.len();
    avancer(&bilan);

    for mbid in a_faire {
        match client.artiste_par_mbid(&mbid) {
            Ok(Some(a)) => {
                let brute = [BioBrute {
                    mb_artist_id: mbid.clone(),
                    id_theaudiodb: Some(a.id_theaudiodb),
                    biographie_en: a.biographie_en,
                    biographie_fr: a.biographie_fr,
                }];
                lib.theaudiodb_poser(std::slice::from_ref(&mbid), &brute)?;
                bilan.trouves += 1;
            }
            Ok(None) => lib.theaudiodb_poser(std::slice::from_ref(&mbid), &[])?,
            Err(e) => {
                // Pas marqué : l'artiste reviendra au prochain passage.
                tracing::warn!(erreur = %e, %mbid, "biographie non interrogée");
                bilan.interroges += 1;
                bilan.faits += 1;
                avancer(&bilan);
                continue;
            }
        }
        bilan.interroges += 1;
        bilan.faits += 1;
        avancer(&bilan);
    }
    Ok(bilan)
}
