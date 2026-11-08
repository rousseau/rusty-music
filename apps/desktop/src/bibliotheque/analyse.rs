// SPDX-License-Identifier: GPL-3.0-or-later
//! Passes locales : empreintes (analyse), descripteurs, loudness EBU R128
//! et réglage de la normalisation.

use super::*;

/// Avancement de l'analyse en cours, sondé par l'interface.
pub(crate) type EtatAnalyse = Passe<Compte>;

/// Lance le calcul des empreintes des morceaux en attente.
///
/// Déclenché à la main, et pas enchaîné au scan : sur une bibliothèque neuve
/// de cette taille la passe dure des heures, et l'engager sans que
/// l'utilisateur l'ait demandé serait le prendre en otage. L'interface affiche
/// le nombre en attente et le laisse choisir son moment.
///
/// Ouvre sa propre connexion, comme le scan : sous le verrou de la base
/// partagée, l'interface serait figée pendant toute la passe. Le mode WAL
/// autorise un rédacteur et des lecteurs simultanés.
#[tauri::command(async)]
pub(crate) fn start_analysis(app: tauri::AppHandle, etat: State<Etat>) -> Result<(), String> {
    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.analyse,
        "une analyse est déjà en cours",
        Default::default(),
        move |etat| {
            let issue = Library::open(&db)
                .map_err(|e| e.to_string())
                .and_then(|lib| {
                    let fils = fils_pour_passe(&lib);
                    rusty_music_analysis::passe::empreintes(
                        &lib,
                        None,
                        i64::MAX,
                        fils,
                        |faits, total| {
                            let mut a = verrou(&etat.analyse);
                            a.avancement.faits = faits;
                            a.avancement.total = total;
                        },
                    )
                    .map_err(|e| e.to_string())
                    // La projection doit suivre : sans elle les empreintes existent
                    // mais aucun point neuf n'apparaît sur la carte. Elle reprend
                    // les coordonnées en base, les nouveaux posés près de leurs
                    // voisins : le nuage reste en place.
                    .and_then(|r| {
                        rusty_music_analysis::passe::projeter_tout(
                            &lib,
                            None,
                            rusty_music_analysis::passe::Projection::Reprise,
                        )
                        .map_err(|e| e.to_string())
                        .map(|p| (r, p))
                    })
                });

            let bilan = match issue {
                Ok((r, p)) => format!(
                    "{} empreintes · {} en échec · {} points sur la carte",
                    r.empreintes, r.echecs, p.empreintes
                ),
                Err(e) => format!("échec : {e}"),
            };
            tracing::info!(%bilan, "analyse terminée");

            bilan
        },
    )
}

/// Avancement de l'analyse. L'interface sonde, comme pour le scan.
#[tauri::command(async)]
pub(crate) fn analysis_state(etat: State<Etat>) -> Result<EtatAnalyse, String> {
    Ok(verrou(&etat.analyse).clone())
}

/// Avancement de la mesure tempo/tonalité/énergie, sondé par l'interface.
pub(crate) type EtatDescripteurs = Passe<Compte>;

/// Mesure tempo/tonalité/énergie des morceaux en attente — ou de tous,
/// `force` effaçant d'abord ce qui est déjà mesuré.
///
/// Sépare cette passe de `start_analysis` (empreintes CLAP) : les deux
/// n'ont ni le même coût ni la même fréquence de relance. Un correctif de
/// l'algorithme (tempo doublé sur les rythmiques marquées, par exemple)
/// justifie de remesurer une bibliothèque déjà couverte — les empreintes,
/// elles, n'ont pas de raison de changer sans changer de modèle.
#[tauri::command(async)]
pub(crate) fn start_descripteurs(
    app: tauri::AppHandle,
    etat: State<Etat>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);

    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.descripteurs,
        "une mesure est déjà en cours",
        Default::default(),
        move |etat| {
            let issue = Library::open(&db)
                .map_err(|e| e.to_string())
                .and_then(|lib| {
                    let fils = fils_pour_passe(&lib);
                    if force {
                        lib.effacer_descripteurs().map_err(|e| e.to_string())?;
                    }
                    rusty_music_analysis::passe::descripteurs(
                        &lib,
                        i64::MAX,
                        fils,
                        |faits, total| {
                            let mut d = verrou(&etat.descripteurs);
                            d.avancement.faits = faits;
                            d.avancement.total = total;
                        },
                    )
                    .map_err(|e| e.to_string())
                });

            let bilan = match issue {
                Ok(r) => format!(
                    "{} mesurés · {} sans tempo · {} sans tonalité · {} en échec",
                    r.mesures, r.sans_tempo, r.sans_tonalite, r.echecs
                ),
                Err(e) => format!("échec : {e}"),
            };
            tracing::info!(%bilan, "mesure des descripteurs terminée");

            bilan
        },
    )
}

