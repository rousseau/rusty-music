// SPDX-License-Identifier: GPL-3.0-or-later
//! Base locale SQLite : la seule source de vérité pour les trois modules.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::functions::FunctionFlags;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use rusqlite_migration::{Migrations, M};

use crate::error::Result;
use crate::tags::TrackMeta;

mod analyse;
mod bibliotheque;
mod carte;
mod decouvrir;
mod enrichissement;
mod pistes;

pub struct Library {
    pub conn: Connection,
}

/// Enregistre `titre_normalise(t)` côté SQLite, pour que les jointures vers
/// `mb_release_groups.title_norm` appliquent exactement la même normalisation
/// que celle qui a rempli cette colonne ([`crate::musicbrainz::normaliser_titre`]).
/// Sans ça, `title_norm` retire les espaces (« temple of the dog » →
/// « templeofthedog ») mais une jointure en `lower(trim(...))` les garde : la
/// jointure ne matche jamais pour un titre à plusieurs mots, et l'année
/// bascule silencieusement sur celle du tag.
fn enregistrer_fonctions(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        "titre_normalise",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let titre: String = ctx.get::<Option<String>>(0)?.unwrap_or_default();
            Ok(crate::musicbrainz::normaliser_titre(&titre))
        },
    )
}

/// Résumé d'un morceau tel que servi à l'interface.
///
/// `Deserialize` en plus de `Serialize` : le plan interprété du champ
/// d'intention d'Explorer (`PlanTexte`, `apps/desktop/src/main.rs`) en
/// embarque un pour l'afficher, et le reçoit de retour une fois l'utilisateur
/// passé par l'inspecteur — le seul aller-retour JS de ce genre à ce jour.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrackRow {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<i64>,
    pub year: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Identifiant MusicBrainz de **l'artiste du morceau** (`mb_artist_id`),
    /// pas de l'artiste d'album — sert à retrouver ses albums
    /// (`Library::albums_of_artist`) depuis un morceau, sans passer par la
    /// liste des artistes. Doit rester l'identifiant du nom porté par
    /// `artist` : sur une compilation « Various Artists », l'artiste d'album
    /// (`mb_album_artist_id`) est le fourre-tout, pas le morceau — l'utiliser
    /// ici ferait cliquer « Helmet » et ouvrir les albums d'autres artistes
    /// que la seule compilation a en commun.
    pub artist_mbid: Option<String>,
}

/// Une piste vue par la vérification des pochettes.
#[derive(Debug, Clone)]
pub struct PisteVerifPochette {
    pub path: String,
    pub taille: i64,
    pub mtime: i64,
    pub artiste: String,
    pub album: String,
}

/// Le résultat de la vérification d'un dossier (ligne de `pochettes_verif`).
#[derive(Debug, Clone, PartialEq)]
pub struct VerifPochette {
    pub dossier: String,
    pub artiste: String,
    pub album: String,
    pub signature: String,
    pub version: i32,
    /// Hachage de l'image intégrée — sert à repérer une même image sur des
    /// albums différents.
    pub emb_hash: Option<String>,
    /// `None`, `"illisible"` ou `"divergente"`.
    pub anomalie: Option<String>,
    pub detail: String,
    pub mesure: Option<f64>,
}

/// Une piste en attente de mesure de loudness — chemin et clé d'album
/// (normalisée comme `album_loudness` : chaîne vide si absente) pour que
/// `crate::loudness::actualiser` sache quels albums une passe touche.
pub struct PisteLoudness {
    pub id: i64,
    pub path: String,
    pub album: String,
    pub album_artist: String,
}

/// Répartition d'une valeur continue en tranches régulières (mode
/// Bibliothèque). `comptes[i]` couvre `[min + i*pas, min + (i+1)*pas)` ; ce
/// qui déborde par le haut tombe dans `hors_gamme`, ce qui manque dans
/// `sans_valeur` — deux absences de nature différente qu'il ne faut pas
/// confondre dans une même barre.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Histogramme {
    pub min: f64,
    pub pas: f64,
    pub comptes: Vec<i64>,
    pub hors_gamme: i64,
    pub sans_valeur: i64,
}

/// Paramètres du calcul de la carte — projection t-SNE et clustering
/// k-means. Rien pour les empreintes elles-mêmes : le modèle CLAP importé
/// est figé au moment du build (`crates/analysis/src/encodeur.rs`), il n'y a
/// pas de bouton à tourner de ce côté-là.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ParametresCarte {
    /// Voisinage que t-SNE cherche à préserver. Doit rester bien en dessous
    /// du nombre de morceaux — `projeter` s'en charge, ce n'est pas à
    /// l'appelant d'y penser.
    pub perplexite: f32,
    /// Passes de descente de gradient. Plus haut stabilise le dessin, plus
    /// lentement.
    pub epoques: usize,
    /// Nombre de familles (k de k-means), sur la carte comme dans le rail
    /// « Familles ».
    pub familles: usize,
    /// Passes de k-means++. Au-delà d'une poignée de dizaines, les centres
    /// ont déjà cessé de bouger sur une bibliothèque de cette taille.
    pub iterations_kmeans: usize,
    /// Écart-type du noyau gaussien de la nappe de densité — voir
    /// [`crate::density::ParametresDensite`], que ce champ (et les deux
    /// suivants) alimente directement.
    pub densite_noyau: f64,
    /// Cellules par côté de la grille de densité.
    pub densite_resolution: usize,
    /// Bandes par nappe de densité.
    pub densite_bandes: usize,
}

impl Default for ParametresCarte {
    /// Les valeurs mesurées jusqu'ici, gardées à l'identique : changer de
    /// défaut ici changerait le dessin de la carte de quiconque n'a jamais
    /// ouvert ce réglage.
    fn default() -> Self {
        let d = crate::density::ParametresDensite::default();
        Self {
            perplexite: 30.0,
            epoques: 1000,
            familles: 12,
            iterations_kmeans: 50,
            densite_noyau: d.noyau,
            densite_resolution: d.resolution,
            densite_bandes: d.bandes,
        }
    }
}

impl ParametresCarte {
    /// Les trois champs de densité, sous la forme qu'attend
    /// [`crate::density::calculer`].
    pub fn parametres_densite(&self) -> crate::density::ParametresDensite {
        crate::density::ParametresDensite {
            noyau: self.densite_noyau,
            resolution: self.densite_resolution,
            bandes: self.densite_bandes,
        }
    }
}

/// Vocabulaire par défaut des familles par genre — voir
/// [`Library::vocabulaire_familles`]. Vit ici, à côté de `ParametresCarte`,
/// et non dans `rusty-music-analysis` : c'est une donnée de configuration de
/// la bibliothèque (réglable, persistée), pas un algorithme ; `analysis` en
/// dépend, pas l'inverse.
///
/// **Mesuré sur une bibliothèque réelle avant d'être écrit** — les genres
/// les mieux votés d'au moins cinq artistes (`mb_genres`) : `hip hop` (130
/// artistes), `jazz` (81), `alternative rock` (67), `electronic` (52),
/// `rock` (50), `folk` (40), `reggae` (21), `pop` (21), `chanson française`
/// (19), `funk` (17), `soul` (15), `blues` (15), `nu metal` (14), `dub`
/// (14), etc. Une bibliothèque différente peut vouloir une autre liste —
/// c'est justement pour ça qu'elle est réglable plutôt que figée dans le
/// code.
const VOCABULAIRE_DEFAUT: &[(&str, &[&str])] = &[
    (
        "Rock",
        &[
            "rock",
            "alternative rock",
            "hard rock",
            "psychedelic rock",
            "folk rock",
            "blues rock",
            "pop rock",
            "progressive rock",
            "punk",
            "grunge",
            "indie rock",
            "post-punk",
            "garage rock",
        ],
    ),
    (
        "Metal",
        &[
            "metal",
            "nu metal",
            "alternative metal",
            "thrash metal",
            "industrial metal",
            "groove metal",
            "heavy metal",
            "death metal",
            "black metal",
            "doom metal",
        ],
    ),
    ("Hip Hop", &["hip hop", "boom bap", "rap", "trap"]),
    (
        "Électronique",
        &[
            "electronic",
            "drum and bass",
            "big beat",
            "ambient",
            "downtempo",
            "trip hop",
            "house",
            "techno",
            "idm",
            "electronica",
            "synthwave",
        ],
    ),
    ("Jazz", &["jazz", "acid jazz", "fusion", "bebop", "swing"]),
    ("Reggae", &["reggae", "dub", "dancehall", "ska"]),
    ("Soul · Funk", &["funk", "soul", "r&b", "disco", "motown"]),
    (
        "Folk",
        &[
            "folk",
            "celtic",
            "bluegrass",
            "country",
            "americana",
            "singer-songwriter",
        ],
    ),
    (
        "Chanson",
        &["chanson française", "chanson", "variété française"],
    ),
    (
        "Classique",
        &["classical", "baroque", "opera", "contemporary classical"],
    ),
    ("Pop", &["pop", "synthpop", "dance pop", "indie pop"]),
    ("Monde", &["afrobeat", "world", "latin", "flamenco"]),
];

/// Un artiste et son volume dans la bibliothèque.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArtistRow {
    pub name: String,
    /// Identifiant MusicBrainz d'artiste d'album, quand les fichiers le
    /// portent. C'est la clé de regroupement ; `None` = regroupé sur le nom.
    pub mbid: Option<String>,
    pub tracks: i64,
    pub albums: i64,
}

/// Un album et son volume. `artist` est l'artiste d'album quand il est
/// renseigné, sinon celui de la piste — c'est ce qui tient les compilations.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlbumRow {
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<i64>,
    pub tracks: i64,
    // Chemin d'une piste de l'album, pour en tirer la pochette (`tags::read_cover`).
    // N'importe laquelle convient : c'est la même pochette pour tout l'album,
    // embarquée ou dans le dossier.
    pub path: String,
    /// Quand l'album est arrivé dans la bibliothèque, en secondes depuis
    /// l'époque : la **date de modification la plus récente** de ses fichiers.
    /// Ce n'est pas `added_at` (la date de passage du scan : tout ce qu'un même
    /// scan trouve y a le même jour) mais celle des fichiers, qui dit quand on
    /// les a posés là. La plus récente des pistes : un morceau ajouté à un album
    /// existant le remonte. `None` si aucune piste n'a de date.
    pub ajoute_le: Option<i64>,
}

/// Un album annoté pour le mode Explorer → Anneau : identité, famille
/// dominante, date de sortie, chemin d'une piste pour la pochette.
///
/// `id` est [`hacher`]`(artiste, album)`, casté en `i64` — le même hachage
/// que celui qui regroupe déjà les pistes d'un disque dans
/// [`Library::ordre_darrivee`] ; aucune autre identité d'album n'existe dans
/// le schéma (voir [`Library::albums`], une simple agrégation SQL sans id
/// stable). Voir [`Library::albums_annotes`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlbumNoeud {
    pub id: i64,
    pub name: String,
    pub artist: String,
    /// Cluster dominant, `None` tant qu'aucun morceau de l'album n'est
    /// projeté (voir [`Library::familles_des_albums`]).
    pub famille: Option<i64>,
    /// AAAAMMJJ, le plus ancien parmi les morceaux de l'album — voir la table
    /// de fiabilité de [`Library::ordre_darrivee`].
    pub date: u32,
    pub path: Option<String>,
    /// Somme des durées de ses morceaux — sert l'épaisseur de l'arc d'album
    /// sur le second anneau du mode Explorer → Anneau. Choisie plutôt que la
    /// popularité générale : toujours disponible, même sans passe
    /// popularité, voir `docs/popularite.md`.
    pub duree_ms: i64,
}

/// Un point de la carte.
///
/// Porte les mêmes champs qu'un [`TrackRow`], plus la position et la famille :
/// l'interface manipule ainsi une seule forme de « morceau », qu'il vienne
/// d'une liste ou de la carte. Deux formes distinctes, et l'inspecteur ou le
/// minutage se retrouvent sans durée selon d'où l'on a cliqué.
/// Ce que la passe de descripteurs a mesuré d'un morceau.
///
/// Chaque champ est facultatif **séparément** : un morceau peut avoir un tempo
/// sans tonalité lisible, et l'inverse.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DescripteursVus {
    /// Battements par minute. **Ambiguïté d'octave connue** : un morceau à
    /// 174 BPM peut être rendu à 87, la préférence du détecteur allant vers
    /// 120 (`crates/analysis/src/descripteurs.rs`).
    pub bpm: Option<f32>,
    /// Notée à l'anglaise — « F min », « C maj ».
    pub tonalite: Option<String>,
    /// Valeur efficace, entre 0 et 1.
    pub energie: Option<f32>,
    /// Taux de passage par zéro — élevé pour un son bruité/percussif.
    pub zcr: Option<f32>,
    /// Centroïde spectral (Hz), moyenne puis écart-type entre trames.
    pub centroide_moy: Option<f32>,
    pub centroide_ecart: Option<f32>,
    /// Rolloff spectral (Hz, seuil 85 %), même paire.
    pub rolloff_moy: Option<f32>,
    pub rolloff_ecart: Option<f32>,
    /// Aplatissement spectral (0..1, bruit vs tonal), même paire.
    pub flatness_moy: Option<f32>,
    pub flatness_ecart: Option<f32>,
}

/// Qualité d'encodage d'un morceau, telle que `tags::read` l'a lue sur le
/// disque. Chaque champ est facultatif **séparément** : un morceau scanné avant
/// que le format ne soit lu n'a ni `codec` ni `bitrate`, et `bit_depth` n'a de
/// sens que pour les conteneurs sans perte.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QualitePiste {
    pub codec: Option<String>,
    /// kb/s.
    pub bitrate: Option<i64>,
    /// Hz.
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    /// Bits — renseigné pour les formats sans perte seulement.
    pub bit_depth: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MapPoint {
    pub id: i64,
    pub path: String,
    pub x: f32,
    pub y: f32,
    pub cluster: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Artiste de regroupement (`tracks.album_artist`), distinct de `artist`
    /// qui peut lister un featuring entier (« X feat. Y »). C'est ce champ
    /// qu'il faut utiliser pour identifier *un* artiste, pas `artist` — voir
    /// le commentaire sur `mb_album_artist_id` dans le schéma.
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<i64>,
    /// La meilleure date connue, pas la seule colonne `tracks.year` — voir
    /// le repli documenté sur [`Library::map_view`]. `None` seulement quand
    /// aucune source ne donne de date, pas seulement le tag.
    pub year: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Descripteurs mesurés. Absents tant que `rusty-music descripteurs` n'est
    /// pas passé — la carte doit savoir colorer sans eux.
    pub bpm: Option<f32>,
    pub energy: Option<f32>,
    /// Popularité générale, rang percentile `0..1` dans la bibliothèque
    /// (`track_popularite.relative`). Absente tant que les passes `enrich` +
    /// `popularité` ne sont pas passées, ou si l'entité est inconnue de
    /// ListenBrainz et Deezer. La carte s'en sert pour ancrer les artistes les
    /// plus connus sur les monuments (`crate::ancrage`, dans `rusty_music_carto`).
    pub popularite: Option<f64>,
}

/// Un morceau et sa place dans l'ordre du peuplement.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArriveeBrute {
    pub track_id: i64,
    /// AAAAMMJJ. Le mois et le jour valent 00 quand on ne les connaît pas.
    pub date: u32,
    pub source: String,
    /// Hachage stable de (artiste, album) : regroupe les pistes d'un disque.
    pub album: u64,
    pub piste: u16,
}

/// `1973-03-01` → `19730301`, `1973-03` → `19730300`, `1973` → `19730000`.
fn date_iso_vers_cle(d: &str) -> Option<u32> {
    let mut parts = d.split('-');
    let a: u32 = parts.next()?.parse().ok()?;
    if !(1900..=2100).contains(&a) {
        return None;
    }
    let m: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let j: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    Some(a * 10_000 + m.min(12) * 100 + j.min(31))
}

/// Un instant epoch vers la même clé AAAAMMJJ, sans dépendance de date : on
/// n'a besoin que d'un ordre, et l'arithmétique civile grégorienne tient en
/// quinze lignes (algorithme de Howard Hinnant).
fn epoch_vers_cle(secondes: i64) -> u32 {
    let jours = secondes.div_euclid(86_400) + 719_468;
    let ere = jours.div_euclid(146_097);
    let doe = jours - ere * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + ere * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y.clamp(1900, 2100) as u32) * 10_000 + (m as u32) * 100 + (d as u32)
}

/// Année d'un album, la plus fiable connue : `first_release_date` de
/// MusicBrainz (la date de l'œuvre, pas du pressage — corrige les rééditions),
/// sauf compilation dont la date de release-group ne dit rien de ses titres ;
/// sinon le tag. Même arbitrage que la table de fiabilité de
/// [`Library::ordre_darrivee`], réduit à ce dont un album a besoin — sans les
/// médianes de secours, qui n'ont de sens que faute d'aucune date pour un
/// morceau isolé.
fn annee_fiable_album(
    mb_date: Option<&str>,
    compilation: bool,
    tag_annee: Option<i64>,
) -> Option<i64> {
    if !compilation {
        if let Some(a) = mb_date
            .and_then(date_iso_vers_cle)
            .map(|k| (k / 10_000) as i64)
        {
            return Some(a);
        }
    }
    tag_annee.filter(|a| (1900..=2100).contains(a))
}

/// Construit une [`AlbumRow`] depuis une ligne `(album, artiste, année tag,
/// pistes, chemin, date MusicBrainz, types secondaires)` — la forme commune
/// aux requêtes de [`Library::albums`] et [`Library::albums_of_artist`].
fn album_row_from_row(r: &rusqlite::Row) -> rusqlite::Result<AlbumRow> {
    let tag_annee: Option<i64> = r.get(2)?;
    let mb_date: Option<String> = r.get(5)?;
    let types: Option<String> = r.get(6)?;
    let compilation = types
        .as_deref()
        .is_some_and(|t| t.to_lowercase().contains("compilation"));
    Ok(AlbumRow {
        name: r.get(0)?,
        artist: r.get(1)?,
        year: annee_fiable_album(mb_date.as_deref(), compilation, tag_annee),
        tracks: r.get(3)?,
        path: r.get(4)?,
        ajoute_le: r.get::<_, Option<i64>>(7)?.filter(|&t| t > 0),
    })
}

/// Hachage stable d'un couple artiste/album. Doit rendre la même valeur d'une
/// exécution à l'autre : c'est lui qui départage les arrivées d'une même année.
pub(crate) fn hacher(artiste: &str, album: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in artiste
        .as_bytes()
        .iter()
        .chain(b"\x1f")
        .chain(album.as_bytes())
    {
        h ^= *octet as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Une racine surveillée, telle qu'affichée dans les réglages.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RootRow {
    pub path: String,
    pub added_at: i64,
    pub last_scan: Option<i64>,
    pub tracks: i64,
}

/// Un album MusicBrainz prêt à ranger : identifiant, titre tel qu'il est
/// publié, titre normalisé pour le rapprochement, genres avec leurs votes,
/// date de première parution du release-group (`first-release-date`,
/// souvent partielle) et types secondaires joints par une virgule
/// (« Live,Compilation ») — c'est de ces deux derniers champs que dépendent
/// [`Library::ordre_darrivee`] et [`Library::albums`] pour préférer la date
/// de l'œuvre à celle, moins sûre, du tag.
pub type AlbumRange = (
    String,
    String,
    String,
    Vec<(String, i64)>,
    Option<String>,
    Option<String>,
);

/// Un album et ses éditions multiples : l'artiste, le titre brut le plus
/// représenté, puis chaque édition (titre publié, nombre de pistes).
pub type EditionsAlbum = (String, String, Vec<(String, i64)>);

/// Une piste vue par le nommage des familles : sa famille, son artiste
/// MusicBrainz, son album, et le genre inscrit dans le fichier.
type PisteNommage = (i64, Option<String>, Option<String>, Option<String>);

/// `(groupe, artiste MusicBrainz, album, tag de fichier, genre gagnant
/// CLAP-texte)` — l'entrée du vote à trois sources, voir
/// [`Library::nommer_par_groupe`].
type PisteVote = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Une piste vue par le recalcul de popularité : son id, son MBID
/// d'enregistrement, son MBID d'artiste, son album.
type PistePop = (i64, Option<String>, Option<String>, Option<String>);

/// Une sortie repérée pour le mode Découvrir, prête à ranger. Le titre
/// normalisé n'a pas sa place ici — on ne rapproche pas ces sorties d'un
/// fichier, elles ne sont pas (encore) dans la bibliothèque.
#[derive(Debug, Clone)]
pub struct SortieARanger {
    pub rg_mbid: String,
    pub titre: String,
    /// Date brute telle que MusicBrainz la donne, pour l'affichage.
    pub date_sortie: Option<String>,
    /// Date complétée en `YYYY-MM-DD` ([`crate::musicbrainz::completer_date`]),
    /// pour le filtre de fenêtre et le tri.
    pub date_sortie_norm: Option<String>,
    pub type_primaire: Option<String>,
    /// Types secondaires joints par une virgule (« Live,Compilation »).
    pub types_secondaires: Option<String>,
    /// Noms crédités hors artiste-ancre, joints par « · ». `None` ou vide = pas
    /// une collaboration.
    pub collaborateurs: Option<String>,
}

/// Une sortie récente, telle qu'elle paraît dans le fil du mode Découvrir.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SortieFil {
    pub rg_mbid: String,
    pub artiste_mbid: String,
    pub artiste_nom: String,
    pub titre: String,
    pub date_sortie: Option<String>,
    pub type_primaire: Option<String>,
    pub collaborateurs: Option<String>,
    pub vu: bool,
}

