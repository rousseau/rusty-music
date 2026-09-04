// SPDX-License-Identifier: GPL-3.0-or-later
//! Application de bureau — coquille « Atelier », mode Écoute (module 1).
//!
//! Cette couche ne fait que raccorder : toute la logique vit dans `rusty-music-core`
//! (consultation de la base) et `rusty-music-player` (sortie audio, transport). Elle
//! n'ouvre jamais un fichier musical elle-même.

// Sur Windows, évite d'ouvrir une console derrière la fenêtre.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rusty_music_analysis::chemin::{Empreinte, Graphe};
use rusty_music_core::db::{AlbumRow, ArtistRow, MapPoint, RootRow, TrackRow};
use rusty_music_core::Library;
use rusty_music_player::Player;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Hachage de `ui/`, injecté par `build.rs`. Jamais lu à l'exécution — sa seule
/// fonction est de faire dépendre la compilation de ce fichier du contenu de
/// l'interface, pour que `generate_context!` ré-embarque les assets dès qu'un
/// fichier de `ui/` change. Voir `build.rs`.
const _UI_HASH: &str = env!("RUSTY_UI_HASH");

mod tuiles;

/// État partagé. `rusqlite::Connection` et le lecteur ne sont pas `Sync` : on
/// les protège chacun par un verrou plutôt que d'ouvrir une base par appel.
struct Etat {
    lib: Mutex<Library>,
    player: Mutex<Player>,
    /// Chemin de la base, pour que le scan puisse ouvrir sa propre connexion.
    db: PathBuf,
    /// Racine du cache HD (`crates/superres`), à côté de la base. Le lecteur y
    /// aiguille les pistes régénérées quand la lecture HD est active.
    hd: PathBuf,
    /// Avancement d'une régénération HD, sondé par l'interface.
    superres: Mutex<EtatSuperres>,
    /// Le modèle AERO, chargé à la première régénération puis gardé (~156 Mo).
    superres_modele: Mutex<Option<rusty_music_superres::Modele>>,
    scan: Mutex<EtatScan>,
    analyse: Mutex<EtatAnalyse>,
    descripteurs: Mutex<EtatDescripteurs>,
    enrichissement: Mutex<EtatEnrichissement>,
    /// Avancement de la passe de popularité générale (ListenBrainz + Deezer).
    popularite: Mutex<EtatPopularite>,
    /// Avancement de la passe du mode Découvrir (sorties, collaborations,
    /// voisins), sondé par l'interface.
    decouvrir: Mutex<EtatDecouvrir>,
    demix: Mutex<EtatDemix>,
    /// Où en est la transposition des stems. Elle tourne dans son fil : c'est
    /// une vingtaine de secondes par stem, et l'interface doit rester servie.
    transpose: Mutex<EtatTranspose>,
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
    /// besoin (sonique et errance).
    graphe: Mutex<Option<(usize, Arc<Graphe>)>>,
    /// Verrou pris pour toute la durée d'une construction de `graphe`.
    ///
    /// `graphe` lui-même n'est tenu que le temps de lire ou d'écrire le cache,
    /// jamais pendant le balayage (une quinzaine de secondes) — sinon
    /// l'inspecteur, qui sonde `graphe_progress`, se bloquerait. Mais relâché
    /// ainsi, plusieurs commandes `path`/`prepare_graph`/`path_album` arrivées
    /// coup sur coup (l'utilisateur qui bascule vite entre sonique et errance)
    /// lançaient chacune son propre balayage : six en parallèle, le CPU
    /// sursouscrit d'autant, et chacun passait de 15 s à 85 s — vécu comme un
    /// gel. Ce verrou-ci sérialise : le premier construit, les suivants
    /// attendent puis retrouvent le cache déjà chaud.
    graphe_construction: Mutex<()>,
    /// Avancement du balayage qui construit `graphe`, sondé par l'interface
    /// (`graphe_progress`). `graphe_total` à 0 : aucun balayage en cours —
    /// jamais lancé, ou déjà en cache. Sinon `graphe_fait` sur `graphe_total`
    /// empreintes. Sans lui, la première errance d'une session laisse
    /// l'interface muette une vingtaine de secondes.
    graphe_fait: AtomicUsize,
    graphe_total: AtomicUsize,
    /// Nappe de densité de la carte — polygones prêts à remplir. Recalculée
    /// seulement après une projection/clustering réussi ([`recalculer_densite`]),
    /// jamais par image ni au zoom : c'est tout l'intérêt de la garder ici
    /// plutôt que de la refaire côté interface à chaque geste.
    /// Le réseau de circulation, bâti à la première demande d'itinéraire.
    /// Une trentaine de secondes, dominées par le graphe des voisins — comme
    /// `graphe`, on ne le refait pas à chaque trajet.
    reseau: Mutex<Option<rusty_music_analysis::reseau::Reseau>>,
    densite: Mutex<Option<rusty_music_core::density::ResultatDensite>>,
    /// Le plan de ville importé (`carto ville`), chargé une fois puis gardé —
    /// `ville-paris.db` fait une vingtaine de mégaoctets, pas question de la
    /// relire à chaque « refaire les tuiles ». `None` tant qu'aucune ville
    /// n'a été chargée ; `rassembler` retombe alors sur le monde fictif.
    ville: Mutex<Option<Arc<rusty_music_osm::Extrait>>>,
    /// Le graphe routable du plan de ville (`carto::reseau_reel`), bâti à la
    /// première demande d'itinéraire réel — quelques dizaines de
    /// millisecondes sur Paris, mais pas question de le refaire à chaque
    /// trajet tracé. Non pondéré : sert de référence d'indexation des sommets
    /// et à l'accrochage des morceaux.
    graphe_reel: Mutex<Option<Arc<rusty_music_carto::reseau_reel::Graphe>>>,
    /// Chaque morceau accroché à son sommet de voirie, dans les deux sens —
    /// bâti une fois sur `graphe_reel` (l'indexation des sommets ne dépend pas
    /// de la pondération), invalidé quand les tuiles sont refaites.
    accrochage_voirie: Mutex<Option<Arc<AccrochageVoirie>>>,
    /// Un graphe de voirie **pondéré** par profil — trois entrées au plus. Même
    /// jeu de sommets que `graphe_reel`, seuls les poids d'arête changent.
    graphes_voirie: Mutex<
        std::collections::HashMap<
            rusty_music_carto::cout_itineraire::ProfilVoirie,
            Arc<rusty_music_carto::reseau_reel::Graphe>,
        >,
    >,
    /// La grille « aux abords d'un parc ou de l'eau », pour le profil
    /// panoramique — ne dépend que de l'extrait.
    agrement_voirie: Mutex<Option<Arc<rusty_music_carto::cout_itineraire::ProximiteAgrement>>>,
}

/// Chaque morceau accroché au sommet de voirie le plus proche, dans les deux
/// sens. Bâti par [`charger_accrochage_voirie`].
struct AccrochageVoirie {
    /// Morceau → sommet de voirie (index dans `graphe_reel`).
    sommet_de: std::collections::HashMap<i64, u32>,
    /// Sommet de voirie → les morceaux qui s'y accrochent.
    morceaux_a: std::collections::HashMap<u32, Vec<i64>>,
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

/// Threads pour un calcul de fond lancé depuis l'appli — jamais tous les
/// cœurs, contrairement à la CLI (`scan::default_jobs`) qui n'a rien
/// d'autre à faire tourner sur la machine.
///
/// Signalé directement : la lecture partage le processus avec ces passes
/// (graphe des voisins, analyse, descripteurs), et saturer le CPU la rendait
/// saccadée. Un cœur de côté ne ralentit pas grand-chose sur une passe déjà
/// dominée par l'attente disque — c'est elle qui fixe le rythme, pas le
/// calcul.
fn coeurs_arriere_plan() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .saturating_sub(1)
        .max(1)
}

