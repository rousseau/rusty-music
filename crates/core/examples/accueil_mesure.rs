// SPDX-License-Identifier: GPL-3.0-or-later
//! Mesure A0 de la page d'accueil d'Écouter (`docs/plan-ecouter-accueil.md`) :
//! combien d'albums chaque rangée R1 à R7 trouverait sur une vraie
//! bibliothèque, avant de dessiner la page autour d'elles.
//!
//!   cargo run --release -p rusty-music-core --example accueil_mesure -- \
//!       <rusty-music.db> [accueil-mesure.md] [--modele <modèle CLAP>]
//!
//! Ouvre la base par `Library::open` (migrations comprises) : la lancer sur
//! une **copie** si l'application tourne. Rien n'est écrit en base.
//!
//! Ne recalcule rien de ce que le cœur sait déjà :
//! - albums et année fiable : [`Library::albums`] (MusicBrainz avant tag) ;
//! - date au jour près et appartenance d'une piste à son album :
//!   [`Library::ordre_darrivee`] (source `musicbrainz`, clé AAAAMMJJ, hachage
//!   d'album `hacher(artiste, album)`, le même que `albums()` regroupe) ;
//! - familles : [`Library::familles_des_albums`] et [`Library::familles`] ;
//! - écoutes : [`Library::journal_ecoutes`] (`memoire.rs`).
//!
//! Les journées sont des journées UTC ; à l'échelle de ces mesures, l'écart au
//! jour local ne change rien.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use rusty_music_core::db::{epoch_vers_cle, hacher, AlbumRow};
use rusty_music_core::Library;

const JOUR: i64 = 86_400;

/// Les règles « album écouté » comparées : combien de pistes distinctes d'un
/// album de `n` pistes doivent avoir une écoute le même jour.
/// Pistes requises pour un album de `n` pistes.
type Requis = fn(i64) -> i64;
const SEUILS: [(&str, Requis); 4] = [
    ("1 piste", |_| 1),
    ("3 pistes", |n| n.min(3)),
    ("la moitié", |n| (n + 1) / 2),
    ("3 pistes ou la moitié (plan)", |n| {
        n.min(3).min((n + 1) / 2)
    }),
];
/// L'indice de la règle du plan dans [`SEUILS`].
const SEUIL_PLAN: usize = 3;

struct Album {
    row: AlbumRow,
    /// Date d'œuvre MusicBrainz connue au jour près (AAAAMMJJ).
    date_jour: Option<u32>,
    famille: Option<i64>,
}

