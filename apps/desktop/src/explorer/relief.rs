// SPDX-License-Identifier: GPL-3.0-or-later
//! Le relief musical du plan réel (`docs/plan-balade.md`, P6) : une altitude
//! qui a un sens — énergie, tempo, popularité ou familiarité —, ses tuiles
//! (`dem-<grandeur>`, `courbes-<grandeur>`, écrites à la demande à côté du
//! plan) et les gestes de ses courbes de niveau : la suivre à la molette
//! ([`courbe`]), l'ouvrir ([`ouvrir_courbe`]).
//!
//! Le champ et ses courbes restent en mémoire pour la grandeur affichée
//! (`Etat::relief`) : les gestes y lisent la courbe désignée sans relire les
//! tuiles. Changer de grandeur réécrit les deux archives de cette grandeur et
//! rien d'autre ; l'interface pointe alors ses deux sources dessus
//! (`setTiles`), sans régénérer le plan.

use super::*;

use rusty_music_carto::altitude::{self, Courbe, Grandeur, Relief};

/// Un morceau borde une courbe à moins de tant de mètres : le rail de la
/// molette.
const BORD_COURBE_M: f64 = 40.0;

/// Le relief de la grandeur affichée, et ce que les gestes en lisent.
pub(crate) struct ReliefCharge {
    grandeur: Grandeur,
    /// Change à chaque écriture des archives : l'interface l'ajoute aux URL
    /// des tuiles, que MapLibre garde sinon en cache.
    version: u64,
    relief: Relief,
    courbes: Vec<Courbe>,
    /// Les morceaux logés, `[lon, lat]`.
    positions: Vec<(i64, [f64; 2])>,
}

