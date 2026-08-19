//! Application de bureau — coquille « Atelier », mode Écoute (module 1).
//!
//! Cette couche ne fait que raccorder : toute la logique vit dans `rusty-music-core`
//! (consultation de la base) et `rusty-music-player` (sortie audio, transport). Elle
//! n'ouvre jamais un fichier musical elle-même.

// Sur Windows, évite d'ouvrir une console derrière la fenêtre.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusty_music_analysis::chemin::{Empreinte, Graphe};
use rusty_music_core::db::{AlbumRow, ArtistRow, MapPoint, RootRow, TrackRow};
use rusty_music_core::Library;
use rusty_music_player::Player;
use tauri::{Manager, State};

/// État partagé. `rusqlite::Connection` et le lecteur ne sont pas `Sync` : on
/// les protège chacun par un verrou plutôt que d'ouvrir une base par appel.
struct Etat {
    lib: Mutex<Library>,
    player: Mutex<Player>,
    /// Chemin de la base, pour que le scan puisse ouvrir sa propre connexion.
    db: PathBuf,
    scan: Mutex<EtatScan>,
    analyse: Mutex<EtatAnalyse>,
    enrichissement: Mutex<EtatEnrichissement>,
    demix: Mutex<EtatDemix>,
    /// Le jeu de stems en écoute, s'il y en a un. Il tient sa propre sortie
    /// audio : le lecteur du module 1 se tait pendant ce temps.
    stems: Mutex<Option<rusty_music_player::Multipiste>>,
    /// Enveloppes déjà calculées. Le calcul décode tout le fichier : quelques
    /// secondes par piste sur la carte SD, à ne pas refaire à chaque affichage.
    ondes: Mutex<rusty_music_player::waveform::Cache>,
    /// Empreintes chargées pour le calcul des chemins. Les relire à chaque
    /// requête coûterait 55 Mo de lecture ; on les garde, en réinvalidant
    /// quand leur nombre change — ce qui arrive tant que l'analyse tourne.
    /// Sous `Arc` pour que les calculs longs travaillent sur une copie du
    /// pointeur, verrou relâché : construire le graphe prend une dizaine de
    /// secondes, pendant lesquelles l'inspecteur doit rester servi.
    vecteurs: Mutex<Arc<Vec<Empreinte>>>,
    /// Graphe des plus proches voisins, avec le nombre d'empreintes qui l'a
    /// produit. Le construire est un balayage complet : on ne le refait que
    /// lorsque ce nombre a bougé, et seulement pour les modes qui en ont
    /// besoin (lisse et errance).
    graphe: Mutex<Option<(usize, Arc<Graphe>)>>,
}

/// Avancement du scan en cours, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
struct EtatScan {
    en_cours: bool,
    racine: String,
    /// Morceaux en base : seule mesure d'avancement disponible, le scan ne
    /// rend son rapport qu'à la fin.
    morceaux: i64,
    resultat: Option<String>,
}

/// Les erreurs du moteur ne traversent pas l'IPC : on les rend en texte.
fn echec(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Consultation de la bibliothèque
// ---------------------------------------------------------------------------
/// **Toutes les commandes portent `(async)`, et ce n'est pas décoratif.**
///
/// Sur une fonction *non* asynchrone, cet attribut ne rend pas la fonction
/// asynchrone : il la fait exécuter sur un **pool de fils** au lieu du fil
/// principal (`sync_threadpool`, dans la macro de Tauri). Les arguments, les
/// verrous et les erreurs se comportent exactement comme avant.
///
/// Sans lui, une commande qui touche le disque fige **toute l'interface**, pas
/// seulement elle-même. Le cas mesuré : `play` ouvre le fichier et laisse
/// symphonia le sonder ; sur une carte SD saturée par une passe d'analyse, la
/// totalité des échantillons du profileur montrait le fil principal bloqué dans
/// un `read()`. L'application paraissait tourner dans le vide alors qu'elle
/// attendait le disque.
///
/// La règle est uniforme parce que la frontière ne l'est pas : `skip` ouvre la
/// piste suivante, `waveform` décode le morceau entier, et même un simple
/// lecteur d'état prend un verrou que ces opérations détiennent. Aucune
/// commande d'ici n'a besoin du fil principal — aucune ne touche la fenêtre.

#[tauri::command(async)]
fn artists(etat: State<Etat>) -> Result<Vec<ArtistRow>, String> {
    let r = etat.lib.lock().map_err(echec)?.artists().map_err(echec);
    tracing::info!(n = r.as_ref().map(Vec::len).unwrap_or(0), "artists");
    r
}

#[tauri::command(async)]
fn albums(
    etat: State<Etat>,
    artist: Option<String>,
    mbid: Option<String>,
) -> Result<Vec<AlbumRow>, String> {
    let lib = etat.lib.lock().map_err(echec)?;
    // Identifiant et nom sont passés ensemble : le regroupement réunit les
    // pistes étiquetées MusicBrainz et celles qui ne le sont pas, l'ouverture
    // de l'artiste doit faire de même.
    match artist.as_deref() {
        Some(nom) => lib.albums_of_artist(mbid.as_deref(), nom).map_err(echec),
        None => lib.albums(None).map_err(echec),
    }
}

#[tauri::command(async)]
fn tracks_of_album(
    etat: State<Etat>,
    album: String,
    artist: Option<String>,
) -> Result<Vec<TrackRow>, String> {
    tracing::info!(%album, "tracks_of_album");
    etat.lib
        .lock()
        .map_err(echec)?
        .tracks_of_album(&album, artist.as_deref())
        .map_err(echec)
}

#[tauri::command(async)]
fn search(etat: State<Etat>, query: String, limit: Option<i64>) -> Result<Vec<TrackRow>, String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .search(&query, limit.unwrap_or(50))
        .map_err(echec)
}

#[tauri::command(async)]
fn roots(etat: State<Etat>) -> Result<Vec<RootRow>, String> {
    etat.lib.lock().map_err(echec)?.roots().map_err(echec)
}

