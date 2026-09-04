// SPDX-License-Identifier: GPL-3.0-or-later
//! La passe du mode Découvrir : interroge ListenBrainz, remplit le fil
//! d'actualité (nouveaux disques, collaborations, artistes voisins).
//!
//! Même esprit qu'[`crate::enrichir`] : elle relie deux modules qui ne se
//! parlent pas — le client réseau ([`crate::listenbrainz`]) et le rangement
//! ([`crate::db`]) — et c'est tout ce qu'elle contient : l'ordre des opérations
//! et le tri de ce qui revient.
//!
//! **Deux étapes, deux coûts.**
//! - **Sorties** : *un seul* appel à `fresh-releases` de ListenBrainz rend les
//!   sorties récentes de toute la planète ; on les croise avec la bibliothèque.
//!   Rapide, et sans le piège d'« interroger toute la discographie d'un
//!   artiste » — « Various Artists » à lui seul est crédité sur des dizaines de
//!   milliers de disques.
//! - **Voisins** : `similar-artists`, un appel par artiste. Le voisinage bouge
//!   lentement (suivi périmé à 30 jours) et on plafonne le nombre d'artistes
//!   traités par passe.
//!
//! **Additive et reprenable.** Chaque artiste traité pour ses voisins est marqué
//! en base dans la même transaction que ses données ; l'étape sorties pose sa
//! propre marque. Une passe coupée ne refait rien et ne perd rien. Une
//! bibliothèque qui n'a jamais vu le réseau a simplement un fil vide.

use std::collections::{HashMap, HashSet};

use crate::db::{Library, SortieARanger};
use crate::error::Result;
use crate::listenbrainz::{self, SortieFraiche};
use crate::musicbrainz::completer_date;

/// Types primaires MusicBrainz qu'on garde : les vraies sorties. Un
/// « Broadcast » ou un « Other » n'est pas une nouveauté d'artiste.
const TYPES_GARDES: &[&str] = &["Album", "EP", "Single"];

/// Types secondaires qui disqualifient une sortie — une réédition, un live, une
/// compilation ne sont pas une actualité même quand leur date est récente.
const TYPES_EXCLUS: &[&str] = &[
    "Compilation",
    "Live",
    "Remix",
    "DJ-mix",
    "Mixtape/Street",
    "Interview",
    "Audiobook",
    "Audio drama",
    "Demo",
];

/// Artistes spéciaux de MusicBrainz : des fourre-tout, pas des groupes.
/// « Various Artists » est crédité sur des dizaines de milliers de
/// compilations ; les autres (`[unknown]`, `[traditional]`…) sont du même
/// ordre. Liste publiée par MusicBrainz (special purpose artists).
const ARTISTES_SPECIAUX: &[&str] = &[
    "89ad4ac3-39f7-470e-963a-56509c546377", // Various Artists
    "125ec42a-7229-4250-afc5-e057484327fe", // [unknown]
    "f731ccc4-e22a-43af-a747-64213329e088", // [anonymous]
    "33cf029c-63b0-41a0-9855-be2a3665fb3b", // [data]
    "9be7f096-97ec-4615-8957-8d40b5dcbc41", // [traditional]
    "164f0d73-1234-4e2c-8743-d77bf2191051", // [dialogue]
];

/// Combien de voisins garder par artiste de la bibliothèque.
const VOISINS_PAR_ARTISTE: usize = 10;

/// Combien de sorties garder par artiste et par passe. Certains artistes
/// publient par salves — une douzaine d'EP le même jour — et noieraient le fil.
const SORTIES_PAR_ARTISTE: usize = 4;

/// Plafond d'artistes interrogés pour leurs voisins en une passe. Les appels
/// ListenBrainz coûtent une seconde chacun ; 60, ce sont les artistes les mieux
/// représentés, et de quoi tenir une première passe en une minute.
const PLAFOND_VOISINS: usize = 60;

/// Péremption du suivi des voisins, en jours — bien plus longue que celle des
/// sorties : la similarité entre artistes ne change pas d'une semaine à l'autre.
const PEREMPTION_VOISINS_JOURS: i64 = 30;

/// Au-delà de cette ancienneté (jours), une sortie quitte le fil et la table.
const GARDER_JOURS: i64 = 365;

/// Ce qu'une passe a produit.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BilanDecouvrir {
    /// Étapes franchies (sorties + un par artiste-voisin), pour l'avancement.
    pub artistes: usize,
    /// Total à franchir, connu dès le départ.
    pub total: usize,
    pub sorties_neuves: usize,
    pub voisins_neufs: usize,
    /// Étapes abandonnées après échec réseau — reprises à la prochaine passe.
    pub echecs: usize,
}

