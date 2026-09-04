//! Persistance du plan de ville.
//!
//! Le `.osm.pbf` fait trois cent vingt mégaoctets et met dix secondes à se
//! relire ; la ville qu'on en tire tient dans une base d'une vingtaine. On
//! l'importe une fois, on jette l'archive.
//!
//! Cette base est **distincte de celle de la bibliothèque**. La ville n'est pas
//! une donnée de l'utilisateur : elle est empruntée, elle se remplace en bloc,
//! et le jour où l'on essaie Venise on supprime un fichier au lieu de migrer un
//! schéma.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::{Adresse, Classe, Contour, Extrait, Lieu, PointRemarquable, Troncon};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;

-- Une seule ligne : la ville que ce fichier décrit.
CREATE TABLE IF NOT EXISTS ville (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    nom        TEXT    NOT NULL,
    source     TEXT    NOT NULL,   -- d'où vient l'extrait, pour l'attribution ODbL
    importe_le INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    ouest REAL NOT NULL, sud REAL NOT NULL, est REAL NOT NULL, nord REAL NOT NULL
);

-- La silhouette : un anneau par ligne, en coordonnées géographiques.
CREATE TABLE IF NOT EXISTS frontiere (
    rang       INTEGER PRIMARY KEY,
    geometrie  BLOB NOT NULL              -- f64 LE, paires [lon, lat]
);

