// SPDX-License-Identifier: GPL-3.0-or-later
//! Sérier chaque cellule de la croissance : rendre les rues continues à
//! l'oreille sans toucher au récit chronologique (`docs/plan-balade.md`, P7).
//!
//! [`crate::croissance::peupler`] décide **qui** habite chaque cellule (date,
//! popularité, style) ; ce module ne change que **qui habite quel bâtiment**
//! à l'intérieur d'une cellule. Mesuré par P0 : en marchant, hors album, une
//! rue sonnait presque comme le hasard — les albums d'une cellule y étaient
//! posés dans l'ordre d'arrivée, sans regard pour le son.
//!
//! 1. Les morceaux de la cellule, regroupés par album (un album reste un bloc
//!    contigu, ses pistes dans l'ordre), sont ordonnés par un chemin court
//!    dans l'espace CLAP : plus proche voisin, puis 2-opt.
//! 2. Les bâtiments de la cellule sont déroulés le long des façades, rue par
//!    rue (`crate::facades`, abscisse le long du tronçon).
//! 3. Le sens du déroulement est choisi pour que les morceaux du bord de la
//!    cellule ressemblent à ceux des cellules voisines déjà posées.

use std::collections::{HashMap, HashSet};

use crate::croissance::{Arrivant, Croissance, Parcelle};

/// Distance sous laquelle deux bâtiments de cellules différentes se
/// raccordent (même ordre que `croissance::Parametres::rayon_voisinage`).
pub const RAYON_RACCORD_M: f64 = 60.0;
/// Passes de 2-opt au plus par cellule — une cellule tient quelques dizaines
/// d'albums, l'amélioration s'arrête bien avant.
const PASSES_2OPT: usize = 50;

/// Distance cosinus entre deux vecteurs déjà normalisés ; 1 si l'un manque.
fn cosinus(a: Option<&[f32]>, b: Option<&[f32]>) -> f64 {
    match (a, b) {
        (Some(a), Some(b)) => 1.0 - a.iter().zip(b).map(|(x, y)| (x * y) as f64).sum::<f64>(),
        _ => 1.0,
    }
}

fn normaliser(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / n).collect()
}

/// Un chemin ouvert court entre `n` points, d'après la matrice de distances
/// `d` (symétrique) : plus proche voisin depuis le point le plus excentré,
/// puis 2-opt.
pub fn chemin_court(d: &[Vec<f64>]) -> Vec<usize> {
    let n = d.len();
    if n <= 2 {
        return (0..n).collect();
    }
    // Départ : le plus éloigné de tous les autres en moyenne — un bout naturel.
    let depart = (0..n)
        .max_by(|&a, &b| {
            let sa: f64 = d[a].iter().sum();
            let sb: f64 = d[b].iter().sum();
            sa.total_cmp(&sb).then(b.cmp(&a))
        })
        .unwrap_or(0);
    let mut ordre = vec![depart];
    let mut pris = vec![false; n];
    pris[depart] = true;
    while ordre.len() < n {
        let dernier = *ordre.last().expect("non vide");
        let suivant = (0..n)
            .filter(|&j| !pris[j])
            .min_by(|&a, &b| d[dernier][a].total_cmp(&d[dernier][b]).then(a.cmp(&b)))
            .expect("il en reste");
        pris[suivant] = true;
        ordre.push(suivant);
    }
    // 2-opt sur un chemin ouvert : retourner `ordre[i..=j]` ne change que les
    // arêtes (i-1, i) et (j, j+1), quand elles existent.
    for _ in 0..PASSES_2OPT {
        let mut mieux = false;
        for i in 0..n - 1 {
            for j in i + 1..n {
                let avant = |k: usize| (k > 0).then(|| ordre[k - 1]);
                let apres = |k: usize| (k + 1 < n).then(|| ordre[k + 1]);
                let (a, b) = (ordre[i], ordre[j]);
                let mut gain = 0.0;
                if let Some(p) = avant(i) {
                    gain += d[p][a] - d[p][b];
                }
                if let Some(q) = apres(j) {
                    gain += d[b][q] - d[a][q];
                }
                if gain > 1e-12 {
                    ordre[i..=j].reverse();
                    mieux = true;
                }
            }
        }
        if !mieux {
            break;
        }
    }
    ordre
}

