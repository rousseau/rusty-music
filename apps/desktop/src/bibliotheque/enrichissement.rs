// SPDX-License-Identifier: GPL-3.0-or-later
//! Enrichissement en ligne : MusicBrainz, popularité, biographies,
//! critiques, Last.fm, liens d'artiste, passe « Compléter ».

use super::*;

#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementEnrichissement {
    pub(crate) artistes: usize,
    pub(crate) total: usize,
    pub(crate) avec_genre: usize,
}

pub(crate) type EtatEnrichissement = Passe<AvancementEnrichissement>;

/// Avancement de la passe de popularité générale, sondé par l'interface.
pub(crate) type EtatPopularite = Passe<Compte>;

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

    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.enrichissement,
        "un enrichissement est déjà en cours",
        Default::default(),
        move |etat| {
            let client = rusty_music_enrichissement::musicbrainz::Client::new(&contact);
            // Ouverture et passe dans un même bloc plutôt qu'en chaîne : la
            // fermeture de progression emprunte `etat`, et l'imbriquer dans un
            // `and_then` ferait vivre cet emprunt au-delà du fil.
            let issue = (|| {
                let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
                let total = lib.mb_avancement().map(|(_, t, _)| t).unwrap_or(0) as usize;
                {
                    let mut e = verrou(&etat.enrichissement);
                    e.avancement.total = total;
                }
                rusty_music_enrichissement::enrichir::enrichir(&mut lib, &client, usize::MAX, |b| {
                    let mut e = verrou(&etat.enrichissement);
                    e.avancement.artistes = b.artistes;
                    e.avancement.avec_genre = b.avec_genre;
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
            bilan
        },
    )
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
    let db = etat.db.clone();
    let contact = contact.trim().to_string();
    passe::lancer(
        app,
        |e| &e.popularite,
        "une passe de popularité est déjà en cours",
        Default::default(),
        move |etat| {
            let lb = rusty_music_enrichissement::listenbrainz::Client::new(&contact);
            let dz = rusty_music_enrichissement::deezer::Client::new();
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
                rusty_music_enrichissement::popularite::actualiser(
                    &mut lib,
                    &lb,
                    &dz,
                    depuis,
                    usize::MAX,
                    |b| {
                        let mut p = verrou(&etat.popularite);
                        p.avancement.faits = b.faits;
                        p.avancement.total = b.total;
                    },
                )
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

            bilan
        },
    )
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
pub(crate) type EtatBio = Passe<Compte>;

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
    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.biographies,
        "une passe de biographies est déjà en cours",
        Default::default(),
        move |etat| {
            let client = rusty_music_enrichissement::theaudiodb::Client::new(cle.as_deref());
            let issue = (|| {
                let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
                rusty_music_enrichissement::biographies::actualiser(
                    &mut lib,
                    &client,
                    0,
                    usize::MAX,
                    |b| {
                        let mut e = verrou(&etat.biographies);
                        e.avancement.faits = b.faits;
                        e.avancement.total = b.total;
                    },
                )
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

            bilan
        },
    )
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
pub(crate) type EtatCritiques = Passe<Compte>;

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
    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.critiques,
        "une passe de critiques est déjà en cours",
        Default::default(),
        move |etat| {
            let client = rusty_music_enrichissement::critiquebrainz::Client::new();
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
                rusty_music_enrichissement::critiques::actualiser(
                    &mut lib,
                    &client,
                    depuis,
                    usize::MAX,
                    |b| {
                        let mut e = verrou(&etat.critiques);
                        e.avancement.faits = b.faits;
                        e.avancement.total = b.total;
                    },
                )
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

            bilan
        },
    )
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
    Ok(rg.map(|mbid| rusty_music_enrichissement::critiques::url_ecrire_critique(&mbid)))
}

/// Avancement de la passe de tags Last.fm, sondé par l'interface.
pub(crate) type EtatLastfm = Passe<Compte>;

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
    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.lastfm,
        "une passe Last.fm est déjà en cours",
        Default::default(),
        move |etat| {
            let client = rusty_music_enrichissement::lastfm::Client::new(cle);
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
                rusty_music_enrichissement::lastfm_pass::actualiser(
                    &mut lib,
                    &client,
                    depuis,
                    usize::MAX,
                    |b| {
                        let mut e = verrou(&etat.lastfm);
                        e.avancement.faits = b.faits;
                        e.avancement.total = b.total;
                    },
                )
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

            bilan
        },
    )
}