/// La carte du module 2 : tous les morceaux déjà placés.
///
/// Chargée d'un bloc — 27 000 points tiennent largement, et l'interface les
/// redemande seulement quand l'utilisateur le veut. Tant que l'analyse tourne,
/// chaque rappel en ramène davantage.
#[tauri::command(async)]
fn map_view(etat: State<Etat>) -> Result<Vec<MapPoint>, String> {
    let pts = etat
        .lib
        .lock()
        .map_err(echec)?
        .map_view(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;
    tracing::info!(n = pts.len(), "map_view");
    Ok(pts)
}

/// Trace un chemin et rend les pistes traversées, dans l'ordre du trajet.
///
/// `mode` choisit la fabrique — `chemin.rs` documente ce qui les distingue :
///
/// - `direct` : de `from` à `to`, en droite sur la carte, en `steps` morceaux ;
/// - `lisse` : de `from` à `to`, de voisin en voisin, longueur libre plafonnée
///   à `steps` ;
/// - `errance` : depuis `from` seul, `steps` morceaux tirés au sort.
///
/// Un mode inconnu retombe sur `direct` plutôt que d'échouer : l'interface est
/// la seule à appeler, une faute de frappe y est un bogue à voir, pas une
/// erreur à afficher.
#[tauri::command(async)]
fn path(
    etat: State<Etat>,
    from: i64,
    to: Option<i64>,
    mode: Option<String>,
    steps: usize,
    seed: Option<u64>,
) -> Result<Vec<TrackRow>, String> {
    use rusty_music_analysis::chemin;
    let mode = mode.unwrap_or_else(|| "direct".into());
    let debut = std::time::Instant::now();

    // Le direct raisonne sur la carte, les deux autres sur les empreintes : on
    // ne charge que ce que le mode demande. Les empreintes font 55 Mo.
    let route = match mode.as_str() {
        "errance" => {
            let vecteurs = charger_vecteurs(&etat)?;
            construire_graphe(&etat, &vecteurs)?.errance(from, steps, seed.unwrap_or(1))
        }
        "lisse" => {
            let arrivee = to.ok_or("le mode lisse demande une arrivée")?;
            let vecteurs = charger_vecteurs(&etat)?;
            let complet = construire_graphe(&etat, &vecteurs)?.lisse(from, arrivee);
            if complet.is_empty() {
                // Les deux morceaux ne communiquent pas dans le graphe : mieux
                // vaut la droite que rien du tout. Le journal le dit,
                // l'interface l'annonce.
                tracing::info!(from, arrivee, "lisse impossible, repli sur direct");
                chemin::direct(&points_de_carte(&etat)?, from, arrivee, steps)
            } else {
                chemin::echantillonner(&complet, steps.max(2))
            }
        }
        _ => {
            let arrivee = to.ok_or("le mode direct demande une arrivée")?;
            chemin::direct(&points_de_carte(&etat)?, from, arrivee, steps)
        }
    };

    let pistes = pistes_de(&etat, &route)?;
    tracing::info!(
        mode,
        n = pistes.len(),
        ms = debut.elapsed().as_millis(),
        "chemin tracé"
    );
    Ok(pistes)
}

/// Chemin suivant un tracé dessiné à la souris sur la carte.
///
/// `trace` est la suite des points parcourus, dans le repère de la carte
/// (`[-1, 1]` sur les deux axes) ; `radius` la distance au-delà de laquelle on
/// ne cueille rien. L'interface la calcule depuis le zoom courant, pour que le
/// trait attrape exactement ce qu'il touche à l'écran.
#[tauri::command(async)]
fn path_drawn(
    etat: State<Etat>,
    trace: Vec<(f32, f32)>,
    steps: usize,
    radius: f32,
) -> Result<Vec<TrackRow>, String> {
    let points = points_de_carte(&etat)?;
    let route = rusty_music_analysis::chemin::dessine(&points, &trace, steps, radius);
    let pistes = pistes_de(&etat, &route)?;
    tracing::info!(n = pistes.len(), points = trace.len(), "chemin dessiné");
    Ok(pistes)
}

/// Les coordonnées de carte de tous les morceaux placés.
///
/// Partagé par les deux modes qui raisonnent à l'écran, `direct` et `dessine`.
/// Bien plus léger que les empreintes — deux flottants par morceau contre 512.
fn points_de_carte(etat: &State<Etat>) -> Result<Vec<(i64, f32, f32)>, String> {
    let lib = etat.lib.lock().map_err(echec)?;
    Ok(lib
        .map_points(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?
        .into_iter()
        .map(|(id, x, y, _famille)| (id, x, y))
        .collect())
}

/// Hydrate une suite d'identifiants en pistes complètes, dans le même ordre.
fn pistes_de(etat: &State<Etat>, route: &[i64]) -> Result<Vec<TrackRow>, String> {
    let lib = etat.lib.lock().map_err(echec)?;
    let mut pistes = Vec::with_capacity(route.len());
    for id in route {
        if let Some(t) = lib.track(*id).map_err(echec)? {
            pistes.push(t);
        }
    }
    Ok(pistes)
}

/// Construit le graphe des voisins si le nombre d'empreintes a changé.
///
/// Le balayage est complet — une dizaine de secondes sur la bibliothèque
/// entière. Tout se fait donc verrou relâché : ni la base ni le cache des
/// empreintes ne sont tenus pendant ce temps, et l'interface reste servie.
/// L'appelant qui veut éviter l'attente appelle `prepare_graph` en avance.
fn construire_graphe(etat: &State<Etat>, vecteurs: &[Empreinte]) -> Result<Arc<Graphe>, String> {
    let n = vecteurs.len();
    {
        let cache = etat.graphe.lock().map_err(echec)?;
        if let Some((taille, g)) = cache.as_ref() {
            if *taille == n {
                return Ok(Arc::clone(g));
            }
        }
    }

    let debut = std::time::Instant::now();
    let neuf = Arc::new(Graphe::construire(
        vecteurs,
        rusty_music_analysis::chemin::K_VOISINS,
        std::thread::available_parallelism().map_or(4, |p| p.get()),
    ));
    tracing::info!(n, ms = debut.elapsed().as_millis(), "graphe des voisins");
    *etat.graphe.lock().map_err(echec)? = Some((n, Arc::clone(&neuf)));
    Ok(neuf)
}

/// Prépare le graphe des voisins sans rien tracer.
///
/// L'interface l'appelle en entrant dans le mode Explorer : le balayage a
/// alors le temps de tourner pendant que l'utilisateur regarde la carte,
/// plutôt que de le faire attendre au premier chemin demandé. Rend le nombre
/// de morceaux couverts.
#[tauri::command(async)]
fn prepare_graph(etat: State<Etat>) -> Result<usize, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    Ok(construire_graphe(&etat, &vecteurs)?.taille())
}

/// Les familles de la carte, nommées et comptées.
#[tauri::command(async)]
fn families(etat: State<Etat>) -> Result<Vec<(i64, String, i64)>, String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .familles(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// Les morceaux d'une zone dessinée sur la carte.
///
/// `trace` est le contour, en coordonnées de carte. Les morceaux retenus sont
/// rendus **ordonnés en parcours de proche en proche** et non dans l'ordre de
/// la base : une zone donne des dizaines de morceaux, et les enchaîner au
/// hasard produirait une playlist qui saute d'un bout à l'autre.
#[tauri::command(async)]
fn selection(etat: State<Etat>, trace: Vec<(f32, f32)>) -> Result<Vec<TrackRow>, String> {
    if trace.len() < 3 {
        return Ok(Vec::new());
    }
    let points: Vec<(i64, f32, f32)> = {
        let lib = etat.lib.lock().map_err(echec)?;
        lib.map_points(rusty_music_analysis::passe::MODELE)
            .map_err(echec)?
            .into_iter()
            .map(|(id, x, y, _)| (id, x, y))
            .collect()
    };

    let dedans: Vec<i64> = points
        .iter()
        .filter(|(_, x, y)| dans_le_contour(&trace, *x, *y))
        .map(|(id, _, _)| *id)
        .collect();

    let vecteurs = charger_vecteurs(&etat)?;
    let ordonnes = rusty_music_analysis::chemin::parcours(&vecteurs, &dedans);
    tracing::info!(n = ordonnes.len(), "sélection au lasso");
    pistes_de(&etat, &ordonnes)
}

/// Un point est-il dans le contour ? Lancer de rayon, règle pair-impair.
///
/// On compte les côtés qu'une demi-droite partant du point traverse : un
/// nombre impair veut dire dedans. Tient les contours concaves, ce qu'un
/// lasso tracé à la main est presque toujours.
fn dans_le_contour(contour: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut dedans = false;
    let mut j = contour.len() - 1;
    for i in 0..contour.len() {
        let (xi, yi) = contour[i];
        let (xj, yj) = contour[j];
        // Le côté enjambe-t-il l'ordonnée du point ? La comparaison est
        // volontairement asymétrique : sans cela, un sommet exactement à la
        // hauteur du point serait compté deux fois.
        if (yi > y) != (yj > y) {
            let traverse = xi + (y - yi) / (yj - yi) * (xj - xi);
            if x < traverse {
                dedans = !dedans;
            }
        }
        j = i;
    }
    dedans
}

/// Les morceaux qui sonnent le plus comme celui-ci.
///
/// L'inspecteur du modèle « Atelier » les liste sous le morceau courant. Le
/// voisinage se calcule sur les empreintes, jamais sur la carte : t-SNE
/// déforme les distances qu'on cherche justement à comparer.
#[tauri::command(async)]
fn neighbours(etat: State<Etat>, id: i64, count: usize) -> Result<Vec<TrackRow>, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let proches = rusty_music_analysis::chemin::voisins(&vecteurs, id, count);
    pistes_de(&etat, &proches)
}

/// Charge les empreintes si leur nombre a changé depuis la dernière fois.
///
/// Elles pèsent 55 Mo sur la bibliothèque complète : on ne les relit pas à
/// chaque requête, mais leur nombre augmente tant que l'analyse tourne.
fn charger_vecteurs(etat: &State<Etat>) -> Result<Arc<Vec<Empreinte>>, String> {
    let lib = etat.lib.lock().map_err(echec)?;
    let mut cache = etat.vecteurs.lock().map_err(echec)?;
    // Compter les empreintes, pas les points de la carte : ces derniers
    // excluent les morceaux pas encore projetés, et pendant une analyse les
    // deux nombres diffèrent en permanence — le cache se croirait alors
    // périmé à chaque appel, et le graphe se reconstruirait à chaque chemin.
    let n = lib
        .count_embeddings(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;
    if cache.len() != n {
        *cache = Arc::new(
            lib.embeddings(rusty_music_analysis::passe::MODELE)
                .map_err(echec)?,
        );
        tracing::info!(n = cache.len(), "empreintes chargées");
    }
    Ok(Arc::clone(&cache))
}

/// Remonte une erreur de l'interface dans le journal du processus.
///
/// Sans cela, une exception JavaScript reste dans la console de la vue web,
/// invisible depuis le terminal : le symptôme se voit à l'écran, la cause
/// nulle part. Plusieurs pannes de cette session sont restées obscures faute
/// de ce fil.
#[tauri::command(async)]
fn js_error(message: String, source: Option<String>) {
    tracing::error!(source = source.unwrap_or_default(), "interface : {message}");
}

/// Combien de morceaux restent à analyser — pour dire où en est la carte.
#[tauri::command(async)]
fn map_progress(etat: State<Etat>) -> Result<(i64, i64), String> {
    let lib = etat.lib.lock().map_err(echec)?;
    let total = lib.count().map_err(echec)?;
    let restants = lib
        .pending_analysis(rusty_music_analysis::passe::MODELE, i64::MAX)
        .map_err(echec)?
        .len() as i64;
    Ok((total - restants, total))
}

/// Avancement de l'analyse en cours, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
struct EtatAnalyse {
    en_cours: bool,
    faits: usize,
    total: usize,
    resultat: Option<String>,
}

#[derive(Default, Clone, serde::Serialize)]
struct EtatEnrichissement {
    en_cours: bool,
    artistes: usize,
    total: usize,
    avec_genre: usize,
    resultat: Option<String>,
}

/// Lance le calcul des empreintes des morceaux en attente.
///
/// Déclenché à la main, et pas enchaîné au scan : sur une bibliothèque neuve
/// de cette taille la passe dure des heures, et l'engager sans que
/// l'utilisateur l'ait demandé serait le prendre en otage. L'interface affiche
/// le nombre en attente et le laisse choisir son moment.
///
/// Ouvre sa propre connexion, comme le scan : sous le verrou de la base
/// partagée, l'interface serait figée pendant toute la passe. Le mode WAL
/// autorise un rédacteur et des lecteurs simultanés.
#[tauri::command(async)]
fn start_analysis(app: tauri::AppHandle, etat: State<Etat>) -> Result<(), String> {
    {
        let mut a = etat.analyse.lock().map_err(echec)?;
        if a.en_cours {
            return Err("une analyse est déjà en cours".into());
        }
        *a = EtatAnalyse {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let fils = std::thread::available_parallelism().map_or(4, |p| p.get());

        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|lib| {
                rusty_music_analysis::passe::empreintes(
                    &lib,
                    None,
                    i64::MAX,
                    fils,
                    |faits, total| {
                        if let Ok(mut a) = etat.analyse.lock() {
                            a.faits = faits;
                            a.total = total;
                        }
                    },
                )
                .map_err(|e| e.to_string())
                // La projection doit suivre : sans elle les empreintes existent
                // mais aucun point neuf n'apparaît sur la carte. t-SNE ne
                // s'incrémente pas — il replace l'ensemble, en une trentaine de
                // secondes.
                .and_then(|r| {
                    rusty_music_analysis::passe::projeter_tout(&lib, 12)
                        .map(|p| (r, p))
                        .map_err(|e| e.to_string())
                })
            });

        let bilan = match issue {
            Ok((r, p)) => format!(
                "{} empreintes · {} en échec · {} points sur la carte",
                r.empreintes, r.echecs, p.empreintes
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "analyse terminée");

        // Garde nommé : dans un `if let`, son temporaire vivrait plus
        // longtemps que le `State` dont il emprunte. Même piège que le scan.
        let verrou = etat.analyse.lock();
        if let Ok(mut a) = verrou {
            a.en_cours = false;
            a.resultat = Some(bilan);
        }
    });

    Ok(())
}

/// Avancement de l'analyse. L'interface sonde, comme pour le scan.
#[tauri::command(async)]
fn analysis_state(etat: State<Etat>) -> Result<EtatAnalyse, String> {
    Ok(etat.analyse.lock().map_err(echec)?.clone())
}

/// Lance l'aspiration des genres MusicBrainz.
///
/// Comme l'analyse : déclenchée à la main, sur son propre fil, avec sa propre
/// connexion. Elle dure environ deux heures sur une bibliothèque de
/// vingt-sept mille morceaux — c'est MusicBrainz qui impose le rythme, une
/// requête par seconde, et non nous qui traînons.
///
/// **Reprenable et additive.** L'interrompre ne perd rien, la relancer ne
/// refait rien, et ne jamais la lancer laisse les familles nommées par les
/// tags des fichiers. Rien n'en dépend.
#[tauri::command(async)]
fn start_enrichment(
    app: tauri::AppHandle,
    etat: State<Etat>,
    contact: String,
) -> Result<(), String> {
    let contact = contact.trim().to_string();
    if !contact.contains('@') {
        // MusicBrainz refuse les clients anonymes : leur documentation exige un
        // contact joignable dans l'agent. Mieux vaut le dire ici que récolter
        // des refus deux heures durant.
        return Err("MusicBrainz demande une adresse de contact valable.".into());
    }
    {
        let mut e = etat.enrichissement.lock().map_err(echec)?;
        if e.en_cours {
            return Err("un enrichissement est déjà en cours".into());
        }
        *e = EtatEnrichissement {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let client = rusty_music_core::musicbrainz::Client::new(&contact);
        // Ouverture et passe dans un même bloc plutôt qu'en chaîne : la
        // fermeture de progression emprunte `etat`, et l'imbriquer dans un
        // `and_then` ferait vivre cet emprunt au-delà du fil.
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            let total = lib.mb_avancement().map(|(_, t, _)| t).unwrap_or(0) as usize;
            if let Ok(mut e) = etat.enrichissement.lock() {
                e.total = total;
            }
            rusty_music_core::enrichir::enrichir(&mut lib, &client, usize::MAX, |b| {
                if let Ok(mut e) = etat.enrichissement.lock() {
                    e.artistes = b.artistes;
                    e.avec_genre = b.avec_genre;
                }
            })
            .map_err(|e| e.to_string())
        })();

        let bilan = match issue {
            Ok(b) => format!(
                "{} artistes interrogés, {} avec un genre, {} albums",
                b.artistes, b.avec_genre, b.albums
            ),
            Err(e) => format!("échec : {e}"),
        };
        // Liaison nommée plutôt que verrou pris dans le `if let` : en queue de
        // fermeture, le temporaire d'un `if let` vit jusqu'à la fin du bloc et
        // survivrait donc à l'emprunt d'état dont il sort. Les liaisons, elles,
        // se libèrent dans l'ordre inverse de leur déclaration.
        let mut fin = etat.enrichissement.lock();
        if let Ok(e) = fin.as_mut() {
            e.en_cours = false;
            e.resultat = Some(bilan);
        }
    });
    Ok(())
}

