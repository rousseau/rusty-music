// SPDX-License-Identifier: GPL-3.0-or-later
//! Valise : une sélection d'albums exportée pour le compagnon iOS.
//!
//! Le téléphone n'emporte pas la bibliothèque, il emporte une valise — un
//! dossier autonome que le Mac prépare et que l'iPhone lit sans lui :
//!
//! ```text
//! <sortie>/
//!   valise.json          manifeste (format, date, règles, albums)
//!   base.sqlite          base réduite : même schéma que le bureau
//!   pochettes/<id>.jpg   une par album, 600 px au plus
//!   audio/<id>/NN - Titre.ext
//! ```
//!
//! La base réduite garde le schéma du bureau (`Library::open`) pour que le
//! code de lecture soit le même des deux côtés ; `tracks.path` y reste le
//! chemin **d'origine** (l'identité d'un morceau), et `valise_fichier` dit où
//! le fichier se trouve dans la valise. Le journal `ecoute` part vide : c'est
//! celui du téléphone, que le Mac réimportera.
//!
//! L'audio est copié tel quel : la bibliothèque est presque toute en MP3, un
//! réencodage dégraderait sans rien gagner. Voir `docs/plan-ios-compagnon.md`.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::db::{hacher, Library};
use crate::error::{Error, Result};

/// Version du format de valise, écrite dans le manifeste et dans la base.
pub const VERSION_FORMAT: u32 = 1;

/// Côté le plus long d'une pochette exportée, en pixels.
pub const POCHETTE_MAX: u32 = 600;

/// Tables propres à la valise, ajoutées au schéma du bureau.
const SCHEMA_VALISE: &str = "
CREATE TABLE IF NOT EXISTS valise_info (
    cle    TEXT PRIMARY KEY,
    valeur TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS valise_album (
    id       TEXT    PRIMARY KEY,   -- hacher(artiste, album) en hexadécimal
    album    TEXT    NOT NULL,
    artiste  TEXT    NOT NULL,
    regle    TEXT    NOT NULL,      -- règle qui l'a choisi (Regle::code)
    rang     INTEGER NOT NULL,      -- ordre dans la valise
    pochette TEXT                   -- chemin relatif, NULL si aucune
);
CREATE TABLE IF NOT EXISTS valise_fichier (
    track_id INTEGER PRIMARY KEY,
    album_id TEXT    NOT NULL,
    fichier  TEXT    NOT NULL       -- chemin relatif à la racine de la valise
);
";

/// Ce qui fait entrer un album dans la valise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Regle {
    /// Les derniers arrivés (date de modification la plus récente des fichiers,
    /// comme `AlbumRow::ajoute_le`).
    Recents,
    /// Jamais ou peu écoutés, puis écoutés il y a longtemps ; à égalité, un
    /// tirage qui change d'une valise à l'autre.
    PeuEcoutes,
}

impl Regle {
    pub fn depuis_texte(s: &str) -> Option<Self> {
        match s.trim() {
            "recents" | "récents" => Some(Self::Recents),
            "peu-ecoutes" | "peu-écoutés" => Some(Self::PeuEcoutes),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Recents => "recents",
            Self::PeuEcoutes => "peu-ecoutes",
        }
    }

    pub fn libelle(self) -> &'static str {
        match self {
            Self::Recents => "Récents",
            Self::PeuEcoutes => "Peu écoutés",
        }
    }
}

/// Réglages d'une sélection.
#[derive(Debug, Clone)]
pub struct Reglages {
    /// Règles, chacune avec une part égale du budget ; ce qu'une règle ne
    /// consomme pas passe à la suivante.
    pub regles: Vec<Regle>,
    /// Taille totale de l'audio, en octets.
    pub budget_octets: u64,
    /// Albums de moins de pistes écartés (pistes isolées, extraits).
    pub pistes_min: usize,
    /// Albums d'un même artiste, au plus, dans toute la valise.
    pub par_artiste: usize,
    /// Graine du tirage de « peu écoutés » : deux valises préparées à des
    /// moments différents ne proposent pas les mêmes albums.
    pub graine: u64,
}

impl Default for Reglages {
    fn default() -> Self {
        Self {
            regles: vec![Regle::Recents, Regle::PeuEcoutes],
            budget_octets: 4 * 1024 * 1024 * 1024,
            pistes_min: 3,
            par_artiste: 2,
            graine: 0,
        }
    }
}

