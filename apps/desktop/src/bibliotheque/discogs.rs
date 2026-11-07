// SPDX-License-Identifier: GPL-3.0-or-later
//! Discogs : liaison, téléchargement et import du dump, crédits et labels
//! d'un morceau.

use super::*;

/// Avancement de la passe de liaison Discogs, sondé par l'interface.
///
/// `liees` (éditions neuves reliées, pas simplement vérifiées) sert de
/// déclencheur à l'import automatique qui suit une liaison utile — voir
/// `importerDiscogsSiUtile` côté JS : inutile de relire les ~11 Go du dump
/// (~10 min) si la liaison n'a rien trouvé de nouveau.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementDiscogsLiaison {
    pub(crate) faits: usize,
    pub(crate) total: usize,
    pub(crate) liees: usize,
}

pub(crate) type EtatDiscogsLiaison = Passe<AvancementDiscogsLiaison>;

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

    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.discogs_liaison,
        "une passe de liaison Discogs est déjà en cours",
        Default::default(),
        move |etat| {
            let client = rusty_music_enrichissement::musicbrainz::Client::new(&contact);
            let issue = (|| {
                let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
                rusty_music_enrichissement::discogs_import::lier(
                    &mut lib,
                    &client,
                    usize::MAX,
                    |b| {
                        let mut e = verrou(&etat.discogs_liaison);
                        e.avancement.faits = b.faits;
                        e.avancement.total = b.total;
                        e.avancement.liees = b.liees;
                    },
                )
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

            bilan
        },
    )
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
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementDiscogsDump {
    pub(crate) octets: u64,
    pub(crate) total: Option<u64>,
}

pub(crate) type EtatDiscogsDump = Passe<AvancementDiscogsDump>;

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
    let chemin = chemin_dump_discogs(&etat);
    passe::lancer(
        app,
        |e| &e.discogs_dump,
        "un téléchargement du dump Discogs est déjà en cours",
        Default::default(),
        move |etat| {
            let issue = rusty_music_enrichissement::discogs_import::telecharger_dernier_dump(
                &chemin,
                |vus, total| {
                    let mut d = verrou(&etat.discogs_dump);
                    d.avancement.octets = vus;
                    d.avancement.total = total;
                },
            );

            let bilan = match issue {
                Ok(()) => {
                    let go = std::fs::metadata(&chemin).map(|m| m.len()).unwrap_or(0) as f64 / 1e9;
                    format!("dump téléchargé — {go:.1} Go")
                }
                Err(e) => format!("échec : {e}"),
            };
            tracing::info!(%bilan, "téléchargement du dump Discogs terminé");

            bilan
        },
    )
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
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementDiscogsImport {
    pub(crate) editions_vues: u64,
    pub(crate) editions_retenues: u64,
    pub(crate) fragments_malformes: u64,
}

pub(crate) type EtatDiscogsImport = Passe<AvancementDiscogsImport>;

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

    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.discogs_import,
        "un import du dump Discogs est déjà en cours",
        Default::default(),
        move |etat| {
            let issue = Library::open(&db)
                .map_err(|e| e.to_string())
                .and_then(|mut lib| {
                    rusty_music_enrichissement::discogs_import::importer(&mut lib, &chemin, |b| {
                        let mut i = verrou(&etat.discogs_import);
                        i.avancement.editions_vues = b.editions_vues;
                        i.avancement.editions_retenues = b.editions_retenues;
                        i.avancement.fragments_malformes = b.fragments_malformes;
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

            bilan
        },
    )
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
