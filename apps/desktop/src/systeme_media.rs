// SPDX-License-Identifier: GPL-3.0-or-later
//! « En cours de lecture » du système et commandes à distance.
//!
//! Sur macOS, le morceau en cours (titre, artiste, album, durée, position,
//! pochette) s'affiche dans le centre de contrôle et sur l'écran verrouillé, et
//! les touches média, les AirPods et le centre de contrôle pilotent le lecteur —
//! sans l'autorisation « Surveillance des saisies » que demandent les raccourcis
//! globaux (`enregistrer_touches_media`, gardés en repli ailleurs). Branché par
//! `souvlaki` (MIT), qui enveloppe `MPNowPlayingInfoCenter` et
//! `MPRemoteCommandCenter`.
//!
//! Deux moitiés :
//!
//! - **ce qu'il faut annoncer** : [`Annonce::decider`], pure — compare ce que le
//!   lecteur fait à ce que le système sait déjà et rend les actions à faire.
//!   Compilée et testée partout ;
//! - **l'annonce elle-même** : macOS seulement, sur le fil principal.
//!
//! `souvlaki` pose le temps écoulé mais **pas la vitesse de lecture** : le
//! système n'avance donc pas la barre tout seul, et la position est ré-annoncée
//! chaque seconde pendant la lecture.

// Hors macOS, `Annonce::decider` et ce qui l'entoure ne servent qu'aux tests :
// la CI (`clippy -D warnings`, Linux) y verrait du code mort.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Intervalle de ré-annonce de la position pendant la lecture.
const RAFRAICHISSEMENT: Duration = Duration::from_secs(1);
/// Écart entre la position attendue et la position réelle au-delà duquel on
/// parle d'un saut (déplacement dans la piste) : annoncé sans attendre.
const SAUT: Duration = Duration::from_secs(2);

/// Ce que le système affiche d'un morceau.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fiche {
    pub titre: Option<String>,
    pub artiste: Option<String>,
    pub album: Option<String>,
    pub duree: Option<Duration>,
    /// URL `file://` de la pochette, si on a pu en écrire une.
    pub pochette: Option<String>,
}

/// Une chose à dire au système.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nouveau morceau.
    Fiche(Fiche),
    /// État de lecture et position.
    Lecture { en_pause: bool, position: Duration },
    /// Plus rien à jouer.
    Arret,
}

/// Ce que le système sait déjà, pour ne lui dire que ce qui change.
pub struct Annonce {
    piste: Option<PathBuf>,
    en_pause: bool,
    arrete: bool,
    position: Duration,
    depuis: Instant,
}

impl Annonce {
    pub fn nouveau(maintenant: Instant) -> Self {
        Self {
            piste: None,
            en_pause: true,
            arrete: false,
            position: Duration::ZERO,
            depuis: maintenant,
        }
    }

    /// Les actions à faire pour que le système rejoigne l'état du lecteur.
    /// `fiche` n'est appelée que pour un nouveau morceau — elle peut lire la
    /// base et écrire une pochette, ce qui coûte.
    pub fn decider(
        &mut self,
        courant: Option<&Path>,
        en_pause: bool,
        position: Duration,
        maintenant: Instant,
        fiche: impl FnOnce(&Path) -> Fiche,
    ) -> Vec<Action> {
        let Some(chemin) = courant else {
            if self.arrete {
                return Vec::new();
            }
            self.arrete = true;
            self.piste = None;
            return vec![Action::Arret];
        };

        let mut actions = Vec::new();
        let nouvelle = self.piste.as_deref() != Some(chemin);
        if nouvelle {
            actions.push(Action::Fiche(fiche(chemin)));
            self.piste = Some(chemin.to_path_buf());
        }

        let ecoule = maintenant.duration_since(self.depuis);
        // Où la position devrait être si rien n'avait bougé depuis la dernière annonce.
        let attendue = if self.en_pause || self.arrete {
            self.position
        } else {
            self.position + ecoule
        };
        let saut = position.abs_diff(attendue) > SAUT;
        let a_dire = nouvelle
            || self.arrete
            || en_pause != self.en_pause
            || saut
            || (!en_pause && ecoule >= RAFRAICHISSEMENT);
        if a_dire {
            actions.push(Action::Lecture { en_pause, position });
            self.en_pause = en_pause;
            self.arrete = false;
            self.position = position;
            self.depuis = maintenant;
        }
        actions
    }
}

