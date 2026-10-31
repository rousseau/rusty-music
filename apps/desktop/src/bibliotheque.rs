// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de la Bibliothèque : consultation, scan et dossier surveillé, passes
//! de fond (analyse, descripteurs, loudness, enrichissement en ligne,
//! Discogs), pochettes.

use super::*;

#[tauri::command(async)]
pub(crate) fn artists(etat: State<Etat>) -> Result<Vec<ArtistRow>, String> {
    let r = verrou(&etat.lib).artists().map_err(echec);
    tracing::info!(n = r.as_ref().map(Vec::len).unwrap_or(0), "artists");
    r
}

#[tauri::command(async)]
pub(crate) fn albums(
    etat: State<Etat>,
    artist: Option<String>,
    mbid: Option<String>,
) -> Result<Vec<AlbumRow>, String> {
    let lib = verrou(&etat.lib);
    // Identifiant et nom sont passés ensemble : le regroupement réunit les
    // pistes étiquetées MusicBrainz et celles qui ne le sont pas, l'ouverture
    // de l'artiste doit faire de même.
    match artist.as_deref() {
        Some(nom) => lib.albums_of_artist(mbid.as_deref(), nom).map_err(echec),
        None => lib.albums(None).map_err(echec),
    }
}

#[tauri::command(async)]
pub(crate) fn tracks_of_album(
    etat: State<Etat>,
    album: String,
    artist: Option<String>,
) -> Result<Vec<TrackRow>, String> {
    tracing::info!(%album, "tracks_of_album");
    verrou(&etat.lib)
        .tracks_of_album(&album, artist.as_deref())
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn search(
    etat: State<Etat>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<TrackRow>, String> {
    verrou(&etat.lib)
        .search(&query, limit.unwrap_or(50))
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn roots(etat: State<Etat>) -> Result<Vec<RootRow>, String> {
    verrou(&etat.lib).roots().map_err(echec)
}

/// Statistiques d'ensemble du mode Bibliothèque — un aperçu, pas une passe :
/// trois requêtes d'agrégation sur la base déjà là, rien à mesurer ni à
/// décoder. Rejouée à chaque entrée dans le mode plutôt que mise en cache :
/// à 27 000 morceaux, l'agrégation reste sous la seconde.
#[derive(Clone, serde::Serialize)]
pub(crate) struct StatsBibliotheque {
    total: i64,
    genres: Vec<(String, i64)>,
    tempo: rusty_music_core::db::Histogramme,
    durees: rusty_music_core::db::Histogramme,
    codecs: Vec<(String, i64)>,
    bitrate: rusty_music_core::db::Histogramme,
    sans_mbid: i64,
    humeur: Vec<(String, i64)>,
    sans_bio: i64,
    sans_critique: i64,
    sans_discogs: i64,
}

#[tauri::command(async)]
pub(crate) fn library_stats(etat: State<Etat>) -> Result<StatsBibliotheque, String> {
    let lib = verrou(&etat.lib);
    Ok(StatsBibliotheque {
        total: lib.count().map_err(echec)?,
        genres: lib.stats_genres().map_err(echec)?,
        tempo: lib.stats_tempo().map_err(echec)?,
        durees: lib.stats_durees().map_err(echec)?,
        codecs: lib.stats_codecs().map_err(echec)?,
        bitrate: lib.stats_bitrate().map_err(echec)?,
        sans_mbid: lib.stats_sans_mbid().map_err(echec)?,
        humeur: lib.stats_humeur().map_err(echec)?,
        sans_bio: lib.stats_sans_bio().map_err(echec)?,
        sans_critique: lib.stats_sans_critique().map_err(echec)?,
        sans_discogs: lib.stats_sans_discogs().map_err(echec)?,
    })
}

/// Morceaux dont le genre résolu ne figure pas parmi les dominants de leur
/// famille sonique — voir [`rusty_music_core::db::Library::genres_suspects`].
#[tauri::command(async)]
pub(crate) fn suspect_genres(
    etat: State<Etat>,
) -> Result<Vec<(i64, String, String, String)>, String> {
    verrou(&etat.lib)
        .genres_suspects(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// Albums présents sous plusieurs éditions chez le même artiste.
#[tauri::command(async)]
pub(crate) fn multiple_editions(
    etat: State<Etat>,
) -> Result<Vec<rusty_music_core::db::EditionsAlbum>, String> {
    verrou(&etat.lib).editions_multiples().map_err(echec)
}

/// Fichiers en échec de scan, du plus récent au plus ancien.
#[tauri::command(async)]
pub(crate) fn scan_failures(etat: State<Etat>) -> Result<Vec<(String, String, i64)>, String> {
    verrou(&etat.lib).echecs_scan().map_err(echec)
}

/// Retire un fichier de la liste des échecs, sans y toucher sur le disque —
/// pour un fichier qu'on sait perdu et qu'on ne veut plus voir revenir à
/// chaque scan.
#[tauri::command(async)]
pub(crate) fn dismiss_scan_failure(etat: State<Etat>, path: String) -> Result<(), String> {
    verrou(&etat.lib)
        .effacer_echec_scan(Path::new(&path))
        .map_err(echec)
}

/// Avancement de l'analyse en cours, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct EtatAnalyse {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
}

#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatEnrichissement {
    pub(crate) en_cours: bool,
    artistes: usize,
    total: usize,
    avec_genre: usize,
    pub(crate) resultat: Option<String>,
}

/// Avancement de la passe de popularité générale, sondé par l'interface.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatPopularite {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
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
pub(crate) fn start_analysis(app: tauri::AppHandle, etat: State<Etat>) -> Result<(), String> {
    {
        let mut a = verrou(&etat.analyse);
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
        let _garde = GardePasse::nouvelle(&etat.analyse);

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
                        let mut a = verrou(&etat.analyse);
                        a.faits = faits;
                        a.total = total;
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
                        .map(|p| (r, p))
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

        let mut a = verrou(&etat.analyse);
        a.en_cours = false;
        a.resultat = Some(bilan);
    });

    Ok(())
}

/// Avancement de l'analyse. L'interface sonde, comme pour le scan.
#[tauri::command(async)]
pub(crate) fn analysis_state(etat: State<Etat>) -> Result<EtatAnalyse, String> {
    Ok(verrou(&etat.analyse).clone())
}

/// Avancement de la mesure tempo/tonalité/énergie, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct EtatDescripteurs {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
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
pub(crate) fn start_descripteurs(
    app: tauri::AppHandle,
    etat: State<Etat>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    {
        let mut d = verrou(&etat.descripteurs);
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
        let _garde = GardePasse::nouvelle(&etat.descripteurs);

        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|lib| {
                let fils = fils_pour_passe(&lib);
                if force {
                    lib.effacer_descripteurs().map_err(|e| e.to_string())?;
                }
                rusty_music_analysis::passe::descripteurs(&lib, i64::MAX, fils, |faits, total| {
                    let mut d = verrou(&etat.descripteurs);
                    d.faits = faits;
                    d.total = total;
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

        let mut d = verrou(&etat.descripteurs);
        d.en_cours = false;
        d.resultat = Some(bilan);
    });

    Ok(())
}

#[tauri::command(async)]
pub(crate) fn descripteurs_state(etat: State<Etat>) -> Result<EtatDescripteurs, String> {
    Ok(verrou(&etat.descripteurs).clone())
}

/// Normalisation de volume à la lecture (mode Bibliothèque) : activée ou
/// non, et sur quelle référence — voir `crate::normalisation`'s absence :
/// deux `AtomicBool` suffisent, pas besoin d'un `Mutex` pour un si petit
/// état lu à chaque ouverture de piste.
#[derive(Default)]
pub(crate) struct ReglagesNormalisation {
    pub(crate) actif: AtomicBool,
    /// `false` = gain de piste, `true` = gain d'album.
    pub(crate) album: AtomicBool,
}

/// Avancement de la passe de loudness EBU R128, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct EtatLoudness {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
}

/// Mesure la loudness EBU R128 (piste + album) des morceaux en attente — ou
/// de tous, `force` effaçant d'abord ce qui est déjà mesuré. Même patron que
/// [`start_descripteurs`] : décode le fichier entier (le seul coût de cette
/// passe), reprenable, tourne sur son propre fil et sa propre connexion.
#[tauri::command(async)]
pub(crate) fn start_loudness(
    app: tauri::AppHandle,
    etat: State<Etat>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    {
        let mut l = verrou(&etat.loudness);
        if l.en_cours {
            return Err("une mesure est déjà en cours".into());
        }
        *l = EtatLoudness {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.loudness);

        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|lib| {
                let fils = fils_pour_passe(&lib);
                if force {
                    lib.effacer_loudness().map_err(|e| e.to_string())?;
                }
                rusty_music_core::loudness::actualiser(&lib, i64::MAX, fils, |faits, total| {
                    let mut l = verrou(&etat.loudness);
                    l.faits = faits;
                    l.total = total;
                })
                .map_err(|e| e.to_string())
            });

        let bilan = match issue {
            Ok(b) => format!(
                "{} mesurés · {} albums recalculés · {} en échec",
                b.mesures, b.albums, b.echecs
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "mesure de loudness terminée");

        let mut l = verrou(&etat.loudness);
        l.en_cours = false;
        l.resultat = Some(bilan);
    });

    Ok(())
}

#[tauri::command(async)]
pub(crate) fn loudness_state(etat: State<Etat>) -> Result<EtatLoudness, String> {
    Ok(verrou(&etat.loudness).clone())
}

/// Avancement de la vérification des pochettes, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct EtatVerifPochettes {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
}

/// Vérifie les pochettes des dossiers dont les fichiers ont changé — ou de
/// tous, avec `force` (`rusty_music_core::pochettes_verif`). Même patron que
/// [`start_loudness`] : fil et connexion propres. Ne modifie aucun fichier de
/// l'utilisateur : les signalements se lisent par [`pochettes_suspectes`].
/// Enchaînée juste après le scan, qui est ce qui amène les fichiers neufs.
#[tauri::command(async)]
pub(crate) fn start_verif_pochettes(
    app: tauri::AppHandle,
    etat: State<Etat>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    {
        let mut v = verrou(&etat.verif_pochettes);
        if v.en_cours {
            return Err("une vérification des pochettes est déjà en cours".into());
        }
        *v = EtatVerifPochettes {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.verif_pochettes);

        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|lib| {
                let fils = fils_pour_passe(&lib);
                rusty_music_core::pochettes_verif::actualiser(&lib, fils, force, |faits, total| {
                    let mut v = verrou(&etat.verif_pochettes);
                    v.faits = faits;
                    v.total = total;
                })
                .map_err(|e| e.to_string())
            });

        let bilan = match issue {
            Ok(b) => format!(
                "{} dossiers vérifiés sur {} · {} pochette{} à examiner",
                b.verifies,
                b.dossiers,
                b.suspectes,
                if b.suspectes > 1 { "s" } else { "" }
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "vérification des pochettes terminée");

        let mut v = verrou(&etat.verif_pochettes);
        v.en_cours = false;
        v.resultat = Some(bilan);
    });

    Ok(())
}