/// Les bâtiments d'une cellule déroulés le long des façades : rue par rue,
/// chacune dans l'ordre de ses abscisses, les rues enchaînées par proximité
/// en partant du bâtiment de plus bas coût (le côté intérieur, comme
/// `croissance::peupler`). `rues[i]` : tronçon et abscisse de la façade de la
/// parcelle `i`, `None` sans voie à portée (une rue à elle seule).
pub fn derouler(
    cellule: &[usize],
    parcelles: &[Parcelle],
    rues: &[Option<(usize, f64)>],
) -> Vec<usize> {
    let mut groupes: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    let mut seuls: Vec<Vec<usize>> = Vec::new();
    for &i in cellule {
        match rues[i] {
            Some((t, _)) => groupes.entry(Some(t)).or_default().push(i),
            None => seuls.push(vec![i]),
        }
    }
    let mut rangs: Vec<Vec<usize>> = groupes.into_values().collect();
    for g in &mut rangs {
        g.sort_by(|&a, &b| {
            let (xa, xb) = (rues[a].map_or(0.0, |r| r.1), rues[b].map_or(0.0, |r| r.1));
            xa.total_cmp(&xb)
                .then(parcelles[a].id.cmp(&parcelles[b].id))
        });
    }
    rangs.extend(seuls);
    rangs.sort_by_key(|g| parcelles[g[0]].id);

    let d2 = |a: usize, b: usize| {
        let (p, q) = (parcelles[a].centre, parcelles[b].centre);
        (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)
    };
    let cout = |i: usize| parcelles[i].cout;
    // La rue de départ : celle du bâtiment de plus bas coût, prise par son
    // bout le moins coûteux.
    let premiere = (0..rangs.len())
        .min_by(|&a, &b| {
            let ca = rangs[a]
                .iter()
                .map(|&i| cout(i))
                .fold(f64::INFINITY, f64::min);
            let cb = rangs[b]
                .iter()
                .map(|&i| cout(i))
                .fold(f64::INFINITY, f64::min);
            ca.total_cmp(&cb).then(a.cmp(&b))
        })
        .unwrap_or(0);
    let mut sortie: Vec<usize> = Vec::with_capacity(cellule.len());
    let mut pris = vec![false; rangs.len()];
    let mut courant = premiere;
    let mut g = rangs[courant].clone();
    if cout(*g.last().expect("non vide")) < cout(g[0]) {
        g.reverse();
    }
    loop {
        pris[courant] = true;
        sortie.extend(&g);
        let bout = *sortie.last().expect("non vide");
        // La rue suivante : celle dont un bout est le plus proche.
        let Some((k, inverse)) = (0..rangs.len())
            .filter(|&k| !pris[k])
            .map(|k| {
                let (a, b) = (rangs[k][0], *rangs[k].last().expect("non vide"));
                let (da, db) = (d2(bout, a), d2(bout, b));
                (k, db < da, da.min(db))
            })
            .min_by(|x, y| x.2.total_cmp(&y.2).then(x.0.cmp(&y.0)))
            .map(|(k, inv, _)| (k, inv))
        else {
            break;
        };
        courant = k;
        g = rangs[k].clone();
        if inverse {
            g.reverse();
        }
    }
    sortie
}

/// Grille spatiale sur les parcelles déjà posées, pour le raccord.
struct Grille {
    pas: f64,
    cases: HashMap<(i64, i64), Vec<usize>>,
}

impl Grille {
    fn cle(&self, p: [f64; 2]) -> (i64, i64) {
        (
            (p[0] / self.pas).floor() as i64,
            (p[1] / self.pas).floor() as i64,
        )
    }
    fn ajouter(&mut self, i: usize, p: [f64; 2]) {
        let c = self.cle(p);
        self.cases.entry(c).or_default().push(i);
    }
    fn autour(&self, p: [f64; 2]) -> impl Iterator<Item = usize> + '_ {
        let (cx, cy) = self.cle(p);
        (-1..=1)
            .flat_map(move |dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
            .filter_map(|c| self.cases.get(&c))
            .flatten()
            .copied()
    }
}