/// Fils pour une passe de fond, ajustés au support des racines surveillées.
///
/// Une carte SD ou une clé USB, sous forte lecture concurrente, ne se contente
/// pas de ralentir : rencontré en pratique, un lecteur de carte peut y
/// déclencher une panique noyau (`pcie-sdreader`, timeout de complétion PCIe)
/// et emporter la machine avec lui — pas seulement l'application. Un disque
/// interne n'a pas ce travers — c'est donc lui, et lui seul, qui garde
/// `coeurs_arriere_plan()`. Une seule racine amovible suffit à brider toute la
/// passe : les fils sont partagés entre tous les fichiers, quelle que soit
/// leur racine. Un seul fil à la fois sur l'amovible : la marge de sécurité
/// compte plus que la vitesse, vu le prix d'une erreur ici.
fn fils_pour_passe(lib: &Library) -> usize {
    const FILS_AMOVIBLE: usize = 1;
    let amovible = lib
        .roots()
        .map(|racines| {
            racines
                .iter()
                .any(|r| rusty_music_core::volume::est_amovible(Path::new(&r.path)))
        })
        .unwrap_or(false);
    if amovible {
        FILS_AMOVIBLE
    } else {
        coeurs_arriere_plan()
    }
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

/// Ce que la webview doit savoir avant d'ouvrir la carte MapLibre.
#[derive(serde::Serialize)]
struct EtatTuiles {
    pretes: bool,
    carte: String,
    relief: String,
    octets: u64,
    /// Vraies si l'archive est plus vieille que la dernière projection : la
    /// carte a bougé sous les tuiles, il faut les refaire.
    perimees: bool,
}

#[tauri::command(async)]
fn tuiles_etat(app: tauri::AppHandle, etat: State<Etat>) -> Result<EtatTuiles, String> {
    let carte = tuiles::chemin_carte(&app).map_err(echec)?;
    let relief = tuiles::chemin_relief(&app).map_err(echec)?;
    let meta = std::fs::metadata(&carte).ok();
    let octets = meta.as_ref().map(|m| m.len()).unwrap_or(0)
        + std::fs::metadata(&relief).map(|m| m.len()).unwrap_or(0);

    // La projection la plus récente fait foi : `features.computed_at` bouge à
    // chaque recalcul de la carte.
    let projetee = etat
        .lib
        .lock()
        .map_err(echec)?
        .derniere_projection(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;
    let ecrite = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    Ok(EtatTuiles {
        pretes: carte.is_file() && relief.is_file() && dossier_style(&app).is_some(),
        carte: carte.display().to_string(),
        relief: relief.display().to_string(),
        octets,
        perimees: match (ecrite, projetee) {
            (Some(e), Some(p)) => e < p,
            (None, _) => true,
            _ => false,
        },
    })
}

fn dossier_style(app: &tauri::AppHandle) -> Option<PathBuf> {
    let s = tuiles::dossier(app).ok()?.join("style.json");
    s.is_file().then_some(s)
}

/// Le plan de ville à afficher, ou `None` pour retomber sur le monde procédural.
///
/// À côté de la base : soit importé par l'utilisateur (`carto ville --sortie`),
/// soit installé du paquet au démarrage ([`installer_plan_de_ville`]). Un
/// fichier vide — le repli d'un `cargo build` sans `ville-paris.db`
/// (`apps/desktop/build.rs`) qu'un paquet peut embarquer — compte pour absent.
fn plan_de_ville(db: &Path) -> Option<PathBuf> {
    let p = db.with_file_name("ville-paris.db");
    match std::fs::metadata(&p) {
        Ok(m) if m.is_file() && m.len() > 1_000_000 => Some(p),
        _ => None,
    }
}

/// Installe le plan de ville du paquet à côté de la base si l'utilisateur n'en
/// a pas déjà un. Copie unique ; un `carto ville` ultérieur écrase.
fn installer_plan_de_ville(app: &tauri::App, dossier: &Path) {
    let cible = dossier.join("ville-paris.db");
    if cible.exists() {
        return;
    }
    let Ok(res) = app.path().resource_dir() else {
        return;
    };
    let source = res.join("ville-paris.db");
    // Le paquet peut embarquer un `ville-paris.db` vide (repli de `build.rs`) :
    // on ne copie que du vrai.
    if std::fs::metadata(&source).map(|m| m.len()).unwrap_or(0) < 1_000_000 {
        return;
    }
    match std::fs::create_dir_all(dossier).and_then(|_| std::fs::copy(&source, &cible)) {
        Ok(_) => tracing::info!("plan de ville (Paris) installé depuis le paquet"),
        Err(e) => tracing::warn!(%e, "plan de ville du paquet non installé"),
    }
}

/// Charge `ville-paris.db` une fois, la garde en mémoire ensuite — relire une
/// vingtaine de mégaoctets à chaque « refaire les tuiles » serait un gâchis.
/// Pas d'invalidation : la ville se remplace en bloc (`carto ville`), jamais
/// en place, et un nouvel import survient hors session.
fn charger_ville(etat: &State<Etat>, chemin: &Path) -> Result<Arc<rusty_music_osm::Extrait>, String> {
    let mut cache = etat.ville.lock().map_err(echec)?;
    if let Some(extrait) = &*cache {
        return Ok(extrait.clone());
    }
    let extrait = Arc::new(rusty_music_osm::base::lire(chemin).map_err(echec)?);
    *cache = Some(extrait.clone());
    Ok(extrait)
}

/// Le graphe routable du plan de ville, construit une fois puis gardé — même
/// principe que [`charger_ville`], et sur le même extrait.
fn charger_graphe_reel(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
) -> Result<Arc<rusty_music_carto::reseau_reel::Graphe>, String> {
    let mut cache = etat.graphe_reel.lock().map_err(echec)?;
    if let Some(graphe) = &*cache {
        return Ok(graphe.clone());
    }
    let graphe = Arc::new(rusty_music_carto::reseau_reel::Graphe::construire(extrait));
    *cache = Some(graphe.clone());
    Ok(graphe)
}

/// La grille « aux abords d'un parc ou de l'eau », construite une fois par
/// session — ne dépend que de l'extrait (profil panoramique).
fn charger_agrement_voirie(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
) -> Result<Arc<rusty_music_carto::cout_itineraire::ProximiteAgrement>, String> {
    let mut cache = etat.agrement_voirie.lock().map_err(echec)?;
    if let Some(a) = &*cache {
        return Ok(a.clone());
    }
    let a = Arc::new(rusty_music_carto::cout_itineraire::ProximiteAgrement::nouvelle(extrait, 120.0));
    *cache = Some(a.clone());
    Ok(a)
}

/// Un graphe de voirie pondéré par profil, mis en cache. Le jeu de sommets est
/// identique à celui de `graphe_reel` : seule la pondération d'arête change.
fn charger_graphe_voirie(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
    profil: rusty_music_carto::cout_itineraire::ProfilVoirie,
) -> Result<Arc<rusty_music_carto::reseau_reel::Graphe>, String> {
    use rusty_music_carto::cout_itineraire::friction_itineraire;

    if let Some(g) = etat.graphes_voirie.lock().map_err(echec)?.get(&profil) {
        return Ok(g.clone());
    }
    let agrement = charger_agrement_voirie(etat, extrait)?;
    let graphe = Arc::new(rusty_music_carto::reseau_reel::Graphe::construire_pondere(
        extrait,
        friction_itineraire(profil, Some(&agrement)),
    ));
    etat.graphes_voirie.lock().map_err(echec)?.insert(profil, graphe.clone());
    Ok(graphe)
}

/// Accroche chaque morceau (son adresse réelle, `positions.json`) au sommet de
/// voirie le plus proche. Bâti une fois sur `graphe_base` — l'indexation des
/// sommets ne dépend pas de la pondération, donc l'accrochage vaut pour tous
/// les graphes de voirie pondérés.
///
/// `Err` si aucune position réelle n'est disponible (pas de plan de ville, ou
/// tuiles pas encore générées) — l'appelant en fait un repli.
fn charger_accrochage_voirie(
    etat: &State<Etat>,
    app: &tauri::AppHandle,
    graphe_base: &rusty_music_carto::reseau_reel::Graphe,
) -> Result<Arc<AccrochageVoirie>, String> {
    {
        let cache = etat.accrochage_voirie.lock().map_err(echec)?;
        if let Some(a) = &*cache {
            return Ok(a.clone());
        }
    }

    // `points_de_carte_effectifs(reel = true)` lit `positions.json` : `(id,
    // lon, lat)`. Sur le chemin fictif il retombe sur le t-SNE — sans rapport
    // avec une rue —, d'où le garde-fou `positions.json` ci-dessous.
    let dossier = tuiles::dossier(app).map_err(echec)?;
    if !dossier.join("positions.json").is_file() {
        return Err("carte réelle pas encore générée".into());
    }
    let points = points_de_carte_effectifs(etat, app, true)?;
    if points.is_empty() {
        return Err("aucune adresse réelle".into());
    }

    let index = graphe_base.index_sommets();
    let mut sommet_de: std::collections::HashMap<i64, u32> = std::collections::HashMap::new();
    let mut morceaux_a: std::collections::HashMap<u32, Vec<i64>> = std::collections::HashMap::new();
    for (id, x, y) in points {
        if let Some(s) = index.plus_proche(graphe_base, [x as f64, y as f64], 200.0) {
            sommet_de.insert(id, s);
            morceaux_a.entry(s).or_default().push(id);
        }
    }
    // Ordre déterministe des morceaux d'un même sommet (départage à rang égal).
    for v in morceaux_a.values_mut() {
        v.sort_unstable();
    }
    tracing::info!(
        accroches = sommet_de.len(),
        sommets = morceaux_a.len(),
        "morceaux accrochés à la voirie"
    );
    let a = Arc::new(AccrochageVoirie { sommet_de, morceaux_a });
    *etat.accrochage_voirie.lock().map_err(echec)? = Some(a.clone());
    Ok(a)
}

/// Popularité par morceau, `[0, 1]` — proxy « nombre de morceaux gardés de
/// l'artiste », faute de compteur d'écoute (comme [`construire_reseau`]). Sert
/// de « dénivelé » le long d'un itinéraire.
fn popularites_par_artiste(vue: &[MapPoint]) -> std::collections::HashMap<i64, f32> {
    let mut par_artiste: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for p in vue {
        *par_artiste.entry(p.artist.clone().unwrap_or_default()).or_default() += 1;
    }
    let max = par_artiste.values().copied().max().unwrap_or(1).max(1) as f32;
    vue.iter()
        .map(|p| {
            let n = par_artiste[&p.artist.clone().unwrap_or_default()] as f32;
            (p.id, n / max)
        })
        .collect()
}

/// Les morceaux rencontrés le long d'un tracé de voirie, dans l'ordre du
/// parcours. C'est le cœur du mode « itinéraire » : le trajet suit les rues,
/// la playlist est faite de ce qui les borde.
///
/// - `couloir` : sommet → rang (position le long du tracé), de
///   [`rusty_music_carto::reseau_reel::Graphe::couloir`] ;
/// - `famille` : si `Some`, ne garder que ces morceaux (le filtre d'Explorer) —
///   mais le départ et l'arrivée passent toujours ;
/// - `arrivee_id` : **si `Some`, elle est le terminus** (forcée en queue) et la
///   durée est ignorée — « va jusque-là » l'emporte ;
/// - `duree_cible_ms` : ne s'applique **que sans arrivée** — la playlist
///   s'arrête quand le cumul des durées atteint la cible (moins 90 s de
///   tolérance).
#[allow(clippy::too_many_arguments)]
fn morceaux_le_long(
    accrochage: &AccrochageVoirie,
    couloir: &std::collections::HashMap<u32, usize>,
    depart_id: i64,
    arrivee_id: Option<i64>,
    famille: Option<&HashSet<i64>>,
    duree_cible_ms: Option<u64>,
    duree: &dyn Fn(i64) -> u64,
) -> Vec<i64> {
    const PLAFOND: usize = 120;
    const TOLERANCE_MS: u64 = 90_000;

    // **Une arrivée posée prime : le trajet va jusqu'à elle**, et la durée ne
    // s'applique plus (elle ne sert qu'aux itinéraires sans arrivée — « une
    // balade de 40 min par là »). L'arrivée est forcée en queue et exclue du
    // corps ; le départ toujours en tête.
    let terminus = arrivee_id;
    let borne = |id: i64| id == depart_id || Some(id) == terminus;
    let duree_cible_ms = if terminus.is_some() { None } else { duree_cible_ms };

    // (rang, id) pour chaque morceau du couloir.
    let mut candidats: Vec<(usize, i64)> = Vec::new();
    for (sommet, rang) in couloir {
        if let Some(ids) = accrochage.morceaux_a.get(sommet) {
            for &id in ids {
                candidats.push((*rang, id));
            }
        }
    }
    candidats.sort_unstable_by_key(|&(rang, id)| (rang, id));

    let mut vus: HashSet<i64> = HashSet::new();
    let mut suite: Vec<i64> = Vec::new();
    for (_, id) in candidats {
        if borne(id) || !vus.insert(id) {
            continue;
        }
        if let Some(f) = famille {
            if !f.contains(&id) {
                continue;
            }
        }
        suite.push(id);
    }

    // Départ en tête.
    suite.retain(|&id| !borne(id));
    suite.insert(0, depart_id);

    // La durée prime : on coupe dès que le cumul l'atteint.
    if let Some(cible) = duree_cible_ms {
        let seuil = cible.saturating_sub(TOLERANCE_MS);
        let mut cumul = 0u64;
        let mut coupe = suite.len();
        for (i, &id) in suite.iter().enumerate() {
            cumul += duree(id);
            if cumul >= seuil {
                coupe = i + 1;
                break;
            }
        }
        suite.truncate(coupe.max(1));
    } else if let Some(a) = terminus {
        // Pas de durée : l'arrivée est le terminus.
        if a != depart_id {
            suite.push(a);
        }
    }

    suite.truncate(PLAFOND.max(1));
    suite
}

/// Assemble une [`rusty_music_carto::source::Source`] depuis le plan de
/// ville réel plutôt que depuis le monde engendré — voir
/// `rusty_music_carto::ville::rassembler`.
fn rassembler_ville(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
) -> Result<rusty_music_carto::source::Source, String> {
    let modele = rusty_music_analysis::passe::MODELE;
    let lib = etat.lib.lock().map_err(echec)?;
    let vue = lib.map_view(modele).map_err(echec)?;
    if vue.is_empty() {
        return Err("aucun morceau sur la carte : lancer l'analyse d'abord".into());
    }
    let noms_famille: std::collections::HashMap<i64, String> = lib
        .familles(modele)
        .map_err(echec)?
        .into_iter()
        .map(|(id, nom, _)| (id, nom))
        .collect();
    drop(lib);

    let r = rusty_music_carto::ville::rassembler(
        extrait,
        &vue,
        &noms_famille,
        rusty_music_carto::ville::ESPACEMENT_PAR_DEFAUT,
        Some(rusty_music_carto::ville::ILE_DE_LA_CITE),
    );
    // Pas de `curiosites` sur le plan de ville : `style::couches_ville` ne les
    // rend plus (pastilles brunes plus grosses qu'un bâtiment). Le dispositif
    // monument/refuge/fondation reste propre au monde fictif.
    tracing::info!(
        adresses = r.adresses_posees,
        sans_adresse = r.morceaux_sans_adresse,
        repli_quartier = r.repli_quartier,
        hors_zone = r.hors_zone,
        debordements = r.debordements,
        artistes_ancres = r.artistes_ancres,
        batiments_peuples = r.batiments_peuples,
        erreur_quartiers = r.quartiers_erreur_relative,
        albums = r.source.albums.len(),
        "plan de ville réel assemblé"
    );
    Ok(r.source)
}

/// Fabrique les deux archives. Quelques secondes sur 27 000 morceaux — la
/// commande est `async`, donc hors du fil de l'interface.
///
/// Deux chemins : si `ville-paris.db` existe à côté de la bibliothèque
/// (importée via `carto ville`), la carte affiche le vrai plan de ville
/// ([`rassembler_ville`]) — pas de relief à ombrer, Paris est plat. Sinon,
/// repli sur le monde fictif engendré depuis la bibliothèque
/// ([`rassembler`]), inchangé.
#[tauri::command(async)]
fn engendrer_tuiles(app: tauri::AppHandle, etat: State<Etat>) -> Result<String, String> {
    let (src, paliers, ombrage_rapport) = if let Some(chemin_ville) = plan_de_ville(&etat.db) {
        let extrait = charger_ville(&etat, &chemin_ville)?;
        let src = rassembler_ville(&etat, &extrait)?;
        (src, rusty_music_carto::tuiles::Paliers::ville(), None)
    } else {
        tracing::info!(
            "aucune ville importée — carte procédurale de repli ; lancer \
             `carto ville <extrait.osm.pbf> --commune <nom>` pour Paris"
        );
        let (src, champ, resolution) = rassembler(&etat)?;
        let ombrage = rusty_music_carto::relief::Ombrage::default();
        let rr = rusty_music_carto::relief::ecrire(
            &champ,
            resolution,
            &ombrage,
            &tuiles::chemin_relief(&app).map_err(echec)?,
        )
        .map_err(echec)?;
        (src, rusty_music_carto::tuiles::Paliers::default(), Some(rr))
    };

    let chemin = tuiles::chemin_carte(&app).map_err(echec)?;
    let r = rusty_music_carto::tuiles::ecrire(&src, &paliers, &chemin).map_err(echec)?;

    // Le style part avec les tuiles : même source, mêmes paliers, aucune
    // dérive possible entre ce qui est dans les tuiles et ce qui s'affiche.
    // `construire` retire lui-même la source « relief » sur le plan de ville
    // réel (`Source::est_ville_reelle`) — rien à faire ici pour l'omettre.
    //
    // **Une variante de style par palette de fond de plan.** N'affecte que
    // `style.json`, jamais les tuiles : reconstruire le style est du pur JSON,
    // presque gratuit face à la `Source` (déjà bâtie ci-dessus). L'interface
    // bascule ensuite entre ces fichiers sans régénérer quoi que ce soit.
    // `osm-clair` prend le nom sans suffixe (le style lu par défaut).
    let dossier_style = tuiles::dossier(&app).map_err(echec)?;
    for palette in rusty_music_carto::Palette::toutes() {
        let style =
            rusty_music_carto::style::construire(&src, &paliers, &tuiles::base(), palette);
        let nom = if palette.id == "osm-clair" {
            "style.json".to_string()
        } else {
            format!("style-{}.json", palette.id)
        };
        std::fs::write(
            dossier_style.join(nom),
            serde_json::to_vec_pretty(&style).map_err(echec)?,
        )
        .map_err(echec)?;
    }

    // Les positions réelles (lon/lat) des morceaux, pour que la surcouche du
    // canevas (lasso, survol, chemin dessiné) retrouve chaque morceau là où
    // les tuiles le montrent. Sans ce fichier, `app.js` n'a que les
    // coordonnées t-SNE de `map_view` — sans aucun rapport avec une adresse
    // de rue une fois le plan de ville réel actif. Absent (donc en échec à
    // la lecture) sur le chemin fictif : c'est le signal qu'`app.js` utilise
    // pour savoir dans quel repère il travaille.
    let chemin_positions = tuiles::dossier(&app).map_err(echec)?.join("positions.json");
    if src.est_ville_reelle() {
        let positions: std::collections::HashMap<i64, [f32; 2]> =
            src.morceaux.iter().map(|m| (m.id, [m.x, m.y])).collect();
        std::fs::write(&chemin_positions, serde_json::to_vec(&positions).map_err(echec)?)
            .map_err(echec)?;
    } else {
        std::fs::remove_file(&chemin_positions).ok();
    }

    // L'ancienne archive reste projetée en mémoire tant qu'on ne la lâche pas.
    app.state::<tuiles::Archives>().oublier();

    // Les tuiles viennent de changer : `positions.json` avec, donc tout ce qui
    // en dépend (l'accrochage des morceaux à la voirie, les graphes pondérés,
    // la grille d'agrément, et l'extrait / le graphe de base eux-mêmes en cas
    // de réimport). On repart de zéro à la prochaine demande d'itinéraire.
    *etat.ville.lock().map_err(echec)? = None;
    *etat.graphe_reel.lock().map_err(echec)? = None;
    *etat.accrochage_voirie.lock().map_err(echec)? = None;
    etat.graphes_voirie.lock().map_err(echec)?.clear();
    *etat.agrement_voirie.lock().map_err(echec)? = None;

    match ombrage_rapport {
        Some(rr) => {
            tracing::info!(
                tuiles = r.tuiles,
                relief = rr.tuiles,
                secondes = r.duree.as_secs_f64() + rr.duree.as_secs_f64(),
                "tuiles engendrées"
            );
            Ok(format!(
                "{} tuiles ({:.1} Mo) et {} d'ombrage ({:.1} Mo) en {:.1} s",
                r.tuiles,
                r.octets as f64 / 1_048_576.0,
                rr.tuiles,
                rr.octets as f64 / 1_048_576.0,
                r.duree.as_secs_f64() + rr.duree.as_secs_f64()
            ))
        }
        None => {
            tracing::info!(
                tuiles = r.tuiles,
                secondes = r.duree.as_secs_f64(),
                "tuiles engendrées (plan de ville réel)"
            );
            Ok(format!(
                "{} tuiles ({:.1} Mo) en {:.1} s — plan de ville réel (© les contributeurs OpenStreetMap)",
                r.tuiles,
                r.octets as f64 / 1_048_576.0,
                r.duree.as_secs_f64()
            ))
        }
    }
}

/// Un itinéraire, tel que l'interface le reçoit.
#[derive(serde::Serialize)]
struct ItineraireVu {
    pistes: Vec<MapPoint>,
    duree_ms: u64,
    distance_sonique: f32,
    /// Le dénivelé : la popularité le long du trajet.
    popularite: Vec<f32>,
    classes: Vec<String>,
}

/// Trace un itinéraire dans le réseau de circulation.
///
/// `carto-google-maps.md` §3 : un seul graphe, plusieurs fonctions de coût. La
/// durée cible est la contrainte la plus utile côté utilisateur — « un
/// itinéraire de 40 minutes » est une vraie demande.
#[tauri::command(async)]
fn itineraire(
    etat: State<Etat>,
    depart: i64,
    arrivee: Option<i64>,
    profil: String,
    minutes: Option<u64>,
) -> Result<Vec<ItineraireVu>, String> {
    use rusty_music_analysis::reseau::{Options, Profil};

    let modele = rusty_music_analysis::passe::MODELE;
    {
        // Construction paresseuse, une seule fois par session.
        let mut cache = etat.reseau.lock().map_err(echec)?;
        if cache.is_none() {
            *cache = Some(construire_reseau(&etat, modele)?);
        }
    }
    let cache = etat.reseau.lock().map_err(echec)?;
    let reseau = cache.as_ref().expect("réseau construit juste au-dessus");

    let mut o = Options::nouveau(
        depart,
        match profil.as_str() {
            "sentier" => Profil::Sentier,
            "panoramique" => Profil::Panoramique,
            _ => Profil::Autoroute,
        },
    );
    o.arrivee = arrivee;
    o.alternatives = 1;
    o.duree_cible_ms = minutes.map(|m| m * 60_000);

    let trajets = reseau.itineraires(&o).map_err(echec)?;
    drop(cache);

    let lib = etat.lib.lock().map_err(echec)?;
    let par_id: std::collections::HashMap<i64, MapPoint> = lib
        .map_view(modele)
        .map_err(echec)?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    drop(lib);
    trajets
        .into_iter()
        .map(|t| {
            let pistes: Vec<MapPoint> = t
                .morceaux
                .iter()
                .filter_map(|id| par_id.get(id).cloned())
                .collect();
            Ok(ItineraireVu {
                pistes,
                duree_ms: t.duree_ms,
                distance_sonique: t.distance_sonique,
                popularite: t.popularite,
                classes: t.classes.iter().map(|c| format!("{c:?}").to_lowercase()).collect(),
            })
        })
        .collect()
}

/// Un itinéraire routé sur la voirie réelle, tel que l'interface le reçoit.
#[derive(serde::Serialize)]
struct ItineraireVoirieVu {
    /// Les morceaux rencontrés le long des rues, dans l'ordre — la playlist.
    pistes: Vec<MapPoint>,
    /// La ligne du trajet, déjà routée sur les rues : `[lon, lat]`. L'interface
    /// n'a plus à rappeler `trace_rues`.
    polyligne: Vec<[f64; 2]>,
    /// Somme des durées des `pistes`.
    duree_ms: u64,
    /// Longueur de la `polyligne`, en mètres.
    distance_m: f64,
    /// Le dénivelé : la popularité le long du trajet, un `f32` par piste.
    popularite: Vec<f32>,
    /// Les classes de voie traversées, doublons consécutifs retirés.
    classes: Vec<String>,
}

#[derive(serde::Serialize)]
struct ReponseItineraireVoirie {
    /// `Some(raison)` : l'interface affiche la raison et retombe sur
    /// l'itinéraire musical (`itineraire`).
    repli: Option<String>,
    trajets: Vec<ItineraireVoirieVu>,
}

impl ReponseItineraireVoirie {
    fn repli(raison: &str) -> ReponseItineraireVoirie {
        ReponseItineraireVoirie { repli: Some(raison.into()), trajets: Vec::new() }
    }
}

/// Distance équirectangulaire entre deux points `[lon, lat]`, en mètres — même
/// approximation que `carto::reseau_reel` et `osm::Troncon::longueur_m`.
fn distance_lonlat_m(a: [f64; 2], b: [f64; 2]) -> f64 {
    let lat_moy = (a[1] + b[1]).to_radians() / 2.0;
    let dx = (b[0] - a[0]).to_radians() * lat_moy.cos() * 6_371_000.0;
    let dy = (b[1] - a[1]).to_radians() * 6_371_000.0;
    (dx * dx + dy * dy).sqrt()
}

/// Trace un itinéraire **sur les vraies rues** et compose la playlist des
/// morceaux qui les bordent.
///
/// `docs/carto-ville.md` (révisé) : sur le plan de ville, ce mode ne se
/// contente plus d'habiller le trait d'un chemin musical — c'est la voirie qui
/// choisit les morceaux. Le chemin musical (`itineraire`) reste le repli quand
/// il n'y a pas de ville, pas d'adresse, ou dans le nuage.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn itineraire_voirie(
    app: tauri::AppHandle,
    etat: State<Etat>,
    depart: i64,
    arrivee: Option<i64>,
    profil: String,
    minutes: Option<u64>,
    famille: Option<i64>,
    rayon_m: Option<f64>,
) -> Result<ReponseItineraireVoirie, String> {
    use rusty_music_carto::cout_itineraire::ProfilVoirie;

    let Some(chemin_ville) = plan_de_ville(&etat.db) else {
        return Ok(ReponseItineraireVoirie::repli("aucune ville importée"));
    };
    let extrait = charger_ville(&etat, &chemin_ville)?;
    let graphe_base = charger_graphe_reel(&etat, &extrait)?;
    let accrochage = match charger_accrochage_voirie(&etat, &app, &graphe_base) {
        Ok(a) => a,
        Err(_) => {
            return Ok(ReponseItineraireVoirie::repli(
                "carte réelle pas encore générée — refaire les tuiles",
            ))
        }
    };
    let profil_v = ProfilVoirie::depuis_nom(&profil);
    let graphe = charger_graphe_voirie(&etat, &extrait, profil_v)?;

    // Positions réelles (lon/lat) des morceaux.
    let positions: std::collections::HashMap<i64, [f64; 2]> =
        points_de_carte_effectifs(&etat, &app, true)?
            .into_iter()
            .map(|(id, x, y)| (id, [x as f64, y as f64]))
            .collect();
    let Some(&depart_pos) = positions.get(&depart) else {
        return Ok(ReponseItineraireVoirie::repli("ce morceau n'a pas d'adresse sur la carte"));
    };
    if graphe.accrocher(depart_pos).is_none() {
        return Ok(ReponseItineraireVoirie::repli("le départ n'est rattaché à aucune rue"));
    }
    let arrivee_pos = match arrivee {
        Some(a) => match positions.get(&a) {
            Some(&p) if graphe.accrocher(p).is_some() => Some(p),
            _ => {
                return Ok(ReponseItineraireVoirie::repli(
                    "l'arrivée n'a pas d'adresse sur la carte",
                ))
            }
        },
        None => None,
    };

    let permis: Option<HashSet<i64>> =
        famille.map(|f| morceaux_de_famille(&etat, f)).transpose()?;

    // `map_view` une fois : hydratation, durées, popularité.
    let modele = rusty_music_analysis::passe::MODELE;
    let vue = {
        let lib = etat.lib.lock().map_err(echec)?;
        lib.map_view(modele).map_err(echec)?
    };
    let par_id: std::collections::HashMap<i64, MapPoint> =
        vue.iter().map(|p| (p.id, p.clone())).collect();
    let duree_ms_de: std::collections::HashMap<i64, u64> = vue
        .iter()
        .map(|p| (p.id, p.duration_ms.unwrap_or(0).max(0) as u64))
        .collect();
    let pop_de = popularites_par_artiste(&vue);
    let duree = |id: i64| duree_ms_de.get(&id).copied().unwrap_or(0);

    // Cible = position d'arrivée, sinon un morceau assez loin pour qu'il y ait
    // ~`minutes` de musique en chemin.
    let cible_ms = minutes.filter(|m| *m > 0).map(|m| m * 60_000);
    let cible_pos = match (arrivee_pos, cible_ms) {
        (Some(p), _) => p,
        (None, Some(cible)) => {
            let couts = graphe.couts_depuis(depart_pos);
            let mut candidats: Vec<(u64, i64)> = accrochage
                .sommet_de
                .iter()
                .filter(|(id, _)| {
                    permis.as_ref().is_none_or(|f| f.contains(*id) || **id == depart)
                })
                .filter_map(|(&id, &s)| couts.get(s as usize).copied().flatten().map(|c| (c, id)))
                .collect();
            candidats.sort_unstable();
            // Viser ~2× la durée : le tracé ne longe qu'une partie des morceaux
            // « à portée » (ceux de son couloir), pas tous ceux du disque de
            // coût. Sans cette marge, la playlist manque souvent la cible et le
            // tracé, tronqué au dernier morceau, reste tout petit.
            let vise = cible.saturating_mul(2);
            let mut cumul = 0u64;
            let mut cible_id = candidats.last().map(|&(_, id)| id);
            for &(_, id) in &candidats {
                cumul += duree(id);
                if cumul >= vise {
                    cible_id = Some(id);
                    break;
                }
            }
            match cible_id.and_then(|id| positions.get(&id)) {
                Some(&p) => p,
                None => {
                    return Ok(ReponseItineraireVoirie::repli(
                        "aucun morceau accessible pour cette durée",
                    ))
                }
            }
        }
        (None, None) => {
            return Ok(ReponseItineraireVoirie::repli("choisir une arrivée ou une durée"));
        }
    };

    // Un seul trajet : le meilleur pour ce profil. Les « variantes » (Yen /
    // pénalité) ne donnaient que des versions dégradées du même profil — le
    // choix, c'est le profil lui-même.
    let Some((trace, _)) = graphe.chemin_sommets(depart_pos, cible_pos) else {
        return Ok(ReponseItineraireVoirie::repli("pas de route jusque-là"));
    };

    // Table classe par tronçon, pour nommer les voies traversées.
    let classe_de: std::collections::HashMap<i64, rusty_music_osm::Classe> =
        extrait.troncons.iter().map(|t| (t.id, t.classe)).collect();
    let rayon = rayon_m.unwrap_or(25.0).max(0.0);

    let couloir = graphe.couloir(&trace, rayon);
    let route =
        morceaux_le_long(&accrochage, &couloir, depart, arrivee, permis.as_ref(), cible_ms, &duree);
    let pistes: Vec<MapPoint> = route.iter().filter_map(|id| par_id.get(id).cloned()).collect();
    if pistes.len() < 2 {
        return Ok(ReponseItineraireVoirie::repli(
            "aucun morceau le long de cet itinéraire — essayer une autre durée ou un autre profil",
        ));
    }

    // **Avec une arrivée, le tracé va jusqu'à elle** — on garde tout. Sans
    // arrivée, `chemin_sommets` file jusqu'à une destination provisoire (~la
    // durée de musique plus loin) ; si les morceaux sont denses près du départ,
    // la playlist s'arrête bien avant, et dessiner la polyligne entière puis
    // rejoindre le dernier morceau (souvent revenu près du départ) faisait une
    // boucle. On tronque donc au dernier morceau retenu.
    let fin = if arrivee.is_some() {
        trace.len()
    } else {
        fin_de_trace(&route, &accrochage, &couloir, trace.len())
    };
    let trace = &trace[..fin];

    let polyligne: Vec<[f64; 2]> = trace.iter().map(|&s| graphe.point(s)).collect();
    let distance_m = polyligne.windows(2).map(|w| distance_lonlat_m(w[0], w[1])).sum();
    let duree_ms = pistes.iter().map(|p| duree(p.id)).sum();
    let popularite: Vec<f32> =
        pistes.iter().map(|p| pop_de.get(&p.id).copied().unwrap_or(0.0)).collect();
    let mut classes: Vec<String> = Vec::new();
    for t in graphe.troncons_traverses(trace) {
        if let Some(c) = classe_de.get(&t) {
            let nom = c.nom().to_string();
            if classes.last() != Some(&nom) {
                classes.push(nom);
            }
        }
    }

    Ok(ReponseItineraireVoirie {
        repli: None,
        trajets: vec![ItineraireVoirieVu {
            pistes,
            polyligne,
            duree_ms,
            distance_m,
            popularite,
            classes,
        }],
    })
}

/// Jusqu'où le tracé doit être dessiné : un cran après le sommet du **dernier
/// morceau retenu** dans le couloir. Au moins 2 sommets (pour une polyligne),
/// au plus toute la longueur du tracé. `route` inconnu du couloir → tout le
/// tracé.
fn fin_de_trace(
    route: &[i64],
    accrochage: &AccrochageVoirie,
    couloir: &std::collections::HashMap<u32, usize>,
    longueur_trace: usize,
) -> usize {
    let jusqua = route
        .iter()
        .filter_map(|id| accrochage.sommet_de.get(id))
        .filter_map(|s| couloir.get(s))
        .copied()
        .max()
        .map_or(longueur_trace, |r| r + 1);
    jusqua.min(longueur_trace).max(2.min(longueur_trace))
}

/// Rassemble ce qu'il faut au réseau et le construit.
fn construire_reseau(
    etat: &State<Etat>,
    modele: &str,
) -> Result<rusty_music_analysis::reseau::Reseau, String> {
    use rusty_music_analysis::reseau::{Morceau, Parametres, Reseau};
    use std::collections::HashMap;

    let lib = etat.lib.lock().map_err(echec)?;
    let empreintes = lib.embeddings(modele).map_err(echec)?;
    let vue = lib.map_view(modele).map_err(echec)?;
    if vue.is_empty() {
        return Err("aucun morceau sur la carte".into());
    }
    let points = lib.map_points(modele).map_err(echec)?;
    let mut parametres = lib.parametres_carte().map_err(echec)?.parametres_densite();
    drop(lib);

    // La popularité dont on dispose : le nombre de morceaux gardés d'un
    // artiste. La base ne porte aucun compteur d'écoute.
    let mut par_artiste: HashMap<String, u32> = HashMap::new();
    for p in &vue {
        *par_artiste.entry(p.artist.clone().unwrap_or_default()).or_default() += 1;
    }
    let index: HashMap<&str, u32> = par_artiste
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let morceaux: Vec<Morceau> = vue
        .iter()
        .map(|p| {
            let a = p.artist.clone().unwrap_or_default();
            Morceau {
                id: p.id,
                duree_ms: p.duration_ms.unwrap_or(0).max(0) as u64,
                artiste: index[a.as_str()],
                famille: p.cluster,
                x: p.x,
                y: p.y,
                morceaux_de_lartiste: par_artiste[&a],
            }
        })
        .collect();

    parametres.noyau = rusty_music_carto::relief::NOYAU;
    let champ = rusty_music_core::density::champ_global(&points, &parametres);
    Ok(Reseau::construire(
        empreintes,
        &morceaux,
        &champ,
        parametres.resolution,
        &Parametres {
            fils: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8),
            ..Default::default()
        },
    ))
}

