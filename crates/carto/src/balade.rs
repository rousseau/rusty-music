// SPDX-License-Identifier: GPL-3.0-or-later
//! La balade : la lecture est une marche dans les rues (`docs/plan-balade.md`,
//! P1).
//!
//! Le morceau suivant n'est pas dans une file calculée d'avance : il est
//! choisi au moment où on en a besoin, depuis la position courante. Ce module
//! ne tient que la partie pure — l'état de la marche et les branches qui
//! s'offrent ([`suivants`]), la pente par défaut ([`pente`], P3) et, au
//! carrefour (P2), les deux ou trois branches montrées ([`proposer`]).
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
    /// Carrefours passés (plus d'une branche) : l'allure *s'aventurer* prend
    /// « ailleurs » un carrefour sur trois. Compté par l'appelant, qui sait
    /// quand il quitte un carrefour.
    #[serde(default)]
    pub carrefours: u32,
}

impl EtatBalade {
    /// Une balade qui part de `morceau`, accroché au sommet `sommet`.
    pub fn depuis(sommet: u32, morceau: i64) -> EtatBalade {
        EtatBalade {
            sommet,
            precedent: None,
            sommets_vus: HashSet::from([sommet]),
            morceaux_vus: HashSet::from([morceau]),
            carrefours: 0,
        }
    }

    /// Revient à la position d'un carrefour passé (`avant`, l'état d'alors),
    /// en gardant tout ce qui a été vu depuis : ses branches déjà prises ne
    /// mènent plus au même morceau, ni par les mêmes rues tant qu'il y en a
    /// d'autres.
    pub fn revenir(&mut self, avant: &EtatBalade) {
        self.sommet = avant.sommet;
        self.precedent = avant.precedent;
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

/// La pente de P1, gardée comme référence du banc (`examples/continuite_balade.rs`) :
/// continuer tout droit si la rue continue — la branche qui reste sur le
/// tronçon par lequel on est arrivé —, sinon la branche dont le premier
/// morceau est le plus proche à l'oreille du morceau en cours (`distance`,
/// plus petit = plus proche). `None` si `branches` est vide. L'application
/// suit [`pente`].
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

/// L'allure de la balade : trois préréglages de la pente plutôt que cinq
/// curseurs (`docs/plan-balade.md`, P3), réglés une fois dans le rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Allure {
    /// Le plus proche à l'oreille d'abord : la balade reste dans son quartier.
    Rester,
    /// Proche et neuf à parts égales (défaut).
    #[default]
    Deriver,
    /// Le neuf d'abord, et un carrefour sur trois la voie la plus différente
    /// parmi les plus neuves.
    Aventurer,
}

/// Les poids de la pente : `p = proximité·a + nouveauté·b − déjà-visité·c −
/// usure·d + relation·e`, chaque terme entre 0 et 1 ([`Facettes`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Poids {
    pub proximite: f32,
    pub nouveaute: f32,
    pub deja_visite: f32,
    pub usure: f32,
    pub relation: f32,
}

impl Allure {
    pub const TOUTES: [Allure; 3] = [Allure::Rester, Allure::Deriver, Allure::Aventurer];

    /// Les poids réglés au banc (`examples/continuite_balade.rs`, E3 ;
    /// chiffres au journal, « P3 »).
    pub const fn poids(self) -> Poids {
        match self {
            Allure::Rester => Poids {
                proximite: 1.0,
                nouveaute: 0.1,
                deja_visite: 0.3,
                usure: 0.1,
                relation: 0.2,
            },
            Allure::Deriver => Poids {
                proximite: 1.0,
                nouveaute: 0.5,
                deja_visite: 0.5,
                usure: 0.3,
                relation: 0.2,
            },
            Allure::Aventurer => Poids {
                proximite: 0.3,
                nouveaute: 1.0,
                deja_visite: 0.6,
                usure: 0.5,
                relation: 0.1,
            },
        }
    }

