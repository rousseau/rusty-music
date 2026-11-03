// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de Valise : la valise du compagnon iOS, préparée et envoyée d'ici.

use super::*;

/// Avancement de l'envoi d'une valise, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementEnvoiValise {
    /// L'étape en cours, en mots.
    pub(crate) etape: String,
    pub(crate) faits: usize,
    pub(crate) total: usize,
}
pub(crate) type EtatEnvoiValise = Passe<AvancementEnvoiValise>;

/// Avancement de la réinstallation de l'app compagnon (construction Xcode
/// puis installation) : la dernière ligne du script, pour montrer que ça vit.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementRenouvellement {
    pub(crate) ligne: String,
}
pub(crate) type EtatRenouvellement = Passe<AvancementRenouvellement>;

/// Les réglages gardés de la dernière valise.
#[tauri::command(async)]
pub(crate) fn valise_reglages(
    etat: State<Etat>,
) -> Result<rusty_music_core::valise::Reglages, String> {
    verrou(&etat.lib).reglages_valise().map_err(echec)
}

#[derive(serde::Serialize)]
pub(crate) struct ApercuValise {
    albums: Vec<rusty_music_core::valise::AlbumValise>,
    octets: u64,
    budget_octets: u64,
}

/// Ce que `reglages` mettrait dans la valise. Les réglages sont gardés au
/// passage : l'aperçu est ce qu'on enverra.
#[tauri::command(async)]
pub(crate) fn valise_apercu(
    etat: State<Etat>,
    mut reglages: rusty_music_core::valise::Reglages,
) -> Result<ApercuValise, String> {
    reglages.modele = rusty_music_analysis::passe::MODELE.to_string();
    let lib = verrou(&etat.lib);
    lib.definir_reglages_valise(&reglages).map_err(echec)?;
    let sel = lib.selectionner_valise(&reglages).map_err(echec)?;
    Ok(ApercuValise {
        octets: sel.octets(),
        budget_octets: reglages.budget_octets,
        albums: sel.albums,
    })
}

/// Ce que le Mac sait de l'iPhone.
#[derive(Default, serde::Serialize)]
pub(crate) struct EtatIphone {
    /// Xcode est là : sans lui, ni envoi ni renouvellement.
    xcode: bool,
    appareils: Vec<iphone::Appareil>,
    /// L'appareil retenu (celui demandé, sinon le premier).
    udid: Option<String>,
    app_installee: bool,
    /// Expiration du profil de signature (ISO 8601).
    echeance: Option<String>,
    valises: Vec<iphone::ValiseTelephone>,
    /// Le script de réinstallation est là (machine de développement).
    renouvelable: bool,
    /// Ce qui n'a pas pu être lu, en mots.
    pub(crate) erreur: Option<String>,
}

/// Interroge l'iPhone : plusieurs appels à `devicectl`, une à quelques
/// secondes en Wi-Fi.
#[tauri::command(async)]
pub(crate) fn valise_iphone(udid: Option<String>) -> EtatIphone {
    let mut e = EtatIphone {
        xcode: iphone::xcode_present(),
        renouvelable: iphone::script_reinstaller().is_some(),
        ..Default::default()
    };
    if !e.xcode {
        return e;
    }
    e.appareils = match iphone::appareils() {
        Ok(a) => a,
        Err(err) => {
            e.erreur = Some(err);
            return e;
        }
    };
    let Some(choisi) = udid
        .filter(|u| e.appareils.iter().any(|a| &a.udid == u))
        .or_else(|| e.appareils.first().map(|a| a.udid.clone()))
    else {
        return e;
    };
    e.udid = Some(choisi.clone());
    match iphone::app_installee(&choisi) {
        Ok(oui) => e.app_installee = oui,
        Err(err) => {
            e.erreur = Some(format!(
                "iPhone injoignable — déverrouillé, sur le même Wi-Fi ? ({err})"
            ));
            return e;
        }
    }
    if !e.app_installee {
        return e;
    }
    e.echeance = iphone::echeance(&choisi).unwrap_or_else(|err| {
        e.erreur = Some(err);
        None
    });
    e.valises = iphone::valises(&choisi).unwrap_or_else(|err| {
        e.erreur = Some(err);
        Vec::new()
    });
    e
}

