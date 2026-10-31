// SPDX-License-Identifier: GPL-3.0-or-later
//! Compagnon iOS du mode Écouter — prototype (étape 0 de
//! `docs/plan-ios-compagnon.md`).
//!
//! Lit une **valise** déposée dans le dossier Documents de l'app (voir
//! `rusty_music_core::valise`) et la joue avec le même lecteur que le bureau.
//! La file porte les chemins **d'origine** des morceaux (leur identité dans
//! la base) ; `Player::set_resolveur` les traduit en fichiers de la valise.

mod en_cours;
mod session;
mod session_audio;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use rusqlite::params;
use rusty_music_core::loudness::{ModeGain, CIBLE_LUFS};
use rusty_music_core::memoire::SuiviEcoute;
use rusty_music_core::valise;
use rusty_music_core::Library;
use rusty_music_player::Player;
use serde::Serialize;
use tauri::{Manager, State};

/// Une valise ouverte : sa racine et sa base réduite.
struct Valise {
    racine: PathBuf,
    lib: Library,
}

pub(crate) struct Etat {
    /// Dossier où chercher les valises.
    dossier: PathBuf,
    valise: Mutex<Option<Valise>>,
    /// Ouvert au premier « lecture » : la session audio doit être réglée
    /// avant que `cpal` n'ouvre la sortie.
    player: Mutex<Option<Player>>,
    /// Chemin d'origine → fichier dans la valise, lu par le résolveur du
    /// lecteur (qui ne peut pas tenir le verrou de la valise).
    fichiers: Arc<RwLock<HashMap<PathBuf, PathBuf>>>,
    /// Chemin d'origine → gain de normalisation (album), calculé une fois à
    /// l'ouverture de la valise : le lecteur le demande pour toute piste
    /// ajoutée à la file, sans accès à la base.
    gains: Arc<RwLock<HashMap<PathBuf, f32>>>,
    /// Un seul préchargement à la fois — même raison que sur le bureau
    /// (`precharger_suivante` dans `apps/desktop/src/main.rs`).
    precharge: Mutex<()>,
}

