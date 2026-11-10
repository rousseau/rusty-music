// SPDX-License-Identifier: GPL-3.0-or-later
//! Le coach de Pratiquer, côté application (E4, tranches 2 et 3) : la
//! batterie électronique écoutée par `midir`, la calibration par tapotement
//! sur le décompte, le jugement de chaque coup contre la transcription de la
//! batterie, puis, mesure par mesure, la table gardée dans la base
//! (`pratique_mesure`), la boucle proposée sur les pires mesures et
//! l'escalier de tempo.
//!
//! Tout le calcul est dans `rusty_music_editor::coach` ; ici, l'entrée MIDI,
//! l'état entre deux commandes et la base. Les coups sont horodatés à leur
//! réception sur l'horloge commune (`rusty_music_player::horloge`), celle où
//! le moteur publie son ancre : la latence de CoreMIDI, du module et du
//! geste est ce que la calibration mesure.

use std::collections::VecDeque;
use std::sync::Arc;

use rusty_music_editor::coach::appariement::{
    apparier, place_perdue, reference_sure, Fenetres, Jugement, Reference, Verdict,
};
use rusty_music_editor::coach::batterie::{Classe, Decodeur, Frappe, Profil};
use rusty_music_editor::coach::groove::{self, Echantillon, Groove};
use rusty_music_editor::coach::mesures::{
    cumuler, pires_mesures, relever, Escalier, Proposition, Releve,
};
use rusty_music_editor::coach::{calibrer, Horloge};
use rusty_music_player::horloge::{maintenant_s, Ancre};
use rusty_music_transcription::batterie::{Coup, Piece};

use super::*;

/// Coups gardés en mémoire : une demi-heure de double croche à 120.
const FRAPPES_MAX: usize = 8192;

/// Le chemin de la latence d'une batterie électronique.
const CHEMIN: &str = "midi";

/// L'instrument de la table par mesure (celui de `pratique.instrument`).
const INSTRUMENT: &str = "drums";

/// Longueur au plus de la boucle proposée sur les pires mesures.
const BOUCLE_MAX_MESURES: u32 = 4;

/// Deux instants d'horloge de fin de mesure (ou de boucle) sont la même
/// occurrence à moins de 0,1 s : l'ancre du moteur bouge de quelques ms d'un
/// paquet à l'autre.
const MEME_FIN_S: f64 = 0.1;

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

/// Une mesure en train d'être jouée (à un passage) : ses jugements
/// s'accumulent d'une fenêtre à l'autre jusqu'à ce que sa fin soit jugée.
#[derive(Debug, Clone)]
struct MesureOuverte {
    mesure: u32,
    /// Fin attendue de la mesure, à l'horloge.
    fin_s: f64,
    /// Fin attendue du passage de boucle qui la contient, à l'horloge.
    fin_boucle_s: Option<f64>,
    /// Chaque jugement, avec sa position dans la mesure (0 à 1).
    jugements: Vec<(Jugement, f64)>,
}

/// Un passage de boucle en cours : les mesures déjà relevées.
#[derive(Debug, Clone, Default)]
struct PassageOuvert {
    fin_s: f64,
    mesures: std::collections::BTreeSet<u32>,
    attendues: u32,
    attrapees: u32,
    ecarts: Vec<f64>,
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
    /// Le morceau armé (clé de la table par mesure), s'il est connu.
    chemin: Option<String>,
    /// Les mesures qui attendent au moins une note sûre.
    mesures_sures: Vec<bool>,
    ouvertes: Vec<MesureOuverte>,
    passages: Vec<PassageOuvert>,
    escalier: Escalier,
    /// L'activation d'ADTOF de chaque référence (la force sur le stem).
    forces: Vec<f32>,
    /// La grille des battements, en secondes du morceau (swing, décalage du
    /// disque au temps).
    temps: Vec<f64>,
    /// Les notes sûres attrapées depuis l'armement : la matière du groove.
    echantillons: VecDeque<Echantillon>,
}

