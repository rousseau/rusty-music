// SPDX-License-Identifier: GPL-3.0-or-later
//! La valise par le Wi-Fi, par différence (étape 4B de
//! `docs/plan-ios-compagnon.md`). Le Mac tient une valise prête
//! (`rusty_music_valise::appairage::EnvoiValise`) ; l'app la prend au lancement
//! et à chaque retour au premier plan, sur son propre fil :
//!
//! 1. renvoie au Mac le journal d'écoutes de chaque valise du téléphone —
//!    sans quoi rien n'est remplacé ;
//! 2. monte la nouvelle valise dans `.<nom>.partielle`, en reprenant par lien
//!    dur les fichiers qu'elle porte déjà (même chemin d'origine, même
//!    taille) et en téléchargeant les autres ; le manifeste arrive en dernier ;
//! 3. la met à sa place, retire les anciennes (la valise ouverte attend, comme
//!    après un envoi par le câble) et accuse réception.
//!
//! iOS suspend l'app quelques secondes après sa sortie du premier plan : le
//! fil s'arrête sur une erreur réseau, et la prochaine ouverture reprend où il
//! en était (un fichier présent à la bonne taille n'est pas redemandé).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use rusty_music_valise::appairage::{self, EnvoiValise};
use rusty_music_valise::valise;
use serde::Serialize;
use tauri::Manager;

use crate::{echec, retirer_valises_remplacees, verrou, wifi, Etat};

/// Où en est la valise en route, sondé par l'interface.
#[derive(Clone, Default, Serialize)]
pub(crate) struct EtatSynchro {
    pub(crate) en_cours: bool,
    nom: Option<String>,
    faits: usize,
    total: usize,
    octets: u64,
    octets_total: u64,
    /// Fichiers repris de ce que le téléphone portait déjà.
    repris: usize,
    /// La phrase de fin : arrivée, ou pourquoi elle s'est arrêtée.
    issue: Option<String>,
    /// La valise est arrivée : la liste des valises a changé.
    arrivee: bool,
}

/// Lance la synchronisation si le Mac tient une valise prête ; `false` si une
/// tourne déjà ou s'il n'y a rien à faire (dit par [`EtatSynchro::issue`]).
pub(crate) fn lancer(app: &tauri::AppHandle) -> bool {
    let etat = app.state::<Etat>();
    {
        let mut s = verrou(&etat.synchro);
        if s.en_cours {
            return false;
        }
        *s = EtatSynchro {
            en_cours: true,
            ..Default::default()
        };
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let issue = synchroniser(&etat);
        let mut s = verrou(&etat.synchro);
        s.en_cours = false;
        match issue {
            Ok(Some(phrase)) => {
                s.arrivee = true;
                s.issue = Some(phrase);
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(erreur = %e, "valise interrompue");
                s.issue = Some(format!(
                    "Valise interrompue : {e}. Elle reprendra à la prochaine ouverture."
                ));
            }
        }
    });
    true
}

/// Les valises du téléphone (dossiers qui portent un manifeste).
fn valises(dossier: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dossier)
        .map(|l| {
            l.flatten()
                .map(|e| e.path())
                .filter(|p| p.join("valise.json").is_file())
                .collect()
        })
        .unwrap_or_default()
}

