// SPDX-License-Identifier: GPL-3.0-or-later
//! Construction des tuiles vectorielles et écriture de l'archive PMTiles.
//!
//! Rien ici n'encode le format : `mvt` écrit le protobuf des tuiles, `pmtiles`
//! écrit l'archive. Ce module décide **ce qui entre dans quelle tuile**, ce qui
//! se ramène à trois questions :
//!
//! 1. **quelle couche apparaît à quel zoom** — la révélation par échelle, cœur
//!    du rendu et non détail (voir [`Paliers`]) ;
//! 2. **comment découper les territoires** — un polygone qui traverse
//!    cinquante tuiles ne doit pas être recopié cinquante fois en entier ;
//! 3. **dans quel ordre écrire** — PMTiles range ses tuiles sur une courbe de
//!    Hilbert, et les lire est plus rapide si on les a écrites dans cet ordre.

use std::collections::HashMap;
use std::io::BufWriter;
use std::path::Path;
use std::time::{Duration, Instant};

use mvt::{GeomEncoder, GeomType, Tile};
use pmtiles::{Compression, PmTilesWriter, TileCoord, TileId, TileType};

use crate::projection::{self, ETENDUE_TUILE};
use crate::source::Source;

/// Débordement d'une tuile sur ses voisines, en unités MVT.
///
/// Sans lui, un symbole posé à cheval sur une limite de tuile disparaît d'un
/// côté et réapparaît de l'autre au fil du déplacement — le défaut le plus
/// visible d'un jeu de tuiles mal fabriqué. 256 unités sur 4096 font 1/16 de
/// tuile : de quoi contenir une étiquette de métropole.
const MARGE: f64 = 256.0;

/// Famille fictive des anneaux du littoral — la nappe globale, celle qui
/// n'appartient à aucune famille.
pub const FAMILLE_TERRE: i64 = -100;

/// Famille fictive du bâti des agglomérations.
pub const FAMILLE_BATI: i64 = -101;

/// Familles fictives des surfaces du plan de ville réel — bâti, eau, espaces
/// verts. Distinctes de [`FAMILLE_TERRE`]/[`FAMILLE_BATI`] : le chemin réel et
/// le chemin fictif ne peuplent jamais les deux jeux à la fois, mais les
/// garder séparés évite qu'un style écrit pour l'un s'applique par accident à
/// l'autre.
const FAMILLE_BATIMENT_REEL: i64 = -102;
const FAMILLE_EAU_REELLE: i64 = -103;
const FAMILLE_VERT_REEL: i64 = -104;
/// Aplat de quartier musical sur le plan de ville réel. La famille de la
/// région voyage dans `palier` (comme pour le bâti habité), `famille` ne sert
/// qu'au routage vers la couche `territoires-reels`.
const FAMILLE_TERRITOIRE_REEL: i64 = -105;

/// Zoom d'apparition d'un tronçon réel, par classe OSM.
///
/// **Recalé pour se rapprocher de maptoposter** (qui trace *tout* le réseau
/// d'un coup, sans filtre d'échelle) : les rues résidentielles entrent dès z12,
/// pour que le maillage complet de Paris soit déjà lisible au dézoom — c'est
/// cette texture de mille rues fines qui fait l'aspect « poster ». Seuls
/// l'impasse et la desserte (`Pietonne`/`Service`) attendent le zoom de l'îlot,
/// sinon le centre de Paris vire à la bouillie.
fn classe_reelle_visible_des(classe: rusty_music_osm::Classe) -> u8 {
    use rusty_music_osm::Classe::*;
    match classe {
        Autoroute | Primaire => 10,
        Secondaire | Tertiaire => 11,
        Residentielle => 12,
        Pietonne | Service => 15,
    }
}

/// À quel zoom chaque couche existe.
///
/// C'est la traduction de « loin = continents et genres ; moyen = territoires
/// et artistes ; près = morceaux ». Le style MapLibre reprend exactement ces
/// bornes ([`crate::style`]) : ce qui n'est pas dans la tuile ne peut pas être
/// affiché, et ce qui est affiché sans être dans la tuile ne montre rien.
#[derive(Debug, Clone, Copy)]
pub struct Paliers {
    /// Zoom maximal produit. Au-delà, MapLibre réutilise la dernière tuile
    /// (sur-zoom) : les points gardent leur position, seule la résolution du
    /// découpage cesse d'augmenter.
    pub zoom_max: u8,
    /// Les territoires s'arrêtent tôt : ce sont de grandes formes lisses, et
    /// le sur-zoom les sert sans perte visible. Les produire jusqu'au bout
    /// multiplierait l'archive sans rien ajouter à l'écran.
    pub territoires_jusqu_a: u8,
    /// Les noms de famille s'effacent quand on approche : à l'échelle des
    /// morceaux, « trip hop · downtempo » n'apprend plus rien.
    pub familles_jusqu_a: u8,
    /// Les villes apparaissent à mi-distance.
    pub artistes_des: u8,
    /// Les albums, entre les artistes et les morceaux — plan de ville réel
    /// seulement, `Source.albums` reste vide ailleurs.
    pub albums_des: u8,
    /// Les morceaux ne se révèlent que de près.
    pub morceaux_des: u8,
    /// Classe de route la plus basse à dessiner (0 autoroute … 3 sentier).
    ///
    /// À 1, seules les autoroutes et les nationales entrent dans les tuiles —
    /// 8 030 tronçons sur 261 270. Les secondaires et les sentiers restent
    /// dans le moteur de routage, où ils servent, mais **une carte routière
    /// dessine une hiérarchie, pas chaque voie** : avec eux, chaque morceau
    /// tirait ses douze liens et le résultat était une pelote. Mesuré :
    /// l'archive passait de 17 à 72 Mo et le zoom 9 de 29 000 à 123 000
    /// tuiles.
    pub routes_jusqua: u8,
    /// Pas de la spirale des parcelles — il fixe la taille du bâti. Doit
    /// valoir celui du peuplement, sinon les taches ne couvrent pas les
    /// morceaux qu'elles sont censées contenir.
    pub pas_bati: f32,
    /// Longueur maximale d'un tronçon dessinable, en unités de carte.
    ///
    /// **Une route n'est une route que si elle est locale sur la carte.** Le
    /// réseau relie des morceaux proches *à l'oreille* ; la projection 2D ne
    /// préserve que les voisinages locaux, et un lien sonore parfaitement
    /// justifié peut traverser tout le planisphère. Dessinés tels quels, les
    /// 7 833 nationales faisaient une pelote qui masquait la carte.
    ///
    /// Les tronçons écartés **restent dans le moteur de routage**, où leur
    /// longueur à l'écran n'a aucune importance : c'est le rendu qui a besoin
    /// de cette borne, pas le calcul.
    pub longueur_max_route: f32,
}

impl Default for Paliers {
    fn default() -> Self {
        Self {
            zoom_max: 9,
            territoires_jusqu_a: 6,
            familles_jusqu_a: 7,
            artistes_des: 3,
            albums_des: 5, // sans effet : `Source.albums` est toujours vide ici
            morceaux_des: 6,
            // Les secondaires entrent : depuis que le réseau relie des lieux et
            // non des morceaux, elles ne sont plus 200 000 brins mais quelques
            // milliers de couloirs — et ce sont elles qui donnent au réseau sa
            // continuité entre les villages.
            routes_jusqua: 2,
            pas_bati: 0.0005,
            // **Recalibré depuis que le réseau relie des lieux.** À 0,05, il
            // était réglé pour des routes de morceau à morceau ; deux
            // établissements voisins étant distants d'environ 0,07, il les
            // rejetait toutes et la carte n'avait plus une seule route. 0,20
            // laisse passer une route vers les villes voisines et écarte encore
            // les liens que la projection étire d'un bout du monde à l'autre.
            longueur_max_route: 0.20,
        }
    }
}

