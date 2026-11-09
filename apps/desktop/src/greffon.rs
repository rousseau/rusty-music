// SPDX-License-Identifier: GPL-3.0-or-later
//! Créer : chercher le greffon (plan Éditer 2.2).
//!
//! Quatre temps, et l'interface montre le résultat de chacun :
//!
//! 1. **préfiltre** CLAP — les voisins du morceau entier, guidés si on le
//!    demande par du texte que la tour texte projette dans le même espace
//!    (« plus funk » → « funk drums, syncopated groove ») ;
//! 2. **filtres** — tempo replié à l'octave, distance de tonalité pour les
//!    stems à hauteur, métrique une fois mesurée ;
//! 3. **préparation en tâche de fond** des premiers candidats : démixage
//!    dans le cache de stems existant, pulsation, groove ;
//! 4. **classement** par le groove de leur batterie (densité, syncope,
//!    swing, `rusty_music_analysis::groove`) comparé à celui du morceau
//!    ouvert : « plus funk » = plus syncopé à densité voisine.
//!
//! Aucun modèle nouveau : CLAP (audio et texte) et Beat This! sont déjà là ;
//! COCOLA (2.3) viendra s'ajouter au classement.

use super::*;
use rusty_music_analysis::chemin::Empreinte;

/// Candidats que l'interface fait préparer en tâche de fond (5 à 10 au plan) :
/// trente secondes de démixage et 124 Mo de cache chacun.
pub(crate) const PREPARES: usize = 8;

/// Écart de tonalité toléré pour un stem à hauteur, en quintes : la même, la
/// relative, ou une voisine. Au-delà, il faudrait transposer, et la greffe ne
/// le fait pas encore (`demi_tons_rendu`, plan 2.1).
pub(crate) const TOLERANCE_TONALE: u8 = 1;

/// Le groove d'un morceau démixé, en cache à côté de ses stems.
const FICHIER_GROOVE: &str = "groove.json";
/// À monter dès que la mesure du groove change.
const VERSION_GROOVE: u32 = 1;

/// Ce que l'interface reçoit du groove d'une batterie.
#[cfg_attr(test, derive(specta::Type))]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct GrooveVu {
    version: u32,
    /// Attaques par temps.
    densite: f32,
    /// 0 : tout sur les temps ; 1 : tout sur les doubles croches faibles.
    syncope: f32,
    /// Rapport des deux croches d'un temps (1 droit, 2 ternaire), s'il se lit.
    swing: Option<f32>,
    /// Temps par mesure (Beat This!), s'il se lit.
    metrique: Option<u32>,
}

/// Ce que l'utilisateur a demandé, compris.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Intention {
    /// La phrase donnée à la tour texte de CLAP — en anglais : c'est la
    /// langue qu'elle a apprise.
    texte: String,
    /// +1 : plus syncopé ; −1 : moins ; 0 : comme l'original.
    syncope: i8,
    /// +1 : plus chargé ; −1 : plus clairsemé ; 0 : densité voisine.
    densite: i8,
}

/// Le nom anglais de l'instrument d'un stem, pour la phrase CLAP.
fn instrument(stem: &str) -> &'static str {
    match stem {
        "drums" => "drums",
        "bass" => "bass",
        "vocals" => "vocals",
        "guitar" => "guitar",
        "piano" => "piano",
        _ => "music",
    }
}

/// Les stems qui ont une hauteur, donc une tonalité qui compte.
fn a_hauteur(stem: &str) -> bool {
    stem != "drums"
}

/// Comprend « plus funk », « plus sobre »… Ce qui n'est pas reconnu va tel
/// quel à CLAP, sans objectif de groove : le texte seul oriente.
pub(crate) fn interpreter(guide: &str, stem: &str) -> Intention {
    let g = guide.to_lowercase();
    let inst = instrument(stem);
    let a = |mots: &[&str]| mots.iter().any(|m| g.contains(m));
    let mut phrases = Vec::new();
    let (mut syncope, mut densite) = (0i8, 0i8);
    if a(&["funk", "syncop", "groove"]) {
        phrases.push(format!("funk {inst}, syncopated groove"));
        syncope = 1;
    } else if a(&[
        "droit",
        "carré",
        "carre",
        "régulier",
        "regulier",
        "straight",
    ]) {
        phrases.push(format!("straight steady {inst}"));
        syncope = -1;
    }
    if a(&["chargé", "charge", "dense", "rempli", "busy"]) {
        phrases.push(format!("busy {inst}, lots of fills"));
        densite = 1;
    } else if a(&[
        "sobre",
        "simple",
        "épuré",
        "epure",
        "clairsemé",
        "minimal",
        "sparse",
    ]) {
        phrases.push(format!("sparse minimal {inst}"));
        densite = -1;
    }
    let texte = if phrases.is_empty() {
        guide.trim().to_string()
    } else {
        phrases.join(", ")
    };
    Intention {
        texte,
        syncope,
        densite,
    }
}

