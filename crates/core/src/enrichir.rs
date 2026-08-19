//! La passe d'enrichissement : interroge MusicBrainz, remplit la base.
//!
//! Séparée de [`crate::musicbrainz`], qui ne fait que parler au réseau, et de
//! [`crate::db`], qui ne fait que ranger. Ici vit la seule chose qui relie les
//! deux : l'ordre des opérations, et ce qu'on fait des trous.
//!
//! **Additive par construction.** Une bibliothèque qui n'a jamais vu le réseau
//! reste entièrement utilisable — les familles sont alors nommées par les tags
//! des fichiers, comme avant. L'enrichissement précise, il ne conditionne rien.
//!
//! **Reprend où elle s'est arrêtée.** Chaque artiste traité est marqué en base
//! dans la même transaction que ses données. Une passe coupée au milieu ne
//! refait rien et ne perd rien.

use crate::db::{AlbumRange, Library};
use crate::error::Result;
use crate::musicbrainz::{normaliser_titre, Client};

/// Ce qu'une passe a produit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Bilan {
    /// Artistes interrogés, y compris ceux qui n'ont rien rendu.
    pub artistes: usize,
    /// Artistes ayant au moins un genre.
    pub avec_genre: usize,
    /// Albums découverts, tous artistes confondus.
    pub albums: usize,
    /// Artistes abandonnés après épuisement des tentatives.
    pub echecs: usize,
}

/// Interroge MusicBrainz pour au plus `limite` artistes.
///
/// Deux requêtes par artiste : ses genres, puis le parcours de ses disques
/// avec leurs genres. Le parcours rend jusqu'à cent albums d'un coup — c'est
/// ce qui rend l'échelon album abordable, là où interroger chaque disque
/// séparément demanderait deux appels par disque.
///
/// `avancer` est appelé après chaque artiste, pour qu'une interface puisse
/// montrer où on en est : la passe dure une heure sur une bibliothèque de
/// vingt-sept mille morceaux, et se taire une heure serait indistinguable
/// d'un blocage.
///
/// **Un échec réseau n'interrompt pas la passe.** MusicBrainz coupe, ralentit,
/// rend des 503 ; abandonner tout au premier accroc perdrait le travail des
/// artistes déjà faits. L'artiste fautif n'est pas marqué et reviendra au
/// prochain passage.
pub fn enrichir(
    lib: &mut Library,
    client: &Client,
    limite: usize,
    mut avancer: impl FnMut(&Bilan),
) -> Result<Bilan> {
    let mut bilan = Bilan::default();
    let a_faire = lib.mb_artistes_en_attente("artist", limite)?;

    for cle in a_faire {
        // Un identifiant sur mille en porte plusieurs, séparés par des barres
        // — une piste « X feat. Y ». On interroge le premier, mais on range
        // sous la clé entière : c'est elle que portent les morceaux.
        let interroge = cle.split('/').next().unwrap_or(&cle).to_string();

        let genres = match client.genres_artiste(&interroge) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(artiste = %cle, erreur = %e, "artiste non interrogé");
                bilan.echecs += 1;
                avancer(&bilan);
                continue;
            }
        };
        let paires: Vec<(String, i64)> = genres.iter().map(|g| (g.nom.clone(), g.votes)).collect();
        if !paires.is_empty() {
            bilan.avec_genre += 1;
        }
        lib.mb_poser_genres(&cle, "artist", &paires)?;

        // Les albums ne bloquent pas : leur absence fait simplement retomber le
        // morceau sur les genres de son artiste, ce qui reste juste.
        match client.albums_artiste(&interroge) {
            Ok(albums) => {
                let range: Vec<AlbumRange> = albums
                    .iter()
                    .map(|a| {
                        (
                            a.mbid.clone(),
                            a.titre.clone(),
                            normaliser_titre(&a.titre),
                            a.genres.iter().map(|g| (g.nom.clone(), g.votes)).collect(),
                        )
                    })
                    .collect();
                bilan.albums += range.len();
                lib.mb_poser_albums(&cle, &range)?;
            }
            Err(e) => tracing::warn!(artiste = %cle, erreur = %e, "albums non parcourus"),
        }

        bilan.artistes += 1;
        avancer(&bilan);
    }
    Ok(bilan)
}