/// `file://` + le chemin, percent-encodé : `NSURL` refuse une espace, et le
/// dossier de données de l'application en contient une (« Application Support »).
pub fn url_fichier(chemin: &Path) -> String {
    let mut url = String::from("file://");
    for octet in chemin.to_string_lossy().bytes() {
        match octet {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                url.push(octet as char)
            }
            o => url.push_str(&format!("%{o:02X}")),
        }
    }
    url
}

#[cfg(target_os = "macos")]
pub use macos::{annoncer, demarrer};

#[cfg(not(target_os = "macos"))]
pub use autres::{annoncer, demarrer};

/// Ailleurs que sur macOS : rien à annoncer, les raccourcis globaux restent.
#[cfg(not(target_os = "macos"))]
mod autres {
    use super::Annonce;
    use crate::Etat;

    pub fn demarrer(_app: &tauri::AppHandle) -> bool {
        false
    }

    pub fn annoncer(_app: &tauri::AppHandle, _etat: &Etat, _annonce: &mut Annonce) {}
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use souvlaki::{
        MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition,
        PlatformConfig, SeekDirection,
    };
    use tauri::{AppHandle, Emitter, Manager};

    use super::{url_fichier, Action, Annonce, Fiche};
    use crate::{cover_locale, empreinte, verrou, Etat};

    /// Les commandes à distance, tenues en vie : lâcher `MediaControls` les
    /// détache.
    pub struct Systeme(Mutex<MediaControls>);

    /// `RUSTY_MUSIC_TOUCHES_GLOBALES=1` force le retour aux raccourcis globaux
    /// (touches média captées par l'application, avec l'autorisation
    /// « Surveillance des saisies ») : une issue si le système ne livre pas les
    /// commandes à distance à l'application.
    const REPLI: &str = "RUSTY_MUSIC_TOUCHES_GLOBALES";

    /// Saut d'un « avance/recul » sans durée (touche, AirPods) : dix secondes.
    const PAS: Duration = Duration::from_secs(10);

