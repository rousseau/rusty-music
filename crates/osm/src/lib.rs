// SPDX-License-Identifier: GPL-3.0-or-later
//! Import d'un extrait OpenStreetMap.
//!
//! La carte ne fabrique plus son monde : elle emprunte celui d'une vraie ville.
//! Ce crate lit un `.osm.pbf`, en retient ce qui fait un plan lisible — rues,
//! adresses, bâti, eau, espaces verts — et rend le tout en mémoire. La
//! persistance et le rendu sont l'affaire d'autres crates ; celui-ci ne sait
//! que lire.
//!
//! Il est **volontairement hors du chemin de l'application** : `osmpbf` ne sert
//! qu'une fois, à l'import, et n'a rien à faire dans le binaire du bureau.
//!
//! Les données OSM sont sous **ODbL** : leur réutilisation impose d'attribuer
//! « © les contributeurs OpenStreetMap » et de partager à l'identique toute
//! base dérivée. Voir `docs/carto-ville.md`.

pub mod base;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use osmpbf::{Element, ElementReader};

/// Rectangle géographique, en degrés.
#[derive(Clone, Copy, Debug)]
pub struct Bornes {
    pub ouest: f64,
    pub sud: f64,
    pub est: f64,
    pub nord: f64,
}

impl Bornes {
    pub fn contient(&self, lon: f64, lat: f64) -> bool {
        lon >= self.ouest && lon <= self.est && lat >= self.sud && lat <= self.nord
    }

    /// Élargit les bornes de `d` degrés dans les quatre directions.
    ///
    /// Sert à la collecte des nœuds : une rue qui sort du cadre garde des
    /// sommets au-delà, et sans marge elle serait rejetée faute de pouvoir
    /// résoudre sa géométrie — le périphérique disparaîtrait par endroits.
    pub fn elargies(&self, d: f64) -> Bornes {
        Bornes {
            ouest: self.ouest - d,
            sud: self.sud - d,
            est: self.est + d,
            nord: self.nord + d,
        }
    }
}

/// Paris intra-muros, bois de Boulogne et de Vincennes compris — ils font
/// administrativement partie de la ville, et ce sont ses deux grands poumons
/// verts : les retrancher donnerait une carte amputée.
pub const PARIS: Bornes = Bornes {
    ouest: 2.2241,
    sud: 48.8156,
    est: 2.4699,
    nord: 48.9022,
};

/// Hiérarchie routière telle qu'OSM la déclare.
///
/// Elle n'est pas décorative : c'est elle qui donnera l'épaisseur du trait, et
/// c'est l'épaisseur des routes qui fait qu'une carte se lit d'un coup d'œil.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Classe {
    /// Périphérique et voies rapides.
    Autoroute,
    /// Grands axes : boulevards des Maréchaux, grandes avenues.
    Primaire,
    Secondaire,
    Tertiaire,
    /// La rue ordinaire — le gros du tissu, et le gros de nos adresses.
    Residentielle,
    /// Piéton : quais, passages, escaliers, allées de parc.
    Pietonne,
    /// Desserte, contre-allée, voie de service.
    Service,
}

impl Classe {
    fn depuis(valeur: &str) -> Option<Classe> {
        Some(match valeur {
            "motorway" | "motorway_link" | "trunk" | "trunk_link" => Classe::Autoroute,
            "primary" | "primary_link" => Classe::Primaire,
            "secondary" | "secondary_link" => Classe::Secondaire,
            "tertiary" | "tertiary_link" => Classe::Tertiaire,
            "residential" | "living_street" | "unclassified" => Classe::Residentielle,
            "pedestrian" | "footway" | "path" | "steps" | "cycleway" => Classe::Pietonne,
            "service" => Classe::Service,
            _ => return None,
        })
    }

    /// Réciproque de [`Classe::nom`], pour la relecture depuis la base.
    pub fn depuis_nom(nom: &str) -> Option<Classe> {
        Some(match nom {
            "autoroute" => Classe::Autoroute,
            "primaire" => Classe::Primaire,
            "secondaire" => Classe::Secondaire,
            "tertiaire" => Classe::Tertiaire,
            "residentielle" => Classe::Residentielle,
            "pietonne" => Classe::Pietonne,
            "service" => Classe::Service,
            _ => return None,
        })
    }