impl Paliers {
    /// Paliers pour un vrai plan de ville plutôt que pour le planisphère
    /// fictif : l'échelle n'est plus celle d'un monde entier montré dès le
    /// zoom 0, mais celle d'une ville — le bâti ne se révèle qu'au zoom de
    /// l'îlot. `routes_jusqua`/`longueur_max_route` n'ont pas d'effet sur ce
    /// chemin (réseau sonique vide). `territoires_jusqu_a` **sert** ici : il
    /// borne l'aplat de quartier (`FAMILLE_TERRITOIRE_REEL`), la seule
    /// information de genre visible quand on dézoome pour voir Paris entier.
    ///
    /// **Calibré à l'œil, pas mesuré** — comme [`classe_reelle_visible_des`]
    /// et [`anneau_visible_a`] : à ajuster au premier lancement visuel.
    pub fn ville() -> Self {
        Self {
            zoom_max: 17,
            // L'aplat de quartier tient jusqu'au zoom où les bâtiments
            // individuels colorés (`morceaux_des` = 14) prennent le relais.
            territoires_jusqu_a: 13,
            familles_jusqu_a: 12,
            // **Recalibré** — 13 (calibrage initial) reprenait l'écart de
            // `Paliers::default()` (`artistes_des: 3` sur un `zoom_max` de 9)
            // sans tenir compte de l'étendue réelle : `artistes-point` ne se
            // voyait alors qu'à `artistes_des + 4` = 17, la toute dernière
            // tuile, quasiment jamais atteinte — un artiste ne se distinguait
            // jamais d'un morceau. Paris tient entre les zooms ~10 (la ville
            // entière) et ~17 (la façade) : 11 laisse un palier « artistes
            // principaux » avant que le bâti (`morceaux_des`) ne prenne le
            // relais, avec les rangs (`tuiles::rang_artiste`) qui étagent
            // encore la révélation à l'intérieur de ce palier.
            artistes_des: 11,
            // Entre les artistes et les morceaux : un album regroupe les
            // pistes déjà contiguës le long d'une rue (`ville::rassembler`).
            albums_des: 13,
            // À 15 (calibrage initial), aucun morceau ne se voyait à la vue
            // d'ouverture (zoom 14, voir `style::construire`) ni même à
            // `artistes_des` : la carte semblait vide de toute musique
            // jusqu'à zoomer bien au-delà du quartier. Rapproché
            // d'`artistes_des` pour une révélation progressive, comme sur le
            // monde fictif (`artistes_des: 3, morceaux_des: 6`, un écart de
            // 3 sur 9 zooms ; ici un écart de 1 sur 17, encore à l'œil.
            morceaux_des: 14,
            ..Self::default()
        }
    }
}

/// Ce que la génération a coûté. Mesuré, pas estimé.
#[derive(Debug, Clone)]
pub struct Rapport {
    pub tuiles: usize,
    pub octets: u64,
    pub duree: Duration,
    /// Par niveau de zoom : nombre de tuiles et octets compressés.
    pub par_zoom: Vec<(u8, usize, u64)>,
}

