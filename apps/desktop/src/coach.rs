// SPDX-License-Identifier: GPL-3.0-or-later
//! Le coach de Pratiquer, côté application (E4, tranche 2) : la batterie
//! électronique écoutée par `midir`, la calibration par tapotement sur le
//! décompte, et le jugement de chaque coup contre la transcription de la
//! batterie.
//!
//! Tout le calcul est dans `rusty_music_editor::coach` ; ici, l'entrée MIDI,
//! l'état entre deux commandes et la base. Les coups sont horodatés à leur
//! réception sur l'horloge commune (`rusty_music_player::horloge`), celle où
//! le moteur publie son ancre : la latence de CoreMIDI, du module et du
//! geste est ce que la calibration mesure.

use std::collections::VecDeque;
use std::sync::Arc;

use rusty_music_editor::coach::appariement::{
    apparier, place_perdue, reference_sure, Fenetres, Jugement, Reference,
};
use rusty_music_editor::coach::batterie::{Classe, Decodeur, Frappe, Profil};
use rusty_music_editor::coach::{calibrer, Horloge};
use rusty_music_player::horloge::{maintenant_s, Ancre};
use rusty_music_transcription::batterie::{Coup, Piece};

use super::*;

/// Coups gardés en mémoire : une demi-heure de double croche à 120.
const FRAPPES_MAX: usize = 8192;

/// Le chemin de la latence d'une batterie électronique.
const CHEMIN: &str = "midi";

/// Ce que le fil de `midir` partage avec les commandes.
struct Reception {
    decodeur: Decodeur,
    frappes: VecDeque<Frappe>,
}

impl Reception {
    fn recevoir(&mut self, t_s: f64, message: &[u8]) {
        if let Some(f) = self.decodeur.lire(t_s, message) {
            if self.frappes.len() == FRAPPES_MAX {
                self.frappes.pop_front();
            }
            self.frappes.push_back(f);
        }
    }

    fn entre(&self, t0_s: f64, t1_s: f64) -> Vec<Frappe> {
        self.frappes
            .iter()
            .filter(|f| f.t_s >= t0_s && f.t_s < t1_s)
            .copied()
            .collect()
    }
}

struct Ecoute {
    peripherique: String,
    reception: Arc<Mutex<Reception>>,
    /// La connexion ; la lâcher ferme le port. `None` pour une source
    /// simulée (tests).
    _connexion: Option<midir::MidiInputConnection<()>>,
}

/// L'état du coach : l'écoute ouverte, la référence armée.
#[derive(Default)]
pub(crate) struct EtatCoach {
    ecoute: Option<Ecoute>,
    /// La latence calibrée du périphérique écouté, en secondes.
    latence_s: f64,
    refs: Vec<Reference>,
    /// Premiers temps de mesure, en secondes du morceau.
    mesures: Vec<f64>,
}

/// Ce que l'interface sait de l'écoute.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueEcoute {
    peripherique: String,
    profil: Profil,
    /// La latence calibrée de ce périphérique, si elle l'a été.
    latence_ms: Option<f64>,
}

fn classe(p: Piece) -> Classe {
    match p {
        Piece::GrosseCaisse => Classe::GrosseCaisse,
        Piece::CaisseClaire => Classe::CaisseClaire,
        Piece::Toms => Classe::Toms,
        Piece::Charleston => Classe::Charleston,
        Piece::Cymbales => Classe::Cymbales,
    }
}

fn profil_garde(lib: &rusty_music_core::db::Library, peripherique: &str) -> Profil {
    lib.profil_midi(peripherique)
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str::<Profil>(&j).ok())
        .filter(|p| p.verifier().is_ok())
        .unwrap_or_default()
}

/// Les entrées MIDI que le système voit, par nom.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_peripheriques() -> Result<Vec<String>, String> {
    let entree = midir::MidiInput::new("Rusty Music").map_err(echec)?;
    Ok(entree
        .ports()
        .iter()
        .filter_map(|p| entree.port_name(p).ok())
        .collect())
}

