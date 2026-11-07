// SPDX-License-Identifier: GPL-3.0-or-later
//! Le relief musical du plan réel (`docs/plan-balade.md`, P6) : une altitude
//! qui a un sens — l'énergie des morceaux par défaut, le tempo, la popularité
//! ou la familiarité au choix —, ombrée par MapLibre et coupée de courbes de
//! niveau numérotées (l'emprunt à Qoyra, `recherche-balade-musicale.md`, § 1).
//!
//! **Le champ.** Chaque morceau logé apporte sa valeur, ramenée dans `[0, 1]`
//! ([`normaliser`]). Sur une grille de [`PAS_M`] mètres, la moyenne de ses
//! voisins pondérée par un noyau gaussien de [`SIGMA_M`] mètres
//! (Nadaraya-Watson), étirée entre ses 2ᵉ et 98ᵉ centiles sur la zone peuplée
//! pour que les dix bandes se partagent la ville. Hors zone peuplée,
//! l'altitude décroît vers zéro avec la densité de morceaux : pas de falaise
//! au bord.
//!
//! **Pourquoi pas l'ombrage maison de `relief.rs`.** Le monde fictif n'a pas
//! de mètres ; Paris en a. MapLibre ombre donc lui-même un `raster-dem`
//! (encodage Terrarium) que l'on écrit ici, à [`ALTITUDE_MAX_M`] mètres pour
//! la bande la plus haute — un relief de colline, assez pour un ombrage
//! léger.
//!
//! **Les courbes.** Les anneaux des régions `altitude ≥ k/10` pour `k` de 1 à
//! 9 (`contour`, la brique des isobandes du monde fictif et des quartiers).
//! Chaque anneau est une courbe fermée, numérotée par son niveau : extérieur,
//! il enferme un sommet (au-dessus de son niveau) ; trou, une cuvette.

use std::io::BufWriter;
use std::path::Path;

use pmtiles::{Compression, PmTilesWriter, TileCoord, TileId, TileType};
use serde::{Deserialize, Serialize};

use crate::projection::{self, Geo, Monde, ETENDUE_TUILE};

/// Rayon du noyau (écart-type), mètres : quelques îlots. Plus court, le relief
/// suit chaque immeuble ; plus long, il ne reste qu'une pente d'un bout de
/// Paris à l'autre.
pub const SIGMA_M: f64 = 120.0;
/// Côté d'une cellule de la grille, mètres.
pub const PAS_M: f64 = 20.0;
/// Dix bandes, donc neuf courbes, numérotées de 1 à 9.
pub const NIVEAUX: u8 = 10;
/// Altitude de la bande la plus haute dans les tuiles `raster-dem`, mètres.
pub const ALTITUDE_MAX_M: f64 = 60.0;
/// Densité de morceaux (en morceaux au plus près, noyau de pic 1) à laquelle
/// l'altitude atteint 63 % de sa valeur : un morceau isolé porte déjà presque
/// tout son relief, la plaine commence à deux écarts-types de lui.
const DENSITE_PLEINE: f64 = 0.3;
/// Zooms des tuiles : en deçà, Paris tient dans une poignée de pixels ; au
/// delà, MapLibre agrandit.
pub const ZOOM_MIN: u8 = 10;
pub const ZOOM_DEM_MAX: u8 = 14;
pub const ZOOM_COURBES_MAX: u8 = 15;
/// Côté d'une tuile `raster-dem`, pixels.
const COTE_DEM: u32 = 256;
/// Marge d'une tuile vectorielle, en unités MVT : une ligne qui sort de la
/// tuile y garde un point de plus, pour que le trait ne s'arrête pas au bord.
const MARGE_MVT: f64 = 128.0;

/// La grandeur qui fait l'altitude.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Grandeur {
    #[default]
    Energie,
    Tempo,
    Popularite,
    /// Les écoutes de la table `ecoute` : ce que l'on connaît.
    Familiarite,
}

