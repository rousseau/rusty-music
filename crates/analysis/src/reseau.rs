// SPDX-License-Identifier: GPL-3.0-or-later
//! Le réseau de circulation et ses profils d'itinéraire.
//!
//! `carto-google-maps.md` §2 et §3. Deux idées, et la seconde vient d'OSRM :
//!
//! 1. **une hiérarchie routière** — autoroute, nationale, secondaire, sentier —
//!    dérivée de la centralité d'intermédiarité et de l'importance des
//!    extrémités, les autoroutes suivant la ligne de crête de densité ;
//! 2. **un seul graphe, plusieurs fonctions de coût.** Le graphe ne change
//!    jamais ; seul le prix d'une arête change avec le profil demandé. C'est ce
//!    qui rend les quatre profils comparables, et ce qui évite d'entretenir
//!    quatre structures qui divergeraient.
//!
//! **Aucun plus court chemin n'est écrit ici.** `pathfinding` fournit A*,
//! Dijkstra et Yen ; `rustworkx-core` la centralité de Brandes ; `petgraph`
//! l'arbre couvrant minimal. Ce module décide seulement ce que coûte une arête
//! et quand s'arrêter.

use std::collections::{HashMap, HashSet};

use pathfinding::prelude::{astar, dijkstra, dijkstra_all, yen};
use rustworkx_core::centrality::{betweenness_centrality, edge_betweenness_centrality};

use crate::chemin::{Empreinte, Graphe};

/// Au-delà de ce nombre de nœuds, `rustworkx-core` parallélise Brandes.
const SEUIL_PARALLELE: usize = 50;

/// Les coûts de `pathfinding` doivent être entiers et ordonnés (`C: Ord`).
/// Les distances soniques vivent dans `[0, 2]` : un million de crans y laisse
/// six chiffres significatifs, bien au-delà de ce qui distingue deux
/// transitions.
const ECHELLE: f64 = 1_000_000.0;

/// Combien de candidats demander à Yen par itinéraire voulu, quand il faut
/// écarter ceux qui se répètent.
const ALTERNATIVES_PAR_CANDIDAT: usize = 8;
const ALTERNATIVES_MINIMUM: usize = 16;

/// Un plancher sur la popularité, pour que le profil « autoroute » ne divise
/// pas par zéro et que le « sentier » ne rende pas des arêtes gratuites.
const PLANCHER_POPULARITE: f64 = 0.05;

/// Ce que le réseau doit savoir d'un morceau. Le crate ne lit pas la base :
/// l'appelant rassemble, comme pour `density::calculer`.
#[derive(Debug, Clone)]
pub struct Morceau {
    pub id: i64,
    pub duree_ms: u64,
    /// Indice d'artiste — sert à ne pas choisir deux pôles chez le même.
    pub artiste: u32,
    pub famille: i64,
    /// Position sur la carte, pour lire la densité sous une arête.
    pub x: f32,
    pub y: f32,
    /// Nombre de morceaux de l'artiste dans la bibliothèque.
    ///
    /// **C'est la popularité dont on dispose.** `carto-google-maps.md` prévoit
    /// ListenBrainz et les compteurs de lecture locaux ; ni l'un ni l'autre
    /// n'existe, et la base ne porte aucun compteur d'écoute. Le nombre de
    /// morceaux gardés d'un artiste en est une approximation locale et
    /// honnête. Elle est normalisée en logarithme parce que la distribution
    /// s'étale de 1 à 769 : en échelle linéaire, tout le monde vaudrait zéro
    /// sauf trois artistes.
    pub morceaux_de_lartiste: u32,
}

/// La classe d'une arête dans la hiérarchie routière.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Classe {
    /// Sur l'arbre de crête reliant les grands pôles.
    Autoroute,
    /// Forte centralité d'intermédiarité : le voisinage qui porte le trafic.
    Nationale,
    /// Intra-territoire : l'exploration d'une famille.
    Secondaire,
    /// Le reste — les liens rares entre territoires, la longue traîne.
    Sentier,
}

impl Classe {
    /// Facteur appliqué quand on demande d'éviter les autoroutes.
    fn penalite_evitement(self) -> f64 {
        if self == Classe::Autoroute {
            8.0
        } else {
            1.0
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Arete {
    pub a: u32,
    pub b: u32,
    /// `1 − cosinus` entre les deux empreintes, comme le veut le document.
    pub distance: f32,
    /// Centralité d'intermédiarité de l'arête, normalisée dans `[0, 1]`.
    pub centralite: f32,
    pub classe: Classe,
}

/// À quelle échelle mesurer la centralité d'intermédiarité.
///
/// Brandes coûte `O(V·E)`. Sur 27 000 morceaux et leurs ~325 000 arêtes, cela
/// fait de l'ordre de 10¹⁰ opérations — le chiffre mesuré est dans
/// `docs/journal.md`. Le graphe des artistes, lui, tient en un millier de
/// nœuds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Echelle {
    /// Brandes sur le graphe des morceaux. Exact, et lent.
    Morceaux,
    /// Brandes sur le graphe **contracté des artistes** : une arête entre deux
    /// morceaux hérite de la centralité du couloir qui relie leurs artistes.
    ///
    /// Ce n'est pas qu'un raccourci de calcul. Le document dit « autoroute :
    /// relie les grands pôles » — et les pôles sont les artistes, pas les
    /// morceaux. Mesurer le couloir plutôt que le brin est plus proche de ce
    /// qu'on cherche.
    Artistes,
}

/// Réglages de la construction.
#[derive(Debug, Clone, Copy)]
pub struct Parametres {
    /// Voisins par morceau. Le document donne 8-16 ; le projet retient 12
    /// depuis `chemin::K_VOISINS`.
    pub k: usize,
    /// Fils pour le balayage des voisins.
    pub fils: usize,
    /// Nombre de pôles reliés par l'arbre de crête. Un pôle par artiste, les
    /// plus fournis d'abord.
    pub poles: usize,
    /// Quantile de centralité au-dessus duquel une arête devient nationale.
    pub quantile_nationale: f64,
    /// À quelle échelle mesurer la centralité.
    pub echelle: Echelle,
    /// Poids de la densité dans le coût de l'arbre de crête. À 0, l'arbre
    /// ignore le relief et relie les pôles au plus court sonique ; plus haut,
    /// il contourne les creux pour rester sur les hauteurs.
    pub attrait_crete: f64,
}

impl Default for Parametres {
    fn default() -> Self {
        Self {
            k: crate::chemin::K_VOISINS,
            fils: 8,
            poles: 80,
            echelle: Echelle::Artistes,
            quantile_nationale: 0.97,
            attrait_crete: 3.0,
        }
    }
}

/// Le réseau : le graphe des plus proches voisins, plus une classe et une
/// centralité par arête.
pub struct Reseau {
    ids: Vec<i64>,
    rang: HashMap<i64, u32>,
    morceaux: Vec<Morceau>,
    popularite: Vec<f64>,
    aretes: Vec<Arete>,
    /// Par rang : les indices d'arêtes qui le touchent.
    incidence: Vec<Vec<u32>>,
    /// Empreintes par rang — l'heuristique d'A* s'en sert.
    vecteurs: Vec<Vec<f32>>,
    rapport: RapportConstruction,
}

/// Ce qu'a coûté la construction — mesuré, pas estimé.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RapportConstruction {
    pub morceaux: usize,
    pub aretes: usize,
    pub ms_graphe: u128,
    pub ms_centralite: u128,
    pub ms_crete: u128,
    pub ms_total: u128,
    pub par_classe: Vec<(String, usize)>,
    pub refuges: usize,
}

impl Reseau {
    pub fn taille(&self) -> usize {
        self.ids.len()
    }
    pub fn aretes(&self) -> &[Arete] {
        &self.aretes
    }
    pub fn identifiants(&self) -> &[i64] {
        &self.ids
    }
    /// Popularité normalisée d'un morceau, dans `[0, 1]`.
    pub fn popularite(&self, id: i64) -> Option<f32> {
        self.rang.get(&id).map(|&r| self.popularite[r as usize] as f32)
    }

