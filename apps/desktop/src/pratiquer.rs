// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de Pratiquer : décompte, transcription (basse, batterie,
//! MuScriptor), doigtés.

use super::*;

/// Lance les stems après un décompte de métronome (Pratiquer) : la musique
/// part de `debut`, en secondes du morceau, et `clics` donne chaque clic
/// (instant en secondes du morceau, premier temps ou non). L'interface les
/// place sur la pulsation ; le moteur les joue à l'échantillon près.
#[tauri::command(async)]
pub(crate) fn stems_decompter(
    etat: State<Etat>,
    debut: f64,
    clics: Vec<(f64, bool)>,
) -> Result<(), String> {
    if !debut.is_finite() || debut < 0.0 || clics.iter().any(|c| !c.0.is_finite()) {
        return Err(format!("décompte invalide : début {debut}"));
    }
    if let Some(m) = verrou(&etat.stems).as_ref() {
        m.decompter(std::time::Duration::from_secs_f64(debut), &clics);
    }
    Ok(())
}

/// Une transcription, telle que l'interface la consomme : les notes (pour
/// les corrections à venir) et les mesures quantifiées (pour la partition).
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct VueTranscription {
    version: u32,
    instrument: String,
    modele: String,
    notes: Vec<rusty_music_transcription::Note>,
    /// Les coups de batterie, pour la batterie.
    #[serde(default)]
    coups: Vec<rusty_music_transcription::batterie::Coup>,
    mesures: serde_json::Value,
    /// Accordage de la basse, de la corde grave à l'aiguë, en hauteurs MIDI
    /// (standard, drop D ou cinq cordes : `Accordage::pour`).
    #[serde(default)]
    accordage: Vec<u8>,
    /// Version du doigté de la basse ([`VERSION_DOIGTES`]).
    #[serde(default)]
    doigtes: u32,
    /// La basse du réglage (4 ou 5 cordes) sur laquelle le doigté a été posé.
    #[serde(default)]
    basses: tablature::Basses,
    /// Hauteurs transcrites, avant que le doigté ne ramène à l'octave ce qui
    /// sort du manche : changer de basse repart d'elles (une 4 cordes
    /// remonte le mi♭ grave qu'une 5 cordes joue).
    #[serde(default)]
    hauteurs: Vec<u8>,
    /// Version de la grille des mesures ([`VERSION_MESURES`]).
    #[serde(default)]
    grille: u32,
}

/// Version du cache de transcription : à monter dès que le calcul change.
pub(crate) const VERSION_TRANSCRIPTION: u32 = 5;

/// Version du doigté (`tablature::Couts`) : quand seul le doigté change, les
/// notes en cache sont reposées sans retranscrire (MuScriptor prend des
/// minutes). 1 : avant l'a priori appris ; 2 : a priori de position et coûts
/// réglés sur 89 tablatures (17 oct.) ; 3 : table apprise sur 331 tablatures ;
/// 4 : un motif, un doigté (`tablature::uniformiser`, 29 oct.) ; 5 : 4 ou
/// 5 cordes, toujours dans l'accordage du disque.
pub(crate) const VERSION_DOIGTES: u32 = 5;

/// Pose corde et frette sur les notes, pour les basses du réglage, puis donne
/// un seul doigté à chaque motif répété.
pub(crate) fn doigter(
    notes: &mut [rusty_music_transcription::Note],
    basses: tablature::Basses,
    premiers: &[f32],
) -> tablature::Accordage {
    let accordage = tablature::Accordage::pour(notes, basses);
    tablature::poser(notes, &accordage);
    tablature::uniformiser(notes, &accordage, &tablature::Couts::default(), premiers);
    accordage
}

/// Version de la grille des mesures (`Pulsation::regularisee`) : quand seule
/// elle change, les notes et coups en cache sont requantifiés sans
/// retranscrire. 1 : premiers temps parasites retirés (26 oct.) ; 2 :
/// anticipations coupées au premier temps recousues (29 oct.).
pub(crate) const VERSION_MESURES: u32 = 2;

/// Avancement d'une transcription MuScriptor, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct EtatTranscription {
    pub(crate) en_cours: bool,
    /// Le morceau transcrit, pour que l'interface sache si l'avancement est
    /// le sien.
    id: i64,
    faits: usize,
    total: usize,
    /// Téléchargement des poids de MuScriptor en cours, et son avancement.
    telechargement: bool,
    octets_faits: u64,
    octets_total: u64,
}