    /// Un carrefour sur combien prend d'office la voie la plus différente.
    pub const fn ailleurs_tous_les(self) -> Option<u32> {
        match self {
            Allure::Aventurer => Some(3),
            _ => None,
        }
    }

    pub const fn nom(self) -> &'static str {
        match self {
            Allure::Rester => "rester",
            Allure::Deriver => "dériver",
            Allure::Aventurer => "s'aventurer",
        }
    }
}

/// Distance cosinus CLAP au-delà de laquelle un morceau ne sonne plus du tout
/// proche (proximité 0). Au carrefour, la voie la plus lointaine est en
/// médiane à 0,17 (`examples/carrefours_balade.rs`).
const DISTANCE_LOIN: f32 = 0.4;
/// Morceaux regardés au bout d'une branche pour sa nouveauté : son premier et
/// ceux qui le suivent dans la même rue.
const RIVERAINS: usize = 5;
/// Sommets déjà parcourus le long d'une branche ([`deja_vu`]) au-delà desquels
/// elle est tenue pour entièrement visitée.
const VU_PLEIN: usize = 6;
/// Une écoute de moins de trente jours use la rue.
pub const RECENT_S: i64 = 30 * 86_400;

/// Ce que la bibliothèque sait de l'écoute, au moment de la balade : table
/// `ecoute` et relations MusicBrainz entre artistes.
#[derive(Clone, Debug, Default)]
pub struct Memoire {
    /// Dernière écoute de chaque morceau écouté, secondes Unix.
    pub dernieres: HashMap<i64, i64>,
    /// Les artistes MusicBrainz des morceaux d'un artiste lié.
    pub artistes: HashMap<i64, Vec<String>>,
    /// Paires d'artistes liés, dans les deux sens.
    pub liens: HashSet<(String, String)>,
    /// L'heure de la balade, secondes Unix.
    pub maintenant: i64,
}

impl Memoire {
    /// Lit la mémoire d'écoute de `lib`. Une erreur de lecture laisse la
    /// partie concernée vide : la pente se passe alors de ce terme.
    pub fn lire(lib: &rusty_music_core::db::Library) -> Memoire {
        let dernieres = lib.dernieres_ecoutes().unwrap_or_default();
        let (artistes, liens) = lib.liens_entre_artistes().unwrap_or_default();
        let maintenant = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        Memoire {
            dernieres,
            artistes,
            liens,
            maintenant,
        }
    }

    pub fn jamais_ecoute(&self, id: i64) -> bool {
        !self.dernieres.contains_key(&id)
    }

    pub fn ecoute_recemment(&self, id: i64) -> bool {
        self.dernieres
            .get(&id)
            .is_some_and(|&le| self.maintenant - le < RECENT_S)
    }

    /// Les artistes de `a` et de `b` ont une relation MusicBrainz.
    pub fn relies(&self, a: i64, b: i64) -> bool {
        let (Some(x), Some(y)) = (self.artistes.get(&a), self.artistes.get(&b)) else {
            return false;
        };
        x.iter().any(|p| {
            y.iter()
                .any(|q| self.liens.contains(&(p.clone(), q.clone())))
        })
    }
}

/// Ce que la pente regarde, au morceau en cours.
pub struct Contexte<'a, D, F>
where
    D: Fn(i64) -> f32,
    F: Fn(i64) -> Option<i64>,
{
    pub courant: i64,
    pub memoire: &'a Memoire,
    /// Distance à l'oreille au morceau en cours, plus petit = plus proche.
    pub distance: D,
    /// La famille d'un morceau.
    pub famille: F,
}

/// Les termes de la pente d'une branche, chacun entre 0 et 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Facettes {
    /// 1 − distance du premier morceau au morceau en cours / [`DISTANCE_LOIN`].
    pub proximite: f32,
    /// Part de jamais écoutés parmi les [`RIVERAINS`] premiers morceaux.
    pub nouveaute: f32,
    /// Le déjà parcouru que la branche longe ([`deja_vu`] / [`VU_PLEIN`]).
    pub deja_visite: f32,
    /// Part d'écoutés récemment parmi les morceaux de la rue.
    pub usure: f32,
    /// L'artiste du premier morceau a une relation avec celui en cours.
    pub relation: bool,
}

