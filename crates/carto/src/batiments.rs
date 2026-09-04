//! Une grille spatiale de bâtiments, pour que l'étage 3 loge chaque morceau
//! dans un vrai bâtiment plutôt qu'en bordure de rue.
//!
//! Aucune dépendance à Tauri ou SQLite, comme le reste du crate : il reçoit
//! un [`Extrait`] déjà lu et un [`Repere`] déjà centré.

use std::collections::HashMap;

use rusty_music_osm::Extrait;

use crate::affectation::Repere;

/// Aire minimale, mètres², pour qu'un bâtiment compte comme logement.
///
/// Cabanons et kiosques exclus — sans plancher, un abri de jardin de 3 m²
/// logerait un morceau aussi bien qu'un immeuble. **Calibré à l'œil, pas
/// mesuré**, même honnêteté que le reste des constantes de ce module.
/// N'exclut le bâtiment que du *logement* : il reste rendu tel quel dans
/// `Source.batiments`, qui copie `extrait.batis` sans filtrage.
pub const AIRE_MIN_M2: f64 = 15.0;

/// Taille de cellule de la grille, mètres. Calibré à l'œil, pas mesuré : assez
/// grand pour que peu de bâtiments parisiens (denses) tombent seuls dans leur
/// cellule, assez petit pour qu'une recherche à `RAYON_RECHERCHE` (40 m,
/// `affectation::loger_dans_batiments`) n'ait qu'une poignée de cellules à
/// visiter.
const PAS_GRILLE: f64 = 60.0;

/// Un bâtiment prêt pour le logement : son polygone et son centre en mètres
/// locaux (repère de `affectation::Repere`), son aire.
#[derive(Clone, Debug)]
pub struct Batiment {
    pub id: i64,
    pub polygone: Vec<[f64; 2]>,
    pub centre: [f64; 2],
    pub aire: f64,
}

/// Aire d'un anneau fermé, mètres² — formule du lacet.
///
/// `.abs()` : le sens de parcours d'un anneau OSM n'est pas garanti, et une
/// aire n'a pas de signe à conserver ici (contrairement à `tuiles::orienter`,
/// qui en a besoin pour distinguer un contour d'un trou).
pub fn aire_m2(anneau: &[[f64; 2]]) -> f64 {
    if anneau.len() < 3 {
        return 0.0;
    }
    let mut somme = 0.0;
    for k in 0..anneau.len() {
        let a = anneau[k];
        let b = anneau[(k + 1) % anneau.len()];
        somme += a[0] * b[1] - b[0] * a[1];
    }
    (somme / 2.0).abs()
}

/// Centroïde arithmétique des sommets — une approximation du centre de masse
/// suffisante pour un bâtiment, dont la forme est rarement pathologique.
fn centroide(anneau: &[[f64; 2]]) -> [f64; 2] {
    let n = anneau.len().max(1) as f64;
    let (sx, sy) = anneau.iter().fold((0.0, 0.0), |(sx, sy), p| (sx + p[0], sy + p[1]));
    [sx / n, sy / n]
}

fn cellule(p: [f64; 2]) -> (i32, i32) {
    ((p[0] / PAS_GRILLE).floor() as i32, (p[1] / PAS_GRILLE).floor() as i32)
}

/// Grille spatiale des bâtiments d'un extrait, en mètres locaux.
///
/// Même idiome en deux temps que `rusty_music_osm::Frontiere` (bandes de
/// latitude puis test exact) : un test grossier par cellule, puis un test de
/// distance exact — pas d'arbre équilibré pour quelques dizaines de milliers
/// de bâtiments, cherché depuis un clic ou un échantillon de tracé, jamais en
/// boucle serrée.
pub struct GrilleBatiments {
    batiments: Vec<Batiment>,
    cellules: HashMap<(i32, i32), Vec<u32>>,
}

