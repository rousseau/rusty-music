// SPDX-License-Identifier: GPL-3.0-or-later
//! Une **carte de coût de déplacement** sur la voirie réelle : depuis un point
//! source, combien « coûte » chaque endroit de la ville si l'on se déplace le
//! long des rues, les grandes voies comptant moins au mètre que les impasses.
//!
//! C'est ce qui définit la **zone peuplée** : `ville::preparer` garde les `N`
//! bâtiments au plus faible coût de voirie depuis l'île de la Cité, pas les `N`
//! plus proches *à vol d'oiseau* (qui donnaient un disque). Les grandes voies
//! « rapprochent », un terme organique lissé ([`friction_locale`]) fait
//! serpenter la frontière. `cargo run --example cout_voirie` visualise le champ
//! et compare les deux zones (`docs/carto-ville.md`).
//!
//! Le champ se calcule en trois temps : Dijkstra depuis la source sur le graphe
//! pondéré ([`reseau_reel::Graphe::couts_depuis`]), rasterisation du coût des
//! sommets sur une grille (plus proche sommet), puis isobandes (`contour`,
//! comme `affectation::territoires`).

use std::collections::HashMap;

use rusty_music_osm::{Classe, Extrait};

use crate::affectation::Repere;
use crate::reseau_reel::Graphe;

/// Multiplicateur de coût au mètre, par classe de voie. Une avenue « rapproche »
/// deux points, une impasse les « éloigne ».
///
/// **C'est le bouton de réglage de la forme de la zone peuplée.** Ce qui
/// compte est le *rapport* entre les grandes voies et le résidentiel : à 0,3
/// contre 1,0, une avenue porte le peuplement ~3× plus loin que la trame
/// résidentielle, et la frontière prend une forme d'étoile marquée le long des
/// axes (Grands Boulevards, Rivoli, Sébastopol, avenues rayonnant de l'Étoile,
/// de Nation, de la Bastille). À régler à l'œil avec
/// `cargo run --example cout_voirie`.
pub fn friction(classe: Classe) -> f64 {
    match classe {
        Classe::Autoroute => 0.12,
        Classe::Primaire => 0.18,
        Classe::Secondaire => 0.38,
        Classe::Tertiaire => 0.65,
        Classe::Residentielle => 1.0,
        Classe::Pietonne => 2.0,
        Classe::Service => 3.5,
    }
}

/// Amplitude du terme organique : le coût d'une même classe de voie varie de
/// ±cette fraction selon l'endroit, pour casser la régularité géométrique de la
/// frontière — le peuplement ne suit alors plus une courbe de niveau propre
/// mais serpente (« un peu aléatoire », retour d'usage). Déterministe : c'est
/// un hachage de la position, deux exécutions donnent la même carte.
pub const AMPLITUDE_ORGANIQUE: f64 = 0.18;

/// Bruit de valeur 2D lissé, période ~500 m, dans `[-1, 1]`.
fn bruit(p: [f64; 2]) -> f64 {
    let (fx, fy) = (p[0] / 0.006, p[1] / 0.0045); // ~500 m en lon / lat à Paris
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let h = |i: f64, j: f64| -> f64 {
        let n = (i as i64)
            .wrapping_mul(374_761_393)
            .wrapping_add((j as i64).wrapping_mul(668_265_263));
        let n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
        ((n ^ (n >> 16)) as u32 as f64 / u32::MAX as f64) * 2.0 - 1.0
    };
    let a = h(x0, y0) + sx * (h(x0 + 1.0, y0) - h(x0, y0));
    let b = h(x0, y0 + 1.0) + sx * (h(x0 + 1.0, y0 + 1.0) - h(x0, y0 + 1.0));
    a + sy * (b - a)
}

/// Friction effective : la table de classe × un facteur organique lissé. C'est
/// ce que [`champ_de_cout`] et [`couts_batiments`] passent au graphe.
pub fn friction_locale(classe: Classe, milieu: [f64; 2]) -> f64 {
    friction(classe) * (1.0 + AMPLITUDE_ORGANIQUE * bruit(milieu))
}

