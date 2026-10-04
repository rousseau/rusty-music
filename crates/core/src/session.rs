// SPDX-License-Identifier: GPL-3.0-or-later
//! Reprise de session : la file et la position du lecteur, d'un lancement à
//! l'autre.
//!
//! Une seule ligne dans la table `session` (voir `sql/schema.sql`). La file
//! y est une liste d'**identifiants** de pistes. L'identité d'un morceau est
//! son chemin : un fichier retiré **ou déplacé** perd son identifiant (la
//! base n'a pas de détection de déplacement) et la reprise le saute.
//!
//! Le lecteur (`crates/player`) ignore la base : c'est l'application qui
//! traduit chemins ↔ identifiants, avec [`Library::ids_par_chemins`] et
//! [`Library::chemins_par_ids`].

use crate::db::{track_from_row, Library, TrackRow, TRACK_COLS};
use crate::error::Result;

/// Ce que le lecteur jouait, assez pour le reprendre.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// File dans l'ordre courant (mélangée si l'aléatoire est actif).
    pub file: Vec<i64>,
    /// Ordre d'avant l'aléatoire — vide si l'aléatoire n'a pas servi.
    pub avant_melange: Vec<i64>,
    /// Rang de la piste en cours dans `file`.
    pub rang: usize,
    pub position_ms: u64,
    pub alea: bool,
    /// « aucune », « toutes » ou « une ».
    pub repetition: String,
    /// Volume linéaire du lecteur (1.0 = niveau d'origine).
    pub volume: f32,
}

