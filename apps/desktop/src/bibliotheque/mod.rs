// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de la Bibliothèque. Ici, la consultation (artistes, albums,
//! recherche, statistiques, descripteurs et popularité d'un morceau) ; un
//! module par domaine pour le reste : le scan et le dossier surveillé
//! (`scan`), les passes locales (`analyse`), les vérifications
//! (`verifications`), l'enrichissement en ligne (`enrichissement`), Discogs
//! (`discogs`), les pochettes et leur cache (`pochettes`).

use super::*;

mod analyse;
mod discogs;
mod enrichissement;
mod pochettes;
mod scan;
mod verifications;

pub(crate) use analyse::*;
pub(crate) use discogs::*;
pub(crate) use enrichissement::*;
pub(crate) use pochettes::*;
pub(crate) use scan::*;
pub(crate) use verifications::*;

#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn artists(etat: State<Etat>) -> Result<Vec<ArtistRow>, String> {
    let r = verrou(&etat.lib).artists().map_err(echec);
    tracing::info!(n = r.as_ref().map(Vec::len).unwrap_or(0), "artists");
    r
}

#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn albums(
    etat: State<Etat>,
    artist: Option<String>,
    mbid: Option<String>,
) -> Result<Vec<AlbumRow>, String> {
    let lib = verrou(&etat.lib);
    // Identifiant et nom sont passés ensemble : le regroupement réunit les
    // pistes étiquetées MusicBrainz et celles qui ne le sont pas, l'ouverture
    // de l'artiste doit faire de même.
    match artist.as_deref() {
        Some(nom) => lib.albums_of_artist(mbid.as_deref(), nom).map_err(echec),
        None => lib.albums(None).map_err(echec),
    }
}

#[tauri::command(async)]
pub(crate) fn tracks_of_album(
    etat: State<Etat>,
    album: String,
    artist: Option<String>,
) -> Result<Vec<TrackRow>, String> {
    tracing::info!(%album, "tracks_of_album");
    verrou(&etat.lib)
        .tracks_of_album(&album, artist.as_deref())
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn search(
    etat: State<Etat>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<TrackRow>, String> {
    verrou(&etat.lib)
        .search(&query, limit.unwrap_or(50))
        .map_err(echec)
}

#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn roots(etat: State<Etat>) -> Result<Vec<RootRow>, String> {
    verrou(&etat.lib).roots().map_err(echec)
}

/// Statistiques d'ensemble du mode Bibliothèque — un aperçu, pas une passe :
/// trois requêtes d'agrégation sur la base déjà là, rien à mesurer ni à
/// décoder. Rejouée à chaque entrée dans le mode plutôt que mise en cache :
/// à 27 000 morceaux, l'agrégation reste sous la seconde.
#[cfg_attr(test, derive(specta::Type))]
#[derive(Clone, serde::Serialize)]
pub(crate) struct StatsBibliotheque {
    total: i64,
    genres: Vec<(String, i64)>,
    tempo: rusty_music_core::db::Histogramme,
    durees: rusty_music_core::db::Histogramme,
    codecs: Vec<(String, i64)>,
    bitrate: rusty_music_core::db::Histogramme,
    sans_mbid: i64,
    humeur: Vec<(String, i64)>,
    sans_bio: i64,
    sans_critique: i64,
    sans_discogs: i64,
}

#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn library_stats(etat: State<Etat>) -> Result<StatsBibliotheque, String> {
    let lib = verrou(&etat.lib);
    Ok(StatsBibliotheque {
        total: lib.count().map_err(echec)?,
        genres: lib.stats_genres().map_err(echec)?,
        tempo: lib.stats_tempo().map_err(echec)?,
        durees: lib.stats_durees().map_err(echec)?,
        codecs: lib.stats_codecs().map_err(echec)?,
        bitrate: lib.stats_bitrate().map_err(echec)?,
        sans_mbid: lib.stats_sans_mbid().map_err(echec)?,
        humeur: lib.stats_humeur().map_err(echec)?,
        sans_bio: lib.stats_sans_bio().map_err(echec)?,
        sans_critique: lib.stats_sans_critique().map_err(echec)?,
        sans_discogs: lib.stats_sans_discogs().map_err(echec)?,
    })
}

/// Tempo, tonalité et énergie d'un morceau — ce que la passe a mesuré.
///
/// **Rend `None` plutôt qu'une valeur par défaut** quand le morceau n'est pas
/// mesuré : 15 847 des 27 044 le sont à ce jour, et afficher « 120 BPM » sur
/// les autres serait donner une mesure qu'on n'a pas.
#[tauri::command(async)]
pub(crate) fn descripteurs(
    etat: State<Etat>,
    id: i64,
) -> Result<Option<rusty_music_core::db::DescripteursVus>, String> {
    verrou(&etat.lib).descripteurs(id).map_err(echec)
}

/// La popularité générale des morceaux `ids` — `(track_id, relative 0..1,
/// echelon)`. Un morceau absent de la réponse n'en a pas (jauge grisée).
/// Chargée par lot pour ce qui est visible (file d'attente, liste de pistes),
/// comme les pochettes — jamais dans `TrackRow`.
#[tauri::command(async)]
pub(crate) fn popularites(
    etat: State<Etat>,
    ids: Vec<i64>,
) -> Result<Vec<(i64, f64, String)>, String> {
    verrou(&etat.lib).popularites(&ids).map_err(echec)
}

/// Qualité d'encodage du morceau en écoute — codec, débit, échantillonnage,
/// profondeur de bits. Affichée sous le compteur de temps du transport.
/// Champs partiellement `None` sur un morceau scanné avant la lecture du
/// format (un rescan les remplit).
#[tauri::command(async)]
pub(crate) fn qualite_piste(
    etat: State<Etat>,
    id: i64,
) -> Result<Option<rusty_music_core::db::QualitePiste>, String> {
    verrou(&etat.lib).qualite_piste(id).map_err(echec)
}
