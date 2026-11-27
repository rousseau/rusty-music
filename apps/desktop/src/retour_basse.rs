// SPDX-License-Identifier: GPL-3.0-or-later
//! Entendre sa basse pendant la pratique (E7, tranche 3) : le canal retenu
//! par le coach (`coach_basse.rs`, V34) passe par un préréglage de son
//! (`rusty_music_player::instrument`) et sort sur la sortie du Mac, à côté
//! du morceau (`rusty_music_player::retour`).
//!
//! Le retour naît avec l'écoute du coach, **muet** : rien ne sort tant qu'il
//! n'est pas activé, et jamais à plein volume la première fois (moitié). Une
//! entrée qui semble être un micro (micro intégré, entrée 1 d'une Scarlett
//! Solo) est signalée : larsen possible sur des enceintes.
//!
//! La balance morceau / instrument baisse l'un ou l'autre, jamais les deux :
//! au milieu, chacun à son volume. Côté morceau, elle passe par la part du
//! multipiste (`Multipiste::regler_part`), pas par le volume de la barre du
//! bas. Réglages gardés par instrument (`retour_instrument`, migration
//! n° 17).
//!
//! Le coach et l'accordeur analysent le signal brut : l'entrée du retour est
//! poussée telle quelle, avant l'analyse, et le son traité n'existe que dans
//! le rappel de sortie.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use rusty_music_core::db::RetourInstrument;
use rusty_music_player::instrument::{Chaine, Instrument, Prereglage, Reglage};
use rusty_music_player::retour::{self, Commandes, Entree, Moteur, Sortie};

use super::*;

/// L'instrument dont on garde les réglages.
pub(crate) const INSTRUMENT: &str = "basse";

/// Volume du premier lancement : la moitié, jamais plein.
const VOLUME_DEPART: f32 = 0.5;

/// Laisse la rampe de volume descendre avant de fermer la sortie (pas de
/// clic à l'arrêt).
const FONDU_ARRET: Duration = Duration::from_millis(30);

/// La part du morceau dans la balance, lue au chargement des stems
/// (`etabli.rs`) : un jeu rechargé (transposition, greffe) la garde.
static PART_MORCEAU: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

/// La part du morceau en cours.
pub(crate) fn part_morceau() -> f32 {
    f32::from_bits(PART_MORCEAU.load(Ordering::Relaxed))
}

fn poser_part(etat: &Etat, part: f32) {
    PART_MORCEAU.store(part.to_bits(), Ordering::Relaxed);
    if let Some(m) = verrou(&etat.stems).as_ref() {
        m.regler_part(part);
    }
}

/// Le morceau reprend toute sa place (retour coupé ou écoute fermée).
/// Ne prend que le verrou des stems ; peut être appelé sous celui du coach.
pub(crate) fn morceau_plein(etat: &Etat) {
    PART_MORCEAU.store(1.0f32.to_bits(), Ordering::Relaxed);
    if let Ok(g) = etat.stems.try_lock() {
        if let Some(m) = g.as_ref() {
            m.regler_part(1.0);
        }
    }
}