/// Où en est l'aspiration des genres.
#[tauri::command(async)]
fn enrichment_state(etat: State<Etat>) -> Result<EtatEnrichissement, String> {
    Ok(etat.enrichissement.lock().map_err(echec)?.clone())
}

// ---------------------------------------------------------------------------
// Mode Éditer : démixage (module 3)
// ---------------------------------------------------------------------------

/// Avancement du démixage, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
struct EtatDemix {
    en_cours: bool,
    /// Le morceau en cours de traitement, pour que l'interface sache si les
    /// stems affichés sont bien les siens.
    source: String,
    /// Stems produits : nom et chemin du WAV.
    stems: Vec<(String, String)>,
    resultat: Option<String>,
}

/// Où déposer les stems : à côté de la base, dans un sous-dossier par morceau.
///
/// Pas à côté du morceau d'origine : la bibliothèque est en lecture seule par
/// principe — c'est une source, pas un espace de travail — et elle vit ici sur
/// une carte SD lente.
fn dossier_stems(etat: &State<Etat>, source: &Path) -> PathBuf {
    let base = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "morceau".into());
    racine_stems(etat).join(base)
}

/// Racine du cache de stems, à côté de la base.
fn racine_stems(etat: &State<Etat>) -> PathBuf {
    etat.db.parent().unwrap_or(Path::new(".")).join("stems")
}

