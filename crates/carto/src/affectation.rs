//! Étage 1 de l'affectation : familles musicales → quartiers de Paris.
//!
//! `docs/carto-ville.md` pose le principe (Procruste puis Voronoï sous
//! contrainte de capacité) ; ce module l'exécute. Les deux étages suivants
//! (artistes → rues, morceaux → adresses) ne sont pas ici — celui-ci ne fait
//! qu'une chose : découper la voirie en zones dont la taille suit celle des
//! familles.
//!
//! Aucune dépendance à Burn, Tauri ou SQLite : il reçoit des familles et des
//! rues déjà rassemblées, comme le reste du crate.

use std::collections::{HashMap, HashSet};

use rusty_music_osm::{Extrait, Troncon};

use crate::batiments::{Batiment, GrilleBatiments};

const RAYON_TERRE: f64 = 6_371_000.0;

/// Repère local, en mètres, centré sur la ville.
///
/// Travailler en degrés bruts fausserait toute la géométrie qui suit — un
/// degré de longitude ne vaut pas la même distance qu'un degré de latitude,
/// et l'écart est loin d'être négligeable à cette latitude (environ 34 %).
#[derive(Clone, Copy, Debug)]
pub struct Repere {
    lon0: f64,
    lat0: f64,
    cos_lat0: f64,
}

impl Repere {
    /// Centré sur le barycentre de la frontière communale, ou à défaut sur
    /// celui de tous les tronçons.
    pub fn centre_de(extrait: &Extrait) -> Repere {
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        let mut voir = |p: &[f64; 2]| {
            sx += p[0];
            sy += p[1];
            n += 1.0;
        };
        if let Some(frontiere) = &extrait.frontiere {
            for anneau in &frontiere.anneaux {
                anneau.iter().for_each(&mut voir);
            }
        } else {
            for t in &extrait.troncons {
                t.points.iter().for_each(&mut voir);
            }
        }
        let (lon0, lat0) = if n > 0.0 {
            (sx / n, sy / n)
        } else {
            (2.35, 48.86) // Paris, au cas où l'extrait serait vide
        };
        Repere {
            lon0,
            lat0,
            cos_lat0: lat0.to_radians().cos(),
        }
    }

    /// Projection équirectangulaire locale. À l'échelle d'une ville l'erreur
    /// est très inférieure à celle du tracé lui-même — la même approximation
    /// que `Troncon::longueur_m`.
    pub fn vers_m(&self, p: [f64; 2]) -> [f64; 2] {
        [
            (p[0] - self.lon0).to_radians() * self.cos_lat0 * RAYON_TERRE,
            (p[1] - self.lat0).to_radians() * RAYON_TERRE,
        ]
    }

    /// Réciproque de [`Repere::vers_m`] : un point du repère local, en
    /// mètres, vers ses coordonnées géographiques (`[lon, lat]`, degrés).
    ///
    /// C'est ce qui manquait pour faire le trajet retour : l'affectation
    /// travaille en mètres, mais les tuiles et MapLibre veulent du
    /// lon/lat — la projection Web Mercator (`projection::geo_vers_monde`)
    /// part de là, pas du repère local.
    pub fn depuis_m(&self, p: [f64; 2]) -> [f64; 2] {
        [
            self.lon0 + (p[0] / (self.cos_lat0 * RAYON_TERRE)).to_degrees(),
            self.lat0 + (p[1] / RAYON_TERRE).to_degrees(),
        ]
    }
}

/// Une rue : tous les tronçons OSM qui partagent un nom, résumés en un point
/// et une capacité.
///
/// OSM tronçonne une avenue à chaque changement d'attribut ; pour
/// l'affectation, une avenue est **une** unité, pas une trentaine.
#[derive(Clone, Debug)]
pub struct Rue {
    pub nom: String,
    /// Mètres, somme de tous les tronçons portant ce nom.
    pub longueur: f64,
    /// Mètres, repère local — moyenne des centres de tronçons pondérée par
    /// leur longueur, pour qu'un petit raccord ne pèse pas comme le corps de
    /// la rue.
    pub centre: [f64; 2],
}

/// Regroupe les tronçons de `extrait` par nom.
///
/// Le **halo de la petite couronne** (`crates/osm`, voirie non découpée sur la
/// frontière) n'entre pas dans l'affectation : une rue dont l'essentiel du
/// tracé est hors commune est dessinée mais jamais habitée. C'est la règle
/// « appartient à la ville où elle passe le plus » qu'appliquait le découpage
/// OSM avant qu'on garde le halo.
pub fn rassembler_rues(extrait: &Extrait, repere: &Repere) -> Vec<Rue> {
    let mut acc: HashMap<&str, (f64, f64, f64)> = HashMap::new();
    for t in &extrait.troncons {
        let Some(nom) = t.nom.as_deref() else { continue };
        if let Some(f) = &extrait.frontiere {
            let dedans = t.points.iter().filter(|p| f.contient(**p)).count();
            if dedans * 2 <= t.points.len() {
                continue;
            }
        }
        let m = t.longueur_m();
        if m <= 0.0 {
            continue;
        }
        let n = t.points.len().max(1) as f64;
        let (sx, sy) = t
            .points
            .iter()
            .fold((0.0, 0.0), |(sx, sy), p| (sx + p[0], sy + p[1]));
        let centre_t = repere.vers_m([sx / n, sy / n]);
        let e = acc.entry(nom).or_insert((0.0, 0.0, 0.0));
        e.0 += m;
        e.1 += centre_t[0] * m;
        e.2 += centre_t[1] * m;
    }
    acc.into_iter()
        .map(|(nom, (longueur, sx, sy))| Rue {
            nom: nom.to_string(),
            longueur,
            centre: [sx / longueur, sy / longueur],
        })
        .collect()
}

/// Une famille musicale, telle que l'affectation la voit : son poids (nombre
/// de morceaux) et sa position dans l'espace t-SNE.
#[derive(Clone, Debug)]
pub struct Famille {
    pub id: i64,
    /// Carte `[-1, 1]`, unités internes — pas encore de rapport avec la
    /// géographie.
    pub centroide: [f32; 2],
    pub effectif: usize,
}

/// Barycentre et covariance pondérée d'un nuage de points.
fn moments(points: &[([f64; 2], f64)]) -> ([f64; 2], [[f64; 2]; 2]) {
    let poids_total = points.iter().map(|(_, w)| w).sum::<f64>().max(1e-9);
    let mut centre = [0.0, 0.0];
    for (p, w) in points {
        centre[0] += p[0] * w;
        centre[1] += p[1] * w;
    }
    centre[0] /= poids_total;
    centre[1] /= poids_total;
    let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
    for (p, w) in points {
        let dx = p[0] - centre[0];
        let dy = p[1] - centre[1];
        a += w * dx * dx;
        b += w * dx * dy;
        c += w * dy * dy;
    }
    (centre, [[a / poids_total, b / poids_total], [b / poids_total, c / poids_total]])
}

/// Angle de l'axe principal d'une covariance 2×2 symétrique — forme close,
/// pas de bibliothèque d'algèbre linéaire pour deux nombres.
fn axe_principal(cov: [[f64; 2]; 2]) -> f64 {
    0.5 * (2.0 * cov[0][1]).atan2(cov[0][0] - cov[1][1])
}

/// Étale isotrope d'un nuage — racine de la trace de sa covariance.
fn etalement(cov: [[f64; 2]; 2]) -> f64 {
    (cov[0][0] + cov[1][1]).max(1e-9).sqrt()
}

/// Place chaque famille sur la ville par un Procruste sans correspondance
/// point à point : rotation qui aligne l'axe principal du nuage de familles
/// sur celui du réseau de rues, mise à l'échelle isotrope sur l'étalement,
/// translation du barycentre pondéré sur celui des rues.
///
/// **L'ambiguïté de réflexion (180°) est levée par l'aval** : deux nuages 2D
/// n'ont pas de correspondance canonique, l'axe principal ne dit pas de quel
/// côté est le haut. On engendre donc les deux Procrustes (avec et sans
/// réflexion) et on garde celui dont le [`partitionner`] qui suit a le plus
/// petit coût de transport (somme des distances² rue → germe de sa famille) —
/// une carte miroir éloigne les familles de leurs rues. On ne corrige
/// toujours pas la forme, seulement orientation, échelle, réflexion et
/// position (`carto-ville.md`, objection V2).
///
/// Renvoie aussi la [`Transformation`] elle-même : l'étage 2 en a besoin pour
/// placer un artiste dans la zone de sa famille par le **même** Procruste,
/// plutôt que d'en improviser un second qui ne s'accorderait pas avec le
/// premier.
pub fn semer(familles: &[Famille], rues: &[Rue]) -> (HashMap<i64, [f64; 2]>, Transformation) {
    semer_impl(familles, rues, None, None)
}