/// Envoie une nouvelle valise sur l'iPhone, selon les réglages gardés :
///
/// 1. reprend les écoutes des valises déjà sur le téléphone (`journal.json`) ;
/// 2. prépare la valise dans les données de l'app ;
/// 3. la dépose dans les Documents de l'app compagnon, sous `nom` ;
/// 4. liste les anciennes valises dans `a-retirer.json` — l'app les supprime
///    (devicectl ne sait pas le faire) — seulement celles dont les écoutes
///    sont reprises ;
/// 5. efface la copie locale.
#[tauri::command(async)]
pub(crate) fn valise_envoyer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    udid: String,
    nom: String,
) -> Result<(), String> {
    if nom.is_empty() || nom.contains(['/', '\\']) || nom.starts_with('.') {
        return Err(format!("nom de valise invalide : « {nom} »"));
    }
    let db = etat.db.clone();
    let app_wifi = app.clone();
    let depart = AvancementEnvoiValise {
        etape: "Lecture de l'iPhone".into(),
        ..Default::default()
    };
    passe::lancer(
        app,
        |e| &e.valise_envoi,
        "un envoi est déjà en cours",
        depart,
        move |etat| {
            let etape = |texte: &str, faits: usize, total: usize| {
                let mut e = verrou(&etat.valise_envoi);
                e.avancement.etape = texte.to_string();
                e.avancement.faits = faits;
                e.avancement.total = total;
            };
            let issue = (|| -> Result<String, String> {
                use rusty_music_core::valise;
                let lib = Library::open(&db).map_err(echec)?;
                let travail = db
                    .parent()
                    .map_or_else(std::env::temp_dir, Path::to_path_buf)
                    .join("valise-envoi");
                let _ = std::fs::remove_dir_all(&travail);
                std::fs::create_dir_all(&travail).map_err(echec)?;

                // 1. Les écoutes du téléphone, avant de remplacer quoi que ce soit.
                let anciennes = iphone::valises(&udid)?;
                let mut retirables = Vec::new();
                let mut ecoutes = 0;
                for (i, v) in anciennes.iter().enumerate() {
                    etape("Reprise des écoutes du téléphone", i, anciennes.len());
                    if v.journal {
                        let local = travail.join(format!("journal-{i}.json"));
                        // Un journal illisible garde sa valise : ses écoutes
                        // seraient perdues avec elle.
                        let Ok(()) = iphone::copier_depuis(
                            &udid,
                            &format!("{}/{}", v.nom, valise::JOURNAL),
                            &local,
                        ) else {
                            continue;
                        };
                        let Ok(journal) = valise::lire_journal(&local) else {
                            continue;
                        };
                        ecoutes += lib.importer_journal(&journal).map_err(echec)?.nouvelles;
                    }
                    if v.nom != nom {
                        retirables.push(v.nom.clone());
                    }
                }

                // 2. La valise, dans les données de l'app.
                let mut reglages = lib.reglages_valise().map_err(echec)?;
                reglages.modele = rusty_music_analysis::passe::MODELE.to_string();
                let sel = lib.selectionner_valise(&reglages).map_err(echec)?;
                if sel.albums.is_empty() {
                    return Err("la sélection est vide".into());
                }
                let dossier = travail.join(&nom);
                let rapport = valise::exporter(&lib, &sel, &dossier, |i, n, _| {
                    etape("Préparation de la valise", i, n);
                })
                .map_err(echec)?;

                // 3. Le transfert : pas d'avancement, devicectl n'en donne pas.
                etape("Envoi vers l'iPhone", 0, 0);
                iphone::copier_vers(&udid, &dossier, &nom)?;

                // 4. Les anciennes valises, à retirer par l'app.
                if !retirables.is_empty() {
                    let liste = travail.join(iphone::A_RETIRER);
                    std::fs::write(&liste, serde_json::to_vec(&retirables).map_err(echec)?)
                        .map_err(echec)?;
                    iphone::copier_vers(&udid, &liste, iphone::A_RETIRER)?;
                }

                // 5. Ménage, et l'appairage Wi-Fi au passage (`wifi.rs`).
                let _ = std::fs::remove_dir_all(&travail);
                if let Err(e) = wifi::appairer(&app_wifi, etat, &udid) {
                    tracing::warn!(erreur = %e, "appairage Wi-Fi impossible");
                }

                let mut msg = format!(
                    "« {nom} » envoyée : {} albums, {:.1} Go",
                    rapport.albums,
                    rapport.octets as f64 / (1024.0 * 1024.0 * 1024.0)
                );
                if ecoutes > 0 {
                    msg += &format!(" ; {ecoutes} écoutes du téléphone reprises");
                }
                if !retirables.is_empty() {
                    msg += &format!(
                        " ; {} retirée(s) au prochain lancement de l'app",
                        retirables.join(", ")
                    );
                }
                if !rapport.manquants.is_empty() {
                    msg += &format!(" ; {} fichier(s) introuvable(s)", rapport.manquants.len());
                }
                Ok(msg)
            })();
            issue.unwrap_or_else(|err| format!("échec : {err}"))
        },
    )
}

