// SPDX-License-Identifier: GPL-3.0-or-later
//! Playlists enregistrées : une file qu'on garde, nommée, pour la rejouer.
//!
//! Tables `playlist` et `playlist_piste` (voir `sql/schema.sql`). Le contenu
//! est une liste de chemins, retrouvés dans `tracks` par jointure : une piste
//! disparue de la bibliothèque reste dans la liste (« manquante ») et revient
//! avec son fichier. Le lecteur n'en sait rien : l'application lui passe les
//! chemins comme pour n'importe quelle file.

use std::path::{Path, PathBuf};

use crate::db::{track_from_row, Library, TrackRow, TRACK_COLS};
use crate::error::Result;

/// Longueur maximale d'un nom, en caractères.
const NOM_MAX: usize = 120;

/// Une playlist telle que la liste l'affiche.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PlaylistRow {
    pub id: i64,
    pub nom: String,
    /// D'où elle vient (demande faite à Lama, point de départ…), si connue.
    pub origine: Option<String>,
    /// Pistes de la liste, manquantes comprises.
    pub nb_pistes: i64,
    /// Pistes dont le fichier n'est plus dans la bibliothèque.
    pub nb_manquantes: i64,
    /// Durée des pistes présentes.
    pub duree_ms: i64,
    pub modifiee_le: i64,
}

/// Ce qu'un ajout à une playlist a fait.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AjoutPlaylist {
    /// Pistes ajoutées à la fin.
    pub ajoutes: u32,
    /// Pistes laissées de côté parce qu'elles y étaient déjà.
    pub deja_presents: u32,
}

/// Issue d'une retouche (retirer, remettre, déplacer) d'une playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditionPlaylist {
    Faite,
    /// La playlist n'existe pas.
    Inconnue,
    /// Le rang ne désigne plus le morceau attendu : la playlist a changé
    /// depuis qu'elle est affichée. Rien n'a été touché.
    Perimee,
}

/// Le rang dans le contenu enregistré de la `rang`-ième piste **présente**
/// (celles que `pistes_de_playlist` rend), ou `None` au-delà de la dernière.
fn rang_reel(contenu: &[(String, bool)], rang: usize) -> Option<usize> {
    contenu
        .iter()
        .enumerate()
        .filter(|(_, (_, present))| *present)
        .nth(rang)
        .map(|(i, _)| i)
}

/// Nom nettoyé : espaces de bord retirés, borné, jamais vide.
pub(crate) fn nom_propre(nom: &str) -> String {
    let nom: String = nom.trim().chars().take(NOM_MAX).collect();
    let nom = nom.trim().to_string();
    if nom.is_empty() {
        "Sans titre".to_string()
    } else {
        nom
    }
}

/// Le texte d'une playlist M3U8 étendue : `#EXTM3U`, puis pour chaque piste une
/// ligne `#EXTINF:<secondes>,<artiste> - <titre>` et son chemin. UTF-8, chemins
/// tels qu'ils sont en base (absolus) : lisible par Plex, VLC, foobar…
pub fn texte_m3u8(pistes: &[TrackRow]) -> String {
    // Un titre ne doit jamais casser la ligne : un saut de ligne y ouvrirait
    // une ligne de chemin parasite.
    let une_ligne = |t: &str| t.replace(['\r', '\n'], " ");
    let mut texte = String::from("#EXTM3U\n");
    for t in pistes {
        // -1 : durée inconnue, la valeur que la convention réserve à cet usage.
        let secondes = t.duration_ms.map_or(-1, |ms| (ms + 500) / 1000);
        let titre = une_ligne(t.title.as_deref().unwrap_or(""));
        let legende = match t.artist.as_deref().filter(|a| !a.is_empty()) {
            Some(a) => format!("{} - {titre}", une_ligne(a)),
            None => titre,
        };
        texte.push_str(&format!("#EXTINF:{secondes},{legende}\n{}\n", t.path));
    }
    texte
}

