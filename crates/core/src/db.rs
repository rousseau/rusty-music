// SPDX-License-Identifier: GPL-3.0-or-later
//! Base locale SQLite : la seule source de vérité pour les trois modules.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::tags::TrackMeta;

pub struct Library {
    pub conn: Connection,
}

/// Résumé d'un morceau tel que servi à l'interface.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrackRow {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<i64>,
    pub year: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Identifiant MusicBrainz d'artiste d'album, comme [`ArtistRow::mbid`] —
    /// sert à retrouver ses albums (`Library::albums_of_artist`) depuis un
    /// morceau, sans passer par la liste des artistes.
    pub artist_mbid: Option<String>,
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
            "rock", "alternative rock", "hard rock", "psychedelic rock", "folk rock",
            "blues rock", "pop rock", "progressive rock", "punk", "grunge", "indie rock",
            "post-punk", "garage rock",
        ],
    ),
    (
        "Metal",
        &[
            "metal", "nu metal", "alternative metal", "thrash metal", "industrial metal",
            "groove metal", "heavy metal", "death metal", "black metal", "doom metal",
        ],
    ),
    ("Hip Hop", &["hip hop", "boom bap", "rap", "trap"]),
    (
        "Électronique",
        &[
            "electronic", "drum and bass", "big beat", "ambient", "downtempo", "trip hop",
            "house", "techno", "idm", "electronica", "synthwave",
        ],
    ),
    ("Jazz", &["jazz", "acid jazz", "fusion", "bebop", "swing"]),
    ("Reggae", &["reggae", "dub", "dancehall", "ska"]),
    ("Soul · Funk", &["funk", "soul", "r&b", "disco", "motown"]),
    (
        "Folk",
        &["folk", "celtic", "bluegrass", "country", "americana", "singer-songwriter"],
    ),
    ("Chanson", &["chanson française", "chanson", "variété française"]),
    ("Classique", &["classical", "baroque", "opera", "contemporary classical"]),
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