/// Vitesse de lecture des stems, appliquée immédiatement.
///
/// **Rien n'est recalculé ni rechargé** : la lecture avance d'un pas
/// fractionnaire qu'on écrit ici, et la position ne bouge pas. C'est la
/// différence avec `stems_etirer`, qui retraite le signal et coûte des
/// secondes. La contrepartie est que la hauteur suit la vitesse, comme sur une
/// bande.
#[tauri::command(async)]
fn stems_vitesse(etat: State<Etat>, vitesse: f32) -> Result<(), String> {
    if let Some(m) = etat.stems.lock().map_err(echec)?.as_ref() {
        m.vitesse(vitesse);
    }
    Ok(())
}

/// Vitesse d'un seul stem — le réglage qui désynchronise.
///
/// **C'est un effet, pas un réglage**, et la spec le dit depuis sa décision 4 :
/// deux stems à des vitesses différentes ont deux têtes de lecture, l'écart
/// grandit tant que la lecture continue, et rien ne le rattrape tout seul.
/// L'interface a donc deux devoirs qu'elle n'a pas ailleurs : montrer la
/// dérive mesurée (`stems_state`) et offrir de réaligner.
///
/// Aussi immédiat que la vitesse globale : un flottant que la lecture relit.
#[tauri::command(async)]
fn stems_vitesse_stem(etat: State<Etat>, index: usize, vitesse: f32) -> Result<(), String> {
    if let Some(m) = etat.stems.lock().map_err(echec)?.as_ref() {
        m.vitesse_stem(index, vitesse);
    }
    Ok(())
}

/// Applique une transposition à un jeu de stems, et rend les chemins traités.
///
/// **Une valeur par stem.** Transposer la basse d'une quinte sans toucher au
/// reste est un geste courant ; l'exiger sur les quatre à la fois ne l'était
/// pas. Contrairement à la vitesse, cela ne désynchronise rien : la
/// transposition rend la durée qu'elle a reçue.
///
/// **Le résultat est mis en cache sur le disque**, un dossier par valeur et par
/// stem. Revenir à un réglage déjà calculé est alors immédiat, l'aller-retour
/// entre deux valeurs — le geste naturel quand on cherche la bonne — ne
/// recalcule rien, et **changer la hauteur d'un seul stem ne recalcule que
/// celui-là**. Le sous-dossier reste invisible de `stems_existants`, qui ne lit
/// que le premier niveau.
///
/// Un stem à zéro demi-ton rend son chemin d'origine : il n'y a rien à
/// calculer, et écrire une copie identique serait 31 Mo pour rien.
#[tauri::command(async)]
fn stems_etirer(
    stems: Vec<(String, String)>,
    demi_tons: Vec<f32>,
) -> Result<Vec<(String, String)>, String> {
    use rusty_music_editor::{decode, etirement, wav};

    let mut sortie = Vec::with_capacity(stems.len());
    for (i, (nom, chemin)) in stems.iter().enumerate() {
        let demi_ton = demi_tons.get(i).copied().unwrap_or(0.0);
        if demi_ton.abs() < 1e-3 {
            sortie.push((nom.clone(), chemin.clone()));
            continue;
        }
        let source = PathBuf::from(chemin);
        let dossier = source
            .parent()
            .unwrap_or(Path::new("."))
            .join("traites")
            .join(format!("t{:+03}", demi_ton as i32));
        let cible = dossier.join(source.file_name().unwrap_or_default());
        if !cible.exists() {
            std::fs::create_dir_all(&dossier).map_err(echec)?;
            let s = decode::stereo(&source).map_err(echec)?;
            let entrelace: Vec<f32> = s
                .gauche
                .iter()
                .zip(&s.droite)
                .flat_map(|(g, d)| [*g, *d])
                .collect();
            let out = etirement::transposer(&entrelace, 2, demi_ton);
            let (g, d): (Vec<f32>, Vec<f32>) = out.chunks_exact(2).map(|c| (c[0], c[1])).unzip();
            wav::ecrire(&cible, &g, &d, 44_100).map_err(echec)?;
            tracing::info!(stem = %nom, demi_ton, "stem transposé");
        }
        sortie.push((nom.clone(), cible.display().to_string()));
    }
    Ok(sortie)
}

/// La racine surveillée qui contient `dossier`, s'il y en a une.
///
/// Comparaison composant par composant, jamais sur les chaînes : `/Musique2`
/// n'est pas dans `/Musique`, alors qu'un `starts_with` textuel le dirait.
fn sous_une_racine(dossier: &Path, racines: &[String]) -> Option<String> {
    racines
        .iter()
        .find(|r| dossier.starts_with(Path::new(r)))
        .cloned()
}

