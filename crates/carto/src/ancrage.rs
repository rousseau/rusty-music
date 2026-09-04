//! Étage 0 : reloger les artistes les plus populaires sur les monuments
//! iconiques de Paris.
//!
//! `docs/carto-ville.md` : la popularité générale (`track_popularite`) ne sert
//! qu'à ça. On apparie **par rang pur** — monument le plus emblématique ↔
//! artiste le plus populaire — sans contrainte géographique ni musicale (les
//! lieux importants sont partout dans Paris, pas seulement au centre). Un
//! artiste ancré déménage **entièrement** : tous ses morceaux se logent autour
//! du monument, hors de leur quartier de famille. Ce sont des marqueurs fixes,
//! aucune déformation du reste de la carte.
//!
//! Cet étage tourne **avant** l'agrégation familles/artistes des étages 1-3 :
//! retirer une trentaine de gros artistes après coup fausserait les centroïdes
//! et les cibles de capacité.

use std::collections::{HashMap, HashSet};

use rusty_music_core::db::MapPoint;
use rusty_music_osm::Extrait;

use crate::affectation::Repere;
use crate::batiments::{Batiment, GrilleBatiments};

/// Monuments iconiques de Paris, du plus au moins emblématique. Chaque nom est
/// résolu contre `Extrait::points_remarquables` par correspondance de nom
/// normalisée ; une entrée non trouvée dans l'extrait est simplement ignorée.
///
/// Liste **curatée** à dessein : le tag OSM `wikidata` est quasi universel sur
/// les monuments parisiens et ne les hiérarchise pas ; au-delà d'une trentaine,
/// un appariement par rang n'a plus de sens perceptif (`docs/carto-ville.md`).
const MONUMENTS: &[&str] = &[
    "Tour Eiffel",
    "Notre-Dame",
    "Sacré-Cœur",
    "Arc de triomphe",
    "Louvre",
    "Panthéon",
    "Opéra",
    "Tour Montparnasse",
    "Invalides",
    "Orsay",
    "Pompidou",
    "Sainte-Chapelle",
    "Conciergerie",
    "Luxembourg",
    "Hôtel de Ville",
    "Grand Palais",
    "Petit Palais",
    "Madeleine",
    "Chaillot",
    "Institut de France",
    "Palais-Royal",
    "Moulin Rouge",
    "Tour Saint-Jacques",
    "Colonne de Juillet",
    "Colonne Vendôme",
    "Palais de Tokyo",
    "Catacombes",
    "Bibliothèque nationale",
    "Cité des sciences",
    "Fondation Louis Vuitton",
];

/// Fraction de la popularité médiane retranchée quand *toute* la discographie
/// d'un artiste est inconnue de ListenBrainz/Deezer — pour qu'un artiste
/// entièrement couvert passe devant un artiste au seul tube chanceux.
const PENALITE_COUVERTURE: f64 = 0.3;

/// L'ancre d'un artiste : le monument et son point, en mètres du repère local.
#[derive(Clone, Debug)]
pub struct Ancre {
    pub monument: String,
    pub point_m: [f64; 2],
}

/// Une adresse posée à l'étage 0 : un morceau d'un artiste ancré, dans un vrai
/// bâtiment autour de son monument.
#[derive(Clone, Debug)]
pub struct AdresseAncree {
    pub track_id: i64,
    pub batiment_id: i64,
    pub point_m: [f64; 2],
}

/// Le résultat de [`ancrer`].
#[derive(Default)]
pub struct Ancrages {
    /// Nom d'artiste (`album_artist`) → son ancre.
    pub par_artiste: HashMap<String, Ancre>,
    /// Adresses posées pour **tous** les morceaux des artistes ancrés.
    pub adresses: Vec<AdresseAncree>,
}

impl Ancrages {
    pub fn est_ancre(&self, artiste: &str) -> bool {
        self.par_artiste.contains_key(artiste)
    }
}

/// Nom de regroupement d'un morceau — `album_artist`, repli sur `artist`.
/// Identique au choix de `ville::artistes_depuis_vue`.
pub fn nom_artiste(p: &MapPoint) -> Option<&str> {
    p.album_artist
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(p.artist.as_deref())
        .filter(|s| !s.is_empty())
}

fn plier(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' | 'á' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'î' | 'ï' => 'i',
        'ó' | 'ô' | 'ö' => 'o',
        'ú' | 'û' | 'ü' | 'ù' => 'u',
        'ç' => 'c',
        'œ' => 'o',
        'æ' => 'a',
        _ => c,
    }
}