    /// Branche les commandes à distance. Rend `false` — et laisse alors les
    /// raccourcis globaux prendre le relais — si le repli est demandé ou si le
    /// système refuse.
    pub fn demarrer(app: &AppHandle) -> bool {
        if std::env::var_os(REPLI).is_some() {
            tracing::info!("{REPLI} : touches média par raccourcis globaux");
            return false;
        }
        let mut controles = match MediaControls::new(PlatformConfig {
            display_name: "Rusty Music",
            dbus_name: "fm.rustymusic.desktop",
            hwnd: None,
        }) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(%e, "commandes à distance du système indisponibles");
                return false;
            }
        };
        let pour_evenements = app.clone();
        if let Err(e) = controles.attach(move |evenement| traiter(&pour_evenements, evenement)) {
            tracing::warn!(%e, "commandes à distance du système non branchées");
            return false;
        }
        app.manage(Systeme(Mutex::new(controles)));
        true
    }

    /// Une commande venue du système. Lecture, pause, suivant et précédent
    /// repassent par l'évènement des raccourcis globaux : c'est l'interface qui
    /// sait piloter le transport, garde anti-double-appui comprise.
    fn traiter(app: &AppHandle, evenement: MediaControlEvent) {
        let Some(etat) = app.try_state::<Etat>() else {
            return;
        };
        let (en_pause, position) = {
            let p = verrou(&etat.player);
            (p.is_paused(), p.position())
        };
        // Lecture et pause sont distinctes pour le système (AirPods) mais l'interface
        // n'a qu'une bascule : on n'envoie que si elle changerait quelque chose.
        let basculer = match &evenement {
            MediaControlEvent::Toggle => true,
            MediaControlEvent::Play => en_pause,
            MediaControlEvent::Pause | MediaControlEvent::Stop => !en_pause,
            _ => false,
        };
        if basculer {
            let _ = app.emit("touche-media", "lecture");
            return;
        }
        match evenement {
            MediaControlEvent::Next => {
                let _ = app.emit("touche-media", "suivant");
            }
            MediaControlEvent::Previous => {
                let _ = app.emit("touche-media", "precedent");
            }
            MediaControlEvent::SetPosition(MediaPosition(p)) => aller_a(&etat, p),
            MediaControlEvent::SeekBy(sens, d) => aller_a(&etat, decaler(position, sens, d)),
            MediaControlEvent::Seek(sens) => aller_a(&etat, decaler(position, sens, PAS)),
            _ => {}
        }
    }

    fn decaler(position: Duration, sens: SeekDirection, d: Duration) -> Duration {
        match sens {
            SeekDirection::Forward => position + d,
            SeekDirection::Backward => position.saturating_sub(d),
        }
    }

    fn aller_a(etat: &Etat, position: Duration) {
        if let Err(e) = verrou(&etat.player).seek(position) {
            tracing::debug!(%e, "déplacement demandé par le système impossible");
        }
    }

    /// La fiche du morceau : tags de la bibliothèque, et une pochette écrite
    /// dans un fichier — `souvlaki` charge l'image depuis une URL.
    fn fiche(etat: &Etat, chemin: &Path) -> Fiche {
        let ligne = verrou(&etat.lib)
            .pistes_par_chemins(&[chemin.display().to_string()])
            .ok()
            .and_then(|v| v.into_iter().next());
        let mut f = Fiche::default();
        if let Some(t) = ligne {
            f.titre = t.title;
            f.artiste = t.artist;
            f.album = t.album;
            f.duree = t
                .duration_ms
                .filter(|&ms| ms > 0)
                .map(|ms| Duration::from_millis(ms as u64));
        }
        f.pochette = ecrire_pochette(etat, chemin);
        f
    }

    /// Écrit la pochette locale du morceau sous `<données>/en-cours/` et rend son
    /// URL. Un fichier par morceau, les plus récents seuls gardés : le système
    /// charge l'image après coup, l'écraser sous son nez la lui ferait manquer.
    fn ecrire_pochette(etat: &Etat, chemin: &Path) -> Option<String> {
        let c = cover_locale(&etat.db, chemin)?;
        let extension = match c.mime.as_deref() {
            Some("image/png") => "png",
            Some("image/webp") => "webp",
            _ => "jpg",
        };
        let dossier = etat.db.with_file_name("en-cours");
        std::fs::create_dir_all(&dossier).ok()?;
        let fichier = dossier.join(format!(
            "{:016x}.{extension}",
            empreinte(chemin.to_string_lossy().as_bytes())
        ));
        std::fs::write(&fichier, &c.data).ok()?;
        elaguer(&dossier, &fichier);
        Some(url_fichier(&fichier))
    }

    /// Ne garde que les deux fichiers les plus récents (le courant et le précédent).
    fn elaguer(dossier: &Path, courant: &Path) {
        let Ok(entrees) = std::fs::read_dir(dossier) else {
            return;
        };
        let mut fichiers: Vec<_> = entrees
            .flatten()
            .filter(|e| e.path() != courant)
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        fichiers.sort();
        let en_trop = fichiers.len().saturating_sub(1);
        for (_, ancien) in fichiers.into_iter().take(en_trop) {
            let _ = std::fs::remove_file(ancien);
        }
    }

    /// Dit au système où en est le lecteur. Appelé par le fil de fond ; les appels
    /// à `souvlaki` partent sur le fil principal.
    pub fn annoncer(app: &AppHandle, etat: &Etat, annonce: &mut Annonce) {
        let (courant, en_pause, position) = {
            let p = verrou(&etat.player);
            (p.current().map(Path::to_path_buf), p.is_paused(), p.position())
        };
        let actions = annonce.decider(
            courant.as_deref(),
            en_pause,
            position,
            Instant::now(),
            |c| fiche(etat, c),
        );
        if actions.is_empty() {
            return;
        }
        let app_principal = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(systeme) = app_principal.try_state::<Systeme>() else {
                return;
            };
            let mut controles = systeme.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for action in &actions {
                let resultat = match action {
                    Action::Fiche(f) => controles.set_metadata(MediaMetadata {
                        title: f.titre.as_deref(),
                        album: f.album.as_deref(),
                        artist: f.artiste.as_deref(),
                        cover_url: f.pochette.as_deref(),
                        duration: f.duree,
                    }),
                    Action::Lecture { en_pause, position } => {
                        let progress = Some(MediaPosition(*position));
                        controles.set_playback(if *en_pause {
                            MediaPlayback::Paused { progress }
                        } else {
                            MediaPlayback::Playing { progress }
                        })
                    }
                    Action::Arret => controles.set_playback(MediaPlayback::Stopped),
                };
                if let Err(e) = resultat {
                    tracing::debug!(%e, "annonce au système impossible");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fiche_de(titre: &str) -> impl FnOnce(&Path) -> Fiche {
        let titre = titre.to_string();
        move |_| Fiche {
            titre: Some(titre),
            ..Fiche::default()
        }
    }

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn un_nouveau_morceau_annonce_sa_fiche_puis_la_lecture() {
        let t0 = Instant::now();
        let mut a = Annonce::nouveau(t0);
        let actions = a.decider(Some(Path::new("/m/a")), false, s(0), t0, fiche_de("A"));
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[0], Action::Fiche(f) if f.titre.as_deref() == Some("A")));
        assert_eq!(actions[1], Action::Lecture { en_pause: false, position: s(0) });
    }

    #[test]
    fn la_position_est_reannoncee_chaque_seconde_pas_plus_souvent() {
        let t0 = Instant::now();
        let mut a = Annonce::nouveau(t0);
        a.decider(Some(Path::new("/m/a")), false, s(0), t0, fiche_de("A"));
        let rien = |a: &mut Annonce, dt_ms: u64, pos_ms: u64| {
            a.decider(
                Some(Path::new("/m/a")),
                false,
                Duration::from_millis(pos_ms),
                t0 + Duration::from_millis(dt_ms),
                |_| unreachable!("même morceau : pas de nouvelle fiche"),
            )
        };
        assert!(rien(&mut a, 500, 500).is_empty());
        let un = rien(&mut a, 1000, 1000);
        assert_eq!(un, vec![Action::Lecture { en_pause: false, position: s(1) }]);
        assert!(rien(&mut a, 1500, 1500).is_empty(), "le compteur repart à la dernière annonce");
    }

    #[test]
    fn la_pause_s_annonce_une_fois_puis_se_tait() {
        let t0 = Instant::now();
        let mut a = Annonce::nouveau(t0);
        a.decider(Some(Path::new("/m/a")), false, s(0), t0, fiche_de("A"));
        let pause = a.decider(Some(Path::new("/m/a")), true, s(3), t0 + s(3), |_| unreachable!());
        assert_eq!(pause, vec![Action::Lecture { en_pause: true, position: s(3) }]);
        // Longtemps après, toujours en pause à la même place : rien à dire.
        for k in 4..30 {
            let r = a.decider(Some(Path::new("/m/a")), true, s(3), t0 + s(k), |_| unreachable!());
            assert!(r.is_empty(), "rien à annoncer en pause immobile (t = {k})");
        }
    }

    #[test]
    fn un_deplacement_dans_la_piste_est_annonce_sans_attendre() {
        let t0 = Instant::now();
        let mut a = Annonce::nouveau(t0);
        a.decider(Some(Path::new("/m/a")), false, s(0), t0, fiche_de("A"));
        let r = a.decider(
            Some(Path::new("/m/a")),
            false,
            s(120),
            t0 + Duration::from_millis(200),
            |_| unreachable!(),
        );
        assert_eq!(r, vec![Action::Lecture { en_pause: false, position: s(120) }]);
    }

    #[test]
    fn changer_de_morceau_refait_la_fiche() {
        let t0 = Instant::now();
        let mut a = Annonce::nouveau(t0);
        a.decider(Some(Path::new("/m/a")), false, s(0), t0, fiche_de("A"));
        let r = a.decider(Some(Path::new("/m/b")), false, s(0), t0 + s(1), fiche_de("B"));
        assert!(matches!(&r[0], Action::Fiche(f) if f.titre.as_deref() == Some("B")));
    }

    #[test]
    fn l_arret_s_annonce_une_fois_et_la_reprise_repart_de_zero() {
        let t0 = Instant::now();
        let mut a = Annonce::nouveau(t0);
        // Rien n'a jamais joué : on dit tout de même « arrêté » une fois.
        assert_eq!(a.decider(None, true, s(0), t0, |_| unreachable!()), vec![Action::Arret]);
        assert!(a.decider(None, true, s(0), t0 + s(1), |_| unreachable!()).is_empty());
        let r = a.decider(Some(Path::new("/m/a")), false, s(0), t0 + s(2), fiche_de("A"));
        assert_eq!(r.len(), 2, "fiche puis lecture");
    }

    #[test]
    fn l_url_de_pochette_est_encodee_et_sans_espace() {
        let url = url_fichier(Path::new(
            "/Users/a/Library/Application Support/fm.rustymusic.desktop/en-cours/00ff.jpg",
        ));
        assert_eq!(
            url,
            "file:///Users/a/Library/Application%20Support/fm.rustymusic.desktop/en-cours/00ff.jpg"
        );
        assert!(!url.contains(' '));
        assert_eq!(url_fichier(Path::new("/m/é#?.png")), "file:///m/%C3%A9%23%3F.png");
    }
}
