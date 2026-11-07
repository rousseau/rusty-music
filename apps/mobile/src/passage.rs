// SPDX-License-Identifier: GPL-3.0-or-later
//! Réception du passage de la lecture Mac → iPhone (`rusty_music_valise::passage`).
//! Par le câble (option A), le Mac dépose `session-passage.json` dans les
//! Documents de l'app puis se met en pause ; par le Wi-Fi (option B,
//! `wifi.rs`), il le tient en attente et ne se tait qu'à l'accusé de l'app.
//! L'app regarde au lancement et à chaque retour au premier plan, rouvre la
//! valise qui porte le plus de morceaux de la file et la reprend **en pause**
//! — un passage ne doit jamais faire sonner un iPhone dans une poche. Un
//! passage ne se rejoue pas : le fichier est consommé, qu'il serve ou non, et
//! celui du Wi-Fi est reconnu à son instant d'émission.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use rusty_music_player::{Instantane, Repetition};
use rusty_music_valise::passage::{Passage, FICHIER};
use serde::Serialize;

use crate::{echec, lecteur, ouvrir, session, verrou, wifi, AlbumVue, Etat};

/// Ce que l'app dit du passage reçu.
#[derive(Serialize)]
pub(crate) struct Recu {
    /// La valise rouverte, `None` si aucune ne porte un morceau de la file.
    pub(crate) nom: Option<String>,
    pub(crate) albums: Vec<AlbumVue>,
    /// Morceaux repris et morceaux de la file absents de la valise.
    pistes: usize,
    absents: usize,
    /// La piste que jouait le Mac est dans la valise (sinon on part de la
    /// suivante, au début).
    courante_presente: bool,
}

/// Combien de morceaux de `passage` porte chaque valise du dossier ; rend la
/// mieux pourvue, s'il y en a une.
fn valise_pour(dossier: &Path, passage: &Passage) -> Option<String> {
    std::fs::read_dir(dossier)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let base = e.path().join("base.sqlite");
            if !e.path().join("valise.json").is_file() || !base.is_file() {
                return None;
            }
            let conn = Connection::open_with_flags(&base, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
            let mut stmt = conn
                .prepare(
                    "SELECT 1 FROM tracks t JOIN valise_fichier f ON f.track_id = t.id
                      WHERE t.path = ?1 LIMIT 1",
                )
                .ok()?;
            let n = passage.presents(|c| stmt.exists([c]).unwrap_or(false));
            (n > 0).then(|| (n, e.file_name().to_string_lossy().into_owned()))
        })
        .max()
        .map(|(_, nom)| nom)
}

/// Lit et applique le passage du Mac, déposé au câble ou en attente sur le
/// Wi-Fi, s'il y en a un.
pub(crate) fn recevoir(etat: &Etat) -> Result<Option<Recu>, String> {
    let chemin = etat.dossier.join(FICHIER);
    if chemin.is_file() {
        let lu = Passage::lire(&chemin);
        // Consommé avant d'être appliqué : un passage illisible ou qui échoue
        // ne doit pas revenir à chaque lancement.
        let _ = std::fs::remove_file(&chemin);
        let passage = lu.map_err(echec)?;
        *verrou(&etat.dernier_passage) = passage.emis_le;
        tracing::info!(
            pistes = passage.file.len(),
            "passage reçu du Mac, par le câble"
        );
        return appliquer(etat, &passage).map(Some);
    }

    let Some((client, _)) = wifi::joindre(etat) else {
        return Ok(None);
    };
    let passage = match client.passage_en_attente() {
        Ok(Some(p)) => p,
        Ok(None) => return Ok(None),
        Err(e) => {
            tracing::warn!(erreur = %e, "passage du Mac illisible");
            return Ok(None);
        }
    };
    if std::mem::replace(&mut *verrou(&etat.dernier_passage), passage.emis_le) == passage.emis_le {
        return Ok(None);
    }
    tracing::info!(
        pistes = passage.file.len(),
        "passage reçu du Mac, par le Wi-Fi"
    );
    let recu = appliquer(etat, &passage)?;
    // Le Mac ne se tait qu'à l'accusé : sans valise qui porte la file, rien
    // n'a été repris ici, il continue de jouer.
    if recu.nom.is_some() {
        if let Err(e) = client.accuser(passage.emis_le) {
            tracing::warn!(erreur = %e, "accusé du passage perdu : le Mac continue");
        }
    }
    Ok(Some(recu))
}