/// Notes gardées pour le groove : une dizaine de minutes de jeu dense.
const ECHANTILLONS_MAX: usize = 6000;

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
    chemin: Option<String>,
    coups: Vec<Coup>,
    premiers_temps: Vec<f64>,
    temps: Option<Vec<f64>>,
) -> Result<(), String> {
    let mut coach = verrou(&etat.coach);
    armer(&mut coach, chemin, &coups, premiers_temps);
    poser_temps(&mut coach, temps.unwrap_or_default());
    Ok(())
}

/// La grille des battements (Beat This!), triée, sans doublon.
fn poser_temps(coach: &mut EtatCoach, mut temps: Vec<f64>) {
    temps.retain(|t| t.is_finite());
    temps.sort_by(f64::total_cmp);
    temps.dedup();
    coach.temps = temps;
}

/// Le groove du jeu depuis l'armement (E5) : par pièce, le swing, et les
/// conseils par motifs. Rien sous les seuils de `editor::coach::groove`.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_groove(etat: State<Etat>) -> Groove {
    groove_de(&verrou(&etat.coach))
}

fn groove_de(coach: &EtatCoach) -> Groove {
    let e: Vec<Echantillon> = coach.echantillons.iter().copied().collect();
    groove::analyser(&e, &coach.temps)
}

fn armer(
    coach: &mut EtatCoach,
    chemin: Option<String>,
    coups: &[Coup],
    mut premiers_temps: Vec<f64>,
) {
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
    premiers_temps.dedup();
    coach.mesures_sures = premiers_temps
        .windows(2)
        .map(|w| {
            coach
                .refs
                .iter()
                .any(|r| r.sure && r.tau_s >= w[0] && r.tau_s < w[1])
        })
        .collect();
    coach.mesures = premiers_temps;
    coach.chemin = chemin.filter(|c| !c.is_empty());
    coach.ouvertes.clear();
    coach.passages.clear();
    coach.escalier = Escalier::default();
    coach.forces = coups.iter().map(|c| c.force).collect();
    coach.temps.clear();
    coach.echantillons.clear();
}

/// Le rang de la mesure qui contient `tau` (secondes du morceau), et ses
/// bornes ; `None` avant le premier premier temps ou après le dernier.
fn mesure_de(mesures: &[f64], tau: f64) -> Option<(u32, f64, f64)> {
    let i = mesures.partition_point(|&t| t <= tau).checked_sub(1)?;
    let b = *mesures.get(i + 1)?;
    Some((i as u32, mesures[i], b))
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
    /// Les mesures finies dans la fenêtre, chacune relevée (et cumulée dans
    /// la table du morceau, s'il est connu).
    releves: Vec<Releve>,
    /// Un passage de boucle entier vient de finir.
    passage_fini: Option<PassageFini>,
    /// La boucle à proposer sur les pires mesures, d'après la table du
    /// morceau mise à jour ; seulement quand des mesures ont été relevées.
    boucle_proposee: Option<BoucleProposee>,
}

/// Ce qu'un passage de boucle entier a donné, et ce que l'escalier en
/// tire.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct PassageFini {
    attendues: u32,
    attrapees: u32,
    ecart_et_ms: Option<f64>,
    propre: bool,
    /// La vitesse que l'escalier propose (proposée, jamais imposée).
    proposition: Option<Proposition>,
}

/// La boucle proposée sur les pires mesures.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct BoucleProposee {
    /// Rangs de la première et de la dernière mesure.
    premiere: u32,
    derniere: u32,
    /// Bornes, en secondes du morceau.
    debut_s: f64,
    fin_s: f64,
    /// Score lissé de la pire.
    score: f64,
    /// Vitesse de départ de l'escalier.
    vitesse: f64,
}

