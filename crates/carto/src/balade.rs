// SPDX-License-Identifier: GPL-3.0-or-later
//! La balade : la lecture est une marche dans les rues (`docs/plan-balade.md`,
//! P1).
//!
//! Le morceau suivant n'est pas dans une file calculée d'avance : il est
//! choisi au moment où on en a besoin, depuis la position courante. Ce module
//! ne tient que la partie pure — l'état de la marche et les branches qui
//! s'offrent ([`suivants`]), puis la pente par défaut ([`pente_simple`]).
//! L'application décide quand avancer et ce qu'elle confie au lecteur.
//!
//! Une **branche** est une suite possible : la séquence de sommets de voirie
//! depuis la position courante jusqu'au sommet de son premier morceau non
//! encore joué. Une rue sans morceau (un pont, un quai vide) se traverse sans
//! jouer : la branche continue jusqu'au premier riverain.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::accrochage::AccrochageVoirie;
use crate::reseau_reel::Graphe;

/// Au-delà de ce coût (millimètres, l'unité des arêtes), une direction est
/// réputée vide : 3 km de rues sans un seul morceau.
const COUT_MAX_MM: u64 = 3_000_000;
/// Sommets explorés au plus par direction — garde-fou contre une direction
/// qui ouvre sur toute la ville sans rien trouver.
const SOMMETS_MAX: usize = 20_000;

/// Où en est la marche.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EtatBalade {
    /// Le sommet de voirie du morceau en cours.
    pub sommet: u32,
    /// Le sommet d'où l'on est arrivé : donne le sens de marche. `None` au
    /// départ, où toutes les directions s'offrent.
    pub precedent: Option<u32>,
    /// Sommets déjà parcourus : on évite d'y repasser tant qu'il y a autre chose.
    pub sommets_vus: HashSet<u32>,
    /// Morceaux déjà joués : jamais proposés de nouveau.
    pub morceaux_vus: HashSet<i64>,
}

impl EtatBalade {
    /// Une balade qui part de `morceau`, accroché au sommet `sommet`.
    pub fn depuis(sommet: u32, morceau: i64) -> EtatBalade {
        EtatBalade {
            sommet,
            precedent: None,
            sommets_vus: HashSet::from([sommet]),
            morceaux_vus: HashSet::from([morceau]),
        }
    }

    /// Prend la branche `b` : on se retrouve au sommet de son morceau.
    pub fn avancer(&mut self, b: &Branche) {
        if let [.., avant, dernier] = b.sommets[..] {
            self.precedent = Some(avant);
            self.sommet = dernier;
        }
        self.sommets_vus.extend(b.sommets.iter().copied());
        self.morceaux_vus.insert(b.morceau);
    }
}

/// Une suite possible depuis la position courante.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branche {
    /// Les sommets parcourus, de la position courante (incluse) au sommet du
    /// morceau (inclus). Un seul sommet : le morceau habite le même.
    pub sommets: Vec<u32>,
    /// Le premier morceau non joué de cette branche.
    pub morceau: i64,
    /// Coût du trajet, millimètres.
    pub cout: u64,
}

/// Voisins distincts d'un sommet (deux tronçons peuvent partager un segment).
fn voisins(graphe: &Graphe, s: u32) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = graphe.voisins(s).to_vec();
    v.sort_unstable();
    v.dedup_by_key(|(w, _)| *w);
    v.retain(|&(w, _)| w != s);
    v
}

/// Le premier morceau non joué d'un sommet (plus petit identifiant : même
/// départage que [`AccrochageVoirie`]).
fn premier_libre(etat: &EtatBalade, accrochage: &AccrochageVoirie, s: u32) -> Option<i64> {
    accrochage
        .morceaux_a
        .get(&s)?
        .iter()
        .copied()
        .find(|id| !etat.morceaux_vus.contains(id))
}

/// Plus court chemin depuis `etat.sommet` par l'arête vers `w` jusqu'au plus
/// proche sommet portant un morceau libre. Ne repasse jamais par la position
/// courante ; `eviter_vus` écarte en plus les sommets déjà parcourus.
fn chercher(
    etat: &EtatBalade,
    graphe: &Graphe,
    accrochage: &AccrochageVoirie,
    w: u32,
    poids: u32,
    eviter_vus: bool,
) -> Option<Branche> {
    let origine = etat.sommet;
    if eviter_vus && etat.sommets_vus.contains(&w) {
        return None;
    }
    let mut cout: HashMap<u32, u64> = HashMap::from([(w, poids as u64)]);
    let mut parent: HashMap<u32, u32> = HashMap::from([(w, origine)]);
    let mut tas = BinaryHeap::from([Reverse((poids as u64, w))]);
    let mut explores = 0usize;
    while let Some(Reverse((c, s))) = tas.pop() {
        if c > cout[&s] {
            continue;
        }
        if let Some(morceau) = premier_libre(etat, accrochage, s) {
            let mut sommets = vec![s];
            while let Some(&p) = parent.get(sommets.last().expect("non vide")) {
                sommets.push(p);
            }
            sommets.reverse();
            return Some(Branche {
                sommets,
                morceau,
                cout: c,
            });
        }
        explores += 1;
        if explores > SOMMETS_MAX || c > COUT_MAX_MM {
            return None;
        }
        for (v, p) in voisins(graphe, s) {
            if v == origine || (eviter_vus && etat.sommets_vus.contains(&v)) {
                continue;
            }
            let nc = c + p as u64;
            if cout.get(&v).is_none_or(|&x| nc < x) {
                cout.insert(v, nc);
                parent.insert(v, s);
                tas.push(Reverse((nc, v)));
            }
        }
    }
    None
}