/// Actualise le fil du mode Découvrir.
///
/// `fenetre_jours` : l'âge maximal d'une sortie pour figurer au fil (≈ 30).
/// `limite` : plafond d'artistes-voisins par passe, 0 = le plafond par défaut.
///
/// `avancer` est appelé après chaque étape : la première passe interroge
/// ListenBrainz artiste par artiste pour les voisins, une requête par seconde.
pub fn actualiser(
    lib: &mut Library,
    lb: &listenbrainz::Client,
    fenetre_jours: i64,
    limite: usize,
    mut avancer: impl FnMut(&BilanDecouvrir),
) -> Result<BilanDecouvrir> {
    let bibliotheque = lib.artist_mbids()?;
    let noms = lib.artist_noms()?;
    let aujourdhui = lib.date_il_y_a(0)?;
    let borne = lib.date_il_y_a(fenetre_jours)?;

    let plafond = if limite == 0 { PLAFOND_VOISINS } else { limite.min(PLAFOND_VOISINS) };
    let a_faire_voisins: Vec<(String, String)> = lib
        .decouvrir_en_attente("voisins", PEREMPTION_VOISINS_JOURS, plafond)?
        .into_iter()
        .filter(|(mbid, _)| !ARTISTES_SPECIAUX.contains(&mbid.as_str()))
        .collect();

    let mut bilan = BilanDecouvrir {
        // +1 pour l'étape « sorties » (une requête, pas un artiste).
        total: a_faire_voisins.len() + 1,
        ..Default::default()
    };
    tracing::info!(voisins = a_faire_voisins.len(), "passe Découvrir : démarrage");
    avancer(&bilan);

    // --- 1. Sorties : une requête ListenBrainz, croisée avec la bibliothèque ---
    match lb.sorties_fraiches(fenetre_jours.max(1) as u32) {
        Ok(mut fraiches) => {
            // Les plus récentes d'abord : quand on plafonne un artiste prolifique,
            // c'est ses dernières sorties qu'on garde, pas les premières de la
            // fenêtre.
            fraiches.sort_by(|a, b| b.date_sortie.cmp(&a.date_sortie));
            let mut vus = HashSet::new();
            let mut par_artiste: HashMap<String, usize> = HashMap::new();
            for f in &fraiches {
                let Some(rg) = f.rg_mbid.clone() else { continue };
                if !vus.insert(rg) {
                    continue;
                }
                let Some((ancre, nom, sortie)) =
                    retenir(f, &bibliotheque, &noms, &borne, &aujourdhui)
                else {
                    continue;
                };
                let compte = par_artiste.entry(ancre.clone()).or_default();
                if *compte >= SORTIES_PAR_ARTISTE {
                    continue;
                }
                *compte += 1;
                if lib.decouvrir_ajouter_sortie(&ancre, &nom, &sortie)? {
                    bilan.sorties_neuves += 1;
                }
            }
            lib.decouvrir_marquer_passe("sorties")?;
            tracing::info!(
                vues = fraiches.len(),
                neuves = bilan.sorties_neuves,
                "passe Découvrir : sorties"
            );
        }
        Err(e) => {
            tracing::warn!(erreur = %e, "sorties fraîches ListenBrainz : échec");
            bilan.echecs += 1;
        }
    }
    bilan.artistes += 1;
    avancer(&bilan);

    // --- 2. Voisins, artiste par artiste -------------------------------------
    for (mbid, nom) in &a_faire_voisins {
        let mut voisins: Vec<(String, String, f64, String)> = match lb.artistes_similaires(mbid) {
            Ok(v) => v
                .into_iter()
                .filter(|w| !bibliotheque.contains(&w.mbid))
                .take(VOISINS_PAR_ARTISTE)
                .map(|w| (w.mbid, w.nom, w.score, "listenbrainz".to_string()))
                .collect(),
            Err(e) => {
                tracing::warn!(artiste = %nom, erreur = %e, "voisins non interrogés");
                bilan.echecs += 1;
                bilan.artistes += 1;
                avancer(&bilan);
                continue;
            }
        };

        // Repli : le graphe de collaboration déjà en base, aucun réseau.
        if voisins.is_empty() {
            voisins = lib
                .liens_artiste(mbid)?
                .into_iter()
                .filter(|(dst, _, _)| !bibliotheque.contains(dst))
                .map(|(dst, nom, _rel)| (dst, nom, 0.0, "collab".to_string()))
                .collect();
        }

        bilan.voisins_neufs += lib.decouvrir_poser_voisins(mbid, &voisins)?;
        bilan.artistes += 1;
        avancer(&bilan);
    }

    lib.decouvrir_elaguer(GARDER_JOURS)?;
    tracing::info!(
        sorties = bilan.sorties_neuves,
        voisins = bilan.voisins_neufs,
        echecs = bilan.echecs,
        "passe Découvrir : terminée"
    );
    Ok(bilan)
}

