//! Profils de routage pour l'itinéraire du mode Explorer.
//!
//! `docs/carto-google-maps.md` §3 : un seul graphe, plusieurs fonctions de
//! coût. Ici le graphe est la voirie réelle (`crate::reseau_reel`) et chaque
//! profil est une pondération d'arête différente :
//! - **par le connu** : suit les grandes avenues et boulevards ;
//! - **redécouvrir** : traîne dans les petites rues calmes ;
//! - **panoramique** : petites rues, en longeant les parcs et l'eau.
//!
//! **Ces tables ne sont pas `cout_voirie::friction`** (qui, elle, sert à
//! donner sa forme à la zone peuplée et rend l'autoroute *bon marché* : la
//! ville s'étire le long du périphérique). Ici on route un piéton : les voies
//! rapides (périph, berges) coûtent cher dans **tous** les profils — on ne
//! traverse pas Paris par le périphérique. C'est aussi pourquoi il n'y a plus
//! de case « éviter les autoroutes » : c'est déjà le cas partout.
//! Le *rapport* entre les classes fait le profil, pas les valeurs absolues.

use rusty_music_osm::{Classe, Extrait};

use crate::affectation::Repere;

/// Le profil de routage demandé depuis l'interface (`#bloc-itineraire`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ProfilVoirie {
    /// « Par le connu » : le trajet suit les grands axes — avenues, boulevards.
    ParLeConnu,
    /// « Redécouvrir » : par les rues calmes, à l'écart des grands axes.
    Redecouvrir,
    /// « Panoramique » : rues calmes, en longeant les espaces verts et l'eau.
    Panoramique,
}

impl ProfilVoirie {
    /// Depuis la chaîne passée par l'interface (`data-profil`). Inconnu →
    /// `ParLeConnu`, le défaut du panneau.
    pub fn depuis_nom(s: &str) -> ProfilVoirie {
        match s {
            "sentier" => ProfilVoirie::Redecouvrir,
            "panoramique" => ProfilVoirie::Panoramique,
            _ => ProfilVoirie::ParLeConnu,
        }
    }
}

/// « Par le connu » : le meilleur marché est la voie *secondaire* (l'avenue
/// urbaine typique) puis la primaire (boulevard). L'autoroute reste chère —
/// on marche, on ne prend pas le périph — et le résidentiel/piéton l'est aussi,
/// pour que le trajet colle aux grands axes.
fn friction_par_le_connu(classe: Classe) -> f64 {
    match classe {
        Classe::Autoroute => 3.0,
        Classe::Primaire => 0.5,
        Classe::Secondaire => 0.4,
        Classe::Tertiaire => 0.7,
        Classe::Residentielle => 1.1,
        Classe::Pietonne => 1.2,
        Classe::Service => 2.5,
    }
}

/// « Redécouvrir » : l'inverse — le résidentiel et le piéton sont bon marché,
/// les grands axes coûtent cher (sans être infranchissables : un boulevard
/// reste acceptable s'il évite un long détour).
fn friction_redecouvrir(classe: Classe) -> f64 {
    match classe {
        Classe::Autoroute => 4.0,
        Classe::Primaire => 2.2,
        Classe::Secondaire => 1.7,
        Classe::Tertiaire => 1.1,
        Classe::Residentielle => 0.75,
        Classe::Pietonne => 0.6,
        Classe::Service => 1.0,
    }
}

/// « Panoramique » : comme redécouvrir, un peu moins tranché (on accepte plus
/// volontiers une belle avenue), et le bonus « aux abords d'un parc/de l'eau »
/// se combine par-dessus dans [`friction_itineraire`].
fn friction_panoramique_base(classe: Classe) -> f64 {
    match classe {
        Classe::Autoroute => 4.0,
        Classe::Primaire => 1.5,
        Classe::Secondaire => 1.2,
        Classe::Tertiaire => 1.0,
        Classe::Residentielle => 0.8,
        Classe::Pietonne => 0.6,
        Classe::Service => 1.1,
    }
}

/// Une grille booléenne « à moins de ~120 m d'un espace vert ou d'un plan
/// d'eau », précalculée une fois depuis `extrait.verts` + `extrait.eaux`.
///
/// Tester chaque arête (235 000) contre chaque polygone de parc coûterait
/// trop ; on rasterise les intérieurs une fois, on dilate de la portée voulue,
/// et l'échantillonnage par arête est ensuite un accès tableau.
pub struct ProximiteAgrement {
    repere: Repere,
    valeurs: Vec<bool>,
    nx: usize,
    ny: usize,
    min: [f64; 2],
    pas: f64,
}