/// Ce qu'une règle « écouté » donne pour chaque album : les journées
/// (numéro de jour depuis l'époque) où il compte comme écouté.
type Journees = HashMap<u64, Vec<i64>>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut base: Option<PathBuf> = None;
    let mut sortie = PathBuf::from("accueil-mesure.md");
    let mut modele: Option<String> = None;
    let mut positionnels = 0;
    while let Some(a) = args.next() {
        if a == "--modele" {
            modele = args.next();
        } else {
            match positionnels {
                0 => base = Some(a.into()),
                _ => sortie = a.into(),
            }
            positionnels += 1;
        }
    }
    let base = base.ok_or("usage : accueil_mesure <base.db> [sortie.md] [--modele m]")?;
    let lib = Library::open(&base)?;

    // Le modèle des familles : celui qui a le plus de morceaux rangés, sauf
    // demande contraire (la constante vit dans crates/analysis, hors d'atteinte).
    let modele = match modele {
        Some(m) => m,
        None => lib.conn.query_row(
            "SELECT model FROM features WHERE cluster IS NOT NULL
              GROUP BY model ORDER BY COUNT(*) DESC LIMIT 1",
            [],
            |r| r.get(0),
        )?,
    };

    let maintenant = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let aujourdhui = maintenant.div_euclid(JOUR);
    let cle_auj = epoch_vers_cle(maintenant);

    // --- Albums, dates, familles -------------------------------------------
    let familles_album: HashMap<u64, i64> = lib
        .familles_des_albums(&modele)?
        .into_iter()
        .map(|(album, artiste, c)| (hacher(artiste.as_deref().unwrap_or(""), &album), c))
        .collect();
    let noms_familles: HashMap<i64, String> = lib
        .familles(&modele)?
        .into_iter()
        .map(|(c, nom, _)| (c, nom))
        .collect();

    let mut albums: HashMap<u64, Album> = lib
        .albums(None)?
        .into_iter()
        .map(|row| {
            let cle = hacher(row.artist.as_deref().unwrap_or(""), &row.name);
            let famille = familles_album.get(&cle).copied();
            (
                cle,
                Album {
                    row,
                    date_jour: None,
                    famille,
                },
            )
        })
        .collect();

    let mut album_de_piste: HashMap<i64, u64> = HashMap::new();
    for a in lib.ordre_darrivee()? {
        let Some(album) = albums.get_mut(&a.album) else {
            continue;
        };
        album_de_piste.insert(a.track_id, a.album);
        if a.source == "musicbrainz" && a.date % 100 != 0 && (a.date / 100) % 100 != 0 {
            album.date_jour = Some(album.date_jour.map_or(a.date, |d| d.min(a.date)));
        }
    }

    // --- Journal -------------------------------------------------------------
    let journal = lib.journal_ecoutes()?;
    // (album, jour) → pistes distinctes écoutées ce jour-là.
    let mut pistes_par_jour: HashMap<(u64, i64), HashSet<i64>> = HashMap::new();
    for &(piste, le) in &journal {
        if let Some(&album) = album_de_piste.get(&piste) {
            pistes_par_jour
                .entry((album, le.div_euclid(JOUR)))
                .or_default()
                .insert(piste);
        }
    }
    let journees: Vec<Journees> = SEUILS
        .iter()
        .map(|(_, requis)| {
            let mut j: Journees = HashMap::new();
            for (&(album, jour), pistes) in &pistes_par_jour {
                let n = albums[&album].row.tracks;
                if pistes.len() as i64 >= requis(n) {
                    j.entry(album).or_default().push(jour);
                }
            }
            j.values_mut().for_each(|v| v.sort_unstable());
            j
        })
        .collect();

    let mut md = String::new();
    writeln!(md, "# Mesure A0 — page d'accueil d'Écouter\n")?;
    writeln!(
        md,
        "Base : `{}` · modèle des familles : `{modele}` · mesurée le {} \
         (`crates/core/examples/accueil_mesure.rs`).\n",
        base.display(),
        date(cle_auj)
    )?;

    // --- Bibliothèque et journal ---------------------------------------------
    let n_albums = albums.len();
    let jours_journal: HashSet<i64> = journal.iter().map(|&(_, le)| le.div_euclid(JOUR)).collect();
    let pistes_journal: HashSet<i64> = journal.iter().map(|&(p, _)| p).collect();
    writeln!(md, "## Bibliothèque et journal `ecoute`\n")?;
    writeln!(md, "| mesure | valeur |\n|---|---|")?;
    writeln!(md, "| albums | {n_albums} |")?;
    if let (Some(&(_, premier)), Some(&(_, dernier))) = (journal.first(), journal.last()) {
        writeln!(
            md,
            "| première écoute | {} |",
            date(epoch_vers_cle(premier))
        )?;
        writeln!(
            md,
            "| dernière écoute | {} |",
            date(epoch_vers_cle(dernier))
        )?;
        writeln!(
            md,
            "| ancienneté du journal | {} jours |",
            aujourdhui - premier.div_euclid(JOUR)
        )?;
    } else {
        writeln!(md, "| journal | vide |")?;
    }
    writeln!(
        md,
        "| écoutes (morceaux de la bibliothèque) | {} |",
        journal.len()
    )?;
    writeln!(
        md,
        "| journées avec au moins une écoute | {} |",
        jours_journal.len()
    )?;
    writeln!(
        md,
        "| morceaux distincts écoutés | {} |",
        pistes_journal.len()
    )?;
    let albums_touches: HashSet<u64> = pistes_par_jour.keys().map(|&(a, _)| a).collect();
    writeln!(
        md,
        "| albums dont au moins une piste a été écoutée | {} |\n",
        albums_touches.len()
    )?;

    // --- Sensibilité au seuil « écouté » --------------------------------------
    writeln!(md, "## Sensibilité au seuil « album écouté »\n")?;
    writeln!(
        md,
        "Un album est écouté un jour si assez de ses pistes distinctes y ont une écoute.\n"
    )?;
    writeln!(
        md,
        "| règle | écoutés ≥ 1 jour | ≥ 3 jours | écoutés < 30 j | R1 (≥ 3 j, > 12 mois) | \
         R1 (≥ 3 j, > 3 mois) | R1 (≥ 3 j, > 1 mois) | R1 (≥ 1 j, > 1 mois) | R2 jamais | R6 |"
    )?;
    writeln!(md, "|---|---|---|---|---|---|---|---|---|---|")?;
    for (i, (nom, _)) in SEUILS.iter().enumerate() {
        let j = &journees[i];
        let au_moins = |k: usize| j.values().filter(|v| v.len() >= k).count();
        let r1 = |jours_min: usize, anciennete: i64| {
            j.values()
                .filter(|v| v.len() >= jours_min && aujourdhui - v[v.len() - 1] > anciennete)
                .count()
        };
        let recents = j
            .values()
            .filter(|v| aujourdhui - v[v.len() - 1] < 30)
            .count();
        let (r2, r6) = jamais(&albums, j, maintenant);
        writeln!(
            md,
            "| {nom} | {} | {} | {recents} | {} | {} | {} | {} | {r2} | {r6} |",
            au_moins(1),
            au_moins(3),
            r1(3, 365),
            r1(3, 91),
            r1(3, 30),
            r1(1, 30),
        )?;
    }
    writeln!(md)?;

    // Le reste suit la règle du plan.
    let j = &journees[SEUIL_PLAN];

    // --- R1 à R7 -------------------------------------------------------------
    writeln!(md, "## Rangées (règle du plan)\n")?;
    writeln!(md, "| rangée | candidats | note |\n|---|---|---|")?;
    let r1_12 = j
        .values()
        .filter(|v| v.len() >= 3 && aujourdhui - v[v.len() - 1] > 365)
        .count();
    let r1_3 = j
        .values()
        .filter(|v| v.len() >= 3 && aujourdhui - v[v.len() - 1] > 91)
        .count();
    writeln!(
        md,
        "| R1 `oublies` | {r1_12} | ≥ 3 journées, dernière > 12 mois ; à 3 mois : {r1_3} |"
    )?;
    let (r2, r6) = jamais(&albums, j, maintenant);
    writeln!(
        md,
        "| R2 `jamais` | {r2} | aucune écoute, ajouté il y a plus de 30 j |"
    )?;

    // R3 : anniversaires.
    let doy_auj = jour_an(cle_auj);
    let annee_auj = (cle_auj / 10_000) as i64;
    let dates: Vec<(i32, i64)> = albums
        .values()
        .filter_map(|a| a.date_jour)
        .map(|d| (jour_an(d), (d / 10_000) as i64))
        .collect();
    let rond = |n: i64| n >= 10 && (n % 10 == 0 || n == 25);
    let fenetre = |doy: i32, annee: i64, seulement_rond: bool| {
        dates
            .iter()
            .filter(|&&(d, a)| {
                let ecart = (d - doy).rem_euclid(365).min((doy - d).rem_euclid(365));
                let n = annee - a;
                ecart <= 3 && n >= 1 && (!seulement_rond || rond(n))
            })
            .count()
    };
    writeln!(
        md,
        "| R3 `anniversaire` | {} | ± 3 jours du {} ; dont *N* rond : {} |",
        fenetre(doy_auj, annee_auj, false),
        date(cle_auj),
        fenetre(doy_auj, annee_auj, true)
    )?;

    // R4 : cases famille × décennie.
    let mut cases: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    let mut demi: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    for a in albums.values() {
        if let (Some(f), Some(y)) = (a.famille, a.row.year) {
            *cases.entry((f, y / 10 * 10)).or_default() += 1;
            *demi.entry((f, y / 5 * 5)).or_default() += 1;
        }
    }
    let pleines: Vec<_> = cases.iter().filter(|(_, &n)| n >= 8).collect();
    let couverts: usize = pleines.iter().map(|(_, &n)| n).sum();
    writeln!(
        md,
        "| R4 `epoque` | {} cases ≥ 8 | {couverts} albums couverts |",
        pleines.len()
    )?;

    // R5 : tout l'ensemble, par classe de poids.
    let (mut jamais_ecoute, mut ancien, mut moyen, mut ce_mois) = (0, 0, 0, 0);
    for cle in albums.keys() {
        match j.get(cle).map(|v| aujourdhui - v[v.len() - 1]) {
            None => jamais_ecoute += 1,
            Some(e) if e > 365 => ancien += 1,
            Some(e) if e >= 30 => moyen += 1,
            Some(_) => ce_mois += 1,
        }
    }
    writeln!(
        md,
        "| R5 `hasard` | {n_albums} | jamais : {jamais_ecoute} · > 12 mois : {ancien} · \
         1–12 mois : {moyen} · < 30 j : {ce_mois} |"
    )?;
    writeln!(
        md,
        "| R6 `nouveaux` | {r6} | ajouté il y a moins de 30 j, aucune écoute |"
    )?;

    // R7 : X = album le plus écouté (en journées) des 14 derniers jours.
    let x = j
        .iter()
        .map(|(cle, v)| {
            let recentes = v.iter().filter(|&&d| aujourdhui - d < 14).count();
            (recentes, v[v.len() - 1], *cle)
        })
        .filter(|&(n, _, _)| n > 0)
        .max();
    match x {
        Some((n, _, cle)) => {
            let ax = &albums[&cle];
            let candidats = albums
                .iter()
                .filter(|(c, a)| {
                    a.famille.is_some()
                        && a.row.artist != ax.row.artist
                        && j.get(c).is_none_or(|v| aujourdhui - v[v.len() - 1] >= 30)
                })
                .count();
            writeln!(
                md,
                "| R7 `parce-que` | {candidats} | *X* = {} — {} ({n} journées sur 14 j) ; \
                 voisins possibles : sur la carte, autre artiste, pas écoutés < 30 j |",
                ax.row.name,
                ax.row.artist.as_deref().unwrap_or("?"),
            )?;
        }
        None => writeln!(
            md,
            "| R7 `parce-que` | 0 | aucun album écouté ces 14 jours |"
        )?,
    }
    writeln!(md)?;

    // --- R3 : distribution ---------------------------------------------------
    let avec_date: usize = albums.values().filter(|a| a.date_jour.is_some()).count();
    let avec_annee: usize = albums.values().filter(|a| a.row.year.is_some()).count();
    writeln!(md, "## R3 — dates au jour près\n")?;
    writeln!(md, "| mesure | valeur |\n|---|---|")?;
    writeln!(
        md,
        "| albums datés au jour près (MusicBrainz, hors compilations) | {avec_date} ({}) |",
        pc(avec_date, n_albums)
    )?;
    writeln!(
        md,
        "| albums avec une année | {avec_annee} ({}) |",
        pc(avec_annee, n_albums)
    )?;
    let par_jour: Vec<usize> = (0..365).map(|d| fenetre(d, annee_auj, false)).collect();
    let par_jour_rond: Vec<usize> = (0..365).map(|d| fenetre(d, annee_auj, true)).collect();
    for (nom, v) in [
        ("tous *N*", &par_jour),
        ("*N* rond seulement", &par_jour_rond),
    ] {
        let mut t = v.clone();
        t.sort_unstable();
        writeln!(
            md,
            "| fenêtre ± 3 j, {nom} : min / médiane / max | {} / {} / {} |",
            t[0], t[182], t[364]
        )?;
        writeln!(
            md,
            "| jours de l'année où la rangée tient (≥ 4), {nom} | {} / 365 |",
            v.iter().filter(|&&n| n >= 4).count()
        )?;
    }
    writeln!(md)?;
    writeln!(md, "Albums datés au jour près, par mois de sortie :\n")?;
    let mut par_mois = [0usize; 12];
    for a in albums.values() {
        if let Some(d) = a.date_jour {
            par_mois[((d / 100) % 100 - 1) as usize] += 1;
        }
    }
    writeln!(
        md,
        "| jan | fév | mar | avr | mai | juin | juil | août | sep | oct | nov | déc |"
    )?;
    writeln!(md, "|---|---|---|---|---|---|---|---|---|---|---|---|")?;
    writeln!(md, "| {} |\n", par_mois.map(|n| n.to_string()).join(" | "))?;
    // Un 1er janvier sur-représenté trahit une date d'année déguisée.
    let premiers_janvier = albums
        .values()
        .filter(|a| a.date_jour.is_some_and(|d| d % 10_000 == 101))
        .count();
    writeln!(md, "Dont au 1er janvier : {premiers_janvier}.\n")?;

    // --- R4 : familles × décennies -------------------------------------------
    let sans_famille = albums.values().filter(|a| a.famille.is_none()).count();
    let sans_annee_famille = albums
        .values()
        .filter(|a| a.famille.is_some() && a.row.year.is_none())
        .count();
    writeln!(md, "## R4 — familles × décennies\n")?;
    writeln!(md, "| mesure | valeur |\n|---|---|")?;
    writeln!(md, "| familles | {} |", noms_familles.len())?;
    writeln!(
        md,
        "| albums sans famille (aucun morceau sur la carte) | {sans_famille} ({}) |",
        pc(sans_famille, n_albums)
    )?;
    writeln!(
        md,
        "| albums avec famille mais sans année | {sans_annee_famille} |"
    )?;
    writeln!(md, "| cases non vides | {} |", cases.len())?;
    writeln!(
        md,
        "| cases ≥ 8 albums | {} ({couverts} albums) |",
        pleines.len()
    )?;
    writeln!(
        md,
        "| cases ≥ 16 (à couper en demi-décennies) | {} |",
        cases.values().filter(|&&n| n >= 16).count()
    )?;
    writeln!(
        md,
        "| demi-décennies ≥ 8 albums | {} |\n",
        demi.values().filter(|&&n| n >= 8).count()
    )?;
    let nom = |f: i64| {
        noms_familles
            .get(&f)
            .cloned()
            .unwrap_or_else(|| format!("#{f}"))
    };
    let decennies: Vec<i64> = {
        let mut d: Vec<i64> = cases.keys().map(|&(_, d)| d).collect();
        d.sort_unstable();
        d.dedup();
        d
    };
    let familles_ordre: Vec<i64> = {
        let mut f: Vec<i64> = cases.keys().map(|&(f, _)| f).collect();
        f.sort_unstable();
        f.dedup();
        f
    };
    writeln!(md, "Albums par case (en gras, ≥ 8) :\n")?;
    write!(md, "| famille |")?;
    for d in &decennies {
        write!(md, " {d} |")?;
    }
    writeln!(md)?;
    writeln!(md, "|---|{}", "---|".repeat(decennies.len()))?;
    for f in familles_ordre {
        write!(md, "| {} |", nom(f))?;
        for d in &decennies {
            match cases.get(&(f, *d)) {
                Some(&n) if n >= 8 => write!(md, " **{n}** |")?,
                Some(&n) => write!(md, " {n} |")?,
                None => write!(md, " |")?,
            }
        }
        writeln!(md)?;
    }
    writeln!(md)?;

    std::fs::write(&sortie, &md)?;
    print!("{md}");
    eprintln!("écrit : {}", sortie.display());
    Ok(())
}

