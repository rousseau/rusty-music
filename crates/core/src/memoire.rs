// SPDX-License-Identifier: GPL-3.0-or-later
//! Mémoire d'écoute : l'historique de ce qu'on a écouté et les favoris.
//!
//! Tables `ecoute` (un journal) et `favori` (voir `sql/schema.sql`), par chemin
//! comme les playlists. Tout est local : rien n'est envoyé à un service.
//!
//! De là viennent trois listes calculées — récemment écoutés, les plus
//! écoutés, favoris — que l'interface montre en tête de la vue Playlists.

use crate::db::{track_from_row, Library, TrackRow, TRACK_COLS};
use crate::error::Result;

/// Taille des listes « récemment » et « les plus écoutés ».
pub const LONGUEUR_LISTE: usize = 100;

/// Une liste calculée à partir des écoutes, telle que la vue Playlists l'affiche.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListeIntelligente {
    /// « recents », « plus-ecoutes » ou « favoris » : ce qu'on redemande pour
    /// en obtenir les pistes ([`Library::liste_intelligente`]).
    pub quoi: String,
    pub nom: String,
    pub nb_pistes: i64,
    pub duree_ms: i64,
}

impl Library {
    /// Note une écoute de `chemin`, maintenant.
    pub fn noter_ecoute(&self, chemin: &str) -> Result<()> {
        self.conn
            .execute("INSERT INTO ecoute(chemin) VALUES (?1)", [chemin])?;
        Ok(())
    }

    /// Bascule le favori de `chemin` ; rend son nouvel état (`true` = favori).
    pub fn basculer_favori(&self, chemin: &str) -> Result<bool> {
        let ajoute = self
            .conn
            .execute("INSERT OR IGNORE INTO favori(chemin) VALUES (?1)", [chemin])?;
        if ajoute > 0 {
            return Ok(true);
        }
        self.conn
            .execute("DELETE FROM favori WHERE chemin = ?1", [chemin])?;
        Ok(false)
    }

    /// Les chemins favoris, le plus récemment ajouté d'abord.
    pub fn chemins_favoris(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT chemin FROM favori ORDER BY le DESC, rowid DESC")?;
        let chemins = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(chemins)
    }

    /// Les morceaux écoutés les plus récemment, une fois chacun.
    pub fn pistes_recentes(&self, limite: usize) -> Result<Vec<TrackRow>> {
        self.liste(
            "FROM (SELECT chemin, MAX(le) AS dernier FROM ecoute GROUP BY chemin) e
               JOIN tracks t ON t.path = e.chemin
              ORDER BY e.dernier DESC, t.id DESC",
            limite,
        )
    }