#[tauri::command(async)]
pub(crate) fn verif_pochettes_state(etat: State<Etat>) -> Result<EtatVerifPochettes, String> {
    Ok(verrou(&etat.verif_pochettes).clone())
}

/// Les pochettes à examiner (panneau « Vérifications » du mode Bibliothèque).
#[tauri::command(async)]
pub(crate) fn pochettes_suspectes(
    etat: State<Etat>,
) -> Result<Vec<rusty_music_core::pochettes_verif::PochetteSuspecte>, String> {
    let lib = verrou(&etat.lib);
    rusty_music_core::pochettes_verif::suspectes(&lib).map_err(echec)
}

/// Écarte le signalement d'un dossier (« Ignorer »). Revient tout seul si les
/// fichiers du dossier changent.
#[tauri::command(async)]
pub(crate) fn ignore_pochette_suspecte(etat: State<Etat>, dossier: String) -> Result<(), String> {
    let lib = verrou(&etat.lib);
    lib.ignorer_pochette_suspecte(&dossier).map_err(echec)
}

/// « Image du dossier » : l'album de `dossier` montrera `cover.jpg` (ou
/// équivalent) plutôt que l'image intégrée aux tags. Le choix vit dans la
/// base, pas dans les fichiers, pour survivre à une recopie des MP3. Les
/// pochettes déjà mises en cache pour ses pistes sont effacées.
#[tauri::command(async)]
pub(crate) fn pochette_du_dossier(etat: State<Etat>, dossier: String) -> Result<(), String> {
    let pistes = {
        let lib = verrou(&etat.lib);
        lib.choisir_pochette_dossier(&dossier, true)
            .map_err(echec)?;
        lib.pistes_du_dossier(&dossier).map_err(echec)?
    };
    let cache = etat.db.with_file_name("pochettes");
    for p in pistes {
        let chemin = Path::new(&p);
        if let Some(mtime) = std::fs::metadata(chemin)
            .ok()
            .and_then(|m| m.modified().ok())
        {
            let _ = std::fs::remove_file(cache.join(cle_pochette(chemin, mtime)));
        }
    }
    Ok(())
}

/// Gain de normalisation de volume (linéaire, 1.0 = neutre) pour `chemin`,
/// selon le réglage courant — jamais un blocage : coupé, ou morceau pas
/// encore mesuré, rendent 1.0.
pub(crate) fn gain_pour(etat: &Etat, chemin: &Path) -> f32 {
    if !etat.normalisation.actif.load(Ordering::Relaxed) {
        return 1.0;
    }
    let mode = if etat.normalisation.album.load(Ordering::Relaxed) {
        rusty_music_core::loudness::ModeGain::Album
    } else {
        rusty_music_core::loudness::ModeGain::Piste
    };
    verrou(&etat.lib)
        .gain_lecture(chemin, mode, rusty_music_core::loudness::CIBLE_LUFS)
        .ok()
        .flatten()
        .unwrap_or(1.0)
}

