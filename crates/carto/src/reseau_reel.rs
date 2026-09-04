//! Un graphe routable sur le plan de ville réel — pour qu'un itinéraire
//! suive vraiment les rues, au lieu d'une ligne droite entre deux adresses.
//!
//! `docs/carto-etapes.md`, section « Hors périmètre (reporté) » : ce module
//! comble ce qui restait — construction du graphe, accrochage des adresses,
//! plus court chemin. Le graphe **musical** (kNN sur les empreintes,
//! `crates/analysis/src/reseau.rs`) reste inchangé et fournit toujours la
//! suite ordonnée de morceaux pour les modes direct/sonique/errance ; celui-ci
//! ne fait que dessiner le trait entre deux adresses le long de rues réelles,
//! que `rusty_music_analysis::chemin::dessine` peuple ensuite exactement
//! comme un tracé à la souris.

use std::collections::{HashMap, HashSet, VecDeque};

use pathfinding::prelude::{dijkstra, yen};

use rusty_music_osm::Extrait;

const RAYON_TERRE: f64 = 6_371_000.0;

/// Distance en mètres entre deux points `[lon, lat]` — approximation
/// équirectangulaire, la même que `Troncon::longueur_m` et
/// `affectation::Repere` : à l'échelle d'une ville, l'erreur est très
/// inférieure à celle du tracé lui-même.
fn distance_m(a: [f64; 2], b: [f64; 2]) -> f64 {
    let lat_moy = (a[1] + b[1]).to_radians() / 2.0;
    let dx = (b[0] - a[0]).to_radians() * lat_moy.cos() * RAYON_TERRE;
    let dy = (b[1] - a[1]).to_radians() * RAYON_TERRE;
    (dx * dx + dy * dy).sqrt()
}

/// Clé de nœud : un point `[lon, lat]` comparé bit à bit. Deux tronçons OSM
/// qui partagent un nœud décodent la même paire de flottants — `crates/osm`
/// ne les recalcule jamais — donc l'égalité exacte suffit, pas de tolérance
/// à choisir.
type Cle = (u64, u64);
fn cle(p: [f64; 2]) -> Cle {
    (p[0].to_bits(), p[1].to_bits())
}

/// Un graphe routable : un sommet par point distinct de tronçon, une arête
/// par segment consécutif, pondérée par sa longueur réelle.
///
/// Toutes les classes entrent, piétonnes et dessertes comprises — un
/// itinéraire se marche, contrairement au réseau *dessiné*
/// (`tuiles::classe_reelle_visible_des`), qui les cache pour la lisibilité,
/// pas pour le calcul.
pub struct Graphe {
    points: Vec<[f64; 2]>,
    aretes: Vec<Vec<(u32, u32)>>, // (voisin, poids en millimètres — `dijkstra` veut un coût entier)
    /// `true` si le sommet appartient à la plus grande composante connexe.
    ///
    /// Un extrait OSM comprend toujours quelques fragments isolés — une voie
    /// privée coupée du reste par le filtre `interessante`/`est_trottoir` de
    /// `crates/osm`, une impasse à cheval sur le bord de l'extrait. Y
    /// accrocher une adresse produirait un « aucun chemin trouvé » sur un
    /// trajet par ailleurs tout à fait ordinaire (mesuré : 235 218 sommets
    /// au total, 188 188 dans la composante principale, un fragment de 20
    /// sommets tout près de l'Étoile). [`Graphe::chemin`] n'accroche donc
    /// jamais en dehors de la plus grande.
    routable: Vec<bool>,
    /// Sommet → ids des tronçons OSM qui passent par lui (sans doublon). Sert
    /// à [`Graphe::troncons_traverses`] : nommer les *rues* qu'un tracé
    /// emprunte, pour l'étiquette de l'itinéraire.
    troncons_du_sommet: Vec<Vec<i64>>,
}

impl Graphe {
    /// Construit le graphe depuis les tronçons d'un extrait, arêtes pondérées
    /// par leur seule longueur réelle (millimètres).
    pub fn construire(extrait: &Extrait) -> Graphe {
        Graphe::construire_pondere(extrait, |_, _| 1.0)
    }

