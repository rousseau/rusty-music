// SPDX-License-Identifier: GPL-3.0-or-later
//! Les modèles de l'application : lesquels sont là, d'où ils viennent, et
//! leur téléchargement (Bibliothèque → Modèles).
//!
//! Le paquet ne livre que les petits poids (Pratiquer hors ligne, ADTOF qui
//! n'a pas de source) ; les gros arrivent du réseau. Ceux sans lesquels
//! l'application ne fait pas son travail de base (les deux tours de CLAP :
//! empreintes de la bibliothèque, recherche en mots) sont téléchargés **dès
//! le premier lancement** ([`demarrer`]) ; les autres au premier usage, comme
//! avant, ou à la main depuis la liste.
//!
//! Tous les téléchargements passent par `rusty_music_core::modeles` : un
//! poids demandé ici pendant qu'une passe le télécharge déjà n'est pas
//! téléchargé deux fois, et une analyse lancée avant l'arrivée de CLAP
//! l'attend au lieu d'échouer.

use super::*;
use rusty_music_core::modeles::{self as cm, Provenance};

/// Un fichier de poids et, quand il en a une, sa source en ligne.
#[derive(Clone, Copy)]
struct Fichier {
    nom: &'static str,
    url: Option<&'static str>,
    octets: u64,
    sha256: Option<&'static str>,
}

/// Un modèle tel que la liste le présente : un ou plusieurs fichiers.
struct Entree {
    id: &'static str,
    libelle: &'static str,
    usage: &'static str,
    fichiers: Vec<Fichier>,
    /// Téléchargé dès le premier lancement s'il manque.
    au_demarrage: bool,
    /// Pourquoi la liste ne propose pas de le télécharger, quand c'est le cas.
    remarque: Option<&'static str>,
}

fn fichier(nom: &'static str, url: &'static str, octets: u64, sha256: &'static str) -> Fichier {
    Fichier {
        nom,
        url: Some(url),
        octets,
        sha256: Some(sha256),
    }
}

/// Tous les modèles que l'application peut demander.
fn catalogue() -> Vec<Entree> {
    use rusty_music_analysis::poids as clap;
    use rusty_music_editor::pulsation::{Modele as Pulsation, MEL};
    use rusty_music_editor::Variante;
    use rusty_music_transcription as tr;

    let clap = |p: clap::PoidsEnLigne| fichier(p.nom, p.url, p.octets, p.sha256);
    let tr = |p: tr::Poids| fichier(p.nom, p.url, p.octets, p.sha256);
    let pulsation =
        |p: rusty_music_editor::pulsation::Poids| fichier(p.nom, p.url, p.octets, p.sha256);
    // Les URL des variantes de démixage sont construites : on les fige ici
    // pour la durée du processus.
    let demix = |v: Variante| Fichier {
        nom: v.fichier(),
        url: Some(Box::leak(v.url().into_boxed_str())),
        octets: v.octets(),
        sha256: v.sha256(),
    };

    let mut v = vec![
        Entree {
            id: "clap-audio",
            libelle: "CLAP, écoute",
            usage: "Empreintes des morceaux : carte, familles, voisins, Lama",
            fichiers: vec![clap(clap::AUDIO)],
            au_demarrage: true,
            remarque: None,
        },
        Entree {
            id: "clap-texte",
            libelle: "CLAP, mots",
            usage: "Recherche en mots dans Explorer et Lama",
            fichiers: vec![clap(clap::TEXTE)],
            au_demarrage: true,
            remarque: None,
        },
        Entree {
            id: "htdemucs",
            libelle: "HTDemucs",
            usage: "Démixage en 4 stems (Pratiquer, Créer)",
            fichiers: vec![demix(Variante::Standard)],
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "htdemucs-6s",
            libelle: "HTDemucs, 6 stems",
            usage: "Démixage avec guitare et piano",
            fichiers: vec![demix(Variante::SixStems)],
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "htdemucs-ft",
            libelle: "HTDemucs affiné",
            usage: "Démixage plus fin, quatre fois plus lent",
            fichiers: vec![demix(Variante::Affinee)],
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "roformer-sw",
            libelle: "BS-RoFormer SW",
            usage: "Démixage de qualité en 6 stems",
            fichiers: vec![demix(Variante::RoformerSw)],
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "beat-this",
            libelle: "Beat This!",
            usage: "Temps et mesures (petit modèle)",
            fichiers: vec![pulsation(MEL), pulsation(Pulsation::Petit.poids())],
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "beat-this-complet",
            libelle: "Beat This!, complet",
            usage: "Temps et mesures, un peu plus justes",
            fichiers: vec![pulsation(Pulsation::Complet.poids())],
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "basic-pitch",
            libelle: "Basic Pitch",
            usage: "Transcription de la basse",
            fichiers: vec![tr(tr::basic_pitch::POIDS)],
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "all-in-one",
            libelle: "All-in-one",
            usage: "Sections d'un morceau (couplet, refrain…)",
            fichiers: tr::sections::POIDS.iter().map(|p| tr(*p)).collect(),
            au_demarrage: false,
            remarque: None,
        },
        Entree {
            id: "adtof",
            libelle: "ADTOF",
            usage: "Transcription de la batterie",
            fichiers: vec![Fichier {
                nom: tr::batterie::FICHIER,
                url: None,
                octets: 1_811_789,
                sha256: None,
            }],
            au_demarrage: false,
            remarque: Some("livré avec l'application, sans source en ligne"),
        },
        Entree {
            id: "muscriptor",
            libelle: "MuScriptor",
            usage: "Transcription de qualité de la basse",
            fichiers: vec![Fichier {
                nom: tr::muscriptor::FICHIER,
                url: None,
                octets: tr::muscriptor::OCTETS,
                sha256: None,
            }],
            au_demarrage: false,
            remarque: Some(
                "conditions à accepter sur Hugging Face : se télécharge depuis Pratiquer",
            ),
        },
        Entree {
            id: "aero",
            libelle: "AERO",
            usage: "Super-résolution, bouton « HD »",
            fichiers: vec![fichier(
                rusty_music_superres::POIDS_FICHIER,
                rusty_music_superres::POIDS_URL,
                rusty_music_superres::POIDS_OCTETS,
                rusty_music_superres::POIDS_SHA256,
            )],
            au_demarrage: false,
            remarque: None,
        },
    ];
    if cfg!(feature = "lama-local") {
        v.push(Entree {
            id: "gemma",
            libelle: "Gemma 4 E4B",
            usage: "Lama sans Ollama : les demandes en mots",
            fichiers: vec![fichier(
                explorer::modele_local::FICHIER,
                explorer::modele_local::URL,
                explorer::modele_local::OCTETS,
                explorer::modele_local::SHA256,
            )],
            au_demarrage: false,
            remarque: None,
        });
    }
    v
}