/// Un artiste voisin proposé, avec les artistes de la bibliothèque qui y mènent.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VoisinFil {
    pub dst_mbid: String,
    pub dst_nom: String,
    pub score: f64,
    pub source: String,
    /// Noms des artistes-ancre — « proche de X, Y que vous écoutez ».
    pub portes: Vec<String>,
    /// `mb_album_artist_id` des artistes-ancre — pour filtrer le fil par famille
    /// sonique côté interface (un voisin passe si l'une de ses ancres est dans
    /// une famille cochée).
    pub src_mbids: Vec<String>,
    pub vu: bool,
}

/// Le fil du mode Découvrir : nouveaux disques, collaborations, artistes à
/// écouter ailleurs, plus la date de la dernière passe.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FilDecouvrir {
    pub derniere_passe: Option<i64>,
    pub sorties: Vec<SortieFil>,
    pub collaborations: Vec<SortieFil>,
    pub voisins: Vec<VoisinFil>,
}

/// Un enregistrement dont la popularité reste à récupérer. `artiste` et
/// `titre` servent à la recherche Deezer, qui n'a pas de MBID.
#[derive(Debug, Clone)]
pub struct PisteAPopulariser {
    pub recording_mbid: String,
    pub artiste: Option<String>,
    pub titre: Option<String>,
}

/// Une popularité brute à ranger : `ecoutes` porte la métrique principale de
/// la source (écoutes ListenBrainz, `rank` Deezer), `auditeurs` le compte
/// d'auditeurs distincts quand la source le donne.
#[derive(Debug, Clone, Copy)]
pub struct PopulariteBrute<'a> {
    pub mbid: &'a str,
    pub ecoutes: i64,
    pub auditeurs: Option<i64>,
}

/// Une biographie TheAudioDB à ranger, résolue par MBID d'artiste.
#[derive(Debug, Clone)]
pub struct BioBrute {
    pub mb_artist_id: String,
    pub id_theaudiodb: Option<String>,
    pub biographie_en: Option<String>,
    pub biographie_fr: Option<String>,
}

/// Une biographie telle que servie à l'inspecteur.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BioArtiste {
    pub mb_artist_id: String,
    pub biographie_en: Option<String>,
    pub biographie_fr: Option<String>,
}

/// Un crédit Discogs à ranger — voir `credits_discogs` du schéma.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreditDiscogs {
    pub personne: String,
    pub role: String,
    pub pistes: String,
    pub discogs_artist_id: Option<i64>,
}

/// Un label Discogs à ranger — voir `labels_discogs` du schéma.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LabelDiscogs {
    pub nom: String,
    pub catno: String,
    pub discogs_label_id: Option<i64>,
}

/// Une critique CritiqueBrainz à ranger.
#[derive(Debug, Clone)]
pub struct CritiqueBrute {
    pub id: String,
    pub auteur: Option<String>,
    pub licence_id: String,
    pub licence_nom: Option<String>,
    pub langue: Option<String>,
    pub texte: String,
    pub url_originale: Option<String>,
}

/// Une critique telle que servie à l'inspecteur — l'attribution est
/// obligatoire partout où `texte` apparaît (licence CC).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Critique {
    pub id: String,
    pub auteur: Option<String>,
    pub licence_id: String,
    pub licence_nom: Option<String>,
    pub langue: Option<String>,
    pub texte: String,
    pub url_originale: Option<String>,
}

/// Une famille nommée par vote entre trois sources indépendantes —
/// MusicBrainz, le vocabulaire CLAP-texte et Last.fm. Voir
/// `docs/nommage-familles.md`.
///
/// `fiable` distingue un nom sur lequel au moins deux sources s'accordent
/// (ou qu'une critique CritiqueBrainz est venue confirmer) d'un simple repli
/// sur MusicBrainz seul quand les trois divergent — c'est ce dernier cas que
/// l'inspecteur affiche comme « nom incertain », avec les trois propositions
/// visibles plutôt qu'un choix silencieux.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct VoteFamille {
    pub cluster: i64,
    pub effectif: i64,
    pub fiable: bool,
    /// Ce que la carte, l'anneau et la frise affichent — toujours renseigné,
    /// même quand `fiable` est faux (repli sur le nom MusicBrainz).
    pub nom_affiche: String,
    pub musicbrainz: String,
    pub clap_texte: String,
    pub lastfm: String,
}

/// Convertit une liste `(id, valeur)` en `id → rang percentile` dans `[0, 1]` :
/// la part des valeurs strictement inférieures. Insensible à l'échelle et aux
/// distributions à longue traîne — c'est pourquoi on mélange des rangs, pas
/// des comptes bruts (`docs/popularite.md`).
fn rangs_percentiles(paires: impl Iterator<Item = (i64, f64)>) -> HashMap<i64, f64> {
    let paires: Vec<(i64, f64)> = paires.collect();
    let mut triees: Vec<f64> = paires.iter().map(|(_, v)| *v).collect();
    triees.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = triees.len();
    paires
        .into_iter()
        .map(|(id, v)| {
            let moins = triees.partition_point(|x| *x < v);
            let rang = if n > 1 {
                moins as f64 / (n - 1) as f64
            } else {
                0.5
            };
            (id, rang)
        })
        .collect()
}

/// Colonnes projetées pour un [`TrackRow`], partagées par toutes les requêtes
/// de consultation pour que l'ordre reste aligné sur [`track_from_row`].
pub(crate) const TRACK_COLS: &str =
    "id, path, title, artist, album, track_no, year, duration_ms, mb_artist_id";

/// Les migrations de la base, dans l'ordre ; `PRAGMA user_version` dit
/// combien sont déjà jouées (`rusqlite_migration`). Chacune est jouée une
/// seule fois, dans une transaction.
///
/// La n°1 reconnaît toutes les bases d'avant les migrations versionnées
/// (`user_version` à 0, quelle que soit leur génération) : le schéma, tout en
/// `IF NOT EXISTS`, puis [`migrate`], qui ajoute ce qui manque en comparant à
/// `pragma_table_info`. Ne plus la modifier : un changement de schéma
/// s'ajoute à la fin, en SQL simple.
///
/// Une base ouverte par une version plus ancienne que la dernière migration
/// qu'elle a reçue est refusée (`DatabaseTooFarAhead`), plutôt que lue avec
/// un schéma qu'on ne connaît pas.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up_with_hook(
        include_str!("../sql/schema.sql"),
        |tx: &Transaction| Ok(migrate(tx)?),
    )])
}

/// Ouvre une connexion : réglages, puis migrations en attente.
///
/// Les migrations prennent le verrou d'écriture dès leur `BEGIN`
/// (`IMMEDIATE`) : l'application, la CLI et les passes de fond ouvrent la
/// même base, et deux migrations lancées ensemble en transaction différée
/// se heurteraient à `SQLITE_BUSY` sans attendre `busy_timeout`. La seconde
/// attend donc, puis rejoue ce qu'elle croyait en retard (elle a lu
/// `user_version` avant le verrou) : sans effet pour la n°1, rejouable ; une
/// migration suivante qui ne l'est pas échouerait proprement, et la
/// réouverture suivante passerait.
fn preparer(mut conn: Connection) -> Result<Connection> {
    conn.execute_batch(include_str!("../sql/connexion.sql"))?;
    conn.set_transaction_behavior(TransactionBehavior::Immediate);
    let issue = migrations().to_latest(&mut conn);
    conn.set_transaction_behavior(TransactionBehavior::Deferred);
    issue?;
    enregistrer_fonctions(&conn)?;
    Ok(conn)
}

/// Met à niveau une base créée avant les migrations versionnées — corps de
/// la migration n°1, après le schéma.
///
/// `CREATE TABLE IF NOT EXISTS` ne touche pas à une table existante : sans
/// cela, une base déjà peuplée n'obtiendrait jamais les colonnes ajoutées
/// depuis. Tout est conditionnel (`pragma_table_info`, ligne sentinelle),
/// donc rejouable sans effet sur une base déjà à jour.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('tracks')")?;
    let existantes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;

    for (nom, decl) in [
        ("mb_artist_id", "TEXT"),
        ("mb_album_artist_id", "TEXT"),
        // Débit en kb/s et format, lus par `tags::read` en même temps que
        // `sample_rate`/`channels` — aucun décodage, juste les propriétés du
        // fichier. Absents des morceaux scannés avant cette version : un
        // rescan (« Scanner », case « relire même les fichiers inchangés »)
        // les remplit sans autre passe.
        ("bitrate", "INTEGER"),
        ("codec", "TEXT"),
        // Profondeur de bits des formats sans perte (« 16 bit »), lue par
        // `tags::read` comme `bitrate`/`codec`. `NULL` tant qu'un rescan
        // « relire même les fichiers inchangés » ne l'a pas remplie.
        ("bit_depth", "INTEGER"),
        // MBID d'édition (release, pas release-group) — lu par `tags::read`
        // via `ItemKey::MusicBrainzReleaseId`. Sert à retrouver la relation
        // d'URL Discogs de cette édition précise (`docs/enrichissement-lecteur.md`).
        ("mb_release_id", "TEXT"),
    ] {
        if !existantes.contains(nom) {
            conn.execute_batch(&format!("ALTER TABLE tracks ADD COLUMN {nom} {decl}"))?;
        }
    }

    // Les dates d'œuvre, pour le placement chronologique du peuplement.
    // `first_release_date` est **déjà** dans la réponse MusicBrainz que
    // `mb_poser_albums` reçoit : c'est la date de l'œuvre et non celle du
    // pressage, donc ce qui corrige les rééditions. `secondary_types` porte
    // « Compilation », qui signale une date à ne pas croire.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('mb_release_groups')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);
    for (nom, decl) in [("first_release_date", "TEXT"), ("secondary_types", "TEXT")] {
        if !colonnes.contains(nom) {
            conn.execute_batch(&format!(
                "ALTER TABLE mb_release_groups ADD COLUMN {nom} {decl}"
            ))?;
        }
    }

    // Les deux colonnes ci-dessus ont longtemps existé sans jamais être
    // écrites : `mb_poser_albums` ignorait `first_release_date` et
    // `secondary_types` avant ce correctif. Toute base déjà enrichie porte
    // donc des artistes marqués « déjà faits » dans `mb_fetched` sans
    // aucune date sur leurs release-groups — et `mb_artistes_en_attente` ne
    // revisite jamais un artiste déjà marqué. Sans cette purge, une
    // bibliothèque déjà enrichie resterait pour toujours sans dates,
    // correctif ou pas. La ligne sentinelle (même idiome que
    // `decouvrir_marquer_passe`) garantit que ça ne se fait qu'une fois :
    // sans elle, un artiste dont MusicBrainz ne connaît vraiment aucune
    // date serait purgé à chaque démarrage et sa repasse rejouée sans fin.
    let deja_purge: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mb_fetched WHERE mbid = '@migration' AND kind = 'purge_dates_2026_09')",
        [],
        |r| r.get(0),
    )?;
    if !deja_purge {
        conn.execute_batch(
            "DELETE FROM mb_fetched WHERE kind IN ('artist', 'albums');
             INSERT INTO mb_fetched (mbid, kind) VALUES ('@migration', 'purge_dates_2026_09');",
        )?;
    }

    // Créé ici, et non dans le schéma : la colonne visée peut venir d'être
    // ajoutée juste au-dessus.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tracks_mb_aa ON tracks(mb_album_artist_id)",
    )?;

    // `artist_links` existait déjà, vide, avant le mode Découvrir — sans
    // cette migration, une base créée par une version antérieure n'aurait
    // jamais `dst_name` : `CREATE TABLE IF NOT EXISTS` du schéma ne touche
    // pas une table déjà là.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('artist_links')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    if !colonnes.contains("dst_name") {
        conn.execute_batch("ALTER TABLE artist_links ADD COLUMN dst_name TEXT")?;
    }

    // Index de recherche. `remove_diacritics 2` est ce qui fait que « bjork »
    // trouve « Björk » — `LIKE` en est incapable sans ICU. `content='tracks'`
    // évite de dupliquer les textes : l'index ne stocke que ses termes.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
             title, artist, album,
             content='tracks', content_rowid='id',
             tokenize=\"unicode61 remove_diacritics 2\"
         );

         -- Table externe : c'est à nous de tenir l'index à jour.
         CREATE TRIGGER IF NOT EXISTS tracks_fts_ai AFTER INSERT ON tracks BEGIN
           INSERT INTO tracks_fts(rowid, title, artist, album)
           VALUES (new.id, new.title, new.artist, new.album);
         END;
         CREATE TRIGGER IF NOT EXISTS tracks_fts_ad AFTER DELETE ON tracks BEGIN
           INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album)
           VALUES ('delete', old.id, old.title, old.artist, old.album);
         END;
         CREATE TRIGGER IF NOT EXISTS tracks_fts_au AFTER UPDATE ON tracks BEGIN
           INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album)
           VALUES ('delete', old.id, old.title, old.artist, old.album);
           INSERT INTO tracks_fts(rowid, title, artist, album)
           VALUES (new.id, new.title, new.artist, new.album);
         END;",
    )?;

    // Base déjà peuplée dont l'index vient d'apparaître : on le remplit. Les
    // déclencheurs suffisent ensuite.
    //
    // On compte dans `tracks_fts_docsize`, pas dans `tracks_fts` : sur une
    // table à contenu externe, `COUNT(*)` est délégué à la table de contenu et
    // renvoie donc le nombre de morceaux même quand l'index est vide. La
    // condition serait toujours fausse et la recherche resterait muette.
    let a_reconstruire: bool = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM tracks) > 0
            AND (SELECT COUNT(*) FROM tracks_fts_docsize) = 0",
        [],
        |r| r.get(0),
    )?;
    if a_reconstruire {
        conn.execute_batch("INSERT INTO tracks_fts(tracks_fts) VALUES('rebuild')")?;
    }

    // `0` (« 0000 ») est un espace réservé de tagueur pour « année absente »,
    // jamais une vraie date — `tags::read` ne l'écrit plus depuis cette
    // version, mais une base déjà scannée en porte encore. Sans ce nettoyage,
    // elle continuerait de fausser les bornes de tout ce qui classe ou
    // colore par année, jusqu'à un rescan complet des fichiers concernés.
    conn.execute_batch("UPDATE tracks SET year = NULL WHERE year = 0")?;

    // Descripteurs timbraux (ZCR, centroïde/rolloff/aplatissement spectraux) —
    // ajoutés après la table `descriptors` d'origine, `CREATE TABLE IF NOT
    // EXISTS` ne les apporte donc pas à une base déjà peuplée.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('descriptors')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    for nom in [
        "zcr",
        "centroid_mean",
        "centroid_std",
        "rolloff_mean",
        "rolloff_std",
        "flatness_mean",
        "flatness_std",
    ] {
        if !colonnes.contains(nom) {
            conn.execute_batch(&format!("ALTER TABLE descriptors ADD COLUMN {nom} REAL"))?;
        }
    }

    // Version de l'algorithme tempo/tonalité/énergie ayant produit chaque
    // ligne — voir `rusty_music_analysis::passe::VERSION_DESCRIPTEURS`. `0`
    // pour toute ligne écrite avant cette colonne : plus bas qu'aucune
    // version réelle, donc `pending_descripteurs` les considère à remesurer,
    // comme il se doit pour des mesures dont on ne sait pas si elles datent
    // d'avant ou d'après un correctif (l'inversion d'octave du tempo, par
    // exemple). Remplace le bouton « refaire ce qui est déjà mesuré » comme
    // seul moyen de rattraper un correctif d'algorithme : celui-ci reste,
    // pour une repasse volontaire, mais n'est plus le seul recours.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('descriptors')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    if !colonnes.contains("algo_version") {
        conn.execute_batch(
            "ALTER TABLE descriptors ADD COLUMN algo_version INTEGER NOT NULL DEFAULT 0",
        )?;
    }

    // Le genre gagnant du vote CLAP-texte, par morceau — calculé en lot par
    // `crates/analysis` (le calibrage a besoin du score de tous les morceaux
    // contre tout le vocabulaire avant de prendre l'argmax) et persisté ici
    // au même endroit que `cluster`, pour ne pas refaire le produit matriciel
    // à chaque lecture des familles. Voir `docs/nommage-familles.md`.
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('features')")?;
    let colonnes: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    if !colonnes.contains("texte_label") {
        conn.execute_batch("ALTER TABLE features ADD COLUMN texte_label TEXT")?;
    }

    Ok(())
}

/// Traduit une saisie libre en requête FTS5.
///
/// Chaque mot est mis entre guillemets : sans cela le texte de l'utilisateur
/// serait interprété comme des opérateurs (`AND`, `OR`, `NEAR`, `-`, `*`) et
/// une apostrophe suffirait à produire une erreur de syntaxe. Le dernier mot
/// reçoit un `*` — c'est celui qu'on est en train de taper.
fn requete_fts(q: &str) -> String {
    let mots: Vec<String> = q
        .split_whitespace()
        // Un mot sans caractère alphanumérique ne donne aucun terme au
        // tokenizer : le garder produirait une phrase vide, donc une erreur.
        .filter(|m| m.chars().any(char::is_alphanumeric))
        .map(|m| m.replace('"', "\"\""))
        .collect();

    let Some(dernier) = mots.len().checked_sub(1) else {
        return String::new();
    };
    mots.iter()
        .enumerate()
        .map(|(i, m)| {
            if i == dernier {
                format!("\"{m}\"*")
            } else {
                format!("\"{m}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn track_from_row(r: &rusqlite::Row) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?,
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        track_no: r.get(5)?,
        year: r.get(6)?,
        duration_ms: r.get(7)?,
        artist_mbid: r.get(8)?,
    })
}

impl Library {
    /// Ouvre (ou crée) la base et joue les migrations en attente.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = preparer(Connection::open(db_path)?)?;
        Ok(Self { conn })
    }

    /// Base en mémoire — pratique pour les tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = preparer(Connection::open_in_memory()?)?;
        Ok(Self { conn })
    }
}

/// Plancher de votes sur un genre MusicBrainz.
///
/// **Un, c'est-à-dire aucun filtre — et c'est une correction.** Le seuil avait
/// d'abord été mis à deux, pour écarter l'`amapiano` posé par un unique
/// contributeur sur Yann Tiersen. Mesuré, il faisait l'inverse de ce qu'on
/// attendait : chez cet artiste, `modern classical`, `neoclassicism`,
/// `minimalism` et `instrumental` portent eux aussi une seule voix. Le seuil
/// ne gardait que `rock` (2 voix) et jetait les quatre justes.
///
/// Le coût était général : **55 % des morceaux couverts au lieu de 74 %**, et
/// 360 artistes rendus muets, ceux dont aucune étiquette n'atteint deux voix.
///
/// Ce qui règle vraiment le cas `amapiano` est le classement à votes égaux,
/// dans [`Library::mb_genres`], pas un plancher.
const VOTES_MINIMUM: i64 = 1;