/// Décide si une sortie fraîche ListenBrainz mérite le fil, et la met en forme.
///
/// Il faut : au moins un artiste crédité dans la bibliothèque (et pas un artiste
/// spécial), un type primaire retenu, aucun type secondaire exclu, une date
/// dans `[borne, aujourdhui]`. Rend `(mbid ancre, nom ancre, sortie)`.
fn retenir(
    f: &SortieFraiche,
    bibliotheque: &HashSet<String>,
    noms: &HashMap<String, String>,
    borne: &str,
    aujourdhui: &str,
) -> Option<(String, String, SortieARanger)> {
    let rg_mbid = f.rg_mbid.clone()?;
    let ancre = f
        .artistes_mbids
        .iter()
        .find(|m| bibliotheque.contains(*m) && !ARTISTES_SPECIAUX.contains(&m.as_str()))
        .cloned()?;

    if !type_retenu(f.type_primaire.as_deref(), &f.types_secondaires) {
        return None;
    }
    let norm = completer_date(f.date_sortie.as_deref()?)?;
    if norm.as_str() < borne || norm.as_str() > aujourdhui {
        return None;
    }

    let nom = noms
        .get(&ancre)
        .cloned()
        .or_else(|| f.artistes.first().cloned())
        .unwrap_or_else(|| ancre.clone());

    // ListenBrainz ne détaille pas le crédit : un `artist_credit_name` qui
    // porte plusieurs artistes (« X feat. Y ») marque la collaboration, le
    // libellé entier tient lieu de liste.
    let collaborateurs = (f.artistes_mbids.len() > 1)
        .then(|| f.artistes.first().cloned())
        .flatten()
        .filter(|c| !c.is_empty());

    Some((
        ancre,
        nom,
        SortieARanger {
            rg_mbid,
            titre: f.titre.clone(),
            date_sortie: f.date_sortie.clone(),
            date_sortie_norm: Some(norm),
            type_primaire: f.type_primaire.clone(),
            types_secondaires: joindre(&f.types_secondaires),
            collaborateurs,
        },
    ))
}

/// Un type primaire retenu, et aucun type secondaire exclu.
fn type_retenu(primaire: Option<&str>, secondaires: &[String]) -> bool {
    primaire.is_some_and(|t| TYPES_GARDES.contains(&t))
        && !secondaires.iter().any(|t| TYPES_EXCLUS.contains(&t.as_str()))
}

/// Joint une liste de chaînes par une virgule, ou `None` si elle est vide.
fn joindre(v: &[String]) -> Option<String> {
    (!v.is_empty()).then(|| v.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fraiche(prim: &str, sec: &[&str], date: &str, artistes: &[&str]) -> SortieFraiche {
        SortieFraiche {
            rg_mbid: Some("rg-1".into()),
            titre: "Un disque".into(),
            artistes: vec!["Crédit".into()],
            artistes_mbids: artistes.iter().map(|s| s.to_string()).collect(),
            date_sortie: Some(date.into()),
            type_primaire: Some(prim.into()),
            types_secondaires: sec.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn bib() -> HashSet<String> {
        ["ancre".to_string()].into_iter().collect()
    }
    fn noms() -> HashMap<String, String> {
        [("ancre".to_string(), "L'Ancre".to_string())].into_iter().collect()
    }

    #[test]
    fn retenir_garde_une_sortie_recente_dun_artiste_de_la_bibliotheque() {
        let f = fraiche("Album", &[], "2026-08-20", &["ancre"]);
        let (mbid, nom, s) = retenir(&f, &bib(), &noms(), "2026-07-31", "2026-08-30").expect("retenu");
        assert_eq!(mbid, "ancre");
        assert_eq!(nom, "L'Ancre");
        assert_eq!(s.date_sortie_norm.as_deref(), Some("2026-08-20"));
        assert!(s.collaborateurs.is_none());
    }

    #[test]
    fn retenir_ecarte_ce_qui_ne_va_pas() {
        let b = bib();
        let n = noms();
        // Aucun artiste de la bibliothèque.
        assert!(retenir(&fraiche("Album", &[], "2026-08-20", &["autre"]), &b, &n, "2026-07-31", "2026-08-30").is_none());
        // Artiste spécial (Various Artists).
        assert!(retenir(&fraiche("Album", &[], "2026-08-20", &["89ad4ac3-39f7-470e-963a-56509c546377"]), &b, &n, "2026-07-31", "2026-08-30").is_none());
        // Trop vieux / dans le futur.
        assert!(retenir(&fraiche("Album", &[], "2026-01-01", &["ancre"]), &b, &n, "2026-07-31", "2026-08-30").is_none());
        assert!(retenir(&fraiche("Album", &[], "2027-01-01", &["ancre"]), &b, &n, "2026-07-31", "2026-08-30").is_none());
        // Live, compilation.
        assert!(retenir(&fraiche("Album", &["Live"], "2026-08-20", &["ancre"]), &b, &n, "2026-07-31", "2026-08-30").is_none());
        // Type non retenu.
        assert!(retenir(&fraiche("Broadcast", &[], "2026-08-20", &["ancre"]), &b, &n, "2026-07-31", "2026-08-30").is_none());
    }

    #[test]
    fn retenir_marque_la_collaboration_quand_le_credit_porte_plusieurs_artistes() {
        let mut f = fraiche("Single", &[], "2026-08-10", &["ancre", "invite"]);
        f.artistes = vec!["L'Ancre feat. Invité".into()];
        let (_, _, s) = retenir(&f, &bib(), &noms(), "2026-07-31", "2026-08-30").expect("retenu");
        assert_eq!(s.collaborateurs.as_deref(), Some("L'Ancre feat. Invité"));
    }
}