/// Exporte ce qu'on entend : le mélange des stems audibles, avec leurs niveaux,
/// leur vitesse et leur transposition.
///
/// **Une seule sortie, et c'est un choix.** La spec en prévoyait trois — un
/// stem, la sélection, le mélange. Mettre un stem en solo *est* la sélection :
/// un menu de plus n'aurait dit que ce que le dock montre déjà.
///
/// **Refuse d'écrire dans la bibliothèque surveillée.** Un fichier déposé sous
/// une racine serait ingéré, analysé et placé sur la carte : ce n'est pas un
/// morceau, c'est un rendu. Mieux vaut le dire tout de suite que laisser la
/// surprise arriver à la prochaine surveillance.
#[tauri::command(async)]
fn stems_exporter(
    etat: State<Etat>,
    stems: Vec<(String, String)>,
    niveaux: Vec<f32>,
    vitesses: Vec<f32>,
    demi_tons: Vec<f32>,
    destination: String,
    nom: String,
) -> Result<String, String> {
    use rusty_music_editor::{decode, etirement, wav};

    if stems.is_empty() {
        return Err("aucun stem à exporter".into());
    }
    let dossier = PathBuf::from(&destination);
    {
        let lib = etat.lib.lock().map_err(echec)?;
        let racines: Vec<String> = lib
            .roots()
            .map_err(echec)?
            .into_iter()
            .map(|r| r.path)
            .collect();
        if let Some(racine) = sous_une_racine(&dossier, &racines) {
            return Err(format!(
                "{} est dans la bibliothèque surveillée ({racine}). Un rendu y \
                 serait ingéré comme un morceau — choisis un autre dossier.",
                dossier.display()
            ));
        }
    }

    // Somme des stems audibles, à leur niveau. Un niveau nul — coupé, ou non
    // retenu par un solo — ne contribue pas et n'a même pas besoin d'être lu.
    //
    // **Chaque stem est traité avant d'être sommé, et non l'inverse.** Tant que
    // la vitesse était globale, étirer le mélange revenait au même et coûtait
    // un étirement au lieu de quatre. Avec une vitesse par stem, ce n'est plus
    // vrai : c'est justement le décalage entre les stems qu'il faut rendre, et
    // il n'existe pas dans leur somme.
    let mut melange: Vec<f32> = Vec::new();
    for (i, (_, chemin)) in stems.iter().enumerate() {
        let niveau = niveaux.get(i).copied().unwrap_or(1.0);
        if niveau <= 1e-4 {
            continue;
        }
        let s = decode::stereo(Path::new(chemin)).map_err(echec)?;
        let mut piste: Vec<f32> = s
            .gauche
            .iter()
            .zip(&s.droite)
            .flat_map(|(g, d)| [*g * niveau, *d * niveau])
            .collect();

        // La vitesse est un tempo à la lecture ; à l'écrit, c'est une durée.
        let vitesse = vitesses.get(i).copied().unwrap_or(1.0);
        if (vitesse - 1.0).abs() > 1e-3 {
            piste = etirement::etirer(&piste, 2, 1.0 / vitesse);
        }
        let demi_ton = demi_tons.get(i).copied().unwrap_or(0.0);
        if demi_ton.abs() > 1e-3 {
            piste = etirement::transposer(&piste, 2, demi_ton);
        }

        // Les stems n'ont plus la même longueur dès que leurs vitesses
        // diffèrent : le plus long commande, les autres se taisent avant.
        if melange.len() < piste.len() {
            melange.resize(piste.len(), 0.0);
        }
        for (d, v) in melange.iter_mut().zip(&piste) {
            *d += v;
        }
    }
    if melange.is_empty() {
        return Err("tout est coupé : il n'y a rien à exporter".into());
    }

    std::fs::create_dir_all(&dossier).map_err(echec)?;
    let cible = dossier.join(format!("{nom}.wav"));
    // La somme des stems reconstitue un mélange qui frôlait déjà la pleine
    // échelle : sans écrêtage, un solo à plein niveau saturerait.
    let (g, d): (Vec<f32>, Vec<f32>) = melange
        .chunks_exact(2)
        .map(|c| (c[0].clamp(-1.0, 1.0), c[1].clamp(-1.0, 1.0)))
        .unzip();
    wav::ecrire(&cible, &g, &d, 44_100).map_err(echec)?;
    tracing::info!(fichier = %cible.display(), "export écrit");
    Ok(cible.display().to_string())
}

/// Poids d'un dossier, sous-dossiers compris.
///
/// La récursion n'est pas gratuite ici : les versions étirées et transposées
/// vivent un niveau plus bas, et chacune pèse autant que l'original. Les
/// ignorer sous-estimerait le cache d'un facteur quelconque.
fn poids(dossier: &Path) -> u64 {
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return 0;
    };
    entrees
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                poids(&p)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Taille du cache de stems et nombre de morceaux séparés.
///
/// **Un jeu de quatre stems pèse 124 Mo** en WAV — quinze morceaux séparés font
/// deux gigaoctets. Sans ce compte, c'est une fuite qu'on ne découvre qu'en
/// cherchant pourquoi le disque se remplit (`docs/ui-spec-editeur.md`).
#[tauri::command(async)]
fn stems_cache(etat: State<Etat>) -> Result<(u64, usize), String> {
    let racine = racine_stems(&etat);
    let Ok(entrees) = std::fs::read_dir(&racine) else {
        return Ok((0, 0));
    };
    let mut octets = 0u64;
    let mut morceaux = 0usize;
    for morceau in entrees.flatten() {
        if !morceau.path().is_dir() {
            continue;
        }
        morceaux += 1;
        octets += poids(&morceau.path());
    }
    Ok((octets, morceaux))
}

/// Vide le cache de stems.
///
/// Coupe d'abord la lecture multipiste : effacer les fichiers d'un jeu en train
/// de jouer laisserait la sortie audio sur des données disparues.
#[tauri::command(async)]
fn stems_cache_vider(etat: State<Etat>) -> Result<(), String> {
    *etat.stems.lock().map_err(echec)? = None;
    let racine = racine_stems(&etat);
    if racine.is_dir() {
        std::fs::remove_dir_all(&racine).map_err(echec)?;
    }
    tracing::info!(dossier = %racine.display(), "cache de stems vidé");
    Ok(())
}

/// Lance le démixage d'un morceau.
///
/// Tourne dans son fil : une séparation demande une trentaine de secondes pour
/// un morceau de quatre minutes, et le premier lancement y ajoute la
/// compilation des noyaux GPU.
#[tauri::command(async)]
fn start_demix(
    app: tauri::AppHandle,
    etat: State<Etat>,
    path: String,
    variant: String,
) -> Result<(), String> {
    let source = PathBuf::from(&path);
    if !source.is_file() {
        return Err(format!("{path} est introuvable"));
    }
    let variante = rusty_music_editor::Variante::analyser(&variant)
        .ok_or_else(|| format!("variante inconnue : {variant}"))?;
    {
        let mut d = etat.demix.lock().map_err(echec)?;
        if d.en_cours {
            return Err("un démixage est déjà en cours".into());
        }
        *d = EtatDemix {
            en_cours: true,
            source: path.clone(),
            ..Default::default()
        };
    }

    let sortie = dossier_stems(&etat, &source);
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let issue = rusty_music_editor::Demixeur::charger(None, variante).and_then(|d| {
            d.chauffer();
            d.separer_fichier(&source, &sortie)
        });

        let (stems, bilan) = match issue {
            Ok(fichiers) => {
                let noms: Vec<(String, String)> = fichiers
                    .iter()
                    .map(|f| {
                        let nom = f
                            .file_stem()
                            .map(|s| {
                                s.to_string_lossy()
                                    .rsplit('—')
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_string()
                            })
                            .unwrap_or_default();
                        (nom, f.display().to_string())
                    })
                    .collect();
                let n = noms.len();
                (noms, format!("{n} stems écrits"))
            }
            Err(e) => (Vec::new(), format!("échec : {e}")),
        };
        tracing::info!(%bilan, "démixage terminé");

        let verrou = etat.demix.lock();
        if let Ok(mut d) = verrou {
            d.en_cours = false;
            d.stems = stems;
            d.resultat = Some(bilan);
        }
    });

    Ok(())
}

