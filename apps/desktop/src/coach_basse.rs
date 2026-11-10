// SPDX-License-Identifier: GPL-3.0-or-later
//! Le coach de Pratiquer pour la basse en DI (E5, tranche 3) : l'entrée
//! audio écoutée par `cpal`, chaque bloc poussé dans l'analyseur de
//! `rusty_music_editor::coach::basse` (attaque par flux spectral, hauteur
//! par pYIN causal), et les notes jugées contre la transcription de la basse,
//! dont chaque note est d'abord vérifiée sur le stem démixé : rien n'est dit
//! d'une note que le stem ne confirme pas.
//!
//! **L'horloge.** CoreAudio date chaque bloc capturé (`InputStreamTimestamp`
//! : l'instant du rappel et celui de la capture, le second plus tôt de la
//! latence du tampon). Le premier échantillon du premier bloc est situé sur
//! l'horloge commune (`rusty_music_player::horloge`, celle de l'ancre des
//! stems) : `maintenant_s()` au rappel, moins l'écart rappel − capture.
//! Ensuite, l'analyseur compte les échantillons ; quand ce compte s'écarte de
//! plus de [`DERIVE_MAX_S`] de la date d'un bloc (horloges de l'interface et
//! de l'ordinateur qui dérivent, bloc perdu), l'analyseur repart de cette
//! date. Le reste — convertisseur, pilote, geste — est ce que la calibration
//! mesure (chemin « audio », gardé par entrée).
//!
//! `cpal::Stream` ne se partage pas entre fils sous macOS : un fil le
//! possède et le lâche quand l'écoute se ferme (le canal d'arrêt tombe).

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rusty_music_editor::coach::appariement::Fenetres;
use rusty_music_editor::coach::basse::{
    confiance_reference, juger, references, AnalyseurDI, BilanHauteur, Evenement, JugementBasse,
    NoteDI, NoteRef,
};
use rusty_music_editor::coach::{calibrer, Horloge};
use rusty_music_player::horloge::{maintenant_s, Ancre};

use super::*;

/// Le chemin de la latence d'une entrée audio (`latence.chemin`).
const CHEMIN: &str = "audio";

/// Notes gardées en mémoire : une demi-heure de croches à 120.
const NOTES_MAX: usize = 8192;

/// Écart toléré entre le compte des échantillons et la date d'un bloc avant
/// de reprendre l'origine de l'analyseur.
pub(crate) const DERIVE_MAX_S: f64 = 0.010;

/// Recul du jugement sur le présent : la hauteur d'une note arrive 60 à
/// 75 ms après son attaque, et un rappel de capture tarde d'un tampon.
const RECUL_S: f64 = 0.4;

/// Ce que le fil de capture partage avec les commandes.
#[derive(Default)]
pub(crate) struct Capture {
    analyseur: Option<AnalyseurDI>,
    /// Échantillons pousses depuis l'origine de l'analyseur.
    pousses: u64,
    origine_s: f64,
    notes: VecDeque<NoteDI>,
    /// Origines reprises (dérive ou bloc perdu) depuis l'ouverture.
    reprises: u32,
    erreur: Option<String>,
}

impl Capture {
    /// Un bloc mono capturé à `capture_s` (horloge commune), à `sr` Hz.
    pub(crate) fn recevoir(&mut self, capture_s: f64, sr: f32, bloc: &[f32]) {
        let attendu = self.origine_s + self.pousses as f64 / sr as f64;
        let repartir = match self.analyseur {
            None => true,
            Some(_) => (attendu - capture_s).abs() > DERIVE_MAX_S,
        };
        if repartir {
            if self.analyseur.is_some() {
                self.reprises += 1;
            }
            self.analyseur = Some(AnalyseurDI::new(sr, capture_s));
            self.origine_s = capture_s;
            self.pousses = 0;
        }
        let a = self.analyseur.as_mut().expect("posé juste au-dessus");
        for e in a.pousser(bloc) {
            if let Evenement::Note(n) = e {
                if self.notes.len() == NOTES_MAX {
                    self.notes.pop_front();
                }
                self.notes.push_back(n);
            }
        }
        self.pousses += bloc.len() as u64;
    }