/// Un point prêt à poser, en coordonnées monde.
struct Point {
    u: f64,
    v: f64,
    id: u64,
    etiquettes: Vec<(&'static str, Valeur)>,
}

enum Valeur {
    Texte(String),
    Entier(i64),
    Reel(f64),
}

/// Un anneau de territoire, en coordonnées monde, avec son rang dans le
/// polygone (0 = contour extérieur, ensuite les trous).
/// Un tronçon de route en coordonnées monde, avec ses bornes.
struct Troncon {
    /// Le tracé complet : deux points s'il est droit, davantage s'il épouse le
    /// relief.
    points: Vec<[f64; 2]>,
    classe: u8,
    bornes: [f64; 4],
    /// Écartée du dessin : un lien sonore que la projection étire d'un bout à
    /// l'autre de la carte n'est pas une route.
    trop_longue: bool,
}

struct Anneau {
    points: Vec<[f64; 2]>,
    trou: bool,
    famille: i64,
    palier: i64,
    /// Identifiant du morceau qui habite ce bâtiment, `-1` sinon (tout ce qui
    /// n'est pas du bâti réel). C'est ce qui permet à `app.js` de retrouver
    /// le morceau exact sous le curseur via `queryRenderedFeatures` plutôt
    /// que d'approcher un centroïde à quelques pixels près — un bâtiment
    /// est une forme, pas un point (`carto-etapes.md`).
    morceau: i64,
    /// Identifiant du polygone d'origine. Un trou doit être encodé dans la
    /// même entité que son contour extérieur, sinon il se remplit.
    groupe: u32,
    /// Bornes en coordonnées monde, pour n'essayer que les tuiles concernées.
    bornes: [f64; 4],
}

/// Le rang d'un artiste par quantile de popularité — 3 (le plus prolifique)
/// à 0 (le commun) — plutôt qu'un seuil fixe d'effectif : la distribution
/// change avec chaque bibliothèque, un quantile reste valable pour toutes.
/// Même rôle que `peuplement::Rang::depuis_population` pour les
/// établissements fictifs, mais celui-ci ne suppose aucune population
/// engendrée — juste un rang parmi `total`, déjà fourni par
/// `source::Source::artistes` (trié par effectif décroissant).
///
/// **Calibré à l'œil, pas mesuré** — comme [`classe_reelle_visible_des`] et
/// [`anneau_visible_a`] : à ajuster au premier lancement visuel. Les seuils
/// (5 %, 20 %, 50 %) visent une poignée d'artistes au rang 3, quelques
/// dizaines au rang 2 — assez pour repérer, pas assez pour noyer le zoom
/// intermédiaire.
fn rang_artiste(position: usize, total: usize) -> i64 {
    if total == 0 {
        return 0;
    }
    let frac = position as f64 / total as f64;
    if frac < 0.05 {
        3
    } else if frac < 0.20 {
        2
    } else if frac < 0.50 {
        1
    } else {
        0
    }
}

/// À quel zoom un anneau doit apparaître.
///
/// Les territoires/agglomérations fictifs gardent la règle existante
/// (`territoires_jusqu_a`, une carte lointaine et lisse). Les surfaces du
/// plan de ville réel ont chacune leur logique : l'eau est un repère qu'on
/// veut voir de loin, les espaces verts n'ont de sens que de près — **calibré
/// à l'œil**, comme [`classe_reelle_visible_des`].
///
/// Le bâti — habité comme vacant — se révèle dès `paliers.morceaux_des` : la
/// trame de la ville (le bâti vacant, en lavis) et les bâtiments habités
/// (colorés par famille) apparaissent ensemble, sinon la carte à ce zoom n'est
/// qu'un semis de taches colorées sur du vide. `palier` ne sert plus ici qu'à
/// router vers la bonne couche de style (`batiments-reels` / `batiments-morceaux`).
fn anneau_visible_a(famille: i64, _palier: i64, z: u8, paliers: &Paliers) -> bool {
    match famille {
        FAMILLE_EAU_REELLE => true,
        FAMILLE_VERT_REEL => z >= 11,
        // Bâti habité **et vacant** au même zoom (`morceaux_des`) : dès qu'il y
        // a des bâtiments à l'écran, la trame de la ville doit être là, pas
        // seulement les taches colorées éparses.
        FAMILLE_BATIMENT_REEL => z >= paliers.morceaux_des,
        // `_` couvre les territoires fictifs ET l'aplat de quartier réel
        // (`FAMILLE_TERRITOIRE_REEL`) : tous deux la carte dézoomée, jusqu'à
        // `territoires_jusqu_a` — pour le réel, jusqu'à ce que les bâtiments
        // individuels prennent le relais (`Paliers::ville`).
        _ => z <= paliers.territoires_jusqu_a,
    }
}

/// Un tronçon de rue réelle, prêt pour l'encodage — même rôle que [`Troncon`]
/// mais avec les attributs qu'affiche la couche `routes-reelles` (nom
/// inventé, classe OSM, famille, artiste).
struct TronconReelPret {
    points: Vec<[f64; 2]>,
    bornes: [f64; 4],
    classe: rusty_music_osm::Classe,
    nom: String,
    nom_osm: Option<String>,
    famille: Option<i64>,
    artiste: Option<String>,
}

/// Écrit l'archive PMTiles des couches vectorielles.
pub fn ecrire(source: &Source, paliers: &Paliers, chemin: &Path) -> anyhow::Result<Rapport> {
    ecrire_avec(source, paliers, chemin, None)
}

/// Idem, en déposant au passage les tuiles en clair sous `repertoire`
/// (`z/x/y.mvt`).
///
/// Sert au diagnostic : une arborescence se sert par n'importe quel serveur de
/// fichiers, donc s'ouvre dans un navigateur ordinaire, avec ses outils. Une
/// archive PMTiles demande un lecteur — et quand c'est justement le lecteur
/// qu'on soupçonne, on ne peut plus rien conclure.
pub fn ecrire_avec(
    source: &Source,
    paliers: &Paliers,
    chemin: &Path,
    repertoire: Option<&Path>,
) -> anyhow::Result<Rapport> {
    let depart = Instant::now();

    // Un vrai plan de ville vit en lon/lat (Web Mercator) ; le monde fictif
    // vit dans le carré `[-1.08, 1.08]²` traité comme un planisphère complet
    // (`projection::carte_vers_monde`, le piège que `CLAUDE.md` signale).
    // `source.est_ville_reelle()` décide laquelle des deux projections
    // interpréter les coordonnées de `source` avec — un seul point de bascule
    // pour tous les points/lignes/polygones qui suivent.
    let reel = source.est_ville_reelle();
    let proj = |x: f64, y: f64| -> projection::Monde {
        if reel {
            projection::geo_vers_monde(projection::Geo { lon: x, lat: y })
        } else {
            projection::carte_vers_monde(x, y)
        }
    };

    let ancres = source.ancres_de_familles();
    let noms: HashMap<i64, &str> = source
        .familles
        .iter()
        .map(|f| (f.id, f.nom.as_str()))
        .collect();

    // --- Les points, une fois pour toutes -----------------------------------
    let familles: Vec<Point> = source
        .familles
        .iter()
        .filter_map(|f| {
            let &(x, y) = ancres.get(&f.id)?;
            let m = proj(x as f64, y as f64);
            Some(Point {
                u: m.u,
                v: m.v,
                id: (f.id + 1000) as u64,
                etiquettes: vec![
                    ("nom", Valeur::Texte(f.nom.clone())),
                    ("famille", Valeur::Entier(f.id)),
                    ("effectif", Valeur::Entier(f.effectif as i64)),
                ],
            })
        })
        .collect();

    // Sur le plan de ville réel, l'affectation a déjà posé chaque artiste sur
    // sa rue (`source.artistes_places`) ; sinon (chemin fictif), on retombe
    // sur le barycentre de ses morceaux (`source.artistes()`). Les deux
    // rendent trié par effectif décroissant : l'indice `i` est donc le rang
    // de popularité, sans retri. `rang_artiste` le transforme en palier 0-3.
    let artistes_bruts = if source.artistes_places.is_empty() {
        source.artistes()
    } else {
        source.artistes_places.clone()
    };
    let total_artistes = artistes_bruts.len();
    let artistes: Vec<Point> = artistes_bruts
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            let m = proj(a.x as f64, a.y as f64);
            let mut etiquettes = vec![
                ("nom", Valeur::Texte(a.nom)),
                ("rang", Valeur::Entier(rang_artiste(i, total_artistes))),
                ("famille", Valeur::Entier(a.famille)),
                ("effectif", Valeur::Entier(a.effectif as i64)),
            ];
            // Artiste ancré sur un monument (`crate::ancrage`) : son étiquette
            // le signale, il n'est pas sur une rue de son quartier.
            if let Some(monument) = a.ancre {
                etiquettes.push(("ancre", Valeur::Texte(monument)));
            }
            Point {
                u: m.u,
                v: m.v,
                id: i as u64 + 1,
                etiquettes,
            }
        })
        .collect();

    // Les établissements : le cœur de la lecture cartographique. Chacun porte
    // son rang, sa population, sa date de fondation et son toponyme ; le style
    // en tire six symboles et six seuils de zoom.
    let etablissements: Vec<Point> = source
        .etablissements
        .iter()
        .map(|e| {
            let m = proj(e.cx as f64, e.cy as f64);
            let rang = crate::peuplement::Rang::depuis_population(e.population);
            Point {
                u: m.u,
                v: m.v,
                id: e.id as u64 + 1,
                etiquettes: vec![
                    ("nom", Valeur::Texte(e.nom.clone())),
                    ("rang", Valeur::Entier(rang.indice())),
                    ("population", Valeur::Entier(e.population as i64)),
                    ("fondation", Valeur::Entier((e.fondation_date / 10_000) as i64)),
                    ("famille", Valeur::Entier(e.famille)),
                    ("ile", Valeur::Entier(e.ile as i64)),
                ],
            }
        })
        .collect();

    let curiosites: Vec<Point> = source
        .curiosites
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let m = proj(c.x as f64, c.y as f64);
            let mut etiquettes = vec![
                ("nom", Valeur::Texte(c.nom.clone())),
                ("espece", Valeur::Entier(c.espece.indice())),
            ];
            if let Some(a) = c.annee {
                etiquettes.push(("annee", Valeur::Entier(a as i64)));
            }
            Point {
                u: m.u,
                v: m.v,
                id: i as u64 + 1,
                etiquettes,
            }
        })
        .collect();

    // Les repères réels — musées, monuments, lieux de culte. Vide sur le
    // chemin fictif, où `curiosites` (ci-dessus) tient ce rôle.
    let points_remarquables: Vec<Point> = source
        .points_remarquables
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let m = proj(p.point[0], p.point[1]);
            let mut etiquettes = vec![
                ("nom", Valeur::Texte(p.nom.clone())),
                ("genre", Valeur::Texte(p.genre.clone())),
            ];
            // Monument où un artiste populaire est ancré (`crate::ancrage`).
            if let Some(artiste) = &p.artiste {
                etiquettes.push(("artiste", Valeur::Texte(artiste.clone())));
            }
            Point {
                u: m.u,
                v: m.v,
                id: i as u64 + 1,
                etiquettes,
            }
        })
        .collect();

    // Les albums — échelon entre l'artiste et le morceau. Vide sur le chemin
    // fictif, où aucun équivalent n'existe.
    let albums: Vec<Point> = source
        .albums
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let m = proj(a.point[0], a.point[1]);
            Point {
                u: m.u,
                v: m.v,
                id: i as u64 + 1,
                etiquettes: vec![
                    ("nom", Valeur::Texte(a.nom.clone())),
                    ("artiste", Valeur::Texte(a.artiste.clone())),
                    ("famille", Valeur::Entier(a.famille)),
                    ("effectif", Valeur::Entier(a.effectif as i64)),
                ],
            }
        })
        .collect();

    let morceaux: Vec<Point> = source
        .morceaux
        .iter()
        .map(|m| {
            let p = proj(m.x as f64, m.y as f64);
            let mut etiquettes = vec![
                ("titre", Valeur::Texte(m.titre.clone())),
                ("artiste", Valeur::Texte(m.artiste.clone())),
                ("famille", Valeur::Entier(m.famille)),
            ];
            if let Some(a) = m.annee {
                etiquettes.push(("annee", Valeur::Entier(a as i64)));
            }
            if let Some(b) = m.bpm {
                etiquettes.push(("bpm", Valeur::Reel(b as f64)));
            }
            if let Some(e) = m.energie {
                etiquettes.push(("energie", Valeur::Reel(e as f64)));
            }
            Point {
                u: p.u,
                v: p.v,
                id: m.id as u64,
                etiquettes,
            }
        })
        .collect();

    // --- Les territoires ----------------------------------------------------
    // Seules les nappes par famille deviennent des territoires ; la nappe
    // globale (`famille == None`) servira au relief, pas au pavage.
    // La nappe globale (`famille == None`) devient le **littoral** : c'est le
    // bord du monde, et c'est ce qui manquait le plus à la carte. Les nappes
    // par famille restent les territoires.
    let mut anneaux: Vec<Anneau> = Vec::new();
    let mut groupe = 0u32;
    for bande in &source.bandes {
        let famille = bande.famille.unwrap_or(FAMILLE_TERRE);
        for polygone in &bande.polygones {
            groupe += 1;
            for (rang, anneau) in polygone.iter().enumerate() {
                if anneau.len() < 4 {
                    continue;
                }
                let points: Vec<[f64; 2]> = anneau
                    .iter()
                    .map(|p| {
                        let m = proj(p[0], p[1]);
                        [m.u, m.v]
                    })
                    .collect();
                let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
                for p in &points {
                    b[0] = b[0].min(p[0]);
                    b[1] = b[1].min(p[1]);
                    b[2] = b[2].max(p[0]);
                    b[3] = b[3].max(p[1]);
                }
                anneaux.push(Anneau {
                    points,
                    trou: rang > 0,
                    famille,
                    palier: bande.palier as i64,
                    morceau: -1,
                    groupe,
                    bornes: b,
                });
            }
        }
    }

    // --- Le bâti des agglomérations ------------------------------------------
    //
    // Une tache d'une couleur qui n'est pas celle de la campagne : c'est le
    // premier repère d'un plan, avant même les routes.
    for (n, e) in source.etablissements.iter().enumerate() {
        let contour = crate::peuplement::contour_bati(e, paliers.pas_bati, 28);
        let points: Vec<[f64; 2]> = contour
            .iter()
            .map(|p| {
                let m = proj(p[0] as f64, p[1] as f64);
                [m.u, m.v]
            })
            .collect();
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for p in &points {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
        }
        anneaux.push(Anneau {
            points,
            trou: false,
            famille: FAMILLE_BATI,
            // Le palier porte le rang : le style s'en sert pour faire
            // apparaître les agglomérations les unes après les autres.
            palier: crate::peuplement::Rang::depuis_population(e.population).indice(),
            morceau: -1,
            groupe: 1_000_000 + n as u32,
            bornes: b,
        });
    }

    // --- Le réseau de circulation -------------------------------------------
    let routes: Vec<Troncon> = source
        .routes
        .iter()
        .filter(|r| r.points.len() >= 2)
        .map(|r| {
            let points: Vec<[f64; 2]> = r
                .points
                .iter()
                .map(|p| {
                    let m = proj(p[0] as f64, p[1] as f64);
                    [m.u, m.v]
                })
                .collect();
            let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for p in &points {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
            // La longueur se mesure de bout en bout, pas le long du tracé : un
            // détour pour épouser le relief ne doit pas faire écarter la route.
            let (a, z) = (r.points[0], r.points[r.points.len() - 1]);
            let (dx, dy) = ((a[0] - z[0]) as f64, (a[1] - z[1]) as f64);
            Troncon {
                points,
                classe: r.classe,
                bornes: b,
                trop_longue: (dx * dx + dy * dy).sqrt()
                    > paliers.longueur_max_route as f64,
            }
        })
        .collect();

    // Les rivières : des lignes, comme les routes, mais avec leur débit.
    let rivieres: Vec<Troncon> = source
        .rivieres
        .iter()
        .filter(|r| r.points.len() >= 2)
        .map(|r| {
            let points: Vec<[f64; 2]> = r
                .points
                .iter()
                .map(|p| {
                    let m = proj(p[0] as f64, p[1] as f64);
                    [m.u, m.v]
                })
                .collect();
            let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for p in &points {
                b[0] = b[0].min(p[0]);
                b[1] = b[1].min(p[1]);
                b[2] = b[2].max(p[0]);
                b[3] = b[3].max(p[1]);
            }
            Troncon {
                points,
                // Trois épaisseurs suffisent : ruisseau, rivière, fleuve.
                classe: match r.debit {
                    0..=2_000 => 2,
                    2_001..=8_000 => 1,
                    _ => 0,
                },
                bornes: b,
                trop_longue: false,
            }
        })
        .collect();

    // --- Le plan de ville réel : bâti, eau, verts, frontière, rues ----------
    //
    // Vides sur le chemin fictif : `source.batiments`/`eaux`/`verts`/
    // `troncons_reels`/`frontiere` y restent vides, ces boucles ne coûtent
    // alors rien. Bâti/eau/verts rejoignent `anneaux` : même mécanique de
    // découpe et d'encodage que les territoires et les agglomérations, sous
    // une famille fictive dédiée par nature ([`anneau_visible_a`] décide du
    // zoom, pas [`Paliers::territoires_jusqu_a`]).
    let bornes_de = |points: &[[f64; 2]]| -> [f64; 4] {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for p in points {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
        }
        b
    };
    for (surfaces, famille, groupe_debut) in [
        (&source.eaux, FAMILLE_EAU_REELLE, 3_000_000u32),
        (&source.verts, FAMILLE_VERT_REEL, 4_000_000),
    ] {
        for (n, c) in surfaces.iter().enumerate() {
            let points: Vec<[f64; 2]> = c
                .points
                .iter()
                .map(|p| {
                    let m = proj(p[0], p[1]);
                    [m.u, m.v]
                })
                .collect();
            let bornes = bornes_de(&points);
            anneaux.push(Anneau {
                points,
                trou: false,
                famille,
                palier: 0,
                morceau: -1,
                groupe: groupe_debut + n as u32,
                bornes,
            });
        }
    }

    // Le bâti à part : c'est le seul contour qui porte un occupant. `palier`
    // (réutilisé, comme pour les agglomérations fictives — voir son
    // commentaire) devient la famille du morceau qui l'habite, ou -1 si le
    // bâtiment est vacant. C'est ce que [`anneau_visible_a`] lit pour révéler
    // un bâtiment habité plus tôt qu'un bâtiment vide, et ce que
    // `style::couleur_famille` lit pour le colorer plutôt que d'y poser un
    // point (`carto-ville.md`).
    for (n, b) in source.batiments.iter().enumerate() {
        let points: Vec<[f64; 2]> = b
            .points
            .iter()
            .map(|p| {
                let m = proj(p[0], p[1]);
                [m.u, m.v]
            })
            .collect();
        let bornes = bornes_de(&points);
        anneaux.push(Anneau {
            points,
            trou: false,
            famille: FAMILLE_BATIMENT_REEL,
            palier: b.famille.unwrap_or(-1),
            morceau: b.morceau_id.unwrap_or(-1),
            groupe: 2_000_000 + n as u32,
            bornes,
        });
    }

    // Les aplats de quartier : un groupe par polygone de famille (anneau
    // extérieur puis trous), la famille dans `palier` comme pour le bâti.
    let mut groupe_territoire = 5_000_000u32;
    for terr in &source.territoires_reels {
        for polygone in &terr.polygones {
            groupe_territoire += 1;
            for (rang, anneau) in polygone.iter().enumerate() {
                if anneau.len() < 4 {
                    continue;
                }
                let points: Vec<[f64; 2]> = anneau
                    .iter()
                    .map(|p| {
                        let m = proj(p[0], p[1]);
                        [m.u, m.v]
                    })
                    .collect();
                let bornes = bornes_de(&points);
                anneaux.push(Anneau {
                    points,
                    trou: rang > 0,
                    famille: FAMILLE_TERRITOIRE_REEL,
                    palier: terr.famille,
                    morceau: -1,
                    groupe: groupe_territoire,
                    bornes,
                });
            }
        }
    }

    // La frontière communale : le littoral du plan de ville réel, un contour
    // plutôt qu'un remplissage — `couche_lignes` le porte comme les routes et
    // les rivières.
    let frontiere: Vec<Troncon> = source
        .frontiere
        .iter()
        .flat_map(|anneaux| anneaux.iter())
        .filter(|anneau| anneau.len() >= 2)
        .map(|anneau| {
            let points: Vec<[f64; 2]> = anneau
                .iter()
                .map(|p| {
                    let m = proj(p[0], p[1]);
                    [m.u, m.v]
                })
                .collect();
            let bornes = bornes_de(&points);
            Troncon { points, classe: 0, bornes, trop_longue: false }
        })
        .collect();

    // Les rues réelles. Contrairement au réseau sonique, aucune astuce de
    // crête ni filtre de longueur : ce sont déjà des polylignes courtes et
    // localement cohérentes (`docs/carto-etapes.md`).
    let troncons_reels: Vec<TronconReelPret> = source
        .troncons_reels
        .iter()
        .filter(|t| t.points.len() >= 2)
        .map(|t| {
            let points: Vec<[f64; 2]> = t
                .points
                .iter()
                .map(|p| {
                    let m = proj(p[0], p[1]);
                    [m.u, m.v]
                })
                .collect();
            let bornes = bornes_de(&points);
            TronconReelPret {
                points,
                bornes,
                classe: t.classe,
                nom: t.nom.clone(),
                nom_osm: t.nom_osm.clone(),
                famille: t.famille,
                artiste: t.artiste.clone(),
            }
        })
        .collect();

    // --- Écriture -----------------------------------------------------------
    let fichier = BufWriter::new(std::fs::File::create(chemin)?);
    let base = PmTilesWriter::new(TileType::Mvt)
        .min_zoom(0)
        .max_zoom(paliers.zoom_max)
        .tile_compression(Compression::Gzip);
    // Bornes réelles sur le plan de ville : sans elles, MapLibre ouvrirait sur
    // le centre du planisphère fictif, à des milliers de kilomètres de Paris.
    let base = if let Some((ouest, sud, est, nord)) = bbox_reelle(source) {
        base.bounds(ouest, sud, est, nord)
            .center((ouest + est) / 2.0, (sud + nord) / 2.0)
            // Vue initiale à l'échelle du quartier — non mesurée, à ajuster
            // au premier lancement visuel.
            .center_zoom(14)
    } else {
        let coin = projection::carte_vers_geo(-projection::DEMI_ETENDUE, projection::DEMI_ETENDUE);
        base.bounds(-180.0, coin.lat.min(-coin.lat), 180.0, coin.lat.max(-coin.lat))
            .center(0.0, 0.0)
            .center_zoom(2)
    };
    let mut ecrivain = base.metadata(&metadonnees(source, paliers)).create(fichier)?;

    let contexte = Contexte {
        familles: &familles,
        curiosites: &curiosites,
        rivieres: &rivieres,
        etablissements: &etablissements,
        artistes: &artistes,
        morceaux: &morceaux,
        anneaux: &anneaux,
        routes: &routes,
        troncons_reels: &troncons_reels,
        frontiere: &frontiere,
        points_remarquables: &points_remarquables,
        albums: &albums,
    };

    let mut rapport = Rapport {
        tuiles: 0,
        octets: 0,
        duree: Duration::ZERO,
        par_zoom: Vec::new(),
    };

    for z in 0..=paliers.zoom_max {
        let mut tuiles: HashMap<(u32, u32), Tuile> = HashMap::new();
        let n = 1u32 << z;

        if z <= paliers.familles_jusqu_a {
            semer(&mut tuiles, &familles, n, |t| &mut t.familles);
        }
        // Les établissements sont présents à tous les zooms : c'est le style
        // qui décide lequel apparaît quand, rang par rang.
        semer(&mut tuiles, &etablissements, n, |t| &mut t.etablissements);
        if z >= paliers.artistes_des {
            semer(&mut tuiles, &artistes, n, |t| &mut t.artistes);
        }
        if z >= paliers.morceaux_des {
            semer(&mut tuiles, &morceaux, n, |t| &mut t.morceaux);
        }
        // `anneau_visible_a` décide par famille : territoires/agglomérations
        // fictifs gardent `territoires_jusqu_a`, bâti/eau/verts réels ont
        // chacun leur propre seuil.
        semer_anneaux(&mut tuiles, &anneaux, n, z, paliers);
        semer_routes(&mut tuiles, &routes, paliers, n);
        // Un ruisseau ne se voit pas au planisphère, et le mettre dans les
        // tuiles des premiers zooms — celles qu'on attend à l'ouverture —
        // faisait passer la première image de 2,8 à 4,1 s.
        let classe_max = match z {
            0..=2 => 0,
            3..=4 => 1,
            _ => 2,
        };
        semer_lignes(&mut tuiles, &rivieres, n, classe_max, |t| &mut t.rivieres);
        // La frontière communale : visible à tout zoom, comme le littoral
        // fictif qu'elle remplace.
        semer_lignes(&mut tuiles, &frontiere, n, u8::MAX, |t| &mut t.frontiere);
        semer_troncons_reels(&mut tuiles, &troncons_reels, n, z);
        if z >= 3 {
            semer(&mut tuiles, &curiosites, n, |t| &mut t.curiosites);
        }
        // Les repères réels sont des ancres de repérage — visibles dès que le
        // réseau de rues l'est (`classe_reelle_visible_des`, les autoroutes
        // dès 10), pas seulement de très près. Calibré à l'œil, pas mesuré.
        if z >= 10 {
            semer(&mut tuiles, &points_remarquables, n, |t| &mut t.points_remarquables);
        }
        if z >= paliers.albums_des {
            semer(&mut tuiles, &albums, n, |t| &mut t.albums);
        }

        // Ordre de Hilbert : c'est celui dans lequel PMTiles range son
        // répertoire, et l'écrire ainsi évite au lecteur de sauter partout.
        let mut clefs: Vec<((u32, u32), u64)> = tuiles
            .keys()
            .map(|&(x, y)| {
                let id: TileId = TileCoord::new(z, x, y)
                    .expect("coordonnées dans les bornes du zoom")
                    .into();
                ((x, y), u64::from(id))
            })
            .collect();
        clefs.sort_unstable_by_key(|(_, id)| *id);

        let (mut compte, mut octets) = (0usize, 0u64);
        for ((x, y), _) in clefs {
            let contenu = tuiles.remove(&(x, y)).expect("clé issue de la table");
            let Some(donnees) = encoder_tuile(&contenu, &contexte, z, x, y, &noms)? else {
                continue;
            };
            octets += donnees.len() as u64;
            compte += 1;
            if let Some(r) = repertoire {
                let d = r.join(z.to_string()).join(x.to_string());
                std::fs::create_dir_all(&d)?;
                std::fs::write(d.join(format!("{y}.mvt")), &donnees)?;
            }
            ecrivain.add_tile(TileCoord::new(z, x, y)?, &donnees)?;
        }
        if compte > 0 {
            rapport.par_zoom.push((z, compte, octets));
            rapport.tuiles += compte;
        }
    }

    ecrivain.finalize()?;
    rapport.octets = std::fs::metadata(chemin)?.len();
    rapport.duree = depart.elapsed();
    Ok(rapport)
}