/// Active/coupe la normalisation de volume et choisit sa référence (piste ou
/// album). Réouvre le morceau en cours en tâche de fond à la même position —
/// même recette que `set_amelioration`.
#[tauri::command(async)]
pub(crate) fn set_normalisation(
    app: tauri::AppHandle,
    etat: State<Etat>,
    actif: bool,
    album: bool,
) -> Result<(), String> {
    etat.normalisation.actif.store(actif, Ordering::Relaxed);
    etat.normalisation.album.store(album, Ordering::Relaxed);

    let courant = verrou(&etat.player)
        .current()
        .map(std::path::Path::to_path_buf);
    let Some(chemin) = courant else {
        return Ok(());
    };

    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let gain = gain_pour(&etat, &chemin);
        let ouvert = rusty_music_superres::resoudre(&etat.hd, &chemin);
        let source = match rusty_music_player::ouvrir(&ouvert, gain) {
            Ok(source) => source,
            Err(e) => {
                tracing::warn!(error = %e, "réouverture pour normalisation impossible");
                return;
            }
        };
        let mut player = verrou(&etat.player);
        if let Err(e) = player.remplacer_courant(&chemin, source) {
            tracing::warn!(error = %e, "bascule de normalisation impossible");
        }
    });
    Ok(())
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
pub(crate) fn start_enrichment(
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
        let mut e = verrou(&etat.enrichissement);
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
        let _garde = GardePasse::nouvelle(&etat.enrichissement);
        let client = rusty_music_core::musicbrainz::Client::new(&contact);
        // Ouverture et passe dans un même bloc plutôt qu'en chaîne : la
        // fermeture de progression emprunte `etat`, et l'imbriquer dans un
        // `and_then` ferait vivre cet emprunt au-delà du fil.
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            let total = lib.mb_avancement().map(|(_, t, _)| t).unwrap_or(0) as usize;
            {
                let mut e = verrou(&etat.enrichissement);
                e.total = total;
            }
            rusty_music_core::enrichir::enrichir(&mut lib, &client, usize::MAX, |b| {
                let mut e = verrou(&etat.enrichissement);
                e.artistes = b.artistes;
                e.avec_genre = b.avec_genre;
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
        let mut fin = verrou(&etat.enrichissement);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

/// Où en est l'aspiration des genres.
#[tauri::command(async)]
pub(crate) fn enrichment_state(etat: State<Etat>) -> Result<EtatEnrichissement, String> {
    Ok(verrou(&etat.enrichissement).clone())
}

/// Au-delà de combien de jours une popularité déjà récupérée redevient « à
/// faire » quand on demande un rafraîchissement. Une notoriété bouge lentement.
pub(crate) const POP_PEREMPTION_JOURS: i64 = 90;

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
pub(crate) fn start_popularite(
    app: tauri::AppHandle,
    etat: State<Etat>,
    contact: String,
    rafraichir: bool,
) -> Result<(), String> {
    {
        let mut p = verrou(&etat.popularite);
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
        let _garde = GardePasse::nouvelle(&etat.popularite);
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
                let mut p = verrou(&etat.popularite);
                p.faits = b.faits;
                p.total = b.total;
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

        let mut fin = verrou(&etat.popularite);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

/// Où en est la passe de popularité.
#[tauri::command(async)]
pub(crate) fn popularite_state(etat: State<Etat>) -> Result<EtatPopularite, String> {
    Ok(verrou(&etat.popularite).clone())
}

/// Fraîcheur de la popularité pour la ligne d'alerte du mode Bibliothèque :
/// `(morceaux couverts, epoch de la plus ancienne interrogation, entités
/// périmées, seuil de péremption en jours)`. Le seuil est renvoyé plutôt que
/// recopié en dur côté JS, pour que le texte affiché ne puisse pas mentir si
/// `POP_PEREMPTION_JOURS` change un jour.
#[tauri::command(async)]
pub(crate) fn popularite_fraicheur(
    etat: State<Etat>,
) -> Result<(i64, Option<i64>, i64, i64), String> {
    let (couverts, plus_ancienne, perimes) = verrou(&etat.lib)
        .popularite_fraicheur(POP_PEREMPTION_JOURS)
        .map_err(echec)?;
    Ok((couverts, plus_ancienne, perimes, POP_PEREMPTION_JOURS))
}

/// Avancement de la passe de biographies TheAudioDB, sondé par l'interface.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatBio {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
}

/// Lance la passe de biographies (TheAudioDB).
///
/// Comme la popularité : fil séparé, connexion propre, reprenable et
/// additive. **Aucune clé requise** — la clé de test partagée suffit ;
/// `cle`, si l'utilisateur en a une personnelle, l'accélère. Résolution
/// **exclusivement par MBID** : un artiste sans `mb_artist_id` n'est jamais
/// interrogé par nom (`docs/enrichissement-lecteur.md`).
#[tauri::command(async)]
pub(crate) fn start_biographies(
    app: tauri::AppHandle,
    etat: State<Etat>,
    cle: Option<String>,
) -> Result<(), String> {
    {
        let mut b = verrou(&etat.biographies);
        if b.en_cours {
            return Err("une passe de biographies est déjà en cours".into());
        }
        *b = EtatBio {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.biographies);
        let client = rusty_music_core::theaudiodb::Client::new(cle.as_deref());
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            rusty_music_core::biographies::actualiser(&mut lib, &client, 0, usize::MAX, |b| {
                let mut e = verrou(&etat.biographies);
                e.faits = b.faits;
                e.total = b.total;
            })
            .map_err(|e| e.to_string())
        })();

        let bilan = match issue {
            Ok(b) => format!(
                "{} artistes interrogés, {} avec une biographie",
                b.interroges, b.trouves
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "passe de biographies terminée");

        let mut fin = verrou(&etat.biographies);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn biographies_state(etat: State<Etat>) -> Result<EtatBio, String> {
    Ok(verrou(&etat.biographies).clone())
}

/// La biographie d'un artiste par MBID direct, chargée à l'ouverture de sa
/// grille d'albums au centre (`docs/interface-guidelines.md`, Règle 1,
/// exception du 15 septembre 2026) — pas de MBID, pas d'appel côté UI.
#[tauri::command(async)]
pub(crate) fn bio_artiste(
    etat: State<Etat>,
    mbid: String,
) -> Result<Option<rusty_music_core::db::BioArtiste>, String> {
    verrou(&etat.lib).bio_pour_artiste(&mbid).map_err(echec)
}

/// Avancement de la passe de critiques CritiqueBrainz, sondé par l'interface.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatCritiques {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
}

/// Au-delà de combien de jours une absence de critique redevient « à
/// vérifier ». Une critique neuve reste rare : fenêtre plus longue que la
/// popularité (90 j).
pub(crate) const CRITIQUES_PEREMPTION_JOURS: i64 = 180;

/// Lance la passe de critiques (CritiqueBrainz).
///
/// Aucune clé requise, interrogation par release-group MusicBrainz — jamais
/// de recherche approximative. `rafraichir` : à `false`, ne comble que les
/// albums jamais vérifiés ; à `true`, revérifie aussi ceux de plus de
/// [`CRITIQUES_PEREMPTION_JOURS`] (une critique neuve reste rare).
#[tauri::command(async)]
pub(crate) fn start_critiques(
    app: tauri::AppHandle,
    etat: State<Etat>,
    rafraichir: bool,
) -> Result<(), String> {
    {
        let mut c = verrou(&etat.critiques);
        if c.en_cours {
            return Err("une passe de critiques est déjà en cours".into());
        }
        *c = EtatCritiques {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.critiques);
        let client = rusty_music_core::critiquebrainz::Client::new();
        let depuis = if rafraichir {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64 - CRITIQUES_PEREMPTION_JOURS * 86_400)
                .unwrap_or(0)
        } else {
            0
        };
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            rusty_music_core::critiques::actualiser(&mut lib, &client, depuis, usize::MAX, |b| {
                let mut e = verrou(&etat.critiques);
                e.faits = b.faits;
                e.total = b.total;
            })
            .map_err(|e| e.to_string())
        })();

        let bilan = match issue {
            Ok(b) => format!(
                "{} albums interrogés, {} avec au moins une critique",
                b.albums_interroges, b.albums_avec_critique
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "passe de critiques terminée");

        let mut fin = verrou(&etat.critiques);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn critiques_state(etat: State<Etat>) -> Result<EtatCritiques, String> {
    Ok(verrou(&etat.critiques).clone())
}

/// Les critiques du morceau `id`, via son release-group MusicBrainz.
#[tauri::command(async)]
pub(crate) fn critiques_piste(
    etat: State<Etat>,
    id: i64,
) -> Result<Vec<rusty_music_core::db::Critique>, String> {
    verrou(&etat.lib).critiques_pour_piste(id).map_err(echec)
}

/// Lien de sortie vers la page CritiqueBrainz de l'album du morceau `id` —
/// invite à en écrire une, même (surtout) quand `critiques_piste` est vide.
#[tauri::command(async)]
pub(crate) fn lien_ecrire_critique(etat: State<Etat>, id: i64) -> Result<Option<String>, String> {
    let rg = verrou(&etat.lib)
        .release_group_pour_piste(id)
        .map_err(echec)?;
    Ok(rg.map(|mbid| rusty_music_core::critiques::url_ecrire_critique(&mbid)))
}

/// Avancement de la passe de tags Last.fm, sondé par l'interface.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatLastfm {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    pub(crate) resultat: Option<String>,
}

/// Au-delà de combien de jours des tags redeviennent « à vérifier ».
pub(crate) const LASTFM_PEREMPTION_JOURS: i64 = 180;

/// Lance la passe de tags Last.fm — vote de nommage des familles
/// (`docs/nommage-familles.md`), pas la popularité. Clé personnelle exigée
/// (gratuite, sur last.fm/api) : contrairement aux autres sources, Last.fm
/// n'a pas de clé de test partagée.
#[tauri::command(async)]
pub(crate) fn start_lastfm(
    app: tauri::AppHandle,
    etat: State<Etat>,
    cle: String,
    rafraichir: bool,
) -> Result<(), String> {
    {
        let mut e = verrou(&etat.lastfm);
        if e.en_cours {
            return Err("une passe Last.fm est déjà en cours".into());
        }
        *e = EtatLastfm {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.lastfm);
        let client = rusty_music_core::lastfm::Client::new(cle);
        let depuis = if rafraichir {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64 - LASTFM_PEREMPTION_JOURS * 86_400)
                .unwrap_or(0)
        } else {
            0
        };
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            rusty_music_core::lastfm_pass::actualiser(&mut lib, &client, depuis, usize::MAX, |b| {
                let mut e = verrou(&etat.lastfm);
                e.faits = b.faits;
                e.total = b.total;
            })
            .map_err(|e| e.to_string())
        })();

        let bilan = match issue {
            Ok(b) => {
                let echecs = if b.echecs > 0 {
                    format!(", {} à reprendre plus tard", b.echecs)
                } else {
                    String::new()
                };
                format!(
                    "{} artistes interrogés, {} avec au moins un tag{echecs}",
                    b.artistes_interroges, b.artistes_avec_tags
                )
            }
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "passe Last.fm terminée");

        let mut fin = verrou(&etat.lastfm);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn lastfm_state(etat: State<Etat>) -> Result<EtatLastfm, String> {
    Ok(verrou(&etat.lastfm).clone())
}

/// Avancement de la passe de liaison Discogs, sondé par l'interface.
///
/// `liees` (éditions neuves reliées, pas simplement vérifiées) sert de
/// déclencheur à l'import automatique qui suit une liaison utile — voir
/// `importerDiscogsSiUtile` côté JS : inutile de relire les ~11 Go du dump
/// (~10 min) si la liaison n'a rien trouvé de nouveau.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatDiscogsLiaison {
    pub(crate) en_cours: bool,
    faits: usize,
    total: usize,
    liees: usize,
    pub(crate) resultat: Option<String>,
}

/// Lance la passe de liaison Discogs — retrouve l'édition Discogs de chaque
/// édition MusicBrainz connue (`tracks.mb_release_id`) par la relation d'URL
/// que porte MusicBrainz, jamais par recherche de nom. Légère (une requête
/// MusicBrainz par édition) : peut tourner à chaque enrichissement, contrairement
/// à l'import lourd du dump mensuel (CLI/cron, voir `docs/enrichissement-lecteur.md`).
#[tauri::command(async)]
pub(crate) fn start_discogs_liaison(
    app: tauri::AppHandle,
    etat: State<Etat>,
    contact: String,
) -> Result<(), String> {
    let contact = contact.trim().to_string();
    if !contact.contains('@') {
        return Err("MusicBrainz demande une adresse de contact valable.".into());
    }
    {
        let mut d = verrou(&etat.discogs_liaison);
        if d.en_cours {
            return Err("une passe de liaison Discogs est déjà en cours".into());
        }
        *d = EtatDiscogsLiaison {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.discogs_liaison);
        let client = rusty_music_core::musicbrainz::Client::new(&contact);
        let issue = (|| {
            let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
            rusty_music_core::discogs_import::lier(&mut lib, &client, usize::MAX, |b| {
                let mut e = verrou(&etat.discogs_liaison);
                e.faits = b.faits;
                e.total = b.total;
                e.liees = b.liees;
            })
            .map_err(|e| e.to_string())
        })();

        let bilan = match issue {
            Ok(b) => format!(
                "{} éditions vérifiées, {} reliées à Discogs",
                b.interrogees, b.liees
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "passe de liaison Discogs terminée");

        let mut fin = verrou(&etat.discogs_liaison);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn discogs_liaison_state(etat: State<Etat>) -> Result<EtatDiscogsLiaison, String> {
    Ok(verrou(&etat.discogs_liaison).clone())
}

/// Chemin persistant du dump Discogs téléchargé — à côté de la base, comme
/// `ville-paris.db` (`db.with_file_name`) : un fichier mensuel qu'on garde
/// d'un lancement à l'autre, pas un fichier temporaire — sinon « dernier
/// téléchargement » n'aurait pas de sens.
pub(crate) fn chemin_dump_discogs(etat: &Etat) -> PathBuf {
    etat.db.with_file_name("discogs-releases.xml.gz")
}

/// Avancement du téléchargement du dump Discogs, sondé par l'interface.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatDiscogsDump {
    pub(crate) en_cours: bool,
    octets: u64,
    total: Option<u64>,
    pub(crate) resultat: Option<String>,
}

/// Ce que dit le fichier déjà sur le disque, sans rien télécharger : sa
/// taille et la date de dernière écriture (= dernier téléchargement réussi).
#[derive(Clone, serde::Serialize)]
pub(crate) struct DumpDiscogsInfo {
    octets: u64,
    telecharge_le: i64,
}

/// `None` tant qu'aucun téléchargement n'a abouti — pas de valeur inventée.
#[tauri::command(async)]
pub(crate) fn discogs_dump_info(etat: State<Etat>) -> Result<Option<DumpDiscogsInfo>, String> {
    let chemin = chemin_dump_discogs(&etat);
    let Ok(m) = std::fs::metadata(&chemin) else {
        return Ok(None);
    };
    let telecharge_le = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(Some(DumpDiscogsInfo {
        octets: m.len(),
        telecharge_le,
    }))
}

/// Lance le téléchargement du dernier dump Discogs (`releases.xml.gz`, ≈ 11
/// Go) — remplace un fichier déjà là s'il y en avait un.
#[tauri::command(async)]
pub(crate) fn start_discogs_dump(app: tauri::AppHandle, etat: State<Etat>) -> Result<(), String> {
    {
        let mut d = verrou(&etat.discogs_dump);
        if d.en_cours {
            return Err("un téléchargement du dump Discogs est déjà en cours".into());
        }
        *d = EtatDiscogsDump {
            en_cours: true,
            ..Default::default()
        };
    }

    let chemin = chemin_dump_discogs(&etat);
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.discogs_dump);
        let issue =
            rusty_music_core::discogs_import::telecharger_dernier_dump(&chemin, |vus, total| {
                let mut d = verrou(&etat.discogs_dump);
                d.octets = vus;
                d.total = total;
            });

        let bilan = match issue {
            Ok(()) => {
                let go = std::fs::metadata(&chemin).map(|m| m.len()).unwrap_or(0) as f64 / 1e9;
                format!("dump téléchargé — {go:.1} Go")
            }
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "téléchargement du dump Discogs terminé");

        let mut fin = verrou(&etat.discogs_dump);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn discogs_dump_state(etat: State<Etat>) -> Result<EtatDiscogsDump, String> {
    Ok(verrou(&etat.discogs_dump).clone())
}

/// Avancement de l'import des crédits/labels depuis le dump Discogs déjà sur
/// le disque, sondé par l'interface. Pas de total connu à l'avance (combien
/// d'éditions du dump concernent la bibliothèque ne se sait qu'en le
/// parcourant) : `editions_vues` sert de seul repère de progression, comme
/// le fait déjà `EtatScan::morceaux` pour un scan.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatDiscogsImport {
    pub(crate) en_cours: bool,
    editions_vues: u64,
    editions_retenues: u64,
    fragments_malformes: u64,
    pub(crate) resultat: Option<String>,
}

/// Extrait crédits et labels du dump déjà téléchargé (voir
/// [`start_discogs_dump`]) vers `credits_discogs`/`labels_discogs`, pour les
/// éditions déjà reliées par la liaison Discogs.
///
/// **Sûr à rejouer** : `credits_poser`/`labels_poser` remplacent toujours ce
/// qui existait pour une édition, jamais un ajout — un import relancé
/// n'accumule rien, il rafraîchit. Exposée en commande Tauri comme les
/// autres passes de fond (popularité, biographies, critiques) plutôt que
/// réservée au CLI : ni plus ni moins risqué pour la base que celles-ci.
#[tauri::command(async)]
pub(crate) fn start_discogs_import(app: tauri::AppHandle, etat: State<Etat>) -> Result<(), String> {
    let chemin = chemin_dump_discogs(&etat);
    if !chemin.is_file() {
        return Err("Téléchargez d'abord le dump Discogs.".into());
    }
    if verrou(&etat.discogs_dump).en_cours {
        return Err("le téléchargement du dump est encore en cours".into());
    }
    {
        let mut i = verrou(&etat.discogs_import);
        if i.en_cours {
            return Err("un import du dump Discogs est déjà en cours".into());
        }
        *i = EtatDiscogsImport {
            en_cours: true,
            ..Default::default()
        };
    }

    let db = etat.db.clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.discogs_import);
        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|mut lib| {
                rusty_music_core::discogs_import::importer(&mut lib, &chemin, |b| {
                    let mut i = verrou(&etat.discogs_import);
                    i.editions_vues = b.editions_vues;
                    i.editions_retenues = b.editions_retenues;
                    i.fragments_malformes = b.fragments_malformes;
                })
                .map_err(|e| e.to_string())
            });

        let bilan = match issue {
            Ok(b) => format!(
                "{} éditions parcourues, {} retenues, {} fragments illisibles ignorés",
                b.editions_vues, b.editions_retenues, b.fragments_malformes
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "import du dump Discogs terminé");

        let mut fin = verrou(&etat.discogs_import);
        fin.en_cours = false;
        fin.resultat = Some(bilan);
    });
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn discogs_import_state(etat: State<Etat>) -> Result<EtatDiscogsImport, String> {
    Ok(verrou(&etat.discogs_import).clone())
}

