//! Évaluation qualitative du placement des morceaux sur le plan de ville, et
//! images SVG pour la juger à l'œil.
//!
//! `cargo run --release -p rusty-music-cli --example apercu_placement -- [rusty-music.db] [ville-paris.db]`
//!
//! Écrit dans `/tmp/rusty-music-apercu/` : cinq SVG (ouvrables tels quels),
//! `evaluation.md`, et `index.html` qui les montre côte à côte.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusty_music_carto::affectation::Repere;
use rusty_music_carto::cout_voirie;
use rusty_music_carto::source::Source;
use rusty_music_carto::ville::{self, ILE_DE_LA_CITE};
use rusty_music_carto::Palette;
use rusty_music_core::db::{Library, MapPoint};

const MODELE: &str = rusty_music_analysis::passe::MODELE;

/// Un bâtiment prêt à dessiner : id OSM (pour l'appartenance à la zone),
/// contour et centre en mètres locaux, famille de l'occupant.
struct Bati {
    id: i64,
    centre: [f64; 2],
    famille: Option<i64>,
    contour: Vec<[f64; 2]>,
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let support = support_desktop();
    let lib_path = args
        .next()
        .map(PathBuf::from)
        .or_else(|| existant(&["rusty-music.db".into(), support.join("rusty-music.db")]))
        .expect("base de bibliothèque introuvable — passer le chemin en argument");
    let ville_path = args
        .next()
        .map(PathBuf::from)
        .or_else(|| existant(&["ville-paris.db".into(), support.join("ville-paris.db")]))
        .expect("ville-paris.db introuvable — passer le chemin en argument");

    println!("bibliothèque  : {}", lib_path.display());
    println!("plan de ville : {}", ville_path.display());

    let lib = Library::open(&lib_path)?;
    let vue = lib.map_view(MODELE)?;
    anyhow::ensure!(!vue.is_empty(), "aucun morceau positionné — lancer `carte` d'abord");
    let noms: HashMap<i64, String> =
        lib.familles(MODELE)?.into_iter().map(|(id, nom, _)| (id, nom)).collect();
    let empreintes = lib.embeddings(MODELE)?;

    let extrait = rusty_music_osm::base::lire(&ville_path)?;
    let repere = Repere::centre_de(&extrait);
    let centre_m = repere.vers_m(ILE_DE_LA_CITE);

    let t = std::time::Instant::now();
    let prep = ville::preparer(&extrait, &vue, ville::ESPACEMENT_PAR_DEFAUT, Some(ILE_DE_LA_CITE));
    let autorises = prep.autorises.clone();
    let r = ville::rassembler(&extrait, &vue, &noms, ville::ESPACEMENT_PAR_DEFAUT, Some(ILE_DE_LA_CITE));
    println!(
        "affectation : {:.1} s — {} adresses, {} sans, {} repli quartier, {} hors zone, {} ancrés, {} bâtiments peuplés",
        t.elapsed().as_secs_f64(),
        r.adresses_posees,
        r.morceaux_sans_adresse,
        r.repli_quartier,
        r.hors_zone,
        r.artistes_ancres,
        r.batiments_peuples,
    );

    // `Source::batiments` est dans le même ordre que `extrait.batis` (1:1) —
    // c'est comme ça qu'on récupère l'id OSM absent de `BatimentReel`.
    let batis: Vec<Bati> = r
        .source
        .batiments
        .iter()
        .zip(&extrait.batis)
        .map(|(br, c)| {
            let contour: Vec<[f64; 2]> = br.points.iter().map(|p| repere.vers_m(*p)).collect();
            let n = contour.len().max(1) as f64;
            let centre = contour.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
            Bati { id: c.id, centre, famille: br.famille, contour }
        })
        .collect();

    let sortie = std::env::temp_dir().join("rusty-music-apercu");
    std::fs::create_dir_all(&sortie)?;
    let pal = *Palette::par_id("osm-clair").expect("palette osm-clair");
    let cadre = cadre_frontiere(&r.source, &repere);
    let grille = rusty_music_carto::batiments::GrilleBatiments::nouvelle(&extrait, &repere);
    let euclide: HashSet<i64> =
        grille.n_plus_proches(centre_m, autorises.len()).iter().map(|b| b.id).collect();

