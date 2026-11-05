// SPDX-License-Identifier: GPL-3.0-or-later
//! P0 de la balade (`docs/plan-balade.md`) : une marche dans les rues
//! s'entend-elle comme une progression ou comme un tirage au hasard ?
//!
//! `cargo run --release -p rusty-music-carto --example continuite_balade -- [rusty-music.db] [ville-paris.db] [positions.json] [dossier-de-sortie]`
//!
//! - **E1, continuité** : 500 départs (graine fixe), une suite de 20 morceaux
//!   par méthode — marche auto-évitante sur la voirie, les trois profils de
//!   l'itinéraire sur la voirie (60 min, sans arrivée), `Graphe::sonique`
//!   entre les deux bouts de la marche, `Graphe::errance`, tirage au hasard.
//!   Distance cosinus CLAP entre morceaux consécutifs (médiane, 9ᵉ décile) et
//!   part des changements de famille.
//! - **E2, carrefours** : pour chaque sommet de degré ≥ 3 de la zone peuplée,
//!   les branches habitées, l'entropie des familles de leurs premiers
//!   morceaux, la distance de ces premiers morceaux au morceau d'arrivée.
//!
//! Positions réelles : `positions.json` écrit par l'application à côté des
//! tuiles (celles qu'elle accroche à la voirie). Sorties : `continuite.md`,
//! `distances.svg`, `entropie.svg`.
//!
//! Pièges suivis (plan, § P0) : les morceaux non accrochés sautent sans être
//! comptés ; les transitions qui touchent un artiste ancré sur un monument
//! sont comptées à part ; même graine d'un réglage à l'autre.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use rusty_music_analysis::alea::Alea;
use rusty_music_analysis::chemin::{echantillonner, Graphe as GrapheSonore, K_VOISINS};
use rusty_music_carto::accrochage::{destination_pour_duree, morceaux_le_long, AccrochageVoirie};
use rusty_music_carto::affectation::Repere;
use rusty_music_carto::ancrage;
use rusty_music_carto::cout_itineraire::{friction_itineraire, ProfilVoirie, ProximiteAgrement};
use rusty_music_carto::facades::facades;
use rusty_music_carto::reseau_reel::Graphe;
use rusty_music_carto::ville::{self, ILE_DE_LA_CITE};
use rusty_music_core::db::Library;

/// Le modèle d'empreintes de la bibliothèque (`rusty_music_analysis::passe::MODELE`).
const MODELE: &str = "clap-htsat-unfused-5f";
const GRAINE: u64 = 0xBA1ADE;
const DEPARTS: usize = 500;
const LONGUEUR: usize = 20;
/// Le cadran `bruit` par défaut des chemins d'Explorer (`BRUIT_DEFAUT` de
/// l'application).
const BRUIT: f32 = 0.3;
/// Rayon du couloir de l'itinéraire sur la voirie (défaut de `itineraire_voirie`).
const RAYON_COULOIR_M: f64 = 25.0;
/// Portée de la façade, comme `croissance_apercu`.
const PORTEE_FACADE_M: f64 = 60.0;
/// Côté de la grille qui délimite la zone peuplée pour E2, mètres.
const PAS_ZONE_M: f64 = 100.0;

/// Ce qu'il faut pour « entendre » les morceaux le long d'une arête : chaque
/// morceau est rangé sur le tronçon de sa façade (`facades.rs`). Il s'entend
/// en longeant ce tronçon, au sommet où il est accroché ; si ce tronçon ne
/// passe pas par son sommet (façade sur une desserte non routable, coin de
/// rue), il s'entend dès qu'on touche son sommet.
struct Riverains<'a> {
    graphe: &'a Graphe,
    accrochage: &'a AccrochageVoirie,
    /// Morceau → id OSM du tronçon de sa façade, s'il passe par son sommet.
    facade: HashMap<i64, i64>,
}

impl Riverains<'_> {
    /// Les morceaux entendus en parcourant l'arête `a → b`, dans l'ordre (ceux
    /// de `a`, puis ceux de `b`), sans ceux de `deja`.
    fn le_long(&self, a: u32, b: u32, deja: &HashSet<i64>, sortie: &mut Vec<i64>) {
        let troncons = self.graphe.troncons_traverses(&[a, b]);
        for s in [a, b] {
            let Some(ids) = self.accrochage.morceaux_a.get(&s) else {
                continue;
            };
            for &id in ids {
                let sur_la_rue = self.facade.get(&id).is_none_or(|t| troncons.contains(t));
                if sur_la_rue && !deja.contains(&id) && !sortie.contains(&id) {
                    sortie.push(id);
                }
            }
        }
    }
}

