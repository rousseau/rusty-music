// SPDX-License-Identifier: GPL-3.0-or-later
//! Pratiquer enchaîne une playlist de morceaux à travailler (P2).
//!
//! À la main : précédent et suivant, jamais de passage automatique (décision
//! du 9 nov. 2026). La playlist en cours et le morceau où l'on en est sont
//! gardés dans la base (`Library::pratique_playlist`), comme la mémoire de
//! Pratiquer ; chaque morceau garde la sienne (E1).
//!
//! Le morceau suivant se **démixe d'avance**, en tâche de fond : une seule
//! cible à la fois (la dernière demandée remplace celle qui attend), jamais
//! en même temps qu'un démixage lancé à la main ou celui du morceau ouvert
//! (V31) : celui-là passe d'abord. S'il démarre pendant le démixage d'avance,
//! celui-ci se met en pause au prochain segment ([`ceder`]), le démixage
//! prioritaire attend cette pause pour commencer ([`attendre_que_l_avance_cede`]),
//! puis le démixage d'avance reprend où il en était — rien n'est perdu ni
//! effacé, ce qui vaut mieux qu'une annulation. Jamais non plus un
//! téléchargement de poids : une variante absente de la machine attend que
//! l'utilisateur la demande. Les stems vont dans le cache ordinaire, où
//! `stems_existants` les retrouve.
//!
//! La charge de la batterie du Mac n'est pas lue : rien dans la pile ne la
//! donne sans processus externe ni liaison IOKit écrite pour l'occasion.

use super::*;
use rusty_music_core::playlists::PlaylistPratique;

/// La playlist que Pratiquer enchaîne, ou `None`.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_playlist(etat: State<Etat>) -> Result<Option<PlaylistPratique>, String> {
    verrou(&etat.lib).pratique_playlist().map_err(echec)
}

/// Pratiquer enchaîne la playlist `id` et en est au morceau `path` (`None` :
/// le premier). Rend la playlist telle que le bandeau la montre, ou `None`
/// si elle n'existe plus.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_playlist_poser(
    etat: State<Etat>,
    id: i64,
    path: Option<String>,
) -> Result<Option<PlaylistPratique>, String> {
    let lib = verrou(&etat.lib);
    if !lib.pratiquer_playlist(id, path.as_deref()).map_err(echec)? {
        return Ok(None);
    }
    lib.pratique_playlist().map_err(echec)
}

/// Pratiquer n'enchaîne plus de playlist.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_playlist_fermer(etat: State<Etat>) -> Result<(), String> {
    verrou(&etat.lib).fermer_pratique_playlist().map_err(echec)
}

/// Où en est le démixage d'avance.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementAvance {
    /// Le prochain à préparer (chemin, variante), s'il attend.
    #[serde(skip)]
    cible: Option<(String, rusty_music_editor::Variante)>,
    /// Le morceau en cours de démixage.
    courant: Option<String>,
    /// Le démixage d'avance cède la place à un démixage prioritaire et ne
    /// calcule pas (V31).
    en_pause: bool,
    /// Ceux dont la préparation a échoué depuis le lancement.
    echecs: Vec<String>,
}
pub(crate) type EtatAvance = Passe<AvancementAvance>;

impl EtatAvance {
    /// Vrai si le morceau `path` est en cours de démixage d'avance.
    pub(crate) fn en_cours_sur(&self, path: &str) -> bool {
        self.avancement.courant.as_deref() == Some(path)
    }
}

/// Ce que l'interface sonde du démixage d'avance.
#[cfg_attr(test, derive(specta::Type))]
#[derive(serde::Serialize)]
pub(crate) struct VueAvance {
    en_cours: bool,
    /// Le morceau en cours de démixage : ses stems ne sont pas encore lisibles.
    courant: Option<String>,
    /// En pause : un démixage lancé à la main passe d'abord.
    en_pause: bool,
    /// Le morceau qui attend son tour.
    attente: Option<String>,
    echecs: Vec<String>,
}

/// Ce que [`pratique_preparer`] a fait de la demande.
#[cfg_attr(test, derive(specta::Type))]
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Avance {
    /// Les stems sont déjà là.
    Pret,
    /// En préparation, ou en attente de son tour.
    Prepare,
    /// Les poids de la variante ne sont pas sur la machine : rien ne se
    /// télécharge en tâche de fond.
    PoidsAbsents,
}

