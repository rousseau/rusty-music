// SPDX-License-Identifier: GPL-3.0-or-later
//! Compagnon iOS du mode Écouter — prototype (étape 0 de
//! `docs/plan-ios-compagnon.md`).
//!
//! Lit une **valise** déposée dans le dossier Documents de l'app (voir
//! `rusty_music_core::valise`) et la joue avec le même lecteur que le bureau.
//! La file porte les chemins **d'origine** des morceaux (leur identité dans
//! la base) ; `Player::set_resolveur` les traduit en fichiers de la valise.

mod session_audio;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use rusqlite::params;
use rusty_music_core::loudness::{ModeGain, CIBLE_LUFS};
use rusty_music_core::Library;
use rusty_music_player::Player;
use serde::Serialize;
use tauri::{Manager, State};

/// Une valise ouverte : sa racine et sa base réduite.
struct Valise {
    racine: PathBuf,
    lib: Library,
}

struct Etat {
    /// Dossier où chercher les valises.
    dossier: PathBuf,
    valise: Mutex<Option<Valise>>,
    /// Ouvert au premier « lecture » : la session audio doit être réglée
    /// avant que `cpal` n'ouvre la sortie.
    player: Mutex<Option<Player>>,
    /// Chemin d'origine → fichier dans la valise, lu par le résolveur du
    /// lecteur (qui ne peut pas tenir le verrou de la valise).
    fichiers: Arc<RwLock<HashMap<PathBuf, PathBuf>>>,
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
struct PisteVue {
    id: i64,
    titre: String,
    artiste: Option<String>,
    duree_ms: Option<i64>,
}

#[derive(Serialize)]
struct EtatLecture {
    titre: Option<String>,
    artiste: Option<String>,
    album: Option<String>,
    album_id: Option<String>,
    position_ms: u64,
    duree_ms: Option<u64>,
    pause: bool,
}

/// Les valises du dossier : chaque sous-dossier qui porte un `valise.json`.
#[tauri::command]
fn valises(etat: State<Etat>) -> Result<Vec<ValiseResume>, String> {
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
    let racine = etat.dossier.join(&nom);
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
            .prepare("SELECT t.path, f.fichier FROM tracks t JOIN valise_fichier f ON f.track_id = t.id")
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
    *etat.fichiers.write().unwrap_or_else(|e| e.into_inner()) = fichiers;
    *verrou(&etat.valise) = Some(Valise { racine, lib });
    Ok(albums)
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
fn lecteur<'a>(etat: &'a Etat) -> Result<MutexGuard<'a, Option<Player>>, String> {
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
        *garde = Some(player);
    }
    Ok(garde)
}

/// Gain de normalisation (album, comme le bureau par défaut) — 1.0 si la
/// piste n'a pas été mesurée sur le Mac.
fn gain_pour(etat: &Etat, chemin: &Path) -> f32 {
    verrou(&etat.valise)
        .as_ref()
        .and_then(|v| v.lib.gain_lecture(chemin, ModeGain::Album, CIBLE_LUFS).ok().flatten())
        .unwrap_or(1.0)
}

/// Joue l'album `album_id` à partir de la piste de rang `rang`.
#[tauri::command]
fn jouer(etat: State<Etat>, album_id: String, rang: usize) -> Result<(), String> {
    let file: Vec<PathBuf> = {
        let valise = verrou(&etat.valise);
        let v = valise.as_ref().ok_or("aucune valise ouverte")?;
        pistes_album(&v.lib, &album_id)
            .map_err(echec)?
            .into_iter()
            .map(|(_, p)| p)
            .collect()
    };
    if file.is_empty() {
        return Err("album vide".into());
    }
    let mut garde = lecteur(&etat)?;
    let player = garde.as_mut().expect("ouvert par `lecteur`");
    let gains = file.iter().map(|p| gain_pour(&etat, p)).collect::<Vec<_>>();
    let gains_par_chemin: HashMap<PathBuf, f32> = file.iter().cloned().zip(gains).collect();
    player.set_gain_resolveur(move |p| gains_par_chemin.get(p).copied().unwrap_or(1.0));
    player.play(&file).map_err(echec)?;
    if rang > 0 {
        player.jump_to(rang.min(file.len() - 1)).map_err(echec)?;
    }
    Ok(())
}

#[tauri::command]
fn basculer_pause(etat: State<Etat>) -> Result<bool, String> {
    let garde = verrou(&etat.player);
    let Some(player) = garde.as_ref() else {
        return Ok(true);
    };
    if player.is_paused() {
        player.resume();
    } else {
        player.pause();
    }
    Ok(player.is_paused())
}

#[tauri::command]
fn suivante(etat: State<Etat>) {
    if let Some(player) = verrou(&etat.player).as_ref() {
        player.skip();
    }
}

#[tauri::command]
fn precedente(etat: State<Etat>) -> Result<(), String> {
    match verrou(&etat.player).as_mut() {
        Some(player) => player.previous().map_err(echec),
        None => Ok(()),
    }
}

#[tauri::command]
fn aller_a(etat: State<Etat>, position_ms: u64) -> Result<(), String> {
    match verrou(&etat.player).as_mut() {
        Some(player) => player.seek(Duration::from_millis(position_ms)).map_err(echec),
        None => Ok(()),
    }
}

#[tauri::command]
fn etat_lecture(etat: State<Etat>) -> Option<EtatLecture> {
    let (chemin, position, duree, pause) = {
        let garde = verrou(&etat.player);
        let player = garde.as_ref()?;
        if player.is_finished() {
            return None;
        }
        (
            player.current()?.to_path_buf(),
            player.position(),
            player.duree_courante(),
            player.is_paused(),
        )
    };
    let valise = verrou(&etat.valise);
    let v = valise.as_ref()?;
    let (titre, artiste, album, album_id) = v
        .lib
        .conn
        .query_row(
            "SELECT t.title, t.artist, t.album, f.album_id
               FROM tracks t LEFT JOIN valise_fichier f ON f.track_id = t.id
              WHERE t.path = ?1",
            params![chemin.to_string_lossy()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()?;
    Some(EtatLecture {
        titre,
        artiste,
        album,
        album_id,
        position_ms: position.as_millis() as u64,
        duree_ms: duree.map(|d| d.as_millis() as u64),
        pause,
    })
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
            let original = player.queue().get(rang).cloned().unwrap_or_else(|| piste.clone());
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
        let b = [bloc[0], *bloc.get(1).unwrap_or(&0), *bloc.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if bloc.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if bloc.len() > 2 { T[n as usize & 63] as char } else { '=' });
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
                precharge: Mutex::new(()),
            });
            // Le préchargement tourne côté natif : les minuteries JS d'une
            // webview en arrière-plan sont suspendues, et l'enchaînement des
            // pistes avec elles (même piège que sur le bureau).
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(250));
                precharger_suivante(&handle.state::<Etat>());
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            valises,
            ouvrir_valise,
            pistes,
            pochette,
            jouer,
            basculer_pause,
            suivante,
            precedente,
            aller_a,
            etat_lecture,
        ])
        .run(tauri::generate_context!())
        .expect("échec du lancement de l'application");
}
