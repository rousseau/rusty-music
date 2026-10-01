// SPDX-License-Identifier: GPL-3.0-or-later
//! Le peuplement chronologique de la ville réelle : les morceaux arrivent **par
//! date de sortie** et s'installent petit à petit, comme des habitants.
//!
//! `docs/carto-ville.md` (« La croissance »). Trois règles, dans cet ordre de
//! priorité, et chacune se lit sur la carte :
//!
//! 1. **L'âge se lit dans la distance.** Le tout premier morceau de la
//!    bibliothèque habite l'île de la Cité ; la ville s'étend à mesure que les
//!    morceaux arrivent. L'étendue de la ville à un instant donné est un
//!    *front* — « ouvrir assez de quartiers pour loger ceux qui sont arrivés,
//!    plus une marge » — pas un disque : la distance est le **coût de voirie**
//!    ([`Parcelle::cout`], `crate::cout_voirie`), si bien que les grandes
//!    artères portent le front beaucoup plus loin que les rues du tissu.
//! 2. **Les artères sont la voie rapide : la popularité s'y installe.** Les
//!    morceaux très écoutés prennent les bâtiments qui bordent les grands axes
//!    ([`Parcelle::artere`]) ; les morceaux confidentiels se retirent dans les
//!    petits quartiers entre deux avenues.
//! 3. **Un petit quartier a un style.** La ville est découpée une fois pour
//!    toutes en [`Cellule`]s (une poignée de dizaines de bâtiments chacune).
//!    Un album arrive en bloc et choisit sa cellule : il rejoint celle dont le
//!    style (barycentre t-SNE de ses habitants) est le plus proche, ou en
//!    fonde une nouvelle à côté de cellules qui lui ressemblent — ce qui
//!    dessine des **gradients** de style plutôt que des frontières nettes.
//!
//! Ce module est **pur** : il ne connaît ni OSM ni SQLite, seulement des
//! parcelles (position, coût, bordure d'artère) et des arrivants (date,
//! album, position t-SNE, popularité). `crate::ville` fait la plomberie.
//!
//! Ce n'est pas la stabilité du `peuplement` du monde fictif (spirale
//! phyllotaxique, position jamais recalculée) : ici la place d'un morceau
//! dépend de ce qui l'a précédé — mais le calcul est déterministe, deux
//! exécutions donnent la même carte.

use std::collections::{HashMap, HashSet};

/// Un bâtiment habitable de la zone peuplée.
#[derive(Clone, Debug)]
pub struct Parcelle {
    /// Identifiant OSM du bâtiment.
    pub id: i64,
    /// Centre, mètres du repère local (`affectation::Repere`).
    pub centre: [f64; 2],
    /// Coût de déplacement sur la voirie depuis le cœur historique
    /// (`cout_voirie::couts_batiments`) : le « temps » de la ville. Faible le
    /// long des artères, fort au fond des impasses.
    pub cout: f64,
    /// Importance de la voie qui borde le bâtiment, `0..=1` : 1 pour un grand
    /// axe, ~0,1 pour la rue ordinaire.
    pub artere: f64,
}

/// Un morceau à loger.
#[derive(Clone, Debug)]
pub struct Arrivant {
    pub id: i64,
    /// Année de sortie. `None` = pas de date fiable : arrive en dernier, comme
    /// ce qu'on vient d'ajouter à la bibliothèque (`docs/carto-peuplement-
    /// architecture.md` §2.2, échelon `ingestion`).
    pub annee: Option<i32>,
    /// Artiste de regroupement (`album_artist`, repli sur `artist`).
    pub artiste: String,
    pub album: String,
    pub piste: i64,
    pub famille: i64,
    /// Position t-SNE : la mesure de ressemblance de style entre deux morceaux.
    pub xy: [f32; 2],
    /// Popularité générale, rang percentile `0..=1` (`track_popularite`).
    pub popularite: Option<f64>,
}