/// Un morceau d'où tirer le stem, et pourquoi il est classé là.
#[cfg_attr(test, derive(specta::Type))]
#[derive(serde::Serialize)]
pub(crate) struct CandidatGreffon {
    id: i64,
    /// Le fichier, pour pouvoir le séparer s'il ne l'est pas encore.
    path: String,
    title: Option<String>,
    artist: Option<String>,
    bpm: f32,
    /// Facteur de durée qui l'amènerait au tempo du morceau ouvert.
    facteur: f32,
    /// Repliements d'octave : +1 = joué à double tempo, −1 = à demi-tempo.
    octaves: i32,
    tonalite: Option<String>,
    /// Ses stems sont-ils déjà séparés ?
    separe: bool,
    /// Le groove de sa batterie, une fois préparé. `None` : à préparer.
    groove: Option<GrooveVu>,
    /// Rang parmi les voisins sonores (guidés par le texte s'il y en a), 1 =
    /// le plus proche.
    rang_sonore: usize,
    score: f32,
    /// Pourquoi il est là, en clair : groove, tempo, tonalité, voisinage.
    raisons: Vec<String>,
}

/// Ce que la recherche a trouvé, **et ce qu'elle a écarté** — sans les
/// comptes, une liste courte passe pour une bibliothèque pauvre.
#[cfg_attr(test, derive(specta::Type))]
#[derive(serde::Serialize)]
pub(crate) struct RechercheGreffon {
    /// Tempo du morceau ouvert. Sans lui, rien n'est calable.
    bpm: Option<f32>,
    tonalite: Option<String>,
    /// Le stem remplacé a-t-il une hauteur ? Sinon la tonalité est ignorée.
    a_hauteur: bool,
    /// La phrase envoyée à CLAP, si la recherche est guidée.
    texte: Option<String>,
    /// Le groove du morceau ouvert, la référence du classement.
    groove: Option<GrooveVu>,
    candidats: Vec<CandidatGreffon>,
    ecartes_tempo: usize,
    ecartes_tonalite: usize,
    ecartes_metrique: usize,
    sans_tempo: usize,
}

/// Similarité cosinus.
fn cosinus(a: &[f32], b: &[f32]) -> f32 {
    let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    let n = (aa * bb).sqrt();
    if n <= f32::EPSILON {
        0.0
    } else {
        ab / n
    }
}

/// Les `k` morceaux les plus proches du morceau `id` — et, si `texte` est
/// donné, de la phrase aussi, à parts égales : le morceau garde le style,
/// le texte donne la direction.
pub(crate) fn prefiltre(
    vecteurs: &[Empreinte],
    id: i64,
    texte: Option<&[f32]>,
    k: usize,
) -> Vec<i64> {
    let Some(source) = vecteurs.iter().find(|(i, _)| *i == id).map(|(_, v)| v) else {
        return Vec::new();
    };
    let mut notes: Vec<(i64, f32)> = vecteurs
        .iter()
        .filter(|(i, _)| *i != id)
        .map(|(i, v)| {
            let s = cosinus(v, source);
            (*i, texte.map_or(s, |t| 0.5 * s + 0.5 * cosinus(v, t)))
        })
        .collect();
    let k = k.min(notes.len());
    if k == 0 {
        return Vec::new();
    }
    notes.select_nth_unstable_by(k - 1, |a, b| b.1.total_cmp(&a.1));
    notes.truncate(k);
    notes.sort_by(|a, b| b.1.total_cmp(&a.1));
    notes.into_iter().map(|(i, _)| i).collect()
}

