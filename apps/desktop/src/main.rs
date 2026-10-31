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
mod explorer;
mod iphone;
mod minuteur;
mod systeme;
mod systeme_media;
mod tuiles;
mod valise;

// Les commandes vivent dans un module par mode ; tout est rendu visible ici,
// pour `generate_handler!` et `main`.
use bibliotheque::*;
use decouvrir::*;
use ecouter::*;
use editer::*;
use explorer::*;
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

/// Ce que toute struct `EtatXxx` de passe de fond expose à la garde
/// anti-panique [`GardePasse`] : le signal d'avancement (`en_cours`) que
/// l'interface sonde, et comment le clore proprement si la passe s'arrête
/// autrement qu'en l'écrivant elle-même.
trait EtatPasse {
    fn en_cours(&self) -> bool;
    /// Repositionne l'état comme si la passe avait échoué en interne —
    /// appelé uniquement quand `en_cours` est encore vrai à la destruction de
    /// la garde, c'est-à-dire seulement si le fil s'est arrêté (panique)
    /// avant d'avoir écrit son résultat par le chemin normal.
    fn marquer_echec_interne(&mut self);
}

/// Implémente [`EtatPasse`] pour les structs `EtatXxx` qui suivent toutes le
/// même patron (`en_cours: bool`, `resultat: Option<String>`) — évite de
/// répéter le même corps quinze fois.
macro_rules! impl_etat_passe {
    ($($t:ty),+ $(,)?) => {$(
        impl EtatPasse for $t {
            fn en_cours(&self) -> bool {
                self.en_cours
            }
            fn marquer_echec_interne(&mut self) {
                self.en_cours = false;
                self.resultat = Some("échec : panique interne".to_string());
            }
        }
    )+};
}
impl_etat_passe!(
    EtatScan,
    EtatAnalyse,
    EtatDescripteurs,
    EtatLoudness,
    EtatVerifPochettes,
    EtatPopularite,
    EtatBio,
    EtatCritiques,
    EtatLastfm,
    EtatDiscogsLiaison,
    EtatDiscogsDump,
    EtatDiscogsImport,
    EtatDecouvrir,
    EtatDemix,
    EtatSuperres,
    EtatEnrichissement,
);
impl_etat_passe!(EtatEnvoiValise, EtatRenouvellement);

// `EtatTranspose` porte `erreur`, pas `resultat` — hors du patron commun,
// implémentation à la main.
impl EtatPasse for EtatTranspose {
    fn en_cours(&self) -> bool {
        self.en_cours
    }
    fn marquer_echec_interne(&mut self) {
        self.en_cours = false;
        self.erreur = Some("échec : panique interne".to_string());
    }
}

/// Posée en tête d'un fil de passe de fond (juste après avoir récupéré
/// `etat`) : si le fil se termine — panique comprise — sans avoir écrit son
/// résultat par le chemin normal, `en_cours` redescend quand même à `false`
/// avec un résultat par défaut. Sans elle, une panique dans le travail
/// (décodage, inférence…) laissait la passe « en cours » à vie côté
/// interface, `start_*` refusant tout relancement jusqu'au redémarrage.
///
/// Silencieuse sur le chemin normal : celui-ci écrit déjà `en_cours = false`
/// avant que le fil ne se termine, donc `en_cours()` rend faux à la
/// destruction et la garde ne fait rien.
struct GardePasse<'a, T: EtatPasse> {
    etat: &'a Mutex<T>,
}

impl<'a, T: EtatPasse> GardePasse<'a, T> {
    fn nouvelle(etat: &'a Mutex<T>) -> Self {
        Self { etat }
    }
}