/// Voisins distincts d'un sommet (deux tronçons peuvent partager un segment).
fn voisins(graphe: &Graphe, s: u32) -> Vec<u32> {
    let mut v: Vec<u32> = graphe.voisins(s).iter().map(|&(w, _)| w).collect();
    v.sort_unstable();
    v.dedup();
    v.retain(|&w| w != s);
    v
}

/// Méthode 1 : marche auto-évitante sur la voirie (retour en arrière quand
/// elle s'enferme), morceaux pris le long des façades.
fn marche(r: &Riverains, depart: i64, graine: u64) -> Vec<i64> {
    let mut alea = Alea::depuis(graine);
    let s0 = r.accrochage.sommet_de[&depart];
    let mut suite = vec![depart];
    let mut entendus: HashSet<i64> = HashSet::from([depart]);
    let mut vus: HashSet<u32> = HashSet::from([s0]);
    let mut pile = vec![s0];
    let mut pas = 0;
    while suite.len() < LONGUEUR && pas < 50_000 {
        pas += 1;
        let Some(&courant) = pile.last() else { break };
        let libres: Vec<u32> = voisins(r.graphe, courant)
            .into_iter()
            .filter(|w| !vus.contains(w))
            .collect();
        if libres.is_empty() {
            pile.pop();
            continue;
        }
        let suivant = libres[alea.borne(libres.len())];
        vus.insert(suivant);
        pile.push(suivant);
        let mut neufs = Vec::new();
        r.le_long(courant, suivant, &entendus, &mut neufs);
        for id in neufs {
            entendus.insert(id);
            suite.push(id);
        }
    }
    suite.truncate(LONGUEUR);
    suite
}

/// Méthode 2 : l'itinéraire sur la voirie de l'application, sans arrivée,
/// 60 min — mêmes fonctions que la commande `itineraire_voirie`.
fn itineraire(
    graphe: &Graphe,
    accrochage: &AccrochageVoirie,
    positions: &HashMap<i64, [f64; 2]>,
    depart: i64,
    duree: &dyn Fn(i64) -> u64,
) -> Vec<i64> {
    let cible = 60 * 60_000;
    let pos = positions[&depart];
    let Some(dest) = destination_pour_duree(graphe, accrochage, depart, pos, None, cible, duree)
    else {
        return Vec::new();
    };
    let Some((trace, _)) = graphe.chemin_sommets(pos, positions[&dest]) else {
        return Vec::new();
    };
    let couloir = graphe.couloir(&trace, RAYON_COULOIR_M);
    let mut suite = morceaux_le_long(accrochage, &couloir, depart, None, None, Some(cible), duree);
    suite.truncate(LONGUEUR);
    suite
}

fn cosinus(a: &[f32], b: &[f32]) -> f64 {
    1.0 - a.iter().zip(b).map(|(x, y)| (x * y) as f64).sum::<f64>()
}

fn quantile(trie: &[f64], q: f64) -> f64 {
    if trie.is_empty() {
        return f64::NAN;
    }
    let i = ((trie.len() - 1) as f64 * q).round() as usize;
    trie[i]
}

/// Les transitions d'une méthode : distances et changements de famille, en
/// ville et (à part) celles qui touchent un artiste ancré.
#[derive(Default)]
struct Bilan {
    nom: String,
    suites: usize,
    longueurs: usize,
    ville: Vec<f64>,
    changements: usize,
    /// Transitions de ville entre deux morceaux d'un même album.
    meme_album: usize,
    /// Les distances de ville hors transitions intra-album : un album logé
    /// d'un seul tenant rend la rue continue sans rien dire du placement.
    hors_album: Vec<f64>,
    ancrees: Vec<f64>,
}