/// Comme [`semer`], mais **centre et échelle imposés** — le peuplement dense du
/// centre vers l'extérieur (`docs/carto-ville.md`) place le nuage sur l'île de
/// la Cité, à l'échelle de l'ensemble des bâtiments à peupler, et non sur le
/// barycentre pondéré de *toute* la voirie communale à l'échelle de son
/// étalement (qui étalait le nuage sur 105 km² pour ~27 000 morceaux).
///
/// `rues` doit déjà être filtré au sous-ensemble retenu (celles qui bordent un
/// bâtiment de l'ensemble) : la rotation et le choix de réflexion s'en
/// déduisent, et le [`partitionner`] interne doit voir le même sous-ensemble
/// que celui de l'appelant.
pub fn semer_centre(
    familles: &[Famille],
    rues: &[Rue],
    centre: [f64; 2],
    echelle: f64,
) -> (HashMap<i64, [f64; 2]>, Transformation) {
    semer_impl(familles, rues, Some(centre), Some(echelle))
}

fn semer_impl(
    familles: &[Famille],
    rues: &[Rue],
    centre_impose: Option<[f64; 2]>,
    echelle_imposee: Option<f64>,
) -> (HashMap<i64, [f64; 2]>, Transformation) {
    let pts_f: Vec<([f64; 2], f64)> = familles
        .iter()
        .map(|f| ([f.centroide[0] as f64, f.centroide[1] as f64], f.effectif as f64))
        .collect();
    let pts_r: Vec<([f64; 2], f64)> = rues.iter().map(|r| (r.centre, r.longueur)).collect();

    let (centre_f, cov_f) = moments(&pts_f);
    let (centre_r, cov_r) = moments(&pts_r);
    let rotation = axe_principal(cov_r) - axe_principal(cov_f);
    let echelle = echelle_imposee.unwrap_or_else(|| etalement(cov_r) / etalement(cov_f).max(1e-9));
    let centre_cible = centre_impose.unwrap_or(centre_r);

    let candidate = |reflexion: bool| {
        let t = Transformation { centre_f, centre_cible, rotation, echelle, reflexion };
        let seeds: HashMap<i64, [f64; 2]> =
            familles.iter().map(|f| (f.id, t.appliquer(f.centroide))).collect();
        // Coût de transport du découpage qui suivra : plus il est bas, mieux
        // les familles tombent sur leurs rues.
        let quartiers = partitionner(familles, rues, &seeds);
        let cout: f64 = rues
            .iter()
            .filter_map(|r| quartiers.assignation.get(&r.nom).map(|fam| distance2(r.centre, seeds[fam])))
            .sum();
        (cout, seeds, t)
    };

    let (cout_direct, seeds_direct, t_direct) = candidate(false);
    let (cout_miroir, seeds_miroir, t_miroir) = candidate(true);
    if cout_miroir < cout_direct {
        (seeds_miroir, t_miroir)
    } else {
        (seeds_direct, t_direct)
    }
}

/// Le Procruste calculé par [`semer`], réutilisable point par point.
///
/// Champ par champ plutôt qu'une matrice 2×2 : à ce nombre de paramètres,
/// nommer rotation, échelle et réflexion séparément se relit mieux qu'une
/// matrice générique, et empêche par construction toute cisaille qui trahirait
/// le Procruste (rotation + échelle isotrope + réflexion éventuelle, rien
/// d'autre).
#[derive(Clone, Copy, Debug)]
pub struct Transformation {
    centre_f: [f64; 2],
    /// Le point du repère local, en mètres, sur lequel le barycentre du nuage
    /// de familles est envoyé — barycentre pondéré des rues pour [`semer`],
    /// centre imposé (l'île de la Cité) pour [`semer_centre`].
    centre_cible: [f64; 2],
    rotation: f64,
    echelle: f64,
    /// Retourne l'axe transverse après rotation — le miroir que l'axe
    /// principal seul ne peut pas trancher. Choisi par [`semer`].
    reflexion: bool,
}

impl Transformation {
    /// Envoie un point de l'espace t-SNE (`[-1, 1]`, la carte) dans le repère
    /// local en mètres.
    pub fn appliquer(&self, p: [f32; 2]) -> [f64; 2] {
        let dx = p[0] as f64 - self.centre_f[0];
        let dy = p[1] as f64 - self.centre_f[1];
        let (cos_r, sin_r) = (self.rotation.cos(), self.rotation.sin());
        let rx = dx * cos_r - dy * sin_r;
        let mut ry = dx * sin_r + dy * cos_r;
        if self.reflexion {
            ry = -ry;
        }
        [self.centre_cible[0] + rx * self.echelle, self.centre_cible[1] + ry * self.echelle]
    }
}

/// Le résultat de [`partitionner`].
pub struct Quartiers {
    /// Nom de rue → identifiant de famille.
    pub assignation: HashMap<String, i64>,
    /// Longueur obtenue par famille, mètres.
    pub capacite: HashMap<i64, f64>,
    /// Longueur visée par famille, mètres — proportionnelle à son effectif.
    pub cible: HashMap<i64, f64>,
    /// Poids convergé du diagramme de puissance par famille. Avec les germes
    /// (`seeds`), ils définissent la zone de chaque famille **en tout point**
    /// du plan, pas seulement aux centres de rue — c'est ce que
    /// [`territoires`] contoure pour l'aplat de quartier affiché en dézoomant.
    pub poids: HashMap<i64, f64>,
}

impl Quartiers {
    /// Écart relatif maximal entre capacité obtenue et cible, toutes
    /// familles confondues. C'est le nombre à regarder pour juger si la
    /// contrainte de capacité a tenu.
    pub fn erreur_relative_max(&self) -> f64 {
        self.cible
            .iter()
            .map(|(id, cible)| {
                let obtenue = self.capacite.get(id).copied().unwrap_or(0.0);
                (obtenue - cible).abs() / cible.max(1.0)
            })
            .fold(0.0, f64::max)
    }
}

/// Découpe les rues en zones par familles, sous contrainte de capacité.
///
/// C'est un diagramme de puissance (Voronoï pondéré par addition) dont les
/// poids sont ajustés par rétroaction : une famille en dessous de sa cible
/// devient plus attirante, une famille au-dessus le devient moins. C'est la
/// même idée que l'algorithme de Sinkhorn pour le transport optimal, réduite
/// au cas où un seul côté (les rues) a une masse fixe à recevoir.
///
/// Un vrai Voronoï non pondéré donnerait à une famille dense mais petite en
/// superficie t-SNE (ou à une famille éparse) une zone déséquilibrée par
/// rapport à son nombre de morceaux — typiquement, elle avalerait les bois de
/// Boulogne et Vincennes, presque sans rues, et resterait minuscule en
/// capacité réelle (`carto-ville.md`, objection V3).
pub fn partitionner(familles: &[Famille], rues: &[Rue], seeds: &HashMap<i64, [f64; 2]>) -> Quartiers {
    let total_longueur: f64 = rues.iter().map(|r| r.longueur).sum();
    let total_effectif: f64 = familles.iter().map(|f| f.effectif as f64).sum::<f64>().max(1.0);
    let cible: HashMap<i64, f64> = familles
        .iter()
        .map(|f| (f.id, f.effectif as f64 / total_effectif * total_longueur))
        .collect();

    if familles.is_empty() || rues.is_empty() {
        return Quartiers {
            assignation: HashMap::new(),
            capacite: HashMap::new(),
            cible,
            poids: HashMap::new(),
        };
    }

    // Échelle du pas d'ajustement : l'étalement spatial des rues au carré.
    // Sans cette dimension, le pas serait sans rapport avec les distances
    // au carré qu'il doit contrebalancer dans le coût du diagramme.
    let (centre_rues, cov_rues) = moments(&rues.iter().map(|r| (r.centre, 1.0)).collect::<Vec<_>>());
    let _ = centre_rues;
    let portee2 = (cov_rues[0][0] + cov_rues[1][1]).max(1.0);

    let mut poids: HashMap<i64, f64> = familles.iter().map(|f| (f.id, 0.0)).collect();
    let mut assignation: HashMap<String, i64> = HashMap::new();
    let mut capacite: HashMap<i64, f64> = HashMap::new();

    const ITERATIONS: usize = 80;
    const PAS: f64 = 0.35; // fraction de portee2 déplacée par itération, au taux d'erreur relative

    for iter in 0..ITERATIONS {
        assignation.clear();
        capacite = familles.iter().map(|f| (f.id, 0.0)).collect();
        for rue in rues {
            let mieux = familles
                .iter()
                .map(|f| {
                    let s = seeds[&f.id];
                    let dx = rue.centre[0] - s[0];
                    let dy = rue.centre[1] - s[1];
                    let cout = dx * dx + dy * dy - poids[&f.id];
                    (cout, f.id)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, id)| id)
                .expect("au moins une famille");
            assignation.insert(rue.nom.clone(), mieux);
            *capacite.get_mut(&mieux).unwrap() += rue.longueur;
        }
        // Amorti vers la fin : un pas constant ferait osciller les rues
        // limitrophes indéfiniment entre deux familles à peu près à l'équilibre.
        let amortissement = (1.0 - iter as f64 / ITERATIONS as f64).max(0.1);
        for f in familles {
            let ecart_relatif = (cible[&f.id] - capacite[&f.id]) / cible[&f.id].max(1.0);
            *poids.get_mut(&f.id).unwrap() += PAS * amortissement * portee2 * ecart_relatif;
        }
    }

    Quartiers {
        assignation,
        capacite,
        cible,
        poids,
    }
}