#[tauri::command(async)]
pub(crate) fn lastfm_state(etat: State<Etat>) -> Result<EtatLastfm, String> {
    Ok(verrou(&etat.lastfm).clone())
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
    let client = rusty_music_enrichissement::musicbrainz::Client::new(&contact);
    let liens = client.relations_artiste(&mbid).map_err(|e| e.to_string())?;

    let mut lib = verrou(&etat.lib);
    lib.enregistrer_liens_artiste(&mbid, &liens)
        .map_err(echec)?;
    lib.liens_artiste(&mbid).map_err(echec)
}

/// Avancement de la passe « Compléter ».
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementCompleter {
    pub(crate) etape: String,
    pub(crate) faits: usize,
    pub(crate) total: usize,
}

pub(crate) type EtatCompleter = Passe<AvancementCompleter>;

/// Artistes interrogés entre deux lectures de la demande d'arrêt : une
/// vingtaine de secondes chez MusicBrainz (deux requêtes par artiste).
const TRANCHE_COMPLETER: usize = 10;

/// Ce que « Compléter » interrogerait : la phrase d'avant le geste.
#[derive(serde::Serialize)]
pub(crate) struct ApercuCompleter {
    musicbrainz: usize,
    lastfm: usize,
    secondes: u64,
}

#[tauri::command(async)]
pub(crate) fn completer_apercu(
    etat: State<Etat>,
    contact: String,
    cle_lastfm: String,
) -> Result<ApercuCompleter, String> {
    let lib = verrou(&etat.lib);
    let musicbrainz = if contact.contains('@') {
        lib.mb_artistes_en_attente("artist", usize::MAX)
            .map_err(echec)?
            .len()
    } else {
        0
    };
    let lastfm = if cle_lastfm.trim().is_empty() {
        0
    } else {
        lib.lastfm_candidats(0, usize::MAX).map_err(echec)?.len()
    };
    Ok(ApercuCompleter {
        musicbrainz,
        lastfm,
        // MusicBrainz : deux requêtes par artiste à une par seconde ;
        // Last.fm : une toutes les 300 ms.
        secondes: musicbrainz as u64 * 2 + lastfm as u64 * 3 / 10,
    })
}