fn verrou<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn echec(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct ValiseResume {
    nom: String,
    albums: i64,
    cree_le: Option<i64>,
}

#[derive(Serialize)]
struct AlbumVue {
    id: String,
    album: String,
    artiste: String,
    annee: Option<i64>,
    regle: String,
    pistes: i64,
}

#[derive(Serialize)]
struct Reprise {
    nom: String,
    albums: Vec<AlbumVue>,
}

#[derive(Serialize)]
struct PisteFile {
    titre: String,
    artiste: Option<String>,
    album_id: Option<String>,
    duree_ms: Option<i64>,
}

#[derive(Serialize)]
struct FileVue {
    pistes: Vec<PisteFile>,
    /// Rang de la piste en cours, `None` si la file est épuisée.
    rang: Option<usize>,
    /// Premier rang qu'on peut retirer : ceux d'avant sont déjà confiés à la
    /// sortie (`Player::verrou`).
    retirable: usize,
}

#[derive(Serialize)]
struct PisteVue {
    id: i64,
    titre: String,
    artiste: Option<String>,
    duree_ms: Option<i64>,
}

#[derive(Serialize, Clone, PartialEq)]
pub(crate) struct EtatLecture {
    pub titre: Option<String>,
    pub artiste: Option<String>,
    pub album: Option<String>,
    pub album_id: Option<String>,
    /// Rang dans la file : l'écran de la file suit la piste en cours.
    pub rang: usize,
    pub position_ms: u64,
    pub duree_ms: Option<u64>,
    pub pause: bool,
    /// Pochette de l'album dans la valise, pour « En cours de lecture ».
    #[serde(skip)]
    pub pochette: Option<PathBuf>,
}

/// Supprime les valises que le Mac a remplacées : il dépose leurs noms dans
/// `a-retirer.json` après avoir repris leurs écoutes (`devicectl` ne sait pas
/// supprimer un fichier sur l'iPhone). La valise ouverte attend le prochain
/// passage ; la liste est réécrite sans les noms traités.
fn retirer_valises_remplacees(dossier: &Path, ouverte: Option<&Path>) {
    let liste = dossier.join("a-retirer.json");
    let Some(noms) = std::fs::read(&liste)
        .ok()
        .and_then(|o| serde_json::from_slice::<Vec<String>>(&o).ok())
    else {
        return;
    };
    let mut restants = Vec::new();
    for nom in noms {
        let racine = dossier.join(&nom);
        // Un nom simple, et un dossier qui est bien une valise.
        if nom.contains(['/', '\\'])
            || nom.starts_with('.')
            || !racine.join("valise.json").is_file()
        {
            continue;
        }
        if ouverte == Some(racine.as_path()) {
            restants.push(nom);
            continue;
        }
        match std::fs::remove_dir_all(&racine) {
            Ok(()) => tracing::info!(valise = %nom, "valise remplacée, retirée"),
            Err(e) => tracing::warn!(valise = %nom, "retrait impossible : {e}"),
        }
    }
    let _ = if restants.is_empty() {
        std::fs::remove_file(&liste)
    } else {
        std::fs::write(&liste, serde_json::to_vec(&restants).unwrap_or_default())
    };
}

/// Les valises du dossier : chaque sous-dossier qui porte un `valise.json`.
#[tauri::command]
fn valises(etat: State<Etat>) -> Result<Vec<ValiseResume>, String> {
    {
        let ouverte = verrou(&etat.valise);
        retirer_valises_remplacees(&etat.dossier, ouverte.as_ref().map(|v| v.racine.as_path()));
    }
    let mut out = Vec::new();
    let Ok(entrees) = std::fs::read_dir(&etat.dossier) else {
        return Ok(out);
    };
    for entree in entrees.flatten() {
        let racine = entree.path();
        if !racine.join("valise.json").is_file() {
            continue;
        }
        let manifeste: serde_json::Value = std::fs::read(racine.join("valise.json"))
            .ok()
            .and_then(|o| serde_json::from_slice(&o).ok())
            .unwrap_or_default();
        out.push(ValiseResume {
            nom: entree.file_name().to_string_lossy().into_owned(),
            albums: manifeste["albums"].as_array().map_or(0, |a| a.len() as i64),
            cree_le: manifeste["cree_le"].as_i64(),
        });
    }
    out.sort_by_key(|v| std::cmp::Reverse(v.cree_le));
    Ok(out)
}

/// Ouvre une valise et rend ses albums, dans l'ordre de la valise.
#[tauri::command]
fn ouvrir_valise(etat: State<Etat>, nom: String) -> Result<Vec<AlbumVue>, String> {
    ouvrir(&etat, &nom)
}

fn ouvrir(etat: &Etat, nom: &str) -> Result<Vec<AlbumVue>, String> {
    let racine = etat.dossier.join(nom);
    let lib = Library::open(&racine.join("base.sqlite")).map_err(echec)?;

    let albums = {
        let mut stmt = lib
            .conn
            .prepare(
                "SELECT a.id, a.album, a.artiste, MIN(t.year), a.regle, COUNT(f.track_id)
                   FROM valise_album a
                   JOIN valise_fichier f ON f.album_id = a.id
                   JOIN tracks t ON t.id = f.track_id
                  GROUP BY a.id ORDER BY a.rang",
            )
            .map_err(echec)?;
        let lignes = stmt
            .query_map([], |r| {
                Ok(AlbumVue {
                    id: r.get(0)?,
                    album: r.get(1)?,
                    artiste: r.get(2)?,
                    annee: r.get(3)?,
                    regle: r.get(4)?,
                    pistes: r.get(5)?,
                })
            })
            .map_err(echec)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(echec)?;
        lignes
    };

    let fichiers = {
        let mut stmt = lib
            .conn
            .prepare(
                "SELECT t.path, f.fichier FROM tracks t JOIN valise_fichier f ON f.track_id = t.id",
            )
            .map_err(echec)?;
        let paires = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(echec)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(echec)?;
        paires
            .into_iter()
            .map(|(origine, fichier)| (PathBuf::from(origine), racine.join(fichier)))
            .collect::<HashMap<_, _>>()
    };
    let gains = fichiers
        .keys()
        .filter_map(|p| {
            let g = lib
                .gain_lecture(p, ModeGain::Album, CIBLE_LUFS)
                .ok()
                .flatten()?;
            Some((p.clone(), g))
        })
        .collect();
    *etat.fichiers.write().unwrap_or_else(|e| e.into_inner()) = fichiers;
    *etat.gains.write().unwrap_or_else(|e| e.into_inner()) = gains;
    *verrou(&etat.valise) = Some(Valise { racine, lib });
    Ok(albums)
}

/// Au lancement : rouvre la valise de la dernière session et reprend sa file
/// en pause. `None` s'il n'y a rien à reprendre.
#[tauri::command]
fn reprendre(etat: State<Etat>) -> Result<Option<Reprise>, String> {
    retirer_valises_remplacees(&etat.dossier, None);
    let Some(nom) = session::valise_la_plus_recente(&etat.dossier) else {
        return Ok(None);
    };
    let albums = ouvrir(&etat, &nom)?;
    if let Err(e) = session::restaurer(&etat) {
        tracing::warn!(erreur = %e, "reprise de la session impossible");
    }
    en_cours::afficher(lecture_courante(&etat).as_ref());
    Ok(Some(Reprise { nom, albums }))
}

/// Les pistes d'un album de la valise, dans l'ordre du disque (les fichiers
/// sont numérotés à l'export).
fn pistes_album(lib: &Library, album_id: &str) -> rusqlite::Result<Vec<(PisteVue, PathBuf)>> {
    let mut stmt = lib.conn.prepare(
        "SELECT t.id, COALESCE(t.title, f.fichier), t.artist, t.duration_ms, t.path
           FROM valise_fichier f JOIN tracks t ON t.id = f.track_id
          WHERE f.album_id = ?1 ORDER BY f.fichier",
    )?;
    let lignes = stmt
        .query_map(params![album_id], |r| {
            Ok((
                PisteVue {
                    id: r.get(0)?,
                    titre: r.get(1)?,
                    artiste: r.get(2)?,
                    duree_ms: r.get(3)?,
                },
                PathBuf::from(r.get::<_, String>(4)?),
            ))
        })?
        .collect();
    lignes
}

#[tauri::command]
fn pistes(etat: State<Etat>, album_id: String) -> Result<Vec<PisteVue>, String> {
    let valise = verrou(&etat.valise);
    let v = valise.as_ref().ok_or("aucune valise ouverte")?;
    Ok(pistes_album(&v.lib, &album_id)
        .map_err(echec)?
        .into_iter()
        .map(|(p, _)| p)
        .collect())
}

/// La pochette d'un album, en URL `data:` (JPEG écrit par l'export).
#[tauri::command]
fn pochette(etat: State<Etat>, album_id: String) -> Result<Option<String>, String> {
    let valise = verrou(&etat.valise);
    let v = valise.as_ref().ok_or("aucune valise ouverte")?;
    let chemin: Option<String> = v
        .lib
        .conn
        .query_row(
            "SELECT pochette FROM valise_album WHERE id = ?1",
            params![album_id],
            |r| r.get(0),
        )
        .map_err(echec)?;
    Ok(chemin
        .and_then(|c| std::fs::read(v.racine.join(c)).ok())
        .map(|o| format!("data:image/jpeg;base64,{}", base64(&o))))
}

/// Ouvre la sortie audio au premier besoin, résolveur et gain branchés.
pub(crate) fn lecteur(etat: &Etat) -> Result<MutexGuard<'_, Option<Player>>, String> {
    let mut garde = verrou(&etat.player);
    if garde.is_none() {
        let mut player = Player::new().map_err(echec)?;
        let fichiers = etat.fichiers.clone();
        player.set_resolveur(move |p| {
            fichiers
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(p)
                .cloned()
                .unwrap_or_else(|| p.to_path_buf())
        });
        let gains = etat.gains.clone();
        player.set_gain_resolveur(move |p| {
            gains
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(p)
                .copied()
                .unwrap_or(1.0)
        });
        *garde = Some(player);
    }
    Ok(garde)
}