/// Ce qu'une tuile accumule avant d'être encodée.
#[derive(Default)]
struct Tuile {
    familles: Vec<usize>,
    curiosites: Vec<usize>,
    rivieres: Vec<usize>,
    etablissements: Vec<usize>,
    artistes: Vec<usize>,
    morceaux: Vec<usize>,
    anneaux: Vec<usize>,
    routes: Vec<usize>,
    /// Rues du plan de ville réel — vide sur le chemin fictif.
    troncons_reels: Vec<usize>,
    /// La frontière communale — vide sur le chemin fictif.
    frontiere: Vec<usize>,
    /// Musées, monuments, lieux de culte réels — vide sur le chemin fictif.
    points_remarquables: Vec<usize>,
    /// Albums — vide sur le chemin fictif.
    albums: Vec<usize>,
}

struct Contexte<'a> {
    familles: &'a [Point],
    curiosites: &'a [Point],
    rivieres: &'a [Troncon],
    etablissements: &'a [Point],
    artistes: &'a [Point],
    morceaux: &'a [Point],
    anneaux: &'a [Anneau],
    routes: &'a [Troncon],
    troncons_reels: &'a [TronconReelPret],
    frontiere: &'a [Troncon],
    points_remarquables: &'a [Point],
    albums: &'a [Point],
}

