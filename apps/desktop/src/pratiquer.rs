// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de Pratiquer : décompte, transcription (basse, batterie,
//! MuScriptor), doigtés, corrections de la transcription et export.

use super::*;
use rusty_music_transcription::corrections;

/// Lance les stems après un décompte de métronome (Pratiquer) : la musique
/// part de `debut`, en secondes du morceau, et `clics` donne chaque clic
/// (instant en secondes du morceau, premier temps ou non). L'interface les
/// place sur la pulsation ; le moteur les joue à l'échantillon près.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
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
#[cfg_attr(test, derive(specta::Type))]
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
    /// (`Accordage::pour`) : celui du morceau, pour lequel la tablature est
    /// écrite. La hauteur des stems ne le change pas (V37) ; l'interface dit
    /// celui à utiliser avec elle.
    #[serde(default)]
    accordage: Vec<u8>,
    /// L'accordage choisi à la main pour ce morceau (V37, `pratique.accordage`),
    /// sur lequel le doigté a été posé ; `None` : déduit des notes.
    #[serde(default)]
    choix: Option<tablature::Accord>,
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
    /// Basse : une identité par note, au même rang — ce qu'une correction
    /// vise et ce qui y est corrigé (E2). Jamais dans le cache : les
    /// corrections vivent dans la mémoire de Pratiquer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identites: Vec<corrections::Identite>,
    /// Les corrections gardées qui n'ont trouvé aucune note.
    #[serde(default)]
    sans_effet: usize,
}

/// Version du cache de transcription : à monter dès que le calcul change.
/// 6 : attaques recalées sur le stem de basse (`recaler_sur_stem`, 24 nov.).
pub(crate) const VERSION_TRANSCRIPTION: u32 = 6;

/// Version du doigté (`tablature::Couts`) : quand seul le doigté change, les
/// notes en cache sont reposées sans retranscrire (MuScriptor prend des
/// minutes). 1 : avant l'a priori appris ; 2 : a priori de position et coûts
/// réglés sur 89 tablatures (17 oct.) ; 3 : table apprise sur 331 tablatures ;
/// 4 : un motif, un doigté (`tablature::uniformiser`, 29 oct.) ; 5 : 4 ou
/// 5 cordes, toujours dans l'accordage du disque ; 6 : accordages de basse
/// seulement, standard d'abord, choix à la main (V37, 12 nov.).
pub(crate) const VERSION_DOIGTES: u32 = 6;

/// Pose corde et frette sur les notes, pour les basses du réglage et
/// l'accordage choisi à la main (`choix`), puis donne un seul doigté à chaque
/// motif répété.
pub(crate) fn doigter(
    notes: &mut [rusty_music_transcription::Note],
    basses: tablature::Basses,
    choix: Option<tablature::Accord>,
    premiers: &[f32],
) -> tablature::Accordage {
    let accordage = tablature::Accordage::pour(notes, basses, choix);
    tablature::poser(notes, &accordage);
    tablature::uniformiser(notes, &accordage, &tablature::Couts::default(), premiers);
    accordage
}

/// Version de la grille des mesures (`Pulsation::regularisee`) : quand seule
/// elle change, les notes et coups en cache sont requantifiés sans
/// retranscrire. 1 : premiers temps parasites retirés (26 oct.) ; 2 :
/// anticipations coupées au premier temps recousues (29 oct.) ; 3 : chaque
/// évènement dit le rang de sa note, pour la corriger (E2, 9 nov.) ; 4 :
/// chaque frappe de batterie dit les rangs de ses coups, pour que le coach
/// colore la note qu'il juge (E4, 10 nov.).
pub(crate) const VERSION_MESURES: u32 = 4;

/// Avancement d'une transcription MuScriptor, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
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
#[cfg_attr(test, specta::specta)]
pub(crate) fn etat_transcription(etat: State<Etat>) -> EtatTranscription {
    verrou(&etat.transcription).clone()
}