impl Grandeur {
    pub const TOUTES: [Grandeur; 4] = [
        Grandeur::Energie,
        Grandeur::Tempo,
        Grandeur::Popularite,
        Grandeur::Familiarite,
    ];

    /// L'identifiant des archives et des URL (`dem-energie`).
    pub const fn id(self) -> &'static str {
        match self {
            Grandeur::Energie => "energie",
            Grandeur::Tempo => "tempo",
            Grandeur::Popularite => "popularite",
            Grandeur::Familiarite => "familiarite",
        }
    }

    pub fn par_id(id: &str) -> Option<Grandeur> {
        Grandeur::TOUTES.into_iter().find(|g| g.id() == id)
    }
}

/// Ramène des valeurs brutes dans `[0, 1]`, selon la grandeur. `None` : pas de
/// valeur, le morceau ne compte pas dans le champ.
///
/// - énergie, tempo : **rang** (centile), parce que leurs distributions sont
///   tassées et que dix bandes doivent s'y répartir ;
/// - popularité : déjà un centile (`track_popularite.relative`) ;
/// - familiarité (nombre d'écoutes) : `ln(1 + n) / ln(1 + max)`. Un rang
///   mettrait les 98 % jamais écoutés à mi-hauteur, ex æquo.
pub fn normaliser(g: Grandeur, brutes: &[Option<f32>]) -> Vec<Option<f32>> {
    match g {
        Grandeur::Energie | Grandeur::Tempo => rangs(brutes),
        Grandeur::Popularite => brutes
            .iter()
            .map(|v| v.map(|x| x.clamp(0.0, 1.0)))
            .collect(),
        Grandeur::Familiarite => {
            let max = brutes.iter().flatten().fold(0.0f32, |m, &x| m.max(x));
            let d = (1.0 + max).ln().max(1e-6);
            brutes
                .iter()
                .map(|v| Some((1.0 + v.unwrap_or(0.0).max(0.0)).ln() / d))
                .collect()
        }
    }
}

/// Le rang centile de chaque valeur dans `[0, 1]`, les ex æquo au rang moyen.
pub fn rangs(brutes: &[Option<f32>]) -> Vec<Option<f32>> {
    let mut idx: Vec<usize> = (0..brutes.len()).filter(|&i| brutes[i].is_some()).collect();
    idx.sort_by(|&a, &b| brutes[a].unwrap().total_cmp(&brutes[b].unwrap()));
    let n = idx.len();
    let mut sortie = vec![None; brutes.len()];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && brutes[idx[j + 1]] == brutes[idx[i]] {
            j += 1;
        }
        let r = if n > 1 {
            ((i + j) as f32 / 2.0) / (n - 1) as f32
        } else {
            0.5
        };
        for &k in &idx[i..=j] {
            sortie[k] = Some(r);
        }
        i = j + 1;
    }
    sortie
}

/// Une courbe de niveau : un anneau fermé, `[lon, lat]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Courbe {
    /// Son rang dans [`Relief::courbes`] : l'identifiant `ligne` des tuiles.
    pub id: u32,
    /// De 1 à 9 : l'altitude `niveau / 10`.
    pub niveau: u8,
    /// `true` : l'anneau enferme un sommet (le dedans est au-dessus de son
    /// niveau) ; `false` : une cuvette.
    pub sommet: bool,
    pub points: Vec<[f64; 2]>,
}

/// Le champ d'altitude, sur une grille géographique (rangée 0 au sud).
#[derive(Clone, Debug)]
pub struct Relief {
    ouest: f64,
    sud: f64,
    pas_lon: f64,
    pas_lat: f64,
    nx: usize,
    ny: usize,
    /// L'altitude de chaque cellule, dans `[0, 1]`.
    h: Vec<f32>,
}