impl Facettes {
    pub fn note(&self, p: &Poids) -> f32 {
        p.proximite * self.proximite + p.nouveaute * self.nouveaute
            - p.deja_visite * self.deja_visite
            - p.usure * self.usure
            + if self.relation { p.relation } else { 0.0 }
    }
}

/// Les morceaux au bout de la branche, dans l'ordre où la marche les
/// rencontrerait : son premier, puis les autres morceaux libres de son
/// sommet, puis ceux des sommets voisins qui ne sont pas sur la branche.
/// Au plus `n`.
fn riverains(
    etat: &EtatBalade,
    graphe: &Graphe,
    accrochage: &AccrochageVoirie,
    b: &Branche,
    n: usize,
) -> Vec<i64> {
    let mut v = vec![b.morceau];
    let bout = *b.sommets.last().expect("une branche a au moins un sommet");
    let sur_la_branche: HashSet<u32> = b.sommets.iter().copied().collect();
    let plus_loin = voisins(graphe, bout)
        .into_iter()
        .map(|(w, _)| w)
        .filter(|w| !sur_la_branche.contains(w));
    for s in std::iter::once(bout).chain(plus_loin) {
        for &id in accrochage.morceaux_a.get(&s).into_iter().flatten() {
            if v.len() >= n {
                return v;
            }
            if !etat.morceaux_vus.contains(&id) && !v.contains(&id) {
                v.push(id);
            }
        }
    }
    v
}

/// Les termes de la pente de la branche `b`.
pub fn facettes<D, F>(
    etat: &EtatBalade,
    graphe: &Graphe,
    accrochage: &AccrochageVoirie,
    b: &Branche,
    ctx: &Contexte<D, F>,
) -> Facettes
where
    D: Fn(i64) -> f32,
    F: Fn(i64) -> Option<i64>,
{
    let part = |ids: &[i64], f: &dyn Fn(i64) -> bool| -> f32 {
        if ids.is_empty() {
            return 0.0;
        }
        ids.iter().filter(|&&id| f(id)).count() as f32 / ids.len() as f32
    };
    let devant = riverains(etat, graphe, accrochage, b, RIVERAINS);
    // La rue : les morceaux de chaque sommet traversé, joués ou non, la
    // position courante exceptée.
    let mut rue: Vec<i64> = b
        .sommets
        .iter()
        .skip(usize::from(b.sommets.len() > 1))
        .flat_map(|s| accrochage.morceaux_a.get(s).into_iter().flatten().copied())
        .collect();
    rue.extend(devant.iter().copied());
    rue.sort_unstable();
    rue.dedup();
    Facettes {
        proximite: 1.0 - ((ctx.distance)(b.morceau) / DISTANCE_LOIN).clamp(0.0, 1.0),
        nouveaute: part(&devant, &|id| ctx.memoire.jamais_ecoute(id)),
        deja_visite: (deja_vu(etat, graphe, b) as f32 / VU_PLEIN as f32).min(1.0),
        usure: part(&rue, &|id| ctx.memoire.ecoute_recemment(id)),
        relation: ctx.memoire.relies(ctx.courant, b.morceau),
    }
}

/// La branche la plus différente parmi `candidats` : d'une autre famille que
/// le morceau en cours si l'on peut, la plus lointaine à l'oreille ensuite.
fn plus_differente(
    branches: &[Branche],
    candidats: &[usize],
    distance: impl Fn(i64) -> f32,
    famille: impl Fn(i64) -> Option<i64>,
    famille_courante: Option<i64>,
) -> Option<usize> {
    let loin = |&a: &usize, &b: &usize| {
        distance(branches[a].morceau)
            .total_cmp(&distance(branches[b].morceau))
            .then(b.cmp(&a))
    };
    let autre_famille = |i: &usize| {
        famille_courante.is_some_and(|f| famille(branches[*i].morceau).is_some_and(|g| g != f))
    };
    candidats
        .iter()
        .copied()
        .filter(autre_famille)
        .max_by(loin)
        .or_else(|| candidats.iter().copied().max_by(loin))
}

