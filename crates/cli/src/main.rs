// SPDX-License-Identifier: GPL-3.0-or-later
//! Pilote en ligne de commande du cœur d'ingestion.
//!
//! Sert à valider le cœur avant que l'interface existe :
//!   rusty-music scan  ~/Musique
//!   rusty-music watch ~/Musique
//!   rusty-music stats

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{Parser, Subcommand};

use rusty_music_core::db::TrackRow;
use rusty_music_core::{scan, watch, Library};

#[derive(Parser)]
#[command(
    name = "rusty-music",
    about = "Cœur d'ingestion de la bibliothèque musicale"
)]
struct Cli {
    /// Emplacement de la base. Par défaut : ./rusty-music.db
    #[arg(long, global = true, default_value = "rusty-music.db")]
    db: PathBuf,

    /// Threads de lecture des tags. Par défaut : nombre de cœurs.
    #[arg(long, short = 'j', global = true)]
    jobs: Option<usize>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parcourt un répertoire et ingère les fichiers musicaux
    Scan {
        root: PathBuf,
        /// Relit les tags de tous les fichiers, même inchangés. Nécessaire
        /// après un enrichissement de ce qu'on extrait des tags.
        #[arg(long)]
        force: bool,
    },
    /// Scanne puis surveille le répertoire en continu (Ctrl-C pour arrêter)
    Watch { root: PathBuf },
    /// Affiche l'état de la bibliothèque
    Stats,
    /// Liste les artistes
    Artists,
    /// Liste les albums, tous ou ceux d'un artiste
    Albums {
        #[arg(long)]
        artist: Option<String>,
    },
    /// Liste les pistes d'un album, dans l'ordre du disque
    Tracks {
        album: String,
        #[arg(long)]
        artist: Option<String>,
    },
    /// Cherche dans les titres, artistes et albums
    Search {
        query: String,
        #[arg(long, default_value_t = 30)]
        limit: i64,
    },
    /// Liste les dossiers surveillés (réglage « source de la bibliothèque »)
    Roots,
    /// Oublie un dossier surveillé et les morceaux qui en dépendent
    Forget { root: PathBuf },
    /// Extrait la pochette du premier résultat d'une recherche
    Cover {
        query: String,
        /// Écrit l'image dans ce fichier au lieu d'en résumer le contenu
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Calcule les empreintes des morceaux en attente (reprenable)
    Analyze {
        /// Nombre de morceaux à traiter
        #[arg(long, default_value_t = 1000)]
        limit: i64,
        /// Poids du modèle. Par défaut : ceux produits par ce build — les
        /// seuls qui aillent avec son code généré.
        #[arg(long)]
        model: Option<PathBuf>,
        /// Enchaîne la projection à la fin
        #[arg(long)]
        project: bool,
        /// Nombre de familles (k-means), si projection
        #[arg(long, default_value_t = 12)]
        familles: usize,
    },
    /// Place toutes les empreintes sur la carte (rapide, à rejouer à volonté)
    Project {
        #[arg(long, default_value_t = 12)]
        familles: usize,
    },
    /// Étire ou transpose un fichier, et écrit le résultat
    ///
    /// Sert à juger à l'oreille ce qu'aucune mesure ne tranche : un vocodeur de
    /// phase s'entend, il ne se calcule pas.
    Etirer {
        /// Fichier d'entrée (un stem, ou n'importe quel morceau)
        entree: PathBuf,
        /// Fichier WAV de sortie
        sortie: PathBuf,
        /// Facteur de durée : 1,25 pour un quart plus long
        #[arg(long, default_value_t = 1.0)]
        facteur: f32,
        /// Transposition en demi-tons, à durée constante
        #[arg(long, default_value_t = 0.0)]
        demi_tons: f32,
    },
    /// Met à la place d'un stem celui d'un autre morceau
    ///
    /// Cale le greffon sur le tempo du morceau, le fait entrer là où le stem
    /// remplacé entrait, et le répète ou le coupe pour tenir la place. Les
    /// temps forts ne sont pas alignés : cela demanderait une grille de
    /// battements, hors périmètre du module 3.
    Greffer {
        /// Le stem remplacé — il donne la durée à tenir et l'instant d'entrée
        remplace: PathBuf,
        /// Le stem à greffer, tiré d'un autre morceau
        greffon: PathBuf,
        /// Fichier WAV de sortie
        sortie: PathBuf,
        /// Tempo du morceau ouvert. Sans les deux tempos, rien n'est étiré.
        #[arg(long)]
        bpm_source: Option<f32>,
        /// Tempo du morceau d'où vient le greffon
        #[arg(long)]
        bpm_greffon: Option<f32>,
        /// Ne pas caler sur les temps forts : le greffon entre à la première
        /// attaque, comme avant que la grille de battements existe
        #[arg(long)]
        sans_grille: bool,
    },
    /// Montre la grille de battements d'un fichier : tempo, phase, netteté
    ///
    /// Les candidats suivants disent ce que le premier ne dit pas — sur une
    /// batterie, le contretemps répond souvent presque aussi fort que le temps.
    Battements {
        /// Le fichier à examiner
        fichier: PathBuf,
        /// Combien de couples (tempo, phase) montrer
        #[arg(long, default_value_t = 4)]
        candidats: usize,
        /// Éprouver cette grille-ci au lieu d'en chercher une : le fichier
        /// pulse-t-il au tempo et à la phase qu'on lui impose ?
        #[arg(long, requires = "phase")]
        bpm: Option<f32>,
        /// Phase imposée, en secondes
        #[arg(long, requires = "bpm")]
        phase: Option<f32>,
    },
    /// Mesure tempo, tonalité et énergie des morceaux placés sur la carte
    ///
    /// Décode les mêmes cinq fenêtres que l'analyse. Reprenable : relancer ne
    /// remesure rien.
    Descripteurs {
        /// Nombre de morceaux à traiter au plus (0 = tous)
        #[arg(long, default_value_t = 0)]
        limite: i64,
        /// Fils de décodage (0 = tous les cœurs)
        ///
        /// À réduire pour continuer d'écouter pendant la passe : douze fils
        /// saturent une carte SD, et l'application attend alors le disque.
        #[arg(long, default_value_t = 0)]
        fils: usize,
    },
    /// Trace un itinéraire dans le réseau de circulation
    Itineraire {
        /// Morceau de départ (identifiant)
        depart: i64,
        /// Morceau d'arrivée (identifiant). Facultatif avec `--minutes`.
        #[arg(long)]
        arrivee: Option<i64>,
        /// autoroute | sentier | panoramique
        #[arg(long, default_value = "autoroute")]
        profil: String,
        /// Durée cible de la playlist, en minutes
        #[arg(long)]
        minutes: Option<u64>,
        /// Étapes imposées, dans l'ordre
        #[arg(long, value_delimiter = ',')]
        etapes: Vec<i64>,
        /// Éviter les morceaux les plus connus
        #[arg(long)]
        eviter_autoroutes: bool,
        /// Nombre d'itinéraires proposés (1 à 3)
        #[arg(long, default_value_t = 1)]
        alternatives: usize,
        /// Voisins par morceau
        #[arg(long, default_value_t = 12)]
        k: usize,
    },
    /// Produit l'archive de tuiles vectorielles lue par MapLibre
    Tuiles {
        /// Où écrire l'archive
        #[arg(long, default_value = "carte.pmtiles")]
        sortie: PathBuf,
        /// Zoom maximal produit. Au-delà, MapLibre sur-zoome la dernière tuile.
        #[arg(long, default_value_t = 9)]
        zoom_max: u8,
        /// Dépose aussi les tuiles en clair (`z/x/y`) et une page d'essai, pour
        /// ouvrir la carte dans un navigateur ordinaire.
        #[arg(long)]
        repertoire: Option<PathBuf>,
    },
    /// Affiche les familles de la carte et leurs noms
    ///
    /// Les noms viennent de trois sources, de la plus précise à la plus
    /// grossière : l'album MusicBrainz, l'artiste MusicBrainz, le tag du
    /// fichier. Sert à voir l'effet d'une passe `enrich`.
    Familles {
        /// Montre aussi les artistes dominants de chaque famille
        #[arg(long)]
        artistes: bool,
    },
    /// Étage 1 de l'affectation : répartit les familles musicales sur les
    /// quartiers d'un plan de ville déjà importé (`ville`)
    Quartiers {
        /// La base de ville à lire (voir `ville --sortie`)
        #[arg(long, default_value = "ville-paris.db")]
        ville: PathBuf,
    },
    /// Étage 2 de l'affectation : loge chaque artiste sur une ou plusieurs
    /// rues de la zone de sa famille (rejoue l'étage 1 au passage)
    Rues {
        /// La base de ville à lire (voir `ville --sortie`)
        #[arg(long, default_value = "ville-paris.db")]
        ville: PathBuf,
        /// Distance entre deux adresses le long d'une rue, en mètres
        #[arg(long, default_value_t = 4.0)]
        espacement: f64,
    },
    /// Étage 3 de l'affectation : sème chaque morceau à une adresse le long
    /// de la rue de son artiste (rejoue les étages 1 et 2 au passage), puis
    /// mesure la préservation du voisinage musical → géographique
    Adresses {
        /// La base de ville à lire (voir `ville --sortie`)
        #[arg(long, default_value = "ville-paris.db")]
        ville: PathBuf,
        /// Distance entre deux adresses le long d'une rue, en mètres
        #[arg(long, default_value_t = 4.0)]
        espacement: f64,
        /// Nombre de morceaux échantillonnés pour la mesure de voisinage
        #[arg(long, default_value_t = 500)]
        echantillon: usize,
    },
    /// Importe un plan de ville depuis un extrait OpenStreetMap
    ///
    /// La carte n'invente plus son monde : elle emprunte celui d'une vraie
    /// ville. Prenez l'extrait de la région chez Geofabrik — pour Paris,
    /// `europe/france/ile-de-france-latest.osm.pbf` — puis découpez-le sur la
    /// limite communale. L'archive `.osm.pbf` peut être supprimée après.
    ///
    /// Les données OSM sont sous ODbL : leur affichage impose de citer
    /// « © les contributeurs OpenStreetMap ».
    Ville {
        /// L'extrait `.osm.pbf` à lire
        pbf: PathBuf,
        /// La commune à découper, telle qu'OSM la nomme (`admin_level=8`)
        #[arg(long, default_value = "Paris")]
        commune: String,
        /// Où écrire la base de la ville
        #[arg(long)]
        sortie: Option<PathBuf>,
    },
    /// Aspire les genres MusicBrainz des artistes et de leurs albums
    ///
    /// Deux requêtes par artiste, une par seconde — c'est la limite que
    /// MusicBrainz impose, pas un choix. Comptez une heure pour vingt-sept
    /// mille morceaux. La passe reprend où elle s'est arrêtée : la relancer
    /// après une coupure ne refait rien.
    Enrich {
        /// Nombre d'artistes à traiter au plus (0 = tous)
        #[arg(long, default_value_t = 0)]
        limite: usize,
        /// Adresse de contact envoyée à MusicBrainz, comme leur usage l'exige
        ///
        /// À défaut, la variable d'environnement `RUSTY_MUSIC_CONTACT`.
        #[arg(long)]
        contact: Option<String>,
    },
    /// Actualise le fil du mode Découvrir : nouveaux disques, collaborations,
    /// artistes voisins
    ///
    /// Interroge MusicBrainz (sorties par artiste) et ListenBrainz (artistes
    /// similaires, sorties récentes). Additive et reprenable, comme `enrich`.
    Decouvrir {
        /// Adresse de contact envoyée aux API (ou `RUSTY_MUSIC_CONTACT`)
        #[arg(long)]
        contact: Option<String>,
        /// Âge maximal d'une sortie pour figurer au fil, en jours
        #[arg(long, default_value_t = 30)]
        jours: i64,
        /// Artistes interrogés pour leurs voisins, au plus (0 = défaut)
        #[arg(long, default_value_t = 0)]
        limite: usize,
    },
    /// Récupère la popularité générale des morceaux (ListenBrainz + Deezer)
    ///
    /// Deux API publiques, sans clé ni compte. Additive et reprenable, comme
    /// `enrich` : l'interrompre ne perd rien, la relancer ne refait rien.
    Popularite {
        /// Contact pour le User-Agent ListenBrainz — courtoisie, pas exigé
        /// (ou `RUSTY_MUSIC_CONTACT`)
        #[arg(long)]
        contact: Option<String>,
        /// Entités à traiter au plus, par échelon (0 = toutes)
        #[arg(long, default_value_t = 0)]
        limite: usize,
        /// Réinterroger ce qui date de plus de N jours (0 = ne rafraîchit rien)
        #[arg(long, default_value_t = 0)]
        rafraichir_des: i64,
    },
    /// Sondage de qualité : les k plus proches voisins d'un morceau
    ///
    /// Dans l'espace des empreintes CLAP — pour juger à l'oreille (ou au moins
    /// au nom) si le voisinage veut dire quelque chose.
    Voisins {
        /// Morceau de départ (texte cherché, comme `search`)
        recherche: String,
        /// Nombre de voisins
        #[arg(long, default_value_t = 12)]
        k: usize,
    },
    /// Trace un chemin entre deux morceaux, dans les quatre modes calculés
    ///
    /// Sert surtout à mesurer : le graphe des voisins se construit ici sans
    /// interface, et son coût décide si l'application peut le bâtir à la
    /// demande ou doit le préparer au démarrage.
    Path {
        /// Morceau de départ (texte cherché)
        from: String,
        /// Morceau d'arrivée (texte cherché) ; absent, seule l'errance tourne
        to: Option<String>,
        /// Nombre de morceaux visés
        #[arg(long, default_value_t = 12)]
        steps: usize,
        /// Graine du bruit (les quatre modes calculés en tirent parti)
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Bruit commun aux quatre modes, de 0 (trajet exact) à 1 (dérive
        /// maximale) — softmax pour l'errance, arêtes bruitées pour le
        /// sonique, pont brownien pour le direct (voir la note en tête de
        /// `chemin.rs`)
        #[arg(long, default_value_t = 0.3)]
        bruit: f32,
    },
    /// Sépare un morceau en stems (module 3)
    ///
    /// Écrit quatre WAV — batterie, basse, autre, voix — et contrôle que leur
    /// somme reconstitue le mélange.
    Demix {
        /// Texte cherché (titre/artiste/album), ou chemin de fichier
        query: String,
        /// Où écrire les stems
        #[arg(long, default_value = "./stems/")]
        out: PathBuf,
        /// Variante : htdemucs (défaut) · htdemucs_6s · htdemucs_ft
        #[arg(long, default_value = "htdemucs")]
        modele: String,
        /// Dossier des poids. Par défaut : cherché près de l'exécutable,
        /// puis dans `models/`.
        #[arg(long)]
        models_dir: Option<PathBuf>,
        /// Ne traite que les N premières secondes (0 = tout le morceau)
        #[arg(long, default_value_t = 0)]
        seconds: u64,
    },
    /// Joue le premier résultat d'une recherche, ou un album entier
    Play {
        /// Texte cherché (titre/artiste/album), ou nom d'album avec --album
        query: String,
        /// Joue l'album entier au lieu d'une seule piste
        #[arg(long)]
        album: bool,
        #[arg(long)]
        artist: Option<String>,
        /// S'arrête après N secondes (0 = jouer jusqu'au bout)
        #[arg(long, default_value_t = 0)]
        seconds: u64,
    },
}

/// Durée longue, en h/min — pour l'avancement d'une passe qui dure.
fn duree_h(s: f64) -> String {
    if s < 90.0 {
        format!("{s:.0} s")
    } else if s < 5400.0 {
        format!("{:.0} min", s / 60.0)
    } else {
        format!("{:.1} h", s / 3600.0)
    }
}

/// Durée en `m:ss`, pour que les listes restent lisibles.
fn duree(ms: Option<i64>) -> String {
    match ms {
        Some(ms) if ms > 0 => format!("{}:{:02}", ms / 60000, (ms / 1000) % 60),
        _ => "—".into(),
    }
}

/// Les grilles de battements des deux stems d'une greffe.
///
/// **C'est ici qu'on relie le module 2 au module 3**, et pas dans
/// `crates/editor` : y faire dépendre l'éditeur de `rusty-music-analysis`
/// tirerait CLAP, ses 117 Mo de poids et la génération de code de son
/// `build.rs` dans un crate qui n'a que faire d'un modèle d'empreintes. La
/// grille voyage donc en trois nombres.
///
/// `None` si l'un des deux ne pulse pas — la greffe retombe alors sur la
/// première attaque, ce qu'elle faisait avant que la grille existe.
fn grilles_de(
    remplace: &std::path::Path,
    greffon: &std::path::Path,
) -> Result<Option<(rusty_music_analysis::battements::Grille, rusty_music_analysis::battements::Grille)>>
{
    use rusty_music_analysis::battements;

    let analyseur = rusty_music_analysis::descripteurs::Analyseur::new();
    let une = |chemin: &std::path::Path| -> Result<Option<battements::Grille>> {
        let s = rusty_music_editor::decode::stereo(chemin)?;
        // La grille se lit sur la somme des deux voies : une batterie panoramée
        // à gauche ne doit pas donner une phase différente d'une centrée.
        let mono: Vec<f32> = s
            .gauche
            .iter()
            .zip(&s.droite)
            .map(|(g, d)| (g + d) * 0.5)
            .collect();
        Ok(battements::grille_reechantillonnee(
            &mono,
            rusty_music_editor::SR,
            &analyseur,
        ))
    };
    Ok(match (une(remplace)?, une(greffon)?) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    })
}

/// Ce que vaut une grille imposée sur un fichier, et ce que vaudrait la
/// meilleure. Les deux ensemble : le premier chiffre seul ne dit pas s'il est
/// bon.
fn eprouver(chemin: &std::path::Path, bpm: f32, phase_s: f32) -> Result<(Option<f32>, Option<f32>)> {
    use rusty_music_analysis::battements;

    let s = rusty_music_editor::decode::stereo(chemin)?;
    let mono: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .map(|(g, d)| (g + d) * 0.5)
        .collect();
    let a = rusty_music_analysis::descripteurs::Analyseur::new();
    let cible = rusty_music_analysis::mel::SR;
    let rapport = rusty_music_editor::SR as f64 / cible as f64;
    let n = (mono.len() as f64 / rapport) as usize;
    let a48: Vec<f32> = (0..n)
        .map(|i| {
            let x = i as f64 * rapport;
            let (j, t) = (x.floor() as usize, x.fract() as f32);
            let u = mono[j.min(mono.len() - 1)];
            let v = mono[(j + 1).min(mono.len() - 1)];
            u * (1.0 - t) + v * t
        })
        .collect();
    Ok((
        battements::evaluer(&a48, &a, bpm, phase_s),
        battements::grille(&a48, &a).map(|g| g.nettete),
    ))
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            // `lofty` avertit sur chaque MPEG dont il estime la durée, et
            // `symphonia` sur chaque trame ID3 inconnue : à 27 000 fichiers,
            // ces deux-là seuls remplissent l'écran. Leurs erreurs passent.
            // Directives ajoutées par-dessus RUST_LOG, pour que le régler sur
            // `info` ne ramène pas ce bruit.
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
                .add_directive("lofty=error".parse().expect("directive valide"))
                .add_directive("symphonia=warn".parse().expect("directive valide"))
                // Le décodeur MP3 crie `invalid main_data_begin, underflow`
                // sur la première trame après chaque `seek` : le réservoir de
                // bits repart vide et pointe en arrière dans des octets non
                // décodés. `decode::par_position` se positionne sur chaque
                // fenêtre, donc ~4 fois par fichier — inévitable et sans effet,
                // le décodeur récupère en une trame (~26 ms sur 10 s analysés).
                .add_directive(
                    "symphonia_bundle_mp3::layer3=error"
                        .parse()
                        .expect("directive valide"),
                )
                // ONNX Runtime commente chacune de ses allocations.
                .add_directive("ort=warn".parse().expect("directive valide")),
        )
        .without_time()
        .init();

