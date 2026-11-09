// SPDX-License-Identifier: GPL-3.0-or-later
//! Banc d'empreinte (G1) : CLAP contre d'autres modèles, sur le même
//! échantillon. Le calcul des autres empreintes (MuQ-MuLan, CLaMP 3) se fait
//! en Python dans `experiments/banc-empreinte/` ; cet exemple fournit
//! l'échantillon et mesure tout le monde de la même façon.
//!
//! **`exporter <base> <dossier>`** : tire 2 000 morceaux (graine fixe) parmi
//! ceux dont le genre résolu figure dans le vocabulaire des familles — la
//! famille y est donc le genre déclaré, jamais le repli acoustique sur CLAP,
//! qui avantagerait CLAP. Écrit `echantillon.tsv`, les empreintes CLAP de la
//! base (`clap.f32`, n × 512, f32 petit-boutiste) et, si
//! `requetes.tsv` est là, la tour texte CLAP de l'application sur la colonne
//! `etape` (`clap-texte.f32`).
//!
//! **`fenetres <dossier> <sortie> [n]`** : l'audio que CLAP a analysé (cinq
//! fenêtres de 10 s, mono 48 kHz, décodeur de l'application), en i16, un
//! fichier par morceau (≈ 4,8 Mo ; 9,6 Go pour 2 000 : sous `RUSTY_DEV`).
//!
//! **`mesurer <dossier> <modele>…`** : pour chaque `<modele>.f32` (même ordre
//! que l'échantillon), pureté de genre exact et de famille des 12 voisins dans
//! l'empreinte (cosinus) et après t-SNE au réglage de l'application
//! (`projection::projeter`, paramètres de la base, pas `n / 48`) et au pas
//! ×32 (V10) ; distance cosinus médiane entre morceaux d'une même famille
//! pris dans des albums différents, et entre familles (P7). Avec
//! `<modele>-texte.f32`, les 10 premiers morceaux de chaque requête ; tous
//! les modèles sont alors fondus dans `annoter.tsv` (sans dire qui a rendu
//! quoi) et `cle.tsv` (les rangs par modèle).
//!
//!   cargo run --release -p rusty-music-analysis --example banc_empreinte -- exporter <base.db> <dossier>
//!   cargo run --release -p rusty-music-analysis --example banc_empreinte -- mesurer <dossier> clap muq-mulan clamp3

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusty_music_analysis::alea::Alea;
use rusty_music_analysis::chemin::K_VOISINS;
use rusty_music_analysis::passe::MODELE;
use rusty_music_analysis::projection::{normaliser, projeter};
use rusty_music_core::Library;

const TAILLE: usize = 2_000;
const GRAINE: u64 = 0x0061_BA9C;
const PREMIERS: usize = 10;

struct Morceau {
    id: i64,
    chemin: String,
    duree_s: f64,
    artiste: String,
    titre: String,
    album: String,
    genre: String,
    famille: usize,
    nom_famille: String,
}

fn ecrire_f32(chemin: &Path, vecteurs: &[Vec<f32>]) -> std::io::Result<()> {
    let mut f = std::io::BufWriter::new(fs::File::create(chemin)?);
    for v in vecteurs {
        for x in v {
            f.write_all(&x.to_le_bytes())?;
        }
    }
    Ok(())
}

fn lire_f32(chemin: &Path, n: usize) -> std::io::Result<Vec<Vec<f32>>> {
    let octets = fs::read(chemin)?;
    let tout: Vec<f32> = octets
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    assert!(
        n > 0 && tout.len().is_multiple_of(n),
        "{} : {} valeurs pour {n} lignes",
        chemin.display(),
        tout.len()
    );
    let d = tout.len() / n;
    Ok(tout.chunks_exact(d).map(<[f32]>::to_vec).collect())
}