/// Combien de genres retenir par entité MusicBrainz.
///
/// **Un seul, et c'est mesuré.** Le genre le mieux voté d'un artiste est celui
/// sur lequel les contributeurs s'accordent ; les suivants décrivent ses
/// marges. En retenir trois versait « rock » et « pop » dans toutes les
/// familles, et c'est le générique qui l'emportait. Comparés sur la
/// bibliothèque entière, un genre par artiste rend « Reggae · Afrobeat » là où
/// trois donnaient « Reggae · Ska », et « Rock · Nu Metal » là où trois
/// donnaient « Rock · Alternative Metal ».
///
/// Trouver le distinctif est le travail de [`nommer_les_familles`], pas celui
/// de cette troncature : lui donner plus de matière générique ne l'aide pas,
/// ça la noie.
const GENRES_PAR_ENTITE: usize = 1;

/// Combien des genres les mieux représentés d'une famille comptent comme
/// « dominants », pour [`Library::genres_suspects`]. Plus d'un seul : une
/// famille mêle légitimement des genres voisins (« Reggae · Afrobeat ») sans
/// qu'aucun morceau n'y soit pour autant suspect.
const GENRES_DOMINANTS: usize = 3;

/// Les genres d'un morceau, de la source la plus précise à la plus grossière.
///
/// **L'album l'emporte sur l'artiste, l'artiste sur le fichier.** Chaque
/// échelon a sa raison :
///
/// - l'**album** distingue deux disques d'un même artiste : un enregistrement
///   acoustique d'un groupe électrique doit être étiqueté pour ce qu'il est.
///   Nos fichiers ne portent pas d'identifiant d'album, le rapprochement passe
///   donc par le titre normalisé ;
/// - l'**artiste** couvre le plus de morceaux, et son vocabulaire est curé —
///   `boom bap`, `nu metal`, `anti-folk` là où les fichiers disent « Rock » ;
/// - le **tag du fichier** reste en dernier recours, et il n'est pas
///   décoratif : les chanteurs bretons de la bibliothèque de test n'ont aucun
///   genre chez MusicBrainz, et c'est le fichier qui sait alors de quoi il
///   s'agit.
///
/// On ne mélange pas les sources pour un même morceau. Les verser ensemble
/// ferait cohabiter « Rock » et « rock », et le genre le plus grossier
/// redeviendrait le plus lourd — exactement ce qu'on cherche à quitter.
/// Le titre d'album jusqu'à la première parenthèse ou le premier crochet,
/// en minuscules — sert à rapprocher « Kid A » et « Kid A (Remaster) » sans
/// registre d'éditions à tenir à jour. Voir [`Library::editions_multiples`].
fn titre_album_normalise(titre: &str) -> String {
    let t = titre.to_lowercase();
    let fin = t.find(['(', '[']).unwrap_or(t.len());
    t[..fin].trim().to_string()
}

fn genres_du_morceau(
    artiste: Option<&str>,
    album: Option<&str>,
    tag: Option<&str>,
    albums: &HashMap<(String, String), String>,
    par_album: &HashMap<String, Vec<String>>,
    par_artiste: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let (Some(artiste), Some(album)) = (artiste, album) {
        let cle = (
            artiste.to_string(),
            crate::musicbrainz::normaliser_titre(album),
        );
        if let Some(g) = albums.get(&cle).and_then(|rg| par_album.get(rg)) {
            if !g.is_empty() {
                return g.iter().take(GENRES_PAR_ENTITE).cloned().collect();
            }
        }
    }
    if let Some(g) = artiste.and_then(|a| par_artiste.get(a)) {
        if !g.is_empty() {
            return g.iter().take(GENRES_PAR_ENTITE).cloned().collect();
        }
    }
    tag.filter(|t| !t.is_empty())
        .map(|t| vec![t.to_string()])
        .unwrap_or_default()
}

// Tranches de 10 BPM : assez fin pour distinguer un tempo lent d'un modéré,
// assez large pour qu'une mesure à quelques BPM près reste dans sa tranche.
// 40 couvre les morceaux les plus lents de la bibliothèque de test, 220 les
// plus rapides sans avoir à étendre `hors_gamme` — voir `Library::stats_tempo`.
const TEMPO_MIN: f64 = 40.0;
const TEMPO_PAS: f64 = 10.0;
const TEMPO_TRANCHES: usize = 18;

// Tranches d'une minute, de 0 à 12 — au-delà, `hors_gamme` regroupe les
// morceaux longs (mix, live) plutôt que d'étirer l'histogramme pour eux.
const DUREE_MIN: f64 = 0.0;
const DUREE_PAS: f64 = 60_000.0;
const DUREE_TRANCHES: usize = 12;

// Tranches de 32 kb/s, de 32 à 320 — le domaine habituel du MP3/AAC (128 et
// 320 en sont les repères les plus lus). Un format sans débit constant
// (FLAC, WavPack…) atterrit largement au-delà : `hors_gamme` en dit alors le
// compte, pas une tranche étirée qui écraserait le MP3 dans un coin.
const BITRATE_MIN: f64 = 32.0;
const BITRATE_PAS: f64 = 32.0;
// 10, pas 9 : la tranche [320, 352) doit exister pour accueillir 320 kb/s
// pile — le repère « qualité maximale » du MP3 — dans une tranche plutôt
// que dans `hors_gamme`, où il se confondrait avec les formats sans perte.
const BITRATE_TRANCHES: usize = 10;

/// Répartit `valeurs` en tranches régulières de largeur `pas` à partir de
/// `min` — voir [`Histogramme`]. Sert au tempo (BPM) comme à la durée (ms) :
/// même forme, seules les bornes changent.
fn histogrammer(valeurs: &[Option<f64>], min: f64, pas: f64, tranches: usize) -> Histogramme {
    let mut comptes = vec![0i64; tranches];
    let mut hors_gamme = 0i64;
    let mut sans_valeur = 0i64;
    for v in valeurs {
        match v {
            None => sans_valeur += 1,
            Some(v) if *v < min => hors_gamme += 1,
            Some(v) => match ((v - min) / pas) as usize {
                i if i < tranches => comptes[i] += 1,
                _ => hors_gamme += 1,
            },
        }
    }
    Histogramme {
        min,
        pas,
        comptes,
        hors_gamme,
        sans_valeur,
    }
}

/// Sous ce poids, un tag Last.fm est trop marginal pour compter — l'échelle
/// (0-100) n'est pas celle des votes MusicBrainz, un plancher bas suffit à
/// écarter le bruit occasionnel sans perdre les tags réels.
const LASTFM_POIDS_MINIMUM: u32 = 5;

/// Sous ce plancher, un genre décrit une poche et non une famille : son score
/// serait tiré par le hasard de quelques morceaux. Le seuil est relatif à la
/// taille de la famille, plus un minimum absolu pour les petites bibliothèques.
const PLANCHER_RELATIF: f64 = 0.01;
const PLANCHER_ABSOLU: i64 = 5;

/// Nomme chaque famille par ses genres, à partir des comptes bruts.
///
/// `effectifs` donne `(famille, nombre de morceaux)`, du plus grand au plus
/// petit ; `comptes` donne `(famille, genre, nombre de morceaux de ce genre)`.
///
/// **Ni le genre le plus fréquent, ni le plus caractéristique.** Le plus
/// fréquent ne distingue rien : « Rock » domine six des douze familles de la
/// bibliothèque de test. Le plus caractéristique — celui dont la part dans la
/// famille dépasse le plus sa part dans la bibliothèque — décrit une poche
/// marginale : il nommait « Ska Rock · Latin » une famille de 4 321 morceaux
/// menée par Bob Marley, Femi Kuti et James Brown, sur la foi de 52 morceaux.
///
/// Le score retenu, `part × log₂(sur-représentation)`, exige les deux : le
/// genre doit peser dans la famille **et** y être plus présent qu'ailleurs. La
/// même famille devient « Reggae · Pop ».
fn nommer_les_familles(
    effectifs: &[(i64, i64)],
    comptes: &[(i64, String, i64)],
) -> Vec<(i64, String, i64)> {
    nommer_les_familles_option(effectifs, comptes)
        .into_iter()
        .enumerate()
        .map(|(rang, (famille, nom, effectif))| {
            (
                famille,
                nom.unwrap_or_else(|| format!("Famille {}", rang + 1)),
                effectif,
            )
        })
        .collect()
}

/// Le cœur de [`nommer_les_familles`], sans son repli « Famille N ».
///
/// Le repli est le bon choix pour une légende, qui doit toujours montrer un
/// nom — mais c'est un aveu d'absence, pas une proposition, et le vote à
/// trois sources ([`Library::nommer_par_groupe`]) ne doit jamais le traiter
/// comme telle : sans cette distinction, deux sources qui n'ont *toutes les
/// deux* rien à dire tombent sur le même « Famille N » (même rang, même
/// bibliothèque de familles) et semblent s'accorder — un faux consensus qui
/// a fait disparaître les vrais noms MusicBrainz derrière un désaccord
/// fantôme, mesuré en développant ce chantier.
fn nommer_les_familles_option(
    effectifs: &[(i64, i64)],
    comptes: &[(i64, String, i64)],
) -> Vec<(i64, Option<String>, i64)> {
    // La population de référence, c'est l'ensemble des morceaux classés : la
    // sur-représentation se mesure contre ce que la carte montre, pas contre
    // une bibliothèque dont une partie n'est pas encore analysée.
    let mut global: HashMap<&str, i64> = HashMap::new();
    let mut total = 0i64;
    for (_, genre, n) in comptes {
        *global.entry(genre.as_str()).or_default() += n;
        total += n;
    }

    let mut par_famille: HashMap<i64, Vec<(&str, i64)>> = HashMap::new();
    for (famille, genre, n) in comptes {
        par_famille
            .entry(*famille)
            .or_default()
            .push((genre.as_str(), *n));
    }

    let mut vus: HashSet<String> = HashSet::new();
    let mut sortie = Vec::with_capacity(effectifs.len());
    for (famille, effectif) in effectifs {
        let mut classe: Vec<(&str, f64)> = Vec::new();
        if let Some(genres) = par_famille.get(famille) {
            let dans_la_famille: i64 = genres.iter().map(|(_, n)| n).sum();
            let plancher =
                PLANCHER_ABSOLU.max((dans_la_famille as f64 * PLANCHER_RELATIF).round() as i64);
            for (genre, n) in genres {
                if *n < plancher || total == 0 || dans_la_famille == 0 {
                    continue;
                }
                let part = *n as f64 / dans_la_famille as f64;
                let ailleurs = global[genre] as f64 / total as f64;
                let score = part * (part / ailleurs).log2();
                // Un score négatif signale un genre sous-représenté : il dit
                // ce que la famille n'est pas, ce qui ne la nomme pas.
                if score > 0.0 {
                    classe.push((genre, score));
                }
            }
        }
        classe.sort_by(|a, b| b.1.total_cmp(&a.1));
        let genres: Vec<&str> = classe.into_iter().map(|(g, _)| g).collect();

        let nom = libeller(&genres, &vus);
        if let Some(n) = &nom {
            vus.insert(empreinte_libelle(n));
        }
        sortie.push((*famille, nom, *effectif));
    }
    sortie
}

