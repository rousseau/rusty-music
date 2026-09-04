// SPDX-License-Identifier: GPL-3.0-or-later
//! Ombrage du relief, en tuiles raster, sous les territoires.
//!
//! Le champ de densité tient lieu d'altitude : là où les morceaux
//! s'accumulent, la terre se soulève. `core::density::champ_global` rend ce
//! champ continu — surtout pas les bandes, dont les paliers apparaîtraient
//! comme des falaises.
//!
//! **Pourquoi calculer l'ombrage ici plutôt que le laisser à MapLibre.**
//! MapLibre sait ombrer un modèle numérique de terrain (`raster-dem` +
//! `hillshade`), et ç'aurait été la voie évidente. Deux raisons de ne pas la
//! prendre :
//!
//! - son calcul de pente part de la taille réelle d'un pixel **en mètres**,
//!   déduite du zoom et de la latitude. Notre monde n'a pas de mètres : le
//!   carré de la carte fait 40 000 km de large parce que c'est ce que vaut un
//!   planisphère, et une altitude vraisemblable y serait rigoureusement plate.
//!   Calibrer reviendrait à tâtonner sur une grandeur qui ne veut rien dire ;
//! - ce même calcul dépend du zoom, donc **le relief changerait d'aspect en
//!   zoomant**. Sur une vraie Terre c'est correct ; sur une carte inventée, on
//!   veut que la montagne reste la même montagne, seulement plus grande.
//!
//! L'ombrage de Horn (1981) tient en trente lignes et rend un résultat
//! déterministe, réglable, et constant d'un zoom à l'autre.

use std::io::BufWriter;
use std::path::Path;
use std::time::{Duration, Instant};

use pmtiles::{Compression, PmTilesWriter, TileCoord, TileId, TileType};

/// Côté d'une tuile raster, en pixels.
const COTE: usize = 512;

/// Noyau de densité propre au relief, distinct de celui des territoires.
///
/// `core::density` retient 0,02, réglé pour que les contours de territoire
/// épousent le nuage. Ombré, ce même champ ressemble à du papier froissé : la
/// nappe y porte tout le détail des 27 000 morceaux. 0,05 rend des massifs et
/// des vallées lisibles sans noyer la carte — 0,08 débordait jusqu'aux bords
/// et faisait perdre la forme d'île. Vérifié à l'œil (`example apercu_relief`),
/// pas déduit.
pub const NOYAU: f64 = 0.05;

/// Réglages de l'ombrage.
#[derive(Debug, Clone, Copy)]
pub struct Ombrage {
    /// Zoom maximal produit. Le champ fait 1024 cellules de côté : au-delà de
    /// 2, chaque tuile n'agrandit plus que du flou. On monte quand même à 3
    /// pour que la côte reste franche à l'approche.
    pub zoom_max: u8,
    /// Azimut de la lumière, en degrés (0 = nord, 315 = nord-ouest, la
    /// convention cartographique — l'œil lit mal un relief éclairé d'ailleurs).
    pub azimut: f64,
    /// Hauteur de la lumière au-dessus de l'horizon, en degrés.
    pub elevation: f64,
    /// Exagération du relief, appliquée à une pente mesurée en unités de
    /// monde. Le champ est normalisé dans `[0, 1]` et le monde fait 2 unités
    /// de large : une nappe qui monte de 0 à 1 sur un tiers de carte a déjà une
    /// pente de 3. Au-delà de 1, tout sature à 90° et il ne reste que du noir
    /// et du blanc — mesuré, pas supposé.
    pub exageration: f64,
    /// Courbe appliquée à l'altitude. Sous 1, elle élargit les plaines et
    /// arrondit les épaules — une densité gaussienne est autrement trop
    /// pointue pour ressembler à un paysage.
    pub gamma: f64,
    /// Opacité maximale de l'ombre et de la lumière.
    pub force: f64,
}

