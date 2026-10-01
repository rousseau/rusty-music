// SPDX-License-Identifier: GPL-3.0-or-later
//! Assemble une [`Source`] depuis le vrai plan de ville (`crates/osm`) plutôt
//! que depuis le monde engendré.
//!
//! `docs/carto-ville.md` pose le modèle : trois étages d'affectation
//! (familles → quartiers, artistes → rues, morceaux → adresses),
//! `crate::affectation` les exécute. Ce module les enchaîne — c'est la même
//! séquence que `crates/cli/src/main.rs` rejoue pour `Quartiers`/`Rues`/
//! `Adresses`, factorisée ici pour que le CLI et l'application de bureau la
//! partagent au lieu d'en tenir chacun leur copie.

use std::collections::{HashMap, HashSet};

use rusty_music_core::db::MapPoint;
use rusty_music_osm::{Extrait, Troncon};

use crate::affectation::{self, Artiste, Famille, Repere};
use crate::batiments::GrilleBatiments;
use crate::croissance::{self, Arrivant, Parcelle};
use crate::source::{ContourReel, Morceau, Source, TronconReel};

/// Distance par défaut entre deux adresses le long d'une rue, mètres — la
/// même que celle du CLI (`crates/cli/src/main.rs`, `Cmd::Rues`/`Adresses`).
pub const ESPACEMENT_PAR_DEFAUT: f64 = 4.0;

/// Accumulateur par artiste : somme des x, somme des y, effectif, et le
/// nombre de morceaux par famille (pour trancher la famille dominante).
type CumulArtiste = (f64, f64, usize, HashMap<i64, usize>);

/// Les morceaux d'un même album déjà logés : identifiant et position `[lon, lat]`.
type GroupeAlbum<'a> = HashMap<(&'a str, &'a str), Vec<(i64, [f64; 2])>>;

/// Le type de voie affiché, par classe OSM — `docs/carto-ville.md` fixe la
/// table ; un seul choix par classe pour l'instant, pas de variété tirée au
/// hasard (Avenue vs Boulevard, par exemple).
fn type_de_voie(classe: rusty_music_osm::Classe) -> &'static str {
    use rusty_music_osm::Classe::*;
    match classe {
        Autoroute => "Boulevard Périphérique",
        Primaire => "Avenue",
        Secondaire | Tertiaire => "Rue",
        Residentielle => "Rue",
        Pietonne => "Passage",
        Service => "Impasse",
    }
}

/// Le nom affiché d'une rue — type de voie + artiste. Aucun toponyme réel :
/// `docs/carto-ville.md` le décide, et `nom_osm` (caché) garde le vrai nom
/// pour la traçabilité seule.
fn nom_affiche(classe: rusty_music_osm::Classe, artiste: &str) -> String {
    format!("{} {}", type_de_voie(classe), artiste)
}

/// Le rectangle englobant du territoire, en mètres du repère local : la
/// frontière communale si elle existe, sinon l'enveloppe des centres de rue.
/// Sert de domaine à `affectation::territoires`.
fn bornes_locales(extrait: &Extrait, rues: &[affectation::Rue], repere: &affectation::Repere) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut voir = |p: [f64; 2]| {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    };
    match &extrait.frontiere {
        Some(f) => {
            for anneau in &f.anneaux {
                for p in anneau {
                    voir(repere.vers_m(*p));
                }
            }
        }
        None => {
            for r in rues {
                voir(r.centre);
            }
        }
    }
    if !b[0].is_finite() {
        b = [-1000.0, -1000.0, 1000.0, 1000.0];
    }
    b
}

/// Le cœur historique de Paris : l'île de la Cité. Le peuplement dense part de
/// là (`docs/carto-ville.md`). En dur parce que le crate reste agnostique à la
/// commune — l'appelant qui sait faire Paris passe cette constante, les autres
/// passent `None` et retombent sur le centre de masse du bâti.
pub const ILE_DE_LA_CITE: [f64; 2] = [2.3470, 48.8550];

fn distance2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// Barycentre des centroïdes de familles, pondéré par effectif — le même point
/// que le `centre_f` interne de `affectation::semer_impl`, recalculé ici pour
/// mesurer l'écart t-SNE de chaque morceau à peupler.
fn barycentre_familles(familles: &[Famille]) -> [f64; 2] {
    let poids: f64 = familles.iter().map(|f| f.effectif as f64).sum::<f64>().max(1e-9);
    let (sx, sy) = familles.iter().fold((0.0, 0.0), |(sx, sy), f| {
        (sx + f.centroide[0] as f64 * f.effectif as f64, sy + f.centroide[1] as f64 * f.effectif as f64)
    });
    [sx / poids, sy / poids]
}

/// 95ᵉ centile d'une suite de réels — robuste aux quelques points extrêmes que
/// t-SNE laisse traîner et qui, pris comme rayon, gonfleraient l'échelle.
fn p95(valeurs: impl Iterator<Item = f64>) -> f64 {
    let mut v: Vec<f64> = valeurs.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 * 0.95) as usize).min(v.len() - 1)]
}

/// `true` si un bâtiment du noyau borde la rue `nom` — le critère qui décide
/// quelles rues entrent dans l'affectation (celles de la zone peuplée).
fn borde_le_noyau(
    nom: &str,
    traces: &HashMap<String, affectation::Trace>,
    grille: &GrilleBatiments,
    autorises: &HashSet<i64>,
) -> bool {
    let Some(trace) = traces.get(nom) else { return false };
    let mut s = 0.0;
    loop {
        let (pos, _) = trace.au(s);
        if grille.pres_de(pos, 40.0).iter().any(|b| autorises.contains(&b.id)) {
            return true;
        }
        if s >= trace.longueur() {
            return false;
        }
        s += 40.0;
    }
}

/// Regroupe les familles depuis une vue de carte : centroïde t-SNE non
/// pondéré des morceaux de chaque grappe — la même agrégation que
/// `crates/cli/src/main.rs` refait pour `Quartiers`/`Rues`/`Adresses`.
pub fn familles_depuis_vue<'a>(vue: impl IntoIterator<Item = &'a MapPoint>) -> Vec<Famille> {
    let mut sommes: HashMap<i64, (f64, f64, usize)> = HashMap::new();
    for p in vue {
        let e = sommes.entry(p.cluster).or_insert((0.0, 0.0, 0));
        e.0 += p.x as f64;
        e.1 += p.y as f64;
        e.2 += 1;
    }
    sommes
        .into_iter()
        // Les morceaux sans famille (-1) ne peuplent pas de quartier.
        .filter(|(id, _)| *id >= 0)
        .map(|(id, (sx, sy, n))| Famille {
            id,
            centroide: [(sx / n as f64) as f32, (sy / n as f64) as f32],
            effectif: n,
        })
        .collect()
}

