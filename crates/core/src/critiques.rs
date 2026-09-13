// SPDX-License-Identifier: GPL-3.0-or-later
//! La passe de critiques : interroge CritiqueBrainz, remplit la base.
//!
//! Séparée de [`crate::critiquebrainz`] (qui ne fait que parler au réseau) et
//! de [`crate::db`] (qui ne fait que ranger) — même discipline que
//! [`crate::popularite`].
//!
//! **Additive et best-effort.** La plupart des albums n'ont aucune critique —
//! c'est un état normal, pas une erreur : marqué dans `critiques_fetched`
//! comme pour les autres sources, pour ne pas repasser dessus à chaque fois.

use crate::critiquebrainz;
use crate::db::{CritiqueBrute, Library};
use crate::error::Result;

/// Ce qu'une passe a produit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bilan {
    pub albums_interroges: usize,
    pub albums_avec_critique: usize,
    pub faits: usize,
    pub total: usize,
}

/// Lien de sortie vers la page CritiqueBrainz de l'album — invite à en écrire
/// une quand la liste est vide, plutôt qu'un vide silencieux.
pub fn url_ecrire_critique(mbid_rg: &str) -> String {
    format!("https://critiquebrainz.org/release-group/{mbid_rg}")
}

/// Interroge CritiqueBrainz pour au plus `limite` release-groups.
///
/// `depuis` : instant (epoch s) à partir duquel un release-group déjà
/// interrogé compte comme frais — une critique neuve reste rare, la fenêtre
/// par défaut est donc plus longue que pour la popularité.
///
/// `critiques_candidats` porte sur `mb_release_groups`, qui couvre toute la
/// discographie de chaque artiste connu — bien plus que ce qu'on possède
/// réellement. Sans le filtre par [`Library::release_groups_possedes`], la
/// passe interrogeait CritiqueBrainz (une requête toutes les 500 ms, par
/// courtoisie) pour des albums jamais affichés : sur une bibliothèque de
/// 27 000 morceaux / 2 700 albums mais 83 000 release-groups connus, la
/// passe complète prenait alors près de 9 h au lieu d'une vingtaine de
/// minutes. `limite` s'applique après ce filtre — il compte des albums
/// réellement interrogés, pas des lignes de `mb_release_groups` parcourues.
pub fn actualiser(
    lib: &mut Library,
    client: &critiquebrainz::Client,
    depuis: i64,
    limite: usize,
    mut avancer: impl FnMut(&Bilan),
) -> Result<Bilan> {
    let mut bilan = Bilan::default();
    let possedes = lib.release_groups_possedes()?;
    let a_faire: Vec<String> = lib
        .critiques_candidats(depuis, usize::MAX)?
        .into_iter()
        .filter(|mbid| possedes.contains(mbid))
        .take(limite)
        .collect();
    bilan.total = a_faire.len();
    avancer(&bilan);

    for mbid_rg in a_faire {
        match client.avis_pour_release_group(&mbid_rg) {
            Ok(avis) => {
                if !avis.is_empty() {
                    bilan.albums_avec_critique += 1;
                }
                let brutes: Vec<CritiqueBrute> = avis
                    .into_iter()
                    .map(|c| CritiqueBrute {
                        id: c.id,
                        auteur: c.auteur,
                        licence_id: c.licence_id,
                        licence_nom: c.licence_nom,
                        langue: c.langue,
                        texte: c.texte,
                        url_originale: Some(c.url_originale),
                    })
                    .collect();
                lib.critiques_poser(&mbid_rg, &brutes)?;
            }
            Err(e) => {
                // Pas marqué : l'album reviendra au prochain passage.
                tracing::warn!(erreur = %e, %mbid_rg, "critiques non interrogées");
                bilan.albums_interroges += 1;
                bilan.faits += 1;
                avancer(&bilan);
                continue;
            }
        }
        bilan.albums_interroges += 1;
        bilan.faits += 1;
        avancer(&bilan);
    }
    Ok(bilan)
}
