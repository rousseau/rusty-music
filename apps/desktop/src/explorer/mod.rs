// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes d'Explorer : la carte (`carte`), le voisinage et les familles
//! (`voisinage`), Lama (`lama`), et ce qu'ils partagent (empreintes,
//! centroïdes, graphe des albums).

use super::*;

mod carte;
mod lama;
mod voisinage;

pub(crate) use carte::*;
pub(crate) use lama::*;
pub(crate) use voisinage::*;

/// Charge les empreintes si leur nombre a changé depuis la dernière fois.
///
/// Elles pèsent 55 Mo sur la bibliothèque complète : on ne les relit pas à
/// chaque requête, mais leur nombre augmente tant que l'analyse tourne.
pub(crate) fn charger_vecteurs(etat: &State<Etat>) -> Result<Arc<Vec<Empreinte>>, String> {
    let lib = verrou(&etat.lib);
    let mut cache = verrou(&etat.vecteurs);
    // Compter les empreintes, pas les points de la carte : ces derniers
    // excluent les morceaux pas encore projetés, et pendant une analyse les
    // deux nombres diffèrent en permanence — le cache se croirait alors
    // périmé à chaque appel, et le graphe se reconstruirait à chaque chemin.
    let n = lib
        .count_embeddings(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;
    let a_jour = cache.valeur.as_ref().is_some_and(|v| v.len() == n);
    if !a_jour {
        let neuf = Arc::new(
            lib.embeddings(rusty_music_analysis::passe::MODELE)
                .map_err(echec)?,
        );
        tracing::info!(n = neuf.len(), "empreintes chargées");
        cache.valeur = Some(neuf);
    }
    cache.touche = Some(Instant::now());
    Ok(Arc::clone(
        cache.valeur.as_ref().expect("vient d'être posé"),
    ))
}

/// Centroïdes d'empreinte par album et leurs métadonnées, mis en cache
/// ensemble — mode Explorer → Anneau. Contrairement à `construire_graphe`,
/// pas de verrou de construction séparé : `album_embeddings`/`albums_annotes`
/// ne font qu'agréger du SQL, pas un balayage O(n²) — voir leur documentation
/// côté cœur.
pub(crate) fn charger_centroides_albums(
    etat: &State<Etat>,
) -> Result<(Arc<CentroidesAlbums>, Arc<NoeudsAlbums>), String> {
    let lib = verrou(&etat.lib);
    let n = lib
        .count_embeddings(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;

    let mut cache = verrou(&etat.album_centroides);
    let a_jour = cache
        .valeur
        .as_ref()
        .is_some_and(|(taille, _, _)| *taille == n);
    if !a_jour {
        let debut = std::time::Instant::now();
        let centroides = Arc::new(
            lib.album_embeddings(rusty_music_analysis::passe::MODELE)
                .map_err(echec)?,
        );
        let noeuds = Arc::new(
            lib.albums_annotes(rusty_music_analysis::passe::MODELE)
                .map_err(echec)?
                .into_iter()
                .map(|a| (a.id, a))
                .collect::<HashMap<_, _>>(),
        );
        tracing::info!(
            albums = centroides.len(),
            ms = debut.elapsed().as_millis(),
            "centroïdes d'albums"
        );
        cache.valeur = Some((n, centroides, noeuds));
    }
    let (_, c, no) = cache.valeur.as_ref().expect("vient d'être posé");
    let (c, no) = (Arc::clone(c), Arc::clone(no));
    // Touché à chaque appel, coup au but compris — même raison que
    // `construire_graphe` : un cache très sollicité mais jamais reconstruit
    // ne doit pas sembler inactif à `liberer_si_inactif`.
    cache.touche = Some(Instant::now());
    Ok((c, no))
}

/// Le curseur « voisins » du rail plafonne à 20 (voir `index.html`) : le
/// graphe des albums doit couvrir au moins ça pour que `Graphe::voisins`
/// puisse rendre le maximum demandé, pas seulement ce qu'il faut au fond
/// permanent.
pub(crate) const ANNEAU_K_MAX: usize = 20;

/// Construit (ou retrouve en cache) le graphe des k plus proches albums —
/// voir `Etat::album_graphe`. Balayage complet sur les centroïdes, pas sur
/// les 27 000 morceaux : quelques milliers de nœuds, une fraction de seconde
/// même à plusieurs cœurs de moins que `construire_graphe`.
pub(crate) fn charger_graphe_albums(
    etat: &State<Etat>,
    centroides: &Arc<Vec<(i64, Vec<f32>)>>,
) -> Result<Arc<Graphe>, String> {
    let n = centroides.len();
    let mut cache = verrou(&etat.album_graphe);
    if let Some((taille, g)) = cache.valeur.as_ref() {
        if *taille == n {
            let g = Arc::clone(g);
            cache.touche = Some(Instant::now());
            return Ok(g);
        }
    }
    let debut = std::time::Instant::now();
    let g = Arc::new(Graphe::construire(
        centroides,
        ANNEAU_K_MAX,
        coeurs_arriere_plan(),
    ));
    tracing::info!(
        albums = n,
        ms = debut.elapsed().as_millis(),
        "graphe des albums"
    );
    cache.valeur = Some((n, Arc::clone(&g)));
    cache.touche = Some(Instant::now());
    Ok(g)
}

/// `(fait, total)` du balayage qui construit le graphe des voisins.
///
/// `total == 0` : rien en cours — jamais construit, ou déjà en cache et donc
/// instantané. L'interface sonde ça pendant l'attente de la première playlist
/// « dans l'esprit de » pour montrer une jauge plutôt qu'une roulette muette.
#[tauri::command(async)]
pub(crate) fn graphe_progress(etat: State<Etat>) -> (i64, i64) {
    (
        etat.graphe_fait.load(Ordering::Relaxed) as i64,
        etat.graphe_total.load(Ordering::Relaxed) as i64,
    )
}

/// Combien de morceaux restent à analyser — pour dire où en est la carte.
#[tauri::command(async)]
pub(crate) fn map_progress(etat: State<Etat>) -> Result<(i64, i64), String> {
    let lib = verrou(&etat.lib);
    let total = lib.count().map_err(echec)?;
    let restants = lib
        .pending_analysis(rusty_music_analysis::passe::MODELE, i64::MAX)
        .map_err(echec)?
        .len() as i64;
    Ok((total - restants, total))
}