/// Regroupe les artistes depuis une vue de carte, par `album_artist` (repli
/// sur `artist`) — voir le commentaire sur ce choix dans
/// `crates/cli/src/main.rs`, `Cmd::Rues` (67 débordements avant ce choix, la
/// quasi-totalité des artistes en featuring hip-hop). Rend aussi, par nom
/// d'artiste, ses morceaux triés par (album, piste, identifiant) — la même
/// clé que `CleArrivee` du peuplement, pour qu'un album arrive en bloc.
pub fn artistes_depuis_vue<'a>(
    vue: impl IntoIterator<Item = &'a MapPoint>,
) -> (Vec<Artiste>, HashMap<String, Vec<i64>>) {
    let mut cumul: HashMap<&str, CumulArtiste> = HashMap::new();
    let mut pistes_par_artiste: HashMap<&str, Vec<(String, i64, i64)>> = HashMap::new();
    for p in vue {
        let nom = p.album_artist.as_deref().filter(|s| !s.is_empty()).or(p.artist.as_deref());
        let Some(nom) = nom else { continue };
        if nom.is_empty() {
            continue;
        }
        let e = cumul.entry(nom).or_insert_with(|| (0.0, 0.0, 0, HashMap::new()));
        e.0 += p.x as f64;
        e.1 += p.y as f64;
        e.2 += 1;
        *e.3.entry(p.cluster).or_insert(0) += 1;
        pistes_par_artiste.entry(nom).or_default().push((
            p.album.clone().unwrap_or_default(),
            p.track_no.unwrap_or(0),
            p.id,
        ));
    }

    let artistes: Vec<Artiste> = cumul
        .into_iter()
        .map(|(nom, (sx, sy, n, par_famille))| {
            let dominante = par_famille
                .into_iter()
                // À égalité, le plus petit identifiant : déterministe, comme
                // `source::Source::artistes`.
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                .map(|(f, _)| f)
                .unwrap_or(-1);
            Artiste {
                nom: nom.to_string(),
                famille: dominante,
                centroide: [(sx / n as f64) as f32, (sy / n as f64) as f32],
                effectif: n,
            }
        })
        .filter(|a| a.famille >= 0)
        .collect();

    let pistes: HashMap<String, Vec<i64>> = pistes_par_artiste
        .into_iter()
        .map(|(nom, mut triples)| {
            triples.sort();
            (nom.to_string(), triples.into_iter().map(|(_, _, id)| id).collect())
        })
        .collect();

    (artistes, pistes)
}

/// Ce que [`rassembler`] a produit, au-delà de la [`Source`] elle-même — les
/// mêmes chiffres que le CLI imprime pour `Cmd::Adresses`, utiles à
/// l'appelant pour un journal ou un rapport.
pub struct Resultat {
    pub source: Source,
    pub adresses_posees: usize,
    /// Morceaux sans bâtiment : plus de morceaux que de bâtiments habitables.
    pub morceaux_sans_adresse: usize,
    /// Artistes relogés sur un monument iconique (`crate::ancrage`, étage 0).
    pub artistes_ancres: usize,
    /// Bâtiments de la zone peuplée — les `n` habitables au plus faible coût
    /// de voirie depuis le cœur. Tous occupés à la fin (un morceau par
    /// bâtiment), sauf ceux qu'un morceau sans adresse laisse vides quand la
    /// bibliothèque dépasse le bâti disponible.
    pub batiments_peuples: usize,
    /// Petits quartiers et tronçons d'artère en lesquels la zone est découpée
    /// (`croissance::decouper`), et combien ont été habités.
    pub cellules: usize,
    pub cellules_habitees: usize,
}

/// Les étages 1-2 du **placement par familles** (familles → quartiers, artistes
/// → rues), tel que l'application le faisait avant la croissance chronologique.
/// Gardé pour les diagnostics du CLI (`carto quartiers`, `carto rues`) ; plus
/// utilisé par [`rassembler`].
pub struct Preparation {
    pub repere: Repere,
    pub traces: HashMap<String, affectation::Trace>,
    pub grille: GrilleBatiments,
    pub rues: Vec<affectation::Rue>,
    pub familles: Vec<Famille>,
    pub artistes: Vec<Artiste>,
    pub pistes_par_artiste: HashMap<String, Vec<i64>>,
    pub ancrages: crate::ancrage::Ancrages,
    pub batiments_ancres: HashSet<i64>,
    /// Rue synthétique de chaque artiste ancré (`nom artiste → nom OSM`).
    pub ancre_rue: HashMap<String, String>,
    pub centre_m: [f64; 2],
    pub echelle: f64,
    /// Bâtiments de la zone peuplée — les `n` habitables les plus proches du
    /// centre, moins ceux déjà pris par l'ancrage.
    pub autorises: HashSet<i64>,
    /// Boîte englobante du noyau, mètres locaux, marge 200 m.
    pub noyau_bornes_m: [f64; 4],
    pub rues_noyau: Vec<affectation::Rue>,
    pub seeds: HashMap<i64, [f64; 2]>,
    pub transformation: affectation::Transformation,
    pub quartiers: affectation::Quartiers,
    pub capacites: HashMap<String, usize>,
    pub voirie: affectation::Voirie,
}

/// La zone peuplée et ce qui la détermine : étage 0 (artistes ancrés aux
/// monuments), puis les `N` bâtiments habitables au plus faible coût de voirie
/// depuis le cœur historique. Partagée par le peuplement chronologique
/// ([`rassembler`]) et par le placement par familles que le CLI garde en
/// diagnostic ([`preparer`]).
pub struct Zone {
    pub repere: Repere,
    pub grille: GrilleBatiments,
    pub ancrages: crate::ancrage::Ancrages,
    pub batiments_ancres: HashSet<i64>,
    pub centre_m: [f64; 2],
    /// Bâtiments de la zone peuplée (hors ceux de l'ancrage).
    pub autorises: HashSet<i64>,
    /// Coût de voirie de chaque bâtiment de la zone — le « temps » de la ville
    /// (`crate::croissance::Parcelle::cout`). Repli sur la distance à vol
    /// d'oiseau quand l'extrait n'a pas de graphe routable.
    pub couts: HashMap<i64, f64>,
}