/// Écoute la batterie `peripherique` (son nom, tel que
/// [`coach_peripheriques`] le donne), avec son profil gardé (General MIDI à
/// défaut) ; `None` ferme l'écoute.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_ecouter(
    etat: State<Etat>,
    peripherique: Option<String>,
) -> Result<Option<VueEcoute>, String> {
    let mut coach = verrou(&etat.coach);
    coach.ecoute = None; // ferme l'ancienne avant d'ouvrir le même port
    let Some(nom) = peripherique else {
        return Ok(None);
    };
    let (profil, latence) = {
        let lib = verrou(&etat.lib);
        (
            profil_garde(&lib, &nom),
            lib.latence(&nom, CHEMIN).map_err(echec)?,
        )
    };
    let mut entree = midir::MidiInput::new("Rusty Music").map_err(echec)?;
    // Horloge MIDI et sense actif : des centaines de messages par seconde
    // qu'aucun coup ne porte.
    entree.ignore(midir::Ignore::TimeAndActiveSense);
    let port = entree
        .ports()
        .into_iter()
        .find(|p| entree.port_name(p).ok().as_deref() == Some(nom.as_str()))
        .ok_or_else(|| format!("périphérique MIDI introuvable : {nom}"))?;
    let reception = Arc::new(Mutex::new(Reception {
        decodeur: Decodeur::nouveau(profil.clone()),
        frappes: VecDeque::new(),
    }));
    let pour_le_fil = reception.clone();
    let connexion = entree
        .connect(
            &port,
            "rusty-music-coach",
            move |_, message, _| {
                // L'instant d'abord : le verrou peut attendre une commande.
                let t = maintenant_s();
                if let Ok(mut r) = pour_le_fil.lock() {
                    r.recevoir(t, message);
                }
            },
            (),
        )
        .map_err(|e| e.to_string())?;
    coach.latence_s = latence.as_ref().map_or(0.0, |l| l.latence_ms / 1000.0);
    coach.ecoute = Some(Ecoute {
        peripherique: nom.clone(),
        reception,
        _connexion: Some(connexion),
    });
    Ok(Some(VueEcoute {
        peripherique: nom,
        profil,
        latence_ms: latence.map(|l| l.latence_ms),
    }))
}

/// Garde le profil de correspondance de la batterie écoutée ; il s'applique
/// aux coups suivants.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_garder_profil(etat: State<Etat>, profil: Profil) -> Result<(), String> {
    profil.verifier()?;
    let coach = verrou(&etat.coach);
    let ecoute = coach
        .ecoute
        .as_ref()
        .ok_or("aucune batterie n'est écoutée")?;
    let json = serde_json::to_string(&profil).map_err(echec)?;
    verrou(&etat.lib)
        .garder_profil_midi(&ecoute.peripherique, &json)
        .map_err(echec)?;
    verrou(&ecoute.reception).decodeur = Decodeur::nouveau(profil);
    Ok(())
}

/// Ce qu'une calibration a mesuré.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueCalibration {
    latence_ms: f64,
    dispersion_ms: f64,
    retenues: u32,
    ecartees: u32,
}

/// Les clics d'un décompte, en secondes du morceau, ramenés à l'horloge par
/// l'ancre du moteur (à vitesse r, un clic du morceau tombe 1/r plus loin).
fn clics_a_l_horloge(ancre: &Ancre, clics: &[f64]) -> Vec<f64> {
    let r = ancre.vitesse.max(1e-3);
    clics
        .iter()
        .map(|c| ancre.t_s + (c - ancre.morceau_s) / r)
        .collect()
}

fn calibrer_sur(coach: &EtatCoach, ancre: &Ancre, clics: &[f64]) -> Result<VueCalibration, String> {
    let ecoute = coach
        .ecoute
        .as_ref()
        .ok_or("aucune batterie n'est écoutée")?;
    let mut t = clics_a_l_horloge(ancre, clics);
    t.sort_by(f64::total_cmp);
    let (Some(&premier), Some(&dernier)) = (t.first(), t.last()) else {
        return Err("aucun clic".into());
    };
    let frappes: Vec<f64> = verrou(&ecoute.reception)
        .entre(premier - 0.5, dernier + 1.0)
        .iter()
        .map(|f| f.t_s)
        .collect();
    let c = calibrer(&t, &frappes)?;
    Ok(VueCalibration {
        latence_ms: c.latence_ms,
        dispersion_ms: c.dispersion_ms,
        retenues: c.retenues as u32,
        ecartees: c.ecartees as u32,
    })
}