/// Un album retenu.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlbumValise {
    pub id: String,
    pub album: String,
    pub artiste: String,
    pub annee: Option<i64>,
    pub regle: Regle,
    pub nb_pistes: usize,
    pub octets: u64,
    pub duree_ms: i64,
    pub ajoute_le: Option<i64>,
    pub ecoutes: i64,
    /// Identifiants des pistes, dans l'ordre du disque.
    #[serde(skip)]
    pub pistes: Vec<i64>,
}

/// Une sélection prête à exporter.
#[derive(Debug, Clone)]
pub struct Selection {
    pub reglages: Reglages,
    pub albums: Vec<AlbumValise>,
}

impl Selection {
    pub fn octets(&self) -> u64 {
        self.albums.iter().map(|a| a.octets).sum()
    }
}

/// Un album candidat, tel que la base le décrit.
#[derive(Debug, Clone)]
struct Candidat {
    album: String,
    artiste: String,
    annee: Option<i64>,
    nb_pistes: usize,
    octets: u64,
    duree_ms: i64,
    ajoute_le: Option<i64>,
    ecoutes: i64,
    derniere_ecoute: Option<i64>,
    favori: bool,
    hache: u64,
}

/// Mélange d'un entier sur 64 bits (splitmix64) : un tirage reproductible à
/// graine égale, sans dépendance.
fn melanger(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Lit une taille humaine : « 4G », « 4 Go », « 800M », « 1.5G », ou un
/// nombre d'octets.
pub fn lire_taille(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase().replace(',', ".");
    let (nombre, unite) = match s.find(|c: char| c.is_alphabetic()) {
        Some(i) => (s[..i].trim(), s[i..].trim()),
        None => (s.as_str(), ""),
    };
    let facteur: f64 = match unite {
        "" | "o" | "b" => 1.0,
        "k" | "ko" | "kb" => 1024.0,
        "m" | "mo" | "mb" => 1024.0 * 1024.0,
        "g" | "go" | "gb" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let n: f64 = nombre.parse().ok()?;
    (n >= 0.0).then_some((n * facteur) as u64)
}

impl Library {
    /// Les albums candidats d'une valise : au moins `pistes_min` pistes,
    /// avec leurs écoutes et leur taille.
    fn candidats_valise(&self, pistes_min: usize) -> Result<Vec<Candidat>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.album, COALESCE(t.album_artist, t.artist) AS aa, COUNT(*),
                    SUM(COALESCE(t.size_bytes, 0)), SUM(COALESCE(t.duration_ms, 0)),
                    MAX(t.mtime), MIN(t.year),
                    SUM(COALESCE(e.n, 0)), MAX(e.dernier), MAX(f.chemin IS NOT NULL)
               FROM tracks t
               LEFT JOIN (SELECT chemin, COUNT(*) AS n, MAX(le) AS dernier
                            FROM ecoute GROUP BY chemin) e ON e.chemin = t.path
               LEFT JOIN favori f ON f.chemin = t.path
              WHERE t.album IS NOT NULL AND aa IS NOT NULL
              GROUP BY t.album, aa
             HAVING COUNT(*) >= ?1",
        )?;
        let candidats = stmt
            .query_map(params![pistes_min as i64], |r| {
                let album: String = r.get(0)?;
                let artiste: String = r.get(1)?;
                Ok(Candidat {
                    hache: hacher(&artiste, &album),
                    album,
                    artiste,
                    nb_pistes: r.get::<_, i64>(2)? as usize,
                    octets: r.get::<_, i64>(3)?.max(0) as u64,
                    duree_ms: r.get(4)?,
                    ajoute_le: r.get::<_, Option<i64>>(5)?.filter(|&t| t > 0),
                    annee: r.get(6)?,
                    ecoutes: r.get(7)?,
                    derniere_ecoute: r.get(8)?,
                    favori: r.get::<_, i64>(9)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(candidats)
    }

    /// Les pistes d'un album de valise, dans l'ordre du disque — même
    /// regroupement que `candidats_valise` (artiste d'album, sinon de piste).
    fn pistes_album_valise(&self, album: &str, artiste: &str) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM tracks
              WHERE album = ?1 AND COALESCE(album_artist, artist) = ?2
              ORDER BY track_no, title COLLATE NOCASE, path",
        )?;
        let ids = stmt
            .query_map(params![album, artiste], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Choisit les albums d'une valise selon `reglages`.
    pub fn selectionner_valise(&self, reglages: &Reglages) -> Result<Selection> {
        let candidats = self.candidats_valise(reglages.pistes_min.max(1))?;
        let mut retenus: Vec<AlbumValise> = Vec::new();
        let mut pris: HashSet<u64> = HashSet::new();
        let mut par_artiste: HashMap<String, usize> = HashMap::new();
        let mut reste_total = reglages.budget_octets;

        for (i, &regle) in reglages.regles.iter().enumerate() {
            // Part égale de ce qui reste entre les règles restantes : une
            // règle qui ne remplit pas sa part la laisse aux suivantes.
            let restantes = (reglages.regles.len() - i) as u64;
            let mut part = reste_total / restantes;

            let mut ordre: Vec<&Candidat> = candidats.iter().collect();
            match regle {
                Regle::Recents => ordre.sort_by(|a, b| {
                    b.ajoute_le
                        .unwrap_or(i64::MIN)
                        .cmp(&a.ajoute_le.unwrap_or(i64::MIN))
                        .then(a.hache.cmp(&b.hache))
                }),
                Regle::PeuEcoutes => ordre.sort_by_key(|c| {
                    (
                        c.ecoutes,
                        c.derniere_ecoute.unwrap_or(i64::MIN),
                        c.favori,
                        melanger(c.hache ^ reglages.graine),
                    )
                }),
            }

            for c in ordre {
                if pris.contains(&c.hache) || c.octets == 0 || c.octets > part {
                    continue;
                }
                let n = par_artiste.entry(c.artiste.to_lowercase()).or_default();
                if *n >= reglages.par_artiste.max(1) {
                    continue;
                }
                *n += 1;
                pris.insert(c.hache);
                part -= c.octets;
                reste_total -= c.octets;
                retenus.push(AlbumValise {
                    id: format!("{:016x}", c.hache),
                    album: c.album.clone(),
                    artiste: c.artiste.clone(),
                    annee: c.annee,
                    regle,
                    nb_pistes: c.nb_pistes,
                    octets: c.octets,
                    duree_ms: c.duree_ms,
                    ajoute_le: c.ajoute_le,
                    ecoutes: c.ecoutes,
                    pistes: self.pistes_album_valise(&c.album, &c.artiste)?,
                });
            }
        }

        Ok(Selection {
            reglages: reglages.clone(),
            albums: retenus,
        })
    }
}

/// Ce qu'un export a produit.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Rapport {
    pub albums: usize,
    pub pistes: usize,
    pub octets: u64,
    pub sans_pochette: usize,
    /// Fichiers introuvables ou illisibles au moment de la copie, ignorés.
    pub manquants: Vec<String>,
}

/// Le manifeste `valise.json`.
#[derive(Debug, serde::Serialize)]
struct Manifeste<'a> {
    format: u32,
    cree_le: i64,
    regles: Vec<&'static str>,
    budget_octets: u64,
    octets: u64,
    albums: &'a [AlbumValise],
}

/// Tables du bureau recopiées dans la base réduite, avec le filtre qui ne
/// garde que ce qui concerne la valise. `s` désigne la table source ;
/// `temp.valise_sel` (pistes) et `temp.valise_artistes` (MBID d'artistes)
/// sont posées par [`ecrire_base`]. L'ordre compte : un filtre peut lire une
/// table de la valise déjà remplie.
const TABLES_COPIEES: &[(&str, &str)] = &[
    ("tracks", "s.id IN (SELECT track_id FROM temp.valise_sel)"),
    (
        "track_loudness",
        "s.track_id IN (SELECT track_id FROM temp.valise_sel)",
    ),
    (
        "track_popularite",
        "s.track_id IN (SELECT track_id FROM temp.valise_sel)",
    ),
    (
        "descriptors",
        "s.track_id IN (SELECT track_id FROM temp.valise_sel)",
    ),
    (
        "album_loudness",
        "EXISTS (SELECT 1 FROM valise.tracks t WHERE t.album = s.album
                    AND COALESCE(t.album_artist, t.artist) = s.album_artist)",
    ),
    ("favori", "s.chemin IN (SELECT path FROM valise.tracks)"),
    // Les seuls albums de la valise, pas toute la discographie des artistes :
    // « Various Artists » en a des milliers. Même clé que `mb_albums`.
    (
        "mb_release_groups",
        "s.artist_mbid IN (SELECT mbid FROM temp.valise_artistes)
         AND s.title_norm IN (SELECT titre_normalise(album) FROM valise.tracks)",
    ),
    (
        "theaudiodb_artistes",
        "s.mb_artist_id IN (SELECT mbid FROM temp.valise_artistes)",
    ),
    (
        "mb_genres",
        "s.mbid IN (SELECT mbid FROM temp.valise_artistes)
         OR s.mbid IN (SELECT mbid FROM valise.mb_release_groups)",
    ),
    (
        "artist_links",
        "s.src_mbid IN (SELECT mbid FROM temp.valise_artistes)",
    ),
    (
        "critiques",
        "s.mbid_release_group IN (SELECT mbid FROM valise.mb_release_groups)",
    ),
    (
        "editions_discogs",
        "s.mb_release_id IN (SELECT mb_release_id FROM valise.tracks)",
    ),
    (
        "credits_discogs",
        "s.discogs_release_id IN (SELECT discogs_release_id FROM valise.editions_discogs)",
    ),
    (
        "labels_discogs",
        "s.discogs_release_id IN (SELECT discogs_release_id FROM valise.editions_discogs)",
    ),
    (
        "popularite",
        "s.mbid IN (SELECT mb_recording_id FROM valise.tracks)
         OR s.mbid IN (SELECT mbid FROM valise.mb_release_groups)",
    ),
];

/// Colonnes d'une table dans un schéma (`main` ou `valise`).
fn colonnes(conn: &Connection, schema: &str, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA {schema}.table_info({table})"))?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(cols)
}

/// Écrit la base réduite `chemin` à partir de la bibliothèque.
fn ecrire_base(
    lib: &Library,
    selection: &Selection,
    fichiers: &[(i64, String, String)],
    pochettes: &HashMap<String, String>,
    chemin: &Path,
) -> Result<()> {
    // Le schéma du bureau, migrations comprises, puis les tables de valise.
    {
        let v = Library::open(chemin)?;
        v.conn.execute_batch(SCHEMA_VALISE)?;
    }

    let conn = &lib.conn;
    conn.execute(
        "ATTACH DATABASE ?1 AS valise",
        params![chemin.to_string_lossy()],
    )?;
    let resultat = (|| -> Result<()> {
        conn.execute_batch(
            "DROP TABLE IF EXISTS temp.valise_sel;
             DROP TABLE IF EXISTS temp.valise_artistes;
             CREATE TEMP TABLE valise_sel (track_id INTEGER PRIMARY KEY);
             CREATE TEMP TABLE valise_artistes (mbid TEXT PRIMARY KEY);",
        )?;
        {
            let mut ins = conn.prepare("INSERT OR IGNORE INTO temp.valise_sel VALUES (?1)")?;
            for a in &selection.albums {
                for id in &a.pistes {
                    ins.execute(params![id])?;
                }
            }
        }
        // `mb_artist_id` peut porter plusieurs MBID séparés par « / » ou « ; ».
        conn.execute_batch(
            "WITH RECURSIVE morceaux(reste, mbid) AS (
                 SELECT replace(COALESCE(mb_artist_id, ''), ';', '/') || '/', NULL
                   FROM main.tracks WHERE id IN (SELECT track_id FROM temp.valise_sel)
                 UNION ALL
                 SELECT substr(reste, instr(reste, '/') + 1),
                        trim(substr(reste, 1, instr(reste, '/') - 1))
                   FROM morceaux WHERE reste <> ''
             )
             INSERT OR IGNORE INTO temp.valise_artistes
             SELECT mbid FROM morceaux WHERE mbid IS NOT NULL AND mbid <> ''
             UNION
             SELECT mb_album_artist_id FROM main.tracks
              WHERE id IN (SELECT track_id FROM temp.valise_sel)
                AND mb_album_artist_id IS NOT NULL;",
        )?;

        conn.execute_batch("BEGIN")?;
        for (table, filtre) in TABLES_COPIEES {
            // L'ordre des colonnes diffère entre une base migrée et une base
            // neuve : on ne copie que les colonnes communes, nommées.
            let source = colonnes(conn, "main", table)?;
            if source.is_empty() {
                continue;
            }
            let cible: HashSet<String> = colonnes(conn, "valise", table)?.into_iter().collect();
            let cols: Vec<&str> = source
                .iter()
                .filter(|c| cible.contains(*c))
                .map(String::as_str)
                .collect();
            let liste = cols.join(", ");
            let liste_s = cols
                .iter()
                .map(|c| format!("s.{c}"))
                .collect::<Vec<_>>()
                .join(", ");
            conn.execute(
                &format!(
                    "INSERT OR IGNORE INTO valise.{table} ({liste})
                     SELECT {liste_s} FROM main.{table} AS s WHERE {filtre}"
                ),
                [],
            )?;
        }

        let maintenant = maintenant();
        for (cle, valeur) in [
            ("format", VERSION_FORMAT.to_string()),
            ("cree_le", maintenant.to_string()),
            (
                "regles",
                selection
                    .reglages
                    .regles
                    .iter()
                    .map(|r| r.code())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ] {
            conn.execute(
                "INSERT OR REPLACE INTO valise.valise_info (cle, valeur) VALUES (?1, ?2)",
                params![cle, valeur],
            )?;
        }
        for (rang, a) in selection.albums.iter().enumerate() {
            conn.execute(
                "INSERT INTO valise.valise_album (id, album, artiste, regle, rang, pochette)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    a.id,
                    a.album,
                    a.artiste,
                    a.regle.code(),
                    rang as i64,
                    pochettes.get(&a.id)
                ],
            )?;
        }
        for (track_id, album_id, fichier) in fichiers {
            conn.execute(
                "INSERT INTO valise.valise_fichier (track_id, album_id, fichier) VALUES (?1, ?2, ?3)",
                params![track_id, album_id, fichier],
            )?;
        }
        // Une piste dont le fichier n'a pas pu être copié n'a rien à faire
        // dans la valise.
        conn.execute(
            "DELETE FROM valise.tracks WHERE id NOT IN (SELECT track_id FROM valise.valise_fichier)",
            [],
        )?;
        conn.execute_batch("COMMIT")?;
        Ok(())
    })();
    if resultat.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    }
    let _ = conn.execute_batch(
        "DROP TABLE IF EXISTS temp.valise_sel; DROP TABLE IF EXISTS temp.valise_artistes;",
    );
    conn.execute("DETACH DATABASE valise", [])?;
    resultat?;

    Connection::open(chemin)?.execute_batch("VACUUM")?;
    Ok(())
}

