// SPDX-License-Identifier: GPL-3.0-or-later
//! Des lieux à soi (`docs/plan-balade.md`, P8) : les épingles et les chaînes
//! posées sur la carte (`rusty_music_core::lieux`). L'interface les dessine
//! sur la surcouche de la carte, au-dessus des tuiles : elles ne dépendent ni
//! du style ni des tuiles, et ne bougent pas quand on les régénère.

use super::*;

use rusty_music_core::lieux::{Chaine, Epingle};

#[tauri::command(async)]
pub(crate) fn epingles(etat: State<Etat>) -> Result<Vec<Epingle>, String> {
    verrou(&etat.lib).epingles().map_err(echec)
}

/// « Épingler ici » : rend l'épingle posée.
#[tauri::command(async)]
pub(crate) fn creer_epingle(
    etat: State<Etat>,
    nom: String,
    couleur: String,
    emoji: Option<String>,
    lon: f64,
    lat: f64,
) -> Result<Epingle, String> {
    if !(lon.is_finite() && lat.is_finite()) {
        return Err("position invalide".into());
    }
    verrou(&etat.lib)
        .creer_epingle(&nom, &couleur, emoji.as_deref(), lon, lat)
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn supprimer_epingle(etat: State<Etat>, id: i64) -> Result<bool, String> {
    verrou(&etat.lib).supprimer_epingle(id).map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn chaines(etat: State<Etat>) -> Result<Vec<Chaine>, String> {
    verrou(&etat.lib).chaines().map_err(echec)
}

/// Garde une balade passée ou un itinéraire : ses morceaux deviennent une
/// playlist (par chemin), son tracé `[lon, lat]` est copié tel quel. Rend
/// l'identifiant de la playlist.
#[tauri::command(async)]
pub(crate) fn garder_chaine(
    etat: State<Etat>,
    nom: String,
    couleur: String,
    origine: String,
    morceaux: Vec<i64>,
    trace: Vec<[f64; 2]>,
) -> Result<i64, String> {
    let lib = verrou(&etat.lib);
    let chemins: Vec<String> = morceaux
        .iter()
        .filter_map(|&id| lib.track(id).ok().flatten().map(|t| t.path))
        .collect();
    if chemins.is_empty() {
        return Err("aucun morceau à garder".into());
    }
    let trace: Vec<[f64; 2]> = trace
        .into_iter()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
        .collect();
    lib.garder_chaine(&nom, &couleur, &origine, &chemins, &trace)
        .map_err(echec)
}

/// Retire le tracé d'une chaîne de la carte ; sa playlist reste.
#[tauri::command(async)]
pub(crate) fn oublier_trace(etat: State<Etat>, id: i64) -> Result<bool, String> {
    verrou(&etat.lib).oublier_trace(id).map_err(echec)
}
