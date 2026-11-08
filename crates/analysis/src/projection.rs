// SPDX-License-Identifier: GPL-3.0-or-later
//! Réduction des empreintes à deux dimensions, pour la carte.
//!
//! t-SNE Barnes-Hut. `architecture.md` signalait l'écosystème Rust comme moins
//! mûr que `scikit-learn` et gardait un repli par Python : ce module est
//! l'essai qui tranche.
//!
//! Les empreintes sont normalisées avant projection. CLAP place ses vecteurs
//! sur une sphère où la similarité se lit en cosinus ; une fois normalisés, la
//! distance euclidienne en est une fonction monotone, et t-SNE ne travaille
//! que sur l'ordre des distances.
//!
//! **Score z par dimension et amorçage spectral : essayés, mesurés, écartés.**
//! `bhtsne` 0.7 fournit `spectral_init` (`rust-audio-stack.md` le notait encore
//! comme absent de l'écosystème) ; l'exemple `genre_projection` a chiffré les
//! deux sur la bibliothèque réelle. Ni l'un ni l'autre ne bouge la
//! conservation de voisinage (~13 % des douze plus proches survivent à la
//! projection) ni la pureté de genre du voisinage projeté (~43 %). L'amorçage
//! spectral resserre les petites familles éparses mais **étale les grandes**
//! (Rock, Électronique, Hip-Hop, celles dont on se plaignait), pour un coût de
//! projection multiplié par 3,5. Le plafond est ailleurs : dans l'empreinte,
//! plusieurs familles de genre sont plus dispersées que le hasard (Jazz 1,08×
//! le hasard, Classical 1,31×) — aucune projection 2 d n'en fera un quartier
//! compact, seule une entrée qui porte le genre le pourrait.

/// Coordonnée d'un morceau sur la carte.
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Ramène un vecteur sur la sphère unité.
pub fn normaliser(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n <= f32::EPSILON {
        return v.to_vec();
    }
    v.iter().map(|x| x / n).collect()
}

/// Projette des empreintes en 2D.
///
/// La perplexité gouverne le voisinage que t-SNE cherche à préserver ; elle
/// doit rester bien en dessous du nombre de points, faute de quoi
/// l'algorithme n'a plus de structure locale à respecter.
pub fn projeter(empreintes: &[Vec<f32>], perplexite: f32, epoques: usize) -> Vec<Point> {
    match empreintes.len() {
        0 => return Vec::new(),
        // Sous une poignée de points, t-SNE n'a rien à dire : on les aligne
        // plutôt que de le laisser produire des positions arbitraires.
        n if n < 10 => {
            return (0..n)
                .map(|i| Point {
                    x: i as f32,
                    y: 0.0,
                })
                .collect()
        }
        _ => {}
    }

    let unites: Vec<Vec<f32>> = empreintes.iter().map(|v| normaliser(v)).collect();
    let perplexite = perplexite.min((unites.len() as f32 - 1.0) / 3.0).max(2.0);

    // Le `2` est la dimension de sortie : la carte.
    let mut tsne: bhtsne::tSNE<f32, Vec<f32>, 2> = bhtsne::tSNE::new(&unites);
    tsne.perplexity(perplexite)
        .epochs(epoques)
        .barnes_hut(0.5, |a, b| {
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| (x - y) * (x - y))
                .sum::<f32>()
                .sqrt()
        });

    tsne.embedding()
        .chunks(2)
        .map(|c| Point { x: c[0], y: c[1] })
        .collect()
}

/// Époques d'une reprise : assez pour loger les nouveaux et resserrer leurs
/// voisins, trop peu pour que les anciens dérivent. Mesuré
/// (`examples/stabilite_projection.rs`, 27 805 morceaux dont 5 % ajoutés,
/// trois exécutions) : déplacement médian des anciens 0,6 à 0,9 % de la
/// largeur, contre 15 à 33 % depuis zéro ; conservation des 12 voisins 14 à
/// 16,5 %, contre 13 %. En 1 000 époques, la médiane monte à 1,5-2,4 %.
pub const EPOQUES_REPRISE: usize = 250;

/// Écart-type auquel une reprise ramène les coordonnées cadrées, l'échelle
/// brute de t-SNE étant perdue au cadrage. Celle de la bibliothèque mesurée
/// vaut 7,5 ; reprendre au quart déplace 2,4 à 3,2 % et fait tomber la
/// conservation à 9-10 %, au quadruple le critère tient encore. 10 laisse
/// de la marge aux bibliothèques plus petites, dont l'échelle brute est
/// moindre.
const ECART_REPRISE: f32 = 10.0;