fn vers_texte(ids: &[i64]) -> String {
    ids.iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Relit une liste écrite par [`vers_texte`]. Une valeur illisible est
/// écartée plutôt que de faire échouer toute la reprise.
fn depuis_texte(texte: &str) -> Vec<i64> {
    texte
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

/// Paquets de requêtes : SQLite borne le nombre de paramètres d'une requête,
/// et une file peut compter des dizaines de milliers de pistes.
const PAQUET: usize = 500;

impl Library {
    /// Écrit la session entière (file comprise), en remplaçant la précédente.
    pub fn enregistrer_session(&self, s: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO session
                 (id, file, avant_melange, rang, position_ms, alea, repetition, volume, enregistree_le)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%s','now'))",
            rusqlite::params![
                vers_texte(&s.file),
                vers_texte(&s.avant_melange),
                s.rang as i64,
                s.position_ms as i64,
                s.alea,
                s.repetition,
                f64::from(s.volume),
            ],
        )?;
        Ok(())
    }

    /// Met à jour ce qui bouge sans cesse (rang, position, réglages) sans
    /// réécrire la file. Rend `false` quand aucune session n'existe encore :
    /// l'appelant doit alors faire un [`Self::enregistrer_session`] complet.
    pub fn enregistrer_position(
        &self,
        rang: usize,
        position_ms: u64,
        alea: bool,
        repetition: &str,
        volume: f32,
    ) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE session
                SET rang = ?1, position_ms = ?2, alea = ?3, repetition = ?4,
                    volume = ?5, enregistree_le = strftime('%s','now')
              WHERE id = 1",
            rusqlite::params![
                rang as i64,
                position_ms as i64,
                alea,
                repetition,
                f64::from(volume)
            ],
        )?;
        Ok(n > 0)
    }

    /// La dernière session enregistrée, s'il y en a une.
    pub fn session(&self) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT file, avant_melange, rang, position_ms, alea, repetition, volume
               FROM session WHERE id = 1",
        )?;
        let mut lignes = stmt.query_map([], |r| {
            Ok(Session {
                file: depuis_texte(&r.get::<_, String>(0)?),
                avant_melange: depuis_texte(&r.get::<_, String>(1)?),
                rang: r.get::<_, i64>(2)?.max(0) as usize,
                position_ms: r.get::<_, i64>(3)?.max(0) as u64,
                alea: r.get(4)?,
                repetition: r.get(5)?,
                volume: r.get::<_, f64>(6)? as f32,
            })
        })?;
        Ok(lignes.next().transpose()?)
    }

    /// Identifiant de chaque chemin, `None` pour ceux que la bibliothèque ne
    /// connaît pas. Même longueur et même ordre que `chemins`.
    pub fn ids_par_chemins(&self, chemins: &[String]) -> Result<Vec<Option<i64>>> {
        let mut stmt = self.conn.prepare("SELECT id FROM tracks WHERE path = ?1")?;
        chemins
            .iter()
            .map(|c| {
                Ok(stmt
                    .query_row([c], |r| r.get::<_, i64>(0))
                    .map(Some)
                    .or_else(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => Ok(None),
                        e => Err(e),
                    })?)
            })
            .collect()
    }

    /// Chemin de chaque identifiant, `None` pour ceux qui ont disparu. Même
    /// longueur et même ordre que `ids`.
    pub fn chemins_par_ids(&self, ids: &[i64]) -> Result<Vec<Option<String>>> {
        let mut stmt = self.conn.prepare("SELECT path FROM tracks WHERE id = ?1")?;
        ids.iter()
            .map(|id| {
                Ok(stmt
                    .query_row([id], |r| r.get::<_, String>(0))
                    .map(Some)
                    .or_else(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => Ok(None),
                        e => Err(e),
                    })?)
            })
            .collect()
    }

    /// Les morceaux de `chemins`, **dans cet ordre** ; ceux que la
    /// bibliothèque ne connaît pas sont omis. Sert à reconstruire l'affichage
    /// de la file à partir de ce que le lecteur tient.
    pub fn pistes_par_chemins(&self, chemins: &[String]) -> Result<Vec<TrackRow>> {
        let mut par_chemin = std::collections::HashMap::new();
        for paquet in chemins.chunks(PAQUET) {
            let places = vec!["?"; paquet.len()].join(",");
            let sql = format!("SELECT {TRACK_COLS} FROM tracks WHERE path IN ({places})");
            let mut stmt = self.conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> =
                paquet.iter().map(|c| c as &dyn rusqlite::ToSql).collect();
            for ligne in stmt.query_map(params.as_slice(), track_from_row)? {
                let t = ligne?;
                par_chemin.insert(t.path.clone(), t);
            }
        }
        Ok(chemins
            .iter()
            .filter_map(|c| par_chemin.get(c).cloned())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bibliotheque(chemins: &[&str]) -> Library {
        let lib = Library::open_in_memory().unwrap();
        for c in chemins {
            lib.conn
                .execute(
                    "INSERT INTO tracks(path, title) VALUES (?1, ?2)",
                    rusqlite::params![c, format!("titre de {c}")],
                )
                .unwrap();
        }
        lib
    }

    fn session() -> Session {
        Session {
            file: vec![3, 1, 2],
            avant_melange: vec![1, 2, 3],
            rang: 1,
            position_ms: 61_500,
            alea: true,
            repetition: "toutes".into(),
            volume: 0.4,
        }
    }

    #[test]
    fn la_session_fait_l_aller_retour() {
        let lib = bibliotheque(&[]);
        assert_eq!(lib.session().unwrap(), None);
        lib.enregistrer_session(&session()).unwrap();
        let lue = lib.session().unwrap().expect("session");
        assert_eq!(lue.file, vec![3, 1, 2]);
        assert_eq!(lue.avant_melange, vec![1, 2, 3]);
        assert_eq!((lue.rang, lue.position_ms), (1, 61_500));
        assert!(lue.alea);
        assert_eq!(lue.repetition, "toutes");
        assert!((lue.volume - 0.4).abs() < 1e-6);
    }

    #[test]
    fn une_file_vide_et_un_aleatoire_jamais_utilise_se_relisent_vides() {
        let lib = bibliotheque(&[]);
        let mut s = session();
        s.file.clear();
        s.avant_melange.clear();
        lib.enregistrer_session(&s).unwrap();
        let lue = lib.session().unwrap().expect("session");
        assert!(lue.file.is_empty() && lue.avant_melange.is_empty());
    }

    #[test]
    fn la_position_se_met_a_jour_sans_toucher_a_la_file() {
        let lib = bibliotheque(&[]);
        // Rien d'enregistré : l'appelant doit faire l'écriture complète.
        assert!(!lib.enregistrer_position(0, 10, false, "aucune", 1.0).unwrap());
        lib.enregistrer_session(&session()).unwrap();
        assert!(lib.enregistrer_position(2, 5_000, false, "une", 0.9).unwrap());
        let lue = lib.session().unwrap().expect("session");
        assert_eq!(lue.file, vec![3, 1, 2], "la file ne bouge pas");
        assert_eq!((lue.rang, lue.position_ms), (2, 5_000));
        assert!(!lue.alea);
        assert_eq!(lue.repetition, "une");
    }

    #[test]
    fn une_seule_ligne_quoi_qu_on_enregistre() {
        let lib = bibliotheque(&[]);
        lib.enregistrer_session(&session()).unwrap();
        lib.enregistrer_session(&session()).unwrap();
        let n: i64 = lib
            .conn
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn chemins_et_identifiants_se_traduisent_dans_les_deux_sens() {
        let lib = bibliotheque(&["/m/a.mp3", "/m/b.mp3"]);
        let ids = lib
            .ids_par_chemins(&["/m/b.mp3".into(), "/m/absent.mp3".into(), "/m/a.mp3".into()])
            .unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids[0].is_some() && ids[1].is_none() && ids[2].is_some());
        let chemins = lib
            .chemins_par_ids(&[ids[2].unwrap(), 9_999, ids[0].unwrap()])
            .unwrap();
        assert_eq!(
            chemins,
            vec![Some("/m/a.mp3".into()), None, Some("/m/b.mp3".into())]
        );
    }

    #[test]
    fn les_pistes_gardent_l_ordre_des_chemins_et_omettent_les_inconnus() {
        let lib = bibliotheque(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3"]);
        let pistes = lib
            .pistes_par_chemins(&[
                "/m/c.mp3".into(),
                "/m/inconnu.mp3".into(),
                "/m/a.mp3".into(),
            ])
            .unwrap();
        let titres: Vec<_> = pistes.iter().filter_map(|t| t.title.clone()).collect();
        assert_eq!(titres, vec!["titre de /m/c.mp3", "titre de /m/a.mp3"]);
    }

    #[test]
    fn une_longue_file_passe_par_paquets() {
        let chemins: Vec<String> = (0..1_300).map(|i| format!("/m/{i}.mp3")).collect();
        let refs: Vec<&str> = chemins.iter().map(String::as_str).collect();
        let lib = bibliotheque(&refs);
        let pistes = lib.pistes_par_chemins(&chemins).unwrap();
        assert_eq!(pistes.len(), 1_300);
        assert_eq!(pistes[1_299].path, "/m/1299.mp3");
    }

    #[test]
    fn une_liste_abimee_est_relue_sans_planter() {
        assert_eq!(depuis_texte("1, 2,x,,3"), vec![1, 2, 3]);
        assert!(depuis_texte("").is_empty());
    }
}