/// Réglages. Les poids sont **calibrés à l'œil et sur les mesures de
/// `examples/croissance_apercu.rs`**, comme le reste du crate — voir
/// `docs/carto-ville.md` pour ce que chacun fait bouger.
#[derive(Clone, Copy, Debug)]
pub struct Parametres {
    /// Taille visée d'un petit quartier (bâtiments).
    pub taille_quartier: usize,
    /// Taille visée d'un tronçon d'artère — plus petit : un couloir est étroit
    /// et long, il doit rester un « bout d'avenue ».
    pub taille_couloir: usize,
    /// `artere` à partir de laquelle un bâtiment appartient à un couloir.
    pub seuil_couloir: f64,
    /// Marge du front : la ville ouvre assez de cellules pour loger
    /// `(1 + marge) ×` les morceaux arrivés. Plus elle est grande, plus les
    /// styles ont de cellules vierges où se différencier, plus le centre reste
    /// troué (loger plus tard des morceaux plus récents).
    pub marge_ouverture: f64,
    /// Places ouvertes d'avance avant le premier morceau — l'île de la Cité
    /// n'est pas une seule rue.
    pub base_ouverture: usize,
    /// Portée de l'affinité de style, en fraction de l'étalement t-SNE
    /// (p95 de la distance au barycentre).
    pub sigma_style: f64,
    /// Poids de la part de la famille de l'album dans la cellule.
    pub w_famille: f64,
    /// Poids de l'appariement popularité ↔ artère.
    pub w_artere: f64,
    /// Préférence pour les cellules les plus proches du cœur parmi les
    /// ouvertes : la ville se remplit de l'intérieur.
    pub w_proche: f64,
    /// Bonus à une cellule qui loge déjà l'artiste de l'album.
    pub w_artiste: f64,
    /// Coût de fonder une cellule vierge plutôt que d'en rejoindre une.
    pub w_fondation: f64,
    /// Coût de couper un album en deux cellules (proportionnel à la part qui
    /// déborde).
    pub w_decoupe: f64,
    /// Distance sous laquelle deux bâtiments de cellules différentes rendent
    /// ces cellules voisines, mètres.
    pub rayon_voisinage: f64,
}

impl Default for Parametres {
    fn default() -> Self {
        Self {
            taille_quartier: 90,
            taille_couloir: 45,
            seuil_couloir: 0.75,
            marge_ouverture: 0.20,
            base_ouverture: 150,
            sigma_style: 0.35,
            w_famille: 1.5,
            w_artere: 2.0,
            w_proche: 2.0,
            w_artiste: 1.0,
            w_fondation: 0.6,
            w_decoupe: 1.0,
            rayon_voisinage: 70.0,
        }
    }
}

/// Un petit quartier (ou un tronçon d'artère) : l'unité à laquelle un album
/// choisit sa place.
#[derive(Clone, Debug)]
pub struct Cellule {
    pub id: u32,
    /// Indices dans le tableau de parcelles.
    pub parcelles: Vec<usize>,
    pub couloir: bool,
    pub centre: [f64; 2],
    /// Coût médian de ses parcelles : la date à laquelle le front l'atteint.
    pub cout: f64,
    /// Bordure d'artère moyenne de ses parcelles.
    pub artere: f64,
    pub voisines: Vec<u32>,
}

/// Où un morceau a été logé.
#[derive(Clone, Copy, Debug)]
pub struct Adresse {
    pub id: i64,
    /// Indice dans le tableau de parcelles.
    pub parcelle: usize,
    pub cellule: u32,
}

pub struct Croissance {
    pub adresses: Vec<Adresse>,
    pub cellules: Vec<Cellule>,
    /// Cellule de chaque parcelle.
    pub cellule_de_parcelle: Vec<u32>,
    /// Rang de fondation de chaque cellule (`None` = jamais habitée).
    pub fondation: Vec<Option<u32>>,
    /// Morceaux sans parcelle (plus de morceaux que de bâtiments).
    pub sans_adresse: Vec<i64>,
}

// ---------------------------------------------------------------------------
// Découpage en cellules.
// ---------------------------------------------------------------------------