/// Y a-t-il une édition reliée jamais importée ? Sert à décider si relancer
/// l'import vaut le coût après une liaison — voir `Library::discogs_import_utile`
/// : le critère n'est jamais « cette liaison-ci a-t-elle relié du neuf »,
/// pour rattraper aussi une édition reliée avant ce correctif, ou par un
/// import interrompu.
#[tauri::command(async)]
pub(crate) fn discogs_import_utile(etat: State<Etat>) -> Result<bool, String> {
    verrou(&etat.lib).discogs_import_utile().map_err(echec)
}

/// Les crédits Discogs du morceau `id`, via son édition MusicBrainz — vide
/// tant que la liaison et l'import mensuel n'ont pas encore couvert cette
/// édition, jamais une valeur inventée.
#[tauri::command(async)]
pub(crate) fn credits_piste(
    etat: State<Etat>,
    id: i64,
) -> Result<Vec<rusty_music_core::db::CreditDiscogs>, String> {
    verrou(&etat.lib).credits_pour_piste(id).map_err(echec)
}

/// Les labels (nom + numéro de catalogue) Discogs du morceau `id`, via son
/// édition MusicBrainz — vide tant que la liaison et l'import mensuel n'ont
/// pas encore couvert cette édition, jamais une valeur inventée.
#[tauri::command(async)]
pub(crate) fn labels_piste(
    etat: State<Etat>,
    id: i64,
) -> Result<Vec<rusty_music_core::db::LabelDiscogs>, String> {
    verrou(&etat.lib).labels_pour_piste(id).map_err(echec)
}

