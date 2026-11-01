// SPDX-License-Identifier: GPL-3.0-or-later
//! L'édition MusicBrainz retenue à la main pour un album
//! (`docs/plan-ecouter-v0.3.md`, point 3) : tables `edition_choisie` et
//! `edition_piste` (migration n° 2). Rien n'est écrit dans les fichiers ;
//! [`Library::upsert`] réapplique le choix à chaque scan.

use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};

use super::Library;
use crate::editions::EditionDetaillee;
use crate::error::Result;

/// L'édition retenue pour un album, telle que l'interface l'affiche.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EditionRetenue {
    pub release_id: String,
    pub titre: String,
    pub choisie_le: i64,
}

impl Library {
    /// Retient l'édition `ed` pour l'album (`album`, `artiste`) : chaque
    /// `(chemin, rang)` d'`appariement` reçoit les identifiants de la piste de
    /// rang `rang` de l'édition (enregistrement, artiste crédité), et l'artiste
    /// de l'édition comme artiste d'album. Remplace un choix précédent. Les
    /// identifiants d'artiste ne remplacent qu'à condition d'être connus.
    pub fn retenir_edition(
        &mut self,
        album: &str,
        artiste: &str,
        ed: &EditionDetaillee,
        appariement: &[(PathBuf, usize)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO edition_choisie (album, artiste, release_id, titre, choisie_le)
             VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))
             ON CONFLICT(album, artiste) DO UPDATE SET
               release_id = excluded.release_id, titre = excluded.titre,
               choisie_le = excluded.choisie_le",
            params![album, artiste, ed.id, ed.titre],
        )?;
        tx.execute(
            "DELETE FROM edition_piste WHERE album = ?1 AND artiste = ?2",
            params![album, artiste],
        )?;
        for (chemin, rang) in appariement {
            let Some(p) = ed.pistes.get(*rang) else {
                continue;
            };
            let chemin = chemin.to_string_lossy();
            tx.execute(
                "INSERT OR REPLACE INTO edition_piste
                   (path, album, artiste, release_id, recording_id, artist_mbid, album_artist_mbid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    chemin,
                    album,
                    artiste,
                    ed.id,
                    p.recording_id,
                    p.artiste_mbid.as_ref().or(ed.artiste_mbid.as_ref()),
                    ed.artiste_mbid
                ],
            )?;
            tx.execute(
                "UPDATE tracks SET
                   mb_release_id = ?2, mb_recording_id = ?3,
                   mb_artist_id = COALESCE(?4, mb_artist_id),
                   mb_album_artist_id = COALESCE(?5, mb_album_artist_id)
                 WHERE path = ?1",
                params![
                    chemin,
                    ed.id,
                    p.recording_id,
                    p.artiste_mbid.as_ref().or(ed.artiste_mbid.as_ref()),
                    ed.artiste_mbid
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// L'édition retenue pour l'album, s'il y en a une.
    pub fn edition_retenue(&self, album: &str, artiste: &str) -> Result<Option<EditionRetenue>> {
        Ok(self
            .conn
            .query_row(
                "SELECT release_id, titre, choisie_le FROM edition_choisie
                  WHERE album = ?1 AND artiste = ?2",
                params![album, artiste],
                |r| {
                    Ok(EditionRetenue {
                        release_id: r.get(0)?,
                        titre: r.get(1)?,
                        choisie_le: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Oublie le choix d'édition de l'album et rend les chemins concernés —
    /// à l'appelant d'en relire les tags (`tags::read` puis
    /// [`Library::upsert`]) pour retrouver leurs identifiants d'origine.
    pub fn oublier_edition(&mut self, album: &str, artiste: &str) -> Result<Vec<PathBuf>> {
        let tx = self.conn.transaction()?;
        let chemins = {
            let mut stmt =
                tx.prepare("SELECT path FROM edition_piste WHERE album = ?1 AND artiste = ?2")?;
            let lignes = stmt.query_map(params![album, artiste], |r| r.get::<_, String>(0))?;
            lignes
                .map(|c| c.map(|c| Path::new(&c).to_path_buf()))
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        tx.execute(
            "DELETE FROM edition_piste WHERE album = ?1 AND artiste = ?2",
            params![album, artiste],
        )?;
        tx.execute(
            "DELETE FROM edition_choisie WHERE album = ?1 AND artiste = ?2",
            params![album, artiste],
        )?;
        tx.commit()?;
        Ok(chemins)
    }
}