/// Flou gaussien séparable, sur place, noyau de pic 1.
fn flouter(v: &mut [f64], nx: usize, ny: usize, sigma: f64) {
    let r = (3.0 * sigma).ceil() as isize;
    let noyau: Vec<f64> = (-r..=r)
        .map(|d| (-(d * d) as f64 / (2.0 * sigma * sigma)).exp())
        .collect();
    let mut tampon = vec![0.0; v.len()];
    for y in 0..ny {
        for x in 0..nx {
            let mut s = 0.0;
            for (k, w) in noyau.iter().enumerate() {
                let xx = x as isize + k as isize - r;
                if xx >= 0 && (xx as usize) < nx {
                    s += w * v[y * nx + xx as usize];
                }
            }
            tampon[y * nx + x] = s;
        }
    }
    for y in 0..ny {
        for x in 0..nx {
            let mut s = 0.0;
            for (k, w) in noyau.iter().enumerate() {
                let yy = y as isize + k as isize - r;
                if yy >= 0 && (yy as usize) < ny {
                    s += w * tampon[yy as usize * nx + x];
                }
            }
            v[y * nx + x] = s;
        }
    }
}

fn centile(trie: &[f64], q: f64) -> f64 {
    trie[((trie.len() - 1) as f64 * q).round() as usize]
}

impl Relief {
    /// Le champ des morceaux `points` (`[lon, lat]`, valeur dans `[0, 1]`).
    /// `None` sans morceau.
    pub fn calculer(points: &[([f64; 2], f32)]) -> Option<Relief> {
        if points.is_empty() {
            return None;
        }
        let (mut o, mut s, mut e, mut n) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (p, _) in points {
            o = o.min(p[0]);
            e = e.max(p[0]);
            s = s.min(p[1]);
            n = n.max(p[1]);
        }
        let lat0 = (s + n) / 2.0;
        let pas_lat = PAS_M / 110_540.0;
        let pas_lon = PAS_M / (111_320.0 * lat0.to_radians().cos());
        // Trois écarts-types de marge : la décroissance a la place de finir.
        let marge = 3.0 * SIGMA_M / PAS_M;
        let ouest = o - marge * pas_lon;
        let sud = s - marge * pas_lat;
        let nx = (((e - o) / pas_lon) + 2.0 * marge).ceil() as usize + 1;
        let ny = (((n - s) / pas_lat) + 2.0 * marge).ceil() as usize + 1;

        let mut somme = vec![0.0f64; nx * ny];
        let mut poids = vec![0.0f64; nx * ny];
        for (p, v) in points {
            let x = (((p[0] - ouest) / pas_lon).round() as usize).min(nx - 1);
            let y = (((p[1] - sud) / pas_lat).round() as usize).min(ny - 1);
            somme[y * nx + x] += *v as f64;
            poids[y * nx + x] += 1.0;
        }
        let sigma = SIGMA_M / PAS_M;
        flouter(&mut somme, nx, ny, sigma);
        flouter(&mut poids, nx, ny, sigma);

        let moyenne: Vec<f64> = somme
            .iter()
            .zip(&poids)
            .map(|(s, w)| if *w > 1e-9 { s / w } else { 0.0 })
            .collect();
        let mut peuple: Vec<f64> = moyenne
            .iter()
            .zip(&poids)
            .filter(|(_, w)| **w >= 0.5)
            .map(|(m, _)| *m)
            .collect();
        if peuple.is_empty() {
            peuple = moyenne.clone();
        }
        peuple.sort_by(f64::total_cmp);
        let (mut bas, mut haut) = (centile(&peuple, 0.02), centile(&peuple, 0.98));
        if haut - bas < 1e-6 {
            bas = peuple[0];
            haut = peuple[peuple.len() - 1].max(bas + 1e-6);
        }
        let h = moyenne
            .iter()
            .zip(&poids)
            .map(|(m, w)| {
                let etire = ((m - bas) / (haut - bas)).clamp(0.0, 1.0);
                (etire * (1.0 - (-w / DENSITE_PLEINE).exp())) as f32
            })
            .collect();
        Some(Relief {
            ouest,
            sud,
            pas_lon,
            pas_lat,
            nx,
            ny,
            h,
        })
    }

