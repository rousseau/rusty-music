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
//! **Le canal** (V34). Une interface a plusieurs entrées : sur une Scarlett
//! Solo, l'entrée 1 est le micro et l'instrument (Hi-Z) la 2 ; sur une 2i2,
//! la basse peut être sur l'une ou l'autre. Tous les canaux sont écoutés
//! (jusqu'à [`CANAUX_MAX`]), chacun par son analyseur ; le coach juge celui
//! qui est retenu. En « auto », la calibration retient le canal où arrivent
//! les attaques des clics (le plus fort si plusieurs en portent : une 2i2 en
//! mono sur les deux), et le jugement passe de lui-même au canal qui porte
//! la basse quand le retenu reste muet ; choisi à la main, le canal ne
//! bouge plus et la ligne du coach dit seulement où la basse semble être.
//! Le canal se garde par interface (`canal_entree`), comme la latence.
//!
//! `cpal::Stream` ne se partage pas entre fils sous macOS : un fil le
//! possède et le lâche quand l'écoute se ferme (le canal d'arrêt tombe).

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rusty_music_editor::coach::appariement::Fenetres;
use rusty_music_editor::coach::basse::{
    confiance_reference, juger, references, transposer, AnalyseurDI, BilanHauteur, Evenement,
    JugementBasse, NoteDI, NoteRef,
};
use rusty_music_editor::coach::{calibrer, Calibration, Horloge};
use rusty_music_player::horloge::{maintenant_s, Ancre};

use super::*;

/// Le chemin de la latence d'une entrée audio (`latence.chemin`).
const CHEMIN: &str = "audio";

/// Notes gardées en mémoire : une demi-heure de croches à 120.
const NOTES_MAX: usize = 8192;

/// Écart toléré entre le compte des échantillons et la date d'un bloc avant
/// de reprendre l'origine de l'analyseur.
pub(crate) const DERIVE_MAX_S: f64 = 0.010;

/// Canaux d'une interface écoutés au plus : une Scarlett 18i20 en a 18, et
/// chaque canal écouté a son analyseur.
pub(crate) const CANAUX_MAX: usize = 8;

/// Fenêtre où l'on cherche la basse sur un autre canal que le retenu.
const FENETRE_AILLEURS_S: f64 = 3.0;

/// Notes avec une hauteur qu'un autre canal doit porter dans la fenêtre pour
/// que la basse y semble (le souffle d'un micro n'en donne pas).
const NOTES_AILLEURS: usize = 3;

/// Recul du jugement sur le présent : la hauteur d'une note arrive 60 à
/// 75 ms après son attaque, et un rappel de capture tarde d'un tampon.
const RECUL_S: f64 = 0.4;

/// Un canal de l'interface : son analyseur et ses notes.
#[derive(Default)]
pub(crate) struct Voie {
    analyseur: Option<AnalyseurDI>,
    /// Échantillons pousses depuis l'origine de l'analyseur.
    pousses: u64,
    origine_s: f64,
    notes: VecDeque<NoteDI>,
    /// Origines reprises (dérive ou bloc perdu) depuis l'ouverture.
    reprises: u32,
}

impl Voie {
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

/// Ce que le fil de capture partage avec les commandes : une voie par canal
/// écouté.
#[derive(Default)]
pub(crate) struct Capture {
    voies: Vec<Voie>,
    erreur: Option<String>,
}

impl Capture {
    /// Un bloc capturé à `capture_s` (horloge commune), à `sr` Hz : une
    /// tranche par canal.
    pub(crate) fn recevoir(&mut self, capture_s: f64, sr: f32, canaux: &[Vec<f32>]) {
        if self.voies.len() < canaux.len() {
            self.voies.resize_with(canaux.len(), Voie::default);
        }
        for (v, bloc) in self.voies.iter_mut().zip(canaux) {
            v.recevoir(capture_s, sr, bloc);
        }
    }

    /// Les notes du canal `canal` entre `t0_s` et `t1_s`.
    fn entre(&self, canal: usize, t0_s: f64, t1_s: f64) -> Vec<NoteDI> {
        self.voies
            .get(canal)
            .map_or_else(Vec::new, |v| v.entre(t0_s, t1_s))
    }

    /// Les notes avec une hauteur du canal `canal` entre `t0_s` et `t1_s`.
    fn hautes(&self, canal: usize, t0_s: f64, t1_s: f64) -> usize {
        self.voies.get(canal).map_or(0, |v| {
            v.notes
                .iter()
                .filter(|n| n.t_s >= t0_s && n.t_s < t1_s && n.f0_hz.is_some())
                .count()
        })
    }