/// Calibre la latence de la batterie écoutée sur les clics d'un décompte
/// qu'on vient de jouer (`clics`, ceux passés à `stems_decompter`, en
/// secondes du morceau), en tapant dessus ; la garde pour ce périphérique.
/// Les stems doivent jouer encore : c'est leur ancre qui situe les clics.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_calibrer(etat: State<Etat>, clics: Vec<f64>) -> Result<VueCalibration, String> {
    let ancre = verrou(&etat.stems)
        .as_ref()
        .and_then(|m| m.ancre())
        .ok_or("la lecture est arrêtée : relancer le décompte")?;
    let mut coach = verrou(&etat.coach);
    let vue = calibrer_sur(&coach, &ancre, &clics)?;
    let peripherique = coach
        .ecoute
        .as_ref()
        .map(|e| e.peripherique.clone())
        .unwrap_or_default();
    let garde = rusty_music_core::db::LatenceCalibree {
        peripherique,
        chemin: CHEMIN.into(),
        latence_ms: vue.latence_ms,
        dispersion_ms: Some(vue.dispersion_ms),
        frappes: vue.retenues,
    };
    garde.verifier()?;
    verrou(&etat.lib).garder_latence(&garde).map_err(echec)?;
    coach.latence_s = vue.latence_ms / 1000.0;
    Ok(vue)
}

/// Arme le coach sur la transcription de la batterie du morceau ouvert :
/// `coups` tels que `transcrire` les a rendus (leur rang est celui des
/// jugements), `premiers_temps` la grille des mesures, en secondes du
/// morceau. Une liste vide désarme.
#[tauri::command(async)]
pub(crate) fn coach_armer(
    etat: State<Etat>,
    coups: Vec<Coup>,
    premiers_temps: Vec<f64>,
) -> Result<(), String> {
    let mut coach = verrou(&etat.coach);
    armer(&mut coach, &coups, premiers_temps);
    Ok(())
}

fn armer(coach: &mut EtatCoach, coups: &[Coup], mut premiers_temps: Vec<f64>) {
    coach.refs = coups
        .iter()
        .map(|c| Reference {
            tau_s: c.instant_s as f64,
            classe: classe(c.piece),
            sure: reference_sure(classe(c.piece), c.force),
        })
        .collect();
    premiers_temps.retain(|t| t.is_finite());
    premiers_temps.sort_by(f64::total_cmp);
    coach.mesures = premiers_temps;
}

/// Une mesure où le musicien a perdu sa place.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct PlacePerdue {
    /// Rang de la mesure dans les premiers temps armés.
    mesure: u32,
    passage: i32,
    /// −1 : il joue la mesure d'avant ; +1 : celle d'après.
    decalage: i32,
}

/// Le jugement d'une fenêtre de temps.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueJugements {
    /// Fin de la fenêtre jugée, à l'horloge : le `depuis_s` de l'appel
    /// suivant.
    jusqua_s: f64,
    jugements: Vec<Jugement>,
    /// Les coups reçus dans la fenêtre (le rang `frappe` des jugements).
    frappes: Vec<Frappe>,
    places_perdues: Vec<PlacePerdue>,
}

/// Ce qui reste à attendre après la fin d'une fenêtre pour qu'un coup en
/// retard soit arrivé : la fenêtre d'attrape, à la vitesse la plus lente,
/// et un peu de marge.
const RECUL_S: f64 = 0.4;