/// Reconstruit et réinstalle l'app compagnon (`scripts/reinstaller-valise.sh`)
/// — quelques minutes, sur une machine qui a le dépôt et Xcode.
#[tauri::command(async)]
pub(crate) fn valise_renouveler(app: tauri::AppHandle, udid: String) -> Result<(), String> {
    let script = iphone::script_reinstaller()
        .ok_or("script de réinstallation introuvable (il faut le dépôt des sources)")?;
    let depart = AvancementRenouvellement {
        ligne: "Construction…".into(),
    };
    let deja = "une réinstallation est déjà en cours";
    passe::lancer(
        app,
        |e| &e.valise_renouvellement,
        deja,
        depart,
        move |etat| {
            use std::io::BufRead;
            // Lancée depuis le Finder, l'app n'a pas le PATH du terminal : cargo
            // et Homebrew n'y sont pas.
            let home = std::env::var("HOME").unwrap_or_default();
            let path = format!(
                "{home}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:{}",
                std::env::var("PATH").unwrap_or_default()
            );
            let issue = (|| -> Result<String, String> {
                let mut enfant = std::process::Command::new("/bin/bash")
                    .arg("-c")
                    .arg("\"$0\" \"$1\" 2>&1")
                    .arg(&script)
                    .arg(&udid)
                    .env("PATH", path)
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .map_err(echec)?;
                let mut dernieres: std::collections::VecDeque<String> = Default::default();
                if let Some(sortie) = enfant.stdout.take() {
                    for ligne in std::io::BufReader::new(sortie)
                        .lines()
                        .map_while(Result::ok)
                    {
                        let ligne = ligne.trim().to_string();
                        if ligne.is_empty() {
                            continue;
                        }
                        verrou(&etat.valise_renouvellement).avancement.ligne = ligne.clone();
                        dernieres.push_back(ligne);
                        if dernieres.len() > 4 {
                            dernieres.pop_front();
                        }
                    }
                }
                let statut = enfant.wait().map_err(echec)?;
                let fin = dernieres.back().cloned().unwrap_or_default();
                if statut.success() {
                    Ok(fin.trim_start_matches(['─', ' ']).to_string())
                } else {
                    Err(Vec::from(dernieres).join(" · "))
                }
            })();
            issue.unwrap_or_else(|err| format!("échec : {err}"))
        },
    )
}

#[derive(serde::Serialize)]
pub(crate) struct EtatValise {
    envoi: EtatEnvoiValise,
    renouvellement: EtatRenouvellement,
}

/// Où en sont l'envoi et la réinstallation.
#[tauri::command]
pub(crate) fn valise_etat(etat: State<Etat>) -> EtatValise {
    EtatValise {
        envoi: verrou(&etat.valise_envoi).clone(),
        renouvellement: verrou(&etat.valise_renouvellement).clone(),
    }
}

/// Ce que le passage a emporté, pour la phrase de l'interface.
#[derive(serde::Serialize)]
pub(crate) struct LectureEmportee {
    pistes: usize,
    titre: Option<String>,
    position_ms: u64,
    /// Le Mac s'est mis en pause. `false` quand la lecture a changé pendant
    /// le dépôt, ou par le Wi-Fi tant que l'iPhone ne l'a pas prise.
    mac_en_pause: bool,
    /// Par le Wi-Fi (`wifi.rs`) : la lecture attend que l'iPhone la prenne.
    par_wifi: bool,
    /// Pourquoi le câble n'a pas servi, quand il a été essayé.
    cable_echec: Option<String>,
}