    /// Les tronçons du réseau en coordonnées de carte, prêts à devenir des
    /// tuiles : `(x1, y1, x2, y2, classe)`.
    ///
    /// Des n-uplets bruts et non un type partagé : `carto` ne doit pas
    /// dépendre de ce crate — il tirerait Burn et un modèle de 4 400 lignes
    /// générées pour dessiner des traits.
    pub fn troncons(&self) -> Vec<(f32, f32, f32, f32, u8)> {
        self.aretes
            .iter()
            .map(|a| {
                let (ma, mb) = (&self.morceaux[a.a as usize], &self.morceaux[a.b as usize]);
                let classe = match a.classe {
                    Classe::Autoroute => 0,
                    Classe::Nationale => 1,
                    Classe::Secondaire => 2,
                    Classe::Sentier => 3,
                };
                (ma.x, ma.y, mb.x, mb.y, classe)
            })
            .collect()
    }

    /// Les arêtes par identifiants de morceaux : `(a, b, classe)`.
    ///
    /// Sert à agréger le réseau par établissement — une route relie des lieux,
    /// et c'est la géométrie des lieux, non celle des morceaux, qui décide de
    /// ce qu'on peut dessiner.
    pub fn troncons_identifies(&self) -> Vec<(i64, i64, u8)> {
        self.aretes
            .iter()
            .map(|a| {
                let classe = match a.classe {
                    Classe::Autoroute => 0,
                    Classe::Nationale => 1,
                    Classe::Secondaire => 2,
                    Classe::Sentier => 3,
                };
                (self.ids[a.a as usize], self.ids[a.b as usize], classe)
            })
            .collect()
    }

    /// Les refuges isolés : les morceaux dont même le plus proche voisin est
    /// loin. `quantile` fixe ce que « loin » veut dire (0,99 = le centième le
    /// plus à l'écart).
    pub fn refuges(&self, quantile: f64) -> Vec<i64> {
        let mut meilleure = vec![f32::MAX; self.ids.len()];
        for a in &self.aretes {
            meilleure[a.a as usize] = meilleure[a.a as usize].min(a.distance);
            meilleure[a.b as usize] = meilleure[a.b as usize].min(a.distance);
        }
        let mut triees: Vec<f32> = meilleure.iter().copied().filter(|d| d.is_finite()).collect();
        if triees.is_empty() {
            return Vec::new();
        }
        triees.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        let seuil = triees[((triees.len() as f64 * quantile) as usize).min(triees.len() - 1)];
        self.ids
            .iter()
            .enumerate()
            .filter(|(r, _)| meilleure[*r] >= seuil)
            .map(|(_, id)| *id)
            .collect()
    }

    /// Construit le réseau.
    ///
    /// `champ` et `gn` viennent de `core::density::champ_global` : ils servent
    /// à faire suivre aux autoroutes la ligne de crête.
    pub fn construire(
        empreintes: Vec<Empreinte>,
        morceaux: &[Morceau],
        champ: &[f64],
        gn: usize,
        p: &Parametres,
    ) -> Self {
        let total = std::time::Instant::now();
        let t0 = std::time::Instant::now();
        let graphe = Graphe::construire(&empreintes, p.k, p.fils);
        let ms_graphe = t0.elapsed().as_millis();

        let ids: Vec<i64> = graphe.identifiants().to_vec();
        let rang: HashMap<i64, u32> = ids.iter().enumerate().map(|(r, i)| (*i, r as u32)).collect();

        // Les morceaux, remis dans l'ordre des rangs du graphe.
        let par_id: HashMap<i64, &Morceau> = morceaux.iter().map(|m| (m.id, m)).collect();
        let morceaux: Vec<Morceau> = ids
            .iter()
            .map(|id| {
                par_id.get(id).map(|m| (*m).clone()).unwrap_or(Morceau {
                    id: *id,
                    duree_ms: 0,
                    artiste: u32::MAX,
                    famille: -1,
                    x: 0.0,
                    y: 0.0,
                    morceaux_de_lartiste: 1,
                })
            })
            .collect();

        let max_artiste = morceaux
            .iter()
            .map(|m| m.morceaux_de_lartiste)
            .max()
            .unwrap_or(1)
            .max(1) as f64;
        let popularite: Vec<f64> = morceaux
            .iter()
            .map(|m| (m.morceaux_de_lartiste as f64 + 1.0).ln() / (max_artiste + 1.0).ln())
            .collect();

        // Les empreintes reprises par valeur et remises dans l'ordre des rangs :
        // elles servent à `1 − cosinus` maintenant, et à l'heuristique d'A*
        // ensuite. Les reprendre plutôt que les copier évite d'en garder deux
        // fois 55 Mo sur 27 000 morceaux.
        let vecteurs: Vec<Vec<f32>> = {
            let mut par_id: HashMap<i64, Vec<f32>> = empreintes.into_iter().collect();
            ids.iter()
                .map(|id| par_id.remove(id).unwrap_or_default())
                .collect()
        };

        let paires = graphe.aretes_uniques();
        let mut aretes: Vec<Arete> = paires
            .iter()
            .map(|&(a, b)| Arete {
                a,
                b,
                distance: 1.0 - cosinus(&vecteurs[a as usize], &vecteurs[b as usize]),
                centralite: 0.0,
                classe: Classe::Sentier,
            })
            .collect();

        let mut incidence: Vec<Vec<u32>> = vec![Vec::new(); ids.len()];
        for (i, a) in aretes.iter().enumerate() {
            incidence[a.a as usize].push(i as u32);
            incidence[a.b as usize].push(i as u32);
        }

        let mut reseau = Reseau {
            ids,
            rang,
            morceaux,
            popularite,
            aretes: std::mem::take(&mut aretes),
            incidence,
            vecteurs,
            rapport: RapportConstruction {
                ms_graphe,
                ..Default::default()
            },
        };
        reseau.classer(champ, gn, p);

        let mut comptes: HashMap<&str, usize> = HashMap::new();
        for a in &reseau.aretes {
            *comptes.entry(nom_classe(a.classe)).or_default() += 1;
        }
        let mut par_classe: Vec<(String, usize)> =
            comptes.into_iter().map(|(n, c)| (n.to_string(), c)).collect();
        par_classe.sort_by_key(|(n, _)| n.clone());

        reseau.rapport.morceaux = reseau.taille();
        reseau.rapport.aretes = reseau.aretes.len();
        reseau.rapport.par_classe = par_classe;
        reseau.rapport.refuges = reseau.refuges(0.99).len();
        reseau.rapport.ms_total = total.elapsed().as_millis();
        reseau
    }

    /// Ce que la construction a coûté.
    pub fn rapport(&self) -> &RapportConstruction {
        &self.rapport
    }

    /// Construit le réseau et rend au passage ce qu'il a coûté.
    pub fn construire_mesure(
        empreintes: Vec<Empreinte>,
        morceaux: &[Morceau],
        champ: &[f64],
        gn: usize,
        p: &Parametres,
    ) -> (Self, RapportConstruction) {
        let reseau = Self::construire(empreintes, morceaux, champ, gn, p);
        let rapport = reseau.rapport.clone();
        (reseau, rapport)
    }
}

fn nom_classe(c: Classe) -> &'static str {
    match c {
        Classe::Autoroute => "autoroute",
        Classe::Nationale => "nationale",
        Classe::Secondaire => "secondaire",
        Classe::Sentier => "sentier",
    }
}