/// Avancement du démixage. L'interface sonde, faute de rapport intermédiaire.
#[tauri::command(async)]
fn demix_state(etat: State<Etat>) -> Result<EtatDemix, String> {
    Ok(etat.demix.lock().map_err(echec)?.clone())
}

/// Charge un jeu de stems et les met en lecture simultanée.
///
/// Remplace le multipiste précédent s'il y en avait un : deux jeux de stems à
/// la fois n'auraient aucun sens, et chacun tient une sortie audio.
#[tauri::command(async)]
fn stems_play(etat: State<Etat>, stems: Vec<(String, String)>) -> Result<Vec<String>, String> {
    let pistes: Vec<(String, PathBuf)> = stems
        .into_iter()
        .map(|(nom, chemin)| (nom, PathBuf::from(chemin)))
        .collect();
    let multi = rusty_music_player::Multipiste::charger(&pistes).map_err(echec)?;
    let noms = multi.noms().to_vec();

    // Le lecteur du module 1 se tait : deux sorties audio superposées, ce
    // serait le morceau d'origine par-dessus ses propres stems.
    if let Ok(p) = etat.player.lock() {
        p.pause();
    }
    *etat.stems.lock().map_err(echec)? = Some(multi);
    tracing::info!(n = noms.len(), "stems en lecture");
    Ok(noms)
}

/// Règle le niveau d'une piste — c'est ce que font solo et coupure.
#[tauri::command(async)]
fn stems_gain(etat: State<Etat>, levels: Vec<f32>) -> Result<(), String> {
    let garde = etat.stems.lock().map_err(echec)?;
    if let Some(m) = garde.as_ref() {
        for (i, n) in levels.iter().enumerate() {
            m.regler(i, *n);
        }
    }
    Ok(())
}

/// Transport du multipiste : pause, reprise, déplacement.
#[tauri::command(async)]
fn stems_transport(etat: State<Etat>, action: String, position: Option<f64>) -> Result<(), String> {
    let garde = etat.stems.lock().map_err(echec)?;
    let Some(m) = garde.as_ref() else {
        return Ok(());
    };
    match action.as_str() {
        "pause" => m.pause(),
        "reprendre" => m.reprendre(),
        "deplacer" => m.deplacer(std::time::Duration::from_secs_f64(position.unwrap_or(0.0))),
        // Réaligner ne touche pas aux vitesses : on garde l'effet, on efface
        // l'écart déjà pris. Les deux gestes sont distincts, et il faut les
        // deux — égaliser les vitesses arrête la dérive mais laisse le décalage.
        "realigner" => m.realigner(),
        "arreter" => {
            drop(garde);
            *etat.stems.lock().map_err(echec)? = None;
        }
        _ => return Err(format!("action inconnue : {action}")),
    }
    Ok(())
}

/// État du multipiste, sondé par l'interface pour animer sa barre.
#[derive(Clone, Default, serde::Serialize)]
struct EtatStems {
    actif: bool,
    en_pause: bool,
    position_ms: u64,
    duree_ms: u64,
    niveaux: Vec<f32>,
    /// Vitesse de chaque stem. L'interface les tient déjà, mais c'est le
    /// moteur qui les borne : les relire évite d'afficher une valeur que la
    /// lecture n'applique pas.
    vitesses: Vec<f32>,
    /// De combien le stem le plus éloigné s'est écarté de la référence.
    /// **Mesuré, pas prédit** — « les stems ont dérivé de 1,4 s » se vérifie à
    /// l'oreille, « ils peuvent se désynchroniser » ne dit rien.
    derive_ms: u64,
}

#[tauri::command(async)]
fn stems_state(etat: State<Etat>) -> Result<EtatStems, String> {
    let garde = etat.stems.lock().map_err(echec)?;
    Ok(match garde.as_ref() {
        Some(m) => EtatStems {
            actif: !m.fini(),
            en_pause: m.en_pause(),
            position_ms: m.position().as_millis() as u64,
            duree_ms: m.duree().as_millis() as u64,
            niveaux: m.niveaux(),
            vitesses: m.vitesses(),
            derive_ms: m.derive().as_millis() as u64,
        },
        None => EtatStems::default(),
    })
}

/// Spectrogramme d'un stem, en intensités sur un octet.
///
/// La coloration appartient à l'interface : elle applique la rampe
/// séquentielle déjà retenue pour la carte, plutôt qu'une palette importée.
#[tauri::command(async)]
fn stem_spectre(path: String, width: usize, height: usize) -> Result<SpectreVue, String> {
    let s =
        rusty_music_player::spectre::calculer(Path::new(&path), width, height).map_err(echec)?;
    Ok(SpectreVue {
        largeur: s.largeur,
        hauteur: s.hauteur,
        pixels: s.pixels,
    })
}

#[derive(serde::Serialize)]
struct SpectreVue {
    largeur: usize,
    hauteur: usize,
    pixels: Vec<u8>,
}

