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
