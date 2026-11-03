// SPDX-License-Identifier: GPL-3.0-or-later
//! Application de bureau — coquille « Atelier », mode Écoute (module 1).
//!
//! Cette couche ne fait que raccorder : toute la logique vit dans `rusty-music-core`
//! (consultation de la base) et `rusty-music-player` (sortie audio, transport). Elle
//! n'ouvre jamais un fichier musical elle-même.

// Sur Windows, évite d'ouvrir une console derrière la fenêtre.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusty_music_analysis::chemin::{echantillonner, Empreinte, Graphe};
use rusty_music_core::db::{AlbumNoeud, AlbumRow, ArtistRow, MapPoint, RootRow, TrackRow};
use rusty_music_core::filtres_playlist::{self, FiltresPlaylist};
use rusty_music_core::memoire::{ListeIntelligente, SuiviEcoute};
use rusty_music_core::playlists::PlaylistRow;
use rusty_music_core::session::{retenir, Session};
use rusty_music_core::Library;
use rusty_music_player::Player;
use rusty_music_transcription::tablature;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Hachage de `ui/`, injecté par `build.rs`. Jamais lu à l'exécution — sa seule
/// fonction est de faire dépendre la compilation de ce fichier du contenu de
/// l'interface, pour que `generate_context!` ré-embarque les assets dès qu'un
/// fichier de `ui/` change. Voir `build.rs`.
const _UI_HASH: &str = env!("RUSTY_UI_HASH");

mod a_propos;
mod bibliotheque;
mod decouvrir;
mod ecouter;
mod editer;
mod editions;
mod explorer;
mod iphone;
mod minuteur;
mod passe;
mod systeme;
mod systeme_media;
mod tuiles;
mod valise;
mod wifi;

// Les commandes vivent dans un module par mode ; tout est rendu visible ici,
// pour `generate_handler!` et `main`.
use bibliotheque::*;
use decouvrir::*;
use ecouter::*;
use editer::*;
use editions::*;
use explorer::*;
use passe::*;
use systeme::*;
use valise::*;

// **Toutes les commandes portent `(async)`, et ce n'est pas décoratif.**
//
// Sur une fonction *non* asynchrone, cet attribut ne rend pas la fonction
// asynchrone : il la fait exécuter sur un **pool de fils** au lieu du fil
// principal (`sync_threadpool`, dans la macro de Tauri). Les arguments, les
// verrous et les erreurs se comportent exactement comme avant.
//
// Sans lui, une commande qui touche le disque fige **toute l'interface**, pas
// seulement elle-même. Le cas mesuré : `play` ouvre le fichier et laisse
// symphonia le sonder ; sur une carte SD saturée par une passe d'analyse, la
// totalité des échantillons du profileur montrait le fil principal bloqué dans
// un `read()`. L'application paraissait tourner dans le vide alors qu'elle
// attendait le disque.
//
// La règle est uniforme parce que la frontière ne l'est pas : `skip` ouvre la
// piste suivante, `spectre_transport` décode le morceau entier, et même un
// simple lecteur d'état prend un verrou que ces opérations détiennent. Aucune
// commande d'ici n'a besoin du fil principal — aucune ne touche la fenêtre.

/// Centroïdes d'empreinte par album (identifiant, vecteur) — voir
/// `charger_centroides_albums`.
type CentroidesAlbums = Vec<(i64, Vec<f32>)>;
/// Métadonnées d'album indexées par le même identifiant que
/// [`CentroidesAlbums`].
type NoeudsAlbums = HashMap<i64, AlbumNoeud>;
/// Contenu du cache `Etat::album_centroides` — toujours reconstruits
/// ensemble, avec le nombre d'empreintes qui les a produits (voir
/// [`charger_centroides_albums`]).
type CentroidesEtNoeuds = (usize, Arc<CentroidesAlbums>, Arc<NoeudsAlbums>);