/// Les valeurs d'une grandeur, ramenées dans `[0, 1]`
/// ([`altitude::normaliser`]), pour chaque morceau qui en a une. La balade s'en
/// sert aussi : les allures du relief suivent la grandeur affichée.
pub(crate) fn valeurs_grandeur(etat: &Etat, g: Grandeur) -> Result<HashMap<i64, f32>, String> {
    let lib = verrou(&etat.lib);
    let vue = lib
        .map_view(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;
    let brutes: Vec<Option<f32>> = match g {
        Grandeur::Energie => vue.iter().map(|p| p.energy).collect(),
        Grandeur::Tempo => vue.iter().map(|p| p.bpm).collect(),
        Grandeur::Popularite => vue.iter().map(|p| p.popularite.map(|x| x as f32)).collect(),
        Grandeur::Familiarite => {
            let n = lib.nombre_ecoutes().map_err(echec)?;
            vue.iter()
                .map(|p| Some(n.get(&p.id).copied().unwrap_or(0) as f32))
                .collect()
        }
    };
    Ok(vue
        .iter()
        .zip(altitude::normaliser(g, &brutes))
        .filter_map(|(p, v)| v.map(|v| (p.id, v)))
        .collect())
}

/// Ce que l'interface reçoit après [`relief_musical`].
#[derive(serde::Serialize)]
pub(crate) struct ReliefVue {
    grandeur: Grandeur,
    version: u64,
    courbes: usize,
}

fn vue(r: &ReliefCharge) -> ReliefVue {
    ReliefVue {
        grandeur: r.grandeur,
        version: r.version,
        courbes: r.courbes.len(),
    }
}

/// Prépare le relief de `grandeur` : le champ, ses courbes et leurs deux
/// archives. Déjà prêt pour cette grandeur, il est rendu tel quel — sauf la
/// familiarité, refaite à chaque demande : chaque écoute la change.
#[tauri::command(async)]
pub(crate) fn relief_musical(
    app: tauri::AppHandle,
    etat: State<Etat>,
    grandeur: Grandeur,
) -> Result<ReliefVue, String> {
    if let Some(r) = verrou(&etat.relief).as_ref() {
        if r.grandeur == grandeur && grandeur != Grandeur::Familiarite {
            return Ok(vue(r));
        }
    }
    let debut = std::time::Instant::now();
    let dossier = tuiles::dossier(&app).map_err(echec)?;
    if plan_de_ville(&etat.db).is_none() || !dossier.join("positions.json").is_file() {
        return Err("le relief se pose sur le plan de ville réel".into());
    }
    let positions: Vec<(i64, [f64; 2])> = points_de_carte_effectifs(&etat, &app, true)?
        .into_iter()
        .map(|(id, x, y)| (id, [x as f64, y as f64]))
        .collect();
    let valeurs = valeurs_grandeur(&etat, grandeur)?;
    let points: Vec<([f64; 2], f32)> = positions
        .iter()
        .filter_map(|(id, p)| valeurs.get(id).map(|v| (*p, *v)))
        .collect();
    let relief = Relief::calculer(&points).ok_or("aucun morceau n'a cette grandeur")?;
    let courbes = relief.courbes();
    let archives = app.state::<tuiles::Archives>();
    for (genre, ecrire) in [
        (
            "dem",
            &(|c: &Path| relief.ecrire_dem(c)) as &dyn Fn(&Path) -> anyhow::Result<usize>,
        ),
        ("courbes", &|c: &Path| relief.ecrire_courbes(&courbes, c)),
    ] {
        let quoi = format!("{genre}-{}", grandeur.id());
        // Écrire à côté puis remplacer : une tuile demandée pendant
        // l'écriture lit encore l'ancienne archive, entière.
        let chemin = tuiles::chemin_archive(&app, &quoi).map_err(echec)?;
        let provisoire = chemin.with_extension("pmtiles.tmp");
        ecrire(&provisoire).map_err(echec)?;
        archives.oublier_une(&quoi);
        std::fs::rename(&provisoire, &chemin).map_err(echec)?;
    }
    let version = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    tracing::info!(
        grandeur = grandeur.id(),
        morceaux = points.len(),
        courbes = courbes.len(),
        ms = debut.elapsed().as_millis() as u64,
        "relief musical écrit"
    );
    let charge = ReliefCharge {
        grandeur,
        version,
        relief,
        courbes,
        positions,
    };
    let v = vue(&charge);
    *verrou(&etat.relief) = Some(Arc::new(charge));
    Ok(v)
}

fn relief_charge(etat: &Etat) -> Result<Arc<ReliefCharge>, String> {
    verrou(&etat.relief)
        .clone()
        .ok_or_else(|| "aucun relief affiché".into())
}

fn courbe_de(r: &ReliefCharge, ligne: u32) -> Result<&Courbe, String> {
    r.courbes
        .get(ligne as usize)
        .ok_or_else(|| "courbe inconnue (relief changé ?)".into())
}

/// Un morceau qui borde la courbe, à sa place le long d'elle.
#[derive(serde::Serialize)]
pub(crate) struct Riverain {
    morceau: i64,
    /// Position le long de la courbe, mètres.
    a: f64,
    /// Le point de la courbe le plus proche du morceau, `[lon, lat]`.
    point: [f64; 2],
}

#[derive(serde::Serialize)]
pub(crate) struct CourbeVue {
    ligne: u32,
    niveau: u8,
    sommet: bool,
    points: Vec<[f64; 2]>,
    /// Les morceaux qui la bordent, dans l'ordre de la courbe : le rail de la
    /// molette.
    le_long: Vec<Riverain>,
}

/// La courbe `ligne` du relief affiché : son tracé et ses riverains.
#[tauri::command(async)]
pub(crate) fn courbe(etat: State<Etat>, ligne: u32) -> Result<CourbeVue, String> {
    let r = relief_charge(&etat)?;
    let c = courbe_de(&r, ligne)?;
    let le_long = altitude::le_long(c, &r.positions, BORD_COURBE_M)
        .into_iter()
        .map(|(morceau, a, point)| Riverain { morceau, a, point })
        .collect();
    Ok(CourbeVue {
        ligne,
        niveau: c.niveau,
        sommet: c.sommet,
        points: c.points.clone(),
        le_long,
    })
}

/// « Ouvrir » la courbe `ligne` : la playlist de la bande qu'elle borde, dans
/// ce qu'elle enferme, enchaînée de proche en proche comme une sélection au
/// lasso (`selection`).
#[tauri::command(async)]
pub(crate) fn ouvrir_courbe(etat: State<Etat>, ligne: u32) -> Result<Vec<TrackRow>, String> {
    let r = relief_charge(&etat)?;
    let c = courbe_de(&r, ligne)?;
    let dedans = altitude::ouvrir(&r.relief, c, &r.positions);
    let vecteurs = charger_vecteurs(&etat)?;
    let ordonnes = rusty_music_analysis::chemin::parcours(&vecteurs, &dedans);
    tracing::info!(
        ligne,
        niveau = c.niveau,
        n = ordonnes.len(),
        "courbe ouverte"
    );
    pistes_de(&etat, &ordonnes)
}

/// Oublie le relief et ses archives : le plan vient d'être régénéré, les
/// adresses ont pu bouger.
pub(crate) fn oublier_relief(app: &tauri::AppHandle, etat: &Etat) {
    *verrou(&etat.relief) = None;
    for g in Grandeur::TOUTES {
        for genre in ["dem", "courbes"] {
            if let Ok(c) = tuiles::chemin_archive(app, &format!("{genre}-{}", g.id())) {
                std::fs::remove_file(c).ok();
            }
        }
    }
}
