// SPDX-License-Identifier: GPL-3.0-or-later
//! Des lieux à soi (`docs/plan-balade.md`, P8) : ce que l'utilisateur pose
//! sur la carte, sans jamais déplacer un morceau.
//!
//! - **Épingle** : un nom, une couleur, un emoji facultatif, une position
//!   `[lon, lat]`. Pas un morceau ni un bâtiment : elle reste où on l'a
//!   posée quand les tuiles sont régénérées ou la bibliothèque réanalysée.
//! - **Chaîne** : une balade passée ou un itinéraire gardé sous un nom. C'est
//!   une playlist (`crate::playlists`, ses morceaux par chemin) avec en plus
//!   un tracé `[lon, lat]` et une couleur. Le tracé est copié au moment de la
//!   garder : il ne dépend ni des sommets de voirie (renumérotés par un
//!   réimport du plan) ni des positions des morceaux.
//!
//! Les tables `epingle` et `chaine` sont celles de la migration n° 6.

use crate::db::Library;
use crate::error::Result;
use crate::playlists::nom_propre;

/// Une couleur `#RRGGBB`, ou le repli.
fn couleur_propre(c: &str) -> String {
    let c = c.trim();
    let valide =
        c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
    if valide {
        c.to_ascii_uppercase()
    } else {
        COULEUR_DEFAUT.to_string()
    }
}

/// La couleur d'une épingle ou d'une chaîne quand on n'en donne pas.
pub const COULEUR_DEFAUT: &str = "#C4552D";

