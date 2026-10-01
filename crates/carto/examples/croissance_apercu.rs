// SPDX-License-Identifier: GPL-3.0-or-later
//! Évalue le peuplement chronologique de Paris : mesures chiffrées et images
//! SVG pour le juger à l'œil.
//!
//! `cargo run --release -p rusty-music-carto --example croissance_apercu -- [rusty-music.db] [ville-paris.db] [dossier-de-sortie]`
//!
//! Mesures (`evaluation.md`) : l'âge se lit-il dans la distance ? la
//! popularité s'installe-t-elle sur les artères ? les styles se regroupent-ils ?
//! Images : année de sortie, popularité, famille — plan entier et cœur de ville.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rusty_music_carto::affectation::Repere;
use rusty_music_carto::facades::facades;
use rusty_music_carto::ville::{self, ILE_DE_LA_CITE};
use rusty_music_carto::{cout_voirie, Palette};
use rusty_music_core::db::{Library, MapPoint};

/// Le modèle d'empreintes de la bibliothèque (`rusty_music_analysis::passe::MODELE`,
/// que ce crate ne dépend pas de connaître).
const MODELE: &str = "clap-htsat-unfused-5f";

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let support = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/fm.rustymusic.desktop");
    let lib_path = args.next().map(PathBuf::from).unwrap_or_else(|| support.join("rusty-music.db"));
    let ville_path = args.next().map(PathBuf::from).unwrap_or_else(|| support.join("ville-paris.db"));
    let sortie = args.next().map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("croissance-apercu"));
    std::fs::create_dir_all(&sortie)?;

    let lib = Library::open(&lib_path)?;
    let vue = lib.map_view(MODELE)?;
    let noms: HashMap<i64, String> = lib.familles(MODELE)?.into_iter().map(|(id, nom, _)| (id, nom)).collect();
    let extrait = rusty_music_osm::base::lire(&ville_path)?;
    let repere = Repere::centre_de(&extrait);
    let centre_m = repere.vers_m(ILE_DE_LA_CITE);

    let t = std::time::Instant::now();
    // Réglages surchargeables par l'environnement : `CR_MARGE=0.2 CR_W_ARTERE=3 …`.
    let mut reglages = rusty_music_carto::croissance::Parametres::default();
    let lire = |nom: &str| std::env::var(nom).ok().and_then(|v| v.parse::<f64>().ok());
    if let Some(v) = lire("CR_MARGE") { reglages.marge_ouverture = v; }
    if let Some(v) = lire("CR_BASE") { reglages.base_ouverture = v as usize; }
    if let Some(v) = lire("CR_SIGMA") { reglages.sigma_style = v; }
    if let Some(v) = lire("CR_W_FAMILLE") { reglages.w_famille = v; }
    if let Some(v) = lire("CR_W_ARTERE") { reglages.w_artere = v; }
    if let Some(v) = lire("CR_W_PROCHE") { reglages.w_proche = v; }
    if let Some(v) = lire("CR_W_ARTISTE") { reglages.w_artiste = v; }
    if let Some(v) = lire("CR_W_FONDATION") { reglages.w_fondation = v; }
    if let Some(v) = lire("CR_W_DECOUPE") { reglages.w_decoupe = v; }
    if let Some(v) = lire("CR_QUARTIER") { reglages.taille_quartier = v as usize; }
    if let Some(v) = lire("CR_COULOIR") { reglages.taille_couloir = v as usize; }
    if let Some(v) = lire("CR_SEUIL_COULOIR") { reglages.seuil_couloir = v; }
    println!("réglages : {reglages:?}");
    let r = ville::rassembler_avec(&extrait, &vue, &noms, Some(ILE_DE_LA_CITE), &reglages);
    println!(
        "croissance : {:.1} s — {} adresses, {} sans, {} cellules ({} habitées), {} ancrés",
        t.elapsed().as_secs_f64(),
        r.adresses_posees,
        r.morceaux_sans_adresse,
        r.cellules,
        r.cellules_habitees,
        r.artistes_ancres,
    );

    // Un morceau placé : position en mètres, année, popularité, famille, voie, coût.
    struct Place {
        pos: [f64; 2],
        annee: Option<i32>,
        pop: Option<f64>,
        famille: i64,
        artere: f64,
        classe: Option<rusty_music_osm::Classe>,
        cout: f64,
        dist: f64,
    }
    let par_id: HashMap<i64, &MapPoint> = vue.iter().map(|p| (p.id, p)).collect();
    let ids_pos: Vec<(i64, [f64; 2])> =
        r.source.morceaux.iter().map(|m| (m.id, repere.vers_m([m.x as f64, m.y as f64]))).collect();
    let centres: Vec<[f64; 2]> = ids_pos.iter().map(|x| x.1).collect();
    let fac = facades(&extrait, &repere, &centres, 60.0);
    let couts: HashMap<i64, f64> =
        cout_voirie::couts_batiments(&extrait, &repere, &ids_pos, ILE_DE_LA_CITE).into_iter().collect();
    let places: Vec<Place> = ids_pos
        .iter()
        .zip(&fac)
        .map(|((id, pos), f)| {
            let p = par_id[id];
            Place {
                pos: *pos,
                annee: p.year.map(|a| a as i32),
                pop: p.popularite,
                famille: p.cluster,
                artere: f.map_or(0.0, |f| f.artere),
                classe: f.map(|f| f.classe),
                cout: couts[id],
                dist: ((pos[0] - centre_m[0]).powi(2) + (pos[1] - centre_m[1]).powi(2)).sqrt(),
            }
        })
        .collect();

    let mut md = String::new();
    writeln!(md, "# Évaluation du peuplement chronologique\n")?;
    writeln!(md, "{} morceaux placés, {} cellules.\n", places.len(), r.cellules)?;

    // --- 1. L'âge se lit-il dans la distance ? --------------------------------
    let datés: Vec<&Place> = places.iter().filter(|p| p.annee.is_some()).collect();
    let a: Vec<f64> = datés.iter().map(|p| p.annee.unwrap() as f64).collect();
    let rho_cout = spearman(&a, &datés.iter().map(|p| p.cout).collect::<Vec<_>>());
    let rho_dist = spearman(&a, &datés.iter().map(|p| p.dist).collect::<Vec<_>>());
    writeln!(md, "## 1. L'âge se lit dans la distance\n")?;
    writeln!(md, "Corrélation de rang (Spearman) année ↔ distance, sur {} morceaux datés :", datés.len())?;
    writeln!(md, "- année ↔ **coût de voirie** : ρ = {rho_cout:.2}")?;
    writeln!(md, "- année ↔ distance à vol d'oiseau : ρ = {rho_dist:.2}\n")?;
    writeln!(md, "| décennie | morceaux | distance médiane (m) | distance p90 (m) | coût médian |")?;
    writeln!(md, "|---|--:|--:|--:|--:|")?;
    for dec in (1960..=2020).step_by(10) {
        let v: Vec<&&Place> = datés.iter().filter(|p| p.annee.unwrap() / 10 * 10 == dec).collect();
        if v.is_empty() {
            continue;
        }
        let d = quantile(v.iter().map(|p| p.dist).collect(), 0.5);
        let d90 = quantile(v.iter().map(|p| p.dist).collect(), 0.9);
        let c = quantile(v.iter().map(|p| p.cout).collect(), 0.5);
        writeln!(md, "| {dec}s | {} | {d:.0} | {d90:.0} | {c:.0} |", v.len())?;
    }
    let mut par_age: Vec<&&Place> = datés.iter().collect();
    par_age.sort_by_key(|p| p.annee);
    let cent: Vec<&&Place> = par_age.iter().take(100).copied().collect();
    writeln!(
        md,
        "\nLes 100 plus anciens : distance médiane {:.0} m, {} à moins de 600 m de l'île de la Cité.\n",
        quantile(cent.iter().map(|p| p.dist).collect(), 0.5),
        cent.iter().filter(|p| p.dist < 600.0).count()
    )?;

    // --- 2. La popularité s'installe-t-elle sur les artères ? -----------------
    let connues: Vec<&Place> = places.iter().filter(|p| p.pop.is_some()).collect();
    let moy = |v: &[&&Place]| v.iter().map(|p| p.pop.unwrap()).sum::<f64>() / v.len().max(1) as f64;
    let sur: Vec<&&Place> = connues.iter().filter(|p| p.artere >= 0.75).collect();
    let hors: Vec<&&Place> = connues.iter().filter(|p| p.artere < 0.75).collect();
    let haut = connues.iter().filter(|p| p.pop.unwrap() >= 0.9).count().max(1);
    let bas = connues.iter().filter(|p| p.pop.unwrap() <= 0.1).count().max(1);
    let haut_sur = connues.iter().filter(|p| p.pop.unwrap() >= 0.9 && p.artere >= 0.75).count();
    let bas_sur = connues.iter().filter(|p| p.pop.unwrap() <= 0.1 && p.artere >= 0.75).count();
    writeln!(md, "## 2. La popularité s'installe sur les artères\n")?;
    writeln!(
        md,
        "- part des morceaux sur une artère (Primaire/Secondaire) : {:.0} %",
        100.0 * sur.len() as f64 / connues.len() as f64
    )?;
    writeln!(md, "- popularité moyenne : artères {:.2}, petits quartiers {:.2}", moy(&sur), moy(&hors))?;
    writeln!(
        md,
        "- parmi les 10 % les plus populaires : {:.0} % sur une artère ; parmi les 10 % les moins populaires : {:.0} %\n",
        100.0 * haut_sur as f64 / haut as f64,
        100.0 * bas_sur as f64 / bas as f64
    )?;

    writeln!(md, "Par voie qui borde le bâtiment :\n")?;
    writeln!(md, "| voie | morceaux | popularité moy. | année médiane | distance médiane (m) |")?;
    writeln!(md, "|---|--:|--:|--:|--:|")?;
    let mut classes: Vec<Option<rusty_music_osm::Classe>> = places.iter().map(|p| p.classe).collect();
    classes.sort_by_key(|c| c.map(|c| c.nom()));
    classes.dedup();
    for c in classes {
        let v: Vec<&Place> = places.iter().filter(|p| p.classe == c).collect();
        let pops: Vec<f64> = v.iter().filter_map(|p| p.pop).collect();
        let ans: Vec<f64> = v.iter().filter_map(|p| p.annee.map(|a| a as f64)).collect();
        writeln!(
            md,
            "| {} | {} | {:.2} | {:.0} | {:.0} |",
            c.map_or("(aucune)", |c| c.nom()),
            v.len(),
            pops.iter().sum::<f64>() / pops.len().max(1) as f64,
            quantile(ans, 0.5),
            quantile(v.iter().map(|p| p.dist).collect(), 0.5)
        )?;
    }
    writeln!(md)?;

    // --- 3. Les styles se regroupent-ils ? -------------------------------------
    // Part des 8 plus proches voisins habités qui partagent la famille.
    let pas = 80.0;
    let cle = |p: [f64; 2]| ((p[0] / pas).floor() as i32, (p[1] / pas).floor() as i32);
    let mut grille: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, p) in places.iter().enumerate() {
        grille.entry(cle(p.pos)).or_default().push(i);
    }
    let (mut meme, mut tot) = (0usize, 0usize);
    for (i, p) in places.iter().enumerate().step_by(3) {
        let (cx, cy) = cle(p.pos);
        let mut cand: Vec<(f64, usize)> = Vec::new();
        for dx in -2..=2 {
            for dy in -2..=2 {
                if let Some(v) = grille.get(&(cx + dx, cy + dy)) {
                    for &j in v {
                        if j != i {
                            let d = (places[j].pos[0] - p.pos[0]).powi(2) + (places[j].pos[1] - p.pos[1]).powi(2);
                            cand.push((d, j));
                        }
                    }
                }
            }
        }
        cand.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, j) in cand.iter().take(8) {
            tot += 1;
            meme += (places[j].famille == p.famille) as usize;
        }
    }
    let mut fam: HashMap<i64, usize> = HashMap::new();
    for p in &places {
        *fam.entry(p.famille).or_default() += 1;
    }
    let hasard: f64 = fam.values().map(|&n| (n as f64 / places.len() as f64).powi(2)).sum();
    writeln!(md, "## 3. Les styles se regroupent\n")?;
    writeln!(
        md,
        "Part des 8 voisins les plus proches qui sont de la même famille : **{:.0} %** (le hasard ferait {:.0} %).\n",
        100.0 * meme as f64 / tot.max(1) as f64,
        100.0 * hasard
    )?;
    // Artistes demandés (`CR_ARTISTES="Arctic Monkeys,Puya"`) : où habitent-ils ?
    if let Ok(liste) = std::env::var("CR_ARTISTES") {
        for nom in liste.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            let ancre = r.source.artistes_places.iter().find(|a| a.nom.eq_ignore_ascii_case(nom)).and_then(|a| a.ancre.clone());
            let v: Vec<(&Place, i64)> = ids_pos
                .iter()
                .zip(&places)
                .filter(|((id, _), _)| par_id[id].album_artist.as_deref().or(par_id[id].artist.as_deref()).is_some_and(|a| a.eq_ignore_ascii_case(nom)))
                .map(|((id, _), p)| (p, *id))
                .collect();
            let ans: Vec<i32> = v.iter().filter_map(|(p, _)| p.annee).collect();
            println!(
                "{nom} : {} morceaux, années {:?}–{:?}, distance médiane {:.0} m, ancré : {:?}",
                v.len(),
                ans.iter().min(),
                ans.iter().max(),
                quantile(v.iter().map(|(p, _)| p.dist).collect(), 0.5),
                ancre
            );
        }
    }
    std::fs::write(sortie.join("evaluation.md"), &md)?;
    println!("\n{md}");

    // --- Images ------------------------------------------------------------------
    let pal = *Palette::par_id("osm-clair").expect("palette");
    let cadre = cadre_frontiere(&r.source, &repere);
    let z = 1500.0;
    let cadre_centre = [centre_m[0] - z, centre_m[1] - z, centre_m[0] + z, centre_m[1] + z];
    let c_annee: Vec<String> = places.iter().map(|p| couleur_annee(p.annee)).collect();
    let c_pop: Vec<String> = places.iter().map(|p| couleur_pop(p.pop)).collect();
    let c_fam: Vec<String> = places.iter().map(|p| famille_coul(&pal, p.famille)).collect();
    for (nom, cadre, large) in [("", cadre, 1600.0), ("-centre", cadre_centre, 1400.0)] {
        image(&sortie.join(format!("01-annee{nom}.svg")), &r.source, &repere, &pal, &c_annee, cadre, large)?;
        image(&sortie.join(format!("02-popularite{nom}.svg")), &r.source, &repere, &pal, &c_pop, cadre, large)?;
        image(&sortie.join(format!("03-familles{nom}.svg")), &r.source, &repere, &pal, &c_fam, cadre, large)?;
    }
    // Les aplats de quartier (ce que la carte montre en dézoomant).
    {
        let [x0, y0, x1, y1] = cadre;
        let (largeur, h) = (1600.0, 1600.0 * (y1 - y0) / (x1 - x0));
        let xy = |p: [f64; 2]| ((p[0] - x0) / (x1 - x0) * largeur, (y1 - p[1]) / (y1 - y0) * h);
        let mut svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='{largeur:.0}' height='{h:.0}' viewBox='0 0 {largeur:.0} {h:.0}'><rect width='100%' height='100%' fill='#f4f2ee'/>");
        for t in &r.source.territoires_reels {
            let c = famille_coul(&pal, t.famille);
            for poly in &t.polygones {
                svg.push_str("<path fill-rule='evenodd' fill='");
                svg.push_str(&c);
                svg.push_str("' fill-opacity='0.75' d='");
                for anneau in poly {
                    for (i, q) in anneau.iter().enumerate() {
                        let (x, y) = xy(repere.vers_m(*q));
                        let _ = write!(svg, "{}{x:.1} {y:.1} ", if i == 0 { "M" } else { "L" });
                    }
                    svg.push('Z');
                }
                svg.push_str("'/>");
            }
        }
        svg.push_str("</svg>");
        std::fs::write(sortie.join("04-territoires.svg"), svg)?;
    }
    println!("images : {}", sortie.display());
    Ok(())
}