impl ProximiteAgrement {
    /// `portee_m` : distance au-delà de la bordure d'un parc encore comptée
    /// comme « aux abords » (~120 m est un bon défaut — une rue qui borde le
    /// parc, plus le trottoir d'en face).
    pub fn nouvelle(extrait: &Extrait, portee_m: f64) -> ProximiteAgrement {
        let repere = Repere::centre_de(extrait);
        let contours: Vec<&Vec<[f64; 2]>> = extrait
            .verts
            .iter()
            .chain(&extrait.eaux)
            .map(|c| &c.points)
            .filter(|pts| pts.len() >= 3)
            .collect();

        let pas = 40.0_f64;
        // Bornes : l'enveloppe des contours, marge d'une portée.
        let (mut min, mut max) = ([f64::MAX; 2], [f64::MIN; 2]);
        for pts in &contours {
            for p in pts.iter() {
                let m = repere.vers_m(*p);
                min[0] = min[0].min(m[0]);
                min[1] = min[1].min(m[1]);
                max[0] = max[0].max(m[0]);
                max[1] = max[1].max(m[1]);
            }
        }
        if min[0] > max[0] {
            // Aucun contour exploitable — grille vide, `proche` répondra `false`.
            return ProximiteAgrement { repere, valeurs: Vec::new(), nx: 0, ny: 0, min: [0.0; 2], pas };
        }
        min[0] -= portee_m;
        min[1] -= portee_m;
        max[0] += portee_m;
        max[1] += portee_m;
        let nx = (((max[0] - min[0]) / pas).ceil() as usize).max(1);
        let ny = (((max[1] - min[1]) / pas).ceil() as usize).max(1);
        let mut valeurs = vec![false; nx * ny];

        // 1. Intérieurs : test de parité sur le centre de chaque cellule, borné
        //    à la boîte englobante du contour.
        for pts in &contours {
            let (mut cmin, mut cmax) = ([f64::MAX; 2], [f64::MIN; 2]);
            let ms: Vec<[f64; 2]> = pts.iter().map(|p| repere.vers_m(*p)).collect();
            for m in &ms {
                cmin[0] = cmin[0].min(m[0]);
                cmin[1] = cmin[1].min(m[1]);
                cmax[0] = cmax[0].max(m[0]);
                cmax[1] = cmax[1].max(m[1]);
            }
            let gx0 = (((cmin[0] - min[0]) / pas).floor() as isize).max(0) as usize;
            let gy0 = (((cmin[1] - min[1]) / pas).floor() as isize).max(0) as usize;
            let gx1 = (((cmax[0] - min[0]) / pas).ceil() as usize).min(nx);
            let gy1 = (((cmax[1] - min[1]) / pas).ceil() as usize).min(ny);
            for gy in gy0..gy1 {
                let cy = min[1] + (gy as f64 + 0.5) * pas;
                for gx in gx0..gx1 {
                    let cx = min[0] + (gx as f64 + 0.5) * pas;
                    if point_dans_anneau([cx, cy], &ms) {
                        valeurs[gy * nx + gx] = true;
                    }
                }
            }
        }

        // 2. Dilatation de `portee_m` : une cellule vraie contamine ses
        //    voisines dans un rayon de `r` cellules.
        let r = (portee_m / pas).ceil() as isize;
        if r > 0 {
            let source = valeurs.clone();
            for gy in 0..ny as isize {
                for gx in 0..nx as isize {
                    if !source[gy as usize * nx + gx as usize] {
                        continue;
                    }
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let (x, y) = (gx + dx, gy + dy);
                            if x >= 0 && y >= 0 && (x as usize) < nx && (y as usize) < ny {
                                valeurs[y as usize * nx + x as usize] = true;
                            }
                        }
                    }
                }
            }
        }

        ProximiteAgrement { repere, valeurs, nx, ny, min, pas }
    }

    /// `p` (`[lon, lat]`) est-il aux abords d'un parc ou d'un plan d'eau ?
    pub fn proche(&self, p: [f64; 2]) -> bool {
        if self.valeurs.is_empty() {
            return false;
        }
        let m = self.repere.vers_m(p);
        let gx = ((m[0] - self.min[0]) / self.pas).floor();
        let gy = ((m[1] - self.min[1]) / self.pas).floor();
        if gx < 0.0 || gy < 0.0 || gx as usize >= self.nx || gy as usize >= self.ny {
            return false;
        }
        self.valeurs[gy as usize * self.nx + gx as usize]
    }
}

/// Test de parité pair-impair d'un point contre un anneau (fermé ou non).
fn point_dans_anneau(p: [f64; 2], anneau: &[[f64; 2]]) -> bool {
    let mut dedans = false;
    let n = anneau.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (anneau[i], anneau[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x > p[0] {
                dedans = !dedans;
            }
        }
        j = i;
    }
    dedans
}

