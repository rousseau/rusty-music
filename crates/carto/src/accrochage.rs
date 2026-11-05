// SPDX-License-Identifier: GPL-3.0-or-later
//! Les morceaux accrochés à la voirie, et la playlist qu'un tracé en tire.
//!
//! Chaque morceau habite une adresse réelle (`positions.json`, écrit par
//! l'application à côté des tuiles) ; on l'accroche au sommet routable le plus
//! proche de [`Graphe`]. Un itinéraire sur la voirie compose ensuite sa
//! playlist des morceaux qui bordent son couloir ([`morceaux_le_long`]).
//!
//! Sorti de `apps/desktop` pour que les outils de mesure
//! (`examples/continuite_balade.rs`) fabriquent les mêmes suites que
//! l'application.

use std::collections::{HashMap, HashSet};

use crate::reseau_reel::Graphe;

/// Portée de l'accrochage, mètres : au-delà, le morceau n'est rattaché à
/// aucune rue (un bois, le bord de l'extrait).
pub const PORTEE_ACCROCHAGE_M: f64 = 200.0;

/// Chaque morceau accroché au sommet de voirie le plus proche, dans les deux
/// sens.
#[derive(Debug, Default)]
pub struct AccrochageVoirie {
    /// Morceau → sommet de voirie (index dans le graphe de base).
    pub sommet_de: HashMap<i64, u32>,
    /// Sommet de voirie → les morceaux qui s'y accrochent.
    pub morceaux_a: HashMap<u32, Vec<i64>>,
}

impl AccrochageVoirie {
    /// Accroche chaque `(id, lon, lat)` au sommet routable le plus proche à
    /// moins de [`PORTEE_ACCROCHAGE_M`]. Bâti sur le graphe de base :
    /// l'indexation des sommets ne dépend pas de la pondération, donc
    /// l'accrochage vaut pour tous les graphes de voirie pondérés du même
    /// extrait.
    pub fn construire(
        graphe_base: &Graphe,
        points: impl IntoIterator<Item = (i64, f32, f32)>,
    ) -> AccrochageVoirie {
        let index = graphe_base.index_sommets();
        let mut sommet_de: HashMap<i64, u32> = HashMap::new();
        let mut morceaux_a: HashMap<u32, Vec<i64>> = HashMap::new();
        for (id, x, y) in points {
            if let Some(s) =
                index.plus_proche(graphe_base, [x as f64, y as f64], PORTEE_ACCROCHAGE_M)
            {
                sommet_de.insert(id, s);
                morceaux_a.entry(s).or_default().push(id);
            }
        }
        // Ordre déterministe des morceaux d'un même sommet (départage à rang égal).
        for v in morceaux_a.values_mut() {
            v.sort_unstable();
        }
        AccrochageVoirie {
            sommet_de,
            morceaux_a,
        }
    }
}

/// Sans arrivée, la destination provisoire d'un itinéraire de `cible_ms` de
/// musique : un morceau accessible assez loin pour qu'il y ait ~`cible_ms` en
/// chemin. `None` si aucun morceau n'est accessible.
///
/// `permis` restreint les candidats (le filtre de familles d'Explorer), le
/// départ passe toujours.
pub fn destination_pour_duree(
    graphe: &Graphe,
    accrochage: &AccrochageVoirie,
    depart: i64,
    depart_pos: [f64; 2],
    permis: Option<&HashSet<i64>>,
    cible_ms: u64,
    duree: &dyn Fn(i64) -> u64,
) -> Option<i64> {
    let couts = graphe.couts_depuis(depart_pos);
    let mut candidats: Vec<(u64, i64)> = accrochage
        .sommet_de
        .iter()
        .filter(|(id, _)| permis.is_none_or(|f| f.contains(*id) || **id == depart))
        .filter_map(|(&id, &s)| couts.get(s as usize).copied().flatten().map(|c| (c, id)))
        .collect();
    candidats.sort_unstable();
    // Viser ~2× la durée : le tracé ne longe qu'une partie des morceaux
    // « à portée » (ceux de son couloir), pas tous ceux du disque de
    // coût. Sans cette marge, la playlist manque souvent la cible et le
    // tracé, tronqué au dernier morceau, reste tout petit.
    let vise = cible_ms.saturating_mul(2);
    let mut cumul = 0u64;
    let mut cible_id = candidats.last().map(|&(_, id)| id);
    for &(_, id) in &candidats {
        cumul += duree(id);
        if cumul >= vise {
            cible_id = Some(id);
            break;
        }
    }
    cible_id
}