/// Un champ de coût rasterisé, en mètres pondérés depuis la source.
pub struct ChampCout {
    /// `resolution × resolution`, ligne par ligne, `f64::INFINITY` hors de
    /// portée de la voirie.
    pub valeurs: Vec<f64>,
    pub resolution: usize,
    /// `[xmin, ymin, xmax, ymax]`, mètres du repère local.
    pub bornes_m: [f64; 4],
    /// Le coût maximal fini rencontré — pour caler une échelle de couleur.
    pub cout_max: f64,
}

/// Index spatial grossier des sommets porteurs d'un coût.
struct SemisSommets {
    cellules: HashMap<(i32, i32), Vec<u32>>,
    pas: f64,
}

impl SemisSommets {
    fn nouveau(points_m: &[[f64; 2]], couts: &[Option<f64>], pas: f64) -> SemisSommets {
        let mut cellules: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, (p, c)) in points_m.iter().zip(couts).enumerate() {
            if c.is_some() {
                let cle = ((p[0] / pas).floor() as i32, (p[1] / pas).floor() as i32);
                cellules.entry(cle).or_default().push(i as u32);
            }
        }
        SemisSommets { cellules, pas }
    }

    /// Le sommet porteur d'un coût le plus proche de `p`, en élargissant
    /// l'anneau de cellules jusqu'à en trouver un (ou abandonner).
    fn plus_proche(&self, p: [f64; 2], points_m: &[[f64; 2]]) -> Option<u32> {
        let (cx, cy) = ((p[0] / self.pas).floor() as i32, (p[1] / self.pas).floor() as i32);
        for anneau in 0..40_i32 {
            let mut meilleur: Option<(u32, f64)> = None;
            for dx in -anneau..=anneau {
                for dy in -anneau..=anneau {
                    // Seulement le bord de l'anneau (les précédents sont déjà vus).
                    if anneau > 0 && dx.abs() != anneau && dy.abs() != anneau {
                        continue;
                    }
                    let Some(v) = self.cellules.get(&(cx + dx, cy + dy)) else { continue };
                    for &i in v {
                        let q = points_m[i as usize];
                        let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
                        if meilleur.is_none_or(|(_, md)| d < md) {
                            meilleur = Some((i, d));
                        }
                    }
                }
            }
            if let Some((i, _)) = meilleur {
                // Un anneau de plus pour être sûr qu'un sommet d'une cellule
                // voisine non encore visitée n'est pas plus proche.
                if anneau > 0 {
                    return Some(i);
                }
            }
        }
        None
    }
}

/// Calcule le champ de coût depuis `source` (`[lon, lat]`), rasterisé sur une
/// grille `resolution × resolution` couvrant la frontière communale (ou, à
/// défaut, l'enveloppe des sommets).
pub fn champ_de_cout(
    extrait: &Extrait,
    repere: &Repere,
    source: [f64; 2],
    resolution: usize,
) -> ChampCout {
    let graphe = Graphe::construire_pondere(extrait, friction_locale);
    let couts_mm = graphe.couts_depuis(source);
    let points_m: Vec<[f64; 2]> = graphe.points().iter().map(|p| repere.vers_m(*p)).collect();
    let couts_m: Vec<Option<f64>> =
        couts_mm.iter().map(|c| c.map(|v| v as f64 / 1000.0)).collect();

    let gn = resolution.max(2);
    let bornes_m = bornes(extrait, repere, &points_m);
    let [xmin, ymin, xmax, ymax] = bornes_m;
    let pas_x = ((xmax - xmin) / gn as f64).max(1e-6);
    let pas_y = ((ymax - ymin) / gn as f64).max(1e-6);

    let semis = SemisSommets::nouveau(&points_m, &couts_m, 90.0);

    let mut valeurs = vec![f64::INFINITY; gn * gn];
    let mut cout_max = 0.0_f64;
    for gy in 0..gn {
        let cy = ymin + (gy as f64 + 0.5) * pas_y;
        for gx in 0..gn {
            let cx = xmin + (gx as f64 + 0.5) * pas_x;
            let p = [cx, cy];
            if let Some(i) = semis.plus_proche(p, &points_m) {
                if let Some(c) = couts_m[i as usize] {
                    // On n'accepte le coût du sommet que s'il est raisonnablement
                    // proche (200 m) : sinon une cellule au milieu d'un parc
                    // hériterait du coût d'une rue lointaine.
                    let d = ((points_m[i as usize][0] - cx).powi(2)
                        + (points_m[i as usize][1] - cy).powi(2))
                    .sqrt();
                    if d < 200.0 {
                        valeurs[gy * gn + gx] = c;
                        cout_max = cout_max.max(c);
                    }
                }
            }
        }
    }

    ChampCout { valeurs, resolution: gn, bornes_m, cout_max }
}