/// Bissection récursive d'un nuage de parcelles en feuilles de taille bornée :
/// on coupe à la médiane, selon l'axe le plus étendu. Les feuilles sont donc
/// **équilibrées en bâtiments**, quelle que soit la densité — un îlot dense du
/// centre fait de petites cellules, la périphérie de plus grandes.
fn bissecter(ids: &mut [usize], parcelles: &[Parcelle], max: usize, sortie: &mut Vec<Vec<usize>>) {
    if ids.len() <= max {
        if !ids.is_empty() {
            sortie.push(ids.to_vec());
        }
        return;
    }
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for &i in ids.iter() {
        let c = parcelles[i].centre;
        b[0] = b[0].min(c[0]);
        b[1] = b[1].min(c[1]);
        b[2] = b[2].max(c[0]);
        b[3] = b[3].max(c[1]);
    }
    let axe = if b[2] - b[0] >= b[3] - b[1] { 0 } else { 1 };
    ids.sort_by(|&a, &c| {
        parcelles[a].centre[axe]
            .total_cmp(&parcelles[c].centre[axe])
            .then(parcelles[a].id.cmp(&parcelles[c].id))
    });
    let milieu = ids.len() / 2;
    let (g, d) = ids.split_at_mut(milieu);
    bissecter(g, parcelles, max, sortie);
    bissecter(d, parcelles, max, sortie);
}

fn mediane(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

/// Découpe la zone peuplée en cellules : d'abord les bâtiments qui bordent un
/// grand axe (couloirs), puis le reste (petits quartiers). Deux découpages
/// séparés plutôt qu'un seul : sans ça une cellule mêlerait un bout d'avenue
/// et les rues qui la bordent, et « la popularité s'installe sur l'artère »
/// n'aurait plus de sens à l'échelle de la cellule.
pub fn decouper(parcelles: &[Parcelle], p: &Parametres) -> Vec<Cellule> {
    let (mut couloir, mut quartier): (Vec<usize>, Vec<usize>) =
        (0..parcelles.len()).partition(|&i| parcelles[i].artere >= p.seuil_couloir);
    let mut feuilles: Vec<(Vec<usize>, bool)> = Vec::new();
    let mut tmp = Vec::new();
    bissecter(&mut couloir, parcelles, p.taille_couloir.max(2), &mut tmp);
    feuilles.extend(tmp.drain(..).map(|f| (f, true)));
    bissecter(&mut quartier, parcelles, p.taille_quartier.max(2), &mut tmp);
    feuilles.extend(tmp.drain(..).map(|f| (f, false)));

    let mut cellules: Vec<Cellule> = feuilles
        .into_iter()
        .map(|(ids, couloir)| {
            let n = ids.len() as f64;
            let centre = ids.iter().fold([0.0, 0.0], |a, &i| {
                [a[0] + parcelles[i].centre[0] / n, a[1] + parcelles[i].centre[1] / n]
            });
            let cout = mediane(ids.iter().map(|&i| parcelles[i].cout).collect());
            let artere = ids.iter().map(|&i| parcelles[i].artere).sum::<f64>() / n;
            Cellule { id: 0, parcelles: ids, couloir, centre, cout, artere, voisines: Vec::new() }
        })
        .collect();
    // Numérotation stable : par coût croissant (donc par « âge » de la cellule),
    // centre en départage.
    cellules.sort_by(|a, b| {
        a.cout
            .total_cmp(&b.cout)
            .then(a.centre[0].total_cmp(&b.centre[0]))
            .then(a.centre[1].total_cmp(&b.centre[1]))
    });
    for (i, c) in cellules.iter_mut().enumerate() {
        c.id = i as u32;
    }

    // Voisinage : deux cellules dont deux bâtiments sont à moins de
    // `rayon_voisinage`. Grille de hachage au pas du rayon.
    let pas = p.rayon_voisinage.max(1.0);
    let cle = |c: [f64; 2]| ((c[0] / pas).floor() as i32, (c[1] / pas).floor() as i32);
    let mut cellule_de = vec![0u32; parcelles.len()];
    for c in &cellules {
        for &i in &c.parcelles {
            cellule_de[i] = c.id;
        }
    }
    let mut grille: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, pc) in parcelles.iter().enumerate() {
        grille.entry(cle(pc.centre)).or_default().push(i);
    }
    let r2 = pas * pas;
    let mut voisines: Vec<HashSet<u32>> = vec![HashSet::new(); cellules.len()];
    for (i, pc) in parcelles.iter().enumerate() {
        let (cx, cy) = cle(pc.centre);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(v) = grille.get(&(cx + dx, cy + dy)) else { continue };
                for &j in v {
                    if cellule_de[j] == cellule_de[i] {
                        continue;
                    }
                    let q = parcelles[j].centre;
                    if (q[0] - pc.centre[0]).powi(2) + (q[1] - pc.centre[1]).powi(2) <= r2 {
                        voisines[cellule_de[i] as usize].insert(cellule_de[j]);
                    }
                }
            }
        }
    }
    for (c, v) in cellules.iter_mut().zip(voisines) {
        let mut v: Vec<u32> = v.into_iter().collect();
        v.sort_unstable();
        c.voisines = v;
    }
    cellules
}

