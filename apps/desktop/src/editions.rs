// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes du choix d'édition MusicBrainz, face à face
//! (`docs/plan-ecouter-v0.3.md`, point 3) : les éditions proposées pour
//! l'album ouvert, la comparaison piste à piste avec l'une d'elles, retenir et
//! oublier. Seuls le titre et l'artiste de l'album partent sur le réseau
//! (avec l'adresse de contact, que MusicBrainz exige) ; rien n'est écrit dans
//! les fichiers.

use super::*;
use rusty_music_core::editions::{apparier, EditionDetaillee, EditionProposee, Paire, PisteLocale};

fn client(contact: &str) -> Result<rusty_music_core::musicbrainz::Client, String> {
    let contact = contact.trim();
    if !contact.contains('@') {
        return Err("MusicBrainz demande une adresse de contact valable.".into());
    }
    Ok(rusty_music_core::musicbrainz::Client::new(contact))
}

/// Les pistes de l'album local, dans l'ordre de l'album. Le verrou de la base
/// est rendu avant tout appel réseau.
fn pistes_locales(etat: &Etat, album: &str, artist: Option<&str>) -> Result<Vec<TrackRow>, String> {
    verrou(&etat.lib)
        .tracks_of_album(album, artist)
        .map_err(echec)
}

/// Les éditions de l'album chez MusicBrainz, classées par concordance du
/// nombre de pistes avec l'album local.
#[tauri::command(async)]
pub(crate) fn editions_proposees(
    etat: State<Etat>,
    contact: String,
    album: String,
    artist: Option<String>,
) -> Result<Vec<EditionProposee>, String> {
    let n = pistes_locales(&etat, &album, artist.as_deref())?.len() as u32;
    client(&contact)?
        .chercher_editions(&album, artist.as_deref().unwrap_or_default(), n)
        .map_err(echec)
}

/// Une édition face à l'album local : ses pistes, celles de l'album, et leur
/// alignement rang à rang.
#[derive(serde::Serialize)]
pub(crate) struct Comparaison {
    edition: EditionDetaillee,
    locales: Vec<TrackRow>,
    paires: Vec<Paire>,
}

fn comparer(
    etat: &Etat,
    contact: &str,
    id: &str,
    album: &str,
    artist: Option<&str>,
) -> Result<Comparaison, String> {
    let locales = pistes_locales(etat, album, artist)?;
    let edition = client(contact)?
        .edition(id)
        .map_err(echec)?
        .ok_or("édition introuvable chez MusicBrainz")?;
    let vues: Vec<PisteLocale> = locales
        .iter()
        .map(|t| PisteLocale {
            titre: t.title.clone().unwrap_or_default(),
            duree_ms: t.duration_ms,
        })
        .collect();
    let paires = apparier(&vues, &edition.pistes);
    Ok(Comparaison {
        edition,
        locales,
        paires,
    })
}

#[tauri::command(async)]
pub(crate) fn edition_comparee(
    etat: State<Etat>,
    contact: String,
    id: String,
    album: String,
    artist: Option<String>,
) -> Result<Comparaison, String> {
    comparer(&etat, &contact, &id, &album, artist.as_deref())
}

/// Retient l'édition `id` pour l'album : chaque piste appariée reçoit les
/// identifiants de la sienne. La comparaison est refaite ici plutôt que reçue
/// de l'interface : ce qui s'écrit est ce que MusicBrainz dit à cet instant.
#[tauri::command(async)]
pub(crate) fn retenir_edition(
    etat: State<Etat>,
    contact: String,
    id: String,
    album: String,
    artist: Option<String>,
) -> Result<(), String> {
    let c = comparer(&etat, &contact, &id, &album, artist.as_deref())?;
    let appariement: Vec<(PathBuf, usize)> = c
        .paires
        .iter()
        .filter_map(|p| Some((PathBuf::from(&c.locales[p.locale?].path), p.edition?)))
        .collect();
    tracing::info!(%album, edition = %id, pistes = appariement.len(), "édition retenue");
    verrou(&etat.lib)
        .retenir_edition(
            &album,
            artist.as_deref().unwrap_or_default(),
            &c.edition,
            &appariement,
        )
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn edition_retenue(
    etat: State<Etat>,
    album: String,
    artist: Option<String>,
) -> Result<Option<rusty_music_core::db::EditionRetenue>, String> {
    verrou(&etat.lib)
        .edition_retenue(&album, artist.as_deref().unwrap_or_default())
        .map_err(echec)
}

/// Oublie le choix : les pistes reprennent les identifiants de leurs tags,
/// relus ici. Rend le nombre de pistes relues.
#[tauri::command(async)]
pub(crate) fn oublier_edition(
    etat: State<Etat>,
    album: String,
    artist: Option<String>,
) -> Result<usize, String> {
    let lib = &mut *verrou(&etat.lib);
    let chemins = lib
        .oublier_edition(&album, artist.as_deref().unwrap_or_default())
        .map_err(echec)?;
    let mut relues = 0;
    for chemin in &chemins {
        match rusty_music_core::tags::read(chemin) {
            Ok(meta) => {
                lib.upsert(&meta).map_err(echec)?;
                relues += 1;
            }
            Err(e) => tracing::warn!(chemin = %chemin.display(), "tags illisibles : {e}"),
        }
    }
    Ok(relues)
}