    image_zone(&sortie.join("01-zone.svg"), &r.source, &repere, &pal, &batis, &autorises, &euclide, centre_m, cadre)?;
    image_familles(&sortie.join("02-familles.svg"), &r.source, &repere, &pal, &batis, cadre)?;
    image_quartiers(&sortie.join("03-quartiers.svg"), &r.source, &repere, &pal, cadre)?;
    image_cout(&sortie.join("04-cout.svg"), &extrait, &repere, cadre)?;
    let z = 1250.0;
    image_centre(
        &sortie.join("05-centre.svg"),
        &r.source,
        &repere,
        &pal,
        &batis,
        [centre_m[0] - z, centre_m[1] - z, centre_m[0] + z, centre_m[1] + z],
    )?;

    let eval = evaluation(&r.source, &repere, &batis, &autorises, &euclide, centre_m, &empreintes, &vue);
    std::fs::write(sortie.join("evaluation.md"), &eval)?;
    std::fs::write(sortie.join("index.html"), INDEX_HTML)?;
    println!("\n{eval}");
    println!("images + évaluation : {}", sortie.display());
    println!("  ouvrir {}/index.html", sortie.display());
    Ok(())
}

fn support_desktop() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/fm.rustymusic.desktop")
}
fn existant(c: &[PathBuf]) -> Option<PathBuf> {
    c.iter().find(|p| p.is_file()).cloned()
}

// --- Toile SVG ----------------------------------------------------------

struct Toile {
    buf: String,
    w: f64,
    h: f64,
    cadre: [f64; 4],
}

impl Toile {
    fn nouvelle(cadre: [f64; 4], largeur: f64, fond: &str) -> Toile {
        let [x0, y0, x1, y1] = cadre;
        let h = largeur * (y1 - y0) / (x1 - x0).max(1.0);
        let mut buf = String::with_capacity(6_000_000);
        buf.push_str(&format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='{largeur:.0}' height='{h:.0}' \
             viewBox='0 0 {largeur:.0} {h:.0}'><rect width='100%' height='100%' fill='{fond}'/>"
        ));
        Toile { buf, w: largeur, h, cadre }
    }
    fn xy(&self, p: [f64; 2]) -> (f64, f64) {
        let [x0, y0, x1, y1] = self.cadre;
        ((p[0] - x0) / (x1 - x0).max(1.0) * self.w, (y1 - p[1]) / (y1 - y0).max(1.0) * self.h)
    }
    fn forme(&mut self, pts: &[[f64; 2]], fill: &str, stroke: &str, sw: f64, fill_op: f64) {
        if pts.len() < 2 {
            return;
        }
        self.buf.push_str("<path d='M");
        for (i, p) in pts.iter().enumerate() {
            let (x, y) = self.xy(*p);
            self.buf.push_str(if i == 0 { "" } else { "L" });
            self.buf.push_str(&format!("{x:.1} {y:.1} "));
        }
        self.buf.push_str(&format!(
            "' fill='{fill}' fill-opacity='{fill_op}' stroke='{stroke}' stroke-width='{sw}'/>"
        ));
    }
    fn point(&mut self, p: [f64; 2], r: f64, fill: &str, op: f64) {
        let (x, y) = self.xy(p);
        self.buf.push_str(&format!("<circle cx='{x:.1}' cy='{y:.1}' r='{r}' fill='{fill}' fill-opacity='{op}'/>"));
    }
    fn texte(&mut self, p: [f64; 2], s: &str, couleur: &str) {
        let (x, y) = self.xy(p);
        let s: String = s.chars().map(|c| if "<>&".contains(c) { ' ' } else { c }).collect();
        self.buf.push_str(&format!(
            "<text x='{x:.1}' y='{y:.1}' font-family='sans-serif' font-size='12' fill='{couleur}' \
             stroke='#fff' stroke-width='3' paint-order='stroke'>{s}</text>"
        ));
    }
    fn finir(mut self, chemin: &Path) -> std::io::Result<()> {
        self.buf.push_str("</svg>");
        std::fs::write(chemin, self.buf)
    }
}

fn coul_fam(pal: &Palette, id: i64) -> &'static str {
    if id < 0 {
        pal.autres
    } else {
        pal.familles[id as usize % pal.familles.len()]
    }
}

fn cadre_frontiere(source: &Source, repere: &Repere) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    if let Some(anneaux) = &source.frontiere {
        for a in anneaux {
            for p in a {
                let m = repere.vers_m(*p);
                b[0] = b[0].min(m[0]);
                b[1] = b[1].min(m[1]);
                b[2] = b[2].max(m[0]);
                b[3] = b[3].max(m[1]);
            }
        }
    }
    if !b[0].is_finite() {
        b = [-6000.0, -6000.0, 6000.0, 6000.0];
    }
    let (mx, my) = ((b[2] - b[0]) * 0.03, (b[3] - b[1]) * 0.03);
    [b[0] - mx, b[1] - my, b[2] + mx, b[3] + my]
}