/// Les WAV de stems d'un dossier, avec le nom de leur stem.
///
/// Le nom est ce qui suit le dernier tiret cadratin : `Die Oros — drums.wav`
/// donne `drums`. **Le premier niveau seulement** — `traites/` porte les
/// versions transposées et `greffes/` les stems venus d'ailleurs, qui sont des
/// dérivés du morceau, pas des stems de plus.
fn stems_du_dossier(dossier: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return Vec::new();
    };
    let mut trouves: Vec<(String, PathBuf)> = entrees
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .map(|p| {
            let nom = p
                .file_stem()
                .map(|s| {
                    s.to_string_lossy()
                        .rsplit('—')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();
            (nom, p)
        })
        .collect();
    trouves.sort();
    trouves
}

/// Les stems déjà calculés pour un morceau, s'il y en a.
///
/// Permet de retrouver un démixage d'une session précédente sans le refaire —
/// une séparation coûte trente secondes, on ne la rejoue pas pour rien.
#[tauri::command(async)]
fn stems_existants(etat: State<Etat>, path: String) -> Result<Vec<(String, String)>, String> {
    let dossier = dossier_stems(&etat, Path::new(&path));
    Ok(stems_du_dossier(&dossier)
        .into_iter()
        .map(|(nom, p)| (nom, p.display().to_string()))
        .collect())
}

// ---------------------------------------------------------------------------
// Mode Éditer : greffer le stem d'un autre morceau
// ---------------------------------------------------------------------------

/// Écart de tempo au-delà duquel un morceau n'est pas proposé pour une greffe.
///
/// **Dix pour cent, et le repliement est déjà passé** : un morceau à demi-tempo
/// n'est pas écarté, il est compté pour ce qu'il est. Ce qui reste est le vrai
/// étirement à appliquer, et `wsola` y est transparent. Plus loin, deux choses
/// se dégradent ensemble — la matière étirée, et la confiance qu'on peut avoir
/// dans un tempo mesuré à 6 % près (`docs/suite.md`, 6 bis).
const TOLERANCE_TEMPO: f32 = 0.10;

/// Combien de voisins soniques on examine avant de filtrer sur le tempo.
///
/// Le filtre en écarte beaucoup : sur une bibliothèque réelle, quelques
/// dizaines de voisins ne suffisent pas à en garder une poignée.
const VOISINS_EXAMINES: usize = 400;

/// Un morceau dont on pourrait prendre un stem.
#[derive(serde::Serialize)]
struct Candidat {
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
struct VoisinsStem {
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
fn voisins_de_stem(etat: State<Etat>, id: i64, count: usize) -> Result<VoisinsStem, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let proches = rusty_music_analysis::chemin::voisins(&vecteurs, id, VOISINS_EXAMINES);
    let pistes = pistes_de(&etat, &proches)?;

    let (bpm, tempos) = {
        let lib = etat.lib.lock().map_err(echec)?;
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

/// Un stem greffé, et ce qu'il a fallu lui faire.
#[derive(serde::Serialize)]
struct Greffon {
    /// Le WAV écrit, à mettre à la place du stem remplacé.
    chemin: String,
    /// Le morceau d'où il vient, tel qu'on l'affiche.
    origine: String,
    facteur: f32,
    octaves: i32,
    retard_s: f32,
    boucles: usize,
}

/// Met à la place d'un stem celui d'un autre morceau.
///
/// **Non destructif, comme tout le module** : la greffe est un fichier de plus
/// dans le cache, sous `greffes/`, et le stem d'origine reste où il est. Rouvrir
/// le morceau retrouve ses quatre stems séparés, pas la greffe — les stems sur
/// le disque sont la seule persistance, et une greffe n'en est pas un.
#[tauri::command(async)]
fn stems_greffer(
    etat: State<Etat>,
    id: i64,
    stem: String,
    remplace: String,
    voisin: i64,
) -> Result<Greffon, String> {
    let (source, autre, tempos) = {
        let lib = etat.lib.lock().map_err(echec)?;
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

    let plan = rusty_music_editor::greffe::greffer(
        Path::new(&remplace),
        &greffon,
        tempos.get(&id).copied(),
        tempos.get(&voisin).copied(),
        &sortie,
    )
    .map_err(echec)?;

    Ok(Greffon {
        chemin: sortie.display().to_string(),
        origine,
        facteur: plan.facteur,
        octaves: plan.octaves,
        retard_s: plan.retard_s,
        boucles: plan.boucles,
    })
}

// ---------------------------------------------------------------------------
// Réglages : source de la bibliothèque
// ---------------------------------------------------------------------------

/// Oublie une racine **et les morceaux qui en dépendent**.
#[tauri::command(async)]
fn forget_root(etat: State<Etat>, path: String) -> Result<usize, String> {
    let n = etat
        .lib
        .lock()
        .map_err(echec)?
        .remove_root(Path::new(&path))
        .map_err(echec)?;
    tracing::info!(%path, morceaux = n, "racine oubliée");
    Ok(n)
}

/// Lance le scan d'une racine dans un thread dédié.
///
/// Le scan tient la base pendant des dizaines de minutes sur un support lent :
/// le faire sous le verrou de `Etat::lib` figerait toute l'interface. Le thread
/// ouvre donc sa **propre** connexion sur le même fichier — le mode WAL du
/// schéma autorise un rédacteur et des lecteurs simultanés.
#[tauri::command(async)]
fn start_scan(app: tauri::AppHandle, etat: State<Etat>, path: String) -> Result<(), String> {
    let racine = PathBuf::from(&path);
    if !racine.is_dir() {
        return Err(format!("{path} n'est pas un dossier"));
    }
    {
        let mut s = etat.scan.lock().map_err(echec)?;
        if s.en_cours {
            return Err("un scan est déjà en cours".into());
        }
        *s = EtatScan {
            en_cours: true,
            racine: path.clone(),
            morceaux: 0,
            resultat: None,
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|lib| {
                rusty_music_core::scan::scan_root(&lib, &racine).map_err(|e| e.to_string())
            });

        let bilan = match issue {
            Ok(r) => format!(
                "{} vus · {} ingérés · {} inchangés · {} retirés · {} en échec",
                r.seen, r.inserted, r.skipped, r.removed, r.failed
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "scan terminé");

        // Le garde est nommé : dans un `if let`, son temporaire vivrait plus
        // longtemps que le `State` dont il emprunte.
        let etat = app.state::<Etat>();
        let verrou = etat.scan.lock();
        if let Ok(mut s) = verrou {
            s.en_cours = false;
            s.resultat = Some(bilan);
        }
    });

    Ok(())
}

/// Avancement du scan. L'interface sonde, faute de rapport intermédiaire.
#[tauri::command(async)]
fn scan_state(etat: State<Etat>) -> Result<EtatScan, String> {
    let mut s = etat.scan.lock().map_err(echec)?.clone();
    if s.en_cours {
        // Le nombre de morceaux progresse pendant le scan : c'est la seule
        // mesure d'avancement qu'on puisse offrir sans instrumenter le cœur.
        s.morceaux = etat.lib.lock().map_err(echec)?.count().map_err(echec)?;
    }
    Ok(s)
}

/// Pochette d'un morceau, en `data:` URI directement consommable par `<img>`.
///
/// Rien n'est mis en cache ici : c'est le rôle de l'interface, qui sait ce
/// qu'elle affiche. Compter 50 à 210 ms par appel sur un support lent.
#[tauri::command(async)]
fn cover(path: String) -> Result<Option<String>, String> {
    let cover = rusty_music_core::tags::read_cover(Path::new(&path)).map_err(echec)?;
    Ok(cover.map(|c| {
        let mime = c.mime.as_deref().unwrap_or("image/jpeg");
        format!("data:{mime};base64,{}", base64(&c.data))
    }))
}

/// Enveloppe d'une piste : crête et RMS par tranche.
///
/// Renvoie `None` tant qu'elle n'est pas prête. Le calcul décode tout le
/// fichier — plusieurs secondes sur un support lent — et tourne donc dans un
/// thread : l'interface redemande et affiche l'onde quand elle arrive, plutôt
/// que de figer en attendant.
#[tauri::command(async)]
fn waveform(
    app: tauri::AppHandle,
    etat: State<Etat>,
    path: String,
    buckets: usize,
    duration_ms: Option<u64>,
) -> Result<Option<rusty_music_player::Waveform>, String> {
    let chemin = PathBuf::from(&path);
    {
        let cache = etat.ondes.lock().map_err(echec)?;
        if let Some(w) = cache.get(&chemin) {
            return Ok(Some(w.clone()));
        }
    }

    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        match rusty_music_player::waveform::compute(&chemin, buckets, duration_ms) {
            Ok(w) => {
                tracing::debug!(path = %chemin.display(), ms = t.elapsed().as_millis(), "onde calculée");
                let etat = app.state::<Etat>();
                let verrou = etat.ondes.lock();
                if let Ok(mut c) = verrou {
                    c.insert(chemin, w);
                }
            }
            Err(e) => tracing::warn!(path = %chemin.display(), error = %e, "onde incalculable"),
        }
    });

    Ok(None)
}

/// Encodage base64 sans dépendance : la seule chose qu'on ait à encoder ici,
/// ce sont des pochettes, et l'ajouter au projet pour ça ne se justifie pas.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for bloc in data.chunks(3) {
        let b = [
            bloc[0],
            *bloc.get(1).unwrap_or(&0),
            *bloc.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if bloc.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if bloc.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct EtatLecture {
    current: Option<String>,
    paused: bool,
    finished: bool,
    position_ms: u64,
    remaining: usize,
    volume: f32,
}

#[tauri::command(async)]
fn play(etat: State<Etat>, paths: Vec<String>) -> Result<(), String> {
    let chemins: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    etat.player
        .lock()
        .map_err(echec)?
        .play(&chemins)
        .map_err(echec)
}

#[tauri::command(async)]
fn toggle_pause(etat: State<Etat>) -> Result<bool, String> {
    let player = etat.player.lock().map_err(echec)?;
    if player.is_paused() {
        player.resume();
    } else {
        player.pause();
    }
    Ok(player.is_paused())
}

#[tauri::command(async)]
fn skip(etat: State<Etat>) -> Result<(), String> {
    etat.player.lock().map_err(echec)?.skip();
    Ok(())
}

#[tauri::command(async)]
fn previous(etat: State<Etat>) -> Result<(), String> {
    etat.player.lock().map_err(echec)?.previous().map_err(echec)
}

#[tauri::command(async)]
fn jump_to(etat: State<Etat>, index: usize) -> Result<(), String> {
    etat.player
        .lock()
        .map_err(echec)?
        .jump_to(index)
        .map_err(echec)
}

#[tauri::command(async)]
fn seek(etat: State<Etat>, position_ms: u64) -> Result<(), String> {
    etat.player
        .lock()
        .map_err(echec)?
        .seek(std::time::Duration::from_millis(position_ms))
        .map_err(echec)
}

#[tauri::command(async)]
fn set_volume(etat: State<Etat>, volume: f32) -> Result<(), String> {
    etat.player.lock().map_err(echec)?.set_volume(volume);
    Ok(())
}

/// Sondé par l'interface : `rodio` ne pousse pas d'évènements.
///
/// Sert aussi à réalimenter la sortie. La file n'est plus chargée d'un bloc —
/// elle immobilisait le lecteur 17 s sur un album de 157 pistes — et c'est ce
/// passage régulier qui prépare la piste suivante, un fichier à la fois.
#[tauri::command(async)]
fn playback_state(etat: State<Etat>) -> Result<EtatLecture, String> {
    let mut player = etat.player.lock().map_err(echec)?;
    if let Err(e) = player.completer() {
        // Une piste illisible ne doit pas interrompre le suivi : la lecture
        // passera simplement à la suivante.
        tracing::warn!(error = %e, "préparation de la piste suivante impossible");
    }
    Ok(EtatLecture {
        current: player.current().map(|p| p.display().to_string()),
        paused: player.is_paused(),
        finished: player.is_finished(),
        position_ms: player.position().as_millis() as u64,
        remaining: player.remaining(),
        volume: player.volume(),
    })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
                // `symphonia` commente chaque trame ID3 qu'il ne gère pas
                // (TCMP, UFID…) et chaque estimation de durée, à chaque piste
                // ouverte : des dizaines de lignes qui noient les nôtres. La
                // directive est ajoutée par-dessus RUST_LOG, pour que fixer
                // `RUST_LOG=info` ne réveille pas ce bruit.
                .add_directive("symphonia=warn".parse().expect("directive valide"))
                // `lofty` en fait autant sur les conteneurs MP4 (« Skipping
                // empty data atom ») à chaque pochette lue.
                .add_directive("lofty=error".parse().expect("directive valide")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // La base vit à côté des données de l'application, pas dans le
            // répertoire courant : une app de bureau n'a pas de « cwd » stable.
            let dossier = app.path().app_data_dir()?;
            let db = dossier.join("rusty-music.db");
            tracing::info!(base = %db.display(), "ouverture de la bibliothèque");

            app.manage(Etat {
                lib: Mutex::new(Library::open(&db)?),
                player: Mutex::new(Player::new()?),
                db,
                scan: Mutex::new(EtatScan::default()),
                analyse: Mutex::new(EtatAnalyse::default()),
                enrichissement: Mutex::new(EtatEnrichissement::default()),
                demix: Mutex::new(EtatDemix::default()),
                stems: Mutex::new(None),
                ondes: Mutex::new(Default::default()),
                vecteurs: Mutex::new(Arc::new(Vec::new())),
                graphe: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            artists,
            albums,
            tracks_of_album,
            search,
            roots,
            forget_root,
            start_scan,
            scan_state,
            cover,
            waveform,
            map_view,
            map_progress,
            start_analysis,
            analysis_state,
            start_enrichment,
            enrichment_state,
            start_demix,
            demix_state,
            stems_existants,
            stems_vitesse,
            stems_vitesse_stem,
            stems_exporter,
            stems_etirer,
            stems_cache,
            stems_cache_vider,
            stems_play,
            stems_gain,
            stems_transport,
            stems_state,
            stem_spectre,
            voisins_de_stem,
            stems_greffer,
            path,
            path_drawn,
            selection,
            families,
            prepare_graph,
            neighbours,
            js_error,
            play,
            toggle_pause,
            skip,
            previous,
            jump_to,
            seek,
            set_volume,
            playback_state,
        ])
        .run(tauri::generate_context!())
        .expect("démarrage de l'application impossible");
}

#[cfg(test)]
mod tests {
    use super::{dans_le_contour, sous_une_racine};
    use std::path::Path;

    /// Un rendu ne doit jamais atterrir sous une racine surveillée : il y
    /// serait ingéré, analysé et placé sur la carte alors que ce n'est pas un
    /// morceau.
    #[test]
    fn un_rendu_ne_sort_pas_dans_la_bibliotheque() {
        let racines = vec![
            "/Volumes/CarteSD/Music".to_string(),
            "/Users/x/Musique".to_string(),
        ];
        let sous = |p: &str| sous_une_racine(Path::new(p), &racines);

        assert!(
            sous("/Volumes/CarteSD/Music").is_some(),
            "la racine elle-même"
        );
        assert!(
            sous("/Volumes/CarteSD/Music/rendus").is_some(),
            "un sous-dossier"
        );
        assert!(sous("/Users/x/Musique/a/b").is_some());

        assert!(sous("/Users/x/Bureau").is_none());
        assert!(sous("/Volumes/CarteSD").is_none(), "au-dessus de la racine");
        // Le piège d'une comparaison de chaînes : ce dossier-là est ailleurs.
        assert!(
            sous("/Users/x/Musique2").is_none(),
            "préfixe textuel trompeur"
        );
    }

    /// Un lasso tracé à la main est presque toujours concave : c'est le cas
    /// que la règle pair-impair doit tenir, et qu'une enveloppe convexe
    /// raterait.
    #[test]
    fn le_contour_tient_les_formes_concaves() {
        // Un « U » : ouvert vers le haut, avec un creux au milieu.
        let u = [
            (-1.0, -1.0),
            (-1.0, 1.0),
            (-0.5, 1.0),
            (-0.5, -0.5),
            (0.5, -0.5),
            (0.5, 1.0),
            (1.0, 1.0),
            (1.0, -1.0),
        ];
        assert!(dans_le_contour(&u, -0.75, 0.0), "branche gauche");
        assert!(dans_le_contour(&u, 0.75, 0.0), "branche droite");
        assert!(!dans_le_contour(&u, 0.0, 0.5), "le creux est dehors");
        assert!(dans_le_contour(&u, 0.0, -0.75), "le fond est dedans");
        assert!(!dans_le_contour(&u, 2.0, 0.0), "loin dehors");
    }

    #[test]
    fn un_carre_se_comporte_comme_un_carre() {
        let c = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
        assert!(dans_le_contour(&c, 0.5, 0.5));
        assert!(!dans_le_contour(&c, 1.5, 0.5));
        assert!(!dans_le_contour(&c, 0.5, 1.5));
        assert!(!dans_le_contour(&c, -0.5, 0.5));
    }
}
