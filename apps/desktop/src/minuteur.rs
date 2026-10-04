// SPDX-License-Identifier: GPL-3.0-or-later
//! Minuteur d'arrêt : « arrête la musique dans 30 minutes », « à la fin de ce
//! morceau », « à la fin de cet album » — pour s'endormir en écoutant, ou finir
//! un disque sans enchaîner sur le suivant.
//!
//! **Côté moteur, pas côté page** : une fenêtre masquée ou un écran éteint
//! ralentit les temporisateurs de la webview, et un minuteur de coucher est
//! précisément ce cas-là.
//!
//! Ce module est une machine d'états **pure** : [`Minuteur::avancer`] reçoit une
//! photo du lecteur ([`Lecteur`]) et rend ce qu'il faut en faire ([`Action`]).
//! Le fil qui la fait tourner et applique les actions est dans `main.rs`.
//!
//! Trois modes :
//!
//! - **dans N minutes** : le volume descend pendant les [`FONDU`] dernières
//!   secondes, puis pause à la position atteinte, volume remis. Compté en temps
//!   réel, pause comprise (un minuteur de chevet). Jamais conservé d'un
//!   lancement à l'autre ;
//! - **à la fin du morceau** : pause au passage au morceau suivant — au plus
//!   quelques millisecondes de celui-ci sont audibles, d'où le battement fin
//!   ([`Minuteur::battement`]) sur la dernière seconde ;
//! - **à la fin de l'album** : pareil, au passage hors de l'album. Si l'on
//!   quitte l'album avant d'en atteindre le dernier morceau (autre lecture
//!   lancée), le minuteur s'efface sans rien arrêter.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Durée de la baisse de volume qui précède l'arrêt d'une durée fixe.
pub const FONDU: Duration = Duration::from_secs(10);

/// Dernière fenêtre d'un morceau où l'on veille à 20 ms plutôt qu'à 100 ms :
/// la pause doit tomber juste après la dernière note, pas dans le suivant.
const FENETRE_FINE: Duration = Duration::from_secs(2);

/// Distance à la fin d'un morceau, **connue de façon fiable**, en deçà de
/// laquelle on met en pause sans attendre le morceau suivant. Le lecteur audio
/// met quelques dizaines de millisecondes à appliquer une pause : attendre le
/// changement de morceau laissait entendre ~35 ms du suivant, un « tic » sur une
/// attaque franche. Les dernières 40 ms d'un morceau sont, en pratique, une
/// queue ou du silence. Le changement de morceau reste le filet quand la durée
/// n'est pas connue.
const MARGE_FIN: Duration = Duration::from_millis(40);

/// Saut en arrière de la position, en répétition d'un seul morceau, qui signale
/// que le morceau recommence.
const RECOMMENCE: Duration = Duration::from_secs(5);

/// Ce que le minuteur sait du lecteur à un instant donné.
#[derive(Debug, Clone)]
pub struct Lecteur {
    /// Morceau en cours, `None` quand la file est épuisée.
    pub courant: Option<PathBuf>,
    pub position: Duration,
    /// Durée **décodée** du morceau en cours, si connue (`Player::duree_courante`),
    /// pas celle des tags : on vise la fin qu'on entend vraiment.
    pub duree: Option<Duration>,
    pub volume: f32,
    /// Répétition d'un seul morceau : le chemin ne change pas à la boucle.
    pub repetition_une: bool,
}

/// Ce que l'appelant doit faire du lecteur.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Rien à faire.
    Rien,
    /// Régler le volume (fondu en cours).
    Volume(f32),
    /// Mettre en pause ; puis remettre ce volume s'il y en a un à remettre.
    /// Le minuteur est terminé.
    Pause { rendre_volume: Option<f32> },
    /// Le minuteur s'efface sans rien arrêter ; remettre ce volume s'il y en a un.
    Annuler { rendre_volume: Option<f32> },
}

/// Ce que l'interface en affiche.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EtatMinuteur {
    /// « duree », « piste » ou « album ».
    pub mode: &'static str,
    /// Temps avant l'arrêt, quand on peut le dire : pour une durée fixe, celui
    /// qui reste ; pour un morceau, ce qu'il en reste à jouer ; pour un album,
    /// le reste du morceau en cours seulement (`None` hors du dernier morceau).
    pub reste_ms: Option<u64>,
}