#[tauri::command]
pub(crate) fn etat_transcription(etat: State<Etat>) -> EtatTranscription {
    verrou(&etat.transcription).clone()
}

/// Les partitions déjà calculées du morceau, sans rien calculer : « bass »,
/// « bass-muscriptor », « drums » selon les caches présents et à jour. Le
/// bloc « Partitions » du rail montre ainsi « prête » plutôt que « Calculer ».
#[tauri::command(async)]
pub(crate) fn transcriptions_en_cache(etat: State<Etat>, id: i64) -> Result<Vec<String>, String> {
    let chemin = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .map(|t| t.path)
        .ok_or_else(|| format!("morceau inconnu : {id}"))?;
    let dossier = dossier_stems(&etat, Path::new(&chemin));
    Ok(["bass", "bass-muscriptor", "drums"]
        .into_iter()
        .filter(|cle| {
            std::fs::read(dossier.join(format!("transcription-{cle}.json")))
                .ok()
                .and_then(|o| serde_json::from_slice::<VueTranscription>(&o).ok())
                .is_some_and(|t| t.version == VERSION_TRANSCRIPTION)
        })
        .map(String::from)
        .collect())
}

/// Vrai si les poids de MuScriptor sont sur la machine (dossier des modèles
/// ou cache Hugging Face) : l'utilisateur doit les télécharger lui-même,
/// après avoir accepté leurs conditions d'usage sur Hugging Face.
#[tauri::command]
pub(crate) fn muscriptor_disponible() -> bool {
    rusty_music_transcription::muscriptor::poids().is_some()
}

/// Vrai si un jeton Hugging Face est déjà posé sur la machine (`HF_TOKEN`
/// ou `hf auth login`) : l'interface ne le demande alors pas.
#[tauri::command]
pub(crate) fn muscriptor_jeton_present() -> bool {
    rusty_music_transcription::muscriptor::jeton_hugging_face().is_some()
}

/// Télécharge les poids de MuScriptor (1,2 Go) dans le dossier des modèles,
/// avec le jeton saisi, sinon celui de la machine. Le jeton n'est envoyé qu'à
/// Hugging Face et n'est conservé nulle part.
#[tauri::command(async)]
pub(crate) fn telecharger_muscriptor(
    etat: State<Etat>,
    jeton: Option<String>,
) -> Result<(), String> {
    use rusty_music_transcription::muscriptor;
    let jeton = jeton
        .map(|j| j.trim().to_string())
        .filter(|j| !j.is_empty())
        .or_else(muscriptor::jeton_hugging_face);
    {
        let mut t = verrou(&etat.transcription);
        if t.telechargement {
            return Err("téléchargement déjà en cours".into());
        }
        (t.telechargement, t.octets_faits, t.octets_total) = (true, 0, muscriptor::OCTETS);
    }
    let issue = muscriptor::telecharger(jeton.as_deref(), |vus, total| {
        let mut t = verrou(&etat.transcription);
        (t.octets_faits, t.octets_total) = (vus, total.unwrap_or(muscriptor::OCTETS));
    });
    verrou(&etat.transcription).telechargement = false;
    issue.map(|_| ()).map_err(echec)
}

