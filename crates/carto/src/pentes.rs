// SPDX-License-Identifier: GPL-3.0-or-later
//! Les pentes entre quartiers (`docs/plan-balade.md`, P3) : les flèches
//! rouges de *The Naked City* (Debord, 1957) sur nos quartiers musicaux.
//!
//! Un **quartier** est une pièce d'un seul tenant de l'aplat d'une famille
//! (la grille de vote des habitants, `ville::grille_des_territoires`). Entre
//! deux quartiers voisins, la **proximité** est la proximité CLAP moyenne
//! (1 − distance cosinus) entre les morceaux qui habitent à moins de
//! [`RAYON_FRONTIERE_M`] de leur frontière commune, de part et d'autre.
//! Chaque quartier garde ses [`PAR_QUARTIER`] voisins les plus proches à
//! l'oreille : une flèche qui passe la couture, du milieu des morceaux de son
//! côté de la frontière au milieu de ceux d'en face — pas de cœur à cœur :
//! un quartier qui s'étire sur la moitié de la ville ferait converger toutes
//! ses voisines sur un seul point. Les flèches sont posées
//! de la plus forte à la plus faible, et une flèche qui en croiserait une
//! déjà posée cède la place au voisin suivant : le dessin ne se croise pas.
//!
//! Tout est en mètres du repère local ; l'appelant convertit.

use std::collections::{HashMap, HashSet, VecDeque};

/// Portée de la frontière : un morceau à moins de 150 m de la limite entre
/// deux quartiers en dit la couture.
pub const RAYON_FRONTIERE_M: f64 = 150.0;
/// Les pentes gardées par quartier : les plus fortes.
pub const PAR_QUARTIER: usize = 2;
/// Un quartier de moins d'habitants n'a pas de pente : un confetti de vote
/// n'est pas une unité d'ambiance.
pub const HABITANTS_MIN: usize = 25;
/// Morceaux au plus pris de chaque côté d'une frontière (régulièrement dans
/// l'ordre des identifiants) : la moyenne se stabilise bien avant.
const ECHANTILLON: usize = 40;
/// Morceaux au moins de chaque côté pour qu'une frontière compte.
const COTE_MIN: usize = 3;

/// La grille de vote des quartiers : la famille gagnante de chaque case,
/// `i64::MIN` pour une case vide.
pub struct Grille<'a> {
    pub gagnante: &'a [i64],
    /// Cases par côté.
    pub gn: usize,
    /// `[xmin, ymin, xmax, ymax]`, mètres.
    pub bornes: [f64; 4],
}

impl Grille<'_> {
    fn pas(&self) -> (f64, f64) {
        let [xmin, ymin, xmax, ymax] = self.bornes;
        (
            ((xmax - xmin) / self.gn as f64).max(1e-6),
            ((ymax - ymin) / self.gn as f64).max(1e-6),
        )
    }

    fn centre(&self, i: usize) -> [f64; 2] {
        let (px, py) = self.pas();
        [
            self.bornes[0] + ((i % self.gn) as f64 + 0.5) * px,
            self.bornes[1] + ((i / self.gn) as f64 + 0.5) * py,
        ]
    }

    fn case(&self, p: [f64; 2]) -> Option<usize> {
        let (px, py) = self.pas();
        let gx = ((p[0] - self.bornes[0]) / px).floor();
        let gy = ((p[1] - self.bornes[1]) / py).floor();
        let n = self.gn as f64;
        (gx >= 0.0 && gy >= 0.0 && gx < n && gy < n).then(|| gy as usize * self.gn + gx as usize)
    }

    /// Les voisines d'une case (4-connexité).
    fn voisines(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let (x, y, n) = (i % self.gn, i / self.gn, self.gn);
        [
            (x > 0).then(|| i - 1),
            (x + 1 < n).then(|| i + 1),
            (y > 0).then(|| i - n),
            (y + 1 < n).then(|| i + n),
        ]
        .into_iter()
        .flatten()
    }
}

/// Longueur d'une flèche, mètres : bornée pour qu'elle se lise à l'échelle
/// du quartier sans traverser la ville.
const LONGUEUR_MIN_M: f64 = 350.0;
const LONGUEUR_MAX_M: f64 = 700.0;