#[tauri::command(async)]
pub(crate) fn descripteurs_state(etat: State<Etat>) -> Result<EtatDescripteurs, String> {
    Ok(verrou(&etat.descripteurs).clone())
}

/// Normalisation de volume à la lecture (mode Bibliothèque) : activée ou
/// non, et sur quelle référence — voir `crate::normalisation`'s absence :
/// deux `AtomicBool` suffisent, pas besoin d'un `Mutex` pour un si petit
/// état lu à chaque ouverture de piste.
#[derive(Default)]
pub(crate) struct ReglagesNormalisation {
    pub(crate) actif: AtomicBool,
    /// `false` = gain de piste, `true` = gain d'album.
    pub(crate) album: AtomicBool,
}

/// Avancement de la passe de loudness EBU R128, sondé par l'interface.
pub(crate) type EtatLoudness = Passe<Compte>;

/// Mesure la loudness EBU R128 (piste + album) des morceaux en attente — ou
/// de tous, `force` effaçant d'abord ce qui est déjà mesuré. Même patron que
/// [`start_descripteurs`] : décode le fichier entier (le seul coût de cette
/// passe), reprenable, tourne sur son propre fil et sa propre connexion.
#[tauri::command(async)]
pub(crate) fn start_loudness(
    app: tauri::AppHandle,
    etat: State<Etat>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);

    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.loudness,
        "une mesure est déjà en cours",
        Default::default(),
        move |etat| {
            let issue = Library::open(&db)
                .map_err(|e| e.to_string())
                .and_then(|lib| {
                    let fils = fils_pour_passe(&lib);
                    if force {
                        lib.effacer_loudness().map_err(|e| e.to_string())?;
                    }
                    rusty_music_core::loudness::actualiser(&lib, i64::MAX, fils, |faits, total| {
                        let mut l = verrou(&etat.loudness);
                        l.avancement.faits = faits;
                        l.avancement.total = total;
                    })
                    .map_err(|e| e.to_string())
                });

            let bilan = match issue {
                Ok(b) => format!(
                    "{} mesurés · {} albums recalculés · {} en échec",
                    b.mesures, b.albums, b.echecs
                ),
                Err(e) => format!("échec : {e}"),
            };
            tracing::info!(%bilan, "mesure de loudness terminée");

            bilan
        },
    )
}

#[tauri::command(async)]
pub(crate) fn loudness_state(etat: State<Etat>) -> Result<EtatLoudness, String> {
    Ok(verrou(&etat.loudness).clone())
}

/// Gain de normalisation de volume (linéaire, 1.0 = neutre) pour `chemin`,
/// selon le réglage courant — jamais un blocage : coupé, ou morceau pas
/// encore mesuré, rendent 1.0.
pub(crate) fn gain_pour(etat: &Etat, chemin: &Path) -> f32 {
    if !etat.normalisation.actif.load(Ordering::Relaxed) {
        return 1.0;
    }
    let mode = if etat.normalisation.album.load(Ordering::Relaxed) {
        rusty_music_core::loudness::ModeGain::Album
    } else {
        rusty_music_core::loudness::ModeGain::Piste
    };
    verrou(&etat.lib)
        .gain_lecture(chemin, mode, rusty_music_core::loudness::CIBLE_LUFS)
        .ok()
        .flatten()
        .unwrap_or(1.0)
}

/// Active/coupe la normalisation de volume et choisit sa référence (piste ou
/// album). Réouvre le morceau en cours en tâche de fond à la même position —
/// même recette que `set_amelioration`.
#[tauri::command(async)]
pub(crate) fn set_normalisation(
    app: tauri::AppHandle,
    etat: State<Etat>,
    actif: bool,
    album: bool,
) -> Result<(), String> {
    etat.normalisation.actif.store(actif, Ordering::Relaxed);
    etat.normalisation.album.store(album, Ordering::Relaxed);

    let courant = verrou(&etat.player)
        .current()
        .map(std::path::Path::to_path_buf);
    let Some(chemin) = courant else {
        return Ok(());
    };

    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let gain = gain_pour(&etat, &chemin);
        let ouvert = rusty_music_superres::resoudre(&etat.hd, &chemin);
        let source = match rusty_music_player::ouvrir(&ouvert, gain) {
            Ok(source) => source,
            Err(e) => {
                tracing::warn!(error = %e, "réouverture pour normalisation impossible");
                return;
            }
        };
        let mut player = verrou(&etat.player);
        if let Err(e) = player.remplacer_courant(&chemin, source) {
            tracing::warn!(error = %e, "bascule de normalisation impossible");
        }
    });
    Ok(())
}