/// Les collaborateurs d'un artiste — mode Découvrir. Sert du cache s'il y
/// en a un, sinon interroge MusicBrainz et le remplit.
///
/// Pas de fil séparé ni de sondage comme `start_enrichment` : ici, une
/// seule requête réseau au plus (une seconde environ), pas une passe sur
/// toute la bibliothèque — inutile d'habiller ça d'un état de fond.
#[tauri::command(async)]
pub(crate) fn artist_links(
    etat: State<Etat>,
    mbid: String,
    contact: String,
) -> Result<Vec<(String, String, String)>, String> {
    {
        let lib = verrou(&etat.lib);
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

    let mut lib = verrou(&etat.lib);
    lib.enregistrer_liens_artiste(&mbid, &liens)
        .map_err(echec)?;
    lib.liens_artiste(&mbid).map_err(echec)
}

/// Démarre la surveillance continue de `root`, si elle ne l'est pas déjà.
///
/// Pose le watcher (immédiat) puis lance la boucle d'évènements dans son
/// propre fil, avec sa propre connexion — même raison que [`start_scan`] : ne
/// pas tenir `Etat::lib` pendant une opération qui dure toute la session.
/// Idempotent : sans effet si `root` est déjà surveillée, donc appelable sans
/// vérifier soi-même (au démarrage, après un scan réussi…).
pub(crate) fn demarrer_surveillance(etat: &Etat, root: &Path) {
    {
        let deja = verrou(&etat.surveillances);
        if deja.contains_key(root) {
            return;
        }
    }
    let (surveillance, rx) = match rusty_music_core::watch::demarrer(root) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(root = %root.display(), %e, "surveillance impossible");
            return;
        }
    };
    verrou(&etat.surveillances).insert(root.to_path_buf(), surveillance);
    tracing::info!(root = %root.display(), "surveillance démarrée");

    let db = etat.db.clone();
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        match Library::open(&db) {
            Ok(lib) => rusty_music_core::watch::boucle(&lib, rx),
            Err(e) => tracing::warn!(%e, "surveillance : base inaccessible"),
        }
        tracing::info!(root = %root.display(), "surveillance arrêtée");
    });
}