/// Reprend une projection depuis `depart` (une coordonnée par empreinte, à
/// n'importe quelle échelle, celle de [`cadrer`] par exemple) au lieu de
/// repartir du hasard : le nuage garde sa forme, et la mémoire spatiale de
/// qui l'a déjà parcouru (Règle 4 de `interface-guidelines.md`).
///
/// Sans exagération précoce ni phase d'élan : t-SNE continue un dessin déjà
/// convergé. `bhtsne` n'optimise pas un sous-ensemble seul ; tout le nuage
/// repart, initialisé, pendant [`EPOQUES_REPRISE`] époques.
pub fn reprendre(empreintes: &[Vec<f32>], perplexite: f32, depart: &[Point]) -> Vec<Point> {
    assert_eq!(empreintes.len(), depart.len());
    if empreintes.len() < 10 {
        return projeter(empreintes, perplexite, EPOQUES_REPRISE);
    }
    let unites: Vec<Vec<f32>> = empreintes.iter().map(|v| normaliser(v)).collect();
    let perplexite = perplexite.min((unites.len() as f32 - 1.0) / 3.0).max(2.0);

    let m = depart.len() as f32;
    let (cx, cy) = (
        depart.iter().map(|p| p.x).sum::<f32>() / m,
        depart.iter().map(|p| p.y).sum::<f32>() / m,
    );
    let ecart = (depart
        .iter()
        .map(|p| (p.x - cx).powi(2) + (p.y - cy).powi(2))
        .sum::<f32>()
        / m)
        .sqrt()
        .max(f32::EPSILON);
    let k = ECART_REPRISE / ecart;
    let initial: Vec<f32> = depart
        .iter()
        .flat_map(|p| [(p.x - cx) * k, (p.y - cy) * k])
        .collect();

    let mut tsne: bhtsne::tSNE<f32, Vec<f32>, 2> = bhtsne::tSNE::new(&unites);
    tsne.perplexity(perplexite)
        .epochs(EPOQUES_REPRISE)
        .initial_embedding(initial)
        .stop_lying_epoch(0)
        .momentum_switch_epoch(0)
        .barnes_hut(0.5, |a, b| {
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| (x - y) * (x - y))
                .sum::<f32>()
                .sqrt()
        });

    tsne.embedding()
        .chunks(2)
        .map(|c| Point { x: c[0], y: c[1] })
        .collect()
}

/// Les points sans coordonnée (`None`) posés au barycentre de leurs
/// `k` plus proches voisins à l'oreille (cosinus des empreintes) parmi ceux
/// qui en ont une : le départ d'une [`reprendre`]. Il faut au moins un point
/// placé.
pub fn placer_nouveaux(empreintes: &[Vec<f32>], places: &[Option<Point>], k: usize) -> Vec<Point> {
    let unites: Vec<Vec<f32>> = empreintes.iter().map(|v| normaliser(v)).collect();
    let connus: Vec<usize> = (0..places.len()).filter(|&i| places[i].is_some()).collect();
    assert!(!connus.is_empty(), "aucun point placé");
    let k = k.clamp(1, connus.len());
    let nouveaux: Vec<usize> = (0..places.len()).filter(|&i| places[i].is_none()).collect();

    let barycentre = |i: usize| -> Point {
        let mut proches: Vec<(f32, usize)> = connus
            .iter()
            .map(|&j| {
                let d: f32 = unites[i].iter().zip(&unites[j]).map(|(a, b)| a * b).sum();
                (-d, j)
            })
            .collect();
        if k < proches.len() {
            proches.select_nth_unstable_by(k, |a, b| a.0.partial_cmp(&b.0).unwrap());
        }
        let (mut x, mut y) = (0.0, 0.0);
        for &(_, j) in &proches[..k] {
            let p = places[j].unwrap();
            x += p.x;
            y += p.y;
        }
        Point {
            x: x / k as f32,
            y: y / k as f32,
        }
    };

    // Balayage complet, nouveaux × placés : réparti sur les cœurs.
    let fils = std::thread::available_parallelism().map_or(1, |n| n.get());
    let part = nouveaux.len().div_ceil(fils).max(1);
    let poses: Vec<(usize, Point)> = std::thread::scope(|s| {
        let taches: Vec<_> = nouveaux
            .chunks(part)
            .map(|lot| s.spawn(|| lot.iter().map(|&i| (i, barycentre(i))).collect::<Vec<_>>()))
            .collect();
        taches.into_iter().flat_map(|t| t.join().unwrap()).collect()
    });

    let mut sortie: Vec<Point> = places
        .iter()
        .map(|p| p.unwrap_or(Point { x: 0.0, y: 0.0 }))
        .collect();
    for (i, p) in poses {
        sortie[i] = p;
    }
    sortie
}

