// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes d'Écouter : transport, file, amélioration « E », « HD » (AERO),
//! minuteur, playlists et listes, session et touches média.

use super::*;

/// Avancement d'une régénération HD (super-résolution), sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementSuperres {
    /// Le morceau traité, pour que l'interface sache si l'avancement est le sien.
    pub(crate) source: String,
    pub(crate) faits: usize,
    pub(crate) total: usize,
    /// Téléchargement des poids au premier usage, en octets (0 / 0 sinon).
    pub(crate) telecharges: u64,
    pub(crate) a_telecharger: u64,
}
pub(crate) type EtatSuperres = Passe<AvancementSuperres>;

/// Régénère un morceau en haute résolution (AERO) — rendu hors ligne vers le
/// cache `hd/`. Tourne dans son fil : ~30 s par minute d'audio stéréo, plus le
/// chargement du modèle au premier appel, et le téléchargement des poids
/// (156 Mo) au tout premier.
#[tauri::command(async)]
pub(crate) fn start_superres(
    app: tauri::AppHandle,
    etat: State<Etat>,
    path: String,
) -> Result<(), String> {
    let source = PathBuf::from(&path);
    if !source.is_file() {
        return Err(format!("{path} est introuvable"));
    }
    let cible = rusty_music_superres::chemin_cache(&etat.hd, &source);
    let depart = AvancementSuperres {
        source: path.clone(),
        ..Default::default()
    };
    let deja = "une régénération est déjà en cours";
    passe::lancer(
        app,
        |e| &e.superres,
        deja,
        depart,
        move |etat| {
            let _ = std::fs::create_dir_all(&etat.hd);
            rusty_music_superres::purger_anciens(&etat.hd);

            let progres = |faits: usize, total: usize| {
                let mut s = verrou(&etat.superres);
                s.avancement.faits = faits;
                s.avancement.total = total;
            };

            let issue = (|| -> Result<f32, String> {
                let mut garde = verrou(&etat.superres_modele);
                if garde.valeur.is_none() {
                    use rusty_music_superres as sr;
                    let modele_onnx = match rusty_music_core::modeles::trouver(sr::POIDS_FICHIER) {
                        Some(p) => p,
                        None => rusty_music_core::modeles::telecharger(
                            sr::POIDS_FICHIER,
                            sr::POIDS_URL,
                            sr::POIDS_OCTETS,
                            Some(sr::POIDS_SHA256),
                            |recus, attendus| {
                                let mut s = verrou(&etat.superres);
                                s.avancement.telecharges = recus;
                                s.avancement.a_telecharger = attendus.unwrap_or(sr::POIDS_OCTETS);
                            },
                        )
                        .map_err(|e| format!("téléchargement du modèle : {e}"))?,
                    };
                    garde.valeur =
                        Some(sr::Modele::charger(&modele_onnx).map_err(|e| e.to_string())?);
                }
                let modele = garde.valeur.as_mut().expect("modèle chargé");
                let resultat = rusty_music_superres::regenerer(&source, &cible, modele, progres);
                garde.touche = Some(Instant::now());
                resultat.map_err(|e| e.to_string())
            })();

            let bilan = match issue {
                // Le spectre de la source est conservé (mélange HF), donc le HD ne
                // peut pas ternir ; mais au-dessus de ~16 kHz de coupure il n'a
                // presque rien à ajouter — autant le dire.
                Ok(coupure) if coupure >= 16_000.0 => format!(
                    "régénéré, mais la source monte déjà à {} kHz — le HD n'ajoute presque rien",
                    (coupure / 1000.0).round() as i32
                ),
                Ok(_) => "régénéré en HD".to_string(),
                Err(e) => {
                    let _ = std::fs::remove_file(&cible);
                    format!("échec : {e}")
                }
            };
            tracing::info!(%bilan, source = %path, "régénération HD terminée");
            bilan
        },
    )
}

#[tauri::command(async)]
pub(crate) fn superres_state(etat: State<Etat>) -> Result<EtatSuperres, String> {
    Ok(verrou(&etat.superres).clone())
}

/// Vrai si le morceau a déjà une version HD en cache.
#[tauri::command(async)]
pub(crate) fn superres_disponible(etat: State<Etat>, path: String) -> Result<bool, String> {
    Ok(rusty_music_superres::chemin_cache(&etat.hd, Path::new(&path)).is_file())
}

/// Active/coupe la lecture des versions HD (quand elles existent). Réouvre le
/// morceau en cours en tâche de fond, comme le bouton « E ».
#[tauri::command(async)]
pub(crate) fn set_lecture_hd(
    app: tauri::AppHandle,
    etat: State<Etat>,
    actif: bool,
) -> Result<(), String> {
    rusty_music_superres::set_lecture_hd(actif);
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
        match rusty_music_player::ouvrir(&ouvert, gain) {
            Ok(source) => {
                let mut player = verrou(&etat.player);
                if let Err(e) = player.remplacer_courant(&chemin, source) {
                    tracing::warn!(error = %e, "bascule HD impossible");
                }
            }
            Err(e) => tracing::warn!(error = %e, "réouverture HD impossible"),
        }
    });
    Ok(())
}