    /// Comme [`Graphe::construire`], mais chaque arête pèse `longueur ×
    /// friction(classe, milieu)` — pour un coût de déplacement qui court le
    /// long des grandes voies et traîne dans les impasses
    /// (`crate::cout_voirie`). `friction` reçoit la classe **et** le point
    /// milieu de l'arête (`[lon, lat]`, pour un terme organique optionnel) et
    /// renvoie un multiplicateur (1.0 = neutre, < 1 « rapide », > 1 « lent »).
    pub fn construire_pondere(
        extrait: &Extrait,
        friction: impl Fn(rusty_music_osm::Classe, [f64; 2]) -> f64,
    ) -> Graphe {
        fn sommet(
            p: [f64; 2],
            index: &mut HashMap<Cle, u32>,
            points: &mut Vec<[f64; 2]>,
            aretes: &mut Vec<Vec<(u32, u32)>>,
        ) -> u32 {
            *index.entry(cle(p)).or_insert_with(|| {
                points.push(p);
                aretes.push(Vec::new());
                (points.len() - 1) as u32
            })
        }

        let mut index: HashMap<Cle, u32> = HashMap::new();
        let mut points: Vec<[f64; 2]> = Vec::new();
        let mut aretes: Vec<Vec<(u32, u32)>> = Vec::new();
        let mut troncons_du_sommet: Vec<Vec<i64>> = Vec::new();

        for t in &extrait.troncons {
            for paire in t.points.windows(2) {
                let a = sommet(paire[0], &mut index, &mut points, &mut aretes);
                let b = sommet(paire[1], &mut index, &mut points, &mut aretes);
                // `troncons_du_sommet` suit la même croissance que `points`.
                while troncons_du_sommet.len() < points.len() {
                    troncons_du_sommet.push(Vec::new());
                }
                for s in [a, b] {
                    if !troncons_du_sommet[s as usize].contains(&t.id) {
                        troncons_du_sommet[s as usize].push(t.id);
                    }
                }
                if a == b {
                    continue;
                }
                let milieu = [(paire[0][0] + paire[1][0]) / 2.0, (paire[0][1] + paire[1][1]) / 2.0];
                let f = friction(t.classe, milieu).max(1e-3);
                let poids = (distance_m(paire[0], paire[1]) * 1000.0 * f).round().max(1.0) as u32;
                aretes[a as usize].push((b, poids));
                aretes[b as usize].push((a, poids));
            }
        }
        let routable = plus_grande_composante(&aretes);
        Graphe { points, aretes, routable, troncons_du_sommet }
    }

    /// Les sommets du graphe, en `[lon, lat]` — lecture seule, pour rasteriser
    /// un champ de coût (`crate::cout_voirie`).
    pub fn points(&self) -> &[[f64; 2]] {
        &self.points
    }

    /// Coût du plus court chemin de `source` (accrochée sur la plus grande
    /// composante) à **chaque** sommet, dans l'unité des arêtes (millimètres
    /// pondérés). `None` pour un sommet hors composante routable ou
    /// inatteignable.
    pub fn couts_depuis(&self, source: [f64; 2]) -> Vec<Option<u64>> {
        let mut cout = vec![None; self.points.len()];
        let Some(s) = self.plus_proche(source, Some(&self.routable)) else {
            return cout;
        };
        cout[s as usize] = Some(0);
        let atteints = pathfinding::prelude::dijkstra_all(&s, |&n| {
            self.aretes[n as usize].iter().map(|&(v, w)| (v, w as u64))
        });
        for (n, (_, c)) in atteints {
            cout[n as usize] = Some(c);
        }
        cout
    }

    pub fn est_vide(&self) -> bool {
        self.points.is_empty()
    }

    /// Sommets et arêtes — diagnostic seulement.
    pub fn taille(&self) -> (usize, usize) {
        (self.points.len(), self.aretes.iter().map(|v| v.len()).sum::<usize>() / 2)
    }

    /// Nombre de sommets atteignables depuis `depart` par une simple
    /// largeur — diagnostic pour juger de la connexité, pas pour le calcul.
    /// Accroche sur le sommet le plus proche **sans filtrer** par
    /// composante, contrairement à [`Graphe::chemin`] — c'est justement ce
    /// qui rend ce diagnostic capable de voir un petit fragment isolé.
    pub fn taille_composante(&self, depart: [f64; 2]) -> Option<usize> {
        let a = self.plus_proche(depart, None)?;
        let mut vus = vec![false; self.points.len()];
        let mut file = std::collections::VecDeque::from([a]);
        vus[a as usize] = true;
        let mut n = 0;
        while let Some(i) = file.pop_front() {
            n += 1;
            for &(j, _) in &self.aretes[i as usize] {
                if !vus[j as usize] {
                    vus[j as usize] = true;
                    file.push_back(j);
                }
            }
        }
        Some(n)
    }