#[derive(Debug, Clone)]
enum Mode {
    Duree {
        echeance: Instant,
        /// Durée du fondu : [`FONDU`], ou la durée entière d'un minuteur plus
        /// court — sans quoi le volume chuterait d'un coup au départ.
        fondu: Duration,
        /// Volume avant le fondu, fixé à son début.
        volume_nominal: Option<f32>,
    },
    Piste {
        cible: PathBuf,
        derniere_position: Duration,
    },
    Album {
        /// Morceaux de l'album encore à venir, morceau en cours compris.
        morceaux: HashSet<PathBuf>,
        /// Dernier morceau de l'album : l'arrêt vient après lui.
        dernier: PathBuf,
        /// Vrai une fois le dernier morceau commencé : à partir de là, quitter
        /// l'album veut dire qu'il est fini.
        atteint: bool,
        derniere_position: Duration,
    },
}

#[derive(Debug, Clone)]
pub struct Minuteur {
    mode: Mode,
}

impl Minuteur {
    /// Arrêt dans `duree`.
    pub fn dans(maintenant: Instant, duree: Duration) -> Self {
        Self {
            mode: Mode::Duree {
                echeance: maintenant + duree,
                fondu: FONDU.min(duree),
                volume_nominal: None,
            },
        }
    }

    /// Arrêt à la fin du morceau `cible`.
    pub fn fin_de_piste(cible: PathBuf, position: Duration) -> Self {
        Self {
            mode: Mode::Piste {
                cible,
                derniere_position: position,
            },
        }
    }

    /// Arrêt à la fin de l'album : `morceaux` sont ceux qui restent à jouer de
    /// l'album (le morceau en cours en tête), `dernier` le dernier d'entre eux.
    pub fn fin_d_album(morceaux: Vec<PathBuf>, position: Duration) -> Option<Self> {
        let dernier = morceaux.last()?.clone();
        Some(Self {
            mode: Mode::Album {
                morceaux: morceaux.into_iter().collect(),
                dernier,
                atteint: false,
                derniere_position: position,
            },
        })
    }

    /// Le volume d'avant le fondu, **pendant** le fondu seulement : c'est lui
    /// qu'il faut enregistrer dans la session, pas le volume en train de baisser.
    pub fn volume_nominal(&self) -> Option<f32> {
        match &self.mode {
            Mode::Duree { volume_nominal, .. } => *volume_nominal,
            _ => None,
        }
    }

    /// Annulation demandée : le volume à remettre si un fondu était en cours.
    pub fn annuler(&self) -> Option<f32> {
        self.volume_nominal()
    }

    /// Délai à attendre avant le prochain appel à [`Self::avancer`] : fin dans
    /// la dernière fenêtre d'un morceau (modes morceau et album), 100 ms sinon.
    pub fn battement(&self, lecteur: &Lecteur) -> Duration {
        let fin_proche = match (&self.mode, lecteur.duree) {
            (Mode::Piste { .. } | Mode::Album { .. }, Some(d)) => {
                d.saturating_sub(lecteur.position) <= FENETRE_FINE
            }
            // Fin inconnue : le seul repère est le changement de morceau, qu'on
            // ne veut pas détecter avec 100 ms de retard. On veille serré.
            (Mode::Piste { .. } | Mode::Album { .. }, None) => true,
            _ => false,
        };
        if fin_proche {
            Duration::from_millis(20)
        } else {
            Duration::from_millis(100)
        }
    }