fn contexte(t: &mut Toile, source: &Source, repere: &Repere, pal: &Palette) {
    for c in &source.verts {
        let p: Vec<[f64; 2]> = c.points.iter().map(|q| repere.vers_m(*q)).collect();
        t.forme(&p, pal.vert, "none", 0.0, 1.0);
    }
    for c in &source.eaux {
        let p: Vec<[f64; 2]> = c.points.iter().map(|q| repere.vers_m(*q)).collect();
        t.forme(&p, pal.mer, "none", 0.0, 1.0);
    }
    if let Some(anneaux) = &source.frontiere {
        for a in anneaux {
            let p: Vec<[f64; 2]> = a.iter().map(|q| repere.vers_m(*q)).collect();
            t.forme(&p, "none", pal.cote, 1.5, 0.0);
        }
    }
}

// --- Images -----------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn image_zone(
    chemin: &Path,
    source: &Source,
    repere: &Repere,
    pal: &Palette,
    batis: &[Bati],
    autorises: &HashSet<i64>,
    euclide: &HashSet<i64>,
    centre_m: [f64; 2],
    cadre: [f64; 4],
) -> std::io::Result<()> {
    let mut t = Toile::nouvelle(cadre, 1500.0, "#f4f2ee");
    contexte(&mut t, source, repere, pal);
    for b in batis {
        let (fill, op, r) = match (autorises.contains(&b.id), euclide.contains(&b.id)) {
            (true, true) => ("#3a6ea5", 0.5, 0.7),
            (true, false) => ("#d9822b", 0.95, 1.0),
            (false, true) => ("#c0392b", 0.95, 1.0),
            (false, false) => ("#b9b5ac", 0.22, 0.5),
        };
        t.point(b.centre, r, fill, op);
    }
    t.point(centre_m, 6.0, "#111", 1.0);
    t.texte([centre_m[0] + 70.0, centre_m[1]], "ile de la Cite", "#111");
    t.texte(
        [cadre[0] + (cadre[2] - cadre[0]) * 0.04, cadre[1] + (cadre[3] - cadre[1]) * 0.06],
        "bleu = zone  ·  orange = gagne par le cout de voirie  ·  rouge = disque euclidien seulement",
        "#333",
    );
    t.finir(chemin)
}

fn image_familles(
    chemin: &Path,
    source: &Source,
    repere: &Repere,
    pal: &Palette,
    batis: &[Bati],
    cadre: [f64; 4],
) -> std::io::Result<()> {
    let mut t = Toile::nouvelle(cadre, 1500.0, "#f4f2ee");
    contexte(&mut t, source, repere, pal);
    for b in batis {
        match b.famille {
            Some(f) => t.point(b.centre, 0.9, coul_fam(pal, f), 0.85),
            None => t.point(b.centre, 0.4, pal.bati, 0.15),
        }
    }
    for p in &source.points_remarquables {
        if let Some(artiste) = &p.artiste {
            let m = repere.vers_m(p.point);
            t.point(m, 4.0, "#111", 1.0);
            t.texte([m[0] + 45.0, m[1]], &format!("{artiste} ({})", p.nom), "#111");
        }
    }
    let mut fams: Vec<&rusty_music_carto::source::Famille> = source.familles.iter().collect();
    fams.sort_by_key(|f| std::cmp::Reverse(f.effectif));
    let x = cadre[0] + (cadre[2] - cadre[0]) * 0.03;
    for (i, f) in fams.iter().enumerate() {
        let y = cadre[1] + (cadre[3] - cadre[1]) * 0.06 + i as f64 * (cadre[3] - cadre[1]) * 0.028;
        t.point([x, y], 6.0, coul_fam(pal, f.id), 1.0);
        t.texte([x + 90.0, y + 40.0], &format!("{} ({})", f.nom, f.effectif), "#222");
    }
    t.finir(chemin)
}