/// Encode une tuile. Rend `None` si elle est vide — une tuile sans entité n'a
/// pas à figurer dans l'archive.
fn encoder_tuile(
    t: &Tuile,
    ctx: &Contexte,
    z: u8,
    x: u32,
    y: u32,
    noms: &HashMap<i64, &str>,
) -> anyhow::Result<Option<Vec<u8>>> {
    let mut tuile = Tile::new(ETENDUE_TUILE as u32);
    let mut vide = true;

    if !t.anneaux.is_empty() {
        for (nom, quoi) in [
            ("cotes", FAMILLE_TERRE),
            ("agglomerations", FAMILLE_BATI),
            ("territoires", i64::MIN),
            ("territoires-reels", FAMILLE_TERRITOIRE_REEL),
            ("batiments", FAMILLE_BATIMENT_REEL),
            ("eaux", FAMILLE_EAU_REELLE),
            ("verts", FAMILLE_VERT_REEL),
        ] {
            if let Some(couche) = couche_polygones(&tuile, nom, quoi, t, ctx, z, x, y, noms)? {
                tuile.add_layer(couche)?;
                vide = false;
            }
        }
    }
    for (nom, indices, lignes) in [
        ("routes", &t.routes, ctx.routes),
        ("rivieres", &t.rivieres, ctx.rivieres),
        ("frontiere", &t.frontiere, ctx.frontiere),
    ] {
        if indices.is_empty() {
            continue;
        }
        if let Some(couche) = couche_lignes(&tuile, nom, indices, lignes, z, x, y)? {
            tuile.add_layer(couche)?;
            vide = false;
        }
    }
    if !t.troncons_reels.is_empty() {
        if let Some(couche) = couche_troncons_reels(&tuile, &t.troncons_reels, ctx.troncons_reels, z, x, y)? {
            tuile.add_layer(couche)?;
            vide = false;
        }
    }
    for (nom, indices, table) in [
        ("familles", &t.familles, ctx.familles),
        ("etablissements", &t.etablissements, ctx.etablissements),
        ("curiosites", &t.curiosites, ctx.curiosites),
        ("artistes", &t.artistes, ctx.artistes),
        ("morceaux", &t.morceaux, ctx.morceaux),
        ("points-remarquables", &t.points_remarquables, ctx.points_remarquables),
        ("albums", &t.albums, ctx.albums),
    ] {
        if indices.is_empty() {
            continue;
        }
        if let Some(couche) = couche_points(&tuile, nom, indices, table, z, x, y)? {
            tuile.add_layer(couche)?;
            vide = false;
        }
    }

    if vide {
        return Ok(None);
    }
    Ok(Some(tuile.to_bytes()?))
}

/// Une couche de points : un symbole par entité, ses étiquettes en attributs.
fn couche_points(
    tuile: &Tile,
    nom: &str,
    indices: &[usize],
    table: &[Point],
    z: u8,
    x: u32,
    y: u32,
) -> anyhow::Result<Option<mvt::Layer>> {
    let mut couche = tuile.create_layer(nom);
    let mut compte = 0;
    for &i in indices {
        let p = &table[i];
        let (px, py) = projection::monde_vers_tuile(
            projection::Monde { u: p.u, v: p.v },
            z,
            x,
            y,
        );
        let geom = GeomEncoder::new(GeomType::Point).point(px, py)?.encode()?;
        let mut entite = couche.into_feature(geom);
        entite.set_id(p.id);
        for (clef, valeur) in &p.etiquettes {
            match valeur {
                Valeur::Texte(t) => entite.add_tag_string(clef, t),
                Valeur::Entier(n) => entite.add_tag_int(clef, *n),
                Valeur::Reel(r) => entite.add_tag_double(clef, *r),
            }
        }
        couche = entite.into_layer();
        compte += 1;
    }
    Ok((compte > 0).then_some(couche))
}

