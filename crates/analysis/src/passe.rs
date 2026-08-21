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
use crate::projection::{cadrer, projeter};
use crate::Embedder;

/// Nom du modèle, tel qu'inscrit en base. Change avec le modèle : deux jeux
/// d'empreintes peuvent alors coexister.
///
/// **Le fenêtrage en fait partie.** Trois fenêtres et neuf fenêtres ne
/// décrivent pas le même morceau ; sans le suffixe, deux passes différentes
/// se mélangeraient dans la même carte sans que rien ne le signale.
pub const MODELE: &str = "clap-htsat-unfused-5f";

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
    mut avancement: impl FnMut(usize, usize) + Send,
) -> Result<Rapport, Error> {
    let pistes = lib.pending_analysis(MODELE, limite)?;
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
    let curseur = AtomicUsize::new(0);

    // Deux canaux en série. Un spectrogramme pèse 1,3 Mo : la borne les
    // compte, sinon les décodeurs prendraient toute la mémoire d'avance sur un
    // encodeur plus lent qu'eux.
    let (tx_spec, rx_spec) =
        std::sync::mpsc::sync_channel::<(i64, Option<(Vec<f32>, usize)>)>(travailleurs.max(1) * 2);
    // `Library` tient une connexion SQLite : `Send` mais pas `Sync`. C'est
    // donc le fil appelant, et lui seul, qui écrit.
    let (tx_emp, rx_emp) = std::sync::mpsc::sync_channel::<(i64, Option<Vec<f32>>)>(8);

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
                    let issue = match spectrogramme(&mel, chemin, FENETRES) {
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
                let empreinte = issue.and_then(|(spec, n)| match agreger(&mut enc, &spec, n) {
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
                Some(v) => match lib.save_embedding(id, MODELE, &v) {
                    Ok(()) => rapport.empreintes += 1,
                    Err(e) => {
                        warn!(error = %e, "écriture impossible");
                        rapport.echecs += 1;
                    }
                },
                None => rapport.echecs += 1,
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
/// `familles` prime sur [`Library::parametres_carte`] quand fourni — c'est
/// ce que garde le `--familles` de la CLI, un réglage ponctuel plutôt qu'un
/// changement du paramètre gardé en base. `None` (l'appli desktop) retombe
/// sur ce que l'interface a réglé — ou les valeurs par défaut, tant que
/// personne n'y a touché.
pub fn projeter_tout(lib: &Library, familles: Option<usize>) -> Result<Rapport, Error> {
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
    let familles = familles.unwrap_or(params.familles);

    let vecteurs: Vec<Vec<f32>> = empreintes.iter().map(|(_, v)| v.clone()).collect();
    let mut points = projeter(&vecteurs, params.perplexite, params.epoques);
    cadrer(&mut points);

    // Le regroupement porte sur les empreintes, pas sur la carte : t-SNE
    // déforme les distances, s'en servir décrirait le dessin, pas la musique.
    let appartenance = kmeans(
        &vecteurs,
        familles.clamp(1, vecteurs.len()),
        params.iterations_kmeans,
    );
    rapport.familles = appartenance
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();

    let maj: Vec<(i64, f32, f32, i64)> = empreintes
        .iter()
        .zip(points.iter().zip(&appartenance))
        .map(|((id, _), (p, c))| (*id, p.x, p.y, *c as i64))
        .collect();
    lib.update_map(MODELE, &maj)?;
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
    Ok(crate::projection::normaliser(&somme))
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

    let pistes = lib.pending_descripteurs(MODELE, limite)?;
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
                    let issue = match analyser(chemin, &a) {
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
                    ) {
                        Ok(()) => rapport.mesures += 1,
                        Err(e) => {
                            warn!(id, error = %e, "écriture impossible");
                            rapport.echecs += 1;
                        }
                    }
                }
                None => rapport.echecs += 1,
            }
            vus += 1;
            avancement(vus, total);
        }
    });

    debug!(?rapport, "passe de descripteurs terminée");
    Ok(rapport)
}