/// Un nom de fichier sûr tiré du nom d'une playlist : les caractères que les
/// systèmes de fichiers refusent ou interprètent deviennent « _ », les points
/// de tête sont retirés (pas de fichier caché), jamais vide.
pub fn nom_de_fichier(nom: &str) -> String {
    let net: String = nom
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .take(100)
        .collect();
    let net = net.trim().trim_start_matches('.').trim().to_string();
    if net.is_empty() {
        "playlist".to_string()
    } else {
        net
    }
}

/// Écrit `pistes` en M3U8 dans `dossier`, sous le nom de la playlist, et rend le
/// chemin écrit. **N'écrase jamais** : si le fichier existe, « (2) », « (3) »…
pub fn ecrire_m3u8(dossier: &Path, nom: &str, pistes: &[TrackRow]) -> Result<PathBuf> {
    let base = nom_de_fichier(nom);
    let mut cible = dossier.join(format!("{base}.m3u8"));
    let mut n = 2;
    while cible.exists() {
        cible = dossier.join(format!("{base} ({n}).m3u8"));
        n += 1;
    }
    std::fs::write(&cible, texte_m3u8(pistes))?;
    Ok(cible)
}

impl Library {
    /// Enregistre `chemins` (dans cet ordre) sous le nom `nom`. Rend l'identifiant.
    pub fn creer_playlist(
        &self,
        nom: &str,
        origine: Option<&str>,
        chemins: &[String],
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let origine = origine.map(str::trim).filter(|o| !o.is_empty());
        tx.execute(
            "INSERT INTO playlist(nom, origine) VALUES (?1, ?2)",
            rusqlite::params![nom_propre(nom), origine],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO playlist_piste(playlist_id, rang, chemin) VALUES (?1, ?2, ?3)",
            )?;
            for (rang, chemin) in chemins.iter().enumerate() {
                stmt.execute(rusqlite::params![id, rang as i64, chemin])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// Les playlists, la plus récemment modifiée d'abord.
    pub fn playlists(&self) -> Result<Vec<PlaylistRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.nom, p.origine, p.modifiee_le,
                    COUNT(pp.chemin), COUNT(t.id), COALESCE(SUM(t.duration_ms), 0)
               FROM playlist p
               LEFT JOIN playlist_piste pp ON pp.playlist_id = p.id
               LEFT JOIN tracks t ON t.path = pp.chemin
              GROUP BY p.id
              ORDER BY p.modifiee_le DESC, p.id DESC",
        )?;
        let lignes = stmt
            .query_map([], |r| {
                let total: i64 = r.get(4)?;
                let presentes: i64 = r.get(5)?;
                Ok(PlaylistRow {
                    id: r.get(0)?,
                    nom: r.get(1)?,
                    origine: r.get(2)?,
                    modifiee_le: r.get(3)?,
                    nb_pistes: total,
                    nb_manquantes: total - presentes,
                    duree_ms: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(lignes)
    }

    /// Les pistes de la playlist `id`, dans l'ordre enregistré. Celles que la
    /// bibliothèque ne connaît plus sont omises.
    pub fn pistes_de_playlist(&self, id: i64) -> Result<Vec<TrackRow>> {
        let sql = format!(
            "SELECT {TRACK_COLS}
               FROM playlist_piste pp JOIN tracks t ON t.path = pp.chemin
              WHERE pp.playlist_id = ?1
              ORDER BY pp.rang"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let pistes = stmt
            .query_map([id], track_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(pistes)
    }

    /// Renomme la playlist `id`. `false` si elle n'existe pas.
    pub fn renommer_playlist(&self, id: i64, nom: &str) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE playlist SET nom = ?2, modifiee_le = strftime('%s','now') WHERE id = ?1",
            rusqlite::params![id, nom_propre(nom)],
        )?;
        Ok(n > 0)
    }

    /// Le contenu enregistré de `id`, dans l'ordre, chaque chemin marqué
    /// présent (connu de la bibliothèque) ou non. `None` si la playlist
    /// n'existe pas.
    fn contenu(&self, id: i64) -> Result<Option<Vec<(String, bool)>>> {
        let existe: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playlist WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )?;
        if !existe {
            return Ok(None);
        }
        let mut stmt = self.conn.prepare(
            "SELECT pp.chemin, t.id IS NOT NULL
               FROM playlist_piste pp LEFT JOIN tracks t ON t.path = pp.chemin
              WHERE pp.playlist_id = ?1
              ORDER BY pp.rang",
        )?;
        let lignes = stmt
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Some(lignes))
    }

    /// Réécrit tout le contenu de `id` (rangs renumérotés à partir de 0) et
    /// date la modification.
    fn reecrire(&self, id: i64, chemins: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM playlist_piste WHERE playlist_id = ?1", [id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO playlist_piste(playlist_id, rang, chemin) VALUES (?1, ?2, ?3)",
            )?;
            for (rang, chemin) in chemins.iter().enumerate() {
                stmt.execute(rusqlite::params![id, rang as i64, chemin])?;
            }
        }
        tx.execute(
            "UPDATE playlist SET modifiee_le = strftime('%s','now') WHERE id = ?1",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Ajoute `chemins` à la fin de `id`, dans l'ordre. Un chemin déjà dans la
    /// playlist (ou répété dans `chemins`) n'est pas ajouté une seconde fois :
    /// le compte rendu le dit. `None` si la playlist n'existe pas.
    pub fn ajouter_a_playlist(&self, id: i64, chemins: &[String]) -> Result<Option<AjoutPlaylist>> {
        let Some(contenu) = self.contenu(id)? else {
            return Ok(None);
        };
        let mut tous: Vec<String> = contenu.into_iter().map(|(c, _)| c).collect();
        let mut vus: std::collections::HashSet<String> = tous.iter().cloned().collect();
        let mut ajout = AjoutPlaylist::default();
        for c in chemins {
            if vus.insert(c.clone()) {
                tous.push(c.clone());
                ajout.ajoutes += 1;
            } else {
                ajout.deja_presents += 1;
            }
        }
        if ajout.ajoutes > 0 {
            self.reecrire(id, &tous)?;
        }
        Ok(Some(ajout))
    }

    /// Retire de `id` la piste au rang `rang` **de la liste affichée**
    /// ([`Self::pistes_de_playlist`], qui omet les manquantes), à condition que
    /// ce soit bien `chemin` — sinon la playlist a changé ailleurs, et rien
    /// n'est retiré. Les manquantes gardent leur place.
    pub fn retirer_de_playlist(
        &self,
        id: i64,
        rang: usize,
        chemin: &str,
    ) -> Result<EditionPlaylist> {
        let Some(contenu) = self.contenu(id)? else {
            return Ok(EditionPlaylist::Inconnue);
        };
        let Some(i) = rang_reel(&contenu, rang).filter(|&i| contenu[i].0 == chemin) else {
            return Ok(EditionPlaylist::Perimee);
        };
        let tous: Vec<String> = contenu
            .into_iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .map(|(_, (c, _))| c)
            .collect();
        self.reecrire(id, &tous)?;
        Ok(EditionPlaylist::Faite)
    }

    /// Remet `chemin` au rang `rang` de la liste affichée (l'annulation d'un
    /// retrait) ; au-delà de la fin, à la fin. Rien si le chemin y est déjà.
    pub fn inserer_dans_playlist(
        &self,
        id: i64,
        rang: usize,
        chemin: &str,
    ) -> Result<EditionPlaylist> {
        let Some(contenu) = self.contenu(id)? else {
            return Ok(EditionPlaylist::Inconnue);
        };
        if contenu.iter().any(|(c, _)| c == chemin) {
            return Ok(EditionPlaylist::Perimee);
        }
        let i = rang_reel(&contenu, rang).unwrap_or(contenu.len());
        let mut tous: Vec<String> = contenu.into_iter().map(|(c, _)| c).collect();
        tous.insert(i, chemin.to_string());
        self.reecrire(id, &tous)?;
        Ok(EditionPlaylist::Faite)
    }

    /// Déplace la piste du rang `de` au rang `a` de la liste affichée (rangs
    /// de la liste **après** le déplacement, comme `deplacer_file`), à
    /// condition que le rang `de` soit bien `chemin`. Une manquante reste
    /// accrochée à la piste qui la suivait.
    pub fn deplacer_dans_playlist(
        &self,
        id: i64,
        de: usize,
        a: usize,
        chemin: &str,
    ) -> Result<EditionPlaylist> {
        let Some(contenu) = self.contenu(id)? else {
            return Ok(EditionPlaylist::Inconnue);
        };
        let Some(i) = rang_reel(&contenu, de).filter(|&i| contenu[i].0 == chemin) else {
            return Ok(EditionPlaylist::Perimee);
        };
        let mut reste = contenu;
        let piste = reste.remove(i);
        // Le rang `a` dans la liste affichée sans la piste déplacée : elle se
        // glisse juste avant la présente qui l'occupe, ou à la fin.
        let cible = rang_reel(&reste, a).unwrap_or(reste.len());
        reste.insert(cible, piste);
        let tous: Vec<String> = reste.into_iter().map(|(c, _)| c).collect();
        self.reecrire(id, &tous)?;
        Ok(EditionPlaylist::Faite)
    }

    /// Supprime la playlist `id` et son contenu. `false` si elle n'existe pas.
    pub fn supprimer_playlist(&self, id: i64) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM playlist_piste WHERE playlist_id = ?1", [id])?;
        let n = tx.execute("DELETE FROM playlist WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(n > 0)
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

    fn v(chemins: &[&str]) -> Vec<String> {
        chemins.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn une_playlist_garde_ses_pistes_dans_l_ordre() {
        let lib = bibliotheque(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3"]);
        let id = lib
            .creer_playlist(
                "Route",
                Some("Dans l'esprit de X"),
                &v(&["/m/c.mp3", "/m/a.mp3", "/m/b.mp3"]),
            )
            .unwrap();
        let titres: Vec<_> = lib
            .pistes_de_playlist(id)
            .unwrap()
            .into_iter()
            .filter_map(|t| t.title)
            .collect();
        assert_eq!(
            titres,
            vec![
                "titre de /m/c.mp3",
                "titre de /m/a.mp3",
                "titre de /m/b.mp3"
            ]
        );
        let liste = lib.playlists().unwrap();
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].nom, "Route");
        assert_eq!(liste[0].origine.as_deref(), Some("Dans l'esprit de X"));
        assert_eq!((liste[0].nb_pistes, liste[0].nb_manquantes), (3, 0));
        assert_eq!(liste[0].duree_ms, 3000);
    }

    #[test]
    fn un_chemin_inconnu_est_manquant_et_revient_avec_son_fichier() {
        let lib = bibliotheque(&["/m/a.mp3"]);
        let id = lib
            .creer_playlist("Mixte", None, &v(&["/m/a.mp3", "/m/disque.mp3"]))
            .unwrap();
        let l = &lib.playlists().unwrap()[0];
        assert_eq!((l.nb_pistes, l.nb_manquantes), (2, 1));
        assert_eq!(lib.pistes_de_playlist(id).unwrap().len(), 1);

        // Le disque revient : un nouvel identifiant, mais le même chemin.
        lib.conn
            .execute(
                "INSERT INTO tracks(path, title) VALUES ('/m/disque.mp3', 'revenu')",
                [],
            )
            .unwrap();
        assert_eq!(lib.playlists().unwrap()[0].nb_manquantes, 0);
        let titres: Vec<_> = lib
            .pistes_de_playlist(id)
            .unwrap()
            .into_iter()
            .filter_map(|t| t.title)
            .collect();
        assert_eq!(titres, vec!["titre de /m/a.mp3", "revenu"]);
    }

    #[test]
    fn le_nom_est_nettoye_et_jamais_vide() {
        let lib = bibliotheque(&[]);
        let a = lib.creer_playlist("   ", None, &[]).unwrap();
        let b = lib.creer_playlist("  Soirée  ", Some("   "), &[]).unwrap();
        let long = "x".repeat(500);
        let c = lib.creer_playlist(&long, None, &[]).unwrap();
        let noms: std::collections::HashMap<i64, PlaylistRow> = lib
            .playlists()
            .unwrap()
            .into_iter()
            .map(|p| (p.id, p))
            .collect();
        assert_eq!(noms[&a].nom, "Sans titre");
        assert_eq!(noms[&b].nom, "Soirée");
        assert_eq!(noms[&b].origine, None, "une origine vide n'est pas retenue");
        assert_eq!(noms[&c].nom.chars().count(), NOM_MAX);
    }

    #[test]
    fn renommer_et_supprimer() {
        let lib = bibliotheque(&["/m/a.mp3"]);
        let id = lib
            .creer_playlist("Avant", None, &v(&["/m/a.mp3"]))
            .unwrap();
        assert!(lib.renommer_playlist(id, "Après").unwrap());
        assert_eq!(lib.playlists().unwrap()[0].nom, "Après");
        assert!(!lib.renommer_playlist(9_999, "Rien").unwrap());

        assert!(lib.supprimer_playlist(id).unwrap());
        assert!(lib.playlists().unwrap().is_empty());
        let restes: i64 = lib
            .conn
            .query_row("SELECT COUNT(*) FROM playlist_piste", [], |r| r.get(0))
            .unwrap();
        assert_eq!(restes, 0, "le contenu part avec la playlist");
        assert!(!lib.supprimer_playlist(id).unwrap());
    }

    fn piste(path: &str, titre: Option<&str>, artiste: Option<&str>, ms: Option<i64>) -> TrackRow {
        TrackRow {
            id: 1,
            path: path.into(),
            title: titre.map(Into::into),
            artist: artiste.map(Into::into),
            album: None,
            track_no: None,
            year: None,
            duration_ms: ms,
            artist_mbid: None,
        }
    }

    #[test]
    fn le_m3u8_porte_duree_legende_et_chemin() {
        let texte = texte_m3u8(&[
            piste(
                "/m/a.mp3",
                Some("Bus to Beelzebub"),
                Some("Soul Coughing"),
                Some(200_499),
            ),
            piste("/m/b.mp3", Some("Sans artiste"), None, Some(200_500)),
            piste("/m/c.mp3", None, Some("X"), None),
        ]);
        assert_eq!(
            texte,
            "#EXTM3U\n\
             #EXTINF:200,Soul Coughing - Bus to Beelzebub\n/m/a.mp3\n\
             #EXTINF:201,Sans artiste\n/m/b.mp3\n\
             #EXTINF:-1,X - \n/m/c.mp3\n"
        );
    }

    #[test]
    fn un_titre_ne_peut_pas_ouvrir_une_ligne_parasite() {
        let texte = texte_m3u8(&[piste(
            "/m/a.mp3",
            Some("ligne 1\n/etc/passwd"),
            Some("A\rB"),
            Some(1000),
        )]);
        let lignes: Vec<_> = texte.lines().collect();
        assert_eq!(
            lignes.len(),
            3,
            "une seule piste : en-tête, légende, chemin"
        );
        assert_eq!(lignes[2], "/m/a.mp3");
    }

    #[test]
    fn le_nom_de_fichier_est_sur() {
        assert_eq!(
            nom_de_fichier("Rock/Metal: le \"best\" ?"),
            "Rock_Metal_ le _best_ _"
        );
        assert_eq!(nom_de_fichier("..cache"), "cache");
        assert_eq!(nom_de_fichier("   "), "playlist");
        assert_eq!(nom_de_fichier("..."), "playlist");
        assert_eq!(nom_de_fichier(&"é".repeat(300)).chars().count(), 100);
    }

    #[test]
    fn l_export_n_ecrase_jamais_un_fichier_existant() {
        let dossier = std::env::temp_dir().join(format!("rusty-music-m3u8-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let pistes = [piste("/m/a.mp3", Some("A"), Some("B"), Some(1000))];

        let un = ecrire_m3u8(&dossier, "Ma liste", &pistes).unwrap();
        let deux = ecrire_m3u8(&dossier, "Ma liste", &pistes).unwrap();
        let trois = ecrire_m3u8(&dossier, "Ma liste", &pistes).unwrap();
        assert_eq!(un.file_name().unwrap(), "Ma liste.m3u8");
        assert_eq!(deux.file_name().unwrap(), "Ma liste (2).m3u8");
        assert_eq!(trois.file_name().unwrap(), "Ma liste (3).m3u8");
        assert!(std::fs::read_to_string(&un)
            .unwrap()
            .starts_with("#EXTM3U\n"));
        let _ = std::fs::remove_dir_all(&dossier);
    }

    #[test]
    fn la_plus_recente_vient_en_premier() {
        let lib = bibliotheque(&[]);
        let a = lib.creer_playlist("A", None, &[]).unwrap();
        let b = lib.creer_playlist("B", None, &[]).unwrap();
        // Même seconde : le départage est l'identifiant décroissant.
        let ids: Vec<_> = lib.playlists().unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![b, a]);
        // Renommer A la remonte (`modifiee_le` plus récent).
        lib.conn
            .execute(
                "UPDATE playlist SET modifiee_le = modifiee_le - 100 WHERE id = ?1",
                [b],
            )
            .unwrap();
        lib.renommer_playlist(a, "A2").unwrap();
        let ids: Vec<_> = lib.playlists().unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![a, b]);
    }

    fn chemins_de(lib: &Library, id: i64) -> Vec<String> {
        let mut stmt = lib
            .conn
            .prepare("SELECT chemin FROM playlist_piste WHERE playlist_id = ?1 ORDER BY rang")
            .unwrap();
        stmt.query_map([id], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<String>, _>>()
            .unwrap()
    }

    #[test]
    fn une_playlist_vide_se_cree_puis_se_remplit_sans_doublon() {
        let lib = bibliotheque(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3"]);
        let id = lib.creer_playlist("À travailler", None, &[]).unwrap();
        assert_eq!(lib.playlists().unwrap()[0].nb_pistes, 0);

        let r = lib
            .ajouter_a_playlist(id, &v(&["/m/a.mp3", "/m/b.mp3"]))
            .unwrap()
            .unwrap();
        assert_eq!((r.ajoutes, r.deja_presents), (2, 0));
        // b est déjà là, c est répété dans la demande : un seul c, pas de b.
        let r = lib
            .ajouter_a_playlist(id, &v(&["/m/b.mp3", "/m/c.mp3", "/m/c.mp3"]))
            .unwrap()
            .unwrap();
        assert_eq!((r.ajoutes, r.deja_presents), (1, 2));
        assert_eq!(
            chemins_de(&lib, id),
            v(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3"])
        );
        assert!(lib
            .ajouter_a_playlist(9_999, &v(&["/m/a.mp3"]))
            .unwrap()
            .is_none());
    }

    #[test]
    fn ajouter_date_la_modification() {
        let lib = bibliotheque(&["/m/a.mp3"]);
        let a = lib.creer_playlist("A", None, &[]).unwrap();
        let b = lib.creer_playlist("B", None, &[]).unwrap();
        lib.conn
            .execute("UPDATE playlist SET modifiee_le = modifiee_le - 100", [])
            .unwrap();
        lib.ajouter_a_playlist(a, &v(&["/m/a.mp3"])).unwrap();
        let ids: Vec<_> = lib.playlists().unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![a, b], "la playlist retouchée remonte en tête");
    }

    #[test]
    fn retirer_puis_remettre_un_rang_affiche() {
        // b manque à la bibliothèque : la liste affichée est a, c, d.
        let lib = bibliotheque(&["/m/a.mp3", "/m/c.mp3", "/m/d.mp3"]);
        let id = lib
            .creer_playlist(
                "P",
                None,
                &v(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3", "/m/d.mp3"]),
            )
            .unwrap();
        // Rang 1 affiché = c (b, manquante, ne compte pas).
        assert_eq!(
            lib.retirer_de_playlist(id, 1, "/m/a.mp3").unwrap(),
            EditionPlaylist::Perimee,
            "le rang ne désigne pas ce chemin : rien n'est retiré"
        );
        assert_eq!(
            lib.retirer_de_playlist(id, 1, "/m/c.mp3").unwrap(),
            EditionPlaylist::Faite
        );
        assert_eq!(
            chemins_de(&lib, id),
            v(&["/m/a.mp3", "/m/b.mp3", "/m/d.mp3"])
        );
        // L'annulation le remet au même rang affiché, avant d.
        assert_eq!(
            lib.inserer_dans_playlist(id, 1, "/m/c.mp3").unwrap(),
            EditionPlaylist::Faite
        );
        assert_eq!(
            chemins_de(&lib, id),
            v(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3", "/m/d.mp3"])
        );
        assert_eq!(
            lib.inserer_dans_playlist(id, 0, "/m/c.mp3").unwrap(),
            EditionPlaylist::Perimee,
            "déjà présent : pas de doublon"
        );
        assert_eq!(
            lib.inserer_dans_playlist(id, 99, "/m/e.mp3").unwrap(),
            EditionPlaylist::Faite
        );
        assert_eq!(chemins_de(&lib, id).last().unwrap(), "/m/e.mp3");
        assert_eq!(
            lib.retirer_de_playlist(9_999, 0, "/m/a.mp3").unwrap(),
            EditionPlaylist::Inconnue
        );
    }

    #[test]
    fn deplacer_un_rang_affiche() {
        let lib = bibliotheque(&["/m/a.mp3", "/m/c.mp3", "/m/d.mp3"]);
        let id = lib
            .creer_playlist(
                "P",
                None,
                &v(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3", "/m/d.mp3"]),
            )
            .unwrap();
        let affiches = |lib: &Library| -> Vec<String> {
            lib.pistes_de_playlist(id)
                .unwrap()
                .into_iter()
                .map(|t| t.path)
                .collect()
        };
        // a vers la fin (rang 2 de la liste après déplacement).
        assert_eq!(
            lib.deplacer_dans_playlist(id, 0, 2, "/m/a.mp3").unwrap(),
            EditionPlaylist::Faite
        );
        assert_eq!(affiches(&lib), v(&["/m/c.mp3", "/m/d.mp3", "/m/a.mp3"]));
        // La manquante b reste accrochée devant c.
        assert_eq!(
            chemins_de(&lib, id),
            v(&["/m/b.mp3", "/m/c.mp3", "/m/d.mp3", "/m/a.mp3"])
        );
        // d en tête (⌥↑ deux fois revient à ceci).
        assert_eq!(
            lib.deplacer_dans_playlist(id, 1, 0, "/m/d.mp3").unwrap(),
            EditionPlaylist::Faite
        );
        assert_eq!(affiches(&lib), v(&["/m/d.mp3", "/m/c.mp3", "/m/a.mp3"]));
        assert_eq!(
            lib.deplacer_dans_playlist(id, 1, 0, "/m/a.mp3").unwrap(),
            EditionPlaylist::Perimee
        );
        assert_eq!(
            lib.deplacer_dans_playlist(id, 7, 0, "/m/a.mp3").unwrap(),
            EditionPlaylist::Perimee
        );
    }
}