/// Faut-il lancer l'autotest de la carte ?
///
/// `RUSTY_MUSIC_AUTOTEST=1`. Une webview du système ne se pilote pas de
/// l'extérieur : sans ce banc, les interactions ne se vérifient qu'à la main,
/// donc ne se vérifient pas.
#[tauri::command(async)]
fn autotest_carte() -> bool {
    std::env::var("RUSTY_MUSIC_AUTOTEST").is_ok_and(|v| v != "0")
}

/// Le mode dans lequel ouvrir l'application, si l'environnement en impose un.
///
/// `RUSTY_MUSIC_MODE=explorer` ouvre directement sur la carte. Sert aux essais
/// — une webview du système ne se pilote pas de l'extérieur — et rend service
/// à qui veut retrouver son mode au démarrage.
#[tauri::command(async)]
fn mode_initial() -> Option<String> {
    std::env::var("RUSTY_MUSIC_MODE").ok().filter(|v| !v.is_empty())
}

/// Renvoie au journal du processus ce que la console de la webview dit.
///
/// Une webview du système n'a pas d'inspecteur accessible depuis l'extérieur :
/// sans ce renvoi, une carte qui ne s'affiche pas ne dit rien du tout. C'est ce
/// qui a permis de trouver que MapLibre démarre dans la fenêtre principale et
/// pas dans une seconde.
#[tauri::command(async)]
fn journal_carte(niveau: String, message: String) {
    match niveau.as_str() {
        "error" => tracing::error!(target: "carte", "{message}"),
        "warn" => tracing::warn!(target: "carte", "{message}"),
        _ => tracing::info!(target: "carte", "{message}"),
    }
}

/// Les octets d'une tuile, pour `maplibregl.addProtocol`.
///
/// Rend un tableau vide quand la tuile n'existe pas : c'est le cas ordinaire
/// sur un monde creux, et MapLibre s'en accommode sans le compter comme une
/// erreur.
#[tauri::command]
async fn tuile(
    app: tauri::AppHandle,
    quoi: String,
    z: u8,
    x: u32,
    y: u32,
) -> Result<tauri::ipc::Response, String> {
    if !tuiles::archive_valide(&quoi) {
        return Err(format!("archive inconnue : {quoi}"));
    }
    let octets = tuiles::lire(&app, &quoi, z, x, y)
        .await
        .map_err(echec)?
        .unwrap_or_default();
    Ok(tauri::ipc::Response::new(octets))
}

/// Le style MapLibre, lu à côté des archives.
///
/// Il est **écrit en même temps que les tuiles**, et c'est délibéré : les deux
/// partagent les mêmes paliers et la même liste de familles, ils ne peuvent
/// plus diverger. Le recalculer à l'ouverture coûtait 42 secondes mesurées —
/// `Library::familles` refait tout l'arbitrage des genres MusicBrainz sur
/// 27 000 morceaux — pour un résultat identique à celui de la veille.
///
/// `theme` choisit la palette de fond de plan : `engendrer_tuiles` a écrit un
/// `style-<id>.json` par palette de [`rusty_music_carto::Palette`]. L'`id` est
/// filtré contre `Palette::par_id` (pas de `../`, pas de nom arbitraire), et on
/// retombe sur `style.json` si le fichier de thème manque — tuiles engendrées
/// avant cette fonctionnalité, ou thème `osm-clair`.
#[tauri::command(async)]
fn style_carte(
    app: tauri::AppHandle,
    theme: Option<String>,
) -> Result<serde_json::Value, String> {
    let dossier = tuiles::dossier(&app).map_err(echec)?;
    let chemin = match theme.as_deref() {
        Some(id)
            if id != "osm-clair"
                && rusty_music_carto::Palette::par_id(id).is_some()
                && dossier.join(format!("style-{id}.json")).is_file() =>
        {
            dossier.join(format!("style-{id}.json"))
        }
        _ => dossier.join("style.json"),
    };
    let texte = std::fs::read_to_string(&chemin)
        .map_err(|e| format!("style introuvable ({}) : {e}", chemin.display()))?;
    serde_json::from_str(&texte).map_err(echec)
}

/// Les positions réelles (lon/lat) des morceaux sur le plan de ville, écrites
/// par `engendrer_tuiles` à côté du style. Échoue sur le chemin fictif — pas
/// d'erreur applicative, `app.js` lit cet échec comme « pas de plan de ville
/// réel actif » et retombe sur les coordonnées t-SNE de `map_view`.
#[tauri::command(async)]
fn positions_carte(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let chemin = tuiles::dossier(&app).map_err(echec)?.join("positions.json");
    let texte = std::fs::read_to_string(&chemin)
        .map_err(|e| format!("positions introuvables ({}) : {e}", chemin.display()))?;
    serde_json::from_str(&texte).map_err(echec)
}

/// Rassemble ce qu'il faut aux deux archives : morceaux, familles, bandes, et
/// le champ continu du relief.
fn rassembler(
    etat: &State<Etat>,
) -> Result<(rusty_music_carto::source::Source, Vec<f64>, usize), String> {
    use rusty_music_carto::source;
    let modele = rusty_music_analysis::passe::MODELE;
    let lib = etat.lib.lock().map_err(echec)?;

    let vue = lib.map_view(modele).map_err(echec)?;
    if vue.is_empty() {
        return Err("aucun morceau sur la carte : lancer l'analyse d'abord".into());
    }
    let vue_gardee = vue.clone();
    let lib_ordre = lib.ordre_darrivee().map_err(echec)?;
    let empreintes_gardees: std::collections::HashMap<i64, Vec<f32>> =
        lib.embeddings(modele).map_err(echec)?.into_iter().collect();
    let familles: Vec<source::Famille> = lib
        .familles(modele)
        .map_err(echec)?
        .into_iter()
        .map(|(id, nom, effectif)| source::Famille {
            id,
            nom,
            effectif: effectif as usize,
        })
        .collect();
    let points = lib.map_points(modele).map_err(echec)?;
    let mut parametres = lib.parametres_carte().map_err(echec)?.parametres_densite();
    drop(lib);

    let nappe = rusty_music_core::density::calculer(&points, &parametres);
    // Le relief veut un champ plus lisse que les territoires : au noyau des
    // contours (0,02) l'ombrage ressemble à du papier froissé — vérifié à
    // l'œil sur la bibliothèque réelle.
    parametres.noyau = rusty_music_carto::relief::NOYAU;
    let champ = rusty_music_core::density::champ_global(&points, &parametres);

    // Le peuplement et le réseau entrent dans les tuiles au même titre que les
    // territoires : sans lieux ni routes, la carte n'est qu'une nappe.
    let ordre = lib_ordre;
    let par_id: std::collections::HashMap<i64, &MapPoint> =
        vue_gardee.iter().map(|p| (p.id, p)).collect();
    let arrivants: Vec<rusty_music_carto::peuplement::Arrivant> = ordre
        .iter()
        .filter_map(|a| {
            let p = par_id.get(&a.track_id)?;
            Some(rusty_music_carto::peuplement::Arrivant {
                track_id: a.track_id,
                x: p.x,
                y: p.y,
                empreinte: empreintes_gardees.get(&a.track_id).cloned().unwrap_or_default(),
                famille: p.cluster,
                date: a.date,
                artiste: p.artist.clone().unwrap_or_default(),
            })
        })
        .collect();
    let peupl = rusty_music_carto::peuplement::peupler(
        &arrivants,
        &rusty_music_carto::peuplement::Parametres::default(),
    );
    tracing::info!(
        etablissements = peupl.rapport.etablissements,
        iles = peupl.rapport.iles,
        "peuplement"
    );

    // **La parcelle remplace la coordonnée t-SNE.** C'est ce qui fait que les
    // morceaux habitent la carte au lieu de flotter dessus. Le CLI le faisait
    // déjà ; ce chemin-ci ne le faisait pas, et le bouton « refaire les
    // tuiles » rendait donc une carte différente de celle du CLI.
    let parcelles: std::collections::HashMap<i64, (f32, f32)> = peupl
        .habitants
        .iter()
        .map(|h| (h.track_id, (h.x, h.y)))
        .collect();
    let morceaux: Vec<source::Morceau> = vue_gardee
        .iter()
        .filter_map(|p| {
            let &(x, y) = parcelles.get(&p.id)?;
            Some(source::Morceau {
                id: p.id,
                x,
                y,
                famille: p.cluster,
                titre: p.title.clone().unwrap_or_default(),
                artiste: p.artist.clone().unwrap_or_default(),
                annee: p.year.map(|a| a as i32),
                bpm: p.bpm,
                energie: p.energy,
            })
        })
        .collect();

    // Le réseau de circulation, construit ici plutôt que réutilisé depuis
    // l'état : `rassembler` tourne dans une commande qui tient déjà le verrou
    // de la bibliothèque, et le réseau met une trentaine de secondes — le
    // partager demanderait de réordonner les verrous pour rien.
    let reseau = construire_reseau(etat, modele)?;
    let etab_de: std::collections::HashMap<i64, u32> = peupl
        .habitants
        .iter()
        .map(|h| (h.track_id, h.etablissement))
        .collect();
    let centres: std::collections::HashMap<u32, (f32, f32)> = peupl
        .etablissements
        .iter()
        .map(|e| (e.id, (e.cx, e.cy)))
        .collect();
    let routes_gardees = source::reseau_entre_lieux(
        &reseau.troncons_identifies(),
        &etab_de,
        &centres,
        &champ,
        parametres.resolution,
    );

    let rivieres = rusty_music_carto::hydro::tracer(
        &champ,
        parametres.resolution,
        &rusty_music_carto::hydro::Parametres::default(),
    );
    let curiosites = source::curiosites(&morceaux, &peupl.etablissements, &reseau.refuges(0.995), 60);

    Ok((
        source::Source {
            morceaux,
            familles,
            bandes: nappe.bandes,
            routes: routes_gardees,
            etablissements: peupl.etablissements,
            rivieres,
            curiosites,
            ..Default::default()
        },
        champ,
        parametres.resolution,
    ))
}