/// Cosinus de deux empreintes. Le calculer plutôt que de reprendre la distance
/// euclidienne du graphe : celle-ci n'égale `2 − 2cos` que si les vecteurs sont
/// exactement unitaires, ce que rien ne garantit ici.
fn cosinus(a: &[f32], b: &[f32]) -> f32 {
    let (mut pv, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        pv += x * y;
        na += x * x;
        nb += y * y;
    }
    let d = (na.sqrt() * nb.sqrt()).max(f32::EPSILON);
    (pv / d).clamp(-1.0, 1.0)
}

// --- Hiérarchie routière ---------------------------------------------------

impl Reseau {
    /// Range les arêtes en quatre classes.
    ///
    /// L'ordre compte : l'arbre de crête passe en premier et l'emporte, parce
    /// qu'une autoroute doit être **continue**. Une classification purement
    /// par seuils rendrait des tronçons d'autoroute épars, ce qui ne se lit pas
    /// comme un réseau.
    fn classer(&mut self, champ: &[f64], gn: usize, p: &Parametres) {
        let t = std::time::Instant::now();
        self.centralite(p.echelle);
        self.rapport.ms_centralite = t.elapsed().as_millis();

        let t = std::time::Instant::now();
        let crete = self.arbre_de_crete(champ, gn, p);
        self.rapport.ms_crete = t.elapsed().as_millis();

        // Seuil des nationales : un quantile de la centralité, calculé sur les
        // arêtes qui ne sont pas déjà des autoroutes.
        let mut restantes: Vec<f32> = self
            .aretes
            .iter()
            .enumerate()
            .filter(|(i, _)| !crete.contains(&(*i as u32)))
            .map(|(_, a)| a.centralite)
            .collect();
        restantes.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        let seuil = if restantes.is_empty() {
            f32::MAX
        } else {
            let i = ((restantes.len() as f64 * p.quantile_nationale) as usize)
                .min(restantes.len() - 1);
            restantes[i]
        };

        for (i, a) in self.aretes.iter_mut().enumerate() {
            a.classe = if crete.contains(&(i as u32)) {
                Classe::Autoroute
            } else if a.centralite >= seuil {
                Classe::Nationale
            } else if self.morceaux[a.a as usize].famille == self.morceaux[a.b as usize].famille
                && self.morceaux[a.a as usize].famille >= 0
            {
                Classe::Secondaire
            } else {
                // Ce qui reste : les liens rares d'un territoire à l'autre et
                // la longue traîne — le sentier du document.
                Classe::Sentier
            };
        }
    }

    /// Centralité d'intermédiarité des arêtes (Brandes), par `rustworkx-core`.
    ///
    /// Non pondérée, et c'est voulu : on cherche quelles arêtes **portent le
    /// trafic** dans la structure du voisinage, pas lesquelles sont les plus
    /// courtes. Une autoroute d'un vrai réseau n'est pas le segment le plus
    /// court, c'est celui par lequel tout le monde passe.
    fn centralite(&mut self, echelle: Echelle) {
        match echelle {
            Echelle::Morceaux => self.centralite_des_morceaux(),
            Echelle::Artistes => self.centralite_des_artistes(),
        }
    }

    /// Brandes sur le graphe des morceaux : exact, `O(V·E)`.
    fn centralite_des_morceaux(&mut self) {
        use petgraph::graph::UnGraph;

        let mut g: UnGraph<(), ()> = UnGraph::with_capacity(self.ids.len(), self.aretes.len());
        let noeuds: Vec<_> = (0..self.ids.len()).map(|_| g.add_node(())).collect();
        // Les arêtes sont ajoutées dans l'ordre de `self.aretes` : l'indice
        // rendu par `edge_betweenness_centrality` est donc le nôtre.
        for a in &self.aretes {
            g.add_edge(noeuds[a.a as usize], noeuds[a.b as usize], ());
        }
        let brut = edge_betweenness_centrality(&g, true, SEUIL_PARALLELE);
        self.poser_centralites(|i| brut.get(i).and_then(|v| *v).unwrap_or(0.0));
    }

    /// Brandes sur le graphe contracté des artistes.
    ///
    /// Une arête entre deux morceaux du **même** artiste ne traverse aucun
    /// couloir : elle hérite alors de la centralité du nœud, pas d'une arête.
    /// Un artiste carrefour a des liens internes qui comptent ; un artiste de
    /// bout de chaîne, non.
    fn centralite_des_artistes(&mut self) {
        use petgraph::graph::{NodeIndex, UnGraph};

        let artistes: Vec<u32> = {
            let mut vus: Vec<u32> = self.morceaux.iter().map(|m| m.artiste).collect();
            vus.sort_unstable();
            vus.dedup();
            vus
        };
        let index: HashMap<u32, usize> =
            artistes.iter().enumerate().map(|(i, a)| (*a, i)).collect();

        let mut g: UnGraph<(), ()> = UnGraph::with_capacity(artistes.len(), artistes.len() * 8);
        let noeuds: Vec<NodeIndex> = artistes.iter().map(|_| g.add_node(())).collect();
        // Une seule arête par couple d'artistes, quel que soit le nombre de
        // brins qui les relient : la centralité mesure la structure, pas
        // l'épaisseur.
        let mut couloir: HashMap<(usize, usize), usize> = HashMap::new();
        for a in &self.aretes {
            let (ia, ib) = (
                index[&self.morceaux[a.a as usize].artiste],
                index[&self.morceaux[a.b as usize].artiste],
            );
            if ia == ib {
                continue;
            }
            let cle = (ia.min(ib), ia.max(ib));
            couloir.entry(cle).or_insert_with(|| {
                g.add_edge(noeuds[cle.0], noeuds[cle.1], ()).index()
            });
        }

        let par_couloir = edge_betweenness_centrality(&g, true, SEUIL_PARALLELE);
        let par_artiste = betweenness_centrality(&g, false, true, SEUIL_PARALLELE);
        // Les deux échelles ne sont pas comparables telles quelles : on ramène
        // la centralité de nœud dans la plage de celle des couloirs, sinon un
        // artiste carrefour écraserait toutes les arêtes réelles.
        let max_couloir = par_couloir
            .iter()
            .filter_map(|v| *v)
            .fold(0.0f64, f64::max)
            .max(f64::MIN_POSITIVE);
        let max_artiste = par_artiste
            .iter()
            .filter_map(|v| *v)
            .fold(0.0f64, f64::max)
            .max(f64::MIN_POSITIVE);

        let valeurs: Vec<f64> = self
            .aretes
            .iter()
            .map(|a| {
                let (ia, ib) = (
                    index[&self.morceaux[a.a as usize].artiste],
                    index[&self.morceaux[a.b as usize].artiste],
                );
                if ia == ib {
                    par_artiste[ia].unwrap_or(0.0) / max_artiste * max_couloir
                } else {
                    let cle = (ia.min(ib), ia.max(ib));
                    couloir
                        .get(&cle)
                        .and_then(|&e| par_couloir.get(e).copied().flatten())
                        .unwrap_or(0.0)
                }
            })
            .collect();
        self.poser_centralites(|i| valeurs[i]);
    }

    /// Normalise et pose les centralités, quelle que soit leur provenance.
    fn poser_centralites(&mut self, valeur: impl Fn(usize) -> f64) {
        let max = (0..self.aretes.len())
            .map(&valeur)
            .fold(0.0f64, f64::max)
            .max(f64::MIN_POSITIVE);
        for (i, a) in self.aretes.iter_mut().enumerate() {
            a.centralite = (valeur(i) / max) as f32;
        }
    }

