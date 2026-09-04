//! De l'espace de la carte au monde géographique fictif.
//!
//! MapLibre ne sait afficher qu'une Terre. La carte, elle, vit dans un carré
//! `[-D, D]²` sans rapport avec une géographie. Le passage de l'un à l'autre
//! est le piège que `CLAUDE.md` signale, et il se joue en une décision :
//!
//! **le carré de la carte est le monde entier.** Pas une région de la Terre,
//! pas un rectangle posé près de l'équateur — le planisphère complet. Le zoom 0
//! montre alors toute la bibliothèque dans une seule tuile, et chaque niveau
//! suivant la découpe en quatre, exactement comme une carte routière.
//!
//! Deux conséquences qu'il vaut mieux avoir en tête :
//!
//! - **la déformation de Mercator est réelle, mais elle nous arrange.** Aux
//!   latitudes extrêmes, une même distance de carte occupe plus de pixels. Le
//!   nuage de points étant centré, ce sont ses bords — les familles rares, la
//!   longue traîne — qui gagnent de la place. C'est le contraire d'un défaut ;
//! - **le carré de la carte doit couvrir exactement le champ de densité.** La
//!   demi-étendue [`DEMI_ETENDUE`] reprend la marge de `core::density`, sinon
//!   les territoires et le relief seraient décalés l'un par rapport à l'autre.

/// Demi-côté du domaine de la carte. Le nuage tient dans `[-1, 1]` après
/// cadrage ; `core::density::MARGE` (0,08) élargit le champ pour que les
/// bandes du bord ne soient pas coupées à vif. Les tuiles couvrent donc le
/// même carré que la nappe de densité, au flottant près.
pub const DEMI_ETENDUE: f64 = 1.08;

/// Résolution d'une tuile vectorielle, en unités internes MVT. 4096 est la
/// valeur usuelle : au zoom maximal, une unité vaut une fraction de pixel.
pub const ETENDUE_TUILE: i32 = 4096;

/// Position dans le carré unité du monde, origine en haut à gauche —
/// la convention des tuiles « slippy map ». `u` va vers l'est, `v` vers le sud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monde {
    pub u: f64,
    pub v: f64,
}

/// Coordonnées géographiques fictives. Ce ne sont pas des lieux : c'est ce que
/// MapLibre attend pour placer une caméra et lire un GeoJSON.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geo {
    pub lon: f64,
    pub lat: f64,
}

/// Carte → carré unité du monde.
///
/// L'axe des ordonnées s'inverse : sur la carte `y` monte, sur une tuile `v`
/// descend. Oublier cette inversion retourne le monde du nord au sud, et le
/// symptôme est déroutant — tout est là, mais en miroir.
pub fn carte_vers_monde(x: f64, y: f64) -> Monde {
    Monde {
        u: (x / DEMI_ETENDUE + 1.0) / 2.0,
        v: (1.0 - y / DEMI_ETENDUE) / 2.0,
    }
}

/// Carré unité du monde → carte.
pub fn monde_vers_carte(m: Monde) -> (f64, f64) {
    (
        (m.u * 2.0 - 1.0) * DEMI_ETENDUE,
        (1.0 - m.v * 2.0) * DEMI_ETENDUE,
    )
}

/// Carré unité du monde → longitude/latitude, par la projection de Mercator
/// sphérique (EPSG:3857), celle qu'emploient toutes les tuiles web.
///
/// `v = 0` donne la latitude 85,051° — le bord nord du carré de Mercator, où
/// la projection est tronquée parce qu'elle diverge au pôle.
pub fn monde_vers_geo(m: Monde) -> Geo {
    let n = std::f64::consts::PI * (1.0 - 2.0 * m.v);
    Geo {
        lon: (m.u * 2.0 - 1.0) * 180.0,
        lat: n.sinh().atan().to_degrees(),
    }
}

/// Longitude/latitude → carré unité du monde.
pub fn geo_vers_monde(g: Geo) -> Monde {
    let phi = g.lat.to_radians();
    Monde {
        u: (g.lon / 180.0 + 1.0) / 2.0,
        v: (1.0 - (phi.tan() + 1.0 / phi.cos()).ln() / std::f64::consts::PI) / 2.0,
    }
}

/// Carte → longitude/latitude, en une fois.
pub fn carte_vers_geo(x: f64, y: f64) -> Geo {
    monde_vers_geo(carte_vers_monde(x, y))
}

/// Coordonnées locales d'un point dans une tuile donnée, en unités MVT.
///
/// Rend des valeurs hors de `[0, ETENDUE_TUILE]` quand le point tombe à côté :
/// c'est voulu. Une tuile doit porter un peu de ses voisines, sinon un symbole
/// à cheval sur la limite disparaît d'un côté et réapparaît de l'autre.
/// L'appelant décide de la marge qu'il tolère (voir `couches::MARGE_TUILE`).
pub fn monde_vers_tuile(m: Monde, z: u8, tx: u32, ty: u32) -> (f64, f64) {
    let n = (1u64 << z) as f64;
    let e = ETENDUE_TUILE as f64;
    ((m.u * n - tx as f64) * e, (m.v * n - ty as f64) * e)
}