/// Étage 0 et zone peuplée.
///
/// La zone est l'ensemble des `N` bâtiments au plus faible **coût de
/// déplacement sur la voirie** depuis le centre (`crate::cout_voirie`), et non
/// les `N` plus proches à vol d'oiseau — sinon elle se lit comme un disque. Les
/// grandes avenues « rapprochent » les bâtiments : la frontière prend une
/// forme étoilée qui suit la ville (`docs/carto-ville.md`).
pub fn zone_peuplee(extrait: &Extrait, vue: &[MapPoint], centre: Option<[f64; 2]>) -> Zone {
    let repere = Repere::centre_de(extrait);
    let grille = GrilleBatiments::nouvelle(extrait, &repere);

    // --- Étage 0 : les artistes les plus populaires filent aux monuments. ---
    let mut batiments_ancres: HashSet<i64> = HashSet::new();
    let ancrages = crate::ancrage::ancrer(vue, extrait, &grille, &repere, &mut batiments_ancres);

    let centre_m = match centre {
        Some(ll) => repere.vers_m(ll),
        None => grille.centre_de_masse(),
    };
    let centre_ll = centre.unwrap_or_else(|| repere.depuis_m(centre_m));
    let n_libres = vue.len().saturating_sub(ancrages.adresses.len());
    let bat_centres: Vec<(i64, [f64; 2])> = grille.tous().iter().map(|b| (b.id, b.centre)).collect();
    let mut couts = crate::cout_voirie::couts_batiments(extrait, &repere, &bat_centres, centre_ll);
    couts.retain(|(id, c)| c.is_finite() && !batiments_ancres.contains(id));
    couts.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let retenus: Vec<(i64, f64)> = if couts.len() >= n_libres {
        couts.into_iter().take(n_libres).collect()
    } else {
        // Repli euclidien : extrait sans graphe routable exploitable.
        grille
            .n_plus_proches(centre_m, n_libres + batiments_ancres.len())
            .into_iter()
            .filter(|b| !batiments_ancres.contains(&b.id))
            .take(n_libres)
            .map(|b| (b.id, distance2(b.centre, centre_m).sqrt()))
            .collect()
    };
    let autorises: HashSet<i64> = retenus.iter().map(|(id, _)| *id).collect();
    let couts: HashMap<i64, f64> = retenus.into_iter().collect();
    Zone { repere, grille, ancrages, batiments_ancres, centre_m, autorises, couts }
}

/// Étages 0 à 2 du peuplement dense (`docs/carto-ville.md`) : ancrage aux
/// monuments, zone peuplée, Procruste centré sur l'île de la Cité, quartiers,
/// logement des artistes sur les rues du noyau.
pub fn preparer(
    extrait: &Extrait,
    vue: &[MapPoint],
    espacement: f64,
    centre: Option<[f64; 2]>,
) -> Preparation {
    let Zone { repere, grille, ancrages, batiments_ancres, centre_m, autorises, couts: _ } =
        zone_peuplee(extrait, vue, centre);
    let rues = affectation::rassembler_rues(extrait, &repere);
    let traces = affectation::traces_des_rues(extrait, &repere);
    let est_ancre =
        |p: &MapPoint| crate::ancrage::nom_artiste(p).is_some_and(|n| ancrages.est_ancre(n));

    // Familles et artistes agrégés **après** le retrait des artistes ancrés —
    // sinon leurs gros effectifs faussent centroïdes et cibles de capacité.
    let familles = familles_depuis_vue(vue.iter().filter(|&p| !est_ancre(p)));
    let (artistes, pistes_par_artiste) =
        artistes_depuis_vue(vue.iter().filter(|&p| !est_ancre(p)));

    let noyau: Vec<&crate::batiments::Batiment> =
        grille.tous().iter().filter(|b| autorises.contains(&b.id)).collect();
    let noyau_bornes_m = {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for bat in &noyau {
            b[0] = b[0].min(bat.centre[0]);
            b[1] = b[1].min(bat.centre[1]);
            b[2] = b[2].max(bat.centre[0]);
            b[3] = b[3].max(bat.centre[1]);
        }
        if b[0].is_finite() {
            [b[0] - 200.0, b[1] - 200.0, b[2] + 200.0, b[3] + 200.0]
        } else {
            bornes_locales(extrait, &rues, &repere)
        }
    };

    // Rue synthétique d'un artiste ancré : le tronçon réel le plus proche de
    // son monument. Retirée du bassin de l'étage 2 pour qu'un nom ne serve
    // pas deux artistes.
    let mut ancre_rue: HashMap<String, String> = HashMap::new();
    for (nom, ancre) in &ancrages.par_artiste {
        if let Some(r) = rues
            .iter()
            .min_by(|a, b| distance2(a.centre, ancre.point_m).total_cmp(&distance2(b.centre, ancre.point_m)))
        {
            ancre_rue.insert(nom.clone(), r.nom.clone());
        }
    }
    let rues_synthetiques: HashSet<&str> = ancre_rue.values().map(String::as_str).collect();

    // Rues retenues pour les étages 1-2 : celles qui bordent au moins un
    // bâtiment du noyau, moins les rues synthétiques.
    let rues_noyau: Vec<affectation::Rue> = rues
        .iter()
        .filter(|r| {
            !rues_synthetiques.contains(r.nom.as_str())
                && borde_le_noyau(&r.nom, &traces, &grille, &autorises)
        })
        .cloned()
        .collect();

    // Échelle du Procruste : p95 des distances au centre dans le noyau sur p95
    // des écarts t-SNE des morceaux à peupler — les deux mesurés sur les
    // points réellement placés, pas sur l'étalement des centroïdes de famille
    // (qui surdimensionnait l'échelle et rejetait les cibles hors du noyau).
    let bary = barycentre_familles(&familles);
    let p95_bati = p95(noyau.iter().map(|b| distance2(b.centre, centre_m).sqrt()));
    let p95_cible = p95(vue.iter().filter(|&p| !est_ancre(p)).map(|p| {
        ((p.x as f64 - bary[0]).powi(2) + (p.y as f64 - bary[1]).powi(2)).sqrt()
    }));
    // Nuage réduit à un point (cas dégénéré des tests) : l'échelle n'a plus de
    // sens, toutes les cibles tombent au centre de toute façon.
    let echelle = if p95_cible < 1e-6 { 1.0 } else { p95_bati / p95_cible };

    let (seeds, transformation) =
        affectation::semer_centre(&familles, &rues_noyau, centre_m, echelle);
    let quartiers = affectation::partitionner(&familles, &rues_noyau, &seeds);
    let capacites = affectation::capacites_par_rue(&traces, &grille, espacement);
    let voirie =
        affectation::loger_artistes(&artistes, &rues_noyau, &quartiers, &transformation, &capacites);

    Preparation {
        repere,
        traces,
        grille,
        rues,
        familles,
        artistes,
        pistes_par_artiste,
        ancrages,
        batiments_ancres,
        ancre_rue,
        centre_m,
        echelle,
        autorises,
        noyau_bornes_m,
        rues_noyau,
        seeds,
        transformation,
        quartiers,
        capacites,
        voirie,
    }
}