    fn entre(&self, t0_s: f64, t1_s: f64) -> Vec<NoteDI> {
        self.notes
            .iter()
            .filter(|n| n.t_s >= t0_s && n.t_s < t1_s)
            .copied()
            .collect()
    }
}

struct EcouteDI {
    entree: String,
    capture: Arc<Mutex<Capture>>,
    /// Le lâcher arrête le fil de capture (et ferme le flux). `None` pour
    /// une source simulée (tests).
    _arret: Option<mpsc::Sender<()>>,
}

/// L'état du coach de basse : l'entrée écoutée, la référence armée.
#[derive(Default)]
pub(crate) struct EtatCoachBasse {
    ecoute: Option<EcouteDI>,
    latence_s: f64,
    refs: Vec<NoteRef>,
}

/// Ce que l'interface sait de l'écoute.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueEcouteBasse {
    entree: String,
    frequence_hz: u32,
    canaux: u32,
    /// La latence calibrée de cette entrée, si elle l'a été.
    latence_ms: Option<f64>,
}

/// Les entrées audio que le système voit, par nom (l'entrée par défaut
/// d'abord).
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_entrees() -> Result<Vec<String>, String> {
    let hote = cpal::default_host();
    let defaut = hote
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|d| d.name().to_string());
    let mut noms: Vec<String> = hote
        .input_devices()
        .map_err(echec)?
        .filter_map(|d| d.description().ok().map(|d| d.name().to_string()))
        .collect();
    noms.dedup();
    if let Some(d) = defaut {
        if let Some(i) = noms.iter().position(|n| *n == d) {
            let n = noms.remove(i);
            noms.insert(0, n);
        }
    }
    Ok(noms)
}

/// Ouvre l'entrée `nom` dans un fil qui garde le flux ; rend sa fréquence et
/// son nombre de canaux. Seul le premier canal est écouté : l'entrée 1 de
/// l'interface, où se branche la basse.
fn ouvrir(nom: &str, capture: Arc<Mutex<Capture>>) -> Result<(mpsc::Sender<()>, u32, u32), String> {
    let (pret_tx, pret_rx) = mpsc::channel::<Result<(u32, u32), String>>();
    let (arret_tx, arret_rx) = mpsc::channel::<()>();
    let nom = nom.to_string();
    std::thread::Builder::new()
        .name("coach-basse".into())
        .spawn(move || {
            let flux = (|| -> Result<(cpal::Stream, u32, u32), String> {
                let hote = cpal::default_host();
                let appareil = hote
                    .input_devices()
                    .map_err(echec)?
                    .find(|d| d.description().is_ok_and(|x| x.name() == nom))
                    .ok_or_else(|| format!("entrée audio introuvable : {nom}"))?;
                let config = appareil.default_input_config().map_err(echec)?;
                let canaux = config.channels().max(1) as usize;
                let sr = config.sample_rate();
                let format = config.sample_format();
                let config: cpal::StreamConfig = config.into();
                let pour_erreur = capture.clone();
                let mut mono = Vec::new();
                let rappel = move |donnees: &cpal::Data, info: &cpal::InputCallbackInfo| {
                    // L'instant d'abord : le verrou peut attendre une commande.
                    let maintenant = maintenant_s();
                    let ts = info.timestamp();
                    let retard = ts
                        .callback
                        .duration_since(&ts.capture)
                        .map_or(0.0, |d| d.as_secs_f64());
                    mono.clear();
                    premier_canal(donnees, canaux, &mut mono);
                    if let Ok(mut c) = capture.lock() {
                        c.recevoir(maintenant - retard, sr as f32, &mono);
                    }
                };
                let flux = appareil
                    .build_input_stream_raw(
                        &config,
                        format,
                        rappel,
                        move |e| {
                            if let Ok(mut c) = pour_erreur.lock() {
                                c.erreur = Some(e.to_string());
                            }
                        },
                        None,
                    )
                    .map_err(echec)?;
                flux.play().map_err(echec)?;
                Ok((flux, sr, canaux as u32))
            })();
            match flux {
                Ok((flux, sr, canaux)) => {
                    let _ = pret_tx.send(Ok((sr, canaux)));
                    // Jusqu'à ce que l'écoute soit lâchée.
                    let _ = arret_rx.recv();
                    drop(flux);
                }
                Err(e) => {
                    let _ = pret_tx.send(Err(e));
                }
            }
        })
        .map_err(echec)?;
    let (sr, canaux) = pret_rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "l'entrée audio ne répond pas".to_string())??;
    Ok((arret_tx, sr, canaux))
}