/// Les morceaux rencontrés le long d'un tracé de voirie, dans l'ordre du
/// parcours. C'est le cœur du mode « itinéraire » : le trajet suit les rues,
/// la playlist est faite de ce qui les borde.
///
/// - `couloir` : sommet → rang (position le long du tracé), de
///   [`Graphe::couloir`] ;
/// - `famille` : si `Some`, ne garder que ces morceaux (le filtre d'Explorer) —
///   mais le départ et l'arrivée passent toujours ;
/// - `arrivee_id` : **si `Some`, elle est le terminus** (forcée en queue) et la
///   durée est ignorée — « va jusque-là » l'emporte ;
/// - `duree_cible_ms` : ne s'applique **que sans arrivée** — la playlist
///   s'arrête quand le cumul des durées atteint la cible (moins 90 s de
///   tolérance).
#[allow(clippy::too_many_arguments)]
pub fn morceaux_le_long(
    accrochage: &AccrochageVoirie,
    couloir: &HashMap<u32, usize>,
    depart_id: i64,
    arrivee_id: Option<i64>,
    famille: Option<&HashSet<i64>>,
    duree_cible_ms: Option<u64>,
    duree: &dyn Fn(i64) -> u64,
) -> Vec<i64> {
    const PLAFOND: usize = 120;
    const TOLERANCE_MS: u64 = 90_000;

    // **Une arrivée posée prime : le trajet va jusqu'à elle**, et la durée ne
    // s'applique plus (elle ne sert qu'aux itinéraires sans arrivée — « une
    // balade de 40 min par là »). L'arrivée est forcée en queue et exclue du
    // corps ; le départ toujours en tête.
    let terminus = arrivee_id;
    let borne = |id: i64| id == depart_id || Some(id) == terminus;
    let duree_cible_ms = if terminus.is_some() {
        None
    } else {
        duree_cible_ms
    };

    // (rang, id) pour chaque morceau du couloir.
    let mut candidats: Vec<(usize, i64)> = Vec::new();
    for (sommet, rang) in couloir {
        if let Some(ids) = accrochage.morceaux_a.get(sommet) {
            for &id in ids {
                candidats.push((*rang, id));
            }
        }
    }
    candidats.sort_unstable_by_key(|&(rang, id)| (rang, id));

    let mut vus: HashSet<i64> = HashSet::new();
    let mut suite: Vec<i64> = Vec::new();
    for (_, id) in candidats {
        if borne(id) || !vus.insert(id) {
            continue;
        }
        if let Some(f) = famille {
            if !f.contains(&id) {
                continue;
            }
        }
        suite.push(id);
    }

    // Départ en tête.
    suite.retain(|&id| !borne(id));
    suite.insert(0, depart_id);

    // La durée prime : on coupe dès que le cumul l'atteint.
    if let Some(cible) = duree_cible_ms {
        let seuil = cible.saturating_sub(TOLERANCE_MS);
        let mut cumul = 0u64;
        let mut coupe = suite.len();
        for (i, &id) in suite.iter().enumerate() {
            cumul += duree(id);
            if cumul >= seuil {
                coupe = i + 1;
                break;
            }
        }
        suite.truncate(coupe.max(1));
    } else if let Some(a) = terminus {
        // Pas de durée : l'arrivée est le terminus.
        if a != depart_id {
            suite.push(a);
        }
    }

    suite.truncate(PLAFOND.max(1));
    suite
}