/// La pente par défaut (P3) : la branche de meilleure note
/// ([`Facettes::note`]) pour l'allure choisie — et, en *s'aventurer*, un
/// carrefour sur trois ([`EtatBalade::carrefours`]), le premier compris, la
/// plus différente : il n'y a qu'un carrefour et demi par heure de balade
/// (`examples/continuite_balade.rs`, E3).
/// `None` si `branches` est vide.
pub fn pente<D, F>(
    etat: &EtatBalade,
    branches: &[Branche],
    graphe: &Graphe,
    accrochage: &AccrochageVoirie,
    allure: Allure,
    ctx: &Contexte<D, F>,
) -> Option<usize>
where
    D: Fn(i64) -> f32,
    F: Fn(i64) -> Option<i64>,
{
    pente_reglee(
        etat,
        branches,
        graphe,
        accrochage,
        &allure.poids(),
        allure.ailleurs_tous_les(),
        ctx,
    )
}

/// [`pente`] avec des poids explicites, pour le banc qui les règle.
pub fn pente_reglee<D, F>(
    etat: &EtatBalade,
    branches: &[Branche],
    graphe: &Graphe,
    accrochage: &AccrochageVoirie,
    poids: &Poids,
    ailleurs_tous_les: Option<u32>,
    ctx: &Contexte<D, F>,
) -> Option<usize>
where
    D: Fn(i64) -> f32,
    F: Fn(i64) -> Option<i64>,
{
    if branches.len() <= 1 {
        return (!branches.is_empty()).then_some(0);
    }
    let f: Vec<Facettes> = branches
        .iter()
        .map(|b| facettes(etat, graphe, accrochage, b, ctx))
        .collect();
    if ailleurs_tous_les.is_some_and(|n| n > 0 && etat.carrefours % n == 0) {
        // La plus différente parmi les plus neuves : s'aventurer, c'est
        // d'abord le neuf.
        let neuve = f.iter().map(|x| x.nouveaute).fold(0.0f32, f32::max);
        let neuves: Vec<usize> = (0..branches.len())
            .filter(|&i| f[i].nouveaute >= neuve)
            .collect();
        return plus_differente(
            branches,
            &neuves,
            &ctx.distance,
            &ctx.famille,
            (ctx.famille)(ctx.courant),
        );
    }
    let notes: Vec<f32> = f.iter().map(|x| x.note(poids)).collect();
    (0..branches.len()).max_by(|&a, &b| notes[a].total_cmp(&notes[b]).then(b.cmp(&a)))
}

/// Branches montrées au carrefour, au plus : au-delà de trois, on ne compare
/// plus, on survole (loi de Hick, `docs/plan-balade.md`).
pub const PROPOSEES_MAX: usize = 3;

/// Combien une branche longe le déjà parcouru : les sommets déjà vus parmi
/// ceux qu'elle traverse et leurs voisins immédiats, la position courante (et
/// ses voisins, dont celui d'où l'on vient) mise à part. Les branches de
/// [`suivants`] évitent le déjà vu quand elles le peuvent : c'est le
/// voisinage qui les départage.
pub fn deja_vu(etat: &EtatBalade, graphe: &Graphe, b: &Branche) -> usize {
    let origine = etat.sommet;
    let pres_origine: HashSet<u32> = voisins(graphe, origine)
        .into_iter()
        .map(|(w, _)| w)
        .chain([origine])
        .collect();
    let mut vus: HashSet<u32> = HashSet::new();
    for &s in b.sommets.iter().skip(1) {
        for v in voisins(graphe, s).into_iter().map(|(w, _)| w).chain([s]) {
            if !pres_origine.contains(&v) && etat.sommets_vus.contains(&v) {
                vus.insert(v);
            }
        }
    }
    vus.len()
}

