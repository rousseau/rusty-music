//! Combien de fenêtres faut-il pour décrire un morceau ?
//!
//! L'encodeur HTSAT prend dix secondes. Trois fenêtres couvrent 12 % d'un
//! morceau de quatre minutes ; AudioMuse-AI, lui, passe le morceau entier.
//! Maintenant que le positionnement rend le décodage sept fois moins cher, on
//! peut se payer davantage de fenêtres — encore faut-il que ça serve.
//!
//! **Ça ne sert pas.** Poussé jusqu'à 25 fenêtres (250 s — la quasi-totalité
//! de la plupart des morceaux de la bibliothèque), le rapport au hasard ne
//! bouge plus depuis 5 : 0,52-0,53× sur toute la plage 5-25, contre 0,58× à
//! une seule fenêtre. Le plafond est atteint à 50 s, pas repoussé par le
//! reste du morceau. Couvrir davantage coûte (environ 5× plus de calcul par
//! morceau, mesuré : 2,53 s à 25 fenêtres contre 0,36 s à 5 sur SSD) sans
//! rien rendre en échange — voir `docs/journal.md`, section empreintes.
//!
//! Le juge est le même qu'au jalon 2, et il est indépendant du modèle :
//! l'album et l'artiste viennent des tags, que le réseau n'a jamais vus. Une
//! représentation est meilleure si elle rapproche les morceaux d'un même
//! album **davantage** que ne le ferait le hasard.
//!
//!   cargo run --release -p rusty-music-analysis --example couverture -- <base.db> [albums]
//!
//! Toutes les fenêtres de `MAX` sont décodées **une seule fois** : les
//! fenêtrages plus courts en sont des sous-ensembles exacts
//! (`fractions(3)` ⊂ `fractions(MAX)`). Les variantes comparées portent donc
//! exactement le même audio — la seule différence est le nombre de fenêtres
//! moyennées.

use std::collections::HashMap;
use std::time::Instant;

use rusty_music_analysis::decode::{fenetres, fractions};
use rusty_music_analysis::mel::Mel;
use rusty_music_analysis::projection::normaliser;
use rusty_music_analysis::Embedder;
use rusty_music_core::Library;

/// Fenêtrages comparés. Chacun doit diviser le plus grand pour que ses
/// positions en soient un sous-ensemble exact.
const VARIANTES: [usize; 6] = [1, 3, 5, 9, 15, 25];
const MAX: usize = 25;

/// Indices, dans les `MAX` fenêtres décodées, de celles que retient `n`.
fn indices(n: usize) -> Vec<usize> {
    let toutes = fractions(MAX);
    fractions(n)
        .iter()
        .map(|f| {
            toutes
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    (a.1 - f)
                        .abs()
                        .partial_cmp(&(b.1 - f).abs())
                        .expect("fractions finies")
                })
                .map(|(i, _)| i)
                .expect("au moins une fenêtre")
        })
        .collect()
}

