// SPDX-License-Identifier: GPL-3.0-or-later
//! Le fil du mode Découvrir.

use super::*;

impl Library {
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
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
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
    /// ListenBrainz pour toute la bibliothèque), pour que la dernière passe
    /// Découvrir se voie même quand il n'y a aucun voisin à interroger.
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
}