/// Minuscules, accents dépliés, ponctuation en espaces, espaces resserrés.
fn normalise(s: &str) -> String {
    let bas: String = s.chars().flat_map(char::to_lowercase).map(plier).collect();
    bas.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn est_compilation(nom_normalise: &str) -> bool {
    matches!(
        nom_normalise,
        "various artists" | "various" | "va" | "compilation" | "divers" | "artistes divers"
    )
}

fn distance2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// Médiane d'une tranche déjà lue — `f64`, sans tri en place de l'appelant.
fn mediane(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Popularité par artiste : médiane des `relative` connus moins une pénalité de
/// couverture. `None` pour un artiste sans aucun morceau couvert (jamais ancré)
/// ou pour une compilation.
fn popularites_par_artiste(vue: &[MapPoint]) -> HashMap<String, f64> {
    let mut connus: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut totaux: HashMap<&str, usize> = HashMap::new();
    for p in vue {
        let Some(nom) = nom_artiste(p) else { continue };
        if est_compilation(&normalise(nom)) {
            continue;
        }
        *totaux.entry(nom).or_default() += 1;
        if let Some(r) = p.popularite {
            connus.entry(nom).or_default().push(r);
        }
    }
    connus
        .into_iter()
        .map(|(nom, rs)| {
            let total = totaux[nom];
            let couverture = rs.len() as f64 / total as f64;
            let score = mediane(rs) - PENALITE_COUVERTURE * (1.0 - couverture);
            (nom.to_string(), score)
        })
        .collect()
}

/// Résout les [`MONUMENTS`] contre l'extrait : `(nom curaté, point en m)`, dans
/// l'ordre de la liste, dédupliqués (deux entrées à moins de 60 m — Notre-Dame
/// a plusieurs nœuds — ne gardent que la première).
fn monuments_resolus(extrait: &Extrait, repere: &Repere) -> Vec<(String, [f64; 2])> {
    let candidats: Vec<(String, [f64; 2])> = extrait
        .points_remarquables
        .iter()
        .map(|p| (normalise(&p.nom), repere.vers_m(p.point)))
        .collect();

    let mut sortie: Vec<(String, [f64; 2])> = Vec::new();
    for cle_brute in MONUMENTS {
        let cle = normalise(cle_brute);
        let trouve = candidats
            .iter()
            .filter(|(nom, _)| nom.contains(&cle) || cle.contains(nom.as_str()))
            .min_by_key(|(nom, _)| nom.len());
        let Some((_, point)) = trouve else { continue };
        if sortie.iter().any(|(_, p)| distance2(*p, *point) < 60.0 * 60.0) {
            continue;
        }
        sortie.push((cle_brute.to_string(), *point));
    }
    sortie
}

/// Les `n` bâtiments libres les plus proches de `point`, en élargissant le
/// rayon jusqu'à en trouver assez.
fn batiments_libres_autour<'a>(
    grille: &'a GrilleBatiments,
    point: [f64; 2],
    n: usize,
    pris: &HashSet<i64>,
) -> Vec<&'a Batiment> {
    let mut rayon = 80.0_f64;
    loop {
        let mut v: Vec<&Batiment> = grille
            .pres_de(point, rayon)
            .into_iter()
            .filter(|b| !pris.contains(&b.id))
            .collect();
        if v.len() >= n || rayon > 3000.0 {
            v.sort_by(|a, b| distance2(a.centre, point).total_cmp(&distance2(b.centre, point)));
            v.truncate(n);
            return v;
        }
        rayon *= 1.7;
    }
}