fn image_quartiers(
    chemin: &Path,
    source: &Source,
    repere: &Repere,
    pal: &Palette,
    cadre: [f64; 4],
) -> std::io::Result<()> {
    let mut t = Toile::nouvelle(cadre, 1500.0, "#f4f2ee");
    for terr in &source.territoires_reels {
        let c = coul_fam(pal, terr.famille);
        for poly in &terr.polygones {
            for anneau in poly {
                let p: Vec<[f64; 2]> = anneau.iter().map(|q| repere.vers_m(*q)).collect();
                t.forme(&p, c, c, 0.5, 0.55);
            }
        }
    }
    contexte(&mut t, source, repere, pal);
    t.finir(chemin)
}

fn image_cout(
    chemin: &Path,
    extrait: &rusty_music_osm::Extrait,
    repere: &Repere,
    cadre: [f64; 4],
) -> std::io::Result<()> {
    let champ = cout_voirie::champ_de_cout(extrait, repere, ILE_DE_LA_CITE, 360);
    let mut finis: Vec<f64> = champ.valeurs.iter().copied().filter(|v| v.is_finite()).collect();
    finis.sort_by(f64::total_cmp);
    let haut = finis.get((finis.len() as f64 * 0.95) as usize).copied().unwrap_or(champ.cout_max).max(1.0);
    let seuils: Vec<f64> = (1..13).map(|i| haut * i as f64 / 13.0).collect();
    let smax = *seuils.last().unwrap_or(&1.0);
    let bandes = cout_voirie::isobandes(&champ, &seuils);

    let mut t = Toile::nouvelle(cadre, 1500.0, "#101216");
    for b in &bandes {
        let c = rampe_viridis((b.seuil / smax).min(1.0));
        for poly in &b.polygones {
            for anneau in poly {
                t.forme(anneau, &c, "none", 0.0, 1.0);
            }
        }
    }
    for tr in &extrait.troncons {
        let p: Vec<[f64; 2]> = tr.points.iter().map(|q| repere.vers_m(*q)).collect();
        t.forme(&p, "none", "#ffffff12", 0.3, 0.0);
    }
    t.point(repere.vers_m(ILE_DE_LA_CITE), 6.0, "#fff", 1.0);
    t.finir(chemin)
}

fn image_centre(
    chemin: &Path,
    source: &Source,
    repere: &Repere,
    pal: &Palette,
    batis: &[Bati],
    cadre: [f64; 4],
) -> std::io::Result<()> {
    let mut t = Toile::nouvelle(cadre, 1600.0, "#f4f2ee");
    let dans = |p: &[[f64; 2]]| {
        p.iter().any(|m| m[0] >= cadre[0] && m[0] <= cadre[2] && m[1] >= cadre[1] && m[1] <= cadre[3])
    };
    for c in &source.eaux {
        let p: Vec<[f64; 2]> = c.points.iter().map(|q| repere.vers_m(*q)).collect();
        if dans(&p) {
            t.forme(&p, pal.mer, "none", 0.0, 1.0);
        }
    }
    for tr in &source.troncons_reels {
        let p: Vec<[f64; 2]> = tr.points.iter().map(|q| repere.vers_m(*q)).collect();
        if dans(&p) {
            t.forme(&p, "none", "#d8d4ca", 1.2, 0.0);
        }
    }
    for b in batis {
        if !dans(&b.contour) {
            continue;
        }
        match b.famille {
            Some(f) => t.forme(&b.contour, coul_fam(pal, f), "#00000022", 0.3, 0.9),
            None => t.forme(&b.contour, "#e8e4db", "none", 0.0, 1.0),
        }
    }
    t.point(repere.vers_m(ILE_DE_LA_CITE), 5.0, "#111", 1.0);
    t.finir(chemin)
}