/// Oublie une racine **et les morceaux qui en dépendent**.
#[tauri::command(async)]
pub(crate) fn forget_root(etat: State<Etat>, path: String) -> Result<usize, String> {
    let n = verrou(&etat.lib)
        .remove_root(Path::new(&path))
        .map_err(echec)?;
    // Sans ça, un évènement filesystem reçu après coup remettrait le morceau
    // en base — la surveillance ne sait pas que l'utilisateur vient de dire
    // « oublie ». Abandonner le handle est le seul moyen de l'arrêter.
    verrou(&etat.surveillances).remove(Path::new(&path));
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
pub(crate) fn start_scan(
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
        let mut s = verrou(&etat.scan);
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
        // Posée avant `scan_root_jobs`, pas après : c'est là qu'est le
        // travail susceptible de paniquer (décodage des tags d'un fichier
        // arbitraire), et la garde ne protège que ce qu'elle a vu commencer.
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.scan);

        let issue = Library::open(&db)
            .map_err(|e| e.to_string())
            .and_then(|lib| {
                let jobs = coeurs_arriere_plan();
                rusty_music_core::scan::scan_root_jobs(&lib, &racine, jobs, force)
                    .map_err(|e| e.to_string())
            });

        let succes = issue.is_ok();
        let bilan = match issue {
            Ok(r) => format!(
                "{} vus · {} ingérés · {} inchangés · {} retirés · {} en échec",
                r.seen, r.inserted, r.skipped, r.removed, r.failed
            ),
            Err(e) => format!("échec : {e}"),
        };
        tracing::info!(%bilan, "scan terminé");

        // Une racine qui vient d'être scannée avec succès — nouvelle ou déjà
        // connue — doit être surveillée en continu ; `demarrer_surveillance`
        // ne fait rien si elle l'est déjà.
        if succes {
            demarrer_surveillance(&etat, &racine);
        }
        let mut s = verrou(&etat.scan);
        s.en_cours = false;
        s.resultat = Some(bilan);
    });

    Ok(())
}