/// Les territoires : un polygone par groupe (contour extérieur puis ses
/// trous), découpé aux limites de la tuile.
#[allow(clippy::too_many_arguments)] // z/x/y voyagent ensemble, comme partout ici
fn couche_polygones(
    tuile: &Tile,
    nom: &str,
    // La famille fictive à retenir, ou `i64::MIN` pour « toutes les vraies ».
    quoi: i64,
    t: &Tuile,
    ctx: &Contexte,
    z: u8,
    x: u32,
    y: u32,
    noms: &HashMap<i64, &str>,
) -> anyhow::Result<Option<mvt::Layer>> {
    let mut couche = tuile.create_layer(nom);
    let mut compte = 0u64;

    // Les indices arrivent dans l'ordre de construction, donc le contour
    // extérieur d'un groupe précède ses trous. Un tri stable par groupe
    // préserve cet ordre.
    let mut indices: Vec<usize> = t
        .anneaux
        .iter()
        .copied()
        .filter(|&i| {
            let f = ctx.anneaux[i].famille;
            if quoi == i64::MIN {
                f >= 0
            } else {
                f == quoi
            }
        })
        .collect();
    indices.sort_by_key(|&i| ctx.anneaux[i].groupe);

    let mut i = 0;
    while i < indices.len() {
        let groupe = ctx.anneaux[indices[i]].groupe;
        let mut j = i;
        let mut anneaux_coupes: Vec<(Vec<[f64; 2]>, bool)> = Vec::new();
        while j < indices.len() && ctx.anneaux[indices[j]].groupe == groupe {
            let a = &ctx.anneaux[indices[j]];
            let local: Vec<[f64; 2]> = a
                .points
                .iter()
                .map(|p| {
                    let (px, py) = projection::monde_vers_tuile(
                        projection::Monde { u: p[0], v: p[1] },
                        z,
                        x,
                        y,
                    );
                    [px, py]
                })
                .collect();
            // Adoucir **avant** de découper : lisser après laisserait des
            // ondulations le long du bord de tuile, visibles comme un raccord.
            //
            // Et seulement de près : aux zooms lointains, une cellule de la
            // grille de densité vaut quelques unités MVT — les marches ne se
            // voient pas, et le lissage ne ferait que quadrupler le poids de la
            // tuile. Mesuré : la tuile du zoom 0 passait de 211 à 553 Ko.
            let passes = if z >= 3 { 2 } else { 0 };
            let coupe = couper(&adoucir(&local, passes));
            if coupe.len() >= 4 {
                anneaux_coupes.push((coupe, a.trou));
            }
            j += 1;
        }

        // Un groupe dont le contour extérieur est tombé hors tuile n'a rien à
        // dire, même si un de ses trous survit.
        if anneaux_coupes.first().is_some_and(|(_, trou)| !*trou) {
            let famille = ctx.anneaux[indices[i]].famille;
            let palier = ctx.anneaux[indices[i]].palier;
            let morceau = ctx.anneaux[indices[i]].morceau;
            let mut geom = GeomEncoder::new(GeomType::Polygon);
            for (mut anneau, trou) in anneaux_coupes {
                orienter(&mut anneau, trou);
                for p in &anneau {
                    geom = geom.point(p[0], p[1])?;
                }
                geom = geom.complete()?;
            }
            let mut entite = couche.into_feature(geom.encode()?);
            compte += 1;
            entite.set_id(compte);
            entite.add_tag_int("famille", famille);
            entite.add_tag_int("palier", palier);
            // -1 pour tout ce qui n'est pas un bâtiment habité : `app.js` ne
            // s'en sert que sur la couche `batiments-morceaux`, mais l'écrire
            // uniformément évite un `if famille == FAMILLE_BATIMENT_REEL`
            // ici — quelques octets par entité, jamais consultés ailleurs.
            entite.add_tag_int("morceau", morceau);
            if let Some(n) = noms.get(&famille) {
                entite.add_tag_string("nom", n);
            }
            couche = entite.into_layer();
        }
        i = j;
    }

    Ok((compte > 0).then_some(couche))
}