/// Emporte la lecture sur l'iPhone (`rusty_music_core::passage`).
///
/// Par le câble d'abord (option A), si `udid` est donné : la file et la
/// position sont déposées dans les Documents de l'app par `devicectl`, puis
/// le Mac se met en pause — **après** le dépôt confirmé, pour qu'un échec
/// laisse la musique où elle était ; l'iPhone est appairé au passage pour le
/// Wi-Fi. Sinon, ou si le câble échoue, par le Wi-Fi (option B) : le passage
/// attend que l'iPhone vienne le prendre, et le Mac ne se tait qu'à son
/// accusé. Le téléphone reprend en pause, à la position lue au départ :
/// quelques secondes plus tôt, jamais plus tard.
#[tauri::command(async)]
pub(crate) fn emporter_lecture(
    app: tauri::AppHandle,
    etat: State<Etat>,
    udid: Option<String>,
) -> Result<LectureEmportee, String> {
    use rusty_music_core::passage::{Passage, FICHIER, VERSION};
    let Some(mut i) = verrou(&etat.player).instantane() else {
        return Err("rien en lecture".into());
    };
    // Pendant le fondu d'un minuteur, c'est le volume d'avant qui compte
    // (comme `sauvegarder_session`).
    if let Some(nominal) = verrou(&etat.minuteur)
        .as_ref()
        .and_then(minuteur::Minuteur::volume_nominal)
    {
        i.volume = nominal;
    }
    let courant = i.file.get(i.rang).cloned();
    let chemins =
        |v: &[PathBuf]| -> Vec<String> { v.iter().map(|p| p.display().to_string()).collect() };
    let passage = Passage {
        version: VERSION,
        emis_le: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64),
        file: chemins(&i.file),
        avant_melange: chemins(&i.avant_melange),
        rang: i.rang,
        position_ms: i.position.as_millis() as u64,
        alea: i.alea,
        repetition: i.repetition.nom().to_string(),
        volume: i.volume,
    };
    let titre = courant.as_ref().and_then(|c| {
        verrou(&etat.lib)
            .pistes_par_chemins(&[c.display().to_string()])
            .ok()?
            .into_iter()
            .next()?
            .title
    });
    let emportee = |mac_en_pause, par_wifi, cable_echec| LectureEmportee {
        pistes: passage.file.len(),
        titre: titre.clone(),
        position_ms: passage.position_ms,
        mac_en_pause,
        par_wifi,
        cable_echec,
    };

    let mut cable_echec = None;
    if let Some(udid) = udid.as_deref() {
        let local = etat
            .db
            .parent()
            .map_or_else(std::env::temp_dir, Path::to_path_buf)
            .join(FICHIER);
        passage.ecrire(&local).map_err(echec)?;
        let depot = iphone::copier_vers(udid, &local, FICHIER);
        let _ = std::fs::remove_file(&local);
        match depot {
            Ok(()) => {
                wifi::retirer_attente(&etat);
                if let Err(e) = wifi::appairer(&app, &etat, udid) {
                    tracing::warn!(erreur = %e, "appairage Wi-Fi impossible");
                }
                let player = verrou(&etat.player);
                let mac_en_pause = player.current() == courant.as_deref();
                if mac_en_pause {
                    player.pause();
                }
                tracing::info!(
                    pistes = passage.file.len(),
                    mac_en_pause,
                    "lecture emportée sur l'iPhone par le câble"
                );
                return Ok(emportee(mac_en_pause, false, None));
            }
            Err(e) => cable_echec = Some(e),
        }
    }
    if wifi::mettre_en_attente(&etat, passage.clone()) {
        tracing::info!(
            pistes = passage.file.len(),
            "lecture en attente pour l'iPhone, par le Wi-Fi"
        );
        return Ok(emportee(false, true, cable_echec));
    }
    Err(cable_echec.unwrap_or_else(|| "aucun iPhone branché ni appairé".into()))
}