    /// Fait avancer le minuteur. Rend l'action à appliquer, et `true` quand le
    /// minuteur est terminé (à retirer).
    pub fn avancer(&mut self, maintenant: Instant, lecteur: &Lecteur) -> (Action, bool) {
        match &mut self.mode {
            Mode::Duree {
                echeance,
                fondu,
                volume_nominal,
            } => {
                if maintenant >= *echeance {
                    return (
                        Action::Pause {
                            rendre_volume: volume_nominal.take(),
                        },
                        true,
                    );
                }
                let reste = *echeance - maintenant;
                if reste > *fondu {
                    return (Action::Rien, false);
                }
                // Début du fondu : on retient le volume d'avant.
                let nominal = *volume_nominal.get_or_insert(lecteur.volume);
                // Carré : la courbe linéaire paraît tomber d'un coup à la fin.
                let f = reste.as_secs_f32() / fondu.as_secs_f32();
                (Action::Volume(nominal * f * f), false)
            }

            Mode::Piste {
                cible,
                derniere_position,
            } => {
                let Some(courant) = &lecteur.courant else {
                    // La file s'est épuisée d'elle-même : rien à arrêter.
                    return (Action::Annuler { rendre_volume: None }, true);
                };
                let recommence = lecteur.repetition_une
                    && courant == cible
                    && lecteur.position + RECOMMENCE < *derniere_position;
                *derniere_position = lecteur.position;
                if courant != cible || recommence || (courant == cible && fin_imminente(lecteur)) {
                    return (Action::Pause { rendre_volume: None }, true);
                }
                (Action::Rien, false)
            }

            Mode::Album {
                morceaux,
                dernier,
                atteint,
                derniere_position,
            } => {
                let Some(courant) = &lecteur.courant else {
                    return (Action::Annuler { rendre_volume: None }, true);
                };
                let recommence = lecteur.repetition_une
                    && courant == dernier
                    && lecteur.position + RECOMMENCE < *derniere_position;
                *derniere_position = lecteur.position;
                if morceaux.contains(courant) {
                    if courant == dernier {
                        *atteint = true;
                    }
                    // Dans l'album ; seuls la fin du dernier morceau et sa boucle
                    // l'arrêtent.
                    if recommence || (courant == dernier && fin_imminente(lecteur)) {
                        return (Action::Pause { rendre_volume: None }, true);
                    }
                    return (Action::Rien, false);
                }
                // Sorti de l'album. Après son dernier morceau, il est fini : on
                // s'arrête. Avant, une autre lecture a pris la main (un autre
                // album, un saut ailleurs) et le minuteur n'a plus de sens.
                if *atteint {
                    (Action::Pause { rendre_volume: None }, true)
                } else {
                    (Action::Annuler { rendre_volume: None }, true)
                }
            }
        }
    }

    /// Ce que l'interface affiche de ce minuteur.
    pub fn etat(&self, maintenant: Instant, lecteur: &Lecteur) -> EtatMinuteur {
        let reste_piste = || {
            lecteur
                .duree
                .map(|d| d.saturating_sub(lecteur.position).as_millis() as u64)
        };
        match &self.mode {
            Mode::Duree { echeance, .. } => EtatMinuteur {
                mode: "duree",
                reste_ms: Some(echeance.saturating_duration_since(maintenant).as_millis() as u64),
            },
            Mode::Piste { .. } => EtatMinuteur {
                mode: "piste",
                reste_ms: reste_piste(),
            },
            Mode::Album { dernier, .. } => EtatMinuteur {
                mode: "album",
                reste_ms: if lecteur.courant.as_ref() == Some(dernier) {
                    reste_piste()
                } else {
                    None
                },
            },
        }
    }
}