/// Un nombre à la française.
fn fr(x: f32, decimales: usize) -> String {
    format!("{x:.decimales$}").replace('.', ",")
}

/// La note de groove d'un candidat face à l'original, et ce qu'elle dit.
///
/// Sans intention, on cherche le groove **le plus proche** ; avec, on
/// récompense l'écart dans le sens demandé. Une différence de syncope de
/// 0,1 pèse 0,5, autant que la moitié de l'écart entre le premier et le
/// dernier voisin sonore ; un rapport de densité de 2 pèse 0,7.
pub(crate) fn noter_groove(
    g: &GrooveVu,
    source: Option<&GrooveVu>,
    intention: Option<&Intention>,
) -> (f32, Vec<String>) {
    let (sens_s, sens_d) = intention.map_or((0, 0), |i| (i.syncope, i.densite));
    let mut raisons = Vec::new();
    let mut note = 0.0;
    match source {
        Some(o) => {
            let d = g.syncope - o.syncope;
            note += if sens_s == 0 {
                -d.abs() * 2.5
            } else {
                f32::from(sens_s) * d * 5.0
            };
            let qui = if d.abs() < 0.03 {
                "aussi syncopé"
            } else if d > 0.0 {
                "plus syncopé"
            } else {
                "moins syncopé"
            };
            raisons.push(format!(
                "syncope {} : {qui} que l'original ({})",
                fr(g.syncope, 2),
                fr(o.syncope, 2)
            ));
            let r = (g.densite.max(0.01) / o.densite.max(0.01)).ln();
            note += if sens_d == 0 {
                -r.abs()
            } else {
                f32::from(sens_d) * r
            };
            let qui = if r.abs() < 0.15 {
                "densité voisine"
            } else if r > 0.0 {
                "plus chargé"
            } else {
                "plus clairsemé"
            };
            raisons.push(format!(
                "{} attaques par temps : {qui} ({})",
                fr(g.densite, 1),
                fr(o.densite, 1)
            ));
        }
        None => {
            // Pas de batterie à l'original : on classe dans l'absolu.
            note += f32::from(sens_s) * (g.syncope - 0.3) * 5.0;
            raisons.push(format!("syncope {}", fr(g.syncope, 2)));
            raisons.push(format!("{} attaques par temps", fr(g.densite, 1)));
        }
    }
    match g.swing {
        Some(s) if s > 1.25 => raisons.push(format!("swingué ({})", fr(s, 1))),
        Some(_) => raisons.push("droit".into()),
        None => {}
    }
    (note, raisons)
}

/// Le tempo dit comme la greffe le fera.
fn raison_tempo(bpm: f32, facteur: f32, octaves: i32) -> String {
    let pct = (facteur - 1.0) * 100.0;
    let mut s = format!("{} BPM", bpm.round());
    if octaves > 0 {
        s += &format!(", joué à {} × son tempo", 1 << octaves);
    } else if octaves < 0 {
        s += &format!(", joué à son tempo divisé par {}", 1 << -octaves);
    }
    if pct.abs() < 0.05 {
        s += ", au même tempo";
    } else {
        s += &format!(
            ", étiré de {}{} %",
            if pct > 0.0 { "+" } else { "" },
            fr(pct, 1)
        );
    }
    s
}

/// La tonalité dite par rapport à l'original.
fn raison_tonalite(tonalite: Option<&str>, source: Option<&str>) -> String {
    match (tonalite, source) {
        (Some(t), Some(o)) => match rusty_music_analysis::descripteurs::distance_tonale(t, o) {
            Some(0) if t == o => format!("même tonalité ({t})"),
            Some(0) => format!("tonalité relative ({t})"),
            Some(1) => format!("tonalité voisine ({t}), à une quinte"),
            Some(n) => format!("{t}, à {n} quintes"),
            None => format!("tonalité {t}"),
        },
        (Some(t), None) => format!("tonalité {t}, l'original n'en a pas"),
        (None, _) => "tonalité inconnue".into(),
    }
}

/// Lit le groove en cache d'un morceau, s'il est à jour.
fn lire_groove(dossier: &Path) -> Option<GrooveVu> {
    let texte = std::fs::read_to_string(dossier.join(FICHIER_GROOVE)).ok()?;
    serde_json::from_str::<GrooveVu>(&texte)
        .ok()
        .filter(|g| g.version == VERSION_GROOVE)
}