/// Les branches qui s'offrent depuis la position courante.
///
/// - Le sommet courant porte encore un morceau libre : une seule branche, sur
///   place (on continue le long de la même façade).
/// - Sinon, une branche par direction, le sens d'arrivée exclu : **au milieu
///   d'une rue, une seule** (on continue) ; au carrefour, celles qui en partent.
///   Chaque direction mène au premier morceau libre, en traversant les rues
///   vides, sans repasser par ce qui a déjà été parcouru.
/// - Rien devant (impasse, ou tout parcouru) : on accepte de repasser par le
///   déjà vu, puis seulement alors le demi-tour.
///
/// Deux directions qui mènent au même morceau n'en font qu'une (la plus
/// courte). Ordre : du plus proche au plus lointain.
pub fn suivants(etat: &EtatBalade, graphe: &Graphe, accrochage: &AccrochageVoirie) -> Vec<Branche> {
    if let Some(morceau) = premier_libre(etat, accrochage, etat.sommet) {
        return vec![Branche {
            sommets: vec![etat.sommet],
            morceau,
            cout: 0,
        }];
    }
    let tous = voisins(graphe, etat.sommet);
    let devant: Vec<(u32, u32)> = tous
        .iter()
        .copied()
        .filter(|&(w, _)| Some(w) != etat.precedent)
        .collect();
    let derriere: Vec<(u32, u32)> = tous
        .iter()
        .copied()
        .filter(|&(w, _)| Some(w) == etat.precedent)
        .collect();

    let essayer = |directions: &[(u32, u32)], eviter_vus: bool| -> Vec<Branche> {
        let mut par_morceau: HashMap<i64, Branche> = HashMap::new();
        for &(w, p) in directions {
            if let Some(b) = chercher(etat, graphe, accrochage, w, p, eviter_vus) {
                match par_morceau.get(&b.morceau) {
                    Some(x) if x.cout <= b.cout => {}
                    _ => {
                        par_morceau.insert(b.morceau, b);
                    }
                }
            }
        }
        let mut v: Vec<Branche> = par_morceau.into_values().collect();
        v.sort_by_key(|b| (b.cout, b.morceau));
        v
    };

    for (directions, eviter_vus) in [(&devant, true), (&devant, false), (&derriere, false)] {
        let b = essayer(directions, eviter_vus);
        if !b.is_empty() {
            return b;
        }
    }
    Vec::new()
}