// ---------------------------------------------------------------------------
// La croissance.
// ---------------------------------------------------------------------------

/// Hachage FNV-1a : stable d'une version de Rust à l'autre, contrairement à
/// `DefaultHasher` — l'ordre des albums d'une même année doit se retrouver.
fn fnv(parties: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in parties {
        for b in s.bytes().chain(std::iter::once(0xff)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Un album qui arrive : le morceau d'un même album sorti la même année, en
/// bloc. (Une compilation de plusieurs décennies se scinde en une arrivée par
/// année, chacune à sa date.)
struct Arrivee {
    pistes: Vec<usize>,
    date: i32,
    cle: u64,
    xy: [f64; 2],
    famille: i64,
    popularite: f64,
    artiste: u32,
}

#[derive(Default)]
struct Habitat {
    /// Parcelles encore libres, du coût le plus bas au plus haut.
    libres: Vec<usize>,
    somme: [f64; 2],
    n: u32,
    familles: HashMap<i64, u32>,
    artistes: HashSet<u32>,
}

fn regrouper(arrivants: &[Arrivant]) -> Vec<Arrivee> {
    let mut artistes: HashMap<&str, u32> = HashMap::new();
    let mut moyenne_artiste: HashMap<&str, (f64, u32)> = HashMap::new();
    for a in arrivants {
        let n = artistes.len() as u32;
        artistes.entry(a.artiste.as_str()).or_insert(n);
        if let Some(p) = a.popularite {
            let e = moyenne_artiste.entry(a.artiste.as_str()).or_insert((0.0, 0));
            e.0 += p;
            e.1 += 1;
        }
    }
    let mut groupes: HashMap<(&str, &str, Option<i32>), Vec<usize>> = HashMap::new();
    for (i, a) in arrivants.iter().enumerate() {
        groupes.entry((a.artiste.as_str(), a.album.as_str(), a.annee)).or_default().push(i);
    }
    let mut arrivees: Vec<Arrivee> = groupes
        .into_iter()
        .map(|((artiste, album, annee), mut pistes)| {
            pistes.sort_by_key(|&i| (arrivants[i].piste, arrivants[i].id));
            let n = pistes.len() as f64;
            let xy = pistes.iter().fold([0.0, 0.0], |s, &i| {
                [s[0] + arrivants[i].xy[0] as f64 / n, s[1] + arrivants[i].xy[1] as f64 / n]
            });
            let mut parts: HashMap<i64, usize> = HashMap::new();
            for &i in &pistes {
                *parts.entry(arrivants[i].famille).or_default() += 1;
            }
            let famille = parts
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                .map(|(f, _)| f)
                .unwrap_or(-1);
            let connues: Vec<f64> = pistes.iter().filter_map(|&i| arrivants[i].popularite).collect();
            let popularite = if connues.is_empty() {
                // Repli : la popularité moyenne de l'artiste, sinon neutre — un
                // morceau inconnu des sources n'est ni tête d'affiche ni
                // confidentiel, il ne tire pas vers les artères.
                moyenne_artiste.get(artiste).map(|(s, n)| s / *n as f64).unwrap_or(0.5)
            } else {
                connues.iter().sum::<f64>() / connues.len() as f64
            };
            let date = annee.unwrap_or(i32::MAX);
            let cle = fnv(&[artiste, album, &date.to_string()]);
            Arrivee { pistes, date, cle, xy, famille, popularite, artiste: artistes[artiste] }
        })
        .collect();
    arrivees.sort_by_key(|a| (a.date, a.cle));
    arrivees
}

/// Étalement t-SNE du nuage : p95 de la distance au barycentre.
fn etalement(arrivants: &[Arrivant]) -> f64 {
    let n = arrivants.len().max(1) as f64;
    let b = arrivants
        .iter()
        .fold([0.0, 0.0], |s, a| [s[0] + a.xy[0] as f64 / n, s[1] + a.xy[1] as f64 / n]);
    let mut d: Vec<f64> = arrivants
        .iter()
        .map(|a| ((a.xy[0] as f64 - b[0]).powi(2) + (a.xy[1] as f64 - b[1]).powi(2)).sqrt())
        .collect();
    d.sort_by(f64::total_cmp);
    d.get(((d.len() as f64) * 0.95) as usize).copied().unwrap_or(1.0).max(1e-9)
}

/// Fait croître la ville : loge chaque arrivant dans une parcelle.
///
/// `parcelles` est la zone peuplée ; il y en a au plus autant que d'arrivants
/// (ceux qui restent sans parcelle sont rendus dans [`Croissance::sans_adresse`],
/// jamais ignorés en silence).
pub fn peupler(parcelles: &[Parcelle], arrivants: &[Arrivant], p: &Parametres) -> Croissance {
    let cellules = decouper(parcelles, p);
    let mut cellule_de_parcelle = vec![0u32; parcelles.len()];
    for c in &cellules {
        for &i in &c.parcelles {
            cellule_de_parcelle[i] = c.id;
        }
    }
    let k = cellules.len();
    let arrivees = regrouper(arrivants);
    let sigma2 = (p.sigma_style * etalement(arrivants)).powi(2).max(1e-12);

    let mut habitats: Vec<Habitat> = cellules
        .iter()
        .map(|c| {
            let mut libres = c.parcelles.clone();
            libres.sort_by(|&a, &b| {
                parcelles[a].cout.total_cmp(&parcelles[b].cout).then(parcelles[a].id.cmp(&parcelles[b].id))
            });
            Habitat { libres, ..Default::default() }
        })
        .collect();

    // Les cellules numérotées par coût croissant (`decouper`) : le front est un
    // préfixe `0..ouvertes`.
    let mut ouvertes = 0usize;
    let mut capacite_ouverte = 0usize;
    let mut places = 0usize;
    let mut fondation: Vec<Option<u32>> = vec![None; k];
    let mut fondees = 0u32;
    let mut adresses: Vec<Adresse> = Vec::with_capacity(arrivants.len());
    let mut sans_adresse: Vec<i64> = Vec::new();

    for arrivee in &arrivees {
        let mut restantes: Vec<usize> = arrivee.pistes.clone();
        while !restantes.is_empty() {
            // Le front : assez de places pour ceux qui sont arrivés, plus la
            // marge. Il ne recule jamais.
            let cible = ((places as f64 + restantes.len() as f64) * (1.0 + p.marge_ouverture)) as usize
                + p.base_ouverture;
            while ouvertes < k && capacite_ouverte < cible {
                capacite_ouverte += cellules[ouvertes].parcelles.len();
                ouvertes += 1;
            }
            // Au moins une cellule ouverte doit avoir de la place.
            while ouvertes < k && habitats[..ouvertes].iter().all(|h| h.libres.is_empty()) {
                capacite_ouverte += cellules[ouvertes].parcelles.len();
                ouvertes += 1;
            }
            let Some(choix) = meilleure_cellule(
                &cellules, &habitats, ouvertes, arrivee, restantes.len(), sigma2, p,
            ) else {
                // Plus aucune parcelle libre dans toute la ville.
                sans_adresse.extend(restantes.iter().map(|&i| arrivants[i].id));
                restantes.clear();
                break;
            };
            if fondation[choix].is_none() {
                fondation[choix] = Some(fondees);
                fondees += 1;
            }
            let h = &mut habitats[choix];
            let m = restantes.len().min(h.libres.len());
            // Un album occupe des bâtiments contigus : le premier au plus bas
            // coût (le côté intérieur de la cellule), chaque suivant au plus
            // proche du précédent.
            let mut dernier: Option<usize> = None;
            for &i in restantes.iter().take(m) {
                let pos = match dernier {
                    None => 0,
                    Some(d) => {
                        let c = parcelles[d].centre;
                        h.libres
                            .iter()
                            .enumerate()
                            .min_by(|(_, &a), (_, &b)| {
                                let da = (parcelles[a].centre[0] - c[0]).powi(2)
                                    + (parcelles[a].centre[1] - c[1]).powi(2);
                                let db = (parcelles[b].centre[0] - c[0]).powi(2)
                                    + (parcelles[b].centre[1] - c[1]).powi(2);
                                da.total_cmp(&db).then(a.cmp(&b))
                            })
                            .map(|(pos, _)| pos)
                            .unwrap_or(0)
                    }
                };
                let parcelle = h.libres.remove(pos);
                dernier = Some(parcelle);
                h.somme[0] += arrivants[i].xy[0] as f64;
                h.somme[1] += arrivants[i].xy[1] as f64;
                h.n += 1;
                *h.familles.entry(arrivants[i].famille).or_default() += 1;
                adresses.push(Adresse { id: arrivants[i].id, parcelle, cellule: choix as u32 });
            }
            h.artistes.insert(arrivee.artiste);
            places += m;
            restantes.drain(..m);
        }
    }

    Croissance { adresses, cellules, cellule_de_parcelle, fondation, sans_adresse }
}

/// Barycentre t-SNE et part de la famille d'une cellule. Une cellule vierge
/// emprunte ce qu'en disent ses voisines habitées (le gradient de style),
/// `None` si elles sont vierges aussi.
fn style_de(
    k: usize,
    cellules: &[Cellule],
    habitats: &[Habitat],
    famille: i64,
) -> Option<([f64; 2], f64)> {
    let h = &habitats[k];
    if h.n > 0 {
        let n = h.n as f64;
        let part = h.familles.get(&famille).copied().unwrap_or(0) as f64 / n;
        return Some(([h.somme[0] / n, h.somme[1] / n], part));
    }
    let (mut s, mut part, mut m) = ([0.0, 0.0], 0.0, 0.0);
    for &v in &cellules[k].voisines {
        let hv = &habitats[v as usize];
        if hv.n == 0 {
            continue;
        }
        let n = hv.n as f64;
        s[0] += hv.somme[0] / n;
        s[1] += hv.somme[1] / n;
        part += hv.familles.get(&famille).copied().unwrap_or(0) as f64 / n;
        m += 1.0;
    }
    (m > 0.0).then(|| ([s[0] / m, s[1] / m], part / m))
}

fn meilleure_cellule(
    cellules: &[Cellule],
    habitats: &[Habitat],
    ouvertes: usize,
    a: &Arrivee,
    restantes: usize,
    sigma2: f64,
    p: &Parametres,
) -> Option<usize> {
    let tire = 2.0 * a.popularite - 1.0; // -1 confidentiel … +1 tête d'affiche
    let mut meilleur: Option<(usize, f64)> = None;
    for k in 0..ouvertes {
        let h = &habitats[k];
        if h.libres.is_empty() {
            continue;
        }
        let mut s = 0.0;
        match style_de(k, cellules, habitats, a.famille) {
            Some((c, part)) => {
                let d2 = ((a.xy[0] - c[0]).powi(2) + (a.xy[1] - c[1]).powi(2)) / sigma2;
                s -= d2.min(9.0);
                s += p.w_famille * part;
            }
            // Cellule vierge entourée de vierges : aucun style à continuer.
            None => s -= 2.0,
        }
        if h.n == 0 {
            s -= p.w_fondation;
        }
        // Popularité ↔ artère : +1 si une tête d'affiche est sur un couloir,
        // -1 si elle est dans un petit quartier ; l'inverse pour un morceau
        // confidentiel.
        let artere = 2.0 * cellules[k].artere.clamp(0.0, 1.0) - 1.0;
        s += p.w_artere * tire * artere;
        // Un morceau sans date n'a pas de place dans l'histoire : il vient
        // d'entrer dans la bibliothèque, donc il va là où la ville s'étend
        // encore, pas dans les trous du vieux centre.
        let eloignement = k as f64 / ouvertes.max(1) as f64;
        s -= p.w_proche * if a.date == i32::MAX { 1.0 - eloignement } else { eloignement };
        if h.artistes.contains(&a.artiste) {
            s += p.w_artiste;
        }
        if h.libres.len() < restantes {
            s -= p.w_decoupe * (1.0 - h.libres.len() as f64 / restantes as f64);
        }
        if meilleur.is_none_or(|(_, ms)| s > ms) {
            meilleur = Some((k, s));
        }
    }
    meilleur.map(|(k, _)| k)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une ville-grille de 60 × 60 bâtiments (pas 20 m). Cœur au centre ; une
    /// avenue horizontale et une verticale (artère 1, coût divisé par 5).
    fn ville() -> Vec<Parcelle> {
        let mut v = Vec::new();
        let n = 60;
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 * 20.0 - 600.0, j as f64 * 20.0 - 600.0);
                let avenue = i == n / 2 || j == n / 2;
                let d = (x * x + y * y).sqrt();
                v.push(Parcelle {
                    id: (i * n + j) as i64,
                    centre: [x, y],
                    cout: if avenue { d * 0.2 } else { d },
                    artere: if avenue { 1.0 } else { 0.1 },
                });
            }
        }
        v
    }

    /// 3 600 morceaux de 1960 à 2020, trois styles aux positions t-SNE
    /// distinctes ; un morceau sur trois est très populaire.
    fn morceaux() -> Vec<Arrivant> {
        let mut v = Vec::new();
        for i in 0..3600i64 {
            let album = i / 12;
            // Style, artiste et popularité sont ceux de l'album : c'est lui qui
            // arrive en bloc.
            let famille = album % 3;
            let (cx, cy) = [(-3.0f32, 0.0f32), (3.0, 0.0), (0.0, 4.0)][famille as usize];
            v.push(Arrivant {
                id: i,
                annee: Some(1960 + (album * 60 / 300) as i32),
                artiste: format!("artiste {}", album % 39),
                album: format!("album {album}"),
                piste: i % 12,
                famille,
                xy: [cx + (i % 7) as f32 * 0.05, cy + (i % 5) as f32 * 0.05],
                popularite: Some(((album * 7919) % 1000) as f64 / 1000.0),
            });
        }
        v
    }

    fn spearman(a: &[f64], b: &[f64]) -> f64 {
        fn rangs(v: &[f64]) -> Vec<f64> {
            let mut idx: Vec<usize> = (0..v.len()).collect();
            idx.sort_by(|&x, &y| v[x].total_cmp(&v[y]));
            let mut r = vec![0.0; v.len()];
            for (rang, &i) in idx.iter().enumerate() {
                r[i] = rang as f64;
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

    #[test]
    fn chaque_morceau_a_exactement_une_parcelle() {
        let (parcelles, arrivants) = (ville(), morceaux());
        let c = peupler(&parcelles, &arrivants, &Parametres::default());
        assert!(c.sans_adresse.is_empty());
        assert_eq!(c.adresses.len(), arrivants.len());
        let parcelles_prises: HashSet<usize> = c.adresses.iter().map(|a| a.parcelle).collect();
        let morceaux_loges: HashSet<i64> = c.adresses.iter().map(|a| a.id).collect();
        assert_eq!(parcelles_prises.len(), arrivants.len(), "une parcelle ne loge qu'un morceau");
        assert_eq!(morceaux_loges.len(), arrivants.len());
    }

    #[test]
    fn plus_de_morceaux_que_de_batiments_rend_le_surplus() {
        let (parcelles, mut arrivants) = (ville(), morceaux());
        arrivants.truncate(3600);
        let petite = &parcelles[..3000];
        let c = peupler(petite, &arrivants, &Parametres::default());
        assert_eq!(c.adresses.len(), 3000);
        assert_eq!(c.sans_adresse.len(), 600);
    }

    #[test]
    fn le_plus_ancien_habite_le_plus_pres_du_coeur() {
        let (parcelles, arrivants) = (ville(), morceaux());
        let c = peupler(&parcelles, &arrivants, &Parametres::default());
        let par_id: HashMap<i64, &Arrivant> = arrivants.iter().map(|a| (a.id, a)).collect();
        let annees: Vec<f64> = c.adresses.iter().map(|a| par_id[&a.id].annee.unwrap() as f64).collect();
        let couts: Vec<f64> = c.adresses.iter().map(|a| parcelles[a.parcelle].cout).collect();
        let rho = spearman(&annees, &couts);
        assert!(rho > 0.6, "âge ↔ coût de voirie : ρ = {rho:.2}");
        // Les cinquante premiers morceaux sont dans les cinquante parcelles
        // les moins chères, ou presque.
        let mut tous: Vec<f64> = parcelles.iter().map(|p| p.cout).collect();
        tous.sort_by(f64::total_cmp);
        let seuil = tous[tous.len() / 10];
        let premiers = c.adresses.iter().filter(|a| a.id < 120).count();
        let proches = c
            .adresses
            .iter()
            .filter(|a| a.id < 120 && parcelles[a.parcelle].cout <= seuil)
            .count();
        assert!(proches * 10 >= premiers * 9, "{proches}/{premiers} dans le premier décile");
    }

    #[test]
    fn la_popularite_s_installe_sur_les_artères() {
        let (parcelles, arrivants) = (ville(), morceaux());
        let c = peupler(&parcelles, &arrivants, &Parametres::default());
        let par_id: HashMap<i64, &Arrivant> = arrivants.iter().map(|a| (a.id, a)).collect();
        let moyenne = |sur_artere: bool| {
            let v: Vec<f64> = c
                .adresses
                .iter()
                .filter(|a| (parcelles[a.parcelle].artere >= 0.75) == sur_artere)
                .filter_map(|a| par_id[&a.id].popularite)
                .collect();
            v.iter().sum::<f64>() / v.len() as f64
        };
        let (sur, hors) = (moyenne(true), moyenne(false));
        assert!(sur > hors + 0.1, "popularité moyenne : artères {sur:.2}, quartiers {hors:.2}");
    }

    #[test]
    fn les_styles_se_regroupent_en_quartiers() {
        let (parcelles, arrivants) = (ville(), morceaux());
        let c = peupler(&parcelles, &arrivants, &Parametres::default());
        let par_id: HashMap<i64, &Arrivant> = arrivants.iter().map(|a| (a.id, a)).collect();
        // Pureté moyenne des cellules habitées : part de la famille majoritaire.
        let mut par_cellule: HashMap<u32, HashMap<i64, usize>> = HashMap::new();
        for a in &c.adresses {
            *par_cellule.entry(a.cellule).or_default().entry(par_id[&a.id].famille).or_default() += 1;
        }
        let (mut dominant, mut total) = (0usize, 0usize);
        for fam in par_cellule.values() {
            dominant += fam.values().max().copied().unwrap_or(0);
            total += fam.values().sum::<usize>();
        }
        let purete = dominant as f64 / total as f64;
        assert!(purete > 0.6, "pureté des cellules : {purete:.2} (le hasard fait ~0,33)");
    }

    #[test]
    fn le_calcul_est_deterministe() {
        let (parcelles, arrivants) = (ville(), morceaux());
        let a = peupler(&parcelles, &arrivants, &Parametres::default());
        let b = peupler(&parcelles, &arrivants, &Parametres::default());
        let pa: Vec<(i64, usize)> = a.adresses.iter().map(|x| (x.id, x.parcelle)).collect();
        let pb: Vec<(i64, usize)> = b.adresses.iter().map(|x| (x.id, x.parcelle)).collect();
        assert_eq!(pa, pb);
    }
}