/// Ramène les coordonnées dans `[-1, 1]`, en préservant les proportions.
///
/// t-SNE rend des échelles arbitraires d'une exécution à l'autre : sans cela
/// l'interface devrait recalculer ses bornes à chaque projection.
pub fn cadrer(points: &mut [Point]) {
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in points.iter() {
        x0 = x0.min(p.x);
        x1 = x1.max(p.x);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
    }
    // Une seule échelle pour les deux axes : étirer déformerait les distances
    // que t-SNE vient justement d'ajuster.
    let etendue = ((x1 - x0).max(y1 - y0) / 2.0).max(f32::EPSILON);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    for p in points.iter_mut() {
        p.x = (p.x - cx) / etendue;
        p.y = (p.y - cy) / etendue;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trois amas nettement séparés dans l'espace d'origine doivent le rester
    /// après projection : c'est tout ce qu'on demande à une carte.
    #[test]
    fn les_amas_survivent_a_la_projection() {
        let mut empreintes = Vec::new();
        let mut appartenance = Vec::new();
        for amas in 0..3 {
            for i in 0..20 {
                let mut v = vec![0.0f32; 32];
                // Chaque amas occupe un octant différent, avec du bruit.
                v[amas * 10] = 1.0;
                v[amas * 10 + 1] = 0.5 + (i as f32) * 0.01;
                empreintes.push(v);
                appartenance.push(amas);
            }
        }

        let mut pts = projeter(&empreintes, 10.0, 500);
        cadrer(&mut pts);
        assert_eq!(pts.len(), 60);
        assert!(pts.iter().all(|p| p.x.is_finite() && p.y.is_finite()));

        // Distance moyenne au sein d'un amas contre distance entre amas.
        let d = |a: &Point, b: &Point| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
        let (mut intra, mut ni) = (0.0, 0);
        let (mut inter, mut ne) = (0.0, 0);
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                let dist = d(&pts[i], &pts[j]);
                if appartenance[i] == appartenance[j] {
                    intra += dist;
                    ni += 1;
                } else {
                    inter += dist;
                    ne += 1;
                }
            }
        }
        let (intra, inter) = (intra / ni as f32, inter / ne as f32);
        assert!(
            inter > intra * 1.5,
            "les amas se sont mélangés : intra {intra:.2}, inter {inter:.2}"
        );
    }

    /// Un nouveau venu se pose parmi ses semblables, et une reprise laisse
    /// les anciens à peu près où ils étaient.
    #[test]
    fn une_reprise_garde_le_nuage_en_place() {
        // Trois amas de 100, chacun bruité sur quelques dimensions : assez de
        // structure locale pour que le dessin ait quelque chose à garder.
        let mut graine = 7u64;
        let mut bruit = || {
            graine = graine
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((graine >> 33) as f32 / (1u64 << 31) as f32) - 0.5
        };
        let mut empreintes = Vec::new();
        for amas in 0..3 {
            for _ in 0..100 {
                let mut v = vec![0.0f32; 32];
                v[amas * 10] = 1.0;
                for d in 1..6 {
                    v[amas * 10 + d] = 0.3 * bruit();
                }
                empreintes.push(v);
            }
        }
        let n = empreintes.len();
        let mut avant = projeter(&empreintes, 15.0, 500);
        cadrer(&mut avant);

        // Le dernier de l'amas 2 arrive sans coordonnée.
        let mut places: Vec<Option<Point>> = avant.iter().copied().map(Some).collect();
        places[n - 1] = None;
        let depart = placer_nouveaux(&empreintes, &places, 5);
        let d = |a: &Point, b: &Point| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
        let centre = |r: std::ops::Range<usize>| {
            let k = r.len() as f32;
            let (x, y) = r.fold((0.0, 0.0), |(x, y), i| (x + avant[i].x, y + avant[i].y));
            Point { x: x / k, y: y / k }
        };
        assert!(
            d(&depart[n - 1], &centre(200..n - 1)) < d(&depart[n - 1], &centre(0..100)),
            "le nouveau venu n'est pas allé vers son amas"
        );

        let mut apres = reprendre(&empreintes, 15.0, &depart);
        cadrer(&mut apres);
        let mut deplacements: Vec<f32> = (0..n - 1).map(|i| d(&apres[i], &avant[i])).collect();
        deplacements.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mediane = deplacements[deplacements.len() / 2];
        // Largeur 2 : 7,5 % de la largeur. Mesuré sur 25 exécutions (Barnes-Hut
        // en parallèle n'est pas déterministe) : de 0,031 à 0,075.
        assert!(mediane < 0.15, "médiane des déplacements {mediane:.3}");
    }

    #[test]
    fn le_cadrage_tient_dans_la_boite() {
        let mut pts = vec![
            Point { x: 100.0, y: -40.0 },
            Point { x: -20.0, y: 60.0 },
            Point { x: 3.0, y: 3.0 },
        ];
        cadrer(&mut pts);
        assert!(pts.iter().all(|p| p.x.abs() <= 1.001 && p.y.abs() <= 1.001));
    }
}