/// Moyenne des fenêtres retenues, ramenée sur la sphère — comme `passe`.
fn agreger(vecteurs: &[Vec<f32>], garde: &[usize]) -> Vec<f32> {
    let mut somme = vec![0.0f32; vecteurs[0].len()];
    for i in garde {
        for (s, x) in somme.iter_mut().zip(&vecteurs[*i]) {
            *s += x / garde.len() as f32;
        }
    }
    normaliser(&somme)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| "rusty-music.db".into());
    let albums: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(40);

    let lib = Library::open(std::path::Path::new(&base))?;

    // Des albums entiers, pas des morceaux épars : sans plusieurs morceaux du
    // même album, la mesure « même album » n'a aucune paire à se mettre sous
    // la dent. `rowid % 7` échantillonne la bibliothèque sans la parcourir en
    // ordre alphabétique, qui la trierait par artiste.
    let mut stmt = lib.conn.prepare(
        "SELECT id, path, album, COALESCE(album_artist, artist) AS art
           FROM tracks
          WHERE album IS NOT NULL AND album <> ''
            AND album IN (
                SELECT album FROM tracks
                 WHERE album IS NOT NULL AND album <> '' AND id % 7 = 0
                 GROUP BY album HAVING COUNT(*) >= 4
                 LIMIT ?1)
          ORDER BY album, track_no",
    )?;
    let pistes: Vec<(i64, String, String, String)> = stmt
        .query_map([albums], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<_, _>>()?;

    println!(
        "{} morceaux, {} albums — {MAX} fenêtres décodées par morceau",
        pistes.len(),
        pistes
            .iter()
            .map(|p| &p.2)
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
    if pistes.len() < 20 {
        eprintln!("pas assez de morceaux pour juger");
        return Ok(());
    }

    let mel = Mel::new();
    let mut enc = Embedder::charger(
        None,
        std::thread::available_parallelism().map_or(1, |n| n.get()),
    )?;

    let garde: HashMap<usize, Vec<usize>> = VARIANTES.iter().map(|n| (*n, indices(*n))).collect();
    let mut empreintes: HashMap<usize, Vec<Vec<f32>>> =
        VARIANTES.iter().map(|n| (*n, Vec::new())).collect();
    let mut retenues: Vec<(String, String)> = Vec::new();
    let mut echecs = 0;

    let t = Instant::now();
    for (i, (_, chemin, album, artiste)) in pistes.iter().enumerate() {
        if i % 25 == 0 {
            eprint!("\r  {i}/{}", pistes.len());
        }
        let blocs = match fenetres(std::path::Path::new(chemin), MAX) {
            Ok(b) if b.len() == MAX => b,
            _ => {
                echecs += 1;
                continue;
            }
        };
        let spec: Vec<f32> = blocs.iter().flat_map(|b| mel.spectrogramme(b)).collect();
        let vecteurs = enc.empreintes(&spec, MAX)?;
        for n in VARIANTES {
            empreintes
                .get_mut(&n)
                .expect("variante déclarée")
                .push(agreger(&vecteurs, &garde[&n]));
        }
        retenues.push((album.clone(), artiste.clone()));
    }
    eprintln!(
        "\r  {} morceaux analysés, {echecs} en échec — {:.0} s",
        retenues.len(),
        t.elapsed().as_secs_f64()
    );

    println!(
        "\n{:>9} {:>10} {:>12} {:>12} {:>10}",
        "fenêtres", "couverture", "même album", "même artiste", "hasard"
    );
    println!("{}", "─".repeat(58));

    for n in VARIANTES {
        let v = &empreintes[&n];
        let (mut alb, mut nalb) = (0.0f64, 0usize);
        let (mut art, mut nart) = (0.0f64, 0usize);
        let (mut tout, mut ntout) = (0.0f64, 0usize);

        for i in 0..v.len() {
            for j in i + 1..v.len() {
                // Distance cosinus : les empreintes sont unitaires.
                let d = 1.0 - v[i].iter().zip(&v[j]).map(|(a, b)| a * b).sum::<f32>() as f64;
                tout += d;
                ntout += 1;
                if retenues[i].0 == retenues[j].0 {
                    alb += d;
                    nalb += 1;
                }
                if !retenues[i].1.is_empty() && retenues[i].1 == retenues[j].1 {
                    art += d;
                    nart += 1;
                }
            }
        }

        let m = |s: f64, k: usize| if k > 0 { s / k as f64 } else { f64::NAN };
        let (ma, mr, mt) = (m(alb, nalb), m(art, nart), m(tout, ntout));
        println!(
            "{n:>9} {:>9.0} s {:>7.3} ({:.2}×) {:>7.3} ({:.2}×) {mt:>10.3}",
            n as f64 * FENETRE_S,
            ma,
            ma / mt,
            mr,
            mr / mt
        );
    }

    println!(
        "\nColonnes « × » : rapport au hasard, plus bas vaut mieux.\n\
         {} paires même album, {} paires même artiste.",
        compter(&retenues, |a, b| a.0 == b.0),
        compter(&retenues, |a, b| !a.1.is_empty() && a.1 == b.1)
    );
    Ok(())
}

/// Durée d'une fenêtre, en secondes — pour la colonne « couverture ».
const FENETRE_S: f64 = 10.0;

fn compter(
    v: &[(String, String)],
    predicat: impl Fn(&(String, String), &(String, String)) -> bool,
) -> usize {
    let mut n = 0;
    for i in 0..v.len() {
        for j in i + 1..v.len() {
            if predicat(&v[i], &v[j]) {
                n += 1;
            }
        }
    }
    n
}