/// Coût de déplacement sur la voirie, de `source` (`[lon, lat]`) à **chaque
/// bâtiment** de `batiments` (`(id, centre en mètres locaux)`).
///
/// C'est ce qui définit la **zone peuplée** quand on veut qu'elle suive les
/// avenues au lieu d'un disque : `ville::preparer` garde les `N` bâtiments au
/// plus faible coût, pas les `N` plus proches à vol d'oiseau
/// (`docs/carto-ville.md`).
///
/// Coût d'un bâtiment = coût du sommet de voirie le plus proche + un petit
/// terme d'écart (`friction résidentielle × distance au sommet`) pour départager
/// deux bâtiments de la même rue. `f64::INFINITY` si aucun sommet routable
/// n'est à moins de `PORTEE_ACCROCHE` mètres, ou si le graphe est vide.
pub fn couts_batiments(
    extrait: &Extrait,
    repere: &Repere,
    batiments: &[(i64, [f64; 2])],
    source: [f64; 2],
) -> Vec<(i64, f64)> {
    const PORTEE_ACCROCHE: f64 = 160.0;
    let graphe = Graphe::construire_pondere(extrait, friction_locale);
    if graphe.est_vide() {
        return batiments.iter().map(|(id, _)| (*id, f64::INFINITY)).collect();
    }
    let couts_m: Vec<Option<f64>> = graphe
        .couts_depuis(source)
        .iter()
        .map(|c| c.map(|v| v as f64 / 1000.0))
        .collect();
    let points_m: Vec<[f64; 2]> = graphe.points().iter().map(|p| repere.vers_m(*p)).collect();
    let semis = SemisSommets::nouveau(&points_m, &couts_m, 90.0);
    let f_res = friction(Classe::Residentielle);

    batiments
        .iter()
        .map(|(id, centre)| {
            let cout = semis
                .plus_proche(*centre, &points_m)
                .and_then(|i| {
                    let c = couts_m[i as usize]?;
                    let d = ((points_m[i as usize][0] - centre[0]).powi(2)
                        + (points_m[i as usize][1] - centre[1]).powi(2))
                    .sqrt();
                    (d <= PORTEE_ACCROCHE).then_some(c + f_res * d)
                })
                .unwrap_or(f64::INFINITY);
            (*id, cout)
        })
        .collect()
}

/// Rectangle englobant, mètres locaux : la frontière communale si elle existe,
/// sinon l'enveloppe des sommets du graphe.
fn bornes(extrait: &Extrait, repere: &Repere, points_m: &[[f64; 2]]) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut voir = |p: [f64; 2]| {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    };
    match &extrait.frontiere {
        Some(f) => {
            for anneau in &f.anneaux {
                for p in anneau {
                    voir(repere.vers_m(*p));
                }
            }
        }
        None => points_m.iter().for_each(|p| voir(*p)),
    }
    if !b[0].is_finite() {
        b = [-1000.0, -1000.0, 1000.0, 1000.0];
    }
    b
}

/// Une bande de coût : le seuil bas et un ou plusieurs polygones (anneau
/// extérieur puis trous), en mètres du repère local.
pub struct Bande {
    pub seuil: f64,
    pub polygones: Vec<Vec<Vec<[f64; 2]>>>,
}