/// Gain de normalisation (album, comme le bureau par défaut) — 1.0 si la
/// piste n'a pas été mesurée sur le Mac.
fn gain_pour(etat: &Etat, chemin: &Path) -> f32 {
    etat.gains
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(chemin)
        .copied()
        .unwrap_or(1.0)
}

/// Les chemins d'origine des pistes d'un album, dans l'ordre du disque.
fn chemins_album(etat: &Etat, album_id: &str) -> Result<Vec<PathBuf>, String> {
    let valise = verrou(&etat.valise);
    let v = valise.as_ref().ok_or("aucune valise ouverte")?;
    let file: Vec<PathBuf> = pistes_album(&v.lib, album_id)
        .map_err(echec)?
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    if file.is_empty() {
        return Err("album vide".into());
    }
    Ok(file)
}

/// Joue l'album `album_id` à partir de la piste de rang `rang`.
#[tauri::command]
fn jouer(etat: State<Etat>, album_id: String, rang: usize) -> Result<(), String> {
    let file = chemins_album(&etat, &album_id)?;
    let mut garde = lecteur(&etat)?;
    let player = garde.as_mut().expect("ouvert par `lecteur`");
    player.play(&file).map_err(echec)?;
    if rang > 0 {
        player.jump_to(rang.min(file.len() - 1)).map_err(echec)?;
    }
    Ok(())
}