    let cli = Cli::parse();
    let lib = Library::open(&cli.db)?;
    let jobs = cli.jobs.unwrap_or_else(scan::default_jobs);

    match cli.cmd {
        Cmd::Scan { root, force } => {
            let rep = scan::scan_root_jobs(&lib, &root, jobs, force)?;
            println!(
                "{} fichiers vus · {} ingérés · {} inchangés · {} retirés · {} en échec",
                rep.seen, rep.inserted, rep.skipped, rep.removed, rep.failed
            );
        }
        Cmd::Watch { root } => {
            let rep = scan::scan_root_jobs(&lib, &root, jobs, false)?;
            println!(
                "scan initial : {} ingérés, {} inchangés, {} retirés",
                rep.inserted, rep.skipped, rep.removed
            );
            println!("surveillance de {} — Ctrl-C pour arrêter", root.display());
            watch::watch_root(&lib, &root)?;
        }
        Cmd::Stats => {
            let total = lib.count()?;
            let pending = lib
                .pending_analysis(rusty_music_analysis::passe::MODELE, i64::MAX)?
                .len();
            println!("{total} morceaux en base · {pending} en attente d'analyse");
        }
        Cmd::Artists => {
            let artistes = lib.artists()?;
            for a in &artistes {
                println!(
                    "{:>5} morceaux  {:>3} albums  {}",
                    a.tracks, a.albums, a.name
                );
            }
            println!("— {} artistes", artistes.len());
        }
        Cmd::Albums { artist } => {
            // Emprunter le même chemin que l'interface quand elle ouvre un
            // artiste : on résout son identifiant, sinon la CLI validerait une
            // requête que l'application n'exécute jamais.
            let albums = match artist.as_deref() {
                Some(nom) => {
                    let mbid = lib
                        .artists()?
                        .into_iter()
                        .find(|a| a.name == nom)
                        .and_then(|a| a.mbid);
                    lib.albums_of_artist(mbid.as_deref(), nom)?
                }
                None => lib.albums(None)?,
            };
            for a in &albums {
                println!(
                    "{:>3} pistes  {}  {} — {}",
                    a.tracks,
                    a.year.map_or("————".into(), |y| y.to_string()),
                    a.artist.as_deref().unwrap_or("(sans artiste)"),
                    a.name
                );
            }
            println!("— {} albums", albums.len());
        }
        Cmd::Tracks { album, artist } => {
            for t in lib.tracks_of_album(&album, artist.as_deref())? {
                println!(
                    "{:>3}. {:<45} {:>6}  {}",
                    t.track_no.unwrap_or(0),
                    t.title.as_deref().unwrap_or("(sans titre)"),
                    duree(t.duration_ms),
                    t.artist.as_deref().unwrap_or("")
                );
            }
        }
        Cmd::Search { query, limit } => {
            let trouves = lib.search(&query, limit)?;
            for t in &trouves {
                println!(
                    "{:<40} {:<25} {:<30} {:>6}",
                    t.title.as_deref().unwrap_or("(sans titre)"),
                    t.artist.as_deref().unwrap_or(""),
                    t.album.as_deref().unwrap_or(""),
                    duree(t.duration_ms)
                );
            }
            println!("— {} résultats (limite {limit})", trouves.len());
        }
        Cmd::Roots => {
            for r in lib.roots()? {
                let scan = match r.last_scan {
                    Some(_) => "scanné",
                    None => "jamais scanné",
                };
                println!("{:>7} morceaux  {}  {}", r.tracks, scan, r.path);
            }
        }
        Cmd::Forget { root } => {
            let n = lib.remove_root(&root)?;
            println!("{} : racine oubliée, {n} morceaux retirés", root.display());
        }
        Cmd::Cover { query, out } => {
            let trouves = lib.search(&query, 1)?;
            let Some(piste) = trouves.first() else {
                anyhow::bail!("rien ne correspond à « {query} »");
            };
            let debut = Instant::now();
            match rusty_music_core::tags::read_cover(Path::new(&piste.path))? {
                Some(cover) => {
                    let origine = match cover.source {
                        rusty_music_core::CoverSource::Embedded => "embarquée",
                        rusty_music_core::CoverSource::Folder => "fichier du dossier",
                    };
                    println!(
                        "{} — {} : {} Ko, {}, {} (en {} ms)",
                        piste.artist.as_deref().unwrap_or("?"),
                        piste.title.as_deref().unwrap_or("?"),
                        cover.data.len() / 1024,
                        cover.mime.as_deref().unwrap_or("type inconnu"),
                        origine,
                        debut.elapsed().as_millis()
                    );
                    if let Some(chemin) = out {
                        std::fs::write(&chemin, &cover.data)?;
                        println!("écrite dans {}", chemin.display());
                    }
                }
                None => println!("aucune pochette pour ce morceau"),
            }
        }
        Cmd::Analyze {
            limit,
            model,
            project,
            familles,
        } => {
            let t = Instant::now();
            // La passe dure des heures sur un support lent : on montre où on
            // en est, et une estimation du temps restant.
            let r = rusty_music_analysis::passe::empreintes(
                &lib,
                model.as_deref(),
                limit,
                jobs,
                |fait, total| {
                    let par = t.elapsed().as_secs_f64() / fait as f64;
                    eprint!(
                        "\r  {fait}/{total} — {:.2} s/morceau, reste {}   ",
                        par,
                        duree_h(par * (total - fait) as f64)
                    );
                },
            )?;
            eprintln!();
            println!(
                "{} demandés · {} empreintes · {} en échec — {}",
                r.demandes,
                r.empreintes,
                r.echecs,
                duree_h(t.elapsed().as_secs_f64())
            );
            if project {
                let t = Instant::now();
                let p = rusty_music_analysis::passe::projeter_tout(&lib, Some(familles))?;
                println!(
                    "carte : {} points, {} familles — {:.1} s",
                    p.empreintes,
                    p.familles,
                    t.elapsed().as_secs_f64()
                );
            } else if r.empreintes > 0 {
                println!("empreintes écrites — lancer `project` pour la carte");
            }
        }
        Cmd::Etirer {
            entree,
            sortie,
            facteur,
            demi_tons,
        } => {
            use rusty_music_editor::{decode, etirement, wav};
            let t = Instant::now();
            let s = decode::stereo(&entree)?;
            let entrelace: Vec<f32> = s
                .gauche
                .iter()
                .zip(&s.droite)
                .flat_map(|(g, d)| [*g, *d])
                .collect();

            // Étirement puis transposition : chacun peut être neutre, les
            // enchaîner permet de faire les deux d'un coup.
            let mut out = etirement::etirer(&entrelace, 2, facteur);
            if demi_tons.abs() > 1e-6 {
                out = etirement::transposer(&out, 2, demi_tons);
            }
            let (g, d): (Vec<f32>, Vec<f32>) = out.chunks_exact(2).map(|c| (c[0], c[1])).unzip();
            wav::ecrire(&sortie, &g, &d, 44_100)?;
            println!(
                "{:.0} s → {:.0} s (×{facteur}, {demi_tons:+} demi-tons) — {:.1} s de calcul\n{}",
                s.duree(),
                g.len() as f64 / 44_100.0,
                t.elapsed().as_secs_f64(),
                sortie.display()
            );
        }
        Cmd::Battements {
            fichier,
            candidats,
            bpm,
            phase,
        } => {
            if let (Some(bpm), Some(phase)) = (bpm, phase) {
                let (impose, meilleur) = eprouver(&fichier, bpm, phase)?;
                match (impose, meilleur) {
                    (Some(i), Some(m)) => println!(
                        "grille imposée {bpm:.1} BPM à {phase:.3} s : {i:.2}\n\
                         meilleure grille trouvée              : {m:.2}\n\
                         phase quelconque                      : 1.00"
                    ),
                    _ => println!("ce fichier ne pulse pas assez pour trancher"),
                }
                return Ok(());
            }
            let s = rusty_music_editor::decode::stereo(&fichier)?;
            let mono: Vec<f32> = s
                .gauche
                .iter()
                .zip(&s.droite)
                .map(|(g, d)| (g + d) * 0.5)
                .collect();
            let a = rusty_music_analysis::descripteurs::Analyseur::new();
            let sr = rusty_music_editor::SR;
            // Le rééchantillonnage est dans `grille_reechantillonnee` ; ici on
            // veut les candidats, donc on le fait une fois pour toutes.
            let cible = rusty_music_analysis::mel::SR;
            let rapport = sr as f64 / cible as f64;
            let n = (mono.len() as f64 / rapport) as usize;
            let a48: Vec<f32> = (0..n)
                .map(|i| {
                    let x = i as f64 * rapport;
                    let (j, t) = (x.floor() as usize, x.fract() as f32);
                    let u = mono[j.min(mono.len() - 1)];
                    let v = mono[(j + 1).min(mono.len() - 1)];
                    u * (1.0 - t) + v * t
                })
                .collect();

            let liste = rusty_music_analysis::battements::candidats(&a48, &a, candidats);
            if liste.is_empty() {
                println!("aucune pulsation");
            }
            for (i, g) in liste.iter().enumerate() {
                let rel = if i == 0 {
                    String::new()
                } else {
                    let d = (g.phase_s - liste[0].phase_s).abs();
                    let p = liste[0].periode();
                    format!("  ({:+.0} % de battement)", (d.min(p - d) / p) * 100.0)
                };
                println!(
                    "{:>2}.  {:>6.1} BPM   phase {:.3} s   netteté {:.2}{rel}",
                    i + 1,
                    g.bpm,
                    g.phase_s,
                    g.nettete
                );
            }
        }
        Cmd::Greffer {
            remplace,
            greffon,
            sortie,
            bpm_source,
            bpm_greffon,
            sans_grille,
        } => {
            let t = Instant::now();
            // Les tempos donnés à la main l'emportent : `--bpm-source` sert
            // précisément à corriger une mesure qu'on juge fausse.
            let grilles = if sans_grille {
                None
            } else {
                grilles_de(&remplace, &greffon)?
            };
            if let Some((a, b)) = grilles {
                println!(
                    "grilles : {:.1} BPM à {:.3} s (netteté {:.1}) · {:.1} BPM à {:.3} s (netteté {:.1})",
                    a.bpm, a.phase_s, a.nettete, b.bpm, b.phase_s, b.nettete
                );
            }
            let plan = rusty_music_editor::greffe::greffer(
                &remplace,
                &greffon,
                bpm_source.or(grilles.map(|(a, _)| a.bpm)),
                bpm_greffon.or(grilles.map(|(_, b)| b.bpm)),
                grilles.map(|(a, b)| rusty_music_editor::greffe::Cale {
                    phase_remplace_s: a.phase_s,
                    phase_greffon_s: b.phase_s,
                    periode_greffon_s: b.periode(),
                }),
                &sortie,
            )?;
            let octave = match plan.octaves {
                0 => String::new(),
                n if n > 0 => format!(", ×{} le tempo", 1 << n),
                n => format!(", ÷{} le tempo", 1 << -n),
            };
            // **La commande vérifie son propre travail.** Écrire « calé sur les
            // temps » est une affirmation ; relire la grille de ce qu'on vient
            // d'écrire et la comparer à celle du stem remplacé en est une
            // preuve. C'est aussi le seul contrôle qui porte sur de la vraie
            // musique — les tests unitaires travaillent sur des clics.
            // **On n'y remesure pas une grille, on éprouve celle du stem
            // remplacé.** Remesurer comparerait deux tirages ambigus : sur une
            // batterie, plusieurs phases ramassent presque autant (voir
            // `rusty-music battements`). La question utile est « la greffe
            // pulse-t-elle là où l'original pulsait », et elle a une réponse.
            if let Some((a, _)) = grilles {
                let (impose, meilleur) = eprouver(&sortie, a.bpm, a.phase_s)?;
                match (impose, meilleur) {
                    (Some(i), Some(m)) => println!(
                        "vérification : sur la greffe, la grille du stem remplacé vaut {i:.2} \
                         (le meilleur couple vaut {m:.2}, une phase quelconque 1,00)"
                    ),
                    _ => println!("vérification : la greffe ne pulse pas assez pour trancher"),
                }
            }
            println!(
                "étiré ×{:.3}{octave}, entrée à {:.1} s, {} passage(s), {} — {:.1} s de calcul\n{}",
                plan.facteur,
                plan.retard_s,
                plan.boucles,
                if plan.cale_aux_temps {
                    "calé sur les temps"
                } else {
                    "calé sur la première attaque"
                },
                t.elapsed().as_secs_f64(),
                sortie.display()
            );
        }
        Cmd::Descripteurs { limite, fils } => {
            let fils = if fils > 0 {
                fils
            } else {
                std::thread::available_parallelism().map_or(4, |p| p.get())
            };
            let (faits, total) = lib.compter_descripteurs(rusty_music_analysis::passe::MODELE)?;
            println!("{faits} / {total} morceaux déjà mesurés — {fils} fils");

            let t = Instant::now();
            let mut dernier = 0usize;
            let r = rusty_music_analysis::passe::descripteurs(
                &lib,
                if limite == 0 { i64::MAX } else { limite },
                fils,
                |vus, total| {
                    if vus >= dernier + 200 {
                        dernier = vus;
                        println!(
                            "  {vus} / {total} — {:.1} min",
                            t.elapsed().as_secs_f64() / 60.0
                        );
                    }
                },
            )?;
            println!(
                "\n{} mesurés sur {} demandés — {:.1} min ({:.2} s/morceau)",
                r.mesures,
                r.demandes,
                t.elapsed().as_secs_f64() / 60.0,
                t.elapsed().as_secs_f64() / r.mesures.max(1) as f64
            );
            println!(
                "{} sans pulsation décelable, {} sans tonalité, {} en échec",
                r.sans_tempo, r.sans_tonalite, r.echecs
            );
        }
        Cmd::Itineraire {
            depart,
            arrivee,
            profil,
            minutes,
            etapes,
            eviter_autoroutes,
            alternatives,
            k,
        } => {
            use rusty_music_analysis::reseau::{Options, Profil};
            let modele = rusty_music_analysis::passe::MODELE;

            let (reseau, rapport) = construire_reseau(&lib, modele, k)?;
            println!(
                "réseau : {} morceaux, {} arêtes, {} refuges — {:.1} s",
                rapport.morceaux,
                rapport.aretes,
                rapport.refuges,
                rapport.ms_total as f64 / 1000.0
            );
            for (nom, n) in &rapport.par_classe {
                println!("  {nom:<12} {n:>7}");
            }

            let profil = if !etapes.is_empty() {
                Profil::Etapes(etapes)
            } else {
                match profil.as_str() {
                    "sentier" => Profil::Sentier,
                    "panoramique" => Profil::Panoramique,
                    "autoroute" => Profil::Autoroute,
                    autre => anyhow::bail!("profil inconnu : {autre}"),
                }
            };
            let mut o = Options::nouveau(depart, profil);
            o.arrivee = arrivee;
            o.eviter_autoroutes = eviter_autoroutes;
            o.alternatives = alternatives.clamp(1, 3);
            if let Some(m) = minutes {
                o.duree_cible_ms = Some(m * 60_000);
            }

            let t = std::time::Instant::now();
            let trajets = reseau.itineraires(&o)?;
            println!("\n{} itinéraire(s) en {:.0} ms", trajets.len(), t.elapsed().as_secs_f64() * 1000.0);
            for (n, i) in trajets.iter().enumerate() {
                println!(
                    "\n— itinéraire {} — {} morceaux, {:.1} min, distance sonique {:.2}",
                    n + 1,
                    i.morceaux.len(),
                    i.duree_ms as f64 / 60_000.0,
                    i.distance_sonique
                );
                for (rang, id) in i.morceaux.iter().enumerate() {
                    let t = lib.track(*id)?;
                    let (titre, artiste) = t
                        .map(|t| (t.title.unwrap_or_default(), t.artist.unwrap_or_default()))
                        .unwrap_or_default();
                    // Le dénivelé : la popularité le long du trajet, ce que
                    // l'interface tracera en profil d'altitude.
                    let pop = i.popularite[rang];
                    let barre = "▁▂▃▄▅▆▇█"
                        .chars()
                        .nth(((pop * 7.0).round() as usize).min(7))
                        .unwrap_or('▁');
                    let voie = i
                        .classes
                        .get(rang)
                        .map(|c| format!(" {c:?}").to_lowercase())
                        .unwrap_or_default();
                    println!("  {barre} {artiste} — {titre}{voie}");
                }
            }
        }
        Cmd::Tuiles {
            sortie,
            zoom_max,
            repertoire,
        } => {
            use rusty_music_carto::{relief, source, tuiles};
            let modele = rusty_music_analysis::passe::MODELE;

            let depart = std::time::Instant::now();
            let vue = lib.map_view(modele)?;
            if vue.is_empty() {
                anyhow::bail!(
                    "aucun morceau sur la carte — lancer `rusty-music analyser` puis `carte` d'abord"
                );
            }
            let familles: Vec<source::Famille> = lib
                .familles(modele)?
                .into_iter()
                .map(|(id, nom, effectif)| source::Famille {
                    id,
                    nom,
                    effectif: effectif as usize,
                })
                .collect();
            let lecture = depart.elapsed();
            let parametres = lib.parametres_carte()?.parametres_densite();

            // --- L'ordre de la chaîne, et il compte ---------------------
            //
            // Le peuplement passe **en premier**, et tout le reste en découle.
            // Une première version le calculait à la fin et n'en gardait que
            // les établissements : les morceaux restaient à leurs coordonnées
            // t-SNE, et la carte montrait un nuage avec des épingles posées
            // dessus. Zoomer sur « LED ZEPPELIN » ne montrait pas les morceaux
            // de Led Zeppelin groupés là, mais les points qui traînaient dans
            // le coin.
            //
            // Désormais : peuplement → parcelles → densité, relief,
            // territoires et réseau, tous calculés sur ces parcelles.
            let t_peupl = std::time::Instant::now();
            let ordre = lib.ordre_darrivee()?;
            let par_id: std::collections::HashMap<i64, &rusty_music_core::db::MapPoint> =
                vue.iter().map(|p| (p.id, p)).collect();
            let empreintes: std::collections::HashMap<i64, Vec<f32>> =
                lib.embeddings(modele)?.into_iter().collect();
            let arrivants: Vec<rusty_music_carto::peuplement::Arrivant> = ordre
                .iter()
                .filter_map(|a| {
                    let p = par_id.get(&a.track_id)?;
                    Some(rusty_music_carto::peuplement::Arrivant {
                        track_id: a.track_id,
                        x: p.x,
                        y: p.y,
                        empreinte: empreintes.get(&a.track_id).cloned().unwrap_or_default(),
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
            println!(
                "  peuplement : {} établissements ({}) en {:.1} s",
                peupl.rapport.etablissements,
                peupl
                    .rapport
                    .par_rang
                    .iter()
                    .map(|(n, c)| format!("{c} {n}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                t_peupl.elapsed().as_secs_f64()
            );

            // **La parcelle remplace la coordonnée t-SNE.** C'est ce qui fait
            // que les morceaux habitent la carte au lieu de flotter dessus.
            let parcelles: std::collections::HashMap<i64, (f32, f32)> = peupl
                .habitants
                .iter()
                .map(|h| (h.track_id, (h.x, h.y)))
                .collect();

            let morceaux: Vec<source::Morceau> = vue
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

            // --- Deux géographies, et il faut les distinguer ------------
            //
            // **La terre ne se définit pas par où sont les maisons.** Le
            // littoral, le relief et les territoires viennent de la
            // distribution d'origine — la projection, lisse et continue. Les
            // agglomérations, les morceaux et les routes viennent des
            // parcelles.
            //
            // Les calculer tous sur les parcelles a été essayé : les morceaux
            // groupés font du champ de densité une série de pics, la terre se
            // fragmente en un archipel d'îles à une ville, et il ne reste
            // aucun continent. C'est cohérent avec le modèle et illisible
            // comme carte.
            let points: Vec<(i64, f32, f32, i64)> = lib.map_points(modele)?;
            let densite = std::time::Instant::now();
            let nappe = rusty_music_core::density::calculer(&points, &parametres);
            let mut para_relief = parametres;
            para_relief.noyau = relief::NOYAU;
            let champ = rusty_music_core::density::champ_global(&points, &para_relief);
            let densite = densite.elapsed();

            // Le réseau : les arêtes viennent du son, mais leur géométrie —
            // donc le filtre de longueur qui décide ce qui est dessinable —
            // vient des parcelles.
            let t_reseau = std::time::Instant::now();
            let (reseau, rap) = construire_reseau_sur(&lib, modele, 12, &parcelles)?;
            // **Le réseau relie des lieux, pas des morceaux.** Tant qu'il
            // reliait les morceaux un à un, les trois quarts des tronçons
            // étaient trop longs pour être dessinés — depuis que les morceaux
            // sont groupés, une arête sonore va d'un établissement à un autre.
            // On les agrège donc par couple d'établissements : une route, sa
            // classe (la meilleure des arêtes qui la traversent) et le nombre
            // de liens qu'elle porte.
            let etab_de: std::collections::HashMap<i64, u32> = peupl
                .habitants
                .iter()
                .map(|h| (h.track_id, h.etablissement))
                .collect();
            let centre: std::collections::HashMap<u32, (f32, f32)> = peupl
                .etablissements
                .iter()
                .map(|e| (e.id, (e.cx, e.cy)))
                .collect();
            let routes = source::reseau_entre_lieux(
                &reseau.troncons_identifies(),
                &etab_de,
                &centre,
                &champ,
                para_relief.resolution,
            );
            let compte = |nom: &str| {
                rap.par_classe.iter().find(|(n, _)| n == nom).map(|(_, n)| *n).unwrap_or(0)
            };
            let longues = routes
                .iter()
                .filter(|r| {
                    let (a, z) = (r.points[0], r.points[r.points.len() - 1]);
                    let (dx, dy) = (a[0] - z[0], a[1] - z[1]);
                    (dx * dx + dy * dy).sqrt() > 0.20 && r.classe <= 1
                })
                .count();
            let ms_reseau = t_reseau.elapsed();

            // Les rivières descendent du relief vers la mer, et les points
            // remarquables signalent ce qui mérite le détour.
            let rivieres = rusty_music_carto::hydro::tracer(
                &champ,
                para_relief.resolution,
                &rusty_music_carto::hydro::Parametres::default(),
            );
            let curiosites = source::curiosites(
                &morceaux,
                &peupl.etablissements,
                &reseau.refuges(0.995),
                60,
            );
            println!(
                "  {} rivières, {} points remarquables",
                rivieres.len(),
                curiosites.len()
            );

            let src = source::Source {
                morceaux,
                familles,
                bandes: nappe.bandes,
                routes,
                etablissements: peupl.etablissements,
                rivieres,
                curiosites,
                ..Default::default()
            };
            let paliers = tuiles::Paliers {
                zoom_max,
                ..Default::default()
            };
            let rep_carte = repertoire.as_ref().map(|r| r.join("carte"));
            let r = tuiles::ecrire_avec(&src, &paliers, &sortie, rep_carte.as_deref())?;

            // Le relief part du même champ, dans le même carré : les deux
            // archives se superposent au pixel près.
            let chemin_relief = sortie.with_file_name(format!(
                "{}-relief.pmtiles",
                sortie
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "carte".into())
            ));
            let ombrage = relief::Ombrage::default();
            let rep_relief = repertoire.as_ref().map(|r| r.join("relief"));
            let rr = relief::ecrire_avec(
                &champ,
                parametres.resolution,
                &ombrage,
                &chemin_relief,
                rep_relief.as_deref(),
            )?;

            // Le style vit **à côté des archives** : c'est là que
            // l'application le lit, et l'écrire avec les tuiles interdit qu'ils
            // divergent. La base `tuiles://` est interceptée côté JavaScript
            // par `maplibregl.addProtocol`.
            let palette = rusty_music_carto::Palette::osm_clair();
            let style_app =
                rusty_music_carto::style::construire(&src, &paliers, "tuiles://tuiles", &palette);
            if let Some(dossier) = sortie.parent() {
                std::fs::write(
                    dossier.join("style.json"),
                    serde_json::to_vec_pretty(&style_app)?,
                )?;
            }

            if let Some(rep) = &repertoire {
                let style = rusty_music_carto::style::construire(&src, &paliers, ".", &palette);
                std::fs::write(rep.join("style.json"), serde_json::to_vec_pretty(&style)?)?;
                std::fs::write(rep.join("index.html"), PAGE_ESSAI)?;
                println!("{}/index.html — servir ce dossier en HTTP\n", rep.display());
            }

            println!(
                "{}\n  lecture de la base : {:.2} s\n  nappe de densité   : {:.2} s\n  \
                 réseau ({} autoroutes, {} nationales, dont {} trop longues pour être dessinées) : {:.2} s\n  \
                 tuiles             : {:.2} s\n  total              : {:.2} s",
                sortie.display(),
                lecture.as_secs_f64(),
                densite.as_secs_f64(),
                compte("autoroute"),
                compte("nationale"),
                longues,
                ms_reseau.as_secs_f64(),
                r.duree.as_secs_f64(),
                (lecture + densite + ms_reseau + r.duree).as_secs_f64(),
            );
            println!(
                "  {} tuiles, {:.1} Mo\n",
                r.tuiles,
                r.octets as f64 / 1_048_576.0
            );
            println!(
                "{}\n  {} tuiles d'ombrage, {:.1} Mo, {:.2} s\n",
                chemin_relief.display(),
                rr.tuiles,
                rr.octets as f64 / 1_048_576.0,
                rr.duree.as_secs_f64()
            );
            println!("  zoom   tuiles      octets   moy/tuile");
            for (z, n, o) in &r.par_zoom {
                println!(
                    "  {z:>4}  {n:>7}  {:>9.1} Ko  {:>7.1} Ko",
                    *o as f64 / 1024.0,
                    *o as f64 / 1024.0 / *n as f64
                );
            }
        }
        Cmd::Ville { pbf, commune, sortie } => {
            let sortie = sortie.unwrap_or_else(|| {
                cli.db.with_file_name(format!("ville-{}.db", commune.to_lowercase()))
            });
            println!("Lecture de {}…", pbf.display());
            let depart = std::time::Instant::now();
            let extrait = rusty_music_osm::extraire(&pbf, rusty_music_osm::PARIS, Some(&commune))?;
            let r = extrait.resume();
            match &extrait.frontiere {
                Some(f) => println!(
                    "  {commune} découpé sur sa limite communale ({} anneau(x)) en {:.1} s",
                    f.anneaux.len(),
                    depart.elapsed().as_secs_f64()
                ),
                None => println!(
                    "  ATTENTION : limite communale introuvable, cadre rectangulaire conservé"
                ),
            }
            println!(
                "  {} tronçons ({:.0} km), {} rues distinctes, {} bâtiments, {} surfaces d'eau, {} espaces verts",
                r.troncons, r.longueur_km, r.rues_distinctes, r.batis, r.eaux, r.verts
            );
            rusty_music_osm::base::ecrire(
                &extrait,
                &sortie,
                &commune,
                "Geofabrik / OpenStreetMap (ODbL)",
            )?;
            let poids = std::fs::metadata(&sortie).map(|m| m.len()).unwrap_or(0);
            println!(
                "  écrit dans {} ({:.1} Mo)",
                sortie.display(),
                poids as f64 / 1e6
            );
            println!("  © les contributeurs OpenStreetMap — données sous ODbL");
        }
        Cmd::Quartiers { ville } => {
            let ville = if ville.is_absolute() {
                ville
            } else {
                cli.db.with_file_name(&ville)
            };
            let modele = rusty_music_analysis::passe::MODELE;
            let vue = lib.map_view(modele)?;
            if vue.is_empty() {
                anyhow::bail!("aucun morceau sur la carte — lancer `analyser` puis `carte` d'abord");
            }
            let noms: HashMap<i64, String> = lib
                .familles(modele)?
                .into_iter()
                .map(|(id, nom, _)| (id, nom))
                .collect();

            println!("Lecture de {}…", ville.display());
            let extrait = rusty_music_osm::base::lire(&ville)?;

            let depart = std::time::Instant::now();
            let prep = rusty_music_carto::ville::preparer(
                &extrait,
                &vue,
                rusty_music_carto::ville::ESPACEMENT_PAR_DEFAUT,
                Some(rusty_music_carto::ville::ILE_DE_LA_CITE),
            );
            let quartiers = &prep.quartiers;
            let duree = depart.elapsed();

            println!(
                "  {} familles ({} morceaux), {} artistes ancrés aux monuments — zone peuplée : {} rues, {} bâtiments\n",
                prep.familles.len(),
                vue.len(),
                prep.ancrages.par_artiste.len(),
                prep.rues_noyau.len(),
                prep.autorises.len(),
            );

            println!(
                "  {:<28} {:>10} {:>10} {:>8}",
                "famille", "cible (m)", "obtenue", "écart"
            );
            let mut ids: Vec<i64> = prep.familles.iter().map(|f| f.id).collect();
            ids.sort_by(|a, b| {
                quartiers.cible[b]
                    .partial_cmp(&quartiers.cible[a])
                    .unwrap()
            });
            for id in ids {
                let cible = quartiers.cible[&id];
                let obtenue = quartiers.capacite.get(&id).copied().unwrap_or(0.0);
                let ecart = 100.0 * (obtenue - cible) / cible.max(1.0);
                let nom = noms.get(&id).cloned().unwrap_or_else(|| format!("famille {id}"));
                println!("  {nom:<28} {cible:>10.0} {obtenue:>10.0} {ecart:>+7.0} %");
            }
            println!(
                "\n  erreur relative maximale : {:.0} % — {} rues, {:.2} s",
                100.0 * quartiers.erreur_relative_max(),
                prep.rues_noyau.len(),
                duree.as_secs_f64()
            );
        }
        Cmd::Rues { ville, espacement } => {
            let ville = if ville.is_absolute() {
                ville
            } else {
                cli.db.with_file_name(&ville)
            };
            let modele = rusty_music_analysis::passe::MODELE;
            let vue = lib.map_view(modele)?;
            if vue.is_empty() {
                anyhow::bail!("aucun morceau sur la carte — lancer `analyser` puis `carte` d'abord");
            }
            println!("Lecture de {}…", ville.display());
            let extrait = rusty_music_osm::base::lire(&ville)?;

            let depart = std::time::Instant::now();
            let prep = rusty_music_carto::ville::preparer(
                &extrait,
                &vue,
                espacement,
                Some(rusty_music_carto::ville::ILE_DE_LA_CITE),
            );
            let voirie = &prep.voirie;
            let duree = depart.elapsed();

            println!(
                "  {} familles, {} artistes ({} morceaux), {} ancrés aux monuments — zone peuplée : {} rues, {} bâtiments, espacement {espacement:.0} m\n",
                prep.familles.len(),
                prep.artistes.len(),
                vue.len(),
                prep.ancrages.par_artiste.len(),
                prep.rues_noyau.len(),
                prep.autorises.len(),
            );

            let mut par_taille: Vec<usize> = voirie.logements.values().map(|l| l.rues.len()).collect();
            par_taille.sort_unstable();
            let sur_une_rue = par_taille.iter().filter(|n| **n == 1).count();
            let sur_plusieurs = par_taille.iter().filter(|n| **n > 1).count();
            let max_rues = par_taille.last().copied().unwrap_or(0);
            let capacite_totale: usize = voirie.logements.values().map(|l| l.capacite).sum();
            let besoin_total: usize = prep.artistes.iter().map(|a| a.effectif).sum();

            println!(
                "  {} artistes logés — {sur_une_rue} sur une seule rue, {sur_plusieurs} sur plusieurs (jusqu'à {max_rues})",
                voirie.logements.len()
            );
            println!(
                "  capacité offerte {capacite_totale} adresses pour {besoin_total} morceaux ({:.0} % de marge)",
                100.0 * (capacite_totale as f64 / besoin_total.max(1) as f64 - 1.0)
            );
            println!(
                "  {} rues jamais prises, {} débordements de zone",
                voirie.rues_libres.len(),
                voirie.debordements.len()
            );
            if !voirie.debordements.is_empty() {
                println!("  débordements : {}", voirie.debordements.join(", "));
            }

            let mut plus_gros: Vec<(&String, &rusty_music_carto::affectation::Logement)> =
                voirie.logements.iter().collect();
            plus_gros.sort_by_key(|(_, l)| std::cmp::Reverse(l.rues.len()));
            println!("\n  les artistes logés sur le plus de rues :");
            for (nom, logement) in plus_gros.iter().take(8) {
                println!(
                    "    {:<28} {} rue(s), {} adresses — {}",
                    nom,
                    logement.rues.len(),
                    logement.capacite,
                    logement.rues.join(" · ")
                );
            }

            println!("\n  {duree:.2?} pour {} artistes", prep.artistes.len());
        }
        Cmd::Adresses { ville, espacement, echantillon } => {
            let ville = if ville.is_absolute() {
                ville
            } else {
                cli.db.with_file_name(&ville)
            };
            let modele = rusty_music_analysis::passe::MODELE;
            let vue = lib.map_view(modele)?;
            if vue.is_empty() {
                anyhow::bail!("aucun morceau sur la carte — lancer `analyser` puis `carte` d'abord");
            }

            let noms: HashMap<i64, String> = lib
                .familles(modele)?
                .into_iter()
                .map(|(id, nom, _)| (id, nom))
                .collect();

            println!("Lecture de {}…", ville.display());
            let extrait = rusty_music_osm::base::lire(&ville)?;

            let depart = std::time::Instant::now();
            let r = rusty_music_carto::ville::rassembler(
                &extrait,
                &vue,
                &noms,
                espacement,
                Some(rusty_music_carto::ville::ILE_DE_LA_CITE),
            );
            let duree = depart.elapsed();

            println!(
                "  {} adresses posées, {} sans adresse, {} repli quartier, {} hors zone — {duree:.2?}",
                r.adresses_posees, r.morceaux_sans_adresse, r.repli_quartier, r.hors_zone,
            );
            println!(
                "  {} artistes ancrés aux monuments, {} bâtiments peuplés, erreur quartiers {:.0} %, {} débordements",
                r.artistes_ancres,
                r.batiments_peuples,
                100.0 * r.quartiers_erreur_relative,
                r.debordements,
            );

            // --- Objection V1 : le voisinage musical survit-il à l'affectation ? ---
            //
            // kNN géographique par balayage complet, comme `chemin::voisins` le
            // fait côté empreintes. Positions en mètres locaux : le lon/lat
            // brut biaiserait le voisinage par la latitude.
            println!("\n  mesure du voisinage (échantillon de {echantillon}) :");
            let empreintes = lib.embeddings(modele)?;
            let repere = rusty_music_carto::affectation::Repere::centre_de(&extrait);
            let positions: HashMap<i64, [f64; 2]> = r
                .source
                .morceaux
                .iter()
                .map(|m| (m.id, repere.vers_m([m.x as f64, m.y as f64])))
                .collect();
            let artiste_de: HashMap<i64, &str> = vue
                .iter()
                .filter_map(|p| rusty_music_carto::ancrage::nom_artiste(p).map(|n| (p.id, n)))
                .collect();
            let ids_places: Vec<i64> = positions.keys().copied().collect();
            let pas = (ids_places.len() / echantillon.max(1)).max(1);
            let echantillon_ids: Vec<i64> = ids_places.iter().step_by(pas).copied().collect();

            const K: usize = rusty_music_analysis::chemin::K_VOISINS;
            let mut recouvrements = Vec::with_capacity(echantillon_ids.len());
            let mut part_meme_artiste = Vec::with_capacity(echantillon_ids.len());
            for &id in &echantillon_ids {
                let musicaux = rusty_music_analysis::chemin::voisins(&empreintes, id, K);
                if musicaux.is_empty() {
                    continue;
                }
                let Some(&ici) = positions.get(&id) else { continue };
                let mut geo: Vec<(i64, f64)> = positions
                    .iter()
                    .filter(|(j, _)| **j != id)
                    .map(|(j, p)| (*j, (p[0] - ici[0]).powi(2) + (p[1] - ici[1]).powi(2)))
                    .collect();
                let k = K.min(geo.len());
                if k == 0 {
                    continue;
                }
                geo.select_nth_unstable_by(k - 1, |a, b| a.1.total_cmp(&b.1));
                geo.truncate(k);
                let geo_set: std::collections::HashSet<i64> = geo.into_iter().map(|(j, _)| j).collect();
                let commun = musicaux.iter().filter(|j| geo_set.contains(j)).count();
                recouvrements.push(commun as f64 / K as f64);

                // Diagnostic : la perte vient-elle de l'ordre intra-rue (le
                // même artiste, mais mal ordonné) ou du placement entre
                // artistes (des voisins sonores chez un tout autre artiste,
                // donc sur une tout autre rue) ?
                if let Some(&nom) = artiste_de.get(&id) {
                    let meme = musicaux.iter().filter(|j| artiste_de.get(j) == Some(&nom)).count();
                    part_meme_artiste.push(meme as f64 / K as f64);
                }
            }
            recouvrements.sort_by(|a, b| a.total_cmp(b));
            let moyenne = recouvrements.iter().sum::<f64>() / recouvrements.len().max(1) as f64;
            let mediane = recouvrements.get(recouvrements.len() / 2).copied().unwrap_or(0.0);
            let part_meme_artiste_moy =
                part_meme_artiste.iter().sum::<f64>() / part_meme_artiste.len().max(1) as f64;
            println!(
                "    recouvrement k={K} — moyenne {:.0} %, médiane {:.0} % sur {} morceaux testés",
                100.0 * moyenne,
                100.0 * mediane,
                recouvrements.len()
            );
            println!(
                "    (part des {K} plus proches voisins musicaux qui restent parmi les {K} plus proches géographiques)"
            );
            println!(
                "    part des voisins musicaux qui sont du MÊME artiste : {:.0} % en moyenne",
                100.0 * part_meme_artiste_moy
            );
        }
        Cmd::Familles { artistes } => {
            let modele = rusty_music_analysis::passe::MODELE;
            let (couverts, total) = lib.mb_couverture(modele, 1)?;
            let (faits, artistes_total, avec) = lib.mb_avancement()?;
            println!(
                "MusicBrainz : {faits}/{artistes_total} artistes interrogés, {avec} avec un genre\n\
                 couverture : {couverts}/{total} morceaux ({:.1} %)\n",
                100.0 * couverts as f64 / total.max(1) as f64
            );
            for (cluster, nom, n) in lib.familles(modele)? {
                println!("{n:>6}  {nom}");
                if artistes {
                    for a in lib.artistes_de_famille(modele, cluster, 6)? {
                        println!("          {a}");
                    }
                }
            }
        }
        Cmd::Enrich { limite, contact } => {
            let mut lib = lib;
            // MusicBrainz refuse les clients anonymes : l'agent doit porter un
            // contact. Mieux vaut le dire ici que récolter des refus.
            let contact = contact
                .or_else(|| std::env::var("RUSTY_MUSIC_CONTACT").ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "MusicBrainz exige un contact dans l'agent.\n  \
                         Le donner avec --contact, ou par RUSTY_MUSIC_CONTACT."
                    )
                })?;
            let client = rusty_music_core::musicbrainz::Client::new(&contact);
            let (faits, total, _) = lib.mb_avancement()?;
            let limite = if limite == 0 { usize::MAX } else { limite };
            println!(
                "{faits} / {total} artistes déjà interrogés — \
                 environ {:.0} min pour le reste",
                (total - faits).max(0) as f64 * 2.2 / 60.0
            );

            let t = Instant::now();
            let mut dernier = 0usize;
            let bilan = rusty_music_core::enrichir::enrichir(&mut lib, &client, limite, |b| {
                // Une ligne toutes les vingt-cinq : assez pour voir que ça
                // avance, assez peu pour ne pas noyer un terminal pendant une
                // heure.
                if b.artistes >= dernier + 25 {
                    dernier = b.artistes;
                    println!(
                        "  {} artistes · {} avec genre · {} albums · {:.0} min",
                        b.artistes,
                        b.avec_genre,
                        b.albums,
                        t.elapsed().as_secs_f64() / 60.0
                    );
                }
            })?;
            println!(
                "\n{} artistes interrogés, {} avec au moins un genre, {} albums — {:.0} min",
                bilan.artistes,
                bilan.avec_genre,
                bilan.albums,
                t.elapsed().as_secs_f64() / 60.0
            );
            if bilan.echecs > 0 {
                println!(
                    "{} artistes abandonnés après plusieurs tentatives ; \
                     relancer la commande les reprendra.",
                    bilan.echecs
                );
            }
        }
        Cmd::Decouvrir {
            contact,
            jours,
            limite,
        } => {
            let mut lib = lib;
            let contact = contact
                .or_else(|| std::env::var("RUSTY_MUSIC_CONTACT").ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Les API demandent un contact dans l'agent.\n  \
                         Le donner avec --contact, ou par RUSTY_MUSIC_CONTACT."
                    )
                })?;
            let lb = rusty_music_core::listenbrainz::Client::new(&contact);

            println!("Actualisation du fil Découvrir…");
            let t = Instant::now();
            let mut dernier = 0usize;
            let bilan =
                rusty_music_core::decouvrir::actualiser(&mut lib, &lb, jours, limite, |b| {
                    if b.artistes >= dernier + 5 {
                        dernier = b.artistes;
                        println!(
                            "  {} / {} étapes · {} sorties · {} voisins · {:.0} min",
                            b.artistes,
                            b.total,
                            b.sorties_neuves,
                            b.voisins_neufs,
                            t.elapsed().as_secs_f64() / 60.0
                        );
                    }
                })?;
            println!(
                "\n{} sorties neuves, {} voisins écrits — {:.0} min",
                bilan.sorties_neuves,
                bilan.voisins_neufs,
                t.elapsed().as_secs_f64() / 60.0
            );
            if bilan.echecs > 0 {
                println!("{} étapes en échec ; relancer la commande les reprendra.", bilan.echecs);
            }
        }
        Cmd::Popularite {
            contact,
            limite,
            rafraichir_des,
        } => {
            let mut lib = lib;
            let contact = contact
                .or_else(|| std::env::var("RUSTY_MUSIC_CONTACT").ok())
                .unwrap_or_default();
            let lb = rusty_music_core::listenbrainz::Client::new(&contact);
            let dz = rusty_music_core::deezer::Client::new();
            let limite = if limite == 0 { usize::MAX } else { limite };
            let depuis = if rafraichir_des <= 0 {
                0
            } else {
                let maintenant = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                maintenant - rafraichir_des * 86_400
            };

            let t = Instant::now();
            let mut dernier = 0usize;
            let bilan = rusty_music_core::popularite::actualiser(
                &mut lib,
                &lb,
                &dz,
                depuis,
                limite,
                |b| {
                    if b.faits >= dernier + 25 {
                        dernier = b.faits;
                        println!(
                            "  {} / {} étapes · {} trouvées sur Deezer · {:.0} min",
                            b.faits,
                            b.total,
                            b.deezer_trouves,
                            t.elapsed().as_secs_f64() / 60.0
                        );
                    }
                },
            )?;
            println!(
                "\n{} enregistrements + {} albums (ListenBrainz), \
                 {} / {} pistes retrouvées (Deezer), {} morceaux couverts — {:.0} min",
                bilan.lb_enregistrements,
                bilan.lb_albums,
                bilan.deezer_trouves,
                bilan.deezer,
                bilan.couverts,
                t.elapsed().as_secs_f64() / 60.0
            );
        }
        Cmd::Project { familles } => {
            let t = Instant::now();
            let p = rusty_music_analysis::passe::projeter_tout(&lib, Some(familles))?;
            println!(
                "{} points, {} familles — {:.1} s",
                p.empreintes,
                p.familles,
                t.elapsed().as_secs_f64()
            );
        }
        Cmd::Voisins { recherche, k } => {
            let modele = rusty_music_analysis::passe::MODELE;
            let trouves = lib.search(&recherche, 10)?;
            // Un titre qui correspond mot pour mot l'emporte sur l'ordre
            // alphabétique de `search` (par artiste/album/piste) : sinon
            // « Bohemian Rhapsody » retombe sur la première piste de l'album
            // du même nom — pas la chanson.
            let depart = trouves
                .iter()
                .find(|t| t.title.as_deref().is_some_and(|ti| ti.eq_ignore_ascii_case(recherche.trim())))
                .or_else(|| trouves.first());
            let Some(depart) = depart else {
                anyhow::bail!("aucun morceau ne correspond à « {recherche} »");
            };
            if trouves.len() > 1 {
                println!(
                    "  ({} résultats pour « {recherche} », pris : {} — {})",
                    trouves.len(),
                    depart.title.as_deref().unwrap_or("?"),
                    depart.artist.as_deref().unwrap_or("?")
                );
            }
            let empreintes = lib.embeddings(modele)?;
            let voisins = rusty_music_analysis::chemin::voisins(&empreintes, depart.id, k);
            if voisins.is_empty() {
                anyhow::bail!("pas d'empreinte pour ce morceau — `analyser` d'abord");
            }
            let par_id: HashMap<i64, TrackRow> = {
                let ids: std::collections::HashSet<i64> = voisins.iter().copied().collect();
                lib.tracks_by_ids(&ids)?.into_iter().map(|t| (t.id, t)).collect()
            };
            println!(
                "\n  {} — {}\n  ses {k} plus proches voisins :",
                depart.title.as_deref().unwrap_or("?"),
                depart.artist.as_deref().unwrap_or("?")
            );
            for (rang, id) in voisins.iter().enumerate() {
                let Some(t) = par_id.get(id) else { continue };
                println!(
                    "  {:>2}. {:<40} {:<25} {}",
                    rang + 1,
                    t.title.as_deref().unwrap_or("?"),
                    t.artist.as_deref().unwrap_or("?"),
                    t.album.as_deref().unwrap_or("")
                );
            }
        }
        Cmd::Path {
            from,
            to,
            steps,
            seed,
            bruit,
        } => {
            use rusty_music_analysis::chemin::{self, Graphe, K_VOISINS};
            use std::collections::HashSet;

            let t = Instant::now();
            let empreintes = lib.embeddings(rusty_music_analysis::passe::MODELE)?;
            println!(
                "{} empreintes chargées — {:.1} s",
                empreintes.len(),
                t.elapsed().as_secs_f64()
            );
            let analyses: HashSet<i64> = empreintes.iter().map(|(id, _)| *id).collect();

            // Le premier résultat de la recherche n'est pas forcément analysé
            // — tant que la passe tourne, la plupart ne le sont pas. On prend
            // donc le premier qui l'est, sinon on dit lequel manque : sans ça,
            // un chemin vide passerait pour un défaut de l'algorithme.
            let un = |q: &str| -> anyhow::Result<rusty_music_core::db::TrackRow> {
                let trouves = lib.search(q, 30)?;
                if trouves.is_empty() {
                    anyhow::bail!("rien ne correspond à « {q} »");
                }
                trouves
                    .into_iter()
                    .find(|p| analyses.contains(&p.id))
                    .ok_or_else(|| {
                        anyhow::anyhow!("aucun résultat de « {q} » n'est encore analysé")
                    })
            };
            let depart = un(&from)?;

            let montrer = |titre: &str, route: &[i64]| -> anyhow::Result<()> {
                println!("\n{titre} — {} morceaux", route.len());
                for id in route {
                    if let Some(p) = lib.track(*id)? {
                        println!(
                            "  {} — {}",
                            p.artist.unwrap_or_default(),
                            p.title.unwrap_or_default()
                        );
                    }
                }
                Ok(())
            };

            let t = Instant::now();
            let fils = std::thread::available_parallelism().map_or(4, |p| p.get());
            let graphe = Graphe::construire(&empreintes, K_VOISINS, fils);
            println!(
                "graphe {K_VOISINS}-plus-proches-voisins sur {fils} fils — {:.1} s",
                t.elapsed().as_secs_f64()
            );

            if let Some(to) = to {
                let arrivee = un(&to)?;
                let t = Instant::now();
                // Le direct raisonne en coordonnées de carte, pas sur les
                // empreintes : c'est un geste à l'écran (voir `chemin`).
                let points: Vec<(i64, f32, f32)> = lib
                    .map_points(rusty_music_analysis::passe::MODELE)?
                    .into_iter()
                    .map(|(id, x, y, _famille)| (id, x, y))
                    .collect();
                let d = chemin::direct(&points, depart.id, arrivee.id, steps, seed, bruit);
                let ms_direct = t.elapsed().as_millis();

                let t = Instant::now();
                let complet = graphe.sonique(depart.id, arrivee.id, seed, bruit);
                let ms_sonique = t.elapsed().as_millis();

                montrer("direct", &d)?;
                if complet.is_empty() {
                    println!("\nsonique — aucun chemin : les deux morceaux sont dans deux composantes disjointes du graphe");
                } else {
                    println!("\n(trajet sonique complet : {} sauts)", complet.len() - 1);
                    montrer("sonique", &chemin::echantillonner(&complet, steps))?;
                }
                println!("\ndirect {ms_direct} ms · sonique {ms_sonique} ms");
            }

            let t = Instant::now();
            let e = graphe.errance(depart.id, steps, seed, bruit);
            let ms = t.elapsed().as_millis();
            montrer("errance", &e)?;
            println!("\nerrance {ms} ms (bruit {bruit})");
        }
        Cmd::Demix {
            query,
            out,
            modele,
            models_dir,
            seconds,
        } => {
            use rusty_music_editor::{decode, sdr, wav, Demixeur, SR};

            // Un chemin direct l'emporte sur la recherche : le module 3 sert
            // aussi à traiter un fichier qui n'est pas dans la bibliothèque.
            let chemin = if Path::new(&query).is_file() {
                PathBuf::from(&query)
            } else {
                let piste = lib
                    .search(&query, 1)?
                    .pop()
                    .ok_or_else(|| anyhow::anyhow!("rien ne correspond à « {query} »"))?;
                println!(
                    "{} — {}",
                    piste.artist.clone().unwrap_or_default(),
                    piste.title.clone().unwrap_or_default()
                );
                PathBuf::from(&piste.path)
            };

            let variante = rusty_music_editor::Variante::analyser(&modele).ok_or_else(|| {
                anyhow::anyhow!(
                    "variante inconnue : « {modele} » — htdemucs, htdemucs_6s ou htdemucs_ft"
                )
            })?;
            let t = Instant::now();
            let demixeur = Demixeur::charger(models_dir.as_deref(), variante)?;
            println!(
                "{} ({} Mo, {} stems) chargé en {:.1} s",
                variante.nom(),
                variante.megaoctets(),
                variante.stems().len(),
                t.elapsed().as_secs_f64()
            );

            let mut audio = decode::stereo(&chemin)?;
            if seconds > 0 {
                let n = (seconds as usize * SR as usize).min(audio.gauche.len());
                audio.gauche.truncate(n);
                audio.droite.truncate(n);
            }
            println!(
                "{:.1} s de stéréo à décoder — séparation sur {}",
                audio.duree(),
                rusty_music_editor::moteur()
            );

            // La chauffe est comptée à part : sur GPU, la première inférence
            // compile ses noyaux, et la mêler au calcul donnerait un chiffre
            // qui ne se reproduit jamais.
            let t = Instant::now();
            demixeur.chauffer();
            println!("chauffe : {:.1} s", t.elapsed().as_secs_f64());

            let t = Instant::now();
            let pistes = demixeur.separer(&audio)?;
            let mis = t.elapsed().as_secs_f64();
            println!(
                "séparation : {:.1} s — {:.1} × le temps réel",
                mis,
                audio.duree() / mis
            );

            // Contrôle de bout en bout : la somme des stems doit reconstituer
            // le mélange. En dessous de 20 dB, la séparation a perdu de la
            // matière en route.
            let mut somme_g = vec![0.0f32; audio.gauche.len()];
            let mut somme_d = vec![0.0f32; audio.droite.len()];
            for p in &pistes {
                for (a, b) in somme_g.iter_mut().zip(&p.gauche) {
                    *a += b;
                }
                for (a, b) in somme_d.iter_mut().zip(&p.droite) {
                    *a += b;
                }
            }
            println!(
                "\nsomme des stems contre l'entrée : {:.1} dB à gauche, {:.1} à droite",
                sdr(&audio.gauche, &somme_g),
                sdr(&audio.droite, &somme_d)
            );

            std::fs::create_dir_all(&out)?;
            let base = chemin
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "morceau".into());
            println!("\n{:<10} {:>10}  fichier", "stem", "RMS");
            for p in &pistes {
                let energie = {
                    let n = (p.gauche.len() + p.droite.len()) as f64;
                    let s: f64 = p
                        .gauche
                        .iter()
                        .chain(&p.droite)
                        .map(|x| (*x as f64).powi(2))
                        .sum();
                    (s / n).sqrt()
                };
                let sortie = out.join(format!("{base} — {}.wav", p.nom));
                wav::ecrire(&sortie, &p.gauche, &p.droite, SR)?;
                println!("{:<10} {energie:>10.6}  {}", p.nom, sortie.display());
            }
        }
        Cmd::Play {
            query,
            album,
            artist,
            seconds,
        } => {
            let pistes = if album {
                lib.tracks_of_album(&query, artist.as_deref())?
            } else {
                lib.search(&query, 1)?
            };
            if pistes.is_empty() {
                anyhow::bail!("rien ne correspond à « {query} »");
            }

            let chemins: Vec<PathBuf> = pistes.iter().map(|t| PathBuf::from(&t.path)).collect();
            let mut player = rusty_music_player::Player::new()?;
            player.play(&chemins)?;

            let debut = Instant::now();
            let mut affichee: Option<PathBuf> = None;
            while !player.is_finished() {
                // On réaffiche seulement quand la piste change.
                let courante = player.current().map(Path::to_path_buf);
                if courante != affichee {
                    if let Some(t) = courante
                        .as_deref()
                        .and_then(|c| pistes.iter().find(|t| Path::new(&t.path) == c))
                    {
                        println!(
                            "♪ {} — {}  [{}]",
                            t.artist.as_deref().unwrap_or("(sans artiste)"),
                            t.title.as_deref().unwrap_or("(sans titre)"),
                            duree(t.duration_ms)
                        );
                    }
                    affichee = courante;
                }
                if seconds > 0 && debut.elapsed() >= Duration::from_secs(seconds) {
                    break;
                }
                // La file n'est plus chargée d'un bloc : c'est cette boucle qui
                // prépare la suite, sans quoi la lecture s'arrêterait au bout
                // des quelques pistes préchargées.
                if let Err(e) = player.completer() {
                    eprintln!("piste ignorée : {e}");
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            println!(
                "arrêt à {}",
                duree(Some(player.position().as_millis() as i64))
            );
        }
    }

    Ok(())
}

/// Page d'essai déposée avec les tuiles en clair. Volontairement minimale :
/// elle sert à juger le rendu et à le mesurer, pas à remplacer l'interface.
const PAGE_ESSAI: &str = include_str!("essai.html");

/// Rassemble ce qu'il faut au réseau de circulation et le construit.
///
/// La popularité est le nombre de morceaux gardés d'un artiste : c'est la
/// seule dont on dispose, la base ne portant aucun compteur d'écoute.
fn construire_reseau(
    lib: &Library,
    modele: &str,
    k: usize,
) -> anyhow::Result<(
    rusty_music_analysis::reseau::Reseau,
    rusty_music_analysis::reseau::RapportConstruction,
)> {
    construire_reseau_sur(lib, modele, k, &std::collections::HashMap::new())
}

/// Idem, mais en plaçant les morceaux là où le peuplement les a installés.
///
/// Les arêtes du réseau viennent du **son** ; leur géométrie vient des
/// parcelles. C'est elle qui décide ce qu'on peut dessiner : une route n'est
/// une route que si elle est courte sur la carte. Un dictionnaire vide laisse
/// les coordonnées de la projection.
fn construire_reseau_sur(
    lib: &Library,
    modele: &str,
    k: usize,
    parcelles: &std::collections::HashMap<i64, (f32, f32)>,
) -> anyhow::Result<(
    rusty_music_analysis::reseau::Reseau,
    rusty_music_analysis::reseau::RapportConstruction,
)> {
    use rusty_music_analysis::reseau::{Morceau, Parametres, Reseau};
    use std::collections::HashMap;

    let empreintes = lib.embeddings(modele)?;
    let vue = lib.map_view(modele)?;
    anyhow::ensure!(!vue.is_empty(), "aucun morceau sur la carte");

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
            let artiste = p.artist.clone().unwrap_or_default();
            let (x, y) = parcelles.get(&p.id).copied().unwrap_or((p.x, p.y));
            Morceau {
                id: p.id,
                duree_ms: p.duration_ms.unwrap_or(0).max(0) as u64,
                artiste: index[artiste.as_str()],
                famille: p.cluster,
                x,
                y,
                morceaux_de_lartiste: par_artiste[&artiste],
            }
        })
        .collect();

    let points = lib.map_points(modele)?;
    let mut parametres = lib.parametres_carte()?.parametres_densite();
    parametres.noyau = rusty_music_carto::relief::NOYAU;
    let champ = rusty_music_core::density::champ_global(&points, &parametres);

    Ok(Reseau::construire_mesure(
        empreintes,
        &morceaux,
        &champ,
        parametres.resolution,
        &Parametres {
            k,
            fils: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8),
            ..Default::default()
        },
    ))
}