/// La fin du morceau en cours est à moins de [`MARGE_FIN`] — la durée décodée
/// doit être connue : sans elle, on ne vise rien.
fn fin_imminente(lecteur: &Lecteur) -> bool {
    lecteur
        .duree
        .is_some_and(|d| d.saturating_sub(lecteur.position) <= MARGE_FIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }
    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }
    fn lecteur(courant: Option<&str>, position: Duration) -> Lecteur {
        Lecteur {
            courant: courant.map(p),
            position,
            duree: Some(s(200)),
            volume: 0.8,
            repetition_une: false,
        }
    }

    #[test]
    fn la_duree_ne_fait_rien_avant_le_fondu() {
        let t0 = Instant::now();
        let mut m = Minuteur::dans(t0, s(30 * 60));
        let (a, fini) = m.avancer(t0 + s(60), &lecteur(Some("/m/a"), s(5)));
        assert_eq!((a, fini), (Action::Rien, false));
        assert_eq!(m.volume_nominal(), None);
    }

    #[test]
    fn le_fondu_descend_de_la_valeur_nominale_a_zero() {
        let t0 = Instant::now();
        let mut m = Minuteur::dans(t0, s(100));
        // Début du fondu (10 s avant l'échéance) : volume nominal, retenu.
        let (a, _) = m.avancer(t0 + s(90), &lecteur(Some("/m/a"), s(5)));
        assert_eq!(a, Action::Volume(0.8));
        assert_eq!(m.volume_nominal(), Some(0.8));
        // À mi-fondu : 0,8 × 0,5² = 0,2 ; le lecteur annonce son volume déjà
        // baissé, mais le nominal retenu ne change pas.
        let mut l = lecteur(Some("/m/a"), s(10));
        l.volume = 0.5;
        let (a, _) = m.avancer(t0 + s(95), &l);
        assert!(matches!(a, Action::Volume(v) if (v - 0.2).abs() < 1e-6), "{a:?}");
        assert_eq!(m.volume_nominal(), Some(0.8));
        // Le volume ne remonte jamais pendant le fondu.
        let mut dernier = f32::MAX;
        for ms in (0..10_000).step_by(250) {
            if let (Action::Volume(v), _) = m.avancer(t0 + s(90) + Duration::from_millis(ms), &l) {
                assert!(v <= dernier + 1e-6, "le volume remonte à t = {ms} ms");
                dernier = v;
            }
        }
    }

    #[test]
    fn un_minuteur_plus_court_que_le_fondu_ne_fait_pas_chuter_le_volume() {
        let t0 = Instant::now();
        let mut m = Minuteur::dans(t0, s(4));
        // Au premier tour, le volume part de sa valeur nominale, pas d'une
        // fraction : le fondu s'étale sur les 4 secondes, pas sur 10.
        let (a, _) = m.avancer(t0, &lecteur(Some("/m/a"), s(0)));
        assert!(matches!(a, Action::Volume(v) if (v - 0.8).abs() < 1e-6), "{a:?}");
        let (a, _) = m.avancer(t0 + s(2), &lecteur(Some("/m/a"), s(2)));
        assert!(matches!(a, Action::Volume(v) if (v - 0.2).abs() < 1e-6), "{a:?}");
    }

    #[test]
    fn a_l_echeance_pause_et_volume_remis() {
        let t0 = Instant::now();
        let mut m = Minuteur::dans(t0, s(100));
        m.avancer(t0 + s(95), &lecteur(Some("/m/a"), s(5)));
        let (a, fini) = m.avancer(t0 + s(100), &lecteur(Some("/m/a"), s(10)));
        assert_eq!(a, Action::Pause { rendre_volume: Some(0.8) });
        assert!(fini);
    }

    #[test]
    fn une_echeance_sans_fondu_commence_ne_remet_rien() {
        // Minuteur très court : jamais passé par le fondu avant l'échéance.
        let t0 = Instant::now();
        let mut m = Minuteur::dans(t0, s(5));
        let (a, fini) = m.avancer(t0 + s(6), &lecteur(Some("/m/a"), s(5)));
        assert_eq!(a, Action::Pause { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn annuler_pendant_le_fondu_rend_le_volume() {
        let t0 = Instant::now();
        let mut m = Minuteur::dans(t0, s(100));
        assert_eq!(m.annuler(), None, "pas de fondu : rien à remettre");
        m.avancer(t0 + s(92), &lecteur(Some("/m/a"), s(5)));
        assert_eq!(m.annuler(), Some(0.8));
    }

    #[test]
    fn la_duree_compte_aussi_en_pause() {
        let t0 = Instant::now();
        let mut m = Minuteur::dans(t0, s(60));
        // Le lecteur est en pause depuis longtemps : l'échéance tombe quand même.
        let (a, fini) = m.avancer(t0 + s(61), &lecteur(Some("/m/a"), s(5)));
        assert!(matches!(a, Action::Pause { .. }) && fini);
    }

    #[test]
    fn fin_de_piste_attend_puis_pause_au_changement() {
        let t0 = Instant::now();
        let mut m = Minuteur::fin_de_piste(p("/m/a"), s(10));
        for pos in [11, 100, 199] {
            let (a, fini) = m.avancer(t0, &lecteur(Some("/m/a"), s(pos)));
            assert_eq!((a, fini), (Action::Rien, false));
        }
        let (a, fini) = m.avancer(t0, &lecteur(Some("/m/b"), Duration::from_millis(10)));
        assert_eq!(a, Action::Pause { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn fin_de_piste_met_en_pause_juste_avant_la_fin_sans_attendre_le_suivant() {
        let t0 = Instant::now();
        let mut m = Minuteur::fin_de_piste(p("/m/a"), s(0));
        // À 1 s de la fin (durée 200 s) : on laisse jouer.
        assert_eq!(m.avancer(t0, &lecteur(Some("/m/a"), s(199))).0, Action::Rien);
        // À 41 ms : pas encore. À 40 ms : pause, **sur le même morceau**.
        assert_eq!(
            m.avancer(t0, &lecteur(Some("/m/a"), Duration::from_millis(199_959))).0,
            Action::Rien
        );
        let (a, fini) = m.avancer(t0, &lecteur(Some("/m/a"), Duration::from_millis(199_960)));
        assert_eq!(a, Action::Pause { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn sans_duree_connue_la_fin_de_piste_attend_le_changement_de_morceau() {
        let t0 = Instant::now();
        let mut m = Minuteur::fin_de_piste(p("/m/a"), s(0));
        let mut l = lecteur(Some("/m/a"), s(199));
        l.duree = None;
        // Aucune fin visée : on ne coupe pas un morceau à l'aveugle.
        assert_eq!(m.avancer(t0, &l).0, Action::Rien);
        l.position = Duration::from_millis(199_990);
        assert_eq!(m.avancer(t0, &l).0, Action::Rien);
        l.courant = Some(p("/m/b"));
        l.position = Duration::from_millis(10);
        assert_eq!(m.avancer(t0, &l).0, Action::Pause { rendre_volume: None });
    }

    #[test]
    fn fin_d_album_vise_la_fin_du_dernier_morceau_seulement() {
        let t0 = Instant::now();
        let mut m = Minuteur::fin_d_album(vec![p("/m/1"), p("/m/2")], s(0)).expect("minuteur");
        // Fin du premier morceau : l'album continue.
        assert_eq!(
            m.avancer(t0, &lecteur(Some("/m/1"), Duration::from_millis(199_990))).0,
            Action::Rien
        );
        // Fin du dernier : pause juste avant, sans sortir de l'album.
        let (a, fini) = m.avancer(t0, &lecteur(Some("/m/2"), Duration::from_millis(199_990)));
        assert_eq!(a, Action::Pause { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn fin_de_piste_en_repetition_d_un_morceau_s_arrete_a_la_boucle() {
        let t0 = Instant::now();
        let mut m = Minuteur::fin_de_piste(p("/m/a"), s(0));
        let mut l = lecteur(Some("/m/a"), s(190));
        l.repetition_une = true;
        assert_eq!(m.avancer(t0, &l).0, Action::Rien);
        l.position = Duration::from_millis(20); // le même chemin, depuis le début
        let (a, fini) = m.avancer(t0, &l);
        assert_eq!(a, Action::Pause { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn sans_repetition_un_retour_en_arriere_n_arrete_rien() {
        // L'utilisateur revient au début du morceau avec « précédent » : même
        // morceau, minuteur toujours valable.
        let t0 = Instant::now();
        let mut m = Minuteur::fin_de_piste(p("/m/a"), s(0));
        m.avancer(t0, &lecteur(Some("/m/a"), s(190)));
        assert_eq!(m.avancer(t0, &lecteur(Some("/m/a"), s(0))).0, Action::Rien);
    }

    #[test]
    fn la_file_epuisee_efface_le_minuteur_sans_rien_arreter() {
        let t0 = Instant::now();
        let mut m = Minuteur::fin_de_piste(p("/m/a"), s(0));
        let (a, fini) = m.avancer(t0, &lecteur(None, s(0)));
        assert_eq!(a, Action::Annuler { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn fin_d_album_laisse_jouer_tout_l_album_puis_pause() {
        let t0 = Instant::now();
        let mut m =
            Minuteur::fin_d_album(vec![p("/m/1"), p("/m/2"), p("/m/3")], s(0)).expect("minuteur");
        for piste in ["/m/1", "/m/2", "/m/3"] {
            assert_eq!(m.avancer(t0, &lecteur(Some(piste), s(5))).0, Action::Rien, "{piste}");
        }
        let (a, fini) = m.avancer(t0, &lecteur(Some("/m/autre-album"), s(0)));
        assert_eq!(a, Action::Pause { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn quitter_l_album_avant_sa_fin_efface_le_minuteur_sans_rien_arreter() {
        let t0 = Instant::now();
        let mut m = Minuteur::fin_d_album(vec![p("/m/1"), p("/m/2"), p("/m/3")], s(0)).expect("minuteur");
        assert_eq!(m.avancer(t0, &lecteur(Some("/m/1"), s(5))).0, Action::Rien);
        // Une autre lecture est lancée avant d'arriver au dernier morceau.
        let (a, fini) = m.avancer(t0, &lecteur(Some("/autre/1"), s(0)));
        assert_eq!(a, Action::Annuler { rendre_volume: None });
        assert!(fini);
    }

    #[test]
    fn fin_d_album_sans_morceau_n_existe_pas() {
        assert!(Minuteur::fin_d_album(Vec::new(), s(0)).is_none());
    }

    #[test]
    fn l_etat_dit_ce_qui_reste() {
        let t0 = Instant::now();
        let l = lecteur(Some("/m/a"), s(150)); // durée 200 s
        let m = Minuteur::dans(t0, s(90));
        assert_eq!(m.etat(t0 + s(30), &l), EtatMinuteur { mode: "duree", reste_ms: Some(60_000) });
        let m = Minuteur::fin_de_piste(p("/m/a"), s(150));
        assert_eq!(m.etat(t0, &l), EtatMinuteur { mode: "piste", reste_ms: Some(50_000) });
        let m = Minuteur::fin_d_album(vec![p("/m/a"), p("/m/b")], s(150)).expect("minuteur");
        assert_eq!(m.etat(t0, &l), EtatMinuteur { mode: "album", reste_ms: None });
        let l2 = lecteur(Some("/m/b"), s(150));
        assert_eq!(m.etat(t0, &l2), EtatMinuteur { mode: "album", reste_ms: Some(50_000) });
    }

    #[test]
    fn le_battement_se_resserre_a_la_fin_d_un_morceau_seulement() {
        let t0 = Instant::now();
        let piste = Minuteur::fin_de_piste(p("/m/a"), s(0));
        assert_eq!(piste.battement(&lecteur(Some("/m/a"), s(100))), Duration::from_millis(100));
        assert_eq!(piste.battement(&lecteur(Some("/m/a"), s(199))), Duration::from_millis(20));
        let duree = Minuteur::dans(t0, s(60));
        assert_eq!(duree.battement(&lecteur(Some("/m/a"), s(199))), Duration::from_millis(100));
        // Sans durée connue, on veille serré tout du long (voir `battement`).
        let mut sans_duree = lecteur(Some("/m/a"), s(10));
        sans_duree.duree = None;
        assert_eq!(piste.battement(&sans_duree), Duration::from_millis(20));
        assert_eq!(duree.battement(&sans_duree), Duration::from_millis(100));
    }

    // ------------------------------------------------ avec un vrai lecteur

    /// WAV mono 8 kHz de `n` échantillons de silence.
    fn ecrire_wav(chemin: &std::path::Path, n: u32) {
        let donnees = n * 2;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + donnees).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&8000u32.to_le_bytes());
        v.extend_from_slice(&16000u32.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&donnees.to_le_bytes());
        v.resize(v.len() + donnees as usize, 0);
        std::fs::write(chemin, v).expect("écriture du wav");
    }

    fn photo(player: &rusty_music_player::Player, connaitre_duree: bool) -> Lecteur {
        Lecteur {
            courant: player.current().map(Path::to_path_buf),
            position: player.position(),
            duree: player.duree_courante().filter(|_| connaitre_duree),
            volume: player.volume(),
            repetition_une: false,
        }
    }

    /// Fait tourner `m` contre `player` comme le fil de `main.rs`, jusqu'à ce
    /// qu'il se termine ou que `limite` s'écoule. Rend ce que le lecteur
    /// faisait à l'instant de la pause : (morceau, position, volumes vus).
    fn derouler(
        m: &mut Minuteur,
        player: &mut rusty_music_player::Player,
        connaitre_duree: bool,
        limite: Duration,
    ) -> (Option<PathBuf>, Duration, Vec<f32>) {
        let debut = Instant::now();
        let mut volumes = Vec::new();
        while debut.elapsed() < limite {
            let _ = player.completer(); // précharge le morceau suivant
            let l = photo(player, connaitre_duree);
            let (action, fini) = m.avancer(Instant::now(), &l);
            match action {
                Action::Volume(v) => {
                    player.set_volume(v);
                    volumes.push(v);
                }
                Action::Pause { rendre_volume } => {
                    player.pause();
                    let vu = (player.current().map(Path::to_path_buf), player.position());
                    if let Some(v) = rendre_volume {
                        player.set_volume(v);
                    }
                    return (vu.0, vu.1, volumes);
                }
                Action::Annuler { .. } | Action::Rien => {}
            }
            if fini {
                break;
            }
            std::thread::sleep(m.battement(&l));
        }
        (player.current().map(Path::to_path_buf), player.position(), volumes)
    }

    /// Deux morceaux de 1,5 s et 3 s, lancés, avec un minuteur « fin du morceau »
    /// armé sur le premier. Rend (morceau, position) à la pause.
    fn jouer_deux_morceaux(connaitre_duree: bool) -> (Option<PathBuf>, Duration, PathBuf, PathBuf) {
        let dossier = std::env::temp_dir().join(format!(
            "rusty-music-minuteur-{}-{connaitre_duree}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dossier).expect("dossier");
        let (a, b) = (dossier.join("a.wav"), dossier.join("b.wav"));
        ecrire_wav(&a, 8000 + 4000); // 1,5 s
        ecrire_wav(&b, 8000 * 3);
        let mut player = rusty_music_player::Player::new().expect("sortie audio indisponible");
        player.play(&[a.clone(), b.clone()]).expect("lecture");
        let mut m = Minuteur::fin_de_piste(a.clone(), Duration::ZERO);
        let (courant, position, _) = derouler(&mut m, &mut player, connaitre_duree, s(8));
        assert!(player.is_paused(), "le lecteur doit être en pause");
        let _ = std::fs::remove_dir_all(&dossier);
        (courant, position, a, b)
    }

    /// « À la fin du morceau », durée décodée connue : la pause tombe **avant** la
    /// fin du premier morceau, et rien du second n'est entendu. Ouvre la sortie
    /// audio : ignoré par défaut.
    #[test]
    #[ignore]
    fn fin_de_piste_pause_juste_avant_la_fin_du_morceau() {
        let (courant, position, a, _) = jouer_deux_morceaux(true);
        eprintln!("pause à {position:?} du morceau d'origine (1,5 s)");
        assert_eq!(courant, Some(a), "la pause tombe sans quitter le morceau");
        assert!(
            position >= Duration::from_millis(1_400) && position <= Duration::from_millis(1_500),
            "pause à {position:?}, hors des derniers 100 ms"
        );
    }

    /// Sans durée connue, le filet : pause au changement de morceau, et le moins
    /// possible du suivant. Ouvre la sortie audio : ignoré par défaut.
    #[test]
    #[ignore]
    fn sans_duree_la_pause_tombe_au_changement_de_morceau() {
        let (courant, position, _, b) = jouer_deux_morceaux(false);
        eprintln!("morceau suivant audible avant la pause : {position:?}");
        assert_eq!(courant, Some(b), "la pause tombe sur le morceau suivant");
        assert!(position < Duration::from_millis(120), "{position:?} du suivant audibles");
    }

    /// Le fondu d'une durée fixe : le volume ne remonte jamais, finit près de
    /// zéro, la pause tombe et le volume est remis. Ouvre la sortie audio.
    #[test]
    #[ignore]
    fn la_duree_fait_un_fondu_puis_une_pause_et_remet_le_volume() {
        let dossier = std::env::temp_dir().join(format!("rusty-music-fondu-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier");
        let a = dossier.join("a.wav");
        ecrire_wav(&a, 8000 * 20);
        let mut player = rusty_music_player::Player::new().expect("sortie audio indisponible");
        player.play(std::slice::from_ref(&a)).expect("lecture");
        player.set_volume(0.8);

        let mut m = Minuteur::dans(Instant::now(), s(3));
        let (_, _, volumes) = derouler(&mut m, &mut player, true, s(8));
        assert!(player.is_paused());
        assert!((player.volume() - 0.8).abs() < 1e-6, "volume remis : {}", player.volume());
        assert!(volumes.len() > 10, "le fondu doit avoir de nombreuses étapes");
        assert!(volumes.windows(2).all(|w| w[1] <= w[0] + 1e-6), "le volume remonte");
        assert!(*volumes.last().expect("volumes") < 0.05, "le fondu finit près de zéro");
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