/// Les partitions déjà calculées du morceau, sans rien calculer : « bass »,
/// « bass-muscriptor », « drums » selon les caches présents et à jour. Le
/// bloc « Partitions » du rail montre ainsi « prête » plutôt que « Calculer ».
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
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
#[cfg_attr(test, specta::specta)]
pub(crate) fn muscriptor_disponible() -> bool {
    rusty_music_transcription::muscriptor::poids().is_some()
}

/// Vrai si un jeton Hugging Face est déjà posé sur la machine (`HF_TOKEN`
/// ou `hf auth login`) : l'interface ne le demande alors pas.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) fn muscriptor_jeton_present() -> bool {
    rusty_music_transcription::muscriptor::jeton_hugging_face().is_some()
}

/// Télécharge les poids de MuScriptor (1,2 Go) dans le dossier des modèles,
/// avec le jeton saisi, sinon celui de la machine. Le jeton n'est envoyé qu'à
/// Hugging Face et n'est conservé nulle part.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
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
/// quelques secondes la première fois. Les corrections de l'utilisateur
/// (E2) sont réappliquées par-dessus, à chaque fois.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn transcrire(
    etat: State<Etat>,
    id: i64,
    instrument: String,
    qualite: Option<bool>,
    basses: Option<tablature::Basses>,
) -> Result<VueTranscription, String> {
    let vue = transcrire_brut(&etat, id, instrument, qualite, basses)?;
    corriger_vue(&etat, id, vue)
}