/// Le groove d'un morceau démixé : du cache, ou mesuré sur son stem `drums`
/// et sa pulsation (Beat This!, elle aussi en cache), puis mis en cache.
/// `None` sans stem `drums` ou sans temps.
pub(crate) fn groove_de(etat: &Etat, chemin: &str) -> Option<GrooveVu> {
    let source = Path::new(chemin);
    let dossier = dossier_stems(etat, source);
    if let Some(g) = lire_groove(&dossier) {
        return Some(g);
    }
    let (_, drums) = stems_du_dossier(&dossier)
        .into_iter()
        .find(|(n, _)| n == "drums")?;
    let p = pulsation_de(
        etat,
        source,
        &dossier.join(rusty_music_editor::pulsation::FICHIER_CACHE),
        rusty_music_editor::pulsation::Modele::Complet,
    )
    .map_err(|e| tracing::warn!("groove : pas de pulsation : {e}"))
    .ok()?;
    let s = rusty_music_editor::decode::stereo(&drums).ok()?;
    let mono: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .map(|(g, d)| (g + d) * 0.5)
        .collect();
    let analyseur = rusty_music_analysis::descripteurs::Analyseur::new();
    let g =
        rusty_music_analysis::groove::groove(&mono, rusty_music_editor::SR, &analyseur, &p.temps)?;
    let vu = GrooveVu {
        version: VERSION_GROOVE,
        densite: g.densite,
        syncope: g.syncope,
        swing: g.swing,
        metrique: p.metrique(),
    };
    if let Ok(json) = serde_json::to_string(&vu) {
        let _ = std::fs::write(dossier.join(FICHIER_GROOVE), json);
    }
    Some(vu)
}