fn juger_sur(
    coach: &EtatCoach,
    ancre: &Ancre,
    boucle: Option<(f64, f64)>,
    depuis_s: f64,
    jusqua_s: f64,
) -> VueJugements {
    let vide = VueJugements {
        jusqua_s: jusqua_s.max(depuis_s),
        jugements: Vec::new(),
        frappes: Vec::new(),
        places_perdues: Vec::new(),
    };
    let Some(ecoute) = coach.ecoute.as_ref() else {
        return vide;
    };
    if coach.refs.is_empty() || jusqua_s <= depuis_s {
        return vide;
    }
    let horloge = Horloge {
        t_s: ancre.t_s,
        morceau_s: ancre.morceau_s,
        vitesse: ancre.vitesse,
        boucle,
        latence_s: coach.latence_s,
    };
    let fenetres = Fenetres::default();
    let marge = 2.0 * fenetres.attrape_s / ancre.vitesse.max(1e-3);
    // Les coups de la fenêtre et de ses marges, renumérotés pour la vue.
    let frappes = verrou(&ecoute.reception).entre(depuis_s - marge, jusqua_s + marge);
    let jugements = apparier(
        &horloge,
        &coach.refs,
        &frappes,
        depuis_s,
        jusqua_s,
        &fenetres,
    );

    // Les mesures qui finissent dans la fenêtre : la place perdue se juge
    // sur une mesure entière.
    let mut places_perdues = Vec::new();
    for (i, w) in coach.mesures.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let Some(fin0) = horloge.attendu(b - 1e-6, 0) else {
            continue;
        };
        let periode = match boucle {
            Some((la, lb)) if a >= la && b <= lb => (lb - la) / horloge.vitesse.max(1e-3),
            _ => 0.0,
        };
        let passages: Vec<i64> = if periode > 0.0 {
            let k0 = ((depuis_s - fin0) / periode).ceil() as i64;
            let k1 = ((jusqua_s - fin0) / periode).floor() as i64;
            (k0..=k1).collect()
        } else {
            vec![0]
        };
        for k in passages {
            let (Some(t0), Some(t1)) = (horloge.attendu(a, k), horloge.attendu(b - 1e-6, k)) else {
                continue;
            };
            if !(t1 >= depuis_s && t1 < jusqua_s) {
                continue;
            }
            if let Some(decalage) =
                place_perdue(&horloge, &coach.refs, &frappes, t0, t1, b - a, &fenetres)
            {
                places_perdues.push(PlacePerdue {
                    mesure: i as u32,
                    passage: k as i32,
                    decalage,
                });
            }
        }
    }
    VueJugements {
        jusqua_s,
        jugements,
        frappes,
        places_perdues,
    }
}