/// Le premier canal d'un bloc entrelacé, en f32.
fn premier_canal(donnees: &cpal::Data, canaux: usize, sortie: &mut Vec<f32>) {
    use cpal::SampleFormat as F;
    match donnees.sample_format() {
        F::F32 => {
            if let Some(x) = donnees.as_slice::<f32>() {
                sortie.extend(x.iter().step_by(canaux).copied());
            }
        }
        F::I16 => {
            if let Some(x) = donnees.as_slice::<i16>() {
                sortie.extend(x.iter().step_by(canaux).map(|&v| v as f32 / 32_768.0));
            }
        }
        F::I32 => {
            if let Some(x) = donnees.as_slice::<i32>() {
                sortie.extend(
                    x.iter()
                        .step_by(canaux)
                        .map(|&v| (v as f64 / 2_147_483_648.0) as f32),
                );
            }
        }
        F::F64 => {
            if let Some(x) = donnees.as_slice::<f64>() {
                sortie.extend(x.iter().step_by(canaux).map(|&v| v as f32));
            }
        }
        _ => {}
    }
}

/// Écoute l'entrée audio `entree` (son nom, tel que [`coach_basse_entrees`]
/// le donne) ; `None` ferme l'écoute. macOS demande l'accès au micro à la
/// première ouverture.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_ecouter(
    etat: State<Etat>,
    entree: Option<String>,
) -> Result<Option<VueEcouteBasse>, String> {
    let mut coach = verrou(&etat.coach_basse);
    coach.ecoute = None; // ferme l'ancienne avant d'ouvrir la même entrée
    let Some(nom) = entree else {
        return Ok(None);
    };
    let latence = verrou(&etat.lib).latence(&nom, CHEMIN).map_err(echec)?;
    let capture = Arc::new(Mutex::new(Capture::default()));
    let (arret, sr, canaux) = ouvrir(&nom, capture.clone())?;
    coach.latence_s = latence.as_ref().map_or(0.0, |l| l.latence_ms / 1000.0);
    coach.ecoute = Some(EcouteDI {
        entree: nom.clone(),
        capture,
        _arret: Some(arret),
    });
    Ok(Some(VueEcouteBasse {
        entree: nom,
        frequence_hz: sr,
        canaux,
        latence_ms: latence.map(|l| l.latence_ms),
    }))
}

/// Une note de la transcription, telle que l'interface l'arme.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct NoteArmee {
    debut_s: f64,
    hauteur: u8,
}

/// Ce que l'armement a trouvé : combien de notes le stem confirme.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueArmementBasse {
    notes: u32,
    sures: u32,
    /// Faux quand le stem de basse manque : aucune note n'est alors sûre.
    stem: bool,
}

/// Arme le coach sur la transcription de la basse du morceau `id` (`notes`
/// au rang de la transcription, celui des jugements) : chaque note est
/// vérifiée sur le stem de basse démixé. Une liste vide désarme.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_armer(
    etat: State<Etat>,
    id: i64,
    notes: Vec<NoteArmee>,
) -> Result<VueArmementBasse, String> {
    let notes: Vec<(f64, u8)> = notes
        .iter()
        .filter(|n| n.debut_s.is_finite())
        .map(|n| (n.debut_s, n.hauteur))
        .collect();
    let stem = if notes.is_empty() {
        None
    } else {
        let chemin = verrou(&etat.lib)
            .track(id)
            .map_err(echec)?
            .map(|t| t.path)
            .ok_or_else(|| format!("morceau inconnu : {id}"))?;
        let dossier = dossier_stems(&etat, &PathBuf::from(&chemin));
        stems_du_dossier(&dossier)
            .into_iter()
            .find(|(n, _)| n == "bass")
            .map(|(_, p)| p)
    };
    let confiances = match &stem {
        Some(p) => {
            let s = rusty_music_editor::decode::stereo(p).map_err(echec)?;
            let mono: Vec<f32> = s
                .gauche
                .iter()
                .zip(&s.droite)
                .map(|(g, d)| 0.5 * (g + d))
                .collect();
            confiance_reference(&mono, rusty_music_editor::decode::SR as f32, &notes)
        }
        None => Vec::new(),
    };
    let refs = references(&notes, &confiances);
    let sures = refs.iter().filter(|r| r.sure).count() as u32;
    verrou(&etat.coach_basse).refs = refs;
    Ok(VueArmementBasse {
        notes: notes.len() as u32,
        sures,
        stem: stem.is_some(),
    })
}