/// Cherche d'où tirer un stem pour remplacer `stem` dans le morceau `id`.
///
/// `guide` : « plus funk », « plus sobre »… ou une phrase anglaise pour CLAP.
/// Rapide (une demi-seconde, l'encodeur texte compris) : seul le groove du
/// morceau ouvert peut se mesurer ici, une seconde environ la première fois ;
/// celui des candidats vient du cache, que [`greffons_preparer`] remplit.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn chercher_greffon(
    etat: State<Etat>,
    id: i64,
    stem: String,
    guide: Option<String>,
    count: usize,
) -> Result<RechercheGreffon, String> {
    let intention = guide
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(|g| interpreter(g, &stem));
    let cible = match &intention {
        Some(i) => {
            let mut garde = verrou(&etat.texte_modele);
            if garde.valeur.is_none() {
                garde.valeur =
                    Some(rusty_music_analysis::EmbedderTexte::charger(None).map_err(echec)?);
            }
            let v = garde
                .valeur
                .as_ref()
                .expect("encodeur texte chargé")
                .embed(&i.texte)
                .map_err(echec)?;
            garde.touche = Some(Instant::now());
            Some(v)
        }
        None => None,
    };

    let vecteurs = charger_vecteurs(&etat)?;
    let proches = prefiltre(&vecteurs, id, cible.as_deref(), VOISINS_EXAMINES);
    let pistes = pistes_de(&etat, &proches)?;
    let (source, bpm, tonalite, tempos, tonalites) = {
        let lib = verrou(&etat.lib);
        let source = lib
            .track(id)
            .map_err(echec)?
            .ok_or_else(|| "morceau ouvert introuvable en base".to_string())?;
        let bpm = lib.tempos(&[id]).map_err(echec)?.get(&id).copied();
        let tonalite = lib.tonalites(&[id]).map_err(echec)?.remove(&id);
        (
            source,
            bpm,
            tonalite,
            lib.tempos(&proches).map_err(echec)?,
            lib.tonalites(&proches).map_err(echec)?,
        )
    };
    let hauteur = a_hauteur(&stem);
    let groove_source = groove_de(&etat, &source.path);
    let metrique_source = groove_source.as_ref().and_then(|g| g.metrique);

    let mut sortie = RechercheGreffon {
        bpm,
        tonalite: tonalite.clone(),
        a_hauteur: hauteur,
        texte: intention.as_ref().map(|i| i.texte.clone()),
        groove: groove_source.clone(),
        candidats: Vec::new(),
        ecartes_tempo: 0,
        ecartes_tonalite: 0,
        ecartes_metrique: 0,
        sans_tempo: 0,
    };
    let Some(bpm_source) = bpm else {
        sortie.sans_tempo = pistes.len();
        return Ok(sortie);
    };

    // Deux fois plus que demandé : la métrique, connue une fois le candidat
    // préparé, en écartera peut-être.
    let n = pistes.len().max(1);
    for (rang, piste) in pistes.into_iter().enumerate() {
        if sortie.candidats.len() >= count * 2 {
            break;
        }
        let Some(bpm) = tempos.get(&piste.id).copied() else {
            sortie.sans_tempo += 1;
            continue;
        };
        let (facteur, octaves) = rusty_music_editor::greffe::tempo_replie(bpm_source, bpm);
        if (facteur - 1.0).abs() > TOLERANCE_TEMPO {
            sortie.ecartes_tempo += 1;
            continue;
        }
        let ton = tonalites.get(&piste.id).cloned();
        if hauteur {
            let d = ton
                .as_deref()
                .zip(tonalite.as_deref())
                .and_then(|(a, b)| rusty_music_analysis::descripteurs::distance_tonale(a, b));
            if d.is_some_and(|d| d > TOLERANCE_TONALE) {
                sortie.ecartes_tonalite += 1;
                continue;
            }
        }
        let dossier = dossier_stems(&etat, Path::new(&piste.path));
        let groove = lire_groove(&dossier);
        if let (Some(a), Some(b)) = (groove.as_ref().and_then(|g| g.metrique), metrique_source) {
            if a != b {
                sortie.ecartes_metrique += 1;
                continue;
            }
        }

        let sonore = 1.0 - rang as f32 / n as f32;
        let mut raisons = Vec::new();
        let mut score = sonore;
        match &groove {
            Some(g) => {
                let (note, r) = noter_groove(g, groove_source.as_ref(), intention.as_ref());
                score += note;
                raisons.extend(r);
            }
            None => raisons.push("groove à mesurer : démixage en attente".into()),
        }
        raisons.push(raison_tempo(bpm, facteur, octaves));
        if hauteur {
            raisons.push(raison_tonalite(ton.as_deref(), tonalite.as_deref()));
        }
        raisons.push(match &intention {
            Some(i) => format!("n° {} pour « {} » et le son du morceau", rang + 1, i.texte),
            None => format!("n° {} des voisins sonores", rang + 1),
        });
        sortie.candidats.push(CandidatGreffon {
            id: piste.id,
            path: piste.path,
            title: piste.title,
            artist: piste.artist,
            bpm,
            facteur,
            octaves,
            tonalite: ton,
            separe: !stems_du_dossier(&dossier).is_empty(),
            groove,
            rang_sonore: rang + 1,
            score,
            raisons,
        });
    }

    // Les candidats mesurés d'abord, par note ; ceux qui attendent leur
    // démixage ensuite, dans l'ordre sonore — le classement s'affine à mesure
    // que la préparation avance.
    sortie.candidats.sort_by(|a, b| {
        b.groove
            .is_some()
            .cmp(&a.groove.is_some())
            .then(b.score.total_cmp(&a.score))
    });
    sortie.candidats.truncate(count);
    tracing::info!(
        retenus = sortie.candidats.len(),
        mesures = sortie.candidats.iter().filter(|c| c.groove.is_some()).count(),
        tempo = sortie.ecartes_tempo,
        tonalite = sortie.ecartes_tonalite,
        metrique = sortie.ecartes_metrique,
        guide = ?sortie.texte,
        "recherche du greffon"
    );
    Ok(sortie)
}

/// Où en est la préparation des greffons.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementGreffons {
    /// Ce qui reste à préparer, dans l'ordre.
    file: Vec<i64>,
    courant: Option<i64>,
    prets: Vec<i64>,
    echecs: Vec<i64>,
}
pub(crate) type EtatGreffons = Passe<AvancementGreffons>;

/// Ce que l'interface sonde de la préparation.
#[cfg_attr(test, derive(specta::Type))]
#[derive(serde::Serialize)]
pub(crate) struct VueGreffons {
    en_cours: bool,
    /// Le morceau en cours de démixage ou de mesure.
    courant: Option<i64>,
    /// Préparés depuis le lancement : stems séparés et groove mesuré.
    prets: Vec<i64>,
    /// Ceux qui n'ont pas pu l'être (fichier illisible, pas de batterie…).
    echecs: Vec<i64>,
    restants: usize,
}