/// Lance « Compléter » : les genres MusicBrainz des artistes en attente, puis
/// leurs tags Last.fm si une clé est posée — ce qui existe déjà, enchaîné, sur
/// une connexion dont le journal note chaque écriture
/// (`Library::ouvrir_passe`). Arrêtable entre deux tranches ; annulable en
/// bloc une fois finie ou arrêtée ([`completer_annuler`]).
#[tauri::command(async)]
pub(crate) fn completer_lancer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    contact: String,
    cle_lastfm: String,
) -> Result<(), String> {
    let contact = contact.trim().to_string();
    let cle_lastfm = cle_lastfm.trim().to_string();
    if !contact.contains('@') && cle_lastfm.is_empty() {
        return Err(
            "il faut une adresse de contact MusicBrainz ou une clé Last.fm (Options de la chaîne)"
                .into(),
        );
    }
    if verrou(&etat.enrichissement).en_cours || verrou(&etat.lastfm).en_cours {
        return Err("une passe de genres ou de tags tourne déjà".into());
    }
    etat.completer_arret.store(false, Ordering::Relaxed);
    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.completer,
        "« Compléter » tourne déjà",
        AvancementCompleter {
            etape: "Préparation".into(),
            ..Default::default()
        },
        move |etat| {
            let etape = |texte: &str, faits: usize, total: usize| {
                let mut e = verrou(&etat.completer);
                e.avancement.etape = texte.to_string();
                e.avancement.faits = faits;
                e.avancement.total = total;
            };
            let arret = || etat.completer_arret.load(Ordering::Relaxed);
            let mut lib = match Library::open(&db) {
                Ok(l) => l,
                Err(e) => return format!("échec : {e}"),
            };
            let id = match lib.ouvrir_passe("completer") {
                Ok(id) => id,
                Err(e) => return format!("échec : {e}"),
            };
            let issue = (|| -> Result<String, String> {
                let mut bouts = Vec::new();
                if contact.contains('@') {
                    let client = rusty_music_enrichissement::musicbrainz::Client::new(&contact);
                    let total = lib
                        .mb_artistes_en_attente("artist", usize::MAX)
                        .map_err(echec)?
                        .len();
                    let (mut faits, mut avec_genre, mut albums) = (0, 0, 0);
                    etape("Genres MusicBrainz", 0, total);
                    while !arret() {
                        let b = rusty_music_enrichissement::enrichir::enrichir(
                            &mut lib,
                            &client,
                            TRANCHE_COMPLETER,
                            |b| etape("Genres MusicBrainz", faits + b.artistes + b.echecs, total),
                        )
                        .map_err(echec)?;
                        faits += b.artistes;
                        avec_genre += b.avec_genre;
                        albums += b.albums;
                        // Rien d'interrogé avec succès : plus personne en
                        // attente, ou seulement des artistes qui échouent.
                        if b.artistes == 0 {
                            break;
                        }
                    }
                    bouts.push(format!(
                        "{faits} artistes chez MusicBrainz, {avec_genre} avec un genre, {albums} albums"
                    ));
                }
                if !cle_lastfm.is_empty() && !arret() {
                    let client =
                        rusty_music_enrichissement::lastfm::Client::new(cle_lastfm.clone());
                    let total = lib.lastfm_candidats(0, usize::MAX).map_err(echec)?.len();
                    let (mut faits, mut avec_tags) = (0, 0);
                    etape("Tags Last.fm", 0, total);
                    while !arret() {
                        let b = rusty_music_enrichissement::lastfm_pass::actualiser(
                            &mut lib,
                            &client,
                            0,
                            TRANCHE_COMPLETER,
                            |b| etape("Tags Last.fm", faits + b.faits, total),
                        )
                        .map_err(echec)?;
                        faits += b.faits - b.echecs;
                        avec_tags += b.artistes_avec_tags;
                        if b.faits == b.echecs {
                            break;
                        }
                    }
                    bouts.push(format!(
                        "{faits} artistes chez Last.fm, {avec_tags} avec des tags"
                    ));
                }
                let mut bilan = bouts.join(" ; ");
                if arret() {
                    bilan = format!("arrêtée : {bilan}");
                }
                Ok(bilan)
            })();
            let bilan = issue.unwrap_or_else(|e| format!("échec : {e}"));
            if let Err(e) = lib.fermer_passe(id, &bilan) {
                tracing::warn!(erreur = %e, "journal de « Compléter » non fermé");
            }
            tracing::info!(%bilan, "passe « Compléter » terminée");
            bilan
        },
    )
}

/// Demande l'arrêt de « Compléter » : pris en compte à la fin de la tranche
/// en cours. Ce qui a été écrit reste au journal, donc annulable.
#[tauri::command]
pub(crate) fn completer_arreter(etat: State<Etat>) {
    etat.completer_arret.store(true, Ordering::Relaxed);
}

#[derive(serde::Serialize)]
pub(crate) struct EtatCompleterVue {
    passe: EtatCompleter,
    /// La dernière passe journalisée, pour « Annuler cette passe ».
    derniere: Option<rusty_music_core::db::PasseJournal>,
}

#[tauri::command(async)]
pub(crate) fn completer_etat(etat: State<Etat>) -> Result<EtatCompleterVue, String> {
    Ok(EtatCompleterVue {
        passe: verrou(&etat.completer).clone(),
        derniere: verrou(&etat.lib)
            .derniere_passe("completer")
            .map_err(echec)?,
    })
}

/// Annule en bloc la dernière passe « Compléter » : rend le nombre de lignes
/// rétablies.
#[tauri::command(async)]
pub(crate) fn completer_annuler(etat: State<Etat>, id: i64) -> Result<usize, String> {
    if verrou(&etat.completer).en_cours {
        return Err("arrêtez d'abord la passe en cours".into());
    }
    let n = verrou(&etat.lib).annuler_passe(id).map_err(echec)?;
    tracing::info!(passe = id, lignes = n, "passe « Compléter » annulée");
    Ok(n)
}