/// Avancement du scan. L'interface sonde, faute de rapport intermédiaire.
#[tauri::command(async)]
pub(crate) fn scan_state(etat: State<Etat>) -> Result<EtatScan, String> {
    let mut s = verrou(&etat.scan).clone();
    if s.en_cours {
        // Le nombre de morceaux progresse pendant le scan : c'est la seule
        // mesure d'avancement qu'on puisse offrir sans instrumenter le cœur.
        s.morceaux = verrou(&etat.lib).count().map_err(echec)?;
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
pub(crate) async fn cover(etat: State<'_, Etat>, path: String) -> Result<Option<String>, String> {
    let dossier_cache = etat.db.with_file_name("pochettes");
    let db_path = etat.db.clone();
    tauri::async_runtime::spawn_blocking(move || cover_extraire(&dossier_cache, &db_path, &path))
        .await
        .map_err(echec)?
}

pub(crate) fn cover_extraire(
    dossier_cache: &Path,
    db_path: &Path,
    path: &str,
) -> Result<Option<String>, String> {
    let chemin = Path::new(path);

    // Sans métadonnées lisibles (fichier disparu, permission refusée), pas de
    // clé de cache fiable : on extrait directement, comme avant ce cache.
    let mtime = std::fs::metadata(chemin)
        .ok()
        .and_then(|m| m.modified().ok());
    let cle = mtime.map(|m| cle_pochette(chemin, m));

    if let Some(cle) = &cle {
        if let Some(valeur) = lire_cache_pochette(dossier_cache, cle) {
            return Ok(valeur);
        }
    }

    let cover = cover_locale(db_path, chemin);
    let (valeur, definitif) = match cover {
        Some(c) => {
            let mime = c.mime.as_deref().unwrap_or("image/jpeg");
            (
                Some(format!("data:{mime};base64,{}", base64(&c.data))),
                true,
            )
        }
        None => match cover_depuis_reseau(dossier_cache, db_path, chemin) {
            Repli::Trouvee(v) => (Some(v), true),
            Repli::Absente => (None, true),
            // Une panne réseau n'est pas une réponse : ne rien écrire, le
            // prochain affichage réessaiera.
            Repli::Indeterminee => (None, false),
        },
    };
    if definitif {
        if let Some(cle) = &cle {
            ecrire_cache_pochette(dossier_cache, cle, valeur.as_deref());
        }
    }
    Ok(valeur)
}

/// Combien de pistes sœurs tenter avant de passer au réseau. Sur la
/// bibliothèque de test, quand la première piste n'a pas d'image, la
/// deuxième la porte (4 albums sur 4) : huit est large sans relire un album
/// de 55 pistes en entier.
pub(crate) const PISTES_SOEURS_MAX: usize = 8;

/// La pochette locale d'un morceau : ses tags, son dossier, puis celles des
/// autres pistes de l'album — la grille n'en présente qu'une (la première par
/// chemin), et c'est parfois précisément celle qui n'a pas d'image.
///
/// Un fichier dont les tags ne se lisent pas compte comme « sans pochette »
/// plutôt que d'échouer : il passe alors au repli réseau au lieu de
/// disparaître en silence (l'interface avale l'erreur de `cover`).
pub(crate) fn cover_locale(db_path: &Path, chemin: &Path) -> Option<rusty_music_core::Cover> {
    // La base d'abord : elle porte le choix « Image du dossier » de
    // l'utilisateur. Illisible, on lit quand même les tags.
    let Ok(lib) = rusty_music_core::db::Library::open(db_path) else {
        return rusty_music_core::tags::read_cover(chemin).ok().flatten();
    };
    if let Ok(Some(c)) = lib.lire_pochette(chemin) {
        return Some(c);
    }
    lib.pistes_soeurs(chemin, PISTES_SOEURS_MAX)
        .ok()?
        .iter()
        .find_map(|p| lib.lire_pochette(p).ok().flatten())
}

/// Issue du repli réseau quand ni les tags ni le dossier n'ont de pochette.
pub(crate) enum Repli {
    Trouvee(String),
    /// Toutes les sources interrogées ont répondu « pas de pochette ».
    Absente,
    /// Au moins une source n'a pas pu répondre (panne réseau, base illisible).
    Indeterminee,
}

/// Réponse d'une source, avant agrégation.
pub(crate) enum Reponse {
    Image(String),
    Non,
    Panne,
}

/// Une source du repli, avec son cache **d'album** (`cle`, partagée par tous
/// les morceaux de l'album) : un hit ou un 404 déjà connu ne repart pas sur le
/// réseau ; une panne n'est jamais écrite.
pub(crate) fn source_de_pochette(
    dossier_cache: &Path,
    cle: &str,
    appel: impl FnOnce() -> rusty_music_core::error::Result<Option<Vec<u8>>>,
) -> Reponse {
    match lire_cache_pochette(dossier_cache, cle) {
        Some(Some(v)) => return Reponse::Image(v),
        Some(None) => return Reponse::Non,
        None => {}
    }
    match appel() {
        Ok(octets) => {
            let valeur = octets.map(|o| format!("data:image/jpeg;base64,{}", base64(&o)));
            ecrire_cache_pochette(dossier_cache, cle, valeur.as_deref());
            valeur.map_or(Reponse::Non, Reponse::Image)
        }
        Err(_) => Reponse::Panne,
    }
}

/// Clé de cache disque d'une pochette Deezer : par album (artiste et titre
/// normalisés), partagée par la grille d'Écouter et le fil Découvrir.
pub(crate) fn cle_pochette_deezer(artiste: &str, album: &str) -> String {
    format!(
        "dz-{:016x}",
        empreinte(
            format!(
                "{}\0{}",
                rusty_music_core::musicbrainz::cle_artiste(artiste),
                rusty_music_core::musicbrainz::normaliser_titre(album)
            )
            .as_bytes(),
        )
    )
}

/// Le client Deezer du processus : sa cadence (150 ms entre requêtes) n'a de
/// sens que partagée, la grille d'albums demande des dizaines de pochettes d'un
/// coup.
pub(crate) fn deezer() -> &'static rusty_music_core::deezer::Client {
    static CLIENT: std::sync::OnceLock<rusty_music_core::deezer::Client> =
        std::sync::OnceLock::new();
    CLIENT.get_or_init(rusty_music_core::deezer::Client::new)
}

/// Le client TheAudioDB (clé partagée) des photos d'artiste : sa cadence
/// (2,1 s entre requêtes, 30/min pour la clé partagée) n'a de sens que partagée
/// par tous les appels. Distinct du client de la passe « biographies », qui
/// porte la clé personnelle de l'utilisateur.
pub(crate) fn theaudiodb() -> &'static rusty_music_core::theaudiodb::Client {
    static CLIENT: std::sync::OnceLock<rusty_music_core::theaudiodb::Client> =
        std::sync::OnceLock::new();
    CLIENT.get_or_init(|| rusty_music_core::theaudiodb::Client::new(None))
}

