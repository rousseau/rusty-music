// SPDX-License-Identifier: GPL-3.0-or-later
//! La voie sur laquelle donne chaque bâtiment : son **tronçon le plus proche**.
//!
//! Sert au peuplement chronologique (`crate::croissance`) : un bâtiment qui
//! borde une grande avenue est sur l'artère, celui qui borde une ruelle est
//! dans un petit quartier. Et, après le logement, à nommer les rues d'après
//! leurs habitants.

use std::collections::HashMap;

use rusty_music_osm::{Classe, Extrait};

use crate::affectation::Repere;

/// Côté de cellule de la grille de segments, mètres.
const PAS: f64 = 50.0;

/// La voie la plus proche d'un bâtiment.
#[derive(Clone, Copy, Debug)]
pub struct Facade {
    pub classe: Classe,
    /// Indice dans `Extrait::troncons`.
    pub troncon: usize,
    /// Distance du centre du bâtiment au tronçon, mètres.
    pub distance: f64,
    /// L'importance de la **plus grande voie à moins de [`RAYON_ARTERE`]**
    /// ([`artere`]), pas seulement de la plus proche : un bâtiment qui donne
    /// sur une ruelle mais touche l'avenue d'à côté est sur l'artère. Sans ça,
    /// un sentier de cour ou une contre-allée « masque » l'avenue.
    pub artere: f64,
}

/// Rayon dans lequel une grande voie fait d'un bâtiment un riverain de
/// l'artère, mètres.
pub const RAYON_ARTERE: f64 = 35.0;

/// Importance d'une voie comme **artère**, `0..=1`. C'est ce que lit
/// `croissance::Parcelle::artere` : un grand axe vaut 1, la rue ordinaire
/// 0,1. L'autoroute (le périphérique) n'est pas une artère habitée — on n'y
/// loge pas, mais les bâtiments qui la longent restent « sur un grand axe »
/// à demi.
pub fn artere(classe: Classe) -> f64 {
    match classe {
        Classe::Primaire => 1.0,
        Classe::Secondaire => 0.8,
        Classe::Autoroute => 0.5,
        Classe::Tertiaire => 0.35,
        Classe::Residentielle => 0.1,
        Classe::Pietonne | Classe::Service => 0.0,
    }
}

fn distance_point_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 <= 1e-12 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    let (qx, qy) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - qx).powi(2) + (p[1] - qy).powi(2)).sqrt()
}

/// Pour chaque point de `centres` (mètres locaux), le tronçon de l'extrait le
/// plus proche à moins de `portee` mètres, `None` sinon.
///
/// Distance point-segment exacte, pas au sommet le plus proche : une avenue
/// aux sommets espacés de 80 m ne doit pas sembler lointaine à un bâtiment qui
/// la borde entre deux sommets.
pub fn facades(extrait: &Extrait, repere: &Repere, centres: &[[f64; 2]], portee: f64) -> Vec<Option<Facade>> {
    let cle = |p: [f64; 2]| ((p[0] / PAS).floor() as i32, (p[1] / PAS).floor() as i32);
    let mut grille: HashMap<(i32, i32), Vec<(u32, u32)>> = HashMap::new();
    let mut segments: Vec<Vec<[f64; 2]>> = Vec::with_capacity(extrait.troncons.len());
    for (t, tr) in extrait.troncons.iter().enumerate() {
        let pts: Vec<[f64; 2]> = tr.points.iter().map(|p| repere.vers_m(*p)).collect();
        for (s, paire) in pts.windows(2).enumerate() {
            let (c0, c1) = (cle(paire[0]), cle(paire[1]));
            for cx in c0.0.min(c1.0)..=c0.0.max(c1.0) {
                for cy in c0.1.min(c1.1)..=c0.1.max(c1.1) {
                    grille.entry((cx, cy)).or_default().push((t as u32, s as u32));
                }
            }
        }
        segments.push(pts);
    }

    let etendue = (portee / PAS).ceil() as i32;
    centres
        .iter()
        .map(|&c| {
            let (cx, cy) = cle(c);
            let mut meilleur: Option<(usize, f64)> = None;
            let mut grande = 0.0_f64;
            for dx in -etendue..=etendue {
                for dy in -etendue..=etendue {
                    let Some(v) = grille.get(&(cx + dx, cy + dy)) else { continue };
                    for &(t, s) in v {
                        let pts = &segments[t as usize];
                        let d = distance_point_segment(c, pts[s as usize], pts[s as usize + 1]);
                        if d <= RAYON_ARTERE {
                            grande = grande.max(artere(extrait.troncons[t as usize].classe));
                        }
                        if d <= portee && meilleur.is_none_or(|(_, md)| d < md) {
                            meilleur = Some((t as usize, d));
                        }
                    }
                }
            }
            meilleur.map(|(t, d)| Facade {
                classe: extrait.troncons[t].classe,
                troncon: t,
                distance: d,
                artere: grande,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::Troncon;

    fn extrait() -> (Extrait, Repere) {
        // Une avenue (primaire) à l'équateur local et une ruelle parallèle ~60 m au nord.
        let troncons = vec![
            Troncon {
                id: 1,
                nom: Some("Avenue".into()),
                classe: Classe::Primaire,
                points: vec![[2.300, 48.850], [2.310, 48.850]],
            },
            Troncon {
                id: 2,
                nom: Some("Ruelle".into()),
                classe: Classe::Residentielle,
                points: vec![[2.300, 48.8506], [2.310, 48.8506]],
            },
        ];
        let e = Extrait { troncons, ..Default::default() };
        let r = Repere::centre_de(&e);
        (e, r)
    }

    #[test]
    fn la_voie_la_plus_proche_est_celle_qui_borde() {
        let (e, r) = extrait();
        let sur_avenue = r.vers_m([2.305, 48.85005]);
        let sur_ruelle = r.vers_m([2.305, 48.8506]);
        let f = facades(&e, &r, &[sur_avenue, sur_ruelle], 60.0);
        assert_eq!(f[0].unwrap().classe, Classe::Primaire);
        assert_eq!(f[1].unwrap().classe, Classe::Residentielle);
    }

    #[test]
    fn un_batiment_sur_la_ruelle_a_cote_de_l_avenue_est_riverain_de_l_artere() {
        let (e, r) = extrait();
        // ~33 m de l'avenue comme de la ruelle (< RAYON_ARTERE) : la voie la
        // plus proche peut être l'une ou l'autre, mais l'avenue est à portée
        // d'artère dans tous les cas.
        let p = r.vers_m([2.305, 48.85015]);
        let f = facades(&e, &r, &[p], 60.0)[0].unwrap();
        assert!(f.artere >= 1.0 - 1e-9, "l'avenue est à portée d'artère : {}", f.artere);
    }

    #[test]
    fn hors_de_portee_il_n_y_a_pas_de_facade() {
        let (e, r) = extrait();
        let loin = r.vers_m([2.305, 48.860]);
        assert!(facades(&e, &r, &[loin], 60.0)[0].is_none());
    }

    #[test]
    fn la_hierarchie_des_artères() {
        assert!(artere(Classe::Primaire) > artere(Classe::Secondaire));
        assert!(artere(Classe::Secondaire) > artere(Classe::Tertiaire));
        assert!(artere(Classe::Tertiaire) > artere(Classe::Residentielle));
        assert!(artere(Classe::Residentielle) > artere(Classe::Service));
    }
}
