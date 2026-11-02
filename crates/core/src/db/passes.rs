// SPDX-License-Identifier: GPL-3.0-or-later
//! Le journal des passes « Compléter » (`docs/plan-ecouter-v0.3.md`, point 4,
//! option A ; idée de Disc Player, une passe qui complète, annulable).
//!
//! Une passe ouvre le journal sur **sa** connexion ([`Library::ouvrir_passe`]) :
//! des déclencheurs temporaires notent, pour chaque ligne ajoutée, modifiée ou
//! retirée dans les tables de [`TABLES_JOURNALISEES`], sa clé et sa valeur
//! d'avant. Les fonctions d'écriture de l'enrichissement n'en savent rien, et
//! les autres connexions (l'application, les passes ordinaires) ne sont pas
//! journalisées. [`Library::annuler_passe`] rejoue le journal à l'envers.
//!
//! Annuler rend aux caches leur état d'avant, marques comprises : un artiste
//! que la passe avait interrogé redevient « à interroger ». Seule la dernière
//! passe s'annule — en rejouer une plus ancienne effacerait ce que les
//! suivantes ont appris sur les mêmes lignes.

use super::*;

/// Les tables qu'écrit une passe « Compléter » : genres MusicBrainz (artistes
/// et albums), tags Last.fm, et leurs marques de passage.
pub const TABLES_JOURNALISEES: &[&str] = &[
    "mb_genres",
    "mb_release_groups",
    "mb_fetched",
    "lastfm_tags",
    "lastfm_fetched",
];

/// Une passe journalisée, telle que l'interface la montre.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PasseJournal {
    pub id: i64,
    pub nom: String,
    pub debut: i64,
    /// `None` : en cours, ou coupée avant la fin (elle s'annule quand même).
    pub fin: Option<i64>,
    pub annulee_le: Option<i64>,
    pub bilan: Option<String>,
    /// Lignes notées au journal.
    pub ecritures: i64,
}

/// Colonnes d'une table de la liste, et celles de sa clé primaire.
fn colonnes(conn: &Connection, table: &str) -> Result<(Vec<String>, Vec<String>)> {
    debug_assert!(TABLES_JOURNALISEES.contains(&table));
    let mut stmt = conn.prepare("SELECT name, pk FROM pragma_table_info(?1) ORDER BY pk, cid")?;
    let mut toutes = Vec::new();
    let mut cle = Vec::new();
    for ligne in stmt.query_map([table], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (nom, pk) = ligne?;
        if pk > 0 {
            cle.push(nom.clone());
        }
        toutes.push(nom);
    }
    Ok((toutes, cle))
}

/// `json_object('a', OLD."a", …)`.
fn objet(cols: &[String], ligne: &str) -> String {
    let paires: Vec<String> = cols
        .iter()
        .map(|c| format!("'{c}', {ligne}.\"{c}\""))
        .collect();
    format!("json_object({})", paires.join(", "))
}

fn declencheur(table: &str, op: &str) -> String {
    format!("journal_{table}_{op}")
}

impl Library {
    /// Ouvre une passe journalisée nommée `nom` : rend son identifiant. Tout
    /// ce que cette connexion écrit ensuite dans les tables journalisées est
    /// noté, jusqu'à [`Self::fermer_passe`].
    pub fn ouvrir_passe(&self, nom: &str) -> Result<i64> {
        // Sans elle, un `INSERT OR REPLACE` retire l'ancienne ligne sans
        // déclencher `DELETE` : sa valeur d'avant serait perdue.
        self.conn.execute_batch("PRAGMA recursive_triggers = ON")?;
        self.conn
            .execute("INSERT INTO passe (nom) VALUES (?1)", params![nom])?;
        let id = self.conn.last_insert_rowid();
        for table in TABLES_JOURNALISEES {
            let (cols, cle) = colonnes(&self.conn, table)?;
            let noter = |op: &str, quand: &str, ligne: &str, avant: &str| {
                format!(
                    "CREATE TEMP TRIGGER IF NOT EXISTS {nom} {quand} {op_sql} ON main.{table}
                     BEGIN
                       INSERT INTO passe_ecriture (passe, table_nom, operation, cle, avant)
                       VALUES ({id}, '{table}', '{op}', {cle_json}, {avant});
                     END;",
                    nom = declencheur(table, op),
                    op_sql = op.to_uppercase(),
                    cle_json = objet(&cle, ligne),
                )
            };
            let ancienne = objet(&cols, "OLD");
            self.conn.execute_batch(&format!(
                "{}{}{}",
                noter("insert", "AFTER", "NEW", "NULL"),
                noter("update", "BEFORE", "OLD", &ancienne),
                noter("delete", "BEFORE", "OLD", &ancienne),
            ))?;
        }
        Ok(id)
    }