/// Hachage stable d'un couple artiste/album. Doit rendre la même valeur d'une
/// exécution à l'autre : c'est lui qui départage les arrivées d'une même année.
fn hacher(artiste: &str, album: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in artiste.as_bytes().iter().chain(b"\x1f").chain(album.as_bytes()) {
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
/// publié, titre normalisé pour le rapprochement, et genres avec leurs votes.
pub type AlbumRange = (String, String, String, Vec<(String, i64)>);

/// Un album et ses éditions multiples : l'artiste, le titre brut le plus
/// représenté, puis chaque édition (titre publié, nombre de pistes).
pub type EditionsAlbum = (String, String, Vec<(String, i64)>);

/// Une piste vue par le nommage des familles : sa famille, son artiste
/// MusicBrainz, son album, et le genre inscrit dans le fichier.
type PisteNommage = (i64, Option<String>, Option<String>, Option<String>);

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
const TRACK_COLS: &str =
    "id, path, title, artist, album, track_no, year, duration_ms, mb_album_artist_id";

/// Met à niveau une base créée par une version antérieure.
///
/// `CREATE TABLE IF NOT EXISTS` ne touche pas à une table existante : sans
/// cela, une base déjà peuplée n'obtiendrait jamais les colonnes ajoutées
/// depuis. On compare à `pragma_table_info` plutôt que de tenir un numéro de
/// version — idempotent, et insensible aux allers-retours entre branches.
fn migrate(conn: &Connection) -> Result<()> {
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

fn track_from_row(r: &rusqlite::Row) -> rusqlite::Result<TrackRow> {
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
    /// Ouvre (ou crée) la base et applique le schéma. Idempotent.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(include_str!("../sql/schema.sql"))?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Base en mémoire — pratique pour les tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(include_str!("../sql/schema.sql"))?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn add_root(&self, root: &Path) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO roots(path) VALUES (?1)",
            params![root.to_string_lossy()],
        )?;
        Ok(())
    }

    /// Insère ou met à jour un morceau. Le chemin fait office de clé d'identité.
    /// Ne touche pas à `analyzed_at` : l'analyse reste valable tant que le
    /// fichier n'a pas changé.
    pub fn upsert(&self, m: &TrackMeta) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tracks
               (path, size_bytes, mtime, title, artist, album, album_artist,
                genre, year, track_no, duration_ms, sample_rate, channels, bitrate, codec,
                bit_depth, mb_recording_id, mb_artist_id, mb_album_artist_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
             ON CONFLICT(path) DO UPDATE SET
               size_bytes=excluded.size_bytes, mtime=excluded.mtime,
               title=excluded.title, artist=excluded.artist, album=excluded.album,
               album_artist=excluded.album_artist, genre=excluded.genre,
               year=excluded.year, track_no=excluded.track_no,
               duration_ms=excluded.duration_ms, sample_rate=excluded.sample_rate,
               channels=excluded.channels, bitrate=excluded.bitrate, codec=excluded.codec,
               bit_depth=excluded.bit_depth,
               mb_recording_id=excluded.mb_recording_id,
               mb_artist_id=excluded.mb_artist_id,
               mb_album_artist_id=excluded.mb_album_artist_id",
            params![
                m.path.to_string_lossy(),
                m.size_bytes,
                m.mtime,
                m.title,
                m.artist,
                m.album,
                m.album_artist,
                m.genre,
                m.year,
                m.track_no,
                m.duration_ms,
                m.sample_rate,
                m.channels,
                m.bitrate,
                m.codec,
                m.bit_depth,
                m.mb_recording_id,
                m.mb_artist_id,
                m.mb_album_artist_id
            ],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM tracks WHERE path = ?1",
            params![m.path.to_string_lossy()],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Vrai si le fichier est déjà en base avec la même taille et la même date
    /// de modification — permet de sauter la relecture des tags au rescan.
    pub fn is_unchanged(&self, path: &Path, size: i64, mtime: i64) -> Result<bool> {
        let hit: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM tracks WHERE path = ?1 AND size_bytes = ?2 AND mtime = ?3",
                params![path.to_string_lossy(), size, mtime],
                |r| r.get(0),
            )
            .optional()?;
        Ok(hit.is_some())
    }

    pub fn remove_path(&self, path: &Path) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM tracks WHERE path = ?1",
            params![path.to_string_lossy()],
        )?)
    }

    /// Note l'échec de lecture d'un fichier — tags illisibles, insertion en
    /// échec, ou décodage audio impossible pendant une passe d'analyse.
    /// `ON CONFLICT` plutôt qu'un doublon : un même fichier qui échoue
    /// à chaque scan ne doit pas empiler les lignes, seulement rafraîchir la
    /// raison et la date. `pending_analysis`/`pending_descripteurs` excluent
    /// ce qui figure ici — sans quoi un fichier qui déstabilise son support
    /// (carte SD, lecteur USB) serait retenté à l'identique à chaque passe.
    pub fn enregistrer_echec_scan(&self, path: &Path, raison: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO scan_failures(path, reason) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET reason = excluded.reason, at = strftime('%s','now')",
            params![path.to_string_lossy(), raison],
        )?;
        Ok(())
    }

    /// Efface un échec — le fichier a fini par se lire, ou l'utilisateur a
    /// choisi de le retirer de la liste sans y revenir.
    pub fn effacer_echec_scan(&self, path: &Path) -> Result<()> {
        self.conn.execute(
            "DELETE FROM scan_failures WHERE path = ?1",
            params![path.to_string_lossy()],
        )?;
        Ok(())
    }

    /// Les fichiers en échec, du plus récent au plus ancien.
    pub fn echecs_scan(&self) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, reason, at FROM scan_failures ORDER BY at DESC")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Retire les morceaux de `root` dont le fichier a disparu du disque.
    ///
    /// Ne supprime que sur une absence avérée (`NotFound`) : un dossier devenu
    /// illisible ou une racine qui bronche remonte une autre erreur, et la
    /// ligne est alors conservée. Mieux vaut une ligne en trop qu'une
    /// bibliothèque vidée par un incident de lecture.
    pub fn prune_missing(&self, root: &Path) -> Result<usize> {
        let disparus: Vec<String> = self
            .paths_under(root)?
            .into_iter()
            .filter(|p| {
                matches!(
                    std::fs::symlink_metadata(p),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound
                )
            })
            .collect();

        let tx = self.conn.unchecked_transaction()?;
        {
            let mut del = tx.prepare("DELETE FROM tracks WHERE path = ?1")?;
            for p in &disparus {
                del.execute(params![p])?;
            }
        }
        tx.commit()?;
        Ok(disparus.len())
    }

    /// Enregistre l'empreinte d'un morceau et sa place sur la carte.
    ///
    /// Le nom du modèle fait partie de la clé : deux jeux d'empreintes peuvent
    /// cohabiter, ce qui permet d'en comparer deux sans tout refaire.
    /// `analyzed_at` n'est posé qu'ici — un morceau reste « en attente » tant
    /// que son empreinte n'est pas écrite.
    pub fn save_features(
        &self,
        track_id: i64,
        model: &str,
        vector: &[f32],
        x: f32,
        y: f32,
        cluster: i64,
    ) -> Result<()> {
        // f32 en petit-boutien, comme l'annonce le schéma.
        let mut blob = Vec::with_capacity(vector.len() * 4);
        for v in vector {
            blob.extend_from_slice(&v.to_le_bytes());
        }

        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO features(track_id, model, dim, vector, x, y, cluster)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(track_id, model) DO UPDATE SET
               dim=excluded.dim, vector=excluded.vector,
               x=excluded.x, y=excluded.y, cluster=excluded.cluster,
               computed_at=strftime('%s','now')",
            params![track_id, model, vector.len() as i64, blob, x, y, cluster],
        )?;
        tx.execute(
            "UPDATE tracks SET analyzed_at = strftime('%s','now') WHERE id = ?1",
            params![track_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Enregistre la seule empreinte, sans coordonnées.
    ///
    /// Sépare le coûteux du gratuit : l'empreinte demande de décoder le
    /// fichier, la position se recalcule en quelques secondes sur toute la
    /// bibliothèque. `analyzed_at` est posé ici — c'est ce travail-là qu'on ne
    /// veut pas refaire après une interruption.
    pub fn save_embedding(&self, track_id: i64, model: &str, vector: &[f32]) -> Result<()> {
        let mut blob = Vec::with_capacity(vector.len() * 4);
        for v in vector {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO features(track_id, model, dim, vector)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(track_id, model) DO UPDATE SET
               dim=excluded.dim, vector=excluded.vector,
               computed_at=strftime('%s','now')",
            params![track_id, model, vector.len() as i64, blob],
        )?;
        tx.execute(
            "UPDATE tracks SET analyzed_at = strftime('%s','now') WHERE id = ?1",
            params![track_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Toutes les empreintes d'un modèle, pour la projection.
    ///
    /// Chargées d'un bloc : t-SNE place chaque point relativement aux autres,
    /// il lui faut l'ensemble. Compter ~2 Ko par morceau, soit 55 Mo sur la
    /// bibliothèque complète.
    pub fn embeddings(&self, model: &str) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id, vector FROM features WHERE model = ?1 ORDER BY track_id")?;
        let rows = stmt
            .query_map(params![model], |r| {
                let id: i64 = r.get(0)?;
                let blob: Vec<u8> = r.get(1)?;
                let v = blob
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                Ok((id, v))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Les familles de la carte, nommées par leurs genres.
    ///
    /// **Trois sources, par ordre de précision décroissante** — l'album
    /// MusicBrainz, puis l'artiste MusicBrainz, puis le tag du fichier. Le
    /// détail de l'arbitrage est dans [`genres_du_morceau`], le nommage dans
    /// [`nommer_les_familles`] : la base ne rend ici que des comptes, pour que
    /// les deux règles se testent sans elle.
    pub fn familles(&self, model: &str) -> Result<Vec<(i64, String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.cluster, t.mb_artist_id, t.album, t.genre
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1",
        )?;
        let pistes: Vec<PisteNommage> = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;

        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        let mut comptes: Vec<(i64, String, i64)> = Vec::new();
        let mut cumul: HashMap<(i64, String), i64> = HashMap::new();
        for (cluster, artiste, album, tag) in &pistes {
            for genre in genres_du_morceau(
                artiste.as_deref(),
                album.as_deref(),
                tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            ) {
                *cumul.entry((*cluster, genre)).or_default() += 1;
            }
        }
        for ((cluster, genre), n) in cumul {
            comptes.push((cluster, genre, n));
        }

        // L'effectif compte tous les morceaux de la famille, y compris ceux
        // sans genre : c'est la taille de la tache sur la carte.
        let mut stmt = self
            .conn
            .prepare("SELECT cluster, COUNT(*) FROM features WHERE model = ?1 GROUP BY cluster")?;
        let mut effectifs: Vec<(i64, i64)> = stmt
            .query_map(params![model], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        effectifs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));

        Ok(nommer_les_familles(&effectifs, &comptes))
    }

    /// Pour chaque album, sa famille sonique dominante — le cluster de la carte
    /// le plus représenté parmi ses morceaux déjà projetés.
    ///
    /// Sert le filtre par famille de la grille de pochettes du mode Écoute, qui
    /// réutilise la légende des familles du mode Explorer. La clé d'album est
    /// celle de [`Library::albums`] : nom + `COALESCE(album_artist, artist)`.
    /// Les albums dont aucun morceau n'est encore sur la carte n'apparaissent
    /// pas — le filtre les laisse alors visibles par défaut.
    pub fn familles_des_albums(
        &self,
        model: &str,
    ) -> Result<Vec<(String, Option<String>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.album, COALESCE(t.album_artist, t.artist), f.cluster, COUNT(*)
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND f.cluster IS NOT NULL AND t.album IS NOT NULL
              GROUP BY t.album, COALESCE(t.album_artist, t.artist), f.cluster",
        )?;
        let lignes: Vec<(String, Option<String>, i64, i64)> = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;

        // Cluster majoritaire par album ; à égalité, le plus petit numéro
        // tranche, pour que le filtre range un album au même endroit d'une
        // session à l'autre.
        let mut par_album: HashMap<(String, Option<String>), (i64, i64)> = HashMap::new();
        for (album, artiste, cluster, n) in lignes {
            let e = par_album.entry((album, artiste)).or_insert((cluster, n));
            if n > e.1 || (n == e.1 && cluster < e.0) {
                *e = (cluster, n);
            }
        }
        Ok(par_album
            .into_iter()
            .map(|((album, artiste), (cluster, _))| (album, artiste, cluster))
            .collect())
    }

    /// Pour chaque artiste (`mb_album_artist_id`), sa famille sonique dominante
    /// — le cluster de la carte le plus représenté parmi ses morceaux projetés.
    ///
    /// Sert le filtre par famille du fil du mode Découvrir (`app.js`), qui
    /// réutilise la légende des familles du mode Explorer. Même arbitrage que
    /// [`Library::familles_des_albums`] : majorité, et le plus petit numéro à
    /// égalité pour que le classement soit stable d'une session à l'autre.
    pub fn familles_des_artistes(&self, model: &str) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_album_artist_id, f.cluster, COUNT(*)
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND f.cluster IS NOT NULL
                AND t.mb_album_artist_id IS NOT NULL AND t.mb_album_artist_id <> ''
              GROUP BY t.mb_album_artist_id, f.cluster",
        )?;
        let lignes: Vec<(String, i64, i64)> = stmt
            .query_map(params![model], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mut par_artiste: HashMap<String, (i64, i64)> = HashMap::new();
        for (mbid, cluster, n) in lignes {
            let e = par_artiste.entry(mbid).or_insert((cluster, n));
            if n > e.1 || (n == e.1 && cluster < e.0) {
                *e = (cluster, n);
            }
        }
        Ok(par_artiste
            .into_iter()
            .map(|(mbid, (cluster, _))| (mbid, cluster))
            .collect())
    }

    /// Le genre le plus précis de chaque morceau analysé, résolu par la même
    /// hiérarchie que [`Library::familles`] (album MusicBrainz, puis artiste,
    /// puis tag du fichier) — mais **par morceau**, pas agrégé par cluster.
    ///
    /// Sert à ancrer un regroupement par genre plutôt que par k-means : un
    /// morceau dont le genre est connu n'a pas besoin d'être placé par
    /// distance, il appartient déjà à sa famille.
    pub fn genres_resolus(&self, model: &str) -> Result<HashMap<i64, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.track_id, t.mb_artist_id, t.album, t.genre
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1",
        )?;
        let pistes: Vec<PisteNommage> = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;

        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        let mut resolus = HashMap::new();
        for (track_id, artiste, album, tag) in &pistes {
            if let Some(genre) = genres_du_morceau(
                artiste.as_deref(),
                album.as_deref(),
                tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            )
            .into_iter()
            .next()
            {
                // En minuscules : MusicBrainz les rend déjà ainsi, mais le
                // repli sur le tag du fichier ne le garantit pas, et
                // `cluster::VOCABULAIRE` compare sans normaliser la casse.
                resolus.insert(*track_id, genre.to_lowercase());
            }
        }
        Ok(resolus)
    }

    /// Les artistes les mieux représentés d'une famille. Diagnostic : c'est en
    /// les lisant qu'on juge si l'étiquette dit vrai.
    pub fn artistes_de_famille(&self, model: &str, cluster: i64, n: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.artist FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND f.cluster = ?2 AND t.artist IS NOT NULL AND t.artist <> ''
              GROUP BY t.artist ORDER BY COUNT(*) DESC LIMIT ?3",
        )?;
        let noms: Vec<String> = stmt
            .query_map(params![model, cluster, n as i64], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(noms)
    }

    /// Morceaux dont le genre résolu ([`genres_du_morceau`]) ne figure pas
    /// parmi les [`GENRES_DOMINANTS`] les plus représentés de leur famille
    /// sonique (cluster de la carte) — le tag dit une chose, l'empreinte en
    /// dit une autre. Un signal à vérifier, pas une certitude : deux genres
    /// voisins (« folk » / « folk-pop ») divergent tout en étant tous deux
    /// justes.
    ///
    /// Les familles trop petites pour qu'un « genre dominant » veuille dire
    /// quelque chose sont ignorées ([`PLANCHER_ABSOLU`], déjà utilisé pour
    /// nommer les familles).
    pub fn genres_suspects(&self, model: &str) -> Result<Vec<(i64, String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.cluster, f.track_id, t.mb_artist_id, t.album, t.genre,
                    COALESCE(t.title, t.path), COALESCE(t.artist, '?')
               FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1",
        )?;
        struct Ligne {
            cluster: i64,
            track_id: i64,
            artiste_mbid: Option<String>,
            album: Option<String>,
            tag: Option<String>,
            titre: String,
            artiste: String,
        }
        let pistes: Vec<Ligne> = stmt
            .query_map(params![model], |r| {
                Ok(Ligne {
                    cluster: r.get(0)?,
                    track_id: r.get(1)?,
                    artiste_mbid: r.get(2)?,
                    album: r.get(3)?,
                    tag: r.get(4)?,
                    titre: r.get(5)?,
                    artiste: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;

        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        // Genre résolu par morceau, et effectifs par (cluster, genre) — même
        // arbitrage que `familles`, gardé par morceau ici au lieu d'être
        // sommé tout de suite : il faut le comparer au dominant de son
        // cluster, pas seulement le compter dedans.
        let mut genre_du_morceau: HashMap<i64, String> = HashMap::new();
        let mut cumul: HashMap<(i64, String), i64> = HashMap::new();
        let mut taille_cluster: HashMap<i64, i64> = HashMap::new();
        for p in &pistes {
            *taille_cluster.entry(p.cluster).or_default() += 1;
            let genres = genres_du_morceau(
                p.artiste_mbid.as_deref(),
                p.album.as_deref(),
                p.tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            );
            // `GENRES_PAR_ENTITE = 1` : au plus un genre par morceau déjà —
            // pas d'ambiguïté à choisir lequel comparer.
            if let Some(g) = genres.into_iter().next() {
                genre_du_morceau.insert(p.track_id, g.clone());
                *cumul.entry((p.cluster, g)).or_default() += 1;
            }
        }

        // Les `GENRES_DOMINANTS` premiers de chaque cluster, par effectif.
        let mut par_cluster: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
        for ((cluster, genre), n) in cumul {
            par_cluster.entry(cluster).or_default().push((genre, n));
        }
        for v in par_cluster.values_mut() {
            v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            v.truncate(GENRES_DOMINANTS);
        }

        let mut suspects = Vec::new();
        for p in &pistes {
            if taille_cluster[&p.cluster] < PLANCHER_ABSOLU {
                continue;
            }
            let Some(genre) = genre_du_morceau.get(&p.track_id) else {
                continue;
            };
            let dominants = par_cluster.get(&p.cluster).map(|v| v.as_slice()).unwrap_or(&[]);
            if dominants.iter().any(|(g, _)| g == genre) {
                continue;
            }
            let etiquette = dominants
                .iter()
                .map(|(g, _)| g.as_str())
                .collect::<Vec<_>>()
                .join(" · ");
            suspects.push((
                p.track_id,
                format!("{} — {}", p.artiste, p.titre),
                genre.clone(),
                etiquette,
            ));
        }
        Ok(suspects)
    }

    /* ------------------------------- statistiques (mode Bibliothèque) */

    /// Répartition de toute la bibliothèque par genre — même arbitrage que
    /// [`Self::familles`] (album MusicBrainz, puis artiste, puis tag du
    /// fichier ; voir [`genres_du_morceau`]), mais sur tous les morceaux, pas
    /// seulement ceux déjà placés sur la carte. Triée par effectif
    /// décroissant ; un morceau sans genre résolu compte dans `"—"`.
    pub fn stats_genres(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT mb_artist_id, album, genre FROM tracks")?;
        let pistes: Vec<(Option<String>, Option<String>, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let par_artiste = self.mb_genres("artist", VOTES_MINIMUM)?;
        let par_album = self.mb_genres("release-group", VOTES_MINIMUM)?;
        let albums = self.mb_albums()?;

        let mut cumul: HashMap<String, i64> = HashMap::new();
        let mut sans_genre = 0i64;
        for (artiste, album, tag) in &pistes {
            let genres = genres_du_morceau(
                artiste.as_deref(),
                album.as_deref(),
                tag.as_deref(),
                &albums,
                &par_album,
                &par_artiste,
            );
            if genres.is_empty() {
                sans_genre += 1;
            }
            for genre in genres {
                *cumul.entry(genre).or_insert(0) += 1;
            }
        }
        if sans_genre > 0 {
            cumul.insert("—".to_string(), sans_genre);
        }
        let mut resultat: Vec<(String, i64)> = cumul.into_iter().collect();
        resultat.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(resultat)
    }

    /// Histogramme du tempo mesuré, par tranches de [`TEMPO_PAS`] BPM à
    /// partir de [`TEMPO_MIN`]. `LEFT JOIN` plutôt qu'un `SELECT bpm FROM
    /// descriptors` : un morceau jamais passé dans la passe « descripteurs »
    /// n'y a aucune ligne, et compterait sinon comme mesuré à zéro plutôt que
    /// comme non mesuré.
    pub fn stats_tempo(&self) -> Result<Histogramme> {
        let valeurs: Vec<Option<f64>> = self
            .conn
            .prepare("SELECT d.bpm FROM tracks t LEFT JOIN descriptors d ON d.track_id = t.id")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(histogrammer(&valeurs, TEMPO_MIN, TEMPO_PAS, TEMPO_TRANCHES))
    }

    /// Histogramme de la durée des morceaux, par tranches de [`DUREE_PAS`]
    /// (une minute) à partir de zéro.
    pub fn stats_durees(&self) -> Result<Histogramme> {
        let valeurs: Vec<Option<f64>> = self
            .conn
            .prepare("SELECT duration_ms FROM tracks")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(histogrammer(&valeurs, DUREE_MIN, DUREE_PAS, DUREE_TRANCHES))
    }

    /// Répartition par format de fichier (« MP3 », « FLAC »…). `NULL` — un
    /// morceau scanné avant que `tags::read` ne lise le format — compte à
    /// part : ce n'est pas « aucun format », c'est « pas encore mesuré ».
    pub fn stats_codecs(&self) -> Result<Vec<(String, i64)>> {
        let valeurs: Vec<Option<String>> = self
            .conn
            .prepare("SELECT codec FROM tracks")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let mut cumul: HashMap<String, i64> = HashMap::new();
        for v in valeurs {
            *cumul.entry(v.unwrap_or_else(|| "non mesuré".to_string())).or_insert(0) += 1;
        }
        let mut resultat: Vec<(String, i64)> = cumul.into_iter().collect();
        resultat.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(resultat)
    }

    /// Histogramme du débit audio, par tranches de [`BITRATE_PAS`] kb/s.
    /// Un format sans débit à annoncer (FLAC…) tombe au-delà de la dernière
    /// tranche plutôt que d'écraser l'échelle pensée pour le domaine
    /// habituel du MP3/AAC — voir [`BITRATE_MAX`].
    pub fn stats_bitrate(&self) -> Result<Histogramme> {
        let valeurs: Vec<Option<f64>> = self
            .conn
            .prepare("SELECT bitrate FROM tracks")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(histogrammer(&valeurs, BITRATE_MIN, BITRATE_PAS, BITRATE_TRANCHES))
    }

    /// Morceaux sans identifiant MusicBrainz d'artiste — ni le tag du
    /// fichier ni un rapprochement ultérieur ne les relie à MusicBrainz, donc
    /// ni genre affiné ni connexions d'artiste possibles pour eux.
    pub fn stats_sans_mbid(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM tracks WHERE mb_artist_id IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    /// Les paramètres du calcul de la carte, une clé absente valant sa
    /// valeur par défaut ([`ParametresCarte::default`]) — voir la table
    /// `parametres_carte`.
    pub fn parametres_carte(&self) -> Result<ParametresCarte> {
        let mut stmt = self.conn.prepare("SELECT cle, valeur FROM parametres_carte")?;
        let lignes: HashMap<String, f64> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let d = ParametresCarte::default();
        let get = |cle: &str, defaut: f64| lignes.get(cle).copied().unwrap_or(defaut);
        Ok(ParametresCarte {
            perplexite: get("perplexite", d.perplexite as f64) as f32,
            epoques: get("epoques", d.epoques as f64) as usize,
            familles: get("familles", d.familles as f64) as usize,
            iterations_kmeans: get("iterations_kmeans", d.iterations_kmeans as f64) as usize,
            densite_noyau: get("densite_noyau", d.densite_noyau),
            densite_resolution: get("densite_resolution", d.densite_resolution as f64) as usize,
            densite_bandes: get("densite_bandes", d.densite_bandes as f64) as usize,
        })
    }

    /// Change un paramètre de la carte. `cle` doit être un des sept champs
    /// de [`ParametresCarte`] — la validation du nom reste à l'appelant
    /// (commande Tauri), cette méthode ne fait qu'écrire.
    pub fn set_parametre_carte(&self, cle: &str, valeur: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO parametres_carte(cle, valeur) VALUES (?1, ?2)
             ON CONFLICT(cle) DO UPDATE SET valeur = excluded.valeur",
            params![cle, valeur],
        )?;
        Ok(())
    }

    /// Le vocabulaire des familles par genre, dans l'ordre où il a été
    /// écrit. Table vide = [`VOCABULAIRE_DEFAUT`] — même convention que
    /// [`Library::parametres_carte`].
    pub fn vocabulaire_familles(&self) -> Result<Vec<(String, Vec<String>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT nom, genre FROM vocabulaire_familles ORDER BY rowid")?;
        let lignes: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        if lignes.is_empty() {
            return Ok(VOCABULAIRE_DEFAUT
                .iter()
                .map(|(nom, genres)| {
                    (
                        nom.to_string(),
                        genres.iter().map(|g| g.to_string()).collect(),
                    )
                })
                .collect());
        }

        // Regroupe par nom en gardant l'ordre de première apparition — celui
        // des `rowid`, donc celui de la dernière écriture.
        let mut ordre: Vec<String> = Vec::new();
        let mut par_nom: HashMap<String, Vec<String>> = HashMap::new();
        for (nom, genre) in lignes {
            if !par_nom.contains_key(&nom) {
                ordre.push(nom.clone());
            }
            par_nom.entry(nom).or_default().push(genre);
        }
        Ok(ordre
            .into_iter()
            .map(|nom| {
                let genres = par_nom.remove(&nom).expect("clé vue à l'instant");
                (nom, genres)
            })
            .collect())
    }

    /// Remplace le vocabulaire des familles en base, en bloc.
    ///
    /// Une famille sans aucun genre est écartée : elle ne pourrait jamais
    /// ancrer un morceau, ce serait une entrée morte dans le réglage. Passer
    /// une liste vide restaure les valeurs par défaut — la table vidée,
    /// [`Library::vocabulaire_familles`] retombe sur [`VOCABULAIRE_DEFAUT`].
    pub fn definir_vocabulaire_familles(&mut self, vocabulaire: &[(String, Vec<String>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM vocabulaire_familles", [])?;
        {
            let mut inserer =
                tx.prepare("INSERT INTO vocabulaire_familles (nom, genre) VALUES (?1, ?2)")?;
            for (nom, genres) in vocabulaire {
                if genres.is_empty() {
                    continue;
                }
                for genre in genres {
                    let genre = genre.trim().to_lowercase();
                    if !genre.is_empty() {
                        inserer.execute(params![nom.trim(), genre])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Albums dont le même artiste (`album_artist` de préférence, sinon
    /// `artist` — même repli que [`Self::albums`]) porte plusieurs titres
    /// distincts qui ne diffèrent que par leur mention d'édition, ex.
    /// « Kid A » et « Kid A (Remaster) ».
    ///
    /// Le regroupement se fait sur un titre normalisé — tout ce qui précède
    /// la première parenthèse ou le premier crochet
    /// ([`titre_album_normalise`]), pas de registre d'éditions à maintenir,
    /// au prix d'un titre légitimement parenthétique tronqué à tort de temps
    /// en temps — mais le résultat rend le titre brut le plus représenté,
    /// pas la forme normalisée : celle-ci sert à regrouper, pas à afficher.
    pub fn editions_multiples(&self) -> Result<Vec<EditionsAlbum>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(album_artist, artist), album, COUNT(*)
               FROM tracks
              WHERE album IS NOT NULL AND COALESCE(album_artist, artist) IS NOT NULL
              GROUP BY COALESCE(album_artist, artist), album",
        )?;
        let lignes: Vec<(String, String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mut par_cle: HashMap<(String, String), Vec<(String, i64)>> = HashMap::new();
        for (artiste, album, n) in lignes {
            let norme = titre_album_normalise(&album);
            par_cle.entry((artiste, norme)).or_default().push((album, n));
        }
        let mut resultat: Vec<EditionsAlbum> = par_cle
            .into_iter()
            .filter(|(_, editions)| editions.len() > 1)
            .map(|((artiste, _norme), mut editions)| {
                editions.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
                (artiste, editions[0].0.clone(), editions)
            })
            .collect();
        resultat.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        Ok(resultat)
    }

    /// Répartition par humeur — tempo × énergie, tous deux déjà mesurés,
    /// **coupés à leur médiane du moment** plutôt qu'à un seuil absolu.
    ///
    /// AudioMuse-AI dérive ces catégories par similarité cosinus entre
    /// l'empreinte CLAP et un jeu de vecteurs-étiquettes (« dansant »,
    /// « agressif »…) — hors de portée ici, le modèle importé
    /// (`clap-audio-encoder-b5`) n'embarque que la tour audio, pas la tour
    /// texte qu'il faudrait pour situer un mot dans le même espace. Le
    /// substitut retenu réutilise ce qui est déjà mesuré. Le seuil absolu a
    /// été écarté après mesure : sur les 741 morceaux déjà remesurés au
    /// moment d'écrire ceci, l'énergie s'étale de 0,03 à 0,41 — un seuil fixe
    /// à 0,5 aurait vidé deux des quatre catégories. La médiane du moment
    /// s'ajuste à l'échelle réelle de la bibliothèque, quelle qu'elle soit,
    /// et reste correcte si cette échelle se déplace en cours de mesure.
    pub fn stats_humeur(&self) -> Result<Vec<(String, i64)>> {
        let valeurs: Vec<(Option<f64>, Option<f64>)> = self
            .conn
            .prepare(
                "SELECT d.bpm, d.energy FROM tracks t LEFT JOIN descriptors d ON d.track_id = t.id",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mediane = |mut v: Vec<f64>| -> Option<f64> {
            if v.is_empty() {
                return None;
            }
            v.sort_by(f64::total_cmp);
            Some(v[v.len() / 2])
        };
        let Some(med_bpm) = mediane(valeurs.iter().filter_map(|(b, _)| *b).collect()) else {
            return Ok(Vec::new());
        };
        let Some(med_energie) = mediane(valeurs.iter().filter_map(|(_, e)| *e).collect()) else {
            return Ok(Vec::new());
        };

        let mut cumul: HashMap<&str, i64> = HashMap::new();
        for (bpm, energie) in &valeurs {
            let etiquette = match (bpm, energie) {
                (Some(b), Some(e)) => match (*b >= med_bpm, *e >= med_energie) {
                    (true, true) => "Énergique",
                    (true, false) => "Enlevé",
                    (false, true) => "Intense",
                    (false, false) => "Calme",
                },
                _ => "non mesuré",
            };
            *cumul.entry(etiquette).or_insert(0) += 1;
        }
        let mut resultat: Vec<(String, i64)> =
            cumul.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        resultat.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        Ok(resultat)
    }

    /* --------------------------------------------- descripteurs musicaux */

    /// Les morceaux dont on n'a pas encore mesuré tempo, tonalité et énergie.
    ///
    /// Restreint à ceux qui ont déjà une empreinte : la passe des descripteurs
    /// décode les mêmes fenêtres, et il n'y a pas de sens à mesurer un morceau
    /// que la carte ne montre pas. Exclut aussi les fichiers en échec connu
    /// (`scan_failures`) — même raison que `pending_analysis`.
    pub fn pending_descripteurs(&self, model: &str, limit: i64) -> Result<Vec<TrackRow>> {
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks
              WHERE EXISTS (SELECT 1 FROM features f
                             WHERE f.track_id = tracks.id AND f.model = ?1)
                AND NOT EXISTS (SELECT 1 FROM descriptors d WHERE d.track_id = tracks.id)
                AND NOT EXISTS (SELECT 1 FROM scan_failures sf WHERE sf.path = tracks.path)
              ORDER BY added_at LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![model, limit], track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Enregistre les descripteurs d'un morceau.
    ///
    /// `bpm` et `musical_key` peuvent être absents : tout n'a pas de pulsation
    /// ni de tonalité, et une valeur inventée colorerait la carte d'un
    /// mensonge. La ligne est écrite quand même — c'est elle qui dit que le
    /// morceau a été mesuré, et qu'il ne faut pas y revenir.
    #[allow(clippy::too_many_arguments)]
    pub fn save_descripteurs(
        &self,
        track_id: i64,
        bpm: Option<f32>,
        musical_key: Option<&str>,
        energy: f32,
        loudness: f32,
        zcr: Option<f32>,
        centroid_mean: Option<f32>,
        centroid_std: Option<f32>,
        rolloff_mean: Option<f32>,
        rolloff_std: Option<f32>,
        flatness_mean: Option<f32>,
        flatness_std: Option<f32>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO descriptors(
                 track_id, bpm, musical_key, energy, loudness,
                 zcr, centroid_mean, centroid_std,
                 rolloff_mean, rolloff_std, flatness_mean, flatness_std)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(track_id) DO UPDATE SET
               bpm=excluded.bpm, musical_key=excluded.musical_key,
               energy=excluded.energy, loudness=excluded.loudness,
               zcr=excluded.zcr,
               centroid_mean=excluded.centroid_mean, centroid_std=excluded.centroid_std,
               rolloff_mean=excluded.rolloff_mean, rolloff_std=excluded.rolloff_std,
               flatness_mean=excluded.flatness_mean, flatness_std=excluded.flatness_std",
            params![
                track_id, bpm, musical_key, energy, loudness,
                zcr, centroid_mean, centroid_std,
                rolloff_mean, rolloff_std, flatness_mean, flatness_std
            ],
        )?;
        Ok(())
    }

    /// Combien de morceaux placés sur la carte ont des descripteurs.
    pub fn compter_descripteurs(&self, model: &str) -> Result<(i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM features WHERE model = ?1",
            params![model],
            |r| r.get(0),
        )?;
        let faits: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM descriptors d
              JOIN features f ON f.track_id = d.track_id AND f.model = ?1",
            params![model],
            |r| r.get(0),
        )?;
        Ok((faits, total))
    }

    /// Efface toutes les mesures de tempo/tonalité/énergie, pour les reprendre
    /// de zéro — sert au bouton « remesurer » du mode Bibliothèque : après une
    /// correction de l'algorithme, les valeurs déjà en base sont celles de
    /// l'ancien, pas des trous à combler. `pending_descripteurs` ne les
    /// reverrait jamais sans ça, une ligne y valant déjà « mesuré ».
    pub fn effacer_descripteurs(&self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM descriptors", [])?)
    }

    /// Tempo mesuré de morceaux donnés, ceux qui en ont.
    ///
    /// **Un morceau absent de la table rendue n'a pas de tempo** — soit qu'il
    /// n'ait pas été mesuré, soit que rien n'y pulse. La greffe de stem s'en
    /// sert pour écarter les candidats qu'elle ne saurait caler : sans les deux
    /// tempos, il n'y a pas de facteur d'étirement à calculer, et en inventer
    /// un ferait flotter la batterie greffée dès la deuxième mesure.
    pub fn tempos(&self, ids: &[i64]) -> Result<HashMap<i64, f32>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let trous = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "SELECT track_id, bpm FROM descriptors
              WHERE bpm IS NOT NULL AND track_id IN ({trous})"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(ids), |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        Ok(rows.collect::<std::result::Result<HashMap<i64, f32>, _>>()?)
    }

    /// Tempo, tonalité et énergie d'un morceau, tels que la passe les a
    /// mesurés.
    ///
    /// **Rien n'est inventé quand la mesure manque.** Les champs sont
    /// facultatifs un à un : un conte lu n'a pas de tempo qui veuille dire
    /// quelque chose, et 15 847 morceaux sur 27 044 seulement sont mesurés à ce
    /// jour. Une valeur par défaut serait pire que rien — elle s'afficherait
    /// comme une mesure.
    pub fn descripteurs(&self, id: i64) -> Result<Option<DescripteursVus>> {
        let mut stmt = self.conn.prepare(
            "SELECT bpm, musical_key, energy,
                    zcr, centroid_mean, centroid_std,
                    rolloff_mean, rolloff_std, flatness_mean, flatness_std
               FROM descriptors WHERE track_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |r| {
            Ok(DescripteursVus {
                bpm: r.get(0)?,
                tonalite: r.get(1)?,
                energie: r.get(2)?,
                zcr: r.get(3)?,
                centroide_moy: r.get(4)?,
                centroide_ecart: r.get(5)?,
                rolloff_moy: r.get(6)?,
                rolloff_ecart: r.get(7)?,
                flatness_moy: r.get(8)?,
                flatness_ecart: r.get(9)?,
            })
        })?;
        Ok(match rows.next() {
            Some(v) => Some(v?),
            None => None,
        })
    }

    /// Qualité d'encodage d'un morceau — codec, débit, échantillonnage,
    /// profondeur de bits. Lue au scan, `None` si le morceau n'est pas en base.
    pub fn qualite_piste(&self, id: i64) -> Result<Option<QualitePiste>> {
        Ok(self
            .conn
            .query_row(
                "SELECT codec, bitrate, sample_rate, channels, bit_depth
                   FROM tracks WHERE id = ?1",
                params![id],
                |r| {
                    Ok(QualitePiste {
                        codec: r.get(0)?,
                        bitrate: r.get(1)?,
                        sample_rate: r.get(2)?,
                        channels: r.get(3)?,
                        bit_depth: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /* ------------------------------------------------ genres MusicBrainz */

    /// Les artistes qu'il reste à interroger pour un échelon donné.
    ///
    /// `echelon` vaut `"artist"` (les genres de l'artiste) ou `"albums"` (le
    /// parcours de ses disques). La trace est gardée **même quand la réponse
    /// est vide** : sans elle, les artistes sans genre — un quart d'entre eux —
    /// seraient réinterrogés à chaque passe, à une requête par seconde.
    ///
    /// Les plus représentés d'abord : la couverture en morceaux monte alors
    /// bien plus vite que la couverture en artistes, et une passe interrompue
    /// laisse déjà un résultat exploitable.
    pub fn mb_artistes_en_attente(&self, echelon: &str, limite: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_artist_id, COUNT(*) n FROM tracks t
              WHERE t.mb_artist_id IS NOT NULL AND t.mb_artist_id <> ''
                AND NOT EXISTS (SELECT 1 FROM mb_fetched f
                                 WHERE f.mbid = t.mb_artist_id AND f.kind = ?1)
              GROUP BY t.mb_artist_id ORDER BY n DESC LIMIT ?2",
        )?;
        let brut: Vec<String> = stmt
            .query_map(params![echelon, limite as i64], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(brut)
    }

    /// Les genres d'un artiste, et la trace du passage.
    ///
    /// Les deux dans une transaction : une passe interrompue entre l'écriture
    /// et la marque réinterrogerait l'artiste, celle-ci interrompue dans
    /// l'autre ordre le tiendrait pour fait sans l'avoir enregistré.
    pub fn mb_poser_genres(
        &mut self,
        mbid: &str,
        echelon: &str,
        genres: &[(String, i64)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (nom, votes) in genres {
            tx.execute(
                "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(mbid, kind, genre) DO UPDATE SET votes = excluded.votes",
                params![mbid, echelon, nom, votes],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO mb_fetched (mbid, kind) VALUES (?1, ?2)",
            params![
                mbid,
                if echelon == "artist" {
                    "artist"
                } else {
                    echelon
                }
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Les albums d'un artiste et leurs genres, en une transaction.
    ///
    /// `albums` porte, pour chaque disque, son identifiant, son titre, le titre
    /// normalisé qui servira au rapprochement, et ses genres.
    pub fn mb_poser_albums(&mut self, artiste: &str, albums: &[AlbumRange]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (mbid, titre, norme, genres) in albums {
            tx.execute(
                "INSERT OR REPLACE INTO mb_release_groups (mbid, artist_mbid, title, title_norm)
                 VALUES (?1, ?2, ?3, ?4)",
                params![mbid, artiste, titre, norme],
            )?;
            for (nom, votes) in genres {
                tx.execute(
                    "INSERT INTO mb_genres (mbid, kind, genre, votes) VALUES (?1, 'release-group', ?2, ?3)
                     ON CONFLICT(mbid, kind, genre) DO UPDATE SET votes = excluded.votes",
                    params![mbid, nom, votes],
                )?;
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO mb_fetched (mbid, kind) VALUES (?1, 'albums')",
            params![artiste],
        )?;
        tx.commit()?;
        Ok(())
    }

    /* -------------------------------------- collaborations (mode Découvrir) */

    /// A-t-on déjà demandé les relations de cet artiste — même si la
    /// réponse était une liste vide. Sans cette trace, un artiste sans
    /// collaboration connue serait réinterrogé à chaque visite du mode
    /// Découvrir, une requête par seconde.
    pub fn liens_artiste_en_cache(&self, mbid: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM mb_fetched WHERE mbid = ?1 AND kind = 'relations')",
            params![mbid],
            |r| r.get(0),
        )?)
    }

    /// Les relations d'un artiste et la trace du passage, dans une
    /// transaction — même raison que [`Self::mb_poser_genres`] : une passe
    /// interrompue entre les deux écritures ne doit ni perdre le résultat,
    /// ni le tenir pour acquis sans l'avoir noté.
    pub fn enregistrer_liens_artiste(
        &mut self,
        mbid: &str,
        liens: &[crate::musicbrainz::Relation],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for lien in liens {
            tx.execute(
                "INSERT INTO artist_links (src_mbid, dst_mbid, dst_name, relation)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(src_mbid, dst_mbid, relation) DO UPDATE SET dst_name = excluded.dst_name",
                params![mbid, lien.dst_mbid, lien.dst_name, lien.relation],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO mb_fetched (mbid, kind) VALUES (?1, 'relations')",
            params![mbid],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Les liens déjà connus d'un artiste — toujours depuis le cache,
    /// jamais le réseau ; c'est [`Self::liens_artiste_en_cache`] qui décide
    /// s'il faut d'abord interroger MusicBrainz.
    pub fn liens_artiste(&self, mbid: &str) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT dst_mbid, COALESCE(dst_name, '?'), relation FROM artist_links
              WHERE src_mbid = ?1 ORDER BY relation, dst_name",
        )?;
        let rows = stmt
            .query_map(params![mbid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /* ------------------------------------------- mode Découvrir : le fil */

    /// Les MBID d'artistes d'album présents dans la bibliothèque.
    ///
    /// Sert deux fois dans la passe Découvrir : écarter des voisins ceux qu'on
    /// possède déjà, et retenir des sorties fraîches de ListenBrainz celles qui
    /// concernent un artiste connu.
    pub fn artist_mbids(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT mb_album_artist_id FROM tracks
              WHERE mb_album_artist_id IS NOT NULL AND mb_album_artist_id <> ''",
        )?;
        let s = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<HashSet<_>, _>>()?;
        Ok(s)
    }

    /// `mbid` → nom, pour tous les artistes d'album de la bibliothèque.
    ///
    /// Le mode Découvrir s'en sert pour nommer l'artiste-ancre d'une sortie
    /// repérée par ListenBrainz, qui ne rend que des identifiants.
    pub fn artist_noms(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT mb_album_artist_id, MIN(COALESCE(album_artist, artist))
               FROM tracks
              WHERE mb_album_artist_id IS NOT NULL AND mb_album_artist_id <> ''
                AND COALESCE(album_artist, artist) IS NOT NULL
              GROUP BY mb_album_artist_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = HashMap::new();
        for r in rows {
            let (mbid, nom) = r?;
            out.insert(mbid, nom);
        }
        Ok(out)
    }

    /// Marque une étape de la passe Découvrir comme faite maintenant.
    ///
    /// `decouvrir_poser_sorties` / `_voisins` le font déjà par artiste ; celle-ci
    /// sert à l'étape « sorties » qui n'a pas d'artiste (une seule requête
    /// ListenBrainz pour toute la bibliothèque), pour que `decouvrir_derniere_passe`
    /// la voie même quand il n'y a aucun voisin à interroger.
    pub fn decouvrir_marquer_passe(&self, etape: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO decouvrir_suivi (mbid, kind) VALUES ('@passe', ?1)",
            params![etape],
        )?;
        Ok(())
    }

    /// Les artistes de la bibliothèque à (ré)interroger pour `kind`
    /// (`"sorties"` ou `"voisins"`), les plus fournis d'abord — la couverture
    /// en morceaux monte alors vite, comme pour [`Self::mb_artistes_en_attente`].
    ///
    /// Un artiste revient quand son suivi est absent ou plus vieux que
    /// `peremption_jours` : une actualité vieillit, là où un genre est acquis
    /// une fois pour toutes. `limite` à 0 = tous.
    pub fn decouvrir_en_attente(
        &self,
        kind: &str,
        peremption_jours: i64,
        limite: usize,
    ) -> Result<Vec<(String, String)>> {
        let limite = if limite == 0 { i64::MAX } else { limite as i64 };
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_album_artist_id,
                    MIN(COALESCE(t.album_artist, t.artist)),
                    COUNT(*) n
               FROM tracks t
               LEFT JOIN decouvrir_suivi s
                      ON s.mbid = t.mb_album_artist_id AND s.kind = ?1
              WHERE t.mb_album_artist_id IS NOT NULL AND t.mb_album_artist_id <> ''
                AND COALESCE(t.album_artist, t.artist) IS NOT NULL
                AND (s.at IS NULL OR s.at < strftime('%s','now') - ?2 * 86400)
              GROUP BY t.mb_album_artist_id
              ORDER BY n DESC
              LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![kind, peremption_jours, limite], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Insère une sortie repérée dans le fil.
    ///
    /// `INSERT OR IGNORE` sur `rg_mbid` : une sortie déjà connue garde son `vu`
    /// et sa date de repérage. Rend `true` si la ligne est neuve.
    pub fn decouvrir_ajouter_sortie(
        &self,
        artiste_mbid: &str,
        artiste_nom: &str,
        s: &SortieARanger,
    ) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO decouvrir_sorties
               (rg_mbid, artiste_mbid, artiste_nom, titre, date_sortie,
                date_sortie_norm, type_primaire, types_secondaires, collaborateurs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                s.rg_mbid,
                artiste_mbid,
                artiste_nom,
                s.titre,
                s.date_sortie,
                s.date_sortie_norm,
                s.type_primaire,
                s.types_secondaires,
                s.collaborateurs,
            ],
        )?;
        Ok(n > 0)
    }

    /// Range les voisins d'un artiste et marque son suivi, dans une
    /// transaction. `ON CONFLICT` rafraîchit le score sans perdre le `vu`.
    /// Rend le nombre de lignes écrites (insérées ou mises à jour).
    pub fn decouvrir_poser_voisins(
        &mut self,
        src_mbid: &str,
        voisins: &[(String, String, f64, String)],
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut ecrits = 0usize;
        for (dst_mbid, dst_nom, score, source) in voisins {
            ecrits += tx.execute(
                "INSERT INTO decouvrir_voisins (src_mbid, dst_mbid, dst_nom, score, source)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(src_mbid, dst_mbid, source)
                   DO UPDATE SET score = excluded.score, dst_nom = excluded.dst_nom",
                params![src_mbid, dst_mbid, dst_nom, score, source],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO decouvrir_suivi (mbid, kind) VALUES (?1, 'voisins')",
            params![src_mbid],
        )?;
        tx.commit()?;
        Ok(ecrits)
    }

    /// Le fil complet du mode Découvrir.
    ///
    /// Ne garde que les sorties dont la date normalisée tombe dans les
    /// `fenetre_jours` derniers jours. Une sortie à collaborateurs va dans
    /// `collaborations`, les autres dans `sorties`. Les voisins sont regroupés
    /// par artiste cible et classés par nombre de portes d'entrée.
    pub fn decouvrir_fil(&self, fenetre_jours: i64) -> Result<FilDecouvrir> {
        let derniere_passe: Option<i64> =
            self.conn
                .query_row("SELECT MAX(at) FROM decouvrir_suivi", [], |r| r.get(0))?;

        let fenetre = format!("-{fenetre_jours} days");
        let mut stmt = self.conn.prepare(
            "SELECT rg_mbid, artiste_mbid, artiste_nom, titre, date_sortie,
                    type_primaire, collaborateurs, vu
               FROM decouvrir_sorties
              WHERE date_sortie_norm IS NOT NULL
                AND date_sortie_norm >= date('now', ?1)
              ORDER BY date_sortie_norm DESC, artiste_nom COLLATE NOCASE",
        )?;
        let toutes: Vec<SortieFil> = stmt
            .query_map(params![fenetre], |r| {
                Ok(SortieFil {
                    rg_mbid: r.get(0)?,
                    artiste_mbid: r.get(1)?,
                    artiste_nom: r.get(2)?,
                    titre: r.get(3)?,
                    date_sortie: r.get(4)?,
                    type_primaire: r.get(5)?,
                    collaborateurs: r.get(6)?,
                    vu: r.get::<_, i64>(7)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let (collaborations, sorties): (Vec<SortieFil>, Vec<SortieFil>) = toutes
            .into_iter()
            .partition(|s| s.collaborateurs.as_deref().is_some_and(|c| !c.is_empty()));

        let mut stmt = self.conn.prepare(
            "SELECT v.dst_mbid, MIN(v.dst_nom), MAX(v.score), MIN(v.source), MIN(v.vu),
                    GROUP_CONCAT(DISTINCT COALESCE(a.nom, v.src_mbid)),
                    GROUP_CONCAT(DISTINCT v.src_mbid)
               FROM decouvrir_voisins v
               LEFT JOIN (
                    SELECT mb_album_artist_id AS mbid,
                           MIN(COALESCE(album_artist, artist)) AS nom
                      FROM tracks
                     WHERE mb_album_artist_id IS NOT NULL
                     GROUP BY mb_album_artist_id
               ) a ON a.mbid = v.src_mbid
              WHERE v.dst_mbid NOT IN (
                        SELECT mb_album_artist_id FROM tracks
                         WHERE mb_album_artist_id IS NOT NULL AND mb_album_artist_id <> '')
              GROUP BY v.dst_mbid
              ORDER BY COUNT(*) DESC, MAX(v.score) DESC
              LIMIT 60",
        )?;
        let voisins: Vec<VoisinFil> = stmt
            .query_map([], |r| {
                let portes: String = r.get::<_, Option<String>>(5)?.unwrap_or_default();
                let src_mbids: String = r.get::<_, Option<String>>(6)?.unwrap_or_default();
                Ok(VoisinFil {
                    dst_mbid: r.get(0)?,
                    dst_nom: r.get(1)?,
                    score: r.get(2)?,
                    source: r.get(3)?,
                    vu: r.get::<_, i64>(4)? != 0,
                    // GROUP_CONCAT DISTINCT ne prend pas de séparateur autre que
                    // la virgule ; un nom d'artiste qui en contient une est rare
                    // et sans conséquence ici (une porte affichée en deux).
                    portes: portes
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect(),
                    // Un mbid ne contient jamais de virgule — le découpage est sûr.
                    src_mbids: src_mbids
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(FilDecouvrir {
            derniere_passe,
            sorties,
            collaborations,
            voisins,
        })
    }

    /// La date d'il y a `jours` jours, en `YYYY-MM-DD`. SQLite tient l'horloge —
    /// pas besoin d'un crate de calendrier pour borner la fenêtre d'actualité.
    pub fn date_il_y_a(&self, jours: i64) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT date('now', ?1)",
            params![format!("-{jours} days")],
            |r| r.get(0),
        )?)
    }

    /// La date (epoch s) de la dernière passe Découvrir, ou `None` si aucune —
    /// l'interface s'en sert pour décider s'il faut relancer à l'ouverture.
    pub fn decouvrir_derniere_passe(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT MAX(at) FROM decouvrir_suivi", [], |r| r.get(0))?)
    }

    /// Marque tout le fil comme vu — les pastilles « nouveau » s'éteignent.
    pub fn decouvrir_tout_vu(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE decouvrir_sorties SET vu = 1 WHERE vu = 0;
             UPDATE decouvrir_voisins SET vu = 1 WHERE vu = 0;",
        )?;
        Ok(())
    }

    /// Efface ce qui est sorti de la fenêtre d'actualité, pour que les tables
    /// ne gonflent pas indéfiniment : les sorties datées de plus de
    /// `garder_jours`, et les voisins qu'aucune passe n'a revus depuis 90 jours.
    pub fn decouvrir_elaguer(&self, garder_jours: i64) -> Result<()> {
        let limite = format!("-{garder_jours} days");
        self.conn.execute(
            "DELETE FROM decouvrir_sorties
              WHERE date_sortie_norm IS NULL OR date_sortie_norm < date('now', ?1)",
            params![limite],
        )?;
        self.conn.execute(
            "DELETE FROM decouvrir_voisins
              WHERE repere_le < strftime('%s','now') - 90 * 86400",
            [],
        )?;
        Ok(())
    }

    /// Genres par identifiant, du plus sûr au moins sûr.
    ///
    /// **Le classement compte plus que le seuil**, et l'avoir cru l'inverse a
    /// coûté un bogue. Chez Yann Tiersen, dix genres portent une seule voix —
    /// `modern classical`, `minimalism`, `instrumental`, tous justes, et
    /// `amapiano`, qui ne l'est pas. Départager par ordre alphabétique mettait
    /// `amapiano` en tête et nommait ainsi la famille de piano néoclassique.
    ///
    /// À votes égaux, on départage donc par **le nombre d'artistes de la
    /// bibliothèque qui portent ce genre** : un genre que personne d'autre ne
    /// porte est un accident de contributeur, un genre partagé par dix-sept
    /// artistes est une catégorie. `amapiano` tombe alors dernier, et
    /// `instrumental` remonte.
    ///
    /// La popularité se mesure toujours sur l'échelon artiste, même quand on
    /// interroge les albums : c'est le plus large échantillon dont on dispose.
    pub fn mb_genres(&self, echelon: &str, plancher: i64) -> Result<HashMap<String, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "WITH portee AS (
                 SELECT genre, COUNT(DISTINCT mbid) n FROM mb_genres
                  WHERE kind = 'artist' GROUP BY genre)
             SELECT g.mbid, g.genre FROM mb_genres g
               LEFT JOIN portee p ON p.genre = g.genre
              WHERE g.kind = ?1 AND g.votes >= ?2
              ORDER BY g.mbid, g.votes DESC, COALESCE(p.n, 0) DESC, g.genre",
        )?;
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        let lignes = stmt.query_map(params![echelon, plancher], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for l in lignes {
            let (mbid, genre) = l?;
            out.entry(mbid).or_default().push(genre);
        }
        Ok(out)
    }

    /// Les albums connus, indexés par `(artiste, titre normalisé)`.
    ///
    /// Nos fichiers ne portent pas d'identifiant d'album : c'est ce couple qui
    /// fait le lien avec un release-group.
    pub fn mb_albums(&self) -> Result<HashMap<(String, String), String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT artist_mbid, title_norm, mbid FROM mb_release_groups")?;
        let lignes = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = HashMap::new();
        for l in lignes {
            let (artiste, norme, mbid) = l?;
            // Un même titre normalisé peut désigner deux release-groups (album
            // et sa compilation homonyme) : le premier suffit, ils portent des
            // genres voisins.
            out.entry((artiste, norme)).or_insert(mbid);
        }
        Ok(out)
    }

    /* --------------------------------------------- popularité générale */

    /// Les enregistrements dont la popularité reste à récupérer sur au moins
    /// une source (ListenBrainz ou Deezer). `depuis` est l'instant à partir
    /// duquel un enregistrement déjà interrogé compte comme « frais » :
    /// `depuis = 0` ne rafraîchit rien, `now − 90 j` réinterroge le périmé.
    /// Les plus représentés d'abord, comme [`Self::mb_artistes_en_attente`].
    pub fn pop_recordings_candidats(
        &self,
        depuis: i64,
        limite: usize,
    ) -> Result<Vec<PisteAPopulariser>> {
        let limite = if limite == usize::MAX {
            i64::MAX
        } else {
            limite as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT t.mb_recording_id, MIN(t.artist), MIN(t.title), COUNT(*) n
               FROM tracks t
              WHERE t.mb_recording_id IS NOT NULL AND t.mb_recording_id <> ''
                AND (SELECT COUNT(*) FROM popularite_fetched f
                      WHERE f.mbid = t.mb_recording_id AND f.kind = 'recording'
                        AND f.source IN ('listenbrainz', 'deezer') AND f.at >= ?1) < 2
              GROUP BY t.mb_recording_id
              ORDER BY n DESC
              LIMIT ?2",
        )?;
        let out = stmt
            .query_map(params![depuis, limite], |r| {
                Ok(PisteAPopulariser {
                    recording_mbid: r.get(0)?,
                    artiste: r.get(1)?,
                    titre: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(out)
    }

    /// Les release-groups dont la popularité ListenBrainz reste à récupérer.
    /// Le lien morceau → release-group se fait en Rust, par
    /// `(mb_artist_id, titre normalisé)` — exactement comme [`genres_du_morceau`].
    pub fn pop_rg_candidats(&self, depuis: i64, limite: usize) -> Result<Vec<String>> {
        let albums = self.mb_albums()?;
        let deja = self.pop_deja_fait("listenbrainz", "release-group", depuis)?;

        let mut stmt = self.conn.prepare(
            "SELECT mb_artist_id, album, COUNT(*) n FROM tracks
              WHERE mb_artist_id IS NOT NULL AND mb_artist_id <> '' AND album IS NOT NULL
              GROUP BY mb_artist_id, album
              ORDER BY n DESC",
        )?;
        let lignes: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;

        let mut vus = HashSet::new();
        let mut out = Vec::new();
        for (artiste, album) in lignes {
            let cle = (artiste, crate::musicbrainz::normaliser_titre(&album));
            if let Some(rg) = albums.get(&cle) {
                if !deja.contains(rg) && vus.insert(rg.clone()) {
                    out.push(rg.clone());
                    if out.len() >= limite {
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    /// Les MBID déjà interrogés (et encore frais) pour une source et un
    /// échelon donnés. `depuis` : voir [`Self::pop_recordings_candidats`].
    pub fn pop_deja_fait(
        &self,
        source: &str,
        kind: &str,
        depuis: i64,
    ) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT mbid FROM popularite_fetched
              WHERE source = ?1 AND kind = ?2 AND at >= ?3",
        )?;
        let s = stmt
            .query_map(params![source, kind, depuis], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(s)
    }

    /// Range les popularités d'un lot et marque tous les MBID demandés comme
    /// interrogés — les deux dans une transaction, comme
    /// [`Self::mb_poser_genres`] : une passe coupée ne perd ni ne refait.
    /// `demandes` couvre tout le lot (y compris les inconnus, pour ne pas y
    /// revenir) ; `trouves` n'en est que le sous-ensemble ayant rendu un
    /// chiffre.
    pub fn pop_poser(
        &mut self,
        source: &str,
        kind: &str,
        demandes: &[String],
        trouves: &[PopulariteBrute<'_>],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for p in trouves {
            tx.execute(
                "INSERT INTO popularite (mbid, kind, source, ecoutes, auditeurs, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))
                 ON CONFLICT(mbid, kind, source)
                 DO UPDATE SET ecoutes = excluded.ecoutes,
                               auditeurs = excluded.auditeurs,
                               at = excluded.at",
                params![p.mbid, kind, source, p.ecoutes, p.auditeurs],
            )?;
        }
        for mbid in demandes {
            tx.execute(
                "INSERT OR REPLACE INTO popularite_fetched (mbid, kind, source)
                 VALUES (?1, ?2, ?3)",
                params![mbid, kind, source],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// État de fraîcheur de la popularité, pour la ligne d'alerte du mode
    /// Bibliothèque : combien de morceaux ont une popularité, l'instant de la
    /// plus ancienne interrogation, et combien d'entités datent de plus de
    /// `peremption_jours`.
    pub fn popularite_fraicheur(&self, peremption_jours: i64) -> Result<(i64, Option<i64>, i64)> {
        let couverts: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM track_popularite", [], |r| r.get(0))?;
        let plus_ancienne: Option<i64> = self
            .conn
            .query_row("SELECT MIN(at) FROM popularite_fetched", [], |r| r.get(0))?;
        let perimes: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM popularite_fetched
              WHERE at < strftime('%s','now') - ?1",
            params![peremption_jours * 86_400],
            |r| r.get(0),
        )?;
        Ok((couverts, plus_ancienne, perimes))
    }

    /// `mbid → métrique brute` pour une source et un échelon (écoutes
    /// ListenBrainz, `rank` Deezer).
    fn pop_valeurs(&self, source: &str, kind: &str) -> Result<HashMap<String, f64>> {
        let mut stmt = self.conn.prepare(
            "SELECT mbid, ecoutes FROM popularite
              WHERE source = ?1 AND kind = ?2 AND ecoutes IS NOT NULL",
        )?;
        let m = stmt
            .query_map(params![source, kind], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as f64))
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(m)
    }

    /// Recalcule `track_popularite` en entier depuis `popularite` et la
    /// distribution de la bibliothèque — voir « la valeur affichée » de
    /// `docs/popularite.md`. Rend le nombre de morceaux couverts.
    ///
    /// Pour chaque morceau : sa métrique par source à son meilleur échelon
    /// (enregistrement → release-group), chaque métrique convertie en **rang
    /// percentile** dans la bibliothèque, puis la **médiane** des rangs des
    /// sources disponibles. Un morceau sans aucune source n'a pas de ligne.
    pub fn recalculer_track_popularite(&mut self) -> Result<usize> {
        let albums = self.mb_albums()?;
        let lb_rec = self.pop_valeurs("listenbrainz", "recording")?;
        let lb_rg = self.pop_valeurs("listenbrainz", "release-group")?;
        let dz_rec = self.pop_valeurs("deezer", "recording")?;

        let pistes: Vec<PistePop> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, mb_recording_id, mb_artist_id, album FROM tracks")?;
            let v = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<std::result::Result<_, _>>()?;
            v
        };

        // Valeur brute par source et échelon de chaque morceau.
        struct Brut {
            id: i64,
            lb: Option<f64>,
            dz: Option<f64>,
            echelon: &'static str,
        }
        let mut bruts = Vec::new();
        for (id, rec, artiste, album) in &pistes {
            let rg = match (artiste, album) {
                (Some(a), Some(al)) => albums
                    .get(&(a.clone(), crate::musicbrainz::normaliser_titre(al)))
                    .cloned(),
                _ => None,
            };
            let lb_r = rec.as_deref().and_then(|m| lb_rec.get(m)).copied();
            let dz_r = rec.as_deref().and_then(|m| dz_rec.get(m)).copied();
            let lb_g = rg.as_deref().and_then(|m| lb_rg.get(m)).copied();

            let lb = lb_r.or(lb_g);
            let echelon = if lb_r.is_some() || dz_r.is_some() {
                "recording"
            } else if lb_g.is_some() {
                "release-group"
            } else {
                continue;
            };
            bruts.push(Brut {
                id: *id,
                lb,
                dz: dz_r,
                echelon,
            });
        }

        let rang_lb = rangs_percentiles(bruts.iter().filter_map(|b| b.lb.map(|v| (b.id, v))));
        let rang_dz = rangs_percentiles(bruts.iter().filter_map(|b| b.dz.map(|v| (b.id, v))));

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM track_popularite", [])?;
        let mut n = 0usize;
        for b in &bruts {
            let mut rangs: Vec<f64> = Vec::new();
            if let Some(r) = rang_lb.get(&b.id) {
                rangs.push(*r);
            }
            if let Some(r) = rang_dz.get(&b.id) {
                rangs.push(*r);
            }
            if rangs.is_empty() {
                continue;
            }
            rangs.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let relative = if rangs.len() % 2 == 1 {
                rangs[rangs.len() / 2]
            } else {
                (rangs[rangs.len() / 2 - 1] + rangs[rangs.len() / 2]) / 2.0
            };
            tx.execute(
                "INSERT INTO track_popularite (track_id, relative, echelon, calcule_le)
                 VALUES (?1, ?2, ?3, strftime('%s','now'))",
                params![b.id, relative, b.echelon],
            )?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Combien d'artistes ont été interrogés, et combien ont rendu un genre.
    pub fn mb_avancement(&self) -> Result<(i64, i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT mb_artist_id) FROM tracks
              WHERE mb_artist_id IS NOT NULL AND mb_artist_id <> ''",
            [],
            |r| r.get(0),
        )?;
        let faits: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM mb_fetched WHERE kind = 'artist'",
            [],
            |r| r.get(0),
        )?;
        let avec: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT mbid) FROM mb_genres WHERE kind = 'artist'",
            [],
            |r| r.get(0),
        )?;
        Ok((faits, total, avec))
    }

    /// Combien de morceaux ont un genre MusicBrainz utilisable.
    pub fn mb_couverture(&self, model: &str, plancher: i64) -> Result<(i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM features WHERE model = ?1",
            params![model],
            |r| r.get(0),
        )?;
        let couverts: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM features f JOIN tracks t ON t.id = f.track_id
              WHERE f.model = ?1 AND EXISTS (
                    SELECT 1 FROM mb_genres g
                     WHERE g.kind = 'artist' AND g.votes >= ?2
                       AND g.mbid = t.mb_artist_id)",
            params![model, plancher],
            |r| r.get(0),
        )?;
        Ok((couverts, total))
    }

    /// Combien d'empreintes existent pour ce modèle.
    ///
    /// Sert à savoir si un cache d'empreintes est périmé sans les relire.
    /// Attention à ne pas compter `map_points` à sa place : celui-ci écarte
    /// les morceaux pas encore projetés, si bien que les deux nombres
    /// diffèrent pendant toute la durée d'une analyse — un cache réglé
    /// dessus se croirait périmé à chaque appel.
    pub fn count_embeddings(&self, model: &str) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM features WHERE model = ?1",
            params![model],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Écrit les positions et les familles d'un coup.
    pub fn update_map(&self, model: &str, points: &[(i64, f32, f32, i64)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE features SET x=?3, y=?4, cluster=?5 WHERE track_id=?1 AND model=?2",
            )?;
            for (id, x, y, c) in points {
                stmt.execute(params![id, model, x, y, c])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// La carte complète, prête à dessiner : positions et étiquettes.
    ///
    /// Une seule requête plutôt qu'un aller-retour par point — à 27 000
    /// morceaux, l'interface les charge tous d'un coup et n'y revient qu'à la
    /// demande de l'utilisateur.
    pub fn map_view(&self, model: &str) -> Result<Vec<MapPoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.path, f.x, f.y, COALESCE(f.cluster, -1),
                    t.title, t.artist, t.album_artist, t.album, t.track_no, t.year, t.duration_ms,
                    d.bpm, d.energy, tp.relative
               FROM features f
               JOIN tracks t ON t.id = f.track_id
               LEFT JOIN descriptors d ON d.track_id = t.id
               LEFT JOIN track_popularite tp ON tp.track_id = t.id
              WHERE f.model = ?1 AND f.x IS NOT NULL
              ORDER BY t.id",
        )?;
        let rows = stmt
            .query_map(params![model], |r| {
                Ok(MapPoint {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    x: r.get(2)?,
                    y: r.get(3)?,
                    cluster: r.get(4)?,
                    title: r.get(5)?,
                    artist: r.get(6)?,
                    album_artist: r.get(7)?,
                    album: r.get(8)?,
                    track_no: r.get(9)?,
                    year: r.get(10)?,
                    duration_ms: r.get(11)?,
                    bpm: r.get(12)?,
                    energy: r.get(13)?,
                    popularite: r.get(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Positions sur la carte pour un modèle donné — ce que lira le module 2.
    /// L'ordre d'arrivée des morceaux, pour le placement chronologique.
    ///
    /// Rend `(track_id, date AAAAMMJJ, source, clé d'album, disque, piste)`,
    /// **déjà trié** : l'ordre lexicographique de ce n-uplet est l'ordre du
    /// peuplement.
    ///
    /// L'échelle de datation, du plus fiable au moins :
    ///
    /// | source | d'où | ce qu'elle vaut |
    /// |---|---|---|
    /// | `musicbrainz` | `first_release_date` du release-group | la date de l'œuvre, pas du pressage — corrige les rééditions |
    /// | `tag` | `tracks.year` | date de l'édition ; 26 493 morceaux sur 27 044 |
    /// | `album` | médiane des frères datés du même album | +56 mesurés |
    /// | `artiste` | médiane des morceaux datés de l'artiste | +23 mesurés |
    /// | `ingestion` | `tracks.added_at` | 472 morceaux, dont 504 n'ont aucun MBID : rien ne les sauvera |
    ///
    /// Les morceaux sans date ne sont **ni écartés ni maquillés** : ils portent
    /// leur date d'entrée dans la bibliothèque et `date_source = 'ingestion'`,
    /// pour que la carte puisse les rendre autrement et qu'une correction
    /// ultérieure sache lesquels rejouer.
    pub fn ordre_darrivee(&self) -> Result<Vec<ArriveeBrute>> {
        // 1. Ce que les tags disent.
        let mut stmt = self.conn.prepare(
            // `GROUP BY t.id` n'est pas une précaution : un artiste peut avoir
            // plusieurs release-groups au même titre normalisé — un album et sa
            // réédition, une version live homonyme — et la jointure rendait
            // alors **plusieurs lignes par morceau**. Mesuré : 28 363 arrivées
            // pour 27 044 morceaux. `MIN` retient la plus ancienne, ce qui est
            // précisément la date d'œuvre qu'on cherche.
            "SELECT t.id, t.year, t.album, t.album_artist, t.artist, t.track_no, t.added_at,
                    MIN(r.first_release_date), MIN(r.secondary_types)
               FROM tracks t
               LEFT JOIN mb_release_groups r
                      ON r.artist_mbid = t.mb_album_artist_id
                     AND r.title_norm = lower(trim(COALESCE(t.album, '')))
              GROUP BY t.id
              ORDER BY t.id",
        )?;
        struct Ligne {
            id: i64,
            annee: Option<i64>,
            album: String,
            artiste: String,
            piste: i64,
            ajoute: i64,
            mb_date: Option<String>,
            compilation: bool,
        }
        let lignes: Vec<Ligne> = stmt
            .query_map([], |r| {
                let album: Option<String> = r.get(2)?;
                let album_artiste: Option<String> = r.get(3)?;
                let artiste: Option<String> = r.get(4)?;
                let types: Option<String> = r.get(8)?;
                Ok(Ligne {
                    id: r.get(0)?,
                    annee: r.get(1)?,
                    album: album.unwrap_or_default(),
                    artiste: album_artiste
                        .or(artiste)
                        .unwrap_or_default(),
                    piste: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    ajoute: r.get(6)?,
                    mb_date: r.get(7)?,
                    compilation: types
                        .as_deref()
                        .is_some_and(|t| t.to_lowercase().contains("compilation")),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;

        // 2. Les médianes de secours, par album puis par artiste.
        let mut par_album: HashMap<&str, Vec<i64>> = HashMap::new();
        let mut par_artiste: HashMap<&str, Vec<i64>> = HashMap::new();
        for l in &lignes {
            if let Some(a) = l.annee.filter(|a| (1900..=2100).contains(a)) {
                if !l.album.is_empty() {
                    par_album.entry(l.album.as_str()).or_default().push(a);
                }
                if !l.artiste.is_empty() {
                    par_artiste.entry(l.artiste.as_str()).or_default().push(a);
                }
            }
        }
        let mediane = |v: &mut Vec<i64>| -> i64 {
            v.sort_unstable();
            v[v.len() / 2]
        };
        let med_album: HashMap<&str, i64> = par_album
            .into_iter()
            .map(|(k, mut v)| (k, mediane(&mut v)))
            .collect();
        let med_artiste: HashMap<&str, i64> = par_artiste
            .into_iter()
            .map(|(k, mut v)| (k, mediane(&mut v)))
            .collect();

        let mut sortie: Vec<ArriveeBrute> = lignes
            .iter()
            .map(|l| {
                // Une compilation date d'elle-même, pas des œuvres qu'elle
                // rassemble : sa date de release-group ne vaut rien ici.
                let mb = if l.compilation { None } else { l.mb_date.as_deref() };
                let (date, source) = if let Some(d) = mb.and_then(date_iso_vers_cle) {
                    (d, "musicbrainz")
                } else if let Some(a) = l.annee.filter(|a| (1900..=2100).contains(a)) {
                    (a as u32 * 10_000, "tag")
                } else if let Some(&a) = med_album.get(l.album.as_str()) {
                    (a as u32 * 10_000, "album")
                } else if let Some(&a) = med_artiste.get(l.artiste.as_str()) {
                    (a as u32 * 10_000, "artiste")
                } else {
                    (epoch_vers_cle(l.ajoute), "ingestion")
                };
                ArriveeBrute {
                    track_id: l.id,
                    date,
                    source: source.to_string(),
                    // Un album arrive **en bloc** : sans ce regroupement, ses
                    // pistes s'éparpilleraient parmi les 1 341 arrivées d'une
                    // même année et chacune irait fonder ailleurs.
                    album: hacher(&l.artiste, &l.album),
                    piste: l.piste.clamp(0, 9999) as u16,
                }
            })
            .collect();

        sortie.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then_with(|| a.album.cmp(&b.album))
                .then_with(|| a.piste.cmp(&b.piste))
                .then_with(|| a.track_id.cmp(&b.track_id))
        });
        Ok(sortie)
    }

    /// Quand la projection a été calculée pour la dernière fois, en epoch.
    ///
    /// Sert à savoir si un dérivé de la carte — les tuiles vectorielles, par
    /// exemple — a été fabriqué avant ou après le dernier recalcul. `None`
    /// quand rien n'est encore projeté.
    pub fn derniere_projection(&self, model: &str) -> Result<Option<i64>> {
        let v: Option<i64> = self.conn.query_row(
            "SELECT MAX(computed_at) FROM features WHERE model = ?1 AND x IS NOT NULL",
            params![model],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    pub fn map_points(&self, model: &str) -> Result<Vec<(i64, f32, f32, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id, x, y, cluster FROM features
              WHERE model = ?1 AND x IS NOT NULL
              ORDER BY track_id",
        )?;
        let rows = stmt
            .query_map(params![model], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?)
    }

    /// Morceaux sans empreinte **pour ce modèle** (module 2).
    ///
    /// Le critère est l'absence d'empreinte, et non le drapeau `analyzed_at` :
    /// celui-ci ignore le modèle. Changer de représentation — un autre réseau,
    /// un autre fenêtrage — laissait donc les morceaux déjà passés hors de la
    /// nouvelle passe, et la carte restait amputée de moitié sans le moindre
    /// message. `analyzed_at` garde son rôle de date, pas de verrou.
    ///
    /// **Exclut aussi les fichiers déjà en échec** (`scan_failures`) : sans
    /// empreinte, un fichier qui échoue au décodage y reste indéfiniment, et
    /// le retenter à chaque passe reviendrait à retaper la même lecture
    /// problématique sur le même support — c'est précisément ce qui a fait
    /// paniquer le pilote PCIe d'un lecteur de carte SD en pratique. Retirer
    /// un fichier de cette liste (`effacer_echec_scan`) lui rend sa chance.
    pub fn pending_analysis(&self, model: &str, limit: i64) -> Result<Vec<TrackRow>> {
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks
              WHERE NOT EXISTS (
                    SELECT 1 FROM features f
                     WHERE f.track_id = tracks.id AND f.model = ?1)
                AND NOT EXISTS (
                    SELECT 1 FROM scan_failures sf WHERE sf.path = tracks.path)
              ORDER BY added_at LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![model, limit], track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---------------------------------------------------------------------
    // Consultation — ce que les modules lisent. Aucun d'eux ne relit le disque.
    // ---------------------------------------------------------------------

    /// Un morceau par son identifiant.
    pub fn track(&self, id: i64) -> Result<Option<TrackRow>> {
        let sql = format!("SELECT {TRACK_COLS} FROM tracks WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![id], track_from_row)
            .optional()?)
    }

    /// Artistes par ordre alphabétique.
    ///
    /// Le regroupement se fait sur l'identifiant MusicBrainz d'artiste d'album,
    /// avec repli sur le nom quand il manque. Sans cela, chaque « X feat. Y »
    /// forme sa propre entrée : 3 543 artistes au lieu de ~1 000 sur la
    /// bibliothèque de test. On ne peut pas se rabattre sur l'identifiant
    /// d'artiste *de piste*, qui porte plusieurs valeurs sur un featuring.
    ///
    /// Les morceaux sans artiste sont exclus (ils restent atteignables par
    /// album et par recherche) : la bibliothèque de test en compte 55, non
    /// étiquetés à la source.
    pub fn artists(&self) -> Result<Vec<ArtistRow>> {
        let mut stmt = self.conn.prepare(
            "WITH src AS (
               SELECT COALESCE(album_artist, artist) AS nom,
                      mb_album_artist_id            AS mbid,
                      album
                 FROM tracks
                WHERE COALESCE(album_artist, artist) IS NOT NULL
             ),
             -- Tous les fichiers d'un artiste ne portent pas forcément son
             -- identifiant MusicBrainz. Sans ce rattrapage, ses pistes
             -- étiquetées et les autres tombent dans deux paniers et
             -- l'artiste apparaît en double, avec des comptes partiels.
             -- Le HAVING laisse de côté les noms qui désignent plusieurs
             -- artistes : mieux vaut deux lignes qu'une fusion abusive.
             resolu AS (
               SELECT nom, MIN(mbid) AS mbid
                 FROM src WHERE mbid IS NOT NULL
                GROUP BY nom HAVING COUNT(DISTINCT mbid) = 1
             )
             SELECT MIN(src.nom),
                    COALESCE(src.mbid, resolu.mbid),
                    COUNT(*),
                    COUNT(DISTINCT src.album)
               FROM src LEFT JOIN resolu ON resolu.nom = src.nom
             -- Le repli sur le nom doit rester distinct d'un identifiant :
             -- COALESCE seul confondrait un nom et un MBID homonymes.
              GROUP BY COALESCE('id:' || COALESCE(src.mbid, resolu.mbid), 'nom:' || src.nom)
              ORDER BY MIN(src.nom) COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ArtistRow {
                    name: r.get(0)?,
                    mbid: r.get(1)?,
                    tracks: r.get(2)?,
                    albums: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Albums d'un artiste tel que [`Library::artists`] le regroupe.
    ///
    /// Prend l'identifiant **et** le nom, exactement comme le regroupement :
    /// un artiste réunit ses pistes étiquetées MusicBrainz et celles qui ne le
    /// sont pas. Filtrer sur le seul identifiant ferait disparaître les
    /// secondes, et la ligne annoncerait plus d'albums qu'elle n'en ouvre.
    ///
    /// L'identifiant peut manquer à l'appel (une case de la grille d'albums ne
    /// le porte pas). On le rattrape alors depuis le nom, comme
    /// [`Library::artists`] : sans ce repli, un artiste dont toutes les pistes
    /// sont étiquetées MusicBrainz ne renverrait aucun album.
    pub fn albums_of_artist(&self, mbid: Option<&str>, name: &str) -> Result<Vec<AlbumRow>> {
        let mut stmt = self.conn.prepare(
            "WITH resolu AS (
               SELECT MIN(mb_album_artist_id) AS mbid
                 FROM tracks
                WHERE mb_album_artist_id IS NOT NULL
                  AND COALESCE(album_artist, artist) = ?2
               HAVING COUNT(DISTINCT mb_album_artist_id) = 1
             )
             SELECT album, COALESCE(album_artist, artist), MIN(year), COUNT(*), MIN(path)
               FROM tracks
              WHERE album IS NOT NULL
                AND ( (COALESCE(?1, (SELECT mbid FROM resolu)) IS NOT NULL
                       AND mb_album_artist_id = COALESCE(?1, (SELECT mbid FROM resolu)))
                   OR (mb_album_artist_id IS NULL
                       AND COALESCE(album_artist, artist) = ?2) )
              GROUP BY album, COALESCE(album_artist, artist)
              ORDER BY album COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map(params![mbid, name], |r| {
                Ok(AlbumRow {
                    name: r.get(0)?,
                    artist: r.get(1)?,
                    year: r.get(2)?,
                    tracks: r.get(3)?,
                    path: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Albums, tous ou ceux d'un artiste donné.
    pub fn albums(&self, artist: Option<&str>) -> Result<Vec<AlbumRow>> {
        // `artist` filtre sur l'artiste d'album comme sur celui de la piste :
        // sinon un album entier échappe au filtre dès qu'une piste porte un
        // invité en artiste.
        let mut stmt = self.conn.prepare(
            "SELECT album, COALESCE(album_artist, artist), MIN(year), COUNT(*), MIN(path)
               FROM tracks
              WHERE album IS NOT NULL
                AND (?1 IS NULL OR album_artist = ?1 OR artist = ?1)
              GROUP BY album, COALESCE(album_artist, artist)
              ORDER BY album COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map(params![artist], |r| {
                Ok(AlbumRow {
                    name: r.get(0)?,
                    artist: r.get(1)?,
                    year: r.get(2)?,
                    tracks: r.get(3)?,
                    path: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Pistes d'un album, dans l'ordre du disque.
    pub fn tracks_of_album(&self, album: &str, artist: Option<&str>) -> Result<Vec<TrackRow>> {
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks
              WHERE album = ?1
                AND (?2 IS NULL OR album_artist = ?2 OR artist = ?2)
              ORDER BY track_no, title COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![album, artist], track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Recherche sur titre, artiste et album.
    ///
    /// Passe par l'index FTS5, dont le tokenizer replie les diacritiques :
    /// « bjork » trouve « Björk », « kanan » trouve « Kanañ ». La recherche
    /// porte sur des mots entiers, le dernier faisant office de préfixe pour
    /// rester utilisable au fil de la frappe.
    pub fn search(&self, q: &str, limit: i64) -> Result<Vec<TrackRow>> {
        let requete = requete_fts(q);
        if requete.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks
              WHERE id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1)
              ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, track_no
              LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![requete, limit], track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Les morceaux dont l'identifiant est dans `ids`, dans n'importe quel
    /// ordre — à l'appelant de les rassembler par identifiant s'il veut un
    /// ordre précis. Sert à afficher un titre/artiste à partir d'une liste de
    /// voisins (`chemin::voisins` ne rend que des identifiants).
    pub fn tracks_by_ids(&self, ids: &std::collections::HashSet<i64>) -> Result<Vec<TrackRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let places = vec!["?"; ids.len()].join(",");
        let sql = format!("SELECT {TRACK_COLS} FROM tracks WHERE id IN ({places})");
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
        let rows = stmt
            .query_map(params.as_slice(), track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// La popularité générale des morceaux `ids` : `(track_id, relative 0..1,
    /// echelon)`. Seuls ceux qui en ont une figurent — l'interface grise les
    /// autres. Commande séparée, comme les descripteurs : la popularité ne
    /// vit pas dans `TrackRow`, l'interface la demande pour ce qu'elle affiche.
    pub fn popularites(&self, ids: &[i64]) -> Result<Vec<(i64, f64, String)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let places = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "SELECT track_id, relative, echelon FROM track_popularite
              WHERE track_id IN ({places})"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(ids), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Racines surveillées, avec le nombre de morceaux rattachés à chacune.
    /// Sert l'écran de réglages où l'on change la source de la bibliothèque.
    pub fn roots(&self) -> Result<Vec<RootRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, added_at, last_scan FROM roots ORDER BY path")?;
        let brutes = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        brutes
            .into_iter()
            .map(|(path, added_at, last_scan)| {
                Ok(RootRow {
                    tracks: self.count_under(Path::new(&path))?,
                    path,
                    added_at,
                    last_scan,
                })
            })
            .collect()
    }

    /// Retire une racine **et les morceaux qui en dépendent**.
    ///
    /// C'est l'opération « changer de source » des réglages : sans la purge des
    /// morceaux, la base garderait des lignes pointant vers un disque absent.
    /// Renvoie le nombre de morceaux retirés.
    pub fn remove_root(&self, root: &Path) -> Result<usize> {
        let sous_racine = self.paths_under(root)?;
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut del = tx.prepare("DELETE FROM tracks WHERE path = ?1")?;
            for p in &sous_racine {
                del.execute(params![p])?;
            }
        }
        tx.execute(
            "DELETE FROM roots WHERE path = ?1",
            params![root.to_string_lossy()],
        )?;
        tx.commit()?;
        Ok(sous_racine.len())
    }

    /// Chemins de la base situés sous `root`.
    ///
    /// Le rattachement se fait par composants (`Path::starts_with`) et non par
    /// `LIKE` : un motif SQL capterait aussi `/Musique/autre` pour la racine
    /// `/Musique/autr_`, et les chemins contenant `%` ou `_` sont courants.
    fn paths_under(&self, root: &Path) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM tracks")?;
        let tous = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(tous
            .into_iter()
            .filter(|p| Path::new(p).starts_with(root))
            .collect())
    }

    fn count_under(&self, root: &Path) -> Result<i64> {
        Ok(self.paths_under(root)?.len() as i64)
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
    for (rang, (famille, effectif)) in effectifs.iter().enumerate() {
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

        let nom = libeller(&genres, &vus).unwrap_or_else(|| format!("Famille {}", rang + 1));
        vus.insert(empreinte_libelle(&nom));
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
        assert_eq!(albums.len(), 2, "les albums étiquetés ne remontent pas : {albums:?}");
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
        lib.effacer_echec_scan(Path::new("/musique/casse.mp3")).unwrap();
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
        lib.save_descripteurs(sans_tempo, None, None, 0.0, -100.0, None, None, None, None, None, None, None)
            .unwrap();
        lib.save_descripteurs(mesure, Some(122.0), None, 0.5, -12.0, None, None, None, None, None, None, None)
            .unwrap();

        let h = lib.stats_tempo().unwrap();
        assert_eq!(h.sans_valeur, 2);
        assert_eq!(h.comptes.iter().sum::<i64>(), 1);
        let tranche = ((122.0 - TEMPO_MIN) / TEMPO_PAS) as usize;
        assert_eq!(h.comptes[tranche], 1);
    }

    /// Une famille de huit morceaux, quatre genres — Jazz minoritaire, seul
    /// hors du top 3 (`GENRES_DOMINANTS`). C'est lui, et seulement lui, qui
    /// doit ressortir suspect.
    #[test]
    fn genres_suspects_ignore_le_dominant_et_signale_lisole() {
        let lib = Library::open_in_memory().unwrap();
        // Effectifs tous distincts : aucune égalité à départager dans le
        // classement des dominants.
        let genres = [
            ("Rock", 4),
            ("Pop", 3),
            ("Electronic", 2),
            ("Jazz", 1),
        ];
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
        assert_eq!(editions.len(), 1, "une seule paire d'éditions : {editions:?}");
        let (artiste, titre, versions) = &editions[0];
        assert_eq!(artiste, "Radiohead");
        assert_eq!(titre, "Kid A");
        assert_eq!(versions.len(), 2);
    }

    #[test]
    fn titre_album_normalise_tronque_a_la_premiere_parenthese_ou_crochet() {
        assert_eq!(titre_album_normalise("Kid A"), "kid a");
        assert_eq!(titre_album_normalise("Kid A (Remaster)"), "kid a");
        assert_eq!(titre_album_normalise("OK Computer [Deluxe Edition]"), "ok computer");
    }

    /// La médiane s'ajuste à l'échelle réelle des valeurs, même une échelle
    /// resserrée loin de [0, 1] — le cas mesuré qui a écarté un seuil fixe.
    #[test]
    fn stats_humeur_se_calibre_sur_sa_propre_echelle() {
        let lib = Library::open_in_memory().unwrap();
        // Énergie resserrée entre 0,03 et 0,41, comme mesuré en vrai — un
        // seuil fixe à 0,5 rangerait tout le monde dans « lent/léger ».
        let pistes = [
            (60.0, 0.05), // lent, léger  -> Calme
            (60.0, 0.40), // lent, dense  -> Intense
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
            lib.save_descripteurs(id, Some(*bpm as f32), None, *energie as f32, -10.0, None, None, None, None, None, None, None)
                .unwrap();
        }
        let humeur = lib.stats_humeur().unwrap();
        let compte = |nom: &str| humeur.iter().find(|(n, _)| n == nom).map(|(_, c)| *c).unwrap_or(0);
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
        assert_eq!(liens, vec![("b".to_string(), "Artiste B".to_string(), "member of band".to_string())]);

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

        assert!(lib.decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg1", &hier, None)).unwrap());
        lib.decouvrir_marquer_passe("sorties").unwrap();
        lib.decouvrir_tout_vu().unwrap();

        // Une seconde passe revoit la même sortie : rien de neuf, et le « vu »
        // tient — sans quoi la pastille « nouveau » se rallumerait à chaque
        // actualisation.
        assert!(!lib.decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg1", &hier, None)).unwrap());
        assert!(lib.decouvrir_ajouter_sortie("a", "Artiste A", &sortie("rg2", &hier, None)).unwrap());

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
        assert_eq!(fil.sorties.iter().map(|s| &s.rg_mbid).collect::<Vec<_>>(), ["recent"]);
        assert_eq!(fil.collaborations.iter().map(|s| &s.rg_mbid).collect::<Vec<_>>(), ["collab"]);
        assert!(fil.derniere_passe.is_some());
    }

    #[test]
    fn decouvrir_en_attente_respecte_la_peremption_et_lordre() {
        let mut lib = Library::open_in_memory().unwrap();
        // Deux morceaux pour « gros », un seul pour « petit » : le plus fourni
        // d'abord.
        for (i, mbid, nom) in [(1, "gros", "Gros"), (2, "gros", "Gros"), (3, "petit", "Petit")] {
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
        assert_eq!(attente.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), ["petit"]);
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
                ("dedans".into(), "Déjà là".into(), 0.8, "listenbrainz".into()),
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
                PopulariteBrute { mbid: "rec-a", ecoutes: 100, auditeurs: Some(10) },
                PopulariteBrute { mbid: "rec-c", ecoutes: 5, auditeurs: Some(1) },
            ],
        )
        .unwrap();
        lib.pop_poser(
            "listenbrainz",
            "release-group",
            &["rg-x".into(), "rg-y".into()],
            &[
                PopulariteBrute { mbid: "rg-x", ecoutes: 50, auditeurs: Some(5) },
                PopulariteBrute { mbid: "rg-y", ecoutes: 9, auditeurs: Some(2) },
            ],
        )
        .unwrap();
        lib.pop_poser(
            "deezer",
            "recording",
            &["rec-a".into()],
            &[PopulariteBrute { mbid: "rec-a", ecoutes: 900_000, auditeurs: None }],
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
        assert_eq!(vus.iter().map(|(id, ..)| *id).collect::<Vec<_>>(), vec![1, 2]);
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
        lib.pop_poser("listenbrainz", "recording", &["rec-1".into()], &[]).unwrap();
        assert_eq!(lib.pop_recordings_candidats(0, 100).unwrap().len(), 1);

        // Fait sur les deux : plus candidat.
        lib.pop_poser("deezer", "recording", &["rec-1".into()], &[]).unwrap();
        assert!(lib.pop_recordings_candidats(0, 100).unwrap().is_empty());

        // `depuis` très grand : tout fetch compte comme périmé, l'entité revient.
        let futur = i64::MAX;
        assert_eq!(lib.pop_recordings_candidats(futur, 100).unwrap().len(), 1);
        assert!(lib.pop_deja_fait("listenbrainz", "recording", futur).unwrap().is_empty());
        assert_eq!(lib.pop_deja_fait("listenbrainz", "recording", 0).unwrap().len(), 1);
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
        let par_id: HashMap<i64, &ArriveeBrute> =
            ordre.iter().map(|a| (a.track_id, a)).collect();
        assert_eq!(par_id[&1].source, "tag");
        assert_eq!(par_id[&2].source, "album");
        assert_eq!(par_id[&2].date, 19_900_000);
        assert_eq!(par_id[&3].source, "artiste");
        assert_eq!(par_id[&4].source, "ingestion");
        // Aucun morceau n'est perdu en route.
        assert_eq!(ordre.len(), 4);
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
}