/// Démixe et mesure, en tâche de fond, les candidats `ids`, dans cet ordre.
///
/// **Si la préparation tourne déjà, la file est remplacée** : c'est ainsi
/// qu'un candidat cliqué passe devant les autres. Les stems vont dans le cache
/// existant, comme un démixage lancé à la main.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn greffons_preparer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    mut ids: Vec<i64>,
    variant: String,
) -> Result<(), String> {
    ids.truncate(PREPARES);
    let variante = rusty_music_editor::Variante::analyser(&variant)
        .ok_or_else(|| format!("variante inconnue : {variant}"))?;
    {
        let mut g = verrou(&etat.greffons);
        if g.en_cours {
            g.avancement.file = ids;
            return Ok(());
        }
        *g = Passe {
            en_cours: true,
            avancement: AvancementGreffons {
                file: ids,
                ..Default::default()
            },
            resultat: None,
        };
    }
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.greffons);
        let mut demixeur = None;
        let mut faits = 0usize;
        loop {
            // Prendre le suivant et, s'il n'y en a plus, clore — sous le même
            // verrou, pour qu'une file remplacée entre-temps ne se perde pas.
            let id = {
                let mut g = verrou(&etat.greffons);
                if g.avancement.file.is_empty() {
                    g.en_cours = false;
                    g.avancement.courant = None;
                    g.resultat = Some(format!("{faits} greffons préparés"));
                    return;
                }
                let id = g.avancement.file.remove(0);
                g.avancement.courant = Some(id);
                id
            };
            let issue = preparer_un(&etat, id, variante, &mut demixeur);
            let mut g = verrou(&etat.greffons);
            match issue {
                Ok(()) => {
                    faits += 1;
                    g.avancement.prets.push(id);
                }
                Err(e) => {
                    tracing::warn!(id, "greffon non préparé : {e}");
                    g.avancement.echecs.push(id);
                }
            }
        }
    });
    Ok(())
}

/// Un greffon : séparé s'il ne l'est pas, puis son groove mesuré.
fn preparer_un(
    etat: &Etat,
    id: i64,
    variante: rusty_music_editor::Variante,
    demixeur: &mut Option<rusty_music_editor::Demixeur>,
) -> Result<(), String> {
    let chemin = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .ok_or_else(|| format!("morceau inconnu : {id}"))?
        .path;
    let source = PathBuf::from(&chemin);
    let dossier = dossier_stems(etat, &source);
    if stems_du_dossier(&dossier).is_empty() {
        // Pas deux séparations à la fois : celle que l'utilisateur a lancée
        // passe d'abord.
        while verrou(&etat.demix).en_cours {
            std::thread::sleep(Duration::from_millis(500));
        }
        if demixeur.is_none() {
            rusty_music_editor::Demixeur::assurer_poids(variante, |_, _| {}).map_err(echec)?;
            let d = rusty_music_editor::Demixeur::charger(None, variante).map_err(echec)?;
            d.chauffer();
            *demixeur = Some(d);
        }
        let debut = Instant::now();
        let fichiers = demixeur
            .as_ref()
            .expect("chargé juste au-dessus")
            .separer_fichier_suivi(&source, &dossier, |_| {})
            .map_err(echec)?;
        oublier_derives_des_stems(&dossier, &fichiers);
        tracing::info!(id, s = debut.elapsed().as_secs(), "greffon séparé");
    }
    groove_de(etat, &chemin)
        .map(|_| ())
        .ok_or_else(|| "pas de groove mesurable (pas de stem drums, ou pas de temps)".into())
}

/// Avancement de la préparation. L'interface sonde.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn greffons_etat(etat: State<Etat>) -> VueGreffons {
    let g = verrou(&etat.greffons);
    VueGreffons {
        en_cours: g.en_cours,
        courant: g.avancement.courant,
        prets: g.avancement.prets.clone(),
        echecs: g.avancement.echecs.clone(),
        restants: g.avancement.file.len(),
    }
}