/// Un modèle, vu de l'interface.
#[derive(serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct ModeleEtat {
    id: &'static str,
    libelle: &'static str,
    usage: &'static str,
    /// Taille de tous ses fichiers, en octets.
    octets: u64,
    /// `telecharge`, `paquet` (livré avec l'application), `autre` (dossier du
    /// dépôt ou de `RUSTY_MUSIC_MODELS`), `partiel` ou `absent`.
    etat: &'static str,
    /// Octets occupés dans le dossier de l'application, ceux que
    /// « Supprimer » libère.
    octets_supprimables: u64,
    telechargeable: bool,
    au_demarrage: bool,
    remarque: Option<&'static str>,
}

fn etat_de(e: &Entree) -> ModeleEtat {
    let trouves: Vec<Option<std::path::PathBuf>> =
        e.fichiers.iter().map(|f| cm::trouver(f.nom)).collect();
    let presents = trouves.iter().flatten().count();
    let provenances: Vec<Provenance> = trouves
        .iter()
        .flatten()
        .map(|p| cm::provenance(p))
        .collect();
    let etat = if presents == 0 {
        "absent"
    } else if presents < e.fichiers.len() {
        "partiel"
    } else if provenances.iter().all(|p| *p == Provenance::Paquet) {
        "paquet"
    } else if provenances.iter().all(|p| *p == Provenance::Autre) {
        "autre"
    } else {
        "telecharge"
    };
    let octets_supprimables = trouves
        .iter()
        .flatten()
        .filter(|p| cm::provenance(p) == Provenance::Telecharge)
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    ModeleEtat {
        id: e.id,
        libelle: e.libelle,
        usage: e.usage,
        octets: e.fichiers.iter().map(|f| f.octets).sum(),
        etat,
        octets_supprimables,
        telechargeable: e.fichiers.iter().all(|f| f.url.is_some()),
        au_demarrage: e.au_demarrage,
        remarque: e.remarque,
    }
}

/// La liste des modèles et leur état.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn modeles_lister() -> Vec<ModeleEtat> {
    catalogue().iter().map(etat_de).collect()
}

/// Où en est le téléchargement : le modèle en cours, ses octets.
#[derive(Clone, Default, serde::Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct AvancementModeles {
    /// Identifiant du modèle en cours.
    id: Option<String>,
    libelle: Option<String>,
    recus: u64,
    total: u64,
    /// Modèles restants dans la file, celui en cours compris.
    restants: usize,
}

pub(crate) type EtatModeles = Passe<AvancementModeles>;

#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) fn modeles_telechargement_state(etat: State<Etat>) -> EtatModeles {
    verrou(&etat.modeles).clone()
}

/// Télécharge les modèles demandés, l'un après l'autre, ce qui manque
/// seulement.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn modeles_telecharger(app: tauri::AppHandle, ids: Vec<String>) -> Result<(), String> {
    let entrees: Vec<Entree> = catalogue()
        .into_iter()
        .filter(|e| ids.iter().any(|i| i == e.id))
        .collect();
    if let Some(e) = entrees
        .iter()
        .find(|e| e.fichiers.iter().any(|f| f.url.is_none()))
    {
        return Err(format!("{} ne se télécharge pas d'ici", e.libelle));
    }
    telecharger_entrees(app, entrees)
}