/// État partagé. `rusqlite::Connection` et le lecteur ne sont pas `Sync` : on
/// les protège chacun par un verrou plutôt que d'ouvrir une base par appel.
struct Etat {
    lib: Mutex<Library>,
    player: Mutex<Player>,
    /// Minuteur d'arrêt en cours, s'il y en a un — voir `minuteur.rs`. Ordre des
    /// verrous : `minuteur` avant `player` et `lib`, jamais l'inverse.
    minuteur: Mutex<Option<minuteur::Minuteur>>,
    /// Chemin de la base, pour que le scan puisse ouvrir sa propre connexion.
    db: PathBuf,
    /// Racine du cache HD (`crates/superres`), à côté de la base. Le lecteur y
    /// aiguille les pistes régénérées quand la lecture HD est active.
    hd: PathBuf,
    /// Avancement d'une régénération HD, sondé par l'interface.
    superres: Mutex<EtatSuperres>,
    /// Le modèle AERO, chargé à la première régénération puis gardé (~156 Mo)
    /// — jusqu'à ce que [`liberer_si_inactif`] le décharge faute d'usage
    /// récent (voir [`Cache`]).
    superres_modele: Mutex<Cache<rusty_music_superres::Modele>>,
    scan: Mutex<EtatScan>,
    /// Une surveillance continue par racine (`notify`), démarrée au lancement
    /// pour chaque racine connue et à l'ajout d'une nouvelle ([`demarrer_surveillance`]).
    /// Retirer une entrée (`oublier`) abandonne le handle et arrête la
    /// surveillance correspondante — c'est le seul signal d'arrêt qui existe.
    surveillances: Mutex<std::collections::HashMap<PathBuf, rusty_music_core::watch::Surveillance>>,
    analyse: Mutex<EtatAnalyse>,
    descripteurs: Mutex<EtatDescripteurs>,
    /// Avancement de la passe de loudness EBU R128 (piste + album).
    loudness: Mutex<EtatLoudness>,
    verif_pochettes: Mutex<EtatVerifPochettes>,
    /// Normalisation de volume à la lecture : activation et référence
    /// (piste ou album) — réglage du mode Bibliothèque, lu par le résolveur
    /// de gain installé sur `player` ([`Player::set_gain_resolveur`]) et par
    /// [`gain_pour`] aux quelques sites qui rouvrent la piste en cours.
    normalisation: Arc<ReglagesNormalisation>,
    enrichissement: Mutex<EtatEnrichissement>,
    /// Avancement de la passe de popularité générale (ListenBrainz + Deezer).
    popularite: Mutex<EtatPopularite>,
    /// Avancement de la passe de biographies (TheAudioDB).
    biographies: Mutex<EtatBio>,
    /// Avancement de la passe de critiques (CritiqueBrainz).
    critiques: Mutex<EtatCritiques>,
    /// Avancement de la passe de tags Last.fm — sert le nommage des
    /// familles (`docs/nommage-familles.md`), pas la popularité.
    lastfm: Mutex<EtatLastfm>,
    /// Avancement de la passe « Compléter » (bloc « État » de Bibliothèque),
    /// et la demande d'arrêt, lue entre deux tranches d'artistes.
    completer: Mutex<EtatCompleter>,
    completer_arret: AtomicBool,
    /// Avancement de la passe de liaison Discogs (légère — retrouve l'édition
    /// Discogs de chaque édition MusicBrainz connue).
    discogs_liaison: Mutex<EtatDiscogsLiaison>,
    /// Avancement du téléchargement du dump Discogs.
    discogs_dump: Mutex<EtatDiscogsDump>,
    /// Avancement de l'import (crédits + labels) depuis le dump téléchargé.
    /// Séparé du scan normal — déclenché à la main depuis le bloc « Dump
    /// Discogs » du mode Bibliothèque, pas enchaîné à « Scanner ».
    discogs_import: Mutex<EtatDiscogsImport>,
    /// Avancement de la passe du mode Découvrir (sorties, collaborations,
    /// voisins), sondé par l'interface.
    decouvrir: Mutex<EtatDecouvrir>,
    /// Avancement de l'envoi d'une valise sur l'iPhone (mode Valise).
    valise_envoi: Mutex<EtatEnvoiValise>,
    /// Avancement de la réinstallation de l'app compagnon (mode Valise).
    valise_renouvellement: Mutex<EtatRenouvellement>,
    /// Le serveur Wi-Fi du compagnon : appairage, passage en attente
    /// (`wifi.rs`). Ordre des verrous : `wifi` avant `player`.
    wifi: Mutex<wifi::EtatWifi>,
    demix: Mutex<EtatDemix>,
    /// Où en est la transposition des stems. Elle tourne dans son fil : c'est
    /// une vingtaine de secondes par stem, et l'interface doit rester servie.
    transpose: Mutex<EtatTranspose>,
    /// Le jeu de stems en écoute, s'il y en a un. Il tient sa propre sortie
    /// audio : le lecteur du module 1 se tait pendant ce temps.
    stems: Mutex<Option<rusty_music_player::Multipiste>>,
    /// Le pisteur de pulsation (Beat This!), chargé au premier usage puis
    /// gardé — 83 Mo de poids, 30 ms à charger : le garder évite surtout de
    /// relire le disque à chaque morceau ouvert.
    pisteur: Mutex<Option<rusty_music_editor::pulsation::Pisteur>>,
    /// Le modèle de langue intégré de Lama (Gemma 4 E4B, 4,6 Go), chargé à
    /// la première demande, déchargé après inactivité.
    #[cfg(feature = "lama-local")]
    lama_local: Mutex<Cache<Arc<rusty_music_core::llm_local::ModeleLocal>>>,
    /// Avancement du téléchargement de ce modèle.
    lama_telechargement: Mutex<EtatTelechargementLama>,
    /// Basic Pitch (transcription), chargé au premier usage puis gardé —
    /// 230 Ko de poids, une session ONNX Runtime.
    transcripteur: Mutex<Option<rusty_music_transcription::basic_pitch::Transcripteur>>,
    /// ADTOF (batterie), chargé au premier usage puis gardé.
    batteur: Mutex<Option<rusty_music_transcription::batterie::Batteur>>,
    /// MuScriptor (transcription de qualité de la basse) : 1,2 Go de poids
    /// sur le GPU, chargés au premier usage, déchargés après inactivité
    /// (voir [`Cache`]).
    muscriptor: Mutex<
        Cache<
            rusty_music_transcription::muscriptor::Muscriptor<
                rusty_music_transcription::muscriptor::Moteur,
            >,
        >,
    >,
    /// Avancement d'une transcription MuScriptor (segments de 5 s faits sur
    /// le total), sondé par l'interface : une minute environ par morceau.
    transcription: Mutex<EtatTranscription>,
    /// all-in-one (sections du morceau) : 8 plis, chargés au premier usage,
    /// déchargés après inactivité.
    sections: Mutex<
        Cache<
            rusty_music_transcription::sections::Sections<
                rusty_music_transcription::sections::Moteur,
            >,
        >,
    >,
    /// Empreintes chargées pour le calcul des chemins. Les relire à chaque
    /// requête coûterait 55 Mo de lecture ; on les garde, en réinvalidant
    /// quand leur nombre change — ce qui arrive tant que l'analyse tourne.
    /// Sous `Arc` pour que les calculs longs travaillent sur une copie du
    /// pointeur, verrou relâché : construire le graphe prend une dizaine de
    /// secondes, pendant lesquelles l'inspecteur doit rester servi.
    ///
    /// Le contenu n'est jamais absent (un cache vide est simplement un `Vec`
    /// vide) — voir [`liberer_vecteurs_si_inactif`], seule variante qui ne
    /// vide pas le `Cache` lui-même, seulement le `Vec` qu'il porte.
    vecteurs: Mutex<Cache<Arc<Vec<Empreinte>>>>,
    /// Graphe des plus proches voisins, avec le nombre d'empreintes qui l'a
    /// produit. Le construire est un balayage complet : on ne le refait que
    /// lorsque ce nombre a bougé, et seulement pour les modes qui en ont
    /// besoin (sonique et errance). Éviction par inactivité, voir [`Cache`].
    graphe: Mutex<Cache<(usize, Arc<Graphe>)>>,
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
    /// Centroïdes d'empreinte par album (mode Explorer → Anneau), avec le
    /// nombre d'empreintes qui les a produits — même invalidation que
    /// `graphe`. Pas de balayage O(n²) ici (voir `album_embeddings`), donc
    /// pas besoin d'un verrou de construction séparé : le calcul est assez
    /// court pour rester sous le verrou du cache lui-même. Toujours
    /// reconstruits et déchargés ensemble — d'où un seul `Cache` pour les
    /// deux plutôt qu'une paire de champs séparés.
    album_centroides: Mutex<Cache<CentroidesEtNoeuds>>,
    /// Graphe des k plus proches albums par empreinte — le fond permanent du
    /// mode Explorer → Anneau (voir `reseau_albums`), distinct du graphe des
    /// morceaux (`graphe` ci-dessus). `k` couvre déjà le maximum du curseur
    /// « voisins » du rail : pas la peine de le reconstruire quand l'anneau
    /// en redemande simplement plus large. Éviction par inactivité, voir [`Cache`].
    album_graphe: Mutex<Cache<(usize, Arc<Graphe>)>>,
    /// Le réseau de circulation, bâti à la première demande d'itinéraire.
    /// Une trentaine de secondes, dominées par le graphe des voisins — comme
    /// `graphe`, on ne le refait pas à chaque trajet.
    reseau: Mutex<Option<rusty_music_analysis::reseau::Reseau>>,
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
    /// L'encodeur texte de CLAP (champ d'intention d'Explorer), chargé à la
    /// première requête puis gardé — 478 Mo de poids, pas question de les
    /// relire à chaque prompt (un « Recomposer » rejoue le même plan sans
    /// repasser par Ollama). Même patron que `superres_modele`, y compris
    /// pour l'éviction par inactivité.
    texte_modele: Mutex<Cache<rusty_music_analysis::EmbedderTexte>>,
    /// Un seul préchargement à la fois — voir `precharger_suivante`. Verrou
    /// distinct de `player` : il est tenu pendant toute l'ouverture du
    /// fichier, que `player` ne doit jamais attendre.
    precharge: Mutex<()>,
}