/// Assemble une [`Source`] à partir d'un plan de ville importé et d'une vue
/// de la bibliothèque déjà projetée (t-SNE + familles).
///
/// Enchaîne l'étage 0 (artistes ancrés aux monuments), la zone peuplée, puis
/// la **croissance chronologique** de `crate::croissance` : les morceaux
/// arrivent par date de sortie, l'île de la Cité d'abord, et s'installent
/// petit à petit (`docs/carto-ville.md`). Les positions, en mètres locaux, sont
/// converties en lon/lat par [`Repere::depuis_m`] ; le bâti, l'eau, les
/// espaces verts et la frontière de l'extrait sont copiés tels quels.
/// `Source::etablissements`/`bandes`/`routes`/`rivieres` restent vides : c'est
/// ce qui fait basculer `tuiles`/`style` sur le rendu réel
/// ([`Source::est_ville_reelle`]).
///
/// `noms_famille` vient de `Library::familles` (id → nom) — ce module ne lit
/// pas la base, il reçoit ce que l'appelant en a déjà tiré. Une famille absente
/// de la table retombe sur `famille {id}` plutôt que sur un nom vide.
pub fn rassembler(
    extrait: &Extrait,
    vue: &[MapPoint],
    noms_famille: &HashMap<i64, String>,
    centre: Option<[f64; 2]>,
) -> Resultat {
    rassembler_avec(extrait, vue, noms_famille, centre, &croissance::Parametres::default())
}

