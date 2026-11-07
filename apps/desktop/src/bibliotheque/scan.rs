// SPDX-License-Identifier: GPL-3.0-or-later
//! Scan et dossier surveillé : surveillance par racine, oubli d'une racine,
//! scan et son avancement.

use super::*;

/// Démarre la surveillance continue de `root`, si elle ne l'est pas déjà.
///
/// Pose le watcher (immédiat) puis lance la boucle d'évènements dans son
/// propre fil, avec sa propre connexion — même raison que [`start_scan`] : ne
/// pas tenir `Etat::lib` pendant une opération qui dure toute la session.
/// Idempotent : sans effet si `root` est déjà surveillée, donc appelable sans
/// vérifier soi-même (au démarrage, après un scan réussi…).
pub(crate) fn demarrer_surveillance(etat: &Etat, root: &Path) {
    {
        let deja = verrou(&etat.surveillances);
        if deja.contains_key(root) {
            return;
        }
    }
    let (surveillance, rx) = match rusty_music_core::watch::demarrer(root) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(root = %root.display(), %e, "surveillance impossible");
            return;
        }
    };
    verrou(&etat.surveillances).insert(root.to_path_buf(), surveillance);
    tracing::info!(root = %root.display(), "surveillance démarrée");

    let db = etat.db.clone();
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        match Library::open(&db) {
            Ok(lib) => rusty_music_core::watch::boucle(&lib, rx),
            Err(e) => tracing::warn!(%e, "surveillance : base inaccessible"),
        }
        tracing::info!(root = %root.display(), "surveillance arrêtée");
    });
}

/// Oublie une racine **et les morceaux qui en dépendent**.
#[tauri::command(async)]
pub(crate) fn forget_root(etat: State<Etat>, path: String) -> Result<usize, String> {
    let n = verrou(&etat.lib)
        .remove_root(Path::new(&path))
        .map_err(echec)?;
    // Sans ça, un évènement filesystem reçu après coup remettrait le morceau
    // en base — la surveillance ne sait pas que l'utilisateur vient de dire
    // « oublie ». Abandonner le handle est le seul moyen de l'arrêter.
    verrou(&etat.surveillances).remove(Path::new(&path));
    tracing::info!(%path, morceaux = n, "racine oubliée");
    Ok(n)
}

/// Lance le scan d'une racine dans un thread dédié.
///
/// Le scan tient la base pendant des dizaines de minutes sur un support lent :
/// le faire sous le verrou de `Etat::lib` figerait toute l'interface. Le thread
/// ouvre donc sa **propre** connexion sur le même fichier — le mode WAL du
/// schéma autorise un rédacteur et des lecteurs simultanés.
#[tauri::command(async)]
pub(crate) fn start_scan(
    app: tauri::AppHandle,
    etat: State<Etat>,
    path: String,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    let racine = PathBuf::from(&path);
    if !racine.is_dir() {
        return Err(format!("{path} n'est pas un dossier"));
    }
    let db = etat.db.clone();
    let depart = AvancementScan {
        racine: path.clone(),
        morceaux: 0,
    };
    // La garde de `lancer` est posée avant `scan_root_jobs` : c'est là qu'est
    // le travail susceptible de paniquer (décodage des tags d'un fichier
    // arbitraire), et la garde ne protège que ce qu'elle a vu commencer.
    passe::lancer(
        app,
        |e| &e.scan,
        "un scan est déjà en cours",
        depart,
        move |etat| {
            let issue = Library::open(&db)
                .map_err(|e| e.to_string())
                .and_then(|lib| {
                    let jobs = coeurs_arriere_plan();
                    rusty_music_core::scan::scan_root_jobs(&lib, &racine, jobs, force)
                        .map_err(|e| e.to_string())
                });

            let succes = issue.is_ok();
            let bilan = match issue {
                Ok(r) => format!(
                    "{} vus · {} ingérés · {} inchangés · {} retirés · {} en échec",
                    r.seen, r.inserted, r.skipped, r.removed, r.failed
                ),
                Err(e) => format!("échec : {e}"),
            };
            tracing::info!(%bilan, "scan terminé");

            // Une racine qui vient d'être scannée avec succès — nouvelle ou déjà
            // connue — doit être surveillée en continu ; `demarrer_surveillance`
            // ne fait rien si elle l'est déjà.
            if succes {
                demarrer_surveillance(etat, &racine);
            }
            bilan
        },
    )
}

/// Avancement du scan. L'interface sonde, faute de rapport intermédiaire.
#[tauri::command(async)]
pub(crate) fn scan_state(etat: State<Etat>) -> Result<EtatScan, String> {
    let mut s = verrou(&etat.scan).clone();
    if s.en_cours {
        // Le nombre de morceaux progresse pendant le scan : c'est la seule
        // mesure d'avancement qu'on puisse offrir sans instrumenter le cœur.
        s.avancement.morceaux = verrou(&etat.lib).count().map_err(echec)?;
    }
    Ok(s)
}