impl Bilan {
    fn ajouter(&mut self, suite: &[i64], ctx: &Contexte) {
        // Les morceaux sans adresse accrochée sautent sans être comptés.
        let suite: Vec<i64> = suite
            .iter()
            .copied()
            .filter(|id| ctx.accroches.contains(id) && ctx.empreinte.contains_key(id))
            .collect();
        if suite.len() < 2 {
            return;
        }
        self.suites += 1;
        self.longueurs += suite.len();
        for w in suite.windows(2) {
            let d = cosinus(&ctx.empreinte[&w[0]], &ctx.empreinte[&w[1]]);
            if ctx.ancres.contains(&w[0]) || ctx.ancres.contains(&w[1]) {
                self.ancrees.push(d);
            } else {
                self.ville.push(d);
                if ctx.famille[&w[0]] != ctx.famille[&w[1]] {
                    self.changements += 1;
                }
                if ctx
                    .album
                    .get(&w[0])
                    .is_some_and(|a| ctx.album.get(&w[1]) == Some(a))
                {
                    self.meme_album += 1;
                } else {
                    self.hors_album.push(d);
                }
            }
        }
    }

    fn trier(&mut self) {
        self.ville.sort_by(f64::total_cmp);
        self.ancrees.sort_by(f64::total_cmp);
        self.hors_album.sort_by(f64::total_cmp);
    }

    fn mediane(&self) -> f64 {
        quantile(&self.ville, 0.5)
    }
}

struct Contexte {
    empreinte: HashMap<i64, Vec<f32>>,
    famille: HashMap<i64, i64>,
    /// Morceau → (artiste de l'album, album), quand l'album est connu.
    album: HashMap<i64, (String, String)>,
    accroches: HashSet<i64>,
    ancres: HashSet<i64>,
}