/// Démixe d'avance le morceau `path` avec la variante `variant`, en tâche de
/// fond. Remplace la cible qui attendait ; celle en cours de calcul va au
/// bout.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_preparer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    path: String,
    variant: String,
) -> Result<Avance, String> {
    let variante = rusty_music_editor::Variante::analyser(&variant)
        .ok_or_else(|| format!("variante inconnue : {variant}"))?;
    let source = PathBuf::from(&path);
    if !source.is_file() {
        return Err(format!("{path} est introuvable"));
    }
    if !stems_du_dossier(&dossier_stems(&etat, &source)).is_empty() {
        return Ok(Avance::Pret);
    }
    if !variante.presente() {
        return Ok(Avance::PoidsAbsents);
    }
    {
        let mut a = verrou(&etat.avance);
        if a.avancement.courant.as_deref() == Some(path.as_str()) {
            return Ok(Avance::Prepare);
        }
        a.avancement.cible = Some((path, variante));
        if a.en_cours {
            return Ok(Avance::Prepare);
        }
        a.en_cours = true;
        a.resultat = None;
    }
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.avance);
        let mut demixeur: Option<(rusty_music_editor::Variante, rusty_music_editor::Demixeur)> =
            None;
        loop {
            // Un démixage lancé à la main passe d'abord ; la cible peut
            // changer pendant qu'on l'attend.
            ceder(&etat.demix, &etat.avance);
            let (chemin, variante) = {
                let mut a = verrou(&etat.avance);
                let Some(c) = a.avancement.cible.take() else {
                    a.en_cours = false;
                    a.avancement.courant = None;
                    a.resultat = Some("rien à préparer".into());
                    return;
                };
                a.avancement.courant = Some(c.0.clone());
                c
            };
            let issue = preparer_un(&etat, &chemin, variante, &mut demixeur);
            let mut a = verrou(&etat.avance);
            a.avancement.courant = None;
            if let Err(e) = issue {
                tracing::warn!(%chemin, "démixage d'avance : {e}");
                a.avancement.echecs.push(chemin);
            }
        }
    });
    Ok(Avance::Prepare)
}

fn preparer_un(
    etat: &Etat,
    chemin: &str,
    variante: rusty_music_editor::Variante,
    demixeur: &mut Option<(rusty_music_editor::Variante, rusty_music_editor::Demixeur)>,
) -> Result<(), String> {
    let source = PathBuf::from(chemin);
    let dossier = dossier_stems(etat, &source);
    if !stems_du_dossier(&dossier).is_empty() {
        return Ok(());
    }
    // Chaque étape qui calcule (chargement, chauffe, décodage, puis chaque
    // segment) commence par céder la place à un démixage prioritaire.
    if demixeur.as_ref().map(|(v, _)| *v) != Some(variante) {
        *demixeur = None;
        ceder(&etat.demix, &etat.avance);
        let d = rusty_music_editor::Demixeur::charger(None, variante).map_err(echec)?;
        ceder(&etat.demix, &etat.avance);
        d.chauffer();
        *demixeur = Some((variante, d));
    }
    ceder(&etat.demix, &etat.avance);
    let debut = Instant::now();
    let fichiers = demixeur
        .as_ref()
        .expect("chargé juste au-dessus")
        .1
        .separer_fichier_suivi(&source, &dossier, |_| ceder(&etat.demix, &etat.avance))
        .map_err(echec)?;
    oublier_derives_des_stems(&dossier, &fichiers);
    tracing::info!(%chemin, s = debut.elapsed().as_secs(), "démixé d'avance");
    Ok(())
}

/// Avancement du démixage d'avance. L'interface sonde.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_preparation(etat: State<Etat>) -> VueAvance {
    let a = verrou(&etat.avance);
    VueAvance {
        en_cours: a.en_cours,
        courant: a.avancement.courant.clone(),
        en_pause: a.avancement.en_pause,
        attente: a.avancement.cible.as_ref().map(|(c, _)| c.clone()),
        echecs: a.avancement.echecs.clone(),
    }
}

/// Intervalle des sondages de [`ceder`] et [`attendre_que_l_avance_cede`].
const SONDE: Duration = Duration::from_millis(if cfg!(test) { 2 } else { 200 });