/// La transcription telle que le modèle l'a donnée (le cache), sans les
/// corrections.
fn transcrire_brut(
    etat: &State<Etat>,
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
    let choix = accordage_choisi(etat, &chemin);
    let source = PathBuf::from(&chemin);
    let dossier = dossier_stems(etat, &source);
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
        let doigtes_a_jour = instrument != "bass"
            || (t.doigtes == VERSION_DOIGTES && t.basses == basses && t.choix == choix);
        if doigtes_a_jour && t.grille == VERSION_MESURES {
            return Ok(t);
        }
        // Seuls le doigté ou la grille des mesures ont changé : les notes en
        // cache sont reposées et requantifiées, sans retranscrire.
        let pulsation = pulsation_de(
            etat,
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
                let accordage = doigter(&mut t.notes, basses, choix, &pulsation.premiers_temps);
                t.accordage = accordage.cordes;
                t.doigtes = VERSION_DOIGTES;
                t.basses = basses;
                t.choix = choix;
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
        etat,
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
            choix: None,
            doigtes: 0,
            basses: tablature::Basses::default(),
            hauteurs: Vec::new(),
            grille: VERSION_MESURES,
            identites: Vec::new(),
            sans_effet: 0,
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
    recaler_sur_stem(&mut notes, &mono);
    let hauteurs = notes.iter().map(|n| n.hauteur).collect();
    let accordage = doigter(&mut notes, basses, choix, &pulsation.premiers_temps);
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
        choix,
        doigtes: VERSION_DOIGTES,
        basses,
        hauteurs,
        grille: VERSION_MESURES,
        identites: Vec::new(),
        sans_effet: 0,
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

/// Recale les attaques de la transcription sur celles du stem de basse
/// (`coach::basse::recaler`, 24 nov.) : une note que le modèle a datée 60 à
/// 160 ms loin d'une attaque de la même note dans le stem y revient, avant
/// d'être aimantée sur la grille. Sa fin ne bouge pas, sauf pour lui laisser
/// 50 ms.
fn recaler_sur_stem(notes: &mut [rusty_music_transcription::Note], mono: &[f32]) {
    use rusty_music_editor::coach::basse;
    let du_stem = basse::notes_du_stem(mono, rusty_music_editor::SR as f32);
    let cles: Vec<(f64, u8)> = notes
        .iter()
        .map(|n| (n.debut_s as f64, n.hauteur))
        .collect();
    let mut recalees = 0;
    for (n, x) in notes.iter_mut().zip(basse::recaler(&cles, &du_stem)) {
        if let Some(x) = x {
            n.debut_s = x as f32;
            n.fin_s = n.fin_s.max(n.debut_s + 0.05);
            recalees += 1;
        }
    }
    tracing::info!(
        recalees,
        attaques = du_stem.len(),
        "transcription : attaques recalées sur le stem"
    );
}

/// Les corrections de la basse du morceau `id`, réappliquées sur sa
/// transcription brute (E2) : notes corrigées, identités, mesures
/// requantifiées. Sans correction, seulement les identités.
fn corriger_vue(
    etat: &State<Etat>,
    id: i64,
    vue: VueTranscription,
) -> Result<VueTranscription, String> {
    if vue.instrument != "bass" {
        return Ok(vue);
    }
    let chemin = chemin_du_morceau(etat, id)?;
    let liste = verrou(&etat.lib)
        .corrections_pratique(&chemin, "bass")
        .map_err(echec)?;
    let source = PathBuf::from(&chemin);
    let cache = dossier_stems(etat, &source).join(rusty_music_editor::pulsation::FICHIER_CACHE);
    appliquer_corrections(vue, &liste, || {
        let p = pulsation_de(
            etat,
            &source,
            &cache,
            rusty_music_editor::pulsation::Modele::Complet,
        )?;
        let metrique = p.metrique().unwrap_or(4);
        Ok((p.temps, p.premiers_temps, metrique))
    })
}

/// Le cœur de [`corriger_vue`], sans l'application autour : `grille` donne
/// les temps, les premiers temps et la métrique, demandés seulement s'il y a
/// des corrections à poser.
fn appliquer_corrections(
    mut vue: VueTranscription,
    liste: &[rusty_music_core::db::CorrectionPratique],
    grille: impl FnOnce() -> Result<(Vec<f32>, Vec<f32>, u32), String>,
) -> Result<VueTranscription, String> {
    let accordage = tablature::Accordage {
        cordes: vue.accordage.clone(),
        frettes: tablature::Accordage::basse4().frettes,
    };
    let c = corrections::appliquer(&vue.notes, &vue.hauteurs, liste, &accordage);
    vue.identites = c.identites;
    vue.sans_effet = c.sans_effet;
    if liste.is_empty() {
        return Ok(vue);
    }
    vue.notes = c.notes;
    // Les hauteurs transcrites suivent le rang des notes ; une note ajoutée
    // n'en a pas d'autre que la sienne.
    vue.hauteurs = vue.identites.iter().map(|i| i.hauteur).collect();
    let (temps, premiers, metrique) = grille()?;
    let mesures = rusty_music_transcription::quantification::quantifier(
        &vue.notes, &temps, &premiers, metrique,
    );
    vue.mesures = serde_json::to_value(&mesures).map_err(echec)?;
    Ok(vue)
}

/// L'accordage choisi à la main pour le morceau `chemin` (V37), s'il y en a
/// un qu'on sait lire.
fn accordage_choisi(etat: &State<Etat>, chemin: &str) -> Option<tablature::Accord> {
    let texte = verrou(&etat.lib).accordage_pratique(chemin).ok()??;
    serde_json::from_value(serde_json::Value::String(texte)).ok()
}

/// Impose l'accordage de la tablature de basse du morceau `id` (V37), ou
/// rend la main aux notes (`None`) ; gardé par morceau. L'interface redemande
/// ensuite la transcription : le doigté est reposé sans retranscrire.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_accordage(
    etat: State<Etat>,
    id: i64,
    accordage: Option<tablature::Accord>,
) -> Result<(), String> {
    let chemin = chemin_du_morceau(&etat, id)?;
    let texte = accordage
        .map(|a| serde_json::to_value(a).map_err(echec))
        .transpose()?
        .and_then(|v| v.as_str().map(str::to_owned));
    verrou(&etat.lib)
        .garder_accordage_pratique(&chemin, texte.as_deref())
        .map_err(echec)?;
    tracing::info!(id, ?accordage, "accordage de la tablature");
    Ok(())
}

fn chemin_du_morceau(etat: &State<Etat>, id: i64) -> Result<String, String> {
    verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .map(|t| t.path)
        .ok_or_else(|| format!("morceau inconnu : {id}"))
}

/// Corrige une note de la basse transcrite du morceau `id` (E2) : corde
/// imposée, octave, note supprimée, ajoutée, ou rétablie telle que transcrite.
/// Gardée dans la mémoire de Pratiquer ; rend la transcription corrigée.
///
/// Une note ajoutée est posée sur la grille par l'interface : elle est
/// décalée du décalage de jeu du morceau
/// (`quantification::decalage_de_jeu`), comme si elle avait été jouée avec
/// les autres, pour retomber sur sa double croche.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn corriger_transcription(
    etat: State<Etat>,
    id: i64,
    qualite: Option<bool>,
    basses: Option<tablature::Basses>,
    mut correction: rusty_music_core::db::CorrectionPratique,
) -> Result<VueTranscription, String> {
    correction.verifier()?;
    let brute = transcrire_brut(&etat, id, "bass".into(), qualite, basses)?;
    let chemin = chemin_du_morceau(&etat, id)?;
    if correction.genre == "ajouter" {
        let source = PathBuf::from(&chemin);
        let p = pulsation_de(
            &etat,
            &source,
            &dossier_stems(&etat, &source).join(rusty_music_editor::pulsation::FICHIER_CACHE),
            rusty_music_editor::pulsation::Modele::Complet,
        )?;
        let decalage =
            rusty_music_transcription::quantification::decalage_de_jeu(&brute.notes, &p.temps);
        correction.debut_s += decalage as f64;
    }
    verrou(&etat.lib)
        .corriger_pratique(&chemin, "bass", &correction)
        .map_err(echec)?;
    tracing::info!(id, genre = %correction.genre, "transcription corrigée");
    corriger_vue(&etat, id, brute)
}