    /// L'altitude sous `[lon, lat]`, dans `[0, 1]` ; 0 hors de la grille.
    pub fn altitude(&self, p: [f64; 2]) -> f32 {
        let fx = (p[0] - self.ouest) / self.pas_lon;
        let fy = (p[1] - self.sud) / self.pas_lat;
        if fx < 0.0 || fy < 0.0 || fx > (self.nx - 1) as f64 || fy > (self.ny - 1) as f64 {
            return 0.0;
        }
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.nx - 1), (y0 + 1).min(self.ny - 1));
        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let v = |x: usize, y: usize| self.h[y * self.nx + x];
        let bas = v(x0, y0) * (1.0 - tx) + v(x1, y0) * tx;
        let haut = v(x0, y1) * (1.0 - tx) + v(x1, y1) * tx;
        bas * (1.0 - ty) + haut * ty
    }

    /// La bande d'altitude d'un point, de 0 à 9.
    pub fn bande(&self, p: [f64; 2]) -> u8 {
        ((self.altitude(p) * NIVEAUX as f32) as u8).min(NIVEAUX - 1)
    }

    /// Les courbes de niveau 1 à 9, numérotées dans l'ordre.
    pub fn courbes(&self) -> Vec<Courbe> {
        let valeurs: Vec<f64> = self.h.iter().map(|&v| v as f64).collect();
        let seuils: Vec<f64> = (1..NIVEAUX).map(|k| k as f64 / NIVEAUX as f64).collect();
        // Une valeur de la grille vaut pour le centre de sa cellule.
        let constructeur = contour::ContourBuilder::new(self.nx, self.ny, true)
            .x_origin(self.ouest - 0.5 * self.pas_lon)
            .y_origin(self.sud - 0.5 * self.pas_lat)
            .x_step(self.pas_lon)
            .y_step(self.pas_lat);
        let Ok(contours) = constructeur.contours(&valeurs, &seuils) else {
            return Vec::new();
        };
        let mut courbes = Vec::new();
        for (k, c) in contours.iter().enumerate() {
            let niveau = (k + 1) as u8;
            for poly in &c.geometry().0 {
                let anneaux = std::iter::once((poly.exterior(), true))
                    .chain(poly.interiors().iter().map(|a| (a, false)));
                for (anneau, sommet) in anneaux {
                    let points: Vec<[f64; 2]> = anneau.coords().map(|c| [c.x, c.y]).collect();
                    if points.len() < 4 {
                        continue;
                    }
                    courbes.push(Courbe {
                        id: courbes.len() as u32,
                        niveau,
                        sommet,
                        points,
                    });
                }
            }
        }
        courbes
    }

    /// Les bornes `[ouest, sud, est, nord]` de la grille.
    pub fn bornes(&self) -> [f64; 4] {
        [
            self.ouest,
            self.sud,
            self.ouest + (self.nx - 1) as f64 * self.pas_lon,
            self.sud + (self.ny - 1) as f64 * self.pas_lat,
        ]
    }

    /// Les tuiles qui couvrent la grille au zoom `z`, en ordre de Hilbert.
    fn tuiles(&self, z: u8) -> Vec<(u32, u32)> {
        let [o, s, e, n] = self.bornes();
        let no = projection::geo_vers_monde(Geo { lon: o, lat: n });
        let se = projection::geo_vers_monde(Geo { lon: e, lat: s });
        let k = (1u32 << z) as f64;
        let mut t: Vec<((u32, u32), u64)> = Vec::new();
        for x in (no.u * k).floor() as u32..=(se.u * k).floor() as u32 {
            for y in (no.v * k).floor() as u32..=(se.v * k).floor() as u32 {
                if let Ok(c) = TileCoord::new(z, x, y) {
                    t.push(((x, y), u64::from(TileId::from(c))));
                }
            }
        }
        t.sort_unstable_by_key(|(_, id)| *id);
        t.into_iter().map(|(c, _)| c).collect()
    }

    /// Une tuile `raster-dem` en encodage Terrarium :
    /// `altitude = R·256 + G + B/256 − 32768`.
    pub fn tuile_dem(&self, z: u8, x: u32, y: u32) -> anyhow::Result<Vec<u8>> {
        let k = (1u32 << z) as f64;
        let c = COTE_DEM as usize;
        let mut pixels = vec![0u8; c * c * 3];
        for py in 0..c {
            for px in 0..c {
                let g = projection::monde_vers_geo(Monde {
                    u: (x as f64 + (px as f64 + 0.5) / c as f64) / k,
                    v: (y as f64 + (py as f64 + 0.5) / c as f64) / k,
                });
                let m = self.altitude([g.lon, g.lat]) as f64 * ALTITUDE_MAX_M + 32768.0;
                let i = (py * c + px) * 3;
                pixels[i] = (m / 256.0).floor() as u8;
                pixels[i + 1] = (m.floor() as u32 % 256) as u8;
                pixels[i + 2] = ((m - m.floor()) * 256.0) as u8;
            }
        }
        let mut sortie = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut sortie, COTE_DEM, COTE_DEM);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            enc.write_header()?.write_image_data(&pixels)?;
        }
        Ok(sortie)
    }

    /// L'archive `raster-dem` : rend le nombre de tuiles.
    pub fn ecrire_dem(&self, chemin: &Path) -> anyhow::Result<usize> {
        let [o, s, e, n] = self.bornes();
        let mut ecrivain = PmTilesWriter::new(TileType::Png)
            .min_zoom(ZOOM_MIN)
            .max_zoom(ZOOM_DEM_MAX)
            .tile_compression(Compression::None)
            .bounds(o, s, e, n)
            .center((o + e) / 2.0, (s + n) / 2.0)
            .center_zoom(12)
            .create(BufWriter::new(std::fs::File::create(chemin)?))?;
        let mut compte = 0;
        for z in ZOOM_MIN..=ZOOM_DEM_MAX {
            for (x, y) in self.tuiles(z) {
                ecrivain.add_tile(TileCoord::new(z, x, y)?, &self.tuile_dem(z, x, y)?)?;
                compte += 1;
            }
        }
        ecrivain.finalize()?;
        Ok(compte)
    }

    /// L'archive des courbes, couche `courbes` : `ligne` (l'identifiant de la
    /// courbe), `niveau`, `sommet`. Rend le nombre de tuiles.
    pub fn ecrire_courbes(&self, courbes: &[Courbe], chemin: &Path) -> anyhow::Result<usize> {
        let [o, s, e, n] = self.bornes();
        let mut ecrivain = PmTilesWriter::new(TileType::Mvt)
            .min_zoom(ZOOM_MIN)
            .max_zoom(ZOOM_COURBES_MAX)
            .tile_compression(Compression::Gzip)
            .bounds(o, s, e, n)
            .center((o + e) / 2.0, (s + n) / 2.0)
            .center_zoom(12)
            .create(BufWriter::new(std::fs::File::create(chemin)?))?;
        // Les courbes en carré-monde, une fois.
        let monde: Vec<Vec<(f64, f64)>> = courbes
            .iter()
            .map(|c| {
                c.points
                    .iter()
                    .map(|p| {
                        let m = projection::geo_vers_monde(Geo {
                            lon: p[0],
                            lat: p[1],
                        });
                        (m.u, m.v)
                    })
                    .collect()
            })
            .collect();
        let mut compte = 0;
        for z in ZOOM_MIN..=ZOOM_COURBES_MAX {
            for (x, y) in self.tuiles(z) {
                if let Some(octets) = tuile_courbes(courbes, &monde, z, x, y)? {
                    ecrivain.add_tile(TileCoord::new(z, x, y)?, &octets)?;
                    compte += 1;
                }
            }
        }
        ecrivain.finalize()?;
        Ok(compte)
    }
}