fn telecharger_entrees(app: tauri::AppHandle, entrees: Vec<Entree>) -> Result<(), String> {
    let restants = entrees.len();
    passe::lancer(
        app,
        |e| &e.modeles,
        "un téléchargement de modèles est déjà en cours",
        AvancementModeles {
            restants,
            ..Default::default()
        },
        move |etat| {
            let mut echecs = Vec::new();
            for (i, e) in entrees.iter().enumerate() {
                {
                    let mut p = verrou(&etat.modeles);
                    p.avancement = AvancementModeles {
                        id: Some(e.id.into()),
                        libelle: Some(e.libelle.into()),
                        recus: 0,
                        total: e.fichiers.iter().map(|f| f.octets).sum(),
                        restants: restants - i,
                    };
                }
                // Les octets des fichiers déjà arrivés de ce modèle.
                let mut deja = 0u64;
                for f in &e.fichiers {
                    if cm::trouver(f.nom).is_some() {
                        deja += f.octets;
                        continue;
                    }
                    let Some(url) = f.url else { continue };
                    let issue =
                        cm::telecharger(f.nom, url, f.octets / 10 * 9, f.sha256, |vus, _| {
                            verrou(&etat.modeles).avancement.recus = deja + vus;
                        });
                    match issue {
                        Ok(_) => deja += f.octets,
                        Err(err) => {
                            tracing::warn!(modele = e.id, fichier = f.nom, error = %err, "téléchargement impossible");
                            echecs.push(format!("{} : {err}", e.libelle));
                            break;
                        }
                    }
                }
            }
            if echecs.is_empty() {
                format!(
                    "{restants} modèle{} prêt{}",
                    if restants > 1 { "s" } else { "" },
                    if restants > 1 { "s" } else { "" }
                )
            } else {
                format!("échec : {}", echecs.join(" ; "))
            }
        },
    )
}

/// Supprime les fichiers d'un modèle téléchargés dans le dossier de
/// l'application (jamais ceux du paquet). Rend les octets libérés.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn modeles_supprimer(id: String) -> Result<u64, String> {
    let e = catalogue()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("modèle inconnu : {id}"))?;
    let mut liberes = 0;
    for f in &e.fichiers {
        liberes += cm::supprimer(f.nom).map_err(|err| err.to_string())?;
    }
    tracing::info!(modele = e.id, octets = liberes, "modèle supprimé");
    Ok(liberes)
}

/// Au lancement : télécharge en fond les modèles requis qui manquent.
/// Silencieux quand tout est là ; sans réseau, la passe échoue et la liste
/// le dit, l'application marche sans (l'analyse attendra un prochain essai).
pub(crate) fn demarrer(app: tauri::AppHandle) {
    let manquants: Vec<Entree> = catalogue()
        .into_iter()
        .filter(|e| e.au_demarrage && e.fichiers.iter().any(|f| cm::trouver(f.nom).is_none()))
        .collect();
    if manquants.is_empty() {
        return;
    }
    tracing::info!(
        modeles = ?manquants.iter().map(|e| e.id).collect::<Vec<_>>(),
        "premier lancement : téléchargement des modèles requis"
    );
    if let Err(e) = telecharger_entrees(app, manquants) {
        tracing::warn!(error = %e, "téléchargement des modèles requis non lancé");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_identifiants_sont_uniques() {
        let c = catalogue();
        let mut ids: Vec<_> = c.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.len());
    }

    /// Ce qui se télécharge au démarrage a une source et une empreinte, et
    /// n'est plus dans le paquet : c'est tout l'intérêt.
    #[test]
    fn les_modeles_du_demarrage_viennent_du_reseau() {
        let ressources: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let ressources = ressources["bundle"]["resources"].as_object().unwrap();
        for e in catalogue().iter().filter(|e| e.au_demarrage) {
            for f in &e.fichiers {
                assert!(f.url.is_some() && f.sha256.is_some(), "{}", f.nom);
                assert!(
                    !ressources.contains_key(&format!("../../models/{}", f.nom)),
                    "{} : téléchargé au démarrage, il n'a rien à faire dans le paquet",
                    f.nom
                );
            }
        }
    }

    /// Tout poids livré dans le paquet figure dans la liste.
    #[test]
    fn la_liste_couvre_le_paquet() {
        let ressources: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let noms: Vec<&str> = catalogue()
            .iter()
            .flat_map(|e| e.fichiers.iter().map(|f| f.nom))
            .collect();
        for (_, dest) in ressources["bundle"]["resources"].as_object().unwrap() {
            if let Some(nom) = dest.as_str().unwrap().strip_prefix("models/") {
                assert!(noms.contains(&nom), "{nom} manque à la liste des modèles");
            }
        }
    }
}