/// Découpe un anneau au rectangle de la tuile, marge comprise
/// (Sutherland-Hodgman, un demi-plan à la fois).
///
/// **Pourquoi l'écrire plutôt que le prendre ailleurs** : l'intersection d'un
/// polygone quelconque avec un rectangle demanderait les opérations booléennes
/// de `geo`, un crate bien plus gros que ce qu'on en tirerait. Sutherland-
/// Hodgman est l'algorithme de manuel pour une fenêtre convexe, et son seul
/// défaut connu — des languettes d'aire nulle le long du bord sur un polygone
/// concave — est invisible dans un remplissage. C'est exactement pour cela que
/// les fabricants de tuiles s'en contentent.
fn couper(anneau: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let min = -MARGE;
    let max = ETENDUE_TUILE as f64 + MARGE;
    let mut points = anneau.to_vec();
    // Ouest, est, nord, sud.
    for bord in 0..4 {
        if points.is_empty() {
            return Vec::new();
        }
        let dedans = |p: &[f64; 2]| match bord {
            0 => p[0] >= min,
            1 => p[0] <= max,
            2 => p[1] >= min,
            _ => p[1] <= max,
        };
        let coupe = |a: &[f64; 2], b: &[f64; 2]| -> [f64; 2] {
            let (i, seuil) = match bord {
                0 => (0, min),
                1 => (0, max),
                2 => (1, min),
                _ => (1, max),
            };
            let d = b[i] - a[i];
            // Un segment parallèle au bord ne le franchit pas : `dedans` a
            // déjà tranché, on rend une extrémité plutôt que de diviser par 0.
            let t = if d.abs() < f64::EPSILON {
                0.0
            } else {
                (seuil - a[i]) / d
            };
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        };

        let mut sortie: Vec<[f64; 2]> = Vec::with_capacity(points.len() + 4);
        for k in 0..points.len() {
            let a = points[k];
            let b = points[(k + 1) % points.len()];
            let (da, db) = (dedans(&a), dedans(&b));
            if da {
                sortie.push(a);
            }
            if da != db {
                sortie.push(coupe(&a, &b));
            }
        }
        points = sortie;
    }

    // Refermer, et retirer les points que l'arrondi entier rendrait identiques.
    if let (Some(&premier), Some(&dernier)) = (points.first(), points.last()) {
        if (premier[0] - dernier[0]).abs() > 1e-9 || (premier[1] - dernier[1]).abs() > 1e-9 {
            points.push(premier);
        }
    }
    points.dedup_by(|a, b| (a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5);
    points
}

/// Adoucit un anneau par coupe de coin (Chaikin, 1974).
///
/// Les contours viennent d'une grille de 1024 cellules pour le monde entier :
/// dès le zoom 4, une tuile n'en couvre plus que 64 et les marches d'escalier
/// se voient. Deux passes de Chaikin suffisent à en faire une côte ; chaque
/// passe remplace un sommet par deux points au quart et aux trois quarts de
/// ses arêtes, ce qui rapproche la courbe sans jamais la déplacer beaucoup.
fn adoucir(anneau: &[[f64; 2]], passes: usize) -> Vec<[f64; 2]> {
    let mut points = anneau.to_vec();
    for _ in 0..passes {
        if points.len() < 4 {
            break;
        }
        let mut sortie = Vec::with_capacity(points.len() * 2);
        for k in 0..points.len() {
            let a = points[k];
            let b = points[(k + 1) % points.len()];
            sortie.push([a[0] * 0.75 + b[0] * 0.25, a[1] * 0.75 + b[1] * 0.25]);
            sortie.push([a[0] * 0.25 + b[0] * 0.75, a[1] * 0.25 + b[1] * 0.75]);
        }
        points = sortie;
    }
    points
}

/// Aire signée (formule du lacet). Dans le repère d'une tuile, où l'axe des
/// ordonnées descend, une aire positive se lit **horaire** à l'écran.
fn aire_signee(anneau: &[[f64; 2]]) -> f64 {
    let mut somme = 0.0;
    for k in 0..anneau.len() {
        let a = anneau[k];
        let b = anneau[(k + 1) % anneau.len()];
        somme += a[0] * b[1] - b[0] * a[1];
    }
    somme / 2.0
}

/// Impose le sens de parcours qu'exige la spécification MVT : contour
/// extérieur horaire, trous antihoraires. S'en remettre au sens que rendent
/// les isobandes serait un pari — et un trou pris pour un contour se remplit.
fn orienter(anneau: &mut [[f64; 2]], trou: bool) {
    let aire = aire_signee(anneau);
    let correct = if trou { aire < 0.0 } else { aire > 0.0 };
    if !correct {
        anneau.reverse();
    }
}

/// Répartit des points dans les tuiles qu'ils touchent, marge comprise.
fn semer(
    tuiles: &mut HashMap<(u32, u32), Tuile>,
    points: &[Point],
    n: u32,
    choix: impl Fn(&mut Tuile) -> &mut Vec<usize>,
) {
    let marge_monde = MARGE / ETENDUE_TUILE as f64 / n as f64;
    for (i, p) in points.iter().enumerate() {
        let x0 = ((p.u - marge_monde) * n as f64).floor().max(0.0) as u32;
        let x1 = (((p.u + marge_monde) * n as f64).floor() as i64).min(n as i64 - 1);
        let y0 = ((p.v - marge_monde) * n as f64).floor().max(0.0) as u32;
        let y1 = (((p.v + marge_monde) * n as f64).floor() as i64).min(n as i64 - 1);
        for x in x0..=(x1.max(x0 as i64) as u32) {
            for y in y0..=(y1.max(y0 as i64) as u32) {
                choix(tuiles.entry((x, y)).or_default()).push(i);
            }
        }
    }
}

/// Sème des lignes quelconques — les rivières — dans les tuiles qu'elles
/// traversent. Sans le filtre de classe propre aux routes : un fleuve n'a pas
/// de rang à respecter.
fn semer_lignes(
    tuiles: &mut HashMap<(u32, u32), Tuile>,
    lignes: &[Troncon],
    n: u32,
    classe_max: u8,
    choix: impl Fn(&mut Tuile) -> &mut Vec<usize>,
) {
    for (i, r) in lignes.iter().enumerate() {
        if r.classe > classe_max {
            continue;
        }
        let x0 = (r.bornes[0] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let x1 = (r.bornes[2] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y0 = (r.bornes[1] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y1 = (r.bornes[3] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        for x in x0..=x1 {
            for y in y0..=y1 {
                choix(tuiles.entry((x, y)).or_default()).push(i);
            }
        }
    }
}

/// Les tronçons de route dans les tuiles qu'ils traversent, jusqu'à la classe
/// que `routes_jusqua` autorise.
fn semer_routes(
    tuiles: &mut HashMap<(u32, u32), Tuile>,
    routes: &[Troncon],
    paliers: &Paliers,
    n: u32,
) {
    for (i, r) in routes.iter().enumerate() {
        if r.classe > paliers.routes_jusqua || r.trop_longue {
            continue;
        }
        let x0 = (r.bornes[0] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let x1 = (r.bornes[2] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y0 = (r.bornes[1] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y1 = (r.bornes[3] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        for x in x0..=x1 {
            for y in y0..=y1 {
                tuiles.entry((x, y)).or_default().routes.push(i);
            }
        }
    }
}

/// La couche des routes : un segment par tronçon, sa classe en attribut.
fn couche_lignes(
    tuile: &Tile,
    nom: &str,
    indices: &[usize],
    lignes: &[Troncon],
    z: u8,
    x: u32,
    y: u32,
) -> anyhow::Result<Option<mvt::Layer>> {
    let mut couche = tuile.create_layer(nom);
    let mut compte = 0u64;
    for &i in indices {
        let r = &lignes[i];
        // Le détour d'une route ne se voit pas de loin : sous le zoom 4, on
        // n'envoie que ses deux bouts. Les neuf points de chaque tracé
        // multipliaient le poids des premières tuiles — celles que l'on attend
        // à l'ouverture — et la carte mettait 3,5 s à paraître au lieu de 0,5.
        let source_points: Vec<&[f64; 2]> = if z < 4 && r.points.len() > 2 {
            vec![&r.points[0], &r.points[r.points.len() - 1]]
        } else {
            r.points.iter().collect()
        };
        let local: Vec<(f64, f64)> = source_points
            .iter()
            .map(|p| projection::monde_vers_tuile(projection::Monde { u: p[0], v: p[1] }, z, x, y))
            .collect();
        // Un tronçon dont aucun point n'approche la tuile n'a rien à y faire :
        // ses bornes l'ont laissé entrer, ce filtre le refuse.
        let dedans = |v: f64| v >= -MARGE && v <= ETENDUE_TUILE as f64 + MARGE;
        if !local.iter().any(|&(px, py)| dedans(px) && dedans(py)) {
            continue;
        }
        let mut geom = GeomEncoder::new(GeomType::Linestring);
        for &(px, py) in &local {
            geom = geom.point(px, py)?;
        }
        let geom = geom.encode()?;
        let mut entite = couche.into_feature(geom);
        compte += 1;
        entite.set_id(compte);
        entite.add_tag_int("classe", r.classe as i64);
        couche = entite.into_layer();
    }
    Ok((compte > 0).then_some(couche))
}

/// Idem pour les anneaux de territoire, via leurs bornes. [`anneau_visible_a`]
/// filtre par famille et par zoom : territoires/agglomérations fictifs d'un
/// côté, surfaces réelles de l'autre, chacun sa règle.
fn semer_anneaux(tuiles: &mut HashMap<(u32, u32), Tuile>, anneaux: &[Anneau], n: u32, z: u8, paliers: &Paliers) {
    for (i, a) in anneaux.iter().enumerate() {
        if !anneau_visible_a(a.famille, a.palier, z, paliers) {
            continue;
        }
        let x0 = (a.bornes[0] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let x1 = (a.bornes[2] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y0 = (a.bornes[1] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y1 = (a.bornes[3] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        for x in x0..=x1 {
            for y in y0..=y1 {
                tuiles.entry((x, y)).or_default().anneaux.push(i);
            }
        }
    }
}

/// Les tronçons réels dans les tuiles qu'ils traversent, jusqu'au zoom que
/// [`classe_reelle_visible_des`] autorise pour leur classe.
fn semer_troncons_reels(tuiles: &mut HashMap<(u32, u32), Tuile>, troncons: &[TronconReelPret], n: u32, z: u8) {
    for (i, t) in troncons.iter().enumerate() {
        if z < classe_reelle_visible_des(t.classe) {
            continue;
        }
        let x0 = (t.bornes[0] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let x1 = (t.bornes[2] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y0 = (t.bornes[1] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let y1 = (t.bornes[3] * n as f64).floor().clamp(0.0, n as f64 - 1.0) as u32;
        for x in x0..=x1 {
            for y in y0..=y1 {
                tuiles.entry((x, y)).or_default().troncons_reels.push(i);
            }
        }
    }
}

/// La couche `routes-reelles` : un segment par tronçon OSM, son nom inventé,
/// sa classe, et — quand connus — sa famille et son artiste.
fn couche_troncons_reels(
    tuile: &Tile,
    indices: &[usize],
    troncons: &[TronconReelPret],
    z: u8,
    x: u32,
    y: u32,
) -> anyhow::Result<Option<mvt::Layer>> {
    let mut couche = tuile.create_layer("routes-reelles");
    let mut compte = 0u64;
    for &i in indices {
        let t = &troncons[i];
        let local: Vec<(f64, f64)> = t
            .points
            .iter()
            .map(|p| projection::monde_vers_tuile(projection::Monde { u: p[0], v: p[1] }, z, x, y))
            .collect();
        let dedans = |v: f64| v >= -MARGE && v <= ETENDUE_TUILE as f64 + MARGE;
        if !local.iter().any(|&(px, py)| dedans(px) && dedans(py)) {
            continue;
        }
        let mut geom = GeomEncoder::new(GeomType::Linestring);
        for &(px, py) in &local {
            geom = geom.point(px, py)?;
        }
        let geom = geom.encode()?;
        let mut entite = couche.into_feature(geom);
        compte += 1;
        entite.set_id(compte);
        entite.add_tag_string("classe", t.classe.nom());
        entite.add_tag_string("nom", &t.nom);
        if let Some(nom_osm) = &t.nom_osm {
            entite.add_tag_string("nom_osm", nom_osm);
        }
        if let Some(famille) = t.famille {
            entite.add_tag_int("famille", famille);
        }
        if let Some(artiste) = &t.artiste {
            entite.add_tag_string("artiste", artiste);
        }
        couche = entite.into_layer();
    }
    Ok((compte > 0).then_some(couche))
}

/// Le rectangle englobant du plan de ville réel — `None` sur le chemin
/// fictif. Sert à cadrer l'archive PMTiles sur Paris plutôt que sur le
/// planisphère.
pub(crate) fn bbox_reelle(source: &Source) -> Option<(f64, f64, f64, f64)> {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut vu = false;
    let mut voir = |p: &[f64; 2]| {
        b.0 = b.0.min(p[0]);
        b.1 = b.1.min(p[1]);
        b.2 = b.2.max(p[0]);
        b.3 = b.3.max(p[1]);
        vu = true;
    };
    if let Some(anneaux) = &source.frontiere {
        for anneau in anneaux {
            anneau.iter().for_each(&mut voir);
        }
    } else {
        for t in &source.troncons_reels {
            t.points.iter().for_each(&mut voir);
        }
    }
    vu.then_some(b)
}

fn metadonnees(source: &Source, paliers: &Paliers) -> String {
    let familles: Vec<serde_json::Value> = source
        .familles
        .iter()
        .map(|f| serde_json::json!({ "id": f.id, "nom": f.nom, "effectif": f.effectif }))
        .collect();
    serde_json::json!({
        "name": "rusty-music",
        "format": "pbf",
        // `true` : plan de ville réel (OpenStreetMap, ODbL — attribution
        // requise, voir `apps/desktop/ui/app.js`). `false` : monde fictif.
        "ville_reelle": source.est_ville_reelle(),
        "familles": familles,
        "paliers": {
            "zoom_max": paliers.zoom_max,
            "territoires_jusqu_a": paliers.territoires_jusqu_a,
            "familles_jusqu_a": paliers.familles_jusqu_a,
            "artistes_des": paliers.artistes_des,
            "morceaux_des": paliers.morceaux_des,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{Famille, Morceau};
    use rusty_music_core::density::Bande;

    fn carre(cx: f64, cy: f64, r: f64) -> Vec<[f64; 2]> {
        vec![
            [cx - r, cy - r],
            [cx + r, cy - r],
            [cx + r, cy + r],
            [cx - r, cy + r],
            [cx - r, cy - r],
        ]
    }

    /// Un anneau entièrement dans la tuile ne doit pas être touché.
    #[test]
    fn un_anneau_interieur_survit_intact() {
        let a = carre(2048.0, 2048.0, 500.0);
        let c = couper(&a);
        assert!(c.len() >= 4, "anneau perdu : {c:?}");
        for p in &c {
            assert!(p[0] >= 1548.0 - 1.0 && p[0] <= 2548.0 + 1.0, "{p:?}");
        }
    }

    /// Un anneau entièrement hors de la tuile et de sa marge disparaît.
    #[test]
    fn un_anneau_exterieur_disparait() {
        let a = carre(20000.0, 20000.0, 100.0);
        assert!(couper(&a).is_empty());
    }

    /// Un anneau à cheval est ramené dans la fenêtre, marge comprise, sans
    /// jamais la déborder.
    #[test]
    fn un_anneau_a_cheval_est_ramene_dans_la_fenetre() {
        let a = carre(4096.0, 2048.0, 1500.0);
        let c = couper(&a);
        assert!(!c.is_empty());
        let borne = ETENDUE_TUILE as f64 + MARGE;
        for p in &c {
            assert!(
                p[0] >= -MARGE - 1.0 && p[0] <= borne + 1.0,
                "point hors fenêtre : {p:?}"
            );
        }
        // Le découpage ne doit pas vider la forme : elle occupe encore la
        // moitié gauche de la tuile.
        assert!(c.iter().any(|p| p[0] < 3000.0));
    }

    /// Le sens de parcours est imposé, pas hérité. Un trou parcouru comme un
    /// contour se remplirait à l'écran, et rien dans les données ne le dirait.
    #[test]
    fn le_sens_de_parcours_est_impose() {
        let mut contour = carre(2048.0, 2048.0, 1000.0);
        orienter(&mut contour, false);
        assert!(aire_signee(&contour) > 0.0, "contour extérieur non horaire");

        let mut trou = carre(2048.0, 2048.0, 300.0);
        orienter(&mut trou, true);
        assert!(aire_signee(&trou) < 0.0, "trou non antihoraire");

        // Idempotent : réorienter ne doit pas retourner ce qui était déjà bon.
        let avant = contour.clone();
        orienter(&mut contour, false);
        assert_eq!(avant, contour);
    }

    fn source_dessai() -> Source {
        let mut morceaux = Vec::new();
        for i in 0..200i64 {
            let t = i as f32 / 200.0 * std::f32::consts::TAU;
            morceaux.push(Morceau {
                id: i + 1,
                x: t.cos() * 0.6,
                y: t.sin() * 0.6,
                famille: i % 3,
                titre: format!("morceau {i}"),
                artiste: format!("artiste {}", i % 7),
                annee: Some(1990 + (i % 30) as i32),
                bpm: Some(120.0),
                energie: Some(0.4),
            });
        }
        Source {
            morceaux,
            familles: (0..3)
                .map(|id| Famille {
                    id,
                    nom: format!("famille {id}"),
                    effectif: 66,
                })
                .collect(),
            bandes: vec![Bande {
                famille: Some(0),
                palier: 0,
                polygones: vec![vec![
                    // Contour puis trou, en coordonnées carte.
                    vec![[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5], [-0.5, -0.5]],
                    vec![[-0.1, -0.1], [-0.1, 0.1], [0.1, 0.1], [0.1, -0.1], [-0.1, -0.1]],
                ]],
            }],
            ..Default::default()
        }
    }

    /// Le contrat de bout en bout : une archive lisible, non vide, dont chaque
    /// zoom ne porte que les couches que les paliers autorisent.
    #[test]
    fn larchive_respecte_les_paliers() {
        let dossier = std::env::temp_dir().join("carto-essai");
        std::fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join("essai.pmtiles");
        let paliers = Paliers {
            zoom_max: 4,
            territoires_jusqu_a: 3,
            familles_jusqu_a: 2,
            artistes_des: 1,
            albums_des: 2,
            morceaux_des: 3,
            // Les secondaires entrent : depuis que le réseau relie des lieux et
            // non des morceaux, elles ne sont plus 200 000 brins mais quelques
            // milliers de couloirs — et ce sont elles qui donnent au réseau sa
            // continuité entre les villages.
            routes_jusqua: 2,
            pas_bati: 0.0005,
            // **Recalibré depuis que le réseau relie des lieux.** À 0,05, il
            // était réglé pour des routes de morceau à morceau ; deux
            // établissements voisins étant distants d'environ 0,07, il les
            // rejetait toutes et la carte n'avait plus une seule route. 0,20
            // laisse passer une route vers les villes voisines et écarte encore
            // les liens que la projection étire d'un bout du monde à l'autre.
            longueur_max_route: 0.20,
        };
        let r = ecrire(&source_dessai(), &paliers, &chemin).unwrap();

        assert!(r.tuiles > 0, "archive vide");
        assert!(r.octets > 0);
        let zooms: Vec<u8> = r.par_zoom.iter().map(|(z, _, _)| *z).collect();
        assert_eq!(zooms, vec![0, 1, 2, 3, 4], "un zoom manque : {zooms:?}");

        // Le zoom 4 ne porte ni territoires ni familles : ses tuiles doivent
        // rester plus légères que celles du zoom 3, qui portent tout.
        let octets_z4 = r.par_zoom.iter().find(|(z, _, _)| *z == 4).unwrap().2;
        assert!(octets_z4 > 0);
        std::fs::remove_file(&chemin).ok();
    }

    /// Sans morceau, sans famille et sans bande, il n'y a pas de tuile — et
    /// surtout pas une archive pleine de tuiles vides.
    #[test]
    fn une_source_vide_ne_produit_aucune_tuile() {
        let chemin = std::env::temp_dir().join("carto-vide.pmtiles");
        let source = Source::default();
        let r = ecrire(&source, &Paliers::default(), &chemin).unwrap();
        assert_eq!(r.tuiles, 0);
        std::fs::remove_file(&chemin).ok();
    }

    /// Le contrat de bout en bout pour un plan de ville réel : les cinq
    /// couches réelles produisent des tuiles, et l'archive se cadre sur Paris
    /// plutôt que sur le planisphère fictif.
    #[test]
    fn une_source_de_ville_reelle_produit_les_couches_reelles() {
        use rusty_music_osm::Classe;
        use crate::source::{BatimentReel, ContourReel, TronconReel};

        let source = Source {
            troncons_reels: vec![TronconReel {
                points: vec![[2.340, 48.850], [2.345, 48.855]],
                classe: Classe::Residentielle,
                nom: "Rue Nina Simone".into(),
                nom_osm: Some("Rue de Test".into()),
                famille: Some(0),
                artiste: Some("Nina Simone".into()),
            }],
            batiments: vec![BatimentReel { points: carre(2.340, 48.850, 0.0005), morceau_id: None, famille: None }],
            eaux: vec![ContourReel { points: carre(2.350, 48.860, 0.001) }],
            verts: vec![ContourReel { points: carre(2.330, 48.840, 0.001) }],
            frontiere: Some(vec![carre(2.30, 48.80, 0.1)]),
            ..Default::default()
        };
        assert!(source.est_ville_reelle());

        let dossier = std::env::temp_dir().join("carto-essai-ville");
        std::fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join("ville.pmtiles");
        // zoom_max assez haut pour révéler bâti (≥15) et rue résidentielle
        // (≥14) — voir `classe_reelle_visible_des`/`anneau_visible_a`.
        let paliers = Paliers { zoom_max: 16, ..Paliers::default() };
        let r = ecrire(&source, &paliers, &chemin).unwrap();

        assert!(r.tuiles > 0, "archive vide");
        // L'eau, visible à tout zoom, doit apparaître dès le premier niveau
        // écrit.
        assert!(r.par_zoom.first().is_some_and(|(_, _, octets)| *octets > 0));
        std::fs::remove_file(&chemin).ok();
    }

    /// Le bâti — vacant comme habité — se révèle dès `morceaux_des`, et pas
    /// avant : la trame de la ville et les bâtiments colorés apparaissent
    /// ensemble, sinon la carte à ce zoom n'est qu'un semis de taches sur du
    /// vide (`carto-ville.md`, révisé — étape « le bâti »).
    #[test]
    fn le_bati_se_revele_au_seuil_des_morceaux() {
        let p = Paliers::ville();
        for palier in [-1_i64, 3] {
            assert!(anneau_visible_a(FAMILLE_BATIMENT_REEL, palier, p.morceaux_des, &p), "palier {palier} : doit se révéler au seuil des morceaux");
            assert!(!anneau_visible_a(FAMILLE_BATIMENT_REEL, palier, p.morceaux_des - 1, &p), "palier {palier} : pas avant ce seuil");
        }
    }

    /// Le rang par quantile doit rester monotone (le premier artiste n'est
    /// jamais moins bien classé qu'un autre plus loin dans le tri par
    /// effectif décroissant) et couvrir les quatre paliers sur une
    /// population de taille réaliste.
    #[test]
    fn rang_artiste_est_monotone_et_couvre_les_quatre_paliers() {
        let total = 1000;
        let mut vus = std::collections::HashSet::new();
        let mut precedent = i64::MAX;
        for position in 0..total {
            let r = rang_artiste(position, total);
            assert!(r <= precedent, "le rang doit décroître ou rester stable avec la position");
            precedent = r;
            vus.insert(r);
        }
        assert_eq!(vus, [0, 1, 2, 3].into_iter().collect(), "les quatre paliers doivent tous apparaître sur 1000 artistes");
        // Une bibliothèque sans artiste ne doit pas diviser par zéro.
        assert_eq!(rang_artiste(0, 0), 0);
    }
}