/// Le libellé d'une famille : son meilleur genre, précisé par le meilleur
/// suivant qui ne le redise pas et ne donne pas un nom déjà pris.
///
/// Les deux règles sont chacune tirées d'un libellé qui n'apprenait rien :
/// « Electronic · Electro », où le second mot redit le premier ; et deux
/// familles ressorties « Metal · Rock », le rock dominant la moitié de la
/// bibliothèque. La seconde descend alors son classement — « Metal · Grunge ».
/// Met une majuscule aux mots entièrement minuscules d'un genre.
///
/// Les deux sources n'écrivent pas pareil : MusicBrainz impose le tout en
/// minuscules (`trip hop`, `boom bap`), les tags des fichiers arrivent en
/// capitales (`Reggae`, `Hip-Hop`). Mélangés dans une même légende, ça se voit.
/// On ne touche qu'aux mots tout en minuscules, pour ne pas défigurer `R&B`
/// ni `IDM`.
fn capitaliser(genre: &str) -> String {
    genre
        .split_inclusive([' ', '-', '/'])
        .map(|mot| {
            if mot.chars().any(char::is_uppercase) {
                return mot.to_string();
            }
            let mut c = mot.chars();
            match c.next() {
                Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Forme canonique d'un libellé : ses mots, triés.
///
/// Sert à repérer les permutations. « Electronic · Hip Hop » et « Hip Hop ·
/// Electronic » sont deux libellés distincts au sens des chaînes, et la même
/// chose pour qui lit la légende.
fn empreinte_libelle(nom: &str) -> String {
    let mut mots: Vec<&str> = nom.split(" · ").collect();
    mots.sort_unstable();
    mots.join("\u{0}")
}

fn libeller(genres: &[&str], vus: &HashSet<String>) -> Option<String> {
    let tete = capitaliser(genres.first()?);
    let tete = tete.as_str();
    let mut repli = None;
    for g in &genres[1..] {
        if se_redisent(tete, g) {
            continue;
        }
        let nom = format!("{tete} · {}", capitaliser(g));
        if !vus.contains(&empreinte_libelle(&nom)) {
            return Some(nom);
        }
        // Le meilleur doublon, gardé au cas où aucune paire ne soit libre :
        // mieux vaut un nom en double qu'un nom amputé.
        repli.get_or_insert(nom);
    }
    if !vus.contains(&empreinte_libelle(tete)) {
        return Some(tete.to_string());
    }
    repli.or_else(|| Some(tete.to_string()))
}

/// Deux genres se redisent-ils ?
///
/// Purement lexical : un mot commun, ou l'un préfixe de l'autre sur au moins
/// cinq lettres. Ça attrape « Electro » dans « Electronic » et « Hip-Hop »
/// dans « Rap/Hip Hop » sans confondre « Rock » et « Rockabilly ». Ça ne
/// rapproche pas deux synonymes qui ne se ressemblent pas — « Rap » et
/// « Hip-Hop » — ce qui demanderait un vocabulaire des genres, propre à
/// chaque bibliothèque.
fn se_redisent(a: &str, b: &str) -> bool {
    let mots = |g: &str| -> Vec<String> {
        g.split(|c: char| !c.is_alphanumeric())
            .filter(|m| m.chars().count() >= 3)
            .map(str::to_lowercase)
            .collect()
    };
    let (ma, mb) = (mots(a), mots(b));
    ma.iter().any(|x| {
        mb.iter().any(|y| {
            x == y || (x.len().min(y.len()) >= 5 && (x.starts_with(y) || y.starts_with(x)))
        })
    })
}

/// Le libellé de tête d'un nom de famille (« X · Y » → « X »), normalisé
/// pour comparer trois sources qui n'écrivent pas pareil (casse, ponctuation)
/// — sert au vote de [`arbitrer_vote`], pas à l'affichage.
fn tete_normalisee(nom: &str) -> String {
    let tete = nom.split(" · ").next().unwrap_or(nom);
    tete.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Combine les trois votes d'une famille en un nom affiché et un drapeau de
/// confiance — voir `docs/nommage-familles.md`.
///
/// Volontairement simple, comme demandé : **au moins deux des trois
/// libellés de tête s'accordent** → fiable, ce libellé est affiché. Si les
/// trois divergent, une critique CritiqueBrainz disponible peut confirmer
/// l'un des trois (`critique_confirme`) ; sinon, repli sur le nom
/// MusicBrainz seul — le filet de sécurité déjà en place avant ce chantier,
/// pour que la carte garde toujours un nom.
fn arbitrer_vote(
    nom_mb: &str,
    nom_clap: &str,
    nom_lastfm: &str,
    critique_confirme: impl Fn(&str) -> bool,
) -> (bool, String) {
    let sources = [nom_mb, nom_clap, nom_lastfm];
    let normalises: Vec<Option<String>> = sources
        .iter()
        .map(|s| {
            let t = tete_normalisee(s);
            (!t.is_empty()).then_some(t)
        })
        .collect();

    for (i, a) in normalises.iter().enumerate() {
        let Some(a) = a else { continue };
        for (j, b) in normalises.iter().enumerate().skip(i + 1) {
            let Some(b) = b else { continue };
            if a == b {
                // Préfère la casse MusicBrainz quand elle est du bon côté de
                // l'accord — c'est la convention de capitalisation établie.
                let choisi = if i == 0 || j == 0 { nom_mb } else { sources[i] };
                return (true, choisi.to_string());
            }
        }
    }

    for nom in sources {
        if !nom.is_empty() && critique_confirme(nom) {
            return (true, nom.to_string());
        }
    }

    (false, nom_mb.to_string())
}

#[cfg(test)]
mod tests_vote {
    use super::arbitrer_vote;

    #[test]
    fn deux_sources_daccord_lemportent() {
        let (fiable, nom) = arbitrer_vote(
            "Alternative Metal",
            "A Heavy Metal Band With Screamed Vocals",
            "alternative metal",
            |_| false,
        );
        assert!(fiable);
        assert_eq!(nom, "Alternative Metal");
    }

    #[test]
    fn trois_sources_en_desaccord_replient_sur_musicbrainz() {
        let (fiable, nom) = arbitrer_vote(
            "Reggae · Rock",
            "An African Percussion Ensemble",
            "worldbeat",
            |_| false,
        );
        assert!(!fiable);
        assert_eq!(nom, "Reggae · Rock");
    }

    #[test]
    fn une_critique_confirme_une_proposition_divergente() {
        let (fiable, nom) = arbitrer_vote(
            "Reggae · Rock",
            "An African Percussion Ensemble",
            "worldbeat",
            |candidat| candidat == "An African Percussion Ensemble",
        );
        assert!(fiable);
        assert_eq!(nom, "An African Percussion Ensemble");
    }

    #[test]
    fn des_sources_vides_ne_saccordent_jamais_entre_elles() {
        let (fiable, nom) = arbitrer_vote("Reggae · Rock", "", "", |_| false);
        assert!(!fiable);
        assert_eq!(nom, "Reggae · Rock");
    }

    /// `arbitrer_vote` compare des chaînes, il ne sait pas qu'un repli
    /// « Famille N » n'est pas une vraie proposition — la garantie que deux
    /// replis identiques ne s'accordent jamais vient d'ailleurs : voir
    /// `nommer_les_familles_option` et le test de régression
    /// `db::tests::familles_votees_ignore_le_faux_accord_de_deux_replis`.
    /// Documenté ici pour que ce piège ne se réintroduise pas au niveau du
    /// vote lui-même : lui refiler « Famille 1 »/« Famille 1 » les ferait
    /// s'accorder à tort, exactement le bug mesuré en développant ce
    /// chantier.
    #[test]
    fn arbitrer_vote_ne_distingue_pas_un_repli_dune_vraie_proposition() {
        let (fiable, _) = arbitrer_vote("Rock · Grunge", "Famille 1", "Famille 1", |_| false);
        assert!(
            fiable,
            "arbitrer_vote seul ne peut pas le détecter — c'est voulu, voir le commentaire"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'ordre des trois sources, éprouvé cas par cas. C'est la décision du
    /// 17 août : MusicBrainz d'abord, tag du fichier en repli.
    #[test]
    fn lalbum_lemporte_sur_lartiste_et_lartiste_sur_le_fichier() {
        let artiste = "mbid-radiohead";
        let albums = HashMap::from([
            (
                (artiste.to_string(), "kida".to_string()),
                "rg-kida".to_string(),
            ),
            (
                (artiste.to_string(), "okcomputer".to_string()),
                "rg-okc".to_string(),
            ),
        ]);
        let par_album = HashMap::from([
            (
                "rg-kida".to_string(),
                vec!["electronic".to_string(), "art rock".to_string()],
            ),
            // Un release-group connu mais sans genre : il ne doit pas capturer
            // le morceau, sinon celui-ci perdrait les genres de son artiste.
            ("rg-okc".to_string(), Vec::new()),
        ]);
        let par_artiste =
            HashMap::from([(artiste.to_string(), vec!["alternative rock".to_string()])]);

        let g = |album, tag| {
            genres_du_morceau(Some(artiste), album, tag, &albums, &par_album, &par_artiste)
        };

        // 1. l'album quand il sait — et son meilleur genre seulement : « art
        //    rock », deuxième de la liste, ne suit pas. Voir GENRES_PAR_ENTITE.
        assert_eq!(g(Some("Kid A"), Some("Rock")), ["electronic"]);
        // 2. l'artiste quand l'album ne sait pas
        assert_eq!(g(Some("OK Computer"), Some("Rock")), ["alternative rock"]);
        // 3. l'artiste aussi quand l'album nous est inconnu
        assert_eq!(g(Some("Amnesiac"), Some("Rock")), ["alternative rock"]);

        // Le titre se rapproche malgré les mentions d'édition et la casse.
        assert_eq!(g(Some("KID A (remaster)"), None), ["electronic"]);
    }

    /// Le repli n'est pas décoratif : les chanteurs bretons de la
    /// bibliothèque de test n'ont aucun genre chez MusicBrainz, et le tag du
    /// fichier est alors la seule chose qui sache de quoi il s'agit.
    #[test]
    fn le_tag_du_fichier_sauve_ce_que_musicbrainz_ignore() {
        let sans_album: HashMap<(String, String), String> = HashMap::new();
        let sans_genre: HashMap<String, Vec<String>> = HashMap::new();
        let g = genres_du_morceau(
            Some("mbid-inconnu"),
            Some("Kan ha diskan"),
            Some("Traditional"),
            &sans_album,
            &sans_genre,
            &sans_genre,
        );
        assert_eq!(g, ["Traditional"]);

        // Et un morceau que personne ne sait nommer ne rend rien plutôt
        // qu'une étiquette inventée.
        let rien = |tag| genres_du_morceau(None, None, tag, &sans_album, &sans_genre, &sans_genre);
        assert!(rien(None).is_empty());
        assert!(rien(Some("")).is_empty());
    }

    /// On ne verse pas les deux vocabulaires dans le même sac : « Rock » du
    /// fichier et « rock » de MusicBrainz se cumuleraient, et le genre le plus
    /// grossier redeviendrait le plus lourd.
    #[test]
    fn les_sources_ne_se_melangent_pas_pour_un_meme_morceau() {
        let par_artiste = HashMap::from([("a".to_string(), vec!["boom bap".to_string()])]);
        let sans_album: HashMap<(String, String), String> = HashMap::new();
        let sans_genre: HashMap<String, Vec<String>> = HashMap::new();
        let g = genres_du_morceau(
            Some("a"),
            None,
            Some("Rock"),
            &sans_album,
            &sans_genre,
            &par_artiste,
        );
        assert_eq!(g, ["boom bap"], "le tag du fichier ne doit pas s'ajouter");
    }

    /// Les deux sources n'écrivent pas pareil : MusicBrainz tout en
    /// minuscules, les tags des fichiers en capitales. La légende doit s'en
    /// remettre sans défigurer les sigles.
    #[test]
    fn la_legende_ne_melange_pas_les_casses() {
        assert_eq!(capitaliser("trip hop"), "Trip Hop");
        assert_eq!(capitaliser("boom bap"), "Boom Bap");
        assert_eq!(capitaliser("nu metal"), "Nu Metal");
        assert_eq!(capitaliser("anti-folk"), "Anti-Folk");
        // Ceux qui portent déjà une capitale sont laissés tels quels.
        assert_eq!(capitaliser("R&B"), "R&B");
        assert_eq!(capitaliser("Rap/Hip Hop"), "Rap/Hip Hop");
        assert_eq!(capitaliser("Children's"), "Children's");
        assert_eq!(capitaliser("IDM"), "IDM");
    }

    /// Le défaut relevé sur la vraie bibliothèque : une famille de 4 321
    /// morceaux menée par Bob Marley, Femi Kuti et James Brown se nommait
    /// « Ska Rock · Latin » — deux genres sur-représentés mais marginaux —
    /// tandis que le reggae, cinq fois plus présent, était ignoré.
    #[test]
    fn un_genre_marginal_ne_nomme_pas_une_famille() {
        let comptes = vec![
            (1, "Rock".into(), 556),
            (1, "Pop".into(), 379),
            (1, "Reggae".into(), 320),
            (1, "Ska Rock".into(), 52),
            (2, "Rock".into(), 3000),
            (2, "Pop".into(), 400),
            (2, "Reggae".into(), 5),
            (2, "Ska Rock".into(), 6),
        ];
        let noms = nommer_les_familles(&[(1, 1307), (2, 3411)], &comptes);
        let nom = &noms.iter().find(|(c, _, _)| *c == 1).unwrap().1;

        // « Ska Rock » y est 3,2 fois sur-représenté — plus que la pop — et ne
        // la nomme pourtant pas : cinquante morceaux sur treize cents.
        // « Rock », lui, est majoritaire dans la famille sans lui être propre.
        assert_eq!(nom, "Reggae · Pop");
    }

    /// Le rock domine six familles sur douze : deux d'entre elles sortaient
    /// « Metal · Rock ». Une légende de douze pastilles dont deux portent le
    /// même nom ne sert à rien.
    #[test]
    fn deux_familles_ne_portent_pas_le_meme_nom() {
        let comptes = vec![
            (1, "Metal".into(), 440),
            (1, "Rock".into(), 687),
            (1, "Alternative".into(), 286),
            (2, "Metal".into(), 312),
            (2, "Rock".into(), 598),
            (2, "Grunge".into(), 208),
            (3, "Jazz".into(), 300),
        ];
        let noms = nommer_les_familles(&[(1, 1413), (2, 1118), (3, 300)], &comptes);
        let libelles: Vec<&str> = noms.iter().map(|(_, n, _)| n.as_str()).collect();
        let uniques: HashSet<&str> = libelles.iter().copied().collect();
        assert_eq!(
            uniques.len(),
            libelles.len(),
            "libellés en double : {libelles:?}"
        );
    }

    /// « Electronic · Electro » et « Hip-Hop · Rap/Hip Hop » : le second mot
    /// redisait le premier au lieu de le préciser.
    #[test]
    fn un_libelle_ne_se_repete_pas() {
        assert!(se_redisent("Electronic", "Electro"));
        assert!(se_redisent("Hip-Hop", "Rap/Hip Hop"));
        assert!(se_redisent("Rock", "Hard Rock"));
        // Ces deux-là sont bien deux genres distincts.
        assert!(!se_redisent("Rock", "Rockabilly"));
        assert!(!se_redisent("Metal", "Grunge"));

        let comptes = vec![
            (1, "Electronic".into(), 372),
            (1, "Electro".into(), 219),
            (1, "Electronica".into(), 151),
            (1, "Jazz".into(), 184),
            (2, "Rock".into(), 2000),
        ];
        let noms = nommer_les_familles(&[(1, 926), (2, 2000)], &comptes);
        assert_eq!(
            noms.iter().find(|(c, _, _)| *c == 1).unwrap().1,
            "Electronic · Jazz"
        );
    }

    /// Une famille sans genre exploitable garde un nom : elle existe sur la
    /// carte, l'utilisateur doit pouvoir la désigner.
    #[test]
    fn une_famille_sans_genre_garde_un_nom() {
        let noms = nommer_les_familles(&[(7, 42)], &[]);
        assert_eq!(noms, vec![(7, "Famille 1".to_string(), 42)]);
    }

    /// Une empreinte existe dès qu'elle est calculée ; sa place sur la carte
    /// n'arrive qu'à la projection suivante. Les deux comptes diffèrent donc
    /// pendant toute une analyse, et c'est le premier qui dit si un cache
    /// d'empreintes est périmé — s'y tromper faisait recharger 55 Mo et
    /// reconstruire le graphe des voisins à chaque requête.
    #[test]
    fn compter_les_empreintes_nest_pas_compter_les_points_de_la_carte() {
        let lib = Library::open_in_memory().unwrap();
        for i in 0..3 {
            lib.upsert(&TrackMeta {
                path: std::path::PathBuf::from(format!("/m/{i}.flac")),
                ..Default::default()
            })
            .unwrap();
            lib.save_embedding(i + 1, "essai", &[0.1, 0.2]).unwrap();
        }

        assert_eq!(lib.count_embeddings("essai").unwrap(), 3);
        assert_eq!(lib.embeddings("essai").unwrap().len(), 3);
        // Aucune projection encore : la carte est vide, les empreintes non.
        assert_eq!(lib.map_points("essai").unwrap().len(), 0);

        lib.update_map("essai", &[(1, 0.0, 0.0, 0)]).unwrap();
        assert_eq!(lib.map_points("essai").unwrap().len(), 1);
        assert_eq!(lib.count_embeddings("essai").unwrap(), 3);
        assert_eq!(lib.count_embeddings("autre-modele").unwrap(), 0);
    }

    /// Élague les fichiers disparus de la racine, et rien d'autre : un morceau
    /// encore sur le disque et un morceau d'une autre racine doivent rester.
    #[test]
    fn prune_missing_ne_retire_que_les_disparus_de_la_racine() {
        let racine = std::env::temp_dir().join(format!("rusty-music-test-{}", std::process::id()));
        std::fs::create_dir_all(&racine).unwrap();
        let present = racine.join("present.flac");
        std::fs::write(&present, b"").unwrap();

        let lib = Library::open_in_memory().unwrap();
        for p in [
            present.clone(),
            racine.join("disparu.flac"),
            std::path::PathBuf::from("/une/autre/racine/disparu.flac"),
        ] {
            lib.upsert(&TrackMeta {
                path: p,
                ..Default::default()
            })
            .unwrap();
        }
        assert_eq!(lib.count().unwrap(), 3);

        assert_eq!(lib.prune_missing(&racine).unwrap(), 1);
        assert_eq!(lib.count().unwrap(), 2);

        // Le fichier encore présent est toujours là.
        let reste: i64 = lib
            .conn
            .query_row(
                "SELECT COUNT(*) FROM tracks WHERE path = ?1",
                params![present.to_string_lossy()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reste, 1);

        std::fs::remove_dir_all(&racine).unwrap();
    }

    /// Bibliothèque miniature : deux albums d'un artiste, une compilation dont
    /// les pistes portent des artistes différents, et un morceau sans artiste.
    fn bibliotheque_test() -> Library {
        let lib = Library::open_in_memory().unwrap();
        let ajoute = |path: &str,
                      titre: &str,
                      artiste: Option<&str>,
                      album: &str,
                      album_artiste: Option<&str>,
                      no: i64,
                      annee: i64| {
            lib.upsert(&TrackMeta {
                path: path.into(),
                title: Some(titre.into()),
                artist: artiste.map(str::to_string),
                album: Some(album.into()),
                album_artist: album_artiste.map(str::to_string),
                track_no: Some(no),
                year: Some(annee),
                ..Default::default()
            })
            .unwrap();
        };

        ajoute(
            "/m/Air/Moon/02 Sexy Boy.mp3",
            "Sexy Boy",
            Some("Air"),
            "Moon Safari",
            None,
            2,
            1998,
        );
        ajoute(
            "/m/Air/Moon/01 La femme.mp3",
            "La femme d'argent",
            Some("Air"),
            "Moon Safari",
            None,
            1,
            1998,
        );
        ajoute(
            "/m/Air/Talkie/01 Venus.mp3",
            "Venus",
            Some("Air"),
            "Talkie Walkie",
            None,
            1,
            2004,
        );
        // Compilation : l'artiste d'album diffère des artistes de piste.
        ajoute(
            "/m/Comp/Spawn/03 Satan.mp4",
            "Satan",
            Some("Orbital"),
            "Spawn",
            Some("Various"),
            3,
            1997,
        );
        ajoute(
            "/m/Comp/Spawn/01 Trip.mp4",
            "Trip Like I Do",
            Some("Filter"),
            "Spawn",
            Some("Various"),
            1,
            1997,
        );
        // Sans artiste, comme les 55 morceaux non étiquetés de la vraie base.
        ajoute(
            "/m/_/Kanan/03 Bizied.mp3",
            "Bizied",
            None,
            "Kanañ a ri!",
            None,
            3,
            2017,
        );
        lib
    }

    /// Le cas qui motive tout le regroupement : trois pistes du même album,
    /// dont deux en featuring. Les artistes de piste diffèrent, l'identifiant
    /// d'artiste d'album est le même — une seule entrée doit sortir.
    #[test]
    fn artists_regroupe_les_featurings_par_identifiant_musicbrainz() {
        let lib = Library::open_in_memory().unwrap();
        let nerd = "3fb49f5a-fdc0-4789-9c84-22b38b3f3cb5";
        for (path, artiste) in [
            ("/m/nerd/01.mp3", "N.E.R.D"),
            ("/m/nerd/02.mp3", "N.E.R.D feat. Lee Harvey"),
            ("/m/nerd/03.mp3", "N.E.R.D feat. Vita"),
        ] {
            lib.upsert(&TrackMeta {
                path: path.into(),
                artist: Some(artiste.into()),
                album: Some("In Search Of...".into()),
                album_artist: Some("N.E.R.D".into()),
                mb_album_artist_id: Some(nerd.into()),
                ..Default::default()
            })
            .unwrap();
        }

        let artistes = lib.artists().unwrap();
        assert_eq!(artistes.len(), 1, "les featurings doivent fusionner");
        assert_eq!(artistes[0].name, "N.E.R.D");
        assert_eq!(artistes[0].mbid.as_deref(), Some(nerd));
        assert_eq!(artistes[0].tracks, 3);

        // Et l'on retrouve ses albums par identifiant.
        let albums = lib.albums_of_artist(Some(nerd), "N.E.R.D").unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].tracks, 3);
    }

    /// Couverture MusicBrainz partielle : certaines pistes d'un artiste
    /// portent l'identifiant, d'autres non. Il ne doit pas pour autant
    /// apparaître deux fois, avec des comptes d'albums qui se contredisent.
    #[test]
    fn artists_ne_dedouble_pas_un_artiste_partiellement_etiquete() {
        let lib = Library::open_in_memory().unwrap();
        let id = "3fb49f5a-fdc0-4789-9c84-22b38b3f3cb5";
        let poser = |path: &str, album: &str, mbid: Option<&str>| {
            lib.upsert(&TrackMeta {
                path: path.into(),
                artist: Some("N.E.R.D".into()),
                album: Some(album.into()),
                album_artist: Some("N.E.R.D".into()),
                mb_album_artist_id: mbid.map(str::to_string),
                ..Default::default()
            })
            .unwrap();
        };
        poser("/m/1.mp3", "In Search Of...", Some(id));
        poser("/m/2.mp3", "Fly or Die", None); // album non étiqueté

        let artistes = lib.artists().unwrap();
        assert_eq!(artistes.len(), 1, "artiste dédoublé : {artistes:?}");
        assert_eq!(
            artistes[0].albums, 2,
            "les deux albums doivent être comptés ensemble"
        );
        assert_eq!(artistes[0].mbid.as_deref(), Some(id));

        // Et l'ouvrir doit montrer les deux albums annoncés, pas seulement
        // celui qui porte l'identifiant.
        let albums = lib
            .albums_of_artist(artistes[0].mbid.as_deref(), &artistes[0].name)
            .unwrap();
        assert_eq!(
            albums.len(),
            2,
            "la ligne annonce plus d'albums qu'elle n'en ouvre"
        );
    }

    /// Ouvrir un artiste depuis une case de la grille d'albums : `AlbumRow` ne
    /// porte pas d'identifiant, l'appel se fait donc sur le nom seul. Ses
    /// albums étiquetés MusicBrainz doivent quand même remonter.
    #[test]
    fn albums_of_artist_sans_identifiant_rattrape_les_albums_etiquetes() {
        let lib = Library::open_in_memory().unwrap();
        let id = "3fb49f5a-fdc0-4789-9c84-22b38b3f3cb5";
        for (path, album) in [("/m/1.mp3", "In Search Of..."), ("/m/2.mp3", "Fly or Die")] {
            lib.upsert(&TrackMeta {
                path: path.into(),
                artist: Some("N.E.R.D".into()),
                album: Some(album.into()),
                album_artist: Some("N.E.R.D".into()),
                mb_album_artist_id: Some(id.into()),
                ..Default::default()
            })
            .unwrap();
        }

        let albums = lib.albums_of_artist(None, "N.E.R.D").unwrap();
        assert_eq!(
            albums.len(),
            2,
            "les albums étiquetés ne remontent pas : {albums:?}"
        );
    }

    /// La date d'ajout d'un album est la date de fichier **la plus récente** de
    /// ses pistes — un morceau posé plus tard le remonte —, pas la date de
    /// passage du scan. Une date absente ou nulle ne compte pas.
    #[test]
    fn albums_rendent_la_date_de_fichier_la_plus_recente() {
        let lib = Library::open_in_memory().unwrap();
        let pistes = [
            ("/m/a/1.mp3", "Ancien", 1_000_000_000),
            ("/m/a/2.mp3", "Ancien", 1_100_000_000),
            ("/m/b/1.mp3", "Récent", 1_700_000_000),
            ("/m/b/2.mp3", "Récent", 1_600_000_000),
            ("/m/c/1.mp3", "Sans date", 0),
        ];
        for (path, album, mtime) in pistes {
            lib.upsert(&TrackMeta {
                path: path.into(),
                artist: Some("X".into()),
                album: Some(album.into()),
                mtime,
                ..Default::default()
            })
            .unwrap();
        }
        let par_nom: std::collections::HashMap<_, _> = lib
            .albums(None)
            .unwrap()
            .into_iter()
            .map(|a| (a.name, a.ajoute_le))
            .collect();
        assert_eq!(par_nom["Ancien"], Some(1_100_000_000));
        assert_eq!(par_nom["Récent"], Some(1_700_000_000));
        assert_eq!(
            par_nom["Sans date"], None,
            "une date nulle n'est pas une date"
        );

        // Même colonne par la voie « albums d'un artiste ».
        let de_x = lib.albums_of_artist(None, "X").unwrap();
        assert_eq!(
            de_x.iter()
                .find(|a| a.name == "Récent")
                .and_then(|a| a.ajoute_le),
            Some(1_700_000_000)
        );
    }

    /// Sans identifiant, le repli se fait sur le nom — et deux artistes
    /// distincts ne doivent pas fusionner sous prétexte qu'ils n'en ont pas.
    #[test]
    fn artists_repli_sur_le_nom_sans_identifiant() {
        let lib = Library::open_in_memory().unwrap();
        for (path, artiste) in [("/m/a/1.mp3", "Alpha"), ("/m/b/1.mp3", "Beta")] {
            lib.upsert(&TrackMeta {
                path: path.into(),
                artist: Some(artiste.into()),
                album: Some(format!("Album {artiste}")),
                ..Default::default()
            })
            .unwrap();
        }
        let artistes = lib.artists().unwrap();
        assert_eq!(artistes.len(), 2);
        assert!(artistes.iter().all(|a| a.mbid.is_none()));
    }

    #[test]
    fn artists_compte_et_exclut_les_sans_artiste() {
        let lib = bibliotheque_test();
        let artistes = lib.artists().unwrap();
        let noms: Vec<&str> = artistes.iter().map(|a| a.name.as_str()).collect();
        // Ordre alphabétique ; le morceau sans artiste n'apparaît pas ; et les
        // pistes de la compilation se rangent sous leur artiste d'album plutôt
        // que d'ouvrir une entrée par invité (Orbital, Filter).
        assert_eq!(noms, ["Air", "Various"]);

        let air = &artistes[0];
        assert_eq!(air.tracks, 3);
        assert_eq!(air.albums, 2);

        let various = &artistes[1];
        assert_eq!(various.tracks, 2);
        assert_eq!(various.albums, 1);
    }

    #[test]
    fn albums_regroupe_la_compilation_sous_son_artiste_dalbum() {
        let lib = bibliotheque_test();

        let tous = lib.albums(None).unwrap();
        let noms: Vec<&str> = tous.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            noms,
            ["Kanañ a ri!", "Moon Safari", "Spawn", "Talkie Walkie"]
        );

        // La compilation forme un seul album malgré deux artistes de piste.
        let spawn = tous.iter().find(|a| a.name == "Spawn").unwrap();
        assert_eq!(spawn.tracks, 2);
        assert_eq!(spawn.artist.as_deref(), Some("Various"));

        // Filtrer par artiste de piste ramène quand même la compilation.
        let chez_orbital = lib.albums(Some("Orbital")).unwrap();
        assert_eq!(chez_orbital.len(), 1);
        assert_eq!(chez_orbital[0].name, "Spawn");

        let chez_air = lib.albums(Some("Air")).unwrap();
        assert_eq!(chez_air.len(), 2);
    }

    #[test]
    fn familles_des_albums_prend_le_cluster_majoritaire() {
        let lib = Library::open_in_memory().unwrap();
        let ajoute = |path: &str, album: &str, artiste: &str, cluster: Option<i64>| {
            let id = lib
                .upsert(&TrackMeta {
                    path: path.into(),
                    album: Some(album.into()),
                    album_artist: Some(artiste.into()),
                    ..Default::default()
                })
                .unwrap();
            if let Some(c) = cluster {
                lib.save_features(id, "clap", &[0.0], 0.0, 0.0, c).unwrap();
            }
        };

        ajoute("/m/a1.mp3", "A", "X", Some(1));
        ajoute("/m/a2.mp3", "A", "X", Some(1));
        ajoute("/m/a3.mp3", "A", "X", Some(2));
        ajoute("/m/b1.mp3", "B", "Y", Some(3));
        ajoute("/m/b2.mp3", "B", "Y", Some(3));
        // Égalité 5 / 7 : le plus petit numéro tranche.
        ajoute("/m/c1.mp3", "C", "Z", Some(7));
        ajoute("/m/c2.mp3", "C", "Z", Some(5));
        // Aucun morceau projeté : l'album n'apparaît pas, le filtre le laisse
        // visible par défaut.
        ajoute("/m/d1.mp3", "D", "W", None);

        let mut f = lib.familles_des_albums("clap").unwrap();
        f.sort();
        assert_eq!(
            f,
            vec![
                ("A".to_string(), Some("X".to_string()), 1),
                ("B".to_string(), Some("Y".to_string()), 3),
                ("C".to_string(), Some("Z".to_string()), 5),
            ]
        );
    }

    #[test]
    fn album_embeddings_moyenne_les_empreintes_dun_meme_album() {
        let lib = Library::open_in_memory().unwrap();
        let ajoute = |path: &str, artiste: &str, album: &str, vecteur: &[f32]| {
            let id = lib
                .upsert(&TrackMeta {
                    path: path.into(),
                    artist: Some(artiste.into()),
                    album_artist: Some(artiste.into()),
                    album: Some(album.into()),
                    ..Default::default()
                })
                .unwrap();
            lib.save_features(id, "clap", vecteur, 0.0, 0.0, 0).unwrap();
        };
        ajoute("/m/x1.mp3", "X", "A", &[1.0, 0.0]);
        ajoute("/m/x2.mp3", "X", "A", &[3.0, 0.0]);
        ajoute("/m/y1.mp3", "Y", "B", &[0.0, 5.0]);

        let par_id: HashMap<i64, Vec<f32>> =
            lib.album_embeddings("clap").unwrap().into_iter().collect();

        assert_eq!(par_id[&(hacher("X", "A") as i64)], vec![2.0, 0.0]);
        assert_eq!(par_id[&(hacher("Y", "B") as i64)], vec![0.0, 5.0]);
    }

    #[test]
    fn albums_annotes_croise_famille_et_date() {
        let lib = Library::open_in_memory().unwrap();
        let ajoute =
            |path: &str, artiste: &str, album: &str, annee: i64, cluster: i64, duree_ms: i64| {
                let id = lib
                    .upsert(&TrackMeta {
                        path: path.into(),
                        artist: Some(artiste.into()),
                        album_artist: Some(artiste.into()),
                        album: Some(album.into()),
                        year: Some(annee),
                        duration_ms: Some(duree_ms),
                        ..Default::default()
                    })
                    .unwrap();
                lib.save_features(id, "clap", &[0.0], 0.0, 0.0, cluster)
                    .unwrap();
            };
        ajoute("/m/x1.mp3", "X", "A", 1990, 1, 200_000);
        ajoute("/m/x2.mp3", "X", "A", 1990, 1, 180_000);
        ajoute("/m/x3.mp3", "X", "B", 1994, 2, 240_000);

        let par_id: HashMap<i64, AlbumNoeud> = lib
            .albums_annotes("clap")
            .unwrap()
            .into_iter()
            .map(|n| (n.id, n))
            .collect();

        let a = &par_id[&(hacher("X", "A") as i64)];
        assert_eq!(a.famille, Some(1));
        assert_eq!(a.date, 19_900_000);
        assert_eq!(a.artist, "X");
        assert_eq!(a.duree_ms, 380_000, "somme des durées de l'album");

        let b = &par_id[&(hacher("X", "B") as i64)];
        assert_eq!(b.famille, Some(2));
        assert_eq!(b.date, 19_940_000);
    }

    #[test]
    fn tracks_of_album_respecte_lordre_du_disque() {
        let lib = bibliotheque_test();
        let pistes = lib.tracks_of_album("Moon Safari", None).unwrap();
        let titres: Vec<&str> = pistes.iter().filter_map(|t| t.title.as_deref()).collect();
        assert_eq!(titres, ["La femme d'argent", "Sexy Boy"]);
        assert_eq!(pistes[0].track_no, Some(1));
    }

    #[test]
    fn search_couvre_titre_artiste_et_album() {
        let lib = bibliotheque_test();
        assert_eq!(lib.search("sexy", 50).unwrap().len(), 1); // titre
        assert_eq!(lib.search("AIR", 50).unwrap().len(), 3); // artiste, casse ignorée
        assert_eq!(lib.search("Moon", 50).unwrap().len(), 2); // album
        assert_eq!(lib.search("introuvable", 50).unwrap().len(), 0);
        assert_eq!(lib.search("a", 2).unwrap().len(), 2); // la limite s'applique
    }

    #[test]
    fn search_albums_regroupe_par_album() {
        let lib = bibliotheque_test();
        // « Moon Safari » compte deux pistes retenues par la recherche : sans
        // le regroupement, il apparaîtrait deux fois.
        let r = lib.search_albums("Moon", 50).unwrap();
        assert_eq!(r.len(), 1, "l'album ne doit sortir qu'une fois : {r:?}");
        assert_eq!(r[0].1, "Moon Safari");
        assert_eq!(lib.search_albums("introuvable", 50).unwrap().len(), 0);
    }

    /// Mise à niveau d'une base peuplée avant que l'index de recherche
    /// n'existe. Les autres tests ne voient pas ce cas : leurs lignes sont
    /// insérées après, donc indexées au vol par les déclencheurs.
    #[test]
    fn migration_indexe_une_base_deja_peuplee() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../sql/schema.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO tracks(path, title, album) VALUES ('/m/a.mp3', 'Bizied', 'Kanañ a ri!')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();
        let lib = Library { conn };

        assert_eq!(
            lib.search("kanan", 10).unwrap().len(),
            1,
            "l'index n'a pas été reconstruit pour les lignes préexistantes"
        );
    }

    /// Une base enrichie par le code d'avant le correctif porte des artistes
    /// marqués « déjà faits » sans aucune date sur leurs release-groups —
    /// `mb_poser_albums` ne les écrivait pas encore. La migration doit les
    /// remettre « à faire » pour qu'un `enrich` ultérieur les rattrape.
    #[test]
    fn purge_dates_remet_en_attente_les_artistes_deja_fetches() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../sql/schema.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO tracks(path, artist, mb_artist_id) VALUES ('/m/a.mp3', 'X', 'mbid-x')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
             VALUES ('rg-1', 'mbid-x', 'Album', 'album')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mb_fetched (mbid, kind) VALUES ('mbid-x', 'artist')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mb_fetched (mbid, kind) VALUES ('mbid-x', 'albums')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();
        let lib = Library { conn };

        assert_eq!(
            lib.mb_artistes_en_attente("artist", 10).unwrap(),
            vec!["mbid-x".to_string()],
            "l'artiste doit redevenir « à faire » après la purge"
        );
    }

    /// Sans ligne sentinelle, la purge rejouerait à chaque démarrage — et un
    /// artiste dont MusicBrainz ne connaît vraiment aucune date (une repasse
    /// honnête peut légitimement en rendre aucune) serait remis « à faire »
    /// indéfiniment, contrairement à l'esprit « reprend où elle s'est
    /// arrêtée » du module.
    #[test]
    fn purge_dates_ne_se_repete_pas() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../sql/schema.sql"))
            .unwrap();
        migrate(&conn).unwrap(); // premier « démarrage » : pose la sentinelle

        // Une repasse d'enrich réussie depuis : l'artiste est de nouveau
        // marqué fait, toujours sans date connue.
        conn.execute(
            "INSERT INTO tracks(path, artist, mb_artist_id) VALUES ('/m/a.mp3', 'X', 'mbid-x')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mb_fetched (mbid, kind) VALUES ('mbid-x', 'artist')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap(); // deuxième « démarrage »
        let lib = Library { conn };

        assert!(
            lib.mb_artistes_en_attente("artist", 10).unwrap().is_empty(),
            "la purge ne doit jouer qu'une fois"
        );
    }

    #[test]
    fn migrations_bien_formees() {
        migrations().validate().unwrap();
    }

    /// Une base neuve sort numérotée ; rouverte, elle ne rejoue rien. Témoin :
    /// une année « 0 », que la migration n°1 efface, posée après la première
    /// ouverture et toujours là après la seconde.
    #[test]
    fn migration_jouee_une_seule_fois() {
        let chemin =
            std::env::temp_dir().join(format!("rusty-music-migr-{}.db", std::process::id()));
        let nettoyer = || {
            for suffixe in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffixe}", chemin.display()));
            }
        };
        nettoyer();
        {
            let lib = Library::open(&chemin).unwrap();
            let v: i64 = lib
                .conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert!(v >= 1, "user_version = {v}");
            lib.conn
                .execute("INSERT INTO tracks(path, year) VALUES ('/m/a.mp3', 0)", [])
                .unwrap();
        }
        let lib = Library::open(&chemin).unwrap();
        let annee: Option<i64> = lib
            .conn
            .query_row("SELECT year FROM tracks", [], |r| r.get(0))
            .unwrap();
        drop(lib);
        nettoyer();
        assert_eq!(annee, Some(0), "la migration n°1 a été rejouée");
    }

    /// Une base de la toute première génération (schéma du 19 août 2026,
    /// sans numéro de version) : la migration n°1 la complète sans rien
    /// perdre.
    #[test]
    fn migration_reconnait_la_premiere_generation() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (
                 id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size_bytes INTEGER,
                 mtime INTEGER, title TEXT, artist TEXT, album TEXT, album_artist TEXT,
                 genre TEXT, year INTEGER, track_no INTEGER, duration_ms INTEGER,
                 sample_rate INTEGER, channels INTEGER, mb_recording_id TEXT,
                 mb_artist_id TEXT, mb_album_artist_id TEXT,
                 added_at INTEGER NOT NULL DEFAULT (strftime('%s','now')), analyzed_at INTEGER);
             CREATE TABLE descriptors (
                 track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                 bpm REAL, musical_key TEXT, energy REAL, loudness REAL);
             CREATE TABLE artist_links (
                 src_mbid TEXT NOT NULL, dst_mbid TEXT NOT NULL, relation TEXT NOT NULL,
                 PRIMARY KEY (src_mbid, dst_mbid, relation));
             INSERT INTO tracks(path, title, artist, album, year)
                 VALUES ('/m/a.flac', 'Bizied', 'Denez', 'Kanañ a ri!', 0);
             INSERT INTO descriptors(track_id, bpm) VALUES (1, 92.0);",
        )
        .unwrap();

        let conn = preparer(conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert!(v >= 1);
        for (table, colonne) in [
            ("tracks", "mb_release_id"),
            ("tracks", "bit_depth"),
            ("descriptors", "algo_version"),
            ("artist_links", "dst_name"),
        ] {
            let n: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                    [colonne],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{table}.{colonne} manque");
        }
        let (bpm, annee): (f64, Option<i64>) = conn
            .query_row(
                "SELECT d.bpm, t.year FROM tracks t JOIN descriptors d ON d.track_id = t.id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((bpm, annee), (92.0, None));
        let lib = Library { conn };
        assert_eq!(lib.search("kanan", 10).unwrap().len(), 1);
    }

    #[test]
    fn search_ignore_les_accents() {
        let lib = bibliotheque_test();
        // Le point de tout l'exercice : la bibliothèque est pleine de titres
        // accentués, et on ne tape pas les accents en cherchant.
        assert_eq!(lib.search("kanan", 50).unwrap().len(), 1);
        assert_eq!(lib.search("Kanañ", 50).unwrap().len(), 1);
        assert_eq!(lib.search("KANAN A RI", 50).unwrap().len(), 1);
    }

    #[test]
    fn search_ne_laisse_pas_la_saisie_devenir_une_requete() {
        let lib = bibliotheque_test();
        // Ces saisies contiennent des opérateurs FTS5 ou de la ponctuation
        // seule. Aucune ne doit remonter d'erreur de syntaxe ni tout ramener.
        for saisie in [
            "%",
            "*",
            "-",
            "\"",
            "AND",
            "a OR b",
            "NEAR(x y)",
            "^",
            "moon\"",
        ] {
            let r = lib.search(saisie, 50);
            assert!(
                r.is_ok(),
                "« {saisie} » a fait échouer la recherche : {r:?}"
            );
        }
        assert!(lib.search("%", 50).unwrap().is_empty());
        assert!(lib.search("AND", 50).unwrap().is_empty());
    }

    #[test]
    fn search_porte_sur_des_mots_entiers() {
        let lib = bibliotheque_test();
        // « exy » ne trouve pas « Sexy » : on cherche des mots, pas des
        // sous-chaînes. Seul le dernier mot vaut préfixe, pour la frappe.
        assert!(lib.search("exy", 50).unwrap().is_empty());
        assert_eq!(lib.search("Sex", 50).unwrap().len(), 1);
        assert_eq!(lib.search("Sexy Boy", 50).unwrap().len(), 1);
    }

    #[test]
    fn track_par_identifiant() {
        let lib = bibliotheque_test();
        let trouve = lib.search("Venus", 1).unwrap();
        let id = trouve[0].id;
        assert_eq!(
            lib.track(id).unwrap().unwrap().title.as_deref(),
            Some("Venus")
        );
        assert!(lib.track(999_999).unwrap().is_none());
    }

    #[test]
    fn remove_root_emporte_les_morceaux_de_la_racine_seulement() {
        let lib = bibliotheque_test();
        lib.add_root(Path::new("/m/Air")).unwrap();
        lib.add_root(Path::new("/m/Comp")).unwrap();

        let racines = lib.roots().unwrap();
        assert_eq!(racines.len(), 2);
        assert_eq!(racines[0].path, "/m/Air");
        assert_eq!(racines[0].tracks, 3);

        // Changer de source : on retire l'ancienne racine et ses morceaux.
        assert_eq!(lib.remove_root(Path::new("/m/Air")).unwrap(), 3);
        assert_eq!(lib.count().unwrap(), 3);
        assert_eq!(lib.roots().unwrap().len(), 1);
        // Les autres racines sont intactes.
        assert_eq!(lib.albums(Some("Orbital")).unwrap().len(), 1);
    }

    #[test]
    fn save_features_marque_le_morceau_analyse() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert(&TrackMeta {
                path: "/m/a.flac".into(),
                title: Some("A".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(lib.pending_analysis("clap", 10).unwrap().len(), 1);

        lib.save_features(id, "clap", &[0.1, 0.2, 0.3], -0.5, 0.25, 2)
            .unwrap();

        // Le morceau sort de la file d'attente, et se retrouve sur la carte.
        assert!(lib.pending_analysis("clap", 10).unwrap().is_empty());
        // Mais il reste en attente pour tout autre modèle : c'est le point.
        // Avec l'ancien critère (`analyzed_at`), changer de représentation
        // laissait la moitié de la bibliothèque hors de la passe suivante.
        assert_eq!(lib.pending_analysis("clap-9f", 10).unwrap().len(), 1);
        let pts = lib.map_points("clap").unwrap();
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].0, id);
        assert!((pts[0].1 + 0.5).abs() < 1e-6);
        assert_eq!(pts[0].3, 2);

        // Un autre modèle cohabite sans écraser le premier.
        lib.save_features(id, "musicnn", &[1.0], 0.0, 0.0, 0)
            .unwrap();
        assert_eq!(lib.map_points("clap").unwrap().len(), 1);
        assert_eq!(lib.map_points("musicnn").unwrap().len(), 1);

        // Réécrire le même couple met à jour au lieu de dupliquer.
        lib.save_features(id, "clap", &[0.9], 0.1, 0.1, 5).unwrap();
        let pts = lib.map_points("clap").unwrap();
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].3, 5);
    }

    #[test]
    fn upsert_is_idempotent() {
        let lib = Library::open_in_memory().unwrap();
        let m = TrackMeta {
            path: "/musique/a.flac".into(),
            title: Some("A".into()),
            ..Default::default()
        };
        let first = lib.upsert(&m).unwrap();
        let second = lib.upsert(&m).unwrap();
        assert_eq!(first, second);
        assert_eq!(lib.count().unwrap(), 1);
    }

    #[test]
    fn pending_analysis_exclut_un_fichier_deja_en_echec() {
        let lib = Library::open_in_memory().unwrap();
        let m = TrackMeta {
            path: "/musique/casse.mp3".into(),
            title: Some("Casse".into()),
            ..Default::default()
        };
        lib.upsert(&m).unwrap();
        assert_eq!(lib.pending_analysis("clap", i64::MAX).unwrap().len(), 1);

        // Un fichier qui a déjà fait planter une passe ne doit pas être
        // retenté à chaque relance — voir `passe::empreintes`.
        lib.enregistrer_echec_scan(Path::new("/musique/casse.mp3"), "décodage impossible")
            .unwrap();
        assert!(lib.pending_analysis("clap", i64::MAX).unwrap().is_empty());

        // L'utilisateur peut lui redonner sa chance.
        lib.effacer_echec_scan(Path::new("/musique/casse.mp3"))
            .unwrap();
        assert_eq!(lib.pending_analysis("clap", i64::MAX).unwrap().len(), 1);
    }

    #[test]
    fn qualite_piste_rend_ce_que_le_scan_a_lu() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert(&TrackMeta {
                path: "/m/a.flac".into(),
                codec: Some("FLAC".into()),
                sample_rate: Some(44_100),
                channels: Some(2),
                bit_depth: Some(16),
                bitrate: None,
                ..Default::default()
            })
            .unwrap();

        let q = lib.qualite_piste(id).unwrap().unwrap();
        assert_eq!(q.codec.as_deref(), Some("FLAC"));
        assert_eq!(q.sample_rate, Some(44_100));
        assert_eq!(q.channels, Some(2));
        assert_eq!(q.bit_depth, Some(16));
        assert_eq!(q.bitrate, None);

        // Morceau absent : None, pas une erreur.
        assert!(lib.qualite_piste(999).unwrap().is_none());
    }

    #[test]
    fn histogrammer_range_hors_gamme_et_sans_valeur() {
        let valeurs = vec![Some(5.0), Some(12.0), Some(19.0), Some(29.0), None];
        // Tranches [10,15) et [15,20) ; 5.0 est sous le plancher, 29.0 dépasse.
        let h = histogrammer(&valeurs, 10.0, 5.0, 2);
        assert_eq!(h.comptes, vec![1, 1]); // 12 -> [10,15), 19 -> [15,20)
        assert_eq!(h.hors_gamme, 2); // 5.0 (< min) et 29.0 (>= min + 2*pas)
        assert_eq!(h.sans_valeur, 1);
    }

    /// Sans genre MusicBrainz en base, le repli sur le tag du fichier
    /// s'applique à toute la bibliothèque — et les morceaux sans tag
    /// comptent dans « — » plutôt que de disparaître silencieusement.
    #[test]
    fn stats_genres_replie_sur_le_tag_et_compte_les_morceaux_sans_genre() {
        let lib = Library::open_in_memory().unwrap();
        for (path, genre) in [
            ("/m/a.mp3", Some("Rock")),
            ("/m/b.mp3", Some("Rock")),
            ("/m/c.mp3", Some("Jazz")),
            ("/m/d.mp3", None),
        ] {
            lib.upsert(&TrackMeta {
                path: path.into(),
                genre: genre.map(str::to_string),
                ..Default::default()
            })
            .unwrap();
        }
        let stats = lib.stats_genres().unwrap();
        assert_eq!(
            stats,
            vec![
                ("Rock".to_string(), 2),
                ("Jazz".to_string(), 1),
                ("—".to_string(), 1),
            ]
        );
    }

    /// Un morceau jamais rescanné depuis l'arrivée du champ `codec` n'a « pas
    /// de format identifié » : un tag NULL, pas une valeur inventée.
    #[test]
    fn stats_codecs_distingue_le_format_du_non_mesure() {
        let lib = Library::open_in_memory().unwrap();
        for (path, codec) in [
            ("/m/a.mp3", Some("MP3")),
            ("/m/b.mp3", Some("MP3")),
            ("/m/c.flac", Some("FLAC")),
            ("/m/d.mp3", None),
        ] {
            lib.upsert(&TrackMeta {
                path: path.into(),
                codec: codec.map(str::to_string),
                ..Default::default()
            })
            .unwrap();
        }
        let stats = lib.stats_codecs().unwrap();
        assert_eq!(
            stats,
            vec![
                ("MP3".to_string(), 2),
                ("FLAC".to_string(), 1),
                ("non mesuré".to_string(), 1),
            ]
        );
    }

    #[test]
    fn stats_bitrate_range_bien_le_mp3_et_ecarte_le_flac() {
        let lib = Library::open_in_memory().unwrap();
        for (path, bitrate) in [
            ("/m/a.mp3", Some(128)),
            ("/m/b.mp3", Some(320)),
            ("/m/c.flac", Some(1000)), // bien au-delà de BITRATE_TRANCHES*BITRATE_PAS
            ("/m/d.wav", None),
        ] {
            lib.upsert(&TrackMeta {
                path: path.into(),
                bitrate,
                ..Default::default()
            })
            .unwrap();
        }
        let h = lib.stats_bitrate().unwrap();
        assert_eq!(h.comptes.iter().sum::<i64>(), 2); // 128 et 320
        assert_eq!(h.hors_gamme, 1); // le FLAC à 1000 kb/s
        assert_eq!(h.sans_valeur, 1); // le WAV sans débit
    }

    /// Le `LEFT JOIN` doit distinguer « jamais mesuré » (aucune ligne dans
    /// `descriptors`) et « mesuré, sans tempo trouvé » (`bpm` NULL) — les deux
    /// sont « sans valeur » pour l'histogramme, mais un morceau avec un vrai
    /// tempo doit atterrir dans sa tranche.
    #[test]
    fn stats_tempo_distingue_mesure_et_non_mesure() {
        let lib = Library::open_in_memory().unwrap();
        let jamais = lib
            .upsert(&TrackMeta {
                path: "/m/jamais.mp3".into(),
                ..Default::default()
            })
            .unwrap();
        let sans_tempo = lib
            .upsert(&TrackMeta {
                path: "/m/silence.mp3".into(),
                ..Default::default()
            })
            .unwrap();
        let mesure = lib
            .upsert(&TrackMeta {
                path: "/m/mesure.mp3".into(),
                ..Default::default()
            })
            .unwrap();
        let _ = jamais; // jamais analysé : aucune ligne `descriptors`, exprès.
        lib.save_descripteurs(
            sans_tempo, None, None, 0.0, -100.0, None, None, None, None, None, None, None, 1,
        )
        .unwrap();
        lib.save_descripteurs(
            mesure,
            Some(122.0),
            None,
            0.5,
            -12.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            1,
        )
        .unwrap();

        let h = lib.stats_tempo().unwrap();
        assert_eq!(h.sans_valeur, 2);
        assert_eq!(h.comptes.iter().sum::<i64>(), 1);
        let tranche = ((122.0 - TEMPO_MIN) / TEMPO_PAS) as usize;
        assert_eq!(h.comptes[tranche], 1);
    }

    /// Une mesure écrite par un algorithme plus ancien redevient « en
    /// attente » — c'est ce qui permet à un correctif (l'inversion d'octave
    /// du tempo, par exemple) de se rattraper tout seul, sans bouton
    /// « refaire ce qui est déjà mesuré ».
    #[test]
    fn pending_descripteurs_reprend_une_mesure_perimee() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert(&TrackMeta {
                path: "/m/perime.mp3".into(),
                ..Default::default()
            })
            .unwrap();
        lib.save_embedding(id, "modele", &[0.1, 0.2]).unwrap();
        lib.save_descripteurs(
            id,
            Some(90.0),
            None,
            0.5,
            -12.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            1,
        )
        .unwrap();

        // Version 1 : déjà mesuré, rien à refaire.
        assert!(lib
            .pending_descripteurs("modele", 1, 10)
            .unwrap()
            .is_empty());
        let (faits, total) = lib.compter_descripteurs("modele", 1).unwrap();
        assert_eq!((faits, total), (1, 1));

        // Version 2 (l'algorithme a changé) : la mesure d'avant ne compte plus.
        let pending = lib.pending_descripteurs("modele", 2, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        let (faits, total) = lib.compter_descripteurs("modele", 2).unwrap();
        assert_eq!((faits, total), (0, 1));

        // Remesuré en version 2 : de nouveau à jour.
        lib.save_descripteurs(
            id,
            Some(180.0),
            None,
            0.5,
            -12.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            2,
        )
        .unwrap();
        assert!(lib
            .pending_descripteurs("modele", 2, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn pending_loudness_reprend_une_mesure_perimee() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert(&TrackMeta {
                path: "/m/perime.flac".into(),
                ..Default::default()
            })
            .unwrap();
        lib.save_loudness(id, -14.0, -1.0, 1).unwrap();

        // Version 1 : déjà mesuré, rien à refaire.
        assert!(lib.pending_loudness(1, 10).unwrap().is_empty());
        let (faits, total) = lib.compter_loudness(1).unwrap();
        assert_eq!((faits, total), (1, 1));

        // Version 2 (la méthode de mesure a changé) : la mesure d'avant ne
        // compte plus.
        let pending = lib.pending_loudness(2, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        let (faits, total) = lib.compter_loudness(2).unwrap();
        assert_eq!((faits, total), (0, 1));

        // Remesuré en version 2 : de nouveau à jour.
        lib.save_loudness(id, -14.0, -1.0, 2).unwrap();
        assert!(lib.pending_loudness(2, 10).unwrap().is_empty());
    }

    /// Un morceau jamais passé dans la passe de loudness doit jouer sans
    /// normalisation, jamais bloquer la lecture.
    #[test]
    fn gain_lecture_absent_ne_bloque_rien() {
        let lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/pasmesure.flac".into(),
            ..Default::default()
        })
        .unwrap();
        let gain = lib
            .gain_lecture(
                Path::new("/m/pasmesure.flac"),
                crate::loudness::ModeGain::Piste,
                crate::loudness::CIBLE_LUFS,
            )
            .unwrap();
        assert!(gain.is_none());
    }

    /// En mode album, le plafond anti-écrêtage reste calculé sur le pic
    /// **de la piste réellement jouée** — jamais un pic d'album, qui
    /// n'existe pas et n'aurait pas de sens (chaque piste peut écrêter
    /// indépendamment des autres).
    #[test]
    fn gain_lecture_mode_album_utilise_le_pic_de_la_piste() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert(&TrackMeta {
                path: "/m/piste-forte.flac".into(),
                album: Some("Album".into()),
                album_artist: Some("Artiste".into()),
                ..Default::default()
            })
            .unwrap();
        // Piste déjà proche de 0 dBTP : toute normalisation généreuse doit
        // être plafonnée.
        lib.save_loudness(id, -25.0, -0.2, crate::loudness::VERSION_LOUDNESS)
            .unwrap();
        lib.save_album_loudness(
            "Album",
            "Artiste",
            -25.0,
            1,
            crate::loudness::VERSION_LOUDNESS,
        )
        .unwrap();

        let gain = lib
            .gain_lecture(
                Path::new("/m/piste-forte.flac"),
                crate::loudness::ModeGain::Album,
                crate::loudness::CIBLE_LUFS,
            )
            .unwrap()
            .expect("mesurée");
        let attendu = crate::loudness::db_vers_lineaire(0.2); // plafond, pas le naïf (+7 dB)
        assert!(
            (gain - attendu).abs() < 1e-6,
            "gain = {gain}, attendu {attendu}"
        );
    }

    /// Mode album choisi, mais l'album n'a pas encore de ligne
    /// `album_loudness` (pas assez de pistes mesurées pour le recalculer) :
    /// on rejoue le gain piste plutôt que de bloquer ou d'inventer une
    /// valeur d'album.
    #[test]
    fn gain_lecture_mode_album_replie_sur_la_piste_si_lalbum_manque() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert(&TrackMeta {
                path: "/m/orphelin.flac".into(),
                album: Some("Album inachevé".into()),
                album_artist: Some("Artiste".into()),
                ..Default::default()
            })
            .unwrap();
        lib.save_loudness(id, -20.0, -1.0, crate::loudness::VERSION_LOUDNESS)
            .unwrap();

        let piste = lib
            .gain_lecture(
                Path::new("/m/orphelin.flac"),
                crate::loudness::ModeGain::Piste,
                crate::loudness::CIBLE_LUFS,
            )
            .unwrap();
        let album = lib
            .gain_lecture(
                Path::new("/m/orphelin.flac"),
                crate::loudness::ModeGain::Album,
                crate::loudness::CIBLE_LUFS,
            )
            .unwrap();
        assert_eq!(piste, album);
    }

    /// Une famille de huit morceaux, quatre genres — Jazz minoritaire, seul
    /// hors du top 3 (`GENRES_DOMINANTS`). C'est lui, et seulement lui, qui
    /// doit ressortir suspect.
    #[test]
    fn genres_suspects_ignore_le_dominant_et_signale_lisole() {
        let lib = Library::open_in_memory().unwrap();
        // Effectifs tous distincts : aucune égalité à départager dans le
        // classement des dominants.
        let genres = [("Rock", 4), ("Pop", 3), ("Electronic", 2), ("Jazz", 1)];
        let mut i = 0;
        for (genre, n) in genres {
            for _ in 0..n {
                let id = lib
                    .upsert(&TrackMeta {
                        path: format!("/m/{i}.mp3").into(),
                        title: Some(format!("Piste {i}")),
                        artist: Some("Artiste".into()),
                        genre: Some(genre.to_string()),
                        ..Default::default()
                    })
                    .unwrap();
                lib.save_features(id, "clap", &[0.0], 0.0, 0.0, 0).unwrap();
                i += 1;
            }
        }
        let suspects = lib.genres_suspects("clap").unwrap();
        assert_eq!(suspects.len(), 1, "un seul suspect attendu : {suspects:?}");
        assert_eq!(suspects[0].2, "Jazz");
        assert_eq!(suspects[0].3, "Rock · Pop · Electronic");
    }

    /// Une famille trop petite (`PLANCHER_ABSOLU`) ne doit rien signaler : le
    /// « dominant » d'un groupe de deux morceaux n'apprend rien.
    #[test]
    fn genres_suspects_ignore_les_petites_familles() {
        let lib = Library::open_in_memory().unwrap();
        for (i, genre) in ["Rock", "Jazz"].iter().enumerate() {
            let id = lib
                .upsert(&TrackMeta {
                    path: format!("/m/{i}.mp3").into(),
                    genre: Some(genre.to_string()),
                    ..Default::default()
                })
                .unwrap();
            lib.save_features(id, "clap", &[0.0], 0.0, 0.0, 0).unwrap();
        }
        assert!(lib.genres_suspects("clap").unwrap().is_empty());
    }

    #[test]
    fn editions_multiples_rapproche_les_mentions_dedition() {
        let lib = Library::open_in_memory().unwrap();
        for (i, (artiste, album)) in [
            ("Radiohead", "Kid A"),
            ("Radiohead", "Kid A (Remaster)"),
            ("Radiohead", "OK Computer"),
            // Autre artiste, même titre : ne doit pas se mélanger.
            ("Air", "Kid A"),
        ]
        .iter()
        .enumerate()
        {
            lib.upsert(&TrackMeta {
                path: format!("/m/{i}.mp3").into(),
                artist: Some(artiste.to_string()),
                album: Some(album.to_string()),
                ..Default::default()
            })
            .unwrap();
        }
        let editions = lib.editions_multiples().unwrap();
        assert_eq!(
            editions.len(),
            1,
            "une seule paire d'éditions : {editions:?}"
        );
        let (artiste, titre, versions) = &editions[0];
        assert_eq!(artiste, "Radiohead");
        assert_eq!(titre, "Kid A");
        assert_eq!(versions.len(), 2);
    }

    #[test]
    fn titre_album_normalise_tronque_a_la_premiere_parenthese_ou_crochet() {
        assert_eq!(titre_album_normalise("Kid A"), "kid a");
        assert_eq!(titre_album_normalise("Kid A (Remaster)"), "kid a");
        assert_eq!(
            titre_album_normalise("OK Computer [Deluxe Edition]"),
            "ok computer"
        );
    }

    /// La médiane s'ajuste à l'échelle réelle des valeurs, même une échelle
    /// resserrée loin de [0, 1] — le cas mesuré qui a écarté un seuil fixe.
    #[test]
    fn stats_humeur_se_calibre_sur_sa_propre_echelle() {
        let lib = Library::open_in_memory().unwrap();
        // Énergie resserrée entre 0,03 et 0,41, comme mesuré en vrai — un
        // seuil fixe à 0,5 rangerait tout le monde dans « lent/léger ».
        let pistes = [
            (60.0, 0.05),  // lent, léger  -> Calme
            (60.0, 0.40),  // lent, dense  -> Intense
            (150.0, 0.05), // rapide, léger -> Enlevé
            (150.0, 0.40), // rapide, dense -> Énergique
        ];
        for (i, (bpm, energie)) in pistes.iter().enumerate() {
            let id = lib
                .upsert(&TrackMeta {
                    path: format!("/m/{i}.mp3").into(),
                    ..Default::default()
                })
                .unwrap();
            lib.save_descripteurs(
                id,
                Some(*bpm as f32),
                None,
                *energie as f32,
                -10.0,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                1,
            )
            .unwrap();
        }
        let humeur = lib.stats_humeur().unwrap();
        let compte = |nom: &str| {
            humeur
                .iter()
                .find(|(n, _)| n == nom)
                .map(|(_, c)| *c)
                .unwrap_or(0)
        };
        assert_eq!(compte("Calme"), 1);
        assert_eq!(compte("Intense"), 1);
        assert_eq!(compte("Enlevé"), 1);
        assert_eq!(compte("Énergique"), 1);
    }

    #[test]
    fn parametres_carte_retombe_sur_les_defauts_puis_retient_ce_quon_change() {
        let lib = Library::open_in_memory().unwrap();
        let p = lib.parametres_carte().unwrap();
        assert_eq!(p.familles, ParametresCarte::default().familles);
        assert_eq!(p.perplexite, ParametresCarte::default().perplexite);

        lib.set_parametre_carte("familles", 20.0).unwrap();
        let p = lib.parametres_carte().unwrap();
        assert_eq!(p.familles, 20);
        // Le reste n'a pas bougé : changer une clé ne doit pas réinitialiser
        // les autres.
        assert_eq!(p.epoques, ParametresCarte::default().epoques);

        // Rejouer la même clé met à jour, ne duplique pas.
        lib.set_parametre_carte("familles", 8.0).unwrap();
        assert_eq!(lib.parametres_carte().unwrap().familles, 8);
    }

    #[test]
    fn liens_artiste_se_met_en_cache_meme_vide() {
        let mut lib = Library::open_in_memory().unwrap();
        assert!(!lib.liens_artiste_en_cache("a").unwrap());

        lib.enregistrer_liens_artiste(
            "a",
            &[crate::musicbrainz::Relation {
                dst_mbid: "b".to_string(),
                dst_name: "Artiste B".to_string(),
                relation: "member of band".to_string(),
            }],
        )
        .unwrap();
        assert!(lib.liens_artiste_en_cache("a").unwrap());
        let liens = lib.liens_artiste("a").unwrap();
        assert_eq!(
            liens,
            vec![(
                "b".to_string(),
                "Artiste B".to_string(),
                "member of band".to_string()
            )]
        );

        // Un artiste sans aucune relation connue doit rester marqué comme
        // déjà interrogé, sans quoi il serait réinterrogé à chaque visite.
        lib.enregistrer_liens_artiste("c", &[]).unwrap();
        assert!(lib.liens_artiste_en_cache("c").unwrap());
        assert!(lib.liens_artiste("c").unwrap().is_empty());
    }

    fn sortie(rg: &str, date_norm: &str, collab: Option<&str>) -> SortieARanger {
        SortieARanger {
            rg_mbid: rg.into(),
            titre: format!("Disque {rg}"),
            date_sortie: Some(date_norm.into()),
            date_sortie_norm: Some(date_norm.into()),
            type_primaire: Some("Album".into()),
            types_secondaires: None,
            collaborateurs: collab.map(str::to_string),
        }
    }

    #[test]
    fn decouvrir_ajouter_sortie_preserve_vu_a_la_reinsertion() {
        let lib = Library::open_in_memory().unwrap();
        let hier = lib.date_il_y_a(1).unwrap();

        assert!(lib
            .decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg1", &hier, None))
            .unwrap());
        lib.decouvrir_marquer_passe("sorties").unwrap();
        lib.decouvrir_tout_vu().unwrap();

        // Une seconde passe revoit la même sortie : rien de neuf, et le « vu »
        // tient — sans quoi la pastille « nouveau » se rallumerait à chaque
        // actualisation.
        assert!(!lib
            .decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg1", &hier, None))
            .unwrap());
        assert!(lib
            .decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg2", &hier, None))
            .unwrap());

        let fil = lib.decouvrir_fil(30).unwrap();
        assert_eq!(fil.sorties.len(), 2);
        assert!(fil.sorties.iter().find(|s| s.rg_mbid == "rg1").unwrap().vu);
        assert!(!fil.sorties.iter().find(|s| s.rg_mbid == "rg2").unwrap().vu);
    }

    #[test]
    fn decouvrir_fil_filtre_la_fenetre_et_separe_les_collaborations() {
        let lib = Library::open_in_memory().unwrap();
        let hier = lib.date_il_y_a(1).unwrap();
        let vieux = lib.date_il_y_a(120).unwrap();

        for s in [
            sortie("recent", &hier, None),
            sortie("collab", &hier, Some("Invité")),
            sortie("vieux", &vieux, None),
        ] {
            lib.decouvrir_ajouter_sortie("a", "Artiste A", &s).unwrap();
        }
        lib.decouvrir_marquer_passe("sorties").unwrap();

        let fil = lib.decouvrir_fil(30).unwrap();
        assert_eq!(
            fil.sorties.iter().map(|s| &s.rg_mbid).collect::<Vec<_>>(),
            ["recent"]
        );
        assert_eq!(
            fil.collaborations
                .iter()
                .map(|s| &s.rg_mbid)
                .collect::<Vec<_>>(),
            ["collab"]
        );
        assert!(fil.derniere_passe.is_some());
    }

    #[test]
    fn decouvrir_en_attente_respecte_la_peremption_et_lordre() {
        let mut lib = Library::open_in_memory().unwrap();
        // Deux morceaux pour « gros », un seul pour « petit » : le plus fourni
        // d'abord.
        for (i, mbid, nom) in [
            (1, "gros", "Gros"),
            (2, "gros", "Gros"),
            (3, "petit", "Petit"),
        ] {
            lib.upsert(&TrackMeta {
                path: format!("/m/{i}.mp3").into(),
                title: Some("t".into()),
                artist: Some(nom.into()),
                album: Some("al".into()),
                album_artist: Some(nom.into()),
                mb_album_artist_id: Some(mbid.into()),
                ..Default::default()
            })
            .unwrap();
        }

        let attente = lib.decouvrir_en_attente("voisins", 30, 0).unwrap();
        assert_eq!(
            attente.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
            ["gros", "petit"]
        );

        // « gros » vient d'être interrogé : il sort de la liste jusqu'à
        // péremption.
        lib.decouvrir_poser_voisins("gros", &[]).unwrap();
        let attente = lib.decouvrir_en_attente("voisins", 30, 0).unwrap();
        assert_eq!(
            attente.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
            ["petit"]
        );
    }

    #[test]
    fn decouvrir_voisins_ecarte_les_artistes_de_la_bibliotheque() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.mp3".into(),
            title: Some("t".into()),
            artist: Some("Connu".into()),
            album: Some("al".into()),
            album_artist: Some("Connu".into()),
            mb_album_artist_id: Some("src".into()),
            ..Default::default()
        })
        .unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/2.mp3".into(),
            title: Some("t".into()),
            artist: Some("Déjà là".into()),
            album: Some("al".into()),
            album_artist: Some("Déjà là".into()),
            mb_album_artist_id: Some("dedans".into()),
            ..Default::default()
        })
        .unwrap();

        lib.decouvrir_poser_voisins(
            "src",
            &[
                ("dehors".into(), "Dehors".into(), 0.9, "listenbrainz".into()),
                (
                    "dedans".into(),
                    "Déjà là".into(),
                    0.8,
                    "listenbrainz".into(),
                ),
            ],
        )
        .unwrap();

        let fil = lib.decouvrir_fil(30).unwrap();
        assert_eq!(fil.voisins.len(), 1);
        assert_eq!(fil.voisins[0].dst_mbid, "dehors");
        assert_eq!(fil.voisins[0].portes, vec!["Connu".to_string()]);
    }

    #[test]
    fn rangs_percentiles_place_par_valeur_croissante() {
        let r = rangs_percentiles([(10, 5.0), (20, 5.0), (30, 100.0), (40, 1.0)].into_iter());
        assert_eq!(r[&40], 0.0, "la plus petite valeur → 0");
        assert_eq!(r[&30], 1.0, "la plus grande → 1");
        // Deux valeurs égales : même rang (part des valeurs strictement plus petites).
        assert_eq!(r[&10], r[&20]);
        assert!((r[&10] - (1.0 / 3.0)).abs() < 1e-9);
    }

    /// La popularité par morceau : posée par source, résolue au meilleur
    /// échelon, mélangée en rang. Un morceau sans aucune source n'a pas de ligne.
    #[test]
    fn recalcul_track_popularite_resout_echelon_et_melange_les_rangs() {
        let mut lib = Library::open_in_memory().unwrap();
        let ajoute = |lib: &Library, id: i64, rec: &str, art: &str, album: &str| {
            lib.conn
                .execute(
                    "INSERT INTO tracks (id, path, mb_recording_id, mb_artist_id, mb_album_artist_id, album, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?4, ?5, 0)",
                    params![id, format!("/m/{id}.flac"), rec, art, album],
                )
                .unwrap();
        };
        ajoute(&lib, 1, "rec-a", "art-x", "Disque X");
        ajoute(&lib, 2, "rec-b", "art-x", "Disque X"); // même album, pas de reco propre côté LB
        ajoute(&lib, 3, "rec-c", "art-y", "Disque Y");
        ajoute(&lib, 4, "rec-d", "art-z", "Disque Z"); // aucune source → pas de ligne

        lib.conn
            .execute(
                "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
                 VALUES ('rg-x', 'art-x', 'Disque X', 'disquex'), ('rg-y', 'art-y', 'Disque Y', 'disquey')",
                [],
            )
            .unwrap();

        lib.pop_poser(
            "listenbrainz",
            "recording",
            &["rec-a".into(), "rec-c".into()],
            &[
                PopulariteBrute {
                    mbid: "rec-a",
                    ecoutes: 100,
                    auditeurs: Some(10),
                },
                PopulariteBrute {
                    mbid: "rec-c",
                    ecoutes: 5,
                    auditeurs: Some(1),
                },
            ],
        )
        .unwrap();
        lib.pop_poser(
            "listenbrainz",
            "release-group",
            &["rg-x".into(), "rg-y".into()],
            &[
                PopulariteBrute {
                    mbid: "rg-x",
                    ecoutes: 50,
                    auditeurs: Some(5),
                },
                PopulariteBrute {
                    mbid: "rg-y",
                    ecoutes: 9,
                    auditeurs: Some(2),
                },
            ],
        )
        .unwrap();
        lib.pop_poser(
            "deezer",
            "recording",
            &["rec-a".into()],
            &[PopulariteBrute {
                mbid: "rec-a",
                ecoutes: 900_000,
                auditeurs: None,
            }],
        )
        .unwrap();

        let couverts = lib.recalculer_track_popularite().unwrap();
        assert_eq!(couverts, 3, "le morceau 4 (aucune source) est écarté");

        let lignes: Vec<(i64, f64, String)> = lib
            .conn
            .prepare("SELECT track_id, relative, echelon FROM track_popularite ORDER BY track_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();

        assert_eq!(lignes.len(), 3);
        assert_eq!(lignes[0].0, 1);
        assert_eq!(lignes[0].2, "recording"); // reco LB + Deezer
        assert_eq!(lignes[1].0, 2);
        assert_eq!(lignes[1].2, "release-group"); // repli sur l'album X
        assert_eq!(lignes[2].2, "recording"); // reco LB connue
                                              // Le morceau 1 (le plus écouté partout) domine.
        assert!(lignes[0].1 > lignes[1].1 && lignes[0].1 > lignes[2].1);

        // `popularites` ne rend que les morceaux couverts parmi ceux demandés.
        let mut vus = lib.popularites(&[1, 2, 4, 999]).unwrap();
        vus.sort_by_key(|(id, ..)| *id);
        assert_eq!(
            vus.iter().map(|(id, ..)| *id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(lib.popularites(&[]).unwrap(), Vec::new());
    }

    /// Une entité déjà interrogée ne revient pas dans les candidats — sauf si
    /// `depuis` la déclare périmée.
    #[test]
    fn pop_candidats_excluent_ce_qui_est_deja_fait() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks (id, path, mb_recording_id, artist, title, added_at)
                 VALUES (1, '/m/1.flac', 'rec-1', 'Art', 'Titre', 0)",
                [],
            )
            .unwrap();

        assert_eq!(lib.pop_recordings_candidats(0, 100).unwrap().len(), 1);

        // Fait sur une seule source : encore candidat (l'autre reste à faire).
        lib.pop_poser("listenbrainz", "recording", &["rec-1".into()], &[])
            .unwrap();
        assert_eq!(lib.pop_recordings_candidats(0, 100).unwrap().len(), 1);

        // Fait sur les deux : plus candidat.
        lib.pop_poser("deezer", "recording", &["rec-1".into()], &[])
            .unwrap();
        assert!(lib.pop_recordings_candidats(0, 100).unwrap().is_empty());

        // `depuis` très grand : tout fetch compte comme périmé, l'entité revient.
        let futur = i64::MAX;
        assert_eq!(lib.pop_recordings_candidats(futur, 100).unwrap().len(), 1);
        assert!(lib
            .pop_deja_fait("listenbrainz", "recording", futur)
            .unwrap()
            .is_empty());
        assert_eq!(
            lib.pop_deja_fait("listenbrainz", "recording", 0)
                .unwrap()
                .len(),
            1
        );
    }

    /// La ligne d'alerte : rien à signaler tant que rien n'est vieux ; l'âge
    /// d'un fetch le fait basculer dans « périmé ».
    #[test]
    fn popularite_fraicheur_compte_le_perime() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks (id, path, added_at) VALUES (1, '/m/1.flac', 0)",
                [],
            )
            .unwrap();
        lib.pop_poser("listenbrainz", "recording", &["rec-neuf".into()], &[])
            .unwrap();
        // Un fetch daté d'il y a 200 jours.
        lib.conn
            .execute(
                "INSERT INTO popularite_fetched (mbid, kind, source, at)
                 VALUES ('rec-vieux', 'recording', 'listenbrainz',
                         strftime('%s','now') - 200*86400)",
                [],
            )
            .unwrap();

        let (couverts, _plus_ancienne, perimes) = lib.popularite_fraicheur(90).unwrap();
        assert_eq!(couverts, 0, "aucun track_popularite calculé ici");
        assert_eq!(perimes, 1, "seul le fetch de 200 j dépasse 90 j");
        // Seuil relevé au-delà de l'âge du vieux : plus rien de périmé.
        assert_eq!(lib.popularite_fraicheur(300).unwrap().2, 0);
    }

    /// Un artiste sans MBID n'entre jamais dans les candidats — aucun repli
    /// par nom, voir `docs/enrichissement-lecteur.md`.
    #[test]
    fn theaudiodb_candidats_ignore_les_artistes_sans_mbid() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute_batch(
                "INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (1, '/m/1.flac', 'art-1', 0);
                 INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (2, '/m/2.flac', NULL, 0);",
            )
            .unwrap();
        assert_eq!(
            lib.theaudiodb_candidats(0, 100).unwrap(),
            vec!["art-1".to_string()]
        );

        lib.theaudiodb_poser(
            &["art-1".to_string()],
            &[BioBrute {
                mb_artist_id: "art-1".into(),
                id_theaudiodb: Some("111".into()),
                biographie_en: Some("bio en".into()),
                biographie_fr: Some("bio fr".into()),
            }],
        )
        .unwrap();
        assert!(lib.theaudiodb_candidats(0, 100).unwrap().is_empty());

        let bios = lib.bio_pour_piste(1).unwrap();
        assert_eq!(bios.len(), 1);
        assert_eq!(bios[0].biographie_fr.as_deref(), Some("bio fr"));
        assert!(lib.bio_pour_piste(2).unwrap().is_empty());
    }

    /// `stats_sans_bio` compte un morceau sans MBID d'artiste comme sans
    /// biographie, et un morceau dont l'artiste n'a été interrogé qu'à vide
    /// (marqué « déjà demandé » sans qu'aucune biographie n'ait été posée)
    /// tout autant — seule une biographie effectivement posée retire un
    /// morceau du compte.
    #[test]
    fn stats_sans_bio_retombe_sur_absence_de_mbid_et_de_biographie() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute_batch(
                "INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (1, '/m/1.flac', 'art-1', 0);
                 INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (2, '/m/2.flac', 'art-2', 0);
                 INSERT INTO tracks (id, path, mb_artist_id, added_at) VALUES (3, '/m/3.flac', NULL, 0);",
            )
            .unwrap();
        assert_eq!(lib.stats_sans_bio().unwrap(), 3);

        // Interrogé mais sans biographie trouvée : ne compte toujours pas.
        lib.theaudiodb_poser(&["art-2".to_string()], &[]).unwrap();
        assert_eq!(lib.stats_sans_bio().unwrap(), 3);

        lib.theaudiodb_poser(
            &["art-1".to_string()],
            &[BioBrute {
                mb_artist_id: "art-1".into(),
                id_theaudiodb: Some("111".into()),
                biographie_en: Some("bio en".into()),
                biographie_fr: None,
            }],
        )
        .unwrap();
        assert_eq!(
            lib.stats_sans_bio().unwrap(),
            2,
            "seul le morceau 1 a désormais une biographie"
        );
    }

    /// `stats_sans_discogs` exige un crédit *ou* un label — l'un des deux
    /// suffit à sortir un morceau du compte, mais une édition liée sans que
    /// l'import n'ait rien trouvé y reste.
    #[test]
    fn stats_sans_discogs_exige_un_credit_ou_un_label() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute_batch(
                "INSERT INTO tracks (id, path, mb_release_id, added_at) VALUES (1, '/m/1.flac', 'rel-1', 0);
                 INSERT INTO tracks (id, path, mb_release_id, added_at) VALUES (2, '/m/2.flac', 'rel-2', 0);
                 INSERT INTO tracks (id, path, mb_release_id, added_at) VALUES (3, '/m/3.flac', NULL, 0);",
            )
            .unwrap();
        assert_eq!(lib.stats_sans_discogs().unwrap(), 3);

        // Édition reliée mais l'import n'y a rien trouvé : compte encore.
        lib.discogs_lier_edition("rel-2", Some(500)).unwrap();
        assert_eq!(lib.stats_sans_discogs().unwrap(), 3);

        lib.labels_poser(
            500,
            &[LabelDiscogs {
                nom: "Label".into(),
                catno: "".into(),
                discogs_label_id: None,
            }],
        )
        .unwrap();
        assert_eq!(
            lib.stats_sans_discogs().unwrap(),
            2,
            "le morceau 2 a désormais un label"
        );
    }

    /// `stats_sans_critique` résout le release-group par (artiste, titre
    /// normalisé) — même appariement que `critiques_pour_piste` — plutôt que
    /// de compter les critiques une piste à la fois.
    #[test]
    fn stats_sans_critique_resout_par_artiste_et_titre_normalise() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute_batch(
                "INSERT INTO tracks (id, path, mb_artist_id, album, added_at)
                   VALUES (1, '/m/1.flac', 'art-1', 'Titre', 0);
                 INSERT INTO tracks (id, path, mb_artist_id, album, added_at)
                   VALUES (2, '/m/2.flac', 'art-2', 'Autre Album', 0);
                 INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
                   VALUES ('rg-1', 'art-1', 'Titre', 'titre');",
            )
            .unwrap();
        // Le morceau 2 n'a pas de release-group connu du tout.
        assert_eq!(lib.stats_sans_critique().unwrap(), 2);

        lib.critiques_poser("rg-1", &[]).unwrap();
        assert_eq!(
            lib.stats_sans_critique().unwrap(),
            2,
            "release-group interrogé, mais rien trouvé"
        );

        lib.critiques_poser(
            "rg-1",
            &[CritiqueBrute {
                id: "crit-1".into(),
                auteur: None,
                licence_id: "CC BY-SA 3.0".into(),
                licence_nom: None,
                langue: None,
                texte: "Un album marquant.".into(),
                url_originale: None,
            }],
        )
        .unwrap();
        assert_eq!(
            lib.stats_sans_critique().unwrap(),
            1,
            "le morceau 1 a désormais une critique"
        );
    }

    /// Le lien Discogs se pose y compris quand il est absent (`None`) — pour
    /// ne pas revérifier une édition sans relation cataloguée à chaque passe.
    #[test]
    fn discogs_liaison_candidats_et_lien_absent_memorise() {
        let lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks (id, path, mb_release_id, added_at)
                 VALUES (1, '/m/1.flac', 'rel-1', 0)",
                [],
            )
            .unwrap();
        assert_eq!(
            lib.discogs_liaison_candidats(100).unwrap(),
            vec!["rel-1".to_string()]
        );

        lib.discogs_lier_edition("rel-1", None).unwrap();
        assert!(lib.discogs_liaison_candidats(100).unwrap().is_empty());
        assert!(lib.discogs_wanted_ids().unwrap().is_empty());
    }

    /// Les crédits se retrouvent depuis une piste via son édition MusicBrainz,
    /// et un import réécrit ceux d'une édition plutôt que de les cumuler.
    #[test]
    fn credits_poser_remplace_les_credits_dune_edition() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks (id, path, mb_release_id, added_at)
                 VALUES (1, '/m/1.flac', 'rel-1', 0)",
                [],
            )
            .unwrap();
        lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();

        lib.credits_poser(
            249_504,
            &[CreditDiscogs {
                personne: "Ancien".into(),
                role: "Mixed By".into(),
                pistes: String::new(),
                discogs_artist_id: None,
            }],
        )
        .unwrap();
        assert_eq!(lib.credits_pour_piste(1).unwrap().len(), 1);

        lib.credits_poser(
            249_504,
            &[CreditDiscogs {
                personne: "Nouveau".into(),
                role: "Producer".into(),
                pistes: "A1, A2".into(),
                discogs_artist_id: Some(1),
            }],
        )
        .unwrap();
        let credits = lib.credits_pour_piste(1).unwrap();
        assert_eq!(credits.len(), 1, "l'ancien crédit doit avoir été remplacé");
        assert_eq!(credits[0].personne, "Nouveau");
        assert_eq!(credits[0].pistes, "A1, A2");
    }

    /// Même structure que `credits_poser_remplace_les_credits_dune_edition` :
    /// un import réécrit les labels d'une édition plutôt que de les cumuler.
    #[test]
    fn labels_poser_remplace_les_labels_dune_edition() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks (id, path, mb_release_id, added_at)
                 VALUES (1, '/m/1.flac', 'rel-1', 0)",
                [],
            )
            .unwrap();
        lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();

        lib.labels_poser(
            249_504,
            &[LabelDiscogs {
                nom: "Ancien Label".into(),
                catno: "OLD1".into(),
                discogs_label_id: None,
            }],
        )
        .unwrap();
        assert_eq!(lib.labels_pour_piste(1).unwrap().len(), 1);

        lib.labels_poser(
            249_504,
            &[LabelDiscogs {
                nom: "Nouveau Label".into(),
                catno: "NEW1".into(),
                discogs_label_id: Some(1),
            }],
        )
        .unwrap();
        let labels = lib.labels_pour_piste(1).unwrap();
        assert_eq!(labels.len(), 1, "l'ancien label doit avoir été remplacé");
        assert_eq!(labels[0].nom, "Nouveau Label");
        assert_eq!(labels[0].catno, "NEW1");
    }

    /// La justification même de la fonctionnalité : une édition sur un
    /// sous-label (Big Dada) porte aussi son label parent (Ninja Tune), les
    /// deux doivent revenir, triés par nom — pas un seul « label principal ».
    #[test]
    fn labels_pour_piste_rend_plusieurs_labels_dune_edition() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks (id, path, mb_release_id, added_at)
                 VALUES (1, '/m/1.flac', 'rel-1', 0)",
                [],
            )
            .unwrap();
        lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();

        lib.labels_poser(
            249_504,
            &[
                LabelDiscogs {
                    nom: "Ninja Tune".into(),
                    catno: "ZEN123".into(),
                    discogs_label_id: Some(222),
                },
                LabelDiscogs {
                    nom: "Big Dada".into(),
                    catno: "BD456".into(),
                    discogs_label_id: Some(333),
                },
            ],
        )
        .unwrap();

        let labels = lib.labels_pour_piste(1).unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].nom, "Big Dada");
        assert_eq!(labels[1].nom, "Ninja Tune");
    }

    /// Le cas qui a motivé `discogs_importes` : une édition reliée mais
    /// jamais importée (liaison faite sous un binaire d'avant ce correctif,
    /// ou import interrompu) doit rester détectée comme « utile à importer »
    /// même si *cette* liaison-ci n'a rien relié de neuf — l'ancien critère
    /// (« la liaison vient-elle de relier quelque chose ? ») la manquait.
    #[test]
    fn discogs_import_utile_detecte_une_edition_reliee_jamais_importee() {
        let lib = Library::open_in_memory().unwrap();
        assert!(
            !lib.discogs_import_utile().unwrap(),
            "rien de relié : rien à importer"
        );

        lib.discogs_lier_edition("rel-1", Some(249_504)).unwrap();
        assert!(
            lib.discogs_import_utile().unwrap(),
            "reliée, jamais importée"
        );

        lib.discogs_marquer_importe(249_504).unwrap();
        assert!(
            !lib.discogs_import_utile().unwrap(),
            "marquée importée, même sans crédit ni label trouvé"
        );

        // Une deuxième édition reliée, elle, redevient utile — la marque de
        // la première ne doit pas masquer la nouvelle.
        lib.discogs_lier_edition("rel-2", Some(300_000)).unwrap();
        assert!(lib.discogs_import_utile().unwrap());
    }

    /// Le repli pochette ne dépend que des tags de la piste : release MBID
    /// telle quelle, et artiste d'album de préférence à l'artiste de la piste.
    #[test]
    fn repli_pochette_lit_release_et_artiste_album_depuis_les_tags() {
        let lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks (path, artist, album, album_artist, mb_release_id)
                 VALUES ('/m/a.mp3', 'Invité', 'Album', 'Groupe', 'rel-1'),
                        ('/m/b.mp3', 'Solo', 'Autre', '', NULL)",
                [],
            )
            .unwrap();
        let a = Path::new("/m/a.mp3");
        let b = Path::new("/m/b.mp3");
        assert_eq!(lib.release_pour_path(a).unwrap().as_deref(), Some("rel-1"));
        assert_eq!(lib.release_pour_path(b).unwrap(), None);
        assert_eq!(
            lib.release_pour_path(Path::new("/m/inconnu.mp3")).unwrap(),
            None
        );
        assert_eq!(
            lib.artiste_album_pour_path(a).unwrap(),
            Some(("Groupe".to_string(), "Album".to_string()))
        );
        lib.conn
            .execute(
                "INSERT INTO tracks (path, artist, album, album_artist)
                 VALUES ('/m/a2.mp3', 'Autre invité', 'Album', 'Groupe'),
                        ('/m/a3.mp3', 'Groupe', 'Album', 'Autre groupe')",
                [],
            )
            .unwrap();
        assert_eq!(
            lib.pistes_soeurs(a, 8).unwrap(),
            [PathBuf::from("/m/a2.mp3")],
            "même album et même artiste d'album, sans la piste elle-même ni un homonyme"
        );
        assert_eq!(
            lib.artiste_album_pour_path(b).unwrap(),
            Some(("Solo".to_string(), "Autre".to_string())),
            "album_artist vide : repli sur l'artiste de la piste"
        );
    }

    /// « Aucune critique » se mémorise comme les autres réponses vides du
    /// projet — l'album ne revient pas à chaque passe.
    #[test]
    fn critiques_poser_memorise_labsence_de_critique() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute(
                "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
                 VALUES ('rg-1', 'art-1', 'Titre', 'titre')",
                [],
            )
            .unwrap();
        assert_eq!(
            lib.critiques_candidats(0, 100).unwrap(),
            vec!["rg-1".to_string()]
        );

        lib.critiques_poser("rg-1", &[]).unwrap();
        assert!(lib.critiques_candidats(0, 100).unwrap().is_empty());

        lib.critiques_poser(
            "rg-1",
            &[CritiqueBrute {
                id: "crit-1".into(),
                auteur: Some("Quelquun".into()),
                licence_id: "CC BY-SA 3.0".into(),
                licence_nom: Some("Creative Commons Attribution-ShareAlike 3.0".into()),
                langue: Some("fr".into()),
                texte: "Un album marquant.".into(),
                url_originale: Some("https://critiquebrainz.org/review/crit-1".into()),
            }],
        )
        .unwrap();
        let critiques = lib.critiques_pour_release_group("rg-1").unwrap();
        assert_eq!(critiques.len(), 1);
        assert_eq!(critiques[0].auteur.as_deref(), Some("Quelquun"));
    }

    /// `mb_release_groups` porte toute la discographie d'un artiste (utile à
    /// `mb_albums` pour désambiguïser un titre), mais un seul des deux albums
    /// de l'artiste ici est réellement dans la bibliothèque — seul son
    /// release-group doit ressortir, pas celui du disque qu'on n'a pas. Voir
    /// `critiques::actualiser`, qui s'en sert pour ne pas interroger
    /// CritiqueBrainz sur des albums jamais affichés.
    #[test]
    fn release_groups_possedes_exclut_le_reste_de_la_discographie() {
        let lib = Library::open_in_memory().unwrap();
        lib.conn
            .execute_batch(
                "INSERT INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
                 VALUES ('rg-possede', 'art-1', 'Album Possédé', 'albumpossede'),
                        ('rg-absent', 'art-1', 'Autre Album', 'autrealbum')",
            )
            .unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.flac".into(),
            mb_artist_id: Some("art-1".into()),
            album: Some("Album Possédé".into()),
            ..Default::default()
        })
        .unwrap();

        let possedes = lib.release_groups_possedes().unwrap();
        assert_eq!(possedes, HashSet::from(["rg-possede".to_string()]));
    }

    /// Bout en bout : `familles_votees` combine MusicBrainz, CLAP-texte et
    /// Last.fm. Une famille où deux sources s'accordent sort fiable ; une où
    /// les trois divergent replie sur MusicBrainz, comme avant ce chantier.
    #[test]
    fn familles_votees_distingue_laccord_du_desaccord() {
        let mut lib = Library::open_in_memory().unwrap();

        let ajoute = |lib: &Library, path: &str, artiste_mbid: &str, cluster: i64| -> i64 {
            let id = lib
                .upsert(&TrackMeta {
                    path: path.into(),
                    artist: Some(artiste_mbid.into()),
                    mb_artist_id: Some(artiste_mbid.into()),
                    ..Default::default()
                })
                .unwrap();
            lib.save_features(id, "clap", &[0.0], 0.0, 0.0, cluster)
                .unwrap();
            id
        };

        // Famille 1 : MusicBrainz et Last.fm s'accordent sur « metal ».
        for i in 0..5 {
            let id = ajoute(&lib, &format!("/m/x{i}.mp3"), "mbid-x", 1);
            lib.conn
                .execute(
                    "UPDATE features SET texte_label = 'A Punk Rock Band Playing Fast'
                      WHERE track_id = ?1 AND model = 'clap'",
                    rusqlite::params![id],
                )
                .unwrap();
        }
        lib.conn
            .execute(
                "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-x','artist','Alternative Metal',5)",
                [],
            )
            .unwrap();
        lib.lastfm_tags_poser(
            "mbid-x",
            &[crate::lastfm::Tag {
                nom: "alternative metal".into(),
                poids: 50,
            }],
        )
        .unwrap();

        // Famille 2 : les trois sources divergent, comme le ska français de
        // l'essai — repli attendu sur le nom MusicBrainz seul.
        for i in 0..5 {
            let id = ajoute(&lib, &format!("/m/y{i}.mp3"), "mbid-y", 2);
            lib.conn
                .execute(
                    "UPDATE features SET texte_label = 'An African Percussion Ensemble'
                      WHERE track_id = ?1 AND model = 'clap'",
                    rusqlite::params![id],
                )
                .unwrap();
        }
        lib.conn
            .execute(
                "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-y','artist','Reggae',5)",
                [],
            )
            .unwrap();
        lib.lastfm_tags_poser(
            "mbid-y",
            &[crate::lastfm::Tag {
                nom: "worldbeat".into(),
                poids: 50,
            }],
        )
        .unwrap();

        let votes = lib.familles_votees("clap").unwrap();
        let f1 = votes.iter().find(|v| v.cluster == 1).unwrap();
        assert!(f1.fiable, "metal : MusicBrainz et Last.fm s'accordent");
        assert_eq!(f1.nom_affiche, "Alternative Metal");

        let f2 = votes.iter().find(|v| v.cluster == 2).unwrap();
        assert!(
            !f2.fiable,
            "trois sources en désaccord, comme le ska de l'essai"
        );
        assert_eq!(f2.nom_affiche, "Reggae");
        assert_eq!(f2.musicbrainz, "Reggae");
        assert_eq!(f2.clap_texte, "An African Percussion Ensemble");
        assert_eq!(f2.lastfm, "Worldbeat");

        // `familles()` (les 5 points de lecture de l'interface) projette le
        // même nom affiché, signature inchangée.
        let simples = lib.familles("clap").unwrap();
        assert!(simples.contains(&(1, "Alternative Metal".to_string(), 5)));
        assert!(simples.contains(&(2, "Reggae".to_string(), 5)));

        // Affinage local : les mêmes morceaux, redécoupés en sous-groupes
        // ignorant `features.cluster` — l'identifiant de sous-groupe est
        // local à cet appel, jamais réécrit en base.
        let mut assignations: Vec<(i64, i64)> = Vec::new();
        for (i, (id, _)) in lib
            .embeddings_du_cluster("clap", 1)
            .unwrap()
            .into_iter()
            .enumerate()
        {
            assignations.push((id, (i % 2) as i64));
        }
        for (id, _) in lib.embeddings_du_cluster("clap", 2).unwrap() {
            assignations.push((id, 9)); // un sous-groupe à part, sans rapport
        }
        let sous = lib.sous_groupes_votes("clap", &assignations).unwrap();
        // Trois sous-groupes locaux (0, 1, 9), aucun n'est un cluster 1/2 de
        // `features.cluster` — la fonction ne lit que ce qu'on lui donne.
        let mut ids: Vec<i64> = sous.iter().map(|v| v.cluster).collect();
        ids.sort();
        assert_eq!(ids, vec![0, 1, 9]);
    }

    /// Régression, mesurée en développant ce chantier sur la bibliothèque
    /// réelle : quand CLAP-texte et Last.fm n'ont **aucune** donnée (l'état
    /// courant tant que le vocabulaire n'est pas généré et que Last.fm n'a
    /// pas tourné), les deux tombaient sur le même repli « Famille N » —
    /// même rang, même liste de familles — et `arbitrer_vote` prenait ce
    /// faux consensus pour un accord, effaçant les vrais noms MusicBrainz de
    /// toute la carte. `familles()` doit rendre les noms MusicBrainz
    /// intacts, comme avant l'introduction du vote.
    #[test]
    fn familles_votees_ignore_le_faux_accord_de_deux_replis() {
        let lib = Library::open_in_memory().unwrap();
        let ajoute = |path: &str, artiste_mbid: &str, cluster: i64| {
            let id = lib
                .upsert(&TrackMeta {
                    path: path.into(),
                    mb_artist_id: Some(artiste_mbid.into()),
                    ..Default::default()
                })
                .unwrap();
            lib.save_features(id, "clap", &[0.0], 0.0, 0.0, cluster)
                .unwrap();
        };
        for i in 0..5 {
            ajoute(&format!("/m/x{i}.mp3"), "mbid-x", 1);
        }
        for i in 0..5 {
            ajoute(&format!("/m/y{i}.mp3"), "mbid-y", 2);
        }
        lib.conn
            .execute(
                "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-x','artist','Rock',5)",
                [],
            )
            .unwrap();
        lib.conn
            .execute(
                "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES ('mbid-y','artist','Jazz',5)",
                [],
            )
            .unwrap();
        // Ni `texte_label` (aucun vocabulaire généré), ni `lastfm_tags`
        // (aucune passe Last.fm) — l'état par défaut d'une bibliothèque qui
        // n'a pas encore mis en place ces deux sources.

        let votes = lib.familles_votees("clap").unwrap();
        let f1 = votes.iter().find(|v| v.cluster == 1).unwrap();
        let f2 = votes.iter().find(|v| v.cluster == 2).unwrap();
        assert!(!f1.fiable);
        assert!(!f2.fiable);
        assert_eq!(f1.nom_affiche, "Rock");
        assert_eq!(f2.nom_affiche, "Jazz");
        assert_ne!(
            f1.nom_affiche, f2.nom_affiche,
            "pas de faux consensus entre les deux familles"
        );
    }
}