/// Une pente : d'un quartier vers un voisin.
#[derive(Clone, Debug, PartialEq)]
pub struct Pente {
    /// D'où part la flèche et où elle arrive, mètres : le milieu des morceaux
    /// de chaque côté de la frontière, à [`LONGUEUR_MIN_M`]–[`LONGUEUR_MAX_M`]
    /// l'un de l'autre.
    pub de: [f64; 2],
    pub vers: [f64; 2],
    pub famille_de: i64,
    pub famille_vers: i64,
    /// Proximité CLAP moyenne de part et d'autre de la frontière, 1 = même son.
    pub proximite: f32,
}

/// Les pièces d'un seul tenant de chaque famille : le quartier de chaque
/// case (`u32::MAX` pour une case vide), et la famille de chaque quartier.
fn quartiers(g: &Grille) -> (Vec<u32>, Vec<i64>) {
    let mut q = vec![u32::MAX; g.gagnante.len()];
    let mut familles = Vec::new();
    for depart in 0..g.gagnante.len() {
        if g.gagnante[depart] == i64::MIN || q[depart] != u32::MAX {
            continue;
        }
        let id = familles.len() as u32;
        let f = g.gagnante[depart];
        familles.push(f);
        q[depart] = id;
        let mut file = VecDeque::from([depart]);
        while let Some(i) = file.pop_front() {
            for j in g.voisines(i) {
                if q[j] == u32::MAX && g.gagnante[j] == f {
                    q[j] = id;
                    file.push_back(j);
                }
            }
        }
    }
    (q, familles)
}

/// Ramène `de → vers` à une longueur entre [`LONGUEUR_MIN_M`] et
/// [`LONGUEUR_MAX_M`], autour de son milieu.
fn borner(de: [f64; 2], vers: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    let (dx, dy) = (vers[0] - de[0], vers[1] - de[1]);
    let l = dx.hypot(dy);
    if l < 1e-6 {
        return (de, vers);
    }
    let cible = l.clamp(LONGUEUR_MIN_M, LONGUEUR_MAX_M);
    let k = cible / l / 2.0;
    let m = [(de[0] + vers[0]) / 2.0, (de[1] + vers[1]) / 2.0];
    (
        [m[0] - dx * k, m[1] - dy * k],
        [m[0] + dx * k, m[1] + dy * k],
    )
}

/// Au plus `n` éléments de `v`, pris régulièrement.
fn echantillon(v: &[i64], n: usize) -> Vec<i64> {
    if v.len() <= n {
        return v.to_vec();
    }
    (0..n).map(|k| v[k * v.len() / n]).collect()
}