fn rampe_viridis(x: f64) -> String {
    let stops = [
        (0.0, (43.0, 10.0, 61.0)),
        (0.35, (62.0, 106.0, 176.0)),
        (0.6, (47.0, 154.0, 160.0)),
        (0.8, (167.0, 217.0, 75.0)),
        (1.0, (242.0, 230.0, 61.0)),
    ];
    let mut c = stops[0].1;
    for w in stops.windows(2) {
        if x >= w[0].0 && x <= w[1].0 {
            let f = (x - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
            c = (
                w[0].1 .0 + f * (w[1].1 .0 - w[0].1 .0),
                w[0].1 .1 + f * (w[1].1 .1 - w[0].1 .1),
                w[0].1 .2 + f * (w[1].1 .2 - w[0].1 .2),
            );
        }
    }
    format!("#{:02x}{:02x}{:02x}", c.0 as u8, c.1 as u8, c.2 as u8)
}

// --- Évaluation ------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn evaluation(
    source: &Source,
    repere: &Repere,
    batis: &[Bati],
    autorises: &HashSet<i64>,
    euclide: &HashSet<i64>,
    centre_m: [f64; 2],
    empreintes: &[(i64, Vec<f32>)],
    vue: &[MapPoint],
) -> String {
    let _ = (repere, vue);
    let mut s = String::from("# Évaluation qualitative du placement\n\n");

    // Forme de la zone.
    let rayons = |ids: &HashSet<i64>| {
        let mut d: Vec<f64> = batis
            .iter()
            .filter(|b| ids.contains(&b.id))
            .map(|b| ((b.centre[0] - centre_m[0]).powi(2) + (b.centre[1] - centre_m[1]).powi(2)).sqrt())
            .collect();
        d.sort_by(f64::total_cmp);
        let p = |q: f64| d.get(((d.len() as f64) * q) as usize).copied().unwrap_or(0.0);
        (p(0.5), p(0.95), d.last().copied().unwrap_or(0.0))
    };
    let (c50, c95, cmax) = rayons(autorises);
    let (e50, e95, emax) = rayons(euclide);
    let commun = autorises.intersection(euclide).count();
    let aire_bat = 22.0 * 22.0;
    let remplissage = (autorises.len() as f64 * aire_bat) / (std::f64::consts::PI * cmax * cmax).max(1.0);
    s.push_str(&format!(
        "## Forme de la zone peuplée\n\n\
         Rayon depuis l'île de la Cité (médiane / p95 / max), mètres :\n\
         - coût de voirie : {c50:.0} / {c95:.0} / {cmax:.0}\n\
         - disque euclidien de même taille : {e50:.0} / {e95:.0} / {emax:.0}\n\
         - bâtiments communs : {} %\n\
         - étirement des bras : p95 ×{:.2}, max ×{:.2}\n\
         - remplissage du disque (aire zone / π·rmax²) : {:.2} — 1,00 = cercle plein, < = étoilé\n\n",
        100 * commun / autorises.len().max(1),
        c95 / e95.max(1.0),
        cmax / emax.max(1.0),
        remplissage,
    ));

    // Éclatement des familles.
    let mut par_fam: HashMap<i64, Vec<[f64; 2]>> = HashMap::new();
    for b in batis {
        if let Some(f) = b.famille {
            par_fam.entry(f).or_default().push(b.centre);
        }
    }
    let noms: HashMap<i64, &str> = source.familles.iter().map(|f| (f.id, f.nom.as_str())).collect();
    let mut ids: Vec<i64> = par_fam.keys().copied().collect();
    ids.sort_by_key(|f| std::cmp::Reverse(par_fam[f].len()));
    s.push_str("## Compacité des familles\n\n| famille | bâtiments | giration (m) | amas ≥ 30 | plus gros amas |\n|---|--:|--:|--:|--:|\n");
    let mut gir_moy = 0.0;
    for f in &ids {
        let pts = &par_fam[f];
        let n = pts.len() as f64;
        let bary = pts.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
        let gir = (pts.iter().map(|p| (p[0] - bary[0]).powi(2) + (p[1] - bary[1]).powi(2)).sum::<f64>() / n).sqrt();
        gir_moy += gir / ids.len() as f64;
        let (amas, gros) = composantes(pts, 45.0);
        s.push_str(&format!(
            "| {} | {} | {:.0} | {amas} | {} % |\n",
            noms.get(f).copied().unwrap_or("?"),
            pts.len(),
            gir,
            (100.0 * gros as f64 / n) as i64,
        ));
    }
    s.push_str(&format!(
        "\nGiration moyenne : {gir_moy:.0} m. Plus elle est petite et plus « plus gros amas » approche 100 %, moins la famille est éclatée.\n\n"
    ));

    // Voisinage musical préservé (objection V1).
    let pos: HashMap<i64, [f64; 2]> = source
        .morceaux
        .iter()
        .map(|m| (m.id, [m.x as f64, m.y as f64]))
        .collect();
    const K: usize = rusty_music_analysis::chemin::K_VOISINS;
    let ids_ech: Vec<i64> = {
        let mut v: Vec<i64> = pos.keys().copied().collect();
        v.sort_unstable();
        v.into_iter().step_by((pos.len() / 1500).max(1)).collect()
    };
    let mut recouv = Vec::new();
    for &id in &ids_ech {
        let musicaux = rusty_music_analysis::chemin::voisins(empreintes, id, K);
        if musicaux.is_empty() {
            continue;
        }
        let Some(&ici) = pos.get(&id) else { continue };
        let mut geo: Vec<(i64, f64)> = pos
            .iter()
            .filter(|(j, _)| **j != id)
            .map(|(j, p)| (*j, (p[0] - ici[0]).powi(2) + (p[1] - ici[1]).powi(2)))
            .collect();
        let k = K.min(geo.len());
        if k == 0 {
            continue;
        }
        geo.select_nth_unstable_by(k - 1, |a, b| a.1.total_cmp(&b.1));
        geo.truncate(k);
        let g: HashSet<i64> = geo.into_iter().map(|(j, _)| j).collect();
        recouv.push(musicaux.iter().filter(|j| g.contains(j)).count() as f64 / K as f64);
    }
    recouv.sort_by(f64::total_cmp);
    let moy = recouv.iter().sum::<f64>() / recouv.len().max(1) as f64;
    let med = recouv.get(recouv.len() / 2).copied().unwrap_or(0.0);
    s.push_str(&format!(
        "## Voisinage musical préservé (objection V1)\n\n\
         Part des {K} plus proches voisins musicaux qui restent parmi les {K} plus proches géographiques :\n\
         **moyenne {:.0} %, médiane {:.0} %** sur {} morceaux.\n\n\
         (Mesuré sur les positions t-SNE, pas sur les adresses : c'est la qualité de la projection amont, pas de l'affectation.)\n\n",
        100.0 * moy,
        100.0 * med,
        recouv.len(),
    ));

    // Ancrage.
    let ancres: Vec<String> = source
        .artistes_places
        .iter()
        .filter_map(|a| a.ancre.as_ref().map(|m| format!("- {} → {m}", a.nom)))
        .collect();
    s.push_str(&format!(
        "## Ancrage aux monuments\n\n{} artistes ancrés.\n{}\n",
        ancres.len(),
        if ancres.is_empty() { "_(aucun — bibliothèque sans données de popularité ?)_".into() } else { ancres.join("\n") },
    ));

    s
}