/// Une rangée de petits multiples par série : histogramme à une seule teinte,
/// médiane marquée. Fond explicite, encre neutre.
fn histogrammes(titre: &str, axe: &str, series: &[(String, Vec<f64>)], max: f64) -> String {
    const BACS: usize = 40;
    const L: f64 = 640.0;
    const G: f64 = 210.0; // marge gauche (étiquettes)
                          // Une rangée seule prend de la hauteur ; plusieurs se partagent la page.
    let h_rang = if series.len() == 1 { 180.0 } else { 64.0 };
    let largeur = L + G + 20.0;
    let hauteur = 50.0 + h_rang * series.len() as f64 + 40.0;
    let mut s = String::new();
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{largeur}" height="{hauteur}" viewBox="0 0 {largeur} {hauteur}" font-family="system-ui, sans-serif" font-size="12">
<rect width="100%" height="100%" fill="#ffffff"/>
<text x="16" y="26" font-size="15" font-weight="600" fill="#1f2328">{titre}</text>
"##
    );
    for (r, (nom, valeurs)) in series.iter().enumerate() {
        let y0 = 50.0 + h_rang * r as f64;
        let base = y0 + h_rang - 14.0;
        let mut comptes = [0usize; BACS];
        for &v in valeurs {
            let b = ((v / max) * BACS as f64)
                .floor()
                .clamp(0.0, BACS as f64 - 1.0) as usize;
            comptes[b] += 1;
        }
        let pic = comptes.iter().copied().max().unwrap_or(1).max(1) as f64;
        let mut tries = valeurs.clone();
        tries.sort_by(f64::total_cmp);
        let med = quantile(&tries, 0.5);
        let _ = writeln!(
            s,
            r##"<text x="16" y="{:.1}" fill="#1f2328">{nom}</text><text x="16" y="{:.1}" fill="#656d76" font-size="11">médiane {med:.3} · n = {}</text>"##,
            base - 22.0,
            base - 8.0,
            valeurs.len()
        );
        let pas = L / BACS as f64;
        for (b, &c) in comptes.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let h = (h_rang - 22.0) * c as f64 / pic;
            let _ = writeln!(
                s,
                r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{h:.1}" rx="1.5" fill="#3b7dd8"><title>{:.3}–{:.3} : {c}</title></rect>"##,
                G + b as f64 * pas + 1.0,
                base - h,
                pas - 2.0,
                b as f64 * max / BACS as f64,
                (b + 1) as f64 * max / BACS as f64,
            );
        }
        let xm = G + med / max * L;
        let _ = writeln!(
            s,
            r##"<line x1="{G}" x2="{:.1}" y1="{base:.1}" y2="{base:.1}" stroke="#d0d7de"/><line x1="{xm:.1}" x2="{xm:.1}" y1="{:.1}" y2="{:.1}" stroke="#1f2328" stroke-width="2"/>"##,
            G + L,
            base - (h_rang - 18.0),
            base + 3.0,
        );
    }
    let y_axe = 50.0 + h_rang * series.len() as f64 + 4.0;
    for i in 0..=4 {
        let v = max * i as f64 / 4.0;
        let x = G + L * i as f64 / 4.0;
        let _ = writeln!(
            s,
            r##"<text x="{x:.1}" y="{y_axe:.1}" fill="#656d76" text-anchor="middle">{v:.2}</text>"##
        );
    }
    let _ = writeln!(
        s,
        r##"<text x="{:.1}" y="{:.1}" fill="#656d76" text-anchor="middle">{axe}</text></svg>"##,
        G + L / 2.0,
        y_axe + 20.0
    );
    s
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let support = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/Application Support/fm.rustymusic.desktop");
    let lib_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| support.join("rusty-music.db"));
    let ville_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| support.join("ville-paris.db"));
    let positions_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| support.join("tuiles/positions.json"));
    let sortie = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("continuite-balade"));
    std::fs::create_dir_all(&sortie)?;
    let t = std::time::Instant::now();
    let fils = std::thread::available_parallelism().map_or(4, |n| n.get());

    // --- Bibliothèque : empreintes normalisées, familles, durées. ---
    let lib = Library::open(&lib_path)?;
    let vue = lib.map_view(MODELE)?;
    let empreintes = lib.embeddings(MODELE)?;
    let empreinte: HashMap<i64, Vec<f32>> = empreintes
        .iter()
        .map(|(id, v)| {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            (*id, v.iter().map(|x| x / n).collect())
        })
        .collect();
    let famille: HashMap<i64, i64> = vue.iter().map(|p| (p.id, p.cluster)).collect();
    let duree_de: HashMap<i64, u64> = vue
        .iter()
        .map(|p| (p.id, p.duration_ms.unwrap_or(0).max(0) as u64))
        .collect();
    let duree = |id: i64| duree_de.get(&id).copied().unwrap_or(0);

    // --- Ville : graphe, accrochage aux positions réelles, façades, ancrage. ---
    let extrait = rusty_music_osm::base::lire(&ville_path)?;
    let reelles: HashMap<i64, [f32; 2]> =
        serde_json::from_str(&std::fs::read_to_string(&positions_path)?)?;
    let positions: HashMap<i64, [f64; 2]> = reelles
        .iter()
        .map(|(&id, &[x, y])| (id, [x as f64, y as f64]))
        .collect();
    let graphe = Graphe::construire(&extrait);
    let accrochage =
        AccrochageVoirie::construire(&graphe, reelles.iter().map(|(&id, &[x, y])| (id, x, y)));
    let repere = Repere::centre_de(&extrait);
    let ids: Vec<i64> = accrochage.sommet_de.keys().copied().collect();
    let centres: Vec<[f64; 2]> = ids.iter().map(|id| repere.vers_m(positions[id])).collect();
    let fac = facades(&extrait, &repere, &centres, PORTEE_FACADE_M);
    let mut troncons_du_sommet: HashMap<u32, HashSet<i64>> = HashMap::new();
    let mut facade: HashMap<i64, i64> = HashMap::new();
    let mut sans_rue = 0usize;
    for (id, f) in ids.iter().zip(&fac) {
        let Some(f) = f else { continue };
        let s = accrochage.sommet_de[id];
        let incidents = troncons_du_sommet.entry(s).or_insert_with(|| {
            voisins(&graphe, s)
                .into_iter()
                .flat_map(|w| graphe.troncons_traverses(&[s, w]))
                .collect()
        });
        let t = extrait.troncons[f.troncon].id;
        if incidents.contains(&t) {
            facade.insert(*id, t);
        } else {
            sans_rue += 1;
        }
    }
    let zone = ville::zone_peuplee(&extrait, &vue, Some(ILE_DE_LA_CITE));
    let ancres: HashSet<i64> = zone.ancrages.adresses.iter().map(|a| a.track_id).collect();
    let album: HashMap<i64, (String, String)> = vue
        .iter()
        .filter_map(|p| {
            let a = p.album.clone().filter(|a| !a.is_empty())?;
            Some((p.id, (ancrage::nom_artiste(p).unwrap_or("").to_string(), a)))
        })
        .collect();
    let ctx = Contexte {
        empreinte,
        famille,
        album,
        accroches: accrochage.sommet_de.keys().copied().collect(),
        ancres,
    };
    let riverains = Riverains {
        graphe: &graphe,
        accrochage: &accrochage,
        facade,
    };
    println!(
        "ville : {:.1} s — {} positions, {} accrochés, {} façades sur leur sommet, {} ailleurs, {} ancrés",
        t.elapsed().as_secs_f64(),
        positions.len(),
        ctx.accroches.len(),
        riverains.facade.len(),
        sans_rue,
        ctx.ancres.len()
    );

    // --- Départs : tirés parmi les morceaux accrochés hors monuments. ---
    let mut candidats: Vec<i64> = ctx
        .accroches
        .iter()
        .copied()
        .filter(|id| !ctx.ancres.contains(id) && ctx.empreinte.contains_key(id))
        .collect();
    candidats.sort_unstable();
    let mut alea = Alea::depuis(GRAINE);
    let departs: Vec<i64> = (0..DEPARTS)
        .map(|_| candidats[alea.borne(candidats.len())])
        .collect();

    // --- E1. ---
    let marches: Vec<Vec<i64>> = departs
        .iter()
        .enumerate()
        .map(|(i, &d)| marche(&riverains, d, GRAINE ^ (i as u64 + 1)))
        .collect();

    let agrement = ProximiteAgrement::nouvelle(&extrait, 120.0);
    let profils = [
        (ProfilVoirie::ParLeConnu, "2a. itinéraire « par le connu »"),
        (ProfilVoirie::Redecouvrir, "2b. itinéraire « redécouvrir »"),
        (ProfilVoirie::Panoramique, "2c. itinéraire « panoramique »"),
    ];
    let mut itineraires: Vec<(&str, Vec<Vec<i64>>)> = Vec::new();
    for (profil, nom) in profils {
        let g = Graphe::construire_pondere(&extrait, friction_itineraire(profil, Some(&agrement)));
        let morceaux = departs.len().div_ceil(fils);
        let suites: Vec<Vec<i64>> = std::thread::scope(|sc| {
            let taches: Vec<_> = departs
                .chunks(morceaux)
                .map(|lot| {
                    let (g, acc, pos, duree) = (&g, &accrochage, &positions, &duree);
                    sc.spawn(move || {
                        lot.iter()
                            .map(|&d| itineraire(g, acc, pos, d, duree))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            taches
                .into_iter()
                .flat_map(|t| t.join().expect("fil d'itinéraire"))
                .collect()
        });
        println!("{nom} : {:.1} s", t.elapsed().as_secs_f64());
        itineraires.push((nom, suites));
    }

    let sonore = GrapheSonore::construire(&empreintes, K_VOISINS, fils);
    println!("graphe des voisins : {:.1} s", t.elapsed().as_secs_f64());

    let mut bilans: Vec<Bilan> = Vec::new();
    let nouveau = |nom: &str| Bilan {
        nom: nom.into(),
        ..Bilan::default()
    };
    let mut b1 = nouveau("1. marche dans les rues");
    for m in &marches {
        b1.ajouter(m, &ctx);
    }
    bilans.push(b1);
    for (nom, suites) in &itineraires {
        let mut b = nouveau(nom);
        for s in suites {
            b.ajouter(s, &ctx);
        }
        bilans.push(b);
    }
    let mut b3 = nouveau("3. sonique (bouts de la marche)");
    let mut b4 = nouveau("4. errance");
    let mut b5 = nouveau("5. hasard");
    let mut alea5 = Alea::depuis(GRAINE ^ 5);
    for (i, (&d, m)) in departs.iter().zip(&marches).enumerate() {
        let graine = GRAINE ^ (i as u64 + 1);
        if let Some(&arrivee) = m.last().filter(|_| m.len() >= 2) {
            let complet = sonore.sonique(d, arrivee, graine, BRUIT);
            b3.ajouter(&echantillonner(&complet, LONGUEUR), &ctx);
        }
        b4.ajouter(&sonore.errance(d, LONGUEUR, graine, BRUIT), &ctx);
        let mut h = vec![d];
        while h.len() < LONGUEUR {
            let x = candidats[alea5.borne(candidats.len())];
            if !h.contains(&x) {
                h.push(x);
            }
        }
        b5.ajouter(&h, &ctx);
    }
    bilans.extend([b3, b4, b5]);
    for b in &mut bilans {
        b.trier();
    }
    let med = |prefixe: &str| {
        bilans
            .iter()
            .find(|b| b.nom.starts_with(prefixe))
            .map_or(f64::NAN, Bilan::mediane)
    };
    let (m1, m3, m5) = (med("1."), med("3."), med("5."));
    let med_ha = |prefixe: &str| {
        bilans
            .iter()
            .find(|b| b.nom.starts_with(prefixe))
            .map_or(f64::NAN, |b| quantile(&b.hors_album, 0.5))
    };
    let (h1, h3, h5) = (med_ha("1."), med_ha("3."), med_ha("5."));

    // --- E2 : carrefours de la zone peuplée. ---
    let cle = |p: [f64; 2]| {
        (
            (p[0] / PAS_ZONE_M).floor() as i64,
            (p[1] / PAS_ZONE_M).floor() as i64,
        )
    };
    let habitees: HashSet<(i64, i64)> = ctx
        .accroches
        .iter()
        .filter(|id| !ctx.ancres.contains(id))
        .map(|id| cle(repere.vers_m(positions[id])))
        .collect();
    let dans_zone = |s: u32| {
        let (cx, cy) = cle(repere.vers_m(graphe.point(s)));
        (-1..=1).any(|dx| (-1..=1).any(|dy| habitees.contains(&(cx + dx, cy + dy))))
    };
    let degre: Vec<usize> = (0..graphe.points().len() as u32)
        .map(|s| voisins(&graphe, s).len())
        .collect();
    // Premier morceau d'une branche : on suit la rue depuis `v` par `w`
    // jusqu'au prochain carrefour ou cul-de-sac. Les morceaux de `v` lui-même
    // et ceux des artistes ancrés n'en font pas partie.
    let premier = |v: u32, w: u32| -> Option<i64> {
        let mut exclus: HashSet<i64> = accrochage
            .morceaux_a
            .get(&v)
            .map(|ids| ids.iter().copied().collect())
            .unwrap_or_default();
        exclus.extend(ctx.ancres.iter().copied());
        let (mut prec, mut cour) = (v, w);
        for _ in 0..2_000 {
            let mut neufs = Vec::new();
            riverains.le_long(prec, cour, &exclus, &mut neufs);
            if let Some(&id) = neufs.iter().find(|id| ctx.empreinte.contains_key(id)) {
                return Some(id);
            }
            if degre[cour as usize] != 2 {
                return None;
            }
            let suivant = voisins(&graphe, cour).into_iter().find(|&x| x != prec)?;
            if suivant == v {
                return None;
            }
            (prec, cour) = (cour, suivant);
        }
        None
    };

    let mut carrefours = 0usize;
    let mut branches_habitees: Vec<usize> = Vec::new();
    let mut entropies: Vec<f64> = Vec::new();
    let mut distances_arrivee: Vec<f64> = Vec::new();
    let (mut arrivees, mut a_deux, mut a_proche, mut a_autre, mut utiles) = (0, 0, 0, 0, 0);
    // Variante : « proche » = sous la médiane de la marche (seuil du plan trop
    // strict pour des premiers morceaux qui ne sont jamais du même album).
    let (mut a_proche_large, mut utiles_large) = (0, 0);
    let mut carrefours_utiles = 0usize;
    for v in 0..graphe.points().len() as u32 {
        if degre[v as usize] < 3 || !dans_zone(v) {
            continue;
        }
        carrefours += 1;
        let firsts: Vec<i64> = voisins(&graphe, v)
            .into_iter()
            .filter_map(|w| premier(v, w))
            .collect();
        branches_habitees.push(firsts.len());
        if firsts.len() >= 2 {
            let mut comptes: HashMap<i64, usize> = HashMap::new();
            for id in &firsts {
                *comptes.entry(ctx.famille[id]).or_default() += 1;
            }
            let n = firsts.len() as f64;
            entropies.push(
                comptes
                    .values()
                    .map(|&c| {
                        let p = c as f64 / n;
                        -p * p.log2()
                    })
                    .sum(),
            );
        }
        // Chaque branche habitée tour à tour comme arrivée : son premier
        // morceau (le plus près du carrefour) est le morceau d'arrivée, les
        // autres branches sont les suites proposées.
        let mut utile_ici = false;
        for (i, &arrivee) in firsts.iter().enumerate() {
            arrivees += 1;
            let options: Vec<(f64, bool)> = firsts
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(_, &o)| {
                    let d = cosinus(&ctx.empreinte[&arrivee], &ctx.empreinte[&o]);
                    (d, ctx.famille[&o] != ctx.famille[&arrivee])
                })
                .collect();
            distances_arrivee.extend(options.iter().map(|o| o.0));
            let proches: Vec<usize> = (0..options.len()).filter(|&k| options[k].0 < m3).collect();
            let autres: Vec<usize> = (0..options.len()).filter(|&k| options[k].1).collect();
            if options.len() >= 2 {
                a_deux += 1;
            }
            if !proches.is_empty() {
                a_proche += 1;
            }
            if !autres.is_empty() {
                a_autre += 1;
            }
            let utile =
                options.len() >= 2 && proches.iter().any(|&p| autres.iter().any(|&a| a != p));
            if utile {
                utiles += 1;
                utile_ici = true;
            }
            let proches_l: Vec<usize> = (0..options.len()).filter(|&k| options[k].0 < m1).collect();
            if !proches_l.is_empty() {
                a_proche_large += 1;
            }
            if options.len() >= 2 && proches_l.iter().any(|&p| autres.iter().any(|&a| a != p)) {
                utiles_large += 1;
            }
        }
        if utile_ici {
            carrefours_utiles += 1;
        }
    }
    distances_arrivee.sort_by(f64::total_cmp);
    println!("carrefours : {:.1} s", t.elapsed().as_secs_f64());

    // --- Rapport. ---
    let pc = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    let mut md = String::new();
    writeln!(md, "# Continuité de la balade (P0)\n")?;
    writeln!(
        md,
        "Graine `{GRAINE:#x}`, {DEPARTS} départs tirés parmi les {} morceaux accrochés hors monuments, \
         suites de {LONGUEUR} morceaux. Distance = 1 − cosinus des empreintes CLAP. \
         Transitions qui touchent un artiste ancré comptées à part ; morceaux non accrochés retirés.\n",
        candidats.len()
    )?;
    writeln!(
        md,
        "Ville : {} positions réelles, {} accrochés à la voirie, {} ancrés sur monuments ; \
         {} façades sur un tronçon de leur sommet, {} entendues au sommet (façade ailleurs).\n",
        positions.len(),
        ctx.accroches.len(),
        ctx.ancres.len(),
        riverains.facade.len(),
        sans_rue
    )?;
    writeln!(md, "## E1 — continuité\n")?;
    writeln!(
        md,
        "| méthode | suites | long. moy. | transitions | médiane | 9ᵉ décile | changement de famille | même album | médiane hors album | 9ᵉ déc. hors album | ancrées (n, médiane) |"
    )?;
    writeln!(
        md,
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
    )?;
    for b in &bilans {
        writeln!(
            md,
            "| {} | {} | {:.1} | {} | {:.3} | {:.3} | {:.0} % | {:.0} % | {:.3} | {:.3} | {}, {:.3} |",
            b.nom,
            b.suites,
            b.longueurs as f64 / b.suites.max(1) as f64,
            b.ville.len(),
            b.mediane(),
            quantile(&b.ville, 0.9),
            pc(b.changements, b.ville.len()),
            pc(b.meme_album, b.ville.len()),
            quantile(&b.hors_album, 0.5),
            quantile(&b.hors_album, 0.9),
            b.ancrees.len(),
            quantile(&b.ancrees, 0.5),
        )?;
    }
    let p7 = (m1 - m5).abs() < (m1 - m3).abs();
    writeln!(
        md,
        "\n**Critère** : médiane marche {m1:.3}, hasard {m5:.3}, sonique {m3:.3} → |1−5| = {:.3}, |1−3| = {:.3}. \
         **{}**\n",
        (m1 - m5).abs(),
        (m1 - m3).abs(),
        if p7 {
            "La marche est plus proche du hasard : P7 passe avant P1."
        } else {
            "La marche est plus proche du sonique : P1 peut commencer."
        }
    )?;
    writeln!(
        md,
        "Hors transitions intra-album : marche {h1:.3}, hasard {h5:.3}, sonique {h3:.3} → |1−5| = {:.3}, |1−3| = {:.3} ({}).\n",
        (h1 - h5).abs(),
        (h1 - h3).abs(),
        if (h1 - h5).abs() < (h1 - h3).abs() {
            "plus proche du hasard"
        } else {
            "plus proche du sonique"
        }
    )?;
    writeln!(md, "## E2 — carrefours\n")?;
    writeln!(
        md,
        "{carrefours} sommets de degré ≥ 3 dans la zone peuplée (cases de {PAS_ZONE_M} m habitées et leurs voisines). \
         Une branche = la rue suivie jusqu'au prochain carrefour ; son premier morceau est le premier riverain entendu.\n"
    )?;
    writeln!(md, "| branches habitées | carrefours | part |")?;
    writeln!(md, "|---:|---:|---:|")?;
    let max_b = branches_habitees.iter().copied().max().unwrap_or(0);
    for k in 0..=max_b {
        let n = branches_habitees.iter().filter(|&&x| x == k).count();
        let etiquette = if k == max_b && k >= 5 {
            format!("{k}+")
        } else {
            k.to_string()
        };
        writeln!(md, "| {etiquette} | {n} | {:.1} % |", pc(n, carrefours))?;
        if k >= 5 {
            break;
        }
    }
    let mut ent = entropies.clone();
    ent.sort_by(f64::total_cmp);
    writeln!(
        md,
        "\nEntropie des familles des premiers morceaux (carrefours à ≥ 2 branches habitées, n = {}) : \
         médiane {:.2} bit, 1ᵉʳ décile {:.2}, 9ᵉ décile {:.2} ; {:.0} % à 0 (toutes les branches de la même famille).\n",
        ent.len(),
        quantile(&ent, 0.5),
        quantile(&ent, 0.1).max(0.0),
        quantile(&ent, 0.9),
        pc(ent.iter().filter(|&&e| e == 0.0).count(), ent.len()),
    )?;
    writeln!(
        md,
        "Distance du morceau d'arrivée aux premiers morceaux des autres branches : médiane {:.3}, \
         1ᵉʳ décile {:.3}, 9ᵉ décile {:.3} (seuil « proche » = médiane du sonique, {m3:.3}).\n",
        quantile(&distances_arrivee, 0.5),
        quantile(&distances_arrivee, 0.1),
        quantile(&distances_arrivee, 0.9),
    )?;
    writeln!(md, "| arrivées à un carrefour ({arrivees}) | part |")?;
    writeln!(md, "|---|---:|")?;
    writeln!(
        md,
        "| au moins deux suites habitées | {:.1} % |",
        pc(a_deux, arrivees)
    )?;
    writeln!(
        md,
        "| une suite proche (< médiane sonique) | {:.1} % |",
        pc(a_proche, arrivees)
    )?;
    writeln!(
        md,
        "| une suite d'une autre famille | {:.1} % |",
        pc(a_autre, arrivees)
    )?;
    writeln!(
        md,
        "| **utile** : ≥ 2 suites, dont une proche et une autre d'une autre famille | **{:.1} %** |",
        pc(utiles, arrivees)
    )?;
    writeln!(
        md,
        "| variante, « proche » = sous la médiane de la marche ({m1:.3}) : une suite proche | {:.1} % |",
        pc(a_proche_large, arrivees)
    )?;
    writeln!(
        md,
        "| variante : utile | {:.1} % |",
        pc(utiles_large, arrivees)
    )?;
    writeln!(
        md,
        "\nCarrefours avec au moins une arrivée utile : {carrefours_utiles} sur {carrefours} ({:.1} %).",
        pc(carrefours_utiles, carrefours)
    )?;
    std::fs::write(sortie.join("continuite.md"), &md)?;

    let series: Vec<(String, Vec<f64>)> = bilans
        .iter()
        .map(|b| (b.nom.clone(), b.ville.clone()))
        .collect();
    let max_d = bilans
        .iter()
        .map(|b| quantile(&b.ville, 0.995))
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max)
        .max(0.05);
    std::fs::write(
        sortie.join("distances.svg"),
        histogrammes(
            "E1 — distance CLAP entre morceaux consécutifs",
            "distance cosinus",
            &series,
            max_d,
        ),
    )?;
    let max_e = ent.last().copied().unwrap_or(1.0).max(1.0);
    std::fs::write(
        sortie.join("entropie.svg"),
        histogrammes(
            "E2 — entropie des familles aux carrefours",
            "entropie (bits)",
            &[(
                format!(
                    "≥ 2 branches · {:.0} % à 0 bit",
                    pc(ent.iter().filter(|&&e| e == 0.0).count(), ent.len())
                ),
                entropies,
            )],
            max_e,
        ),
    )?;
    print!("{md}");
    println!(
        "\n→ {} ({:.1} s)",
        sortie.display(),
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