/// Trace un chemin et rend les pistes traversées, dans l'ordre du trajet.
///
/// `mode` choisit la fabrique — `chemin.rs` documente ce qui les distingue :
///
/// - `direct` : de `from` à `to`, en droite sur la carte, en `steps` morceaux ;
/// - `sonique` : de `from` à `to`, de voisin en voisin, longueur libre
///   plafonnée à `steps` ;
/// - `errance` : depuis `from` seul, `steps` morceaux tirés au sort.
///
/// Un mode inconnu retombe sur `direct` plutôt que d'échouer : l'interface est
/// la seule à appeler, une faute de frappe y est un bogue à voir, pas une
/// erreur à afficher.
///
/// `bruit` (0 à 1) est le cadran commun aux quatre modes — voir la note en
/// tête de `chemin.rs`. `0` reproduit le trajet exact ; les quatre fabriques
/// le traduisent chacune dans son propre registre (softmax pour l'errance,
/// arêtes bruitées pour le sonique, pont brownien pour le direct et le
/// dessiné). Réglable par un curseur du rail, visible dans les 4 modes.
const BRUIT_DEFAUT: f32 = 0.3;

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)] // interface de commande Tauri : chaque champ vient du JS
fn path(
    app: tauri::AppHandle,
    etat: State<Etat>,
    from: i64,
    to: Option<i64>,
    mode: Option<String>,
    steps: usize,
    seed: Option<u64>,
    bruit: Option<f32>,
    reel: Option<bool>,
    famille: Option<i64>,
) -> Result<Vec<TrackRow>, String> {
    use rusty_music_analysis::chemin;
    let mode = mode.unwrap_or_else(|| "direct".into());
    let graine = seed.unwrap_or(1);
    let bruit = bruit.unwrap_or(BRUIT_DEFAUT);
    let reel = reel.unwrap_or(false);
    let debut = std::time::Instant::now();

    // Filtre par famille : quand une famille est isolée dans Explorer, le
    // chemin ne doit traverser qu'elle. Le nuage passé au mode direct est
    // amputé des autres familles ; le graphe sonique est restreint.
    let permis = match famille {
        Some(f) => Some(morceaux_de_famille(&etat, f)?),
        None => None,
    };
    let carte_filtree = |etat: &State<Etat>, app: &tauri::AppHandle| -> Result<Vec<(i64, f32, f32)>, String> {
        let mut p = points_de_carte_effectifs(etat, app, reel)?;
        // Sur le plan de ville réel, les positions sont des lon/lat : on les
        // projette en mètres pour que « le plus proche » se mesure comme à
        // l'écran (voir [`RepereLocal`]). Le nuage t-SNE, lui, est déjà isotrope.
        if reel {
            RepereLocal::projeter_liste(&mut p);
        }
        if let Some(ref ids) = permis {
            p.retain(|(id, _, _)| ids.contains(id));
        }
        Ok(p)
    };

    // Le direct raisonne sur la carte, les deux autres sur les empreintes : on
    // ne charge que ce que le mode demande. Les empreintes font 55 Mo.
    let route = match mode.as_str() {
        "errance" => {
            let vecteurs = charger_vecteurs(&etat)?;
            let base = construire_graphe(&etat, &vecteurs)?;
            let restreint;
            let graphe: &Graphe = match &permis {
                Some(ids) => {
                    restreint = base.restreint(ids);
                    &restreint
                }
                None => &base,
            };
            graphe.errance(from, steps, graine, bruit)
        }
        "sonique" => {
            let arrivee = to.ok_or("le mode sonique demande une arrivée")?;
            let vecteurs = charger_vecteurs(&etat)?;
            let base = construire_graphe(&etat, &vecteurs)?;
            let restreint;
            let graphe: &Graphe = match &permis {
                Some(ids) => {
                    restreint = base.restreint(ids);
                    &restreint
                }
                None => &base,
            };
            let complet = graphe.sonique(from, arrivee, graine, bruit);
            if complet.is_empty() {
                // Les deux morceaux ne communiquent pas dans le graphe : mieux
                // vaut la droite que rien du tout. Le journal le dit,
                // l'interface l'annonce.
                tracing::info!(from, arrivee, "sonique impossible, repli sur direct");
                chemin::direct(&carte_filtree(&etat, &app)?, from, arrivee, steps, graine, bruit)
            } else {
                chemin::echantillonner(&complet, steps.max(2))
            }
        }
        _ => {
            let arrivee = to.ok_or("le mode direct demande une arrivée")?;
            chemin::direct(&carte_filtree(&etat, &app)?, from, arrivee, steps, graine, bruit)
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
/// (`[-1, 1]` sur les deux axes — ou en lon/lat sur le plan de ville réel,
/// voir [`points_de_carte_effectifs`]) ; `radius` la distance au-delà de
/// laquelle on ne cueille rien. L'interface la calcule depuis le zoom
/// courant, pour que le trait attrape exactement ce qu'il touche à l'écran.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)] // interface de commande Tauri : chaque champ vient du JS
fn path_drawn(
    app: tauri::AppHandle,
    etat: State<Etat>,
    trace: Vec<(f32, f32)>,
    steps: usize,
    radius: f32,
    seed: Option<u64>,
    bruit: Option<f32>,
    reel: Option<bool>,
    famille: Option<i64>,
) -> Result<Vec<TrackRow>, String> {
    let reel = reel.unwrap_or(false);
    let mut points = points_de_carte_effectifs(&etat, &app, reel)?;
    let mut trace = trace;
    // Plan de ville réel : positions ET tracé sont des lon/lat. On les projette
    // dans le même repère métrique (voir [`RepereLocal`]) ; `radius` arrive déjà
    // en mètres depuis l'interface (`tracerDessin`, branche `carteReelle`).
    if reel && ressemble_a_lon_lat(&points) {
        let rep = RepereLocal::autour(&points);
        for p in &mut points {
            let (x, y) = rep.projeter(p.1, p.2);
            (p.1, p.2) = (x, y);
        }
        for t in &mut trace {
            let (x, y) = rep.projeter(t.0, t.1);
            *t = (x, y);
        }
    }
    // Filtre par famille : le trait ne cueille que dans la famille isolée.
    if let Some(f) = famille {
        let ids = morceaux_de_famille(&etat, f)?;
        points.retain(|(id, _, _)| ids.contains(id));
    }
    let route = rusty_music_analysis::chemin::dessine(
        &points,
        &trace,
        steps,
        radius,
        seed.unwrap_or(1),
        bruit.unwrap_or(BRUIT_DEFAUT),
    );
    let pistes = pistes_de(&etat, &route)?;
    tracing::info!(n = pistes.len(), points = trace.len(), "chemin dessiné");
    Ok(pistes)
}

/// Les segments de rues réelles entre chaque paire consécutive d'un
/// itinéraire déjà choisi — habille le trait dessiné sur le plan de ville
/// réel, sans toucher au choix des morceaux. Ce choix reste entièrement
/// celui du réseau **sonique** (`itineraire` ci-dessus, ou `path`/
/// `path_drawn` pour direct/dessiné) : la question ici n'est pas « quels
/// morceaux » mais « quel trait dessiner entre eux ».
///
/// `ids` est la liste ordonnée des morceaux d'un chemin déjà tracé ; le
/// résultat a un élément de moins, un `Vec` de points `[lon, lat]` par paire
/// consécutive. Un `Vec` vide en position `i` marque un segment sans chemin
/// trouvé (l'une des deux adresses n'a pas de position réelle, ou les deux
/// n'appartiennent pas à la même composante routable) — `app.js` retombe
/// alors sur le trait synthétique existant pour ce segment précis, pas pour
/// tout l'itinéraire.
#[tauri::command(async)]
fn trace_rues(app: tauri::AppHandle, etat: State<Etat>, ids: Vec<i64>) -> Result<Vec<Vec<[f64; 2]>>, String> {
    if ids.len() < 2 {
        return Ok(Vec::new());
    }
    let Some(chemin_ville) = plan_de_ville(&etat.db) else {
        return Ok(Vec::new());
    };
    let extrait = charger_ville(&etat, &chemin_ville)?;
    let graphe = charger_graphe_reel(&etat, &extrait)?;
    // Habillage du trait par les vraies rues : n'a de sens que sur le plan de
    // ville, et raisonne sur les adresses réelles quelle que soit la vue.
    let points = points_de_carte_effectifs(&etat, &app, true)?;
    let position = |id: i64| points.iter().find(|(i, _, _)| *i == id).map(|(_, x, y)| [*x as f64, *y as f64]);

    let segments: Vec<Vec<[f64; 2]>> = ids
        .windows(2)
        .map(|paire| match (position(paire[0]), position(paire[1])) {
            (Some(a), Some(b)) => graphe.chemin(a, b).unwrap_or_default(),
            _ => Vec::new(),
        })
        .collect();
    tracing::info!(n = segments.iter().filter(|s| !s.is_empty()).count(), sur = segments.len(), "trait habillé de rues réelles");
    Ok(segments)
}

/// Reprojette des positions `[lon, lat]` en mètres locaux (équirectangulaire).
///
/// `direct` et `dessine` cueillent, à chaque pas, le morceau **le plus proche**
/// du point visé, et `dessine` rééchantillonne le tracé à pas d'arc constant.
/// En degrés bruts, ces deux mesures sont faussées : à la latitude de Paris un
/// degré de longitude vaut ~0,66 degré de latitude, donc « le plus proche »
/// tirait vers le nord-sud et le trait ne suivait pas la ligne visée. On projette
/// d'abord en mètres — l'origine est quelconque (translation sans effet ni sur
/// « le plus proche » ni sur une longueur d'arc), seul compte le rapport des
/// axes.
struct RepereLocal {
    lon0: f64,
    lat0: f64,
    cos_lat0: f64,
}

impl RepereLocal {
    fn autour(points: &[(i64, f32, f32)]) -> RepereLocal {
        let n = points.len().max(1) as f64;
        let lon0 = points.iter().map(|p| p.1 as f64).sum::<f64>() / n;
        let lat0 = points.iter().map(|p| p.2 as f64).sum::<f64>() / n;
        RepereLocal { lon0, lat0, cos_lat0: lat0.to_radians().cos() }
    }

    fn projeter(&self, lon: f32, lat: f32) -> (f32, f32) {
        const R: f64 = 6_371_000.0;
        (
            ((lon as f64 - self.lon0).to_radians() * self.cos_lat0 * R) as f32,
            ((lat as f64 - self.lat0).to_radians() * R) as f32,
        )
    }

    /// Reprojette une liste de positions en place — sans effet si elles ne
    /// ressemblent pas à des lon/lat (garde-fou : `points_de_carte_effectifs`
    /// retombe en silence sur le t-SNE si `positions.json` manque, et une
    /// latitude t-SNE tourne autour de 0, pas de 48,8).
    fn projeter_liste(points: &mut [(i64, f32, f32)]) {
        if !ressemble_a_lon_lat(points) {
            return;
        }
        let rep = RepereLocal::autour(points);
        for p in points.iter_mut() {
            let (x, y) = rep.projeter(p.1, p.2);
            (p.1, p.2) = (x, y);
        }
    }
}

/// Les positions ressemblent-elles à des lon/lat (plan de ville réel) plutôt
/// qu'à des coordonnées t-SNE ? La latitude d'une vraie ville est loin de 0 ;
/// le nuage t-SNE tient dans `[-1,1]²`.
fn ressemble_a_lon_lat(points: &[(i64, f32, f32)]) -> bool {
    let Some(&(_, _, lat)) = points.first() else { return false };
    lat.abs() > 5.0 && lat.abs() < 89.0
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

/// Les identifiants des morceaux d'une famille (cluster de la carte).
///
/// Le filtre par famille du mode Explorer (`carte.isolee`, `app.js`) borne
/// aussi le calcul d'un chemin : quand une famille est isolée, la playlist ne
/// doit contenir que des morceaux de cette famille — les points hors famille
/// sont retirés du nuage avant `chemin::direct`/`dessine`, et le graphe
/// sonique est restreint (voir [`Graphe::restreint`]).
fn morceaux_de_famille(etat: &State<Etat>, famille: i64) -> Result<HashSet<i64>, String> {
    let lib = etat.lib.lock().map_err(echec)?;
    Ok(lib
        .map_points(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?
        .into_iter()
        .filter(|(_, _, _, f)| *f == famille)
        .map(|(id, _, _, _)| id)
        .collect())
}

/// Les coordonnées **effectivement affichées** — celles que `versEcran`/
/// `versCarte` d'`app.js` utilisent, dans le même repère.
///
/// Sur le plan de ville réel, un morceau n'habite plus sa position t-SNE
/// mais une adresse (`positions.json`, écrit par `engendrer_tuiles` à côté
/// des tuiles) : le lasso, le tracé dessiné et le mode direct doivent
/// raisonner sur cette même adresse, sinon ils testent un contour dessiné
/// sur Paris contre des positions qui n'ont plus aucun rapport avec l'écran.
///
/// **Mais uniquement quand c'est le plan de ville qui est à l'écran.** Le
/// nuage t-SNE et le plan de ville coexistent (bouton « Points » / « Carte »
/// du rail) : dans le nuage, `versEcran`/`versCarte` restent en t-SNE même
/// quand une ville est importée. L'interface passe donc `reel` — vrai
/// seulement quand `carte.affichage === "carte" && villeReelle` — et sur le
/// nuage on retombe sur les positions t-SNE, celles que le geste vise
/// réellement.
///
/// Retombe aussi sur [`points_de_carte`] (t-SNE) si `positions.json` est
/// absent ou vide — le chemin fictif, ou aucune génération de tuiles pas
/// encore lancée.
fn points_de_carte_effectifs(
    etat: &State<Etat>,
    app: &tauri::AppHandle,
    reel: bool,
) -> Result<Vec<(i64, f32, f32)>, String> {
    if reel {
        if let Ok(dossier) = tuiles::dossier(app) {
            if let Ok(texte) = std::fs::read_to_string(dossier.join("positions.json")) {
                if let Ok(reelles) =
                    serde_json::from_str::<std::collections::HashMap<i64, [f32; 2]>>(&texte)
                {
                    if !reelles.is_empty() {
                        return Ok(reelles.into_iter().map(|(id, [x, y])| (id, x, y)).collect());
                    }
                }
            }
        }
    }
    points_de_carte(etat)
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
    let en_cache = |etat: &State<Etat>| -> Result<Option<Arc<Graphe>>, String> {
        Ok(etat
            .graphe
            .lock()
            .map_err(echec)?
            .as_ref()
            .filter(|(taille, _)| *taille == n)
            .map(|(_, g)| Arc::clone(g)))
    };
    if let Some(g) = en_cache(etat)? {
        return Ok(g);
    }

    // Un seul balayage à la fois. Ceux qui attendent ici retrouvent, une fois
    // le verrou obtenu, le cache déjà rempli par le premier — d'où la seconde
    // vérification avant de se lancer à son tour.
    let _construction = etat.graphe_construction.lock().map_err(echec)?;
    if let Some(g) = en_cache(etat)? {
        return Ok(g);
    }

    let debut = std::time::Instant::now();
    etat.graphe_fait.store(0, Ordering::Relaxed);
    etat.graphe_total.store(n, Ordering::Relaxed);
    let neuf = Arc::new(Graphe::construire_suivi(
        vecteurs,
        rusty_music_analysis::chemin::K_VOISINS,
        coeurs_arriere_plan(),
        &etat.graphe_fait,
    ));
    etat.graphe_total.store(0, Ordering::Relaxed);
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

/// La famille sonique dominante de chaque album — le filtre par famille de la
/// grille de pochettes du mode Écoute (`app.js`), qui réutilise la légende des
/// familles du mode Explorer.
#[tauri::command(async)]
fn album_families(etat: State<Etat>) -> Result<Vec<(String, Option<String>, i64)>, String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .familles_des_albums(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// La famille sonique dominante de chaque artiste (`mb_album_artist_id`) — le
/// filtre par famille du fil du mode Découvrir (`app.js`), qui réutilise la
/// légende des familles du mode Explorer.
#[tauri::command(async)]
fn artist_families(etat: State<Etat>) -> Result<Vec<(String, i64)>, String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .familles_des_artistes(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

#[tauri::command(async)]
fn map_parameters(etat: State<Etat>) -> Result<rusty_music_core::db::ParametresCarte, String> {
    etat.lib.lock().map_err(echec)?.parametres_carte().map_err(echec)
}

/// `cle` doit être un champ de [`rusty_music_core::db::ParametresCarte`] —
/// vérifié ici plutôt que laissé filer jusqu'à la base, qui accepterait
/// n'importe quelle chaîne sans jamais la relire.
#[tauri::command(async)]
fn set_map_parameter(etat: State<Etat>, cle: String, valeur: f64) -> Result<(), String> {
    const CLES: [&str; 7] = [
        "perplexite",
        "epoques",
        "familles",
        "iterations_kmeans",
        "densite_noyau",
        "densite_resolution",
        "densite_bandes",
    ];
    if !CLES.contains(&cle.as_str()) {
        return Err(format!("paramètre inconnu : {cle}"));
    }
    etat.lib
        .lock()
        .map_err(echec)?
        .set_parametre_carte(&cle, valeur)
        .map_err(echec)
}

/// Le vocabulaire des familles par genre, dans l'ordre d'affichage.
#[tauri::command(async)]
fn vocabulaire_familles(etat: State<Etat>) -> Result<Vec<(String, Vec<String>)>, String> {
    etat.lib.lock().map_err(echec)?.vocabulaire_familles().map_err(echec)
}

/// Remplace le vocabulaire en bloc. Une liste vide restaure les valeurs par
/// défaut — voir `Library::definir_vocabulaire_familles`.
///
/// Ne relance rien elle-même : comme pour `set_map_parameter`, c'est
/// `project` (rappelé par l'interface après coup) qui recalcule la carte
/// avec le nouveau vocabulaire.
#[tauri::command(async)]
fn definir_vocabulaire_familles(
    etat: State<Etat>,
    vocabulaire: Vec<(String, Vec<String>)>,
) -> Result<(), String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .definir_vocabulaire_familles(&vocabulaire)
        .map_err(echec)
}

/// Recalcule la nappe de densité depuis les positions actuellement en base
/// et la met en cache. À rappeler après toute projection/clustering réussi
/// — et seulement alors : jamais par image, jamais au zoom, c'est tout
/// l'intérêt du cache (voir `rusty_music_core::density`).
fn recalculer_densite(etat: &Etat, lib: &Library) -> Result<(), String> {
    let parametres = lib.parametres_carte().map_err(echec)?.parametres_densite();
    let points = lib
        .map_points(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;
    let resultat = rusty_music_core::density::calculer(&points, &parametres);
    *etat.densite.lock().map_err(echec)? = Some(resultat);
    Ok(())
}

/// La nappe de densité de la carte — polygones prêts à remplir, une teinte
/// par famille plus une nappe globale (voir `rusty_music_core::density`).
///
/// Servie depuis le cache, rempli après chaque projection/clustering
/// ([`recompute_map`] et la passe complète d'analyse). Calculée à la volée
/// seulement au tout premier appel d'une session dont la carte porte déjà
/// des positions — l'appli vient de (re)démarrer sans repasser par
/// « Recalculer la carte ».
#[tauri::command(async)]
fn density_view(etat: State<Etat>) -> Result<rusty_music_core::density::ResultatDensite, String> {
    if let Some(r) = etat.densite.lock().map_err(echec)?.as_ref() {
        return Ok(r.clone());
    }
    let lib = etat.lib.lock().map_err(echec)?;
    recalculer_densite(&etat, &lib)?;
    Ok(etat
        .densite
        .lock()
        .map_err(echec)?
        .clone()
        .expect("recalculée juste au-dessus"))
}

/// `passe::Rapport` ne dérive pas `Serialize` — `rusty-music-analysis` ne
/// dépend pas de `serde`, et ce n'est pas ce seul retour de commande qui
/// justifie de le lui ajouter.
#[derive(serde::Serialize)]
struct RapportCarte {
    empreintes: usize,
    familles: usize,
}

/// Rejoue la projection t-SNE et le clustering sur les empreintes déjà là,
/// avec les paramètres actuels — pas l'encodage CLAP, qui ne dépend
/// d'aucun d'eux. Bon marché : quelques secondes à quelques dizaines de
/// secondes sur toute la bibliothèque, pas besoin du fil séparé et du
/// sondage qu'`Analyser` demande.
#[tauri::command(async)]
fn recompute_map(etat: State<Etat>) -> Result<RapportCarte, String> {
    let lib = etat.lib.lock().map_err(echec)?;
    let r = rusty_music_analysis::passe::projeter_tout(&lib, None).map_err(|e| e.to_string())?;
    recalculer_densite(&etat, &lib)?;
    Ok(RapportCarte {
        empreintes: r.empreintes,
        familles: r.familles,
    })
}

/// Rejoue seulement la nappe de densité, sur les positions et familles
/// déjà en base — pas la projection t-SNE ni le clustering, qui n'en
/// dépendent pas. C'est cette commande que le rail appelle quand on ajuste
/// la résolution, le noyau ou le nombre de bandes : quelques centaines de
/// millisecondes plutôt que de rejouer `recompute_map` en entier.
#[tauri::command(async)]
fn recompute_density(etat: State<Etat>) -> Result<(), String> {
    let lib = etat.lib.lock().map_err(echec)?;
    recalculer_densite(&etat, &lib)
}

/// Sous ce carré de distance entre empreintes CLAP, deux morceaux sonnent au
/// point d'être indiscernables — mesuré sur la bibliothèque réelle : la
/// distance médiane entre plus proches voisins y vaut 0,044, son 1ᵉʳ
/// centile 0,0059. En dessous de ce seuil-ci, 66 paires sur 27 042 morceaux
/// — un ordre de grandeur sous le 1ᵉʳ centile, pas un effet de bord de la
/// distribution générale.
const SEUIL_DOUBLON_D2: f32 = 1e-3;

/// Tolérance de durée entre deux morceaux suspectés doublons. Un peu de jeu
/// pour l'arrondi des tags, pas assez pour confondre deux versions
/// réellement différentes (single édité / album, live).
const TOLERANCE_DUREE_MS: i64 = 3000;

#[derive(Clone, serde::Serialize)]
struct DoublonProbable {
    a: TrackRow,
    b: TrackRow,
    distance2: f32,
}

/// Paires de morceaux dont l'empreinte sonore est quasi identique — voir
/// [`SEUIL_DOUBLON_D2`]. Biaisé vers le silence plutôt que le faux
/// positif : un doublon manqué reste juste un doublon, un faux doublon
/// signalé use la confiance dans le reste de la liste.
#[tauri::command(async)]
fn probable_duplicates(etat: State<Etat>) -> Result<Vec<DoublonProbable>, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let graphe = construire_graphe(&etat, &vecteurs)?;
    let lib = etat.lib.lock().map_err(echec)?;

    let mut vus: HashSet<(i64, i64)> = HashSet::new();
    let mut paires = Vec::new();
    for (id, voisin, d2) in graphe.plus_proches() {
        if d2 >= SEUIL_DOUBLON_D2 {
            continue;
        }
        // Chaque paire proche se présente deux fois (une fois depuis chaque
        // morceau) : la clé triée ne la garde qu'une.
        let cle = (id.min(voisin), id.max(voisin));
        if !vus.insert(cle) {
            continue;
        }
        let (Some(a), Some(b)) = (lib.track(cle.0).map_err(echec)?, lib.track(cle.1).map_err(echec)?)
        else {
            continue;
        };
        if let (Some(da), Some(db)) = (a.duration_ms, b.duration_ms) {
            if (da - db).abs() > TOLERANCE_DUREE_MS {
                continue;
            }
        }
        paires.push(DoublonProbable { a, b, distance2: d2 });
    }
    paires.sort_by(|x, y| x.distance2.total_cmp(&y.distance2));
    Ok(paires)
}

#[derive(Clone, serde::Serialize)]
struct PointIsole {
    piste: TrackRow,
    plus_proche: TrackRow,
    distance2: f32,
}

/// Morceaux dont même le plus proche voisin reste sonoremement lointain —
/// des coins délaissés de la carte, sans repère pour y aller depuis un
/// autre morceau connu. Seuil auto-calibré sur la bibliothèque du moment
/// (99ᵉ centile des distances au plus proche voisin) plutôt qu'une
/// constante absolue : ce qui compte comme « loin » dépend de la densité
/// propre à chaque bibliothèque, pas d'un nombre choisi une fois pour
/// toutes.
#[tauri::command(async)]
fn isolated_points(etat: State<Etat>) -> Result<Vec<PointIsole>, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let graphe = construire_graphe(&etat, &vecteurs)?;
    let lib = etat.lib.lock().map_err(echec)?;

    let mut proches = graphe.plus_proches();
    if proches.is_empty() {
        return Ok(Vec::new());
    }
    let mut distances: Vec<f32> = proches.iter().map(|(_, _, d2)| *d2).collect();
    distances.sort_by(f32::total_cmp);
    let seuil = distances[((distances.len() - 1) as f64 * 0.99) as usize];

    proches.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut points = Vec::new();
    for (id, voisin, d2) in proches {
        if d2 < seuil {
            break;
        }
        let (Some(piste), Some(plus_proche)) =
            (lib.track(id).map_err(echec)?, lib.track(voisin).map_err(echec)?)
        else {
            continue;
        };
        points.push(PointIsole { piste, plus_proche, distance2: d2 });
    }
    Ok(points)
}

/// Les morceaux d'une zone dessinée sur la carte.
///
/// `trace` est le contour, en coordonnées de carte. Les morceaux retenus sont
/// rendus **ordonnés en parcours de proche en proche** et non dans l'ordre de
/// la base : une zone donne des dizaines de morceaux, et les enchaîner au
/// hasard produirait une playlist qui saute d'un bout à l'autre.
#[tauri::command(async)]
fn selection(
    app: tauri::AppHandle,
    etat: State<Etat>,
    trace: Vec<(f32, f32)>,
    reel: Option<bool>,
    famille: Option<i64>,
) -> Result<Vec<TrackRow>, String> {
    if trace.len() < 3 {
        return Ok(Vec::new());
    }
    let points = points_de_carte_effectifs(&etat, &app, reel.unwrap_or(false))?;
    let permis = match famille {
        Some(f) => Some(morceaux_de_famille(&etat, f)?),
        None => None,
    };

    let dedans: Vec<i64> = points
        .iter()
        .filter(|(_, x, y)| dans_le_contour(&trace, *x, *y))
        .map(|(id, _, _)| *id)
        .filter(|id| permis.as_ref().is_none_or(|ids| ids.contains(id)))
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

/// Playlist « dans l'esprit de l'album » : une errance ordinaire, mais partie
/// du morceau le plus central de l'album plutôt que d'un seul morceau choisi
/// à la main. `chemin::parcours` donne ce pivot — le morceau dont l'empreinte
/// est la plus proche de la moyenne de l'album — et filtre déjà les morceaux
/// sans empreinte, donc un album partiellement analysé n'est pas un problème.
#[tauri::command(async)]
fn path_album(
    etat: State<Etat>,
    album: String,
    artist: Option<String>,
    steps: usize,
    seed: u64,
    bruit: Option<f32>,
) -> Result<Vec<TrackRow>, String> {
    let ids: Vec<i64> = {
        let lib = etat.lib.lock().map_err(echec)?;
        lib.tracks_of_album(&album, artist.as_deref())
            .map_err(echec)?
            .into_iter()
            .map(|t| t.id)
            .collect()
    };

    let vecteurs = charger_vecteurs(&etat)?;
    let pivot = *rusty_music_analysis::chemin::parcours(&vecteurs, &ids)
        .first()
        .ok_or("aucun morceau analysé pour cet album")?;

    let route = construire_graphe(&etat, &vecteurs)?.errance(
        pivot,
        steps,
        seed,
        bruit.unwrap_or(BRUIT_DEFAUT),
    );
    pistes_de(&etat, &route)
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

/// `(fait, total)` du balayage qui construit le graphe des voisins.
///
/// `total == 0` : rien en cours — jamais construit, ou déjà en cache et donc
/// instantané. L'interface sonde ça pendant l'attente de la première playlist
/// « dans l'esprit de » pour montrer une jauge plutôt qu'une roulette muette.
#[tauri::command(async)]
fn graphe_progress(etat: State<Etat>) -> (i64, i64) {
    (
        etat.graphe_fait.load(Ordering::Relaxed) as i64,
        etat.graphe_total.load(Ordering::Relaxed) as i64,
    )
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

/// Statistiques d'ensemble du mode Bibliothèque — un aperçu, pas une passe :
/// trois requêtes d'agrégation sur la base déjà là, rien à mesurer ni à
/// décoder. Rejouée à chaque entrée dans le mode plutôt que mise en cache :
/// à 27 000 morceaux, l'agrégation reste sous la seconde.
#[derive(Clone, serde::Serialize)]
struct StatsBibliotheque {
    total: i64,
    genres: Vec<(String, i64)>,
    tempo: rusty_music_core::db::Histogramme,
    durees: rusty_music_core::db::Histogramme,
    codecs: Vec<(String, i64)>,
    bitrate: rusty_music_core::db::Histogramme,
    sans_mbid: i64,
    humeur: Vec<(String, i64)>,
}

#[tauri::command(async)]
fn library_stats(etat: State<Etat>) -> Result<StatsBibliotheque, String> {
    let lib = etat.lib.lock().map_err(echec)?;
    Ok(StatsBibliotheque {
        total: lib.count().map_err(echec)?,
        genres: lib.stats_genres().map_err(echec)?,
        tempo: lib.stats_tempo().map_err(echec)?,
        durees: lib.stats_durees().map_err(echec)?,
        codecs: lib.stats_codecs().map_err(echec)?,
        bitrate: lib.stats_bitrate().map_err(echec)?,
        sans_mbid: lib.stats_sans_mbid().map_err(echec)?,
        humeur: lib.stats_humeur().map_err(echec)?,
    })
}

/// Morceaux dont le genre résolu ne figure pas parmi les dominants de leur
/// famille sonique — voir [`rusty_music_core::db::Library::genres_suspects`].
#[tauri::command(async)]
fn suspect_genres(etat: State<Etat>) -> Result<Vec<(i64, String, String, String)>, String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .genres_suspects(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// Albums présents sous plusieurs éditions chez le même artiste.
#[tauri::command(async)]
fn multiple_editions(etat: State<Etat>) -> Result<Vec<rusty_music_core::db::EditionsAlbum>, String> {
    etat.lib.lock().map_err(echec)?.editions_multiples().map_err(echec)
}

/// Fichiers en échec de scan, du plus récent au plus ancien.
#[tauri::command(async)]
fn scan_failures(etat: State<Etat>) -> Result<Vec<(String, String, i64)>, String> {
    etat.lib.lock().map_err(echec)?.echecs_scan().map_err(echec)
}

/// Retire un fichier de la liste des échecs, sans y toucher sur le disque —
/// pour un fichier qu'on sait perdu et qu'on ne veut plus voir revenir à
/// chaque scan.
#[tauri::command(async)]
fn dismiss_scan_failure(etat: State<Etat>, path: String) -> Result<(), String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .effacer_echec_scan(Path::new(&path))
        .map_err(echec)
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

/// Avancement de la passe de popularité générale, sondé par l'interface.
#[derive(Default, Clone, serde::Serialize)]
struct EtatPopularite {
    en_cours: bool,
    faits: usize,
    total: usize,
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

        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|lib| {
                let fils = fils_pour_passe(&lib);
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
                    rusty_music_analysis::passe::projeter_tout(&lib, None)
                        .map_err(|e| e.to_string())
                        .and_then(|p| {
                            recalculer_densite(&etat, &lib).map(|()| (r, p))
                        })
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

/// Avancement de la mesure tempo/tonalité/énergie, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
struct EtatDescripteurs {
    en_cours: bool,
    faits: usize,
    total: usize,
    resultat: Option<String>,
}

/// Combien de morceaux de la carte ont déjà leurs descripteurs.
#[tauri::command(async)]
fn descripteurs_progress(etat: State<Etat>) -> Result<(i64, i64), String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .compter_descripteurs(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// Mesure tempo/tonalité/énergie des morceaux en attente — ou de tous,
/// `force` effaçant d'abord ce qui est déjà mesuré.
///
/// Sépare cette passe de `start_analysis` (empreintes CLAP) : les deux
/// n'ont ni le même coût ni la même fréquence de relance. Un correctif de
/// l'algorithme (tempo doublé sur les rythmiques marquées, par exemple)
/// justifie de remesurer une bibliothèque déjà couverte — les empreintes,
/// elles, n'ont pas de raison de changer sans changer de modèle.
#[tauri::command(async)]
fn start_descripteurs(app: tauri::AppHandle, etat: State<Etat>, force: Option<bool>) -> Result<(), String> {
    let force = force.unwrap_or(false);
    {
        let mut d = etat.descripteurs.lock().map_err(echec)?;
        if d.en_cours {
            return Err("une mesure est déjà en cours".into());
        }
        *d = EtatDescripteurs {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();

        let issue = Library::open(&db).map_err(|e| e.to_string()).and_then(|lib| {
            let fils = fils_pour_passe(&lib);
            if force {
                lib.effacer_descripteurs().map_err(|e| e.to_string())?;
            }
            rusty_music_analysis::passe::descripteurs(&lib, i64::MAX, fils, |faits, total| {
                if let Ok(mut d) = etat.descripteurs.lock() {
                    d.faits = faits;
                    d.total = total;
                }
            })
            .map_err(|e| e.to_string())
        });

        let bilan = match issue {
            Ok(r) => format!(
                "{} mesurés · {} sans tempo · {} sans tonalité · {} en échec",
                r.mesures, r.sans_tempo, r.sans_tonalite, r.echecs
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "mesure des descripteurs terminée");

        let verrou = etat.descripteurs.lock();
        if let Ok(mut d) = verrou {
            d.en_cours = false;
            d.resultat = Some(bilan);
        }
    });

    Ok(())
}

#[tauri::command(async)]
fn descripteurs_state(etat: State<Etat>) -> Result<EtatDescripteurs, String> {
    Ok(etat.descripteurs.lock().map_err(echec)?.clone())
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

/// Au-delà de combien de jours une popularité déjà récupérée redevient « à
/// faire » quand on demande un rafraîchissement. Une notoriété bouge lentement.
const POP_PEREMPTION_JOURS: i64 = 90;

/// Lance la passe de popularité générale (ListenBrainz + Deezer).
///
/// Comme l'enrichissement : fil séparé, connexion propre, reprenable et
/// additive. Mais **aucune clé ni compte** — ListenBrainz et Deezer sont des
/// API publiques. `contact`, s'il est renseigné, part dans le `User-Agent` de
/// ListenBrainz par courtoisie ; son absence ne saute rien.
///
/// `rafraichir` : à `false`, on ne comble que les trous ; à `true`, on
/// réinterroge aussi ce qui date de plus de [`POP_PEREMPTION_JOURS`].
#[tauri::command(async)]
fn start_popularite(
    app: tauri::AppHandle,
    etat: State<Etat>,
    contact: String,
    rafraichir: bool,
) -> Result<(), String> {
    {
        let mut p = etat.popularite.lock().map_err(echec)?;
        if p.en_cours {
            return Err("une passe de popularité est déjà en cours".into());
        }
        *p = EtatPopularite {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    let contact = contact.trim().to_string();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let lb = rusty_music_core::listenbrainz::Client::new(&contact);
        let dz = rusty_music_core::deezer::Client::new();
        // `depuis` : instant avant lequel une entité déjà interrogée redevient
        // « à faire ». `0` (par défaut) ne rafraîchit rien.
        let depuis = if rafraichir {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64 - POP_PEREMPTION_JOURS * 86_400)
                .unwrap_or(0)
        } else {
            0
        };
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            rusty_music_core::popularite::actualiser(&mut lib, &lb, &dz, depuis, usize::MAX, |b| {
                if let Ok(mut p) = etat.popularite.lock() {
                    p.faits = b.faits;
                    p.total = b.total;
                }
            })
            .map_err(|e| e.to_string())
        })();

        let bilan = match issue {
            Ok(b) => format!(
                "{} enregistrements + {} albums (ListenBrainz), \
                 {} / {} pistes retrouvées (Deezer), {} morceaux couverts",
                b.lb_enregistrements, b.lb_albums, b.deezer_trouves, b.deezer, b.couverts
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "passe de popularité terminée");

        let mut fin = etat.popularite.lock();
        if let Ok(p) = fin.as_mut() {
            p.en_cours = false;
            p.resultat = Some(bilan);
        }
    });
    Ok(())
}

/// Où en est la passe de popularité.
#[tauri::command(async)]
fn popularite_state(etat: State<Etat>) -> Result<EtatPopularite, String> {
    Ok(etat.popularite.lock().map_err(echec)?.clone())
}

/// Fraîcheur de la popularité pour la ligne d'alerte du mode Bibliothèque :
/// `(morceaux couverts, epoch de la plus ancienne interrogation, entités de
/// plus de 90 jours)`.
#[tauri::command(async)]
fn popularite_fraicheur(etat: State<Etat>) -> Result<(i64, Option<i64>, i64), String> {
    etat.lib
        .lock()
        .map_err(echec)?
        .popularite_fraicheur(POP_PEREMPTION_JOURS)
        .map_err(echec)
}

/// Les collaborateurs d'un artiste — mode Découvrir. Sert du cache s'il y
/// en a un, sinon interroge MusicBrainz et le remplit.
///
/// Pas de fil séparé ni de sondage comme `start_enrichment` : ici, une
/// seule requête réseau au plus (une seconde environ), pas une passe sur
/// toute la bibliothèque — inutile d'habiller ça d'un état de fond.
#[tauri::command(async)]
fn artist_links(
    etat: State<Etat>,
    mbid: String,
    contact: String,
) -> Result<Vec<(String, String, String)>, String> {
    {
        let lib = etat.lib.lock().map_err(echec)?;
        if lib.liens_artiste_en_cache(&mbid).map_err(echec)? {
            return lib.liens_artiste(&mbid).map_err(echec);
        }
    }

    let contact = contact.trim().to_string();
    if !contact.contains('@') {
        return Err("MusicBrainz demande une adresse de contact valable.".into());
    }

    // Hors verrou, comme le scan et l'enrichissement : ne pas geler la base
    // pendant un aller-retour réseau.
    let client = rusty_music_core::musicbrainz::Client::new(&contact);
    let liens = client.relations_artiste(&mbid).map_err(|e| e.to_string())?;

    let mut lib = etat.lib.lock().map_err(echec)?;
    lib.enregistrer_liens_artiste(&mbid, &liens).map_err(echec)?;
    lib.liens_artiste(&mbid).map_err(echec)
}

/// Avancement de la passe du mode Découvrir, sondé par l'interface — comme
/// `enrichment_state`.
#[derive(Clone, Default, serde::Serialize)]
struct EtatDecouvrir {
    en_cours: bool,
    artistes: usize,
    total: usize,
    sorties_neuves: usize,
    voisins_neufs: usize,
    resultat: Option<String>,
}

/// Le fil du mode Découvrir, tel que la base le tient.
///
/// Lecture seule : `start_decouvrir` fait le travail réseau. La fenêtre est
/// fixée à un mois — c'est l'esprit du mode, une actualité.
#[tauri::command(async)]
fn decouvrir_feed(etat: State<Etat>) -> Result<rusty_music_core::db::FilDecouvrir, String> {
    etat.lib.lock().map_err(echec)?.decouvrir_fil(30).map_err(echec)
}

#[tauri::command(async)]
fn decouvrir_state(etat: State<Etat>) -> Result<EtatDecouvrir, String> {
    Ok(etat.decouvrir.lock().map_err(echec)?.clone())
}

/// Marque tout le fil comme vu — les pastilles « nouveau » s'éteignent.
#[tauri::command(async)]
fn decouvrir_tout_vu(etat: State<Etat>) -> Result<(), String> {
    etat.lib.lock().map_err(echec)?.decouvrir_tout_vu().map_err(echec)
}

/// La pochette d'une sortie du fil Découvrir, en `data:` URI — comme `cover`,
/// mais servie depuis Cover Art Archive plutôt que des tags d'un fichier local.
///
/// Cache disque partagé avec les pochettes locales (`<données app>/pochettes/`,
/// clé `caa-<mbid>`) : une vignette déjà récupérée ne repart pas sur le réseau,
/// et un album sans pochette (fréquent le mois de sa sortie) n'est pas
/// redemandé. Le cache mémoire côté interface fait le reste pendant la session.
#[tauri::command]
async fn decouvrir_pochette(
    etat: State<'_, Etat>,
    rg_mbid: String,
) -> Result<Option<String>, String> {
    // L'identifiant vient du fil, mais on le vérifie : il sert de nom de
    // fichier de cache et part dans une URL.
    if rg_mbid.len() != 36 || !rg_mbid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("identifiant de release-group invalide".into());
    }
    let dossier_cache = etat.db.with_file_name("pochettes");
    tauri::async_runtime::spawn_blocking(move || {
        let cle = format!("caa-{rg_mbid}");
        if let Some(valeur) = lire_cache_pochette(&dossier_cache, &cle) {
            return Ok(valeur);
        }
        let valeur = rusty_music_core::pochette::release_group(&rg_mbid)
            .map_err(echec)?
            .map(|octets| format!("data:image/jpeg;base64,{}", base64(&octets)));
        ecrire_cache_pochette(&dossier_cache, &cle, valeur.as_deref());
        Ok(valeur)
    })
    .await
    .map_err(echec)?
}

/// Lance l'actualisation du fil Découvrir : sorties récentes (ListenBrainz
/// `fresh-releases`, une requête) puis artistes similaires (un appel par
/// artiste de la bibliothèque, cadencé).
///
/// Comme `start_enrichment` : déclenchée à la main (ou par l'interface à
/// l'ouverture du mode si la dernière passe est ancienne), sur son propre fil,
/// avec sa propre connexion. Additive et reprenable.
#[tauri::command(async)]
fn start_decouvrir(app: tauri::AppHandle, etat: State<Etat>, contact: String) -> Result<(), String> {
    let contact = contact.trim().to_string();
    if !contact.contains('@') {
        return Err("Une adresse de contact valable est demandée pour interroger les API.".into());
    }
    {
        let mut d = etat.decouvrir.lock().map_err(echec)?;
        if d.en_cours {
            return Err("une actualisation est déjà en cours".into());
        }
        *d = EtatDecouvrir {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let lb = rusty_music_core::listenbrainz::Client::new(&contact);
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            rusty_music_core::decouvrir::actualiser(&mut lib, &lb, 30, 0, |b| {
                if let Ok(mut d) = etat.decouvrir.lock() {
                    d.artistes = b.artistes;
                    d.total = b.total;
                    d.sorties_neuves = b.sorties_neuves;
                    d.voisins_neufs = b.voisins_neufs;
                }
            })
            .map_err(|e| e.to_string())
        })();

        let bilan = match issue {
            Ok(b) => format!(
                "{} sorties neuves, {} voisins sur {} artistes",
                b.sorties_neuves, b.voisins_neufs, b.artistes
            ),
            Err(e) => format!("échec : {e}"),
        };
        let mut fin = etat.decouvrir.lock();
        if let Ok(d) = fin.as_mut() {
            d.en_cours = false;
            d.resultat = Some(bilan);
        }
    });
    Ok(())
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

/// Avancement d'une régénération HD (super-résolution), sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
struct EtatSuperres {
    en_cours: bool,
    /// Le morceau traité, pour que l'interface sache si l'avancement est le sien.
    source: String,
    faits: usize,
    total: usize,
    resultat: Option<String>,
}

/// Régénère un morceau en haute résolution (AERO) — rendu hors ligne vers le
/// cache `hd/`. Tourne dans son fil : ~30 s par minute d'audio stéréo, plus le
/// chargement du modèle au premier appel.
#[tauri::command(async)]
fn start_superres(app: tauri::AppHandle, etat: State<Etat>, path: String) -> Result<(), String> {
    let source = PathBuf::from(&path);
    if !source.is_file() {
        return Err(format!("{path} est introuvable"));
    }
    let modele_onnx = rusty_music_core::modeles::trouver("aero-11025-44100.onnx")
        .ok_or_else(|| rusty_music_core::modeles::introuvable("aero-11025-44100.onnx"))?;
    {
        let mut s = etat.superres.lock().map_err(echec)?;
        if s.en_cours {
            return Err("une régénération est déjà en cours".into());
        }
        *s = EtatSuperres {
            en_cours: true,
            source: path.clone(),
            ..Default::default()
        };
    }
    let cible = rusty_music_superres::chemin_cache(&etat.hd, &source);
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _ = std::fs::create_dir_all(&etat.hd);
        rusty_music_superres::purger_anciens(&etat.hd);

        let progres = |faits: usize, total: usize| {
            let mut s = match etat.superres.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            s.faits = faits;
            s.total = total;
        };

        let issue = (|| -> rusty_music_superres::Result<f32> {
            let mut garde = etat.superres_modele.lock().expect("verrou modèle");
            if garde.is_none() {
                *garde = Some(rusty_music_superres::Modele::charger(&modele_onnx)?);
            }
            let modele = garde.as_mut().expect("modèle chargé");
            rusty_music_superres::regenerer(&source, &cible, modele, progres)
        })();

        let bilan = match issue {
            // Le spectre de la source est conservé (mélange HF), donc le HD ne
            // peut pas ternir ; mais au-dessus de ~16 kHz de coupure il n'a
            // presque rien à ajouter — autant le dire.
            Ok(coupure) if coupure >= 16_000.0 => format!(
                "régénéré, mais la source monte déjà à {} kHz — le HD n'ajoute presque rien",
                (coupure / 1000.0).round() as i32
            ),
            Ok(_) => "régénéré en HD".to_string(),
            Err(e) => {
                let _ = std::fs::remove_file(&cible);
                format!("échec : {e}")
            }
        };
        tracing::info!(%bilan, source = %path, "régénération HD terminée");
        if let Ok(mut s) = etat.superres.lock() {
            s.en_cours = false;
            s.resultat = Some(bilan);
        }
        // Libère le handle `State` avant la fin de la fermeture, pour que
        // l'emprunt du verrou ci-dessus se termine à temps (E0597 sinon).
        let _ = etat;
    });
    Ok(())
}

#[tauri::command(async)]
fn superres_state(etat: State<Etat>) -> Result<EtatSuperres, String> {
    Ok(etat.superres.lock().map_err(echec)?.clone())
}

/// Vrai si le morceau a déjà une version HD en cache.
#[tauri::command(async)]
fn superres_disponible(etat: State<Etat>, path: String) -> Result<bool, String> {
    Ok(rusty_music_superres::chemin_cache(&etat.hd, Path::new(&path)).is_file())
}

/// Active/coupe la lecture des versions HD (quand elles existent). Réouvre le
/// morceau en cours en tâche de fond, comme le bouton « E ».
#[tauri::command(async)]
fn set_lecture_hd(app: tauri::AppHandle, etat: State<Etat>, actif: bool) -> Result<(), String> {
    rusty_music_superres::set_lecture_hd(actif);
    let courant = etat
        .player
        .lock()
        .map_err(echec)?
        .current()
        .map(std::path::Path::to_path_buf);
    let Some(chemin) = courant else {
        return Ok(());
    };
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let ouvert = rusty_music_superres::resoudre(&etat.hd, &chemin);
        match rusty_music_player::ouvrir(&ouvert) {
            Ok(source) => {
                let verrou = etat.player.lock();
                if let Ok(mut player) = verrou {
                    if let Err(e) = player.remplacer_courant(&chemin, source) {
                        tracing::warn!(error = %e, "bascule HD impossible");
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "réouverture HD impossible"),
        }
    });
    Ok(())
}

/// Taille du cache HD et nombre de morceaux régénérés.
#[tauri::command(async)]
fn superres_cache(etat: State<Etat>) -> Result<(u64, usize), String> {
    let dossier = &etat.hd;
    let mut octets = 0;
    let mut n = 0;
    if let Ok(entrees) = std::fs::read_dir(dossier) {
        for e in entrees.flatten() {
            if let Ok(m) = e.metadata() {
                octets += m.len();
                n += 1;
            }
        }
    }
    Ok((octets, n))
}

/// Vide le cache HD.
#[tauri::command(async)]
fn vider_cache_hd(etat: State<Etat>) -> Result<(), String> {
    if etat.hd.is_dir() {
        std::fs::remove_dir_all(&etat.hd).map_err(echec)?;
    }
    Ok(())
}

/// Le dossier où l'application écrit tout : la base et les empreintes, les
/// journaux, les stems démixés, les rendus HD, les tuiles de la carte. Montré en
/// tête du mode Bibliothèque — l'utilisateur doit pouvoir retrouver, sauvegarder
/// ou purger ce que le logiciel pose sur son disque.
#[tauri::command(async)]
fn dossier_donnees(etat: State<Etat>) -> Result<String, String> {
    let d = etat.db.parent().ok_or("la base n'a pas de dossier parent")?;
    Ok(d.display().to_string())
}

/// Vitesse de lecture des stems, appliquée immédiatement.
///
/// **Rien n'est rechargé** : la vitesse est un flottant que la lecture relit à
/// chaque trame, et la position ne bouge pas. C'est la différence avec
/// [`start_etirer`], qui réécrit un fichier et coûte des dizaines de secondes.
///
/// **La hauteur ne suit pas la vitesse.** L'étireur `wsola` travaille dans la
/// lecture elle-même — recouvrement-addition temporel, la méthode d'`atempo`.
/// Le commentaire qui figurait ici disait le contraire ; il datait d'avant que
/// l'étireur soit branché, quand la vitesse n'était qu'un rééchantillonnage.
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
/// Où va la version transposée d'un stem. `None` quand il n'y a rien à faire.
///
/// Séparé du calcul pour que l'on puisse répondre à « y a-t-il du travail ? »
/// sans rien calculer — c'est ce qui permet au cas déjà en cache de rester
/// instantané.
fn cible_transposee(chemin: &str, demi_ton: f32) -> Option<PathBuf> {
    if demi_ton.abs() < 1e-3 {
        return None;
    }
    let source = PathBuf::from(chemin);
    Some(
        source
            .parent()
            .unwrap_or(Path::new("."))
            .join("traites")
            .join(format!("t{:+03}", demi_ton as i32))
            .join(source.file_name().unwrap_or_default()),
    )
}

/// Transpose un stem et rend le chemin du résultat. Ne recalcule rien si le
/// fichier est déjà là.
fn transposer_un(chemin: &str, demi_ton: f32) -> Result<String, String> {
    use rusty_music_editor::{decode, etirement, wav};

    let Some(cible) = cible_transposee(chemin, demi_ton) else {
        return Ok(chemin.to_string());
    };
    if cible.exists() {
        return Ok(cible.display().to_string());
    }
    let dossier = cible.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dossier).map_err(echec)?;

    let source = PathBuf::from(chemin);
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
    tracing::info!(chemin, demi_ton, "stem transposé");
    Ok(cible.display().to_string())
}

/// Où en est la transposition. Sondé par l'interface, comme le démixage.
#[derive(Default, Clone, serde::Serialize)]
struct EtatTranspose {
    en_cours: bool,
    /// Stems déjà traités, et combien il y en a en tout. Un stem de quatre
    /// minutes demande une vingtaine de secondes : un pourcentage global
    /// laisserait croire à un blocage entre deux.
    faits: usize,
    total: usize,
    /// Les chemins à jouer, une fois tout fini.
    stems: Vec<(String, String)>,
    erreur: Option<String>,
}

/// Lance la transposition des stems **dans son fil**.
///
/// **Pourquoi un fil, alors que la commande était déjà `async`.** Une commande
/// Tauri `async` tourne sur le runtime, pas sur le fil de l'interface — mais
/// une boucle de calcul qui ne rend jamais la main y monopolise un ouvrier du
/// runtime. Quatre stems à une vingtaine de secondes, et **toutes les autres
/// commandes attendent derrière** : le sondage du transport, l'état de lecture,
/// le moindre clic. L'interface ne gelait pas, elle faisait la queue — ce qui
/// se voit pareil.
///
/// `async` suffit pour une commande qui attend ; il ne suffit pas pour une
/// commande qui calcule. Le démixage l'avait déjà réglé ainsi, la transposition
/// ne l'avait pas suivi parce qu'elle était courte du temps du vocodeur de
/// phase — 0,84 s par stem. `wsola` l'a portée à 17,9 s sans que ce chemin-là
/// soit revu.
///
/// **Rien à calculer = rien à attendre.** Si tous les stems sont neutres ou
/// déjà en cache, l'état est rempli sur place et aucun fil n'est lancé : le
/// chargement d'un jeu de stems sans réglage reste immédiat.
#[tauri::command(async)]
fn start_etirer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    stems: Vec<(String, String)>,
    demi_tons: Vec<f32>,
) -> Result<EtatTranspose, String> {
    let a_faire = stems
        .iter()
        .enumerate()
        .filter(|(i, (_, chemin))| {
            cible_transposee(chemin, demi_tons.get(*i).copied().unwrap_or(0.0))
                .is_some_and(|c| !c.exists())
        })
        .count();

    if a_faire == 0 {
        let mut sortie = Vec::with_capacity(stems.len());
        for (i, (nom, chemin)) in stems.iter().enumerate() {
            let demi_ton = demi_tons.get(i).copied().unwrap_or(0.0);
            sortie.push((nom.clone(), transposer_un(chemin, demi_ton)?));
        }
        let fini = EtatTranspose {
            en_cours: false,
            faits: 0,
            total: 0,
            stems: sortie,
            erreur: None,
        };
        *etat.transpose.lock().map_err(echec)? = fini.clone();
        return Ok(fini);
    }

    {
        let mut t = etat.transpose.lock().map_err(echec)?;
        if t.en_cours {
            return Err("une transposition est déjà en cours".into());
        }
        *t = EtatTranspose {
            en_cours: true,
            faits: 0,
            total: a_faire,
            stems: Vec::new(),
            erreur: None,
        };
    }

    let depart = etat.transpose.lock().map_err(echec)?.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();

        // **Un fil par stem.** Mesuré sur un morceau de 272 s : 92,0 s en file
        // contre 23,5 s en parallèle, soit 3,9× — l'étirement est le seul poste
        // qui compte (22,8 s des 23,0 s d'un stem) et il ne partage rien.
        //
        // Ce qui rendait cette parallélisation dangereuse avant, et ne l'est
        // plus : chaque transposition tenait cinq tampons pleins du signal, et
        // quatre à la fois faisaient pagineur la machine — ce qui fige
        // l'interface aussi sûrement qu'un calcul mal placé.
        // `reechantillonner_entrelace` les a ramenés à deux.
        let resultats: Vec<Result<String, String>> = std::thread::scope(|portee| {
            let taches: Vec<_> = stems
                .iter()
                .enumerate()
                .map(|(i, (_, chemin))| {
                    let demi_ton = demi_tons.get(i).copied().unwrap_or(0.0);
                    let etat = &etat;
                    portee.spawn(move || {
                        // Le compteur n'avance que sur les stems réellement
                        // calculés : un stem déjà en cache passe en quelques
                        // microsecondes, et le voir sauter ne dirait rien de
                        // juste.
                        let travail =
                            cible_transposee(chemin, demi_ton).is_some_and(|c| !c.exists());
                        let issue = transposer_un(chemin, demi_ton);
                        if travail && issue.is_ok() {
                            if let Ok(mut t) = etat.transpose.lock() {
                                t.faits += 1;
                            }
                        }
                        issue
                    })
                })
                .collect();
            taches
                .into_iter()
                .map(|t| t.join().unwrap_or_else(|_| Err("fil interrompu".into())))
                .collect()
        });

        let mut sortie = Vec::with_capacity(stems.len());
        let mut erreur = None;
        for ((nom, _), issue) in stems.iter().zip(resultats) {
            match issue {
                Ok(c) => sortie.push((nom.clone(), c)),
                Err(e) => erreur = Some(e),
            }
        }

        // Le verrou est nommé plutôt que pris dans le `if let` : un temporaire
        // en fin de portée vivrait plus longtemps que `etat`. Même précaution
        // qu'à la fin du démixage.
        let verrou = etat.transpose.lock();
        if let Ok(mut t) = verrou {
            t.en_cours = false;
            t.stems = if erreur.is_some() { Vec::new() } else { sortie };
            t.erreur = erreur;
        }
    });

    Ok(depart)
}

/// Où en est la transposition lancée par [`start_etirer`].
#[tauri::command(async)]
fn etirer_state(etat: State<Etat>) -> Result<EtatTranspose, String> {
    Ok(etat.transpose.lock().map_err(echec)?.clone())
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

/// Spectrogramme du **son réellement joué** pour le morceau à `path` :
///
/// - version HD du cache si la lecture HD est active et qu'elle existe ;
/// - sinon le fichier d'origine.
///
/// Quand ce qui joue diffère de l'original (HD), rend **aussi** le
/// spectrogramme de l'original (`pixels_ref`) : l'interface peut alors teinter
/// ce que le HD a ajouté. Rend la coupure estimée de la source, pour tracer le
/// trait « au-dessus, c'est reconstruit ».
#[tauri::command(async)]
fn spectre_transport(
    etat: State<Etat>,
    path: String,
    width: usize,
    height: usize,
) -> Result<SpectreTransport, String> {
    let origine = PathBuf::from(&path);
    let joue = rusty_music_superres::resoudre(&etat.hd, &origine);
    let hd = joue != origine;
    let e = rusty_music_player::amelioration().actif();

    // Le son joué : version HD du cache, sinon l'original ; puis l'excitateur
    // « E » par-dessus s'il est actif (il n'existe qu'en mémoire).
    let s = if e {
        rusty_music_player::spectre_ameliore(&joue, width, height).map_err(echec)?
    } else {
        rusty_music_player::spectre::calculer(&joue, width, height).map_err(echec)?
    };

    // Référence à teinter : l'original brut, dès que le son joué en diffère.
    let modifie = hd || e;
    let pixels_ref = if modifie {
        rusty_music_player::spectre::calculer(&origine, width, height)
            .ok()
            .filter(|r| r.pixels.len() == s.pixels.len())
            .map(|r| r.pixels)
    } else {
        None
    };
    Ok(SpectreTransport {
        largeur: s.largeur,
        hauteur: s.hauteur,
        pixels: s.pixels,
        pixels_ref,
        hd: modifie,
    })
}

#[derive(serde::Serialize)]
struct SpectreTransport {
    largeur: usize,
    hauteur: usize,
    pixels: Vec<u8>,
    pixels_ref: Option<Vec<u8>>,
    hd: bool,
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
    /// Le greffon est-il entré sur un battement ? L'interface le dit plutôt
    /// que de laisser l'oreille le découvrir.
    cale_aux_temps: bool,
}

/// Les grilles de battements des deux stems.
///
/// **C'est ici que le module 2 rejoint le module 3.** `crates/editor` ne dépend
/// pas de `rusty-music-analysis` — ce serait tirer CLAP, ses 117 Mo de poids et
/// la génération de code de son `build.rs` dans un crate qui n'a que faire d'un
/// modèle d'empreintes. L'application dépend des deux, c'est donc elle qui les
/// relie, en trois nombres.
fn grilles_des_stems(
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

    // **Les grilles se mesurent sur les stems, les tempos viennent de la base.**
    // Ce n'est pas une incohérence : la base a mesuré le morceau entier, ce qui
    // est plus sûr qu'une batterie seule, et c'est ce tempo-là qui a servi à
    // choisir le voisin. La grille n'apporte que la phase — mais elle apporte
    // aussi son propre tempo, plus fin, dont on se sert pour l'étirement quand
    // la base n'en a pas.
    let grilles = grilles_des_stems(Path::new(&remplace), &greffon);
    let plan = rusty_music_editor::greffe::greffer(
        Path::new(&remplace),
        &greffon,
        tempos.get(&id).copied().or(grilles.map(|(a, _)| a.bpm)),
        tempos.get(&voisin).copied().or(grilles.map(|(_, b)| b.bpm)),
        grilles.map(|(a, b)| rusty_music_editor::greffe::Cale {
            phase_remplace_s: a.phase_s,
            phase_greffon_s: b.phase_s,
            periode_greffon_s: b.periode(),
        }),
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
        cale_aux_temps: plan.cale_aux_temps,
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
fn start_scan(
    app: tauri::AppHandle,
    etat: State<Etat>,
    path: String,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
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
        let issue = Library::open(&db).map_err(|e| e.to_string()).and_then(|lib| {
            let jobs = coeurs_arriere_plan();
            rusty_music_core::scan::scan_root_jobs(&lib, &racine, jobs, force)
                .map_err(|e| e.to_string())
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
/// **Mise en cache sur disque**, sous `<données app>/pochettes/` — voir
/// [`cle_pochette`]. Le cache mémoire côté interface (`app.js`) évite de
/// repayer les 50 à 210 ms d'extraction pendant une session, mais repart à
/// froid à chaque lancement ; celui-ci persiste, et une pochette déjà servie
/// une fois ne relit plus jamais le fichier source tant qu'il n'a pas changé.
///
/// **Corps bloquant renvoyé sur le pool `spawn_blocking`.** La lecture disque
/// et le parsing des tags (`lofty`) coûtent 50 à 210 ms à froid, et la grille
/// d'albums en demande des dizaines d'un coup. Exécutées telles quelles sur le
/// pool du runtime, elles en occupaient tous les fils et une commande de
/// transport (`play`, `toggle_pause`) attendait derrière — la lecture partait
/// avec plusieurs secondes de retard pendant qu'on faisait défiler les
/// pochettes. Le pool dédié aux tâches bloquantes n'entre pas en concurrence
/// avec les commandes courtes.
#[tauri::command]
async fn cover(etat: State<'_, Etat>, path: String) -> Result<Option<String>, String> {
    let dossier_cache = etat.db.with_file_name("pochettes");
    tauri::async_runtime::spawn_blocking(move || cover_extraire(&dossier_cache, &path))
        .await
        .map_err(echec)?
}

fn cover_extraire(dossier_cache: &Path, path: &str) -> Result<Option<String>, String> {
    let chemin = Path::new(path);

    // Sans métadonnées lisibles (fichier disparu, permission refusée), pas de
    // clé de cache fiable : on extrait directement, comme avant ce cache.
    let mtime = std::fs::metadata(chemin).ok().and_then(|m| m.modified().ok());
    let cle = mtime.map(|m| cle_pochette(chemin, m));

    if let Some(cle) = &cle {
        if let Some(valeur) = lire_cache_pochette(dossier_cache, cle) {
            return Ok(valeur);
        }
    }

    let cover = rusty_music_core::tags::read_cover(chemin).map_err(echec)?;
    let valeur = cover.map(|c| {
        let mime = c.mime.as_deref().unwrap_or("image/jpeg");
        format!("data:{mime};base64,{}", base64(&c.data))
    });
    if let Some(cle) = &cle {
        ecrire_cache_pochette(dossier_cache, cle, valeur.as_deref());
    }
    Ok(valeur)
}

/// Les chemins de piste d'au plus `max` albums d'un artiste qui portent une
/// pochette lisible — la matière du tuilage en mosaïque de la vue Artistes.
///
/// Rend des chemins, pas des images : l'interface les repasse à `cover`, dont
/// le cache mémoire (borné, partagé avec la grille d'albums et l'inspecteur)
/// et le cache disque font le reste. Le tri des tags, lui, se fait ici une
/// fois — chaque album candidat passe par [`cover_extraire`], qui remplit le
/// cache disque au passage et permet d'écarter les albums sans pochette (d'où
/// un résultat parfois plus court que `max`).
#[tauri::command]
async fn artist_covers(
    etat: State<'_, Etat>,
    name: String,
    mbid: Option<String>,
    max: usize,
) -> Result<Vec<String>, String> {
    let albums = {
        let lib = etat.lib.lock().map_err(echec)?;
        lib.albums_of_artist(mbid.as_deref(), &name).map_err(echec)?
    };
    // Borne dure : un artiste prolifique ne doit pas faire lire les tags de
    // cent albums pour une vignette qui n'en montre que quatre.
    let max = max.min(9);
    let dossier_cache = etat.db.with_file_name("pochettes");
    tauri::async_runtime::spawn_blocking(move || {
        let mut chemins: Vec<String> = Vec::new();
        for album in albums {
            if chemins.len() >= max {
                break;
            }
            if let Ok(Some(_)) = cover_extraire(&dossier_cache, &album.path) {
                chemins.push(album.path);
            }
        }
        Ok(chemins)
    })
    .await
    .map_err(echec)?
}

/// Marqueur d'entrée « pas de pochette » dans le cache disque : un octet nul,
/// qu'aucune image ni aucune `data:` URI valide ne peut produire — pas
/// d'ambiguïté avec un contenu réel. Sans lui, un morceau sans pochette
/// resonderait ses tags à chaque affichage, cache ou pas.
const SANS_POCHETTE: &[u8] = b"\0";

/// Clé de cache : hachage FNV-1a (même construction que `db::hacher`) du
/// chemin **et** de la date de modification du fichier.
///
/// La mtime dans la clé, pas seulement le chemin, est ce qui rend
/// l'invalidation automatique : remplacer le fichier (nouveau tag,
/// ré-encodage) change sa mtime, donc sa clé — l'ancienne entrée reste sur
/// disque, inerte, plutôt que d'exiger une purge active.
fn cle_pochette(path: &Path, mtime: std::time::SystemTime) -> String {
    let epoch = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in path
        .to_string_lossy()
        .as_bytes()
        .iter()
        .chain(&epoch.to_le_bytes())
    {
        h ^= *octet as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

/// Lit une entrée du cache. `None` extérieur = pas en cache (à calculer),
/// `Some(None)` intérieur = en cache et confirmé sans pochette.
fn lire_cache_pochette(dossier: &Path, cle: &str) -> Option<Option<String>> {
    let octets = std::fs::read(dossier.join(cle)).ok()?;
    if octets == SANS_POCHETTE {
        Some(None)
    } else {
        Some(String::from_utf8(octets).ok())
    }
}

/// Écrit une entrée. Une erreur d'écriture (disque plein, dossier en lecture
/// seule) ne doit pas faire échouer la requête : la pochette a déjà été
/// extraite, autant la rendre, le prochain appel réessaiera le cache.
///
/// **Sans limite de taille pour l'instant** : contrairement à `target/` (un
/// artefact de compilation qui peut regrossir indéfiniment à chaque
/// recompilation), le nombre de pochettes distinctes est borné par celui des
/// albums de la bibliothèque, et chaque entrée pèse au plus quelques centaines
/// de Ko. Si ça devient un problème mesuré, appliquer la même politique que
/// `POCHETTES_MAX` côté JS (`app.js`) plutôt qu'inventer une seconde règle.
fn ecrire_cache_pochette(dossier: &Path, cle: &str, valeur: Option<&str>) {
    if std::fs::create_dir_all(dossier).is_err() {
        return;
    }
    let _ = std::fs::write(dossier.join(cle), valeur.map_or(SANS_POCHETTE, str::as_bytes));
}

/// Tempo, tonalité et énergie d'un morceau — ce que la passe a mesuré.
///
/// **Rend `None` plutôt qu'une valeur par défaut** quand le morceau n'est pas
/// mesuré : 15 847 des 27 044 le sont à ce jour, et afficher « 120 BPM » sur
/// les autres serait donner une mesure qu'on n'a pas.
#[tauri::command(async)]
fn descripteurs(
    etat: State<Etat>,
    id: i64,
) -> Result<Option<rusty_music_core::db::DescripteursVus>, String> {
    etat.lib.lock().map_err(echec)?.descripteurs(id).map_err(echec)
}

/// La popularité générale des morceaux `ids` — `(track_id, relative 0..1,
/// echelon)`. Un morceau absent de la réponse n'en a pas (jauge grisée).
/// Chargée par lot pour ce qui est visible (file d'attente, liste de pistes),
/// comme les pochettes — jamais dans `TrackRow`.
#[tauri::command(async)]
fn popularites(etat: State<Etat>, ids: Vec<i64>) -> Result<Vec<(i64, f64, String)>, String> {
    etat.lib.lock().map_err(echec)?.popularites(&ids).map_err(echec)
}

/// Qualité d'encodage du morceau en écoute — codec, débit, échantillonnage,
/// profondeur de bits. Affichée sous le compteur de temps du transport.
/// Champs partiellement `None` sur un morceau scanné avant la lecture du
/// format (un rescan les remplit).
#[tauri::command(async)]
fn qualite_piste(
    etat: State<Etat>,
    id: i64,
) -> Result<Option<rusty_music_core::db::QualitePiste>, String> {
    etat.lib.lock().map_err(echec)?.qualite_piste(id).map_err(echec)
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

/// Comme `play`, mais ne redémarre pas si le premier morceau ne change pas —
/// la régénération d'un chemin sur la carte (bruit, « Autre tirage »).
#[tauri::command(async)]
fn set_queue(etat: State<Etat>, paths: Vec<String>) -> Result<(), String> {
    let chemins: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    etat.player
        .lock()
        .map_err(echec)?
        .set_queue(&chemins)
        .map_err(echec)
}

/// Remplace la file par `paths` en gardant la piste écoutée sans coupure.
///
/// Le bouton ✦ « playlist dans l'esprit de ce morceau » : la nouvelle liste
/// part du morceau en cours et doit vraiment enchaîner dès le suivant.
/// `set_queue` gardait le préchargement de l'ancienne file — un ou deux
/// morceaux des résultats de recherche se glissaient avant la playlist.
#[tauri::command(async)]
fn remplacer_file(etat: State<Etat>, paths: Vec<String>) -> Result<(), String> {
    let chemins: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();

    let courant = etat
        .player
        .lock()
        .map_err(echec)?
        .current()
        .map(std::path::Path::to_path_buf);

    // Tête de file différente de ce qu'on écoute (ou rien en lecture) :
    // `play` suffit, rien à rouvrir.
    if courant.as_deref() != chemins.first().map(PathBuf::as_path) {
        return etat
            .player
            .lock()
            .map_err(echec)?
            .play(&chemins)
            .map_err(echec);
    }
    let chemin = courant.expect("tête de file == piste en cours");

    // Hors du verrou `player` : `ouvrir` peut lire le disque plusieurs
    // secondes. Même précaution que la bascule d'amélioration et le
    // préchargement.
    let ouvert = rusty_music_superres::resoudre(&etat.hd, &chemin);
    let source = rusty_music_player::ouvrir(&ouvert).map_err(echec)?;
    etat.player
        .lock()
        .map_err(echec)?
        .rebrancher_file(&chemins, source)
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

/// Bouton « E » : active/coupe l'amélioration du son (excitation
/// psychoacoustique) et, si fourni, règle son intensité (`0.0`..=`1.0`). Si un
/// morceau joue, il est réouvert **en tâche de fond** à la même position — le
/// son en cours continue pendant le décodage, puis on bascule sans coupure. Le
/// préchargement se reconstruit tout seul au sondage suivant, avec la nouvelle
/// version.
#[tauri::command(async)]
fn set_amelioration(
    app: tauri::AppHandle,
    etat: State<Etat>,
    actif: bool,
    intensite: Option<f32>,
) -> Result<(), String> {
    let ame = rusty_music_player::amelioration();
    ame.set_actif(actif);
    if let Some(i) = intensite {
        ame.set_intensite(i);
    }

    let courant = etat
        .player
        .lock()
        .map_err(echec)?
        .current()
        .map(std::path::Path::to_path_buf);
    let Some(chemin) = courant else {
        return Ok(());
    };

    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        // Hors du verrou `player` : `ouvrir` lit le drapeau global qu'on vient
        // de poser et applique (ou non) l'amélioration. Le chemin est résolu
        // vers le cache HD s'il y a lieu, comme dans le préchargement.
        let ouvert = rusty_music_superres::resoudre(&etat.hd, &chemin);
        let source = match rusty_music_player::ouvrir(&ouvert) {
            Ok(source) => source,
            Err(e) => {
                tracing::warn!(error = %e, "réouverture pour amélioration impossible");
                return;
            }
        };
        let mut player = match etat.player.lock() {
            Ok(player) => player,
            Err(_) => return,
        };
        if let Err(e) = player.remplacer_courant(&chemin, source) {
            tracing::warn!(error = %e, "bascule d'amélioration impossible");
        }
    });
    Ok(())
}

/// Sondé par l'interface : `rodio` ne pousse pas d'évènements.
///
/// Sert aussi à réalimenter la sortie. La file n'est plus chargée d'un bloc —
/// elle immobilisait le lecteur 17 s sur un album de 157 pistes — et c'est ce
/// passage régulier qui prépare la piste suivante, un fichier à la fois.
#[tauri::command(async)]
fn playback_state(etat: State<Etat>) -> Result<EtatLecture, String> {
    // En trois temps plutôt qu'un `player.completer()` verrou tenu : la
    // lecture disque de `ouvrir` peut prendre plusieurs secondes sur la carte
    // SD, et ce verrou est aussi celui de `toggle_pause` — sondé toutes les
    // 200 ms, il rendait le bouton lecture/pause silencieusement peu réactif
    // le temps qu'une piste suivante se précharge.
    let a_charger = etat.player.lock().map_err(echec)?.a_precharger();
    if let Some((rang, piste)) = a_charger {
        match rusty_music_player::ouvrir(&piste) {
            Ok(source) => etat
                .player
                .lock()
                .map_err(echec)?
                .charger_precharge(rang, source),
            Err(e) => {
                // Une piste illisible ne doit pas interrompre le suivi : la
                // lecture passera simplement à la suivante.
                tracing::warn!(error = %e, "préparation de la piste suivante impossible");
            }
        }
    }
    let player = etat.player.lock().map_err(echec)?;
    Ok(EtatLecture {
        current: player.current().map(|p| p.display().to_string()),
        paused: player.is_paused(),
        finished: player.is_finished(),
        position_ms: player.position().as_millis() as u64,
        remaining: player.remaining(),
        volume: player.volume(),
    })
}

/// Enregistre les touches média du clavier (▶⏸, précédent, suivant) comme
/// raccourcis globaux et relaie chaque appui à l'interface par un évènement —
/// c'est elle qui sait piloter le transport (garde anti-double-appui, sondage
/// de l'état, mise à jour de l'inspecteur), pas ce module.
///
/// **Sans ça, ces trois touches restent au système** : sur macOS, une touche
/// média que personne ne capte ouvre ou pilote l'app Musique à la place —
/// aucune erreur, aucun message, juste la mauvaise application qui répond.
/// macOS demande l'autorisation « Surveillance des saisies » au premier
/// appui pour les capter ; échoue en silence si elle est refusée ou sur une
/// plateforme sans ces touches, un clavier qui n'en a pas n'en a simplement
/// pas besoin.
fn enregistrer_touches_media(app: tauri::AppHandle) {
    use global_hotkey::{
        hotkey::{Code, HotKey},
        GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    };

    let gestionnaire = match GlobalHotKeyManager::new() {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(%e, "gestionnaire de touches média indisponible");
            return;
        }
    };

    let lecture = HotKey::new(None, Code::MediaPlayPause);
    let suivant = HotKey::new(None, Code::MediaTrackNext);
    let precedent = HotKey::new(None, Code::MediaTrackPrevious);
    if let Err(e) = gestionnaire.register_all(&[lecture, suivant, precedent]) {
        tracing::warn!(%e, "échec de l'enregistrement des touches média");
        return;
    }

    let noms: [(u32, &str); 3] = [
        (lecture.id(), "lecture"),
        (suivant.id(), "suivant"),
        (precedent.id(), "precedent"),
    ];

    let pour_fil = app.clone();
    std::thread::spawn(move || {
        let recepteur = GlobalHotKeyEvent::receiver();
        while let Ok(evt) = recepteur.recv() {
            if evt.state() != HotKeyState::Pressed {
                continue; // on agit à l'appui, pas au relâchement
            }
            if let Some((_, nom)) = noms.iter().find(|(id, _)| *id == evt.id()) {
                let _ = pour_fil.emit("touche-media", *nom);
            }
        }
    });

    // Le gestionnaire doit vivre autant que l'application : le laisser
    // tomber couperait l'écoute des touches.
    app.manage(gestionnaire);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;

            // La base vit à côté des données de l'application, pas dans le
            // répertoire courant : une app de bureau n'a pas de « cwd » stable.
            let dossier = app.path().app_data_dir()?;
            let db = dossier.join("rusty-music.db");
            let hd = dossier.join("hd");

            // Le journal console seul ne survit pas à un plantage qui emporte
            // la machine — rencontré en pratique pendant une analyse. Un
            // second récepteur écrit donc les mêmes évènements dans un fichier
            // sous les données de l'app, pour retrouver après coup le dernier
            // fichier en cours de traitement sans avoir eu à garder un
            // terminal ouvert. `non_blocking` fait l'écriture sur son propre
            // fil : la garde doit vivre jusqu'à la fin du processus, sans quoi
            // les dernières lignes resteraient dans son tampon — fuite
            // volontaire, il n'y en a qu'une pour toute la durée de vie de
            // l'appli.
            let dossier_logs = dossier.join("logs");
            std::fs::create_dir_all(&dossier_logs)?;
            let fichier = tracing_appender::rolling::daily(dossier_logs, "rusty-music.log");
            let (fichier, garde) = tracing_appender::non_blocking(fichier);
            Box::leak(Box::new(garde));

            let directives = || {
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
                    // `symphonia` commente chaque trame ID3 qu'il ne gère pas
                    // (TCMP, UFID…) et chaque estimation de durée, à chaque
                    // piste ouverte : des dizaines de lignes qui noient les
                    // nôtres. La directive est ajoutée par-dessus RUST_LOG,
                    // pour que fixer `RUST_LOG=info` ne réveille pas ce bruit.
                    .add_directive("symphonia=warn".parse().expect("directive valide"))
                    // Le décodeur MP3 crie `invalid main_data_begin, underflow`
                    // sur la première trame après chaque `seek` : le réservoir
                    // de bits repart vide et pointe en arrière dans des octets
                    // non décodés. `decode::par_position` se positionne sur
                    // chaque fenêtre, ~4 fois par fichier — inévitable et sans
                    // effet, le décodeur récupère en une trame (~26 ms sur 10 s
                    // analysés).
                    .add_directive(
                        "symphonia_bundle_mp3::layer3=error"
                            .parse()
                            .expect("directive valide"),
                    )
                    // `lofty` en fait autant sur les conteneurs MP4 (« Skipping
                    // empty data atom ») à chaque pochette lue.
                    .add_directive("lofty=error".parse().expect("directive valide"))
            };

            tracing_subscriber::registry()
                .with(directives())
                .with(tracing_subscriber::fmt::layer())
                .with(tracing_subscriber::fmt::layer().with_writer(fichier).with_ansi(false))
                .init();

            tracing::info!(base = %db.display(), "ouverture de la bibliothèque");

            // Le plan de ville du paquet (Paris), si l'utilisateur n'a pas déjà
            // le sien à côté de la base. Sans lui, la carte est procédurale.
            installer_plan_de_ville(app, &dossier);

            // Le lecteur aiguille les chemins de la file vers le cache HD quand
            // la lecture HD est active (`crates/superres`). La file, elle, garde
            // les chemins d'origine — c'est eux que l'interface suit.
            let mut player = Player::new()?;
            let hd_pour_lecteur = hd.clone();
            player.set_resolveur(move |p| rusty_music_superres::resoudre(&hd_pour_lecteur, p));

            app.manage(Etat {
                lib: Mutex::new(Library::open(&db)?),
                player: Mutex::new(player),
                db,
                hd,
                superres: Mutex::new(EtatSuperres::default()),
                superres_modele: Mutex::new(None),
                scan: Mutex::new(EtatScan::default()),
                analyse: Mutex::new(EtatAnalyse::default()),
                descripteurs: Mutex::new(EtatDescripteurs::default()),
                enrichissement: Mutex::new(EtatEnrichissement::default()),
                popularite: Mutex::new(EtatPopularite::default()),
                decouvrir: Mutex::new(EtatDecouvrir::default()),
                demix: Mutex::new(EtatDemix::default()),
                transpose: Mutex::new(EtatTranspose::default()),
                stems: Mutex::new(None),
                ondes: Mutex::new(Default::default()),
                vecteurs: Mutex::new(Arc::new(Vec::new())),
                graphe: Mutex::new(None),
                graphe_construction: Mutex::new(()),
                graphe_fait: AtomicUsize::new(0),
                graphe_total: AtomicUsize::new(0),
                reseau: Mutex::new(None),
                densite: Mutex::new(None),
                ville: Mutex::new(None),
                graphe_reel: Mutex::new(None),
                accrochage_voirie: Mutex::new(None),
                graphes_voirie: Mutex::new(std::collections::HashMap::new()),
                agrement_voirie: Mutex::new(None),
            });
            app.manage(tuiles::Archives::default());

            // La fenêtre est bâtie ici, pas déclarée dans `tauri.conf.json`, pour
            // une seule raison : y accrocher `on_web_resource_request`.
            //
            // **Sans cache-control, la webview sert un `app.js` périmé.** Tauri
            // renvoie les fichiers embarqués sans le moindre en-tête de fraîcheur ;
            // WKWebView les garde alors sur disque et continue de servir la
            // version d'un ancien build même après recompilation — le pendant
            // côté exécution du piège déjà décrit dans `build.rs` côté
            // compilation. `no-store` sur le protocole `tauri://` le coupe net :
            // les assets sont en mémoire, rien à gagner à les mettre en cache.
            // `RUSTY_MUSIC_INCOGNITO` : mode d'essai — webview sans persistance
            // (jamais de cache périmé, jamais de `localStorage` partagé avec
            // l'app installée) et fenêtre au premier plan pour la capture
            // d'écran. Sans effet en usage normal.
            let essai = std::env::var("RUSTY_MUSIC_INCOGNITO").is_ok();
            let fenetre =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Rusty Music")
                    .inner_size(1400.0, 900.0)
                    .min_inner_size(960.0, 620.0)
                    .incognito(essai)
                    .always_on_top(essai)
                    .focused(true)
                    .on_web_resource_request(|req, resp| {
                        if req.uri().scheme_str() == Some("tauri") {
                            resp.headers_mut().insert(
                                tauri::http::header::CACHE_CONTROL,
                                tauri::http::HeaderValue::from_static("no-store"),
                            );
                        }
                    })
                    .build()?;

            // Purge unique du cache de la webview. Les anciens builds n'ont
            // laissé aucun en-tête de fraîcheur : WKWebView sert alors sans fin
            // l'`index.html` d'une version antérieure, y compris après
            // recompilation. Une fois cette purge faite (marqueur à côté de la
            // base), le `no-store` ci-dessus suffit — on ne recommence pas, car
            // la purge efface aussi le `localStorage` (adresse MusicBrainz,
            // onglet mémorisé).
            let marqueur = dossier.join(".webview-purgee-v1");
            if !marqueur.exists() {
                let _ = fenetre.clear_all_browsing_data();
                let _ = fenetre.reload();
                let _ = std::fs::write(&marqueur, b"1");
            }

            // Sans cela, la fenêtre s'ouvre parfois sans être le répondeur
            // clavier : les raccourcis (espace, flèches) n'arrivent alors
            // jamais au JS tant qu'on n'a pas cliqué une première fois dans
            // la fenêtre.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }

            enregistrer_touches_media(app.handle().clone());

            // Un cache HD produit par une version antérieure du pipeline
            // (rééchantillonnage, mélange…) donnerait un son étouffé joué tel
            // quel : on l'efface au démarrage.
            rusty_music_superres::purger_anciens(&app.state::<Etat>().hd);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            artists,
            albums,
            tracks_of_album,
            artist_covers,
            search,
            roots,
            forget_root,
            start_scan,
            scan_state,
            cover,
            waveform,
            descripteurs,
            map_view,
            tuiles_etat,
            engendrer_tuiles,
            style_carte,
            positions_carte,
            trace_rues,
            journal_carte,
            mode_initial,
            autotest_carte,
            itineraire,
            itineraire_voirie,
            tuile,
            map_progress,
            graphe_progress,
            density_view,
            library_stats,
            probable_duplicates,
            isolated_points,
            suspect_genres,
            multiple_editions,
            scan_failures,
            dismiss_scan_failure,
            map_parameters,
            set_map_parameter,
            vocabulaire_familles,
            definir_vocabulaire_familles,
            recompute_map,
            recompute_density,
            start_analysis,
            analysis_state,
            start_descripteurs,
            descripteurs_state,
            descripteurs_progress,
            start_enrichment,
            enrichment_state,
            start_popularite,
            popularite_state,
            popularite_fraicheur,
            popularites,
            artist_links,
            start_decouvrir,
            decouvrir_state,
            decouvrir_feed,
            decouvrir_tout_vu,
            decouvrir_pochette,
            start_demix,
            demix_state,
            stems_existants,
            stems_vitesse,
            stems_vitesse_stem,
            stems_exporter,
            start_etirer,
            etirer_state,
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
            path_album,
            selection,
            families,
            album_families,
            artist_families,
            prepare_graph,
            neighbours,
            js_error,
            qualite_piste,
            set_amelioration,
            spectre_transport,
            start_superres,
            superres_state,
            superres_disponible,
            set_lecture_hd,
            superres_cache,
            vider_cache_hd,
            dossier_donnees,
            play,
            set_queue,
            remplacer_file,
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
    use super::{
        cle_pochette, dans_le_contour, ecrire_cache_pochette, lire_cache_pochette,
        fin_de_trace, morceaux_le_long, sous_une_racine, AccrochageVoirie, RepereLocal,
    };
    use std::collections::{HashMap, HashSet};
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

    fn dossier_de_test(nom: &str) -> std::path::PathBuf {
        // `std::env::temp_dir()`, jamais un chemin fixe du dépôt — voir la
        // consigne de l'incident des 199 Go.
        let d = std::env::temp_dir().join(format!(
            "rusty-music-test-pochettes-{nom}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    #[test]
    fn une_cle_change_avec_le_chemin_et_la_mtime() {
        let t0 = std::time::UNIX_EPOCH;
        let t1 = t0 + std::time::Duration::from_secs(1);
        let a = Path::new("/a.mp3");
        let b = Path::new("/b.mp3");
        assert_ne!(cle_pochette(a, t0), cle_pochette(b, t0), "chemins différents");
        assert_ne!(cle_pochette(a, t0), cle_pochette(a, t1), "mtimes différentes");
        assert_eq!(cle_pochette(a, t0), cle_pochette(a, t0), "déterministe");
    }

    #[test]
    fn le_cache_sert_ce_quil_a_recu() {
        let dossier = dossier_de_test("sert");
        let cle = "abc";
        ecrire_cache_pochette(&dossier, cle, Some("data:image/jpeg;base64,zzz"));
        assert_eq!(
            lire_cache_pochette(&dossier, cle),
            Some(Some("data:image/jpeg;base64,zzz".to_string()))
        );
        std::fs::remove_dir_all(&dossier).ok();
    }

    #[test]
    fn le_cache_distingue_absent_de_confirme_sans_pochette() {
        let dossier = dossier_de_test("sans");
        assert_eq!(
            lire_cache_pochette(&dossier, "jamais-vu"),
            None,
            "pas encore en cache"
        );
        ecrire_cache_pochette(&dossier, "sans-art", None);
        assert_eq!(
            lire_cache_pochette(&dossier, "sans-art"),
            Some(None),
            "en cache, confirmé sans pochette"
        );
        std::fs::remove_dir_all(&dossier).ok();
    }

    #[test]
    fn une_mtime_differente_retrouve_une_cle_differente_donc_pas_lentree_perimee() {
        let dossier = dossier_de_test("perime");
        let chemin = Path::new("/musique/piste.mp3");
        let ancienne = cle_pochette(chemin, std::time::UNIX_EPOCH);
        ecrire_cache_pochette(&dossier, &ancienne, Some("vieille-pochette"));

        let nouvelle = cle_pochette(
            chemin,
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(60),
        );
        assert_eq!(
            lire_cache_pochette(&dossier, &nouvelle),
            None,
            "le fichier a changé : la clé change, l'ancienne entrée ne sert plus"
        );
        std::fs::remove_dir_all(&dossier).ok();
    }

    /// Un accrochage monté à la main : trois sommets alignés, un morceau par
    /// sommet, plus un intrus d'une autre famille sur le sommet du milieu.
    fn accrochage_dessai() -> AccrochageVoirie {
        let mut sommet_de = HashMap::new();
        let mut morceaux_a: HashMap<u32, Vec<i64>> = HashMap::new();
        for (id, s) in [(10, 0u32), (20, 1), (30, 1), (40, 2)] {
            sommet_de.insert(id, s);
            morceaux_a.entry(s).or_default().push(id);
        }
        AccrochageVoirie { sommet_de, morceaux_a }
    }

    #[test]
    fn morceaux_le_long_ordonne_dedoublonne_et_borne_la_famille() {
        let acc = accrochage_dessai();
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 1), (2, 2)].into_iter().collect();
        let famille: HashSet<i64> = [10, 20, 40].into_iter().collect(); // 30 exclu
        let duree = |_id: i64| 0u64;
        let suite = morceaux_le_long(&acc, &couloir, 10, Some(40), Some(&famille), None, &duree);
        // départ 10 en tête, arrivée 40 en queue, 30 filtré, 20 au milieu.
        assert_eq!(suite, vec![10, 20, 40]);
    }

    #[test]
    fn morceaux_le_long_coupe_a_la_duree_cible() {
        let acc = accrochage_dessai();
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 1), (2, 2)].into_iter().collect();
        let duree = |_id: i64| 10 * 60_000u64; // 10 min chacun
        // Cible 25 min, tolérance 90 s : 10 (0) + 20 (10) + 30 (20) → 30 min ≥ 23,5.
        let suite = morceaux_le_long(&acc, &couloir, 10, None, None, Some(25 * 60_000), &duree);
        assert_eq!(suite, vec![10, 20, 30]);
    }

    #[test]
    fn morceaux_le_long_larrivee_prime_sur_la_duree() {
        let acc = accrochage_dessai();
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 1), (2, 2)].into_iter().collect();
        let duree = |_id: i64| 10 * 60_000u64;
        // Arrivée 40 posée : « va jusque-là » l'emporte, la durée (15 min) est
        // ignorée — la playlist va jusqu'à 40.
        let suite = morceaux_le_long(&acc, &couloir, 10, Some(40), None, Some(15 * 60_000), &duree);
        assert_eq!(suite, vec![10, 20, 30, 40]);
    }

    #[test]
    fn fin_de_trace_sarrete_au_dernier_morceau_pas_a_la_destination() {
        let acc = accrochage_dessai(); // 10@sommet0, 20&30@sommet1, 40@sommet2
        // Tracé de 10 sommets ; le couloir place le sommet 1 au rang 3.
        let couloir: HashMap<u32, usize> =
            [(0u32, 0usize), (1, 3), (2, 8)].into_iter().collect();
        // Playlist = [10, 20] → dernier morceau au rang 3 → tracé coupé à 4.
        assert_eq!(fin_de_trace(&[10, 20], &acc, &couloir, 10), 4);
        // Playlist qui va jusqu'à 40 (rang 8) → coupé à 9.
        assert_eq!(fin_de_trace(&[10, 20, 40], &acc, &couloir, 10), 9);
        // Morceaux hors couloir → tout le tracé.
        assert_eq!(fin_de_trace(&[99], &acc, &couloir, 10), 10);
        // Tracé dégénéré.
        assert_eq!(fin_de_trace(&[10], &acc, &couloir, 1), 1);
    }

    #[test]
    fn repere_local_corrige_lanisotropie_lon_lat() {
        // Trois points autour de Notre-Dame : l'un ~730 m à l'est (Δlon), l'autre
        // ~1110 m au nord (Δlat). En degrés bruts, 0,01° dans les deux sens
        // paraîtraient à égale distance ; en mètres, l'est est bien plus proche.
        let ancre = (0i64, 2.35_f32, 48.85_f32);
        let est = (1i64, 2.36_f32, 48.85_f32);
        let nord = (2i64, 2.35_f32, 48.86_f32);
        let mut pts = vec![ancre, est, nord];

        // En degrés bruts, un écart de 0,01° pèse pareil au nord et à l'est —
        // c'est justement le biais que la projection corrige.
        let d2_deg = |a: (i64, f32, f32), b: (i64, f32, f32)| {
            (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)
        };
        assert!((d2_deg(ancre, est) - d2_deg(ancre, nord)).abs() < 1e-6);

        RepereLocal::projeter_liste(&mut pts);
        let d2 = |a: usize, b: usize| {
            (pts[a].1 - pts[b].1).powi(2) + (pts[a].2 - pts[b].2).powi(2)
        };
        // 0 = ancre, 1 = est (~730 m), 2 = nord (~1110 m).
        assert!(d2(0, 1) < d2(0, 2), "après projection, l'est doit être le plus proche");
        let est_m = d2(0, 1).sqrt();
        assert!((est_m - 730.0).abs() < 40.0, "~730 m attendus, obtenu {est_m}");
    }
}
