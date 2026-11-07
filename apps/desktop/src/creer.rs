// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de Créer : greffer le stem d'un autre morceau (candidats,
//! tempo cible, greffe).

use super::*;

/// Écart de tempo au-delà duquel un morceau n'est pas proposé pour une greffe.
///
/// **Dix pour cent, et le repliement est déjà passé** : un morceau à demi-tempo
/// n'est pas écarté, il est compté pour ce qu'il est. Ce qui reste est le vrai
/// étirement à appliquer, et `wsola` y est transparent. Plus loin, deux choses
/// se dégradent ensemble — la matière étirée, et la confiance qu'on peut avoir
/// dans un tempo mesuré à 6 % près (`docs/suite.md`, 6 bis).
pub(crate) const TOLERANCE_TEMPO: f32 = 0.10;

/// Combien de voisins soniques on examine avant de filtrer sur le tempo.
///
/// Le filtre en écarte beaucoup : sur une bibliothèque réelle, quelques
/// dizaines de voisins ne suffisent pas à en garder une poignée.
pub(crate) const VOISINS_EXAMINES: usize = 400;

/// Un morceau dont on pourrait prendre un stem.
#[derive(serde::Serialize)]
pub(crate) struct Candidat {
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
    /// Ce que l'étirement lui fera subir, en pourcentage.
    ecart: f32,
    /// Ses stems sont-ils déjà séparés ? Sinon il faut compter trente secondes.
    separe: bool,
}

/// Ce que la recherche a trouvé, **et ce qu'elle a écarté**.
///
/// Les deux comptes ne sont pas décoratifs : sans eux, une liste courte passe
/// pour une bibliothèque pauvre en voisins alors que c'est le tempo, ou son
/// absence, qui a trié.
#[derive(serde::Serialize)]
pub(crate) struct VoisinsStem {
    /// Tempo du morceau ouvert. Sans lui, rien n'est calable.
    bpm: Option<f32>,
    candidats: Vec<Candidat>,
    /// Voisins écartés parce que leur tempo est trop loin.
    ecartes: usize,
    /// Voisins écartés faute de tempo mesuré.
    sans_tempo: usize,
}

/// Les morceaux d'où l'on pourrait tirer un stem à greffer.
///
/// **Le voisinage se calcule sur le morceau entier, pas sur le stem.** C'est
/// une limite assumée : la bibliothèque ne contient que des empreintes de
/// mélanges complets, et comparer une batterie seule à des mélanges
/// compare deux choses différentes. Embarquer une empreinte par stem
/// supposerait de démixer les 27 000 morceaux — trente secondes chacun.
///
/// Le tempo, lui, est une contrainte dure et non un classement : une batterie
/// qui ne tombe pas juste ne sert à rien, si proche soit-elle.
#[tauri::command(async)]
pub(crate) fn voisins_de_stem(
    etat: State<Etat>,
    id: i64,
    count: usize,
) -> Result<VoisinsStem, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let proches = rusty_music_analysis::chemin::voisins(&vecteurs, id, VOISINS_EXAMINES);
    let pistes = pistes_de(&etat, &proches)?;

    let (bpm, tempos) = {
        let lib = verrou(&etat.lib);
        let bpm = lib.tempos(&[id]).map_err(echec)?.get(&id).copied();
        (bpm, lib.tempos(&proches).map_err(echec)?)
    };

    let Some(bpm_source) = bpm else {
        // Rien à caler sans tempo : on le dit plutôt que de proposer une liste
        // dont aucune entrée ne tiendrait.
        return Ok(VoisinsStem {
            bpm: None,
            candidats: Vec::new(),
            ecartes: 0,
            sans_tempo: pistes.len(),
        });
    };

    let mut candidats = Vec::new();
    let (mut ecartes, mut sans_tempo) = (0usize, 0usize);
    for piste in pistes {
        let Some(bpm) = tempos.get(&piste.id).copied() else {
            sans_tempo += 1;
            continue;
        };
        let (facteur, octaves) = rusty_music_editor::greffe::tempo_replie(bpm_source, bpm);
        let ecart = (facteur - 1.0).abs();
        if ecart > TOLERANCE_TEMPO {
            ecartes += 1;
            continue;
        }
        if candidats.len() < count {
            let dossier = dossier_stems(&etat, Path::new(&piste.path));
            candidats.push(Candidat {
                id: piste.id,
                path: piste.path,
                title: piste.title,
                artist: piste.artist,
                bpm,
                facteur,
                octaves,
                ecart: (facteur - 1.0) * 100.0,
                separe: !stems_du_dossier(&dossier).is_empty(),
            });
        }
    }

    tracing::info!(
        retenus = candidats.len(),
        ecartes,
        sans_tempo,
        "voisins pour une greffe"
    );
    Ok(VoisinsStem {
        bpm: Some(bpm_source),
        candidats,
        ecartes,
        sans_tempo,
    })
}

