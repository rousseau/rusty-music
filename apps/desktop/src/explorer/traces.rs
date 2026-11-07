// SPDX-License-Identifier: GPL-3.0-or-later
//! Traces et brouillard (`docs/plan-balade.md`, P5) : ce que la carte montre
//! de l'écoute passée.
//!
//! - **Brouillard** : les morceaux jamais écoutés restent voilés. L'interface
//!   reçoit la liste des écoutés ([`morceaux_ecoutes`]), puis chaque écoute
//!   nouvelle par l'évènement `ecoute-notee` (`ecouter::suivre_ecoute`), et
//!   lève le voile par `feature-state` — sans toucher aux tuiles
//!   (`docs/carto-ville.md`, « Le brouillard »).
//! - **Sentiers battus** : les arêtes du graphe de voirie parcourues en
//!   balade, avec leur nombre de passages ([`sentiers_carte`]).
//! - **Balades passées** : la liste ([`balades_passees`]), le tracé de l'une
//!   ([`balade_passee`]), et sa relecture ([`balade_rejouer`]).

use super::*;

use rusty_music_core::db::BaladePassee;

/// Balades passées listées dans le panneau de la carte.
const BALADES_LISTEES: usize = 12;

/// Les morceaux écoutés au moins une fois : tout le reste est sous le brouillard.
#[tauri::command(async)]
pub(crate) fn morceaux_ecoutes(etat: State<Etat>) -> Result<Vec<i64>, String> {
    verrou(&etat.lib).morceaux_ecoutes().map_err(echec)
}

/// Le graphe de voirie, s'il y a une ville importée.
fn graphe_ville(
    etat: &State<Etat>,
) -> Result<Option<Arc<rusty_music_carto::reseau_reel::Graphe>>, String> {
    let Some(chemin) = plan_de_ville(&etat.db) else {
        return Ok(None);
    };
    let extrait = charger_ville(etat, &chemin)?;
    charger_graphe_reel(etat, &extrait).map(Some)
}

/// Une arête `(a, b)` du graphe d'aujourd'hui : un plan de ville réimporté
/// renumérote ses sommets, et les chemins gardés avant n'y ont plus de sens.
fn arete_valide(graphe: &rusty_music_carto::reseau_reel::Graphe, a: u32, b: u32) -> bool {
    let n = graphe.taille().0 as u32;
    a < n && b < n && graphe.voisins(a).iter().any(|&(w, _)| w == b)
}

/// Les sentiers battus en GeoJSON : une ligne par arête parcourue en balade,
/// `passages` en attribut. Vide sans ville importée.
#[tauri::command(async)]
pub(crate) fn sentiers_carte(etat: State<Etat>) -> Result<serde_json::Value, String> {
    let passages = verrou(&etat.lib).passages_sentiers().map_err(echec)?;
    let mut entites = Vec::new();
    if let Some(graphe) = graphe_ville(&etat)? {
        for ((a, b), n) in passages {
            if !arete_valide(&graphe, a, b) {
                continue;
            }
            entites.push(serde_json::json!({
                "type": "Feature",
                "properties": { "passages": n },
                "geometry": {
                    "type": "LineString",
                    "coordinates": [graphe.point(a), graphe.point(b)],
                }
            }));
        }
    }
    Ok(serde_json::json!({ "type": "FeatureCollection", "features": entites }))
}

/// Les balades passées, la plus récente d'abord.
#[tauri::command(async)]
pub(crate) fn balades_passees(etat: State<Etat>) -> Result<Vec<BaladePassee>, String> {
    verrou(&etat.lib)
        .balades_passees(BALADES_LISTEES)
        .map_err(echec)
}

/// Une balade passée à redessiner : ses morceaux dans l'ordre et, dans la
/// ville, le tracé de ses pas le long des rues, `[lon, lat]`.
#[derive(serde::Serialize)]
pub(crate) struct BaladeDessin {
    pistes: Vec<i64>,
    trace: Vec<[f64; 2]>,
}

#[tauri::command(async)]
pub(crate) fn balade_passee(etat: State<Etat>, id: i64) -> Result<BaladeDessin, String> {
    let pas = verrou(&etat.lib).pas_de_balade(id).map_err(echec)?;
    if pas.is_empty() {
        return Err("balade inconnue".into());
    }
    let mut trace = Vec::new();
    if pas.iter().any(|p| !p.chemin.is_empty()) {
        if let Some(graphe) = graphe_ville(&etat)? {
            for p in &pas {
                let valide = p
                    .chemin
                    .windows(2)
                    .all(|w| arete_valide(&graphe, w[0], w[1]));
                if valide {
                    trace.extend(p.chemin.iter().map(|&s| graphe.point(s)));
                }
            }
        }
    }
    Ok(BaladeDessin {
        pistes: pas.iter().map(|p| p.morceau).collect(),
        trace,
    })
}

/// Rejoue une balade passée : ses morceaux, dans l'ordre, comme une file
/// ordinaire. Ce n'est plus une balade — rien n'y est choisi.
#[tauri::command(async)]
pub(crate) fn balade_rejouer(etat: State<Etat>, id: i64) -> Result<(), String> {
    let chemins: Vec<PathBuf> = {
        let lib = verrou(&etat.lib);
        let pas = lib.pas_de_balade(id).map_err(echec)?;
        pas.iter()
            .filter_map(|p| lib.track(p.morceau).ok().flatten())
            .map(|t| PathBuf::from(t.path))
            .collect()
    };
    if chemins.is_empty() {
        return Err("aucun morceau de cette balade n'est dans la bibliothèque".into());
    }
    verrou(&etat.player).play(&chemins).map_err(echec)
}
