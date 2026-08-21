//! Pilote en ligne de commande du cœur d'ingestion.
//!
//! Sert à valider le cœur avant que l'interface existe :
//!   rusty-music scan  ~/Musique
//!   rusty-music watch ~/Musique
//!   rusty-music stats

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    /// Trace un chemin entre deux morceaux, dans les trois modes calculés
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