/// Rouvre la valise qui porte le plus de la file et y reprend le passage.
fn appliquer(etat: &Etat, passage: &Passage) -> Result<Recu, String> {
    let Some(nom) = valise_pour(&etat.dossier, passage) else {
        return Ok(Recu {
            nom: None,
            albums: Vec::new(),
            pistes: 0,
            absents: passage.file.len(),
            courante_presente: false,
        });
    };
    let albums = ouvrir(etat, &nom)?;
    let r = {
        let fichiers = etat.fichiers.read().unwrap_or_else(|e| e.into_inner());
        passage.reduire(|c| fichiers.contains_key(Path::new(c)))
    };
    {
        let mut garde = lecteur(etat)?;
        let player = garde.as_mut().expect("ouvert par `lecteur`");
        // Le passage remplace ce que l'iPhone tenait : la lecture du Mac est
        // la plus récente. `restaurer` charge la piste en pause. Le volume
        // reste celui du téléphone : celui du Mac valait pour ses enceintes.
        let volume = player.volume();
        player.stop();
        player
            .restaurer(Instantane {
                file: r.file.iter().map(PathBuf::from).collect(),
                avant_melange: r.avant_melange.iter().map(PathBuf::from).collect(),
                rang: r.rang,
                position: Duration::from_millis(r.position_ms),
                alea: passage.alea,
                repetition: Repetition::depuis_nom(&passage.repetition),
                volume,
            })
            .map_err(echec)?;
    }
    // Sans attendre la sauvegarde périodique : si iOS ferme l'app tout de
    // suite, le prochain lancement reprend ce passage, pas l'ancienne session.
    if let Err(e) = session::sauvegarder(etat, &mut session::Memo::default()) {
        tracing::warn!(erreur = %e, "sauvegarde du passage impossible");
    }
    Ok(Recu {
        nom: Some(nom),
        albums,
        pistes: r.file.len(),
        absents: r.absents,
        courante_presente: r.courante_presente,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_valise::passage::VERSION;

    /// Une valise réduite à ce que `valise_pour` lit : ses pistes et leurs
    /// fichiers.
    fn valise(dossier: &Path, nom: &str, chemins: &[&str]) {
        let racine = dossier.join(nom);
        std::fs::create_dir_all(&racine).unwrap();
        std::fs::write(racine.join("valise.json"), "{}").unwrap();
        let conn = Connection::open(racine.join("base.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks(id INTEGER PRIMARY KEY, path TEXT);
             CREATE TABLE valise_fichier(track_id INTEGER, fichier TEXT);",
        )
        .unwrap();
        for (i, c) in chemins.iter().enumerate() {
            conn.execute(
                "INSERT INTO tracks(id, path) VALUES (?1, ?2)",
                rusqlite::params![i as i64, c],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO valise_fichier(track_id, fichier) VALUES (?1, ?2)",
                rusqlite::params![i as i64, format!("{i}.mp3")],
            )
            .unwrap();
        }
    }

    #[test]
    fn la_valise_qui_porte_le_plus_de_la_file_est_choisie() {
        let dossier =
            std::env::temp_dir().join(format!("rusty-valise-passage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        valise(&dossier, "ancienne", &["/m/a.mp3"]);
        valise(&dossier, "recente", &["/m/a.mp3", "/m/b.mp3", "/m/x.mp3"]);
        let passage = |file: &[&str]| Passage {
            version: VERSION,
            emis_le: 0,
            file: file.iter().map(|c| c.to_string()).collect(),
            avant_melange: Vec::new(),
            rang: 0,
            position_ms: 0,
            alea: false,
            repetition: "aucune".into(),
            volume: 1.0,
        };
        assert_eq!(
            valise_pour(&dossier, &passage(&["/m/a.mp3", "/m/b.mp3", "/m/c.mp3"])).as_deref(),
            Some("recente")
        );
        assert_eq!(valise_pour(&dossier, &passage(&["/m/z.mp3"])), None);
        std::fs::remove_dir_all(&dossier).unwrap();
    }
}