    pub fn nom(self) -> &'static str {
        match self {
            Classe::Autoroute => "autoroute",
            Classe::Primaire => "primaire",
            Classe::Secondaire => "secondaire",
            Classe::Tertiaire => "tertiaire",
            Classe::Residentielle => "residentielle",
            Classe::Pietonne => "pietonne",
            Classe::Service => "service",
        }
    }
}

/// Une voie, telle qu'OSM la découpe.
///
/// Attention : OSM tronçonne une avenue en autant de segments que ses attributs
/// changent (sens unique, revêtement, numéro de bus). Une « rue » ici est donc
/// un *tronçon*, pas une rue au sens de la poste. Le regroupement par nom est
/// fait plus tard, à l'affectation.
#[derive(Clone, Debug)]
pub struct Troncon {
    pub id: i64,
    pub nom: Option<String>,
    pub classe: Classe,
    /// Sommets en `[lon, lat]`, degrés.
    pub points: Vec<[f64; 2]>,
}

impl Troncon {
    /// Longueur en mètres, par approximation équirectangulaire.
    ///
    /// À l'échelle d'une ville, l'erreur est très inférieure à celle du tracé
    /// lui-même : inutile de sortir la formule de Vincenty pour mesurer une rue.
    pub fn longueur_m(&self) -> f64 {
        let mut total = 0.0;
        for paire in self.points.windows(2) {
            total += distance_m(paire[0], paire[1]);
        }
        total
    }
}

/// Un point adressable : c'est là qu'un morceau viendra habiter.
#[derive(Clone, Debug)]
pub struct Adresse {
    pub numero: String,
    pub rue: Option<String>,
    pub point: [f64; 2],
}

/// Un contour fermé : bâtiment, plan d'eau, pelouse.
#[derive(Clone, Debug)]
pub struct Contour {
    pub id: i64,
    pub points: Vec<[f64; 2]>,
}

/// Un toponyme ponctuel d'OSM (`place=*`) : quartier, faubourg, lieu-dit.
#[derive(Clone, Debug)]
pub struct Lieu {
    pub nom: String,
    pub genre: String,
    pub point: [f64; 2],
}

/// Un repère réel notable — musée, monument, lieu de culte — retenu pour
/// servir d'ancre visuelle sur la carte, indépendamment des morceaux.
///
/// Distinct de [`Lieu`] : vocabulaire de tags différent (`tourism`/
/// `historic`/`amenity`, pas `place`), et intention différente — un `Lieu`
/// n'aide qu'à poser les quartiers et n'est jamais affiché, un
/// `PointRemarquable` **est** affiché.
#[derive(Clone, Debug)]
pub struct PointRemarquable {
    pub id: i64,
    pub nom: String,
    /// Le genre retenu par [`genre_repere`] : `"attraction"`, `"musee"`,
    /// `"point_de_vue"`, `"oeuvre"`, `"monument"`, `"memorial"`,
    /// `"chateau"`, `"site_archeologique"` ou `"lieu_de_culte"`.
    pub genre: String,
    pub point: [f64; 2],
}

/// Le genre d'un repère notable, depuis ses tags `tourism`/`historic`/
/// `amenity` — `None` si aucun ne correspond au petit jeu retenu ici.
///
/// Volontairement étroit : le but est de poser quelques dizaines d'ancres
/// reconnaissables (tour Eiffel, musées, lieux de culte majeurs), pas de
/// cataloguer tout ce qu'OSM tague comme point d'intérêt.
fn genre_repere(tourism: Option<&str>, historic: Option<&str>, amenity: Option<&str>) -> Option<&'static str> {
    match tourism {
        Some("attraction") => return Some("attraction"),
        Some("museum") => return Some("musee"),
        Some("viewpoint") => return Some("point_de_vue"),
        Some("artwork") => return Some("oeuvre"),
        _ => {}
    }
    match historic {
        Some("monument") => return Some("monument"),
        Some("memorial") => return Some("memorial"),
        Some("castle") => return Some("chateau"),
        Some("archaeological_site") => return Some("site_archeologique"),
        _ => {}
    }
    if amenity == Some("place_of_worship") {
        return Some("lieu_de_culte");
    }
    None
}