/// Ce qu'il faut pour proposer un **BPM cible** dans la barre d'outils plutôt
/// qu'un pourcentage de vitesse.
#[derive(serde::Serialize)]
pub(crate) struct TempoCible {
    /// Tempo du morceau entier, tel que la passe de descripteurs l'a mesuré.
    /// C'est plus sûr qu'une mesure sur un seul stem, et c'est le nombre que
    /// l'utilisateur reconnaît. `None` : morceau non mesuré.
    bpm: Option<f32>,
    /// Netteté de la pulsation, mesurée sur le stem `drums` de l'établi
    /// (`battements::Grille`). **Au-delà de 2, la pulsation est franche** ;
    /// l'interface ne propose le BPM cible qu'à cette condition. `None` : pas
    /// de stem `drums`, ou il ne se décode pas.
    nettete: Option<f32>,
}

/// Le tempo du morceau ouvert et la netteté de sa pulsation.
///
/// Le BPM cible n'a de sens que sur une matière qui pulse : une nappe ou un
/// morceau rubato n'a pas de tempo à viser, et lui en proposer un donnerait
/// une précision qu'on n'a pas. L'interface s'en sert pour choisir entre le
/// réglage en BPM et le réglage en pourcentage (`docs/ui-spec-editeur.md`).
///
/// La netteté se mesure sur le stem `drums` — le meilleur signal de pulsation
/// une fois le morceau séparé — par la même grille que la greffe
/// (`grilles_des_stems`). Le tempo, lui, vient de la base : mesuré sur le
/// mélange entier, il est plus robuste qu'une batterie seule.
#[tauri::command(async)]
pub(crate) fn tempo_cible(etat: State<Etat>, id: i64) -> Result<TempoCible, String> {
    let (bpm, chemin) = {
        let lib = verrou(&etat.lib);
        let bpm = lib.tempos(&[id]).map_err(echec)?.get(&id).copied();
        let chemin = lib.track(id).map_err(echec)?.map(|t| t.path);
        (bpm, chemin)
    };

    let nettete = chemin.and_then(|p| {
        let dossier = dossier_stems(&etat, Path::new(&p));
        let (_, drums) = stems_du_dossier(&dossier)
            .into_iter()
            .find(|(n, _)| n == "drums")?;
        let s = rusty_music_editor::decode::stereo(&drums).ok()?;
        let mono: Vec<f32> = s
            .gauche
            .iter()
            .zip(&s.droite)
            .map(|(g, d)| (g + d) * 0.5)
            .collect();
        let analyseur = rusty_music_analysis::descripteurs::Analyseur::new();
        rusty_music_analysis::battements::grille_reechantillonnee(
            &mono,
            rusty_music_editor::SR,
            &analyseur,
        )
        .map(|g| g.nettete)
    });

    Ok(TempoCible { bpm, nettete })
}

/// Un stem greffé, et ce qu'il a fallu lui faire.
#[derive(serde::Serialize)]
pub(crate) struct Greffon {
    /// Le WAV écrit, à mettre à la place du stem remplacé.
    chemin: String,
    /// Le morceau d'où il vient, tel qu'on l'affiche.
    origine: String,
    facteur: f32,
    octaves: i32,
    retard_s: f32,
    boucles: usize,
    /// Le greffon est-il entré sur un battement ? L'interface le dit plutôt
    /// que de laisser l'oreille le découvrir.
    cale_aux_temps: bool,
    /// Greffe mesure par mesure : mesures posées, dont appariées par section
    /// (refrain sur refrain). 0 pour la greffe d'un bloc.
    mesures: usize,
    mesures_par_section: usize,
    /// Pourquoi on s'est rabattu sur la greffe d'un bloc, s'il y a lieu.
    repli: Option<String>,
}

/// Les grilles de battements des deux stems.
///
/// **C'est ici que le module 2 rejoint le module 3.** `crates/editor` ne dépend
/// pas de `rusty-music-analysis` — ce serait tirer CLAP, ses 117 Mo de poids et
/// la génération de code de son `build.rs` dans un crate qui n'a que faire d'un
/// modèle d'empreintes. L'application dépend des deux, c'est donc elle qui les
/// relie, en trois nombres.
pub(crate) fn grilles_des_stems(
    remplace: &Path,
    greffon: &Path,
) -> Option<(
    rusty_music_analysis::battements::Grille,
    rusty_music_analysis::battements::Grille,
)> {
    use rusty_music_analysis::battements;

    let analyseur = rusty_music_analysis::descripteurs::Analyseur::new();
    let une = |chemin: &Path| -> Option<battements::Grille> {
        let s = rusty_music_editor::decode::stereo(chemin).ok()?;
        // Somme des deux voies : une batterie panoramée à gauche ne doit pas
        // rendre une phase différente d'une batterie centrée.
        let mono: Vec<f32> = s
            .gauche
            .iter()
            .zip(&s.droite)
            .map(|(g, d)| (g + d) * 0.5)
            .collect();
        battements::grille_reechantillonnee(&mono, rusty_music_editor::SR, &analyseur)
    };
    Some((une(remplace)?, une(greffon)?))
}