#[cfg(test)]
mod tests_ordre {
    use super::*;

    #[test]
    fn les_dates_iso_se_convertissent_en_cle() {
        assert_eq!(date_iso_vers_cle("1973-03-01"), Some(19_730_301));
        assert_eq!(date_iso_vers_cle("1973-03"), Some(19_730_300));
        assert_eq!(date_iso_vers_cle("1973"), Some(19_730_000));
        // Une date partielle doit rester **avant** toute date complète de la
        // même année : c'est ce que le remplissage par des zéros garantit.
        assert!(date_iso_vers_cle("1973").unwrap() < date_iso_vers_cle("1973-01-01").unwrap());
        assert_eq!(date_iso_vers_cle("pas une date"), None);
        assert_eq!(date_iso_vers_cle("1650-01-01"), None);
    }

    #[test]
    fn lepoch_se_convertit_en_cle() {
        // Valeurs de référence, vérifiées contre une bibliothèque de dates.
        assert_eq!(epoch_vers_cle(1_787_000_000), 20_260_817);
        assert_eq!(epoch_vers_cle(1_000_000_000), 20_010_909);
        assert_eq!(epoch_vers_cle(1_700_000_000), 20_231_114);
        assert_eq!(epoch_vers_cle(0), 19_700_101);
        // Monotone : c'est tout ce que l'ordre du peuplement lui demande.
        assert!(epoch_vers_cle(1_000_000_000) < epoch_vers_cle(1_700_000_000));
    }