/// La limite administrative d'une commune, en anneaux fermés.
///
/// Un rectangle n'est pas Paris. Le cadre englobant attrapait Boulogne, Ivry et
/// Saint-Denis — « Rue de Paris », « Autoroute de l'Est » — et surtout il
/// perdait la seule chose qui rend un plan reconnaissable au premier coup
/// d'œil : **la silhouette**. Paris a la sienne, escargot flanqué de ses deux
/// bois. On la découpe donc pour de vrai.
pub struct Frontiere {
    pub anneaux: Vec<Vec<[f64; 2]>>,
    /// Segments rangés par bande de latitude. Sans cet index, tester les
    /// 400 000 objets de l'extrait contre les milliers de segments du contour
    /// coûterait le milliard d'opérations ; avec, une poignée par test.
    bandes: Vec<Vec<[f64; 4]>>,
    sud: f64,
    nord: f64,
    ouest: f64,
    est: f64,
}

const BANDES: usize = 256;

impl Frontiere {
    /// Construit l'index spatial à partir des anneaux fermés du contour.
    pub fn nouvelle(anneaux: Vec<Vec<[f64; 2]>>) -> Frontiere {
        let (mut sud, mut nord) = (f64::MAX, f64::MIN);
        let (mut ouest, mut est) = (f64::MAX, f64::MIN);
        for anneau in &anneaux {
            for p in anneau {
                ouest = ouest.min(p[0]);
                est = est.max(p[0]);
                sud = sud.min(p[1]);
                nord = nord.max(p[1]);
            }
        }
        let mut bandes = vec![Vec::new(); BANDES];
        let hauteur = (nord - sud).max(f64::EPSILON);
        for anneau in &anneaux {
            for paire in anneau.windows(2) {
                let (a, b) = (paire[0], paire[1]);
                let bas = a[1].min(b[1]);
                let haut = a[1].max(b[1]);
                let i0 = (((bas - sud) / hauteur) * BANDES as f64) as usize;
                let i1 = (((haut - sud) / hauteur) * BANDES as f64) as usize;
                for bande in bandes.iter_mut().take(i1.min(BANDES - 1) + 1).skip(i0.min(BANDES - 1)) {
                    bande.push([a[0], a[1], b[0], b[1]]);
                }
            }
        }
        Frontiere { anneaux, bandes, sud, nord, ouest, est }
    }

    /// Test de parité par lancer de rayon vers l'est.
    ///
    /// La règle pair-impair traite les trous sans code supplémentaire : un point
    /// dans une enclave traverse deux fois, donc il est dehors.
    pub fn contient(&self, p: [f64; 2]) -> bool {
        if p[0] < self.ouest || p[0] > self.est || p[1] < self.sud || p[1] > self.nord {
            return false;
        }
        let hauteur = (self.nord - self.sud).max(f64::EPSILON);
        let i = ((((p[1] - self.sud) / hauteur) * BANDES as f64) as usize).min(BANDES - 1);
        let mut dedans = false;
        for s in &self.bandes[i] {
            let (x1, y1, x2, y2) = (s[0], s[1], s[2], s[3]);
            if (y1 > p[1]) != (y2 > p[1]) {
                let x = x1 + (p[1] - y1) / (y2 - y1) * (x2 - x1);
                if x > p[0] {
                    dedans = !dedans;
                }
            }
        }
        dedans
    }
}

/// Tout ce qu'on retient de la ville.
#[derive(Default)]
pub struct Extrait {
    pub troncons: Vec<Troncon>,
    pub adresses: Vec<Adresse>,
    pub batis: Vec<Contour>,
    pub eaux: Vec<Contour>,
    pub verts: Vec<Contour>,
    pub lieux: Vec<Lieu>,
    pub points_remarquables: Vec<PointRemarquable>,
    /// La limite communale, si elle a été demandée et trouvée.
    pub frontiere: Option<Frontiere>,
}

/// Distance en mètres entre deux points `[lon, lat]`.
fn distance_m(a: [f64; 2], b: [f64; 2]) -> f64 {
    const RAYON: f64 = 6_371_000.0;
    let lat_moy = (a[1] + b[1]).to_radians() / 2.0;
    let dx = (b[0] - a[0]).to_radians() * lat_moy.cos() * RAYON;
    let dy = (b[1] - a[1]).to_radians() * RAYON;
    (dx * dx + dy * dy).sqrt()
}