/// Les pentes entre quartiers voisins. `habitants` : position (mètres) et
/// identifiant de chaque morceau logé ; `distance` : distance cosinus CLAP
/// entre deux morceaux, `None` si l'un n'a pas d'empreinte.
///
/// Ordre stable : par quartier de départ, puis pente décroissante.
pub fn entre_quartiers(
    g: &Grille,
    habitants: &[([f64; 2], i64)],
    distance: impl Fn(i64, i64) -> Option<f32>,
) -> Vec<Pente> {
    let (q, familles) = quartiers(g);
    let n = familles.len();
    if n < 2 {
        return Vec::new();
    }

    // Les cases de chaque quartier au bord d'un autre, avec ces autres.
    let mut face: HashMap<usize, Vec<u32>> = HashMap::new();
    for i in 0..q.len() {
        if q[i] == u32::MAX {
            continue;
        }
        let mut autres: Vec<u32> = g
            .voisines(i)
            .map(|j| q[j])
            .filter(|&k| k != u32::MAX && k != q[i])
            .collect();
        if !autres.is_empty() {
            autres.sort_unstable();
            autres.dedup();
            face.insert(i, autres);
        }
    }

    // Les habitants de chaque quartier.
    let mut loges: Vec<(usize, u32, i64, [f64; 2])> = Vec::new(); // (case, quartier, id, position)
    let mut effectif = vec![0usize; n];
    for &(p, id) in habitants {
        let Some(i) = g.case(p) else { continue };
        if q[i] == u32::MAX {
            continue;
        }
        effectif[q[i] as usize] += 1;
        loges.push((i, q[i], id, p));
    }
    let retenu = |k: u32| effectif[k as usize] >= HABITANTS_MIN;

    // Chaque habitant à moins de RAYON_FRONTIERE_M d'une case de son
    // quartier qui fait face à un autre : il en dit la couture.
    let (px, py) = g.pas();
    let (rx, ry) = (
        (RAYON_FRONTIERE_M / px).ceil() as i64,
        (RAYON_FRONTIERE_M / py).ceil() as i64,
    );
    let mut cote: HashMap<(u32, u32), Vec<i64>> = HashMap::new();
    let mut milieu: HashMap<(u32, u32), [f64; 3]> = HashMap::new(); // Σx, Σy, n
    for &(i, k, id, pos) in &loges {
        if !retenu(k) {
            continue;
        }
        let c = g.centre(i);
        let (x, y) = ((i % g.gn) as i64, (i / g.gn) as i64);
        let mut vus: HashSet<u32> = HashSet::new();
        for dy in -ry..=ry {
            for dx in -rx..=rx {
                let (xx, yy) = (x + dx, y + dy);
                if xx < 0 || yy < 0 || xx >= g.gn as i64 || yy >= g.gn as i64 {
                    continue;
                }
                let j = yy as usize * g.gn + xx as usize;
                let Some(autres) = face.get(&j).filter(|_| q[j] == k) else {
                    continue;
                };
                let d = g.centre(j);
                if (d[0] - c[0]).hypot(d[1] - c[1]) > RAYON_FRONTIERE_M {
                    continue;
                }
                for &a in autres {
                    if retenu(a) && vus.insert(a) {
                        cote.entry((k, a)).or_default().push(id);
                        let m = milieu.entry((k, a)).or_insert([0.0; 3]);
                        m[0] += pos[0];
                        m[1] += pos[1];
                        m[2] += 1.0;
                    }
                }
            }
        }
    }

    // La proximité de chaque frontière, une fois par paire.
    let mut proximite: HashMap<(u32, u32), f32> = HashMap::new();
    let mut paires: Vec<(u32, u32)> = cote.keys().copied().filter(|&(a, b)| a < b).collect();
    paires.sort_unstable();
    for (a, b) in paires {
        let (Some(xa), Some(xb)) = (cote.get(&(a, b)), cote.get(&(b, a))) else {
            continue;
        };
        if xa.len() < COTE_MIN || xb.len() < COTE_MIN {
            continue;
        }
        let (mut xa, mut xb) = (xa.clone(), xb.clone());
        xa.sort_unstable();
        xb.sort_unstable();
        let (ea, eb) = (echantillon(&xa, ECHANTILLON), echantillon(&xb, ECHANTILLON));
        let (mut somme, mut compte) = (0.0f64, 0usize);
        for &u in &ea {
            for &v in &eb {
                if let Some(d) = distance(u, v) {
                    somme += (1.0 - d) as f64;
                    compte += 1;
                }
            }
        }
        if compte > 0 {
            let p = (somme / compte as f64) as f32;
            proximite.insert((a, b), p);
            proximite.insert((b, a), p);
        }
    }

    // Les plus fortes d'abord ; au plus PAR_QUARTIER par quartier, sans
    // croiser une flèche déjà posée.
    let mut candidates: Vec<(u32, u32, f32)> =
        proximite.iter().map(|(&(a, b), &p)| (a, b, p)).collect();
    candidates.sort_by(|x, y| y.2.total_cmp(&x.2).then((x.0, x.1).cmp(&(y.0, y.1))));
    let mut posees: Vec<(Pente, Vec<[f64; 2]>)> = Vec::new();
    let mut nombre = vec![0usize; n];
    for (a, b, p) in candidates {
        if nombre[a as usize] >= PAR_QUARTIER {
            continue;
        }
        let centre = |k: (u32, u32)| {
            let m = milieu[&k];
            [m[0] / m[2], m[1] / m[2]]
        };
        let (de, vers) = borner(centre((a, b)), centre((b, a)));
        let pente = Pente {
            de,
            vers,
            famille_de: familles[a as usize],
            famille_vers: familles[b as usize],
            proximite: p,
        };
        let courbe = fleche(pente.de, pente.vers).0;
        if posees.iter().any(|(_, c)| se_croisent(c, &courbe)) {
            continue;
        }
        nombre[a as usize] += 1;
        posees.push((pente, courbe));
    }
    posees.sort_by(|x, y| {
        (x.0.de[1], x.0.de[0])
            .partial_cmp(&(y.0.de[1], y.0.de[0]))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(y.0.proximite.total_cmp(&x.0.proximite))
    });
    posees.into_iter().map(|(p, _)| p).collect()
}