    /// Le sommet le plus proche d'un point — l'accrochage d'une adresse sur
    /// le réseau. `dans` restreint la recherche à un sous-ensemble de
    /// sommets (la plus grande composante, pour [`Graphe::chemin`]) ; `None`
    /// cherche partout.
    ///
    /// Balayage complet : quelques centaines de milliers de sommets, une
    /// poignée de millisecondes en release. Un calcul déclenché par un clic
    /// ne justifie pas l'index spatial qu'un accrochage répété en boucle
    /// demanderait (même choix que `main.rs::selection`, en pire cas).
    fn plus_proche(&self, p: [f64; 2], dans: Option<&[bool]>) -> Option<u32> {
        self.points
            .iter()
            .enumerate()
            .filter(|(i, _)| match dans {
                Some(m) => m[*i],
                None => true,
            })
            .map(|(i, q)| (i as u32, distance_m(p, *q)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Le plus court chemin entre deux points géographiques, en `[lon, lat]`
    /// le long des rues.
    ///
    /// `None` si le graphe est vide, ou si aucun chemin n'existe entre les
    /// deux sommets accrochés — rare au centre d'une ville, mais une île sans
    /// pont dans l'extrait n'est pas à exclure.
    pub fn chemin(&self, depart: [f64; 2], arrivee: [f64; 2]) -> Option<Vec<[f64; 2]>> {
        let (sommets, _) = self.chemin_sommets(depart, arrivee)?;
        Some(sommets.into_iter().map(|i| self.points[i as usize]).collect())
    }

    /// Comme [`Graphe::chemin`], mais rend la suite d'**indices de sommets** et
    /// le coût total (unité des arêtes du graphe : millimètres, pondérés par la
    /// friction si le graphe vient de [`Graphe::construire_pondere`]). Les
    /// modes d'itinéraire en ont besoin pour savoir *quels sommets* — donc
    /// quels morceaux — le trajet longe, pas seulement sa géométrie.
    pub fn chemin_sommets(&self, depart: [f64; 2], arrivee: [f64; 2]) -> Option<(Vec<u32>, u64)> {
        let a = self.plus_proche(depart, Some(&self.routable))?;
        let b = self.plus_proche(arrivee, Some(&self.routable))?;
        if a == b {
            return Some((vec![a], 0));
        }
        let (chemin, cout) =
            dijkstra(&a, |n| self.aretes[*n as usize].iter().copied(), |n| *n == b)?;
        Some((chemin, cout as u64))
    }

    /// Jusqu'à `k` tracés simples distincts entre deux points, du moins cher au
    /// plus cher — les « itinéraires alternatifs » à la Google Maps
    /// (`pathfinding::yen`). `k` est ramené dans `1..=5` : au-delà, Yen paie
    /// cher pour des variantes qui se ressemblent.
    pub fn chemins_sommets_yen(
        &self,
        depart: [f64; 2],
        arrivee: [f64; 2],
        k: usize,
    ) -> Vec<(Vec<u32>, u64)> {
        let (Some(a), Some(b)) = (
            self.plus_proche(depart, Some(&self.routable)),
            self.plus_proche(arrivee, Some(&self.routable)),
        ) else {
            return Vec::new();
        };
        if a == b {
            return vec![(vec![a], 0)];
        }
        yen(
            &a,
            |n| self.aretes[*n as usize].iter().copied(),
            |n| *n == b,
            k.clamp(1, 5),
        )
        .into_iter()
        .map(|(chemin, cout)| (chemin, cout as u64))
        .collect()
    }

    /// `k` itinéraires **volontairement dispersés** entre deux points, coût
    /// réel croissant. Le premier est le plus court ; chacun des suivants est
    /// calculé après avoir renchéri (× `penalite`) les arêtes déjà empruntées,
    /// pour être poussé sur d'autres rues.
    ///
    /// Sur un graphe de voirie, les k plus courts chemins (Yen) ne diffèrent
    /// souvent que d'un pâté de maisons — inutilisable pour de vraies
    /// « variantes ». La méthode de pénalité (celle des GPS grand public) donne
    /// des trajets franchement différents. `penalite` ≈ 2,5 : une variante
    /// accepte un détour jusqu'à ~2,5× plus long pour éviter une rue déjà prise.
    pub fn itineraires_disperses(
        &self,
        depart: [f64; 2],
        arrivee: [f64; 2],
        k: usize,
        penalite: f64,
    ) -> Vec<(Vec<u32>, u64)> {
        let (Some(a), Some(b)) = (
            self.plus_proche(depart, Some(&self.routable)),
            self.plus_proche(arrivee, Some(&self.routable)),
        ) else {
            return Vec::new();
        };
        if a == b {
            return vec![(vec![a], 0)];
        }

        let arete = |x: u32, y: u32| if x < y { (x, y) } else { (y, x) };
        let cout_reel = |chemin: &[u32]| -> u64 {
            chemin
                .windows(2)
                .map(|w| {
                    self.aretes[w[0] as usize]
                        .iter()
                        .find(|(v, _)| *v == w[1])
                        .map_or(0, |(_, c)| *c as u64)
                })
                .sum()
        };

        let mut penalises: HashMap<(u32, u32), f64> = HashMap::new();
        let mut routes: Vec<(Vec<u32>, u64)> = Vec::new();
        // Deux fois plus de tentatives que de variantes voulues : certaines
        // seront écartées pour trop de recouvrement.
        let tentatives = k.clamp(1, 6) * 2;
        for _ in 0..tentatives {
            let penalises_ref = &penalises;
            let trouve = dijkstra(
                &a,
                |&n| {
                    self.aretes[n as usize].iter().map(move |&(v, w)| {
                        let f = penalises_ref.get(&arete(n, v)).copied().unwrap_or(1.0);
                        (v, ((w as f64) * f).round().max(1.0) as u64)
                    })
                },
                |&n| n == b,
            );
            let Some((chemin, _)) = trouve else { break };
            for w in chemin.windows(2) {
                *penalises.entry(arete(w[0], w[1])).or_insert(1.0) *= penalite;
            }
            if routes.iter().all(|(r, _)| !partagent_trop(r, &chemin)) {
                let c = cout_reel(&chemin);
                routes.push((chemin, c));
                if routes.len() >= k.max(1) {
                    break;
                }
            }
        }
        routes.sort_by_key(|(_, c)| *c);
        routes
    }

    /// Les voisins d'un sommet : `(voisin, poids)`.
    pub fn voisins(&self, sommet: u32) -> &[(u32, u32)] {
        &self.aretes[sommet as usize]
    }

    /// Le point `[lon, lat]` d'un sommet.
    pub fn point(&self, sommet: u32) -> [f64; 2] {
        self.points[sommet as usize]
    }

    /// Accroche un point `[lon, lat]` au sommet routable le plus proche — le
    /// même choix que [`Graphe::chemin`] (jamais un fragment isolé). Balayage
    /// linéaire : pour un accrochage en boucle, passer par [`IndexSommets`].
    pub fn accrocher(&self, p: [f64; 2]) -> Option<u32> {
        self.plus_proche(p, Some(&self.routable))
    }

    /// Les ids de tronçons OSM qu'un tracé emprunte, dans l'ordre de première
    /// visite. Pour chaque arête `(a, b)` consécutive, les tronçons communs aux
    /// deux sommets.
    pub fn troncons_traverses(&self, trace: &[u32]) -> Vec<i64> {
        let mut vus: HashSet<i64> = HashSet::new();
        let mut ordre = Vec::new();
        for paire in trace.windows(2) {
            for &t in &self.troncons_du_sommet[paire[0] as usize] {
                if self.troncons_du_sommet[paire[1] as usize].contains(&t) && vus.insert(t) {
                    ordre.push(t);
                }
            }
        }
        ordre
    }

    /// Le **couloir** d'un tracé : les sommets qu'un morceau accroché peut
    /// légitimement partager avec ce trajet, chacun étiqueté par un *rang*
    /// (position le long du tracé) qui sert à ordonner la playlist.
    ///
    /// - chaque sommet du tracé, au rang = sa position dans `trace` ;
    /// - les sommets à `rayon_m` mètres d'un sommet du tracé (largeur d'abord le
    ///   long des arêtes), au rang de leur sommet source — pour rattraper un
    ///   morceau accroché de l'autre côté de la rue ou sur une desserte que le
    ///   tracé longe.
    ///
    /// **On n'élargit pas à tout le tronçon.** Un « way » OSM peut faire des
    /// kilomètres : longer 200 m d'une avenue ne doit pas ramasser les adresses
    /// à son autre bout, sinon la playlist fait des allers-retours et le tracé
    /// dessiné, tronqué au dernier morceau, part en boucle.
    pub fn couloir(&self, trace: &[u32], rayon_m: f64) -> HashMap<u32, usize> {
        let mut couloir: HashMap<u32, usize> = HashMap::new();
        let mut dist: HashMap<u32, f64> = HashMap::new();
        let mut file: VecDeque<u32> = VecDeque::new();
        for (i, &s) in trace.iter().enumerate() {
            // Un sommet revu plus loin dans le tracé (boucle du plus court
            // chemin — rare mais possible) garde son **premier** rang.
            couloir.entry(s).or_insert(i);
            dist.insert(s, 0.0);
            file.push_back(s);
        }
        if rayon_m > 0.0 {
            while let Some(s) = file.pop_front() {
                let d0 = dist[&s];
                let rang = couloir[&s];
                for &(v, _) in &self.aretes[s as usize] {
                    let d = d0 + distance_m(self.points[s as usize], self.points[v as usize]);
                    if d <= rayon_m && dist.get(&v).is_none_or(|&dv| d + 1e-6 < dv) {
                        dist.insert(v, d);
                        couloir.entry(v).and_modify(|r| *r = (*r).min(rang)).or_insert(rang);
                        file.push_back(v);
                    }
                }
            }
        }
        couloir
    }

    /// Un index spatial des sommets routables, pour accrocher **beaucoup** de
    /// points d'un coup — les ~27 000 morceaux sur leurs adresses. Là où
    /// [`Graphe::accrocher`] balaie les 190 000 sommets à chaque appel (bon
    /// pour deux clics), [`IndexSommets::plus_proche`] ne regarde qu'une
    /// poignée de cellules.
    pub fn index_sommets(&self) -> IndexSommets {
        let pas = PAS_INDEX;
        let mut cellules: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, p) in self.points.iter().enumerate() {
            if self.routable[i] {
                let cle = ((p[0] / pas).floor() as i32, (p[1] / pas).floor() as i32);
                cellules.entry(cle).or_default().push(i as u32);
            }
        }
        IndexSommets { cellules, pas }
    }
}

/// Côté de cellule de [`IndexSommets`], en degrés — ~45 m en longitude à Paris,
/// ~65 m en latitude. Assez fin pour qu'une cellule ne contienne qu'une
/// poignée de sommets, assez large pour qu'un accrochage trouve son compte en
/// un ou deux anneaux.
const PAS_INDEX: f64 = 0.0006;

/// Grille de hachage spatiale sur les sommets routables d'un [`Graphe`].
/// Construite par [`Graphe::index_sommets`], valable pour **tout** graphe issu
/// du même extrait : `construire` et `construire_pondere` numérotent les
/// sommets dans le même ordre (première apparition), la friction ne le change
/// pas.
pub struct IndexSommets {
    cellules: HashMap<(i32, i32), Vec<u32>>,
    pas: f64,
}

impl IndexSommets {
    /// Le sommet routable le plus proche de `p` (`[lon, lat]`), ou `None` si le
    /// plus proche est au-delà de `portee_m` mètres (un morceau au fond d'un
    /// bois, hors de portée de toute voirie — il n'entrera dans aucun
    /// itinéraire, ce qui est le bon comportement).
    ///
    /// Anneaux de cellules élargis un à un ; on continue un anneau de plus
    /// après le premier succès, au cas où une cellule voisine encore non vue
    /// tiendrait un sommet plus proche. Cf. `cout_voirie::SemisSommets`, même
    /// principe, repère différent (mètres locaux là-bas).
    pub fn plus_proche(&self, graphe: &Graphe, p: [f64; 2], portee_m: f64) -> Option<u32> {
        let (cx, cy) = ((p[0] / self.pas).floor() as i32, (p[1] / self.pas).floor() as i32);
        let mut meilleur: Option<(u32, f64)> = None;
        for anneau in 0..40_i32 {
            for dx in -anneau..=anneau {
                for dy in -anneau..=anneau {
                    if anneau > 0 && dx.abs() != anneau && dy.abs() != anneau {
                        continue;
                    }
                    let Some(v) = self.cellules.get(&(cx + dx, cy + dy)) else { continue };
                    for &i in v {
                        let d = distance_m(p, graphe.points[i as usize]);
                        if meilleur.is_none_or(|(_, md)| d < md) {
                            meilleur = Some((i, d));
                        }
                    }
                }
            }
            if meilleur.is_some() && anneau > 0 {
                break;
            }
        }
        meilleur.filter(|&(_, d)| d <= portee_m).map(|(i, _)| i)
    }
}

/// Deux tracés se recouvrent-ils trop pour compter comme des variantes ? Vrai
/// s'ils partagent plus de 65 % des sommets du plus court des deux.
fn partagent_trop(a: &[u32], b: &[u32]) -> bool {
    if a.is_empty() || b.is_empty() {
        return true;
    }
    let ens: HashSet<u32> = a.iter().copied().collect();
    let communs = b.iter().filter(|s| ens.contains(s)).count();
    communs * 100 > a.len().min(b.len()) * 65
}

/// Marque les sommets de la plus grande composante connexe — largeur
/// d'abord depuis chaque sommet non encore vu, on garde la plus grosse.
fn plus_grande_composante(aretes: &[Vec<(u32, u32)>]) -> Vec<bool> {
    let n = aretes.len();
    let mut composante = vec![u32::MAX; n]; // à quelle composante appartient chaque sommet
    let mut tailles: Vec<usize> = Vec::new();

    for depart in 0..n {
        if composante[depart] != u32::MAX {
            continue;
        }
        let id = tailles.len() as u32;
        let mut taille = 0usize;
        let mut file = std::collections::VecDeque::from([depart as u32]);
        composante[depart] = id;
        while let Some(i) = file.pop_front() {
            taille += 1;
            for &(j, _) in &aretes[i as usize] {
                if composante[j as usize] == u32::MAX {
                    composante[j as usize] = id;
                    file.push_back(j);
                }
            }
        }
        tailles.push(taille);
    }

    let Some((plus_grande, _)) = tailles.iter().enumerate().max_by_key(|(_, t)| **t) else {
        return vec![false; n];
    };
    composante.iter().map(|&c| c == plus_grande as u32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::{Classe, Troncon};

    /// Trois rues en U : `Rue A` (nord-sud), `Rue B` (est-ouest, relie les
    /// deux extrémités nord), `Rue C` (nord-sud, parallèle à A). Le plus
    /// court chemin du pied de A au pied de C doit passer par B, pas à
    /// travers le vide entre A et C.
    fn extrait_en_u() -> Extrait {
        let troncons = vec![
            Troncon {
                id: 1,
                nom: Some("Rue A".into()),
                classe: Classe::Residentielle,
                points: vec![[2.30, 48.85], [2.30, 48.86]],
            },
            Troncon {
                id: 2,
                nom: Some("Rue B".into()),
                classe: Classe::Residentielle,
                points: vec![[2.30, 48.86], [2.32, 48.86]],
            },
            Troncon {
                id: 3,
                nom: Some("Rue C".into()),
                classe: Classe::Residentielle,
                points: vec![[2.32, 48.86], [2.32, 48.85]],
            },
        ];
        Extrait { troncons, ..Default::default() }
    }

    #[test]
    fn le_chemin_suit_les_rues_pas_le_vide() {
        let g = Graphe::construire(&extrait_en_u());
        let trace = g.chemin([2.30, 48.85], [2.32, 48.85]).expect("un chemin existe");
        // Le vol d'oiseau ferait ~1,5 km ; le détour en U par les trois rues
        // en fait nettement plus — c'est justement ce qui prouve que le
        // chemin ne coupe pas à travers le vide entre A et C.
        let mut longueur = 0.0;
        for f in trace.windows(2) {
            longueur += distance_m(f[0], f[1]);
        }
        let vol_doiseau = distance_m([2.30, 48.85], [2.32, 48.85]);
        assert!(longueur > vol_doiseau * 1.3, "chemin trop direct : {longueur} m vs {vol_doiseau} m à vol d'oiseau");
        // Passe bien par le sommet nord des deux rues (le coin de la rue B).
        assert!(trace.iter().any(|p| (p[0] - 2.30).abs() < 1e-9 && (p[1] - 48.86).abs() < 1e-9));
    }

    #[test]
    fn deux_troncons_partageant_un_noeud_se_relient() {
        let g = Graphe::construire(&extrait_en_u());
        // Trois rues, chacune un segment : au moins 4 sommets distincts (les
        // extrémités), mais les nœuds partagés ne doivent pas se dupliquer.
        assert_eq!(g.points.len(), 4, "les nœuds partagés doivent fusionner : {:?}", g.points);
    }

    #[test]
    fn un_graphe_vide_ne_fait_pas_tomber_lappelant() {
        let g = Graphe::construire(&Extrait::default());
        assert!(g.est_vide());
        assert!(g.chemin([2.3, 48.8], [2.4, 48.9]).is_none());
    }

    /// Un fragment isolé (une voie coupée du reste par les filtres d'import)
    /// ne doit jamais recevoir d'adresse : `chemin` doit accrocher sur la
    /// grande composante même quand le fragment est géographiquement plus
    /// proche du point demandé. Mesuré en vrai sur Paris : un fragment de 20
    /// sommets près de l'Étoile a fait échouer un trajet par ailleurs
    /// ordinaire avant ce correctif.
    #[test]
    fn un_fragment_isole_nattire_pas_laccrochage() {
        let mut extrait = extrait_en_u();
        // Un tronçon minuscule, très proche de l'arrivée visée, mais qui ne
        // touche aucune des trois rues en U.
        extrait.troncons.push(Troncon {
            id: 99,
            nom: Some("Voie privée".into()),
            classe: Classe::Service,
            points: vec![[2.3201, 48.8501], [2.3202, 48.8502]],
        });
        let g = Graphe::construire(&extrait);
        let (sommets, _) = g.taille();
        assert_eq!(sommets, 6, "quatre sommets du U plus les deux du fragment");

        // Viser un point tout près du fragment isolé (2,3201/48,8501) doit
        // quand même accrocher sur le U et trouver un chemin.
        let trace = g.chemin([2.30, 48.85], [2.3201, 48.8501]);
        assert!(trace.is_some(), "l'accrochage n'a pas dû se laisser attirer par le fragment isolé");
    }

    /// Une grille de rues 3×3 (deux rues nord-sud, deux rues est-ouest), pour
    /// éprouver Yen et les couloirs sur un graphe qui a de vraies alternatives.
    fn extrait_grille() -> Extrait {
        let xs = [2.30, 2.31, 2.32];
        let ys = [48.85, 48.86, 48.87];
        let mut troncons = Vec::new();
        let mut id = 1;
        for &x in &xs {
            troncons.push(Troncon {
                id,
                nom: Some(format!("Avenue {x}")),
                classe: Classe::Residentielle,
                points: ys.iter().map(|&y| [x, y]).collect(),
            });
            id += 1;
        }
        for &y in &ys {
            troncons.push(Troncon {
                id,
                nom: Some(format!("Rue {y}")),
                classe: Classe::Residentielle,
                points: xs.iter().map(|&x| [x, y]).collect(),
            });
            id += 1;
        }
        Extrait { troncons, ..Default::default() }
    }

    #[test]
    fn le_meme_ordre_de_sommets_quelle_que_soit_la_friction() {
        let e = extrait_grille();
        let simple = Graphe::construire(&e);
        let pondere = Graphe::construire_pondere(&e, |c, _| match c {
            Classe::Residentielle => 0.3,
            _ => 5.0,
        });
        assert_eq!(
            simple.points, pondere.points,
            "l'indexation des sommets doit être indépendante de la friction"
        );
    }

    #[test]
    fn chemin_sommets_rend_les_indices_et_le_cout() {
        let g = Graphe::construire(&extrait_en_u());
        let (sommets, cout) =
            g.chemin_sommets([2.30, 48.85], [2.32, 48.85]).expect("un chemin existe");
        assert_eq!(sommets.first().copied(), g.accrocher([2.30, 48.85]));
        assert_eq!(sommets.last().copied(), g.accrocher([2.32, 48.85]));
        assert!(cout > 0, "un trajet non trivial a un coût");
        // Le tracé passe par le coin nord-ouest (sommet du haut de la rue A).
        assert!(sommets.iter().any(|&s| g.point(s) == [2.30, 48.86]));
    }

    #[test]
    fn troncons_traverses_liste_les_trois_rues_du_u() {
        let g = Graphe::construire(&extrait_en_u());
        let (sommets, _) = g.chemin_sommets([2.30, 48.85], [2.32, 48.85]).unwrap();
        assert_eq!(g.troncons_traverses(&sommets), vec![1, 2, 3]);
    }

    #[test]
    fn couloir_suit_le_trace_et_ses_rangs_croissent() {
        let g = Graphe::construire(&extrait_en_u());
        let (sommets, _) = g.chemin_sommets([2.30, 48.85], [2.32, 48.85]).unwrap();
        let couloir = g.couloir(&sommets, 0.0);
        // Les quatre sommets du U sont sur le tracé.
        for coin in [[2.30, 48.85], [2.30, 48.86], [2.32, 48.86], [2.32, 48.85]] {
            let s = g.accrocher(coin).unwrap();
            assert!(couloir.contains_key(&s), "le couloir doit couvrir {coin:?}");
        }
        // Le rang du pied de A précède celui du pied de C.
        assert!(
            couloir[&g.accrocher([2.30, 48.85]).unwrap()]
                < couloir[&g.accrocher([2.32, 48.85]).unwrap()]
        );
    }

    #[test]
    fn couloir_ne_ramasse_pas_le_bout_lointain_dune_longue_avenue() {
        // Une avenue de 2 km (un seul tronçon), et une rue perpendiculaire qui
        // la coupe à 200 m de son début. Le trajet ne fait que traverser
        // l'avenue : le couloir ne doit PAS contenir les sommets de l'avenue
        // loin de la traversée.
        let avenue: Vec<[f64; 2]> = (0..=20).map(|i| [2.30 + i as f64 * 0.001, 48.86]).collect();
        let perp = vec![[2.302, 48.855], [2.302, 48.86], [2.302, 48.865]];
        let extrait = Extrait {
            troncons: vec![
                Troncon { id: 1, nom: Some("Avenue".into()), classe: Classe::Secondaire, points: avenue },
                Troncon { id: 2, nom: Some("Perp".into()), classe: Classe::Residentielle, points: perp },
            ],
            ..Default::default()
        };
        let g = Graphe::construire(&extrait);
        let (trace, _) = g.chemin_sommets([2.302, 48.855], [2.302, 48.865]).unwrap();
        let couloir = g.couloir(&trace, 25.0);
        // Le bout lointain de l'avenue (2 km à l'est) est hors couloir.
        let loin = g.accrocher([2.319, 48.86]).unwrap();
        assert!(!couloir.contains_key(&loin), "le couloir ne doit pas filer jusqu'au bout de l'avenue");
        // Le point de traversée, lui, y est.
        let croisement = g.accrocher([2.302, 48.86]).unwrap();
        assert!(couloir.contains_key(&croisement));
    }

    #[test]
    fn index_sommets_trouve_le_meme_sommet_que_le_balayage() {
        let g = Graphe::construire(&extrait_grille());
        let index = g.index_sommets();
        for cible in [[2.301, 48.851], [2.319, 48.869], [2.3155, 48.8602], [2.30, 48.87]] {
            assert_eq!(
                index.plus_proche(&g, cible, 500.0),
                g.accrocher(cible),
                "index et balayage doivent s'accorder sur {cible:?}"
            );
        }
    }

    #[test]
    fn index_sommets_rend_none_hors_portee() {
        let g = Graphe::construire(&extrait_grille());
        let index = g.index_sommets();
        assert!(index.plus_proche(&g, [2.5, 49.0], 200.0).is_none());
    }

    #[test]
    fn yen_rend_des_traces_distincts() {
        let g = Graphe::construire(&extrait_grille());
        let traces = g.chemins_sommets_yen([2.30, 48.85], [2.32, 48.87], 2);
        assert_eq!(traces.len(), 2, "la grille offre deux plus courts chemins de même longueur");
        assert_ne!(traces[0].0, traces[1].0);
    }

    #[test]
    fn itineraires_disperses_evitent_les_rues_deja_prises() {
        let g = Graphe::construire(&extrait_grille());
        let traces = g.itineraires_disperses([2.30, 48.85], [2.32, 48.87], 3, 2.5);
        assert!(traces.len() >= 2, "la grille permet plusieurs itinéraires distincts");
        // Aucun couple ne se recouvre trop.
        for i in 0..traces.len() {
            for j in i + 1..traces.len() {
                assert!(
                    !partagent_trop(&traces[i].0, &traces[j].0),
                    "les variantes {i} et {j} se ressemblent trop"
                );
            }
        }
        // Coûts croissants.
        assert!(traces.windows(2).all(|w| w[0].1 <= w[1].1));
    }
}