    #[test]
    fn le_hachage_dalbum_est_stable_et_discriminant() {
        assert_eq!(hacher("a", "b"), hacher("a", "b"));
        assert_ne!(hacher("a", "b"), hacher("b", "a"));
        // Le séparateur évite qu'« ab » + « c » collisionne avec « a » + « bc ».
        assert_ne!(hacher("ab", "c"), hacher("a", "bc"));
    }

    /// L'échelle de datation, de bout en bout : chaque échelon doit prendre le
    /// relais du précédent, et un morceau sans rien doit tout de même arriver.
    #[test]
    fn lechelle_de_datation_rattrape_les_trous() {
        let lib = Library::open_in_memory().unwrap();
        let poser = |id: i64, annee: Option<i64>, album: &str, artiste: &str| {
            lib.conn
                .execute(
                    "INSERT INTO tracks (id, path, year, album, album_artist, track_no, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, 1000000000)",
                    params![id, format!("/x/{id}.flac"), annee, album, artiste],
                )
                .unwrap();
        };
        poser(1, Some(1990), "Disque", "Groupe");
        poser(2, None, "Disque", "Groupe"); // rattrapé par l'album
        poser(3, None, "Autre", "Groupe"); // rattrapé par l'artiste
        poser(4, None, "Seul", "Inconnu"); // rien : date d'ingestion

        let ordre = lib.ordre_darrivee().unwrap();
        let par_id: HashMap<i64, &ArriveeBrute> = ordre.iter().map(|a| (a.track_id, a)).collect();
        assert_eq!(par_id[&1].source, "tag");
        assert_eq!(par_id[&2].source, "album");
        assert_eq!(par_id[&2].date, 19_900_000);
        assert_eq!(par_id[&3].source, "artiste");
        assert_eq!(par_id[&4].source, "ingestion");
        // Aucun morceau n'est perdu en route.
        assert_eq!(ordre.len(), 4);
    }