/// Écrit une partie exportée par la partition (alphaTab : MIDI ou Guitar Pro
/// 7) là où l'utilisateur l'a choisi (dialogue « Enregistrer »).
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn exporter_partie(chemin: String, octets: Vec<u8>) -> Result<(), String> {
    let p = Path::new(&chemin);
    let extension = p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(extension.as_deref(), Some("mid" | "midi" | "gp")) {
        return Err(format!("format d'export inconnu : {chemin}"));
    }
    if octets.is_empty() {
        return Err("partie vide".into());
    }
    std::fs::write(p, &octets).map_err(echec)?;
    tracing::info!(chemin, octets = octets.len(), "partie exportée");
    Ok(())
}

/// Ce que Pratiquer a retenu du morceau `path` (E1), ou `None`.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_memoire(
    etat: State<Etat>,
    path: String,
) -> Result<Option<rusty_music_core::db::MemoirePratique>, String> {
    verrou(&etat.lib).memoire_pratique(&path).map_err(echec)
}

/// Garde ce que Pratiquer a réglé sur le morceau `path` (E1). Créer n'y
/// écrit rien.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn pratique_garder(
    etat: State<Etat>,
    path: String,
    memoire: rusty_music_core::db::MemoirePratique,
) -> Result<(), String> {
    memoire.verifier()?;
    verrou(&etat.lib)
        .garder_pratique(&path, &memoire)
        .map_err(echec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_core::db::CorrectionPratique;
    use rusty_music_transcription::{quantification, Note};

    /// Une basse transcrite comme le fait `transcrire_brut` : notes doigtées,
    /// hauteurs transcrites, mesures quantifiées. `retard` décale les attaques,
    /// comme une transcription refaite par un autre calcul.
    fn transcription_brute(retard: f32, grille: &(Vec<f32>, Vec<f32>, u32)) -> VueTranscription {
        let mut notes: Vec<Note> = [28u8, 31, 33, 35, 36, 35, 33, 31]
            .iter()
            .enumerate()
            .map(|(i, &h)| Note {
                debut_s: 0.5 + 0.5 * i as f32 + retard,
                fin_s: 0.9 + 0.5 * i as f32 + retard,
                hauteur: h,
                amplitude: 0.4,
                corde: None,
                frette: None,
            })
            .collect();
        let hauteurs = notes.iter().map(|n| n.hauteur).collect();
        let accordage = doigter(&mut notes, tablature::Basses::Quatre, None, &grille.1);
        let mesures = quantification::quantifier(&notes, &grille.0, &grille.1, grille.2);
        VueTranscription {
            version: VERSION_TRANSCRIPTION,
            instrument: "bass".into(),
            modele: "basic-pitch".into(),
            notes,
            coups: Vec::new(),
            mesures: serde_json::to_value(&mesures).unwrap(),
            accordage: accordage.cordes,
            choix: None,
            doigtes: VERSION_DOIGTES,
            basses: tablature::Basses::Quatre,
            hauteurs,
            grille: VERSION_MESURES,
            identites: Vec::new(),
            sans_effet: 0,
        }
    }

    /// Le critère d'E2 : les corrections gardées dans la mémoire de Pratiquer
    /// survivent à une transcription refaite (le cache effacé, les attaques
    /// datées autrement) ; la partition requantifiée est la même.
    #[test]
    fn une_correction_survit_a_une_nouvelle_transcription() {
        let lib = rusty_music_core::Library::open_in_memory().unwrap();
        let chemin = "/Musique/a.flac";
        let grille = (
            (0..24).map(|i| 0.5 + 0.5 * i as f32).collect::<Vec<f32>>(),
            vec![0.5, 2.5, 4.5, 6.5, 8.5, 10.5],
            4,
        );
        let brute = transcription_brute(0.0, &grille);
        let n = |i: usize| (brute.notes[i].debut_s as f64, brute.hauteurs[i]);
        let correction = |genre: &str, (debut_s, hauteur): (f64, u8)| CorrectionPratique {
            genre: genre.into(),
            debut_s,
            hauteur,
            duree_s: None,
            corde: None,
            frette: None,
            octaves: None,
            demitons: None,
        };
        for c in [
            CorrectionPratique {
                corde: Some(0),
                ..correction("corde", n(2))
            },
            CorrectionPratique {
                octaves: Some(1),
                ..correction("octave", n(0))
            },
            correction("supprimer", n(5)),
            // V18 : une note transcrite corrigée au demi-ton (si1 → la♯1).
            CorrectionPratique {
                demitons: Some(-1),
                ..correction("demiton", n(3))
            },
            CorrectionPratique {
                // Deux doubles croches, comme les pose l'interface (une
                // durée de silence de la partition).
                duree_s: Some(0.25),
                ..correction("ajouter", (4.75, 38))
            },
        ] {
            c.verifier().unwrap();
            lib.corriger_pratique(chemin, "bass", &c).unwrap();
        }
        let liste = lib.corrections_pratique(chemin, "bass").unwrap();
        let g = || Ok(grille.clone());
        let corrigee = appliquer_corrections(brute, &liste, g).unwrap();
        assert_eq!(corrigee.sans_effet, 0);
        assert_eq!(corrigee.notes.len(), 8);
        // mi1 monté d'une octave, la1 sur la corde de mi (5ᵉ frette).
        assert_eq!(corrigee.notes[0].hauteur, 40);
        assert_eq!(
            (corrigee.notes[2].corde, corrigee.notes[2].frette),
            (Some(0), Some(5))
        );
        assert_eq!(corrigee.notes[3].hauteur, 34);
        assert_eq!(corrigee.identites[3].demitons, -1);
        assert!(corrigee.identites[7].ajoutee);

        // Le cache effacé, la basse retranscrite 15 ms plus tard.
        let refaite =
            appliquer_corrections(transcription_brute(0.015, &grille), &liste, g).unwrap();
        assert_eq!(refaite.sans_effet, 0);
        let jeu = |v: &VueTranscription| -> Vec<(u8, Option<u8>, Option<u8>)> {
            v.notes
                .iter()
                .map(|n| (n.hauteur, n.corde, n.frette))
                .collect()
        };
        assert_eq!(jeu(&corrigee), jeu(&refaite));
        assert_eq!(corrigee.mesures, refaite.mesures);
        // Sans correction, les mesures du cache restent et chaque note a son
        // identité.
        let sans = appliquer_corrections(transcription_brute(0.0, &grille), &[], || {
            Err("pas de grille sans correction".into())
        })
        .unwrap();
        assert_eq!(sans.identites.len(), sans.notes.len());
        assert!(sans.identites.iter().all(|i| !i.corrigee()));
    }
}
