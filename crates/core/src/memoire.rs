// SPDX-License-Identifier: GPL-3.0-or-later
//! Mémoire d'écoute : l'historique de ce qu'on a écouté et les favoris.
//!
//! Tables `ecoute` (un journal) et `favori` (voir `sql/schema.sql`), par chemin
//! comme les playlists. Tout est local : rien n'est envoyé à un service.
//!
//! De là viennent trois listes calculées — récemment écoutés, les plus
//! écoutés, favoris — que l'interface montre en tête de la vue Playlists.

use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// Une écoute compte après 30 secondes **réellement jouées**, ou la moitié de
/// la piste si elle est plus courte que une minute — le premier seuil atteint.
pub const SEUIL_ECOUTE: Duration = Duration::from_secs(30);

/// Ce que le suivi d'écoute sait de la piste en cours, d'un battement à
/// l'autre. Commun au bureau et au compagnon iOS : une écoute compte au même
/// moment des deux côtés.
pub struct SuiviEcoute {
    chemin: Option<PathBuf>,
    /// Temps réellement joué sur cette piste (pause exclue, saut exclu).
    joue: Duration,
    /// Temps à jouer pour que l'écoute compte.
    seuil: Duration,
    compte: bool,
    position: Duration,
    dernier_battement: std::time::Instant,
}

impl SuiviEcoute {
    pub fn nouveau() -> Self {
        Self {
            chemin: None,
            joue: Duration::ZERO,
            seuil: SEUIL_ECOUTE,
            compte: false,
            position: Duration::ZERO,
            dernier_battement: std::time::Instant::now(),
        }
    }

    /// Le temps écoulé depuis le battement précédent, à passer à
    /// [`SuiviEcoute::avancer`]. Un battement très en retard (appareil
    /// endormi) ne vaut pas du temps joué : plafonné à 2 s.
    pub fn battement(&mut self) -> Duration {
        let maintenant = std::time::Instant::now();
        let ecoule = maintenant
            .duration_since(self.dernier_battement)
            .min(Duration::from_secs(2));
        self.dernier_battement = maintenant;
        ecoule
    }
}

/// Seuil d'une piste de `duree_ms` : 30 s, ou la moitié si elle est plus courte
/// qu'une minute. Une durée inconnue ou nulle garde les 30 s.
pub fn seuil_ecoute(duree_ms: Option<i64>) -> Duration {
    match duree_ms {
        Some(ms) if ms > 0 => SEUIL_ECOUTE.min(Duration::from_millis(ms as u64 / 2)),
        _ => SEUIL_ECOUTE,
    }
}

impl SuiviEcoute {
    /// Fait avancer le suivi d'un battement : `courant` est la piste en cours,
    /// `ecoule` le temps écoulé depuis le battement précédent. Rend le chemin
    /// à noter ([`Library::noter_ecoute`]) quand l'écoute **vient** de compter, `None` sinon.
    ///
    /// On compte le **temps écoulé en lecture**, pas la position : sauter à la
    /// fin d'une piste ne la fait pas compter. Une piste qui recommence
    /// (répétition « une », retour au début) repart de zéro. `duree` donne la
    /// durée d'une piste, demandée une seule fois par piste.
    pub fn avancer(
        &mut self,
        courant: Option<PathBuf>,
        en_pause: bool,
        position: Duration,
        ecoule: Duration,
        duree: impl FnOnce(&Path) -> Option<i64>,
    ) -> Option<PathBuf> {
        let recommence = courant == self.chemin && position + Duration::from_secs(5) < self.position;
        if courant != self.chemin || recommence {
            self.seuil = courant.as_deref().map_or(SEUIL_ECOUTE, |c| seuil_ecoute(duree(c)));
            self.chemin = courant;
            self.joue = Duration::ZERO;
            self.compte = false;
        }
        self.position = position;

        let chemin = self.chemin.as_ref()?;
        if en_pause || self.compte {
            return None;
        }
        self.joue += ecoule;
        if self.joue < self.seuil {
            return None;
        }
        self.compte = true;
        Some(chemin.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fait jouer `suivi` pendant `secondes` battements d'une seconde sur la
    /// piste `piste` (durée `duree_ms`), de la position `depart`. Rend les
    /// écoutes notées.
    fn jouer(
        suivi: &mut SuiviEcoute,
        piste: &str,
        duree_ms: i64,
        depart: u64,
        secondes: u64,
        en_pause: bool,
    ) -> Vec<String> {
        let mut notees = Vec::new();
        for t in 0..secondes {
            let r = suivi.avancer(
                Some(std::path::PathBuf::from(piste)),
                en_pause,
                Duration::from_secs(depart + t),
                Duration::from_secs(1),
                |_| Some(duree_ms),
            );
            notees.extend(r.map(|p| p.display().to_string()));
        }
        notees
    }

    #[test]
    fn une_ecoute_compte_apres_trente_secondes_jouees_une_seule_fois() {
        let mut s = SuiviEcoute::nouveau();
        assert!(jouer(&mut s, "/m/a", 200_000, 0, 29, false).is_empty());
        assert_eq!(jouer(&mut s, "/m/a", 200_000, 29, 100, false), vec!["/m/a"]);
    }

    #[test]
    fn la_pause_et_le_saut_ne_font_pas_compter() {
        let mut s = SuiviEcoute::nouveau();
        // 60 s en pause : rien n'est joué.
        assert!(jouer(&mut s, "/m/a", 200_000, 0, 60, true).is_empty());
        // Sauter à la fin ne vaut pas 30 s de lecture.
        assert!(jouer(&mut s, "/m/a", 200_000, 190, 5, false).is_empty());
    }

    #[test]
    fn une_piste_courte_compte_a_la_moitie() {
        let mut s = SuiviEcoute::nouveau();
        // 40 s de durée : seuil de 20 s.
        assert_eq!(jouer(&mut s, "/m/a", 40_000, 0, 20, false), vec!["/m/a"]);
        assert_eq!(seuil_ecoute(None), Duration::from_secs(30));
        assert_eq!(seuil_ecoute(Some(0)), Duration::from_secs(30));
        assert_eq!(seuil_ecoute(Some(10 * 60_000)), Duration::from_secs(30));
    }

    #[test]
    fn changer_de_piste_remet_le_compte_a_zero() {
        let mut s = SuiviEcoute::nouveau();
        assert!(jouer(&mut s, "/m/a", 200_000, 0, 25, false).is_empty());
        // 25 s de plus sur une autre piste : ni a (changée) ni b (pas assez).
        assert!(jouer(&mut s, "/m/b", 200_000, 0, 25, false).is_empty());
        assert_eq!(jouer(&mut s, "/m/b", 200_000, 25, 10, false), vec!["/m/b"]);
    }

    #[test]
    fn la_repetition_d_une_piste_la_fait_compter_a_chaque_tour() {
        let mut s = SuiviEcoute::nouveau();
        let mut notees = jouer(&mut s, "/m/a", 40_000, 0, 40, false);
        // Le morceau recommence à 0 : la position a reculé de plus de 5 s.
        notees.extend(jouer(&mut s, "/m/a", 40_000, 0, 40, false));
        assert_eq!(notees, vec!["/m/a", "/m/a"]);
    }

    #[test]
    fn rien_en_lecture_ne_compte() {
        let mut s = SuiviEcoute::nouveau();
        let r = s.avancer(None, false, Duration::ZERO, Duration::from_secs(60), |_| None);
        assert!(r.is_none());
    }

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
