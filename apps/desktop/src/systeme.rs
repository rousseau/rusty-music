// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes transverses : journal de l'interface, modes d'essai et d'audit,
//! dossier de données et stockage.

use super::*;

/// Le mode dans lequel ouvrir l'application, si l'environnement en impose un.
///
/// `RUSTY_MUSIC_MODE=explorer` ouvre directement sur la carte. Sert aux essais
/// — une webview du système ne se pilote pas de l'extérieur — et rend service
/// à qui veut retrouver son mode au démarrage.
#[tauri::command(async)]
pub(crate) fn mode_initial() -> Option<String> {
    std::env::var("RUSTY_MUSIC_MODE")
        .ok()
        .filter(|v| !v.is_empty())
}

/// Faut-il laisser l'interface s'auditer ? `RUSTY_MUSIC_AUDIT=1` (voir
/// `scripts/audit-interface.sh`) : elle parcourt écrans, états, thèmes et tailles
/// de fenêtre, mesure le gabarit et écrit ses constats au journal.
pub(crate) fn audit_actif() -> bool {
    std::env::var("RUSTY_MUSIC_AUDIT").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Redimensionne la fenêtre principale pour l'audit d'interface — refusé hors
/// audit, une application ne se laisse pas redimensionner par son contenu.
#[tauri::command(async)]
pub(crate) fn essai_fenetre(
    app: tauri::AppHandle,
    largeur: f64,
    hauteur: f64,
) -> Result<(), String> {
    if !audit_actif() {
        return Err("essai_fenetre n'existe que sous RUSTY_MUSIC_AUDIT".into());
    }
    let fenetre = app
        .get_webview_window("main")
        .ok_or("fenêtre principale introuvable")?;
    fenetre
        .set_size(tauri::LogicalSize::new(largeur, hauteur))
        .map_err(|e| e.to_string())
}

/// Le même renvoi pour le reste de l'interface (partition, lecteur) : une
/// version release n'a pas d'inspecteur du tout.
#[tauri::command(async)]
pub(crate) fn journal_ui(niveau: String, source: String, message: String) {
    match niveau.as_str() {
        "error" => tracing::error!(target: "ui", "{source} : {message}"),
        "warn" => tracing::warn!(target: "ui", "{source} : {message}"),
        _ => tracing::info!(target: "ui", "{source} : {message}"),
    }
}

/// Remonte une erreur de l'interface dans le journal du processus.
///
/// Sans cela, une exception JavaScript reste dans la console de la vue web,
/// invisible depuis le terminal : le symptôme se voit à l'écran, la cause
/// nulle part. Plusieurs pannes de cette session sont restées obscures faute
/// de ce fil.
#[tauri::command(async)]
pub(crate) fn js_error(message: String, source: Option<String>) {
    tracing::error!(source = source.unwrap_or_default(), "interface : {message}");
}

/// Le dossier où l'application écrit tout : la base et les empreintes, les
/// journaux, les stems démixés, les rendus HD, les tuiles de la carte. Montré en
/// tête du mode Bibliothèque — l'utilisateur doit pouvoir retrouver, sauvegarder
/// ou purger ce que le logiciel pose sur son disque.
#[tauri::command(async)]
pub(crate) fn dossier_donnees(etat: State<Etat>) -> Result<String, String> {
    let d = etat
        .db
        .parent()
        .ok_or("la base n'a pas de dossier parent")?;
    Ok(d.display().to_string())
}

/// Poids du logiciel lui-même — pas ce qu'il produit (stems, HD), mais ce
/// qu'il faut avoir sur le disque pour qu'il tourne : l'exécutable, les
/// modèles ML qu'il charge, la base locale et le plan de ville s'il est
/// importé. Complète `stems_cache`/`superres_cache` pour une trace complète
/// de l'empreinte disque du logiciel.
#[derive(Clone, serde::Serialize)]
pub(crate) struct StockageLogiciel {
    executable: u64,
    modeles: u64,
    base: u64,
    ville: u64,
}

#[tauri::command(async)]
pub(crate) fn stockage_logiciel(etat: State<Etat>) -> Result<StockageLogiciel, String> {
    let executable = std::env::current_exe()
        .and_then(std::fs::metadata)
        .map(|m| m.len())
        .unwrap_or(0);

    // Tous les dossiers de poids qui existent, chacun une fois : les poids
    // téléchargés par l'application (données utilisateur) s'ajoutent à ceux du
    // paquet ou du dépôt. Deux chemins vers le même dossier ne comptent qu'une
    // fois.
    let mut vus = std::collections::HashSet::new();
    let modeles: u64 = rusty_music_core::modeles::dossiers()
        .into_iter()
        .filter_map(|d| d.canonicalize().ok())
        .filter(|d| d.is_dir() && vus.insert(d.clone()))
        .map(|d| poids(&d))
        .sum();

    let mut base = std::fs::metadata(&etat.db).map(|m| m.len()).unwrap_or(0);
    for suffixe in ["-wal", "-shm"] {
        let mut chemin = etat.db.clone().into_os_string();
        chemin.push(suffixe);
        base += std::fs::metadata(chemin).map(|m| m.len()).unwrap_or(0);
    }

    let ville = plan_de_ville(&etat.db)
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(StockageLogiciel {
        executable,
        modeles,
        base,
        ville,
    })
}