/// Repli quand ni les tags ni le dossier n'ont de pochette (`docs/suite.md`,
/// dette « restent pochettes »), du plus fiable au plus flou :
///
/// 1. Cover Art Archive par **release** (`MUSICBRAINZ_ALBUMID` des tags) —
///    exacte, sans jointure de titre ;
/// 2. Cover Art Archive par **release-group** (artiste MBID + titre normalisé) ;
/// 3. Deezer par artiste + album, pour ce qui n'a aucun identifiant
///    MusicBrainz (~11 % de la bibliothèque) ou que CAA ne connaît pas.
///
/// Chaque étape ne passe à la suivante que sur « pas de pochette » ou panne.
/// Les clés de cache sont par album (`caar-`, `caa-` — partagée avec
/// `decouvrir_pochette` —, `dz-`) : un album nu n'est redemandé qu'une fois,
/// jamais par morceau. Rien n'est écrit dans la bibliothèque de l'utilisateur.
///
/// Ouvre sa propre connexion à la base plutôt que de faire remonter `Library`
/// jusqu'ici — même raison que le scan (voir le champ `db` de `Etat`).
pub(crate) fn cover_depuis_reseau(dossier_cache: &Path, db_path: &Path, chemin: &Path) -> Repli {
    let Ok(lib) = rusty_music_core::db::Library::open(db_path) else {
        return Repli::Indeterminee;
    };
    let release = lib.release_pour_path(chemin).ok().flatten();
    let rg_mbid = lib.release_group_pour_path(chemin).ok().flatten();
    let artiste_album = lib.artiste_album_pour_path(chemin).ok().flatten();
    drop(lib);

    let mut panne = false;
    let mut essayer = |reponse: Reponse| match reponse {
        Reponse::Image(v) => Some(v),
        Reponse::Non => None,
        Reponse::Panne => {
            panne = true;
            None
        }
    };

    if let Some(id) = &release {
        let r = source_de_pochette(dossier_cache, &format!("caar-{id}"), || {
            rusty_music_core::pochette::release(id)
        });
        if let Some(v) = essayer(r) {
            return Repli::Trouvee(v);
        }
    }
    if let Some(rg) = &rg_mbid {
        let r = source_de_pochette(dossier_cache, &format!("caa-{rg}"), || {
            rusty_music_core::pochette::release_group(rg)
        });
        if let Some(v) = essayer(r) {
            return Repli::Trouvee(v);
        }
    }
    if let Some((artiste, album)) = &artiste_album {
        let cle = cle_pochette_deezer(artiste, album);
        let r = source_de_pochette(dossier_cache, &cle, || {
            deezer().pochette_album(artiste, album)
        });
        if let Some(v) = essayer(r) {
            return Repli::Trouvee(v);
        }
    }
    if panne {
        Repli::Indeterminee
    } else {
        Repli::Absente
    }
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
pub(crate) async fn artist_covers(
    etat: State<'_, Etat>,
    name: String,
    mbid: Option<String>,
    max: usize,
) -> Result<Vec<String>, String> {
    let albums = {
        let lib = verrou(&etat.lib);
        lib.albums_of_artist(mbid.as_deref(), &name)
            .map_err(echec)?
    };
    // Borne dure : un artiste prolifique ne doit pas faire lire les tags de
    // cent albums pour une vignette qui n'en montre que quatre.
    let max = max.min(9);
    let dossier_cache = etat.db.with_file_name("pochettes");
    let db_path = etat.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut chemins: Vec<String> = Vec::new();
        for album in albums {
            if chemins.len() >= max {
                break;
            }
            if let Ok(Some(_)) = cover_extraire(&dossier_cache, &db_path, &album.path) {
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
pub(crate) const SANS_POCHETTE: &[u8] = b"\0";

/// Clé de cache : hachage FNV-1a (même construction que `db::hacher`) du
/// chemin **et** de la date de modification du fichier.
///
/// La mtime dans la clé, pas seulement le chemin, est ce qui rend
/// l'invalidation automatique : remplacer le fichier (nouveau tag,
/// ré-encodage) change sa mtime, donc sa clé — l'ancienne entrée reste sur
/// disque, inerte, plutôt que d'exiger une purge active.
pub(crate) fn cle_pochette(path: &Path, mtime: std::time::SystemTime) -> String {
    let epoch = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut octets = path.to_string_lossy().into_owned().into_bytes();
    octets.extend_from_slice(&epoch.to_le_bytes());
    format!("{:016x}", empreinte(&octets))
}

/// Hachage FNV-1a 64 bits.
pub(crate) fn empreinte(octets: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in octets {
        h ^= *octet as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Durée de validité d'une entrée « pas de pochette » : passé ce délai elle
/// est redemandée — CAA et Deezer s'enrichissent, et l'enrichissement
/// MusicBrainz local peut arriver après le premier affichage. Les pochettes
/// trouvées, elles, ne périment pas.
pub(crate) const NEGATIF_VALIDITE: Duration = Duration::from_secs(30 * 24 * 3600);

/// Lit une entrée du cache. `None` extérieur = pas en cache (à calculer),
/// `Some(None)` intérieur = en cache et confirmé sans pochette (depuis moins
/// de [`NEGATIF_VALIDITE`]).
pub(crate) fn lire_cache_pochette(dossier: &Path, cle: &str) -> Option<Option<String>> {
    lire_cache_pochette_avec(dossier, cle, NEGATIF_VALIDITE)
}

pub(crate) fn lire_cache_pochette_avec(
    dossier: &Path,
    cle: &str,
    validite: Duration,
) -> Option<Option<String>> {
    let chemin = dossier.join(cle);
    let octets = std::fs::read(&chemin).ok()?;
    if octets == SANS_POCHETTE {
        let age = std::fs::metadata(&chemin)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        // Âge illisible : mieux vaut redemander qu'enterrer l'album.
        return match age {
            Some(a) if a <= validite => Some(None),
            _ => None,
        };
    }
    Some(String::from_utf8(octets).ok())
}

/// Efface les entrées « pas de pochette » d'avant la distinction entre
/// « 404 » et « panne réseau » : elles peuvent être des pannes gravées.
/// Une seule fois (marqueur à côté du cache).
pub(crate) fn purger_negatifs_pochettes(dossier: &Path) {
    let marqueur = dossier.join(".negatifs-purges-v1");
    if marqueur.exists() {
        return;
    }
    if let Ok(entrees) = std::fs::read_dir(dossier) {
        for e in entrees.flatten() {
            if std::fs::read(e.path()).is_ok_and(|o| o == SANS_POCHETTE) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = std::fs::write(&marqueur, b"1");
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
pub(crate) fn ecrire_cache_pochette(dossier: &Path, cle: &str, valeur: Option<&str>) {
    if std::fs::create_dir_all(dossier).is_err() {
        return;
    }
    let _ = std::fs::write(
        dossier.join(cle),
        valeur.map_or(SANS_POCHETTE, str::as_bytes),
    );
}

/// Tempo, tonalité et énergie d'un morceau — ce que la passe a mesuré.
///
/// **Rend `None` plutôt qu'une valeur par défaut** quand le morceau n'est pas
/// mesuré : 15 847 des 27 044 le sont à ce jour, et afficher « 120 BPM » sur
/// les autres serait donner une mesure qu'on n'a pas.
#[tauri::command(async)]
pub(crate) fn descripteurs(
    etat: State<Etat>,
    id: i64,
) -> Result<Option<rusty_music_core::db::DescripteursVus>, String> {
    verrou(&etat.lib).descripteurs(id).map_err(echec)
}

/// La popularité générale des morceaux `ids` — `(track_id, relative 0..1,
/// echelon)`. Un morceau absent de la réponse n'en a pas (jauge grisée).
/// Chargée par lot pour ce qui est visible (file d'attente, liste de pistes),
/// comme les pochettes — jamais dans `TrackRow`.
#[tauri::command(async)]
pub(crate) fn popularites(
    etat: State<Etat>,
    ids: Vec<i64>,
) -> Result<Vec<(i64, f64, String)>, String> {
    verrou(&etat.lib).popularites(&ids).map_err(echec)
}

/// Qualité d'encodage du morceau en écoute — codec, débit, échantillonnage,
/// profondeur de bits. Affichée sous le compteur de temps du transport.
/// Champs partiellement `None` sur un morceau scanné avant la lecture du
/// format (un rescan les remplit).
#[tauri::command(async)]
pub(crate) fn qualite_piste(
    etat: State<Etat>,
    id: i64,
) -> Result<Option<rusty_music_core::db::QualitePiste>, String> {
    verrou(&etat.lib).qualite_piste(id).map_err(echec)
}

/// Encodage base64 sans dépendance : la seule chose qu'on ait à encoder ici,
/// ce sont des pochettes, et l'ajouter au projet pour ça ne se justifie pas.
pub(crate) fn base64(data: &[u8]) -> String {
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