    /// Ferme le journal de cette connexion et inscrit le bilan de la passe.
    pub fn fermer_passe(&self, id: i64, bilan: &str) -> Result<()> {
        for table in TABLES_JOURNALISEES {
            for op in ["insert", "update", "delete"] {
                self.conn.execute_batch(&format!(
                    "DROP TRIGGER IF EXISTS temp.{}",
                    declencheur(table, op)
                ))?;
            }
        }
        self.conn.execute_batch("PRAGMA recursive_triggers = OFF")?;
        self.conn.execute(
            "UPDATE passe SET fin = strftime('%s','now'), bilan = ?2 WHERE id = ?1",
            params![id, bilan],
        )?;
        Ok(())
    }

    /// La dernière passe nommée `nom`, annulée ou non.
    pub fn derniere_passe(&self, nom: &str) -> Result<Option<PasseJournal>> {
        Ok(self
            .conn
            .query_row(
                "SELECT p.id, p.nom, p.debut, p.fin, p.annulee_le, p.bilan,
                        (SELECT COUNT(*) FROM passe_ecriture e WHERE e.passe = p.id)
                   FROM passe p WHERE p.nom = ?1 ORDER BY p.id DESC LIMIT 1",
                params![nom],
                |r| {
                    Ok(PasseJournal {
                        id: r.get(0)?,
                        nom: r.get(1)?,
                        debut: r.get(2)?,
                        fin: r.get(3)?,
                        annulee_le: r.get(4)?,
                        bilan: r.get(5)?,
                        ecritures: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    /// Rejoue à l'envers le journal de la passe `id` et la marque annulée.
    /// Refusé pour une passe déjà annulée, ou qui n'est pas la dernière de
    /// son nom. Rend le nombre de lignes rétablies.
    pub fn annuler_passe(&mut self, id: i64) -> Result<usize> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (nom, annulee): (String, Option<i64>) = tx.query_row(
            "SELECT nom, annulee_le FROM passe WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if annulee.is_some() {
            return Err(crate::error::Error::Parsing(
                "cette passe est déjà annulée".into(),
            ));
        }
        let derniere: i64 = tx.query_row(
            "SELECT MAX(id) FROM passe WHERE nom = ?1",
            params![nom],
            |r| r.get(0),
        )?;
        if derniere != id {
            return Err(crate::error::Error::Parsing(
                "seule la dernière passe s'annule".into(),
            ));
        }
        let ecritures: Vec<(String, String, String, Option<String>)> = tx
            .prepare(
                "SELECT table_nom, operation, cle, avant FROM passe_ecriture
                  WHERE passe = ?1 ORDER BY rang DESC",
            )?
            .query_map(params![id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        for (table, operation, cle, avant) in &ecritures {
            // Le nom de table vient du journal : seules les tables de la
            // liste entrent dans une requête.
            let Some(table) = TABLES_JOURNALISEES.iter().find(|t| *t == table) else {
                continue;
            };
            let (cols, pk) = colonnes(&tx, table)?;
            match (operation.as_str(), avant) {
                ("insert", _) => {
                    let condition: Vec<String> = pk
                        .iter()
                        .map(|c| format!("\"{c}\" = json_extract(?1, '$.{c}')"))
                        .collect();
                    tx.execute(
                        &format!("DELETE FROM {table} WHERE {}", condition.join(" AND ")),
                        params![cle],
                    )?;
                }
                (_, Some(avant)) => {
                    let noms: Vec<String> = cols.iter().map(|c| format!("\"{c}\"")).collect();
                    let valeurs: Vec<String> = cols
                        .iter()
                        .map(|c| format!("json_extract(?1, '$.{c}')"))
                        .collect();
                    tx.execute(
                        &format!(
                            "INSERT OR REPLACE INTO {table} ({}) SELECT {}",
                            noms.join(", "),
                            valeurs.join(", ")
                        ),
                        params![avant],
                    )?;
                }
                _ => {}
            }
        }
        tx.execute(
            "UPDATE passe SET annulee_le = strftime('%s','now') WHERE id = ?1",
            params![id],
        )?;
        tx.commit()?;
        Ok(ecritures.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tout le contenu des tables journalisées, pour comparer avant et après.
    fn instantane(lib: &Library) -> Vec<String> {
        let mut out = Vec::new();
        for table in TABLES_JOURNALISEES {
            let (cols, _) = colonnes(&lib.conn, table).unwrap();
            let mut stmt = lib
                .conn
                .prepare(&format!(
                    "SELECT {} FROM {table} ORDER BY 1",
                    objet(&cols, table)
                ))
                .unwrap();
            for l in stmt.query_map([], |r| r.get::<_, String>(0)).unwrap() {
                out.push(format!("{table} {}", l.unwrap()));
            }
        }
        out.sort();
        out
    }

    fn tag(nom: &str, poids: u32) -> crate::lastfm::Tag {
        crate::lastfm::Tag {
            nom: nom.into(),
            poids,
        }
    }

    #[test]
    fn annuler_rend_les_caches_tels_qu_avant_la_passe() {
        let mut lib = Library::open_in_memory().unwrap();
        // Avant : un artiste déjà connu, avec un genre et des tags.
        lib.mb_poser_genres("a1", "artist", &[("rock".into(), 3)])
            .unwrap();
        lib.lastfm_tags_poser("a1", &[tag("rock", 90), tag("indie", 40)])
            .unwrap();
        let avant = instantane(&lib);

        let id = lib.ouvrir_passe("completer").unwrap();
        // Un genre modifié et un ajouté sur l'artiste connu, un nouvel
        // artiste, ses albums, et des tags remplacés (DELETE puis INSERT).
        lib.mb_poser_genres("a1", "artist", &[("rock".into(), 5), ("punk".into(), 1)])
            .unwrap();
        lib.mb_poser_genres("a2", "artist", &[("jazz".into(), 2)])
            .unwrap();
        lib.mb_poser_albums(
            "a2",
            &[(
                "rg1".into(),
                "Disque".into(),
                "disque".into(),
                vec![("bebop".into(), 1)],
                Some("1959".into()),
                None,
            )],
        )
        .unwrap();
        lib.lastfm_tags_poser("a1", &[tag("punk", 70)]).unwrap();
        lib.fermer_passe(id, "2 artistes").unwrap();
        assert_ne!(instantane(&lib), avant);

        let p = lib.derniere_passe("completer").unwrap().unwrap();
        assert_eq!((p.id, p.bilan.as_deref()), (id, Some("2 artistes")));
        assert!(p.fin.is_some() && p.ecritures > 0);

        assert_eq!(lib.annuler_passe(id).unwrap() as i64, p.ecritures);
        assert_eq!(instantane(&lib), avant);
        assert!(lib.annuler_passe(id).is_err(), "pas deux fois");
    }

    #[test]
    fn hors_passe_rien_n_est_journalise_et_seule_la_derniere_s_annule() {
        let mut lib = Library::open_in_memory().unwrap();
        let premiere = lib.ouvrir_passe("completer").unwrap();
        lib.mb_poser_genres("a1", "artist", &[("rock".into(), 1)])
            .unwrap();
        lib.fermer_passe(premiere, "").unwrap();
        // Après la fermeture, une écriture ordinaire n'est pas notée.
        lib.mb_poser_genres("a2", "artist", &[("jazz".into(), 1)])
            .unwrap();
        let n: i64 = lib
            .conn
            .query_row(
                "SELECT COUNT(*) FROM passe_ecriture WHERE table_nom = 'mb_genres'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);

        let seconde = lib.ouvrir_passe("completer").unwrap();
        lib.fermer_passe(seconde, "").unwrap();
        assert!(lib.annuler_passe(premiere).is_err());
        assert_eq!(lib.annuler_passe(seconde).unwrap(), 0);
    }
}
