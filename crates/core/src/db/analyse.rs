// SPDX-License-Identifier: GPL-3.0-or-later
//! Ce que mesurent les passes d'analyse : empreintes, descripteurs, loudness.

use super::*;

impl Library {
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

    /// Retient le rang de la fenêtre la plus représentative d'une empreinte
    /// déjà enregistrée (`docs/plan-balade.md`, P4).
    pub fn save_fenetre_representative(
        &self,
        track_id: i64,
        model: &str,
        rang: usize,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE features SET fenetre = ?3 WHERE track_id = ?1 AND model = ?2",
            params![track_id, model, rang as i64],
        )?;
        Ok(())
    }

    /// Le rang de la fenêtre la plus représentative, s'il est connu.
    pub fn fenetre_representative(&self, track_id: i64, model: &str) -> Result<Option<usize>> {
        let rang: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT fenetre FROM features WHERE track_id = ?1 AND model = ?2",
                params![track_id, model],
                |r| r.get(0),
            )
            .optional()?;
        Ok(rang.flatten().and_then(|r| usize::try_from(r).ok()))
    }

    /// Les morceaux qui ont une empreinte mais pas encore de fenêtre
    /// représentative : ceux analysés avant qu'on la retienne.
    pub fn pending_fenetres(&self, model: &str, limit: i64) -> Result<Vec<TrackRow>> {
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks
              WHERE EXISTS (
                    SELECT 1 FROM features f
                     WHERE f.track_id = tracks.id AND f.model = ?1 AND f.fenetre IS NULL)
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

    /* --------------------------------------------- descripteurs musicaux */

    /// Les morceaux dont on n'a pas encore mesuré tempo, tonalité et énergie
    /// — ou dont la mesure date d'un algorithme plus ancien que `version`.
    ///
    /// Restreint à ceux qui ont déjà une empreinte : la passe des descripteurs
    /// décode les mêmes fenêtres, et il n'y a pas de sens à mesurer un morceau
    /// que la carte ne montre pas. Exclut aussi les fichiers en échec connu
    /// (`scan_failures`) — même raison que `pending_analysis`.
    ///
    /// `version` fait qu'un correctif de l'algorithme (l'inversion d'octave du
    /// tempo, par exemple) se détecte tout seul : il suffit de faire avancer
    /// `VERSION_DESCRIPTEURS` côté appelant, les lignes plus anciennes
    /// redeviennent « en attente » sans purge explicite.
    pub fn pending_descripteurs(
        &self,
        model: &str,
        version: i32,
        limit: i64,
    ) -> Result<Vec<TrackRow>> {
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks
              WHERE EXISTS (SELECT 1 FROM features f
                             WHERE f.track_id = tracks.id AND f.model = ?1)
                AND NOT EXISTS (SELECT 1 FROM descriptors d
                                 WHERE d.track_id = tracks.id AND d.algo_version >= ?2)
                AND NOT EXISTS (SELECT 1 FROM scan_failures sf WHERE sf.path = tracks.path)
              ORDER BY added_at LIMIT ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![model, version, limit], track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Enregistre les descripteurs d'un morceau.
    ///
    /// `bpm` et `musical_key` peuvent être absents : tout n'a pas de pulsation
    /// ni de tonalité, et une valeur inventée colorerait la carte d'un
    /// mensonge. La ligne est écrite quand même — c'est elle qui dit que le
    /// morceau a été mesuré, et qu'il ne faut pas y revenir.
    ///
    /// `version` est celle de l'algorithme qui a produit la mesure
    /// (`VERSION_DESCRIPTEURS` côté appelant) — voir [`Self::pending_descripteurs`].
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
        version: i32,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO descriptors(
                 track_id, bpm, musical_key, energy, loudness,
                 zcr, centroid_mean, centroid_std,
                 rolloff_mean, rolloff_std, flatness_mean, flatness_std, algo_version)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(track_id) DO UPDATE SET
               bpm=excluded.bpm, musical_key=excluded.musical_key,
               energy=excluded.energy, loudness=excluded.loudness,
               zcr=excluded.zcr,
               centroid_mean=excluded.centroid_mean, centroid_std=excluded.centroid_std,
               rolloff_mean=excluded.rolloff_mean, rolloff_std=excluded.rolloff_std,
               flatness_mean=excluded.flatness_mean, flatness_std=excluded.flatness_std,
               algo_version=excluded.algo_version",
            params![
                track_id,
                bpm,
                musical_key,
                energy,
                loudness,
                zcr,
                centroid_mean,
                centroid_std,
                rolloff_mean,
                rolloff_std,
                flatness_mean,
                flatness_std,
                version
            ],
        )?;
        Ok(())
    }

    /// Combien de morceaux placés sur la carte ont des descripteurs à jour
    /// (`algo_version >= version`) — une mesure faite par un algorithme plus
    /// ancien compte comme non faite, au même titre qu'une mesure absente.
    pub fn compter_descripteurs(&self, model: &str, version: i32) -> Result<(i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM features WHERE model = ?1",
            params![model],
            |r| r.get(0),
        )?;
        let faits: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM descriptors d
              JOIN features f ON f.track_id = d.track_id AND f.model = ?1
             WHERE d.algo_version >= ?2",
            params![model, version],
            |r| r.get(0),
        )?;
        Ok((faits, total))
    }

    /// Efface toutes les mesures de tempo/tonalité/énergie, pour les reprendre
    /// de zéro — sert à la case « refaire ce qui est déjà mesuré » du mode
    /// Bibliothèque, pour une repasse complète voulue à la main (un nouveau
    /// descripteur ajouté à la mesure, par exemple, sans que l'algorithme
    /// tempo/tonalité/énergie lui-même ait changé). Un correctif de cet
    /// algorithme, lui, n'a plus besoin de ce bouton : voir
    /// [`Self::pending_descripteurs`] et sa colonne `algo_version`.
    pub fn effacer_descripteurs(&self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM descriptors", [])?)
    }

    /* --------------------------------------------- loudness (EBU R128) */

    /// Les morceaux dont la loudness (EBU R128 / BS.1770) reste à mesurer —
    /// ou dont la mesure date d'un algorithme plus ancien que `version`.
    ///
    /// Triée par album : `crate::loudness::actualiser` en déduit, une fois
    /// une piste mesurée, les albums que cette passe touche, pour recalculer
    /// leur gain d'album ensuite.
    pub fn pending_loudness(&self, version: i32, limit: i64) -> Result<Vec<PisteLoudness>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, COALESCE(album, ''), COALESCE(album_artist, artist, '')
               FROM tracks
              WHERE NOT EXISTS (SELECT 1 FROM track_loudness tl
                                 WHERE tl.track_id = tracks.id AND tl.algo_version >= ?1)
                AND NOT EXISTS (SELECT 1 FROM scan_failures sf WHERE sf.path = tracks.path)
              ORDER BY 4, 3, track_no
              LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![version, limit], |r| {
                Ok(PisteLoudness {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    album: r.get(2)?,
                    album_artist: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Enregistre la loudness mesurée d'un morceau.
    pub fn save_loudness(
        &self,
        track_id: i64,
        integrated_lufs: f64,
        true_peak_dbtp: f64,
        version: i32,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO track_loudness(track_id, integrated_lufs, true_peak_dbtp, algo_version)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(track_id) DO UPDATE SET
               integrated_lufs=excluded.integrated_lufs,
               true_peak_dbtp=excluded.true_peak_dbtp,
               algo_version=excluded.algo_version,
               mesure_le=strftime('%s','now')",
            params![track_id, integrated_lufs, true_peak_dbtp, version],
        )?;
        Ok(())
    }

    /// Tous les morceaux d'un album (même identité que [`Self::pending_loudness`] :
    /// `album`/`album_artist` normalisés en chaîne vide si absents) — pour
    /// recalculer sa loudness combinée quand l'un de ses morceaux vient
    /// d'être mesuré.
    pub fn pistes_de_lalbum(&self, album: &str, album_artist: &str) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path FROM tracks
              WHERE COALESCE(album, '') = ?1 AND COALESCE(album_artist, artist, '') = ?2",
        )?;
        let rows = stmt
            .query_map(params![album, album_artist], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Enregistre la loudness combinée d'un album — programme EBU R128
    /// combiné, pas une moyenne (voir `crate::loudness::actualiser`).
    pub fn save_album_loudness(
        &self,
        album: &str,
        album_artist: &str,
        integrated_lufs: f64,
        pistes: usize,
        version: i32,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO album_loudness(album, album_artist, integrated_lufs, pistes, algo_version)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(album, album_artist) DO UPDATE SET
               integrated_lufs=excluded.integrated_lufs,
               pistes=excluded.pistes,
               algo_version=excluded.algo_version,
               calcule_le=strftime('%s','now')",
            params![album, album_artist, integrated_lufs, pistes as i64, version],
        )?;
        Ok(())
    }

    /// Combien de morceaux ont une loudness à jour (`algo_version >= version`)
    /// — pour une ligne d'état du rail, comme [`Self::compter_descripteurs`].
    pub fn compter_loudness(&self, version: i32) -> Result<(i64, i64)> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?;
        let faits: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM track_loudness WHERE algo_version >= ?1",
            params![version],
            |r| r.get(0),
        )?;
        Ok((faits, total))
    }

    /// Efface toutes les mesures de loudness (piste et album), pour la case
    /// « refaire ce qui est déjà mesuré » du mode Bibliothèque.
    pub fn effacer_loudness(&self) -> Result<usize> {
        self.conn.execute("DELETE FROM album_loudness", [])?;
        Ok(self.conn.execute("DELETE FROM track_loudness", [])?)
    }

    /// Gain linéaire (1.0 = neutre) à appliquer à la lecture de `chemin`, ou
    /// `None` si sa loudness n'est pas encore mesurée — jamais un blocage,
    /// un morceau non mesuré joue simplement sans normalisation.
    ///
    /// En mode album, replie sur le gain piste si l'album n'a pas (encore)
    /// de ligne `album_loudness` ; le plafond anti-écrêtage utilise toujours
    /// le pic **de la piste**, jamais un pic d'album — la piste réellement
    /// jouée ne doit jamais écrêter, quel que soit le mode choisi.
    pub fn gain_lecture(
        &self,
        chemin: &Path,
        mode: crate::loudness::ModeGain,
        cible: f64,
    ) -> Result<Option<f32>> {
        let chemin_str = chemin.to_string_lossy();
        let trouve = self
            .conn
            .query_row(
                "SELECT tl.true_peak_dbtp, tl.integrated_lufs,
                        COALESCE(tracks.album, ''), COALESCE(tracks.album_artist, tracks.artist, '')
                   FROM tracks JOIN track_loudness tl ON tl.track_id = tracks.id
                  WHERE tracks.path = ?1",
                params![chemin_str],
                |r| {
                    Ok((
                        r.get::<_, f64>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((true_peak_dbtp, piste_lufs, album, album_artist)) = trouve else {
            return Ok(None);
        };
        let lufs = match mode {
            crate::loudness::ModeGain::Piste => piste_lufs,
            crate::loudness::ModeGain::Album if !album.is_empty() => self
                .conn
                .query_row(
                    "SELECT integrated_lufs FROM album_loudness WHERE album = ?1 AND album_artist = ?2",
                    params![album, album_artist],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(piste_lufs),
            crate::loudness::ModeGain::Album => piste_lufs,
        };
        Ok(Some(crate::loudness::db_vers_lineaire(
            crate::loudness::gain_effectif_db(lufs, true_peak_dbtp, cible),
        )))
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