    /// `mb_poser_albums` doit garder la date de parution et les types
    /// secondaires du release-group, pas seulement son titre — sinon
    /// `ordre_darrivee` et `albums` n'ont jamais rien à corriger et
    /// retombent silencieusement sur le seul tag, même après une passe
    /// `enrich` réussie.
    #[test]
    fn mb_poser_albums_conserve_la_date_et_corrige_le_tag() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.flac".into(),
            album: Some("Disque".into()),
            album_artist: Some("Groupe".into()),
            mb_album_artist_id: Some("artiste-1".into()),
            year: Some(1999), // année de réédition, portée par le tag
            ..Default::default()
        })
        .unwrap();

        lib.mb_poser_albums(
            "artiste-1",
            &[(
                "rg-1".into(),
                "Disque".into(),
                "disque".into(),
                Vec::new(),
                Some("1973-03-01".into()), // date de l'œuvre, antérieure au tag
                None,
            )],
        )
        .unwrap();

        let ordre = lib.ordre_darrivee().unwrap();
        assert_eq!(ordre.len(), 1);
        assert_eq!(ordre[0].source, "musicbrainz");
        assert_eq!(ordre[0].date, 19_730_301);

        let albums = lib.albums(None).unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(
            albums[0].year,
            Some(1973),
            "l'année de l'album doit suivre MusicBrainz, pas le seul tag"
        );
    }

    /// Un titre à plusieurs mots doit rejoindre son release-group tout
    /// autant qu'un titre d'un seul mot. `title_norm` retire les espaces
    /// (`normaliser_titre`) : la jointure doit appliquer la même
    /// normalisation, pas un simple `lower(trim(...))` qui les garde — sinon
    /// la jointure ne matche jamais et l'année retombe silencieusement sur
    /// celle du tag (ex. réédition 2016 au lieu de la sortie 1991).
    #[test]
    fn mb_poser_albums_rejoint_un_titre_a_plusieurs_mots() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.flac".into(),
            album: Some("Temple of the Dog".into()),
            album_artist: Some("Temple of the Dog".into()),
            mb_album_artist_id: Some("artiste-1".into()),
            year: Some(2016), // année de la réédition 25e anniversaire
            ..Default::default()
        })
        .unwrap();

        lib.mb_poser_albums(
            "artiste-1",
            &[(
                "rg-1".into(),
                "Temple of the Dog".into(),
                crate::musicbrainz::normaliser_titre("Temple of the Dog"),
                Vec::new(),
                Some("1991-04-16".into()),
                None,
            )],
        )
        .unwrap();

        let albums = lib.albums(None).unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(
            albums[0].year,
            Some(1991),
            "l'année doit suivre MusicBrainz (sortie d'origine), pas le tag (réédition)"
        );
    }

    /// Une compilation date d'elle-même, pas des œuvres qu'elle rassemble :
    /// sa `first_release_date` de release-group ne doit pas écraser le tag.
    #[test]
    fn mb_poser_albums_ignore_la_date_dune_compilation() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.upsert(&TrackMeta {
            path: "/m/1.flac".into(),
            album: Some("Spawn".into()),
            album_artist: Some("Various".into()),
            mb_album_artist_id: Some("artiste-1".into()),
            year: Some(1997),
            ..Default::default()
        })
        .unwrap();

        lib.mb_poser_albums(
            "artiste-1",
            &[(
                "rg-1".into(),
                "Spawn".into(),
                "spawn".into(),
                Vec::new(),
                Some("2010-01-01".into()), // date d'une réédition de la compilation
                Some("Compilation".into()),
            )],
        )
        .unwrap();

        let albums = lib.albums(None).unwrap();
        assert_eq!(
            albums[0].year,
            Some(1997),
            "le tag doit l'emporter sur la compilation"
        );
    }

    /// Les pistes d'un album arrivent **ensemble**. Sans cela, les 1 341
    /// arrivées d'une même année les éparpilleraient et chacune irait fonder
    /// son propre établissement.
    #[test]
    fn un_album_arrive_en_bloc() {
        let lib = Library::open_in_memory().unwrap();
        for (id, album, piste) in [
            (1i64, "A", 1i64),
            (2, "B", 1),
            (3, "A", 2),
            (4, "B", 2),
            (5, "A", 3),
        ] {
            lib.conn
                .execute(
                    "INSERT INTO tracks (id, path, year, album, album_artist, track_no, added_at)
                     VALUES (?1, ?2, 2000, ?3, 'G', ?4, 0)",
                    params![id, format!("/x/{id}.flac"), album, piste],
                )
                .unwrap();
        }
        let ordre = lib.ordre_darrivee().unwrap();
        let albums: Vec<u64> = ordre.iter().map(|a| a.album).collect();
        // Trois pistes du même album, puis deux de l'autre : jamais alternés.
        let mut changements = 0;
        for p in albums.windows(2) {
            if p[0] != p[1] {
                changements += 1;
            }
        }
        assert_eq!(changements, 1, "les albums s'entremêlent : {albums:?}");
        // Et dans l'ordre des pistes à l'intérieur d'un album.
        let premier = albums[0];
        let pistes: Vec<u16> = ordre
            .iter()
            .filter(|a| a.album == premier)
            .map(|a| a.piste)
            .collect();
        assert!(pistes.windows(2).all(|p| p[0] <= p[1]), "{pistes:?}");
    }

    /// « Image du dossier » : le choix vit dans la base et l'emporte sur les
    /// tags, même quand le fichier audio est illisible — une recopie des MP3
    /// ne l'efface donc pas.
    #[test]
    fn le_choix_de_pochette_du_dossier_l_emporte_sur_les_tags() {
        let racine =
            std::env::temp_dir().join(format!("rusty-music-pochette-choix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&racine);
        let dossier = racine.join("Artiste_1/Album 100%");
        std::fs::create_dir_all(dossier.join("CD2")).unwrap();
        let piste = dossier.join("01.mp3");
        std::fs::write(&piste, b"pas un mp3").unwrap();
        std::fs::write(dossier.join("cover.jpg"), b"jpeg du dossier").unwrap();

        let lib = Library::open_in_memory().unwrap();
        for p in [
            &piste,
            &dossier.join("CD2/01.mp3"),
            &racine.join("Artiste_1/autre.mp3"),
        ] {
            lib.conn
                .execute(
                    "INSERT INTO tracks (path) VALUES (?1)",
                    params![p.to_string_lossy()],
                )
                .unwrap();
        }
        let d = dossier.to_string_lossy();
        assert_eq!(
            lib.pistes_du_dossier(&d).unwrap(),
            vec![piste.to_string_lossy()]
        );

        // Sans choix : les tags d'abord, et ce fichier n'en a pas de lisibles.
        assert!(lib.lire_pochette(&piste).is_err());
        lib.choisir_pochette_dossier(&d, true).unwrap();
        let c = lib.lire_pochette(&piste).unwrap().unwrap();
        assert_eq!(c.data, b"jpeg du dossier");
        assert_eq!(c.source, crate::tags::CoverSource::Folder);
        lib.choisir_pochette_dossier(&d, false).unwrap();
        assert!(!lib.pochette_dossier_choisie(&d).unwrap());
        let _ = std::fs::remove_dir_all(&racine);
    }
}
