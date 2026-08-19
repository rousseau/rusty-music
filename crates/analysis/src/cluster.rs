//! Regroupement des morceaux en familles, pour la légende de la carte.
//!
//! K-means avec initialisation k-means++, écrit ici plutôt que pris à
//! `linfa` : c'est une quarantaine de lignes, et `linfa-clustering` entraîne
//! `ndarray` et, selon les options, une BLAS système. `architecture.md` garde
//! `linfa` comme voie documentée le jour où DBSCAN ou un mélange gaussien
//! deviendront nécessaires — eux ne se réécrivent pas à la main.
//!
//! Le regroupement se fait sur les empreintes complètes, pas sur les
//! coordonnées 2D : t-SNE déforme les distances, et regrouper sur son résultat
//! reviendrait à décrire la carte au lieu de la musique.

use crate::alea::Alea;

fn distance2(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Répartit `points` en `k` familles. Renvoie l'indice de famille de chacun.
pub fn kmeans(points: &[Vec<f32>], k: usize, iterations: usize) -> Vec<usize> {
    if points.is_empty() {
        return Vec::new();
    }
    let k = k.clamp(1, points.len());
    let dim = points[0].len();
    let mut alea = Alea::depuis(0x9E37_79B9_7F4A_7C15);

    // k-means++ : le premier centre au hasard, les suivants d'autant plus
    // probables qu'ils sont loin de ceux déjà choisis. Une initialisation
    // uniforme laisse souvent deux centres dans le même amas.
    let mut centres: Vec<Vec<f32>> = vec![points[0].clone()];
    while centres.len() < k {
        let carres: Vec<f32> = points
            .iter()
            .map(|p| {
                centres
                    .iter()
                    .map(|c| distance2(p, c))
                    .fold(f32::MAX, f32::min)
            })
            .collect();
        let total: f32 = carres.iter().sum();
        if total <= f32::EPSILON {
            break; // tous les points sont confondus
        }
        let mut seuil = alea.reel() * total;
        let mut choisi = carres.len() - 1;
        for (i, c) in carres.iter().enumerate() {
            seuil -= c;
            if seuil <= 0.0 {
                choisi = i;
                break;
            }
        }
        centres.push(points[choisi].clone());
    }

    let mut appartenance = vec![0usize; points.len()];
    for _ in 0..iterations {
        let mut bouge = false;
        for (i, p) in points.iter().enumerate() {
            let meilleur = centres
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    distance2(p, a.1)
                        .partial_cmp(&distance2(p, b.1))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map_or(0, |(j, _)| j);
            if appartenance[i] != meilleur {
                appartenance[i] = meilleur;
                bouge = true;
            }
        }

        let mut sommes = vec![vec![0.0f32; dim]; centres.len()];
        let mut effectifs = vec![0usize; centres.len()];
        for (p, &a) in points.iter().zip(&appartenance) {
            for (s, v) in sommes[a].iter_mut().zip(p) {
                *s += v;
            }
            effectifs[a] += 1;
        }
        for (c, (somme, n)) in centres.iter_mut().zip(sommes.iter().zip(&effectifs)) {
            if *n > 0 {
                for (v, s) in c.iter_mut().zip(somme) {
                    *v = s / *n as f32;
                }
            }
        }

        // Convergence : plus aucun point ne change de famille.
        if !bouge {
            break;
        }
    }
    appartenance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separe_trois_amas_nets() {
        let mut points = Vec::new();
        let mut verite = Vec::new();
        for amas in 0..3 {
            for i in 0..15 {
                let mut v = vec![0.0f32; 8];
                v[amas] = 10.0 + i as f32 * 0.05;
                points.push(v);
                verite.push(amas);
            }
        }

        let familles = kmeans(&points, 3, 50);
        assert_eq!(familles.len(), 45);

        // Les étiquettes sont arbitraires : on vérifie que deux points du même
        // amas reçoivent la même, et deux points d'amas différents non.
        for i in 0..points.len() {
            for j in i + 1..points.len() {
                if verite[i] == verite[j] {
                    assert_eq!(familles[i], familles[j], "amas {} éclaté", verite[i]);
                } else {
                    assert_ne!(familles[i], familles[j], "amas confondus");
                }
            }
        }
    }

    #[test]
    fn est_deterministe() {
        let points: Vec<Vec<f32>> = (0..40)
            .map(|i| vec![(i % 7) as f32, (i % 5) as f32, (i % 3) as f32])
            .collect();
        assert_eq!(kmeans(&points, 4, 30), kmeans(&points, 4, 30));
    }

    #[test]
    fn supporte_les_cas_degeneres() {
        assert!(kmeans(&[], 3, 10).is_empty());
        // Plus de familles demandées que de points, et points identiques.
        assert_eq!(kmeans(&vec![vec![1.0, 1.0]; 3], 10, 10).len(), 3);
    }
}