/// Taille du cache HD et nombre de morceaux régénérés.
#[tauri::command(async)]
pub(crate) fn superres_cache(etat: State<Etat>) -> Result<(u64, usize), String> {
    let dossier = &etat.hd;
    let mut octets = 0;
    let mut n = 0;
    if let Ok(entrees) = std::fs::read_dir(dossier) {
        for e in entrees.flatten() {
            if let Ok(m) = e.metadata() {
                octets += m.len();
                n += 1;
            }
        }
    }
    Ok((octets, n))
}

/// Vide le cache HD.
#[tauri::command(async)]
pub(crate) fn vider_cache_hd(etat: State<Etat>) -> Result<(), String> {
    if etat.hd.is_dir() {
        std::fs::remove_dir_all(&etat.hd).map_err(echec)?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub(crate) struct EtatLecture {
    current: Option<String>,
    paused: bool,
    finished: bool,
    position_ms: u64,
    remaining: usize,
    volume: f32,
    /// « aucune », « toutes » ou « une » — reflète `set_repetition`.
    repetition: &'static str,
    alea: bool,
    /// Rang de la file à partir duquel elle peut encore être mélangée ou
    /// réordonnée — voir `rusty_music_player::Player::verrou`.
    verrou: usize,
    /// Pistes sautées depuis le sondage précédent parce qu'elles ne s'ouvraient
    /// pas — relevées une seule fois, l'interface les montre dans le transport.
    ignorees: Vec<PisteIgnoree>,
    /// Minuteur d'arrêt armé, avec ce qu'il reste — l'interface l'affiche.
    minuteur: Option<minuteur::EtatMinuteur>,
    /// Un multipiste de stems est chargé. L'interface s'y aligne : c'est le
    /// moteur qui sait ce qui sonne, pas la copie qu'elle en garde.
    stems: bool,
}

/// Une piste que le lecteur a sautée : fichier disparu, corrompu ou non décodé.
#[derive(serde::Serialize)]
pub(crate) struct PisteIgnoree {
    path: String,
    raison: String,
}

#[tauri::command(async)]
pub(crate) fn play(etat: State<Etat>, paths: Vec<String>) -> Result<(), String> {
    let chemins: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    couper_stems(&etat);
    verrou(&etat.player).play(&chemins).map_err(echec)
}

/// Comme `play`, mais ne redémarre pas si le premier morceau ne change pas —
/// la régénération d'un chemin sur la carte (bruit, « Autre tirage »).
#[tauri::command(async)]
pub(crate) fn set_queue(etat: State<Etat>, paths: Vec<String>) -> Result<(), String> {
    let chemins: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let stems_coupes = couper_stems(&etat);
    let mut player = verrou(&etat.player);
    player.set_queue(&chemins).map_err(echec)?;
    // Même départ que la file en cours : `set_queue` ne redémarre rien, et le
    // lecteur est resté en pause là où `stems_play` l'avait laissé.
    if stems_coupes {
        player.resume();
    }
    Ok(())
}

/// Remplace la file par `paths` en gardant la piste écoutée sans coupure.
///
/// Le bouton ✦ « playlist dans l'esprit de ce morceau » : la nouvelle liste
/// part du morceau en cours et doit vraiment enchaîner dès le suivant.
/// `set_queue` gardait le préchargement de l'ancienne file — un ou deux
/// morceaux des résultats de recherche se glissaient avant la playlist.
#[tauri::command(async)]
pub(crate) fn remplacer_file(etat: State<Etat>, paths: Vec<String>) -> Result<(), String> {
    let chemins: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let stems_coupes = couper_stems(&etat);

    let courant = verrou(&etat.player)
        .current()
        .map(std::path::Path::to_path_buf);

    // Tête de file différente de ce qu'on écoute (ou rien en lecture) :
    // `play` suffit, rien à rouvrir.
    if courant.as_deref() != chemins.first().map(PathBuf::as_path) {
        return verrou(&etat.player).play(&chemins).map_err(echec);
    }
    let chemin = courant.expect("tête de file == piste en cours");

    // Hors du verrou `player` : `ouvrir` peut lire le disque plusieurs
    // secondes. Même précaution que la bascule d'amélioration et le
    // préchargement.
    let gain = gain_pour(&etat, &chemin);
    let ouvert = rusty_music_superres::resoudre(&etat.hd, &chemin);
    let source = rusty_music_player::ouvrir(&ouvert, gain).map_err(echec)?;
    let mut player = verrou(&etat.player);
    player.rebrancher_file(&chemins, source).map_err(echec)?;
    // `rebrancher_file` garde l'état de pause : après des stems, le lecteur est
    // en pause, et « jouer cette playlist » doit sonner.
    if stems_coupes {
        player.resume();
    }
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn toggle_pause(etat: State<Etat>) -> Result<bool, String> {
    let stems_coupes = couper_stems(&etat);
    let player = verrou(&etat.player);
    // Des stems chargés, c'est le lecteur en pause : demander la lecture au
    // lecteur ordinaire, c'est lui rendre la main, pas alterner.
    if stems_coupes || player.is_paused() {
        player.resume();
    } else {
        player.pause();
    }
    Ok(player.is_paused())
}

#[tauri::command(async)]
pub(crate) fn skip(etat: State<Etat>) -> Result<(), String> {
    let stems_coupes = couper_stems(&etat);
    let player = verrou(&etat.player);
    player.skip();
    // ⏭ pendant des stems : le lecteur est en pause depuis `stems_play`, et
    // passer au suivant ne le relance pas.
    if stems_coupes {
        player.resume();
    }
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn previous(etat: State<Etat>) -> Result<(), String> {
    // `previous` recharge la file et relance le lecteur : sans couper les
    // stems avant, le morceau d'origine sonnait par-dessus eux.
    couper_stems(&etat);
    verrou(&etat.player).previous().map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn jump_to(etat: State<Etat>, index: usize) -> Result<(), String> {
    couper_stems(&etat);
    verrou(&etat.player).jump_to(index).map_err(echec)
}

/// File d'attente dans l'ordre courant — après un mélange ou une
/// ré-ordonnance manuelle, c'est le seul moyen fiable pour l'interface de
/// savoir ce que le moteur a réellement retenu (`set_alea`/`deplacer_file`
/// l'ignorent silencieusement quand la piste visée est déjà confiée à la
/// sortie).
pub(crate) fn file_en_chaines(player: &rusty_music_player::Player) -> Vec<String> {
    player
        .queue()
        .iter()
        .map(|p| p.display().to_string())
        .collect()
}

/// Bascule l'aléatoire du panneau « file d'attente ». Rend la file dans son
/// ordre effectif : mélangée à l'activation, restaurée à la désactivation.
#[tauri::command(async)]
pub(crate) fn set_alea(etat: State<Etat>, actif: bool) -> Result<Vec<String>, String> {
    let mut player = verrou(&etat.player);
    player.set_alea(actif);
    Ok(file_en_chaines(&player))
}

/// Répétition du panneau « file d'attente » — `mode` vaut « aucune »,
/// « toutes » ou « une ».
#[tauri::command(async)]
pub(crate) fn set_repetition(etat: State<Etat>, mode: String) -> Result<(), String> {
    let mode = match mode.as_str() {
        "toutes" => rusty_music_player::Repetition::Toutes,
        "une" => rusty_music_player::Repetition::Une,
        _ => rusty_music_player::Repetition::Aucune,
    };
    verrou(&etat.player).set_repetition(mode);
    Ok(())
}

/// Déplace une piste de la file — la ré-ordonnance manuelle par
/// glisser-déposer du panneau. Rend la file dans son ordre effectif : voir
/// [`file_en_chaines`].
#[tauri::command(async)]
pub(crate) fn deplacer_file(etat: State<Etat>, de: usize, a: usize) -> Result<Vec<String>, String> {
    let mut player = verrou(&etat.player);
    player.deplacer(de, a);
    Ok(file_en_chaines(&player))
}

#[tauri::command(async)]
pub(crate) fn seek(etat: State<Etat>, position_ms: u64) -> Result<(), String> {
    verrou(&etat.player)
        .seek(std::time::Duration::from_millis(position_ms))
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn set_volume(etat: State<Etat>, volume: f32) -> Result<(), String> {
    verrou(&etat.player).set_volume(volume);
    // Le même curseur commande les stems quand ils ont la main.
    if let Some(m) = verrou(&etat.stems).as_ref() {
        m.set_volume(volume);
    }
    Ok(())
}

/// Bouton « E » : active/coupe l'amélioration du son (excitation
/// psychoacoustique) et, si fourni, règle son intensité (`0.0`..=`1.0`). Si un
/// morceau joue, il est réouvert **en tâche de fond** à la même position — le
/// son en cours continue pendant le décodage, puis on bascule sans coupure. Le
/// préchargement se reconstruit tout seul au sondage suivant, avec la nouvelle
/// version.
#[tauri::command(async)]
pub(crate) fn set_amelioration(
    app: tauri::AppHandle,
    etat: State<Etat>,
    actif: bool,
    intensite: Option<f32>,
) -> Result<(), String> {
    let ame = rusty_music_player::amelioration();
    ame.set_actif(actif);
    if let Some(i) = intensite {
        ame.set_intensite(i);
    }

    let courant = verrou(&etat.player)
        .current()
        .map(std::path::Path::to_path_buf);
    let Some(chemin) = courant else {
        return Ok(());
    };

    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        // Hors du verrou `player` : `ouvrir` lit le drapeau global qu'on vient
        // de poser et applique (ou non) l'amélioration. Le chemin est résolu
        // vers le cache HD s'il y a lieu, comme dans le préchargement.
        let gain = gain_pour(&etat, &chemin);
        let ouvert = rusty_music_superres::resoudre(&etat.hd, &chemin);
        let source = match rusty_music_player::ouvrir(&ouvert, gain) {
            Ok(source) => source,
            Err(e) => {
                tracing::warn!(error = %e, "réouverture pour amélioration impossible");
                return;
            }
        };
        let mut player = verrou(&etat.player);
        if let Err(e) = player.remplacer_courant(&chemin, source) {
            tracing::warn!(error = %e, "bascule d'amélioration impossible");
        }
    });
    Ok(())
}

/// Précharge la piste suivante dans la file, si la réserve n'est pas pleine.
///
/// En trois temps plutôt qu'un `player.completer()` verrou tenu : la lecture
/// disque de `ouvrir` peut prendre plusieurs secondes sur la carte SD, et ce
/// verrou est aussi celui de `toggle_pause` — le tenir pendant l'I/O rendrait
/// le bouton lecture/pause silencieusement peu réactif le temps qu'une piste
/// suivante se précharge.
///
/// Appelée à la fois par [`playback_state`] (sondage de l'interface) et par
/// le fil de fond démarré dans `main` : la fenêtre en arrière-plan ou
/// minimisée fait ralentir voire suspendre les temporisateurs JS de la
/// webview (WKWebView sur macOS), et avec elle le seul sondage — la lecture
/// s'arrêtait alors en fin de piste jusqu'à ramener l'appli au premier plan.
/// Le fil de fond n'a pas ce problème : il tourne côté natif, indépendamment
/// de la visibilité de la fenêtre.
///
/// **Un seul appel à la fois** (`Etat::precharge`, `try_lock`). Ces deux
/// appelants se croisent : `a_precharger` avance `prochain` avant que le
/// fichier soit ouvert, et la sortie ne compte la piste qu'une fois ajoutée —
/// pendant les ~1 s de décodage, un second appel voyait encore la réserve
/// vide et prenait la piste d'après. Les deux arrivaient dans l'ordre où elles
/// finissaient de décoder : sur un album, la piste 3 (plus courte) pouvait
/// passer avant la 2, que ⏭ semblait alors « sauter ». Le second appelant
/// passe son tour ; le battement suivant réessaiera.
pub(crate) fn precharger_suivante(etat: &Etat) -> Result<(), String> {
    let Ok(_seul) = etat.precharge.try_lock() else {
        return Ok(());
    };
    let a_charger = {
        let mut player = verrou(&etat.player);
        let epoque = player.epoque();
        player.a_precharger().map(|(rang, piste)| {
            // Chemin de bibliothèque (avant résolution HD), pour le gain de
            // normalisation — voir `gain_pour` et `Player::completer`.
            let original = player
                .queue()
                .get(rang)
                .cloned()
                .unwrap_or_else(|| piste.clone());
            (epoque, rang, piste, original)
        })
    };
    if let Some((epoque, rang, piste, original)) = a_charger {
        let gain = gain_pour(etat, &original);
        match rusty_music_player::ouvrir(&piste, gain) {
            // Refusée si la sortie a été vidée pendant l'ouverture (⏮, saut,
            // nouvelle liste) : cette piste est celle de l'ancienne file.
            Ok(source) => {
                verrou(&etat.player).charger_precharge_si(epoque, rang, source);
            }
            Err(e) => {
                // Une piste illisible ne doit pas interrompre le suivi : la
                // lecture passera simplement à la suivante — et le transport
                // le dit (`EtatLecture::ignorees`).
                tracing::warn!(error = %e, "préparation de la piste suivante impossible");
                let mut player = verrou(&etat.player);
                if player.epoque() == epoque {
                    player.signaler_echec(rang, &e.to_string());
                }
            }
        }
    }
    Ok(())
}

/// Sondé par l'interface : `rodio` ne pousse pas d'évènements.
#[tauri::command(async)]
pub(crate) fn playback_state(etat: State<Etat>) -> Result<EtatLecture, String> {
    precharger_suivante(&etat)?;
    // Avant de tenir le verrou du lecteur : le minuteur le prend lui-même, et
    // aucune commande ne tient les stems et le lecteur à la fois.
    let minuteur = etat_du_minuteur(&etat);
    let stems = verrou(&etat.stems).is_some();
    let mut player = verrou(&etat.player);
    let ignorees = player
        .prendre_echecs()
        .into_iter()
        .map(|e| PisteIgnoree {
            path: e.chemin.display().to_string(),
            raison: e.raison,
        })
        .collect();
    Ok(EtatLecture {
        current: player.current().map(|p| p.display().to_string()),
        paused: player.is_paused(),
        finished: player.is_finished(),
        position_ms: player.position().as_millis() as u64,
        remaining: player.remaining(),
        volume: player.volume(),
        repetition: player.repetition().nom(),
        alea: player.alea(),
        verrou: player.verrou(),
        ignorees,
        minuteur,
        stems,
    })
}

/// Photo du lecteur pour le minuteur. La durée est celle que le lecteur a
/// **décodée** (`Player::duree_courante`), pas celle de la base : c'est la fin
/// qu'on entend, et c'est elle que le minuteur vise.
pub(crate) fn photo_lecteur(etat: &Etat) -> minuteur::Lecteur {
    let p = verrou(&etat.player);
    minuteur::Lecteur {
        courant: p.current().map(Path::to_path_buf),
        position: p.position(),
        duree: p.duree_courante(),
        volume: p.volume(),
        repetition_une: p.repetition() == rusty_music_player::Repetition::Une,
    }
}

/// Ce que le minuteur armé affiche, ou `None` s'il n'y en a pas.
pub(crate) fn etat_du_minuteur(etat: &Etat) -> Option<minuteur::EtatMinuteur> {
    let garde = verrou(&etat.minuteur);
    let m = garde.as_ref()?;
    let lecteur = photo_lecteur(etat);
    Some(m.etat(std::time::Instant::now(), &lecteur))
}

/// Applique au lecteur ce que le minuteur demande.
pub(crate) fn appliquer_minuteur(etat: &Etat, action: minuteur::Action) {
    use minuteur::Action;
    match action {
        Action::Rien => {}
        Action::Volume(v) => verrou(&etat.player).set_volume(v),
        Action::Pause { rendre_volume } => {
            let mut p = verrou(&etat.player);
            p.pause();
            // Pause d'abord : remettre le volume avant ferait entendre la
            // reprise du son pendant un instant.
            if let Some(v) = rendre_volume {
                p.set_volume(v);
            }
        }
        Action::Annuler { rendre_volume } => {
            if let Some(v) = rendre_volume {
                verrou(&etat.player).set_volume(v);
            }
        }
    }
}

/// Un tour de surveillance : fait avancer le minuteur armé et rend le délai
/// avant le suivant (500 ms tant qu'aucun n'est armé).
pub(crate) fn surveiller_minuteur(etat: &Etat) -> Duration {
    let mut garde = verrou(&etat.minuteur);
    let Some(m) = garde.as_mut() else {
        return Duration::from_millis(500);
    };
    let lecteur = photo_lecteur(etat);
    let (action, fini) = m.avancer(std::time::Instant::now(), &lecteur);
    let attente = m.battement(&lecteur);
    if fini {
        *garde = None;
    }
    appliquer_minuteur(etat, action);
    attente
}

/// Les morceaux de l'album du morceau en cours qui restent dans la file, dans
/// l'ordre, morceau en cours en tête : la suite ininterrompue de pistes du même
/// album. Sans album connu, le morceau en cours seul.
pub(crate) fn album_a_venir(etat: &Etat, courant: &Path) -> Vec<PathBuf> {
    let a_venir: Vec<String> = {
        let p = verrou(&etat.player);
        let queue = p.queue();
        let debut = queue.iter().position(|c| c == courant).unwrap_or(0);
        queue[debut..]
            .iter()
            .map(|c| c.display().to_string())
            .collect()
    };
    let Ok(pistes) = verrou(&etat.lib).pistes_par_chemins(&a_venir) else {
        return vec![courant.to_path_buf()];
    };
    let Some(album) = pistes.first().and_then(|t| t.album.clone()) else {
        return vec![courant.to_path_buf()];
    };
    pistes
        .iter()
        .take_while(|t| t.album.as_deref() == Some(album.as_str()))
        .map(|t| PathBuf::from(&t.path))
        .collect()
}

/// Arme le minuteur d'arrêt. `mode` : « duree » (avec `minutes`), « piste » ou
/// « album ». Remplace le minuteur précédent, en rendant son volume s'il était
/// en plein fondu.
#[tauri::command(async)]
pub(crate) fn minuteur_demarrer(
    etat: State<Etat>,
    mode: String,
    minutes: Option<u64>,
) -> Result<Option<minuteur::EtatMinuteur>, String> {
    let lecteur = photo_lecteur(&etat);
    let maintenant = std::time::Instant::now();
    let rien_ne_joue = || "rien ne joue : pas de morceau à finir".to_string();
    let nouveau = match mode.as_str() {
        "duree" => {
            let m = minutes
                .filter(|m| (1..=24 * 60).contains(m))
                .ok_or("durée du minuteur invalide")?;
            minuteur::Minuteur::dans(maintenant, Duration::from_secs(m * 60))
        }
        "piste" => minuteur::Minuteur::fin_de_piste(
            lecteur.courant.clone().ok_or_else(rien_ne_joue)?,
            lecteur.position,
        ),
        "album" => {
            let courant = lecteur.courant.clone().ok_or_else(rien_ne_joue)?;
            minuteur::Minuteur::fin_d_album(album_a_venir(&etat, &courant), lecteur.position)
                .ok_or_else(rien_ne_joue)?
        }
        autre => return Err(format!("mode de minuteur inconnu : {autre}")),
    };
    let affichage = nouveau.etat(maintenant, &lecteur);
    let mut garde = verrou(&etat.minuteur);
    if let Some(v) = garde.take().and_then(|ancien| ancien.annuler()) {
        verrou(&etat.player).set_volume(v);
    }
    *garde = Some(nouveau);
    Ok(Some(affichage))
}

/// Désarme le minuteur ; rend le volume s'il était en plein fondu.
#[tauri::command(async)]
pub(crate) fn minuteur_annuler(etat: State<Etat>) {
    if let Some(v) = verrou(&etat.minuteur).take().and_then(|m| m.annuler()) {
        verrou(&etat.player).set_volume(v);
    }
}

/// Le minuteur armé et ce qu'il reste, ou `None`.
#[tauri::command(async)]
pub(crate) fn minuteur_etat(etat: State<Etat>) -> Option<minuteur::EtatMinuteur> {
    etat_du_minuteur(&etat)
}

/// La file du lecteur, avec les métadonnées de la bibliothèque, dans l'ordre
/// du moteur. Sert à l'interface quand elle ne la connaît pas — au lancement,
/// après la reprise de session, le lecteur tient une file que la webview n'a
/// jamais vue.
#[tauri::command(async)]
pub(crate) fn file_courante(etat: State<Etat>) -> Result<Vec<TrackRow>, String> {
    let chemins: Vec<String> = verrou(&etat.player)
        .queue()
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    verrou(&etat.lib)
        .pistes_par_chemins(&chemins)
        .map_err(echec)
}

/// Les playlists enregistrées, la plus récemment modifiée d'abord.
#[tauri::command(async)]
pub(crate) fn playlists(etat: State<Etat>) -> Result<Vec<PlaylistRow>, String> {
    verrou(&etat.lib).playlists().map_err(echec)
}

/// Enregistre `paths` (la file, dans l'ordre) sous le nom `nom`. `origine` dit
/// d'où elle vient quand l'interface le sait (la demande faite à Lama…).
/// Rend l'identifiant de la playlist.
#[tauri::command(async)]
pub(crate) fn creer_playlist(
    etat: State<Etat>,
    nom: String,
    origine: Option<String>,
    paths: Vec<String>,
) -> Result<i64, String> {
    if paths.is_empty() {
        return Err("la file est vide : il n'y a rien à enregistrer".into());
    }
    verrou(&etat.lib)
        .creer_playlist(&nom, origine.as_deref(), &paths)
        .map_err(echec)
}

/// Les pistes de la playlist `id` que la bibliothèque connaît encore, dans
/// l'ordre enregistré.
#[tauri::command(async)]
pub(crate) fn playlist_pistes(etat: State<Etat>, id: i64) -> Result<Vec<TrackRow>, String> {
    verrou(&etat.lib).pistes_de_playlist(id).map_err(echec)
}

/// Bascule le favori du morceau `path` ; rend son nouvel état.
#[tauri::command(async)]
pub(crate) fn basculer_favori(etat: State<Etat>, path: String) -> Result<bool, String> {
    verrou(&etat.lib).basculer_favori(&path).map_err(echec)
}

/// Les chemins des favoris : l'interface en tient l'ensemble pour marquer les
/// lignes (un favori est un chemin, pas un identifiant — voir `memoire.rs`).
#[tauri::command(async)]
pub(crate) fn favoris(etat: State<Etat>) -> Result<Vec<String>, String> {
    verrou(&etat.lib).chemins_favoris().map_err(echec)
}

/// Les listes calculées à partir des écoutes (récents, plus écoutés, favoris)
/// qui ne sont pas vides, pour la tête de la vue Playlists.
#[tauri::command(async)]
pub(crate) fn listes_intelligentes(etat: State<Etat>) -> Result<Vec<ListeIntelligente>, String> {
    verrou(&etat.lib).listes_intelligentes().map_err(echec)
}

/// Les pistes d'une liste calculée (`quoi` : « recents », « plus-ecoutes »,
/// « favoris »).
#[tauri::command(async)]
pub(crate) fn liste_intelligente(etat: State<Etat>, quoi: String) -> Result<Vec<TrackRow>, String> {
    verrou(&etat.lib)
        .liste_intelligente(&quoi)
        .map_err(echec)?
        .ok_or_else(|| format!("liste inconnue : {quoi}"))
}

/// Écrit la playlist `id` en `.m3u8` dans `dossier` (celui que l'utilisateur a
/// choisi) et rend le chemin du fichier. Les pistes que la bibliothèque ne
/// connaît plus n'y figurent pas. N'écrase jamais un fichier existant.
#[tauri::command(async)]
pub(crate) fn exporter_playlist(
    etat: State<Etat>,
    id: i64,
    dossier: String,
) -> Result<String, String> {
    let (nom, pistes) = {
        let lib = verrou(&etat.lib);
        let nom = lib
            .playlists()
            .map_err(echec)?
            .into_iter()
            .find(|p| p.id == id)
            .map(|p| p.nom)
            .ok_or("cette playlist n'existe plus")?;
        (nom, lib.pistes_de_playlist(id).map_err(echec)?)
    };
    if pistes.is_empty() {
        return Err("aucune piste de cette playlist n'est dans la bibliothèque".into());
    }
    rusty_music_core::playlists::ecrire_m3u8(Path::new(&dossier), &nom, &pistes)
        .map(|p| p.display().to_string())
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn renommer_playlist(etat: State<Etat>, id: i64, nom: String) -> Result<(), String> {
    match verrou(&etat.lib)
        .renommer_playlist(id, &nom)
        .map_err(echec)?
    {
        true => Ok(()),
        false => Err("cette playlist n'existe plus".into()),
    }
}

#[tauri::command(async)]
pub(crate) fn supprimer_playlist(etat: State<Etat>, id: i64) -> Result<(), String> {
    match verrou(&etat.lib).supprimer_playlist(id).map_err(echec)? {
        true => Ok(()),
        false => Err("cette playlist n'existe plus".into()),
    }
}

/// Branche [`SuiviEcoute::avancer`] sur le lecteur et la base. Appelé par le fil
/// de fond : côté natif, donc exact même fenêtre masquée.
pub(crate) fn suivre_ecoute(etat: &Etat, s: &mut SuiviEcoute) {
    let ecoule = s.battement();
    let (courant, en_pause, position) = {
        let p = verrou(&etat.player);
        (
            p.current().map(Path::to_path_buf),
            p.is_paused(),
            p.position(),
        )
    };
    let ecoute = s.avancer(courant, en_pause, position, ecoule, |c| {
        verrou(&etat.lib)
            .pistes_par_chemins(&[c.display().to_string()])
            .ok()
            .and_then(|v| v.first().and_then(|t| t.duration_ms))
    });
    if let Some(chemin) = ecoute {
        if let Err(e) = verrou(&etat.lib).noter_ecoute(&chemin.display().to_string()) {
            tracing::warn!(erreur = %e, "écoute non enregistrée");
        }
    }
}

/// Ce que [`sauvegarder_session`] a déjà écrit de la file : tant qu'elle ne
/// change pas, seules la position et les réglages sont mis à jour — une file
/// de plusieurs milliers de pistes n'est pas réécrite toutes les cinq
/// secondes.
#[derive(Default)]
pub(crate) struct MemoSession {
    empreinte_file: Option<u64>,
}

/// Retient la lecture en cours pour la reprise de session (`Library::session`).
/// Sans effet quand rien ne joue : on ne remplace pas une session valable par
/// du vide.
pub(crate) fn sauvegarder_session(etat: &Etat, memo: &mut MemoSession) -> Result<(), String> {
    use std::hash::{Hash, Hasher};
    let Some(mut i) = verrou(&etat.player).instantane() else {
        return Ok(());
    };
    // Pendant le fondu d'un minuteur, le volume du lecteur est en train de
    // baisser : c'est le volume d'avant qu'on retient, sans quoi on le
    // retrouverait presque éteint au lancement suivant.
    if let Some(nominal) = verrou(&etat.minuteur)
        .as_ref()
        .and_then(minuteur::Minuteur::volume_nominal)
    {
        i.volume = nominal;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    i.file.hash(&mut h);
    i.avant_melange.hash(&mut h);
    let empreinte = h.finish();
    let repetition = i.repetition.nom();
    let position_ms = i.position.as_millis() as u64;

    let lib = verrou(&etat.lib);
    if memo.empreinte_file == Some(empreinte)
        && lib
            .enregistrer_position(i.rang, position_ms, i.alea, repetition, i.volume)
            .map_err(echec)?
    {
        return Ok(());
    }

    // La file a changé (ou la ligne n'existe pas) : écriture complète, en
    // identifiants. Une piste que la bibliothèque ne connaît pas est omise, et
    // le rang recalé : il compte les pistes retenues avant la piste en cours.
    let chemins =
        |v: &[PathBuf]| -> Vec<String> { v.iter().map(|p| p.display().to_string()).collect() };
    let ids = lib.ids_par_chemins(&chemins(&i.file)).map_err(echec)?;
    let (file, rang, _) = retenir(ids, i.rang);
    if file.is_empty() {
        return Ok(());
    }
    let avant_melange: Vec<i64> = lib
        .ids_par_chemins(&chemins(&i.avant_melange))
        .map_err(echec)?
        .into_iter()
        .flatten()
        .collect();
    lib.enregistrer_session(&Session {
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
    Ok(())
}

/// Reprend la session enregistrée : la file revient, la piste en cours est
/// chargée **en pause** à sa position. Jamais de son au lancement.
///
/// Fait en tâche de fond : décoder la piste prend de quoi retarder
/// l'ouverture de la fenêtre. Si l'utilisateur a lancé quelque chose entre-temps,
/// `Player::restaurer` s'efface devant. Rend `true` quand une session a bien
/// été reprise.
pub(crate) fn restaurer_session(etat: &Etat) -> Result<bool, String> {
    let (session, chemins, anciens) = {
        let lib = verrou(&etat.lib);
        let Some(s) = lib.session().map_err(echec)? else {
            return Ok(false);
        };
        let chemins = lib.chemins_par_ids(&s.file).map_err(echec)?;
        let anciens = lib.chemins_par_ids(&s.avant_melange).map_err(echec)?;
        (s, chemins, anciens)
    };
    // Les pistes retirées de la bibliothèque depuis sont omises ; le rang suit
    // la piste en cours, ou la suivante si c'est elle qui a disparu (la
    // position, alors, ne signifie plus rien).
    let (chemins, rang, courante_presente) = retenir(chemins, session.rang);
    let file: Vec<PathBuf> = chemins.into_iter().map(PathBuf::from).collect();
    if file.is_empty() {
        return Ok(false);
    }
    let avant_melange: Vec<PathBuf> = anciens.into_iter().flatten().map(PathBuf::from).collect();
    verrou(&etat.player)
        .restaurer(rusty_music_player::Instantane {
            file,
            avant_melange,
            rang,
            position: if courante_presente {
                Duration::from_millis(session.position_ms)
            } else {
                Duration::ZERO
            },
            alea: session.alea,
            repetition: rusty_music_player::Repetition::depuis_nom(&session.repetition),
            volume: session.volume,
        })
        .map_err(echec)
}

/// Enregistre les touches média du clavier (▶⏸, précédent, suivant) comme
/// raccourcis globaux et relaie chaque appui à l'interface par un évènement —
/// c'est elle qui sait piloter le transport (garde anti-double-appui, sondage
/// de l'état, mise à jour de l'inspecteur), pas ce module.
///
/// **Sans ça, ces trois touches restent au système** : sur macOS, une touche
/// média que personne ne capte ouvre ou pilote l'app Musique à la place —
/// aucune erreur, aucun message, juste la mauvaise application qui répond.
/// macOS demande l'autorisation « Surveillance des saisies » au premier
/// appui pour les capter ; échoue en silence si elle est refusée ou sur une
/// plateforme sans ces touches, un clavier qui n'en a pas n'en a simplement
/// pas besoin.
pub(crate) fn enregistrer_touches_media(app: tauri::AppHandle) {
    use global_hotkey::{
        hotkey::{Code, HotKey},
        GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    };

    let gestionnaire = match GlobalHotKeyManager::new() {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(%e, "gestionnaire de touches média indisponible");
            return;
        }
    };

    let lecture = HotKey::new(None, Code::MediaPlayPause);
    let suivant = HotKey::new(None, Code::MediaTrackNext);
    let precedent = HotKey::new(None, Code::MediaTrackPrevious);
    if let Err(e) = gestionnaire.register_all(&[lecture, suivant, precedent]) {
        tracing::warn!(%e, "échec de l'enregistrement des touches média");
        return;
    }

    let noms: [(u32, &str); 3] = [
        (lecture.id(), "lecture"),
        (suivant.id(), "suivant"),
        (precedent.id(), "precedent"),
    ];

    let pour_fil = app.clone();
    std::thread::spawn(move || {
        let recepteur = GlobalHotKeyEvent::receiver();
        while let Ok(evt) = recepteur.recv() {
            if evt.state() != HotKeyState::Pressed {
                continue; // on agit à l'appui, pas au relâchement
            }
            if let Some((_, nom)) = noms.iter().find(|(id, _)| *id == evt.id()) {
                let _ = pour_fil.emit("touche-media", *nom);
            }
        }
    });

    // Le gestionnaire doit vivre autant que l'application : le laisser
    // tomber couperait l'écoute des touches.
    app.manage(gestionnaire);
}