    /// Le canal où la basse semble jouer entre `t0_s` et `t1_s`, quand ce
    /// n'est pas `canal` : un autre en porte au moins [`NOTES_AILLEURS`]
    /// notes avec une hauteur, trois fois plus que lui.
    fn ailleurs(&self, canal: usize, t0_s: f64, t1_s: f64) -> Option<usize> {
        let ici = self.hautes(canal, t0_s, t1_s);
        (0..self.voies.len())
            .filter(|&c| c != canal)
            .map(|c| (c, self.hautes(c, t0_s, t1_s)))
            .filter(|&(_, n)| n >= NOTES_AILLEURS && n >= 3 * ici.max(1))
            .max_by_key(|&(c, n)| (n, std::cmp::Reverse(c)))
            .map(|(c, _)| c)
    }
}

struct EcouteDI {
    entree: String,
    /// Canaux écoutés (ceux de l'interface, [`CANAUX_MAX`] au plus).
    canaux: u32,
    /// Le canal jugé, compté depuis 0.
    canal: usize,
    /// Choisi à la main : ni la calibration ni le jugement ne le changent.
    manuel: bool,
    frequence_hz: u32,
    /// La latence calibrée de cette entrée, si elle l'a été.
    latence_ms: Option<f64>,
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
    /// La référence telle que transcrite, à la hauteur d'origine.
    refs: Vec<NoteRef>,
    /// Le réglage de hauteur des stems (V35), en demi-tons : la référence
    /// jugée est `refs` décalée d'autant, ce que l'on entend et joue.
    demi_tons: i32,
}

/// Ce que l'interface sait de l'écoute.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueEcouteBasse {
    entree: String,
    frequence_hz: u32,
    /// Canaux écoutés (l'interface en a peut-être plus : [`CANAUX_MAX`]).
    canaux: u32,
    /// Le canal jugé, compté depuis 0 (l'entrée 1 de l'interface).
    canal: u32,
    /// Choisi à la main (sinon « auto »).
    manuel: bool,
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
/// le nombre de canaux écoutés : tous ceux de l'interface, jusqu'à
/// [`CANAUX_MAX`], chacun poussé dans sa voie.
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
                let ecoutes = canaux.min(CANAUX_MAX);
                let sr = config.sample_rate();
                let format = config.sample_format();
                let config: cpal::StreamConfig = config.into();
                let pour_erreur = capture.clone();
                let mut tranches = vec![Vec::new(); ecoutes];
                let rappel = move |donnees: &cpal::Data, info: &cpal::InputCallbackInfo| {
                    // L'instant d'abord : le verrou peut attendre une commande.
                    let maintenant = maintenant_s();
                    let ts = info.timestamp();
                    let retard = ts
                        .callback
                        .duration_since(&ts.capture)
                        .map_or(0.0, |d| d.as_secs_f64());
                    for t in tranches.iter_mut() {
                        t.clear();
                    }
                    deentrelacer(donnees, canaux, &mut tranches);
                    if let Ok(mut c) = capture.lock() {
                        c.recevoir(maintenant - retard, sr as f32, &tranches);
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
                Ok((flux, sr, ecoutes as u32))
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

/// Les canaux d'un bloc entrelacé à `canaux` canaux, en f32 : autant de
/// tranches que `sorties` en a (les premiers canaux).
fn deentrelacer(donnees: &cpal::Data, canaux: usize, sorties: &mut [Vec<f32>]) {
    use cpal::SampleFormat as F;
    fn pousser<T: Copy>(x: &[T], canaux: usize, sorties: &mut [Vec<f32>], f: impl Fn(T) -> f32) {
        for trame in x.chunks_exact(canaux) {
            for (s, &v) in sorties.iter_mut().zip(trame) {
                s.push(f(v));
            }
        }
    }
    match donnees.sample_format() {
        F::F32 => {
            if let Some(x) = donnees.as_slice::<f32>() {
                pousser(x, canaux, sorties, |v| v);
            }
        }
        F::I16 => {
            if let Some(x) = donnees.as_slice::<i16>() {
                pousser(x, canaux, sorties, |v| v as f32 / 32_768.0);
            }
        }
        F::I32 => {
            if let Some(x) = donnees.as_slice::<i32>() {
                pousser(x, canaux, sorties, |v| (v as f64 / 2_147_483_648.0) as f32);
            }
        }
        F::F64 => {
            if let Some(x) = donnees.as_slice::<f64>() {
                pousser(x, canaux, sorties, |v| v as f32);
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
    let (latence, garde) = {
        let lib = verrou(&etat.lib);
        (
            lib.latence(&nom, CHEMIN).map_err(echec)?,
            lib.canal_entree(&nom).map_err(echec)?,
        )
    };
    let capture = Arc::new(Mutex::new(Capture::default()));
    let (arret, sr, canaux) = ouvrir(&nom, capture.clone())?;
    coach.latence_s = latence.as_ref().map_or(0.0, |l| l.latence_ms / 1000.0);
    // Le canal gardé, s'il existe encore (autre réglage de l'interface).
    let (canal, manuel) = garde
        .filter(|g| g.canal < canaux)
        .map_or((0, false), |g| (g.canal as usize, g.manuel));
    let ecoute = EcouteDI {
        entree: nom,
        canaux,
        canal,
        manuel,
        frequence_hz: sr,
        latence_ms: latence.map(|l| l.latence_ms),
        capture,
        _arret: Some(arret),
    };
    let vue = ecoute.vue();
    coach.ecoute = Some(ecoute);
    Ok(Some(vue))
}

impl EcouteDI {
    fn vue(&self) -> VueEcouteBasse {
        VueEcouteBasse {
            entree: self.entree.clone(),
            frequence_hz: self.frequence_hz,
            canaux: self.canaux,
            canal: self.canal as u32,
            manuel: self.manuel,
            latence_ms: self.latence_ms,
        }
    }
}

/// Choisit le canal jugé de l'entrée écoutée : `Some(c)` le fixe (compté
/// depuis 0), `None` revient à « auto » (le canal trouvé jusqu'ici reste
/// jusqu'à la prochaine calibration). Gardé pour cette interface.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_canal(
    etat: State<Etat>,
    canal: Option<u32>,
) -> Result<VueEcouteBasse, String> {
    let mut coach = verrou(&etat.coach_basse);
    let ecoute = coach
        .ecoute
        .as_mut()
        .ok_or("aucune entrée audio n'est écoutée")?;
    let garde = choisir_canal(ecoute, canal)?;
    verrou(&etat.lib)
        .garder_canal_entree(&garde)
        .map_err(echec)?;
    Ok(ecoute.vue())
}

fn choisir_canal(
    ecoute: &mut EcouteDI,
    canal: Option<u32>,
) -> Result<rusty_music_core::db::CanalEntree, String> {
    match canal {
        Some(c) if c >= ecoute.canaux => {
            return Err(format!(
                "l'interface n'a que {} entrée{}",
                ecoute.canaux,
                if ecoute.canaux > 1 { "s" } else { "" }
            ))
        }
        Some(c) => {
            ecoute.canal = c as usize;
            ecoute.manuel = true;
        }
        None => ecoute.manuel = false,
    }
    Ok(rusty_music_core::db::CanalEntree {
        peripherique: ecoute.entree.clone(),
        canal: ecoute.canal as u32,
        manuel: ecoute.manuel,
    })
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
/// vérifiée sur le stem de basse démixé, à la hauteur d'origine (le stem
/// d'origine confirme la note d'origine) ; `demi_tons` est le réglage de
/// hauteur en cours, que le jugement ajoute (V35). Une liste vide désarme.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_armer(
    etat: State<Etat>,
    id: i64,
    notes: Vec<NoteArmee>,
    demi_tons: i32,
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
    {
        let mut coach = verrou(&etat.coach_basse);
        coach.refs = refs;
        coach.demi_tons = demi_tons;
    }
    Ok(VueArmementBasse {
        notes: notes.len() as u32,
        sures,
        stem: stem.is_some(),
    })
}

/// Le réglage de hauteur des stems a changé (V35) : le coach juge contre
/// la référence décalée de `demi_tons`, sans réarmer (la confiance du stem
/// ne dépend pas du décalage).
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_transposer(etat: State<Etat>, demi_tons: i32) -> Result<(), String> {
    if !(-24..=24).contains(&demi_tons) {
        return Err(format!("transposition hors bornes : {demi_tons}"));
    }
    verrou(&etat.coach_basse).demi_tons = demi_tons;
    Ok(())
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
    /// Le canal jugé, compté depuis 0 : en « auto », il suit la basse.
    canal: u32,
    /// Canal choisi à la main resté muet : celui où la basse semble jouer.
    ailleurs: Option<u32>,
}

impl VueJugementsBasse {
    fn vide(jusqua_s: f64) -> Self {
        VueJugementsBasse {
            jusqua_s,
            jugements: Vec::new(),
            notes: Vec::new(),
            bilan: BilanHauteur::default(),
            erreur: None,
            canal: 0,
            ailleurs: None,
        }
    }
}

/// Où joue la basse ? En « auto », le canal retenu passe à celui qui la
/// porte quand il reste muet (rendu : le canal à garder) ; choisi à la main,
/// il ne bouge pas, et la vue dit seulement où elle semble être.
fn suivre_canal(ecoute: &mut EcouteDI, vue: &mut VueJugementsBasse) -> Option<usize> {
    let t1 = vue.jusqua_s;
    let ailleurs = verrou(&ecoute.capture).ailleurs(ecoute.canal, t1 - FENETRE_AILLEURS_S, t1);
    let mut change = None;
    match ailleurs {
        Some(c) if !ecoute.manuel => {
            ecoute.canal = c;
            change = Some(c);
        }
        a => vue.ailleurs = a.map(|c| c as u32),
    }
    vue.canal = ecoute.canal as u32;
    change
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
    vue.canal = ecoute.canal as u32;
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
    let notes = capture.entre(ecoute.canal, depuis_s - marge, jusqua_s + marge);
    drop(capture);
    let refs = transposer(&coach.refs, coach.demi_tons);
    let jugements = juger(&horloge, &refs, &notes, depuis_s, jusqua_s, &fenetres);
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
    let mut coach = verrou(&etat.coach_basse);
    let mut vue = VueJugementsBasse::vide(jusqua.max(depuis));
    let garde = coach.ecoute.as_mut().and_then(|e| {
        suivre_canal(e, &mut vue).map(|c| rusty_music_core::db::CanalEntree {
            peripherique: e.entree.clone(),
            canal: c as u32,
            manuel: false,
        })
    });
    if let Some(g) = garde {
        verrou(&etat.lib).garder_canal_entree(&g).map_err(echec)?;
    }
    let mut jugee = juger_sur(&coach, &ancre, boucle, depuis, jusqua);
    jugee.ailleurs = vue.ailleurs;
    Ok(jugee)
}

/// Calibre la latence de l'entrée écoutée sur les clics d'un décompte
/// (`clics`, en secondes du morceau) : on pince une corde à vide, étouffée,
/// sur chaque clic. La garde pour cette entrée.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_basse_calibrer(
    etat: State<Etat>,
    clics: Vec<f64>,
) -> Result<VueCalibrationBasse, String> {
    let ancre = verrou(&etat.stems)
        .as_ref()
        .and_then(|m| m.ancre())
        .ok_or("la lecture est arrêtée : relancer le décompte")?;
    let mut coach = verrou(&etat.coach_basse);
    let vue = calibrer_sur(&mut coach, &ancre, &clics)?;
    let ecoute = coach.ecoute.as_ref().expect("calibrée sur une écoute");
    let garde = rusty_music_core::db::LatenceCalibree {
        peripherique: ecoute.entree.clone(),
        chemin: CHEMIN.into(),
        latence_ms: vue.latence_ms,
        dispersion_ms: Some(vue.dispersion_ms),
        frappes: vue.retenues,
    };
    garde.verifier()?;
    let canal = rusty_music_core::db::CanalEntree {
        peripherique: ecoute.entree.clone(),
        canal: ecoute.canal as u32,
        manuel: ecoute.manuel,
    };
    {
        let lib = verrou(&etat.lib);
        lib.garder_latence(&garde).map_err(echec)?;
        lib.garder_canal_entree(&canal).map_err(echec)?;
    }
    Ok(vue)
}

/// Ce qu'une calibration de la basse a mesuré, et le canal retenu.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueCalibrationBasse {
    latence_ms: f64,
    dispersion_ms: f64,
    retenues: u32,
    ecartees: u32,
    /// Le canal où sont arrivées les attaques, compté depuis 0.
    canal: u32,
}

/// Le canal où arrivent les attaques des clics : parmi ceux dont la
/// calibration tient et retient presque autant de notes que la meilleure
/// (une 2i2 en mono sur les deux entrées), le plus fort (niveau médian des
/// attaques, en dBFS).
fn canal_des_clics(par_canal: &[(usize, Calibration, f32)]) -> Option<usize> {
    let meilleure = par_canal.iter().map(|c| c.1.retenues).max()?;
    par_canal
        .iter()
        .filter(|c| 4 * c.1.retenues >= 3 * meilleure)
        .max_by(|a, b| a.2.total_cmp(&b.2).then(b.0.cmp(&a.0)))
        .map(|c| c.0)
}

fn niveau_median(notes: &[NoteDI]) -> f32 {
    let mut n: Vec<f32> = notes.iter().map(|n| n.niveau_db).collect();
    if n.is_empty() {
        return f32::NEG_INFINITY;
    }
    n.sort_by(f32::total_cmp);
    n[n.len() / 2]
}

/// Calibre sur le canal choisi à la main ; en « auto », sur chaque canal,
/// et retient celui où arrivent les attaques ([`canal_des_clics`]).
fn calibrer_sur(
    coach: &mut EtatCoachBasse,
    ancre: &Ancre,
    clics: &[f64],
) -> Result<VueCalibrationBasse, String> {
    let ecoute = coach
        .ecoute
        .as_mut()
        .ok_or("aucune entrée audio n'est écoutée")?;
    let mut t = coach::clics_a_l_horloge(ancre, clics);
    t.sort_by(f64::total_cmp);
    let (Some(&premier), Some(&dernier)) = (t.first(), t.last()) else {
        return Err("aucun clic".into());
    };
    let candidats: Vec<usize> = if ecoute.manuel {
        vec![ecoute.canal]
    } else {
        (0..ecoute.canaux as usize).collect()
    };
    let mut par_canal = Vec::new();
    let mut erreur = None;
    {
        let capture = verrou(&ecoute.capture);
        for &k in &candidats {
            let notes = capture.entre(k, premier - 0.5, dernier + 1.0);
            let attaques: Vec<f64> = notes.iter().map(|n| n.t_s).collect();
            match calibrer(&t, &attaques) {
                Ok(c) => par_canal.push((k, c, niveau_median(&notes))),
                // L'erreur du canal jugé d'abord : c'est elle qui parle.
                Err(e) if k == ecoute.canal || erreur.is_none() => erreur = Some(e),
                Err(_) => {}
            }
        }
    }
    let Some(k) = canal_des_clics(&par_canal) else {
        return Err(erreur.unwrap_or_else(|| "aucune note sur les clics".into()));
    };
    let c = par_canal
        .iter()
        .find(|c| c.0 == k)
        .expect("choisi parmi eux")
        .1;
    ecoute.canal = k;
    ecoute.latence_ms = Some(c.latence_ms);
    coach.latence_s = c.latence_ms / 1000.0;
    Ok(VueCalibrationBasse {
        latence_ms: c.latence_ms,
        dispersion_ms: c.dispersion_ms,
        retenues: c.retenues as u32,
        ecartees: c.ecartees as u32,
        canal: k as u32,
    })
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
        prise_a(notes, duree_s, 1.0)
    }

    fn prise_a(notes: &[(f32, f32, f32)], duree_s: f32, gain: f32) -> Vec<f32> {
        let mut x = vec![0f32; (duree_s * SR) as usize];
        for &(d, l, m) in notes {
            synthetiser_note(&mut x, SR, d, l, m, gain);
        }
        x
    }

    /// Le souffle d'une entrée micro sans micro : un bruit blanc (±0,05)
    /// reproductible.
    fn bruit(duree_s: f32) -> Vec<f32> {
        let mut g: u32 = 0x9E37_79B9;
        (0..(duree_s * SR) as usize)
            .map(|_| {
                g = g.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((g >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.1
            })
            .collect()
    }

    /// Pousse la prise par blocs de 256 comme le ferait CoreAudio, datés à
    /// partir de `t0_s`, avec une gigue de ±1 ms sur la date de chaque bloc.
    fn capturer(x: &[f32], t0_s: f64) -> Capture {
        capturer_canaux(&[x.to_vec()], t0_s)
    }

    /// La même chose sur une interface à plusieurs canaux (un par prise,
    /// toutes de la même durée).
    fn capturer_canaux(canaux: &[Vec<f32>], t0_s: f64) -> Capture {
        let mut c = Capture::default();
        let n = canaux[0].len();
        for (k, debut) in (0..n).step_by(256).enumerate() {
            let gigue = if k % 2 == 0 { 0.001 } else { -0.001 };
            let blocs: Vec<Vec<f32>> = canaux
                .iter()
                .map(|x| x[debut..(debut + 256).min(n)].to_vec())
                .collect();
            c.recevoir(t0_s + debut as f64 / SR as f64 + gigue, SR, &blocs);
        }
        let silence = vec![vec![0.0; 8192]; canaux.len()];
        c.recevoir(t0_s + n as f64 / SR as f64, SR, &silence);
        c
    }

    fn coach_simule(capture: Capture, refs: Vec<NoteRef>) -> EtatCoachBasse {
        let canaux = capture.voies.len() as u32;
        EtatCoachBasse {
            ecoute: Some(EcouteDI {
                entree: "Interface (entrée 1)".into(),
                canaux,
                canal: 0,
                manuel: false,
                frequence_hz: SR as u32,
                latence_ms: None,
                capture: Arc::new(Mutex::new(capture)),
                _arret: None,
            }),
            latence_s: 0.0,
            refs,
            demi_tons: 0,
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
        let notes: Vec<NoteDI> = c.voies[0].notes.iter().copied().collect();
        assert_eq!(notes.len(), 6, "{notes:#?}");
        // La gigue de ±1 ms reste sous la dérive tolérée : une seule origine.
        assert_eq!(c.voies[0].reprises, 0);
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
            c.recevoir(100.0 + (k * 256) as f64 / SR as f64, SR, &[bloc.to_vec()]);
        }
        c.recevoir(104.0, SR, &[vec![0.0; 8192]]);
        assert_eq!(c.voies[0].reprises, 1);
        // Les notes d'après le trou restent à leur place.
        let n = c.voies[0].notes.iter().find(|n| n.t_s > 101.9).unwrap();
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

    /// La référence de la ligne, confirmée par son « stem » (la ligne même).
    fn refs_de_la_ligne() -> Vec<NoteRef> {
        let stem = prise(&ligne(), 4.0);
        let notes: Vec<(f64, u8)> = ligne().iter().map(|n| (n.0 as f64, n.2 as u8)).collect();
        references(&notes, &confiance_reference(&stem, SR, &notes))
    }

    const ANCRE_1000: Ancre = Ancre {
        t_s: 1000.0,
        morceau_s: 0.0,
        vitesse: 1.0,
    };

    #[test]
    fn la_reference_suit_la_transposition() {
        // V35 : le morceau transposé d'un demi-ton vers le haut, la ligne
        // jouée un demi-ton plus haut, comme on l'entend.
        let refs = refs_de_la_ligne();
        assert_eq!(refs.iter().filter(|r| r.sure).count(), 6);
        let jeu: Vec<(f32, f32, f32)> = ligne().iter().map(|&(t, d, m)| (t, d, m + 1.0)).collect();
        let mut coach = coach_simule(capturer(&prise(&jeu, 4.0), 1000.0), refs);
        // Avec la transposition : justes.
        coach.demi_tons = 1;
        let v = juger_sur(&coach, &ANCRE_1000, None, 1000.0, 1004.0);
        let b = BilanHauteur::de(&v.jugements);
        assert_eq!((b.justes, b.fausses, b.octaves), (6, 0, 0), "{v:#?}");
        // Sans : toutes fausses, d'un demi-ton trop haut.
        coach.demi_tons = 0;
        let v = juger_sur(&coach, &ANCRE_1000, None, 1000.0, 1004.0);
        let b = BilanHauteur::de(&v.jugements);
        assert_eq!((b.justes, b.fausses), (0, 6), "{v:#?}");
        assert!(v
            .jugements
            .iter()
            .all(|j| j.hauteur == Some(Hauteur::Fausse { demi_tons: 1 })));
        // Transposé vers le bas, joué à l'origine : fausses d'un demi-ton
        // trop haut aussi.
        let mut coach = coach_simule(capturer(&prise(&ligne(), 4.0), 1000.0), refs_de_la_ligne());
        coach.demi_tons = -1;
        let v = juger_sur(&coach, &ANCRE_1000, None, 1000.0, 1004.0);
        assert_eq!(BilanHauteur::de(&v.jugements).fausses, 6, "{v:#?}");
    }

    /// La ligne jouée à `vitesse`, la 1ʳᵉ note `avance_s` trop tôt et la
    /// 2ᵉ autant trop tard (horloge), les autres 10 ms en retard.
    fn juger_a_la_vitesse(vitesse: f64, ecart_s: f32) -> Vec<JugementBasse> {
        let r = vitesse as f32;
        let jeu: Vec<(f32, f32, f32)> = ligne()
            .iter()
            .enumerate()
            .map(|(k, &(t, d, m))| {
                let decal = match k {
                    0 => -ecart_s,
                    1 => ecart_s,
                    _ => 0.010,
                };
                (t / r + decal, d, m)
            })
            .collect();
        let coach = coach_simule(
            capturer(&prise(&jeu, 4.0 / r + 0.5), 1000.0),
            refs_de_la_ligne(),
        );
        let ancre = Ancre {
            vitesse,
            ..ANCRE_1000
        };
        juger_sur(&coach, &ancre, None, 1000.0, 1000.0 + 4.0 / vitesse).jugements
    }

    #[test]
    fn le_temps_se_juge_a_la_vitesse_en_cours() {
        // À 70 % comme à 100 % : la 1ʳᵉ note en avance, la 2ᵉ en retard, les
        // autres justes ; l'écart musical est l'écart d'horloge × la vitesse.
        for vitesse in [0.7, 1.0] {
            let js = juger_a_la_vitesse(vitesse, 0.100);
            let par_ref = |k: usize| js.iter().find(|j| j.jugement.reference == Some(k)).unwrap();
            assert_eq!(
                par_ref(0).jugement.verdict,
                Verdict::Avance,
                "{vitesse} : {js:#?}"
            );
            assert_eq!(
                par_ref(1).jugement.verdict,
                Verdict::Retard,
                "{vitesse} : {js:#?}"
            );
            let musical = par_ref(1).jugement.ecart_musical_ms.unwrap();
            assert!(
                (musical - 100.0 * vitesse).abs() < 12.0,
                "{vitesse} : {musical}"
            );
            for k in 2..6 {
                assert!(
                    matches!(par_ref(k).jugement.verdict, Verdict::Parfait | Verdict::Bon),
                    "{vitesse}, note {k} : {js:#?}"
                );
            }
            assert_eq!(BilanHauteur::de(&js).justes, 6, "{vitesse}");
        }
        // Un retard de 48 ms d'horloge : en retard à 100 %, juste à 70 %
        // (34 ms de musique, sous la fenêtre « bon »).
        let r = |v| {
            juger_a_la_vitesse(v, 0.048)
                .into_iter()
                .find(|j| j.jugement.reference == Some(1))
                .unwrap()
                .jugement
                .verdict
        };
        assert_eq!(r(1.0), Verdict::Retard);
        assert!(
            matches!(r(0.7), Verdict::Bon | Verdict::Parfait),
            "{:?}",
            r(0.7)
        );
    }

    /// Seize clics à 120.
    fn clics() -> Vec<f64> {
        (0..16).map(|k| 0.5 + 0.5 * k as f64).collect()
    }

    /// La corde pincée 30 ms après chaque clic.
    fn cordes_sur_les_clics() -> Vec<(f32, f32, f32)> {
        clics()
            .iter()
            .map(|&c| (c as f32 + 0.030, 0.15, 33.0))
            .collect()
    }

    const ANCRE_CALIBRATION: Ancre = Ancre {
        t_s: 200.0,
        morceau_s: 0.0,
        vitesse: 1.0,
    };

    #[test]
    fn calibration_sur_des_cordes_etouffees() {
        let mut coach = coach_simule(
            capturer(&prise(&cordes_sur_les_clics(), 9.0), 200.0),
            Vec::new(),
        );
        let v = calibrer_sur(&mut coach, &ANCRE_CALIBRATION, &clics()).unwrap();
        assert!((v.latence_ms - 30.0).abs() < 8.0, "{v:?}");
        assert!(v.retenues >= 14, "{v:?}");
        assert_eq!(v.canal, 0);
        assert!((coach.latence_s - v.latence_ms / 1000.0).abs() < 1e-9);
    }

    /// La calibration d'une interface à deux canaux : la basse sur l'un, le
    /// souffle de l'entrée micro sur l'autre. Rend le canal retenu.
    fn calibrer_deux_canaux(canaux: [Vec<f32>; 2]) -> (EtatCoachBasse, VueCalibrationBasse) {
        let mut coach = coach_simule(capturer_canaux(&canaux, 200.0), Vec::new());
        let v = calibrer_sur(&mut coach, &ANCRE_CALIBRATION, &clics()).unwrap();
        (coach, v)
    }

    #[test]
    fn la_calibration_trouve_la_basse_sur_l_entree_2() {
        // Une Scarlett Solo : le micro (du souffle) sur l'entrée 1,
        // l'instrument sur la 2.
        let (coach, v) = calibrer_deux_canaux([bruit(9.0), prise(&cordes_sur_les_clics(), 9.0)]);
        assert_eq!(v.canal, 1, "{v:?}");
        assert!((v.latence_ms - 30.0).abs() < 8.0, "{v:?}");
        assert_eq!(coach.ecoute.as_ref().unwrap().canal, 1);
    }

    #[test]
    fn la_calibration_garde_l_entree_1_quand_la_basse_y_est() {
        let (coach, v) = calibrer_deux_canaux([prise(&cordes_sur_les_clics(), 9.0), bruit(9.0)]);
        assert_eq!(v.canal, 0, "{v:?}");
        assert_eq!(coach.ecoute.as_ref().unwrap().canal, 0);
    }

    #[test]
    fn deux_canaux_portent_la_basse_le_plus_fort_l_emporte() {
        // Une 2i2 en mono sur les deux entrées, la 2 plus haut.
        let jeu = cordes_sur_les_clics();
        let (_, v) = calibrer_deux_canaux([prise_a(&jeu, 9.0, 0.25), prise(&jeu, 9.0)]);
        assert_eq!(v.canal, 1, "{v:?}");
        let (_, v) = calibrer_deux_canaux([prise(&jeu, 9.0), prise_a(&jeu, 9.0, 0.25)]);
        assert_eq!(v.canal, 0, "{v:?}");
    }

    #[test]
    fn un_canal_choisi_a_la_main_ne_bouge_plus() {
        let mut coach = coach_simule(
            capturer_canaux(&[bruit(9.0), prise(&cordes_sur_les_clics(), 9.0)], 200.0),
            Vec::new(),
        );
        let e = coach.ecoute.as_mut().unwrap();
        assert!(choisir_canal(e, Some(2)).is_err());
        let garde = choisir_canal(e, Some(0)).unwrap();
        assert_eq!((garde.canal, garde.manuel), (0, true));
        // La calibration ne cherche que sur l'entrée choisie : du souffle.
        assert!(calibrer_sur(&mut coach, &ANCRE_CALIBRATION, &clics()).is_err());
        let e = coach.ecoute.as_mut().unwrap();
        assert_eq!(e.canal, 0);
        // Le jugement ne change pas de canal, mais dit où semble la basse.
        let mut vue = VueJugementsBasse::vide(209.0);
        assert_eq!(suivre_canal(e, &mut vue), None);
        assert_eq!((vue.canal, vue.ailleurs), (0, Some(1)));
        // Revenir à « auto » : le jugement suit la basse, sur l'entrée 2.
        let garde = choisir_canal(e, None).unwrap();
        assert_eq!((garde.canal, garde.manuel), (0, false));
        let mut vue = VueJugementsBasse::vide(209.0);
        assert_eq!(suivre_canal(e, &mut vue), Some(1));
        assert_eq!((vue.canal, vue.ailleurs), (1, None));
        // Puis il y reste.
        let mut vue = VueJugementsBasse::vide(209.0);
        assert_eq!(suivre_canal(e, &mut vue), None);
        assert_eq!(vue.canal, 1);
    }

    #[test]
    fn la_ligne_jouee_sur_l_entree_2_est_jugee_en_auto() {
        // Sans calibration, l'écoute part sur l'entrée 1 (du souffle) ; la
        // basse joue sur la 2 : le jugement y passe et juge ses notes.
        let stem = prise(&ligne(), 4.0);
        let notes: Vec<(f64, u8)> = ligne().iter().map(|n| (n.0 as f64, n.2 as u8)).collect();
        let refs = references(&notes, &confiance_reference(&stem, SR, &notes));
        let mut coach = coach_simule(
            capturer_canaux(&[bruit(4.0), prise(&ligne(), 4.0)], 1000.0),
            refs,
        );
        let ancre = Ancre {
            t_s: 1000.0,
            morceau_s: 0.0,
            vitesse: 1.0,
        };
        // Le souffle seul ne donne aucune note avec une hauteur.
        let capture = coach.ecoute.as_ref().unwrap().capture.clone();
        assert!(capture.lock().unwrap().hautes(0, 1000.0, 1004.0) < 2);
        let mut vue = VueJugementsBasse::vide(1004.0);
        assert_eq!(
            suivre_canal(coach.ecoute.as_mut().unwrap(), &mut vue),
            Some(1)
        );
        let v = juger_sur(&coach, &ancre, None, 1000.0, 1004.0);
        assert_eq!(v.canal, 1);
        assert_eq!(BilanHauteur::de(&v.jugements).justes, 6, "{v:#?}");
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