    /// Les pôles : un morceau par grand artiste, le plus central de chez lui.
    fn poles(&self, combien: usize) -> Vec<u32> {
        let mut par_artiste: HashMap<u32, (u32, f64)> = HashMap::new();
        for (r, m) in self.morceaux.iter().enumerate() {
            if m.artiste == u32::MAX {
                continue;
            }
            // À artiste égal, on garde le morceau le mieux relié : c'est lui le
            // carrefour, pas le premier venu de la discographie.
            let centralite: f64 = self.incidence[r]
                .iter()
                .map(|&i| self.aretes[i as usize].centralite as f64)
                .sum();
            let e = par_artiste.entry(m.artiste).or_insert((r as u32, -1.0));
            if centralite > e.1 {
                *e = (r as u32, centralite);
            }
        }
        let mut candidats: Vec<(u32, u32)> = par_artiste
            .values()
            .map(|&(r, _)| (r, self.morceaux[r as usize].morceaux_de_lartiste))
            .collect();
        // Départage par rang : deux exécutions doivent rendre les mêmes pôles.
        candidats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        candidats.into_iter().take(combien).map(|(r, _)| r).collect()
    }

    /// L'arbre de crête : relie les pôles en suivant les hauteurs de densité.
    ///
    /// Un arbre de Steiner exact est NP-difficile. On prend l'approximation
    /// classique (Kou-Markowsky-Berman) : plus courts chemins entre pôles,
    /// arbre couvrant minimal sur ce graphe réduit, puis les chemins sont
    /// redéployés en arêtes réelles. Les deux briques viennent des crates —
    /// `pathfinding` pour les chemins, `petgraph` pour l'arbre.
    ///
    /// Le coût d'une arête y est sa distance sonique **majorée par le creux**
    /// qu'elle traverse : l'arbre contourne les vallées et reste sur les
    /// crêtes, et le réseau épouse alors le relief comme le veut le document.
    fn arbre_de_crete(&self, champ: &[f64], gn: usize, p: &Parametres) -> HashSet<u32> {
        use petgraph::algo::min_spanning_tree;
        use petgraph::data::FromElements;
        use petgraph::graph::UnGraph;

        let poles = self.poles(p.poles);
        if poles.len() < 2 {
            return HashSet::new();
        }

        // Coût « de crête » d'une arête : plus le milieu est creux, plus elle
        // coûte cher.
        let cout_crete = |i: u32| -> u64 {
            let a = &self.aretes[i as usize];
            let (ma, mb) = (&self.morceaux[a.a as usize], &self.morceaux[a.b as usize]);
            let densite = rusty_music_core::density::echantillonner(
                champ,
                gn,
                (ma.x + mb.x) / 2.0,
                (ma.y + mb.y) / 2.0,
            )
            .clamp(0.0, 1.0);
            let creux = 1.0 - densite;
            entier(a.distance as f64 * (1.0 + p.attrait_crete * creux))
        };
        let voisins = |&r: &u32| -> Vec<(u32, u64)> {
            self.incidence[r as usize]
                .iter()
                .map(|&i| {
                    let a = &self.aretes[i as usize];
                    let autre = if a.a == r { a.b } else { a.a };
                    (autre, cout_crete(i))
                })
                .collect()
        };

        // Première passe : distances de pôle à pôle. On ne garde pas les
        // arbres de plus courts chemins — 80 × 27 000 parents tiendraient
        // 130 Mo pour rien, la seconde passe les recalcule à la demande.
        let index: HashMap<u32, usize> = poles.iter().enumerate().map(|(i, r)| (*r, i)).collect();
        let mut g: UnGraph<(), u64> = UnGraph::with_capacity(poles.len(), poles.len() * 4);
        let noeuds: Vec<_> = poles.iter().map(|_| g.add_node(())).collect();
        for (i, &depart) in poles.iter().enumerate() {
            let atteints = dijkstra_all(&depart, voisins);
            for (cible, (_, cout)) in atteints {
                if let Some(&j) = index.get(&cible) {
                    if j > i {
                        g.add_edge(noeuds[i], noeuds[j], cout);
                    }
                }
            }
        }

        // Deuxième passe : l'arbre couvrant, puis les chemins redéployés.
        let arbre: UnGraph<(), u64> =
            UnGraph::from_elements(min_spanning_tree(&g));
        let mut sur_la_crete = HashSet::new();
        for arete in arbre.edge_indices() {
            let Some((u, v)) = arbre.edge_endpoints(arete) else {
                continue;
            };
            let (depart, arrivee) = (poles[u.index()], poles[v.index()]);
            let Some((chemin, _)) = dijkstra(&depart, voisins, |&n| n == arrivee) else {
                continue;
            };
            for paire in chemin.windows(2) {
                if let Some(i) = self.arete_entre(paire[0], paire[1]) {
                    sur_la_crete.insert(i);
                }
            }
        }
        sur_la_crete
    }

    /// L'indice de l'arête reliant deux rangs, s'il y en a une.
    fn arete_entre(&self, a: u32, b: u32) -> Option<u32> {
        self.incidence[a as usize]
            .iter()
            .copied()
            .find(|&i| {
                let e = &self.aretes[i as usize];
                (e.a == a && e.b == b) || (e.a == b && e.b == a)
            })
    }
}

/// Vrai si deux itinéraires partagent l'essentiel de leurs morceaux. Sert à
/// ne pas proposer trois variantes qui n'en sont pas.
fn se_ressemblent(a: &[i64], b: &[i64]) -> bool {
    let ensemble: HashSet<i64> = a.iter().copied().collect();
    let communs = b.iter().filter(|id| ensemble.contains(id)).count();
    communs * 4 >= a.len().min(b.len()) * 3
}

/// L'angle, en radians, correspondant à une distance `1 − cosinus`.
///
/// C'est la grandeur sur laquelle on route ; `1 − cos` est celle qu'on
/// rapporte. Les deux classent les arêtes dans le même ordre — `acos` est
/// décroissante — mais seule la première est une métrique.
fn angle(distance: f32) -> f32 {
    (1.0 - distance).clamp(-1.0, 1.0).acos()
}

/// Convertit un coût réel en entier, comme l'exigent les bornes de
/// `pathfinding` (`C: Zero + Ord + Copy`). Jamais zéro : une arête gratuite
/// autoriserait des boucles sans fin.
fn entier(v: f64) -> u64 {
    ((v.max(0.0) * ECHELLE) as u64).max(1)
}

// --- Profils et itinéraires ------------------------------------------------

/// Les quatre profils du document. **Trois sont des fonctions de coût sur le
/// même graphe** ; le quatrième, les étapes imposées, est le seul qui n'en
/// soit pas une — c'est un enchaînement de tronçons, comme les arrêts d'un
/// itinéraire routier.
#[derive(Debug, Clone)]
pub enum Profil {
    /// `distance ÷ popularité` — le trajet court, par les morceaux connus.
    Autoroute,
    /// `distance × popularité` — évite ce qui est déjà connu, pour redécouvrir.
    Sentier,
    /// Pénalise le maintien dans le même territoire : maximise la diversité de
    /// genres traversés.
    Panoramique,
    /// Étapes imposées, dans l'ordre, avec un coût neutre entre elles.
    Etapes(Vec<i64>),
}

impl Profil {
    /// Le plus petit facteur que ce profil puisse appliquer à une distance.
    ///
    /// Sert à l'heuristique d'A*, qui doit **minorer** le coût restant : la
    /// surestimer rendrait des trajets qui ne sont pas les moins chers, sans
    /// que rien ne le signale.
    fn facteur_minimal(&self) -> f64 {
        match self {
            // popularité ≤ 1, donc le diviseur vaut au plus 1 + plancher.
            Profil::Autoroute => 1.0 / (1.0 + PLANCHER_POPULARITE),
            // popularité ≥ 0, donc le multiplicateur vaut au moins le plancher.
            Profil::Sentier => PLANCHER_POPULARITE,
            Profil::Panoramique | Profil::Etapes(_) => 1.0,
        }
    }
}