fn centre(points: &[[f64; 2]]) -> [f64; 2] {
    let n = points.len().max(1) as f64;
    let (sx, sy) = points
        .iter()
        .fold((0.0, 0.0), |(sx, sy), p| (sx + p[0], sy + p[1]));
    [sx / n, sy / n]
}

/// Coordonnées stockées en dix-millionièmes de degré — l'unité native d'OSM.
///
/// Huit octets par nœud au lieu de seize : sur les quelque deux millions de
/// nœuds parisiens, cela fait la différence entre une table qui tient au chaud
/// et une qui déborde.
type Coord = [i32; 2];

fn vers_degres(c: Coord) -> [f64; 2] {
    [c[0] as f64 / 1e7, c[1] as f64 / 1e7]
}


/// Recoud des tronçons de frontière en anneaux fermés.
///
/// Le raccord se fait sur les **identifiants de nœuds**, pas sur les
/// coordonnées : OSM partage le même nœud entre deux tronçons voisins, l'égalité
/// est donc exacte. Comparer des flottants introduirait ici des trous invisibles.
fn assembler(mut restants: Vec<Vec<i64>>) -> Vec<Vec<i64>> {
    let mut anneaux = Vec::new();
    while let Some(mut courant) = restants.pop() {
        loop {
            if courant.len() > 3 && courant.first() == courant.last() {
                break;
            }
            let fin = *courant.last().expect("tronçon non vide");
            let trouve = restants.iter().position(|m| *m.first().unwrap() == fin).map(|i| (i, false))
                .or_else(|| restants.iter().position(|m| *m.last().unwrap() == fin).map(|i| (i, true)));
            let Some((i, inverser)) = trouve else { break };
            let mut m = restants.swap_remove(i);
            if inverser {
                m.reverse();
            }
            courant.extend(m.into_iter().skip(1));
        }
        if courant.len() > 3 {
            anneaux.push(courant);
        }
    }
    anneaux
}

/// Trottoirs et passages piétons : à écarter.
///
/// Ils comptent pour l'essentiel des 142 000 tronçons piétons de l'extrait, et
/// les dessiner double chaque rue d'un liseré parasite. Ce ne sont pas des rues,
/// ce sont les bords des rues.
fn est_trottoir(tags: &HashMap<&str, &str>) -> bool {
    matches!(tags.get("footway").copied(), Some("sidewalk") | Some("crossing"))
        || matches!(tags.get("highway").copied(), Some("crossing") | Some("elevator"))
}