fn propre(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn lire_echantillon(dossier: &Path) -> Vec<Morceau> {
    let texte = fs::read_to_string(dossier.join("echantillon.tsv")).expect("echantillon.tsv");
    texte
        .lines()
        .skip(1)
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            Morceau {
                id: c[0].parse().unwrap(),
                chemin: c[1].into(),
                duree_s: c[2].parse().unwrap(),
                artiste: c[3].into(),
                titre: c[4].into(),
                album: c[5].into(),
                genre: c[6].into(),
                famille: c[7].parse().unwrap(),
                nom_famille: c[8].into(),
            }
        })
        .collect()
}

/// Requêtes : `id \t demande \t etape`, en-tête compris.
fn lire_requetes(dossier: &Path) -> Vec<(String, String, String)> {
    let Ok(texte) = fs::read_to_string(dossier.join("requetes.tsv")) else {
        return Vec::new();
    };
    texte
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c[0].into(), c[1].into(), c[2].into())
        })
        .collect()
}

fn exporter(base: &Path, dossier: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(dossier)?;
    let lib = Library::open(base)?;
    let vocabulaire = lib.vocabulaire_familles()?;
    let famille_du_genre: HashMap<&str, usize> = vocabulaire
        .iter()
        .enumerate()
        .flat_map(|(i, (_, alias))| alias.iter().map(move |a| (a.as_str(), i)))
        .collect();
    let genres = lib.genres_resolus(MODELE)?;
    let empreintes: HashMap<i64, Vec<f32>> = lib.embeddings(MODELE)?.into_iter().collect();

    let mut candidats: Vec<i64> = empreintes
        .keys()
        .copied()
        .filter(|id| {
            genres
                .get(id)
                .is_some_and(|g| famille_du_genre.contains_key(g.as_str()))
        })
        .collect();
    candidats.sort_unstable();
    println!(
        "{} empreintes · {} à genre résolu · {} ancrés dans le vocabulaire ({} familles)",
        empreintes.len(),
        genres.len(),
        candidats.len(),
        vocabulaire.len()
    );
    // Fisher-Yates, graine fixe.
    let mut alea = Alea::depuis(GRAINE);
    for i in (1..candidats.len()).rev() {
        let j = alea.borne(i + 1);
        candidats.swap(i, j);
    }
    let mut tires: Vec<i64> = Vec::new();
    for id in candidats {
        if tires.len() == TAILLE {
            break;
        }
        let Some(t) = lib.track(id)? else { continue };
        // Les fichiers absents ne servent à rien aux autres modèles.
        if !Path::new(&t.path).exists() {
            continue;
        }
        tires.push(id);
    }
    tires.sort_unstable();

    let mut tsv =
        String::from("id\tchemin\tduree_s\tartiste\ttitre\talbum\tgenre\tfamille\tnom_famille\n");
    let mut vecteurs = Vec::new();
    for &id in &tires {
        let t = lib.track(id)?.expect("morceau tiré");
        let genre = &genres[&id];
        let f = famille_du_genre[genre.as_str()];
        tsv.push_str(&format!(
            "{id}\t{}\t{:.3}\t{}\t{}\t{}\t{genre}\t{f}\t{}\n",
            propre(&t.path),
            t.duration_ms.unwrap_or(0) as f64 / 1000.0,
            propre(t.artist.as_deref().unwrap_or("")),
            propre(t.title.as_deref().unwrap_or("")),
            propre(t.album.as_deref().unwrap_or("")),
            propre(&vocabulaire[f].0),
        ));
        vecteurs.push(empreintes[&id].clone());
    }
    fs::write(dossier.join("echantillon.tsv"), tsv)?;
    let params = lib.parametres_carte()?;
    fs::write(
        dossier.join("reglage.txt"),
        format!("{} {}\n", params.perplexite, params.epoques),
    )?;
    ecrire_f32(&dossier.join("clap.f32"), &vecteurs)?;
    println!("{} morceaux tirés → {}", tires.len(), dossier.display());

    let requetes = lire_requetes(dossier);
    if !requetes.is_empty() {
        let enc = rusty_music_analysis::EmbedderTexte::charger(None)?;
        let debut = Instant::now();
        let textes: Vec<Vec<f32>> = requetes
            .iter()
            .map(|(_, _, e)| enc.embed(e))
            .collect::<Result<_, _>>()?;
        ecrire_f32(&dossier.join("clap-texte.f32"), &textes)?;
        println!(
            "{} requêtes encodées par la tour texte CLAP en {:.1} s",
            textes.len(),
            debut.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

fn cosinus(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Les `k` plus proches voisins de chaque point, en distance euclidienne
/// (sur des vecteurs unitaires, c'est l'ordre du cosinus).
fn voisins(points: &[Vec<f32>], k: usize) -> Vec<Vec<usize>> {
    (0..points.len())
        .map(|i| {
            let mut d: Vec<(f32, usize)> = points
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(j, p)| {
                    (
                        p.iter()
                            .zip(&points[i])
                            .map(|(a, b)| (a - b) * (a - b))
                            .sum(),
                        j,
                    )
                })
                .collect();
            d.select_nth_unstable_by(k - 1, |a, b| a.0.total_cmp(&b.0));
            d.truncate(k);
            d.into_iter().map(|(_, j)| j).collect()
        })
        .collect()
}

/// Pureté de genre exact et de famille des voisins.
fn purete(v: &[Vec<usize>], m: &[Morceau]) -> (f64, f64) {
    let (mut g, mut f, mut n) = (0usize, 0usize, 0usize);
    for (i, vs) in v.iter().enumerate() {
        for &j in vs {
            g += usize::from(m[i].genre == m[j].genre);
            f += usize::from(m[i].famille == m[j].famille);
            n += 1;
        }
    }
    (g as f64 / n as f64, f as f64 / n as f64)
}

fn conservation(a: &[Vec<usize>], b: &[Vec<usize>]) -> f64 {
    let communs: usize = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let y: HashSet<_> = y.iter().collect();
            x.iter().filter(|j| y.contains(j)).count()
        })
        .sum();
    communs as f64 / (a.len() * a[0].len()) as f64
}

fn mediane(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    let m = v.len() / 2;
    *v.select_nth_unstable_by(m, f32::total_cmp).1
}

/// t-SNE au pas multiplié par `facteur` (1 = l'application).
fn tsne(unites: &[Vec<f32>], perplexite: f32, epoques: usize, facteur: f32) -> Vec<Vec<f32>> {
    if facteur == 1.0 {
        return projeter(unites, perplexite, epoques)
            .into_iter()
            .map(|p| vec![p.x, p.y])
            .collect();
    }
    let perplexite = perplexite.min((unites.len() as f32 - 1.0) / 3.0).max(2.0);
    let pas = (unites.len() as f32 / 48.0).max(50.0) * facteur;
    let mut t: bhtsne::tSNE<f32, Vec<f32>, 2> = bhtsne::tSNE::new(unites);
    t.perplexity(perplexite).epochs(epoques).learning_rate(pas);
    t.barnes_hut(0.5, |a, b| {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt()
    });
    t.embedding()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| c.to_vec())
        .collect()
}