/// Les bornes en carré-monde d'une tuile, marge comprise.
///
/// Sert à ne pas balayer 27 000 points pour chaque tuile : on écarte d'abord
/// tout ce qui tombe hors de ces bornes.
pub fn bornes_tuile(z: u8, tx: u32, ty: u32, marge: f64) -> (f64, f64, f64, f64) {
    let n = (1u64 << z) as f64;
    let cote = 1.0 / n;
    let m = marge * cote;
    (
        tx as f64 * cote - m,
        ty as f64 * cote - m,
        (tx + 1) as f64 * cote + m,
        (ty + 1) as f64 * cote + m,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'aller-retour doit être l'identité, sinon les couches se décalent
    /// entre elles sans qu'aucune n'ait l'air fausse isolément.
    #[test]
    fn laller_retour_carte_monde_est_neutre() {
        for &(x, y) in &[(0.0, 0.0), (1.0, -1.0), (-1.08, 1.08), (0.37, -0.62)] {
            let (rx, ry) = monde_vers_carte(carte_vers_monde(x, y));
            assert!((rx - x).abs() < 1e-12, "x : {x} → {rx}");
            assert!((ry - y).abs() < 1e-12, "y : {y} → {ry}");
        }
    }

    #[test]
    fn laller_retour_monde_geo_est_neutre() {
        for &(u, v) in &[(0.5, 0.5), (0.0, 0.0), (1.0, 1.0), (0.13, 0.87)] {
            let m = Monde { u, v };
            let r = geo_vers_monde(monde_vers_geo(m));
            assert!((r.u - u).abs() < 1e-9, "u : {u} → {}", r.u);
            assert!((r.v - v).abs() < 1e-9, "v : {v} → {}", r.v);
        }
    }

    /// Le centre de la carte est le point (0°, 0°), et les coins touchent les
    /// bords du carré de Mercator. C'est ce qui fait que le zoom 0 montre
    /// toute la bibliothèque et rien d'autre.
    #[test]
    fn la_carte_occupe_le_monde_entier() {
        let centre = carte_vers_geo(0.0, 0.0);
        assert!(centre.lon.abs() < 1e-12 && centre.lat.abs() < 1e-12);

        let nord_ouest = carte_vers_geo(-DEMI_ETENDUE, DEMI_ETENDUE);
        assert!((nord_ouest.lon + 180.0).abs() < 1e-9);
        assert!(
            (nord_ouest.lat - 85.0511287798066).abs() < 1e-9,
            "latitude du coin : {}",
            nord_ouest.lat
        );

        let sud_est = carte_vers_geo(DEMI_ETENDUE, -DEMI_ETENDUE);
        assert!((sud_est.lon - 180.0).abs() < 1e-9);
        assert!((sud_est.lat + 85.0511287798066).abs() < 1e-9);
    }

    /// L'inversion de l'axe des ordonnées : le haut de la carte doit être le
    /// nord. Le test existe parce que l'erreur inverse est invisible sur un
    /// nuage à peu près symétrique.
    #[test]
    fn le_haut_de_la_carte_est_le_nord() {
        assert!(carte_vers_geo(0.0, 0.5).lat > carte_vers_geo(0.0, -0.5).lat);
        assert!(carte_vers_geo(0.5, 0.0).lon > carte_vers_geo(-0.5, 0.0).lon);
    }

    /// Au zoom 0 il n'y a qu'une tuile, et le centre de la carte tombe en son
    /// milieu. Au zoom 1, le même point tombe au coin des quatre tuiles.
    #[test]
    fn les_coordonnees_de_tuile_suivent_le_zoom() {
        let m = carte_vers_monde(0.0, 0.0);
        let (px, py) = monde_vers_tuile(m, 0, 0, 0);
        assert!((px - 2048.0).abs() < 1e-6 && (py - 2048.0).abs() < 1e-6);

        let (px, py) = monde_vers_tuile(m, 1, 0, 0);
        assert!((px - 4096.0).abs() < 1e-6 && (py - 4096.0).abs() < 1e-6);
    }

    #[test]
    fn les_bornes_de_tuile_encadrent_leur_contenu() {
        let (u0, v0, u1, v1) = bornes_tuile(2, 1, 1, 0.0);
        assert!((u0 - 0.25).abs() < 1e-12 && (u1 - 0.5).abs() < 1e-12);
        assert!((v0 - 0.25).abs() < 1e-12 && (v1 - 0.5).abs() < 1e-12);
        // Avec marge, la tuile déborde symétriquement.
        let (a0, _, a1, _) = bornes_tuile(2, 1, 1, 0.1);
        assert!(a0 < u0 && a1 > u1);
    }
}