/// Parts de la balance `b` (−1 morceau seul, 1 instrument seul) : (morceau,
/// instrument). Au milieu, les deux pleines.
pub(crate) fn parts(b: f32) -> (f32, f32) {
    let b = if b.is_finite() {
        b.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    ((1.0 - b).min(1.0), (1.0 + b).min(1.0))
}

/// L'entrée `nom` semble-t-elle être un micro, sur le canal `canal` (compté
/// depuis 0) ? Micro intégré ou de casque, ou entrée 1 d'une Scarlett Solo
/// (l'instrument est sur la 2).
pub(crate) fn semble_micro(nom: &str, canal: usize) -> bool {
    let n = nom.to_lowercase();
    // Le Mustang Micro (Fender) est un préampli de guitare en USB-C, pas un
    // microphone : son nom trompait le mot « micro » (26 nov.).
    if n.contains("mustang micro") {
        return false;
    }
    let micro = [
        "micro",
        "mic ",
        "built-in",
        "intégré",
        "macbook",
        "airpods",
        "iphone",
        "casque",
    ]
    .iter()
    .any(|m| n.contains(m))
        || n.ends_with(" mic");
    micro || (n.contains("scarlett solo") && canal == 0)
}

/// Le retour d'une écoute du coach.
pub(crate) struct Retour {
    commandes: Commandes,
    moteur: Arc<Mutex<Moteur>>,
    sortie: Option<Sortie>,
    sr_entree: u32,
    sr_sortie: u32,
    reglage: RetourInstrument,
    /// Sortie ouverte et son audible (sauf volume nul).
    actif: bool,
    micro: bool,
    /// Le réglage « comme le disque » du dernier morceau mesuré, et
    /// l'instrument dont le stem a servi (« bass », « guitar »).
    disque: Option<(i64, String, Option<Reglage>)>,
    /// Guitare, « comme le disque » : le profil du stem du morceau et les
    /// secondes de jeu sur lesquelles l'égaliseur a été calculé.
    guitare: Option<GuitareDisque>,
}

/// L'égaliseur « comme le disque » d'une guitare : du profil de ce qu'on
/// joue à celui du stem. Recalculé au fil du jeu ([`RAFRAICHIR_S`]).
#[derive(Clone, Copy)]
struct GuitareDisque {
    id: i64,
    cible: Option<[f32; 5]>,
    secondes: f32,
}

/// Jeu minimal avant de partir du profil joué plutôt que de la référence.
const JEU_MIN_S: f32 = 3.0;
/// L'égaliseur se recalcule toutes les tant de secondes jouées…
const RAFRAICHIR_S: f32 = 5.0;
/// … jusqu'à ce que le profil soit sûr.
const JEU_SUR_S: f32 = 60.0;

impl GuitareDisque {
    fn reglage(&self, joue: Option<([f32; 5], f32)>) -> Option<Reglage> {
        let depart = joue.filter(|j| j.1 >= JEU_MIN_S).map(|j| j.0);
        let sr = rusty_music_editor::decode::SR as f32;
        self.cible
            .map(|c| Reglage::comme_le_disque_guitare(c, depart, sr))
    }
}

impl Retour {
    /// Prépare un retour pour une entrée à `sr_entree` Hz, muet, avec les
    /// réglages gardés (ou ceux du premier lancement).
    pub(crate) fn creer(
        sr_entree: u32,
        garde: Option<RetourInstrument>,
    ) -> Result<(Entree, Retour), String> {
        let sr_sortie = retour::sr_sortie_par_defaut()?;
        let reglage = garde
            .filter(|g| Prereglage::analyser(&g.prereglage).is_some())
            .unwrap_or(RetourInstrument {
                instrument: INSTRUMENT.into(),
                prereglage: Prereglage::Brut.nom().into(),
                volume: VOLUME_DEPART,
                balance: 0.0,
            });
        // « Comme le disque » attend le stem : brut jusque-là.
        let chaine = chaine_de(&reglage.prereglage, None, sr_sortie);
        let (entree, moteur, commandes) = retour::retour(sr_entree, sr_sortie, chaine)?;
        let r = Retour {
            commandes,
            moteur: Arc::new(Mutex::new(moteur)),
            sortie: None,
            sr_entree,
            sr_sortie,
            reglage,
            actif: false,
            micro: false,
            disque: None,
            guitare: None,
        };
        r.appliquer_volume();
        Ok((entree, r))
    }

    /// Le canal jugé a changé : le retour le suit.
    pub(crate) fn suivre_canal(&mut self, entree: &str, canal: usize) {
        self.commandes.partage.choisir_canal(canal);
        self.micro = semble_micro(entree, canal);
    }

    fn appliquer_volume(&self) {
        let (_, part) = parts(self.reglage.balance);
        let p = &self.commandes.partage;
        p.regler_volume(self.reglage.volume * part);
        p.couper(!self.actif);
    }

    /// Ouvre ou ferme la sortie.
    fn activer(&mut self, actif: bool) -> Result<(), String> {
        if actif && self.sortie.is_none() {
            // La sortie par défaut a pu changer (casque branché) depuis
            // l'ouverture de l'écoute.
            let sr = retour::sr_sortie_par_defaut()?;
            if sr != self.sr_sortie {
                let chaine = self.chaine(sr);
                verrou(&self.moteur).preparer(self.sr_entree, sr, chaine)?;
                self.sr_sortie = sr;
            }
            self.sortie = Some(retour::jouer(self.moteur.clone())?);
        }
        self.actif = actif;
        self.appliquer_volume();
        if !actif && self.sortie.is_some() {
            std::thread::sleep(FONDU_ARRET);
            self.sortie = None;
        }
        Ok(())
    }

    /// Guitare, « comme le disque » : l'égaliseur suit le profil de ce qui
    /// est joué, recalculé toutes les [`RAFRAICHIR_S`] secondes jouées
    /// jusqu'à [`JEU_SUR_S`].
    fn rafraichir_guitare(&mut self, joue: Option<([f32; 5], f32)>) {
        let Some(g) = self.guitare else { return };
        let Some((_, s)) = joue else { return };
        let comme =
            Prereglage::analyser(&self.reglage.prereglage) == Some(Prereglage::CommeLeDisque);
        if !comme
            || g.cible.is_none()
            || s < JEU_MIN_S
            || g.secondes >= JEU_SUR_S
            || s < g.secondes + RAFRAICHIR_S
        {
            return;
        }
        let reglage = g.reglage(joue);
        let ancien = self.disque.replace((g.id, "guitar".into(), reglage));
        if self.commandes.changer_chaine(self.chaine(self.sr_sortie)) {
            self.guitare = Some(GuitareDisque { secondes: s, ..g });
        } else {
            // Le son d'avant n'est pas encore pris : au prochain sondage.
            self.disque = ancien;
        }
    }

    fn chaine(&self, sr: u32) -> Chaine {
        let disque = self.disque.as_ref().and_then(|(_, _, r)| r.as_ref());
        chaine_de(&self.reglage.prereglage, disque, sr)
    }

    /// Latence estimée entre l'entrée et la sortie, en ms : le moteur, plus
    /// un tampon d'appareil de chaque côté. Sans les convertisseurs.
    fn latence_ms(&self) -> Option<f64> {
        let s = self.sortie.as_ref()?;
        let moteur = verrou(&self.moteur).latence_trames();
        let tampon_s = f64::from(s.tampon.unwrap_or(retour::TAMPON_APPAREIL));
        let tampon_e =
            f64::from(retour::TAMPON_APPAREIL) * f64::from(s.sr) / f64::from(self.sr_entree.max(1));
        Some((moteur + tampon_s + tampon_e) * 1000.0 / f64::from(s.sr.max(1)))
    }

    fn vue(&self, ecoute: bool) -> VueRetourBasse {
        let p = &self.commandes.partage;
        VueRetourBasse {
            ecoute,
            disponible: true,
            actif: self.actif,
            prereglage: self.reglage.prereglage.clone(),
            volume: self.reglage.volume,
            balance: self.reglage.balance,
            micro: self.micro,
            latence_ms: self.latence_ms(),
            stem_disque: self.disque.as_ref().map(|(_, _, r)| r.is_some()),
            jeu_s: self.guitare.map(|g| g.secondes),
            trous: p.sous_alimentations(),
            prereglages: prereglages(),
        }
    }
}

/// La chaîne d'un préréglage par son nom ; « comme le disque » sans stem
/// mesuré sonne brut.
fn chaine_de(nom: &str, disque: Option<&Reglage>, sr: u32) -> Chaine {
    let p = Prereglage::analyser(nom).unwrap_or(Prereglage::Brut);
    let reglage = match (p, disque) {
        (Prereglage::CommeLeDisque, Some(r)) => r.clone(),
        (Prereglage::CommeLeDisque, None) => Reglage::de(Prereglage::Brut),
        _ => Reglage::de(p),
    };
    Chaine::new(&reglage, sr as f32)
}

fn prereglages() -> Vec<PrereglageBasse> {
    Instrument::Basse
        .prereglages()
        .iter()
        .map(|p| PrereglageBasse {
            nom: p.nom().into(),
            libelle: p.libelle().into(),
        })
        .collect()
}

/// Un préréglage, tel que l'interface le montre.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct PrereglageBasse {
    nom: String,
    libelle: String,
}