/// Le dessin d'une pente, mètres : une courbe qui s'incurve sur sa droite
/// (deux pentes opposées entre les mêmes quartiers ne se recouvrent pas) et
/// s'arrête avant les cœurs, et la pointe de flèche, un triangle fermé.
pub fn fleche(de: [f64; 2], vers: [f64; 2]) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let (dx, dy) = (vers[0] - de[0], vers[1] - de[1]);
    let l = dx.hypot(dy).max(1e-6);
    // La droite du sens de marche, et le point de contrôle de la courbe.
    let (nx, ny) = (dy / l, -dx / l);
    let c = [
        de[0] + dx / 2.0 + nx * 0.2 * l,
        de[1] + dy / 2.0 + ny * 0.2 * l,
    ];
    let bezier = |t: f64| {
        let u = 1.0 - t;
        [
            u * u * de[0] + 2.0 * u * t * c[0] + t * t * vers[0],
            u * u * de[1] + 2.0 * u * t * c[1] + t * t * vers[1],
        ]
    };
    let (debut, fin) = (0.18, 0.82);
    let courbe: Vec<[f64; 2]> = (0..=16)
        .map(|k| bezier(debut + (fin - debut) * k as f64 / 16.0))
        .collect();
    let pointe = courbe[16];
    let avant = courbe[15];
    let (tx, ty) = (pointe[0] - avant[0], pointe[1] - avant[1]);
    let lt = tx.hypot(ty).max(1e-9);
    let (tx, ty) = (tx / lt, ty / lt);
    let long = (0.16 * l).clamp(30.0, 140.0);
    let large = long * 0.45;
    let base = [pointe[0] - tx * long, pointe[1] - ty * long];
    let tete = vec![
        pointe,
        [base[0] - ty * large, base[1] + tx * large],
        [base[0] + ty * large, base[1] - tx * large],
        pointe,
    ];
    // La courbe s'arrête au pied de la pointe.
    let mut courbe = courbe;
    *courbe.last_mut().expect("non vide") =
        [pointe[0] - tx * long * 0.6, pointe[1] - ty * long * 0.6];
    (courbe, tete)
}

/// Deux courbes se croisent (segments sécants, contact aux extrémités exclu).
pub fn se_croisent(c1: &[[f64; 2]], c2: &[[f64; 2]]) -> bool {
    fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    fn secants(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
        let (o1, o2) = (orient(a, b, c), orient(a, b, d));
        let (o3, o4) = (orient(c, d, a), orient(c, d, b));
        o1 * o2 < 0.0 && o3 * o4 < 0.0
    }
    let boite = |c: &[[f64; 2]]| {
        c.iter()
            .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            })
    };
    let (a, b) = (boite(c1), boite(c2));
    if a[2] < b[0] || b[2] < a[0] || a[3] < b[1] || b[3] < a[1] {
        return false;
    }
    c1.windows(2)
        .any(|s| c2.windows(2).any(|t| secants(s[0], s[1], t[0], t[1])))
}

