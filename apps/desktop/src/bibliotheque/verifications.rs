// SPDX-License-Identifier: GPL-3.0-or-later
//! Vérifications de la Bibliothèque : genres suspects, éditions multiples,
//! échecs de scan, pochettes suspectes.

use super::*;

/// Morceaux dont le genre résolu ne figure pas parmi les dominants de leur
/// famille sonique — voir [`rusty_music_core::db::Library::genres_suspects`].
#[tauri::command(async)]
pub(crate) fn suspect_genres(
    etat: State<Etat>,
) -> Result<Vec<(i64, String, String, String)>, String> {
    verrou(&etat.lib)
        .genres_suspects(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// Albums présents sous plusieurs éditions chez le même artiste.
#[tauri::command(async)]
pub(crate) fn multiple_editions(
    etat: State<Etat>,
) -> Result<Vec<rusty_music_core::db::EditionsAlbum>, String> {
    verrou(&etat.lib).editions_multiples().map_err(echec)
}

/// Fichiers en échec de scan, du plus récent au plus ancien.
#[tauri::command(async)]
pub(crate) fn scan_failures(etat: State<Etat>) -> Result<Vec<(String, String, i64)>, String> {
    verrou(&etat.lib).echecs_scan().map_err(echec)
}

/// Retire un fichier de la liste des échecs, sans y toucher sur le disque —
/// pour un fichier qu'on sait perdu et qu'on ne veut plus voir revenir à
/// chaque scan.
#[tauri::command(async)]
pub(crate) fn dismiss_scan_failure(etat: State<Etat>, path: String) -> Result<(), String> {
    verrou(&etat.lib)
        .effacer_echec_scan(Path::new(&path))
        .map_err(echec)
}

/// Avancement de la vérification des pochettes, sondé par l'interface.
pub(crate) type EtatVerifPochettes = Passe<Compte>;

/// Vérifie les pochettes des dossiers dont les fichiers ont changé — ou de
/// tous, avec `force` (`rusty_music_core::pochettes_verif`). Même patron que
/// [`start_loudness`] : fil et connexion propres. Ne modifie aucun fichier de
/// l'utilisateur : les signalements se lisent par [`pochettes_suspectes`].
/// Enchaînée juste après le scan, qui est ce qui amène les fichiers neufs.
#[tauri::command(async)]
pub(crate) fn start_verif_pochettes(
    app: tauri::AppHandle,
    etat: State<Etat>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);

    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.verif_pochettes,
        "une vérification des pochettes est déjà en cours",
        Default::default(),
        move |etat| {
            let issue = Library::open(&db)
                .map_err(|e| e.to_string())
                .and_then(|lib| {
                    let fils = fils_pour_passe(&lib);
                    rusty_music_core::pochettes_verif::actualiser(
                        &lib,
                        fils,
                        force,
                        |faits, total| {
                            let mut v = verrou(&etat.verif_pochettes);
                            v.avancement.faits = faits;
                            v.avancement.total = total;
                        },
                    )
                    .map_err(|e| e.to_string())
                });

            let bilan = match issue {
                Ok(b) => format!(
                    "{} dossiers vérifiés sur {} · {} pochette{} à examiner",
                    b.verifies,
                    b.dossiers,
                    b.suspectes,
                    if b.suspectes > 1 { "s" } else { "" }
                ),
                Err(e) => format!("échec : {e}"),
            };
            tracing::info!(%bilan, "vérification des pochettes terminée");

            bilan
        },
    )
}

#[tauri::command(async)]
pub(crate) fn verif_pochettes_state(etat: State<Etat>) -> Result<EtatVerifPochettes, String> {
    Ok(verrou(&etat.verif_pochettes).clone())
}

/// Les pochettes à examiner (panneau « Vérifications » du mode Bibliothèque).
#[tauri::command(async)]
pub(crate) fn pochettes_suspectes(
    etat: State<Etat>,
) -> Result<Vec<rusty_music_core::pochettes_verif::PochetteSuspecte>, String> {
    let lib = verrou(&etat.lib);
    rusty_music_core::pochettes_verif::suspectes(&lib).map_err(echec)
}

/// Écarte le signalement d'un dossier (« Ignorer »). Revient tout seul si les
/// fichiers du dossier changent.
#[tauri::command(async)]
pub(crate) fn ignore_pochette_suspecte(etat: State<Etat>, dossier: String) -> Result<(), String> {
    let lib = verrou(&etat.lib);
    lib.ignorer_pochette_suspecte(&dossier).map_err(echec)
}

/// « Image du dossier » : l'album de `dossier` montrera `cover.jpg` (ou
/// équivalent) plutôt que l'image intégrée aux tags. Le choix vit dans la
/// base, pas dans les fichiers, pour survivre à une recopie des MP3. Les
/// pochettes déjà mises en cache pour ses pistes sont effacées.
#[tauri::command(async)]
pub(crate) fn pochette_du_dossier(etat: State<Etat>, dossier: String) -> Result<(), String> {
    let pistes = {
        let lib = verrou(&etat.lib);
        lib.choisir_pochette_dossier(&dossier, true)
            .map_err(echec)?;
        lib.pistes_du_dossier(&dossier).map_err(echec)?
    };
    let cache = etat.db.with_file_name("pochettes");
    for p in pistes {
        let chemin = Path::new(&p);
        if let Some(mtime) = std::fs::metadata(chemin)
            .ok()
            .and_then(|m| m.modified().ok())
        {
            let _ = std::fs::remove_file(cache.join(cle_pochette(chemin, mtime)));
        }
    }
    Ok(())
}