/// Un quartier musical comme **surface** : la région du plan que le diagramme
/// de puissance de l'étage 1 attribue à une famille. Anneaux en mètres du
/// repère local (comme [`Rue::centre`]) ; l'appelant les repasse en lon/lat.
pub struct Territoire {
    pub famille: i64,
    /// Un ou plusieurs polygones, chacun : anneau extérieur puis trous.
    pub polygones: Vec<Vec<Vec<[f64; 2]>>>,
}

/// Contoure les zones du diagramme de puissance convergé ([`partitionner`]) en
/// un polygone par famille — l'aplat de quartier affiché en dézoomant.
///
/// La classification `argmin_f (‖p − germe_f‖² − poids_f)` est celle de
/// [`partitionner`], mais évaluée sur une grille de tout le rectangle
/// `bornes` (mètres locaux) plutôt qu'aux seuls centres de rue, et restreinte
/// aux cellules que `dedans` accepte (la limite communale). Chaque famille
/// devient un champ 0/1 que `contour` transforme en polygone — même brique que
/// `core::density` pour les nappes du monde fictif.
pub fn territoires(
    seeds: &HashMap<i64, [f64; 2]>,
    poids: &HashMap<i64, f64>,
    bornes: [f64; 4],
    resolution: usize,
    dedans: impl Fn([f64; 2]) -> bool,
) -> Vec<Territoire> {
    let gn = resolution.max(2);
    let [xmin, ymin, xmax, ymax] = bornes;
    let pas_x = ((xmax - xmin) / gn as f64).max(1e-6);
    let pas_y = ((ymax - ymin) / gn as f64).max(1e-6);

    let mut ids: Vec<i64> = seeds.keys().copied().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        return Vec::new();
    }

    // Famille gagnante de chaque cellule, ou `i64::MIN` hors commune.
    let mut gagnante = vec![i64::MIN; gn * gn];
    for gy in 0..gn {
        let cy = ymin + (gy as f64 + 0.5) * pas_y;
        for gx in 0..gn {
            let cx = xmin + (gx as f64 + 0.5) * pas_x;
            if !dedans([cx, cy]) {
                continue;
            }
            let mut meilleur = f64::INFINITY;
            let mut choix = i64::MIN;
            for &f in &ids {
                let s = seeds[&f];
                let dx = cx - s[0];
                let dy = cy - s[1];
                let cout = dx * dx + dy * dy - poids.get(&f).copied().unwrap_or(0.0);
                if cout < meilleur {
                    meilleur = cout;
                    choix = f;
                }
            }
            gagnante[gy * gn + gx] = choix;
        }
    }

    let constructeur = contour::ContourBuilder::new(gn, gn, true)
        .x_origin(xmin)
        .y_origin(ymin)
        .x_step(pas_x)
        .y_step(pas_y);

    let mut sortie = Vec::new();
    for &f in &ids {
        let champ: Vec<f64> = gagnante.iter().map(|&g| if g == f { 1.0 } else { 0.0 }).collect();
        let Ok(bandes) = constructeur.isobands(&champ, &[0.5, 1.5]) else { continue };
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
            sortie.push(Territoire { famille: f, polygones });
        }
    }
    sortie
}

fn anneau_ligne(ligne: &geo_types::LineString<f64>) -> Vec<[f64; 2]> {
    ligne.coords().map(|c| [c.x, c.y]).collect()
}

// ---------------------------------------------------------------------------
// Étage 2 — artistes → rues.
// ---------------------------------------------------------------------------

/// Un artiste tel que l'étage 2 le voit : son nombre de morceaux et sa
/// position t-SNE, comme une [`Famille`] mais à l'échelle en dessous.
#[derive(Clone, Debug)]
pub struct Artiste {
    pub nom: String,
    pub famille: i64,
    pub centroide: [f32; 2],
    pub effectif: usize,
}

/// Ce qu'un artiste a reçu : une ou plusieurs rues, dans l'ordre où elles ont
/// été prises (la première est la plus proche de sa position).
#[derive(Clone, Debug)]
pub struct Logement {
    pub rues: Vec<String>,
    /// Nombre de **bâtiments logeables** le long de ces rues (somme des
    /// [`capacites_par_rue`] correspondantes). Peut dépasser `effectif` : la
    /// dernière rue prise est rarement remplie pile.
    pub capacite: usize,
}

/// Compte, pour chaque rue nommée, les bâtiments logeables qui la bordent.
///
/// C'est la capacité **réelle** de l'étage 2 : combien de morceaux une rue
/// peut loger dans de vrais bâtiments, et non `longueur / espacement` qui
/// suppose une adresse tous les quelques mètres — quatre fois trop optimiste
/// face à des façades de 15-30 m (`docs/carto-ville.md`, « La mesure qui
/// compte »).
///
/// Chaque bâtiment est rattaché à **une seule** rue — celle dont un
/// échantillon de tracé passe le plus près de son centre — pour qu'un
/// bâtiment de coin ne soit pas compté deux fois.
pub fn capacites_par_rue(
    traces: &HashMap<String, Trace>,
    grille: &GrilleBatiments,
    espacement: f64,
) -> HashMap<String, usize> {
    let pas = (espacement * 4.0).max(1.0);
    let mut attribution: HashMap<i64, (&str, f64)> = HashMap::new();
    for (nom, trace) in traces {
        let mut s = 0.0;
        loop {
            let (pos, _) = trace.au(s);
            for b in grille.pres_de(pos, RAYON_RECHERCHE) {
                let d = distance2(b.centre, pos);
                attribution
                    .entry(b.id)
                    .and_modify(|meilleur| {
                        if d < meilleur.1 {
                            *meilleur = (nom.as_str(), d);
                        }
                    })
                    .or_insert((nom.as_str(), d));
            }
            if s >= trace.longueur() {
                break;
            }
            s += pas;
        }
    }
    let mut capacite: HashMap<String, usize> = traces.keys().map(|n| (n.clone(), 0)).collect();
    for (_, (rue, _)) in attribution {
        if let Some(c) = capacite.get_mut(rue) {
            *c += 1;
        }
    }
    capacite
}

/// Le résultat de [`loger_artistes`].
pub struct Voirie {
    pub logements: HashMap<String, Logement>,
    /// Rues jamais prises par personne — il y en aura, la capacité vise
    /// large exprès (voir [`loger_artistes`]).
    pub rues_libres: Vec<String>,
    /// Artistes dont la zone de famille n'avait plus assez de rues et qui ont
    /// dû emprunter une rue d'une autre famille. Compté, jamais caché : c'est
    /// le signe que `espacement` est trop large ou qu'une famille est trop
    /// petite en rues pour son effectif.
    pub debordements: Vec<String>,
}