fn boucle_proposee(
    mesures: &[f64],
    table: &[rusty_music_core::db::MesurePratique],
) -> Option<BoucleProposee> {
    let (a, b) = pires_mesures(table, BOUCLE_MAX_MESURES)?;
    let debut_s = *mesures.get(a as usize)?;
    let fin_s = *mesures.get(b as usize + 1)?;
    let score = table
        .iter()
        .filter(|m| (a..=b).contains(&m.mesure))
        .map(|m| m.score)
        .fold(0.0, f64::max);
    Some(BoucleProposee {
        premiere: a,
        derniere: b,
        debut_s,
        fin_s,
        score,
        vitesse: Escalier::DEPART,
    })
}

/// Ce qui reste à attendre après la fin d'une fenêtre pour qu'un coup en
/// retard soit arrivé : la fenêtre d'attrape, à la vitesse la plus lente,
/// et un peu de marge.
const RECUL_S: f64 = 0.4;

fn juger_sur(
    coach: &mut EtatCoach,
    ancre: &Ancre,
    boucle: Option<(f64, f64)>,
    depuis_s: f64,
    jusqua_s: f64,
) -> VueJugements {
    let vide = VueJugements::vide(jusqua_s.max(depuis_s));
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
    let neufs = groove::echantillons(&coach.refs, &coach.forces, &frappes, &jugements);
    for e in neufs {
        if coach.echantillons.len() == ECHANTILLONS_MAX {
            coach.echantillons.pop_front();
        }
        coach.echantillons.push_back(e);
    }

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
    let (releves, passage_fini) =
        relever_fenetre(coach, &horloge, boucle, &jugements, &frappes, jusqua_s);
    VueJugements {
        jusqua_s,
        jugements,
        frappes,
        places_perdues,
        releves,
        passage_fini,
        boucle_proposee: None,
    }
}

impl VueJugements {
    fn vide(jusqua_s: f64) -> Self {
        VueJugements {
            jusqua_s,
            jugements: Vec::new(),
            frappes: Vec::new(),
            places_perdues: Vec::new(),
            releves: Vec::new(),
            passage_fini: None,
            boucle_proposee: None,
        }
    }
}