/// Combien de paires de courbes se croisent.
pub fn croisements(courbes: &[Vec<[f64; 2]>]) -> usize {
    let mut n = 0;
    for i in 0..courbes.len() {
        for j in i + 1..courbes.len() {
            n += usize::from(se_croisent(&courbes[i], &courbes[j]));
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une bande de 30 × 10 cases de 50 m : trois quartiers côte à côte —
    /// famille 1 (x < 10), famille 2 (10 ≤ x < 20), famille 1 encore
    /// (x ≥ 20, un second quartier de la même famille).
    fn bande() -> (Vec<i64>, usize) {
        let gn = 30;
        let mut g = vec![i64::MIN; gn * gn];
        for y in 0..10 {
            for x in 0..gn {
                g[y * gn + x] = if (10..20).contains(&x) { 2 } else { 1 };
            }
        }
        (g, gn)
    }

    /// Un habitant par case habitée, identifiant = case.
    fn habitants(g: &Grille) -> Vec<([f64; 2], i64)> {
        (0..g.gagnante.len())
            .filter(|&i| g.gagnante[i] != i64::MIN)
            .map(|i| (g.centre(i), i as i64))
            .collect()
    }

    #[test]
    fn les_pentes_vont_vers_les_voisins_les_plus_proches_a_loreille() {
        let (gagnante, gn) = bande();
        let g = Grille {
            gagnante: &gagnante,
            gn,
            bornes: [0.0, 0.0, 1500.0, 1500.0],
        };
        let h = habitants(&g);
        // Le quartier de gauche sonne comme le centre (0,1), celui de droite
        // moins (0,5).
        let x = |id: i64| (id as usize) % gn;
        let distance = |a: i64, b: i64| {
            let (xa, xb) = (x(a), x(b));
            let gauche = |x: usize| x < 10;
            let droite = |x: usize| x >= 20;
            Some(if gauche(xa) || gauche(xb) {
                0.1
            } else if droite(xa) || droite(xb) {
                0.5
            } else {
                0.0
            })
        };
        let p = entre_quartiers(&g, &h, distance);
        // Trois quartiers, deux frontières : gauche↔centre, centre↔droite.
        assert_eq!(p.len(), 4);
        let du_centre: Vec<&Pente> = p.iter().filter(|x| x.famille_de == 2).collect();
        assert_eq!(du_centre.len(), 2);
        // La plus forte d'abord : vers la gauche (0,9), puis la droite (0,5).
        assert!((du_centre[0].proximite - 0.9).abs() < 1e-6);
        assert!(du_centre[0].vers[0] < du_centre[0].de[0]);
        assert!((du_centre[1].proximite - 0.5).abs() < 1e-6);
        // La flèche passe la couture (x = 500 m), à longueur bornée.
        let (de, vers) = (du_centre[0].de, du_centre[0].vers);
        assert!(vers[0] < 500.0 && de[0] > 500.0, "{de:?} → {vers:?}");
        let l = (vers[0] - de[0]).hypot(vers[1] - de[1]);
        assert!(
            (LONGUEUR_MIN_M - 1e-6..=LONGUEUR_MAX_M + 1e-6).contains(&l),
            "{l}"
        );
    }

    #[test]
    fn un_confetti_na_pas_de_pente() {
        let (mut gagnante, gn) = bande();
        // Une case isolée d'une famille 3 au milieu du centre.
        gagnante[5 * gn + 15] = 3;
        let g = Grille {
            gagnante: &gagnante,
            gn,
            bornes: [0.0, 0.0, 1500.0, 1500.0],
        };
        let p = entre_quartiers(&g, &habitants(&g), |_, _| Some(0.2));
        assert!(p.iter().all(|x| x.famille_de != 3 && x.famille_vers != 3));
        assert_eq!(p.len(), 4);
    }

    /// Quatre quartiers en croix autour d'un vide : les deux diagonales se
    /// croiseraient ; la plus faible cède la place.
    #[test]
    fn une_fleche_qui_croiserait_cede_la_place() {
        // 40 × 40 cases de 10 m ; quatre quartiers en damier (2 × 2 blocs de
        // 20 cases), familles 1 2 / 3 4 : chacun touche ses deux voisins de
        // côté, et les diagonales se touchent au centre.
        let gn = 40;
        let mut gagnante = vec![i64::MIN; gn * gn];
        for y in 0..gn {
            for x in 0..gn {
                gagnante[y * gn + x] = match (x < 20, y < 20) {
                    (true, true) => 1,
                    (false, true) => 2,
                    (true, false) => 3,
                    (false, false) => 4,
                };
            }
        }
        let g = Grille {
            gagnante: &gagnante,
            gn,
            bornes: [0.0, 0.0, 400.0, 400.0],
        };
        let h = habitants(&g);
        let p = entre_quartiers(&g, &h, |_, _| Some(0.2));
        let courbes: Vec<Vec<[f64; 2]>> = p.iter().map(|x| fleche(x.de, x.vers).0).collect();
        assert_eq!(croisements(&courbes), 0);
        assert!(p.len() >= 4, "{}", p.len());
        for f in 1..=4 {
            assert!(p.iter().filter(|x| x.famille_de == f).count() <= PAR_QUARTIER);
        }
    }

    #[test]
    fn deux_pentes_opposees_ne_se_croisent_pas() {
        let (a, b) = ([0.0, 0.0], [1000.0, 0.0]);
        let (aller, tete) = fleche(a, b);
        let (retour, _) = fleche(b, a);
        // L'aller s'incurve à sa droite (vers le bas), le retour aussi (vers
        // le haut).
        assert!(aller[8][1] < -50.0 && retour[8][1] > 50.0);
        assert_eq!(croisements(&[aller.clone(), retour]), 0);
        // La pointe est un triangle fermé au bout de la courbe.
        assert_eq!(tete.len(), 4);
        assert_eq!(tete[0], tete[3]);
        // Une croix se compte.
        let x1 = vec![[0.0, 0.0], [10.0, 10.0]];
        let x2 = vec![[0.0, 10.0], [10.0, 0.0]];
        assert_eq!(croisements(&[x1, x2]), 1);
    }
}