/// Ce qu'on demande au routeur.
#[derive(Debug, Clone)]
pub struct Options {
    pub profil: Profil,
    pub depart: i64,
    /// Destination. `None` avec une durée cible : « quarante minutes à partir
    /// d'ici », sans point d'arrivée imposé.
    pub arrivee: Option<i64>,
    /// **Le paramètre le plus utile côté utilisateur** : la somme des durées
    /// des morceaux du trajet. Un « itinéraire de 40 minutes ».
    pub duree_cible_ms: Option<u64>,
    /// Écart toléré autour de la cible.
    pub tolerance_ms: u64,
    /// « Éviter les autoroutes » : contourne les morceaux les plus connus.
    pub eviter_autoroutes: bool,
    /// Nombre d'itinéraires proposés (Yen). 1 à 3, comme Google Maps.
    pub alternatives: usize,
}

impl Options {
    pub fn nouveau(depart: i64, profil: Profil) -> Self {
        Self {
            profil,
            depart,
            arrivee: None,
            duree_cible_ms: None,
            tolerance_ms: 90_000,
            eviter_autoroutes: false,
            alternatives: 1,
        }
    }
    pub fn vers(mut self, arrivee: i64) -> Self {
        self.arrivee = Some(arrivee);
        self
    }
    pub fn duree(mut self, ms: u64) -> Self {
        self.duree_cible_ms = Some(ms);
        self
    }
    pub fn alternatives(mut self, n: usize) -> Self {
        self.alternatives = n.clamp(1, 3);
        self
    }
}

/// Un itinéraire rendu.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Itineraire {
    /// Les morceaux traversés, du départ à l'arrivée.
    pub morceaux: Vec<i64>,
    /// Somme des durées — la « durée du trajet ».
    pub duree_ms: u64,
    /// Somme des `1 − cosinus` de chaque saut : ce que le trajet coûte à
    /// l'oreille.
    pub distance_sonique: f32,
    /// Popularité de chaque morceau, dans l'ordre — le profil d'altitude.
    pub popularite: Vec<f32>,
    /// La classe de chaque tronçon : `morceaux.len() - 1` valeurs.
    pub classes: Vec<Classe>,
}

#[derive(Debug, thiserror::Error)]
pub enum Erreur {
    #[error("morceau absent du réseau : {0}")]
    Inconnu(i64),
    #[error("aucun trajet ne relie ces morceaux")]
    Injoignable,
    #[error("aucun trajet ne tient dans la durée demandée")]
    HorsDuree,
}

impl Reseau {
    /// Le coût d'un saut, selon le profil. **C'est la seule chose qui change
    /// d'un profil à l'autre** : le graphe, lui, ne bouge jamais.
    ///
    /// La grandeur de base n'est pas `1 − cosinus` mais **l'angle** qu'il
    /// représente, et c'est une correction. `1 − cos` vaut la moitié du carré
    /// de la corde : ce n'est pas une distance, l'inégalité triangulaire n'y
    /// tient pas, et la somme de petits sauts y est bien plus faible que le
    /// saut direct. A* ne peut alors s'appuyer sur aucun minorant. L'angle,
    /// lui, est la géodésique de la sphère : une vraie métrique, et il classe
    /// les arêtes exactement dans le même ordre. `1 − cos` reste ce qu'on
    /// **rapporte** comme distance sonique — c'est la grandeur du document.
    fn cout(&self, o: &Options, arete: u32, vers: u32) -> u64 {
        let a = &self.aretes[arete as usize];
        let d = angle(a.distance) as f64;
        let pop = self.popularite[vers as usize];
        let base = match &o.profil {
            Profil::Autoroute => d / (PLANCHER_POPULARITE + pop),
            Profil::Sentier => d * (PLANCHER_POPULARITE + pop),
            Profil::Panoramique => {
                let depuis = if a.a == vers { a.b } else { a.a };
                // Rester dans le même territoire coûte le triple : le trajet
                // préfère alors changer de famille dès qu'il le peut.
                let meme = self.morceaux[depuis as usize].famille
                    == self.morceaux[vers as usize].famille;
                d * if meme { 3.0 } else { 1.0 }
            }
            Profil::Etapes(_) => d,
        };
        entier(base * if o.eviter_autoroutes { a.classe.penalite_evitement() } else { 1.0 })
    }

    /// Les voisins d'un rang et ce qu'ils coûtent, pour ce profil.
    fn successeurs(&self, o: &Options, r: u32) -> Vec<(u32, u64)> {
        self.incidence[r as usize]
            .iter()
            .map(|&i| {
                let a = &self.aretes[i as usize];
                let autre = if a.a == r { a.b } else { a.a };
                (autre, self.cout(o, i, autre))
            })
            .collect()
    }

    /// Minorant du coût restant jusqu'à `but` — l'heuristique d'A*.
    ///
    /// L'angle entre deux empreintes est la distance géodésique sur la sphère :
    /// il respecte l'inégalité triangulaire, donc la somme des angles le long
    /// d'un chemin ne peut pas descendre sous l'angle direct. Multiplié par le
    /// plus petit facteur que le profil puisse appliquer, on tient un minorant
    /// — la condition d'admissibilité.
    ///
    /// **La même construction sur `1 − cos` serait fausse** : la somme y est
    /// bien plus petite que le direct, l'heuristique majorerait, et A* rendrait
    /// des trajets plus chers que l'optimum sans rien signaler. Mesuré sur le
    /// corpus d'essai avant correction : 726 352 contre 344 551, plus du double.
    fn heuristique(&self, profil: &Profil, r: u32, but: u32) -> u64 {
        let cos = cosinus(&self.vecteurs[r as usize], &self.vecteurs[but as usize]);
        let direct = cos.clamp(-1.0, 1.0).acos() as f64;
        entier(direct * profil.facteur_minimal()).saturating_sub(1)
    }

    /// Calcule les itinéraires demandés.
    pub fn itineraires(&self, o: &Options) -> Result<Vec<Itineraire>, Erreur> {
        let depart = *self.rang.get(&o.depart).ok_or(Erreur::Inconnu(o.depart))?;

        if let Profil::Etapes(etapes) = &o.profil {
            return Ok(vec![self.par_etapes(o, depart, etapes)?]);
        }
        match o.duree_cible_ms {
            Some(cible) => self.par_duree(o, depart, cible),
            None => self.par_destination(o, depart),
        }
    }

    /// Trajet vers une destination, sans contrainte de durée : A*, et Yen si
    /// on veut des variantes.
    fn par_destination(&self, o: &Options, depart: u32) -> Result<Vec<Itineraire>, Erreur> {
        let arrivee = o
            .arrivee
            .and_then(|id| self.rang.get(&id).copied())
            .ok_or(Erreur::Injoignable)?;

        if o.alternatives <= 1 {
            let (chemin, _) = astar(
                &depart,
                |&r| self.successeurs(o, r),
                |&r| self.heuristique(&o.profil, r, arrivee),
                |&r| r == arrivee,
            )
            .ok_or(Erreur::Injoignable)?;
            return Ok(vec![self.decrire(&chemin)]);
        }

        // Yen ne prend pas d'heuristique : c'est un empilement de Dijkstra.
        let variantes = yen(
            &depart,
            |&r| self.successeurs(o, r),
            |&r| r == arrivee,
            o.alternatives,
        );
        if variantes.is_empty() {
            return Err(Erreur::Injoignable);
        }
        Ok(variantes.into_iter().map(|(c, _)| self.decrire(&c)).collect())
    }