/// Ce que l'interface sait du retour.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct VueRetourBasse {
    /// Une entrée est écoutée par le coach.
    ecoute: bool,
    /// Le retour a pu être préparé (une sortie audio existe).
    disponible: bool,
    /// Le son de l'instrument sort.
    actif: bool,
    prereglage: String,
    /// Volume de l'instrument, 0 à 1.
    volume: f32,
    /// −1 morceau seul, 0 les deux, 1 instrument seul.
    balance: f32,
    /// L'entrée semble être un micro : larsen possible.
    micro: bool,
    /// Latence estimée entrée → sortie (ms), sortie ouverte.
    latence_ms: Option<f64>,
    /// « Comme le disque » : `Some(false)` si le morceau n'a pas de stem de
    /// basse, `None` s'il n'a pas été demandé.
    stem_disque: Option<bool>,
    /// Guitare, « comme le disque » : les secondes de votre jeu d'après
    /// lesquelles l'égaliseur est calculé (0 : d'après une guitare de
    /// référence, faute de jeu entendu).
    jeu_s: Option<f32>,
    /// Trous depuis l'ouverture (l'entrée n'a pas suivi : un craquement).
    trous: u64,
    prereglages: Vec<PrereglageBasse>,
}

fn vue_sans_retour(ecoute: bool, garde: Option<RetourInstrument>) -> VueRetourBasse {
    let g = garde.unwrap_or(RetourInstrument {
        instrument: INSTRUMENT.into(),
        prereglage: Prereglage::Brut.nom().into(),
        volume: VOLUME_DEPART,
        balance: 0.0,
    });
    VueRetourBasse {
        ecoute,
        disponible: false,
        actif: false,
        prereglage: g.prereglage,
        volume: g.volume,
        balance: g.balance,
        micro: false,
        latence_ms: None,
        stem_disque: None,
        jeu_s: None,
        trous: 0,
        prereglages: prereglages(),
    }
}

