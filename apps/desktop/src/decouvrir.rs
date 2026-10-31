// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de Découvrir : le fil des sorties et sa passe.

use super::*;

/// Avancement de la passe du mode Découvrir, sondé par l'interface — comme
/// `enrichment_state`.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementDecouvrir {
    pub(crate) artistes: usize,
    pub(crate) total: usize,
    pub(crate) sorties_neuves: usize,
    pub(crate) voisins_neufs: usize,
}
pub(crate) type EtatDecouvrir = Passe<AvancementDecouvrir>;

/// Le fil du mode Découvrir, tel que la base le tient.
///
/// Lecture seule : `start_decouvrir` fait le travail réseau. La fenêtre est
/// fixée à un mois — c'est l'esprit du mode, une actualité.
#[tauri::command(async)]
pub(crate) fn decouvrir_feed(
    etat: State<Etat>,
) -> Result<rusty_music_core::db::FilDecouvrir, String> {
    verrou(&etat.lib).decouvrir_fil(30).map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn decouvrir_state(etat: State<Etat>) -> Result<EtatDecouvrir, String> {
    Ok(verrou(&etat.decouvrir).clone())
}

/// Marque tout le fil comme vu — les pastilles « nouveau » s'éteignent.
#[tauri::command(async)]
pub(crate) fn decouvrir_tout_vu(etat: State<Etat>) -> Result<(), String> {
    verrou(&etat.lib).decouvrir_tout_vu().map_err(echec)
}

// ── Mode Valise : la valise du compagnon iOS, préparée et envoyée d'ici ──
// Sélection : `rusty_music_core::valise`. iPhone : `iphone.rs` (devicectl).
// Voir `docs/plan-ios-compagnon.md`.

/// La pochette d'une sortie du fil Découvrir, en `data:` URI — comme `cover`,
/// mais servie depuis Cover Art Archive plutôt que des tags d'un fichier local.
///
/// Cache disque partagé avec les pochettes locales (`<données app>/pochettes/`,
/// clés `caa-<mbid>` puis `dz-…`, les mêmes que le repli de `cover`) : une
/// vignette déjà récupérée ne repart pas sur le réseau, et un album sans
/// pochette (fréquent le mois de sa sortie) n'est pas redemandé. Le cache
/// mémoire côté interface fait le reste pendant la session.
///
/// Cover Art Archive d'abord ; si elle n'a rien, Deezer (recherche par
/// artiste et titre) — les sorties toutes fraîches y sont souvent avant d'être
/// sur CAA. `Err` = panne réseau, jamais écrite : l'interface pourra redemander.
#[tauri::command]
pub(crate) async fn decouvrir_pochette(
    etat: State<'_, Etat>,
    rg_mbid: String,
    artiste: String,
    titre: String,
) -> Result<Option<String>, String> {
    // L'identifiant vient du fil, mais on le vérifie : il sert de nom de
    // fichier de cache et part dans une URL.
    if rg_mbid.len() != 36 || !rg_mbid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("identifiant de release-group invalide".into());
    }
    let dossier_cache = etat.db.with_file_name("pochettes");
    tauri::async_runtime::spawn_blocking(move || {
        let mut panne = false;
        let r = source_de_pochette(&dossier_cache, &format!("caa-{rg_mbid}"), || {
            rusty_music_core::pochette::release_group(&rg_mbid)
        });
        match r {
            Reponse::Image(v) => return Ok(Some(v)),
            Reponse::Non => {}
            Reponse::Panne => panne = true,
        }
        let r = source_de_pochette(
            &dossier_cache,
            &cle_pochette_deezer(&artiste, &titre),
            || deezer().pochette_album(&artiste, &titre),
        );
        match r {
            Reponse::Image(v) => Ok(Some(v)),
            Reponse::Non if !panne => Ok(None),
            Reponse::Non | Reponse::Panne => {
                Err("pochette indisponible (panne réseau)".to_string())
            }
        }
    })
    .await
    .map_err(echec)?
}