fn mesurer(dossier: &Path, modeles: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let m = lire_echantillon(dossier);
    let n = m.len();
    let requetes = lire_requetes(dossier);
    // Les réglages de la carte, relevés dans la base à l'export.
    let reglage = fs::read_to_string(dossier.join("reglage.txt"))?;
    let mut r = reglage.split_whitespace();
    let perplexite: f32 = r.next().unwrap().parse()?;
    let epoques: usize = r.next().unwrap().parse()?;
    let mut lignes = Vec::new();
    // (requête, morceau) → rang par modèle.
    let mut rangs: BTreeMap<(usize, usize), BTreeMap<String, usize>> = BTreeMap::new();

    for modele in modeles {
        let chemin = dossier.join(format!("{modele}.f32"));
        if !chemin.exists() {
            println!("{modele} : {} absent, passé", chemin.display());
            continue;
        }
        let brutes = lire_f32(&chemin, n)?;
        let d = brutes[0].len();
        let unites: Vec<Vec<f32>> = brutes.iter().map(|v| normaliser(v)).collect();
        let debut = Instant::now();
        let v_hd = voisins(&unites, K_VOISINS);
        let (g_hd, f_hd) = purete(&v_hd, &m);

        let mut apres = Vec::new();
        for facteur in [1.0f32, 32.0] {
            let p = tsne(&unites, perplexite, epoques, facteur);
            let v2 = voisins(&p, K_VOISINS);
            let (g, f) = purete(&v2, &m);
            apres.push((g, f, conservation(&v_hd, &v2)));
        }

        // P7 : même famille, albums différents ; familles différentes.
        let mut intra = Vec::new();
        let mut inter = Vec::new();
        for i in 0..n {
            for j in (i + 1)..n {
                let dist = 1.0 - cosinus(&unites[i], &unites[j]);
                if m[i].famille != m[j].famille {
                    inter.push(dist);
                } else if m[i].album.is_empty()
                    || m[j].album.is_empty()
                    || (&m[i].artiste, &m[i].album) != (&m[j].artiste, &m[j].album)
                {
                    intra.push(dist);
                }
            }
        }
        let (di, de) = (mediane(intra), mediane(inter));
        println!(
            "{modele} ({d} d, {:.0} s) : 512d genre {:.1} % famille {:.1} % · \
             t-SNE app genre {:.1} % famille {:.1} % (conservation {:.1} %) · \
             t-SNE ×32 genre {:.1} % famille {:.1} % (conservation {:.1} %) · \
             intra {di:.3} inter {de:.3} rapport {:.3}",
            debut.elapsed().as_secs_f64(),
            100.0 * g_hd,
            100.0 * f_hd,
            100.0 * apres[0].0,
            100.0 * apres[0].1,
            100.0 * apres[0].2,
            100.0 * apres[1].0,
            100.0 * apres[1].1,
            100.0 * apres[1].2,
            di / de
        );
        lignes.push(format!(
            "| {modele} | {d} | {:.1} % | {:.1} % | {:.1} % | {:.1} % | {:.1} % | {:.1} % | {di:.3} | {de:.3} | {:.3} |",
            100.0 * g_hd,
            100.0 * f_hd,
            100.0 * apres[0].0,
            100.0 * apres[0].1,
            100.0 * apres[1].0,
            100.0 * apres[1].1,
            di / de
        ));

        let chemin_texte = dossier.join(format!("{modele}-texte.f32"));
        if !requetes.is_empty() && chemin_texte.exists() {
            let textes = lire_f32(&chemin_texte, requetes.len())?;
            assert_eq!(
                textes[0].len(),
                d,
                "{modele} : texte et audio de tailles différentes"
            );
            let mut sortie = String::from("requete\trang\tid\tartiste\ttitre\tgenre\tscore\n");
            for (q, t) in textes.iter().enumerate() {
                let t = normaliser(t);
                let mut s: Vec<(f32, usize)> = unites
                    .iter()
                    .enumerate()
                    .map(|(i, u)| (cosinus(&t, u), i))
                    .collect();
                s.sort_by(|a, b| b.0.total_cmp(&a.0));
                for (r, (score, i)) in s.iter().take(PREMIERS).enumerate() {
                    sortie.push_str(&format!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{score:.4}\n",
                        requetes[q].0,
                        r + 1,
                        m[*i].id,
                        m[*i].artiste,
                        m[*i].titre,
                        m[*i].genre
                    ));
                    rangs
                        .entry((q, *i))
                        .or_default()
                        .insert(modele.clone(), r + 1);
                }
            }
            fs::write(dossier.join(format!("premiers-{modele}.tsv")), sortie)?;
        }
    }

    println!(
        "\n| modèle | dim | genre (empreinte) | famille (empreinte) | genre (t-SNE app) | famille (t-SNE app) | genre (t-SNE ×32) | famille (t-SNE ×32) | intra-famille | inter-familles | rapport |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for l in &lignes {
        println!("{l}");
    }

    if !rangs.is_empty() {
        // À annoter à l'aveugle : l'ordre des lignes ne dit rien du modèle
        // (tri par requête puis par identifiant).
        let mut annoter = String::from(
            "requete\tdemande\tetape\tid\tartiste\ttitre\talbum\tgenre\tchemin\tpertinent (o/n)\n",
        );
        let mut cle = String::from("requete\tid");
        let noms: Vec<&String> = modeles.iter().collect();
        for nom in &noms {
            cle.push_str(&format!("\t{nom}"));
        }
        cle.push('\n');
        for ((q, i), r) in &rangs {
            let (rid, demande, etape) = &requetes[*q];
            let x = &m[*i];
            annoter.push_str(&format!(
                "{rid}\t{demande}\t{etape}\t{}\t{}\t{}\t{}\t{}\t{}\t\n",
                x.id, x.artiste, x.titre, x.album, x.genre, x.chemin
            ));
            cle.push_str(&format!("{rid}\t{}", x.id));
            for nom in &noms {
                match r.get(*nom) {
                    Some(rang) => cle.push_str(&format!("\t{rang}")),
                    None => cle.push('\t'),
                }
            }
            cle.push('\n');
        }
        fs::write(dossier.join("annoter.tsv"), annoter)?;
        fs::write(dossier.join("cle.tsv"), cle)?;
        println!(
            "\n{} lignes à annoter → {}",
            rangs.len(),
            dossier.join("annoter.tsv").display()
        );
    }
    // Quelques repères sur l'échantillon.
    let mut par_famille: BTreeMap<&str, usize> = BTreeMap::new();
    for x in &m {
        *par_famille.entry(x.nom_famille.as_str()).or_default() += 1;
    }
    let genres: HashSet<&str> = m.iter().map(|x| x.genre.as_str()).collect();
    let duree: f64 = m.iter().map(|x| x.duree_s).sum();
    println!(
        "\néchantillon : {n} morceaux, {} genres, {:.0} h ; familles : {par_famille:?}",
        genres.len(),
        duree / 3600.0
    );
    Ok(())
}