/// Met à la place d'un stem celui d'un autre morceau.
///
/// **Non destructif, comme tout le module** : la greffe est un fichier de plus
/// dans le cache, sous `greffes/`, et le stem d'origine reste où il est. Rouvrir
/// le morceau retrouve ses quatre stems séparés, pas la greffe — les stems sur
/// le disque sont la seule persistance, et une greffe n'en est pas un.
#[tauri::command(async)]
pub(crate) fn stems_greffer(
    etat: State<Etat>,
    id: i64,
    stem: String,
    remplace: String,
    voisin: i64,
) -> Result<Greffon, String> {
    let (source, autre, tempos) = {
        let lib = verrou(&etat.lib);
        let source = lib
            .track(id)
            .map_err(echec)?
            .ok_or_else(|| "morceau ouvert introuvable en base".to_string())?;
        let autre = lib
            .track(voisin)
            .map_err(echec)?
            .ok_or_else(|| "le morceau d'où greffer est introuvable en base".to_string())?;
        let tempos = lib.tempos(&[id, voisin]).map_err(echec)?;
        (source, autre, tempos)
    };

    let origine = format!(
        "{} — {}",
        autre.artist.clone().unwrap_or_else(|| "?".into()),
        autre.title.clone().unwrap_or_else(|| "?".into())
    );
    let dossier_autre = dossier_stems(&etat, Path::new(&autre.path));
    let Some((_, greffon)) = stems_du_dossier(&dossier_autre)
        .into_iter()
        .find(|(n, _)| *n == stem)
    else {
        return Err(format!(
            "{origine} n'a pas de stem « {stem} » — il faut d'abord le séparer"
        ));
    };

    let base = Path::new(&autre.path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "morceau".into());
    let sortie = dossier_stems(&etat, Path::new(&source.path))
        .join("greffes")
        .join(format!("{stem} — {base}.wav"));

    // **Les grilles se mesurent sur les stems, les tempos viennent de la base.**
    // Ce n'est pas une incohérence : la base a mesuré le morceau entier, ce qui
    // est plus sûr qu'une batterie seule, et c'est ce tempo-là qui a servi à
    // choisir le voisin. La grille n'apporte que la phase — mais elle apporte
    // aussi son propre tempo, plus fin, dont on se sert pour l'étirement quand
    // la base n'en a pas.
    // **Mesure par mesure d'abord** (plan 2.1 et 2.4) : les premiers temps
    // de Beat This! des deux côtés et, si les poids d'all-in-one sont là,
    // les sections — refrain sur refrain. Sinon, la greffe d'un bloc.
    use rusty_music_editor::greffe;
    let grille_de = |chemin: &str, tid: i64| -> Result<greffe::Grille, String> {
        let dossier = dossier_stems(&etat, Path::new(chemin));
        let p = pulsation_de(
            &etat,
            Path::new(chemin),
            &dossier.join(rusty_music_editor::pulsation::FICHIER_CACHE),
            rusty_music_editor::pulsation::Modele::Complet,
        )?;
        let sections = if sections_disponibles() {
            sections_de(&etat, tid)
                .map(|v| {
                    v.sections
                        .into_iter()
                        .map(|x| greffe::Partie {
                            debut_s: x.debut_s,
                            fin_s: x.fin_s,
                            etiquette: x.etiquette,
                        })
                        .collect()
                })
                .unwrap_or_else(|e| {
                    tracing::warn!("greffe : sections indisponibles : {e}");
                    Vec::new()
                })
        } else {
            Vec::new()
        };
        Ok(greffe::Grille {
            metrique: p.metrique(),
            premiers_temps: p.premiers_temps,
            sections,
        })
    };
    let par_mesures = grille_de(&source.path, id)
        .and_then(|a| Ok((a, grille_de(&autre.path, voisin)?)))
        .and_then(|(a, b)| greffe::mesure_a_mesure(&a, &b).map(|()| (a, b)));
    let (plan, repli) = match par_mesures {
        Ok((a, b)) => (
            greffe::greffer_par_mesures(Path::new(&remplace), &greffon, &a, &b, &sortie)
                .map_err(echec)?,
            None,
        ),
        Err(raison) => {
            tracing::info!("greffe d'un bloc : {raison}");
            let grilles = grilles_des_stems(Path::new(&remplace), &greffon);
            let plan = greffe::greffer(
                Path::new(&remplace),
                &greffon,
                tempos.get(&id).copied().or(grilles.map(|(a, _)| a.bpm)),
                tempos.get(&voisin).copied().or(grilles.map(|(_, b)| b.bpm)),
                grilles.map(|(a, b)| greffe::Cale {
                    phase_remplace_s: a.phase_s,
                    phase_greffon_s: b.phase_s,
                    periode_greffon_s: b.periode(),
                }),
                &sortie,
            )
            .map_err(echec)?;
            (plan, Some(raison))
        }
    };

    Ok(Greffon {
        chemin: sortie.display().to_string(),
        origine,
        facteur: plan.facteur,
        octaves: plan.octaves,
        retard_s: plan.retard_s,
        boucles: plan.boucles,
        cale_aux_temps: plan.cale_aux_temps,
        mesures: plan.mesures,
        mesures_par_section: plan.mesures_par_section,
        repli,
    })
}

// ---------------------------------------------------------------------------
// Réglages : source de la bibliothèque
// ---------------------------------------------------------------------------