    /// Trajet à durée cible — la contrainte prioritaire.
    ///
    /// **Un plus court chemin à coûts positifs ne repasse jamais par un nœud.**
    /// C'est acquis, et gratuitement : il suffit de ne pas mettre la durée dans
    /// l'état de recherche. Une première version le faisait — l'état portait le
    /// palier de durée écoulée — et rendait alors des *promenades* et non des
    /// chemins : sur la bibliothèque réelle, deux titres de reels irlandais
    /// alternés quatre fois, parce que rebondir entre deux voisins très proches
    /// est le moyen le moins cher de remplir quarante minutes. Interdire le
    /// demi-tour n'y suffisait pas : le cycle passait à trois morceaux.
    ///
    /// La durée est donc traitée **par le choix de la destination**, pas par le
    /// coût :
    ///
    /// - **sans arrivée imposée** — un seul arbre de plus courts chemins depuis
    ///   le départ (`dijkstra_all`), la durée cumulée le long de l'arbre, et on
    ///   garde la destination dont le trajet dure ce qu'on a demandé ;
    /// - **avec une arrivée imposée** — Yen énumère des chemins simples du
    ///   moins cher au plus cher, et on retient celui dont la durée colle.
    fn par_duree(&self, o: &Options, depart: u32, cible: u64) -> Result<Vec<Itineraire>, Erreur> {
        let ecart = |ms: u64| ms.abs_diff(cible);
        let mut candidats: Vec<Itineraire> = match o.arrivee.and_then(|id| self.rang.get(&id).copied())
        {
            Some(arrivee) => {
                let mut v: Vec<Itineraire> = yen(
                    &depart,
                    |&r| self.successeurs(o, r),
                    |&r| r == arrivee,
                    (o.alternatives * ALTERNATIVES_PAR_CANDIDAT).max(ALTERNATIVES_MINIMUM),
                )
                .into_iter()
                .map(|(c, _)| self.decrire(&c))
                .collect();
                v.sort_by_key(|i| ecart(i.duree_ms));
                v
            }
            None => {
                let arbre = dijkstra_all(&depart, |&r| self.successeurs(o, r));

                // Durée cumulée le long de l'arbre. Chaque nœud n'a qu'un
                // parent : la remontée est unique, et mémoïsée pour ne pas
                // reparcourir la même branche à chaque feuille.
                let mut duree: HashMap<u32, u64> = HashMap::with_capacity(arbre.len() + 1);
                duree.insert(depart, self.morceaux[depart as usize].duree_ms);
                let remonter = |n: u32, duree: &mut HashMap<u32, u64>| -> u64 {
                    let mut pile = Vec::new();
                    let mut courant = n;
                    while !duree.contains_key(&courant) {
                        let Some((parent, _)) = arbre.get(&courant) else {
                            break;
                        };
                        pile.push(courant);
                        courant = *parent;
                    }
                    let mut cumul = duree.get(&courant).copied().unwrap_or(0);
                    while let Some(n) = pile.pop() {
                        cumul += self.morceaux[n as usize].duree_ms;
                        duree.insert(n, cumul);
                    }
                    cumul
                };

                let mut classes: Vec<(u32, u64)> = arbre
                    .keys()
                    .copied()
                    .map(|n| {
                        let d = remonter(n, &mut duree);
                        (n, d)
                    })
                    .filter(|(_, d)| ecart(*d) <= o.tolerance_ms)
                    .collect();
                // Le plus proche de la cible d'abord ; à égalité, le plus
                // petit rang, pour que deux exécutions rendent le même trajet.
                classes.sort_by(|a, b| ecart(a.1).cmp(&ecart(b.1)).then_with(|| a.0.cmp(&b.0)));

                classes
                    .into_iter()
                    .filter_map(|(n, _)| {
                        let mut chemin = vec![n];
                        let mut courant = n;
                        while courant != depart {
                            let (parent, _) = arbre.get(&courant)?;
                            courant = *parent;
                            chemin.push(courant);
                        }
                        chemin.reverse();
                        Some(self.decrire(&chemin))
                    })
                    .collect()
            }
        };

        candidats.retain(|i| ecart(i.duree_ms) <= o.tolerance_ms);
        if candidats.is_empty() {
            return Err(Erreur::HorsDuree);
        }

        // Des variantes qui ne se ressemblent pas : proposer trois trajets qui
        // partagent leurs neuf premiers morceaux n'aide personne.
        let mut sortie: Vec<Itineraire> = Vec::new();
        for c in candidats {
            if sortie.iter().any(|d: &Itineraire| se_ressemblent(&d.morceaux, &c.morceaux)) {
                continue;
            }
            sortie.push(c);
            if sortie.len() >= o.alternatives.max(1) {
                break;
            }
        }
        Ok(sortie)
    }

    /// Trajet à étapes imposées : un tronçon par couple d'étapes consécutives.
    fn par_etapes(&self, o: &Options, depart: u32, etapes: &[i64]) -> Result<Itineraire, Erreur> {
        let mut jalons = vec![depart];
        for id in etapes {
            jalons.push(*self.rang.get(id).ok_or(Erreur::Inconnu(*id))?);
        }
        if let Some(a) = o.arrivee {
            jalons.push(*self.rang.get(&a).ok_or(Erreur::Inconnu(a))?);
        }

        let mut chemin: Vec<u32> = vec![jalons[0]];
        for paire in jalons.windows(2) {
            if paire[0] == paire[1] {
                continue;
            }
            let (tronçon, _) = astar(
                &paire[0],
                |&r| self.successeurs(o, r),
                |&r| self.heuristique(&o.profil, r, paire[1]),
                |&r| r == paire[1],
            )
            .ok_or(Erreur::Injoignable)?;
            chemin.extend_from_slice(&tronçon[1..]);
        }
        Ok(self.decrire(&chemin))
    }