-- Les tronçons tels qu'OSM les découpe. Le regroupement par nom se fait à la
-- lecture : c'est une vue de l'esprit, pas une donnée.
CREATE TABLE IF NOT EXISTS troncons (
    id        INTEGER PRIMARY KEY,        -- l'identifiant OSM, conservé
    nom       TEXT,                       -- le nom RÉEL ; celui qu'on affiche est inventé ailleurs
    classe    TEXT    NOT NULL,
    longueur  REAL    NOT NULL,           -- mètres
    geometrie BLOB    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_troncons_nom ON troncons(nom);

-- Surfaces. Un seul table pour trois natures : elles ne diffèrent que par le
-- remplissage, et les séparer n'apporterait que trois requêtes au lieu d'une.
CREATE TABLE IF NOT EXISTS surfaces (
    id        INTEGER PRIMARY KEY,
    nature    TEXT NOT NULL,              -- bati | eau | vert
    geometrie BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_surfaces_nature ON surfaces(nature);

-- Les adresses d'OSM. Conservées pour mémoire et pour caler le réalisme des
-- numéros ; **elles ne portent pas nos morceaux** — seulement 13 % d'entre
-- elles citent leur rue, ce qui les rend inexploitables pour l'affectation.
CREATE TABLE IF NOT EXISTS adresses (
    rowid_ INTEGER PRIMARY KEY,
    numero TEXT NOT NULL,
    rue    TEXT,
    lon    REAL NOT NULL,
    lat    REAL NOT NULL
);

-- Les toponymes d'OSM (place=*). Ils ne seront pas affichés — on invente les
-- nôtres — mais ils disent où la ville se sent découpée, ce qui aide à poser
-- les quartiers.
CREATE TABLE IF NOT EXISTS lieux (
    rowid_ INTEGER PRIMARY KEY,
    nom    TEXT NOT NULL,
    genre  TEXT NOT NULL,
    lon    REAL NOT NULL,
    lat    REAL NOT NULL
);

-- Musées, monuments, lieux de culte : des ancres réelles, affichées telles
-- quelles — séparée de `lieux` (toponymes jamais affichés) plutôt qu'un
-- champ discriminant dessus, un vocabulaire de tags différent pour une
-- intention différente.
CREATE TABLE IF NOT EXISTS points_remarquables (
    id     INTEGER PRIMARY KEY,   -- l'identifiant OSM (nœud ou voie), conservé
    nom    TEXT NOT NULL,
    genre  TEXT NOT NULL,
    lon    REAL NOT NULL,
    lat    REAL NOT NULL
);
"#;

/// Encode une polyligne en octets : f64 petit-boutien, `lon` puis `lat`.
fn encoder(points: &[[f64; 2]]) -> Vec<u8> {
    let mut octets = Vec::with_capacity(points.len() * 16);
    for p in points {
        octets.extend_from_slice(&p[0].to_le_bytes());
        octets.extend_from_slice(&p[1].to_le_bytes());
    }
    octets
}

/// Décode ce qu'[`encoder`] a produit.
pub fn decoder(octets: &[u8]) -> Vec<[f64; 2]> {
    octets
        .chunks_exact(16)
        .map(|c| {
            [
                f64::from_le_bytes(c[0..8].try_into().expect("8 octets")),
                f64::from_le_bytes(c[8..16].try_into().expect("8 octets")),
            ]
        })
        .collect()
}

/// Écrit l'extrait dans une base neuve. Écrase ce qui s'y trouvait.
pub fn ecrire(extrait: &Extrait, chemin: &Path, nom: &str, source: &str) -> Result<()> {
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut conn = Connection::open(chemin)
        .with_context(|| format!("création de {}", chemin.display()))?;
    conn.execute_batch(SCHEMA)?;
    let tx = conn.transaction()?;
    for table in ["ville", "frontiere", "troncons", "surfaces", "adresses", "lieux", "points_remarquables"] {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
    }

    let (mut ouest, mut sud, mut est, mut nord) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut voir = |p: &[f64; 2]| {
        ouest = ouest.min(p[0]);
        est = est.max(p[0]);
        sud = sud.min(p[1]);
        nord = nord.max(p[1]);
    };
    if let Some(frontiere) = &extrait.frontiere {
        for anneau in &frontiere.anneaux {
            anneau.iter().for_each(&mut voir);
        }
    } else {
        for t in &extrait.troncons {
            t.points.iter().for_each(&mut voir);
        }
    }

    tx.execute(
        "INSERT INTO ville (id, nom, source, ouest, sud, est, nord) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![nom, source, ouest, sud, est, nord],
    )?;

    if let Some(frontiere) = &extrait.frontiere {
        let mut st = tx.prepare("INSERT INTO frontiere (rang, geometrie) VALUES (?1, ?2)")?;
        for (rang, anneau) in frontiere.anneaux.iter().enumerate() {
            st.execute(params![rang as i64, encoder(anneau)])?;
        }
    }
    {
        let mut st = tx.prepare(
            "INSERT OR REPLACE INTO troncons (id, nom, classe, longueur, geometrie) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for t in &extrait.troncons {
            st.execute(params![
                t.id,
                t.nom.as_deref(),
                t.classe.nom(),
                t.longueur_m(),
                encoder(&t.points)
            ])?;
        }
        let mut st = tx.prepare(
            "INSERT OR REPLACE INTO surfaces (id, nature, geometrie) VALUES (?1, ?2, ?3)",
        )?;
        for (nature, contours) in [
            ("bati", &extrait.batis),
            ("eau", &extrait.eaux),
            ("vert", &extrait.verts),
        ] {
            for c in contours {
                st.execute(params![c.id, nature, encoder(&c.points)])?;
            }
        }
        let mut st =
            tx.prepare("INSERT INTO adresses (numero, rue, lon, lat) VALUES (?1, ?2, ?3, ?4)")?;
        for a in &extrait.adresses {
            st.execute(params![a.numero, a.rue.as_deref(), a.point[0], a.point[1]])?;
        }
        let mut st =
            tx.prepare("INSERT INTO lieux (nom, genre, lon, lat) VALUES (?1, ?2, ?3, ?4)")?;
        for l in &extrait.lieux {
            st.execute(params![l.nom, l.genre, l.point[0], l.point[1]])?;
        }
        let mut st = tx.prepare(
            "INSERT OR REPLACE INTO points_remarquables (id, nom, genre, lon, lat) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for p in &extrait.points_remarquables {
            st.execute(params![p.id, p.nom, p.genre, p.point[0], p.point[1]])?;
        }
    }
    tx.commit()?;
    conn.execute_batch("PRAGMA optimize; VACUUM;")?;
    Ok(())
}

/// Relit ce qu'[`ecrire`] a posé.
pub fn lire(chemin: &Path) -> Result<Extrait> {
    let conn = Connection::open_with_flags(chemin, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("ouverture de {}", chemin.display()))?;
    let mut extrait = Extrait::default();

    let mut st = conn.prepare("SELECT id, nom, classe, geometrie FROM troncons")?;
    let lignes = st.query_map([], |l| {
        Ok((
            l.get::<_, i64>(0)?,
            l.get::<_, Option<String>>(1)?,
            l.get::<_, String>(2)?,
            l.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    for ligne in lignes {
        let (id, nom, classe, geometrie) = ligne?;
        extrait.troncons.push(Troncon {
            id,
            nom,
            classe: Classe::depuis_nom(&classe).unwrap_or(Classe::Service),
            points: decoder(&geometrie),
        });
    }

    let mut st = conn.prepare("SELECT id, nature, geometrie FROM surfaces")?;
    let lignes = st.query_map([], |l| {
        Ok((
            l.get::<_, i64>(0)?,
            l.get::<_, String>(1)?,
            l.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    for ligne in lignes {
        let (id, nature, geometrie) = ligne?;
        let contour = Contour {
            id,
            points: decoder(&geometrie),
        };
        match nature.as_str() {
            "bati" => extrait.batis.push(contour),
            "eau" => extrait.eaux.push(contour),
            _ => extrait.verts.push(contour),
        }
    }

    let mut st = conn.prepare("SELECT numero, rue, lon, lat FROM adresses")?;
    let lignes = st.query_map([], |l| {
        Ok(Adresse {
            numero: l.get(0)?,
            rue: l.get(1)?,
            point: [l.get(2)?, l.get(3)?],
        })
    })?;
    for ligne in lignes {
        extrait.adresses.push(ligne?);
    }

    let mut st = conn.prepare("SELECT nom, genre, lon, lat FROM lieux")?;
    let lignes = st.query_map([], |l| {
        Ok(Lieu {
            nom: l.get(0)?,
            genre: l.get(1)?,
            point: [l.get(2)?, l.get(3)?],
        })
    })?;
    for ligne in lignes {
        extrait.lieux.push(ligne?);
    }

    // `points_remarquables` est une table plus récente que le reste du
    // schéma : une base déjà importée avant son ajout ne l'a pas, et
    // `CREATE TABLE IF NOT EXISTS` (côté `ecrire`) ne la crée qu'au prochain
    // import — pas question de faire échouer la lecture d'une base ancienne
    // pour autant, même schéma « non destructif » que le reste de ce module.
    let table_existe: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'points_remarquables'",
        [],
        |l| l.get::<_, i64>(0),
    )? > 0;
    if table_existe {
        let mut st = conn.prepare("SELECT id, nom, genre, lon, lat FROM points_remarquables")?;
        let lignes = st.query_map([], |l| {
            Ok(PointRemarquable {
                id: l.get(0)?,
                nom: l.get(1)?,
                genre: l.get(2)?,
                point: [l.get(3)?, l.get(4)?],
            })
        })?;
        for ligne in lignes {
            extrait.points_remarquables.push(ligne?);
        }
    }

    let mut st = conn.prepare("SELECT geometrie FROM frontiere ORDER BY rang")?;
    let anneaux: Vec<Vec<[f64; 2]>> = st
        .query_map([], |l| l.get::<_, Vec<u8>>(0))?
        .filter_map(|g| g.ok())
        .map(|g| decoder(&g))
        .collect();
    if !anneaux.is_empty() {
        extrait.frontiere = Some(crate::Frontiere::nouvelle(anneaux));
    }
    Ok(extrait)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_aller_retour_par_la_base_conserve_la_ville() {
        let extrait = Extrait {
            troncons: vec![Troncon {
                id: 42,
                nom: Some("Rue de Rivoli".into()),
                classe: Classe::Primaire,
                points: vec![[2.34, 48.85], [2.35, 48.86]],
            }],
            adresses: vec![Adresse {
                numero: "12".into(),
                rue: Some("Rue de Rivoli".into()),
                point: [2.34, 48.85],
            }],
            batis: vec![Contour {
                id: 7,
                points: vec![[2.0, 48.0], [2.1, 48.0], [2.1, 48.1], [2.0, 48.0]],
            }],
            lieux: vec![Lieu {
                nom: "Marais".into(),
                genre: "quarter".into(),
                point: [2.36, 48.86],
            }],
            points_remarquables: vec![PointRemarquable {
                id: 99,
                nom: "Notre-Dame".into(),
                genre: "lieu_de_culte".into(),
                point: [2.3499, 48.8530],
            }],
            frontiere: Some(crate::Frontiere::nouvelle(vec![vec![
                [2.0, 48.0],
                [2.5, 48.0],
                [2.5, 48.5],
                [2.0, 48.5],
                [2.0, 48.0],
            ]])),
            ..Default::default()
        };
        let dossier = std::env::temp_dir().join(format!("ville-{}.db", std::process::id()));
        ecrire(&extrait, &dossier, "Essai", "test").unwrap();
        let relu = lire(&dossier).unwrap();
        assert_eq!(relu.troncons.len(), 1);
        assert_eq!(relu.troncons[0].nom.as_deref(), Some("Rue de Rivoli"));
        assert_eq!(relu.troncons[0].classe, Classe::Primaire);
        assert_eq!(relu.troncons[0].points, vec![[2.34, 48.85], [2.35, 48.86]]);
        assert_eq!(relu.adresses.len(), 1);
        assert_eq!(relu.batis.len(), 1);
        assert_eq!(relu.lieux.len(), 1);
        assert_eq!(relu.points_remarquables.len(), 1);
        assert_eq!(relu.points_remarquables[0].nom, "Notre-Dame");
        assert!(relu.frontiere.is_some_and(|f| f.contient([2.25, 48.25])));
        std::fs::remove_file(&dossier).ok();
    }

    /// `points_remarquables` est arrivée après le reste du schéma — une base
    /// déjà importée avant son ajout n'a pas la table. `lire` doit s'en
    /// accommoder plutôt qu'échouer ; trouvé en vrai sur `ville-paris.db`,
    /// importée avant ce correctif.
    #[test]
    fn lire_une_base_sans_points_remarquables_ne_fait_pas_tomber_lappelant() {
        let dossier = std::env::temp_dir().join(format!("ville-ancienne-{}.db", std::process::id()));
        std::fs::remove_file(&dossier).ok();
        {
            let conn = Connection::open(&dossier).unwrap();
            // Le schéma d'avant l'ajout de `points_remarquables` — juste ce
            // qu'il faut pour que `lire` ait quelque chose à lire ailleurs.
            conn.execute_batch(
                "CREATE TABLE ville (id INTEGER PRIMARY KEY, nom TEXT, source TEXT,
                    ouest REAL, sud REAL, est REAL, nord REAL);
                 CREATE TABLE frontiere (rang INTEGER PRIMARY KEY, geometrie BLOB);
                 CREATE TABLE troncons (id INTEGER PRIMARY KEY, nom TEXT, classe TEXT,
                    longueur REAL, geometrie BLOB);
                 CREATE TABLE surfaces (id INTEGER PRIMARY KEY, nature TEXT, geometrie BLOB);
                 CREATE TABLE adresses (rowid_ INTEGER PRIMARY KEY, numero TEXT, rue TEXT, lon REAL, lat REAL);
                 CREATE TABLE lieux (rowid_ INTEGER PRIMARY KEY, nom TEXT, genre TEXT, lon REAL, lat REAL);",
            )
            .unwrap();
        }
        let relu = lire(&dossier).expect("une base sans points_remarquables doit quand même se lire");
        assert!(relu.points_remarquables.is_empty());
        std::fs::remove_file(&dossier).ok();
    }
}