/// Juge ce qui a été joué depuis `depuis_s` (l'horloge) jusqu'à maintenant,
/// moins le temps qu'un coup en retard arrive. L'interface sonde, et repasse
/// le `jusqua_s` rendu ; `None` ne juge rien et rend seulement le point de
/// départ (après un décompte : rien n'est attendu des clics).
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_juger(
    etat: State<Etat>,
    depuis_s: Option<f64>,
) -> Result<VueJugements, String> {
    let maintenant = maintenant_s();
    let jusqua = maintenant - RECUL_S;
    let depuis = depuis_s.filter(|d| d.is_finite()).unwrap_or(jusqua);
    let (ancre, boucle) = {
        let stems = verrou(&etat.stems);
        let m = stems.as_ref();
        (
            m.and_then(|m| m.ancre()),
            m.and_then(|m| m.boucle())
                .map(|(a, b)| (a.as_secs_f64(), b.as_secs_f64())),
        )
    };
    let Some(ancre) = ancre else {
        // À l'arrêt, rien n'est attendu : on avance sans juger.
        return Ok(VueJugements {
            jusqua_s: jusqua.max(depuis),
            jugements: Vec::new(),
            frappes: Vec::new(),
            places_perdues: Vec::new(),
        });
    };
    let coach = verrou(&etat.coach);
    Ok(juger_sur(&coach, &ancre, boucle, depuis, jusqua))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_editor::coach::appariement::Verdict;

    /// Une source MIDI simulée : des messages bruts injectés à des instants
    /// choisis, comme le fil de `midir` les livrerait.
    fn coach_simule(profil: Profil) -> EtatCoach {
        EtatCoach {
            ecoute: Some(Ecoute {
                peripherique: "Simulée".into(),
                reception: Arc::new(Mutex::new(Reception {
                    decodeur: Decodeur::nouveau(profil),
                    frappes: VecDeque::new(),
                })),
                _connexion: None,
            }),
            ..Default::default()
        }
    }

    fn injecter(coach: &EtatCoach, t_s: f64, message: &[u8]) {
        coach
            .ecoute
            .as_ref()
            .unwrap()
            .reception
            .lock()
            .unwrap()
            .recevoir(t_s, message);
    }

    #[test]
    fn calibration_sur_un_decompte_a_vitesse_reduite() {
        let coach = coach_simule(Profil::general_midi());
        // Décompte de 16 clics à 100 BPM (0,6 s du morceau) avant 10 s, joué
        // à vitesse 0,8 : 0,75 s à l'horloge entre deux clics. L'ancre : le
        // morceau en est à 10 s à l'instant 50 de l'horloge.
        let ancre = Ancre {
            t_s: 50.0,
            morceau_s: 10.0,
            vitesse: 0.8,
        };
        let clics: Vec<f64> = (0..16).map(|i| 10.0 - 0.6 * (16 - i) as f64).collect();
        let a_l_horloge = clics_a_l_horloge(&ancre, &clics);
        assert!((a_l_horloge[15] - (50.0 - 0.75)).abs() < 1e-9);
        // Le batteur tape 42 ms après chaque clic (± 5 ms), caisse claire,
        // sauf sur les deux premiers ; et un coup parasite de grosse caisse.
        for (i, c) in a_l_horloge.iter().enumerate().skip(2) {
            let d = 0.042 + 0.005 * ((i % 3) as f64 - 1.0);
            injecter(&coach, c + d, &[0x99, 38, 100]);
        }
        injecter(&coach, a_l_horloge[7] + 0.33, &[0x99, 36, 100]);
        let v = calibrer_sur(&coach, &ancre, &clics).unwrap();
        assert!((v.latence_ms - 42.0).abs() < 3.0, "{v:?}");
        assert_eq!(v.retenues, 14);
    }

    #[test]
    fn source_simulee_jugee_par_fenetres_successives() {
        let mut coach = coach_simule(Profil::general_midi());
        coach.latence_s = 0.030;
        // La référence : deux mesures de 2 s, grosse caisse et caisse claire
        // en noires, sûres ; une crash incertaine au début ; une note
        // fantôme (force 0,3) de caisse claire.
        let mut coups = Vec::new();
        for i in 0..8 {
            coups.push(Coup {
                instant_s: i as f32 * 0.5,
                piece: if i % 2 == 0 {
                    Piece::GrosseCaisse
                } else {
                    Piece::CaisseClaire
                },
                force: 0.9,
            });
        }
        coups.push(Coup {
            instant_s: 0.0,
            piece: Piece::Cymbales,
            force: 0.9,
        });
        coups.push(Coup {
            instant_s: 1.25,
            piece: Piece::CaisseClaire,
            force: 0.3,
        });
        armer(&mut coach, &coups, vec![0.0, 2.0, 4.0]);
        assert_eq!(coach.refs.iter().filter(|r| !r.sure).count(), 2);

        // Le morceau part de 0 à l'instant 10 de l'horloge, à vitesse 1 ; le
        // batteur, calibré à 30 ms, joue en retard de 30 ms + 12 ms, oublie
        // la caisse claire de 1,5 s et ne joue ni crash ni fantôme.
        let ancre = Ancre {
            t_s: 10.0,
            morceau_s: 0.0,
            vitesse: 1.0,
        };
        for (i, c) in coups.iter().enumerate().take(8) {
            if i == 3 {
                continue;
            }
            let note = if c.piece == Piece::GrosseCaisse {
                36
            } else {
                38
            };
            injecter(&coach, 10.0 + c.instant_s as f64 + 0.042, &[0x99, note, 90]);
        }
        // Des fenêtres de 0,3 s, comme l'interface qui sonde.
        let mut tous = Vec::new();
        let mut t = 9.8;
        while t < 14.5 {
            let v = juger_sur(&coach, &ancre, None, t, t + 0.3);
            for j in &v.jugements {
                tous.push((j.reference, j.verdict, j.ecart_ms));
            }
            t = v.jusqua_s;
        }
        let compte = |v: Verdict| tous.iter().filter(|x| x.1 == v).count();
        assert_eq!(
            compte(Verdict::Parfait) + compte(Verdict::Bon),
            7,
            "{tous:?}"
        );
        assert_eq!(compte(Verdict::Manquee), 1);
        assert_eq!(compte(Verdict::EnTrop), 0);
        assert_eq!(compte(Verdict::NonNotee), 2);
        assert_eq!(tous.len(), 10, "chaque note jugée une fois");
        // L'écart ne compte que ce qui dépasse la latence calibrée.
        let e = tous.iter().find(|x| x.0 == Some(0)).unwrap().2.unwrap();
        assert!((e - 12.0).abs() < 1e-6);
    }

    #[test]
    fn sans_ecoute_ni_reference_rien_n_est_juge() {
        let ancre = Ancre {
            t_s: 0.0,
            morceau_s: 0.0,
            vitesse: 1.0,
        };
        let v = juger_sur(&EtatCoach::default(), &ancre, None, 0.0, 1.0);
        assert!(v.jugements.is_empty());
        assert_eq!(v.jusqua_s, 1.0);
        let coach = coach_simule(Profil::general_midi());
        injecter(&coach, 0.5, &[0x99, 38, 90]);
        assert!(juger_sur(&coach, &ancre, None, 0.0, 1.0)
            .jugements
            .is_empty());
    }
}