    /// Les morceaux écoutés **au moins deux fois**, du plus au moins écouté.
    /// Avec une écoute chacun, « les plus écoutés » ne serait que « récemment ».
    pub fn pistes_les_plus_ecoutees(&self, limite: usize) -> Result<Vec<TrackRow>> {
        self.liste(
            "FROM (SELECT chemin, COUNT(*) AS n, MAX(le) AS dernier
                    FROM ecoute GROUP BY chemin HAVING COUNT(*) >= 2) e
               JOIN tracks t ON t.path = e.chemin
              ORDER BY e.n DESC, e.dernier DESC, t.id DESC",
            limite,
        )
    }

    /// Les favoris que la bibliothèque connaît, le plus récent d'abord.
    pub fn pistes_favorites(&self) -> Result<Vec<TrackRow>> {
        self.liste(
            "FROM favori f JOIN tracks t ON t.path = f.chemin
              ORDER BY f.le DESC, f.rowid DESC",
            usize::MAX,
        )
    }

    fn liste(&self, corps: &str, limite: usize) -> Result<Vec<TrackRow>> {
        let limite = i64::try_from(limite).unwrap_or(i64::MAX);
        let sql = format!("SELECT {TRACK_COLS} {corps} LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let pistes = stmt
            .query_map([limite], track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(pistes)
    }

    /// Les pistes de la liste calculée `quoi` (« recents », « plus-ecoutes »,
    /// « favoris »). `None` pour un nom inconnu.
    pub fn liste_intelligente(&self, quoi: &str) -> Result<Option<Vec<TrackRow>>> {
        Ok(Some(match quoi {
            "recents" => self.pistes_recentes(LONGUEUR_LISTE)?,
            "plus-ecoutes" => self.pistes_les_plus_ecoutees(LONGUEUR_LISTE)?,
            "favoris" => self.pistes_favorites()?,
            _ => return Ok(None),
        }))
    }

    /// Les listes calculées **qui ne sont pas vides** — une liste vide n'a
    /// rien à montrer —, dans l'ordre où la vue Playlists les range.
    pub fn listes_intelligentes(&self) -> Result<Vec<ListeIntelligente>> {
        let mut listes = Vec::new();
        for (quoi, nom) in [
            ("recents", "Récemment écoutés"),
            ("plus-ecoutes", "Les plus écoutés"),
            ("favoris", "Favoris"),
        ] {
            let pistes = self.liste_intelligente(quoi)?.unwrap_or_default();
            if pistes.is_empty() {
                continue;
            }
            listes.push(ListeIntelligente {
                quoi: quoi.into(),
                nom: nom.into(),
                nb_pistes: pistes.len() as i64,
                duree_ms: pistes.iter().filter_map(|t| t.duration_ms).sum(),
            });
        }
        Ok(listes)
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
                    "INSERT INTO tracks(path, title, duration_ms) VALUES (?1, ?2, 1000)",
                    rusqlite::params![c, format!("titre de {c}")],
                )
                .unwrap();
        }
        lib
    }

    /// Écoute à un instant précis, pour ne pas dépendre de l'horloge.
    fn ecoute_a(lib: &Library, chemin: &str, le: i64) {
        lib.conn
            .execute("INSERT INTO ecoute(chemin, le) VALUES (?1, ?2)", rusqlite::params![chemin, le])
            .unwrap();
    }

    fn titres(pistes: Vec<TrackRow>) -> Vec<String> {
        pistes.into_iter().filter_map(|t| t.title).collect()
    }

    #[test]
    fn les_recents_viennent_par_derniere_ecoute_une_fois_chacun() {
        let lib = bibliotheque(&["/m/a", "/m/b", "/m/c"]);
        ecoute_a(&lib, "/m/a", 100);
        ecoute_a(&lib, "/m/b", 200);
        ecoute_a(&lib, "/m/a", 300); // a repasse devant b
        ecoute_a(&lib, "/m/inconnu", 400); // hors bibliothèque : ignoré
        assert_eq!(
            titres(lib.pistes_recentes(10).unwrap()),
            vec!["titre de /m/a", "titre de /m/b"]
        );
        assert_eq!(lib.pistes_recentes(1).unwrap().len(), 1, "la limite s'applique");
    }

    #[test]
    fn les_plus_ecoutes_demandent_deux_ecoutes_et_se_classent_par_nombre() {
        let lib = bibliotheque(&["/m/a", "/m/b", "/m/c"]);
        for t in [10, 20] {
            ecoute_a(&lib, "/m/a", t);
        }
        for t in [30, 40, 50] {
            ecoute_a(&lib, "/m/b", t);
        }
        ecoute_a(&lib, "/m/c", 60); // une seule : pas dans la liste
        assert_eq!(
            titres(lib.pistes_les_plus_ecoutees(10).unwrap()),
            vec!["titre de /m/b", "titre de /m/a"]
        );
    }

    #[test]
    fn le_favori_se_bascule_et_se_range_du_plus_recent() {
        let lib = bibliotheque(&["/m/a", "/m/b"]);
        assert!(lib.basculer_favori("/m/a").unwrap());
        assert!(lib.basculer_favori("/m/b").unwrap());
        assert_eq!(lib.chemins_favoris().unwrap(), vec!["/m/b", "/m/a"]);
        assert!(!lib.basculer_favori("/m/a").unwrap(), "second appel : retiré");
        assert_eq!(lib.chemins_favoris().unwrap(), vec!["/m/b"]);
        assert_eq!(titres(lib.pistes_favorites().unwrap()), vec!["titre de /m/b"]);
    }

    #[test]
    fn un_favori_dont_le_fichier_manque_reste_en_base_et_revient() {
        let lib = bibliotheque(&["/m/a"]);
        lib.basculer_favori("/m/disque").unwrap();
        assert!(lib.pistes_favorites().unwrap().is_empty());
        lib.conn
            .execute("INSERT INTO tracks(path, title) VALUES ('/m/disque', 'revenu')", [])
            .unwrap();
        assert_eq!(titres(lib.pistes_favorites().unwrap()), vec!["revenu"]);
    }

    #[test]
    fn les_listes_vides_ne_sont_pas_proposees() {
        let lib = bibliotheque(&["/m/a", "/m/b"]);
        assert!(lib.listes_intelligentes().unwrap().is_empty());

        lib.noter_ecoute("/m/a").unwrap();
        let noms: Vec<_> = lib.listes_intelligentes().unwrap().into_iter().map(|l| l.quoi).collect();
        assert_eq!(noms, vec!["recents"], "une écoute : ni « plus écoutés » ni favoris");

        lib.noter_ecoute("/m/a").unwrap();
        lib.basculer_favori("/m/b").unwrap();
        let listes = lib.listes_intelligentes().unwrap();
        let noms: Vec<_> = listes.iter().map(|l| l.quoi.as_str()).collect();
        assert_eq!(noms, vec!["recents", "plus-ecoutes", "favoris"]);
        assert_eq!((listes[0].nb_pistes, listes[0].duree_ms), (1, 1000));
    }

    #[test]
    fn un_nom_de_liste_inconnu_n_est_pas_une_liste_vide() {
        let lib = bibliotheque(&[]);
        assert!(lib.liste_intelligente("n-importe-quoi").unwrap().is_none());
        assert!(lib.liste_intelligente("favoris").unwrap().is_some());
    }
}