/// L'état du retour (préréglages, réglages gardés, latence).
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn retour_basse_etat(etat: State<Etat>) -> Result<VueRetourBasse, String> {
    let mut coach = verrou(&etat.coach_basse);
    let ecoute = coach.ecoute_ouverte();
    let joue = coach.profil_entree();
    if let Some(r) = coach.retour_mut() {
        r.rafraichir_guitare(joue);
        return Ok(r.vue(true));
    }
    drop(coach);
    let garde = verrou(&etat.lib)
        .retour_instrument(INSTRUMENT)
        .map_err(echec)?;
    Ok(vue_sans_retour(ecoute, garde))
}

/// Règle le retour : sortie ouverte ou non, préréglage, volume (0 à 1),
/// balance (−1 à 1). `id` est le morceau ouvert, pour « comme le disque »,
/// et `instrument` celui qu'on joue (« bass » par défaut, « guitar ») : son
/// stem sert de modèle.
/// Gardé pour l'instrument.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn retour_basse_regler(
    etat: State<Etat>,
    actif: bool,
    prereglage: String,
    volume: f32,
    balance: f32,
    id: Option<i64>,
    instrument: Option<String>,
) -> Result<VueRetourBasse, String> {
    let instrument = match instrument.as_deref() {
        Some("guitar") => "guitar",
        _ => "bass",
    };
    if Prereglage::analyser(&prereglage).is_none() {
        return Err(format!("préréglage inconnu : {prereglage}"));
    }
    if !volume.is_finite() || !balance.is_finite() {
        return Err("volume ou balance invalide".into());
    }
    let volume = volume.clamp(0.0, 1.0);
    let balance = balance.clamp(-1.0, 1.0);
    // Le stem se décode hors des verrous du coach : il peut prendre une
    // seconde.
    // La guitare : l'égaliseur part de ce qui est joué (`ProfilEnCours`),
    // le profil du stem se garde par morceau.
    let mut guitare = None;
    let disque = match (Prereglage::analyser(&prereglage), id) {
        (Some(Prereglage::CommeLeDisque), Some(id)) if instrument == "guitar" => {
            let (deja, joue) = {
                let mut coach = verrou(&etat.coach_basse);
                let joue = coach.profil_entree();
                let deja = coach
                    .retour_mut()
                    .and_then(|r| r.guitare.filter(|g| g.id == id).map(|g| g.cible));
                (deja, joue)
            };
            let cible = match deja {
                Some(c) => c,
                None => coach_basse::stem_mono(&etat, id, "guitar")?.map(|m| {
                    rusty_music_player::instrument::profil_bandes_de(
                        &m,
                        rusty_music_editor::decode::SR as f32,
                        &rusty_music_player::instrument::BANDES_GUITARE_HZ,
                    )
                }),
            };
            let g = GuitareDisque {
                id,
                cible,
                secondes: joue.map_or(0.0, |j| if j.1 >= JEU_MIN_S { j.1 } else { 0.0 }),
            };
            guitare = Some(g);
            Some((id, "guitar".to_string(), g.reglage(joue)))
        }
        (Some(Prereglage::CommeLeDisque), Some(id)) => {
            let deja = verrou(&etat.coach_basse).retour_mut().and_then(|r| {
                r.disque
                    .as_ref()
                    .filter(|(d, i, _)| *d == id && i == instrument)
                    .cloned()
            });
            match deja {
                Some(d) => Some(d),
                None => {
                    let stem = coach_basse::stem_mono(&etat, id, instrument)?;
                    let sr = rusty_music_editor::decode::SR as f32;
                    Some((
                        id,
                        instrument.to_string(),
                        stem.map(|m| Reglage::comme_le_disque(&m, sr)),
                    ))
                }
            }
        }
        _ => None,
    };
    let garde = RetourInstrument {
        instrument: INSTRUMENT.into(),
        prereglage,
        volume,
        balance,
    };
    let (vue, part) = {
        let mut coach = verrou(&etat.coach_basse);
        match coach.retour_mut() {
            None => (None, 1.0),
            Some(r) => {
                let son_change = r.reglage.prereglage != garde.prereglage || disque.is_some();
                if let Some(d) = disque {
                    r.disque = Some(d);
                }
                if guitare.is_some() {
                    r.guitare = guitare;
                }
                r.reglage = garde.clone();
                if son_change {
                    let chaine = r.chaine(r.sr_sortie);
                    if !r.commandes.changer_chaine(chaine) {
                        return Err("le son précédent n'est pas encore pris".into());
                    }
                }
                r.activer(actif)?;
                let part = if r.actif { parts(balance).0 } else { 1.0 };
                (Some(r.vue(true)), part)
            }
        }
    };
    poser_part(&etat, part);
    verrou(&etat.lib)
        .garder_retour_instrument(&garde)
        .map_err(echec)?;
    Ok(vue.unwrap_or_else(|| vue_sans_retour(false, Some(garde))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_balance_ne_baisse_qu_un_cote() {
        assert_eq!(parts(0.0), (1.0, 1.0));
        assert_eq!(parts(1.0), (0.0, 1.0));
        assert_eq!(parts(-1.0), (1.0, 0.0));
        assert_eq!(parts(0.5), (0.5, 1.0));
        assert_eq!(parts(-0.25), (1.0, 0.75));
        assert_eq!(parts(f32::NAN), (1.0, 1.0));
        assert_eq!(parts(7.0), (0.0, 1.0));
    }

    #[test]
    fn un_micro_se_reconnait() {
        assert!(semble_micro("Microphone MacBook Pro", 0));
        assert!(semble_micro("Scarlett Solo USB", 0));
        assert!(!semble_micro("Scarlett Solo USB", 1));
        assert!(!semble_micro("Scarlett 2i2 USB", 0));
        assert!(semble_micro("AirPods de François", 0));
        assert!(semble_micro("USB Mic", 0));
        assert!(!semble_micro("Focusrite USB", 0));
        assert!(!semble_micro("Mustang Micro Plus", 0));
    }

    #[test]
    fn la_guitare_s_egalise_d_apres_son_jeu_passe_trois_secondes() {
        use rusty_music_player::instrument::Filtre;
        let gains = |r: Option<Reglage>| -> Vec<f32> {
            r.unwrap()
                .filtres
                .iter()
                .map(|f| match f {
                    Filtre::Cloche { db, .. } => *db,
                    _ => 0.0,
                })
                .collect()
        };
        let cible = [3.0, 1.0, 0.0, -1.0, -3.0];
        let g = GuitareDisque {
            id: 1,
            cible: Some(cible),
            secondes: 0.0,
        };
        // Joué comme le stem : plat, mais seulement après trois secondes.
        assert!(gains(g.reglage(Some((cible, 10.0))))
            .iter()
            .all(|d| d.abs() < 1e-3));
        assert!(gains(g.reglage(Some((cible, 1.0))))
            .iter()
            .any(|d| d.abs() > 0.5));
        // Sans stem de guitare, rien.
        let sans = GuitareDisque { cible: None, ..g };
        assert!(sans.reglage(Some((cible, 10.0))).is_none());
    }

    #[test]
    fn comme_le_disque_sans_stem_sonne_brut() {
        let mut a = chaine_de("comme-le-disque", None, 48_000);
        let mut b = chaine_de("brut", None, 48_000);
        let mut x: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();
        let mut y = x.clone();
        a.traiter(&mut x);
        b.traiter(&mut y);
        assert_eq!(x, y);
        // Nom inconnu : brut aussi, jamais de panique.
        let mut c = chaine_de("inconnu", None, 48_000);
        let mut z: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();
        c.traiter(&mut z);
        assert_eq!(z, y);
    }

    /// Le retour d'une écoute simulée : rien ne sort avant l'activation, le
    /// coach garde le brut.
    #[test]
    fn le_retour_part_muet_a_mi_volume() {
        let (_, moteur, commandes) =
            retour::retour(48_000, 48_000, chaine_de("brut", None, 48_000)).unwrap();
        let r = Retour {
            commandes,
            moteur: Arc::new(Mutex::new(moteur)),
            sortie: None,
            sr_entree: 48_000,
            sr_sortie: 48_000,
            reglage: RetourInstrument {
                instrument: INSTRUMENT.into(),
                prereglage: "brut".into(),
                volume: VOLUME_DEPART,
                balance: 0.5,
            },
            actif: false,
            micro: false,
            disque: None,
            guitare: None,
        };
        r.appliquer_volume();
        assert!(r.commandes.partage.coupe());
        assert!(r.commandes.partage.volume() <= 0.5);
        let v = r.vue(true);
        assert!(!v.actif && v.latence_ms.is_none());
        assert_eq!(v.prereglages.len(), 8);
    }
}