/// Loge chaque artiste sur une ou plusieurs rues de la zone de sa famille.
///
/// Glouton, du plus gros artiste au plus petit — sur un espace disputé,
/// commencer par les petits laisserait les gros sans rue assez longue pour
/// les loger d'un bloc. Chaque artiste vise, dans sa zone, la rue libre la
/// plus proche de sa position (la même [`Transformation`] que l'étage 1,
/// appliquée à son centroïde plutôt qu'à celui de sa famille) ; s'il ne
/// tient pas dessus, il prend la suivante la plus proche, jusqu'à tenir.
///
/// « Tenir » se mesure en **bâtiments logeables** (`capacite_rue`, voir
/// [`capacites_par_rue`]), pas en longueur de voirie : c'est la correction
/// qui évite qu'un artiste reçoive une rue trop pauvre en bâtiments pour ses
/// morceaux et que l'étage 3 les éparpille ailleurs (`docs/carto-ville.md`).
/// Une rue sans aucun bâtiment logeable ne fait rien avancer — l'artiste
/// passe à la suivante.
///
/// **Une rue appartient à un seul artiste.** Pas de partage : c'est ce qui
/// fait qu'une rue porte un nom d'artiste sans ambiguïté à l'affichage, et
/// c'est fidèle à une vraie ville — une rue a un nom, pas deux.
pub fn loger_artistes(
    artistes: &[Artiste],
    rues: &[Rue],
    quartiers: &Quartiers,
    transformation: &Transformation,
    capacite_rue: &HashMap<String, usize>,
) -> Voirie {
    let mut par_nom: HashMap<&str, &Rue> = rues.iter().map(|r| (r.nom.as_str(), r)).collect();
    let mut disponibles: HashMap<i64, Vec<&str>> = HashMap::new();
    for (nom, famille) in &quartiers.assignation {
        disponibles.entry(*famille).or_default().push(nom.as_str());
    }

    let mut logements: HashMap<String, Logement> = HashMap::new();
    let mut debordements = Vec::new();

    let mut tries: Vec<&Artiste> = artistes.iter().collect();
    tries.sort_by_key(|a| std::cmp::Reverse(a.effectif));

    for artiste in tries {
        let cible = transformation.appliquer(artiste.centroide);
        let besoin = artiste.effectif;
        let mut prises = Vec::new();
        let mut capacite_prise = 0usize;
        let mut a_deborde = false;

        while capacite_prise < besoin {
            // La zone de la famille d'abord ; si elle n'a plus de rue avec des
            // bâtiments logeables, n'importe quelle rue libre restante, où
            // qu'elle soit — mieux qu'un artiste sans adresse.
            //
            // On ne retient que les rues qui ont au moins un bâtiment
            // logeable : prendre une rue vide n'avancerait pas `capacite_prise`
            // et l'artiste avalerait toute la voirie déserte de sa zone avant
            // d'en sortir.
            let a_de_la_place =
                |n: &&str| capacite_rue.get(*n).copied().unwrap_or(0) > 0;
            let zone: Vec<&str> = disponibles
                .get(&artiste.famille)
                .map(|v| v.iter().copied().filter(a_de_la_place).collect())
                .unwrap_or_default();
            let (candidats, hors_zone) = if !zone.is_empty() {
                (zone, false)
            } else {
                let secours: Vec<&str> =
                    disponibles.values().flatten().copied().filter(a_de_la_place).collect();
                (secours, true)
            };
            let Some(plus_proche) = candidats
                .iter()
                .min_by(|a, b| distance2(par_nom[*a].centre, cible).total_cmp(&distance2(par_nom[*b].centre, cible)))
                .copied()
            else {
                break; // plus aucune rue avec des bâtiments, nulle part
            };
            if hors_zone {
                a_deborde = true;
            }
            let rue = par_nom.remove(plus_proche).expect("cohérence de l'index");
            for v in disponibles.values_mut() {
                v.retain(|n| *n != plus_proche);
            }
            capacite_prise += capacite_rue.get(&rue.nom).copied().unwrap_or(0);
            prises.push(rue.nom.clone());
        }

        if a_deborde {
            debordements.push(artiste.nom.clone());
        }
        logements.insert(
            artiste.nom.clone(),
            Logement {
                rues: prises,
                capacite: capacite_prise,
            },
        );
    }

    let rues_libres = par_nom.keys().map(|n| n.to_string()).collect();
    Voirie {
        logements,
        rues_libres,
        debordements,
    }
}