/// Les branches à montrer au carrefour, en indices de `branches`, la pente par
/// défaut (`defaut`) en tête. Jusqu'à [`PROPOSEES_MAX`] branches, toutes. Au
/// delà : la pente par défaut, la plus différente — d'une autre famille que
/// le morceau en cours d'abord, la plus lointaine à l'oreille ensuite —, puis
/// la plus proche à l'oreille si la pente n'est pas déjà elle (il y a
/// toujours une branche « proche » et une « ailleurs »), sinon la moins
/// visitée (`deja_vu`, en nombre de sommets).
///
/// `distance` : distance à l'oreille au morceau en cours, plus petit = plus
/// proche ; `famille` : la famille d'un morceau, `None` si inconnue.
pub fn proposer(
    branches: &[Branche],
    defaut: usize,
    distance: impl Fn(i64) -> f32,
    famille: impl Fn(i64) -> Option<i64>,
    famille_courante: Option<i64>,
    deja_vu: impl Fn(&Branche) -> usize,
) -> Vec<usize> {
    if branches.is_empty() {
        return Vec::new();
    }
    let defaut = defaut.min(branches.len() - 1);
    let mut retenues = vec![defaut];
    let reste = |retenues: &[usize]| -> Vec<usize> {
        (0..branches.len())
            .filter(|i| !retenues.contains(i))
            .collect()
    };
    if branches.len() <= PROPOSEES_MAX {
        retenues.extend(reste(&retenues));
        return retenues;
    }
    let ailleurs = plus_differente(
        branches,
        &reste(&retenues),
        &distance,
        &famille,
        famille_courante,
    );
    retenues.extend(ailleurs);
    // Une branche proche : la plus proche de toutes, si ce n'est pas la pente.
    let plus_proche = (0..branches.len()).min_by(|&a, &b| {
        distance(branches[a].morceau)
            .total_cmp(&distance(branches[b].morceau))
            .then(a.cmp(&b))
    });
    match plus_proche {
        Some(p) if !retenues.contains(&p) => retenues.push(p),
        _ => {
            let moins_vue = reste(&retenues)
                .into_iter()
                .min_by_key(|&i| (deja_vu(&branches[i]), i));
            retenues.extend(moins_vue);
        }
    }
    retenues
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

    /// Cinq branches synthétiques (les sommets ne servent qu'à `deja_vu`).
    fn cinq() -> Vec<Branche> {
        (0..5)
            .map(|i| Branche {
                sommets: vec![0, 10 + i as u32],
                morceau: 100 + i,
                cout: i as u64,
            })
            .collect()
    }

    #[test]
    fn jusqua_trois_branches_toutes_la_pente_en_tete() {
        let br = &cinq()[..3];
        let p = proposer(br, 2, |_| 0.5, |_| None, None, |_| 0);
        assert_eq!(p, vec![2, 0, 1]);
        assert!(proposer(&[], 0, |_| 0.5, |_| None, None, |_| 0).is_empty());
    }

    #[test]
    fn au_dela_de_trois_la_pente_la_plus_differente_et_une_proche() {
        let br = cinq();
        // Distances à l'oreille : 101 la plus proche, 104 la plus loin, mais
        // 102 seule d'une autre famille.
        let d = |id: i64| [0.3, 0.1, 0.5, 0.4, 0.9][(id - 100) as usize];
        let f = |id: i64| Some(if id == 102 { 7 } else { 1 });
        // La pente (tout droit) n'est pas la plus proche : la proche s'ajoute.
        let p = proposer(&br, 0, d, f, Some(1), |_| 0);
        assert_eq!(p, vec![0, 2, 1]);
        // Sans famille connue, la plus différente est la plus loin.
        let p = proposer(&br, 0, d, |_| None, Some(1), |_| 0);
        assert_eq!(p, vec![0, 4, 1]);
        // La pente est déjà la plus proche : la moins visitée complète.
        let vu = |b: &Branche| if b.morceau == 103 { 0 } else { 2 };
        let p = proposer(&br, 1, d, f, Some(1), vu);
        assert_eq!(p, vec![1, 2, 3]);
    }

    #[test]
    fn deja_vu_compte_le_voisinage_parcouru_sans_le_carrefour() {
        let b = banc();
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        etat.precedent = Some(b.s["o1"]);
        etat.sommets_vus.insert(b.s["o1"]);
        let est = Branche {
            sommets: vec![b.s["c"], b.s["e1"], b.s["e2"]],
            morceau: 2,
            cout: 0,
        };
        // L'arrivée (O1) et le carrefour ne comptent pas.
        assert_eq!(deja_vu(&etat, &b.graphe, &est), 0);
        // E3 déjà parcouru, voisin de E2 : la branche longe le vu.
        etat.sommets_vus.insert(b.s["e3"]);
        assert_eq!(deja_vu(&etat, &b.graphe, &est), 1);
    }

    #[test]
    fn revenir_au_carrefour_propose_les_branches_non_prises() {
        let b = banc();
        let acc = accrochage(&[
            (1, b.s["c"]),
            (20, b.s["e1"]),
            (30, b.s["n1"]),
            (40, b.s["s1"]),
        ]);
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        etat.precedent = Some(b.s["o1"]);
        let carrefour = etat.clone();
        let br = suivants(&etat, &b.graphe, &acc);
        assert_eq!(br.len(), 3);
        let est = br.iter().find(|x| x.morceau == 20).unwrap().clone();
        etat.avancer(&est);
        assert_eq!(etat.sommet, b.s["e1"]);
        // Retour au carrefour : l'est (déjà lu) n'est plus proposé.
        etat.revenir(&carrefour);
        assert_eq!(etat.sommet, b.s["c"]);
        let mut m: Vec<i64> = suivants(&etat, &b.graphe, &acc)
            .iter()
            .map(|x| x.morceau)
            .collect();
        m.sort_unstable();
        assert_eq!(m, vec![30, 40]);
    }

    /// Le carrefour C, arrivé de l'ouest : l'est (20), le nord (30), le sud (40).
    fn carrefour_trois() -> (Banc, AccrochageVoirie, EtatBalade) {
        let b = banc();
        let acc = accrochage(&[
            (1, b.s["c"]),
            (20, b.s["e1"]),
            (30, b.s["n1"]),
            (40, b.s["s1"]),
        ]);
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        etat.precedent = Some(b.s["o1"]);
        (b, acc, etat)
    }

    fn pris(
        b: &Banc,
        acc: &AccrochageVoirie,
        etat: &EtatBalade,
        allure: Allure,
        memoire: &Memoire,
        distance: impl Fn(i64) -> f32,
        famille: impl Fn(i64) -> Option<i64>,
    ) -> i64 {
        let br = suivants(etat, &b.graphe, acc);
        let ctx = Contexte {
            courant: 1,
            memoire,
            distance,
            famille,
        };
        br[pente(etat, &br, &b.graphe, acc, allure, &ctx).unwrap()].morceau
    }

    #[test]
    fn rester_suit_loreille_deriver_le_neuf_assez_proche() {
        let (b, acc, etat) = carrefour_trois();
        // L'est est le plus proche mais déjà écouté (il y a un an) ; le nord,
        // un peu moins proche, jamais écouté ; le sud, loin et neuf.
        let memoire = Memoire {
            dernieres: HashMap::from([(20, 0)]),
            maintenant: 365 * 86_400,
            ..Default::default()
        };
        let d = |id: i64| match id {
            20 => 0.10,
            30 => 0.16,
            _ => 0.5,
        };
        let f = |_| Some(1);
        assert_eq!(pris(&b, &acc, &etat, Allure::Rester, &memoire, d, f), 20);
        assert_eq!(pris(&b, &acc, &etat, Allure::Deriver, &memoire, d, f), 30);
        // Tout neuf : l'oreille départage, quelle que soit l'allure.
        let neuf = Memoire::default();
        for allure in [Allure::Rester, Allure::Deriver] {
            assert_eq!(pris(&b, &acc, &etat, allure, &neuf, d, f), 20);
        }
    }

    #[test]
    fn saventurer_prend_ailleurs_un_carrefour_sur_trois() {
        let (b, acc, mut etat) = carrefour_trois();
        let memoire = Memoire::default();
        let d = |id: i64| match id {
            20 => 0.05,
            30 => 0.10,
            _ => 0.30,
        };
        // Le nord seul est d'une autre famille.
        let f = |id: i64| Some(if id == 30 { 2 } else { 1 });
        etat.carrefours = 1;
        assert_eq!(pris(&b, &acc, &etat, Allure::Aventurer, &memoire, d, f), 20);
        for n in [0, 3] {
            etat.carrefours = n;
            assert_eq!(pris(&b, &acc, &etat, Allure::Aventurer, &memoire, d, f), 30);
        }
        // Les autres allures n'ont pas de tour « ailleurs ».
        assert_eq!(pris(&b, &acc, &etat, Allure::Deriver, &memoire, d, f), 20);
    }

    #[test]
    fn lusure_et_la_relation_departagent() {
        let (b, acc, etat) = carrefour_trois();
        let d = |id: i64| if id == 40 { 0.5 } else { 0.1 };
        let f = |_| Some(1);
        // L'est écouté hier : la rue est usée, le nord l'emporte.
        let mut memoire = Memoire {
            dernieres: HashMap::from([(20, 1000)]),
            maintenant: 1000 + 86_400,
            ..Default::default()
        };
        assert_eq!(pris(&b, &acc, &etat, Allure::Rester, &memoire, d, f), 30);
        // Sans écoute, égalité à l'oreille : l'artiste lié au morceau en
        // cours (1) l'emporte — l'est vient en tête sinon.
        memoire.dernieres.clear();
        assert_eq!(pris(&b, &acc, &etat, Allure::Rester, &memoire, d, f), 20);
        memoire.artistes = HashMap::from([(1, vec!["a".into()]), (30, vec!["b".into()])]);
        memoire.liens = HashSet::from([("a".into(), "b".into()), ("b".into(), "a".into())]);
        assert!(memoire.relies(1, 30) && !memoire.relies(1, 20));
        assert_eq!(pris(&b, &acc, &etat, Allure::Rester, &memoire, d, f), 30);
    }

    #[test]
    fn la_nouveaute_regarde_plus_loin_que_le_premier_morceau() {
        let b = banc();
        // L'est : 20 neuf, puis 21 et 22 en E2, écoutés.
        let acc = accrochage(&[
            (1, b.s["c"]),
            (20, b.s["e1"]),
            (21, b.s["e2"]),
            (22, b.s["e2"]),
        ]);
        let mut etat = EtatBalade::depuis(b.s["c"], 1);
        etat.precedent = Some(b.s["o1"]);
        let memoire = Memoire {
            dernieres: HashMap::from([(21, 0), (22, 0)]),
            maintenant: 365 * 86_400,
            ..Default::default()
        };
        let br = suivants(&etat, &b.graphe, &acc);
        assert_eq!(br.len(), 1);
        let ctx = Contexte {
            courant: 1,
            memoire: &memoire,
            distance: |_| 0.0,
            famille: |_| None,
        };
        let f = facettes(&etat, &b.graphe, &acc, &br[0], &ctx);
        assert!((f.nouveaute - 1.0 / 3.0).abs() < 1e-6, "{f:?}");
        assert_eq!(f.proximite, 1.0);
        assert_eq!(f.usure, 0.0);
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