/// Chemin d'origine → (fichier sur le téléphone, taille), pour tout ce que
/// portent les valises présentes.
fn deja_la(dossier: &Path) -> HashMap<String, (PathBuf, u64)> {
    let mut index = HashMap::new();
    for racine in valises(dossier) {
        let Ok(conn) = Connection::open_with_flags(
            racine.join("base.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) else {
            continue;
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT t.path, f.fichier FROM tracks t JOIN valise_fichier f ON f.track_id = t.id",
        ) else {
            continue;
        };
        let Ok(lignes) =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        else {
            continue;
        };
        for (origine, rel) in lignes.flatten() {
            let fichier = racine.join(rel);
            if let Ok(m) = std::fs::metadata(&fichier) {
                index.insert(origine, (fichier, m.len()));
            }
        }
    }
    index
}

fn avancer(etat: &Etat, maj: impl FnOnce(&mut EtatSynchro)) {
    maj(&mut verrou(&etat.synchro));
}

/// La synchronisation elle-même ; `Ok(None)` quand le Mac n'a rien de prêt.
fn synchroniser(etat: &Etat) -> Result<Option<String>, String> {
    let Some((client, _)) = wifi::joindre(etat) else {
        return Ok(None);
    };
    let Some(envoi) = client.valise_en_attente().map_err(echec)? else {
        return Ok(None);
    };
    let EnvoiValise {
        nom,
        fichiers,
        annexes,
        ..
    } = envoi;
    if nom.is_empty() || nom.contains(['/', '\\']) || nom.starts_with('.') {
        return Err(format!("nom de valise invalide : « {nom} »"));
    }
    tracing::info!(valise = %nom, fichiers = fichiers.len(), "valise prête sur le Mac");
    avancer(etat, |s| {
        s.nom = Some(nom.clone());
        s.total = fichiers.len();
        s.octets_total = fichiers.iter().map(|f| f.octets).sum();
    });

    // 1. Les écoutes d'abord : une valise remplacée ne doit rien emporter.
    let anciennes = valises(&etat.dossier);
    for racine in &anciennes {
        if racine.join(valise::JOURNAL).is_file() {
            let journal = valise::lire_journal(racine).map_err(echec)?;
            client.envoyer_journal(&journal).map_err(echec)?;
        }
    }

    // 2. La nouvelle valise, à côté, fichier par fichier. Une valise partielle
    // d'un envoi remplacé depuis ne reprendra jamais : elle s'en va.
    let partielle = etat.dossier.join(format!(".{nom}.partielle"));
    for e in std::fs::read_dir(&etat.dossier).map_err(echec)?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with('.') && n.ends_with(".partielle") && e.path() != partielle {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    let index = deja_la(&etat.dossier);
    for f in &fichiers {
        if !appairage::chemin_sur(&f.rel) {
            return Err(format!("chemin refusé : {}", f.rel));
        }
        let dest = partielle.join(&f.rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(echec)?;
        }
        let present = std::fs::metadata(&dest).is_ok_and(|m| m.len() == f.octets);
        let mut repris = false;
        if !present {
            let _ = std::fs::remove_file(&dest);
            match index.get(&f.origine) {
                Some((ancien, taille)) if *taille == f.octets => {
                    // Lien dur : même volume, rien à copier, et l'ancienne
                    // valise peut partir sans emporter le fichier.
                    std::fs::hard_link(ancien, &dest)
                        .or_else(|_| std::fs::copy(ancien, &dest).map(|_| ()))
                        .map_err(echec)?;
                    repris = true;
                }
                _ => {
                    let n = client.telecharger(&f.rel, &dest).map_err(echec)?;
                    if n != f.octets {
                        let _ = std::fs::remove_file(&dest);
                        return Err(format!("{} : {n} octets reçus sur {}", f.rel, f.octets));
                    }
                }
            }
        }
        avancer(etat, |s| {
            s.faits += 1;
            s.octets += f.octets;
            s.repris += usize::from(repris);
        });
    }
    for rel in &annexes {
        if !appairage::chemin_sur(rel) {
            return Err(format!("chemin refusé : {rel}"));
        }
        let dest = partielle.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(echec)?;
        }
        client.telecharger(rel, &dest).map_err(echec)?;
    }

    // 3. À sa place ; les anciennes s'en vont (la valise ouverte attend).
    let finale = etat.dossier.join(&nom);
    let ouverte = verrou(&etat.valise).as_ref().map(|v| v.racine.clone());
    if finale.exists() {
        if ouverte.as_deref() == Some(finale.as_path()) {
            return Err(format!("« {nom} » est déjà ouverte ici"));
        }
        std::fs::remove_dir_all(&finale).map_err(echec)?;
    }
    std::fs::rename(&partielle, &finale).map_err(echec)?;
    let retirables: Vec<String> = anciennes
        .iter()
        .filter(|r| **r != finale)
        .filter_map(|r| r.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    if !retirables.is_empty() {
        std::fs::write(
            etat.dossier.join("a-retirer.json"),
            serde_json::to_vec(&retirables).map_err(echec)?,
        )
        .map_err(echec)?;
        retirer_valises_remplacees(&etat.dossier, ouverte.as_deref());
    }
    if let Err(e) = client.accuser_valise(&nom) {
        tracing::warn!(erreur = %e, "accusé de la valise perdu");
    }

    let s = verrou(&etat.synchro).clone();
    let telecharges = s.total - s.repris;
    tracing::info!(valise = %nom, telecharges, repris = s.repris, "valise arrivée par le Wi-Fi");
    Ok(Some(format!(
        "« {nom} » arrivée : {} morceaux, dont {} déjà sur l'iPhone.",
        s.total, s.repris
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une valise réduite à ce que [`deja_la`] lit.
    fn valise(dossier: &Path, nom: &str, pistes: &[(&str, &[u8])]) {
        let racine = dossier.join(nom);
        std::fs::create_dir_all(racine.join("audio")).unwrap();
        std::fs::write(racine.join("valise.json"), "{}").unwrap();
        let conn = Connection::open(racine.join("base.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks(id INTEGER PRIMARY KEY, path TEXT);
             CREATE TABLE valise_fichier(track_id INTEGER, fichier TEXT);",
        )
        .unwrap();
        for (i, (origine, contenu)) in pistes.iter().enumerate() {
            let rel = format!("audio/{i}.mp3");
            std::fs::write(racine.join(&rel), contenu).unwrap();
            conn.execute(
                "INSERT INTO tracks(id, path) VALUES (?1, ?2)",
                rusqlite::params![i as i64, origine],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO valise_fichier(track_id, fichier) VALUES (?1, ?2)",
                rusqlite::params![i as i64, rel],
            )
            .unwrap();
        }
    }

    #[test]
    fn ce_que_le_telephone_porte_se_retrouve_par_l_origine() {
        let dossier = std::env::temp_dir().join(format!("rusty-synchro-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        valise(
            &dossier,
            "ancienne",
            &[("/m/a.mp3", b"aaaa"), ("/m/b.mp3", b"bb")],
        );
        // Une valise partielle (sans manifeste) n'est pas une source.
        std::fs::create_dir_all(dossier.join(".neuve.partielle")).unwrap();
        let index = deja_la(&dossier);
        assert_eq!(index.len(), 2);
        let (fichier, taille) = &index["/m/a.mp3"];
        assert_eq!(*taille, 4);
        assert!(fichier.ends_with("ancienne/audio/0.mp3"));
        assert_eq!(valises(&dossier).len(), 1);
        std::fs::remove_dir_all(&dossier).unwrap();
    }
}
