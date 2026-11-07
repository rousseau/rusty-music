// SPDX-License-Identifier: GPL-3.0-or-later
//! Les morceaux et leurs racines : ingestion, navigation, recherche.

use super::*;

impl Library {
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
    ///
    /// Les identifiants MusicBrainz d'une édition retenue à la main
    /// (`edition_piste`, [`Library::retenir_edition`]) l'emportent sur ceux des
    /// tags : un rescan ne défait pas le choix.
    pub fn upsert(&self, m: &TrackMeta) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tracks
               (path, size_bytes, mtime, title, artist, album, album_artist,
                genre, year, track_no, duration_ms, sample_rate, channels, bitrate, codec,
                bit_depth, mb_recording_id, mb_artist_id, mb_album_artist_id, mb_release_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,
               COALESCE((SELECT recording_id FROM edition_piste WHERE path = ?1), ?17),
               COALESCE((SELECT artist_mbid FROM edition_piste WHERE path = ?1), ?18),
               COALESCE((SELECT album_artist_mbid FROM edition_piste WHERE path = ?1), ?19),
               COALESCE((SELECT release_id FROM edition_piste WHERE path = ?1), ?20))
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
               mb_album_artist_id=excluded.mb_album_artist_id,
               mb_release_id=excluded.mb_release_id",
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
                m.mb_album_artist_id,
                m.mb_release_id
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

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?)
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
             SELECT t.album, COALESCE(t.album_artist, t.artist), MIN(t.year), COUNT(*), MIN(t.path),
                    MIN(r.first_release_date), MIN(r.secondary_types), MAX(t.mtime)
               FROM tracks t
               LEFT JOIN mb_release_groups r
                      ON r.artist_mbid = t.mb_album_artist_id
                     AND r.title_norm = titre_normalise(t.album)
              WHERE t.album IS NOT NULL
                AND ( (COALESCE(?1, (SELECT mbid FROM resolu)) IS NOT NULL
                       AND t.mb_album_artist_id = COALESCE(?1, (SELECT mbid FROM resolu)))
                   OR (t.mb_album_artist_id IS NULL
                       AND COALESCE(t.album_artist, t.artist) = ?2) )
              GROUP BY t.album, COALESCE(t.album_artist, t.artist)
              ORDER BY t.album COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map(params![mbid, name], album_row_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Albums, tous ou ceux d'un artiste donné.
    ///
    /// L'année rendue est [`annee_fiable_album`] — MusicBrainz avant le tag —
    /// pas le seul tag : voir sa doc pour l'arbitrage.
    pub fn albums(&self, artist: Option<&str>) -> Result<Vec<AlbumRow>> {
        // `artist` filtre sur l'artiste d'album comme sur celui de la piste :
        // sinon un album entier échappe au filtre dès qu'une piste porte un
        // invité en artiste.
        let mut stmt = self.conn.prepare(
            "SELECT t.album, COALESCE(t.album_artist, t.artist), MIN(t.year), COUNT(*), MIN(t.path),
                    MIN(r.first_release_date), MIN(r.secondary_types), MAX(t.mtime)
               FROM tracks t
               LEFT JOIN mb_release_groups r
                      ON r.artist_mbid = t.mb_album_artist_id
                     AND r.title_norm = titre_normalise(t.album)
              WHERE t.album IS NOT NULL
                AND (?1 IS NULL OR t.album_artist = ?1 OR t.artist = ?1)
              GROUP BY t.album, COALESCE(t.album_artist, t.artist)
              ORDER BY t.album COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map(params![artist], album_row_from_row)?
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

/// Un artiste et son volume dans la bibliothèque.
#[cfg_attr(feature = "specta", derive(specta::Type))]
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
#[cfg_attr(feature = "specta", derive(specta::Type))]
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

/// Une racine surveillée, telle qu'affichée dans les réglages.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct RootRow {
    pub path: String,
    pub added_at: i64,
    pub last_scan: Option<i64>,
    pub tracks: i64,
}