/// Les morceaux de ligne d'une courbe qui tombent dans la tuile (marge
/// comprise), en unités MVT : chaque passage dans la tuile, avec un point de
/// part et d'autre pour que le trait franchisse le bord.
fn troncons_dans(local: &[(f64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let dedans = |&(px, py): &(f64, f64)| {
        let (a, b) = (-MARGE_MVT, ETENDUE_TUILE as f64 + MARGE_MVT);
        px >= a && px <= b && py >= a && py <= b
    };
    let mut sortie = Vec::new();
    let mut courant: Vec<(f64, f64)> = Vec::new();
    for (i, p) in local.iter().enumerate() {
        if dedans(p) {
            if courant.is_empty() && i > 0 {
                courant.push(local[i - 1]);
            }
            courant.push(*p);
        } else if !courant.is_empty() {
            courant.push(*p);
            sortie.push(std::mem::take(&mut courant));
        }
    }
    if courant.len() >= 2 {
        sortie.push(courant);
    }
    sortie
}

fn tuile_courbes(
    courbes: &[Courbe],
    monde: &[Vec<(f64, f64)>],
    z: u8,
    x: u32,
    y: u32,
) -> anyhow::Result<Option<Vec<u8>>> {
    use mvt::{GeomEncoder, GeomType, Tile};
    let tuile = Tile::new(ETENDUE_TUILE as u32);
    let mut couche = tuile.create_layer("courbes");
    let mut compte = 0u64;
    for (c, points) in courbes.iter().zip(monde) {
        let local: Vec<(f64, f64)> = points
            .iter()
            .map(|&(u, v)| projection::monde_vers_tuile(Monde { u, v }, z, x, y))
            .collect();
        for morceau in troncons_dans(&local) {
            let mut geom = GeomEncoder::new(GeomType::Linestring);
            for (px, py) in morceau {
                geom = geom.point(px, py)?;
            }
            let mut entite = couche.into_feature(geom.encode()?);
            compte += 1;
            entite.set_id(compte);
            entite.add_tag_int("ligne", c.id as i64);
            entite.add_tag_int("niveau", c.niveau as i64);
            entite.add_tag_bool("sommet", c.sommet);
            couche = entite.into_layer();
        }
    }
    if compte == 0 {
        return Ok(None);
    }
    let mut tuile = tuile;
    tuile.add_layer(couche)?;
    Ok(Some(tuile.to_bytes()?))
}

/// La distance en mètres d'un point à un segment, et la position de sa
/// projection le long du segment (`[0, 1]`). Plan local, assez juste à
/// l'échelle d'une rue.
fn projeter(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let kx = 111_320.0 * p[1].to_radians().cos();
    let ky = 110_540.0;
    let (ax, ay) = ((a[0] - p[0]) * kx, (a[1] - p[1]) * ky);
    let (bx, by) = ((b[0] - p[0]) * kx, (b[1] - p[1]) * ky);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (-(ax * dx + ay * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((ax + t * dx).hypot(ay + t * dy), t)
}

/// Les morceaux qui bordent la courbe, à moins de `rayon_m` mètres, dans
/// l'ordre où on les rencontre en la suivant : `(id, position le long de la
/// courbe en mètres, point de la courbe le plus proche)`. Le rail de la
/// molette (le fuseau de Qoyra).
pub fn le_long(
    courbe: &Courbe,
    morceaux: &[(i64, [f64; 2])],
    rayon_m: f64,
) -> Vec<(i64, f64, [f64; 2])> {
    let pts = &courbe.points;
    // Longueur cumulée au début de chaque segment.
    let mut cumul = vec![0.0f64; pts.len()];
    for i in 1..pts.len() {
        let (d, _) = projeter(pts[i], pts[i - 1], pts[i - 1]);
        cumul[i] = cumul[i - 1] + d;
    }
    let [mut o, mut s, mut e, mut n] = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in pts {
        o = o.min(p[0]);
        e = e.max(p[0]);
        s = s.min(p[1]);
        n = n.max(p[1]);
    }
    let marge_lat = rayon_m / 110_540.0;
    let marge_lon = rayon_m / (111_320.0 * s.to_radians().cos());
    let mut pris: Vec<(i64, f64, [f64; 2])> = morceaux
        .iter()
        .filter(|(_, p)| {
            p[0] >= o - marge_lon
                && p[0] <= e + marge_lon
                && p[1] >= s - marge_lat
                && p[1] <= n + marge_lat
        })
        .filter_map(|&(id, p)| {
            let mut meilleur: Option<(f64, f64, [f64; 2])> = None;
            for i in 1..pts.len() {
                let (d, t) = projeter(p, pts[i - 1], pts[i]);
                if meilleur.is_none_or(|m| d < m.0) {
                    let q = [
                        pts[i - 1][0] + t * (pts[i][0] - pts[i - 1][0]),
                        pts[i - 1][1] + t * (pts[i][1] - pts[i - 1][1]),
                    ];
                    meilleur = Some((d, cumul[i - 1] + t * (cumul[i] - cumul[i - 1]), q));
                }
            }
            let (d, le_long, q) = meilleur?;
            (d <= rayon_m).then_some((id, le_long, q))
        })
        .collect();
    pris.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    pris
}

/// Un point est-il dans l'anneau ? Règle pair-impair.
pub fn dans_anneau(anneau: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut dedans = false;
    let mut j = anneau.len() - 1;
    for i in 0..anneau.len() {
        let (a, b) = (anneau[i], anneau[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            dedans = !dedans;
        }
        j = i;
    }
    dedans
}

/// « Ouvrir une courbe » : les morceaux que la courbe enferme et qui sont dans
/// la bande qu'elle borde de l'intérieur — au-dessus d'elle pour un sommet
/// (bande `niveau`), au-dessous pour une cuvette (bande `niveau − 1`).
pub fn ouvrir(relief: &Relief, courbe: &Courbe, morceaux: &[(i64, [f64; 2])]) -> Vec<i64> {
    let bande = if courbe.sommet {
        courbe.niveau
    } else {
        courbe.niveau - 1
    };
    morceaux
        .iter()
        .filter(|(_, p)| dans_anneau(&courbe.points, *p) && relief.bande(*p) == bande)
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une colline : des morceaux hauts autour de `centre`, bas ailleurs, sur
    /// un carré de 2 km.
    fn colline() -> (Vec<([f64; 2], f32)>, [f64; 2]) {
        let centre = [2.35, 48.85];
        let mut pts = Vec::new();
        for i in 0..40 {
            for j in 0..40 {
                let p = [2.336 + i as f64 * 0.0007, 48.841 + j as f64 * 0.00045];
                let d = ((p[0] - centre[0]) * 73_000.0).hypot((p[1] - centre[1]) * 111_000.0);
                pts.push((p, (1.0 - d / 1200.0).clamp(0.0, 1.0) as f32));
            }
        }
        (pts, centre)
    }

    #[test]
    fn les_rangs_vont_de_zero_a_un_et_partagent_les_ex_aequo() {
        let r = rangs(&[Some(3.0), None, Some(1.0), Some(3.0), Some(2.0)]);
        assert_eq!(
            r,
            vec![
                Some(5.0 / 6.0),
                None,
                Some(0.0),
                Some(5.0 / 6.0),
                Some(1.0 / 3.0)
            ]
        );
    }

    #[test]
    fn la_familiarite_garde_le_jamais_ecoute_en_bas() {
        let n = normaliser(Grandeur::Familiarite, &[None, Some(0.0), Some(3.0)]);
        assert_eq!(n[0], Some(0.0));
        assert_eq!(n[1], Some(0.0));
        assert_eq!(n[2], Some(1.0));
    }

    #[test]
    fn la_colline_culmine_au_centre_et_retombe_hors_de_la_ville() {
        let (pts, centre) = colline();
        let r = Relief::calculer(&pts).unwrap();
        assert!(r.altitude(centre) > 0.9, "sommet {}", r.altitude(centre));
        assert!(r.altitude([2.337, 48.842]) < 0.3);
        // Trois écarts-types au-delà du dernier morceau : la plaine.
        assert!(r.altitude([2.336 - 0.006, 48.85]) < 0.05);
    }

    #[test]
    fn les_courbes_entourent_le_sommet_et_sont_numerotees() {
        let (pts, centre) = colline();
        let r = Relief::calculer(&pts).unwrap();
        let courbes = r.courbes();
        for k in 1..NIVEAUX {
            assert!(courbes.iter().any(|c| c.niveau == k), "niveau {k} absent");
        }
        let haute = courbes
            .iter()
            .filter(|c| c.sommet)
            .max_by_key(|c| c.niveau)
            .unwrap();
        assert!(dans_anneau(&haute.points, centre));
        assert!(courbes.iter().enumerate().all(|(i, c)| c.id == i as u32));
    }

    #[test]
    fn ouvrir_une_courbe_rend_sa_bande_dans_ce_quelle_enferme() {
        let (pts, centre) = colline();
        let r = Relief::calculer(&pts).unwrap();
        let courbes = r.courbes();
        let c5 = courbes.iter().find(|c| c.niveau == 5 && c.sommet).unwrap();
        let morceaux: Vec<(i64, [f64; 2])> = pts
            .iter()
            .enumerate()
            .map(|(i, (p, _))| (i as i64, *p))
            .collect();
        let ouverts = ouvrir(&r, c5, &morceaux);
        assert!(!ouverts.is_empty());
        for id in &ouverts {
            let p = morceaux[*id as usize].1;
            assert!(dans_anneau(&c5.points, p));
            assert_eq!(r.bande(p), 5);
        }
        // Le sommet est au-dessus de la bande 5.
        assert!(r.bande(centre) > 5);
    }

    #[test]
    fn le_long_dune_courbe_suit_son_sens() {
        let c = Courbe {
            id: 0,
            niveau: 3,
            sommet: true,
            points: vec![
                [2.30, 48.85],
                [2.31, 48.85],
                [2.31, 48.86],
                [2.30, 48.86],
                [2.30, 48.85],
            ],
        };
        let m = vec![
            (1, [2.3095, 48.8501]),
            (2, [2.302, 48.8499]),
            (3, [2.305, 48.855]),
        ];
        let l = le_long(&c, &m, 40.0);
        assert_eq!(l.iter().map(|x| x.0).collect::<Vec<_>>(), vec![2, 1]);
        assert!(l[0].1 < l[1].1);
    }

    #[test]
    fn une_tuile_dem_se_decode_en_metres() {
        let (pts, centre) = colline();
        let r = Relief::calculer(&pts).unwrap();
        let m = projection::geo_vers_monde(Geo {
            lon: centre[0],
            lat: centre[1],
        });
        let z = 14u8;
        let k = (1u32 << z) as f64;
        let (x, y) = ((m.u * k) as u32, (m.v * k) as u32);
        let png = r.tuile_dem(z, x, y).unwrap();
        let mut lecture = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut t = vec![0; lecture.output_buffer_size().unwrap()];
        lecture.next_frame(&mut t).unwrap();
        let (px, py) = (
            ((m.u * k - x as f64) * 256.0) as usize,
            ((m.v * k - y as f64) * 256.0) as usize,
        );
        let i = (py * 256 + px) * 3;
        let metres = t[i] as f64 * 256.0 + t[i + 1] as f64 + t[i + 2] as f64 / 256.0 - 32768.0;
        let attendu = r.altitude(centre) as f64 * ALTITUDE_MAX_M;
        assert!(
            (metres - attendu).abs() < 1.5,
            "{metres} m contre {attendu} m"
        );
    }

    #[test]
    fn les_archives_secrivent() {
        let (pts, _) = colline();
        let r = Relief::calculer(&pts).unwrap();
        let dossier = std::env::temp_dir().join(format!("relief-musical-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        assert!(r.ecrire_dem(&dossier.join("dem.pmtiles")).unwrap() > 0);
        assert!(
            r.ecrire_courbes(&r.courbes(), &dossier.join("courbes.pmtiles"))
                .unwrap()
                > 0
        );
        std::fs::remove_dir_all(&dossier).ok();
    }
}