/// L'emoji d'une épingle : le premier caractère seulement, rien s'il est vide.
fn emoji_propre(e: Option<&str>) -> Option<String> {
    let e = e?.trim();
    // Un emoji peut tenir sur plusieurs points de code (drapeau, teinte de
    // peau, ZWJ) : on garde le premier groupe, à défaut de segmenter les
    // graphèmes, jusqu'à 8 points de code.
    let premier: String = e.chars().take(8).collect();
    (!premier.is_empty()).then_some(premier)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Epingle {
    pub id: i64,
    pub nom: String,
    pub couleur: String,
    pub emoji: Option<String>,
    pub lon: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Chaine {
    /// La playlist qu'elle est.
    pub playlist_id: i64,
    pub nom: String,
    pub couleur: String,
    /// `"balade"` ou `"itineraire"`.
    pub origine: String,
    pub trace: Vec<[f64; 2]>,
    pub nb_pistes: i64,
}

impl Library {
    /// Pose une épingle. Rend l'épingle telle qu'enregistrée.
    pub fn creer_epingle(
        &self,
        nom: &str,
        couleur: &str,
        emoji: Option<&str>,
        lon: f64,
        lat: f64,
    ) -> Result<Epingle> {
        let (nom, couleur, emoji) = (
            nom_propre(nom),
            couleur_propre(couleur),
            emoji_propre(emoji),
        );
        self.conn.execute(
            "INSERT INTO epingle(nom, couleur, emoji, lon, lat) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![nom, couleur, emoji, lon, lat],
        )?;
        Ok(Epingle {
            id: self.conn.last_insert_rowid(),
            nom,
            couleur,
            emoji,
            lon,
            lat,
        })
    }

    /// Les épingles, la plus ancienne d'abord (les dernières se dessinent
    /// par-dessus).
    pub fn epingles(&self) -> Result<Vec<Epingle>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, nom, couleur, emoji, lon, lat FROM epingle ORDER BY id")?;
        let v = stmt
            .query_map([], |r| {
                Ok(Epingle {
                    id: r.get(0)?,
                    nom: r.get(1)?,
                    couleur: r.get(2)?,
                    emoji: r.get(3)?,
                    lon: r.get(4)?,
                    lat: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Retire l'épingle `id`. `false` si elle n'existe pas.
    pub fn supprimer_epingle(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM epingle WHERE id = ?1", [id])?
            > 0)
    }

    /// Garde une chaîne : une playlist de `chemins`, nommée `nom`, et son
    /// tracé. Rend l'identifiant de la playlist.
    pub fn garder_chaine(
        &self,
        nom: &str,
        couleur: &str,
        origine: &str,
        chemins: &[String],
        trace: &[[f64; 2]],
    ) -> Result<i64> {
        let origine = if origine == "itineraire" {
            "itineraire"
        } else {
            "balade"
        };
        let libelle = match origine {
            "itineraire" => "Chaîne gardée d'un itinéraire",
            _ => "Chaîne gardée d'une balade",
        };
        let id = self.creer_playlist(nom, Some(libelle), chemins)?;
        let trace = serde_json::to_string(trace).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "INSERT INTO chaine(playlist_id, couleur, trace, origine) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, couleur_propre(couleur), trace, origine],
        )?;
        Ok(id)
    }

    /// Les chaînes, la plus ancienne d'abord.
    pub fn chaines(&self) -> Result<Vec<Chaine>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.playlist_id, p.nom, c.couleur, c.origine, c.trace,
                    (SELECT COUNT(*) FROM playlist_piste pp WHERE pp.playlist_id = p.id)
               FROM chaine c JOIN playlist p ON p.id = c.playlist_id
              ORDER BY c.creee_le, c.playlist_id",
        )?;
        let v = stmt
            .query_map([], |r| {
                let trace: String = r.get(4)?;
                Ok(Chaine {
                    playlist_id: r.get(0)?,
                    nom: r.get(1)?,
                    couleur: r.get(2)?,
                    origine: r.get(3)?,
                    trace: serde_json::from_str(&trace).unwrap_or_default(),
                    nb_pistes: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(v)
    }

    /// Retire le tracé de la chaîne `playlist_id` ; la playlist reste.
    pub fn oublier_trace(&self, playlist_id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM chaine WHERE playlist_id = ?1", [playlist_id])?
            > 0)
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
                    "INSERT INTO tracks(path, title, duration_ms) VALUES (?1, ?1, 1000)",
                    [c],
                )
                .unwrap();
        }
        lib
    }

    #[test]
    fn une_epingle_se_pose_se_relit_et_se_retire() {
        let lib = bibliotheque(&[]);
        let e = lib
            .creer_epingle("  Chez moi ", "#c4552d", Some("🏠"), 2.35, 48.85)
            .unwrap();
        assert_eq!(e.nom, "Chez moi");
        assert_eq!(e.couleur, "#C4552D");
        assert_eq!(lib.epingles().unwrap(), vec![e.clone()]);
        let sans = lib
            .creer_epingle("", "rouge", Some("  "), 2.3, 48.8)
            .unwrap();
        assert_eq!(sans.nom, "Sans titre");
        assert_eq!(sans.couleur, COULEUR_DEFAUT);
        assert_eq!(sans.emoji, None);
        assert!(lib.supprimer_epingle(e.id).unwrap());
        assert!(!lib.supprimer_epingle(e.id).unwrap());
        assert_eq!(lib.epingles().unwrap(), vec![sans]);
    }

    #[test]
    fn une_chaine_est_une_playlist_avec_un_trace() {
        let lib = bibliotheque(&["/m/a", "/m/b"]);
        let trace = [[2.35, 48.85], [2.351, 48.851]];
        let id = lib
            .garder_chaine(
                "Dimanche",
                "#3366AA",
                "balade",
                &["/m/a".into(), "/m/b".into()],
                &trace,
            )
            .unwrap();
        let c = &lib.chaines().unwrap()[0];
        assert_eq!(
            (c.playlist_id, c.nom.as_str(), c.nb_pistes),
            (id, "Dimanche", 2)
        );
        assert_eq!(c.trace, trace.to_vec());
        let p = lib.playlists().unwrap();
        assert_eq!(p[0].origine.as_deref(), Some("Chaîne gardée d'une balade"));
        // Oublier le tracé garde la playlist ; supprimer la playlist emporte la chaîne.
        let id2 = lib
            .garder_chaine("Itinéraire", "#000000", "itineraire", &["/m/b".into()], &[])
            .unwrap();
        assert!(lib.oublier_trace(id2).unwrap());
        assert_eq!(lib.playlists().unwrap().len(), 2);
        assert!(lib.supprimer_playlist(id).unwrap());
        assert!(lib.chaines().unwrap().is_empty());
    }

    /// Les critères de P8 : une réanalyse réécrit les empreintes et la carte,
    /// un rescan peut rendre à un fichier un autre identifiant. Ni l'une ni
    /// l'autre ne touchent aux épingles ni aux chaînes.
    #[test]
    fn epingles_et_chaines_survivent_a_un_rescan_et_a_une_reanalyse() {
        let lib = bibliotheque(&["/m/a", "/m/b"]);
        lib.creer_epingle("Ici", "#112233", None, 2.35, 48.85)
            .unwrap();
        let id = lib
            .garder_chaine(
                "C",
                "#112233",
                "balade",
                &["/m/a".into(), "/m/b".into()],
                &[[2.0, 48.0]],
            )
            .unwrap();
        let avant = (lib.epingles().unwrap(), lib.chaines().unwrap());
        // Réanalyse : empreintes, positions de carte et descripteurs sont
        // vidés puis refaits.
        lib.conn
            .execute_batch("DELETE FROM features; DELETE FROM descriptors;")
            .unwrap();
        // Rescan : le fichier /m/a sort puis revient, sous un autre identifiant.
        lib.conn
            .execute("DELETE FROM tracks WHERE path = '/m/a'", [])
            .unwrap();
        lib.conn
            .execute(
                "INSERT INTO tracks(path, title, duration_ms) VALUES ('/m/a', 'a', 1000)",
                [],
            )
            .unwrap();
        assert_eq!((lib.epingles().unwrap(), lib.chaines().unwrap()), avant);
        let chemins: Vec<String> = lib
            .pistes_de_playlist(id)
            .unwrap()
            .into_iter()
            .map(|t| t.path)
            .collect();
        assert_eq!(chemins, vec!["/m/a", "/m/b"]);
    }
}