impl<T: EtatPasse> Drop for GardePasse<'_, T> {
    fn drop(&mut self) {
        let mut g = verrou(self.etat);
        if g.en_cours() {
            g.marquer_echec_interne();
            tracing::warn!("passe de fond interrompue par une panique interne");
        }
    }
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
                discogs_liaison: Mutex::new(EtatDiscogsLiaison::default()),
                discogs_dump: Mutex::new(EtatDiscogsDump::default()),
                discogs_import: Mutex::new(EtatDiscogsImport::default()),
                decouvrir: Mutex::new(EtatDecouvrir::default()),
                valise_envoi: Mutex::new(EtatEnvoiValise::default()),
                valise_renouvellement: Mutex::new(EtatRenouvellement::default()),
                demix: Mutex::new(EtatDemix::default()),
                transpose: Mutex::new(EtatTranspose::default()),
                stems: Mutex::new(None),
                pisteur: Mutex::new(None),
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
                                racine: r.path.clone(),
                                morceaux: 0,
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
                    // Sans ça, la webview capte le glisser-déposer au niveau
                    // natif (macOS/Windows) pour son propre dépôt de fichiers
                    // — jamais utilisé ici — et les évènements DOM
                    // `dragover`/`drop` de la file d'attente (`app.js`) ne
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
    use super::{
        cle_pochette, composer_route, dans_le_contour, ecrire_cache_pochette, fin_de_trace,
        liberer_si_inactif, lire_cache_pochette, morceaux_le_long, parties_du_plan,
        purger_negatifs_pochettes, sous_une_racine, verrou, AccrochageVoirie, Cache, Composition,
        EtatScan, GardePasse, PlanTexte, RepereLocal, BRUIT_DEFAUT,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
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

    /// Une panique survenue pendant qu'une `GardePasse` est en vie doit
    /// laisser `en_cours` à `false` et un résultat posé — sans quoi la passe
    /// resterait « en cours » à vie côté interface. Le chemin normal (pas de
    /// panique) doit rester sans effet, la garde restant silencieuse.
    #[test]
    fn garde_passe_cloture_en_cours_apres_une_panique() {
        let etat = Mutex::new(EtatScan {
            en_cours: true,
            ..Default::default()
        });
        let issue = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _garde = GardePasse::nouvelle(&etat);
            panic!("simulation d'un décodage qui tourne mal");
        }));
        assert!(issue.is_err(), "la panique doit bien traverser le test");
        let s = verrou(&etat);
        assert!(
            !s.en_cours,
            "la garde doit clore la passe malgré la panique"
        );
        assert_eq!(s.resultat.as_deref(), Some("échec : panique interne"));
    }

    /// Sur le chemin normal, la garde ne doit rien écraser : le résultat posé
    /// explicitement avant sa destruction doit survivre.
    #[test]
    fn garde_passe_silencieuse_sur_le_chemin_normal() {
        let etat = Mutex::new(EtatScan {
            en_cours: true,
            ..Default::default()
        });
        {
            let _garde = GardePasse::nouvelle(&etat);
            let mut s = verrou(&etat);
            s.en_cours = false;
            s.resultat = Some("terminé normalement".to_string());
        }
        let s = verrou(&etat);
        assert!(!s.en_cours);
        assert_eq!(s.resultat.as_deref(), Some("terminé normalement"));
    }

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
        assert_ne!(
            cle_pochette(a, t0),
            cle_pochette(b, t0),
            "chemins différents"
        );
        assert_ne!(
            cle_pochette(a, t0),
            cle_pochette(a, t1),
            "mtimes différentes"
        );
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
    fn un_negatif_perime_est_redemande_pas_une_pochette_trouvee() {
        let dossier = dossier_de_test("negatif-perime");
        ecrire_cache_pochette(&dossier, "sans", None);
        ecrire_cache_pochette(&dossier, "avec", Some("data:image/jpeg;base64,zzz"));
        for cle in ["sans", "avec"] {
            let f = std::fs::File::options()
                .write(true)
                .open(dossier.join(cle))
                .unwrap();
            f.set_modified(std::time::SystemTime::now() - Duration::from_secs(40 * 24 * 3600))
                .unwrap();
        }
        assert_eq!(
            lire_cache_pochette(&dossier, "sans"),
            None,
            "négatif de 40 j : à redemander"
        );
        assert_eq!(
            lire_cache_pochette(&dossier, "avec"),
            Some(Some("data:image/jpeg;base64,zzz".to_string())),
            "une pochette trouvée ne périme pas"
        );
        std::fs::remove_dir_all(&dossier).ok();
    }

    #[test]
    fn la_purge_efface_les_negatifs_une_seule_fois() {
        let dossier = dossier_de_test("purge");
        ecrire_cache_pochette(&dossier, "sans", None);
        ecrire_cache_pochette(&dossier, "avec", Some("data:image/jpeg;base64,zzz"));
        purger_negatifs_pochettes(&dossier);
        assert!(!dossier.join("sans").exists());
        assert!(dossier.join("avec").exists());
        // Un négatif écrit après la purge est un vrai 404 : on le garde.
        ecrire_cache_pochette(&dossier, "sans2", None);
        purger_negatifs_pochettes(&dossier);
        assert!(dossier.join("sans2").exists());
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
        AccrochageVoirie {
            sommet_de,
            morceaux_a,
        }
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
        let suite = morceaux_le_long(
            &acc,
            &couloir,
            10,
            Some(40),
            None,
            Some(15 * 60_000),
            &duree,
        );
        assert_eq!(suite, vec![10, 20, 30, 40]);
    }

    #[test]
    fn fin_de_trace_sarrete_au_dernier_morceau_pas_a_la_destination() {
        let acc = accrochage_dessai(); // 10@sommet0, 20&30@sommet1, 40@sommet2
                                       // Tracé de 10 sommets ; le couloir place le sommet 1 au rang 3.
        let couloir: HashMap<u32, usize> = [(0u32, 0usize), (1, 3), (2, 8)].into_iter().collect();
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
        let d2_deg =
            |a: (i64, f32, f32), b: (i64, f32, f32)| (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2);
        assert!((d2_deg(ancre, est) - d2_deg(ancre, nord)).abs() < 1e-6);

        RepereLocal::projeter_liste(&mut pts);
        let d2 = |a: usize, b: usize| (pts[a].1 - pts[b].1).powi(2) + (pts[a].2 - pts[b].2).powi(2);
        // 0 = ancre, 1 = est (~730 m), 2 = nord (~1110 m).
        assert!(
            d2(0, 1) < d2(0, 2),
            "après projection, l'est doit être le plus proche"
        );
        let est_m = d2(0, 1).sqrt();
        assert!(
            (est_m - 730.0).abs() < 40.0,
            "~730 m attendus, obtenu {est_m}"
        );
    }

    // ---- composition d'une playlist par texte (`composer_route`) ----------

    use rusty_music_analysis::chemin::{Empreinte, Graphe};
    use rusty_music_core::db::TrackRow;
    use rusty_music_core::filtres_playlist::{
        CaracteristiquesPiste, FiltresPlaylist, NiveauEnergie,
    };

    const N: usize = 300;
    const TROIS_MIN_MS: i64 = 180_000;

    /// 300 morceaux le long d'un arc de cercle (voisins sonores = indices
    /// voisins), un artiste pour 4 morceaux consécutifs, énergie croissante,
    /// rock aux indices pairs et jazz aux impairs, tous de 1990 et de 3 min.
    fn bibliotheque_synthetique() -> (Vec<Empreinte>, Vec<CaracteristiquesPiste>, Graphe) {
        let vecteurs: Vec<Empreinte> = (0..N)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / (N - 1) as f32;
                (i as i64, vec![a.cos(), a.sin(), 0.0])
            })
            .collect();
        let carac = (0..N)
            .map(|i| CaracteristiquesPiste {
                id: i as i64,
                artiste: Some(format!("artiste {}", i / 4)),
                annee: Some(1990),
                duree_ms: Some(TROIS_MIN_MS),
                bpm: Some(100.0),
                energie: Some(i as f32 / N as f32),
                popularite: None,
                jours_depuis_ecoute: None,
                genres: vec![if i % 2 == 0 {
                    "rock".into()
                } else {
                    "jazz".into()
                }],
            })
            .collect();
        let graphe = Graphe::construire(&vecteurs, 8, 1);
        (vecteurs, carac, graphe)
    }

    fn plan_de(n: usize) -> PlanTexte {
        PlanTexte {
            depart: None,
            arrivee: None,
            arrivee_demandee: None,
            etapes: vec!["vers la fin de l'arc".into()],
            n,
            duree_minutes: None,
            plafond_par_artiste: None,
            filtres: FiltresPlaylist::default(),
            reformulation: None,
            parties: Vec::new(),
            brut: String::new(),
        }
    }

    /// Une partie de test : genre, durée en minutes.
    fn partie(genre: &str, minutes: u32) -> rusty_music_core::ollama::PartieLlm {
        rusty_music_core::ollama::PartieLlm {
            description: format!("{genre} music"),
            genres: vec![genre.into()],
            energie: None,
            duree_minutes: Some(minutes),
            n: None,
        }
    }

    fn piste(id: i64) -> TrackRow {
        TrackRow {
            id,
            path: format!("/p/{id}.flac"),
            title: Some(format!("t{id}")),
            artist: Some(format!("artiste {}", id / 4)),
            album: None,
            track_no: None,
            year: Some(1990),
            duration_ms: Some(TROIS_MIN_MS),
            artist_mbid: None,
        }
    }

    fn composer_complet(plan: &PlanTexte) -> Result<Composition, String> {
        let (vecteurs, carac, graphe) = bibliotheque_synthetique();
        // Un « encodeur » de poche : le rock vise le début de l'arc, tout le
        // reste (jazz, étapes ordinaires) la fin.
        let parties = parties_du_plan(plan, |t| {
            Ok(if t.contains("rock") {
                vec![1.0, 0.0, 0.0]
            } else {
                vec![0.0, 1.0, 0.0]
            })
        })?;
        composer_route(plan, &parties, &carac, &vecteurs, &graphe, 7, BRUIT_DEFAUT)
    }

    fn composer(plan: &PlanTexte) -> Result<(Vec<i64>, Vec<String>), String> {
        composer_complet(plan).map(|c| (c.route, c.relaches))
    }

    fn doublons(route: &[i64]) -> usize {
        route.len() - route.iter().collect::<HashSet<_>>().len()
    }

    #[test]
    fn sans_filtre_la_route_a_la_longueur_demandee() {
        // Départ au début de l'arc, cible à l'autre bout : sans départ nommé,
        // « le plus proche de la cible » serait le bout de l'arc lui-même, d'où
        // une marche auto-évitante vite acculée — un artefact de ce décor
        // unidimensionnel, pas du produit.
        let mut plan = plan_de(20);
        plan.depart = Some(piste(0));
        let (route, relaches) = composer(&plan).unwrap();
        assert!(
            route.len() >= 15 && route.len() <= 20,
            "{} morceaux",
            route.len()
        );
        assert_eq!(doublons(&route), 0);
        assert!(relaches.is_empty(), "{relaches:?}");
    }

    #[test]
    fn un_plafond_demande_par_artiste_est_tenu() {
        let mut plan = plan_de(20);
        plan.plafond_par_artiste = Some(2);
        let (route, relaches) = composer(&plan).unwrap();
        let mut par_artiste: HashMap<i64, usize> = HashMap::new();
        for id in &route {
            *par_artiste.entry(id / 4).or_default() += 1;
        }
        // Le départ est protégé : il peut seul dépasser le plafond.
        let depart = route[0] / 4;
        for (a, n) in &par_artiste {
            assert!(*n <= 2 || *a == depart, "artiste {a} : {n} morceaux");
        }
        assert!(
            !relaches.is_empty(),
            "des morceaux ont été retirés : il faut le dire"
        );
    }

    #[test]
    fn une_duree_demandee_est_approchee() {
        let mut plan = plan_de(10);
        plan.duree_minutes = Some(30); // 10 morceaux de 3 min
        let (route, relaches) = composer(&plan).unwrap();
        let total = route.len() as i64 * TROIS_MIN_MS;
        assert!(
            (total - 30 * 60_000).abs() <= TROIS_MIN_MS,
            "{} morceaux, {} min",
            route.len(),
            total / 60_000
        );
        assert!(
            relaches.iter().all(|r| !r.contains("durée visée")),
            "{relaches:?}"
        );
    }

    #[test]
    fn une_duree_hors_de_portee_est_signalee() {
        let mut plan = plan_de(10);
        plan.duree_minutes = Some(1000); // 333 morceaux : la bibliothèque n'en a que 300
        let (_, relaches) = composer(&plan).unwrap();
        assert!(
            relaches.iter().any(|r| r.contains("durée visée")),
            "{relaches:?}"
        );
    }

    /// Large sélection : découpée dans le graphe complet (`restreint`).
    #[test]
    fn exclure_un_genre_ne_laisse_aucun_morceau_de_ce_genre() {
        let mut plan = plan_de(15);
        plan.filtres.exclure_genres = vec!["rock".into()];
        let (route, _) = composer(&plan).unwrap();
        assert!(!route.is_empty());
        assert!(
            route.iter().all(|id| id % 2 == 1),
            "un rock s'est glissé : {route:?}"
        );
    }

    /// Sélection étroite (un tiers) : graphe des voisins bâti sur les seuls
    /// admissibles, faute de quoi la marche s'arrêterait aussitôt.
    #[test]
    fn un_niveau_denergie_restreint_la_marche_aux_morceaux_calmes() {
        let mut plan = plan_de(15);
        plan.filtres.energie = Some(NiveauEnergie::Calme);
        let (route, relaches) = composer(&plan).unwrap();
        assert!(route.len() >= 10, "{} morceaux : {relaches:?}", route.len());
        // Tiers bas des énergies = indices 0..100.
        assert!(route.iter().all(|id| *id <= 100), "{route:?}");
    }

    #[test]
    fn une_arrivee_nommee_termine_la_playlist_sans_doublon() {
        let mut plan = plan_de(15);
        plan.arrivee = Some(piste(250));
        plan.depart = Some(piste(10));
        let (route, _) = composer(&plan).unwrap();
        assert_eq!(route.first(), Some(&10));
        assert_eq!(route.last(), Some(&250));
        assert_eq!(doublons(&route), 0);
    }

    /// Le départ nommé reste admis même s'il contrevient aux filtres :
    /// l'utilisateur l'a cité.
    #[test]
    fn un_depart_nomme_echappe_aux_filtres() {
        let mut plan = plan_de(10);
        plan.depart = Some(piste(10)); // rock
        plan.filtres.exclure_genres = vec!["rock".into()];
        let (route, _) = composer(&plan).unwrap();
        assert_eq!(route.first(), Some(&10));
        assert!(route[1..].iter().all(|id| id % 2 == 1));
    }

    #[test]
    fn des_exclusions_qui_vident_la_bibliotheque_sont_une_erreur_claire() {
        let mut plan = plan_de(10);
        plan.filtres.exclure_genres = vec!["rock".into(), "jazz".into()];
        let e = composer(&plan).unwrap_err();
        assert!(e.contains("exclusions"), "{e}");
    }

    #[test]
    fn un_critere_desiré_sans_resultat_est_assoupli_et_dit() {
        let mut plan = plan_de(10);
        plan.filtres.energie = Some(NiveauEnergie::Calme);
        plan.filtres.annee_min = Some(2000); // aucun morceau n'est de 2000
        let (route, relaches) = composer(&plan).unwrap();
        assert!(!route.is_empty());
        assert!(
            relaches.iter().any(|r| r.contains("période")),
            "{relaches:?}"
        );
    }

    // ---- parties : « X pendant 12 minutes, puis Y pendant 20 minutes » ----

    fn genre_de(id: i64) -> &'static str {
        if id % 2 == 0 {
            "rock"
        } else {
            "jazz"
        }
    }

    fn duree_min(route: &[i64]) -> i64 {
        route.len() as i64 * TROIS_MIN_MS / 60_000
    }

    /// La demande qui a révélé le défaut : avant les parties, les durées se
    /// perdaient et « rock, hip hop » devenait un filtre commun.
    #[test]
    fn deux_parties_respectent_chacune_leur_genre_et_leur_duree() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 12), partie("jazz", 21)];
        let c = composer_complet(&plan).unwrap();
        assert_eq!(c.parties.len(), 2);
        let (a, b) = (&c.parties[0], &c.parties[1]);
        // Chaque partie ne contient que son genre.
        assert!(
            c.route[a.debut..a.fin]
                .iter()
                .all(|id| genre_de(*id) == "rock"),
            "{:?}",
            &c.route[a.debut..a.fin]
        );
        assert!(
            c.route[b.debut..b.fin]
                .iter()
                .all(|id| genre_de(*id) == "jazz"),
            "{:?}",
            &c.route[b.debut..b.fin]
        );
        // Les durées visées, à un morceau près.
        assert!(
            (a.duree_ms / 60_000 - 12).abs() <= 3,
            "partie 1 : {} min",
            a.duree_ms / 60_000
        );
        assert!(
            (b.duree_ms / 60_000 - 21).abs() <= 3,
            "partie 2 : {} min",
            b.duree_ms / 60_000
        );
        assert_eq!(a.fin, b.debut, "les parties se suivent sans trou");
        assert_eq!(b.fin, c.route.len());
        assert_eq!(
            doublons(&c.route),
            0,
            "une partie ne reprend pas un morceau d'une autre"
        );
        assert_eq!(duree_min(&c.route), (a.duree_ms + b.duree_ms) / 60_000);
    }

    /// La transition entre deux parties est le morceau admissible le plus
    /// proche de la fin de la précédente — jamais un tirage au hasard.
    #[test]
    fn le_depart_dune_partie_est_le_voisin_de_la_fin_de_la_precedente() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 12), partie("jazz", 12)];
        let c = composer_complet(&plan).unwrap();
        let fin_rock = c.route[c.parties[0].fin - 1];
        let debut_jazz = c.route[c.parties[1].debut];
        assert!(
            (fin_rock - debut_jazz).abs() <= 2,
            "fin {fin_rock} → début {debut_jazz}"
        );
        assert!(c.parties[1]
            .depart_pourquoi
            .contains("fin de la partie précédente"));
        assert!(c.parties[0].depart_pourquoi.contains("description"));
    }

    #[test]
    fn les_exclusions_globales_valent_pour_chaque_partie() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 9), partie("jazz", 9)];
        plan.filtres.exclure_artistes = vec!["artiste 0".into(), "artiste 1".into()]; // ids 0..=7
        let c = composer_complet(&plan).unwrap();
        assert!(c.route.iter().all(|id| *id >= 8), "{:?}", c.route);
    }

    #[test]
    fn chaque_partie_rend_son_entonnoir_et_ses_admissibles() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 9), partie("jazz", 9)];
        plan.filtres.annee_min = Some(1980);
        let c = composer_complet(&plan).unwrap();
        for p in &c.parties {
            assert_eq!(
                p.entonnoir.first().map(|(l, n)| (l.as_str(), *n)),
                Some(("Bibliothèque", N))
            );
            assert!(
                p.entonnoir.iter().any(|(l, _)| l.starts_with("genre")),
                "{:?}",
                p.entonnoir
            );
            assert!(p.n_admissibles > 0 && p.n_admissibles <= N);
            assert_eq!(p.ids_admissibles.len(), p.n_admissibles);
        }
        // Les deux parties admettent des morceaux différents (rock / jazz).
        assert!(c.parties[0]
            .ids_admissibles
            .iter()
            .all(|id| genre_de(*id) == "rock"));
    }

    /// Une partie dont plus rien ne convient (tout est pris, ou aucun morceau)
    /// n'est pas une erreur : la suivante se compose, le manque est dit.
    #[test]
    fn une_partie_sans_morceau_disponible_est_signalee_pas_fatale() {
        let mut plan = plan_de(10);
        let mut vide = partie("rock", 12);
        vide.genres = vec!["rock".into()];
        plan.parties = vec![partie("jazz", 9), vide];
        plan.filtres.annee_max = Some(1980); // tous de 1990 : le critère s'assouplit
        let c = composer_complet(&plan).unwrap();
        assert!(!c.route.is_empty());
        assert!(
            c.relaches
                .iter()
                .any(|r| r.contains("assoupli") || r.contains("abandonné")),
            "{:?}",
            c.relaches
        );
    }

    #[test]
    fn sans_partie_explicite_la_playlist_est_une_seule_partie_implicite() {
        let mut plan = plan_de(12);
        plan.depart = Some(piste(0));
        let c = composer_complet(&plan).unwrap();
        assert_eq!(c.parties.len(), 1);
        assert_eq!((c.parties[0].debut, c.parties[0].fin), (0, c.route.len()));
        assert_eq!(c.parties[0].libelle, "Playlist");
    }

    /// Les parties sans taille se partagent le nombre de morceaux voulu.
    #[test]
    fn des_parties_sans_taille_se_partagent_n() {
        let mut plan = plan_de(12);
        let mut a = partie("rock", 0);
        let mut b = partie("jazz", 0);
        a.duree_minutes = None;
        b.duree_minutes = None;
        plan.parties = vec![a, b];
        let c = composer_complet(&plan).unwrap();
        for p in &c.parties {
            assert!(
                (p.fin - p.debut) as i64 >= 3 && (p.fin - p.debut) <= 8,
                "{} morceaux",
                p.fin - p.debut
            );
        }
    }

    /// Bout en bout sur la **vraie** bibliothèque (une copie — ouvrir la base
    /// de l'application migrerait son schéma) : vrai Ollama, vrai encodeur
    /// CLAP-texte, vraies empreintes, vrai graphe. Sous-échantillonné pour que
    /// le graphe (n²) reste raisonnable en debug. Ignoré par défaut :
    /// `RUSTY_DB=copie.db RUSTY_SOUS_ECH=6000 OLLAMA_MODELE=gemma4:e4b-mlx \
    ///  cargo test -p rusty-music-desktop composition_sur_la_vraie -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn composition_sur_la_vraie_bibliotheque() {
        use rusty_music_core::ollama;
        let base = std::env::var("RUSTY_DB").expect("RUSTY_DB=chemin/vers/copie.db");
        let m: usize = std::env::var("RUSTY_SOUS_ECH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6000);
        let modele = std::env::var("OLLAMA_MODELE").unwrap_or_else(|_| "gemma4:e4b-mlx".into());
        let lib = rusty_music_core::db::Library::open(Path::new(&base)).expect("base");

        let tout = lib
            .embeddings(rusty_music_analysis::passe::MODELE)
            .expect("empreintes");
        let pas = (tout.len() / m).max(1);
        let vecteurs: Vec<Empreinte> = tout.into_iter().step_by(pas).collect();
        let analyses: HashSet<i64> = vecteurs.iter().map(|(id, _)| *id).collect();
        let mut carac = lib.caracteristiques_pistes().expect("caractéristiques");
        carac.retain(|p| analyses.contains(&p.id));
        let t = Instant::now();
        let graphe = Graphe::construire(
            &vecteurs,
            rusty_music_analysis::chemin::K_VOISINS,
            super::coeurs_arriere_plan(),
        );
        eprintln!(
            "{} morceaux, graphe en {:.1} s",
            vecteurs.len(),
            t.elapsed().as_secs_f64()
        );
        let encodeur = rusty_music_analysis::EmbedderTexte::charger(None).expect("CLAP texte");
        let vocab = lib.vocabulaire_genres(usize::MAX).unwrap();

        for prompt in [
            "Une playlist calme de 60 minutes, sans rock",
            "Une heure de musique énergique pour courir, pas plus de 2 morceaux par artiste",
            "Des morceaux de jazz peu connus, 20 morceaux",
            "Une playlist qui commence doucement et monte vers du rock énergique, 15 morceaux",
        ] {
            let i =
                ollama::interpreter(ollama::HOTE_DEFAUT, &modele, prompt, &vocab).expect("Ollama");
            let plan = PlanTexte {
                depart: None,
                arrivee: None,
                arrivee_demandee: None,
                etapes: i.etapes.clone(),
                n: i.n.map(|n| n as usize).unwrap_or(20),
                duree_minutes: i.duree_minutes,
                plafond_par_artiste: i.plafond_par_artiste,
                filtres: i.filtres(),
                reformulation: i.reformulation.clone(),
                parties: i.parties.clone(),
                brut: String::new(),
            };
            let parties = parties_du_plan(&plan, |e| Ok(encodeur.embed(e).unwrap())).unwrap();
            let t = Instant::now();
            let resultat =
                composer_route(&plan, &parties, &carac, &vecteurs, &graphe, 3, BRUIT_DEFAUT)
                    .map(|c| (c.route, c.relaches));
            eprintln!(
                "\n« {prompt} »\n  filtres : {:?} · durée {:?} · n {}",
                plan.filtres, plan.duree_minutes, plan.n
            );
            match resultat {
                Err(e) => eprintln!("  ERREUR : {e}"),
                Ok((route, relaches)) => {
                    let total: i64 = route
                        .iter()
                        .map(|id| {
                            carac
                                .iter()
                                .find(|p| p.id == *id)
                                .and_then(|p| p.duree_ms)
                                .unwrap_or(0)
                        })
                        .sum();
                    eprintln!(
                        "  {} morceaux, {} min, en {} ms ; assouplis : {relaches:?}",
                        route.len(),
                        total / 60_000,
                        t.elapsed().as_millis()
                    );
                    for id in route.iter().take(12) {
                        let c = carac.iter().find(|p| p.id == *id).unwrap();
                        let t = lib.track(*id).unwrap().unwrap();
                        eprintln!(
                            "    {:<28} {:<26} énergie {:.2} bpm {:>5.0} {:?}",
                            t.artist
                                .unwrap_or_default()
                                .chars()
                                .take(28)
                                .collect::<String>(),
                            t.title
                                .unwrap_or_default()
                                .chars()
                                .take(26)
                                .collect::<String>(),
                            c.energie.unwrap_or(-1.0),
                            c.bpm.unwrap_or(-1.0),
                            c.genres
                        );
                    }
                }
            }
        }
    }

    /// Exporte de **vraies** données pour vérifier l'interface hors de Tauri
    /// (une webview du système ne se pilote pas) : pour chaque prompt, le plan
    /// interprété par Ollama et la composition sur la vraie bibliothèque, tels
    /// que l'interface les reçoit, plus les points de la carte. Ignoré par
    /// défaut : `RUSTY_DB=copie.db RUSTY_EXPORT=dossier cargo test -p
    /// rusty-music-desktop exporter_pour_maquette -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn exporter_pour_maquette() {
        use rusty_music_core::ollama;
        let base = std::env::var("RUSTY_DB").expect("RUSTY_DB");
        let dossier =
            std::path::PathBuf::from(std::env::var("RUSTY_EXPORT").expect("RUSTY_EXPORT"));
        let modele = std::env::var("OLLAMA_MODELE").unwrap_or_else(|_| "gemma4:e4b-mlx".into());
        let lib = rusty_music_core::db::Library::open(Path::new(&base)).expect("base");
        let vecteurs: Vec<Empreinte> = lib
            .embeddings(rusty_music_analysis::passe::MODELE)
            .expect("empreintes");
        let analyses: HashSet<i64> = vecteurs.iter().map(|(id, _)| *id).collect();
        let mut carac = lib.caracteristiques_pistes().expect("caractéristiques");
        carac.retain(|p| analyses.contains(&p.id));
        let graphe = Graphe::construire(
            &vecteurs,
            rusty_music_analysis::chemin::K_VOISINS,
            super::coeurs_arriere_plan(),
        );
        let encodeur = rusty_music_analysis::EmbedderTexte::charger(None).expect("CLAP texte");
        let vocab = lib.vocabulaire_genres(usize::MAX).unwrap();

        let points = lib
            .map_view(rusty_music_analysis::passe::MODELE)
            .expect("map_view");
        std::fs::write(
            dossier.join("points.json"),
            serde_json::to_string(&points).unwrap(),
        )
        .unwrap();
        let familles = lib
            .familles(rusty_music_analysis::passe::MODELE)
            .expect("familles");
        std::fs::write(
            dossier.join("familles.json"),
            serde_json::to_string(&familles).unwrap(),
        )
        .unwrap();

        for (nom, prompt) in [
            (
                "parties",
                "Rock, pendant 12 minutes, puis hip hop pendant 20 minutes",
            ),
            (
                "duree",
                "Une playlist calme de 60 minutes, sans rock, années 70",
            ),
            (
                "trajet",
                "Commencer par du Portishead, finir par du Massive Attack, 15 morceaux",
            ),
        ] {
            let i =
                ollama::interpreter(ollama::HOTE_DEFAUT, &modele, prompt, &vocab).expect("Ollama");
            let plan = PlanTexte {
                depart: None,
                arrivee: None,
                arrivee_demandee: None,
                etapes: i.etapes.clone(),
                n: i.n.map(|n| n as usize).unwrap_or(20),
                duree_minutes: i.duree_minutes,
                plafond_par_artiste: i.plafond_par_artiste,
                filtres: i.filtres(),
                reformulation: i.reformulation.clone(),
                parties: i.parties.clone(),
                brut: i.brut.clone(),
            };
            let parties = parties_du_plan(&plan, |e| Ok(encodeur.embed(e).unwrap())).unwrap();
            let c = composer_route(&plan, &parties, &carac, &vecteurs, &graphe, 3, BRUIT_DEFAUT)
                .expect("composition");
            let pistes: Vec<TrackRow> = c
                .route
                .iter()
                .filter_map(|id| lib.track(*id).unwrap())
                .collect();
            let duree_ms = pistes.iter().map(|t| t.duration_ms.unwrap_or(0)).sum();
            let r = super::CompositionTexte {
                pistes,
                relaches: c.relaches,
                duree_ms,
                parties: c.parties,
            };
            std::fs::write(
                dossier.join(format!("{nom}-plan.json")),
                serde_json::to_string(&plan).unwrap(),
            )
            .unwrap();
            std::fs::write(
                dossier.join(format!("{nom}-compo.json")),
                serde_json::to_string(&r).unwrap(),
            )
            .unwrap();
            eprintln!(
                "{nom} : {} morceaux, {} min",
                r.pistes.len(),
                r.duree_ms / 60_000
            );
        }
    }

    /// Robustesse de la **composition** : relit les interprétations écrites par
    /// l'exemple `robustesse` (`RUSTY_RESULTATS=res.json`) et compose chacune
    /// sur la vraie bibliothèque (une copie), sans Ollama. Vérifie des
    /// invariants, pas la musique : exclusions respectées, durée, plafond par
    /// artiste, bornes d'années, absence de doublon, jamais de panique.
    ///
    /// `RUSTY_DB=copie.db RUSTY_RESULTATS=res.json RUSTY_SOUS_ECH=6000 \
    ///  cargo test -p rusty-music-desktop robustesse_de_la_composition -- --ignored --nocapture`
    ///
    /// `RUSTY_SORTIE=compositions.json` écrit en plus le détail de chaque
    /// playlist (morceaux, genres, années, énergie, assouplissements, nombre de
    /// morceaux admissibles) — voir `experiments/musicrecointent/noter_playlist.py`,
    /// qui la juge contre des annotations.
    #[test]
    #[ignore]
    fn robustesse_de_la_composition() {
        let base = std::env::var("RUSTY_DB").expect("RUSTY_DB");
        let res = std::env::var("RUSTY_RESULTATS").expect("RUSTY_RESULTATS");
        let m: usize = std::env::var("RUSTY_SOUS_ECH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6000);
        let lib = rusty_music_core::db::Library::open(Path::new(&base)).expect("base");
        let tout = lib
            .embeddings(rusty_music_analysis::passe::MODELE)
            .expect("empreintes");
        let pas = (tout.len() / m).max(1);
        let vecteurs: Vec<Empreinte> = tout.into_iter().step_by(pas).collect();
        let analyses: HashSet<i64> = vecteurs.iter().map(|(id, _)| *id).collect();
        let mut carac = lib.caracteristiques_pistes().expect("caractéristiques");
        carac.retain(|p| analyses.contains(&p.id));
        let graphe = Graphe::construire(
            &vecteurs,
            rusty_music_analysis::chemin::K_VOISINS,
            super::coeurs_arriere_plan(),
        );
        let encodeur = rusty_music_analysis::EmbedderTexte::charger(None).expect("CLAP texte");
        let par_id: HashMap<i64, &CaracteristiquesPiste> =
            carac.iter().map(|p| (p.id, p)).collect();
        eprintln!("{} morceaux placables", vecteurs.len());

        let fichier: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(res).expect("résultats")).expect("json");
        let (mut ok, mut total, mut erreurs_moteur, mut paniques) = (0, 0, 0, 0);
        let mut sorties: Vec<serde_json::Value> = Vec::new();
        for r in fichier["resultats"].as_array().expect("resultats") {
            let id = r["id"].as_str().unwrap_or("?");
            let Some(spec) = r.get("spec") else { continue }; // interprétation échouée : rien à composer
            let i: rusty_music_core::ollama::InterpretationLlm =
                serde_json::from_value(spec.clone()).expect("spec");
            let plan = PlanTexte {
                depart: None,
                arrivee: None,
                arrivee_demandee: None,
                etapes: i.etapes.clone(),
                n: i.n.map(|n| n as usize).unwrap_or(20),
                duree_minutes: i.duree_minutes,
                plafond_par_artiste: i.plafond_par_artiste,
                filtres: i.filtres(),
                reformulation: i.reformulation.clone(),
                parties: i.parties.clone(),
                brut: String::new(),
            };
            total += 1;
            let parties = parties_du_plan(&plan, |e| Ok(encodeur.embed(e).unwrap())).unwrap();
            let t = Instant::now();
            let sortie = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                composer_route(&plan, &parties, &carac, &vecteurs, &graphe, 3, BRUIT_DEFAUT)
                    .map(|c| (c.route, c.relaches, c.parties))
            }));
            let ms = t.elapsed().as_millis();
            let (route, relaches, parties_res) = match sortie {
                Err(_) => {
                    paniques += 1;
                    eprintln!("✗ {id:<22} PANIQUE");
                    sorties.push(serde_json::json!({"id": id, "panique": true}));
                    continue;
                }
                Ok(Err(e)) => {
                    erreurs_moteur += 1;
                    eprintln!("· {id:<22} erreur moteur : {e}");
                    sorties.push(serde_json::json!({"id": id, "erreur": e.to_string()}));
                    continue;
                }
                Ok(Ok(v)) => v,
            };
            sorties.push(serde_json::json!({
                "id": id,
                "n_voulu": plan.n,
                "route": route.iter().filter_map(|i| par_id.get(i)).map(|p| serde_json::json!({
                    "id": p.id, "artiste": p.artiste, "annee": p.annee, "energie": p.energie,
                    "bpm": p.bpm, "genres": p.genres,
                })).collect::<Vec<_>>(),
                "relaches": relaches,
                "admissibles": parties_res.iter().map(|p| p.n_admissibles).collect::<Vec<_>>(),
            }));

            let mut pb: Vec<String> = Vec::new();
            let artiste_de = |id: &i64| -> String {
                par_id
                    .get(id)
                    .and_then(|p| p.artiste.clone())
                    .unwrap_or_default()
                    .to_lowercase()
            };
            // Exclusions dures : aucun genre ni artiste ne contient le mot (mot entier).
            let mot_dans = |texte: &str, mot: &str| -> bool {
                texte
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|m| m == mot)
                    || (mot.contains(' ') && texte.contains(mot))
            };
            if let Some(mots) = r["exclusions_dures"].as_array() {
                for mot in mots.iter().filter_map(|m| m.as_str()) {
                    let n_genre = route
                        .iter()
                        .filter(|id| {
                            par_id
                                .get(id)
                                .is_some_and(|p| p.genres.iter().any(|g| mot_dans(g, mot)))
                        })
                        .count();
                    let n_art = route
                        .iter()
                        .filter(|id| artiste_de(id).contains(mot))
                        .count();
                    if n_genre + n_art > 0 {
                        pb.push(format!(
                            "« {mot} » : {n_genre} morceau(x) de ce genre, {n_art} de cet artiste"
                        ));
                    }
                }
            }
            let mut vus = HashSet::new();
            if route.iter().any(|id| !vus.insert(*id)) {
                pb.push("doublon".into());
            }
            let duree: i64 = route
                .iter()
                .map(|id| par_id.get(id).and_then(|p| p.duree_ms).unwrap_or(0))
                .sum();
            if let Some(d) = plan.duree_minutes {
                let cible = i64::from(d) * 60_000;
                let ecart = (duree - cible).abs() as f64 / cible as f64;
                let dit = relaches.iter().any(|r| r.contains("durée visée"));
                if ecart > 0.12 && !dit {
                    pb.push(format!(
                        "durée {} min pour {d} visées, sans le dire",
                        duree / 60_000
                    ));
                }
            }
            if let Some(p) = plan.plafond_par_artiste {
                let mut compte: HashMap<String, usize> = HashMap::new();
                for id in &route {
                    *compte.entry(artiste_de(id)).or_default() += 1;
                }
                if let Some((a, n)) = compte.iter().find(|(_, n)| **n > p as usize) {
                    pb.push(format!("plafond {p} dépassé : {a} ×{n}"));
                }
            }
            if let Some(min) = plan.filtres.annee_min {
                let hors = route
                    .iter()
                    .filter(|id| {
                        par_id
                            .get(id)
                            .and_then(|p| p.annee)
                            .is_some_and(|a| a < min)
                    })
                    .count();
                if hors > 0 && !relaches.iter().any(|r| r.contains("période")) {
                    pb.push(format!("{hors} morceau(x) avant {min}, sans le dire"));
                }
            }
            if let Some(max) = plan.filtres.annee_max {
                let hors = route
                    .iter()
                    .filter(|id| {
                        par_id
                            .get(id)
                            .and_then(|p| p.annee)
                            .is_some_and(|a| a > max)
                    })
                    .count();
                if hors > 0 && !relaches.iter().any(|r| r.contains("période")) {
                    pb.push(format!("{hors} morceau(x) après {max}, sans le dire"));
                }
            }
            if route.len() < 5 {
                pb.push(format!("seulement {} morceau(x)", route.len()));
            }
            // Parties : se suivent sans trou, couvrent toute la route, chacune
            // dans son genre quand elle en a un, et près de sa durée.
            let mut attendu_debut = 0;
            for (k, pr) in parties_res.iter().enumerate() {
                if pr.debut != attendu_debut {
                    pb.push(format!(
                        "partie {k} : début {} au lieu de {attendu_debut}",
                        pr.debut
                    ));
                }
                attendu_debut = pr.fin;
                if let Some(partie) = plan.parties.get(k) {
                    let dit = pr.relaches.iter().any(|r| {
                        r.contains("abandonn")
                            || r.contains("déjà pris")
                            || r.contains("durée visée")
                    });
                    if !partie.genres.is_empty() && !dit {
                        let hors = route[pr.debut..pr.fin]
                            .iter()
                            .filter(|id| {
                                // Départ nommé exclu : seuls comptent les morceaux choisis.
                                !par_id.get(id).is_some_and(|p| {
                                    partie.genres.iter().any(|g| {
                                        p.genres.iter().any(|pg| {
                                            pg.to_lowercase()
                                                .split(|c: char| !c.is_alphanumeric())
                                                .any(|m| {
                                                    g.to_lowercase()
                                                        .split(|c: char| !c.is_alphanumeric())
                                                        .any(|x| x == m)
                                                })
                                        })
                                    })
                                })
                            })
                            .count();
                        if hors > 0 {
                            pb.push(format!("partie {k} ({:?}) : {hors} morceau(x) d'un autre genre, sans le dire", partie.genres));
                        }
                    }
                    if let Some(m) = partie.duree_minutes {
                        let cible = i64::from(m) * 60_000;
                        let ecart = (pr.duree_ms - cible).abs() as f64 / cible as f64;
                        if ecart > 0.25 && !dit {
                            pb.push(format!(
                                "partie {k} : {} min pour {m} visées, sans le dire",
                                pr.duree_ms / 60_000
                            ));
                        }
                    }
                }
            }
            if attendu_debut != route.len() {
                pb.push(format!(
                    "les parties couvrent {attendu_debut} morceaux sur {}",
                    route.len()
                ));
            }
            if pb.is_empty() {
                ok += 1;
                eprintln!(
                    "✓ {id:<22} {:>3} morceaux, {:>3} min, {ms} ms{}",
                    route.len(),
                    duree / 60_000,
                    if relaches.is_empty() {
                        String::new()
                    } else {
                        format!(" — assouplis : {}", relaches.len())
                    }
                );
                if parties_res.len() > 1 || !relaches.is_empty() {
                    for pr in &parties_res {
                        eprintln!(
                            "      · {:<18} {:>2} morceaux, {:>3} min (visé {}), {} admissibles, départ : {}",
                            pr.libelle, pr.fin - pr.debut, pr.duree_ms / 60_000,
                            pr.cible_ms.map_or("—".into(), |c| format!("{} min", c / 60_000)),
                            pr.n_admissibles, pr.depart_pourquoi
                        );
                        for r in &pr.relaches {
                            eprintln!("          ↳ {r}");
                        }
                    }
                }
            } else {
                eprintln!(
                    "✗ {id:<22} {:>3} morceaux, {:>3} min",
                    route.len(),
                    duree / 60_000
                );
                for p in pb {
                    eprintln!("      {p}");
                }
            }
        }
        eprintln!("\n== composition : {ok}/{total} sans problème, {erreurs_moteur} refus clairs du moteur, {paniques} paniques");
        if let Ok(chemin) = std::env::var("RUSTY_SORTIE") {
            let artistes: std::collections::BTreeSet<&str> =
                carac.iter().filter_map(|p| p.artiste.as_deref()).collect();
            let sortie = serde_json::json!({
                "compositions": sorties,
                "vocabulaire": lib.vocabulaire_genres(usize::MAX).unwrap_or_default(),
                "artistes": artistes,
                "morceaux_placables": carac.len(),
            });
            std::fs::write(chemin, serde_json::to_string(&sortie).unwrap())
                .expect("écriture de RUSTY_SORTIE");
        }
    }
}