fn distance2(a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

// ---------------------------------------------------------------------------
// Étage 3 — morceaux → bâtiments.
// ---------------------------------------------------------------------------

/// Rayon de recherche des bâtiments autour d'un point de rue, mètres.
/// Calibré à l'œil, pas mesuré — assez large pour attraper les bâtiments des
/// deux côtés d'une rue parisienne ordinaire, assez étroit pour ne pas
/// déborder sur la rue parallèle suivante.
const RAYON_RECHERCHE: f64 = 40.0;

/// Une position le long d'une polyligne, échantillonnable par distance
/// parcourue depuis le début. Sert à semer les adresses le long d'une rue
/// sans exiger que ses tronçons soient déjà mis bout à bout par OSM — ils ne
/// le sont pas toujours (voir [`assembler_trace`]).
pub struct Trace {
    points: Vec<[f64; 2]>,
    cumul: Vec<f64>,
}

impl Trace {
    fn nouvelle(points: Vec<[f64; 2]>) -> Trace {
        let mut cumul = vec![0.0; points.len()];
        for i in 1..points.len() {
            cumul[i] = cumul[i - 1] + distance2(points[i - 1], points[i]).sqrt();
        }
        Trace { points, cumul }
    }

    pub fn longueur(&self) -> f64 {
        self.cumul.last().copied().unwrap_or(0.0)
    }

    /// Position et tangente unitaire à la distance `s` depuis le début de la
    /// trace, bornée à ses deux extrémités.
    pub fn au(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        if self.points.len() < 2 {
            return (self.points.first().copied().unwrap_or([0.0, 0.0]), [1.0, 0.0]);
        }
        let s = s.clamp(0.0, self.longueur());
        let i = self
            .cumul
            .partition_point(|&c| c <= s)
            .saturating_sub(1)
            .min(self.points.len() - 2);
        let (a, b) = (self.points[i], self.points[i + 1]);
        let long_segment = (self.cumul[i + 1] - self.cumul[i]).max(1e-9);
        let t = (s - self.cumul[i]) / long_segment;
        let pos = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let dir = [(b[0] - a[0]) / long_segment, (b[1] - a[1]) / long_segment];
        (pos, dir)
    }
}

/// Met bout à bout les tronçons d'une même rue en une seule polyligne, en
/// mètres locaux.
///
/// OSM ne garantit ni que les tronçons d'un même nom se suivent dans l'ordre
/// où ils apparaissent, ni que leur sens soit cohérent d'un tronçon à
/// l'autre. On les ordonne donc par projection sur l'axe principal du nuage
/// de leurs points — une régression, pas une topologie — puis on retourne
/// chaque tronçon si son premier point est plus proche de la fin de la trace
/// en cours que son dernier.
///
/// **Approximatif sur une rue en épingle à cheveux**, qui se replierait sur
/// elle-même : assez bon pour semer des adresses dans un ordre cohérent, pas
/// pour un tracé fidèle à afficher (`carto-ville.md`, objection V7).
fn assembler_trace(troncons: &[&Troncon], repere: &Repere) -> Vec<[f64; 2]> {
    if troncons.is_empty() {
        return Vec::new();
    }
    let converses: Vec<Vec<[f64; 2]>> = troncons
        .iter()
        .map(|t| t.points.iter().map(|p| repere.vers_m(*p)).collect())
        .collect();

    let tous: Vec<([f64; 2], f64)> = converses.iter().flatten().map(|p| (*p, 1.0)).collect();
    let (centre, cov) = moments(&tous);
    let angle = axe_principal(cov);
    let (cos_a, sin_a) = (angle.cos(), angle.sin());
    let projection = |p: [f64; 2]| (p[0] - centre[0]) * cos_a + (p[1] - centre[1]) * sin_a;

    let mut ordre: Vec<usize> = (0..converses.len()).collect();
    ordre.sort_by(|&i, &j| {
        let pi = converses[i].iter().copied().map(projection).sum::<f64>() / converses[i].len().max(1) as f64;
        let pj = converses[j].iter().copied().map(projection).sum::<f64>() / converses[j].len().max(1) as f64;
        pi.total_cmp(&pj)
    });

    let mut trace: Vec<[f64; 2]> = Vec::new();
    for i in ordre {
        let mut pts = converses[i].clone();
        if pts.is_empty() {
            continue;
        }
        if let Some(&dernier) = trace.last() {
            let debut = distance2(pts[0], dernier);
            let fin = distance2(*pts.last().expect("non vide"), dernier);
            if fin < debut {
                pts.reverse();
            }
        }
        trace.extend(pts);
    }
    trace
}

/// Construit la trace de chaque rue nommée de `extrait`, en mètres locaux —
/// ce qu'il faut pour échantillonner les bâtiments le long d'une rue dans
/// [`loger_dans_batiments`].
pub fn traces_des_rues(extrait: &Extrait, repere: &Repere) -> HashMap<String, Trace> {
    extrait
        .rues_par_nom()
        .into_iter()
        .map(|(nom, troncons)| (nom.to_string(), Trace::nouvelle(assembler_trace(&troncons, repere))))
        .collect()
}

/// Une adresse posée : un morceau, la rue qui lui donne son nom affiché
/// (`ville::nom_affiche`), le bâtiment réel qui l'habite, sa position.
#[derive(Clone, Debug)]
pub struct Adresse {
    pub track_id: i64,
    pub rue: String,
    pub batiment_id: i64,
    /// Mètres, repère local — le même que [`Rue::centre`] et [`Trace`].
    pub point: [f64; 2],
    /// `true` si ce morceau a dû se replier sur une autre rue du quartier de
    /// sa famille — les rues propres à son artiste étaient déjà pleines.
    /// Reste dans le bon voisinage musical, juste pas sur sa rue attitrée.
    pub repli_quartier: bool,
    /// `true` si ce morceau a dû se replier sur un bâtiment n'importe où
    /// dans Paris — le quartier de sa famille entier était épuisé. Dernier
    /// recours, compté, jamais caché (voir `ville::rassembler`).
    pub hors_zone: bool,
}

/// Loge les morceaux d'un artiste dans de vrais bâtiments le long des rues de
/// son [`Logement`]. `pistes` donne, pour chaque morceau, son identifiant et
/// sa **cible** : la position en mètres locaux que lui donne le Procruste des
/// étages 1-2 (`Transformation::appliquer`) appliqué à sa propre coordonnée
/// t-SNE — pas seulement à celle de son artiste. À l'appelant de les avoir
/// triées par (album, piste, identifiant), la même clé que `CleArrivee` du
/// peuplement (`carto-peuplement-architecture.md`).
///
/// **Un bâtiment par morceau, jamais partagé** — décidé avec l'utilisateur :
/// pas un immeuble d'appartements, une maison par morceau. `batiments_pris`
/// est partagé entre tous les appels (un par artiste) sur tout l'extrait :
/// c'est ce qui empêche deux artistes de réclamer le même bâtiment.
///
/// Trois cercles de recherche, dans l'ordre :
///
/// 1. **Les rues de l'artiste** (`logement.rues`, la plus proche d'abord,
///    depuis l'étage 2).
/// 2. **Le reste du quartier de sa famille** (`quartier_rues`) — mesuré sur
///    la vraie bibliothèque : la capacité des rues (étage 2, en longueur)
///    suppose une adresse tous les `espacement` mètres, bien plus dense que
///    de vrais bâtiments (15-30 m de façade chacun) — un artiste épuise donc
///    couramment ses propres rues avant ses morceaux (51 % de repli mesuré
///    avant l'ajout de ce cercle). Rester dans le quartier de la famille
///    évite qu'un repli parte n'importe où dans Paris et détruise le
///    voisinage géographique que l'affectation cherche à préserver.
/// 3. **N'importe où dans Paris** (`grille` entière) — dernier recours, si le
///    quartier entier est épuisé.
///
/// Dans chaque cercle, on échantillonne la [`Trace`] de chaque rue tous les
/// `espacement * 4` mètres pour réunir un bassin de bâtiments libres, puis
/// **chaque morceau prend celui du bassin le plus proche de sa cible**. C'est
/// ce qui fait qu'un morceau atterrit près de ses voisins sonores — et que
/// les pistes d'un album, de cibles presque identiques, se groupent — plutôt
/// qu'au hasard de l'ordre des pistes ou de la taille des bâtiments. On perd
/// « l'artiste prolifique hérite des plus grands bâtiments » (l'ancien tri par
/// aire décroissante) : c'était un proxy de popularité, jamais central
/// (`docs/carto-ville.md`).
pub fn loger_dans_batiments(
    pistes: &[(i64, [f64; 2])],
    logement: &Logement,
    quartier_rues: &[String],
    traces: &HashMap<String, Trace>,
    grille: &GrilleBatiments,
    batiments_pris: &mut HashSet<i64>,
    espacement: f64,
) -> Vec<Adresse> {
    let mut sortie = Vec::with_capacity(pistes.len());
    let mut i = 0usize;
    let pas_echantillon = (espacement * 4.0).max(1.0);

    chercher_et_loger(&logement.rues, false, pistes, &mut i, traces, grille, batiments_pris, pas_echantillon, &mut sortie);

    if i < pistes.len() {
        // Le reste du quartier : les rues de la famille jamais tentées
        // ci-dessus (celles de l'artiste l'ont déjà été).
        let autres: Vec<String> = quartier_rues.iter().filter(|r| !logement.rues.contains(r)).cloned().collect();
        chercher_et_loger(&autres, true, pistes, &mut i, traces, grille, batiments_pris, pas_echantillon, &mut sortie);
    }

    // Dernier recours : le quartier entier est épuisé, il reste des
    // morceaux. Un morceau sans logement serait pire qu'un logement hors
    // zone — mais celui-ci est rare, contrairement au cercle 2 ci-dessus.
    // Le bâtiment libre le plus proche de la cible, toujours — même hors zone,
    // autant le poser au moins du bon côté de Paris.
    if i < pistes.len() {
        let rue_repli = logement.rues.first().cloned().unwrap_or_default();
        while i < pistes.len() {
            let (id, cible) = pistes[i];
            let choix = grille
                .tous()
                .iter()
                .filter(|b| !batiments_pris.contains(&b.id))
                .min_by(|a, b| {
                    distance2(a.centre, cible)
                        .total_cmp(&distance2(b.centre, cible))
                        .then(a.id.cmp(&b.id))
                })
                .map(|b| (b.id, b.centre));
            let Some((bid, centre)) = choix else {
                break;
            };
            batiments_pris.insert(bid);
            sortie.push(Adresse {
                track_id: id,
                rue: rue_repli.clone(),
                batiment_id: bid,
                point: centre,
                repli_quartier: false,
                hors_zone: true,
            });
            i += 1;
        }
    }

    sortie
}

/// Réunit un bassin de bâtiments libres le long de `rues` puis loge les
/// morceaux restants de `pistes[*i..]` dedans — **chacun dans celui le plus
/// proche de sa cible**. Le cœur commun aux deux premiers cercles de
/// [`loger_dans_batiments`]. Fonction plutôt que fermeture : elle emprunte
/// `batiments_pris` en mutable en même temps que `i` et `sortie`, ce qu'une
/// fermeture capturante ne peut pas faire proprement.
///
/// L'échantillonnage s'arrête dès que le bassin peut loger tous les morceaux
/// restants : inutile de balayer un quartier entier pour deux adresses.
#[allow(clippy::too_many_arguments)]
fn chercher_et_loger(
    rues: &[String],
    repli_quartier: bool,
    pistes: &[(i64, [f64; 2])],
    i: &mut usize,
    traces: &HashMap<String, Trace>,
    grille: &GrilleBatiments,
    batiments_pris: &mut HashSet<i64>,
    pas_echantillon: f64,
    sortie: &mut Vec<Adresse>,
) {
    if *i >= pistes.len() {
        return;
    }
    let besoin = pistes.len() - *i;

    let mut bassin: Vec<(&Batiment, &str)> = Vec::new();
    let mut vus: HashSet<i64> = HashSet::new();
    for rue in rues {
        if bassin.len() >= besoin {
            break;
        }
        let Some(trace) = traces.get(rue) else { continue };
        let mut s = 0.0;
        loop {
            let (pos, _) = trace.au(s);
            for b in grille.pres_de(pos, RAYON_RECHERCHE) {
                if !batiments_pris.contains(&b.id) && vus.insert(b.id) {
                    bassin.push((b, rue.as_str()));
                }
            }
            if s >= trace.longueur() {
                break;
            }
            s += pas_echantillon;
        }
    }

    while *i < pistes.len() {
        let (id, cible) = pistes[*i];
        let choix = bassin
            .iter()
            .filter(|(b, _)| !batiments_pris.contains(&b.id))
            .min_by(|(a, _), (b, _)| {
                distance2(a.centre, cible)
                    .total_cmp(&distance2(b.centre, cible))
                    .then(a.id.cmp(&b.id))
            })
            .map(|(b, rue)| (b.id, b.centre, rue.to_string()));
        let Some((bid, centre, rue)) = choix else {
            break;
        };
        batiments_pris.insert(bid);
        sortie.push(Adresse {
            track_id: id,
            rue,
            batiment_id: bid,
            point: centre,
            repli_quartier,
            hors_zone: false,
        });
        *i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::{Classe, Troncon};

    fn extrait_lineaire() -> Extrait {
        // Dix rues alignées d'ouest en est, toutes de même longueur — un cas
        // qu'on peut vérifier à la main.
        let troncons = (0..10)
            .map(|i| Troncon {
                id: i,
                nom: Some(format!("Rue {i}")),
                classe: Classe::Residentielle,
                points: vec![[2.30 + i as f64 * 0.01, 48.85], [2.30 + i as f64 * 0.01 + 0.005, 48.85]],
            })
            .collect();
        Extrait {
            troncons,
            ..Default::default()
        }
    }

    #[test]
    fn le_repere_convertit_un_dixieme_de_degre_en_quelques_kilometres() {
        let extrait = extrait_lineaire();
        let repere = Repere::centre_de(&extrait);
        let a = repere.vers_m([2.30, 48.85]);
        let b = repere.vers_m([2.40, 48.85]);
        let d = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        // 0,1° de longitude à 48,85° de latitude ≈ 7,3 km.
        assert!((7000.0..7600.0).contains(&d), "{d} m");
    }

    #[test]
    fn depuis_m_est_la_reciproque_de_vers_m() {
        let extrait = extrait_lineaire();
        let repere = Repere::centre_de(&extrait);
        for lon_lat in [[2.30, 48.85], [2.45, 48.90], [2.20, 48.81]] {
            let m = repere.vers_m(lon_lat);
            let retour = repere.depuis_m(m);
            assert!((retour[0] - lon_lat[0]).abs() < 1e-9, "lon {retour:?} vs {lon_lat:?}");
            assert!((retour[1] - lon_lat[1]).abs() < 1e-9, "lat {retour:?} vs {lon_lat:?}");
        }
    }

    #[test]
    fn rassembler_regroupe_par_nom_et_somme_les_longueurs() {
        let mut extrait = extrait_lineaire();
        // Un second tronçon pour "Rue 0", comme OSM les découpe en vrai.
        extrait.troncons.push(Troncon {
            id: 100,
            nom: Some("Rue 0".into()),
            classe: Classe::Residentielle,
            points: vec![[2.305, 48.85], [2.31, 48.85]],
        });
        let repere = Repere::centre_de(&extrait);
        let rues = rassembler_rues(&extrait, &repere);
        assert_eq!(rues.len(), 10, "toujours dix noms distincts");
        let rue0 = rues.iter().find(|r| r.nom == "Rue 0").unwrap();
        assert!(rue0.longueur > rues.iter().find(|r| r.nom == "Rue 1").unwrap().longueur);
    }

    #[test]
    fn semer_place_les_grandes_familles_loin_du_barycentre_dans_le_meme_sens() {
        // Deux familles alignées sur x dans l'espace t-SNE ; les rues aussi
        // s'étalent sur x. L'ordre relatif doit survivre à la transformation.
        let familles = vec![
            Famille { id: 1, centroide: [-0.5, 0.0], effectif: 100 },
            Famille { id: 2, centroide: [0.5, 0.0], effectif: 100 },
        ];
        let extrait = extrait_lineaire();
        let repere = Repere::centre_de(&extrait);
        let rues = rassembler_rues(&extrait, &repere);
        let (seeds, _transfo) = semer(&familles, &rues);
        assert!(seeds[&1][0] < seeds[&2][0], "l'ordre le long de l'axe est préservé");
    }

    #[test]
    fn partitionner_respecte_la_capacite_a_deux_familles_tres_inegales() {
        let familles = vec![
            Famille { id: 1, centroide: [-0.8, 0.0], effectif: 900 },
            Famille { id: 2, centroide: [0.8, 0.0], effectif: 100 },
        ];
        let extrait = extrait_lineaire();
        let repere = Repere::centre_de(&extrait);
        let rues = rassembler_rues(&extrait, &repere);
        let (seeds, _transfo) = semer(&familles, &rues);
        let q = partitionner(&familles, &rues, &seeds);

        assert_eq!(q.assignation.len(), rues.len(), "chaque rue est assignée");
        let erreur = q.erreur_relative_max();
        assert!(erreur < 0.35, "erreur relative maximale {erreur:.2}, attendu < 0,35");

        // La grosse famille doit avoir reçu la majorité des rues.
        let total: f64 = q.capacite.values().sum();
        assert!(q.capacite[&1] > total * 0.7, "famille 1 : {:.0} / {total:.0}", q.capacite[&1]);
    }

    #[test]
    fn partitionner_narrive_pas_a_zero_famille() {
        let q = partitionner(&[], &[], &HashMap::new());
        assert!(q.assignation.is_empty());
    }

    /// Le halo de petite couronne (voirie hors commune) est dessiné mais
    /// n'entre pas dans l'affectation : `rassembler_rues` l'écarte.
    #[test]
    fn le_halo_hors_commune_nentre_pas_dans_laffectation() {
        use rusty_music_osm::Frontiere;
        // Frontière = carré [2.30, 48.84]..[2.32, 48.86].
        let frontiere = Frontiere::nouvelle(vec![vec![
            [2.30, 48.84], [2.32, 48.84], [2.32, 48.86], [2.30, 48.86], [2.30, 48.84],
        ]]);
        let dedans = Troncon {
            id: 1, nom: Some("Rue Dedans".into()), classe: Classe::Residentielle,
            points: vec![[2.305, 48.85], [2.310, 48.851]],
        };
        let dehors = Troncon {
            id: 2, nom: Some("Rue Dehors".into()), classe: Classe::Residentielle,
            points: vec![[2.340, 48.85], [2.345, 48.851]],
        };
        let extrait = Extrait {
            troncons: vec![dedans, dehors],
            frontiere: Some(frontiere),
            ..Default::default()
        };
        let repere = Repere::centre_de(&extrait);
        let noms: Vec<String> = rassembler_rues(&extrait, &repere).into_iter().map(|r| r.nom).collect();
        assert!(noms.iter().any(|n| n == "Rue Dedans"), "{noms:?}");
        assert!(!noms.iter().any(|n| n == "Rue Dehors"), "le halo a été assigné : {noms:?}");
    }

    #[test]
    fn loger_artistes_donne_plusieurs_rues_au_plus_gros() {
        // Une seule famille, cinq rues de 100 m : à 10 m d'espacement,
        // chaque rue loge 10 morceaux. Le gros artiste (25 morceaux) doit en
        // prendre trois ; les petits (5 chacun) tiennent sur une seule.
        let familles = vec![Famille { id: 1, centroide: [0.0, 0.0], effectif: 40 }];
        let troncons: Vec<Troncon> = (0..5)
            .map(|i| Troncon {
                id: i,
                nom: Some(format!("Rue {i}")),
                classe: Classe::Residentielle,
                points: vec![[2.30 + i as f64 * 0.01, 48.85], [2.30 + i as f64 * 0.01 + 0.0009, 48.85]],
            })
            .collect();
        let extrait = Extrait { troncons, ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let rues = rassembler_rues(&extrait, &repere);
        // ~66 m par rue à cette latitude, pour ce pas de longitude.
        assert!(rues.iter().all(|r| (50.0..80.0).contains(&r.longueur)), "{:?}", rues.iter().map(|r| r.longueur).collect::<Vec<_>>());

        let (seeds, transfo) = semer(&familles, &rues);
        let quartiers = partitionner(&familles, &rues, &seeds);

        let artistes = vec![
            Artiste { nom: "Gros".into(), famille: 1, centroide: [0.0, 0.0], effectif: 25 },
            Artiste { nom: "Petit A".into(), famille: 1, centroide: [0.0, 0.0], effectif: 5 },
            Artiste { nom: "Petit B".into(), famille: 1, centroide: [0.0, 0.0], effectif: 5 },
            Artiste { nom: "Petit C".into(), famille: 1, centroide: [0.0, 0.0], effectif: 5 },
        ];
        // 13 bâtiments logeables par rue : "Gros" (25) tient sur deux rues,
        // les petits (5) sur une.
        let capacites: HashMap<String, usize> = (0..5).map(|i| (format!("Rue {i}"), 13)).collect();
        let voirie = loger_artistes(&artistes, &rues, &quartiers, &transfo, &capacites);

        assert!(voirie.debordements.is_empty(), "tout tient dans la seule famille");
        let gros = &voirie.logements["Gros"];
        assert!(gros.rues.len() >= 2, "25 morceaux à 10/rue tiennent difficilement sur une seule : {:?}", gros.rues);
        assert!(gros.capacite >= 25);
        for petit in ["Petit A", "Petit B", "Petit C"] {
            let l = &voirie.logements[petit];
            assert_eq!(l.rues.len(), 1, "{petit} devrait tenir sur une seule rue");
            assert!(l.capacite >= 5);
        }
        // Aucune rue prêtée deux fois.
        let mut toutes: Vec<&str> = voirie.logements.values().flat_map(|l| l.rues.iter().map(|s| s.as_str())).collect();
        toutes.sort_unstable();
        let avant = toutes.len();
        toutes.dedup();
        assert_eq!(toutes.len(), avant, "une rue ne doit appartenir qu'à un seul artiste");
    }

    #[test]
    fn loger_artistes_deborde_proprement_quand_la_zone_est_trop_petite() {
        // Deux familles ; la famille 2 n'a qu'une rue minuscule pour un
        // artiste qui a besoin de plus — il doit emprunter chez la famille 1.
        let familles = vec![
            Famille { id: 1, centroide: [-0.9, 0.0], effectif: 90 },
            Famille { id: 2, centroide: [0.9, 0.0], effectif: 10 },
        ];
        let mut troncons: Vec<Troncon> = (0..9)
            .map(|i| Troncon {
                id: i,
                nom: Some(format!("Grande rue {i}")),
                classe: Classe::Residentielle,
                points: vec![[2.20 + i as f64 * 0.01, 48.85], [2.20 + i as f64 * 0.01 + 0.0009, 48.85]],
            })
            .collect();
        troncons.push(Troncon {
            id: 100,
            nom: Some("Petite rue".into()),
            classe: Classe::Residentielle,
            points: vec![[2.45, 48.85], [2.4501, 48.85]],
        });
        let extrait = Extrait { troncons, ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let rues = rassembler_rues(&extrait, &repere);
        let (seeds, transfo) = semer(&familles, &rues);
        let quartiers = partitionner(&familles, &rues, &seeds);

        let artistes = vec![Artiste {
            nom: "Trop grand pour sa rue".into(),
            famille: 2,
            centroide: [0.9, 0.0],
            effectif: 50,
        }];
        // "Petite rue" (zone de la famille 2) ne loge que 2 morceaux ; les
        // grandes rues de la famille 1 en logent 20 chacune.
        let mut capacites: HashMap<String, usize> =
            (0..9).map(|i| (format!("Grande rue {i}"), 20)).collect();
        capacites.insert("Petite rue".into(), 2);
        let voirie = loger_artistes(&artistes, &rues, &quartiers, &transfo, &capacites);
        let logement = &voirie.logements["Trop grand pour sa rue"];
        assert!(logement.capacite >= 50, "capacité {}", logement.capacite);
        assert!(!voirie.debordements.is_empty(), "a dû sortir de sa zone");
    }

    #[test]
    fn loger_artistes_saute_une_rue_sans_batiment() {
        // Deux rues dans la même zone : la plus proche de l'artiste n'a aucun
        // bâtiment logeable, la plus loin en a assez. L'artiste doit prendre
        // la seconde, pas s'accrocher à la première.
        let familles = vec![Famille { id: 1, centroide: [0.0, 0.0], effectif: 10 }];
        let troncons: Vec<Troncon> = (0..2)
            .map(|i| Troncon {
                id: i,
                nom: Some(format!("Rue {i}")),
                classe: Classe::Residentielle,
                points: vec![[2.30 + i as f64 * 0.02, 48.85], [2.30 + i as f64 * 0.02 + 0.005, 48.85]],
            })
            .collect();
        let extrait = Extrait { troncons, ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let rues = rassembler_rues(&extrait, &repere);
        let (seeds, transfo) = semer(&familles, &rues);
        let quartiers = partitionner(&familles, &rues, &seeds);

        // Artiste au niveau de "Rue 0" (x ~ -1), mais "Rue 0" n'a rien.
        let artistes = vec![Artiste { nom: "A".into(), famille: 1, centroide: [-1.0, 0.0], effectif: 5 }];
        let mut capacites: HashMap<String, usize> = HashMap::new();
        capacites.insert("Rue 0".into(), 0);
        capacites.insert("Rue 1".into(), 8);

        let voirie = loger_artistes(&artistes, &rues, &quartiers, &transfo, &capacites);
        let logement = &voirie.logements["A"];
        assert_eq!(logement.rues, vec!["Rue 1".to_string()], "a sauté la rue sans bâtiment");
        assert!(logement.capacite >= 5);
    }

    #[test]
    fn territoires_contoure_une_zone_par_famille() {
        // Deux germes de part et d'autre de l'origine, aucun poids : le plan
        // se coupe en deux moitiés. Chaque famille doit ressortir avec un
        // polygone non vide, et un point de son côté doit tomber dedans.
        let mut seeds = HashMap::new();
        seeds.insert(1, [-500.0, 0.0]);
        seeds.insert(2, [500.0, 0.0]);
        let poids: HashMap<i64, f64> = [(1, 0.0), (2, 0.0)].into_iter().collect();

        let terrs = territoires(&seeds, &poids, [-1000.0, -1000.0, 1000.0, 1000.0], 80, |_| true);
        assert_eq!(terrs.len(), 2, "une zone par famille");
        for t in &terrs {
            assert!(t.polygones.iter().any(|p| p.first().is_some_and(|a| a.len() >= 4)));
        }

        // La zone de la famille 1 couvre l'ouest, celle de la famille 2 l'est.
        let ouest = terrs.iter().find(|t| t.famille == 1).unwrap();
        let centre_x: f64 = {
            let anneau = &ouest.polygones[0][0];
            anneau.iter().map(|p| p[0]).sum::<f64>() / anneau.len() as f64
        };
        assert!(centre_x < 0.0, "la zone de la famille 1 doit pencher à l'ouest : {centre_x}");
    }

    #[test]
    fn territoires_respecte_le_masque() {
        let mut seeds = HashMap::new();
        seeds.insert(1, [0.0, 0.0]);
        let poids: HashMap<i64, f64> = [(1, 0.0)].into_iter().collect();
        // Masque qui ne garde que le quart nord-est.
        let terrs = territoires(&seeds, &poids, [-100.0, -100.0, 100.0, 100.0], 60, |p| p[0] > 0.0 && p[1] > 0.0);
        assert_eq!(terrs.len(), 1);
        let anneau = &terrs[0].polygones[0][0];
        assert!(anneau.iter().all(|p| p[0] > -2.0 && p[1] > -2.0), "la zone déborde le masque : {anneau:?}");
    }

    #[test]
    fn capacites_par_rue_compte_les_batiments_de_chaque_rue() {
        let troncons = vec![
            Troncon { id: 1, nom: Some("Peuplée".into()), classe: Classe::Residentielle, points: vec![[2.30, 48.85], [2.31, 48.85]] },
            Troncon { id: 2, nom: Some("Déserte".into()), classe: Classe::Residentielle, points: vec![[2.30, 48.86], [2.31, 48.86]] },
        ];
        let mut extrait = Extrait { troncons, ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let traces = traces_des_rues(&extrait, &repere);

        // Trois bâtiments le long de "Peuplée", aucun le long de "Déserte".
        let trace = &traces["Peuplée"];
        for k in 0..3 {
            let (pos, _) = trace.au(trace.longueur() * k as f64 / 3.0);
            extrait.batis.push(contour_carre(&repere, 10 + k, [pos[0], pos[1] + 8.0], 12.0));
        }

        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let capacites = capacites_par_rue(&traces, &grille, 4.0);
        assert_eq!(capacites.get("Peuplée").copied(), Some(3));
        assert_eq!(capacites.get("Déserte").copied(), Some(0));
    }

    #[test]
    fn trace_interpole_lineairement_entre_deux_points() {
        let trace = Trace::nouvelle(vec![[0.0, 0.0], [100.0, 0.0]]);
        assert_eq!(trace.longueur(), 100.0);
        let (pos, dir) = trace.au(25.0);
        assert_eq!(pos, [25.0, 0.0]);
        assert_eq!(dir, [1.0, 0.0]);
        // Bornée aux extrémités, pas d'extrapolation.
        assert_eq!(trace.au(-10.0).0, [0.0, 0.0]);
        assert_eq!(trace.au(1000.0).0, [100.0, 0.0]);
    }

    #[test]
    fn assembler_trace_met_bout_a_bout_deux_troncons_inverses() {
        // Deux tronçons colinéaires sur l'axe des x, le second donné dans le
        // sens retour — comme OSM les fournit parfois.
        let a = Troncon { id: 1, nom: Some("Rue".into()), classe: Classe::Residentielle, points: vec![[2.30, 48.85], [2.301, 48.85]] };
        let b = Troncon { id: 2, nom: Some("Rue".into()), classe: Classe::Residentielle, points: vec![[2.303, 48.85], [2.302, 48.85]] };
        let extrait = Extrait { troncons: vec![a, b], ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let trace = Trace::nouvelle(assembler_trace(&[&extrait.troncons[0], &extrait.troncons[1]], &repere));
        // La longueur doit être proche de la somme des deux segments, pas
        // d'un aller-retour qui compterait le trou entre eux deux fois.
        let attendu = extrait.troncons[0].longueur_m() + extrait.troncons[1].longueur_m();
        assert!((trace.longueur() - attendu).abs() < attendu * 0.5, "trace {} vs troncons {attendu}", trace.longueur());
    }

    /// Un bâtiment carré, construit en mètres locaux puis reconverti en
    /// lon/lat — l'inverse de ce que `GrilleBatiments` applique en interne.
    fn contour_carre(repere: &Repere, id: i64, centre_m: [f64; 2], cote: f64) -> rusty_music_osm::Contour {
        let r = cote / 2.0;
        let anneau_m = [
            [centre_m[0] - r, centre_m[1] - r],
            [centre_m[0] + r, centre_m[1] - r],
            [centre_m[0] + r, centre_m[1] + r],
            [centre_m[0] - r, centre_m[1] + r],
            [centre_m[0] - r, centre_m[1] - r],
        ];
        rusty_music_osm::Contour { id, points: anneau_m.iter().map(|p| repere.depuis_m(*p)).collect() }
    }

    #[test]
    fn loger_dans_batiments_prend_le_batiment_le_plus_proche_de_la_cible() {
        let troncon = Troncon {
            id: 1,
            nom: Some("Rue Test".into()),
            classe: Classe::Residentielle,
            points: vec![[2.30, 48.85], [2.31, 48.85]],
        };
        let mut extrait = Extrait { troncons: vec![troncon], ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let traces = traces_des_rues(&extrait, &repere);
        let trace = &traces["Rue Test"];
        let debut = trace.au(0.0).0;
        let milieu = trace.au(trace.longueur() / 2.0).0;

        // Un petit bâtiment (16 m²) près du début, un grand (400 m²) près du
        // milieu — la taille ne décide plus, la cible du morceau si.
        extrait.batis.push(contour_carre(&repere, 1, [debut[0], debut[1] + 10.0], 4.0));
        extrait.batis.push(contour_carre(&repere, 2, [milieu[0], milieu[1] + 10.0], 20.0));

        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let logement = Logement { rues: vec!["Rue Test".into()], capacite: 100 };

        // Cible près du début : le petit bâtiment, pas le grand.
        let mut pris = HashSet::new();
        let pres_du_debut = loger_dans_batiments(&[(42, debut)], &logement, &[], &traces, &grille, &mut pris, 10.0);
        assert_eq!(pres_du_debut.len(), 1);
        assert_eq!(pres_du_debut[0].batiment_id, 1, "le bâtiment le plus proche de la cible, pas le plus grand");
        assert!(!pres_du_debut[0].repli_quartier);
        assert!(!pres_du_debut[0].hors_zone);

        // Cible près du milieu : le grand bâtiment, parce qu'il est là, pas
        // parce qu'il est grand.
        let mut pris = HashSet::new();
        let pres_du_milieu = loger_dans_batiments(&[(7, milieu)], &logement, &[], &traces, &grille, &mut pris, 10.0);
        assert_eq!(pres_du_milieu[0].batiment_id, 2);
    }

    #[test]
    fn loger_dans_batiments_suit_la_cible_pas_lordre_des_pistes() {
        let troncon = Troncon {
            id: 1,
            nom: Some("Rue Test".into()),
            classe: Classe::Residentielle,
            points: vec![[2.30, 48.85], [2.32, 48.85]],
        };
        let mut extrait = Extrait { troncons: vec![troncon], ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let traces = traces_des_rues(&extrait, &repere);
        let trace = &traces["Rue Test"];
        let debut = trace.au(0.0).0;
        let fin = trace.au(trace.longueur()).0;

        extrait.batis.push(contour_carre(&repere, 1, [debut[0], debut[1] + 10.0], 10.0));
        extrait.batis.push(contour_carre(&repere, 2, [fin[0], fin[1] + 10.0], 10.0));

        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let logement = Logement { rues: vec!["Rue Test".into()], capacite: 100 };
        let mut pris = HashSet::new();

        // Le morceau donné en premier vise la fin de la rue, le second vise le
        // début — l'ordre des pistes ne doit plus rien décider.
        let adresses = loger_dans_batiments(
            &[(10, fin), (20, debut)],
            &logement,
            &[],
            &traces,
            &grille,
            &mut pris,
            10.0,
        );
        let dix = adresses.iter().find(|a| a.track_id == 10).unwrap();
        let vingt = adresses.iter().find(|a| a.track_id == 20).unwrap();
        assert_eq!(dix.batiment_id, 2, "le morceau visant la fin habite le bâtiment de la fin");
        assert_eq!(vingt.batiment_id, 1, "celui visant le début habite le bâtiment du début");
    }

    #[test]
    fn loger_dans_batiments_deborde_sur_la_rue_suivante_quand_les_batiments_manquent() {
        let troncons = vec![
            Troncon { id: 1, nom: Some("Courte".into()), classe: Classe::Residentielle, points: vec![[2.30, 48.85], [2.3003, 48.85]] },
            Troncon { id: 2, nom: Some("Longue".into()), classe: Classe::Residentielle, points: vec![[2.31, 48.85], [2.35, 48.85]] },
        ];
        let mut extrait = Extrait { troncons, ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let traces = traces_des_rues(&extrait, &repere);

        // Un seul bâtiment logeable près de « Courte ».
        let m_courte = traces["Courte"].au(0.0).0;
        extrait.batis.push(contour_carre(&repere, 100, [m_courte[0], m_courte[1] + 10.0], 10.0));

        // Trois bâtiments espacés le long de « Longue ».
        let longue = traces["Longue"].longueur();
        for (n, s) in [0.0, longue / 2.0, longue].into_iter().enumerate() {
            let (pos, _) = traces["Longue"].au(s);
            extrait.batis.push(contour_carre(&repere, 200 + n as i64, [pos[0], pos[1] + 10.0], 10.0));
        }

        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let logement = Logement { rues: vec!["Courte".into(), "Longue".into()], capacite: 100 };
        // Quatre morceaux qui visent tous le début de « Courte » : un seul
        // bâtiment y loge, les trois autres débordent sur « Longue ».
        let pistes: Vec<(i64, [f64; 2])> = (0..4).map(|n| (n, m_courte)).collect();
        let mut pris = HashSet::new();

        let adresses = loger_dans_batiments(&pistes, &logement, &[], &traces, &grille, &mut pris, 10.0);
        assert_eq!(adresses.len(), 4, "les quatre bâtiments disponibles doivent tous être pris");
        assert_eq!(adresses.iter().filter(|a| a.rue == "Courte").count(), 1, "un seul bâtiment logeait près de Courte");
        assert_eq!(adresses.iter().filter(|a| a.rue == "Longue").count(), 3, "les morceaux restants débordent sur Longue");
        assert!(adresses.iter().all(|a| !a.repli_quartier), "tout devait tenir dans les rues assignées à l'artiste, sans repli");
        assert!(adresses.iter().all(|a| !a.hors_zone), "tout devait tenir dans les rues assignées");
    }

    #[test]
    fn loger_dans_batiments_ne_reattribue_jamais_un_batiment_deja_pris() {
        let troncon = Troncon {
            id: 1,
            nom: Some("Rue Test".into()),
            classe: Classe::Residentielle,
            points: vec![[2.30, 48.85], [2.31, 48.85]],
        };
        let mut extrait = Extrait { troncons: vec![troncon], ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let traces = traces_des_rues(&extrait, &repere);
        let m = traces["Rue Test"].au(0.0).0;

        // Un seul bâtiment près de la rue.
        extrait.batis.push(contour_carre(&repere, 1, [m[0], m[1] + 10.0], 10.0));
        // Un second, loin de toute rue assignée — seul le repli hors zone
        // doit pouvoir le trouver.
        extrait.batis.push(contour_carre(&repere, 2, [m[0] + 5000.0, m[1] + 5000.0], 10.0));

        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let logement = Logement { rues: vec!["Rue Test".into()], capacite: 100 };
        let mut pris = HashSet::new();

        let a = loger_dans_batiments(&[(1, m)], &logement, &[], &traces, &grille, &mut pris, 10.0);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].batiment_id, 1);
        assert!(!a[0].hors_zone);

        // Un second artiste, sur la même rue, avec le même `pris` partagé —
        // exactement comme `ville::rassembler` le fait entre deux appels.
        // Quartier vide : rien à essayer au deuxième cercle, direct au repli
        // hors zone.
        let b = loger_dans_batiments(&[(2, m)], &logement, &[], &traces, &grille, &mut pris, 10.0);
        assert_eq!(b.len(), 1, "le second artiste doit quand même être logé, ailleurs");
        assert_eq!(b[0].batiment_id, 2, "jamais le même bâtiment que le premier artiste");
        assert!(!b[0].repli_quartier, "aucune rue de quartier fournie, ce cercle ne peut pas avoir joué");
        assert!(b[0].hors_zone, "le seul bâtiment restant est hors de la zone assignée");
    }

    #[test]
    fn loger_dans_batiments_replie_dabord_sur_le_quartier_avant_paris_entier() {
        let troncons = vec![
            Troncon { id: 1, nom: Some("Rue Artiste".into()), classe: Classe::Residentielle, points: vec![[2.30, 48.85], [2.3003, 48.85]] },
            Troncon { id: 2, nom: Some("Rue Voisine".into()), classe: Classe::Residentielle, points: vec![[2.31, 48.85], [2.3103, 48.85]] },
        ];
        let mut extrait = Extrait { troncons, ..Default::default() };
        let repere = Repere::centre_de(&extrait);
        let traces = traces_des_rues(&extrait, &repere);

        // Aucun bâtiment sur la rue de l'artiste — un seul, sur une autre
        // rue du même quartier.
        let m = traces["Rue Voisine"].au(0.0).0;
        extrait.batis.push(contour_carre(&repere, 1, [m[0], m[1] + 10.0], 10.0));
        // Et un second, très loin, hors de tout quartier — seul le repli
        // final « Paris entier » doit pouvoir le trouver.
        extrait.batis.push(contour_carre(&repere, 2, [m[0] + 5000.0, m[1] + 5000.0], 10.0));

        let grille = GrilleBatiments::nouvelle(&extrait, &repere);
        let logement = Logement { rues: vec!["Rue Artiste".into()], capacite: 100 };
        let quartier_rues = vec!["Rue Artiste".into(), "Rue Voisine".into()];
        let mut pris = HashSet::new();

        let cible = traces["Rue Artiste"].au(0.0).0;
        let adresses = loger_dans_batiments(&[(1, cible), (2, cible)], &logement, &quartier_rues, &traces, &grille, &mut pris, 10.0);
        assert_eq!(adresses.len(), 2);

        let premier = adresses.iter().find(|a| a.track_id == 1).unwrap();
        assert_eq!(premier.batiment_id, 1, "le premier morceau prend le bâtiment du quartier, pas celui hors zone");
        assert!(premier.repli_quartier, "logé sur une rue du quartier, pas sur celle de l'artiste");
        assert!(!premier.hors_zone);

        let second = adresses.iter().find(|a| a.track_id == 2).unwrap();
        assert_eq!(second.batiment_id, 2, "le quartier est épuisé, dernier recours Paris entier");
        assert!(second.hors_zone);
    }
}