impl GrilleBatiments {
    /// Construit la grille depuis `extrait.batis`, filtré par
    /// [`AIRE_MIN_M2`].
    pub fn nouvelle(extrait: &Extrait, repere: &Repere) -> GrilleBatiments {
        let mut batiments = Vec::new();
        for c in &extrait.batis {
            let polygone: Vec<[f64; 2]> = c.points.iter().map(|p| repere.vers_m(*p)).collect();
            let aire = aire_m2(&polygone);
            if aire < AIRE_MIN_M2 {
                continue;
            }
            let centre = centroide(&polygone);
            batiments.push(Batiment { id: c.id, polygone, centre, aire });
        }

        let mut cellules: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, b) in batiments.iter().enumerate() {
            cellules.entry(cellule(b.centre)).or_default().push(i as u32);
        }
        GrilleBatiments { batiments, cellules }
    }

    pub fn est_vide(&self) -> bool {
        self.batiments.is_empty()
    }

    /// Tous les bâtiments de la grille, sans filtre spatial — le filet de
    /// sécurité d'`affectation::loger_dans_batiments` en a besoin quand la
    /// recherche le long d'une rue ne suffit plus.
    pub fn tous(&self) -> &[Batiment] {
        &self.batiments
    }

    /// Les `n` bâtiments logeables les plus proches de `point` (repère local,
    /// mètres), du plus proche au plus loin.
    ///
    /// C'est ce qui définit la **zone peuplée** : `docs/carto-ville.md` place
    /// un morceau par bâtiment en partant du centre, donc la zone est
    /// exactement l'ensemble des `#morceaux` bâtiments les plus proches de
    /// l'île de la Cité. Tri complet plutôt qu'un tas partiel : appelé une
    /// fois, sur quelques dizaines de milliers de bâtiments.
    pub fn n_plus_proches(&self, point: [f64; 2], n: usize) -> Vec<&Batiment> {
        let mut tries: Vec<&Batiment> = self.batiments.iter().collect();
        tries.sort_by(|a, b| {
            let da = (a.centre[0] - point[0]).powi(2) + (a.centre[1] - point[1]).powi(2);
            let db = (b.centre[0] - point[0]).powi(2) + (b.centre[1] - point[1]).powi(2);
            da.total_cmp(&db).then(a.id.cmp(&b.id))
        });
        tries.truncate(n);
        tries
    }

    /// Centre de masse des bâtiments logeables (repère local, mètres) — le
    /// repli quand aucun centre de ville n'est fourni à
    /// [`crate::ville::rassembler`]. Pour une ville globalement radiale, il
    /// tombe près du cœur historique.
    pub fn centre_de_masse(&self) -> [f64; 2] {
        if self.batiments.is_empty() {
            return [0.0, 0.0];
        }
        let n = self.batiments.len() as f64;
        let (sx, sy) = self
            .batiments
            .iter()
            .fold((0.0, 0.0), |(sx, sy), b| (sx + b.centre[0], sy + b.centre[1]));
        [sx / n, sy / n]
    }

    /// Les bâtiments dont le centre tombe à moins de `rayon` mètres de
    /// `point`.
    pub fn pres_de(&self, point: [f64; 2], rayon: f64) -> Vec<&Batiment> {
        let (cx, cy) = cellule(point);
        // +1 : un bâtiment peut être plus proche de `point` que ne le suggère
        // sa propre cellule, si `point` est près du bord de la sienne.
        let etendue = (rayon / PAS_GRILLE).ceil() as i32 + 1;
        let rayon2 = rayon * rayon;
        let mut trouves = Vec::new();
        for dx in -etendue..=etendue {
            for dy in -etendue..=etendue {
                let Some(indices) = self.cellules.get(&(cx + dx, cy + dy)) else { continue };
                for &i in indices {
                    let b = &self.batiments[i as usize];
                    let ddx = b.centre[0] - point[0];
                    let ddy = b.centre[1] - point[1];
                    if ddx * ddx + ddy * ddy <= rayon2 {
                        trouves.push(b);
                    }
                }
            }
        }
        trouves
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::{Classe, Contour, Extrait, Troncon};

    fn carre(cx: f64, cy: f64, cote: f64) -> Vec<[f64; 2]> {
        let r = cote / 2.0;
        vec![
            [cx - r, cy - r],
            [cx + r, cy - r],
            [cx + r, cy + r],
            [cx - r, cy + r],
            [cx - r, cy - r],
        ]
    }

    #[test]
    fn aire_m2_dun_carre_connu() {
        // Un carré de 10 m de côté fait 100 m², quel que soit son sens de
        // parcours.
        let mut anneau = carre(0.0, 0.0, 10.0);
        assert!((aire_m2(&anneau) - 100.0).abs() < 1e-9);
        anneau.reverse();
        assert!((aire_m2(&anneau) - 100.0).abs() < 1e-9, "le sens de parcours ne doit pas changer l'aire");
    }

    /// Un extrait minimal, en degrés, centré à peu près sur `(2.30, 48.85)` —
    /// juste assez pour que `Repere::centre_de` ait quelque chose à moyenner.
    fn extrait_dessai() -> Extrait {
        Extrait {
            troncons: vec![Troncon {
                id: 1,
                nom: Some("Rue".into()),
                classe: Classe::Residentielle,
                points: vec![[2.30, 48.85], [2.301, 48.85]],
            }],
            ..Default::default()
        }
    }

    /// Convertit un carré en mètres locaux vers des degrés, pour construire
    /// un `Contour` OSM de test — l'inverse de ce que `GrilleBatiments`
    /// applique en interne.
    fn contour_carre(repere: &Repere, id: i64, centre_m: [f64; 2], cote: f64) -> Contour {
        let anneau_m = carre(centre_m[0], centre_m[1], cote);
        Contour { id, points: anneau_m.iter().map(|p| repere.depuis_m(*p)).collect() }
    }

    #[test]
    fn un_batiment_trop_petit_est_exclu_de_la_grille() {
        let mut extrait = extrait_dessai();
        let repere = Repere::centre_de(&extrait);
        // 3 m de côté : 9 m², sous AIRE_MIN_M2 (15).
        extrait.batis.push(contour_carre(&repere, 1, [0.0, 0.0], 3.0));
        // 10 m de côté : 100 m², largement au-dessus.
        extrait.batis.push(contour_carre(&repere, 2, [50.0, 0.0], 10.0));

        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let tous = grille.pres_de([0.0, 0.0], 1000.0);
        assert_eq!(tous.len(), 1, "seul le grand bâtiment doit entrer dans la grille");
        assert_eq!(tous[0].id, 2);
    }

    #[test]
    fn pres_de_trouve_selon_la_distance() {
        let mut extrait = extrait_dessai();
        let repere = Repere::centre_de(&extrait);
        extrait.batis.push(contour_carre(&repere, 1, [10.0, 0.0], 10.0));
        extrait.batis.push(contour_carre(&repere, 2, [200.0, 0.0], 10.0));
        let grille = GrilleBatiments::nouvelle(&extrait, &repere);

        let proches = grille.pres_de([0.0, 0.0], 50.0);
        assert_eq!(proches.len(), 1, "le bâtiment à 200 m ne doit pas entrer dans un rayon de 50 m");
        assert_eq!(proches[0].id, 1);

        let tous = grille.pres_de([0.0, 0.0], 500.0);
        assert_eq!(tous.len(), 2, "les deux doivent entrer dans un rayon de 500 m");
    }

    #[test]
    fn une_grille_vide_ne_fait_pas_tomber_lappelant() {
        let extrait = extrait_dessai();
        let repere = Repere::centre_de(&extrait);
        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        assert!(grille.est_vide());
        assert!(grille.pres_de([0.0, 0.0], 100.0).is_empty());
    }
}