/// Amas d'au moins 30 points (reliés si à moins de `d` m) : le nombre, et la
/// taille du plus gros. Flood-fill sur grille.
fn composantes(pts: &[[f64; 2]], d: f64) -> (usize, usize) {
    let mut cell: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in pts.iter().enumerate() {
        cell.entry(((p[0] / d).floor() as i64, (p[1] / d).floor() as i64)).or_default().push(i);
    }
    let mut vu = vec![false; pts.len()];
    let (mut amas, mut gros) = (0usize, 0usize);
    for depart in 0..pts.len() {
        if vu[depart] {
            continue;
        }
        let mut pile = vec![depart];
        vu[depart] = true;
        let mut taille = 0usize;
        while let Some(i) = pile.pop() {
            taille += 1;
            let (cx, cy) = ((pts[i][0] / d).floor() as i64, (pts[i][1] / d).floor() as i64);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(v) = cell.get(&(cx + dx, cy + dy)) else { continue };
                    for &j in v {
                        if !vu[j]
                            && (pts[i][0] - pts[j][0]).powi(2) + (pts[i][1] - pts[j][1]).powi(2) <= d * d
                        {
                            vu[j] = true;
                            pile.push(j);
                        }
                    }
                }
            }
        }
        if taille >= 30 {
            amas += 1;
        }
        gros = gros.max(taille);
    }
    (amas, gros)
}

const INDEX_HTML: &str = r#"<!doctype html><meta charset=utf-8>
<title>Rusty Music — aperçu du placement</title>
<style>body{margin:0;background:#222;font:14px system-ui;color:#ddd}
h2{margin:24px 12px 6px}img{display:block;width:100%;max-width:1500px;margin:0 12px;background:#fff}
a{color:#8bf}</style>
<h2>01 — Forme de la zone peuplée (coût de voirie vs disque)</h2><img src=01-zone.svg>
<h2>02 — Bâtiments habités, colorés par famille musicale</h2><img src=02-familles.svg>
<h2>03 — Aplats de quartier (territoires)</h2><img src=03-quartiers.svg>
<h2>04 — Champ de coût de voirie depuis l'île de la Cité</h2><img src=04-cout.svg>
<h2>05 — Zoom 2,5 km sur le centre, bâtiments réels</h2><img src=05-centre.svg>
<p style=margin:24px><a href=evaluation.md>evaluation.md</a></p>
"#;