/// Délai d'inactivité au-delà duquel un poids lourd chargé à la demande
/// (`texte_modele`, `superres_modele`) est déchargé — voir le fil lancé dans
/// `main`. Choisi assez long pour ne jamais gêner un « Recomposer » qui
/// rejoue le même plan coup sur coup (quelques secondes entre deux clics),
/// assez court pour rendre la mémoire une fois qu'on est passé à l'écoute :
/// mesuré en pratique, le champ d'intention à lui seul faisait grimper le
/// processus de 76 Mo à 938 Mo, l'essentiel restant en mémoire sans plus
/// jamais servir une fois la playlist composée.
const SEUIL_EVICTION_POIDS: Duration = Duration::from_secs(3 * 60);

/// Un contenu mis en cache et l'instant de son dernier usage, sous un seul
/// verrou.
///
/// Remplace les anciennes paires `Mutex<Option<T>>` + `Mutex<Option<Instant>>`
/// (une par cache lourd de [`Etat`]) : les chargeurs les verrouillaient dans
/// l'ordre contenu-puis-horodatage, tandis que [`liberer_si_inactif`],
/// appelée par le fil d'éviction, les verrouillait dans l'ordre inverse — deux
/// verrous distincts pris en ordre opposé selon le chemin, ce qui pouvait
/// interbloquer si un chargement et une éviction se croisaient. Un seul
/// verrou pour les deux élimine la question de l'ordre.
struct Cache<T> {
    valeur: Option<T>,
    touche: Option<Instant>,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            valeur: None,
            touche: None,
        }
    }
}

