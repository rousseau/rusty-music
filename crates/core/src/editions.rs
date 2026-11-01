// SPDX-License-Identifier: GPL-3.0-or-later
//! Choisir l'édition MusicBrainz d'un album, face à face.
//!
//! Un album de la bibliothèque, ses éditions chez MusicBrainz : les lire dans
//! les réponses de l'API, les classer par concordance avec l'album local, et
//! apparier les pistes de l'une et de l'autre. Rien ici ne parle au réseau
//! (le client est [`crate::musicbrainz::Client::chercher_editions`] et
//! [`crate::musicbrainz::Client::edition`]) ni n'écrit en base
//! ([`crate::db::Library::retenir_edition`]).
//!
//! **Une concordance n'est pas une décision** — l'esprit du « nom incertain »
//! de `docs/nommage-familles.md` : on classe, on marque ce qui concorde et ce
//! qui diffère, l'utilisateur retient. Idée de la comparaison face à face :
//! Disc Player (https://github.com/eudj1n/snowsky-disc-player,
//! `src/layout/AlbumCompare.vue`, licence MIT) ; voir
//! `docs/plan-ecouter-v0.3.md`, point 3.

use serde::Serialize;
use serde_json::Value;

use crate::musicbrainz::normaliser_titre;

/// Écart de durée à partir duquel deux pistes appariées sont marquées.
pub const ECART_MARQUE_MS: i64 = 3_000;

/// Une édition rendue par la recherche — de quoi la reconnaître dans une liste.
/// La recherche ne donne pas les durées : elles viennent avec
/// [`EditionDetaillee`], une requête de plus, à la sélection.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditionProposee {
    pub id: String,
    pub titre: String,
    pub artiste: String,
    pub date: Option<String>,
    pub pays: Option<String>,
    pub label: Option<String>,
    pub catalogue: Option<String>,
    /// Formats des supports, joints (« 2×CD », « Vinyl + Digital Media »).
    pub format: Option<String>,
    pub pistes: u32,
    /// Même nombre de pistes que l'album local : une concordance, rien de plus.
    pub concorde: bool,
}

/// Une piste d'une édition détaillée.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PisteEdition {
    pub disque: u32,
    pub position: u32,
    pub titre: String,
    pub duree_ms: Option<i64>,
    pub recording_id: String,
    /// Premier artiste crédité sur la piste (une compilation en a plusieurs).
    pub artiste_mbid: Option<String>,
}

/// Une édition et toutes ses pistes, dans l'ordre des supports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditionDetaillee {
    pub id: String,
    pub titre: String,
    pub artiste: String,
    /// Premier artiste crédité sur l'édition — l'artiste de l'album.
    pub artiste_mbid: Option<String>,
    pub release_group: Option<String>,
    pub pistes: Vec<PisteEdition>,
}

/// Une piste de l'album local, telle que l'appariement la voit.
#[derive(Debug, Clone, PartialEq)]
pub struct PisteLocale {
    pub titre: String,
    pub duree_ms: Option<i64>,
}

/// Une ligne de l'alignement : une piste locale, une piste de l'édition, ou les
/// deux. `ecart_ms` : durée de l'édition moins durée locale, quand les deux
/// sont connues ; `titre_differe` : les titres normalisés ne se ressemblent pas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Paire {
    pub locale: Option<usize>,
    pub edition: Option<usize>,
    pub ecart_ms: Option<i64>,
    pub titre_differe: bool,
}

impl Paire {
    /// Vrai si la ligne mérite d'être signalée : piste en trop ou manquante,
    /// écart de durée de [`ECART_MARQUE_MS`] ou plus, titre différent.
    pub fn marquee(&self) -> bool {
        self.locale.is_none()
            || self.edition.is_none()
            || self.ecart_ms.is_some_and(|e| e.abs() >= ECART_MARQUE_MS)
            || self.titre_differe
    }
}