/// Range les jugements d'une fenêtre dans leur mesure (à leur passage), puis
/// relève les mesures dont la fin attendue est passée et ferme les passages
/// de boucle finis : l'escalier en tire sa proposition. Un passage n'est
/// compté que s'il a été joué en entier (toutes les mesures de la boucle
/// qui attendent une note sûre relevées).
fn relever_fenetre(
    coach: &mut EtatCoach,
    horloge: &Horloge,
    boucle: Option<(f64, f64)>,
    jugements: &[Jugement],
    frappes: &[Frappe],
    jusqua_s: f64,
) -> (Vec<Releve>, Option<PassageFini>) {
    let r = horloge.vitesse.max(1e-3);
    for j in jugements {
        let (tau, k) = match (j.reference, j.frappe) {
            (Some(i), _) => (coach.refs[i].tau_s, j.passage),
            (None, Some(f)) => horloge.morceau_a(frappes[f].t_s),
            (None, None) => continue,
        };
        let Some((mesure, a, b)) = mesure_de(&coach.mesures, tau) else {
            continue;
        };
        // L'instant attendu de `tau` à ce passage, s'il existe ; sinon (un
        // coup en trop hors boucle), celui que donne l'horloge sans boucle.
        let Some(t_tau) = horloge.attendu(tau, k).or_else(|| horloge.attendu(tau, 0)) else {
            continue;
        };
        let fin_s = t_tau + (b - tau) / r;
        let fin_boucle_s = boucle
            .filter(|&(la, lb)| a >= la - 1e-3 && b <= lb + 1e-3)
            .map(|(_, lb)| t_tau + (lb - tau) / r);
        let x = ((tau - a) / (b - a)).clamp(0.0, 1.0);
        match coach
            .ouvertes
            .iter_mut()
            .find(|o| o.mesure == mesure && (o.fin_s - fin_s).abs() < MEME_FIN_S)
        {
            Some(o) => o.jugements.push((*j, x)),
            None => coach.ouvertes.push(MesureOuverte {
                mesure,
                fin_s,
                fin_boucle_s,
                jugements: vec![(*j, x)],
            }),
        }
    }

    let mut releves = Vec::new();
    let (finies, ouvertes): (Vec<_>, Vec<_>) = std::mem::take(&mut coach.ouvertes)
        .into_iter()
        .partition(|o| o.fin_s <= jusqua_s);
    coach.ouvertes = ouvertes;
    for o in finies {
        let rel = relever(o.mesure, &o.jugements);
        if let Some(fb) = o.fin_boucle_s {
            let p = match coach
                .passages
                .iter_mut()
                .position(|p| (p.fin_s - fb).abs() < MEME_FIN_S)
            {
                Some(i) => &mut coach.passages[i],
                None => {
                    coach.passages.push(PassageOuvert {
                        fin_s: fb,
                        ..Default::default()
                    });
                    coach.passages.last_mut().expect("poussé")
                }
            };
            p.mesures.insert(o.mesure);
            p.attendues += rel.attendues;
            p.attrapees += rel.attrapees;
            p.ecarts.extend(o.jugements.iter().filter_map(|(j, _)| {
                matches!(
                    j.verdict,
                    Verdict::Parfait | Verdict::Bon | Verdict::Avance | Verdict::Retard
                )
                .then_some(j.ecart_musical_ms)
                .flatten()
            }));
        }
        if !rel.vide() {
            releves.push(rel);
        }
    }
    releves.sort_by_key(|r| r.mesure);

    // Les passages dont la dernière mesure est passée (avec de la marge :
    // elle se ferme dans le même appel ou avant).
    let (finis, encore): (Vec<_>, Vec<_>) = std::mem::take(&mut coach.passages)
        .into_iter()
        .partition(|p| p.fin_s + 0.05 <= jusqua_s);
    coach.passages = encore;
    let mut passage_fini = None;
    if let Some((la, lb)) = boucle {
        for p in finis {
            let entier = coach
                .mesures
                .windows(2)
                .zip(&coach.mesures_sures)
                .enumerate()
                .filter(|(_, (w, &sure))| sure && w[0] >= la - 1e-3 && w[1] <= lb + 1e-3)
                .all(|(i, _)| p.mesures.contains(&(i as u32)));
            if !entier || p.attendues == 0 {
                continue;
            }
            let ecart_et_ms = ecart_type(&p.ecarts);
            let proposition =
                coach
                    .escalier
                    .apres(horloge.vitesse, p.attendues, p.attrapees, ecart_et_ms, 1.0);
            passage_fini = Some(PassageFini {
                attendues: p.attendues,
                attrapees: p.attrapees,
                ecart_et_ms,
                propre: rusty_music_editor::coach::mesures::propre(
                    p.attendues,
                    p.attrapees,
                    ecart_et_ms,
                ),
                proposition,
            });
        }
    }
    (releves, passage_fini)
}