/// Côté démixage d'avance : tant qu'un démixage prioritaire tourne (`demix`
/// en cours), se dit en pause et attend ; rend la main quand il a fini.
///
/// Deux verrous, jamais tenus ensemble. La sortie de pause écrit d'abord
/// `en_pause = false` puis relit `demix` : un démixage prioritaire qui
/// démarre à ce moment-là pose `en_cours` puis lit `en_pause`, et l'un des
/// deux voit forcément l'écriture de l'autre — soit l'avance se remet en
/// pause, soit le prioritaire l'attend. Jamais deux calculs ensemble.
pub(crate) fn ceder(demix: &Mutex<EtatDemix>, avance: &Mutex<EtatAvance>) {
    loop {
        if verrou(demix).en_cours {
            verrou(avance).avancement.en_pause = true;
            std::thread::sleep(SONDE);
            continue;
        }
        {
            let mut a = verrou(avance);
            if !a.avancement.en_pause {
                return;
            }
            a.avancement.en_pause = false;
        }
        if !verrou(demix).en_cours {
            return;
        }
    }
}

/// Côté démixage prioritaire, `demix` déjà marqué en cours : attend que le
/// démixage d'avance qui calcule se soit mis en pause (au plus un segment).
/// Ne garde rien si la passe d'avance est finie, panique comprise.
pub(crate) fn attendre_que_l_avance_cede(avance: &Mutex<EtatAvance>) {
    loop {
        {
            let a = verrou(avance);
            if !a.en_cours || a.avancement.courant.is_none() || a.avancement.en_pause {
                return;
            }
        }
        std::thread::sleep(SONDE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Un « segment » de calcul simulé : compte les calculs simultanés.
    fn segment(actifs: &AtomicUsize, max: &AtomicUsize) {
        let n = actifs.fetch_add(1, Ordering::SeqCst) + 1;
        max.fetch_max(n, Ordering::SeqCst);
        std::thread::sleep(Duration::from_micros(300));
        actifs.fetch_sub(1, Ordering::SeqCst);
    }

    #[test]
    fn jamais_deux_demixages_ensemble_et_l_avance_reprend() {
        let demix = Arc::new(Mutex::new(EtatDemix::default()));
        let avance = Arc::new(Mutex::new(EtatAvance::default()));
        let actifs = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let pause_vue = Arc::new(AtomicBool::new(false));

        // Le démixage d'avance : 400 segments, un point de cession avant
        // chacun, comme le rappel de progression de `preparer_un`.
        {
            let mut a = verrou(&avance);
            a.en_cours = true;
            a.avancement.courant = Some("suivant.flac".into());
        }
        let fond = {
            let (demix, avance, actifs, max) =
                (demix.clone(), avance.clone(), actifs.clone(), max.clone());
            std::thread::spawn(move || {
                let mut faits = 0;
                for _ in 0..400 {
                    ceder(&demix, &avance);
                    segment(&actifs, &max);
                    faits += 1;
                }
                let mut a = verrou(&avance);
                a.en_cours = false;
                a.avancement.courant = None;
                faits
            })
        };

        // Trois démixages « à la main » successifs, lancés pendant l'avance,
        // comme `start_demix` : marquer en cours, attendre la pause, calculer.
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(5));
            verrou(&demix).en_cours = true;
            attendre_que_l_avance_cede(&avance);
            for _ in 0..20 {
                if verrou(&avance).avancement.en_pause {
                    pause_vue.store(true, Ordering::SeqCst);
                }
                segment(&actifs, &max);
            }
            verrou(&demix).en_cours = false;
        }

        assert_eq!(fond.join().unwrap(), 400, "l'avance reprend et va au bout");
        assert_eq!(
            max.load(Ordering::SeqCst),
            1,
            "deux séparations ont calculé ensemble"
        );
        assert!(
            pause_vue.load(Ordering::SeqCst),
            "l'avance ne s'est jamais dite en pause"
        );
        assert!(!verrou(&avance).avancement.en_pause);
    }

    #[test]
    fn rien_a_attendre_sans_avance_qui_calcule() {
        let avance = Mutex::new(EtatAvance::default());
        attendre_que_l_avance_cede(&avance);
        // Passe finie sur une panique : `courant` est resté, `en_cours` non.
        verrou(&avance).avancement.courant = Some("x".into());
        attendre_que_l_avance_cede(&avance);
    }
}