fn famille_coul(pal: &Palette, f: i64) -> String {
    if f < 0 { pal.autres.to_string() } else { pal.familles[f as usize % pal.familles.len()].to_string() }
}

/// Bleu (ancien) → rouge (récent) en passant par le jaune ; gris si non daté.
fn couleur_annee(a: Option<i32>) -> String {
    let Some(a) = a else { return "#999999".into() };
    let t = ((a - 1960) as f64 / 66.0).clamp(0.0, 1.0);
    rampe(t)
}

/// Clair (confidentiel) → foncé (tête d'affiche).
fn couleur_pop(p: Option<f64>) -> String {
    let Some(p) = p else { return "#bbbbbb".into() };
    rampe(p.clamp(0.0, 1.0))
}

fn rampe(t: f64) -> String {
    const STOPS: [[f64; 3]; 5] =
        [[49.0, 54.0, 149.0], [69.0, 117.0, 180.0], [254.0, 224.0, 144.0], [244.0, 109.0, 67.0], [165.0, 0.0, 38.0]];
    let x = t * 4.0;
    let i = (x.floor() as usize).min(3);
    let f = x - i as f64;
    let c: Vec<u8> = (0..3).map(|k| (STOPS[i][k] + (STOPS[i + 1][k] - STOPS[i][k]) * f) as u8).collect();
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn spearman(a: &[f64], b: &[f64]) -> f64 {
    fn rangs(v: &[f64]) -> Vec<f64> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&x, &y| v[x].total_cmp(&v[y]));
        let mut r = vec![0.0; v.len()];
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
                j += 1;
            }
            let moy = (i + j) as f64 / 2.0;
            for k in i..=j {
                r[idx[k]] = moy;
            }
            i = j + 1;
        }
        r
    }
    let (ra, rb) = (rangs(a), rangs(b));
    let n = a.len() as f64;
    let (ma, mb) = (ra.iter().sum::<f64>() / n, rb.iter().sum::<f64>() / n);
    let cov: f64 = ra.iter().zip(&rb).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = ra.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = rb.iter().map(|y| (y - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

fn quantile(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(((v.len() as f64 * q) as usize).min(v.len().saturating_sub(1))).copied().unwrap_or(0.0)
}

fn cadre_frontiere(source: &rusty_music_carto::source::Source, repere: &Repere) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for a in source.frontiere.iter().flatten() {
        for p in a {
            let m = repere.vers_m(*p);
            b = [b[0].min(m[0]), b[1].min(m[1]), b[2].max(m[0]), b[3].max(m[1])];
        }
    }
    if !b[0].is_finite() {
        b = [-6000.0, -6000.0, 6000.0, 6000.0];
    }
    let (mx, my) = ((b[2] - b[0]) * 0.03, (b[3] - b[1]) * 0.03);
    [b[0] - mx, b[1] - my, b[2] + mx, b[3] + my]
}

/// Une image : le contexte (eau, verts, frontière), les bâtiments habités en
/// pastilles colorées, les grands axes en traits.
fn image(
    chemin: &Path,
    source: &rusty_music_carto::source::Source,
    repere: &Repere,
    pal: &Palette,
    couleurs: &[String],
    cadre: [f64; 4],
    largeur: f64,
) -> std::io::Result<()> {
    let [x0, y0, x1, y1] = cadre;
    let h = largeur * (y1 - y0) / (x1 - x0);
    let xy = |p: [f64; 2]| ((p[0] - x0) / (x1 - x0) * largeur, (y1 - p[1]) / (y1 - y0) * h);
    let mut s = String::with_capacity(4_000_000);
    let _ = write!(s, "<svg xmlns='http://www.w3.org/2000/svg' width='{largeur:.0}' height='{h:.0}' viewBox='0 0 {largeur:.0} {h:.0}'><rect width='100%' height='100%' fill='#f4f2ee'/>");
    let forme = |s: &mut String, pts: &[[f64; 2]], fill: &str| {
        s.push_str("<path d='M");
        for (i, p) in pts.iter().enumerate() {
            let (x, y) = xy(repere.vers_m(*p));
            let _ = write!(s, "{}{x:.1} {y:.1} ", if i == 0 { "" } else { "L" });
        }
        let _ = write!(s, "' fill='{fill}'/>");
    };
    for c in &source.verts {
        forme(&mut s, &c.points, pal.vert);
    }
    for c in &source.eaux {
        forme(&mut s, &c.points, pal.mer);
    }
    // Les grands axes, en traits fins gris.
    for t in &source.troncons_reels {
        if matches!(t.classe, rusty_music_osm::Classe::Primaire | rusty_music_osm::Classe::Secondaire) {
            s.push_str("<polyline fill='none' stroke='#9a968c' stroke-width='0.6' points='");
            for p in &t.points {
                let (x, y) = xy(repere.vers_m(*p));
                let _ = write!(s, "{x:.1},{y:.1} ");
            }
            s.push_str("'/>");
        }
    }
    let rayon = (largeur / (x1 - x0) * 9.0).clamp(0.9, 6.0);
    for (m, c) in source.morceaux.iter().zip(couleurs) {
        let (x, y) = xy(repere.vers_m([m.x as f64, m.y as f64]));
        if x >= -5.0 && y >= -5.0 && x <= largeur + 5.0 && y <= h + 5.0 {
            let _ = write!(s, "<circle cx='{x:.1}' cy='{y:.1}' r='{rayon:.1}' fill='{c}'/>");
        }
    }
    s.push_str("</svg>");
    std::fs::write(chemin, s)
}