/// Le jugement d'une fenêtre de temps.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueJugementsBasse {
    /// Fin de la fenêtre jugée, à l'horloge : le `depuis_s` de l'appel
    /// suivant.
    jusqua_s: f64,
    jugements: Vec<JugementBasse>,
    /// Les notes jouées dans la fenêtre (le rang `frappe` des jugements).
    notes: Vec<NoteDI>,
    bilan: BilanHauteur,
    /// Une erreur du flux audio (entrée débranchée…), s'il y en a eu une.
    erreur: Option<String>,
}

impl VueJugementsBasse {
    fn vide(jusqua_s: f64) -> Self {
        VueJugementsBasse {
            jusqua_s,
            jugements: Vec::new(),
            notes: Vec::new(),
            bilan: BilanHauteur::default(),
            erreur: None,
        }
    }
}

fn juger_sur(
    coach: &EtatCoachBasse,
    ancre: &Ancre,
    boucle: Option<(f64, f64)>,
    depuis_s: f64,
    jusqua_s: f64,
) -> VueJugementsBasse {
    let mut vue = VueJugementsBasse::vide(jusqua_s.max(depuis_s));
    let Some(ecoute) = coach.ecoute.as_ref() else {
        return vue;
    };
    let capture = verrou(&ecoute.capture);
    vue.erreur = capture.erreur.clone();
    if coach.refs.is_empty() || jusqua_s <= depuis_s {
        return vue;
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
    let notes = capture.entre(depuis_s - marge, jusqua_s + marge);
    drop(capture);
    let jugements = juger(&horloge, &coach.refs, &notes, depuis_s, jusqua_s, &fenetres);
    vue.bilan = BilanHauteur::de(&jugements);
    vue.jugements = jugements;
    vue.notes = notes;
    vue
}

/// Juge les notes de basse entre `depuis_s` (le `jusqua_s` du dernier appel)
/// et un peu avant maintenant ; `None` ne juge rien et rend seulement le
/// point de départ (après un décompte).
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_juger(
    etat: State<Etat>,
    depuis_s: Option<f64>,
) -> Result<VueJugementsBasse, String> {
    let jusqua = maintenant_s() - RECUL_S;
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
        return Ok(VueJugementsBasse::vide(jusqua.max(depuis)));
    };
    Ok(juger_sur(
        &verrou(&etat.coach_basse),
        &ancre,
        boucle,
        depuis,
        jusqua,
    ))
}