/// La fonction de friction à passer à [`crate::reseau_reel::Graphe::construire_pondere`].
///
/// `agrement` n'a d'effet que pour [`ProfilVoirie::Panoramique`] ; le passer
/// `None` revient à un panoramique sans bonus « aux abords d'un parc ».
pub fn friction_itineraire(
    profil: ProfilVoirie,
    agrement: Option<&ProximiteAgrement>,
) -> impl Fn(Classe, [f64; 2]) -> f64 + '_ {
    move |classe, milieu| match profil {
        ProfilVoirie::ParLeConnu => friction_par_le_connu(classe),
        ProfilVoirie::Redecouvrir => friction_redecouvrir(classe),
        ProfilVoirie::Panoramique => {
            let base = friction_panoramique_base(classe);
            match agrement {
                Some(a) if a.proche(milieu) => base * 0.65,
                _ => base,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::{Contour, Troncon};

    #[test]
    fn depuis_nom_reconnait_les_trois_profils() {
        assert_eq!(ProfilVoirie::depuis_nom("autoroute"), ProfilVoirie::ParLeConnu);
        assert_eq!(ProfilVoirie::depuis_nom("sentier"), ProfilVoirie::Redecouvrir);
        assert_eq!(ProfilVoirie::depuis_nom("panoramique"), ProfilVoirie::Panoramique);
        assert_eq!(ProfilVoirie::depuis_nom("n'importe quoi"), ProfilVoirie::ParLeConnu);
    }

    #[test]
    fn par_le_connu_suit_les_grands_axes_pas_le_periph() {
        let f = friction_itineraire(ProfilVoirie::ParLeConnu, None);
        let o = [0.0, 0.0];
        // L'avenue (secondaire/primaire) est le meilleur marché ; le résidentiel
        // coûte plus cher ; l'autoroute (périph) coûte cher — on ne traverse pas
        // Paris à pied par le périphérique.
        assert!(f(Classe::Secondaire, o) < f(Classe::Residentielle, o));
        assert!(f(Classe::Primaire, o) < f(Classe::Residentielle, o));
        assert!(f(Classe::Autoroute, o) > f(Classe::Residentielle, o));
    }

    #[test]
    fn redecouvrir_inverse_la_hierarchie() {
        let f = friction_itineraire(ProfilVoirie::Redecouvrir, None);
        let o = [0.0, 0.0];
        // L'avenue coûte plus cher que la rue résidentielle — l'inverse de
        // « par le connu » — et l'autoroute reste chère.
        assert!(f(Classe::Primaire, o) > f(Classe::Residentielle, o));
        assert!(f(Classe::Secondaire, o) > f(Classe::Tertiaire, o));
        assert!(f(Classe::Autoroute, o) > f(Classe::Primaire, o));
    }

    #[test]
    fn lautoroute_est_chere_dans_tous_les_profils() {
        let o = [0.0, 0.0];
        for p in [ProfilVoirie::ParLeConnu, ProfilVoirie::Redecouvrir, ProfilVoirie::Panoramique] {
            let f = friction_itineraire(p, None);
            assert!(
                f(Classe::Autoroute, o) >= 2.0,
                "{p:?} : l'autoroute doit rester dissuasive pour un piéton"
            );
        }
    }

    #[test]
    fn panoramique_favorise_les_abords_de_parc() {
        // Un carré de « vert » de ~400 m de côté autour de (2.30, 48.86).
        let vert = Contour {
            id: 1,
            points: vec![
                [2.298, 48.858],
                [2.302, 48.858],
                [2.302, 48.862],
                [2.298, 48.862],
                [2.298, 48.858],
            ],
        };
        let extrait = Extrait {
            troncons: vec![Troncon {
                id: 1,
                nom: None,
                classe: Classe::Residentielle,
                points: vec![[2.30, 48.86], [2.31, 48.86]],
            }],
            verts: vec![vert],
            ..Default::default()
        };
        let agrement = ProximiteAgrement::nouvelle(&extrait, 120.0);
        assert!(agrement.proche([2.30, 48.86]), "au cœur du parc");
        assert!(!agrement.proche([2.35, 48.86]), "loin du parc");

        let f = friction_itineraire(ProfilVoirie::Panoramique, Some(&agrement));
        let pres = f(Classe::Residentielle, [2.30, 48.86]);
        let loin = f(Classe::Residentielle, [2.35, 48.86]);
        assert!(pres < loin, "une rue au bord du parc doit être préférée : {pres} vs {loin}");
    }
}