/// `RUSTY_MUSIC_ESSAI_GREFFON=<id>` : cherche au démarrage un greffon pour la
/// batterie du morceau `id` (guide : `RUSTY_MUSIC_ESSAI_GUIDE`, « plus funk »
/// par défaut), **sans le son**, fait préparer les 8 premiers comme
/// l'interface, puis relit le classement et quitte. Le détail, durées
/// comprises, va dans `essai-greffon.json` à côté de la base. À lancer sur une
/// copie des données (`RUSTY_MUSIC_DONNEES`) : les démixages vont dans son
/// cache de stems. Sans effet en usage normal.
pub(crate) fn essai_greffon(app: &tauri::AppHandle) {
    let Some(id) = std::env::var("RUSTY_MUSIC_ESSAI_GREFFON")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
    else {
        return;
    };
    let guide = std::env::var("RUSTY_MUSIC_ESSAI_GUIDE").unwrap_or_else(|_| "plus funk".into());
    let app = app.clone();
    std::thread::spawn(move || {
        // Laisse la reprise de session passer avant.
        std::thread::sleep(Duration::from_secs(5));
        let etat = app.state::<Etat>();
        verrou(&etat.player).set_volume(0.0);
        let mut sortie = serde_json::Map::new();
        let mut noter = |cle: &str, v: serde_json::Value| {
            sortie.insert(cle.into(), v);
        };
        let ms = |t: Instant| serde_json::json!(t.elapsed().as_millis() as u64);

        // Les temps de `chercher_greffon`, séparés : la tour texte (chargée
        // ici, gardée pour l'appel qui suit), puis les empreintes et le
        // préfiltre.
        let intention = interpreter(&guide, "drums");
        let t = Instant::now();
        let cible = match rusty_music_analysis::EmbedderTexte::charger(None) {
            Ok(e) => {
                let chargee = t.elapsed().as_millis() as u64;
                let t2 = Instant::now();
                let v = e.embed(&intention.texte).ok();
                noter("texte_chargement_ms", serde_json::json!(chargee));
                noter("texte_encodage_ms", ms(t2));
                let mut g = verrou(&etat.texte_modele);
                g.valeur = Some(e);
                g.touche = Some(Instant::now());
                v
            }
            Err(e) => {
                tracing::warn!(erreur = %e, "essai de greffon : tour texte absente");
                None
            }
        };
        let t = Instant::now();
        match charger_vecteurs(&etat) {
            Ok(v) => {
                noter("empreintes", serde_json::json!(v.len()));
                noter("empreintes_ms", ms(t));
                let t = Instant::now();
                let p = prefiltre(&v, id, cible.as_deref(), VOISINS_EXAMINES);
                noter("prefiltre_ms", ms(t));
                noter("prefiltre_n", serde_json::json!(p.len()));
            }
            Err(e) => tracing::warn!(erreur = %e, "essai de greffon : empreintes"),
        }

        let chercher = |cle: &str, sortie: &mut serde_json::Map<String, serde_json::Value>| {
            let t = Instant::now();
            match chercher_greffon(
                app.state::<Etat>(),
                id,
                "drums".into(),
                Some(guide.clone()),
                12,
            ) {
                Ok(r) => {
                    sortie.insert(format!("{cle}_ms"), ms(t));
                    let v = serde_json::to_value(&r).unwrap_or_default();
                    sortie.insert(cle.into(), v);
                    Some(r)
                }
                Err(e) => {
                    tracing::warn!(erreur = %e, "essai de greffon : recherche");
                    None
                }
            }
        };
        let Some(premiere) = chercher("premiere", &mut sortie) else {
            app.exit(1);
            return;
        };
        let ids: Vec<i64> = premiere
            .candidats
            .iter()
            .filter(|c| c.groove.is_none())
            .map(|c| c.id)
            .take(PREPARES)
            .collect();
        sortie.insert("prepares".into(), serde_json::json!(ids));
        let t = Instant::now();
        if !ids.is_empty() {
            if let Err(e) = greffons_preparer(app.clone(), etat, ids, "htdemucs".into()) {
                tracing::warn!(erreur = %e, "essai de greffon : préparation");
            }
            std::thread::sleep(Duration::from_secs(1));
            while verrou(&app.state::<Etat>().greffons).en_cours {
                std::thread::sleep(Duration::from_secs(2));
            }
        }
        sortie.insert("preparation_ms".into(), ms(t));
        let vue = greffons_etat(app.state::<Etat>());
        sortie.insert(
            "preparation".into(),
            serde_json::to_value(&vue).unwrap_or_default(),
        );
        chercher("finale", &mut sortie);

        let fichier = app
            .state::<Etat>()
            .db
            .parent()
            .unwrap_or(Path::new("."))
            .join("essai-greffon.json");
        match serde_json::to_string_pretty(&sortie) {
            Ok(j) => {
                let _ = std::fs::write(&fichier, j);
                tracing::info!(fichier = %fichier.display(), "essai de greffon écrit");
            }
            Err(e) => tracing::warn!(erreur = %e, "essai de greffon : écriture"),
        }
        app.exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groove(densite: f32, syncope: f32) -> GrooveVu {
        GrooveVu {
            version: VERSION_GROOVE,
            densite,
            syncope,
            swing: None,
            metrique: Some(4),
        }
    }

    #[test]
    fn plus_funk_veut_dire_plus_syncope_a_densite_voisine() {
        let i = interpreter("une batterie plus funk", "drums");
        assert_eq!((i.syncope, i.densite), (1, 0));
        assert_eq!(i.texte, "funk drums, syncopated groove");
        let i = interpreter("plus sobre", "bass");
        assert_eq!((i.syncope, i.densite), (0, -1));
        assert_eq!(i.texte, "sparse minimal bass");
        // Ce qui n'est pas reconnu va tel quel à CLAP.
        let i = interpreter("  latin percussion ", "drums");
        assert_eq!(
            (i.syncope, i.densite, i.texte.as_str()),
            (0, 0, "latin percussion")
        );
    }

    #[test]
    fn le_classement_suit_l_intention() {
        let source = groove(2.0, 0.20);
        let funk = interpreter("plus funk", "drums");
        let syncope = groove(2.1, 0.40);
        let carre = groove(2.0, 0.10);
        let charge = groove(5.0, 0.40);
        let (a, raisons) = noter_groove(&syncope, Some(&source), Some(&funk));
        let (b, _) = noter_groove(&carre, Some(&source), Some(&funk));
        let (c, _) = noter_groove(&charge, Some(&source), Some(&funk));
        assert!(a > b, "le plus syncopé passe devant : {a} contre {b}");
        assert!(a > c, "à densité voisine : {a} contre {c}");
        assert_eq!(
            raisons[0],
            "syncope 0,40 : plus syncopé que l'original (0,20)"
        );
        assert_eq!(raisons[1], "2,1 attaques par temps : densité voisine (2,0)");
        // Sans intention, le plus proche de l'original gagne.
        let (a, _) = noter_groove(&syncope, Some(&source), None);
        let (b, _) = noter_groove(&carre, Some(&source), None);
        assert!(b > a);
    }

    #[test]
    fn tempo_et_tonalite_se_disent() {
        assert_eq!(raison_tempo(112.0, 1.021, 0), "112 BPM, étiré de +2,1 %");
        assert_eq!(
            raison_tempo(60.0, 1.0, -1),
            "60 BPM, joué à son tempo divisé par 2, au même tempo"
        );
        assert_eq!(
            raison_tonalite(Some("A min"), Some("C maj")),
            "tonalité relative (A min)"
        );
        assert_eq!(
            raison_tonalite(Some("E min"), Some("C maj")),
            "tonalité voisine (E min), à une quinte"
        );
        assert_eq!(raison_tonalite(None, Some("C maj")), "tonalité inconnue");
    }

    #[test]
    fn le_texte_oriente_le_prefiltre() {
        let v = |x: f32, y: f32| vec![x, y];
        let vecteurs: Vec<Empreinte> = vec![
            (1, v(1.0, 0.0)),
            (2, v(0.9, 0.1)),
            (3, v(0.7, 0.7)),
            (4, v(0.0, 1.0)),
        ];
        assert_eq!(prefiltre(&vecteurs, 1, None, 2), vec![2, 3]);
        // Guidé vers (0, 1) : le 3, à mi-chemin, passe devant le 2.
        assert_eq!(prefiltre(&vecteurs, 1, Some(&[0.0, 1.0]), 2), vec![3, 2]);
        assert!(prefiltre(&vecteurs, 99, None, 2).is_empty());
    }
}