fn maintenant() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// La pochette d'un album, réduite à [`POCHETTE_MAX`] et écrite en JPEG.
/// Rend l'extension écrite, `None` si l'album n'a pas de pochette lisible.
fn ecrire_pochette(piste: &Path, sans_ext: &Path) -> Option<PathBuf> {
    let cover = crate::tags::read_cover(piste).ok()??;
    let image = image::load_from_memory(&cover.data).ok()?;
    let image = if image.width() > POCHETTE_MAX || image.height() > POCHETTE_MAX {
        image.thumbnail(POCHETTE_MAX, POCHETTE_MAX)
    } else {
        image
    };
    let mut octets = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image.to_rgb8())
        .write_to(&mut octets, image::ImageFormat::Jpeg)
        .ok()?;
    let chemin = sans_ext.with_extension("jpg");
    std::fs::write(&chemin, octets.into_inner()).ok()?;
    Some(chemin)
}

/// Nom de fichier d'une piste dans la valise : « 03 - Titre.mp3 ».
fn nom_piste(rang: usize, titre: Option<&str>, source: &Path) -> String {
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let titre = titre
        .map(crate::playlists::nom_de_fichier)
        .unwrap_or_else(|| "piste".to_string());
    if ext.is_empty() {
        format!("{:02} - {titre}", rang + 1)
    } else {
        format!("{:02} - {titre}.{ext}", rang + 1)
    }
}