/// R2 (jamais écoutés, ajoutés il y a plus de 30 jours) et R6 (jamais
/// écoutés, ajoutés depuis moins de 30 jours). Un album sans date d'ajout
/// compte comme ancien.
fn jamais(albums: &HashMap<u64, Album>, j: &Journees, maintenant: i64) -> (usize, usize) {
    let mut r2 = 0;
    let mut r6 = 0;
    for (cle, a) in albums {
        if j.contains_key(cle) {
            continue;
        }
        match a.row.ajoute_le {
            Some(t) if maintenant - t < 30 * JOUR => r6 += 1,
            _ => r2 += 1,
        }
    }
    (r2, r6)
}

/// Rang du jour dans une année de 365 jours (le 29 février se confond avec
/// le 1er mars).
fn jour_an(cle: u32) -> i32 {
    const CUMUL: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m = ((cle / 100) % 100).clamp(1, 12) as usize;
    let j = (cle % 100).clamp(1, 31) as i32;
    (CUMUL[m - 1] + j - 1).min(364)
}

fn date(cle: u32) -> String {
    format!("{}-{:02}-{:02}", cle / 10_000, (cle / 100) % 100, cle % 100)
}

fn pc(n: usize, total: usize) -> String {
    if total == 0 {
        return "—".into();
    }
    format!("{:.1} %", 100.0 * n as f64 / total as f64)
}