/// Jusqu'où le tracé doit être dessiné : un cran après le sommet du **dernier
/// morceau retenu** dans le couloir. Au moins 2 sommets (pour une polyligne),
/// au plus toute la longueur du tracé. `route` inconnu du couloir → tout le
/// tracé.
pub fn fin_de_trace(
    route: &[i64],
    accrochage: &AccrochageVoirie,
    couloir: &HashMap<u32, usize>,
    longueur_trace: usize,
) -> usize {
    let jusqua = route
        .iter()
        .filter_map(|id| accrochage.sommet_de.get(id))
        .filter_map(|s| couloir.get(s))
        .copied()
        .max()
        .map_or(longueur_trace, |r| r + 1);
    jusqua.min(longueur_trace).max(2.min(longueur_trace))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un accrochage monté à la main : trois sommets alignés, un morceau par
    /// sommet, plus un intrus d'une autre famille sur le sommet du milieu.
    fn accrochage_dessai() -> AccrochageVoirie {
        let mut sommet_de = HashMap::new();
        let mut morceaux_a: HashMap<u32, Vec<i64>> = HashMap::new();
        for (id, s) in [(10, 0u32), (20, 1), (30, 1), (40, 2)] {
            sommet_de.insert(id, s);
            morceaux_a.entry(s).or_default().push(id);
        }
        AccrochageVoirie {
            sommet_de,
            morceaux_a,
        }
    }

    #[test]
    fn morceaux_le_long_ordonne_dedoublonne_et_borne_la_famille() {
        let acc = accrochage_dessai();
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 1), (2, 2)].into_iter().collect();
        let famille: HashSet<i64> = [10, 20, 40].into_iter().collect(); // 30 exclu
        let duree = |_id: i64| 0u64;
        let suite = morceaux_le_long(&acc, &couloir, 10, Some(40), Some(&famille), None, &duree);
        // départ 10 en tête, arrivée 40 en queue, 30 filtré, 20 au milieu.
        assert_eq!(suite, vec![10, 20, 40]);
    }

    #[test]
    fn morceaux_le_long_coupe_a_la_duree_cible() {
        let acc = accrochage_dessai();
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 1), (2, 2)].into_iter().collect();
        let duree = |_id: i64| 10 * 60_000u64; // 10 min chacun
                                               // Cible 25 min, tolérance 90 s : 10 (0) + 20 (10) + 30 (20) → 30 min ≥ 23,5.
        let suite = morceaux_le_long(&acc, &couloir, 10, None, None, Some(25 * 60_000), &duree);
        assert_eq!(suite, vec![10, 20, 30]);
    }

    #[test]
    fn morceaux_le_long_larrivee_prime_sur_la_duree() {
        let acc = accrochage_dessai();
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 1), (2, 2)].into_iter().collect();
        let duree = |_id: i64| 10 * 60_000u64;
        // Arrivée 40 posée : « va jusque-là » l'emporte, la durée (15 min) est
        // ignorée — la playlist va jusqu'à 40.
        let suite = morceaux_le_long(
            &acc,
            &couloir,
            10,
            Some(40),
            None,
            Some(15 * 60_000),
            &duree,
        );
        assert_eq!(suite, vec![10, 20, 30, 40]);
    }

    #[test]
    fn fin_de_trace_sarrete_au_dernier_morceau_pas_a_la_destination() {
        let acc = accrochage_dessai(); // 10@sommet0, 20&30@sommet1, 40@sommet2
                                       // Tracé de 10 sommets ; le couloir place le sommet 1 au rang 3.
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 3), (2, 8)].into_iter().collect();
        // Playlist = [10, 20] → dernier morceau au rang 3 → tracé coupé à 4.
        assert_eq!(fin_de_trace(&[10, 20], &acc, &couloir, 10), 4);
        // Playlist qui va jusqu'à 40 (rang 8) → coupé à 9.
        assert_eq!(fin_de_trace(&[10, 20, 40], &acc, &couloir, 10), 9);
        // Morceaux hors couloir → tout le tracé.
        assert_eq!(fin_de_trace(&[99], &acc, &couloir, 10), 10);
        // Tracé dégénéré.
        assert_eq!(fin_de_trace(&[10], &acc, &couloir, 1), 1);
    }
}