/// Exporte `selection` dans le dossier `sortie`, qui doit être absent ou vide.
///
/// `progres(i, n, album)` est appelé avant la copie de chaque album.
pub fn exporter(
    lib: &Library,
    selection: &Selection,
    sortie: &Path,
    mut progres: impl FnMut(usize, usize, &AlbumValise),
) -> Result<Rapport> {
    if sortie.exists() && std::fs::read_dir(sortie)?.next().is_some() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("le dossier {} n'est pas vide", sortie.display()),
        )));
    }
    std::fs::create_dir_all(sortie.join("audio"))?;
    std::fs::create_dir_all(sortie.join("pochettes"))?;

    let mut rapport = Rapport::default();
    let mut fichiers: Vec<(i64, String, String)> = Vec::new();
    let mut pochettes: HashMap<String, String> = HashMap::new();
    let mut lecture = lib
        .conn
        .prepare("SELECT path, title FROM tracks WHERE id = ?1")?;

    for (i, album) in selection.albums.iter().enumerate() {
        progres(i, selection.albums.len(), album);
        let dossier_rel = format!("audio/{}", album.id);
        std::fs::create_dir_all(sortie.join(&dossier_rel))?;

        let mut premiere: Option<PathBuf> = None;
        for (rang, &id) in album.pistes.iter().enumerate() {
            let (path, titre): (String, Option<String>) =
                lecture.query_row(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let source = PathBuf::from(&path);
            let rel = format!(
                "{dossier_rel}/{}",
                nom_piste(rang, titre.as_deref(), &source)
            );
            match std::fs::copy(&source, sortie.join(&rel)) {
                Ok(n) => {
                    rapport.pistes += 1;
                    rapport.octets += n;
                    fichiers.push((id, album.id.clone(), rel));
                    premiere.get_or_insert(source);
                }
                Err(_) => rapport.manquants.push(path),
            }
        }

        let pochette = premiere
            .as_deref()
            .and_then(|p| ecrire_pochette(p, &sortie.join("pochettes").join(&album.id)));
        match pochette {
            Some(_) => {
                pochettes.insert(album.id.clone(), format!("pochettes/{}.jpg", album.id));
            }
            None => rapport.sans_pochette += 1,
        }
        rapport.albums += 1;
    }
    drop(lecture);

    ecrire_base(
        lib,
        selection,
        &fichiers,
        &pochettes,
        &sortie.join("base.sqlite"),
    )?;

    let manifeste = Manifeste {
        format: VERSION_FORMAT,
        cree_le: maintenant(),
        regles: selection.reglages.regles.iter().map(|r| r.code()).collect(),
        budget_octets: selection.reglages.budget_octets,
        octets: rapport.octets,
        albums: &selection.albums,
    };
    let json = serde_json::to_string_pretty(&manifeste)
        .map_err(|e| Error::Parsing(format!("manifeste : {e}")))?;
    std::fs::write(sortie.join("valise.json"), json)?;

    Ok(rapport)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::TrackMeta;

    #[test]
    fn tailles() {
        assert_eq!(lire_taille("4G"), Some(4 * 1024 * 1024 * 1024));
        assert_eq!(lire_taille("4 Go"), Some(4 * 1024 * 1024 * 1024));
        assert_eq!(lire_taille("1,5g"), Some(1024 * 1024 * 1024 * 3 / 2));
        assert_eq!(lire_taille("800M"), Some(800 * 1024 * 1024));
        assert_eq!(lire_taille("12345"), Some(12345));
        assert_eq!(lire_taille("3 To"), None);
        assert_eq!(lire_taille("beaucoup"), None);
    }

    /// Une bibliothèque de `n_albums` albums de 3 pistes, chacune de
    /// `octets` octets, aux fichiers réels dans `racine` ; l'album `k`
    /// arrive au temps `1000 + k` (le dernier est le plus récent).
    fn bibliotheque(racine: &Path, n_albums: usize, octets: usize) -> Library {
        let lib = Library::open_in_memory().unwrap();
        for k in 0..n_albums {
            let dossier = racine.join(format!("src/A{k}"));
            std::fs::create_dir_all(&dossier).unwrap();
            for p in 0..3 {
                let path = dossier.join(format!("{p}.mp3"));
                std::fs::write(&path, vec![0u8; octets]).unwrap();
                lib.upsert(&TrackMeta {
                    path: path.clone(),
                    title: Some(format!("Titre {p}")),
                    artist: Some(format!("Artiste {}", k % 3)),
                    album: Some(format!("Album {k}")),
                    track_no: Some(3 - p as i64),
                    size_bytes: octets as i64,
                    mtime: 1000 + k as i64,
                    ..Default::default()
                })
                .unwrap();
            }
        }
        lib
    }

    fn racine_test(nom: &str) -> PathBuf {
        let r =
            std::env::temp_dir().join(format!("rusty-music-valise-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        std::fs::create_dir_all(&r).unwrap();
        r
    }

    #[test]
    fn recents_dabord_sous_le_budget_et_deux_par_artiste() {
        let racine = racine_test("recents");
        let lib = bibliotheque(&racine, 9, 100);
        let sel = lib
            .selectionner_valise(&Reglages {
                regles: vec![Regle::Recents],
                budget_octets: 300 * 5,
                pistes_min: 3,
                par_artiste: 1,
                graine: 0,
            })
            .unwrap();
        let noms: Vec<&str> = sel.albums.iter().map(|a| a.album.as_str()).collect();
        // Un album par artiste (trois artistes), les plus récents d'abord.
        assert_eq!(noms, ["Album 8", "Album 7", "Album 6"]);
        assert!(sel.octets() <= 300 * 5);
        // Ordre du disque : numéro de piste, pas ordre d'insertion.
        let premier = &sel.albums[0];
        let titres: Vec<String> = premier
            .pistes
            .iter()
            .map(|id| {
                lib.conn
                    .query_row("SELECT title FROM tracks WHERE id = ?1", [id], |r| r.get(0))
                    .unwrap()
            })
            .collect();
        assert_eq!(titres, ["Titre 2", "Titre 1", "Titre 0"]);
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn peu_ecoutes_ecarte_les_albums_ecoutes() {
        let racine = racine_test("peu");
        let lib = bibliotheque(&racine, 4, 100);
        // Les albums 0 à 2 ont été écoutés ; seul le 3 ne l'a jamais été.
        for k in 0..3 {
            let chemin = racine.join(format!("src/A{k}/0.mp3"));
            lib.noter_ecoute(&chemin.to_string_lossy()).unwrap();
        }
        let sel = lib
            .selectionner_valise(&Reglages {
                regles: vec![Regle::PeuEcoutes],
                budget_octets: 300,
                pistes_min: 3,
                par_artiste: 2,
                graine: 42,
            })
            .unwrap();
        assert_eq!(sel.albums.len(), 1);
        assert_eq!(sel.albums[0].album, "Album 3");
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn export_complet_relisible() {
        let racine = racine_test("export");
        let lib = bibliotheque(&racine, 3, 64);
        // Un fichier disparu entre le scan et l'export : ignoré, signalé.
        std::fs::remove_file(racine.join("src/A2/1.mp3")).unwrap();
        lib.basculer_favori(&racine.join("src/A2/0.mp3").to_string_lossy())
            .unwrap();
        lib.noter_ecoute(&racine.join("src/A0/0.mp3").to_string_lossy())
            .unwrap();
        let sel = lib
            .selectionner_valise(&Reglages {
                regles: vec![Regle::Recents],
                budget_octets: 10_000,
                pistes_min: 3,
                par_artiste: 2,
                graine: 0,
            })
            .unwrap();
        assert_eq!(sel.albums.len(), 3);

        let sortie = racine.join("valise");
        let mut vus = 0;
        let rapport = exporter(&lib, &sel, &sortie, |_, _, _| vus += 1).unwrap();
        assert_eq!(vus, 3);
        assert_eq!(rapport.albums, 3);
        assert_eq!(rapport.pistes, 8);
        assert_eq!(rapport.manquants.len(), 1);
        assert_eq!(rapport.sans_pochette, 3);
        assert!(sortie.join("valise.json").is_file());

        // La base réduite s'ouvre avec le code du bureau.
        let v = Library::open(&sortie.join("base.sqlite")).unwrap();
        let n: i64 = v
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 8);
        // Le journal d'écoutes part vide ; les favoris voyagent.
        let ecoutes: i64 = v
            .conn
            .query_row("SELECT COUNT(*) FROM ecoute", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ecoutes, 0);
        assert_eq!(v.chemins_favoris().unwrap().len(), 1);
        // Chaque piste pointe vers un fichier présent dans la valise.
        let mut stmt = v
            .conn
            .prepare(
                "SELECT t.path, f.fichier FROM tracks t JOIN valise_fichier f ON f.track_id = t.id",
            )
            .unwrap();
        let paires: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(paires.len(), 8);
        for (origine, fichier) in &paires {
            assert!(origine.starts_with(&*racine.to_string_lossy()));
            assert!(sortie.join(fichier).is_file(), "{fichier}");
        }
        // La recherche plein texte de la valise est alimentée.
        assert_eq!(v.search("Titre", 20).unwrap().len(), 8);

        // Un second export dans le même dossier est refusé.
        assert!(exporter(&lib, &sel, &sortie, |_, _, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&racine);
    }
}