impl Default for Ombrage {
    fn default() -> Self {
        Self {
            zoom_max: 3,
            azimut: 315.0,
            elevation: 45.0,
            exageration: 0.20,
            gamma: 0.55,
            force: 0.55,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rapport {
    pub tuiles: usize,
    pub octets: u64,
    pub duree: Duration,
}

/// Écrit l'archive PMTiles des tuiles d'ombrage.
///
/// `champ` vient de `core::density::champ_global` : une grille carrée de côté
/// `gn`, normalisée dans `[0, 1]`, couvrant le même domaine que les tuiles
/// vectorielles.
pub fn ecrire(champ: &[f64], gn: usize, o: &Ombrage, chemin: &Path) -> anyhow::Result<Rapport> {
    ecrire_avec(champ, gn, o, chemin, None)
}

/// Idem, en déposant au passage les tuiles en clair sous `repertoire`.
pub fn ecrire_avec(
    champ: &[f64],
    gn: usize,
    o: &Ombrage,
    chemin: &Path,
    repertoire: Option<&Path>,
) -> anyhow::Result<Rapport> {
    let depart = Instant::now();
    anyhow::ensure!(
        champ.len() == gn * gn,
        "champ de {} valeurs pour une grille {gn}×{gn}",
        champ.len()
    );

    let fichier = BufWriter::new(std::fs::File::create(chemin)?);
    let mut ecrivain = PmTilesWriter::new(TileType::Png)
        .min_zoom(0)
        .max_zoom(o.zoom_max)
        // Un PNG est déjà compressé : le repasser en gzip ne gagne rien et
        // coûte un décodage de plus à l'affichage.
        .tile_compression(Compression::None)
        .bounds(-180.0, -85.051_128_78, 180.0, 85.051_128_78)
        .center(0.0, 0.0)
        .center_zoom(2)
        .create(fichier)?;

    let mut tuiles = 0usize;
    for z in 0..=o.zoom_max {
        let n = 1u32 << z;
        let mut clefs: Vec<((u32, u32), u64)> = Vec::new();
        for x in 0..n {
            for y in 0..n {
                let id: TileId = TileCoord::new(z, x, y)?.into();
                clefs.push(((x, y), u64::from(id)));
            }
        }
        clefs.sort_unstable_by_key(|(_, id)| *id);
        for ((x, y), _) in clefs {
            let png = tuile(champ, gn, o, z, x, y)?;
            if let Some(r) = repertoire {
                let d = r.join(z.to_string()).join(x.to_string());
                std::fs::create_dir_all(&d)?;
                std::fs::write(d.join(format!("{y}.png")), &png)?;
            }
            ecrivain.add_tile(TileCoord::new(z, x, y)?, &png)?;
            tuiles += 1;
        }
    }
    ecrivain.finalize()?;

    Ok(Rapport {
        tuiles,
        octets: std::fs::metadata(chemin)?.len(),
        duree: depart.elapsed(),
    })
}

/// Échantillonne le champ en coordonnées monde, par interpolation bilinéaire.
///
/// L'axe des ordonnées se retourne : le champ est indexé dans le repère de la
/// carte, où `y` monte, alors que `v` descend.
fn echantillon(champ: &[f64], gn: usize, u: f64, v: f64) -> f64 {
    let gx = (u * gn as f64 - 0.5).clamp(0.0, gn as f64 - 1.0);
    let gy = ((1.0 - v) * gn as f64 - 0.5).clamp(0.0, gn as f64 - 1.0);
    let (x0, y0) = (gx.floor() as usize, gy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(gn - 1), (y0 + 1).min(gn - 1));
    let (fx, fy) = (gx - x0 as f64, gy - y0 as f64);
    let h = |x: usize, y: usize| champ[y * gn + x];
    let haut = h(x0, y0) * (1.0 - fx) + h(x1, y0) * fx;
    let bas = h(x0, y1) * (1.0 - fx) + h(x1, y1) * fx;
    haut * (1.0 - fy) + bas * fy
}

/// Fait épouser à une route la ligne de crête de densité entre ses deux bouts.
///
/// `carto-google-maps.md` : « faire suivre aux autoroutes la ligne de crête de
/// densité — le réseau épouse alors le relief, comme une vraie carte
/// routière ». Sans cela, les routes rayonnent en étoile depuis chaque ville :
/// des segments droits entre des points, pas des routes.
///
/// Pour chaque point intermédiaire, on cherche le décalage perpendiculaire qui
/// passe par le sol le plus haut, dans une fenêtre bornée par `amplitude` ;
/// puis on adoucit la ligne brisée qui en résulte, sans quoi elle zigzaguerait
/// d'un échantillon à l'autre.
pub fn epouser_le_relief(
    a: [f32; 2],
    b: [f32; 2],
    champ: &[f64],
    gn: usize,
    amplitude: f32,
) -> Vec<[f32; 2]> {
    const SEGMENTS: usize = 8;
    const ESSAIS: i32 = 6;

    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let longueur = (dx * dx + dy * dy).sqrt();
    if longueur < 1e-6 || amplitude <= 0.0 {
        return vec![a, b];
    }
    // Perpendiculaire unitaire.
    let (nx, ny) = (-dy / longueur, dx / longueur);
    // Une route courte ne fait pas de détour : l'amplitude suit la longueur.
    let amp = amplitude.min(longueur * 0.35);

    let mut points = Vec::with_capacity(SEGMENTS + 1);
    points.push(a);
    for i in 1..SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        // Le détour s'annule aux extrémités : une route arrive dans la ville,
        // elle ne la contourne pas.
        let enveloppe = (t * std::f32::consts::PI).sin();
        let (bx, by) = (a[0] + dx * t, a[1] + dy * t);
        let mut meilleur = (0.0f32, f64::MIN);
        for e in -ESSAIS..=ESSAIS {
            let d = amp * enveloppe * (e as f32 / ESSAIS as f32);
            let densite = crate::densite_sous(champ, gn, bx + nx * d, by + ny * d);
            // À densité égale, le tracé le plus droit gagne : sinon deux
            // exécutions choisiraient des détours différents sur un plateau.
            if densite > meilleur.1 + 1e-9 {
                meilleur = (d, densite);
            }
        }
        points.push([bx + nx * meilleur.0, by + ny * meilleur.0]);
    }
    points.push(b);

    // Adoucir : la recherche échantillonne, elle ne lisse pas.
    for _ in 0..2 {
        let mut lisse = vec![points[0]];
        for f in points.windows(3) {
            lisse.push([
                (f[0][0] + 2.0 * f[1][0] + f[2][0]) / 4.0,
                (f[0][1] + 2.0 * f[1][1] + f[2][1]) / 4.0,
            ]);
        }
        lisse.push(*points.last().expect("au moins deux points"));
        points = lisse;
    }
    points
}

/// Une tuile d'ombrage isolée, en PNG — pour juger un réglage sans écrire
/// l'archive entière ni la relire.
pub fn apercu(
    champ: &[f64],
    gn: usize,
    o: &Ombrage,
    z: u8,
    tx: u32,
    ty: u32,
) -> anyhow::Result<Vec<u8>> {
    tuile(champ, gn, o, z, tx, ty)
}

/// Une tuile d'ombrage, en RGBA.
///
/// La couche est une **surimpression** : ombre bleutée dans les creux, lumière
/// chaude sur les versants exposés, transparente sur le plat. Posée sous les
/// territoires, elle les sculpte sans les teinter.
fn tuile(
    champ: &[f64],
    gn: usize,
    o: &Ombrage,
    z: u8,
    tx: u32,
    ty: u32,
) -> anyhow::Result<Vec<u8>> {
    let n = (1u64 << z) as f64;
    let pas = 1.0 / n / COTE as f64;

    let azimut = (360.0 - o.azimut + 90.0).to_radians();
    let zenith = (90.0 - o.elevation).to_radians();

    // L'altitude, courbée puis exagérée. Le pas horizontal est constant en
    // unités de monde : c'est ce qui rend l'ombrage identique à tous les zooms.
    let altitude = |u: f64, v: f64| echantillon(champ, gn, u, v).max(0.0).powf(o.gamma);

    let mut pixels = vec![0u8; COTE * COTE * 4];
    for py in 0..COTE {
        for px in 0..COTE {
            let u = (tx as f64 + (px as f64 + 0.5) / COTE as f64) / n;
            let v = (ty as f64 + (py as f64 + 0.5) / COTE as f64) / n;

            // Horn : dérivées sur un voisinage 3×3, pondérées.
            let a = altitude(u - pas, v - pas);
            let b = altitude(u, v - pas);
            let c = altitude(u + pas, v - pas);
            let d = altitude(u - pas, v);
            let f = altitude(u + pas, v);
            let g = altitude(u - pas, v + pas);
            let h = altitude(u, v + pas);
            let i = altitude(u + pas, v + pas);

            // Le pas de dérivation est l'écart **en unités de monde**, et
            // c'est lui qui rend l'ombrage indépendant du zoom : le champ ne
            // change pas, donc une pente mesurée en unités de monde ne change
            // pas non plus. Diviser par l'écart en fraction de tuile
            // (`pas * n`, constant) semblait plus direct et aplatissait le
            // relief à chaque niveau — l'écart de hauteur, lui, se réduit de
            // moitié quand on descend d'un zoom.
            let ech = o.exageration / (8.0 * pas);
            let dzdx = ((c + 2.0 * f + i) - (a + 2.0 * d + g)) * ech;
            // Pas d'inversion à faire ici : la formule de Horn suppose déjà
            // des ordonnées qui descendent — la convention des lignes d'une
            // grille — et c'est exactement celle de `v`. « Corriger » ce signe
            // éclairait le sud-est, ce qu'a attrapé
            // `la_lumiere_vient_bien_du_nord_ouest`.
            let dzdy = ((g + 2.0 * h + i) - (a + 2.0 * b + c)) * ech;

            let pente = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
            let aspect = dzdy.atan2(-dzdx);
            let eclat = zenith.cos() * pente.cos()
                + zenith.sin() * pente.sin() * (azimut - aspect).cos();
            let eclat = eclat.clamp(0.0, 1.0);

            // Le point neutre n'est pas 0,5 mais l'éclairement d'un sol
            // **plat**, `cos(zenith)` — 0,71 à 45°. Prendre 0,5 couvrait toute
            // la carte, mer comprise, d'un voile clair de 18 % : le test
            // `un_champ_plat_ne_produit_pas_dombre` est né de ce défaut.
            let plat = zenith.cos();
            let ecart = if eclat >= plat {
                (eclat - plat) / (1.0 - plat).max(1e-6)
            } else {
                (eclat - plat) / plat.max(1e-6)
            };
            let (r, v_, b_, alpha) = if ecart < 0.0 {
                (38u8, 48u8, 66u8, -ecart * o.force)
            } else {
                (255u8, 249u8, 232u8, ecart * o.force * 0.8)
            };
            let k = (py * COTE + px) * 4;
            pixels[k] = r;
            pixels[k + 1] = v_;
            pixels[k + 2] = b_;
            pixels[k + 3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }

    let mut sortie = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut sortie, COTE as u32, COTE as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut ecr = enc.write_header()?;
        ecr.write_image_data(&pixels)?;
    }
    Ok(sortie)
}

/// Pour le style : le champ n'est utile que si quelque chose s'y voit.
/// Rend la part de pixels dont l'ombrage n'est pas transparent, sur un
/// échantillonnage grossier — sert au diagnostic, pas au rendu.
pub fn couverture(champ: &[f64], gn: usize, o: &Ombrage) -> f64 {
    let mut vus = 0usize;
    let mut total = 0usize;
    let pas = 1.0 / 128.0;
    for i in 0..128 {
        for j in 0..128 {
            let (u, v) = ((i as f64 + 0.5) * pas, (j as f64 + 0.5) * pas);
            let a = echantillon(champ, gn, u, v).max(0.0).powf(o.gamma);
            total += 1;
            if a > 0.01 {
                vus += 1;
            }
        }
    }
    vus as f64 / total as f64
}

/// Les tuiles produites, indexées — sert aux tests, qui n'ont pas à relire une
/// archive pour vérifier un pixel.
#[cfg(test)]
fn tuiles_brutes(
    champ: &[f64],
    gn: usize,
    o: &Ombrage,
    z: u8,
) -> std::collections::HashMap<(u32, u32), Vec<u8>> {
    let n = 1u32 << z;
    let mut m = std::collections::HashMap::new();
    for x in 0..n {
        for y in 0..n {
            m.insert((x, y), tuile(champ, gn, o, z, x, y).unwrap());
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un cône centré : le versant nord-ouest doit être éclairé et le versant
    /// sud-est dans l'ombre, puisque la lumière vient du nord-ouest.
    fn cone(gn: usize) -> Vec<f64> {
        let mut champ = vec![0.0; gn * gn];
        let c = gn as f64 / 2.0;
        for y in 0..gn {
            for x in 0..gn {
                let d = (((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt()) / (gn as f64 / 3.0);
                champ[y * gn + x] = (1.0 - d).max(0.0);
            }
        }
        champ
    }

    /// Lit un pixel RGBA d'une tuile PNG.
    fn pixel(png: &[u8], x: usize, y: usize) -> [u8; 4] {
        let mut lecture = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut t = vec![0; lecture.output_buffer_size().unwrap()];
        let info = lecture.next_frame(&mut t).unwrap();
        let k = (y * info.width as usize + x) * 4;
        [t[k], t[k + 1], t[k + 2], t[k + 3]]
    }

    /// Le cône occupe le tiers central de la tuile : ses deux versants tombent
    /// donc autour de (171, 171) et (341, 341) sur 512 pixels. On lit ces deux
    /// points-là plutôt qu'une moyenne de quart — depuis que le plat ne peint
    /// plus rien, une moyenne de quart est surtout une moyenne de vide.
    #[test]
    fn la_lumiere_vient_bien_du_nord_ouest() {
        let gn = 256;
        let champ = cone(gn);
        let png = tuile(&champ, gn, &Ombrage::default(), 0, 0, 0).unwrap();

        let nord_ouest = pixel(&png, 171, 171);
        let sud_est = pixel(&png, 341, 341);

        assert!(
            nord_ouest[0] > 128 && nord_ouest[3] > 10,
            "le versant nord-ouest devrait être éclairé : {nord_ouest:?}"
        );
        assert!(
            sud_est[0] < 128 && sud_est[3] > 10,
            "le versant sud-est devrait être à l'ombre : {sud_est:?}"
        );
    }

    /// L'ombrage doit être le même d'un zoom à l'autre : c'est la raison même
    /// de ne pas s'en remettre au `hillshade` de MapLibre. On compare le
    /// centre de la tuile unique du zoom 0 au coin correspondant des quatre
    /// tuiles du zoom 1.
    #[test]
    fn lombrage_ne_depend_pas_du_zoom() {
        let gn = 256;
        let champ = cone(gn);
        let o = Ombrage::default();

        let z0 = tuiles_brutes(&champ, gn, &o, 0);
        let z1 = tuiles_brutes(&champ, gn, &o, 1);

        // Le point choisi est **sur un versant**, pas au sommet ni sur le
        // plat : c'est là seulement que la comparaison a un sens. La première
        // version de ce test lisait un point saturé en ombre pleine — les deux
        // zooms y rendaient 140, et le test passait alors que le relief
        // s'aplatissait bel et bien d'un niveau à l'autre.
        // Pixel (171, 171) au zoom 0 ↔ pixel (342, 342) de la tuile (0, 0) au
        // zoom 1 : même point du monde, u = v = 0,335.
        let a = pixel(&z0[&(0, 0)], 171, 171);
        let b = pixel(&z1[&(0, 0)], 342, 342);
        assert!(
            a[3] > 30 && a[3] < 225,
            "point de comparaison saturé, le test ne prouverait rien : {a:?}"
        );
        let ecart = (a[3] as i32 - b[3] as i32).abs();
        assert!(
            ecart <= 12,
            "l'ombrage change avec le zoom : alpha {} contre {}",
            a[3],
            b[3]
        );
        assert_eq!(a[0] < 128, b[0] < 128, "l'ombre et la lumière s'inversent");
    }

    /// Une route doit **contourner le creux** pour rester sur les hauteurs, et
    /// arriver quand même exactement dans les deux villes qu'elle relie.
    #[test]
    fn une_route_epouse_la_crete() {
        // Un champ où une crête horizontale passe au-dessus de la ligne droite.
        let gn = 128;
        let mut champ = vec![0.0f64; gn * gn];
        for y in 0..gn {
            for x in 0..gn {
                // Sommet le long de y = 0,35 (en coordonnées de carte).
                let cy = ((y as f64 + 0.5) / gn as f64) * 2.16 - 1.08;
                champ[y * gn + x] = (-((cy - 0.35).powi(2)) / 0.02).exp();
            }
        }
        let a = [-0.5f32, 0.0];
        let b = [0.5f32, 0.0];
        let trace = epouser_le_relief(a, b, &champ, gn, 0.35);

        assert!(trace.len() > 2, "le tracé devrait être infléchi");
        assert_eq!(trace[0], a, "la route doit partir de la ville");
        assert_eq!(*trace.last().unwrap(), b, "et y arriver");

        // Le milieu doit avoir grimpé vers la crête.
        let milieu = trace[trace.len() / 2];
        assert!(
            milieu[1] > 0.08,
            "le tracé reste dans le creux : y = {}",
            milieu[1]
        );
        // Et rester borné par l'amplitude demandée.
        assert!(trace.iter().all(|p| p[1].abs() <= 0.36), "détour hors bornes");
        // Déterministe.
        assert_eq!(trace, epouser_le_relief(a, b, &champ, gn, 0.35));
    }

    /// Un champ plat ne doit produire aucune ombre : sinon la carte serait
    /// couverte d'un voile là où il n'y a rien.
    #[test]
    fn un_champ_plat_ne_produit_pas_dombre() {
        let gn = 64;
        let champ = vec![0.0; gn * gn];
        let png = tuile(&champ, gn, &Ombrage::default(), 0, 0, 0).unwrap();
        let mut lecture = png::Decoder::new(std::io::Cursor::new(&png[..])).read_info().unwrap();
        let mut t = vec![0; lecture.output_buffer_size().unwrap()];
        lecture.next_frame(&mut t).unwrap();
        let max_alpha = t.chunks(4).map(|p| p[3]).max().unwrap();
        assert_eq!(max_alpha, 0, "du relief est apparu sur un terrain plat");
    }
}