fn ecart_type(x: &[f64]) -> Option<f64> {
    if x.len() < 2 {
        return None;
    }
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    Some((x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt())
}

/// Cumule les relevés dans la table du morceau armé et rend la boucle à
/// proposer d'après la table mise à jour.
fn garder_releves(
    lib: &rusty_music_core::db::Library,
    chemin: &str,
    mesures: &[f64],
    releves: &[Releve],
) -> Result<Option<BoucleProposee>, String> {
    let mut table = lib.mesures_pratique(chemin, INSTRUMENT).map_err(echec)?;
    let mut nouvelles = Vec::new();
    for r in releves {
        let i = table.iter().position(|m| m.mesure == r.mesure);
        let m = cumuler(i.map(|i| &table[i]), r);
        m.verifier()?;
        match i {
            Some(i) => table[i] = m.clone(),
            None => table.push(m.clone()),
        }
        nouvelles.push(m);
    }
    lib.garder_mesures_pratique(chemin, INSTRUMENT, &nouvelles)
        .map_err(echec)?;
    Ok(boucle_proposee(mesures, &table))
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
        return Ok(VueJugements::vide(jusqua.max(depuis)));
    };
    let mut coach = verrou(&etat.coach);
    let mut vue = juger_sur(&mut coach, &ancre, boucle, depuis, jusqua);
    if let (Some(chemin), false) = (coach.chemin.clone(), vue.releves.is_empty()) {
        // Une table qui ne se garde pas n'empêche pas de jouer.
        match garder_releves(&verrou(&etat.lib), &chemin, &coach.mesures, &vue.releves) {
            Ok(b) => vue.boucle_proposee = b,
            Err(e) => eprintln!("coach : table par mesure non gardée : {e}"),
        }
    }
    Ok(vue)
}

/// La table par mesure du coach pour un morceau.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueTable {
    mesures: Vec<rusty_music_core::db::MesurePratique>,
    /// La boucle à proposer sur les pires mesures, si le coach est armé sur
    /// ce morceau (il faut sa grille de mesures).
    boucle_proposee: Option<BoucleProposee>,
}