/// Lit l'extrait et n'en garde que ce qui tombe dans `bornes`.
///
/// Deux passes, et l'ordre compte. Un `.osm.pbf` référence les sommets d'une
/// voie par identifiant, sans coordonnées : impossible de filtrer les voies
/// géographiquement avant de connaître les nœuds. On collecte donc d'abord les
/// nœuds du cadre — ce qui borne la mémoire à la ville, non au fichier — puis
/// on résout les voies contre eux.
pub fn extraire(chemin: &Path, bornes: Bornes, commune: Option<&str>) -> Result<Extrait> {
    // Le **halo** : la voirie, l'eau et les espaces verts sont retenus jusqu'à
    // ~2 km au-delà du cadre (petite couronne), pas seulement dans la commune.
    // Ce halo n'est jamais habité — il ne sert qu'à donner au fondu de bordure
    // de la carte de la matière à dissoudre plutôt que de s'arrêter net sur le
    // périphérique (cf. `docs/carto-ville.md`, révisé).
    let halo = bornes.elargies(0.02);
    // Les nœuds, un cran plus large encore : une rue du halo garde des sommets
    // au-delà, et sans marge elle serait rejetée faute de pouvoir résoudre sa
    // géométrie.
    let marge = halo.elargies(0.025);
    let mut extrait = Extrait::default();

    // ---- Passe 1 : les nœuds du cadre, les adresses, les toponymes, et les
    //      tronçons que la relation de frontière déclare comme siens.
    let mut coords: HashMap<i64, Coord> = HashMap::new();
    let mut voies_frontiere: HashSet<i64> = HashSet::new();
    let lecteur = ElementReader::from_path(chemin)
        .with_context(|| format!("ouverture de {}", chemin.display()))?;
    lecteur.for_each(|element| {
        match element {
            Element::Relation(relation) => {
                let Some(cherchee) = commune else { return };
                let tags: HashMap<&str, &str> = relation.tags().collect();
                let est_la_commune = tags.get("boundary") == Some(&"administrative")
                    && tags.get("admin_level") == Some(&"8")
                    && tags.get("name") == Some(&cherchee);
                if !est_la_commune {
                    return;
                }
                for membre in relation.members() {
                    if membre.member_type == osmpbf::RelMemberType::Way
                        && matches!(membre.role(), Ok("outer") | Ok("inner") | Ok(""))
                    {
                        voies_frontiere.insert(membre.member_id);
                    }
                }
            }
            Element::Node(_) | Element::DenseNode(_) => {
                let (id, lon, lat, tags): (i64, f64, f64, Vec<(String, String)>) = match element {
                    Element::Node(n) => (
                        n.id(),
                        n.lon(),
                        n.lat(),
                        n.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    ),
                    Element::DenseNode(n) => (
                        n.id(),
                        n.lon(),
                        n.lat(),
                        n.tags().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                    ),
                    _ => unreachable!("filtré par le bras de match"),
                };
                if !marge.contient(lon, lat) {
                    return;
                }
                coords.insert(id, [(lon * 1e7) as i32, (lat * 1e7) as i32]);
                if !bornes.contient(lon, lat) {
                    return;
                }
                let cherche = |cle: &str| {
                    tags.iter().find(|(k, _)| k == cle).map(|(_, v)| v.clone())
                };
                if let Some(numero) = cherche("addr:housenumber") {
                    extrait.adresses.push(Adresse {
                        numero,
                        rue: cherche("addr:street"),
                        point: [lon, lat],
                    });
                }
                if let (Some(genre), Some(nom)) = (cherche("place"), cherche("name")) {
                    extrait.lieux.push(Lieu { nom, genre, point: [lon, lat] });
                }
                if let Some(nom) = cherche("name") {
                    if let Some(genre) = genre_repere(
                        cherche("tourism").as_deref(),
                        cherche("historic").as_deref(),
                        cherche("amenity").as_deref(),
                    ) {
                        extrait.points_remarquables.push(PointRemarquable {
                            id,
                            nom,
                            genre: genre.to_string(),
                            point: [lon, lat],
                        });
                    }
                }
            }
            Element::Way(_) => {}
        }
    })?;

    // ---- Passe 2 : les voies, résolues contre les nœuds retenus.
    let mut morceaux_frontiere: Vec<Vec<i64>> = Vec::new();
    let lecteur = ElementReader::from_path(chemin)?;
    lecteur.for_each(|element| {
        let Element::Way(voie) = element else { return };

        // La frontière d'abord : ses tronçons ne portent aucun tag qui les
        // ferait retenir autrement.
        if voies_frontiere.contains(&voie.id()) {
            let refs: Vec<i64> = voie.refs().collect();
            if refs.len() >= 2 {
                morceaux_frontiere.push(refs);
            }
            return;
        }

        let tags: HashMap<&str, &str> = voie.tags().collect();
        let genre_way = genre_repere(tags.get("tourism").copied(), tags.get("historic").copied(), tags.get("amenity").copied());
        let est_repere = genre_way.is_some() && tags.contains_key("name");
        let interessante = tags.contains_key("highway")
            || tags.contains_key("building")
            || tags.get("natural").is_some_and(|v| *v == "water")
            || tags.contains_key("waterway")
            || tags.contains_key("leisure")
            || tags.contains_key("landuse")
            || est_repere;
        if !interessante || est_trottoir(&tags) {
            return;
        }

        // Une voie dont un seul sommet manque est rejetée : mieux vaut une rue
        // absente qu'une rue qui coupe à travers champs.
        let mut points = Vec::new();
        for reference in voie.refs() {
            match coords.get(&reference) {
                Some(c) => points.push(vers_degres(*c)),
                None => return,
            }
        }
        if points.len() < 2 || !points.iter().any(|p| halo.contient(p[0], p[1])) {
            return;
        }

        if let Some(classe) = tags.get("highway").and_then(|v| Classe::depuis(v)) {
            extrait.troncons.push(Troncon {
                id: voie.id(),
                nom: tags.get("name").map(|v| v.to_string()),
                classe,
                points,
            });
            return;
        }

        // Les surfaces doivent être fermées pour être remplies.
        if points.first() != points.last() || points.len() < 4 {
            return;
        }
        let contour = Contour { id: voie.id(), points };
        // Indépendant du bras suivant : un repère peut aussi être un
        // bâtiment (Notre-Dame porte `building=yes` *et* `historic=*`) — les
        // deux doivent survivre. Seuls les repères en anneau fermé arrivent
        // ici (le filtre ci-dessus l'exige) ; un repère ouvert (une ligne,
        // rare pour ce jeu de tags) est silencieusement perdu, comme
        // n'importe quelle autre surface non fermée.
        if est_repere {
            extrait.points_remarquables.push(PointRemarquable {
                id: voie.id(),
                nom: tags.get("name").expect("est_repere garantit un nom").to_string(),
                genre: genre_way.expect("est_repere garantit un genre").to_string(),
                point: centre(&contour.points),
            });
        }
        if tags.contains_key("building") {
            if let Some(numero) = tags.get("addr:housenumber") {
                extrait.adresses.push(Adresse {
                    numero: numero.to_string(),
                    rue: tags.get("addr:street").map(|v| v.to_string()),
                    point: centre(&contour.points),
                });
            }
            extrait.batis.push(contour);
        } else if tags.get("natural").is_some_and(|v| *v == "water")
            || tags
                .get("waterway")
                .is_some_and(|v| matches!(*v, "riverbank" | "dock" | "canal"))
        {
            extrait.eaux.push(contour);
        } else if tags
            .get("leisure")
            .is_some_and(|v| matches!(*v, "park" | "garden" | "pitch" | "playground"))
            || tags.get("landuse").is_some_and(|v| {
                matches!(*v, "grass" | "forest" | "cemetery" | "village_green" | "meadow")
            })
        {
            extrait.verts.push(contour);
        }
    })?;

    // ---- La frontière, recousue puis appliquée.
    if !morceaux_frontiere.is_empty() {
        let anneaux: Vec<Vec<[f64; 2]>> = assembler(morceaux_frontiere)
            .into_iter()
            .map(|ids| {
                ids.iter()
                    .filter_map(|i| coords.get(i).copied().map(vers_degres))
                    .collect::<Vec<_>>()
            })
            .filter(|a| a.len() > 3)
            .collect();
        if !anneaux.is_empty() {
            extrait.frontiere = Some(Frontiere::nouvelle(anneaux));
        }
    }
    if let Some(frontiere) = &extrait.frontiere {
        // **La voirie, l'eau et les espaces verts ne sont pas découpés sur la
        // frontière** : le halo de la petite couronne (jusqu'à ~2 km, voir
        // `halo`) reste, pour que le fondu de bordure de la carte ait de la
        // matière à dissoudre. Ce halo n'est jamais habité : `ville::rassembler`
        // et `affectation::rassembler_rues` écartent de l'assignation toute rue
        // dont l'essentiel du tracé est hors commune.
        //
        // Le bâti, les adresses et les repères restent, eux, bornés à la
        // commune — une couronne de bâti alourdirait les tuiles sans rien
        // apporter au rendu de fond.
        extrait.adresses.retain(|a| frontiere.contient(a.point));
        extrait.lieux.retain(|l| frontiere.contient(l.point));
        extrait.points_remarquables.retain(|p| frontiere.contient(p.point));
        extrait.batis.retain(|c| frontiere.contient(centre(&c.points)));
    }

    Ok(extrait)
}