/// [`rassembler`] avec des réglages de croissance explicites — pour comparer
/// des variantes (`examples/croissance_apercu.rs`).
pub fn rassembler_avec(
    extrait: &Extrait,
    vue: &[MapPoint],
    noms_famille: &HashMap<i64, String>,
    centre: Option<[f64; 2]>,
    reglages: &croissance::Parametres,
) -> Resultat {
    let Zone { repere, grille, ancrages, batiments_ancres: _, centre_m: _, autorises, couts } =
        zone_peuplee(extrait, vue, centre);
    let par_id: HashMap<i64, &MapPoint> = vue.iter().map(|p| (p.id, p)).collect();
    let est_ancre =
        |p: &MapPoint| crate::ancrage::nom_artiste(p).is_some_and(|n| ancrages.est_ancre(n));

    // --- Les parcelles : chaque bâtiment de la zone, avec son coût de voirie
    // (le « temps » de la ville) et l'importance de la voie qui le borde. ----
    let batiments_zone: Vec<&crate::batiments::Batiment> =
        grille.tous().iter().filter(|b| autorises.contains(&b.id)).collect();
    let centres: Vec<[f64; 2]> = batiments_zone.iter().map(|b| b.centre).collect();
    let facades = crate::facades::facades(extrait, &repere, &centres, FACADE_PORTEE_M);
    let parcelles: Vec<Parcelle> = batiments_zone
        .iter()
        .zip(&facades)
        .map(|(b, f)| Parcelle {
            id: b.id,
            centre: b.centre,
            cout: couts.get(&b.id).copied().unwrap_or(0.0),
            artere: f.map_or(0.0, |f| f.artere),
        })
        .collect();

    // --- Les arrivants : tous les morceaux hors ceux des artistes ancrés. ---
    let arrivants: Vec<Arrivant> = vue
        .iter()
        .filter(|p| !est_ancre(p))
        .map(|p| Arrivant {
            id: p.id,
            annee: p.year.map(|a| a as i32),
            artiste: nom_regroupement(p).to_string(),
            album: p.album.clone().unwrap_or_default(),
            piste: p.track_no.unwrap_or(0),
            famille: p.cluster,
            xy: [p.x, p.y],
            popularite: p.popularite,
        })
        .collect();

    // --- La croissance : l'étage unique qui remplace familles → quartiers,
    // artistes → rues, morceaux → adresses (`docs/carto-ville.md`). ----------
    let croissance = croissance::peupler(&parcelles, &arrivants, reglages);

    // Bâtiment -> morceau qui l'habite — c'est ce qui permet à `tuiles`/
    // `style` de colorer le bâtiment entier plutôt que d'y poser un point
    // (`source::BatimentReel`, `carto-ville.md`).
    let mut occupation: HashMap<i64, i64> = HashMap::new();
    let mut positions_m: HashMap<i64, [f64; 2]> = HashMap::new();
    for a in &croissance.adresses {
        occupation.insert(parcelles[a.parcelle].id, a.id);
        positions_m.insert(a.id, parcelles[a.parcelle].centre);
    }
    let morceaux_sans_adresse = croissance.sans_adresse.len();

    // --- Fusion des adresses de l'étage 0. --------------------------------
    for a in &ancrages.adresses {
        occupation.insert(a.batiment_id, a.track_id);
        positions_m.insert(a.track_id, a.point_m);
    }
    let adresses_posees = positions_m.len();
    let positions: HashMap<i64, [f64; 2]> =
        positions_m.iter().map(|(&id, &m)| (id, repere.depuis_m(m))).collect();

    // --- Les morceaux, en lon/lat, prêts pour `Source`. ---------------------
    let morceaux: Vec<Morceau> = positions
        .iter()
        .filter_map(|(&id, lonlat)| {
            let p = par_id.get(&id)?;
            Some(Morceau {
                id,
                x: lonlat[0] as f32,
                y: lonlat[1] as f32,
                famille: p.cluster,
                titre: p.title.clone().unwrap_or_default(),
                artiste: p.artist.clone().unwrap_or_default(),
                annee: p.year.map(|a| a as i32),
                bpm: p.bpm,
                energie: p.energy,
            })
        })
        .collect();

    // --- Les bâtiments, avec leur occupant, prêts pour `Source`. ------------
    // Tout le bâti de l'extrait est rendu, occupé ou non (la texture d'un
    // vrai îlot parisien) ; seul l'occupant décide de la couleur.
    let batiments: Vec<crate::source::BatimentReel> = extrait
        .batis
        .iter()
        .map(|c| {
            let morceau_id = occupation.get(&c.id).copied();
            let occupant = morceau_id.and_then(|id| par_id.get(&id));
            let famille = occupant.map(|p| p.cluster);
            let annee = occupant.and_then(|p| p.year).map(|a| a as i32);
            crate::source::BatimentReel {
                points: c.points.clone(),
                morceau_id,
                famille,
                annee,
                bpm: occupant.and_then(|p| p.bpm),
                energie: occupant.and_then(|p| p.energy),
            }
        })
        .collect();

    // --- Les albums, prêts pour `Source`. ------------------------------------
    // Échelon de révélation entre l'artiste (une rue) et le morceau (un
    // bâtiment) — voir `source::AlbumReel`. Un album regroupe ses pistes déjà
    // logées, par (artiste, titre) ; son ancre est le morceau du groupe le
    // plus proche du barycentre, même idiome que `Source::ancres_de_familles`
    // (écrit ici directement, un seul appelant).
    let mut groupes_albums: GroupeAlbum = HashMap::new();
    for (&id, lonlat) in &positions {
        let Some(p) = par_id.get(&id) else { continue };
        let artiste = p.album_artist.as_deref().filter(|s| !s.is_empty()).or(p.artist.as_deref()).unwrap_or("");
        let album = p.album.as_deref().unwrap_or("");
        if artiste.is_empty() || album.is_empty() {
            continue;
        }
        groupes_albums.entry((artiste, album)).or_default().push((id, *lonlat));
    }
    let albums: Vec<crate::source::AlbumReel> = groupes_albums
        .into_iter()
        .map(|((artiste, album), membres)| {
            let n = membres.len() as f64;
            let (sx, sy) = membres.iter().fold((0.0, 0.0), |(sx, sy), (_, p)| (sx + p[0], sy + p[1]));
            let (bx, by) = (sx / n, sy / n);
            let ancre = membres
                .iter()
                .min_by(|a, b| {
                    let da = (a.1[0] - bx).powi(2) + (a.1[1] - by).powi(2);
                    let db = (b.1[0] - bx).powi(2) + (b.1[1] - by).powi(2);
                    da.total_cmp(&db)
                })
                .map(|(_, p)| *p)
                .unwrap_or([bx, by]);
            let famille = membres.first().and_then(|(id, _)| par_id.get(id)).map(|p| p.cluster).unwrap_or(-1);
            crate::source::AlbumReel {
                point: ancre,
                nom: album.to_string(),
                artiste: artiste.to_string(),
                famille,
                effectif: membres.len(),
            }
        })
        .collect();

    // --- Les familles nommées, prêtes pour `Source`. ------------------------
    let familles = familles_depuis_vue(vue.iter());
    let familles_source = familles
        .iter()
        .map(|f| crate::source::Famille {
            id: f.id,
            nom: noms_famille.get(&f.id).cloned().unwrap_or_else(|| format!("famille {}", f.id)),
            effectif: f.effectif,
        })
        .collect();

    // --- Métadonnées des artistes ancrés (étage 0). ------------------------
    // Famille dominante et effectif, lus dans la vue complète (les ancrés ne
    // sont pas des arrivants, mais leur rue synthétique et leur pastille en
    // ont besoin).
    let mut famille_ancre: HashMap<&str, HashMap<i64, usize>> = HashMap::new();
    let mut effectif_ancre: HashMap<&str, usize> = HashMap::new();
    for p in vue {
        let Some(nom) = crate::ancrage::nom_artiste(p) else { continue };
        if !ancrages.est_ancre(nom) {
            continue;
        }
        *effectif_ancre.entry(nom).or_default() += 1;
        *famille_ancre.entry(nom).or_default().entry(p.cluster).or_default() += 1;
    }
    let famille_dominante_ancre: HashMap<&str, i64> = famille_ancre
        .iter()
        .map(|(nom, parts)| {
            let f = parts
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(f, _)| *f)
                .unwrap_or(-1);
            (*nom, f)
        })
        .collect();

    // --- Les rues d'après leurs habitants. -----------------------------------
    // Une rue (un nom OSM) prend le nom de l'artiste le plus présent parmi les
    // bâtiments qui donnent sur elle, et la couleur de leur famille dominante.
    // Une rue sans habitant reste « Rue » — mieux qu'un nom vide, et
    // honnête : personne n'y habite.
    let mut par_rue: HashMap<&str, (HashMap<&str, usize>, HashMap<i64, usize>)> = HashMap::new();
    for a in &croissance.adresses {
        let Some(f) = facades[a.parcelle] else { continue };
        let Some(nom_rue) = extrait.troncons[f.troncon].nom.as_deref() else { continue };
        let Some(p) = par_id.get(&a.id) else { continue };
        let e = par_rue.entry(nom_rue).or_default();
        *e.0.entry(nom_regroupement(p)).or_default() += 1;
        *e.1.entry(p.cluster).or_default() += 1;
    }
    fn dominant<K: Copy + Ord + std::hash::Hash>(m: &HashMap<K, usize>) -> Option<K> {
        m.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(k, _)| *k)
    }
    let mut artiste_de_la_rue: HashMap<&str, &str> =
        par_rue.iter().filter_map(|(r, (art, _))| dominant(art).map(|a| (*r, a))).collect();
    let mut famille_de_la_rue: HashMap<&str, i64> =
        par_rue.iter().filter_map(|(r, (_, fam))| dominant(fam).map(|f| (*r, f))).collect();
    // Les rues synthétiques des artistes ancrés : le tronçon nommé le plus
    // proche de leur monument porte leur nom, et la famille qu'ils ont quittée.
    for (nom, ancre) in &ancrages.par_artiste {
        let proche = crate::facades::facades(extrait, &repere, &[ancre.point_m], 150.0);
        let Some(Some(f)) = proche.first() else { continue };
        let Some(nom_rue) = extrait.troncons[f.troncon].nom.as_deref() else { continue };
        artiste_de_la_rue.insert(nom_rue, nom.as_str());
        famille_de_la_rue
            .insert(nom_rue, famille_dominante_ancre.get(nom.as_str()).copied().unwrap_or(-1));
    }
    // Une rue du halo (petite couronne, hors commune) peut porter le même nom
    // qu'une rue de Paris : sans ce garde-fou elle hériterait de la famille et
    // de l'artiste de son homonyme parisien. On ne colore que ce qui est
    // vraiment dans la commune (même règle que `rassembler_rues`).
    let dans_commune = |t: &Troncon| {
        extrait.frontiere.as_ref().is_none_or(|f| {
            t.points.iter().filter(|p| f.contient(**p)).count() * 2 > t.points.len()
        })
    };
    let troncons_reels: Vec<TronconReel> = extrait
        .troncons
        .iter()
        .map(|t| {
            let (famille, artiste) = if dans_commune(t) {
                let nom = t.nom.as_deref();
                (
                    nom.and_then(|n| famille_de_la_rue.get(n)).copied(),
                    nom.and_then(|n| artiste_de_la_rue.get(n)).copied(),
                )
            } else {
                (None, None)
            };
            let nom_affiche = match artiste {
                Some(a) => nom_affiche(t.classe, a),
                None => type_de_voie(t.classe).to_string(),
            };
            TronconReel {
                points: t.points.clone(),
                classe: t.classe,
                nom: nom_affiche,
                nom_osm: t.nom.clone(),
                famille,
                artiste: artiste.map(str::to_string),
            }
        })
        .collect();

    // --- La première année de sortie de chaque artiste, pour le curseur
    // temporel du plan de ville réel (`docs/carto-ville.md`). Les artistes
    // ancrés ont leurs morceaux dans `ancrages.adresses`, les autres dans
    // `croissance.adresses`.
    let mut annee_artiste: HashMap<&str, i32> = HashMap::new();
    for p in vue {
        let (Some(y), nom) = (p.year, nom_regroupement(p)) else { continue };
        annee_artiste.entry(nom).and_modify(|m| *m = (*m).min(y as i32)).or_insert(y as i32);
    }

    // --- Les artistes, posés sur leur quartier, prêts pour `Source`. --------
    // Pas au barycentre de leurs morceaux logés : il tombe dans un vide entre
    // deux amas, au milieu d'une chaussée. On prend le **bâtiment de l'artiste
    // le plus proche de ce barycentre** — un point réellement habité.
    let mut par_artiste: HashMap<&str, Vec<([f64; 2], i64)>> = HashMap::new();
    for a in &croissance.adresses {
        let Some(p) = par_id.get(&a.id) else { continue };
        par_artiste.entry(nom_regroupement(p)).or_default().push((parcelles[a.parcelle].centre, p.cluster));
    }
    let mut artistes_places: Vec<crate::source::Artiste> = par_artiste
        .iter()
        .map(|(nom, habitats)| {
            let n = habitats.len() as f64;
            let b = habitats.iter().fold([0.0, 0.0], |s, (c, _)| [s[0] + c[0] / n, s[1] + c[1] / n]);
            let medoide = habitats
                .iter()
                .min_by(|x, y| distance2(x.0, b).total_cmp(&distance2(y.0, b)))
                .map(|(c, _)| *c)
                .unwrap_or(b);
            let mut parts: HashMap<i64, usize> = HashMap::new();
            for (_, f) in habitats {
                *parts.entry(*f).or_default() += 1;
            }
            let lonlat = repere.depuis_m(medoide);
            crate::source::Artiste {
                nom: nom.to_string(),
                x: lonlat[0] as f32,
                y: lonlat[1] as f32,
                famille: dominant(&parts).unwrap_or(-1),
                effectif: habitats.len(),
                ancre: None,
                annee: annee_artiste.get(nom).copied(),
            }
        })
        .collect();
    // Les artistes ancrés, posés sur leur monument.
    for (nom, ancre) in &ancrages.par_artiste {
        let lonlat = repere.depuis_m(ancre.point_m);
        artistes_places.push(crate::source::Artiste {
            nom: nom.clone(),
            x: lonlat[0] as f32,
            y: lonlat[1] as f32,
            famille: famille_dominante_ancre.get(nom.as_str()).copied().unwrap_or(-1),
            effectif: effectif_ancre.get(nom.as_str()).copied().unwrap_or(0),
            ancre: Some(ancre.monument.clone()),
            annee: annee_artiste.get(nom.as_str()).copied(),
        });
    }
    // `tuiles::rang_artiste` prend l'indice pour le rang : trier par effectif
    // décroissant, nom en départage.
    artistes_places.sort_by(|a, b| b.effectif.cmp(&a.effectif).then_with(|| a.nom.cmp(&b.nom)));

    // --- Les quartiers musicaux comme aplats, prêts pour `Source`. ----------
    // Contourés d'après les habitants réels (et non plus d'un diagramme de
    // puissance posé d'en haut) : ce qui donne à la carte une information de
    // genre visible en dézoomant, quand les bâtiments ne sont pas encore
    // révélés — et qui épouse la forme de la ville qui a poussé.
    let bornes_m = {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for p in &parcelles {
            b[0] = b[0].min(p.centre[0]);
            b[1] = b[1].min(p.centre[1]);
            b[2] = b[2].max(p.centre[0]);
            b[3] = b[3].max(p.centre[1]);
        }
        if b[0].is_finite() {
            [b[0] - 200.0, b[1] - 200.0, b[2] + 200.0, b[3] + 200.0]
        } else {
            [-1000.0, -1000.0, 1000.0, 1000.0]
        }
    };
    let habitants: Vec<([f64; 2], i64)> = positions_m
        .iter()
        .filter_map(|(id, m)| par_id.get(id).map(|p| (*m, p.cluster)))
        .collect();
    let dans_commune_m = |p_m: [f64; 2]| match &extrait.frontiere {
        Some(f) => f.contient(repere.depuis_m(p_m)),
        None => true,
    };
    let territoires_reels: Vec<crate::source::TerritoireReel> =
        territoires_des_habitants(&habitants, bornes_m, 360, dans_commune_m)
            .into_iter()
            .map(|t| crate::source::TerritoireReel {
                famille: t.famille,
                polygones: t
                    .polygones
                    .into_iter()
                    .map(|poly| {
                        poly.into_iter()
                            .map(|anneau| anneau.into_iter().map(|p| repere.depuis_m(p)).collect())
                            .collect()
                    })
                    .collect(),
            })
            .collect();

    let source = Source {
        morceaux,
        familles: familles_source,
        troncons_reels,
        batiments,
        artistes_places,
        territoires_reels,
        eaux: extrait.eaux.iter().map(|c| ContourReel { points: c.points.clone() }).collect(),
        verts: extrait.verts.iter().map(|c| ContourReel { points: c.points.clone() }).collect(),
        frontiere: extrait.frontiere.as_ref().map(|f| f.anneaux.clone()),
        points_remarquables: extrait
            .points_remarquables
            .iter()
            .map(|p| {
                let point_m = repere.vers_m(p.point);
                let artiste = ancrages
                    .par_artiste
                    .iter()
                    .find(|(_, a)| distance2(a.point_m, point_m) < 25.0)
                    .map(|(nom, _)| nom.clone());
                let annee = artiste.as_deref().and_then(|nom| annee_artiste.get(nom).copied());
                crate::source::PointReel {
                    point: p.point,
                    nom: p.nom.clone(),
                    genre: p.genre.clone(),
                    artiste,
                    annee,
                }
            })
            .collect(),
        albums,
        ..Default::default()
    };

    Resultat {
        source,
        adresses_posees,
        morceaux_sans_adresse,
        artistes_ancres: ancrages.par_artiste.len(),
        batiments_peuples: autorises.len(),
        cellules: croissance.cellules.len(),
        cellules_habitees: croissance.fondation.iter().flatten().count(),
    }
}