/// Le nom crédité d'une entité (`artist-credit`) : les noms et leurs liaisons.
fn credit(v: &Value) -> String {
    v["artist-credit"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| {
                    format!(
                        "{}{}",
                        c["name"].as_str().unwrap_or_default(),
                        c["joinphrase"].as_str().unwrap_or_default()
                    )
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn premier_artiste(v: &Value) -> Option<String> {
    v["artist-credit"][0]["artist"]["id"]
        .as_str()
        .map(str::to_string)
}

fn texte(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// Les formats des supports, joints : deux CD donnent « 2×CD ».
fn formats(media: &[Value]) -> Option<String> {
    let mut parts: Vec<(String, usize)> = Vec::new();
    for m in media {
        let f = m["format"].as_str().unwrap_or("?").to_string();
        match parts.iter_mut().find(|(g, _)| *g == f) {
            Some((_, n)) => *n += 1,
            None => parts.push((f, 1)),
        }
    }
    (!parts.is_empty()).then(|| {
        parts
            .into_iter()
            .map(|(f, n)| if n > 1 { format!("{n}×{f}") } else { f })
            .collect::<Vec<_>>()
            .join(" + ")
    })
}

/// Les éditions d'une réponse de recherche `ws/2/release?query=…`, classées
/// pour un album local de `pistes_locales` pistes : nombre de pistes égal
/// d'abord, puis le plus proche ; à égalité, l'ordre de pertinence de
/// MusicBrainz.
pub fn editions_de_recherche(v: &Value, pistes_locales: u32) -> Vec<EditionProposee> {
    let mut eds: Vec<EditionProposee> = v["releases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let id = r["id"].as_str()?.to_string();
            let media = r["media"].as_array().cloned().unwrap_or_default();
            let pistes = r["track-count"]
                .as_u64()
                .map(|n| n as u32)
                .unwrap_or_else(|| {
                    media
                        .iter()
                        .filter_map(|m| m["track-count"].as_u64())
                        .sum::<u64>() as u32
                });
            let info = &r["label-info"][0];
            Some(EditionProposee {
                id,
                titre: r["title"].as_str().unwrap_or_default().to_string(),
                artiste: credit(r),
                date: texte(&r["date"]),
                pays: texte(&r["country"]),
                label: texte(&info["label"]["name"]),
                catalogue: texte(&info["catalog-number"]),
                format: formats(&media),
                pistes,
                concorde: pistes == pistes_locales,
            })
        })
        .collect();
    // `sort_by_key` est stable : l'ordre de MusicBrainz départage.
    eds.sort_by_key(|e| e.pistes.abs_diff(pistes_locales));
    eds
}

/// Une édition détaillée, d'une réponse `ws/2/release/{id}?inc=recordings+artist-credits`.
pub fn edition_detaillee(v: &Value) -> Option<EditionDetaillee> {
    let id = v["id"].as_str()?.to_string();
    let mut pistes = Vec::new();
    for (i, m) in v["media"].as_array().into_iter().flatten().enumerate() {
        let disque = m["position"].as_u64().unwrap_or(i as u64 + 1) as u32;
        for (j, t) in m["tracks"].as_array().into_iter().flatten().enumerate() {
            let Some(recording_id) = t["recording"]["id"].as_str() else {
                continue;
            };
            pistes.push(PisteEdition {
                disque,
                position: t["position"].as_u64().unwrap_or(j as u64 + 1) as u32,
                titre: t["title"].as_str().unwrap_or_default().to_string(),
                duree_ms: t["length"]
                    .as_i64()
                    .or_else(|| t["recording"]["length"].as_i64()),
                recording_id: recording_id.to_string(),
                artiste_mbid: premier_artiste(t).or_else(|| premier_artiste(&t["recording"])),
            });
        }
    }
    Some(EditionDetaillee {
        id,
        titre: v["title"].as_str().unwrap_or_default().to_string(),
        artiste: credit(v),
        artiste_mbid: premier_artiste(v),
        release_group: texte(&v["release-group"]["id"]),
        pistes,
    })
}

/// Apparie les pistes locales (dans l'ordre de l'album) et celles de
/// l'édition (dans l'ordre des supports), **rang à rang** : la n-ième avec la
/// n-ième, les pistes en trop d'un côté restant seules. Ni réordonnancement
/// ni appariement par titre : un album local dans le désordre se voit, il ne
/// se corrige pas en silence.
pub fn apparier(locales: &[PisteLocale], edition: &[PisteEdition]) -> Vec<Paire> {
    (0..locales.len().max(edition.len()))
        .map(|i| {
            let l = locales.get(i);
            let e = edition.get(i);
            let ecart_ms = match (l.and_then(|l| l.duree_ms), e.and_then(|e| e.duree_ms)) {
                (Some(a), Some(b)) => Some(b - a),
                _ => None,
            };
            let titre_differe = match (l, e) {
                (Some(l), Some(e)) => {
                    let (a, b) = (normaliser_titre(&l.titre), normaliser_titre(&e.titre));
                    !(a == b || a.contains(&b) || b.contains(&a))
                }
                _ => false,
            };
            Paire {
                locale: l.map(|_| i),
                edition: e.map(|_| i),
                ecart_ms,
                titre_differe,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn recherche() -> Value {
        json!({ "releases": [
            { "id": "a", "title": "Night Drive (Deluxe)", "track-count": 12,
              "artist-credit": [{ "name": "Northline", "joinphrase": "" }],
              "date": "2014", "country": "US",
              "label-info": [{ "catalog-number": "LR-03", "label": { "name": "Lumen" } }],
              "media": [{ "format": "CD", "track-count": 6 }, { "format": "CD", "track-count": 6 }] },
            { "id": "b", "title": "Night Drive", "track-count": 3,
              "artist-credit": [{ "name": "Northline", "joinphrase": " & " }, { "name": "Iris", "joinphrase": "" }],
              "date": "2010", "media": [{ "format": "Vinyl", "track-count": 3 }] },
            { "id": "c", "title": "Night Drive", "track-count": 4, "media": [] },
            { "title": "sans identifiant" }
        ]})
    }

    #[test]
    fn la_recherche_classe_par_nombre_de_pistes_et_marque_la_concordance() {
        let eds = editions_de_recherche(&recherche(), 3);
        let ids: Vec<&str> = eds.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["b", "c", "a"]);
        assert!(eds[0].concorde && !eds[1].concorde);
        assert_eq!(eds[0].artiste, "Northline & Iris");
        assert_eq!(eds[2].format.as_deref(), Some("2×CD"));
        assert_eq!(eds[2].label.as_deref(), Some("Lumen"));
        assert_eq!(eds[2].catalogue.as_deref(), Some("LR-03"));
        assert_eq!(eds[1].date, None);
    }

    #[test]
    fn une_edition_detaillee_garde_l_ordre_des_supports() {
        let v = json!({
            "id": "r", "title": "Night Drive",
            "artist-credit": [{ "name": "Northline", "artist": { "id": "art" } }],
            "release-group": { "id": "rg" },
            "media": [
                { "position": 1, "tracks": [
                    { "position": 1, "title": "Night Drive", "length": 243000, "recording": { "id": "r1" } },
                    { "position": 2, "title": "City Glow", "recording": { "id": "r2", "length": 176000 },
                      "artist-credit": [{ "artist": { "id": "invite" } }] }
                ]},
                { "position": 2, "tracks": [
                    { "position": 1, "title": "Last Exit", "length": 229000, "recording": { "id": "r3" } },
                    { "position": 2, "title": "sans enregistrement" }
                ]}
            ]
        });
        let e = edition_detaillee(&v).unwrap();
        assert_eq!(e.artiste_mbid.as_deref(), Some("art"));
        assert_eq!(e.release_group.as_deref(), Some("rg"));
        assert_eq!(e.pistes.len(), 3);
        assert_eq!((e.pistes[2].disque, e.pistes[2].position), (2, 1));
        assert_eq!(e.pistes[1].duree_ms, Some(176_000));
        assert_eq!(e.pistes[1].artiste_mbid.as_deref(), Some("invite"));
        assert_eq!(e.pistes[0].artiste_mbid, None);
    }

    fn locale(titre: &str, s: i64) -> PisteLocale {
        PisteLocale {
            titre: titre.into(),
            duree_ms: Some(s * 1000),
        }
    }
    fn piste(titre: &str, s: i64) -> PisteEdition {
        PisteEdition {
            disque: 1,
            position: 1,
            titre: titre.into(),
            duree_ms: Some(s * 1000),
            recording_id: "x".into(),
            artiste_mbid: None,
        }
    }

    #[test]
    fn l_appariement_marque_ecarts_titres_et_pistes_seules() {
        let paires = apparier(
            &[
                locale("Night Drive", 243),
                locale("City Glow (remaster)", 176),
                locale("Last Exit", 229),
            ],
            &[
                piste("Night Drive", 244),
                piste("City Glow", 176),
                piste("Autre chose", 236),
                piste("Bonus", 200),
            ],
        );
        assert_eq!(paires.len(), 4);
        assert!(!paires[0].marquee(), "1 s d'écart : sous le seuil");
        assert!(
            !paires[1].marquee(),
            "variante de titre contenue dans l'autre"
        );
        assert!(
            paires[2].titre_differe && paires[2].ecart_ms == Some(7_000) && paires[2].marquee()
        );
        assert_eq!((paires[3].locale, paires[3].edition), (None, Some(3)));
        assert!(paires[3].marquee());
    }
}
