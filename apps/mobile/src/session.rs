// SPDX-License-Identifier: GPL-3.0-or-later
//! Reprise de session (étape 5 de `docs/plan-ios-compagnon.md`) : la file et
//! la position, dans la table `session` de la **base de la valise** ouverte —
//! le même mécanisme que le bureau (`restaurer_session` /
//! `sauvegarder_session` dans `apps/desktop/src/main.rs`).
//!
//! Au lancement, la valise rouverte est celle dont la session est la plus
//! récente ; la file revient **en pause** (jamais de son surprise). Une valise
//! remplacée par le Mac emporte sa session avec elle : la nouvelle repart de
//! zéro, ses pistes n'étant pas forcément les mêmes.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use rusty_music_core::session::{retenir, Session};
use rusty_music_player::{Instantane, Repetition};

use crate::{echec, lecteur, verrou, Etat};

/// La valise du dossier dont la session est la plus récente. Les bases sont
/// ouvertes en lecture seule : une valise sans table `session` (exportée
/// avant elle, ou jamais jouée) est simplement ignorée.
pub(crate) fn valise_la_plus_recente(dossier: &Path) -> Option<String> {
    let entrees = std::fs::read_dir(dossier).ok()?;
    entrees
        .flatten()
        .filter_map(|e| {
            let base = e.path().join("base.sqlite");
            if !e.path().join("valise.json").is_file() || !base.is_file() {
                return None;
            }
            let conn = Connection::open_with_flags(&base, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
            let quand: i64 = conn
                .query_row("SELECT enregistree_le FROM session WHERE id = 1", [], |r| r.get(0))
                .ok()?;
            Some((quand, e.file_name().to_string_lossy().into_owned()))
        })
        .max()
        .map(|(_, nom)| nom)
}

/// Ce que [`sauvegarder`] a déjà écrit : la file n'est réécrite que si elle
/// change, et rien ne l'est si rien n'a bougé (pause prolongée).
#[derive(Default)]
pub(crate) struct Memo {
    empreinte_file: Option<u64>,
    derniere: Option<(usize, u64)>,
}

/// Retient la lecture en cours dans la base de la valise. Sans effet quand
/// rien ne joue : on ne remplace pas une session valable par du vide.
pub(crate) fn sauvegarder(etat: &Etat, memo: &mut Memo) -> Result<(), String> {
    let Some(i) = verrou(&etat.player).as_ref().and_then(|p| p.instantane()) else {
        return Ok(());
    };
    let valise = verrou(&etat.valise);
    let Some(v) = valise.as_ref() else {
        return Ok(());
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.racine.hash(&mut h);
    i.file.hash(&mut h);
    i.avant_melange.hash(&mut h);
    let empreinte = h.finish();
    let position_ms = i.position.as_millis() as u64;
    if memo.empreinte_file == Some(empreinte) && memo.derniere == Some((i.rang, position_ms)) {
        return Ok(());
    }
    let repetition = i.repetition.nom();

    if memo.empreinte_file == Some(empreinte)
        && v
            .lib
            .enregistrer_position(i.rang, position_ms, i.alea, repetition, i.volume)
            .map_err(echec)?
    {
        memo.derniere = Some((i.rang, position_ms));
        return Ok(());
    }

    let chemins = |f: &[PathBuf]| -> Vec<String> {
        f.iter().map(|p| p.to_string_lossy().into_owned()).collect()
    };
    let ids = v.lib.ids_par_chemins(&chemins(&i.file)).map_err(echec)?;
    let (file, rang, _) = retenir(ids, i.rang);
    if file.is_empty() {
        return Ok(());
    }
    let avant_melange = v
        .lib
        .ids_par_chemins(&chemins(&i.avant_melange))
        .map_err(echec)?
        .into_iter()
        .flatten()
        .collect();
    v.lib
        .enregistrer_session(&Session {
            file,
            avant_melange,
            rang,
            position_ms,
            alea: i.alea,
            repetition: repetition.to_string(),
            volume: i.volume,
        })
        .map_err(echec)?;
    memo.empreinte_file = Some(empreinte);
    memo.derniere = Some((i.rang, position_ms));
    Ok(())
}

/// Reprend la session de la valise ouverte, en pause. Rend `true` quand une
/// file a bien été reprise ; `Player::restaurer` s'efface si une lecture a
/// déjà été lancée.
pub(crate) fn restaurer(etat: &Etat) -> Result<bool, String> {
    let (session, chemins, anciens) = {
        let valise = verrou(&etat.valise);
        let Some(v) = valise.as_ref() else {
            return Ok(false);
        };
        let Some(s) = v.lib.session().map_err(echec)? else {
            return Ok(false);
        };
        let chemins = v.lib.chemins_par_ids(&s.file).map_err(echec)?;
        let anciens = v.lib.chemins_par_ids(&s.avant_melange).map_err(echec)?;
        (s, chemins, anciens)
    };
    let (chemins, rang, courante_presente) = retenir(chemins, session.rang);
    if chemins.is_empty() {
        return Ok(false);
    }
    let mut garde = lecteur(etat)?;
    let player = garde.as_mut().expect("ouvert par `lecteur`");
    player
        .restaurer(Instantane {
            file: chemins.into_iter().map(PathBuf::from).collect(),
            avant_melange: anciens.into_iter().flatten().map(PathBuf::from).collect(),
            rang,
            position: if courante_presente {
                Duration::from_millis(session.position_ms)
            } else {
                Duration::ZERO
            },
            alea: session.alea,
            repetition: Repetition::depuis_nom(&session.repetition),
            volume: session.volume,
        })
        .map_err(echec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, RwLock};

    fn etat(dossier: &Path) -> Etat {
        Etat {
            dossier: dossier.to_path_buf(),
            valise: Mutex::new(None),
            player: Mutex::new(None),
            fichiers: Arc::new(RwLock::new(HashMap::new())),
            gains: Arc::new(RwLock::new(HashMap::new())),
            precharge: Mutex::new(()),
        }
    }

    /// De bout en bout sur une vraie valise et la sortie audio du Mac :
    /// `RUSTY_VALISE_ESSAI=<dossier des valises> cargo test -p
    /// rusty-music-mobile -- --ignored`. La valise y est modifiée (session).
    #[test]
    #[ignore = "valise réelle et sortie audio"]
    fn la_file_revient_en_pause_a_sa_position() {
        let dossier = PathBuf::from(std::env::var_os("RUSTY_VALISE_ESSAI").expect("RUSTY_VALISE_ESSAI"));
        let nom = std::fs::read_dir(&dossier)
            .unwrap()
            .flatten()
            .find(|e| e.path().join("valise.json").is_file())
            .expect("une valise")
            .file_name()
            .to_string_lossy()
            .into_owned();

        let a = etat(&dossier);
        let albums = crate::ouvrir(&a, &nom).unwrap();
        assert!(albums.len() >= 2, "deux albums au moins");
        let premier = crate::chemins_album(&a, &albums[0].id).unwrap();
        let second = crate::chemins_album(&a, &albums[1].id).unwrap();
        {
            let mut g = lecteur(&a).unwrap();
            let p = g.as_mut().unwrap();
            p.play(&premier).unwrap();
            // « À la suite », comme `ajouter_album`.
            let file: Vec<PathBuf> = p.queue().iter().cloned().chain(second.clone()).collect();
            p.set_queue(&file).unwrap();
            p.jump_to(1).unwrap();
            p.seek(Duration::from_secs(12)).unwrap();
            p.pause();
        }
        let mut memo = Memo::default();
        sauvegarder(&a, &mut memo).unwrap();
        drop(a);

        assert_eq!(valise_la_plus_recente(&dossier).as_deref(), Some(nom.as_str()));
        let b = etat(&dossier);
        crate::ouvrir(&b, &nom).unwrap();
        assert!(restaurer(&b).unwrap());
        let g = verrou(&b.player);
        let p = g.as_ref().unwrap();
        assert_eq!(p.queue().len(), premier.len() + second.len());
        assert_eq!(p.rang_courant(), Some(1));
        assert!(p.is_paused(), "jamais de son au lancement");
        let pos = p.position().as_secs_f32();
        assert!((11.5..13.0).contains(&pos), "position {pos}");
    }
}