/// Les fenêtres que CLAP a analysées (`decode::fenetres`, 5 × 10 s, mono
/// 48 kHz), en i16 petit-boutiste, un fichier `<id>.i16` par morceau : les
/// autres modèles entendent exactement le même audio. `limite` : les n
/// premiers morceaux seulement (essai).
fn fenetres(
    dossier: &Path,
    sortie: &Path,
    limite: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    use rusty_music_analysis::decode;
    fs::create_dir_all(sortie)?;
    let m = lire_echantillon(dossier);
    let debut = Instant::now();
    let mut echecs = 0;
    for x in m.iter().take(limite) {
        let cible = sortie.join(format!("{}.i16", x.id));
        if cible.exists() {
            continue;
        }
        match decode::fenetres(Path::new(&x.chemin), decode::FENETRES) {
            Ok(blocs) => {
                let mut octets = Vec::new();
                for b in &blocs {
                    for s in b {
                        octets.extend_from_slice(
                            &((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes(),
                        );
                    }
                }
                fs::write(&cible, octets)?;
            }
            Err(e) => {
                echecs += 1;
                eprintln!("{} : {e}", x.id);
            }
        }
    }
    println!(
        "fenêtres de {} morceaux en {:.0} s ({echecs} échecs) → {}",
        m.len().min(limite),
        debut.elapsed().as_secs_f64(),
        sortie.display()
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("exporter") if args.len() == 3 => {
            exporter(Path::new(&args[1]), &PathBuf::from(&args[2]))
        }
        Some("fenetres") if args.len() >= 3 => fenetres(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args.get(3).map_or(usize::MAX, |n| n.parse().unwrap()),
        ),
        Some("mesurer") if args.len() >= 3 => mesurer(Path::new(&args[1]), &args[2..]),
        _ => {
            eprintln!(
                "usage : banc_empreinte exporter <base.db> <dossier>\n        banc_empreinte fenetres <dossier> <sortie> [n]\n        banc_empreinte mesurer <dossier> <modele>…"
            );
            std::process::exit(2)
        }
    }
}