/// Calibre la latence de l'entrée écoutée sur les clics d'un décompte
/// (`clics`, en secondes du morceau) : on pince une corde à vide, étouffée,
/// sur chaque clic. La garde pour cette entrée.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_calibrer(
    etat: State<Etat>,
    clics: Vec<f64>,
) -> Result<coach::VueCalibration, String> {
    let ancre = verrou(&etat.stems)
        .as_ref()
        .and_then(|m| m.ancre())
        .ok_or("la lecture est arrêtée : relancer le décompte")?;
    let mut coach = verrou(&etat.coach_basse);
    let (vue, entree) = calibrer_sur(&coach, &ancre, &clics)?;
    let garde = rusty_music_core::db::LatenceCalibree {
        peripherique: entree,
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

fn calibrer_sur(
    coach: &EtatCoachBasse,
    ancre: &Ancre,
    clics: &[f64],
) -> Result<(coach::VueCalibration, String), String> {
    let ecoute = coach
        .ecoute
        .as_ref()
        .ok_or("aucune entrée audio n'est écoutée")?;
    let mut t = coach::clics_a_l_horloge(ancre, clics);
    t.sort_by(f64::total_cmp);
    let (Some(&premier), Some(&dernier)) = (t.first(), t.last()) else {
        return Err("aucun clic".into());
    };
    let attaques: Vec<f64> = verrou(&ecoute.capture)
        .entre(premier - 0.5, dernier + 1.0)
        .iter()
        .map(|n| n.t_s)
        .collect();
    let c = calibrer(&t, &attaques)?;
    Ok((coach::VueCalibration::de(&c), ecoute.entree.clone()))
}

/// Les notes jugées attrapées.
#[cfg(test)]
fn attrapees(js: &[JugementBasse]) -> usize {
    use rusty_music_editor::coach::appariement::Verdict;
    js.iter()
        .filter(|j| {
            matches!(
                j.jugement.verdict,
                Verdict::Parfait | Verdict::Bon | Verdict::Avance | Verdict::Retard
            )
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_editor::coach::appariement::Verdict;
    use rusty_music_editor::coach::basse::{synthetiser_note, Hauteur};

    const SR: f32 = 48_000.0;

    /// Une prise de basse, telle qu'un fichier enregistré la donnerait :
    /// (début, durée, MIDI).
    fn prise(notes: &[(f32, f32, f32)], duree_s: f32) -> Vec<f32> {
        let mut x = vec![0f32; (duree_s * SR) as usize];
        for &(d, l, m) in notes {
            synthetiser_note(&mut x, SR, d, l, m, 1.0);
        }
        x
    }

    /// Pousse la prise par blocs de 256 comme le ferait CoreAudio, datés à
    /// partir de `t0_s`, avec une gigue de ±1 ms sur la date de chaque bloc.
    fn capturer(x: &[f32], t0_s: f64) -> Capture {
        let mut c = Capture::default();
        for (k, bloc) in x.chunks(256).enumerate() {
            let gigue = if k % 2 == 0 { 0.001 } else { -0.001 };
            c.recevoir(t0_s + (k * 256) as f64 / SR as f64 + gigue, SR, bloc);
        }
        c.recevoir(t0_s + x.len() as f64 / SR as f64, SR, &vec![0.0; 8192]);
        c
    }

    fn coach_simule(capture: Capture, refs: Vec<NoteRef>) -> EtatCoachBasse {
        EtatCoachBasse {
            ecoute: Some(EcouteDI {
                entree: "Interface (entrée 1)".into(),
                capture: Arc::new(Mutex::new(capture)),
                _arret: None,
            }),
            latence_s: 0.0,
            refs,
        }
    }

    fn ligne() -> Vec<(f32, f32, f32)> {
        vec![
            (0.50, 0.40, 28.0),
            (1.00, 0.40, 33.0),
            (1.50, 0.40, 38.0),
            (2.00, 0.40, 43.0),
            (2.50, 0.40, 40.0),
            (3.00, 0.40, 31.0),
        ]
    }

    #[test]
    fn la_capture_date_les_notes_sur_l_horloge_commune() {
        let c = capturer(&prise(&ligne(), 4.0), 500.0);
        let notes: Vec<NoteDI> = c.notes.iter().copied().collect();
        assert_eq!(notes.len(), 6, "{notes:#?}");
        // La gigue de ±1 ms reste sous la dérive tolérée : une seule origine.
        assert_eq!(c.reprises, 0);
        for (n, &(d, _, m)) in notes.iter().zip(&ligne()) {
            let ecart_ms = (n.t_s - 500.0 - d as f64) * 1000.0;
            assert!(ecart_ms.abs() < 10.0, "{d} : {ecart_ms:.1} ms");
            assert!((n.hauteur().unwrap() - m).abs() < 0.5);
        }
    }

    #[test]
    fn un_bloc_perdu_reprend_l_origine() {
        let x = prise(&ligne(), 4.0);
        let mut c = Capture::default();
        // 50 ms de blocs perdus à 1,7 s (entre la 3ᵉ et la 4ᵉ note).
        let trou = (1.7 * SR) as usize / 256;
        for (k, bloc) in x.chunks(256).enumerate() {
            if (trou..trou + 9).contains(&k) {
                continue;
            }
            c.recevoir(100.0 + (k * 256) as f64 / SR as f64, SR, bloc);
        }
        c.recevoir(104.0, SR, &vec![0.0; 8192]);
        assert_eq!(c.reprises, 1);
        // Les notes d'après le trou restent à leur place.
        let n = c.notes.iter().find(|n| n.t_s > 101.9).unwrap();
        assert!((n.t_s - 102.0).abs() < 0.01, "{n:?}");
    }

    #[test]
    fn une_prise_jugee_contre_la_reference_verifiee_sur_le_stem() {
        // La référence : la ligne, vérifiée sur son « stem » (la même ligne).
        // La transcription a vu la 5ᵉ note un ton trop bas : le stem ne la
        // confirme pas, elle n'est pas notée.
        let stem = prise(&ligne(), 4.0);
        let mut notes: Vec<(f64, u8)> = ligne().iter().map(|n| (n.0 as f64, n.2 as u8)).collect();
        notes[4].1 -= 2;
        let refs = references(&notes, &confiance_reference(&stem, SR, &notes));
        assert_eq!(refs.iter().filter(|r| r.sure).count(), 5);
        // Le jeu : 2ᵉ note un demi-ton trop haut, 6ᵉ une octave au-dessus,
        // le tout 15 ms en retard ; la musique part à 1000 s de l'horloge.
        let mut jeu = ligne();
        jeu[1].2 += 1.0;
        jeu[5].2 += 12.0;
        for n in &mut jeu {
            n.0 += 0.015;
        }
        let coach = coach_simule(capturer(&prise(&jeu, 4.0), 1000.0), refs);
        let ancre = Ancre {
            t_s: 1000.0,
            morceau_s: 0.0,
            vitesse: 1.0,
        };
        // Deux fenêtres successives, comme le sondage de l'interface.
        let a = juger_sur(&coach, &ancre, None, 1000.0, 1001.8);
        let b = juger_sur(&coach, &ancre, None, a.jusqua_s, 1004.0);
        let js: Vec<JugementBasse> = a.jugements.into_iter().chain(b.jugements).collect();
        let bilan = BilanHauteur::de(&js);
        assert_eq!(
            (bilan.justes, bilan.fausses, bilan.octaves),
            (3, 1, 1),
            "{js:#?}"
        );
        assert_eq!(attrapees(&js), 5);
        let incertaine = js.iter().find(|j| j.jugement.reference == Some(4)).unwrap();
        assert_eq!(incertaine.jugement.verdict, Verdict::NonNotee);
        assert_eq!(incertaine.hauteur, None);
        let fausse = js.iter().find(|j| j.jugement.reference == Some(1)).unwrap();
        assert_eq!(fausse.hauteur, Some(Hauteur::Fausse { demi_tons: 1 }));
    }

    #[test]
    fn calibration_sur_des_cordes_etouffees() {
        // Seize clics à 120, la corde pincée 30 ms après chacun.
        let clics: Vec<f64> = (0..16).map(|k| 0.5 + 0.5 * k as f64).collect();
        let jeu: Vec<(f32, f32, f32)> = clics
            .iter()
            .map(|&c| (c as f32 + 0.030, 0.15, 33.0))
            .collect();
        let coach = coach_simule(capturer(&prise(&jeu, 9.0), 200.0), Vec::new());
        let ancre = Ancre {
            t_s: 200.0,
            morceau_s: 0.0,
            vitesse: 1.0,
        };
        let (v, entree) = calibrer_sur(&coach, &ancre, &clics).unwrap();
        assert!((v.latence_ms - 30.0).abs() < 8.0, "{v:?}");
        assert!(v.retenues >= 14, "{v:?}");
        assert_eq!(entree, "Interface (entrée 1)");
    }

    #[test]
    fn sans_ecoute_ni_reference_rien_n_est_juge() {
        let ancre = Ancre {
            t_s: 0.0,
            morceau_s: 0.0,
            vitesse: 1.0,
        };
        let v = juger_sur(&EtatCoachBasse::default(), &ancre, None, 0.0, 5.0);
        assert!(v.jugements.is_empty());
        let coach = coach_simule(capturer(&prise(&ligne(), 4.0), 0.0), Vec::new());
        assert!(juger_sur(&coach, &ancre, None, 0.0, 5.0)
            .jugements
            .is_empty());
    }
}