/// Découpe le champ en isobandes aux `seuils` donnés (mètres pondérés).
pub fn isobandes(champ: &ChampCout, seuils: &[f64]) -> Vec<Bande> {
    let gn = champ.resolution;
    let [xmin, ymin, xmax, ymax] = champ.bornes_m;
    let pas_x = ((xmax - xmin) / gn as f64).max(1e-6);
    let pas_y = ((ymax - ymin) / gn as f64).max(1e-6);

    // `contour` veut un champ fini : on remplace l'infini par une valeur bien
    // au-delà du dernier seuil, pour que ces cellules tombent hors de toutes
    // les bandes.
    let plafond = seuils.last().copied().unwrap_or(0.0) * 4.0 + 1.0;
    let borne: Vec<f64> = champ
        .valeurs
        .iter()
        .map(|v| if v.is_finite() { *v } else { plafond })
        .collect();

    let constructeur = contour::ContourBuilder::new(gn, gn, true)
        .x_origin(xmin)
        .y_origin(ymin)
        .x_step(pas_x)
        .y_step(pas_y);

    let mut bornes_seuils: Vec<f64> = vec![0.0];
    bornes_seuils.extend_from_slice(seuils);
    bornes_seuils.push(plafond);

    let mut sortie = Vec::new();
    for paire in bornes_seuils.windows(2) {
        let Ok(bandes) = constructeur.isobands(&borne, &[paire[0], paire[1]]) else { continue };
        let polygones: Vec<Vec<Vec<[f64; 2]>>> = bandes
            .into_iter()
            .flat_map(|b| {
                b.geometry()
                    .0
                    .iter()
                    .map(|poly| {
                        let mut anneaux = vec![anneau_ligne(poly.exterior())];
                        anneaux.extend(poly.interiors().iter().map(anneau_ligne));
                        anneaux
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        if !polygones.is_empty() {
            sortie.push(Bande { seuil: paire[0], polygones });
        }
    }
    sortie
}

fn anneau_ligne(ligne: &geo_types::LineString<f64>) -> Vec<[f64; 2]> {
    ligne.coords().map(|c| [c.x, c.y]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::Troncon;

    fn extrait_croix() -> Extrait {
        // Une grande voie E-O rapide, une petite voie N-S lente qui la croise.
        let troncons = vec![
            Troncon {
                id: 1,
                nom: Some("Avenue".into()),
                classe: Classe::Primaire,
                points: vec![[2.30, 48.85], [2.31, 48.85], [2.32, 48.85]],
            },
            Troncon {
                id: 2,
                nom: Some("Ruelle".into()),
                classe: Classe::Service,
                points: vec![[2.31, 48.84], [2.31, 48.85], [2.31, 48.86]],
            },
        ];
        Extrait { troncons, ..Default::default() }
    }

    #[test]
    fn la_grande_voie_coute_moins_au_metre() {
        let extrait = extrait_croix();
        let repere = Repere::centre_de(&extrait);
        let g = Graphe::construire_pondere(&extrait, friction_locale);
        let couts = g.couts_depuis([2.31, 48.85]);
        // Sommet au bout de l'avenue (est) vs sommet au bout de la ruelle
        // (nord) — distances géographiques comparables, coût très différent.
        let pts = g.points();
        let est = pts.iter().position(|p| (p[0] - 2.32).abs() < 1e-9).unwrap();
        let nord = pts.iter().position(|p| (p[1] - 48.86).abs() < 1e-9).unwrap();
        let ce = couts[est].unwrap();
        let cn = couts[nord].unwrap();
        assert!(ce < cn, "l'avenue ({ce}) doit coûter moins que la ruelle ({cn})");
        let _ = repere;
    }

    #[test]
    fn le_champ_est_borne_et_a_un_maximum_fini() {
        let extrait = extrait_croix();
        let repere = Repere::centre_de(&extrait);
        let champ = champ_de_cout(&extrait, &repere, [2.31, 48.85], 40);
        assert_eq!(champ.valeurs.len(), 40 * 40);
        assert!(champ.cout_max.is_finite() && champ.cout_max >= 0.0);
        assert!(champ.valeurs.iter().any(|v| v.is_finite()), "au moins une cellule sur la voirie");
    }

    #[test]
    fn les_isobandes_sortent_des_polygones() {
        let extrait = extrait_croix();
        let repere = Repere::centre_de(&extrait);
        let champ = champ_de_cout(&extrait, &repere, [2.31, 48.85], 60);
        let bandes = isobandes(&champ, &[50.0, 150.0, 400.0]);
        assert!(!bandes.is_empty(), "au moins une bande de coût");
    }
}
