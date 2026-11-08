// SPDX-License-Identifier: GPL-3.0-or-later
//! La passe d'analyse : des morceaux en base aux points de la carte.
//!
//! Décodage et empreintes se font morceau par morceau, en parallèle. La
//! projection et le regroupement, eux, ont besoin de **tout** le lot d'un
//! coup : t-SNE place chaque point relativement aux autres. La passe est donc
//! en deux temps, et ce n'est pas un choix d'implémentation mais la nature de
//! l'algorithme.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rusty_music_core::Library;
use tracing::{debug, warn};

use crate::cluster::kmeans;
use crate::decode::{fenetres, FENETRES};
use crate::mel::Mel;
use crate::projection::{cadrer, placer_nouveaux, projeter, reprendre, Point};
use crate::Embedder;

/// Nom du modèle, tel qu'inscrit en base. Change avec le modèle : deux jeux
/// d'empreintes peuvent alors coexister.
///
/// **Le fenêtrage en fait partie.** Trois fenêtres et neuf fenêtres ne
/// décrivent pas le même morceau ; sans le suffixe, deux passes différentes
/// se mélangeraient dans la même carte sans que rien ne le signale.
pub const MODELE: &str = "clap-htsat-unfused-5f";

/// Version de l'algorithme tempo/tonalité/énergie (`descripteurs::analyser`).
///
/// **À incrémenter à chaque correctif qui change la mesure elle-même** —
/// l'inversion d'octave du tempo (`battements.rs`, sept. 2026) en est
/// l'exemple qui a motivé cette colonne (v2 : tonalité par profils d'Albrecht-
/// Shanahan, chroma à FFT 16384 et séparation harmonique/percussive, oct.
/// 2026 ; v3 : plafond du double ramené à 200 BPM, calé sur GTZAN) : sans version, rien ne distingue une
/// mesure d'avant le correctif d'une mesure d'après, et
/// `pending_descripteurs` les traite toutes deux comme « déjà faites ». Un
/// ajout de sortie qui ne change pas les valeurs déjà écrites (un nouveau
/// descripteur en plus des existants, par exemple) n'a pas besoin de
/// l'incrément — la case « refaire ce qui est déjà mesuré » du mode
/// Bibliothèque reste le bon outil pour ce cas-là.
pub const VERSION_DESCRIPTEURS: i32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("base de données : {0}")]
    Base(#[from] rusty_music_core::Error),

    #[error("modèle : {0}")]
    Modele(#[from] crate::Error),

    #[error("décodage : {0}")]
    Decodage(#[from] crate::decode::Error),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Rapport {
    pub demandes: usize,
    pub empreintes: usize,
    pub echecs: usize,
    pub familles: usize,
    /// Ce que [`projeter_tout`] a fait des coordonnées.
    pub projection: Projection,
}

/// Comment [`projeter_tout`] place les points : la demande, puis ce qui a
/// été fait.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    /// Depuis zéro : un nouveau dessin.
    #[default]
    Refaite,
    /// Depuis les coordonnées précédentes, les nouveaux près de leurs
    /// voisins ([`crate::projection::reprendre`]).
    Reprise,
    /// Rien d'ajouté depuis la dernière : coordonnées laissées telles quelles.
    Gardee,
}

/// Calcule les empreintes des morceaux en attente.
///
/// Chaque empreinte est écrite dès qu'elle est prête : la passe complète dure
/// des heures sur un support lent, et une interruption ne doit rien coûter de
/// plus que le morceau en cours. Les coordonnées viennent après, par
/// [`projeter_tout`].
///
/// `travailleurs` est le nombre de fils de **décodage**. L'inférence, elle,
/// n'a qu'un fil : le modèle vit sur le GPU, et en charger douze copies y
/// occuperait 1,4 Go pour rien. Le décodage occupe donc les cœurs, l'encodeur
/// occupe l'accélérateur, et les deux avancent de front.
pub fn empreintes(
    lib: &Library,
    modele: Option<&Path>,
    limite: i64,
    travailleurs: usize,
    avancement: impl FnMut(usize, usize) + Send,
) -> Result<Rapport, Error> {
    let pistes = lib.pending_analysis(MODELE, limite)?;
    passe_clap(lib, pistes, false, modele, travailleurs, avancement)
}

/// Retrouve la fenêtre représentative des morceaux analysés avant qu'on la
/// retienne (`docs/plan-balade.md`, P4). Même chaîne que [`empreintes`] —
/// les mêmes fenêtres passent dans CLAP —, mais seule la fenêtre est écrite :
/// l'empreinte en place ne bouge pas. Comptées dans `Rapport::empreintes`.
pub fn fenetres_representatives(
    lib: &Library,
    modele: Option<&Path>,
    limite: i64,
    travailleurs: usize,
    avancement: impl FnMut(usize, usize) + Send,
) -> Result<Rapport, Error> {
    let pistes = lib.pending_fenetres(MODELE, limite)?;
    passe_clap(lib, pistes, true, modele, travailleurs, avancement)
}

/// Décodage sur les cœurs, CLAP sur l'accélérateur, écriture ici ;
/// `fenetre_seule` n'écrit que la fenêtre représentative.
fn passe_clap(
    lib: &Library,
    pistes: Vec<rusty_music_core::db::TrackRow>,
    fenetre_seule: bool,
    modele: Option<&Path>,
    travailleurs: usize,
    mut avancement: impl FnMut(usize, usize) + Send,
) -> Result<Rapport, Error> {
    let mut rapport = Rapport {
        demandes: pistes.len(),
        ..Default::default()
    };
    if pistes.is_empty() {
        return Ok(rapport);
    }

    let file: Vec<(i64, PathBuf)> = pistes
        .iter()
        .map(|t| (t.id, PathBuf::from(&t.path)))
        .collect();
    let total = file.len();
    // Pour retrouver, sur le fil d'écriture, le chemin d'un échec signalé par
    // son seul identifiant — c'est lui qui va dans `scan_failures`.
    let chemins: std::collections::HashMap<i64, &Path> =
        file.iter().map(|(id, p)| (*id, p.as_path())).collect();
    let curseur = AtomicUsize::new(0);

    // Deux canaux en série. Un spectrogramme pèse 1,3 Mo : la borne les
    // compte, sinon les décodeurs prendraient toute la mémoire d'avance sur un
    // encodeur plus lent qu'eux.
    let (tx_spec, rx_spec) =
        std::sync::mpsc::sync_channel::<(i64, Option<(Vec<f32>, usize)>)>(travailleurs.max(1) * 2);
    // `Library` tient une connexion SQLite : `Send` mais pas `Sync`. C'est
    // donc le fil appelant, et lui seul, qui écrit.
    let (tx_emp, rx_emp) = std::sync::mpsc::sync_channel::<(i64, Option<(Vec<f32>, usize)>)>(8);

    std::thread::scope(|pool| {
        // --- décodage, sur les cœurs ---
        for _ in 0..travailleurs.max(1) {
            let tx = tx_spec.clone();
            let (curseur, file) = (&curseur, &file);
            pool.spawn(move || {
                let mel = Mel::new();
                loop {
                    let i = curseur.fetch_add(1, Ordering::Relaxed);
                    let Some((id, chemin)) = file.get(i) else {
                        break;
                    };
                    // Un décodage qui panique (rodio/symphonia sur un
                    // fichier hasardeux) ne doit pas emporter toute la passe
                    // — voir `rusty_music_core::panique`.
                    let resultat = match rusty_music_core::panique::sans_panique(|| {
                        spectrogramme(&mel, chemin, FENETRES)
                    }) {
                        Ok(r) => r,
                        Err(message) => Err(Error::Decodage(crate::decode::Error::Panique {
                            path: chemin.clone(),
                            message,
                        })),
                    };
                    let issue = match resultat {
                        Ok(s) => Some(s),
                        Err(e) => {
                            warn!(path = %chemin.display(), error = %e, "décodage impossible");
                            None
                        }
                    };
                    if tx.send((*id, issue)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx_spec);

        // --- inférence, sur l'accélérateur ---
        pool.spawn(move || {
            let mut enc = match Embedder::charger(modele, 1) {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "modèle inutilisable");
                    return;
                }
            };
            for (id, issue) in rx_spec {
                let empreinte =
                    issue.and_then(|(spec, n)| match agreger_et_choisir(&mut enc, &spec, n) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            warn!(id, error = %e, "empreinte impossible");
                            None
                        }
                    });
                if tx_emp.send((id, empreinte)).is_err() {
                    break;
                }
            }
        });

        // --- écriture, ici ---
        let mut vus = 0usize;
        for (id, issue) in rx_emp {
            match issue {
                // Écrite dès qu'elle arrive : une interruption ne coûte que
                // le morceau en cours, pas la passe entière.
                Some((v, rang)) => match if fenetre_seule {
                    Ok(())
                } else {
                    lib.save_embedding(id, MODELE, &v)
                }
                .and_then(|()| lib.save_fenetre_representative(id, MODELE, rang))
                {
                    Ok(()) => rapport.empreintes += 1,
                    Err(e) => {
                        warn!(error = %e, "écriture impossible");
                        rapport.echecs += 1;
                    }
                },
                None => {
                    rapport.echecs += 1;
                    // Un fichier qui déstabilise déjà son support ne doit pas
                    // être retapé à chaque passe — voir `pending_analysis`.
                    if let Some(chemin) = chemins.get(&id) {
                        let _ = lib.enregistrer_echec_scan(
                            chemin,
                            "décodage impossible pendant l'analyse (empreintes)",
                        );
                    }
                }
            }
            vus += 1;
            if vus % 25 == 0 || vus == total {
                debug!("{vus}/{total}");
                avancement(vus, total);
            }
        }
    });

    Ok(rapport)
}

/// Place sur la carte **toutes** les empreintes du modèle.
///
/// Ne se découpe pas : les coordonnées t-SNE n'ont de sens que relativement à
/// l'ensemble projeté d'un bloc — deux lots donneraient deux repères sans
/// rapport. Heureusement l'opération est bon marché (6 s sur 27 000 points),
/// donc rejouable après chaque lot d'empreintes.
///
/// `familles` ne sert plus qu'au repli k-means ([`kmeans_de_secours`]) — voir
/// plus bas — le regroupement par genre n'a pas de compte à fixer, il rend
/// ce que le vocabulaire et la bibliothèque donnent.
///
/// **Le regroupement se fait par genre, pas par k-means, dès qu'il y a un
/// genre à lire.** `cluster.rs` explique le choix : k-means, purement
/// acoustique, a rendu des familles que rien de commun ne nomme (mesuré :
/// « Reggae · Rock »). Le vocabulaire de [`cluster::familles_par_genre`]
/// ancre chaque famille sur un genre reconnaissable et ne retombe sur
/// l'empreinte que pour ce que le vocabulaire ne sait pas nommer.
///
/// **`familles` retrouve son rôle d'avant** dans un seul cas : aucun morceau
/// n'a de genre résolu — bibliothèque jamais passée par `enrich` et sans tag
/// `genre` exploitable. Le vocabulaire n'aurait alors aucune ancre, et sans
/// repli la carte perdrait toute famille d'un coup. `kmeans` reprend la main
/// pour ce cas précis, comme avant cette bascule.
///
/// **Le nuage reste en place** (D5 de `synthese-novembre-2026.md`). Demandée
/// en [`Projection::Reprise`], la projection repart des coordonnées en base :
/// sans rien de nouveau elle les garde ; avec des nouveaux, elle les pose près
/// de leurs voisins et reprend t-SNE ([`crate::projection::reprendre`]). Elle
/// refait tout quand moins de la moitié des morceaux avaient une place, ou
/// quand la perplexité ou les époques ont changé depuis la dernière
/// projection refaite. [`Projection::Refaite`] force un nouveau dessin.
pub fn projeter_tout(
    lib: &Library,
    familles: Option<usize>,
    demande: Projection,
) -> Result<Rapport, Error> {
    let empreintes = lib.embeddings(MODELE)?;
    let mut rapport = Rapport {
        demandes: empreintes.len(),
        empreintes: empreintes.len(),
        ..Default::default()
    };
    if empreintes.is_empty() {
        return Ok(rapport);
    }

    let params = lib.parametres_carte()?;

    let vecteurs: Vec<Vec<f32>> = empreintes.iter().map(|(_, v)| v.clone()).collect();

    let precedents: std::collections::HashMap<i64, Point> = lib
        .map_points(MODELE)?
        .into_iter()
        .map(|(id, x, y, _)| (id, Point { x, y }))
        .collect();
    let places: Vec<Option<Point>> = empreintes
        .iter()
        .map(|(id, _)| precedents.get(id).copied())
        .collect();
    let connus = places.iter().filter(|p| p.is_some()).count();
    // Sans note (base d'avant D5), les paramètres sont supposés inchangés :
    // la première passe ne redessine pas tout.
    let memes_parametres = lib
        .projection_retenue()?
        .is_none_or(|(p, e)| p == params.perplexite && e == params.epoques);
    rapport.projection =
        if demande == Projection::Refaite || !memes_parametres || connus * 2 < places.len() {
            Projection::Refaite
        } else if connus == places.len() {
            Projection::Gardee
        } else {
            Projection::Reprise
        };

    let points: Vec<Point> = match rapport.projection {
        Projection::Refaite => {
            let mut p = projeter(&vecteurs, params.perplexite, params.epoques);
            cadrer(&mut p);
            lib.retenir_projection(params.perplexite, params.epoques)?;
            p
        }
        Projection::Reprise => {
            let depart = placer_nouveaux(&vecteurs, &places, crate::chemin::K_VOISINS);
            let mut p = reprendre(&vecteurs, params.perplexite, &depart);
            cadrer(&mut p);
            p
        }
        Projection::Gardee => places.into_iter().flatten().collect(),
    };
    debug!(
        projection = ?rapport.projection,
        nouveaux = empreintes.len() - connus,
        "projection"
    );

    // Le regroupement porte sur les empreintes, pas sur la carte : t-SNE
    // déforme les distances, s'en servir décrirait le dessin, pas la musique.
    let genres = lib.genres_resolus(MODELE)?;
    let genres_alignes: Vec<Option<String>> = empreintes
        .iter()
        .map(|(id, _)| genres.get(id).cloned())
        .collect();
    let vocabulaire = lib.vocabulaire_familles()?;

    let appartenance: Vec<i64> = if genres_alignes.iter().any(Option::is_some) {
        crate::cluster::familles_par_genre(&vecteurs, &genres_alignes, &vocabulaire)
            .into_iter()
            // Sans famille (aucun genre du vocabulaire ancré assez près) :
            // -1, convention déjà en usage côté carte pour « morceau sans
            // famille ».
            .map(|f| f.map_or(-1, |f| f as i64))
            .collect()
    } else {
        kmeans(
            &vecteurs,
            familles.unwrap_or(params.familles).clamp(1, vecteurs.len()),
            params.iterations_kmeans,
        )
        .into_iter()
        .map(|f| f as i64)
        .collect()
    };
    rapport.familles = appartenance
        .iter()
        .filter(|f| **f >= 0)
        .collect::<std::collections::HashSet<_>>()
        .len();

    let maj: Vec<(i64, f32, f32, i64)> = empreintes
        .iter()
        .zip(points.iter().zip(&appartenance))
        .map(|((id, _), (p, c))| (*id, p.x, p.y, *c))
        .collect();
    lib.update_map(MODELE, &maj)?;

    // Vote CLAP-texte pour le nommage des familles (`docs/nommage-
    // familles.md`) : calculé en lot, comme la projection et le
    // regroupement ci-dessus, car le calibrage a besoin du score de tous
    // les morceaux avant de centrer chaque colonne. Absent tant que le
    // vocabulaire n'a pas été généré hors ligne — MusicBrainz et Last.fm
    // votent alors seuls.
    if let Some(vocab) = crate::vocabulaire_texte::charger() {
        let gagnants = crate::vocabulaire_texte::voter(&vecteurs, &vocab);
        let maj_labels: Vec<(i64, Option<String>)> = empreintes
            .iter()
            .zip(gagnants)
            .map(|((id, _), g)| (*id, g))
            .collect();
        lib.update_texte_labels(MODELE, &maj_labels)?;
    }

    Ok(rapport)
}

/// Spectrogramme log-mel d'un morceau, prêt pour l'encodeur.
///
/// Rend les valeurs à plat et le nombre de fenêtres réellement obtenues : un
/// morceau plus court qu'une fenêtre n'en donne qu'une, et l'encodeur doit le
/// savoir pour ne pas moyenner du remplissage.
pub fn spectrogramme(mel: &Mel, chemin: &Path, n: usize) -> Result<(Vec<f32>, usize), Error> {
    let blocs = fenetres(chemin, n)?;
    let spec: Vec<f32> = blocs.iter().flat_map(|b| mel.spectrogramme(b)).collect();
    Ok((spec, blocs.len()))
}

/// Empreinte à partir d'un spectrogramme : moyenne des fenêtres, ramenée sur
/// la sphère.
///
/// Un morceau qui change de caractère en route est mieux représenté par
/// l'ensemble de ses fenêtres que par ses dix secondes centrales.
pub fn agreger(enc: &mut Embedder, spec: &[f32], n: usize) -> Result<Vec<f32>, Error> {
    Ok(agreger_et_choisir(enc, spec, n)?.0)
}

/// [`agreger`], et le rang de la fenêtre la plus représentative
/// ([`fenetre_representative`]).
pub fn agreger_et_choisir(
    enc: &mut Embedder,
    spec: &[f32],
    n: usize,
) -> Result<(Vec<f32>, usize), Error> {
    let vecteurs = enc.empreintes(spec, n)?;
    if vecteurs.is_empty() {
        return Err(Error::Modele(crate::Error::Sortie(
            "aucune fenêtre".to_string(),
        )));
    }
    let mut somme = vec![0.0f32; vecteurs[0].len()];
    for v in &vecteurs {
        for (s, x) in somme.iter_mut().zip(v) {
            *s += x / vecteurs.len() as f32;
        }
    }
    let rang = fenetre_representative(&vecteurs).unwrap_or(0);
    Ok((crate::projection::normaliser(&somme), rang))
}

/// La fenêtre la plus représentative d'un morceau : celle dont l'empreinte
/// est la plus proche (cosinus) de leur moyenne. C'est elle que l'aura de la
/// carte fait entendre (`docs/plan-balade.md`, P4). `None` sans fenêtre.
pub fn fenetre_representative(vecteurs: &[Vec<f32>]) -> Option<usize> {
    let premier = vecteurs.first()?;
    let mut moyenne = vec![0.0f32; premier.len()];
    for v in vecteurs {
        for (m, x) in moyenne.iter_mut().zip(v) {
            *m += x;
        }
    }
    let moyenne = crate::projection::normaliser(&moyenne);
    let cosinus = |v: &Vec<f32>| {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        v.iter().zip(&moyenne).map(|(a, b)| a * b).sum::<f32>() / n
    };
    (0..vecteurs.len()).max_by(|&a, &b| cosinus(&vecteurs[a]).total_cmp(&cosinus(&vecteurs[b])))
}

/// Empreinte d'un morceau, de bout en bout.
///
/// Publique pour que les exemples de mesure empruntent le chemin de production
/// exact, et non une copie qui aurait dérivé.
pub fn empreinte(
    mel: &Mel,
    enc: &mut Embedder,
    chemin: &Path,
    n: usize,
) -> Result<Vec<f32>, Error> {
    let (spec, obtenues) = spectrogramme(mel, chemin, n)?;
    agreger(enc, &spec, obtenues)
}

/// Ce qu'une passe de descripteurs a produit.
#[derive(Debug, Default, Clone, Copy)]
pub struct RapportDescripteurs {
    pub demandes: usize,
    pub mesures: usize,
    /// Morceaux mesurés mais sans pulsation décelable.
    pub sans_tempo: usize,
    /// Morceaux mesurés mais sans tonalité décelable.
    pub sans_tonalite: usize,
    pub echecs: usize,
}

/// Mesure tempo, tonalité et énergie des morceaux qui n'en ont pas.
///
/// Même forme que [`empreintes`], en plus simple : il n'y a pas d'étage
/// d'inférence, seulement du décodage et du calcul, tous deux sur les cœurs.
/// L'écriture reste sur le fil appelant, `Library` tenant une connexion SQLite
/// qui n'est pas `Sync`.
///
/// Comme la passe d'empreintes, elle écrit au fil de l'eau et se reprend :
/// `pending_descripteurs` ne rend que ce qui manque.
pub fn descripteurs(
    lib: &Library,
    limite: i64,
    travailleurs: usize,
    mut avancement: impl FnMut(usize, usize) + Send,
) -> Result<RapportDescripteurs, Error> {
    use crate::descripteurs::{analyser, Analyseur, Descripteurs};

    let pistes = lib.pending_descripteurs(MODELE, VERSION_DESCRIPTEURS, limite)?;
    let mut rapport = RapportDescripteurs {
        demandes: pistes.len(),
        ..Default::default()
    };
    if pistes.is_empty() {
        return Ok(rapport);
    }

    let file: Vec<(i64, PathBuf)> = pistes
        .into_iter()
        .map(|p| (p.id, PathBuf::from(p.path)))
        .collect();
    let total = file.len();
    // Même usage que dans `empreintes` : retrouver le chemin d'un échec sur
    // le fil d'écriture, pour l'enregistrer dans `scan_failures`.
    let chemins: std::collections::HashMap<i64, &Path> =
        file.iter().map(|(id, p)| (*id, p.as_path())).collect();
    let curseur = AtomicUsize::new(0);
    let (tx, rx) =
        std::sync::mpsc::sync_channel::<(i64, Option<Descripteurs>)>(travailleurs.max(1) * 2);

    std::thread::scope(|pool| {
        for _ in 0..travailleurs.max(1) {
            let tx = tx.clone();
            let (curseur, file) = (&curseur, &file);
            pool.spawn(move || {
                // Un analyseur par fil : il porte deux plans de FFT, coûteux à
                // établir et bon marché à garder.
                let a = Analyseur::new();
                loop {
                    let i = curseur.fetch_add(1, Ordering::Relaxed);
                    let Some((id, chemin)) = file.get(i) else {
                        break;
                    };
                    // Même précaution que dans `empreintes` : un décodage qui
                    // panique ne doit pas emporter toute la passe.
                    let resultat =
                        match rusty_music_core::panique::sans_panique(|| analyser(chemin, &a)) {
                            Ok(r) => r,
                            Err(message) => Err(crate::decode::Error::Panique {
                                path: chemin.clone(),
                                message,
                            }),
                        };
                    let issue = match resultat {
                        Ok(d) => Some(d),
                        Err(e) => {
                            warn!(path = %chemin.display(), error = %e, "descripteurs impossibles");
                            None
                        }
                    };
                    if tx.send((*id, issue)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut vus = 0usize;
        for (id, issue) in rx {
            match issue {
                Some(d) => {
                    if d.bpm.is_none() {
                        rapport.sans_tempo += 1;
                    }
                    if d.tonalite.is_none() {
                        rapport.sans_tonalite += 1;
                    }
                    match lib.save_descripteurs(
                        id,
                        d.bpm,
                        d.tonalite.as_deref(),
                        d.energie,
                        d.sonie,
                        d.zcr,
                        d.centroide_moy,
                        d.centroide_ecart,
                        d.rolloff_moy,
                        d.rolloff_ecart,
                        d.flatness_moy,
                        d.flatness_ecart,
                        VERSION_DESCRIPTEURS,
                    ) {
                        Ok(()) => rapport.mesures += 1,
                        Err(e) => {
                            warn!(id, error = %e, "écriture impossible");
                            rapport.echecs += 1;
                        }
                    }
                }
                None => {
                    rapport.echecs += 1;
                    if let Some(chemin) = chemins.get(&id) {
                        let _ = lib.enregistrer_echec_scan(
                            chemin,
                            "décodage impossible pendant l'analyse (descripteurs)",
                        );
                    }
                }
            }
            vus += 1;
            avancement(vus, total);
        }
    });

    debug!(?rapport, "passe de descripteurs terminée");
    Ok(rapport)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trois amas d'empreintes, `n` morceaux de plus à chaque appel.
    fn ajouter(lib: &Library, debut: usize, n: usize) {
        for i in debut..debut + n {
            let id = lib
                .upsert(&rusty_music_core::TrackMeta {
                    path: PathBuf::from(format!("/m/{i}.flac")),
                    ..Default::default()
                })
                .unwrap();
            let mut v = vec![0.0f32; 32];
            v[(i % 3) * 10] = 1.0;
            v[(i % 3) * 10 + 1] = 0.5 + (i as f32) * 0.003;
            lib.save_embedding(id, MODELE, &v).unwrap();
        }
    }

    /// Rien de neuf : le nuage est gardé tel quel. Des nouveaux : il est
    /// repris. La perplexité change : il est refait.
    #[test]
    fn la_projection_garde_reprend_ou_refait_le_nuage() {
        let lib = Library::open_in_memory().unwrap();
        ajouter(&lib, 0, 60);
        let r = projeter_tout(&lib, Some(3), Projection::Reprise).unwrap();
        assert_eq!(r.projection, Projection::Refaite, "aucun point placé");
        let avant = lib.map_points(MODELE).unwrap();

        let r = projeter_tout(&lib, Some(3), Projection::Reprise).unwrap();
        assert_eq!(r.projection, Projection::Gardee);
        let xy = |p: &[(i64, f32, f32, i64)]| {
            p.iter()
                .map(|(i, x, y, _)| (*i, *x, *y))
                .collect::<Vec<_>>()
        };
        assert_eq!(xy(&lib.map_points(MODELE).unwrap()), xy(&avant));

        ajouter(&lib, 60, 6);
        let r = projeter_tout(&lib, Some(3), Projection::Reprise).unwrap();
        assert_eq!(r.projection, Projection::Reprise);
        assert_eq!(lib.map_points(MODELE).unwrap().len(), 66);

        lib.set_parametre_carte("perplexite", 12.0).unwrap();
        let r = projeter_tout(&lib, Some(3), Projection::Reprise).unwrap();
        assert_eq!(r.projection, Projection::Refaite, "perplexité changée");
        let r = projeter_tout(&lib, Some(3), Projection::Reprise).unwrap();
        assert_eq!(r.projection, Projection::Gardee);

        let r = projeter_tout(&lib, Some(3), Projection::Refaite).unwrap();
        assert_eq!(r.projection, Projection::Refaite, "demandé");
    }

    #[test]
    fn la_fenetre_representative_est_la_plus_proche_de_la_moyenne() {
        // Trois fenêtres semblables et une intro à part : la moyenne penche
        // vers les trois, et c'est la plus centrale d'entre elles qui gagne.
        let fenetres = vec![
            vec![0.0, 0.0, 1.0],
            vec![1.0, 0.2, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![1.0, -0.2, 0.0],
        ];
        assert_eq!(fenetre_representative(&fenetres), Some(2));
        assert_eq!(fenetre_representative(&[]), None);
        assert_eq!(fenetre_representative(&[vec![0.3, 0.4]]), Some(0));
    }
}
