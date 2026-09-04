//! Regroupement des morceaux en familles, pour la légende de la carte.
//!
//! Deux méthodes. [`kmeans`] est purement acoustique : elle ne sait rien des
//! genres et regroupe par similarité d'empreinte, ce qui produit parfois des
//! familles que rien de commun ne nomme — mesuré : « Reggae · Rock » sur la
//! bibliothèque réelle, deux genres que rien ne rapproche sinon le hasard de
//! l'entraînement du modèle. [`familles_par_genre`] part au contraire d'un
//! vocabulaire de genres reconnaissables et n'utilise l'empreinte que pour
//! les morceaux que le vocabulaire ne sait pas nommer. C'est la méthode
//! retenue pour la carte ; `kmeans` reste comme point de comparaison et pour
//! qui voudrait un regroupement sans dépendre d'aucun genre déclaré.
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

use std::collections::HashMap;

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

/// Répartit `points` en familles de `vocabulaire`, plus le repli acoustique
/// pour ce qu'il ne couvre pas.
///
/// Le vocabulaire est fourni par l'appelant, pas compilé en dur : c'est un
/// réglage de bibliothèque ([`rusty_music_core::db::Library::vocabulaire_familles`]),
/// pas un algorithme. Ce module n'en connaît aucun par défaut.
///
/// Deux passes :
///
/// 1. **Ancrage.** Un morceau dont `genres[i]` figure dans le vocabulaire
///    rejoint directement la famille correspondante — aucun calcul de
///    distance, le genre déclaré est pris tel quel.
/// 2. **Repli acoustique.** Pour les autres (genre inconnu, absent du
///    vocabulaire, ou empreinte sans genre du tout), chaque famille ancrée a
///    un centroïde — la moyenne des empreintes qu'elle vient de recevoir. Le
///    morceau rejoint le centroïde le plus proche, comme un k-means à une
///    seule itération dont les centres de départ sont donnés par les genres
///    plutôt que tirés au hasard.
///
/// Une famille du vocabulaire sans aucun morceau ancré n'a pas de centroïde
/// et ne peut recevoir personne au repli : elle disparaît simplement du
/// résultat, plutôt que de forcer une comparaison contre un centre qui ne
/// représenterait rien.
///
/// `genres[i]` doit être en minuscules — c'est ainsi que MusicBrainz les
/// rend et que `vocabulaire` doit donc l'être aussi, aucune casse à
/// normaliser ici.
pub fn familles_par_genre(
    points: &[Vec<f32>],
    genres: &[Option<String>],
    vocabulaire: &[(String, Vec<String>)],
) -> Vec<Option<usize>> {
    assert_eq!(points.len(), genres.len(), "un genre par morceau, même absent");
    if points.is_empty() || vocabulaire.is_empty() {
        return vec![None; points.len()];
    }
    let dim = points[0].len();

    // Table de recherche genre -> famille : construite une fois, pas à
    // chaque morceau.
    let index_du_genre: HashMap<&str, usize> = vocabulaire
        .iter()
        .enumerate()
        .flat_map(|(i, (_, alias))| alias.iter().map(move |a| (a.as_str(), i)))
        .collect();

    let mut appartenance: Vec<Option<usize>> = vec![None; points.len()];
    let mut a_ancrer: Vec<usize> = Vec::new();
    for (i, g) in genres.iter().enumerate() {
        match g.as_deref().and_then(|g| index_du_genre.get(g)) {
            Some(&famille) => appartenance[i] = Some(famille),
            None => a_ancrer.push(i),
        }
    }

    let mut sommes = vec![vec![0.0f32; dim]; vocabulaire.len()];
    let mut effectifs = vec![0usize; vocabulaire.len()];
    for (i, a) in appartenance.iter().enumerate() {
        if let Some(f) = a {
            for (s, v) in sommes[*f].iter_mut().zip(&points[i]) {
                *s += v;
            }
            effectifs[*f] += 1;
        }
    }
    let centroides: Vec<Option<Vec<f32>>> = sommes
        .into_iter()
        .zip(&effectifs)
        .map(|(somme, &n)| {
            (n > 0).then(|| somme.into_iter().map(|s| s / n as f32).collect())
        })
        .collect();

    for i in a_ancrer {
        appartenance[i] = centroides
            .iter()
            .enumerate()
            .filter_map(|(f, c)| c.as_ref().map(|c| (f, distance2(&points[i], c))))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(f, _)| f);
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

    /// Un petit vocabulaire ad hoc, indépendant du réglage réel — les tests
    /// n'ont pas à connaître les douze familles par défaut, seulement le
    /// contrat de la fonction.
    fn vocabulaire_essai() -> Vec<(String, Vec<String>)> {
        vec![
            ("Reggae".to_string(), vec!["reggae".to_string(), "dub".to_string()]),
            ("Rock".to_string(), vec!["rock".to_string()]),
            ("Jazz".to_string(), vec!["jazz".to_string()]),
            ("Classique".to_string(), vec!["classical".to_string()]),
        ]
    }
    const REGGAE: usize = 0;
    const ROCK: usize = 1;
    const JAZZ: usize = 2;
    const CLASSIQUE: usize = 3;

    #[test]
    fn un_genre_du_vocabulaire_ancre_directement_sans_toucher_a_lempreinte() {
        // Deux morceaux du même genre, à des empreintes opposées : le genre
        // déclaré doit l'emporter sur toute notion de distance.
        let points = vec![vec![1.0, 0.0], vec![-1.0, 0.0]];
        let genres = vec![Some("reggae".to_string()), Some("reggae".to_string())];
        let familles = familles_par_genre(&points, &genres, &vocabulaire_essai());
        assert_eq!(familles[0], Some(REGGAE));
        assert_eq!(familles[1], Some(REGGAE));
    }

    #[test]
    fn un_genre_hors_vocabulaire_bascule_au_repli_acoustique() {
        // Une ancre "Rock" nette à (10, 0) ; un morceau de genre inconnu
        // tout près doit la rejoindre au repli.
        let points = vec![vec![10.0, 0.0], vec![10.1, 0.0]];
        let genres = vec![Some("rock".to_string()), Some("un genre que rien ne connaît".to_string())];
        let familles = familles_par_genre(&points, &genres, &vocabulaire_essai());
        assert_eq!(familles[0], Some(ROCK));
        assert_eq!(familles[1], Some(ROCK), "devrait rejoindre le centroïde le plus proche");
    }

    #[test]
    fn un_morceau_sans_genre_passe_aussi_par_le_repli() {
        let points = vec![vec![0.0, 5.0], vec![0.0, 5.05]];
        let genres = vec![Some("jazz".to_string()), None];
        let familles = familles_par_genre(&points, &genres, &vocabulaire_essai());
        assert_eq!(familles[1], Some(JAZZ));
    }

    #[test]
    fn une_famille_jamais_ancree_narrive_jamais_par_repli() {
        // Aucun morceau "Classique" : sa famille ne doit jamais apparaître,
        // faute de centroïde pour la représenter.
        let points = vec![vec![1.0, 1.0], vec![1.0, 1.0], vec![50.0, 50.0]];
        let genres = vec![
            Some("rock".to_string()),
            Some("rock".to_string()),
            None, // loin de tout, mais "Classique" ne peut pas le recevoir
        ];
        let familles = familles_par_genre(&points, &genres, &vocabulaire_essai());
        assert!(!familles.contains(&Some(CLASSIQUE)));
        assert_eq!(familles[2], Some(ROCK), "seule famille ancrée disponible");
    }

    #[test]
    fn supporte_une_bibliotheque_vide() {
        assert!(familles_par_genre(&[], &[], &vocabulaire_essai()).is_empty());
    }

    #[test]
    fn supporte_un_vocabulaire_vide() {
        let points = vec![vec![1.0, 1.0]];
        let genres = vec![Some("rock".to_string())];
        assert_eq!(familles_par_genre(&points, &genres, &[]), vec![None]);
    }
}