/// Transport, commun à l'interface et aux commandes à distance (écran
/// verrouillé, centre de contrôle, AirPods — voir `en_cours`).
pub(crate) enum Geste {
    /// `None` bascule ; `Some(true)` met en pause, `Some(false)` relance.
    Pause(Option<bool>),
    Suivante,
    Precedente,
    AllerA(Duration),
}

pub(crate) fn transport(etat: &Etat, geste: Geste) -> Result<(), String> {
    let mut garde = verrou(&etat.player);
    let Some(player) = garde.as_mut() else {
        return Ok(());
    };
    match geste {
        Geste::Pause(voulu) => {
            if voulu.unwrap_or(!player.is_paused()) {
                player.pause();
            } else {
                player.resume();
            }
        }
        Geste::Suivante => player.skip(),
        Geste::Precedente => player.previous().map_err(echec)?,
        Geste::AllerA(position) => player.seek(position).map_err(echec)?,
    }
    drop(garde);
    // Le geste se voit tout de suite sur l'écran verrouillé, sans attendre
    // le battement suivant.
    en_cours::afficher(lecture_courante(etat).as_ref());
    Ok(())
}

#[tauri::command]
fn basculer_pause(etat: State<Etat>) -> Result<(), String> {
    transport(&etat, Geste::Pause(None))
}

#[tauri::command]
fn suivante(etat: State<Etat>) -> Result<(), String> {
    transport(&etat, Geste::Suivante)
}

#[tauri::command]
fn precedente(etat: State<Etat>) -> Result<(), String> {
    transport(&etat, Geste::Precedente)
}

#[tauri::command]
fn aller_a(etat: State<Etat>, position_ms: u64) -> Result<(), String> {
    transport(&etat, Geste::AllerA(Duration::from_millis(position_ms)))
}

/// Met l'album à la suite de la file — ou le lance si rien ne joue.
#[tauri::command]
fn ajouter_album(etat: State<Etat>, album_id: String) -> Result<(), String> {
    let album = chemins_album(&etat, &album_id)?;
    let mut garde = lecteur(&etat)?;
    let player = garde.as_mut().expect("ouvert par `lecteur`");
    if player.is_finished() || player.rang_courant().is_none() {
        return player.play(&album).map_err(echec);
    }
    // `set_queue` garde la piste en cours et ce qui est déjà confié à la
    // sortie : la file commence par les mêmes pistes, rien n'est coupé.
    let file: Vec<PathBuf> = player.queue().iter().cloned().chain(album).collect();
    player.set_queue(&file).map_err(echec)
}

/// La file du lecteur, avec ce qu'il faut pour l'afficher.
#[tauri::command]
fn file_lecture(etat: State<Etat>) -> Result<FileVue, String> {
    let (file, rang, retirable) = {
        let garde = verrou(&etat.player);
        let Some(p) = garde.as_ref() else {
            return Ok(FileVue {
                pistes: Vec::new(),
                rang: None,
                retirable: 0,
            });
        };
        let rang = if p.is_finished() {
            None
        } else {
            p.rang_courant()
        };
        (p.queue().to_vec(), rang, p.verrou())
    };
    let valise = verrou(&etat.valise);
    let v = valise.as_ref().ok_or("aucune valise ouverte")?;
    let mut stmt = v
        .lib
        .conn
        .prepare(
            "SELECT t.title, t.artist, t.duration_ms, f.album_id
               FROM tracks t LEFT JOIN valise_fichier f ON f.track_id = t.id
              WHERE t.path = ?1",
        )
        .map_err(echec)?;
    let pistes = file
        .iter()
        .map(|chemin| {
            stmt.query_row(params![chemin.to_string_lossy()], |r| {
                Ok(PisteFile {
                    titre: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    artiste: r.get(1)?,
                    duree_ms: r.get(2)?,
                    album_id: r.get(3)?,
                })
            })
            .unwrap_or_else(|_| PisteFile {
                titre: chemin
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                artiste: None,
                duree_ms: None,
                album_id: None,
            })
        })
        .collect();
    Ok(FileVue {
        pistes,
        rang,
        retirable,
    })
}