/// Rayon de recherche de la voie qui borde un bâtiment, mètres : au-delà, le
/// bâtiment n'est pas « sur » une voie (cour intérieure, parc) et ne compte pas
/// comme bordant une artère.
const FACADE_PORTEE_M: f64 = 60.0;

/// Le nom sous lequel on regroupe les morceaux d'un même artiste : l'artiste
/// d'album, à défaut l'artiste du morceau. Voir `artistes_depuis_vue` pour la
/// raison du choix (featurings).
fn nom_regroupement(p: &MapPoint) -> &str {
    p.album_artist
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(p.artist.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("(artiste inconnu)")
}

/// Rayon d'influence d'un habitant sur l'aplat de son quartier, mètres : un
/// point de la carte prend la famille qui domine dans ce rayon.
const RAYON_TERRITOIRE_M: f64 = 130.0;

/// Les aplats de quartier d'après les habitants réels : sur une grille, chaque
/// point prend la **famille majoritaire** parmi les habitants à moins de
/// [`RAYON_TERRITOIRE_M`] (pondérés par la proximité), et reste vide s'il n'y
/// en a aucun. `dedans` limite à la commune. Le vote adoucit le confetti d'un
/// bâtiment à l'autre sans inventer de frontière : l'aplat ne dit que ce que
/// les habitants disent.
fn territoires_des_habitants(
    habitants: &[([f64; 2], i64)],
    bornes: [f64; 4],
    resolution: usize,
    dedans: impl Fn([f64; 2]) -> bool,
) -> Vec<affectation::Territoire> {
    let gn = resolution.max(2);
    let [xmin, ymin, xmax, ymax] = bornes;
    let pas_x = ((xmax - xmin) / gn as f64).max(1e-6);
    let pas_y = ((ymax - ymin) / gn as f64).max(1e-6);
    let mut ids: Vec<i64> = habitants.iter().map(|h| h.1).collect::<HashSet<_>>().into_iter().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        return Vec::new();
    }
    let rang: HashMap<i64, usize> = ids.iter().enumerate().map(|(i, &f)| (f, i)).collect();

    let pas = RAYON_TERRITOIRE_M;
    let cle = |p: [f64; 2]| ((p[0] / pas).floor() as i32, (p[1] / pas).floor() as i32);
    let mut grille: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, h) in habitants.iter().enumerate() {
        grille.entry(cle(h.0)).or_default().push(i);
    }

    let mut gagnante = vec![i64::MIN; gn * gn];
    let mut voix = vec![0.0f64; ids.len()];
    for gy in 0..gn {
        let cy = ymin + (gy as f64 + 0.5) * pas_y;
        for gx in 0..gn {
            let cx = xmin + (gx as f64 + 0.5) * pas_x;
            let p = [cx, cy];
            if !dedans(p) {
                continue;
            }
            voix.iter_mut().for_each(|v| *v = 0.0);
            let (kx, ky) = cle(p);
            let mut vus = 0usize;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(v) = grille.get(&(kx + dx, ky + dy)) else { continue };
                    for &i in v {
                        let d = distance2(habitants[i].0, p).sqrt();
                        if d <= pas {
                            voix[rang[&habitants[i].1]] += 1.0 - d / pas;
                            vus += 1;
                        }
                    }
                }
            }
            if vus == 0 {
                continue;
            }
            let (meilleur, _) = voix
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1).then_with(|| b.0.cmp(&a.0)))
                .unwrap_or((0, &0.0));
            gagnante[gy * gn + gx] = ids[meilleur];
        }
    }
    affectation::contourer(&gagnante, &ids, bornes, gn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_music_osm::{Classe, Troncon};

    fn point(id: i64, x: f32, y: f32, cluster: i64, artist: &str, album: &str, piste: i64) -> MapPoint {
        MapPoint {
            id,
            path: format!("piste-{id}.flac"),
            x,
            y,
            cluster,
            title: Some(format!("titre {id}")),
            artist: Some(artist.to_string()),
            album_artist: Some(artist.to_string()),
            album: Some(album.to_string()),
            track_no: Some(piste),
            year: Some(2000),
            duration_ms: None,
            bpm: None,
            energy: None,
            popularite: None,
        }
    }

    /// Un bâtiment carré, construit en mètres locaux puis reconverti en
    /// lon/lat — même helper que `affectation::tests::contour_carre`.
    fn contour_carre(repere: &Repere, id: i64, centre_m: [f64; 2], cote: f64) -> rusty_music_osm::Contour {
        let r = cote / 2.0;
        let anneau_m = [
            [centre_m[0] - r, centre_m[1] - r],
            [centre_m[0] + r, centre_m[1] - r],
            [centre_m[0] + r, centre_m[1] + r],
            [centre_m[0] - r, centre_m[1] + r],
            [centre_m[0] - r, centre_m[1] - r],
        ];
        rusty_music_osm::Contour { id, points: anneau_m.iter().map(|p| repere.depuis_m(*p)).collect() }
    }

    fn extrait_dessai() -> Extrait {
        let troncons = (0..6)
            .map(|i| Troncon {
                id: i,
                nom: Some(format!("Rue {i}")),
                classe: Classe::Residentielle,
                points: vec![[2.30 + i as f64 * 0.01, 48.85], [2.30 + i as f64 * 0.01 + 0.005, 48.85]],
            })
            .collect();
        let mut extrait = Extrait { troncons, ..Default::default() };

        // Trois bâtiments par rue — largement assez pour douze morceaux sur
        // six rues. Sans bâtiment, un morceau n'a plus nulle part où habiter
        // depuis que l'étage 3 loge dans de vrais bâtiments, pas en bordure
        // de rue.
        let repere = Repere::centre_de(&extrait);
        let traces = affectation::traces_des_rues(&extrait, &repere);
        let mut id = 1000;
        for nom in traces.keys() {
            let trace = &traces[nom];
            for s in [0.0, trace.longueur() / 2.0, trace.longueur()] {
                let (pos, _) = trace.au(s);
                extrait.batis.push(contour_carre(&repere, id, [pos[0], pos[1] + 5.0], 10.0));
                id += 1;
            }
        }
        extrait
    }

    #[test]
    fn rassembler_pose_chaque_morceau_a_une_adresse_reelle() {
        let extrait = extrait_dessai();
        let vue: Vec<MapPoint> = (0..12)
            .map(|i| point(i, 0.0, 0.0, i % 2, &format!("Artiste {}", i % 2), "Album", i))
            .collect();

        let r = rassembler(&extrait, &vue, &HashMap::new(), None);

        assert_eq!(r.morceaux_sans_adresse, 0, "tout doit tenir sur six rues pour douze morceaux");
        assert_eq!(r.adresses_posees, 12);
        assert_eq!(r.source.morceaux.len(), 12);
        // Les positions doivent être des coordonnées géographiques plausibles
        // — proches de l'extrait, pas de l'espace t-SNE `[-1, 1]` d'origine.
        for m in &r.source.morceaux {
            assert!((2.0..2.6).contains(&m.x), "x hors de la fourchette parisienne : {}", m.x);
            assert!((48.7..49.0).contains(&m.y), "y hors de la fourchette parisienne : {}", m.y);
        }
        assert!(r.source.est_ville_reelle());
        assert!(!r.source.troncons_reels.is_empty());
        // Chaque rue logée porte un nom affiché non vide.
        assert!(r.source.troncons_reels.iter().all(|t| !t.nom.is_empty()));
    }

    fn point_annee(id: i64, cluster: i64, artist: &str, annee: Option<i64>) -> MapPoint {
        MapPoint { year: annee, ..point(id, 0.0, 0.0, cluster, artist, "Album", id) }
    }

    /// L'invariant que `docs/carto-ville.md` réclame pour le curseur
    /// temporel : à toute année choisie, le nombre de bâtiments montrés comme
    /// occupés doit être exactement égal au nombre de morceaux logés dont
    /// l'année est ≤ à celle-là — un morceau sans année compte comme toujours
    /// occupé des deux côtés (décision du 5 sept. 2026, cohérente avec le
    /// bucket incertain du streamgraph). Vrai par construction — l'année du
    /// bâtiment est recopiée de celle de son occupant — mais c'est
    /// exactement le genre de recopie qui se troue en silence.
    #[test]
    fn le_batiment_habite_porte_lannee_de_son_occupant() {
        let extrait = extrait_dessai();
        let annees = [
            Some(1990), Some(2005), None, Some(1998), Some(2010), Some(1990),
            Some(2020), None, Some(2001), Some(1995), Some(2015), Some(1990),
        ];
        let vue: Vec<MapPoint> = annees
            .iter()
            .enumerate()
            .map(|(i, &a)| point_annee(i as i64, (i % 2) as i64, &format!("Artiste {}", i % 2), a))
            .collect();

        let r = rassembler(&extrait, &vue, &HashMap::new(), None);
        assert_eq!(r.morceaux_sans_adresse, 0, "l'invariant suppose tout le monde logé");

        for seuil in [1989, 1995, 2000, 2010, 2020, 2021] {
            let occupes = r
                .source
                .batiments
                .iter()
                .filter(|b| b.morceau_id.is_some())
                .filter(|b| b.annee.is_none_or(|a| a <= seuil))
                .count();
            let attendus = r.source.morceaux.iter().filter(|m| m.annee.is_none_or(|a| a <= seuil)).count();
            assert_eq!(occupes, attendus, "seuil {seuil}");
        }
    }

    #[test]
    fn le_type_de_voie_suit_la_hierarchie_osm() {
        assert_eq!(type_de_voie(Classe::Autoroute), "Boulevard Périphérique");
        assert_eq!(type_de_voie(Classe::Service), "Impasse");
    }

    #[test]
    fn le_noyau_se_remplit_sans_trou_et_laisse_la_peripherie_vide() {
        // 18 bâtiments (six rues × trois), 12 morceaux : la zone peuplée est
        // les 12 bâtiments les plus proches du centre, tous occupés, les 6
        // autres vacants.
        let extrait = extrait_dessai();
        let vue: Vec<MapPoint> = (0..12)
            .map(|i| point(i, (i % 3) as f32 * 0.1, 0.0, i % 2, &format!("Artiste {}", i % 2), "Album", i))
            .collect();

        let r = rassembler(&extrait, &vue, &HashMap::new(), None);

        assert_eq!(r.batiments_peuples, 12, "N = nombre de morceaux");
        assert_eq!(r.morceaux_sans_adresse, 0);
        let occupes = r.source.batiments.iter().filter(|b| b.morceau_id.is_some()).count();
        assert_eq!(occupes, 12, "chaque morceau dans un bâtiment, un par bâtiment");
        assert_eq!(
            r.source.batiments.len() - occupes,
            6,
            "la périphérie (6 bâtiments) reste vacante, pas de trou au milieu"
        );
    }

    #[test]
    fn un_artiste_populaire_est_ancre_sur_un_monument() {
        let mut extrait = extrait_dessai();
        extrait.points_remarquables = vec![rusty_music_osm::PointRemarquable {
            id: 1,
            nom: "Tour Eiffel".into(),
            genre: "monument".into(),
            // Sur la première rue de l'extrait.
            point: [2.30, 48.85],
        }];

        let mut vue: Vec<MapPoint> =
            (0..10).map(|i| point(i, 0.0, 0.0, 0, "Foule", "Commun", i)).collect();
        // Un artiste nettement plus populaire que les autres (aucune pop).
        vue.push(point(100, 0.0, 0.0, 0, "Vedette", "Tube", 1));
        vue.push(point(101, 0.0, 0.0, 0, "Vedette", "Tube", 2));
        for m in vue.iter_mut().filter(|m| m.album_artist.as_deref() == Some("Vedette")) {
            m.popularite = Some(0.97);
        }

        let r = rassembler(&extrait, &vue, &HashMap::new(), None);
        assert_eq!(r.artistes_ancres, 1);
        let vedette = r
            .source
            .artistes_places
            .iter()
            .find(|a| a.nom == "Vedette")
            .expect("la vedette doit être placée");
        assert_eq!(vedette.ancre.as_deref(), Some("Tour Eiffel"));
        // Le monument porte le nom de l'artiste ancré.
        assert!(r
            .source
            .points_remarquables
            .iter()
            .any(|p| p.artiste.as_deref() == Some("Vedette")));
    }

    #[test]
    fn les_territoires_suivent_les_habitants() {
        // Deux îlots de 5 × 5 habitants à 1 km l'un de l'autre, deux familles.
        let mut habitants = Vec::new();
        for i in 0..5 {
            for j in 0..5 {
                habitants.push(([i as f64 * 20.0, j as f64 * 20.0], 1));
                habitants.push(([1000.0 + i as f64 * 20.0, j as f64 * 20.0], 2));
            }
        }
        let t = territoires_des_habitants(&habitants, [-300.0, -300.0, 1300.0, 400.0], 160, |_| true);
        let familles: HashSet<i64> = t.iter().map(|t| t.famille).collect();
        assert_eq!(familles, HashSet::from([1, 2]));
        // Rien entre les deux îlots : aucun habitant à moins de 130 m.
        let aplat_au_milieu = t.iter().any(|t| {
            t.polygones.iter().any(|p| p[0].iter().any(|q| (400.0..600.0).contains(&q[0])))
        });
        assert!(!aplat_au_milieu, "le vide entre deux quartiers reste vide");
    }

    /// La coloration par tempo et énergie sur le plan réel : le bâtiment porte
    /// le tempo et l'énergie de son occupant (comme son année), et reste sans
    /// valeur quand le morceau n'en a pas.
    #[test]
    fn le_batiment_habite_porte_le_tempo_et_l_energie_de_son_occupant() {
        let extrait = extrait_dessai();
        let vue: Vec<MapPoint> = (0..12)
            .map(|i| MapPoint {
                bpm: (i % 2 == 0).then_some(100.0 + i as f32),
                energy: (i % 3 == 0).then_some(0.5),
                ..point(i, 0.0, 0.0, i % 2, &format!("Artiste {}", i % 2), "Album", i)
            })
            .collect();
        let r = rassembler(&extrait, &vue, &HashMap::new(), None);
        let par_id: HashMap<i64, &MapPoint> = vue.iter().map(|p| (p.id, p)).collect();
        let mut vus = 0;
        for b in r.source.batiments.iter().filter(|b| b.morceau_id.is_some()) {
            let p = par_id[&b.morceau_id.unwrap()];
            assert_eq!(b.bpm, p.bpm);
            assert_eq!(b.energie, p.energy);
            vus += 1;
        }
        assert_eq!(vus, 12);
        assert!(r.source.batiments.iter().filter(|b| b.morceau_id.is_none()).all(|b| b.bpm.is_none() && b.energie.is_none()));
    }
}