/// Apparie les artistes les plus populaires aux monuments iconiques et loge
/// **tous** leurs morceaux autour. Marque les bâtiments utilisés dans `pris`.
pub fn ancrer(
    vue: &[MapPoint],
    extrait: &Extrait,
    grille: &GrilleBatiments,
    repere: &Repere,
    pris: &mut HashSet<i64>,
) -> Ancrages {
    let monuments = monuments_resolus(extrait, repere);
    if monuments.is_empty() || grille.est_vide() {
        return Ancrages::default();
    }

    let scores = popularites_par_artiste(vue);
    let mut classes: Vec<(String, f64)> = scores.into_iter().collect();
    classes.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    // Morceaux par artiste, triés par (album, piste, id) — un album arrive en
    // bloc, comme `ville::artistes_depuis_vue`.
    let mut pistes: HashMap<&str, Vec<(String, i64, i64)>> = HashMap::new();
    for p in vue {
        if let Some(nom) = nom_artiste(p) {
            pistes.entry(nom).or_default().push((
                p.album.clone().unwrap_or_default(),
                p.track_no.unwrap_or(0),
                p.id,
            ));
        }
    }

    let mut ancrages = Ancrages::default();
    for ((artiste, _score), (monument, point)) in classes.iter().zip(monuments.iter()) {
        let Some(triples) = pistes.get(artiste.as_str()) else { continue };
        let mut triples = triples.clone();
        triples.sort();
        let ids: Vec<i64> = triples.into_iter().map(|(_, _, id)| id).collect();

        let batiments = batiments_libres_autour(grille, *point, ids.len(), pris);
        for (&track_id, b) in ids.iter().zip(batiments.iter()) {
            pris.insert(b.id);
            ancrages.adresses.push(AdresseAncree {
                track_id,
                batiment_id: b.id,
                point_m: b.centre,
            });
        }
        ancrages.par_artiste.insert(
            artiste.clone(),
            Ancre { monument: monument.clone(), point_m: *point },
        );
    }
    ancrages
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::{Contour, Extrait, PointRemarquable, Troncon};

    fn morceau(id: i64, artiste: &str, album: &str, piste: i64, pop: Option<f64>) -> MapPoint {
        MapPoint {
            id,
            path: format!("{id}.flac"),
            x: 0.0,
            y: 0.0,
            cluster: 0,
            title: Some(format!("t{id}")),
            artist: Some(artiste.into()),
            album_artist: Some(artiste.into()),
            album: Some(album.into()),
            track_no: Some(piste),
            year: None,
            duration_ms: None,
            bpm: None,
            energy: None,
            popularite: pop,
        }
    }

    #[test]
    fn normalise_deplie_accents_et_ponctuation() {
        assert_eq!(normalise("Basilique du Sacré-Cœur"), "basilique du sacre cour");
        assert_eq!(normalise("Panthéon"), "pantheon");
        assert!(normalise("Musée du Louvre").contains("louvre"));
    }

    #[test]
    fn popularite_penalise_la_faible_couverture() {
        // A : trois morceaux connus à 0,8. B : un seul connu à 0,9, deux
        // inconnus. A doit passer devant B malgré le rang brut plus bas.
        let vue = vec![
            morceau(1, "A", "x", 1, Some(0.8)),
            morceau(2, "A", "x", 2, Some(0.8)),
            morceau(3, "A", "x", 3, Some(0.8)),
            morceau(4, "B", "y", 1, Some(0.9)),
            morceau(5, "B", "y", 2, None),
            morceau(6, "B", "y", 3, None),
        ];
        let s = popularites_par_artiste(&vue);
        assert!(s["A"] > s["B"], "A {:.3} vs B {:.3}", s["A"], s["B"]);
    }

    #[test]
    fn une_compilation_nest_jamais_ancree() {
        let vue = vec![
            morceau(1, "Various Artists", "hits", 1, Some(0.99)),
            morceau(2, "Various Artists", "hits", 2, Some(0.99)),
        ];
        assert!(popularites_par_artiste(&vue).is_empty());
    }

    #[test]
    fn ancrer_pose_lartiste_le_plus_populaire_sur_le_monument() {
        // Deux monuments, deux artistes ; le plus populaire prend le premier de
        // la liste curatée présent dans l'extrait.
        let mut troncons = vec![Troncon {
            id: 1,
            nom: Some("Rue".into()),
            classe: rusty_music_osm::Classe::Residentielle,
            points: vec![[2.30, 48.85], [2.31, 48.85]],
        }];
        troncons.push(Troncon {
            id: 2,
            nom: Some("Autre".into()),
            classe: rusty_music_osm::Classe::Residentielle,
            points: vec![[2.35, 48.87], [2.36, 48.87]],
        });
        let mut extrait = Extrait { troncons, ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        // Un bâtiment près de chaque monument.
        let carre = |c: [f64; 2]| {
            let m = repere.vers_m(c);
            let r = 6.0;
            Contour {
                id: (m[0] as i64).abs() % 1000 + 10,
                points: [[m[0] - r, m[1] - r], [m[0] + r, m[1] - r], [m[0] + r, m[1] + r], [m[0] - r, m[1] + r], [m[0] - r, m[1] - r]]
                    .iter()
                    .map(|p| repere.depuis_m(*p))
                    .collect(),
            }
        };
        for _ in 0..6 {
            let mut c1 = carre([2.305, 48.851]);
            c1.id += extrait.batis.len() as i64;
            let mut c2 = carre([2.355, 48.871]);
            c2.id += 500 + extrait.batis.len() as i64;
            extrait.batis.push(c1);
            extrait.batis.push(c2);
        }
        extrait.points_remarquables = vec![
            PointRemarquable { id: 1, nom: "Tour Eiffel".into(), genre: "monument".into(), point: [2.305, 48.851] },
            PointRemarquable { id: 2, nom: "Panthéon".into(), genre: "monument".into(), point: [2.355, 48.871] },
        ];

        let vue = vec![
            morceau(1, "Populaire", "a", 1, Some(0.95)),
            morceau(2, "Populaire", "a", 2, Some(0.95)),
            morceau(3, "Discret", "b", 1, Some(0.2)),
        ];
        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let mut pris = HashSet::new();
        let a = ancrer(&vue, &extrait, &grille, &repere, &mut pris);

        assert_eq!(a.par_artiste.get("Populaire").map(|x| x.monument.as_str()), Some("Tour Eiffel"));
        assert_eq!(a.adresses.iter().filter(|ad| ad.track_id == 1 || ad.track_id == 2).count(), 2);
        assert_eq!(pris.len(), a.adresses.len());
    }
}
