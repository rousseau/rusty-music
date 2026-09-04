// SPDX-License-Identifier: GPL-3.0-or-later
//! La passe de popularité générale : interroge ListenBrainz et Deezer, remplit
//! la base.
//!
//! Séparée de [`crate::listenbrainz`] / [`crate::deezer`] (qui ne font que
//! parler au réseau) et de [`crate::db`] (qui ne fait que ranger). Comme
//! [`crate::enrichir`], elle est la seule à connaître l'ordre des opérations.
//!
//! **Additive.** Une bibliothèque qui n'a jamais vu le réseau reste
//! utilisable — la jauge de popularité affiche « — », rien de plus.
//!
//! **Reprenable.** Chaque entité interrogée est marquée en base dans la même
//! transaction que sa donnée ([`Library::pop_poser`]). Une passe coupée ne
//! refait rien et ne perd rien. Un échec réseau n'interrompt pas la passe :
//! l'entité fautive n'est pas marquée et revient au prochain passage.

use crate::db::{Library, PopulariteBrute};
use crate::deezer;
use crate::error::Result;
use crate::listenbrainz;

/// MBID par requête POST ListenBrainz — même taille que la sonde de phase 0.
const LOT_LB: usize = 60;

/// Ce qu'une passe a produit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bilan {
    /// Enregistrements interrogés sur ListenBrainz.
    pub lb_enregistrements: usize,
    /// Release-groups interrogés sur ListenBrainz.
    pub lb_albums: usize,
    /// Pistes cherchées sur Deezer.
    pub deezer: usize,
    /// Pistes retrouvées sur Deezer (artiste + titre concordants).
    pub deezer_trouves: usize,
    /// Morceaux ayant une popularité après le recalcul final.
    pub couverts: usize,
    /// Étapes réseau faites / à faire — pour la jauge d'avancement.
    pub faits: usize,
    pub total: usize,
}

/// Interroge les deux sources pour au plus `limite` entités par échelon, puis
/// recalcule `track_popularite`.
///
/// `depuis` est l'instant (epoch s) à partir duquel une entité déjà
/// interrogée compte comme fraîche : `0` ne rafraîchit rien (on ne comble que
/// les trous), `now − 90 j` réinterroge le périmé.
///
/// `avancer` est rappelé régulièrement, pour qu'une interface montre où on en
/// est : la passe dure quelques minutes sur une bibliothèque de vingt-sept
/// mille morceaux.
pub fn actualiser(
    lib: &mut Library,
    lb: &listenbrainz::Client,
    dz: &deezer::Client,
    depuis: i64,
    limite: usize,
    mut avancer: impl FnMut(&Bilan),
) -> Result<Bilan> {
    let mut bilan = Bilan::default();

    let recs = lib.pop_recordings_candidats(depuis, limite)?;
    let rgs = lib.pop_rg_candidats(depuis, limite)?;
    let lb_faits = lib.pop_deja_fait("listenbrainz", "recording", depuis)?;
    let dz_faits = lib.pop_deja_fait("deezer", "recording", depuis)?;

    let lb_a_faire: Vec<String> = recs
        .iter()
        .map(|p| p.recording_mbid.clone())
        .filter(|m| !lb_faits.contains(m))
        .collect();
    let dz_a_faire: Vec<&crate::db::PisteAPopulariser> = recs
        .iter()
        .filter(|p| !dz_faits.contains(&p.recording_mbid))
        .collect();

    bilan.total = lb_a_faire.len() + rgs.len() + dz_a_faire.len();
    avancer(&bilan);

    // --- ListenBrainz : enregistrements, par lots ------------------------
    for lot in lb_a_faire.chunks(LOT_LB) {
        match lb.popularite_enregistrements(lot) {
            Ok(trouves) => {
                let brutes: Vec<PopulariteBrute> = trouves
                    .iter()
                    .map(|p| PopulariteBrute {
                        mbid: &p.mbid,
                        ecoutes: p.ecoutes,
                        auditeurs: Some(p.auditeurs),
                    })
                    .collect();
                lib.pop_poser("listenbrainz", "recording", lot, &brutes)?;
            }
            Err(e) => tracing::warn!(erreur = %e, "lot d'enregistrements non interrogé"),
        }
        bilan.lb_enregistrements += lot.len();
        bilan.faits += lot.len();
        avancer(&bilan);
    }

    // --- ListenBrainz : release-groups, par lots ------------------------
    for lot in rgs.chunks(LOT_LB) {
        match lb.popularite_albums(lot) {
            Ok(trouves) => {
                let brutes: Vec<PopulariteBrute> = trouves
                    .iter()
                    .map(|p| PopulariteBrute {
                        mbid: &p.mbid,
                        ecoutes: p.ecoutes,
                        auditeurs: Some(p.auditeurs),
                    })
                    .collect();
                lib.pop_poser("listenbrainz", "release-group", lot, &brutes)?;
            }
            Err(e) => tracing::warn!(erreur = %e, "lot de release-groups non interrogé"),
        }
        bilan.lb_albums += lot.len();
        bilan.faits += lot.len();
        avancer(&bilan);
    }

    // --- Deezer : une recherche par piste -----------------------------------
    for p in dz_a_faire {
        let un = std::slice::from_ref(&p.recording_mbid);
        match (p.artiste.as_deref(), p.titre.as_deref()) {
            (Some(artiste), Some(titre)) => match dz.rang_piste(artiste, titre) {
                Ok(Some(rank)) => {
                    let brute = [PopulariteBrute {
                        mbid: &p.recording_mbid,
                        ecoutes: rank,
                        auditeurs: None,
                    }];
                    lib.pop_poser("deezer", "recording", un, &brute)?;
                    bilan.deezer_trouves += 1;
                }
                Ok(None) => lib.pop_poser("deezer", "recording", un, &[])?,
                Err(e) => {
                    // Pas marquée : l'entité reviendra au prochain passage.
                    tracing::warn!(erreur = %e, artiste, titre, "piste Deezer non cherchée");
                    bilan.deezer += 1;
                    bilan.faits += 1;
                    avancer(&bilan);
                    continue;
                }
            },
            // Sans artiste ni titre, rien à chercher — mais on marque, pour ne
            // pas repasser dessus à chaque fois.
            _ => lib.pop_poser("deezer", "recording", un, &[])?,
        }
        bilan.deezer += 1;
        bilan.faits += 1;
        if bilan.deezer % 10 == 0 {
            avancer(&bilan);
        }
    }

    bilan.couverts = lib.recalculer_track_popularite()?;
    avancer(&bilan);
    Ok(bilan)
}