/// Verrou tolérant à l'empoisonnement : une panique survenue ailleurs pendant
/// que ce verrou était tenu ne doit pas condamner toutes les commandes qui le
/// prennent ensuite — voir la garde anti-panique posée sur chaque fil de
/// passe de fond, qui rend cet empoisonnement possible mais transitoire.
fn verrou<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Décharge `cache.valeur` si `cache.touche` indique plus de
/// [`SEUIL_EVICTION_POIDS`] d'inactivité — appelé périodiquement par le fil
/// d'éviction lancé dans `main`. Le prochain usage recharge simplement depuis
/// le disque.
fn liberer_si_inactif<T>(cache: &Mutex<Cache<T>>, nom: &str) {
    let mut c = verrou(cache);
    let Some(dernier) = c.touche else { return };
    if dernier.elapsed() < SEUIL_EVICTION_POIDS {
        return;
    }
    if c.valeur.take().is_some() {
        tracing::info!(nom, "mémoire : poids déchargés après inactivité");
    }
    c.touche = None;
}

/// Variante de [`liberer_si_inactif`] pour `vecteurs`, qui n'est jamais
/// absent (un cache vide est simplement un `Vec` vide) — `charger_vecteurs`
/// recharge dès que sa taille ne correspond plus au compte de la base, donc
/// le vider suffit à forcer la relecture au prochain besoin.
fn liberer_vecteurs_si_inactif(etat: &Etat) {
    let mut c = verrou(&etat.vecteurs);
    let Some(dernier) = c.touche else { return };
    if dernier.elapsed() < SEUIL_EVICTION_POIDS {
        return;
    }
    let vide = c.valeur.as_ref().is_none_or(|v| v.is_empty());
    if !vide {
        c.valeur = Some(Arc::new(Vec::new()));
        tracing::info!("mémoire : empreintes déchargées après inactivité");
    }
    c.touche = None;
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
struct AvancementScan {
    racine: String,
    /// Morceaux en base : seule mesure d'avancement disponible, le scan ne
    /// rend son rapport qu'à la fin.
    morceaux: i64,
}
type EtatScan = Passe<AvancementScan>;

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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tracing_subscriber::layer::SubscriberExt;
            use tracing_subscriber::util::SubscriberInitExt;

            // La base vit à côté des données de l'application, pas dans le
            // répertoire courant : une app de bureau n'a pas de « cwd » stable.
            // `RUSTY_MUSIC_DONNEES` : un autre dossier de données, pour les essais
            // (l'audit d'interface travaille sur une **copie** de la base : il
            // parcourt tous les écrans, dont certains écrivent — « tout vu » de
            // Découvrir, par exemple). Sans effet en usage normal.
            let dossier = match std::env::var_os("RUSTY_MUSIC_DONNEES") {
                Some(d) if !d.is_empty() => {
                    let d = PathBuf::from(d);
                    std::fs::create_dir_all(&d)?;
                    d
                }
                _ => app.path().app_data_dir()?,
            };
            let db = dossier.join("rusty-music.db");
            let hd = dossier.join("hd");
            // Les poids téléchargés par l'application (variantes de démixage
            // absentes) vont ici : c'est le seul endroit inscriptible d'une
            // application empaquetée.
            rusty_music_core::modeles::definir_dossier_utilisateur(dossier.join("models"));

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

            // Normalisation de volume (mode Bibliothèque) : activation et
            // référence poussées par l'interface (`set_normalisation`), lues
            // à chaque ouverture interne de piste ([`Player::completer`]).
            // Connexion dédiée, sur le principe de `resoudre` pour le cache
            // HD : le lecteur ignore tout de la base par ailleurs (voir son
            // en-tête), et SQLite en WAL tolère ce second lecteur sans se
            // disputer avec `Etat.lib`.
            let normalisation = Arc::new(ReglagesNormalisation::default());
            let normalisation_pour_gain = normalisation.clone();
            let lib_pour_gain = Mutex::new(Library::open(&db)?);
            player.set_gain_resolveur(move |chemin| {
                if !normalisation_pour_gain.actif.load(Ordering::Relaxed) {
                    return 1.0;
                }
                let mode = if normalisation_pour_gain.album.load(Ordering::Relaxed) {
                    rusty_music_core::loudness::ModeGain::Album
                } else {
                    rusty_music_core::loudness::ModeGain::Piste
                };
                verrou(&lib_pour_gain)
                    .gain_lecture(chemin, mode, rusty_music_core::loudness::CIBLE_LUFS)
                    .ok()
                    .flatten()
                    .unwrap_or(1.0)
            });

            app.manage(Etat {
                lib: Mutex::new(Library::open(&db)?),
                player: Mutex::new(player),
                minuteur: Mutex::new(None),
                db,
                hd,
                loudness: Mutex::new(EtatLoudness::default()),
                verif_pochettes: Mutex::new(EtatVerifPochettes::default()),
                normalisation,
                superres: Mutex::new(EtatSuperres::default()),
                superres_modele: Mutex::new(Cache::default()),
                scan: Mutex::new(EtatScan::default()),
                surveillances: Mutex::new(std::collections::HashMap::new()),
                analyse: Mutex::new(EtatAnalyse::default()),
                descripteurs: Mutex::new(EtatDescripteurs::default()),
                enrichissement: Mutex::new(EtatEnrichissement::default()),
                popularite: Mutex::new(EtatPopularite::default()),
                biographies: Mutex::new(EtatBio::default()),
                critiques: Mutex::new(EtatCritiques::default()),
                lastfm: Mutex::new(EtatLastfm::default()),
                completer: Mutex::new(EtatCompleter::default()),
                completer_arret: AtomicBool::new(false),
                discogs_liaison: Mutex::new(EtatDiscogsLiaison::default()),
                discogs_dump: Mutex::new(EtatDiscogsDump::default()),
                discogs_import: Mutex::new(EtatDiscogsImport::default()),
                decouvrir: Mutex::new(EtatDecouvrir::default()),
                valise_envoi: Mutex::new(EtatEnvoiValise::default()),
                valise_renouvellement: Mutex::new(EtatRenouvellement::default()),
                wifi: Mutex::new(wifi::EtatWifi::default()),
                demix: Mutex::new(EtatDemix::default()),
                transpose: Mutex::new(EtatTranspose::default()),
                stems: Mutex::new(None),
                pisteur: Mutex::new(None),
                #[cfg(feature = "lama-local")]
                lama_local: Mutex::new(Cache::default()),
                lama_telechargement: Mutex::new(EtatTelechargementLama::default()),
                transcripteur: Mutex::new(None),
                batteur: Mutex::new(None),
                muscriptor: Mutex::new(Cache::default()),
                transcription: Mutex::new(EtatTranscription::default()),
                sections: Mutex::new(Cache::default()),
                // Jamais absent (voir la doc du champ) : un `Cache` par
                // défaut serait `valeur: None`, ce qui forcerait un premier
                // rechargement même sur une base vide — poser directement un
                // `Vec` vide évite cet aller-retour superflu.
                vecteurs: Mutex::new(Cache { valeur: Some(Arc::new(Vec::new())), touche: None }),
                graphe: Mutex::new(Cache::default()),
                graphe_construction: Mutex::new(()),
                graphe_fait: AtomicUsize::new(0),
                graphe_total: AtomicUsize::new(0),
                album_centroides: Mutex::new(Cache::default()),
                album_graphe: Mutex::new(Cache::default()),
                reseau: Mutex::new(None),
                ville: Mutex::new(None),
                graphe_reel: Mutex::new(None),
                accrochage_voirie: Mutex::new(None),
                graphes_voirie: Mutex::new(std::collections::HashMap::new()),
                agrement_voirie: Mutex::new(None),
                texte_modele: Mutex::new(Cache::default()),
                precharge: Mutex::new(()),
            });
            app.manage(tuiles::Archives::default());

            // Le serveur Wi-Fi du compagnon iOS, si un iPhone a été appairé.
            wifi::demarrer(app.handle());

            // Réalimente la file indépendamment du sondage de l'interface —
            // voir `precharger_suivante`. Fil dédié plutôt que raccroché à un
            // évènement de fenêtre : plus simple, et couvre aussi bien la
            // minimisation que la perte de focus ou l'occlusion.
            let etat_arriere_plan = app.handle().clone();
            std::thread::spawn(move || {
                // Mémoire d'écoute : le même battement natif compte le temps
                // réellement joué (`suivre_ecoute`).
                let mut suivi = SuiviEcoute::nouveau();
                // Et l'annonce au système (« En cours de lecture »).
                let mut annonce = systeme_media::Annonce::nouveau(std::time::Instant::now());
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Some(etat) = etat_arriere_plan.try_state::<Etat>() {
                        if let Err(e) = precharger_suivante(&etat) {
                            tracing::warn!(erreur = %e, "préchargement en arrière-plan impossible");
                        }
                        suivre_ecoute(&etat, &mut suivi);
                        systeme_media::annoncer(&etat_arriere_plan, &etat, &mut annonce);
                    }
                }
            });

            // Reprise de session : la file et la position, relues ici (en
            // pause, voir `restaurer_session`) puis réécrites toutes les cinq
            // secondes. Côté natif plutôt que par l'interface, pour la même
            // raison que le préchargement : une fenêtre masquée ralentit les
            // temporisateurs de la webview.
            let etat_session = app.handle().clone();
            std::thread::spawn(move || {
                if let Some(etat) = etat_session.try_state::<Etat>() {
                    match restaurer_session(&etat) {
                        // La page ne sonde le lecteur que quand il joue : on la
                        // prévient, sans quoi une reprise en pause resterait
                        // invisible. (Si elle n'est pas encore chargée, son
                        // propre battement de démarrage voit l'état.)
                        Ok(true) => {
                            let _ = etat_session.emit("session-restauree", ());
                        }
                        Ok(false) => {}
                        Err(e) => tracing::warn!(erreur = %e, "reprise de la session impossible"),
                    }
                }
                let mut memo = MemoSession::default();
                loop {
                    std::thread::sleep(Duration::from_secs(5));
                    if let Some(etat) = etat_session.try_state::<Etat>() {
                        if let Err(e) = sauvegarder_session(&etat, &mut memo) {
                            tracing::warn!(erreur = %e, "sauvegarde de la session impossible");
                        }
                    }
                }
            });

            // Minuteur d'arrêt : un fil à part, au battement adaptatif (500 ms au
            // repos, 100 ms armé, 20 ms dans la dernière seconde d'un morceau).
            let etat_minuteur = app.handle().clone();
            std::thread::spawn(move || loop {
                let attente = match etat_minuteur.try_state::<Etat>() {
                    Some(etat) => surveiller_minuteur(&etat),
                    None => Duration::from_millis(500),
                };
                std::thread::sleep(attente);
            });

            // Décharge l'encodeur texte CLAP (478 Mo) et le modèle AERO
            // (156 Mo) une fois qu'ils n'ont plus servi depuis
            // `SEUIL_EVICTION_POIDS` — voir sa documentation. Un intervalle
            // large (une minute) : ce n'est pas une horloge à respecter au
            // Mo près, juste rendre la mémoire pendant qu'on écoute la
            // playlist plutôt qu'un déclencheur ponctuel.
            let etat_eviction = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(60));
                if let Some(etat) = etat_eviction.try_state::<Etat>() {
                    liberer_si_inactif(&etat.texte_modele, "encodeur texte CLAP");
                    liberer_si_inactif(&etat.superres_modele, "modèle AERO");
                    liberer_si_inactif(&etat.muscriptor, "MuScriptor");
                    #[cfg(feature = "lama-local")]
                    liberer_si_inactif(&etat.lama_local, "modèle de langue de Lama");
                    liberer_si_inactif(&etat.sections, "all-in-one");
                    // Même patron pour la continuation automatique (errance/
                    // sonique) : ses caches survivent, eux aussi, bien après
                    // qu'une playlist a été composée — voir la documentation
                    // de `vecteurs`.
                    liberer_vecteurs_si_inactif(&etat);
                    liberer_si_inactif(&etat.graphe, "graphe des voisins");
                    liberer_si_inactif(&etat.album_centroides, "centroïdes d'albums");
                    liberer_si_inactif(&etat.album_graphe, "graphe des albums");
                }
            });

            // Rattrapage puis surveillance continue de chaque racine déjà
            // connue — même patron que `rusty-music watch` en CLI
            // (`scan_root_jobs` puis `watch_root`). La surveillance seule ne
            // suffit pas : `notify` ne voit que ce qui bouge pendant qu'elle
            // tourne, et un dossier ajouté ou modifié pendant que
            // l'application était fermée resterait invisible jusqu'au
            // prochain scan manuel. Le rattrapage est incrémental (taille et
            // mtime inchangées ⇒ sauté), donc bon marché au démarrage normal.
            // Échec toléré par racine : une racine débranchée (clé USB,
            // partage réseau) ne doit pas empêcher les autres de démarrer.
            let racines = verrou(&app.state::<Etat>().lib).roots().unwrap_or_default();
            let app_rattrapage = app.handle().clone();
            std::thread::spawn(move || {
                for r in racines {
                    let racine = PathBuf::from(&r.path);
                    let etat = app_rattrapage.state::<Etat>();
                    let deja_en_cours = {
                        let mut s = verrou(&etat.scan);
                        if s.en_cours {
                            true
                        } else {
                            *s = EtatScan {
                                en_cours: true,
                                avancement: AvancementScan {
                                    racine: r.path.clone(),
                                    morceaux: 0,
                                },
                                resultat: None,
                            };
                            false
                        }
                    };
                    // Un scan manuel a démarré entre-temps (l'utilisateur n'a
                    // pas attendu) : on ne le double pas, la surveillance
                    // seule suffira pour cette racine-ci — un prochain scan,
                    // manuel ou au prochain lancement, rattrapera le reste.
                    if deja_en_cours {
                        demarrer_surveillance(&etat, &racine);
                        continue;
                    }
                    // Comme pour tout scan : une panique (tags d'un fichier
                    // arbitraire) laisserait sinon le scan « en cours » jusqu'au
                    // redémarrage, et « Scanner » refusé d'ici là.
                    let _garde = GardePasse::nouvelle(&etat.scan);
                    let succes = match Library::open(&etat.db) {
                        Ok(lib) => {
                            let jobs = coeurs_arriere_plan();
                            match rusty_music_core::scan::scan_root_jobs(&lib, &racine, jobs, false) {
                                Ok(rep) => {
                                    tracing::info!(root = %racine.display(), ?rep, "rattrapage au démarrage");
                                    true
                                }
                                Err(e) => {
                                    tracing::warn!(root = %racine.display(), %e, "rattrapage impossible");
                                    false
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(%e, "rattrapage : base inaccessible");
                            false
                        }
                    };
                    { let mut s = verrou(&etat.scan);
                        s.en_cours = false;
                    }
                    if succes {
                        demarrer_surveillance(&etat, &racine);
                    }
                }
            });

            // La fenêtre est bâtie ici, pas déclarée dans `tauri.conf.json`, pour
            // une seule raison : y accrocher `on_web_resource_request`.
            //
            // **Sans cache-control, la webview sert une interface (`ui/`) périmée.** Tauri
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
                    // Sans ça, la webview capte le glisser-déposer au niveau
                    // natif (macOS/Windows) pour son propre dépôt de fichiers
                    // — jamais utilisé ici — et les évènements DOM
                    // `dragover`/`drop` de la file d'attente (`ui/js/`) ne
                    // sont plus délivrés du tout.
                    .disable_drag_drop_handler()
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

            let cache_pochettes = dossier.join("pochettes");
            if cache_pochettes.is_dir() {
                purger_negatifs_pochettes(&cache_pochettes);
            }

            // Sans cela, la fenêtre s'ouvre parfois sans être le répondeur
            // clavier : les raccourcis (espace, flèches) n'arrivent alors
            // jamais au JS tant qu'on n'a pas cliqué une première fois dans
            // la fenêtre.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }

            // Commandes à distance du système (macOS) ; ailleurs, ou si le système
            // refuse, les raccourcis globaux — voir `systeme_media`.
            if !systeme_media::demarrer(app.handle()) {
                enregistrer_touches_media(app.handle().clone());
            }

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
            descripteurs,
            map_view,
            engendrer_tuiles,
            style_carte,
            positions_carte,
            trace_rues,
            journal_carte,
            journal_ui,
            mode_initial,
            autotest_carte,
            itineraire,
            itineraire_voirie,
            tuile,
            map_progress,
            graphe_progress,
            library_stats,
            probable_duplicates,
            isolated_points,
            suspect_genres,
            multiple_editions,
            editions_proposees,
            edition_comparee,
            retenir_edition,
            edition_retenue,
            oublier_edition,
            scan_failures,
            dismiss_scan_failure,
            map_parameters,
            set_map_parameter,
            vocabulaire_familles,
            definir_vocabulaire_familles,
            recompute_map,
            start_analysis,
            analysis_state,
            start_descripteurs,
            descripteurs_state,
            start_loudness,
            loudness_state,
            start_verif_pochettes,
            verif_pochettes_state,
            pochettes_suspectes,
            ignore_pochette_suspecte,
            pochette_du_dossier,
            set_normalisation,
            start_enrichment,
            enrichment_state,
            start_popularite,
            popularite_state,
            popularite_fraicheur,
            start_biographies,
            biographies_state,
            bio_artiste,
            start_critiques,
            critiques_state,
            critiques_piste,
            lien_ecrire_critique,
            start_lastfm,
            lastfm_state,
            famille_piste,
            families_detail,
            subdiviser_famille,
            start_discogs_liaison,
            discogs_liaison_state,
            discogs_dump_info,
            start_discogs_dump,
            discogs_dump_state,
            start_discogs_import,
            discogs_import_state,
            discogs_import_utile,
            credits_piste,
            labels_piste,
            popularites,
            artist_links,
            start_decouvrir,
            decouvrir_state,
            decouvrir_feed,
            decouvrir_tout_vu,
            valise_reglages,
            valise_apercu,
            valise_iphone,
            valise_envoyer,
            valise_renouveler,
            valise_etat,
            emporter_lecture,
            wifi::wifi_etat,
            wifi::oublier_iphone,
            completer_apercu,
            completer_lancer,
            completer_arreter,
            completer_etat,
            completer_annuler,
            decouvrir_pochette,
            decouvrir_photo_artiste,
            decouvrir_lien_deezer,
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
            demix_variantes,
            stems_play,
            stems_gain,
            stems_transport,
            stems_boucle,
            stems_decompter,
            pulsation,
            transcrire,
            etat_transcription,
            muscriptor_disponible,
            sections_disponibles,
            sections,
            muscriptor_jeton_present,
            telecharger_muscriptor,
            transcriptions_en_cache,
            stems_state,
            stem_spectre,
            voisins_de_stem,
            tempo_cible,
            stems_greffer,
            path,
            path_drawn,
            path_album,
            path_artist,
            shuffle_artist,
            path_texte_interpreter,
            path_texte,
            ollama_modeles,
            lama_disponibilite,
            lama_telecharger,
            lama_telechargement_state,
            essai_lama,
            essai_fenetre,
            voyage,
            selection,
            families,
            album_families,
            artist_families,
            search_albums,
            album_ring,
            artistes_proches,
            reseau_albums,
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
            stockage_logiciel,
            play,
            set_queue,
            a_propos::a_propos,
            remplacer_file,
            toggle_pause,
            skip,
            previous,
            jump_to,
            set_alea,
            set_repetition,
            deplacer_file,
            seek,
            set_volume,
            playback_state,
            file_courante,
            minuteur_demarrer,
            minuteur_annuler,
            minuteur_etat,
            playlists,
            creer_playlist,
            playlist_pistes,
            exporter_playlist,
            basculer_favori,
            favoris,
            listes_intelligentes,
            liste_intelligente,
            renommer_playlist,
            supprimer_playlist,
        ])
        .build(tauri::generate_context!())
        .expect("démarrage de l'application impossible")
        .run(|app, evenement| {
            // Dernière sauvegarde à la fermeture : la périodique (5 s) laisse
            // jusqu'à cinq secondes de position sur la table.
            if let tauri::RunEvent::Exit = evenement {
                if let Some(etat) = app.try_state::<Etat>() {
                    if let Err(e) = sauvegarder_session(&etat, &mut MemoSession::default()) {
                        tracing::warn!(erreur = %e, "sauvegarde finale de la session impossible");
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// `liberer_si_inactif` doit décharger `valeur` une fois le seuil
    /// d'inactivité dépassé, et rester sans effet avant — isolé de `Etat`,
    /// qui ne se construit pas hors de Tauri.
    #[test]
    fn cache_libere_apres_le_seuil_dinactivite() {
        let cache = Mutex::new(Cache {
            valeur: Some(42),
            touche: Some(Instant::now()),
        });
        liberer_si_inactif(&cache, "essai");
        assert_eq!(
            verrou(&cache).valeur,
            Some(42),
            "encore récent : rien à décharger"
        );

        let vieux = Instant::now() - Duration::from_secs(3 * 60 + 1);
        verrou(&cache).touche = Some(vieux);
        liberer_si_inactif(&cache, "essai");
        let g = verrou(&cache);
        assert_eq!(
            g.valeur, None,
            "inactif depuis plus de SEUIL_EVICTION_POIDS : déchargé"
        );
        assert_eq!(g.touche, None, "l'horodatage repart à zéro avec le contenu");
    }

    /// Un cache jamais touché (jamais chargé) ne doit jamais être déchargé —
    /// il n'y a rien à décharger, et `touche` resterait `None` en boucle.
    #[test]
    fn cache_jamais_touche_ne_panique_pas() {
        let cache: Mutex<Cache<i32>> = Mutex::new(Cache::default());
        liberer_si_inactif(&cache, "essai");
        assert_eq!(verrou(&cache).valeur, None);
    }
}
