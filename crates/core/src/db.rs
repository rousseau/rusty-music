// SPDX-License-Identifier: GPL-3.0-or-later
//! Base locale SQLite : la seule source de vérité pour les trois modules.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::functions::FunctionFlags;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use rusqlite_migration::{Migrations, M};

use crate::error::Result;
use crate::tags::TrackMeta;

mod analyse;
mod bibliotheque;
mod carte;
mod decouvrir;
mod editions;
mod enrichissement;
mod pistes;
mod schema;
#[cfg(test)]
mod tests;

// Les types de chaque domaine restent nommés `db::…` hors du module.
pub use self::analyse::*;
pub use self::bibliotheque::*;
pub use self::carte::*;
pub use self::decouvrir::*;
pub use self::editions::*;
pub use self::enrichissement::*;
pub use self::pistes::*;
use self::schema::preparer;

pub struct Library {
    pub conn: Connection,
}

/// Résumé d'un morceau tel que servi à l'interface.
///
/// `Deserialize` en plus de `Serialize` : le plan interprété du champ
/// d'intention d'Explorer (`PlanTexte`, `apps/desktop/src/main.rs`) en
/// embarque un pour l'afficher, et le reçoit de retour une fois l'utilisateur
/// passé par l'inspecteur — le seul aller-retour JS de ce genre à ce jour.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrackRow {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<i64>,
    pub year: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Identifiant MusicBrainz de **l'artiste du morceau** (`mb_artist_id`),
    /// pas de l'artiste d'album — sert à retrouver ses albums
    /// (`Library::albums_of_artist`) depuis un morceau, sans passer par la
    /// liste des artistes. Doit rester l'identifiant du nom porté par
    /// `artist` : sur une compilation « Various Artists », l'artiste d'album
    /// (`mb_album_artist_id`) est le fourre-tout, pas le morceau — l'utiliser
    /// ici ferait cliquer « Helmet » et ouvrir les albums d'autres artistes
    /// que la seule compilation a en commun.
    pub artist_mbid: Option<String>,
}

/// Hachage stable d'un couple artiste/album. Doit rendre la même valeur d'une
/// exécution à l'autre : c'est lui qui départage les arrivées d'une même année.
pub(crate) fn hacher(artiste: &str, album: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in artiste
        .as_bytes()
        .iter()
        .chain(b"\x1f")
        .chain(album.as_bytes())
    {
        h ^= *octet as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Colonnes projetées pour un [`TrackRow`], partagées par toutes les requêtes
/// de consultation pour que l'ordre reste aligné sur [`track_from_row`].
pub(crate) const TRACK_COLS: &str =
    "id, path, title, artist, album, track_no, year, duration_ms, mb_artist_id";

/// Traduit une saisie libre en requête FTS5.
///
/// Chaque mot est mis entre guillemets : sans cela le texte de l'utilisateur
/// serait interprété comme des opérateurs (`AND`, `OR`, `NEAR`, `-`, `*`) et
/// une apostrophe suffirait à produire une erreur de syntaxe. Le dernier mot
/// reçoit un `*` — c'est celui qu'on est en train de taper.
fn requete_fts(q: &str) -> String {
    let mots: Vec<String> = q
        .split_whitespace()
        // Un mot sans caractère alphanumérique ne donne aucun terme au
        // tokenizer : le garder produirait une phrase vide, donc une erreur.
        .filter(|m| m.chars().any(char::is_alphanumeric))
        .map(|m| m.replace('"', "\"\""))
        .collect();

    let Some(dernier) = mots.len().checked_sub(1) else {
        return String::new();
    };
    mots.iter()
        .enumerate()
        .map(|(i, m)| {
            if i == dernier {
                format!("\"{m}\"*")
            } else {
                format!("\"{m}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn track_from_row(r: &rusqlite::Row) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?,
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        track_no: r.get(5)?,
        year: r.get(6)?,
        duration_ms: r.get(7)?,
        artist_mbid: r.get(8)?,
    })
}

impl Library {
    /// Ouvre (ou crée) la base et joue les migrations en attente.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = preparer(Connection::open(db_path)?)?;
        Ok(Self { conn })
    }

    /// Base en mémoire — pratique pour les tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = preparer(Connection::open_in_memory()?)?;
        Ok(Self { conn })
    }
}

/// Plancher de votes sur un genre MusicBrainz.
///
/// **Un, c'est-à-dire aucun filtre — et c'est une correction.** Le seuil avait
/// d'abord été mis à deux, pour écarter l'`amapiano` posé par un unique
/// contributeur sur Yann Tiersen. Mesuré, il faisait l'inverse de ce qu'on
/// attendait : chez cet artiste, `modern classical`, `neoclassicism`,
/// `minimalism` et `instrumental` portent eux aussi une seule voix. Le seuil
/// ne gardait que `rock` (2 voix) et jetait les quatre justes.
///
/// Le coût était général : **55 % des morceaux couverts au lieu de 74 %**, et
/// 360 artistes rendus muets, ceux dont aucune étiquette n'atteint deux voix.
///
/// Ce qui règle vraiment le cas `amapiano` est le classement à votes égaux,
/// dans [`Library::mb_genres`], pas un plancher.
const VOTES_MINIMUM: i64 = 1;

/// Combien de genres retenir par entité MusicBrainz.
///
/// **Un seul, et c'est mesuré.** Le genre le mieux voté d'un artiste est celui
/// sur lequel les contributeurs s'accordent ; les suivants décrivent ses
/// marges. En retenir trois versait « rock » et « pop » dans toutes les
/// familles, et c'est le générique qui l'emportait. Comparés sur la
/// bibliothèque entière, un genre par artiste rend « Reggae · Afrobeat » là où
/// trois donnaient « Reggae · Ska », et « Rock · Nu Metal » là où trois
/// donnaient « Rock · Alternative Metal ».
///
/// Trouver le distinctif est le travail de [`nommer_les_familles`], pas celui
/// de cette troncature : lui donner plus de matière générique ne l'aide pas,
/// ça la noie.
const GENRES_PAR_ENTITE: usize = 1;

fn genres_du_morceau(
    artiste: Option<&str>,
    album: Option<&str>,
    tag: Option<&str>,
    albums: &HashMap<(String, String), String>,
    par_album: &HashMap<String, Vec<String>>,
    par_artiste: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let (Some(artiste), Some(album)) = (artiste, album) {
        let cle = (
            artiste.to_string(),
            crate::musicbrainz::normaliser_titre(album),
        );
        if let Some(g) = albums.get(&cle).and_then(|rg| par_album.get(rg)) {
            if !g.is_empty() {
                return g.iter().take(GENRES_PAR_ENTITE).cloned().collect();
            }
        }
    }
    if let Some(g) = artiste.and_then(|a| par_artiste.get(a)) {
        if !g.is_empty() {
            return g.iter().take(GENRES_PAR_ENTITE).cloned().collect();
        }
    }
    tag.filter(|t| !t.is_empty())
        .map(|t| vec![t.to_string()])
        .unwrap_or_default()
}

/// `1973-03-01` → `19730301`, `1973-03` → `19730300`, `1973` → `19730000`.
fn date_iso_vers_cle(d: &str) -> Option<u32> {
    let mut parts = d.split('-');
    let a: u32 = parts.next()?.parse().ok()?;
    if !(1900..=2100).contains(&a) {
        return None;
    }
    let m: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let j: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    Some(a * 10_000 + m.min(12) * 100 + j.min(31))
}