/// Saute à la piste de rang `rang` de la file.
#[tauri::command]
fn sauter_a(etat: State<Etat>, rang: usize) -> Result<(), String> {
    let mut garde = verrou(&etat.player);
    let Some(player) = garde.as_mut() else {
        return Ok(());
    };
    player.jump_to(rang).map_err(echec)
}

/// Retire de la file la piste de rang `rang`, si elle n'est pas déjà confiée
/// à la sortie (la piste en cours et celle préchargée ne se retirent pas).
#[tauri::command]
fn retirer_de_la_file(etat: State<Etat>, rang: usize) -> Result<(), String> {
    let mut garde = verrou(&etat.player);
    let Some(player) = garde.as_mut() else {
        return Ok(());
    };
    if rang < player.verrou() || rang >= player.queue().len() {
        return Err("cette piste ne peut plus être retirée".into());
    }
    let mut file = player.queue().to_vec();
    file.remove(rang);
    player.set_queue(&file).map_err(echec)
}

/// Ce qui joue, avec ses métadonnées ; `None` si rien ne joue.
pub(crate) fn lecture_courante(etat: &Etat) -> Option<EtatLecture> {
    let (chemin, rang, position, duree, pause) = {
        let garde = verrou(&etat.player);
        let player = garde.as_ref()?;
        if player.is_finished() {
            return None;
        }
        (
            player.current()?.to_path_buf(),
            player.rang_courant()?,
            player.position(),
            player.duree_courante(),
            player.is_paused(),
        )
    };
    let valise = verrou(&etat.valise);
    let v = valise.as_ref()?;
    let (titre, artiste, album, album_id, pochette) = v
        .lib
        .conn
        .query_row(
            "SELECT t.title, t.artist, t.album, f.album_id, a.pochette
               FROM tracks t LEFT JOIN valise_fichier f ON f.track_id = t.id
                             LEFT JOIN valise_album a ON a.id = f.album_id
              WHERE t.path = ?1",
            params![chemin.to_string_lossy()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .ok()?;
    let pochette = pochette.map(|rel| v.racine.join(rel));
    Some(EtatLecture {
        titre,
        artiste,
        album,
        album_id,
        rang,
        position_ms: position.as_millis() as u64,
        duree_ms: duree.map(|d| d.as_millis() as u64),
        pause,
        pochette,
    })
}

#[tauri::command]
fn etat_lecture(etat: State<Etat>) -> Option<EtatLecture> {
    lecture_courante(&etat)
}

/// Mémoire d'écoute, au même seuil que le bureau ([`SuiviEcoute`]) : une
/// écoute est notée dans la base de la valise (chemin d'origine), puis
/// `journal.json` est réécrit pour que le Mac la reprenne
/// (`rusty-music valise importer`).
fn suivre_ecoute(etat: &Etat, s: &mut SuiviEcoute) {
    let ecoule = s.battement();
    let (courant, en_pause, position) = {
        let garde = verrou(&etat.player);
        match garde.as_ref() {
            Some(p) if !p.is_finished() => (
                p.current().map(Path::to_path_buf),
                p.is_paused(),
                p.position(),
            ),
            _ => (None, false, Duration::ZERO),
        }
    };
    let valise = verrou(&etat.valise);
    let Some(v) = valise.as_ref() else {
        return;
    };
    let ecoute = s.avancer(courant, en_pause, position, ecoule, |c| {
        v.lib
            .pistes_par_chemins(&[c.to_string_lossy().into_owned()])
            .ok()
            .and_then(|t| t.first().and_then(|t| t.duration_ms))
    });
    if let Some(chemin) = ecoute {
        let note = v
            .lib
            .noter_ecoute(&chemin.to_string_lossy())
            .and_then(|()| valise::ecrire_journal(&v.lib, &v.racine));
        if let Err(e) = note {
            tracing::warn!(erreur = %e, "écoute non enregistrée");
        }
    }
}

/// Précharge la piste suivante — version courte de `precharger_suivante` du
/// bureau : le verrou du lecteur n'est pas tenu pendant l'ouverture du
/// fichier.
fn precharger_suivante(etat: &Etat) {
    let Ok(_seul) = etat.precharge.try_lock() else {
        return;
    };
    let a_charger = {
        let mut garde = verrou(&etat.player);
        let Some(player) = garde.as_mut() else {
            return;
        };
        let epoque = player.epoque();
        player.a_precharger().map(|(rang, piste)| {
            let original = player
                .queue()
                .get(rang)
                .cloned()
                .unwrap_or_else(|| piste.clone());
            (epoque, rang, piste, original)
        })
    };
    let Some((epoque, rang, piste, original)) = a_charger else {
        return;
    };
    let gain = gain_pour(etat, &original);
    match rusty_music_player::ouvrir(&piste, gain) {
        Ok(source) => {
            if let Some(player) = verrou(&etat.player).as_mut() {
                player.charger_precharge_si(epoque, rang, source);
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "préparation de la piste suivante impossible");
            if let Some(player) = verrou(&etat.player).as_mut() {
                if player.epoque() == epoque {
                    player.signaler_echec(rang, &e.to_string());
                }
            }
        }
    }
}

/// Encodage base64 sans dépendance, comme sur le bureau.
fn base64(octets: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(octets.len().div_ceil(3) * 4);
    for bloc in octets.chunks(3) {
        let b = [
            bloc[0],
            *bloc.get(1).unwrap_or(&0),
            *bloc.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if bloc.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if bloc.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

/// Le dossier des valises : `$RUSTY_VALISES` s'il est posé, sinon le dossier
/// Documents de l'app (sur iOS, celui que le Finder montre).
fn dossier_valises(app: &tauri::App) -> PathBuf {
    if let Some(d) = std::env::var_os("RUSTY_VALISES") {
        return PathBuf::from(d);
    }
    app.path()
        .document_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
                .add_directive("symphonia=warn".parse().expect("directive valide")),
        )
        .without_time()
        .init();

    session_audio::configurer();

    tauri::Builder::default()
        .setup(|app| {
            let dossier = dossier_valises(app);
            tracing::info!(dossier = %dossier.display(), "dossier des valises");
            app.manage(Etat {
                dossier,
                valise: Mutex::new(None),
                player: Mutex::new(None),
                fichiers: Arc::new(RwLock::new(HashMap::new())),
                gains: Arc::new(RwLock::new(HashMap::new())),
                precharge: Mutex::new(()),
            });
            // Le préchargement tourne côté natif : les minuteries JS d'une
            // webview en arrière-plan sont suspendues, et l'enchaînement des
            // pistes avec elles (même piège que sur le bureau).
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut affiche = en_cours::Suivi::default();
                let mut ecoute = SuiviEcoute::nouveau();
                let mut memo = session::Memo::default();
                let mut tour = 0u32;
                loop {
                    tour = tour.wrapping_add(1);
                    std::thread::sleep(Duration::from_millis(250));
                    let etat = handle.state::<Etat>();
                    precharger_suivante(&etat);
                    suivre_ecoute(&etat, &mut ecoute);
                    affiche.battement(lecture_courante(&etat).as_ref());
                    // Session toutes les cinq secondes, comme le bureau : iOS
                    // peut fermer l'app suspendue sans prévenir.
                    if tour % 20 == 0 {
                        if let Err(e) = session::sauvegarder(&etat, &mut memo) {
                            tracing::warn!(erreur = %e, "sauvegarde de la session impossible");
                        }
                    }
                }
            });
            en_cours::brancher_commandes(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            valises,
            ouvrir_valise,
            pistes,
            pochette,
            reprendre,
            jouer,
            ajouter_album,
            file_lecture,
            sauter_a,
            retirer_de_la_file,
            basculer_pause,
            suivante,
            precedente,
            aller_a,
            etat_lecture,
        ])
        .run(tauri::generate_context!())
        .expect("échec du lancement de l'application");
}