    /// Met en forme un chemin de rangs : durée, distance sonique, profil de
    /// popularité, classes des tronçons.
    fn decrire(&self, chemin: &[u32]) -> Itineraire {
        let mut distance = 0.0f32;
        let mut classes = Vec::with_capacity(chemin.len().saturating_sub(1));
        for paire in chemin.windows(2) {
            match self.arete_entre(paire[0], paire[1]) {
                Some(i) => {
                    distance += self.aretes[i as usize].distance;
                    classes.push(self.aretes[i as usize].classe);
                }
                // N'arrive pas sur un chemin issu du graphe ; on ne ment pas
                // pour autant sur la distance.
                None => classes.push(Classe::Sentier),
            }
        }
        Itineraire {
            morceaux: chemin.iter().map(|&r| self.ids[r as usize]).collect(),
            duree_ms: chemin
                .iter()
                .map(|&r| self.morceaux[r as usize].duree_ms)
                .sum(),
            distance_sonique: distance,
            popularite: chemin
                .iter()
                .map(|&r| self.popularite[r as usize] as f32)
                .collect(),
            classes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un collier : soixante morceaux sur un cercle de l'espace des
    /// empreintes, découpé en trois arcs — les trois familles. Deux morceaux
    /// voisins sur le cercle sont proches ; les familles se touchent à leurs
    /// coutures, ce qui donne un graphe **connexe** avec de vraies frontières.
    ///
    /// Une première version plaçait chaque famille dans un plan orthogonal :
    /// c'était plus net à décrire, et le graphe des six plus proches voisins
    /// tombait en trois composantes disjointes. Aucun trajet entre familles
    /// n'existait, et six tests échouaient sur `Injoignable` — un défaut du
    /// corpus, pas du routage.
    ///
    /// La popularité suit la moitié du cercle : de quoi qu'il existe deux
    /// routes entre deux points opposés, l'une connue et l'autre non, et que
    /// les profils aient à choisir.
    fn corpus() -> (Vec<Empreinte>, Vec<Morceau>) {
        const N: usize = 60;
        let mut empreintes = Vec::new();
        let mut morceaux = Vec::new();
        for i in 0..N {
            let angle = i as f32 / N as f32 * std::f32::consts::TAU;
            let famille = i / 20;
            let mut v = vec![0.0f32; 24];
            v[0] = angle.cos();
            v[1] = angle.sin();
            // Un léger décalage par famille : assez pour les distinguer, pas
            // assez pour couper le collier à la couture.
            v[2 + famille] = 0.06;
            let id = i as i64 + 1;
            empreintes.push((id, v));
            morceaux.push(Morceau {
                id,
                // Des durées inégales : une cible en minutes ne doit pas se
                // ramener à un nombre de morceaux.
                duree_ms: 150_000 + (i as u64 % 5) * 40_000,
                artiste: (i / 5) as u32,
                famille: famille as i64,
                x: angle.cos() * 0.7,
                y: angle.sin() * 0.7,
                // Une moitié du cercle est connue, l'autre non.
                morceaux_de_lartiste: if i < N / 2 { 200 } else { 3 },
            });
        }
        (empreintes, morceaux)
    }

    fn reseau() -> Reseau {
        reseau_a(Echelle::Artistes)
    }

    fn reseau_a(echelle: Echelle) -> Reseau {
        let (empreintes, morceaux) = corpus();
        let points: Vec<(i64, f32, f32, i64)> = morceaux
            .iter()
            .map(|m| (m.id, m.x, m.y, m.famille))
            .collect();
        let parametres = rusty_music_core::density::ParametresDensite {
            noyau: 0.05,
            resolution: 64,
            bandes: 4,
        };
        let champ = rusty_music_core::density::champ_global(&points, &parametres);
        Reseau::construire(
            empreintes,
            &morceaux,
            &champ,
            64,
            &Parametres {
                k: 6,
                fils: 2,
                poles: 8,
                echelle,
                ..Default::default()
            },
        )
    }

    /// **La promesse centrale du document** : un seul graphe, plusieurs coûts.
    /// Le voisinage d'un morceau doit être identique quel que soit le profil ;
    /// seuls les prix changent.
    #[test]
    fn le_graphe_ne_change_pas_dun_profil_a_lautre() {
        let r = reseau();
        let depart = r.identifiants()[0];
        let profils = [
            Profil::Autoroute,
            Profil::Sentier,
            Profil::Panoramique,
            Profil::Etapes(vec![]),
        ];

        let mut voisinages = Vec::new();
        let mut couts = Vec::new();
        for p in profils {
            let o = Options::nouveau(depart, p);
            let s = r.successeurs(&o, 0);
            voisinages.push(s.iter().map(|(v, _)| *v).collect::<Vec<_>>());
            couts.push(s.iter().map(|(_, c)| *c).collect::<Vec<_>>());
        }
        for v in &voisinages[1..] {
            assert_eq!(
                &voisinages[0], v,
                "le voisinage change avec le profil : ce n'est plus un seul graphe"
            );
        }
        assert_ne!(
            couts[0], couts[1],
            "autoroute et sentier devraient facturer différemment"
        );
    }

    /// Autoroute passe par les morceaux connus, sentier les évite. On compare
    /// la popularité moyenne des deux trajets entre les deux mêmes bouts.
    #[test]
    fn autoroute_et_sentier_ne_prennent_pas_la_meme_route() {
        let r = reseau();
        // Deux morceaux **diamétralement opposés** sur le collier : les deux
        // demi-cercles qui les relient ont exactement la même longueur, et
        // seule leur popularité les distingue. Un couple quelconque laissait
        // la longueur décider, et les deux profils prenaient la même route.
        let (a, b) = (r.identifiants()[0], r.identifiants()[30]);
        let moyenne = |i: &Itineraire| {
            i.popularite.iter().sum::<f32>() / i.popularite.len() as f32
        };

        let par_autoroute =
            &r.itineraires(&Options::nouveau(a, Profil::Autoroute).vers(b)).unwrap()[0];
        let par_sentier =
            &r.itineraires(&Options::nouveau(a, Profil::Sentier).vers(b)).unwrap()[0];

        assert!(
            moyenne(par_autoroute) > moyenne(par_sentier),
            "autoroute {:.3} contre sentier {:.3} : les profils ne divergent pas",
            moyenne(par_autoroute),
            moyenne(par_sentier)
        );
    }

    /// L'heuristique d'A* doit **minorer** le coût restant. Si elle le
    /// surestime, A* rend un trajet plus cher que le meilleur sans que rien ne
    /// le dise — d'où la comparaison avec Dijkstra, qui n'a pas d'heuristique.
    #[test]
    fn lheuristique_reste_admissible() {
        let r = reseau();
        for &(i, j) in &[(0usize, 41usize), (7, 22), (13, 55), (30, 2)] {
            let (a, b) = (r.identifiants()[i], r.identifiants()[j]);
            let o = Options::nouveau(a, Profil::Autoroute).vers(b);
            let (rd, ra) = (r.rang[&a], r.rang[&b]);

            let (_, cout_astar) = astar(
                &rd,
                |&n| r.successeurs(&o, n),
                |&n| r.heuristique(&o.profil, n, ra),
                |&n| n == ra,
            )
            .expect("trajet attendu");
            let (_, cout_dijkstra) =
                dijkstra(&rd, |&n| r.successeurs(&o, n), |&n| n == ra).expect("trajet attendu");
            assert_eq!(
                cout_astar, cout_dijkstra,
                "A* ({i}→{j}) rend {cout_astar}, l'optimum est {cout_dijkstra}"
            );
        }
    }

    /// La contrainte prioritaire : « un itinéraire de N minutes ». La durée
    /// rendue est resommée sur les morceaux, pas déduite des paliers.
    #[test]
    fn la_duree_cible_est_tenue() {
        let r = reseau();
        let depart = r.identifiants()[0];
        for minutes in [10u64, 25, 40] {
            let cible = minutes * 60_000;
            let o = Options::nouveau(depart, Profil::Sentier).duree(cible);
            let trajets = r
                .itineraires(&o)
                .unwrap_or_else(|e| panic!("{minutes} min : {e}"));
            let t = &trajets[0];
            assert!(
                t.duree_ms.abs_diff(cible) <= o.tolerance_ms,
                "{minutes} min demandées, {:.1} rendues",
                t.duree_ms as f64 / 60_000.0
            );
            // La durée rendue doit être la somme réelle des morceaux retenus,
            // et non une reconstruction à partir des paliers de recherche.
            let somme: u64 = t
                .morceaux
                .iter()
                .map(|id| r.morceaux[r.rang[id] as usize].duree_ms)
                .sum();
            assert_eq!(t.duree_ms, somme, "la durée rendue n'est pas la vraie somme");
            assert_eq!(t.popularite.len(), t.morceaux.len());
            assert_eq!(t.classes.len(), t.morceaux.len() - 1);
        }
    }

    /// **Un itinéraire ne répète jamais un morceau.** La première version
    /// rendait, sur la bibliothèque réelle, deux titres alternés quatre fois :
    /// la recherche à durée cible cherchait un chemin le moins cher, et
    /// rebondir entre deux voisins proches était le moyen le moins cher de
    /// remplir le temps demandé.
    #[test]
    fn un_itineraire_ne_repete_aucun_morceau() {
        let r = reseau();
        let depart = r.identifiants()[0];
        for minutes in [10u64, 25, 40, 60] {
            let o = Options::nouveau(depart, Profil::Sentier).duree(minutes * 60_000);
            let Ok(trajets) = r.itineraires(&o) else { continue };
            for t in &trajets {
                let uniques: HashSet<_> = t.morceaux.iter().collect();
                assert_eq!(
                    uniques.len(),
                    t.morceaux.len(),
                    "{minutes} min : {} morceaux pour {} distincts — {:?}",
                    t.morceaux.len(),
                    uniques.len(),
                    t.morceaux
                );
            }
        }
    }

    /// Une durée cible avec destination : les deux contraintes tiennent
    /// ensemble.
    #[test]
    fn la_duree_et_la_destination_tiennent_ensemble() {
        let r = reseau();
        let (a, b) = (r.identifiants()[0], r.identifiants()[25]);
        let cible = 20 * 60_000;
        let o = Options::nouveau(a, Profil::Autoroute).vers(b).duree(cible);
        match r.itineraires(&o) {
            Ok(t) => {
                assert_eq!(*t[0].morceaux.last().unwrap(), b, "l'arrivée n'est pas la bonne");
                assert!(t[0].duree_ms.abs_diff(cible) <= o.tolerance_ms);
            }
            // Un refus explicite est une réponse acceptable : il n'existe pas
            // toujours de trajet qui satisfasse les deux à la fois.
            Err(Erreur::HorsDuree) => {}
            Err(e) => panic!("échec inattendu : {e}"),
        }
    }

    /// Les étapes imposées sont traversées, et dans l'ordre.
    #[test]
    fn les_etapes_sont_traversees_dans_lordre() {
        let r = reseau();
        let ids = r.identifiants();
        let (depart, arrivee) = (ids[0], ids[50]);
        let etapes = vec![ids[25], ids[35]];
        let o = Options {
            arrivee: Some(arrivee),
            ..Options::nouveau(depart, Profil::Etapes(etapes.clone()))
        };
        let t = &r.itineraires(&o).unwrap()[0];

        let mut curseur = 0;
        for etape in &etapes {
            let trouve = t.morceaux[curseur..]
                .iter()
                .position(|m| m == etape)
                .unwrap_or_else(|| panic!("étape {etape} absente du trajet"));
            curseur += trouve;
        }
        assert_eq!(t.morceaux[0], depart);
        assert_eq!(*t.morceaux.last().unwrap(), arrivee);
    }

    /// Panoramique doit traverser plus de familles qu'un trajet ordinaire
    /// entre les deux mêmes bouts.
    #[test]
    fn panoramique_traverse_plus_de_territoires() {
        let r = reseau();
        let (a, b) = (r.identifiants()[2], r.identifiants()[12]);
        let familles = |i: &Itineraire| -> usize {
            i.morceaux
                .iter()
                .map(|id| r.morceaux[r.rang[id] as usize].famille)
                .collect::<HashSet<_>>()
                .len()
        };
        let ordinaire = &r.itineraires(&Options::nouveau(a, Profil::Autoroute).vers(b)).unwrap()[0];
        let panorama = &r.itineraires(&Options::nouveau(a, Profil::Panoramique).vers(b)).unwrap()[0];
        assert!(
            familles(panorama) >= familles(ordinaire),
            "panoramique {} territoires, ordinaire {}",
            familles(panorama),
            familles(ordinaire)
        );
    }

    /// Yen doit rendre des itinéraires **distincts**, sans quoi proposer trois
    /// trajets n'a aucun intérêt.
    #[test]
    fn les_alternatives_sont_distinctes() {
        let r = reseau();
        let (a, b) = (r.identifiants()[1], r.identifiants()[45]);
        let t = r
            .itineraires(&Options::nouveau(a, Profil::Autoroute).vers(b).alternatives(3))
            .unwrap();
        assert!(t.len() >= 2, "une seule variante rendue");
        for i in 1..t.len() {
            assert_ne!(t[0].morceaux, t[i].morceaux, "variante {i} identique à la première");
        }
        // Yen classe par **coût du profil**, pas par distance sonique : un
        // trajet moins cher peut être sonorement plus long, puisque le coût
        // divise par la popularité. Ce qu'on peut affirmer, c'est que le
        // premier est l'optimum — le même que rend A* seul.
        let seul = &r
            .itineraires(&Options::nouveau(a, Profil::Autoroute).vers(b))
            .unwrap()[0];
        assert_eq!(
            t[0].morceaux, seul.morceaux,
            "le premier trajet de Yen devrait être celui d'A*"
        );
    }

    /// Toute arête reçoit une classe, et l'autoroute forme un ensemble
    /// connexe : c'est un arbre, pas des tronçons épars.
    #[test]
    fn les_autoroutes_forment_un_reseau_continu() {
        let r = reseau();
        let autoroutes: Vec<&Arete> = r
            .aretes()
            .iter()
            .filter(|a| a.classe == Classe::Autoroute)
            .collect();
        assert!(!autoroutes.is_empty(), "aucune autoroute");

        // Parcours du sous-graphe des autoroutes depuis une extrémité.
        let mut voisins: HashMap<u32, Vec<u32>> = HashMap::new();
        for a in &autoroutes {
            voisins.entry(a.a).or_default().push(a.b);
            voisins.entry(a.b).or_default().push(a.a);
        }
        let depart = autoroutes[0].a;
        let mut vus = HashSet::from([depart]);
        let mut pile = vec![depart];
        while let Some(n) = pile.pop() {
            for &v in voisins.get(&n).map(|v| v.as_slice()).unwrap_or(&[]) {
                if vus.insert(v) {
                    pile.push(v);
                }
            }
        }
        assert_eq!(
            vus.len(),
            voisins.len(),
            "le réseau d'autoroutes est en morceaux : {} nœuds atteints sur {}",
            vus.len(),
            voisins.len()
        );
    }

    /// Éviter les autoroutes doit effectivement en emprunter moins.
    #[test]
    fn eviter_les_autoroutes_les_evite() {
        let r = reseau();
        let (a, b) = (r.identifiants()[0], r.identifiants()[40]);
        let compter = |o: &Options| {
            r.itineraires(o).unwrap()[0]
                .classes
                .iter()
                .filter(|c| **c == Classe::Autoroute)
                .count()
        };
        let normal = Options::nouveau(a, Profil::Autoroute).vers(b);
        let evitant = Options {
            eviter_autoroutes: true,
            ..Options::nouveau(a, Profil::Autoroute).vers(b)
        };
        assert!(
            compter(&evitant) <= compter(&normal),
            "l'évitement en emprunte davantage : {} contre {}",
            compter(&evitant),
            compter(&normal)
        );
    }

    /// Les deux échelles de centralité doivent produire une hiérarchie
    /// utilisable : quatre classes peuplées, et un réseau d'autoroutes qui
    /// tient. Elles ne rendent pas les mêmes arêtes — c'est le but — mais
    /// aucune ne doit dégénérer.
    #[test]
    fn les_deux_echelles_de_centralite_tiennent() {
        for echelle in [Echelle::Morceaux, Echelle::Artistes] {
            let r = reseau_a(echelle);
            let n = |c: Classe| r.aretes().iter().filter(|a| a.classe == c).count();
            assert!(n(Classe::Autoroute) > 0, "{echelle:?} : aucune autoroute");
            assert!(n(Classe::Nationale) > 0, "{echelle:?} : aucune nationale");
            assert_eq!(
                n(Classe::Autoroute) + n(Classe::Nationale) + n(Classe::Secondaire) + n(Classe::Sentier),
                r.aretes().len(),
                "{echelle:?} : des arêtes sans classe"
            );
            assert!(
                r.aretes().iter().all(|a| (0.0..=1.0).contains(&a.centralite)),
                "{echelle:?} : centralité hors de [0, 1]"
            );
            // Et le routage reste possible d'un bout à l'autre.
            let (a, b) = (r.identifiants()[0], r.identifiants()[30]);
            assert!(r.itineraires(&Options::nouveau(a, Profil::Autoroute).vers(b)).is_ok());
        }
    }

    #[test]
    fn un_morceau_inconnu_est_refuse_clairement() {
        let r = reseau();
        assert!(matches!(
            r.itineraires(&Options::nouveau(999_999, Profil::Autoroute).vers(r.identifiants()[0])),
            Err(Erreur::Inconnu(999_999))
        ));
    }
}