/// La photo d'un artiste voisin du fil Découvrir, en `data:` URI — les voisins
/// n'ont qu'un nom et un MBID, aucune pochette.
///
/// Deezer d'abord (par nom, avec garde-fous d'identité : voir
/// `deezer::url_photo_artiste`), rapide ; puis, si Deezer n'en a pas,
/// TheAudioDB par MBID (fiable, mais une requête toutes les 2 s). Deux clés de
/// cache distinctes, pour qu'un « non » de Deezer n'écrase pas la seconde
/// source. `Ok(None)` : aucune des deux n'a de photo — l'interface montre des
/// initiales. `Err` : panne réseau, jamais écrite, à redemander.
#[tauri::command]
pub(crate) async fn decouvrir_photo_artiste(
    etat: State<'_, Etat>,
    mbid: String,
    nom: String,
) -> Result<Option<String>, String> {
    if mbid.len() != 36 || !mbid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("identifiant d'artiste invalide".into());
    }
    let dossier_cache = etat.db.with_file_name("pochettes");
    tauri::async_runtime::spawn_blocking(move || {
        let mut panne = false;
        let cle_deezer = format!(
            "dza-{:016x}",
            empreinte(rusty_music_core::musicbrainz::cle_artiste(&nom).as_bytes())
        );
        match source_de_pochette(&dossier_cache, &cle_deezer, || deezer().photo_artiste(&nom)) {
            Reponse::Image(v) => return Ok(Some(v)),
            Reponse::Non => {}
            Reponse::Panne => panne = true,
        }
        match source_de_pochette(&dossier_cache, &format!("tadb-{mbid}"), || {
            theaudiodb().vignette_par_mbid(&mbid)
        }) {
            Reponse::Image(v) => Ok(Some(v)),
            Reponse::Non if !panne => Ok(None),
            Reponse::Non | Reponse::Panne => Err("photo indisponible (panne réseau)".to_string()),
        }
    })
    .await
    .map_err(echec)?
}

/// L'adresse de la page Deezer d'un album (`titre` donné) ou d'un artiste
/// (`titre` absent) du fil Découvrir — le bouton « Deezer » d'une carte, qui
/// l'ouvre dans le navigateur. Même recherche et mêmes règles de concordance
/// que pour les pochettes et les photos.
///
/// Résolue au clic, sans cache disque : une requête isolée. `Ok(None)` : Deezer
/// ne connaît pas (l'interface se rabat sur sa page de recherche). `Err` :
/// panne réseau.
#[tauri::command]
pub(crate) async fn decouvrir_lien_deezer(
    artiste: String,
    titre: Option<String>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || match titre {
        Some(titre) => deezer().lien_album(&artiste, &titre),
        None => deezer().lien_artiste(&artiste),
    })
    .await
    .map_err(echec)?
    .map_err(echec)
}

/// Lance l'actualisation du fil Découvrir : sorties récentes (ListenBrainz
/// `fresh-releases`, une requête) puis artistes similaires (un appel par
/// artiste de la bibliothèque, cadencé).
///
/// Comme `start_enrichment` : déclenchée à la main (ou par l'interface à
/// l'ouverture du mode si la dernière passe est ancienne), sur son propre fil,
/// avec sa propre connexion. Additive et reprenable.
#[tauri::command(async)]
pub(crate) fn start_decouvrir(
    app: tauri::AppHandle,
    etat: State<Etat>,
    contact: String,
) -> Result<(), String> {
    let contact = contact.trim().to_string();
    if !contact.contains('@') {
        return Err("Une adresse de contact valable est demandée pour interroger les API.".into());
    }

    let db = etat.db.clone();
    passe::lancer(
        app,
        |e| &e.decouvrir,
        "une actualisation est déjà en cours",
        Default::default(),
        move |etat| {
            let lb = rusty_music_core::listenbrainz::Client::new(&contact);
            let issue = (|| {
                let mut lib = Library::open(&db).map_err(|e| e.to_string())?;
                rusty_music_core::decouvrir::actualiser(&mut lib, &lb, 30, 0, |b| {
                    let mut d = verrou(&etat.decouvrir);
                    d.avancement.artistes = b.artistes;
                    d.avancement.total = b.total;
                    d.avancement.sorties_neuves = b.sorties_neuves;
                    d.avancement.voisins_neufs = b.voisins_neufs;
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

            bilan
        },
    )
}

// ---------------------------------------------------------------------------
// Mode Éditer : démixage (module 3)
// ---------------------------------------------------------------------------