/// La pente par défaut, version simple (P1 ; la vraie arrive avec P3) :
/// continuer tout droit si la rue continue — la branche qui reste sur le
/// tronçon par lequel on est arrivé —, sinon la branche dont le premier
/// morceau est le plus proche à l'oreille du morceau en cours (`distance`,
/// plus petit = plus proche). `None` si `branches` est vide.
pub fn pente_simple(
    etat: &EtatBalade,
    branches: &[Branche],
    graphe: &Graphe,
    distance: impl Fn(i64) -> f32,
) -> Option<usize> {
    if branches.len() <= 1 {
        return (!branches.is_empty()).then_some(0);
    }
    let la_meme_rue: Vec<usize> = match etat.precedent {
        Some(p) => {
            let arrivee = graphe.troncons_traverses(&[p, etat.sommet]);
            (0..branches.len())
                .filter(|&i| {
                    branches[i].sommets.get(1).is_some_and(|&w| {
                        graphe
                            .troncons_traverses(&[etat.sommet, w])
                            .iter()
                            .any(|t| arrivee.contains(t))
                    })
                })
                .collect()
        }
        None => Vec::new(),
    };
    let candidats: Vec<usize> = if la_meme_rue.is_empty() {
        (0..branches.len()).collect()
    } else {
        la_meme_rue
    };
    candidats.into_iter().min_by(|&a, &b| {
        distance(branches[a].morceau)
            .total_cmp(&distance(branches[b].morceau))
            .then(a.cmp(&b))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::{Classe, Extrait, Troncon};

    /// Un carrefour C au centre, quatre rues de 100 m environ (≈ 0,0013° en
    /// longitude, 0,0009° en latitude à Paris) :
    ///
    /// ```text
    ///              N2
    ///              |
    ///              N1
    ///              |
    ///  O1 ——— C ——— E1 ——— E2 ——— E3
    ///              |
    ///              S1        (S1 : impasse)
    /// ```
    ///
    /// L'avenue (tronçon 1) va de O1 à E3 en passant par C ; la rue du nord
    /// (tronçon 2) part de C ; l'impasse sud (tronçon 3).
    fn extrait() -> Extrait {
        let c = [2.3500, 48.8500];
        let o1 = [2.3487, 48.8500];
        let e1 = [2.3513, 48.8500];
        let e2 = [2.3526, 48.8500];
        let e3 = [2.3539, 48.8500];
        let n1 = [2.3500, 48.8509];
        let n2 = [2.3500, 48.8518];
        let s1 = [2.3500, 48.8491];
        let t = |id, points: Vec<[f64; 2]>| Troncon {
            id,
            nom: None,
            classe: Classe::Residentielle,
            points,
        };
        Extrait {
            troncons: vec![
                t(1, vec![o1, c, e1, e2, e3]),
                t(2, vec![c, n1, n2]),
                t(3, vec![c, s1]),
            ],
            ..Default::default()
        }
    }

    struct Banc {
        graphe: Graphe,
        s: HashMap<&'static str, u32>,
    }

    fn banc() -> Banc {
        let graphe = Graphe::construire(&extrait());
        let points = [
            ("c", [2.3500, 48.8500]),
            ("o1", [2.3487, 48.8500]),
            ("e1", [2.3513, 48.8500]),
            ("e2", [2.3526, 48.8500]),
            ("e3", [2.3539, 48.8500]),
            ("n1", [2.3500, 48.8509]),
            ("n2", [2.3500, 48.8518]),
            ("s1", [2.3500, 48.8491]),
        ];
        let s = points
            .iter()
            .map(|(nom, p)| (*nom, graphe.accrocher(*p).expect("sommet")))
            .collect();
        Banc { graphe, s }
    }

    fn accrochage(placement: &[(i64, u32)]) -> AccrochageVoirie {
        let mut a = AccrochageVoirie::default();
        for &(id, s) in placement {
            a.sommet_de.insert(id, s);
            a.morceaux_a.entry(s).or_default().push(id);
        }
        for v in a.morceaux_a.values_mut() {
            v.sort_unstable();
        }
        a
    }

    #[test]
    fn un_morceau_libre_sur_place_passe_avant_les_rues() {
        let b = banc();
        let acc = accrochage(&[(1, b.s["c"]), (2, b.s["c"]), (3, b.s["e1"])]);
        let etat = EtatBalade::depuis(b.s["c"], 1);
        let br = suivants(&etat, &b.graphe, &acc);
        assert_eq!(br.len(), 1);
        assert_eq!(br[0].morceau, 2);
        assert_eq!(br[0].sommets, vec![b.s["c"]]);
    }

    #[test]
    fn carrefour_a_quatre_branches_sans_le_sens_darrivee() {
        let b = banc();
        let acc = accrochage(&[
            (1, b.s["c"]),
            (10, b.s["o1"]),
            (20, b.s["e1"]),
            (30, b.s["n1"]),
            (40, b.s["s1"]),
        ]);
        // Au départ, toutes les directions.
        let etat = EtatBalade::depuis(b.s["c"], 1);
        let mut m: Vec<i64> = suivants(&etat, &b.graphe, &acc)
            .iter()
            .map(|x| x.morceau)
            .collect();
        m.sort_unstable();
        assert_eq!(m, vec![10, 20, 30, 40]);
        // Arrivé par l'ouest : trois branches, l'ouest exclu.
        let mut etat = etat;
        etat.precedent = Some(b.s["o1"]);
        let br = suivants(&etat, &b.graphe, &acc);
        let mut m: Vec<i64> = br.iter().map(|x| x.morceau).collect();
        m.sort_unstable();
        assert_eq!(m, vec![20, 30, 40]);
        for x in &br {
            assert_eq!(x.sommets.len(), 2, "un pas jusqu'au voisin");
            assert_eq!(x.sommets[0], b.s["c"]);
        }
    }

    #[test]
    fn au_milieu_dune_rue_une_seule_branche() {
        let b = banc();
        let acc = accrochage(&[(1, b.s["e1"]), (2, b.s["e3"]), (3, b.s["c"])]);
        // En E1, arrivé de C : devant, une seule direction.
        let mut etat = EtatBalade::depuis(b.s["e1"], 1);
        etat.precedent = Some(b.s["c"]);
        let br = suivants(&etat, &b.graphe, &acc);
        assert_eq!(br.len(), 1);
        assert_eq!(br[0].morceau, 2);
    }

    #[test]
    fn une_rue_vide_se_traverse_sans_jouer() {
        let b = banc();
        // E1 et E2 vides : la branche file jusqu'à E3.
        let acc = accrochage(&[(1, b.s["c"]), (2, b.s["e3"])]);
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        etat.precedent = Some(b.s["o1"]);
        let br = suivants(&etat, &b.graphe, &acc);
        assert_eq!(br.len(), 1);
        assert_eq!(br[0].morceau, 2);
        assert_eq!(
            br[0].sommets,
            vec![b.s["c"], b.s["e1"], b.s["e2"], b.s["e3"]]
        );
    }

    #[test]
    fn impasse_demi_tour_seulement_sil_ny_a_rien_dautre() {
        let b = banc();
        let acc = accrochage(&[(1, b.s["s1"]), (2, b.s["e1"])]);
        // Au fond de l'impasse, arrivé de C : rien devant, demi-tour.
        let mut etat = EtatBalade::depuis(b.s["s1"], 1);
        etat.precedent = Some(b.s["c"]);
        let br = suivants(&etat, &b.graphe, &acc);
        assert_eq!(br.len(), 1);
        assert_eq!(br[0].morceau, 2);
        assert_eq!(br[0].sommets, vec![b.s["s1"], b.s["c"], b.s["e1"]]);
    }

    #[test]
    fn le_deja_visite_est_exclu() {
        let b = banc();
        let acc = accrochage(&[
            (1, b.s["c"]),
            (2, b.s["e1"]),
            (3, b.s["n1"]),
            (4, b.s["e2"]),
        ]);
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        etat.precedent = Some(b.s["o1"]);
        // Le morceau 2 a déjà été joué : l'est mène au 4, plus loin.
        etat.morceaux_vus.insert(2);
        let mut m: Vec<i64> = suivants(&etat, &b.graphe, &acc)
            .iter()
            .map(|x| x.morceau)
            .collect();
        m.sort_unstable();
        assert_eq!(m, vec![3, 4]);
        // Le nord déjà parcouru : on ne le repropose pas tant que l'est reste.
        etat.sommets_vus.insert(b.s["n1"]);
        let m: Vec<i64> = suivants(&etat, &b.graphe, &acc)
            .iter()
            .map(|x| x.morceau)
            .collect();
        assert_eq!(m, vec![4]);
        // Tout joué : plus rien.
        etat.morceaux_vus.extend([3, 4]);
        assert!(suivants(&etat, &b.graphe, &acc).is_empty());
    }

    #[test]
    fn avancer_garde_le_sens_de_marche() {
        let b = banc();
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        let br = Branche {
            sommets: vec![b.s["c"], b.s["e1"], b.s["e2"]],
            morceau: 7,
            cout: 0,
        };
        etat.avancer(&br);
        assert_eq!(etat.sommet, b.s["e2"]);
        assert_eq!(etat.precedent, Some(b.s["e1"]));
        assert!(etat.morceaux_vus.contains(&7));
        assert!(etat.sommets_vus.contains(&b.s["e1"]));
        // Sur place : ni le sommet ni le sens ne changent.
        etat.avancer(&Branche {
            sommets: vec![b.s["e2"]],
            morceau: 8,
            cout: 0,
        });
        assert_eq!(etat.sommet, b.s["e2"]);
        assert_eq!(etat.precedent, Some(b.s["e1"]));
    }

    #[test]
    fn la_pente_continue_tout_droit_sinon_le_plus_proche() {
        let b = banc();
        let acc = accrochage(&[
            (1, b.s["c"]),
            (20, b.s["e1"]),
            (30, b.s["n1"]),
            (40, b.s["s1"]),
        ]);
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        etat.precedent = Some(b.s["o1"]);
        let br = suivants(&etat, &b.graphe, &acc);
        // Arrivé par l'avenue : on la suit, même si le nord sonne plus proche.
        let proche_nord = |id: i64| if id == 30 { 0.1 } else { 0.9 };
        let i = pente_simple(&etat, &br, &b.graphe, proche_nord).unwrap();
        assert_eq!(br[i].morceau, 20);
        // Arrivé du sud (l'impasse) : la rue ne continue pas, le plus proche gagne.
        etat.precedent = Some(b.s["s1"]);
        let br = suivants(&etat, &b.graphe, &acc);
        let i = pente_simple(&etat, &br, &b.graphe, proche_nord).unwrap();
        assert_eq!(br[i].morceau, 30);
        assert_eq!(pente_simple(&etat, &[], &b.graphe, proche_nord), None);
    }
}