/// Transcrit le stem `instrument` d'un morceau (`crates/transcription`) :
/// - « bass » : Basic Pitch, ou MuScriptor si `qualite` (une minute par
///   morceau, plus juste — banc `experiments/partitions/`), puis une note à
///   la fois, corde et frette ;
/// - « drums » : ADTOF, cinq pièces.
/// Puis quantification sur la pulsation. En cache à côté des stems ;
/// quelques secondes la première fois.
#[tauri::command(async)]
pub(crate) fn transcrire(
    etat: State<Etat>,
    id: i64,
    instrument: String,
    qualite: Option<bool>,
    basses: Option<tablature::Basses>,
) -> Result<VueTranscription, String> {
    use rusty_music_transcription::{
        basic_pitch, batterie, monophonie, muscriptor, porte, quantification,
    };
    let basses = basses.unwrap_or_default();
    if instrument != "bass" && instrument != "drums" {
        return Err(format!(
            "transcription de « {instrument} » pas encore disponible"
        ));
    }
    let qualite = qualite.unwrap_or(false) && instrument == "bass";
    let chemin = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .map(|t| t.path)
        .ok_or_else(|| format!("morceau inconnu : {id}"))?;
    let source = PathBuf::from(&chemin);
    let dossier = dossier_stems(&etat, &source);
    let cache = dossier.join(if qualite {
        format!("transcription-{instrument}-muscriptor.json")
    } else {
        format!("transcription-{instrument}.json")
    });
    if let Some(mut t) = std::fs::read(&cache)
        .ok()
        .and_then(|o| serde_json::from_slice::<VueTranscription>(&o).ok())
        .filter(|t| t.version == VERSION_TRANSCRIPTION)
    {
        let doigtes_a_jour =
            instrument != "bass" || (t.doigtes == VERSION_DOIGTES && t.basses == basses);
        if doigtes_a_jour && t.grille == VERSION_MESURES {
            return Ok(t);
        }
        // Seuls le doigté ou la grille des mesures ont changé : les notes en
        // cache sont reposées et requantifiées, sans retranscrire.
        let pulsation = pulsation_de(
            &etat,
            &source,
            &dossier.join(rusty_music_editor::pulsation::FICHIER_CACHE),
            rusty_music_editor::pulsation::Modele::Complet,
        )?;
        let metrique = pulsation.metrique().unwrap_or(4);
        t.mesures = if instrument == "bass" {
            if !doigtes_a_jour {
                // Un cache d'avant le champ : ses hauteurs ont été ramenées
                // sur l'accordage du disque, qui les contenait toutes.
                if t.hauteurs.len() != t.notes.len() {
                    t.hauteurs = t.notes.iter().map(|n| n.hauteur).collect();
                }
                for (n, &h) in t.notes.iter_mut().zip(&t.hauteurs) {
                    n.hauteur = h;
                }
                let accordage = doigter(&mut t.notes, basses, &pulsation.premiers_temps);
                t.accordage = accordage.cordes;
                t.doigtes = VERSION_DOIGTES;
                t.basses = basses;
            }
            let mesures = quantification::quantifier(
                &t.notes,
                &pulsation.temps,
                &pulsation.premiers_temps,
                metrique,
            );
            serde_json::to_value(&mesures).map_err(echec)?
        } else {
            let mesures = quantification::quantifier_coups(
                &t.coups,
                &pulsation.temps,
                &pulsation.premiers_temps,
                metrique,
            );
            serde_json::to_value(&mesures).map_err(echec)?
        };
        t.grille = VERSION_MESURES;
        if let Ok(o) = serde_json::to_vec(&t) {
            if let Err(e) = std::fs::write(&cache, o) {
                tracing::warn!(
                    "transcription : cache non écrit ({}) : {e}",
                    cache.display()
                );
            }
        }
        tracing::info!(
            notes = t.notes.len(),
            coups = t.coups.len(),
            "partition reposée sur la grille"
        );
        return Ok(t);
    }
    let (_, stem) = stems_du_dossier(&dossier)
        .into_iter()
        .find(|(n, _)| *n == instrument)
        .ok_or_else(|| format!("pas de stem « {instrument} » séparé pour ce morceau"))?;
    let pulsation = pulsation_de(
        &etat,
        &source,
        &dossier.join(rusty_music_editor::pulsation::FICHIER_CACHE),
        rusty_music_editor::pulsation::Modele::Complet,
    )?;

    let debut = std::time::Instant::now();
    let s = rusty_music_editor::decode::stereo(&stem).map_err(echec)?;
    if instrument == "drums" {
        let (carac, trames, bandes) = batterie::caracteristiques(&s.gauche, &s.droite);
        drop(s);
        let activations = {
            let mut garde = verrou(&etat.batteur);
            if garde.is_none() {
                *garde = Some(batterie::Batteur::charger_installe().map_err(echec)?);
            }
            garde
                .as_mut()
                .expect("chargé juste au-dessus")
                .activations(&carac, trames, bandes)
                .map_err(echec)?
        };
        let coups = batterie::coups(&activations, trames);
        let mesures = quantification::quantifier_coups(
            &coups,
            &pulsation.temps,
            &pulsation.premiers_temps,
            pulsation.metrique().unwrap_or(4),
        );
        tracing::info!(
            ms = debut.elapsed().as_millis() as u64,
            coups = coups.len(),
            mesures = mesures.len(),
            "transcription batterie"
        );
        let vue = VueTranscription {
            version: VERSION_TRANSCRIPTION,
            instrument,
            modele: "adtof-frame-rnn".into(),
            notes: Vec::new(),
            coups,
            mesures: serde_json::to_value(&mesures).map_err(echec)?,
            accordage: Vec::new(),
            doigtes: 0,
            basses: tablature::Basses::default(),
            hauteurs: Vec::new(),
            grille: VERSION_MESURES,
        };
        if let Ok(o) = serde_json::to_vec(&vue) {
            if let Err(e) = std::fs::write(&cache, o) {
                tracing::warn!(
                    "transcription : cache non écrit ({}) : {e}",
                    cache.display()
                );
            }
        }
        return Ok(vue);
    }
    let mono: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .map(|(g, d)| 0.5 * (g + d))
        .collect();
    drop(s);
    let brutes = if qualite {
        let poids = muscriptor::poids().ok_or_else(|| {
            "poids de MuScriptor absents : accepter leurs conditions sur huggingface.co/MuScriptor/muscriptor-medium, \
             puis les télécharger depuis le choix « Qualité »"
                .to_string()
        })?;
        {
            let mut t = verrou(&etat.transcription);
            (t.en_cours, t.id, t.faits, t.total) = (true, id, 0, 0);
        }
        // Le bilan n'arrive qu'à la fin (une minute) : sans cette ligne, une
        // transcription interrompue ne laissait aucune trace au journal.
        tracing::info!(id, "MuScriptor : transcription de la basse lancée");
        let issue = (|| -> Result<Vec<rusty_music_transcription::Note>, String> {
            let mut garde = verrou(&etat.muscriptor);
            if garde.valeur.is_none() {
                garde.valeur = Some(
                    muscriptor::Muscriptor::charger(&poids, &muscriptor::appareil())
                        .map_err(echec)?,
                );
            }
            let m = garde.valeur.as_ref().expect("chargé juste au-dessus");
            let (notes, bilan) = m
                .transcrire(
                    &mono,
                    rusty_music_editor::SR,
                    muscriptor::BASSES,
                    muscriptor::LOT,
                    |faits, total| {
                        let mut t = verrou(&etat.transcription);
                        (t.faits, t.total) = (faits, total);
                    },
                )
                .map_err(echec)?;
            garde.touche = Some(Instant::now());
            tracing::info!(segments = bilan.segments, jetons = bilan.jetons, boucles = ?bilan.boucles, "MuScriptor");
            Ok(notes)
        })();
        verrou(&etat.transcription).en_cours = false;
        issue?
    } else {
        basic_pitch::POIDS.telecharger(|_, _| {}).map_err(echec)?;
        let mut garde = verrou(&etat.transcripteur);
        if garde.is_none() {
            *garde = Some(basic_pitch::Transcripteur::charger_installe().map_err(echec)?);
        }
        let activations = garde
            .as_mut()
            .expect("chargé juste au-dessus")
            .activations(&mono, rusty_music_editor::SR)
            .map_err(echec)?;
        basic_pitch::notes(&activations, &basic_pitch::Reglages::basse())
    };
    // La porte retire ce que le stem ne joue pas vraiment (fuites d'autres
    // instruments, 60 dB sous la basse) avant de choisir les notes.
    let brutes = porte::filtrer(brutes, &mono, rusty_music_editor::SR, porte::SEUIL_DB);
    let mut notes = monophonie::monophonique(brutes);
    let hauteurs = notes.iter().map(|n| n.hauteur).collect();
    let accordage = doigter(&mut notes, basses, &pulsation.premiers_temps);
    let mesures = quantification::quantifier(
        &notes,
        &pulsation.temps,
        &pulsation.premiers_temps,
        pulsation.metrique().unwrap_or(4),
    );
    tracing::info!(
        ms = debut.elapsed().as_millis() as u64,
        notes = notes.len(),
        mesures = mesures.len(),
        "transcription"
    );
    let vue = VueTranscription {
        version: VERSION_TRANSCRIPTION,
        instrument,
        modele: if qualite {
            "muscriptor-medium"
        } else {
            "basic-pitch"
        }
        .into(),
        notes,
        coups: Vec::new(),
        mesures: serde_json::to_value(&mesures).map_err(echec)?,
        accordage: accordage.cordes,
        doigtes: VERSION_DOIGTES,
        basses,
        hauteurs,
        grille: VERSION_MESURES,
    };
    if let Ok(o) = serde_json::to_vec(&vue) {
        if let Err(e) = std::fs::write(&cache, o) {
            tracing::warn!(
                "transcription : cache non écrit ({}) : {e}",
                cache.display()
            );
        }
    }
    Ok(vue)
}