/// La table par mesure gardée pour le morceau `chemin` (la batterie).
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn coach_table(etat: State<Etat>, chemin: String) -> Result<VueTable, String> {
    let coach = verrou(&etat.coach);
    let mesures = verrou(&etat.lib)
        .mesures_pratique(&chemin, INSTRUMENT)
        .map_err(echec)?;
    let boucle_proposee = (coach.chemin.as_deref() == Some(chemin.as_str()))
        .then(|| boucle_proposee(&coach.mesures, &mesures))
        .flatten();
    Ok(VueTable {
        mesures,
        boucle_proposee,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        armer(&mut coach, None, &coups, vec![0.0, 2.0, 4.0]);
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
            let v = juger_sur(&mut coach, &ancre, None, t, t + 0.3);
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
        let v = juger_sur(&mut EtatCoach::default(), &ancre, None, 0.0, 1.0);
        assert!(v.jugements.is_empty());
        assert_eq!(v.jusqua_s, 1.0);
        let mut coach = coach_simule(Profil::general_midi());
        injecter(&coach, 0.5, &[0x99, 38, 90]);
        assert!(juger_sur(&mut coach, &ancre, None, 0.0, 1.0)
            .jugements
            .is_empty());
    }
    /// Un rythme de deux mesures de 2 s (grosse caisse et caisse claire en
    /// noires, sûres ; une crash incertaine au début de chacune).
    fn rythme() -> Vec<Coup> {
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
        for m in 0..2 {
            coups.push(Coup {
                instant_s: m as f32 * 2.0,
                piece: Piece::Cymbales,
                force: 0.9,
            });
        }
        coups
    }

    /// Joue `passages` passages de la boucle [0, 4) à vitesse `r` (le
    /// morceau part de 0 à l'instant 10 de l'horloge), en sautant les coups
    /// que `sauter(passage, rang)` désigne et en décalant chacun de
    /// `ecart(passage, rang)` secondes du morceau ; puis juge par fenêtres
    /// de 0,3 s et rend toutes les vues.
    fn jouer_boucle(
        coach: &mut EtatCoach,
        r: f64,
        passages: i64,
        sauter: impl Fn(i64, usize) -> bool,
        ecart: impl Fn(i64, usize) -> f64,
    ) -> Vec<VueJugements> {
        let coups = rythme();
        let ancre = Ancre {
            t_s: 10.0,
            morceau_s: 0.0,
            vitesse: r,
        };
        for k in 0..passages {
            for (i, c) in coups.iter().enumerate().take(8) {
                if sauter(k, i) {
                    continue;
                }
                let note = if c.piece == Piece::GrosseCaisse {
                    36
                } else {
                    38
                };
                let t = 10.0 + (c.instant_s as f64 + 4.0 * k as f64 + ecart(k, i)) / r;
                injecter(coach, t, &[0x99, note, 90]);
            }
        }
        let fin = 10.0 + 4.0 * passages as f64 / r + 1.0;
        let mut vues = Vec::new();
        let mut t = 9.8;
        while t < fin {
            let v = juger_sur(coach, &ancre, Some((0.0, 4.0)), t, t + 0.3);
            t = v.jusqua_s;
            vues.push(v);
        }
        vues
    }

    #[test]
    fn le_groove_se_nourrit_des_fenetres_jugees() {
        let mut coach = coach_simule(Profil::general_midi());
        armer(&mut coach, None, &rythme(), vec![0.0, 2.0, 4.0]);
        poser_temps(&mut coach, (0..=8).map(|i| i as f64 * 0.5).rev().collect());
        assert_eq!(coach.temps.len(), 9);
        // Trois passages à 0,8, la caisse claire 25 ms (du morceau) devant.
        jouer_boucle(
            &mut coach,
            0.8,
            3,
            |_, _| false,
            |_, i| if i % 2 == 1 { -0.025 } else { 0.0 },
        );
        // 24 notes sûres attrapées : 12 grosses caisses, 12 caisses claires ;
        // la crash n'entre pas.
        assert_eq!(coach.echantillons.len(), 24);
        let g = groove_de(&coach);
        assert_eq!(g.conseils.len(), 1, "{:?}", g.conseils);
        assert_eq!(g.conseils[0].classe, Some(Classe::CaisseClaire));
        assert_eq!(
            g.conseils[0].texte,
            "Tu joues la caisse claire 25 ms devant le disque."
        );
        let cc = g
            .pieces
            .iter()
            .find(|p| p.classe == Classe::CaisseClaire)
            .unwrap();
        assert!((cc.jeu_grille_ms.unwrap() + 25.0).abs() < 1.0, "{cc:?}");
        // Réarmer repart de zéro.
        armer(&mut coach, None, &rythme(), vec![0.0, 2.0, 4.0]);
        assert!(coach.echantillons.is_empty() && coach.temps.is_empty());
    }

    #[test]
    fn chaque_mesure_est_relevee_a_chaque_passage() {
        let mut coach = coach_simule(Profil::general_midi());
        armer(&mut coach, None, &rythme(), vec![0.0, 2.0, 4.0]);
        assert_eq!(coach.mesures_sures, vec![true, true]);
        // Trois passages à 0,8 : le premier juste, le deuxième sans la
        // caisse claire de 1,5 s, le troisième pressé dans la mesure 2
        // (0, −20, −40, −60 ms du morceau).
        let vues = jouer_boucle(
            &mut coach,
            0.8,
            3,
            |k, i| k == 1 && i == 3,
            |k, i| {
                if k == 2 && i >= 4 {
                    -0.020 * (i - 4) as f64
                } else {
                    0.0
                }
            },
        );
        let releves: Vec<&Releve> = vues.iter().flat_map(|v| &v.releves).collect();
        assert_eq!(releves.len(), 6, "{releves:?}");
        assert_eq!(
            releves.iter().map(|r| r.mesure).collect::<Vec<_>>(),
            vec![0, 1, 0, 1, 0, 1]
        );
        assert!(
            releves.iter().all(|r| r.attendues == 4),
            "crash non comptée"
        );
        assert_eq!(releves[0].erreur, 0.0);
        assert_eq!((releves[2].manquees, releves[2].attrapees), (1, 3));
        assert!((releves[2].erreur - 0.25).abs() < 1e-9);
        let pente = releves[5].pente_ms.unwrap();
        assert!((pente + 80.0).abs() < 1.0, "pente {pente}");
        assert!(releves[5].ecart_moy_ms.unwrap() < -25.0);
        // Un passage fini par tour de boucle, propre seulement le premier
        // et le troisième ; personne ne propose rien après un seul propre.
        let passages: Vec<&PassageFini> = vues
            .iter()
            .filter_map(|v| v.passage_fini.as_ref())
            .collect();
        assert_eq!(passages.len(), 3);
        assert_eq!(
            passages.iter().map(|p| p.propre).collect::<Vec<_>>(),
            vec![true, false, true]
        );
        assert_eq!((passages[1].attendues, passages[1].attrapees), (8, 7));
        assert!(passages.iter().all(|p| p.proposition.is_none()));
        // Rien ne traîne : seul le quatrième passage, que la boucle a
        // commencé sans personne, reste ouvert.
        let jusqua = vues.last().unwrap().jusqua_s;
        assert!(coach.ouvertes.iter().all(|o| o.fin_s > jusqua));
        assert!(coach.passages.iter().all(|p| p.fin_s > jusqua));
    }

    #[test]
    fn l_escalier_propose_apres_deux_passages_propres() {
        let mut coach = coach_simule(Profil::general_midi());
        armer(&mut coach, None, &rythme(), vec![0.0, 2.0, 4.0]);
        let vues = jouer_boucle(&mut coach, 0.7, 2, |_, _| false, |_, _| 0.0);
        let p: Vec<_> = vues.iter().filter_map(|v| v.passage_fini.clone()).collect();
        assert_eq!(p.len(), 2);
        let prop = p[1].proposition.unwrap();
        assert!(
            prop.monter && (prop.vitesse - 0.73).abs() < 1e-9,
            "{prop:?}"
        );

        // Un passage commencé en route (la première mesure non jouée) ne
        // compte pas pour l'escalier.
        let mut coach = coach_simule(Profil::general_midi());
        armer(&mut coach, None, &rythme(), vec![0.0, 2.0, 4.0]);
        let vues = jouer_boucle(&mut coach, 0.7, 1, |_, i| i < 4, |_, _| 0.0);
        // La mesure 1 jouée ; la 0, tout manqué : un passage entier, raté.
        let p: Vec<_> = vues.iter().filter_map(|v| v.passage_fini.clone()).collect();
        assert_eq!(p.len(), 1);
        assert!(!p[0].propre);
        assert_eq!(p[0].proposition.unwrap().vitesse, 0.67);
    }

    #[test]
    fn la_table_se_cumule_et_propose_la_pire_mesure() {
        let lib = rusty_music_core::db::Library::open_in_memory().unwrap();
        let mut coach = coach_simule(Profil::general_midi());
        let chemin = "/Musique/coach.flac";
        armer(
            &mut coach,
            Some(chemin.into()),
            &rythme(),
            vec![0.0, 2.0, 4.0],
        );
        // Deux passages : la mesure 2 (rang 1) perd deux notes à chaque fois.
        let vues = jouer_boucle(&mut coach, 0.8, 2, |_, i| i == 5 || i == 6, |_, _| 0.0);
        let mut derniere = None;
        for v in &vues {
            if !v.releves.is_empty() {
                derniere = garder_releves(&lib, chemin, &coach.mesures, &v.releves).unwrap();
            }
        }
        let table = lib.mesures_pratique(chemin, INSTRUMENT).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!((table[0].passages, table[0].score), (2, 0.0));
        assert_eq!(
            (table[1].passages, table[1].attendues, table[1].manquees),
            (2, 8, 4)
        );
        assert!((table[1].score - 0.5).abs() < 1e-9);
        assert_eq!(
            derniere,
            Some(BoucleProposee {
                premiere: 1,
                derniere: 1,
                debut_s: 2.0,
                fin_s: 4.0,
                score: 0.5,
                vitesse: Escalier::DEPART,
            })
        );
    }
}