/// Réattribue les bâtiments à l'intérieur de chaque cellule (voir l'en-tête
/// du module). Le contenu des cellules, et l'ensemble des bâtiments occupés
/// par chacune, ne changent pas. `empreintes` : identifiant → empreinte CLAP
/// (un morceau sans empreinte se range comme s'il était loin de tout).
pub fn serier(
    croissance: &mut Croissance,
    parcelles: &[Parcelle],
    rues: &[Option<(usize, f64)>],
    arrivants: &[Arrivant],
    empreintes: &HashMap<i64, Vec<f32>>,
) {
    let arrivant: HashMap<i64, &Arrivant> = arrivants.iter().map(|a| (a.id, a)).collect();
    let vecteur: HashMap<i64, Vec<f32>> = croissance
        .adresses
        .iter()
        .filter_map(|a| empreintes.get(&a.id).map(|v| (a.id, normaliser(v))))
        .collect();
    let v = |id: i64| vecteur.get(&id).map(Vec::as_slice);

    let mut par_cellule: HashMap<u32, Vec<usize>> = HashMap::new();
    for (k, a) in croissance.adresses.iter().enumerate() {
        par_cellule.entry(a.cellule).or_default().push(k);
    }
    let mut cellules: Vec<u32> = par_cellule.keys().copied().collect();
    // Dans l'ordre du front (les cellules sont numérotées par coût croissant) :
    // chaque cellule se raccorde à celles, plus anciennes, déjà posées.
    cellules.sort_unstable();

    let mut posees = Grille {
        pas: RAYON_RACCORD_M,
        cases: HashMap::new(),
    };
    // Parcelle → morceau, pour les cellules déjà posées.
    let mut occupant: HashMap<usize, i64> = HashMap::new();

    for c in cellules {
        let membres = &par_cellule[&c];
        // Les albums de la cellule, chacun ses pistes dans l'ordre.
        let mut albums: HashMap<(&str, &str), Vec<i64>> = HashMap::new();
        for &k in membres {
            let id = croissance.adresses[k].id;
            let cle = arrivant
                .get(&id)
                .map_or(("", ""), |a| (a.artiste.as_str(), a.album.as_str()));
            albums.entry(cle).or_default().push(id);
        }
        let mut blocs: Vec<Vec<i64>> = albums.into_values().collect();
        for b in &mut blocs {
            b.sort_by_key(|id| (arrivant.get(id).map_or(0, |a| a.piste), *id));
        }
        blocs.sort_by_key(|b| b[0]);
        let moyennes: Vec<Option<Vec<f32>>> = blocs
            .iter()
            .map(|b| {
                let vs: Vec<&[f32]> = b.iter().filter_map(|&id| v(id)).collect();
                let dim = vs.first()?.len();
                let mut m = vec![0.0f32; dim];
                for x in &vs {
                    for (s, y) in m.iter_mut().zip(x.iter()) {
                        *s += y;
                    }
                }
                Some(normaliser(&m))
            })
            .collect();
        let d: Vec<Vec<f64>> = (0..blocs.len())
            .map(|i| {
                (0..blocs.len())
                    .map(|j| {
                        if i == j {
                            0.0
                        } else {
                            cosinus(moyennes[i].as_deref(), moyennes[j].as_deref())
                        }
                    })
                    .collect()
            })
            .collect();
        let ordre = chemin_court(&d);

        let lieux: Vec<usize> = membres
            .iter()
            .map(|&k| croissance.adresses[k].parcelle)
            .collect();
        let deroule = derouler(&lieux, parcelles, rues);

        // Les deux sens : les albums dans un ordre ou dans l'autre (chacun
        // garde ses pistes dans l'ordre). On garde celui qui ressemble le plus,
        // au bord, aux cellules voisines déjà posées.
        let suite = |inverse: bool| -> Vec<i64> {
            let mut o = ordre.clone();
            if inverse {
                o.reverse();
            }
            o.iter().flat_map(|&b| blocs[b].iter().copied()).collect()
        };
        let raccord = |s: &[i64]| -> f64 {
            let mut total = 0.0;
            for (&lieu, &id) in deroule.iter().zip(s) {
                let p = parcelles[lieu].centre;
                let proche = posees
                    .autour(p)
                    .filter(|&q| {
                        let r = parcelles[q].centre;
                        (r[0] - p[0]).powi(2) + (r[1] - p[1]).powi(2)
                            <= RAYON_RACCORD_M * RAYON_RACCORD_M
                    })
                    .min_by(|&a, &b| {
                        let (ra, rb) = (parcelles[a].centre, parcelles[b].centre);
                        let da = (ra[0] - p[0]).powi(2) + (ra[1] - p[1]).powi(2);
                        let db = (rb[0] - p[0]).powi(2) + (rb[1] - p[1]).powi(2);
                        da.total_cmp(&db)
                    });
                if let Some(q) = proche {
                    total += cosinus(v(id), v(occupant[&q]));
                }
            }
            total
        };
        let (avant, arriere) = (suite(false), suite(true));
        let retenue = if raccord(&arriere) < raccord(&avant) {
            arriere
        } else {
            avant
        };

        let rang: HashMap<i64, usize> = membres
            .iter()
            .map(|&k| (croissance.adresses[k].id, k))
            .collect();
        for (&lieu, id) in deroule.iter().zip(retenue) {
            let k = rang[&id];
            croissance.adresses[k].parcelle = lieu;
            occupant.insert(lieu, id);
            posees.ajouter(lieu, parcelles[lieu].centre);
        }
    }
    debug_assert_eq!(
        croissance
            .adresses
            .iter()
            .map(|a| a.parcelle)
            .collect::<HashSet<_>>()
            .len(),
        croissance.adresses.len(),
        "deux morceaux dans le même bâtiment"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrice(points: &[f64]) -> Vec<Vec<f64>> {
        points
            .iter()
            .map(|a| points.iter().map(|b| (a - b).abs()).collect())
            .collect()
    }

    #[test]
    fn le_chemin_court_range_une_droite_dans_l_ordre() {
        let ordre = chemin_court(&matrice(&[5.0, 0.0, 9.0, 2.0, 7.0, 1.0]));
        let valeurs: Vec<f64> = ordre
            .iter()
            .map(|&i| [5.0, 0.0, 9.0, 2.0, 7.0, 1.0][i])
            .collect();
        let croissant = valeurs.windows(2).all(|w| w[0] < w[1]);
        let decroissant = valeurs.windows(2).all(|w| w[0] > w[1]);
        assert!(croissant || decroissant, "{valeurs:?}");
    }

    fn parcelle(id: i64, x: f64, y: f64, cout: f64) -> Parcelle {
        Parcelle {
            id,
            centre: [x, y],
            cout,
            artere: 0.1,
        }
    }

    #[test]
    fn derouler_suit_chaque_rue_puis_passe_a_la_plus_proche() {
        // Rue 0 le long de y = 0 (x = 0, 10, 20), rue 1 le long de y = 30
        // (x = 20, 10, 0) ; le coût le plus bas en (0, 0).
        let parcelles = vec![
            parcelle(1, 20.0, 0.0, 3.0),
            parcelle(2, 0.0, 0.0, 1.0),
            parcelle(3, 10.0, 0.0, 2.0),
            parcelle(4, 0.0, 30.0, 6.0),
            parcelle(5, 20.0, 30.0, 4.0),
            parcelle(6, 10.0, 30.0, 5.0),
        ];
        let rues = vec![
            Some((0, 20.0)),
            Some((0, 0.0)),
            Some((0, 10.0)),
            Some((1, 0.0)),
            Some((1, 20.0)),
            Some((1, 10.0)),
        ];
        let d = derouler(&[0, 1, 2, 3, 4, 5], &parcelles, &rues);
        let ids: Vec<i64> = d.iter().map(|&i| parcelles[i].id).collect();
        // (0,0) → (20,0), puis la rue d'en face par son bout le plus proche (20,30).
        assert_eq!(ids, vec![2, 3, 1, 5, 6, 4]);
    }
}