/// Ce que l'extrait contient, en chiffres.
pub struct Resume {
    pub troncons: usize,
    pub troncons_nommes: usize,
    pub rues_distinctes: usize,
    pub longueur_km: f64,
    pub longueur_nommee_km: f64,
    pub par_classe: Vec<(Classe, usize, f64)>,
    pub adresses: usize,
    pub batis: usize,
    pub eaux: usize,
    pub verts: usize,
    pub lieux: usize,
    pub points_remarquables: usize,
}

impl Extrait {
    /// Regroupe les tronçons par nom : c'est l'unité qui nous intéresse.
    ///
    /// OSM découpe l'avenue des Champs-Élysées en une trentaine de tronçons ;
    /// pour nous, c'est **une** rue, donc un artiste.
    pub fn rues_par_nom(&self) -> HashMap<&str, Vec<&Troncon>> {
        let mut par_nom: HashMap<&str, Vec<&Troncon>> = HashMap::new();
        for troncon in &self.troncons {
            if let Some(nom) = troncon.nom.as_deref() {
                par_nom.entry(nom).or_default().push(troncon);
            }
        }
        par_nom
    }

    /// Le rectangle englobant de l'extrait, en degrés : la frontière communale
    /// si elle a été résolue, sinon les tronçons — la même règle que
    /// [`base::ecrire`] applique pour remplir la table `ville`.
    ///
    /// Rend `(ouest, sud, est, nord)`. `None` si l'extrait est vide.
    pub fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let (mut ouest, mut sud, mut est, mut nord) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut voir = |p: &[f64; 2]| {
            ouest = ouest.min(p[0]);
            est = est.max(p[0]);
            sud = sud.min(p[1]);
            nord = nord.max(p[1]);
        };
        if let Some(frontiere) = &self.frontiere {
            for anneau in &frontiere.anneaux {
                anneau.iter().for_each(&mut voir);
            }
        } else {
            for t in &self.troncons {
                t.points.iter().for_each(&mut voir);
            }
        }
        (ouest <= est).then_some((ouest, sud, est, nord))
    }

    pub fn resume(&self) -> Resume {
        let mut par_classe: HashMap<Classe, (usize, f64)> = HashMap::new();
        let mut longueur = 0.0;
        let mut longueur_nommee = 0.0;
        let mut nommes = 0;
        for troncon in &self.troncons {
            let m = troncon.longueur_m();
            longueur += m;
            if troncon.nom.is_some() {
                nommes += 1;
                longueur_nommee += m;
            }
            let entree = par_classe.entry(troncon.classe).or_insert((0, 0.0));
            entree.0 += 1;
            entree.1 += m;
        }
        let mut par_classe: Vec<_> = par_classe
            .into_iter()
            .map(|(c, (n, m))| (c, n, m / 1000.0))
            .collect();
        par_classe.sort_by_key(|(c, _, _)| *c);

        let noms: HashSet<&str> = self.troncons.iter().filter_map(|t| t.nom.as_deref()).collect();
        Resume {
            troncons: self.troncons.len(),
            troncons_nommes: nommes,
            rues_distinctes: noms.len(),
            longueur_km: longueur / 1000.0,
            longueur_nommee_km: longueur_nommee / 1000.0,
            par_classe,
            adresses: self.adresses.len(),
            batis: self.batis.len(),
            eaux: self.eaux.len(),
            verts: self.verts.len(),
            lieux: self.lieux.len(),
            points_remarquables: self.points_remarquables.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_bornes_de_paris_contiennent_notre_dame_et_les_deux_bois() {
        assert!(PARIS.contient(2.3499, 48.8530), "Notre-Dame");
        assert!(PARIS.contient(2.2500, 48.8600), "bois de Boulogne");
        assert!(PARIS.contient(2.4340, 48.8280), "bois de Vincennes");
        assert!(!PARIS.contient(2.1204, 48.8014), "Versailles est dehors");
    }

    #[test]
    fn la_marge_ne_deplace_pas_le_centre() {
        let large = PARIS.elargies(0.02);
        assert!(large.contient(2.2100, 48.9100), "hors cadre strict, dans la marge");
        assert!(!PARIS.contient(2.2100, 48.9100));
    }

    #[test]
    fn une_distance_parisienne_est_de_lordre_du_kilometre() {
        // Notre-Dame → Arc de Triomphe : 4,6 km à vol d'oiseau.
        let m = distance_m([2.3499, 48.8530], [2.2950, 48.8738]);
        assert!((4500.0..4800.0).contains(&m), "{m} m");
    }

    #[test]
    fn la_hierarchie_sordonne_du_plus_gros_au_plus_petit() {
        assert!(Classe::Autoroute < Classe::Residentielle);
        assert_eq!(Classe::depuis("motorway"), Some(Classe::Autoroute));
        assert_eq!(Classe::depuis("bus_stop"), None);
    }
}
