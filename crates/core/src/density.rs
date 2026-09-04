// SPDX-License-Identifier: GPL-3.0-or-later
//! Nappe de densité de la carte (module 2) : une estimation à noyau gaussien
//! par famille, plus une globale, réduites en bandes de niveau (isobandes).
//!
//! Pur calcul, sans base de données — `calculer` prend les positions déjà
//! lues (le même quadruplet que [`crate::db::Library::map_points`]) et rend
//! des polygones prêts à sérialiser vers l'interface. L'appelant (module 2,
//! après projection + clustering) décide quand le rejouer et où garder le
//! résultat en cache : rien ici ne suppose une session Tauri.

use std::collections::HashMap;

use contour::ContourBuilder;

/// Le domaine de la carte déborde un peu [-1, 1] — même marge que
/// l'interface (`DENSITE_MARGE` côté `app.js`) — pour que les bandes du bord
/// ne soient pas coupées à vif.
pub const MARGE: f64 = 0.08;

/// Au-delà de ce nombre de familles, les plus petites rejoignent le
/// territoire [`AUTRES`] plutôt que de garder chacune sa propre teinte —
/// quatorze teintes simultanées ne se distinguent plus, et surtout pas en
/// deutéranopie. Sept restent séparables (palette `--territoires` côté
/// interface, dérivée d'Okabe-Ito) ; la huitième place va au territoire
/// « autres », rendu en gris neutre plutôt que coloré.
const FAMILLES_MAX: usize = 7;

/// Le territoire fourre-tout : familles au-delà de [`FAMILLES_MAX`] (les
/// plus petites) et points sans cluster. Négatif et distinct de `-1`, le
/// sentinel déjà utilisé ailleurs (`COALESCE(cluster, -1)`) pour « pas de
/// cluster » — assez petit pour tenir sans perte de précision une fois
/// sérialisé en JSON (voir `apps/desktop/ui/app.js`, qui compare sur cette
/// même valeur).
pub const AUTRES: i64 = -2;

/// Paramètres du calcul de densité : largeur de noyau, résolution de grille,
/// nombre de bandes. Un seul jeu pour toute la carte — chaque famille (et la
/// nappe globale) partage la même grille, seule sa densité y varie.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ParametresDensite {
    /// Écart-type du noyau gaussien, en unités de carte (le domaine fait
    /// environ 2 de large). Plus haut : des nappes plus lisses et plus
    /// fondues entre elles ; plus bas : plus fidèle au nuage, plus de bruit.
    pub noyau: f64,
    /// Cellules par côté de la grille de calcul.
    pub resolution: usize,
    /// Nombre de bandes par nappe (autant de seuils moins un).
    pub bandes: usize,
}

impl Default for ParametresDensite {
    /// Grille fine et noyau étroit — 1 % de l'étendue des données, le
    /// plancher demandé plutôt qu'un compromis au milieu de la fourchette.
    /// Vérifié sur des données synthétiques à plusieurs sous-amas par
    /// famille (une seule gaussienne isotrope reste lisse en son cœur quel
    /// que soit le noyau — pas un défaut du calcul, la forme même d'une
    /// gaussienne) : à 1,5 %, les territoires restent nettement plus
    /// arrondis qu'à 1 % ; en dessous de 1 %, hors de la fourchette
    /// demandée, le bord se met à trembler plus qu'à onduler. La
    /// contrepartie en temps de calcul est mesurée (pas supposée) dans le
    /// journal de développement, `docs/journal.md`.
    fn default() -> Self {
        Self {
            noyau: 0.02,
            resolution: 1024,
            bandes: 7,
        }
    }
}

/// Une bande de densité — un anneau extérieur et ses trous éventuels, par
/// polygone (une famille dissociée en deux amas rend deux polygones dans la
/// même bande).
///
/// `famille` vaut `None` pour la nappe globale (toutes familles confondues).
/// `palier` va de 0 (le plus bas, le plus large) à `bandes - 1` (le sommet).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Bande {
    pub famille: Option<i64>,
    pub palier: usize,
    /// Anneaux de coordonnées, dans le repère de la carte : premier anneau
    /// = contour extérieur, suivants = trous — comme un polygone GeoJSON.
    pub polygones: Vec<Vec<Vec<[f64; 2]>>>,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct ResultatDensite {
    pub bandes: Vec<Bande>,
}

/// Le champ de densité global, **avant** réduction en bandes.
///
/// [`calculer`] rend des paliers ; le relief, lui, a besoin de l'altitude
/// continue — un ombrage calculé sur des marches d'escalier ferait apparaître
/// les paliers comme des falaises. La grille est carrée, de côté
/// `parametres.resolution`, et couvre le domaine `[-1-MARGE, 1+MARGE]²`,
/// exactement celui des bandes.
///
/// Les valeurs sont normalisées sur le maximum : `1.0` au sommet, `0.0` là où
/// aucun morceau ne pèse.
pub fn champ_global(points: &[(i64, f32, f32, i64)], parametres: &ParametresDensite) -> Vec<f64> {
    let gn = parametres.resolution.max(2);
    let lo = -1.0 - MARGE;
    let pas = (2.0 + 2.0 * MARGE) / gn as f64;
    let vers_grille = |v: f32| -> usize {
        let t = ((v as f64 - lo) / pas).floor();
        t.clamp(0.0, (gn - 1) as f64) as usize
    };

    let mut champ = vec![0.0f64; gn * gn];
    for &(_, x, y, _) in points {
        champ[vers_grille(y) * gn + vers_grille(x)] += 1.0;
    }
    flouter_gaussien(&mut champ, gn, (parametres.noyau / pas).max(0.5));

    let max = champ.iter().cloned().fold(0.0, f64::max);
    if max > 0.0 {
        for v in &mut champ {
            *v /= max;
        }
    }
    champ
}

/// Lit le champ de densité en coordonnées de **carte**, par interpolation
/// bilinéaire.
///
/// [`champ_global`] rend une grille ; savoir ce qu'elle vaut sous un point
/// demande de refaire la même conversion, et deux copies de cette conversion
/// finiraient par diverger d'une demi-cellule.
pub fn echantillonner(champ: &[f64], gn: usize, x: f32, y: f32) -> f64 {
    if gn < 2 || champ.len() != gn * gn {
        return 0.0;
    }
    let lo = -1.0 - MARGE;
    let pas = (2.0 + 2.0 * MARGE) / gn as f64;
    // Le `-0,5` place l'échantillon au centre de la cellule, pas à son coin.
    let gx = ((x as f64 - lo) / pas - 0.5).clamp(0.0, gn as f64 - 1.0);
    let gy = ((y as f64 - lo) / pas - 0.5).clamp(0.0, gn as f64 - 1.0);
    let (x0, y0) = (gx.floor() as usize, gy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(gn - 1), (y0 + 1).min(gn - 1));
    let (fx, fy) = (gx - x0 as f64, gy - y0 as f64);
    let v = |x: usize, y: usize| champ[y * gn + x];
    let bas = v(x0, y0) * (1.0 - fx) + v(x1, y0) * fx;
    let haut = v(x0, y1) * (1.0 - fx) + v(x1, y1) * fx;
    bas * (1.0 - fy) + haut * fy
}

/// Calcule la nappe de densité : une grille par famille, plus une globale,
/// réduites en bandes de niveau.
///
/// Le recouvrement entre familles se règle **au niveau du champ**, pas du
/// dessin : chaque grille de famille est d'abord normalisée sur son propre
/// maximum, puis, cellule par cellule, seule la famille dont la densité
/// normalisée est la plus forte garde sa valeur — les autres sont mises à
/// zéro. Les bandes qui en sortent ne se chevauchent donc plus par
/// construction, sans mélange de teintes à l'affichage.
///
/// `points` porte `(identifiant, x, y, famille)` — le format de
/// [`crate::db::Library::map_points`]. Au-delà de [`FAMILLES_MAX`] familles,
/// les plus petites (et les points sans cluster, famille négative) rejoignent
/// le territoire [`AUTRES`] plutôt que de garder chacune sa propre nappe.
pub fn calculer(
    points: &[(i64, f32, f32, i64)],
    parametres: &ParametresDensite,
) -> ResultatDensite {
    let gn = parametres.resolution.max(2);
    let lo = -1.0 - MARGE;
    let pas = (2.0 + 2.0 * MARGE) / gn as f64;

    let vers_grille = |v: f32| -> usize {
        let t = ((v as f64 - lo) / pas).floor();
        t.clamp(0.0, (gn - 1) as f64) as usize
    };

    // Les `FAMILLES_MAX` plus grosses familles gardent leur identité ; tout
    // le reste — familles plus petites et points sans cluster — rejoint le
    // territoire `AUTRES`. Un simple comptage avant de remplir les grilles :
    // pas la peine de construire une grille par famille pour la jeter
    // ensuite.
    let mut effectifs: HashMap<i64, usize> = HashMap::new();
    for &(_, _, _, famille) in points {
        if famille >= 0 {
            *effectifs.entry(famille).or_insert(0) += 1;
        }
    }
    let mut classees: Vec<(i64, usize)> = effectifs.into_iter().collect();
    classees.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let principales: std::collections::HashSet<i64> = classees
        .into_iter()
        .take(FAMILLES_MAX)
        .map(|(f, _)| f)
        .collect();

    let mut globale = vec![0.0f64; gn * gn];
    let mut par_famille: HashMap<i64, Vec<f64>> = HashMap::new();
    for &(_, x, y, famille) in points {
        let idx = vers_grille(y) * gn + vers_grille(x);
        globale[idx] += 1.0;
        let cle = if famille >= 0 && principales.contains(&famille) {
            famille
        } else {
            AUTRES
        };
        par_famille
            .entry(cle)
            .or_insert_with(|| vec![0.0; gn * gn])[idx] += 1.0;
    }

    let sigma_cellules = (parametres.noyau / pas).max(0.5);
    flouter_gaussien(&mut globale, gn, sigma_cellules);
    for champ in par_famille.values_mut() {
        flouter_gaussien(champ, gn, sigma_cellules);
    }

    // Normalise chaque famille sur son propre maximum — une petite famille
    // dense garde ainsi un sommet aussi net qu'une grande, avant que le
    // gain de recouvrement ci-dessous ne tranche entre elles.
    let mut normalisees: HashMap<i64, Vec<f64>> = par_famille
        .into_iter()
        .map(|(f, champ)| {
            let max = champ.iter().cloned().fold(0.0, f64::max);
            let n = if max > 0.0 {
                champ.into_iter().map(|v| v / max).collect()
            } else {
                champ
            };
            (f, n)
        })
        .collect();

    if normalisees.len() > 1 {
        resoudre_recouvrement(&mut normalisees, gn);
    }

    let mut bandes: Vec<Bande> = Vec::new();
    bandes.extend(extraire_bandes(&globale, gn, parametres.bandes, None));
    let mut clefs: Vec<i64> = normalisees.keys().copied().collect();
    clefs.sort_unstable();
    for f in clefs {
        bandes.extend(extraire_bandes(
            &normalisees[&f],
            gn,
            parametres.bandes,
            Some(f),
        ));
    }

    ResultatDensite { bandes }
}

/// Densité maximale gagnante, cellule par cellule : calcule d'abord la
/// famille gagnante de chaque cellule (une passe en lecture), puis annule
/// les autres (une passe en écriture) — deux passes séparées pour ne pas
/// emprunter la table à la fois en lecture et en écriture.
fn resoudre_recouvrement(normalisees: &mut HashMap<i64, Vec<f64>>, gn: usize) {
    let clefs: Vec<i64> = normalisees.keys().copied().collect();
    let mut gagnante = vec![i64::MIN; gn * gn];
    for cellule in 0..gn * gn {
        let mut meilleure_valeur = 0.0;
        let mut meilleure_famille = i64::MIN;
        for &f in &clefs {
            let v = normalisees[&f][cellule];
            if v > meilleure_valeur {
                meilleure_valeur = v;
                meilleure_famille = f;
            }
        }
        gagnante[cellule] = meilleure_famille;
    }
    for &f in &clefs {
        let champ = normalisees.get_mut(&f).unwrap();
        for cellule in 0..gn * gn {
            if gagnante[cellule] != f {
                champ[cellule] = 0.0;
            }
        }
    }
}

/// Trois flous en boîte plutôt qu'une vraie convolution gaussienne : une
/// convolution directe coûte `O(rayon)` par cellule, et `rayon` grandit avec
/// la résolution à noyau fixe (en unités de carte) — mesuré, une grille de
/// 1024 y passait 1,6 s (`cargo run --release -p rusty-music-core --example
/// bench_density`, avant ce changement). Le flou en boîte à somme glissante
/// (ci-dessous) coûte `O(1)` par cellule quel que soit le rayon ; trois
/// passes approximent un noyau gaussien à moins de 3 % d'erreur (technique
/// standard de traitement d'image, ex. Getreuer 2013) — largement sous ce
/// qu'un œil distingue sur une carte de densité, contre un gain déterminant
/// à haute résolution : la même grille de 1024 tombe à quelques centaines de
/// millisecondes.
fn flouter_gaussien(champ: &mut [f64], gn: usize, sigma: f64) {
    let rayon = (sigma.round() as i32).max(1);
    for _ in 0..3 {
        flou_boite(champ, gn, rayon);
    }
}

/// Flou en boîte à somme glissante, horizontal puis vertical. Bord
/// prolongé (la cellule de bord répétée au-delà de la grille) plutôt
/// qu'exclu de la moyenne : plus simple à glisser, et le domaine déborde
/// déjà de `MARGE` autour des points, l'effet de bord n'y est pas visible.
fn flou_boite(champ: &mut [f64], gn: usize, rayon: i32) {
    let fenetre = (2 * rayon + 1) as f64;
    let borne = gn as i32 - 1;
    let clamp = |i: i32| i.clamp(0, borne) as usize;

    let mut tmp = vec![0.0f64; gn * gn];
    for y in 0..gn {
        let base = y * gn;
        let mut somme: f64 = (-rayon..=rayon).map(|dx| champ[base + clamp(dx)]).sum();
        for x in 0..gn {
            tmp[base + x] = somme / fenetre;
            let xi = x as i32;
            somme += champ[base + clamp(xi + 1 + rayon)] - champ[base + clamp(xi - rayon)];
        }
    }
    for x in 0..gn {
        let mut somme: f64 = (-rayon..=rayon).map(|dy| tmp[clamp(dy) * gn + x]).sum();
        for y in 0..gn {
            champ[y * gn + x] = somme / fenetre;
            let yi = y as i32;
            somme += tmp[clamp(yi + 1 + rayon) * gn + x] - tmp[clamp(yi - rayon) * gn + x];
        }
    }
}

/// Isobandes d'un champ, via `contour` — un seuil bas (10 % du maximum du
/// champ) sous lequel rien n'est tracé, comme le `thresh` de seaborn : la
/// traîne d'une gaussienne ne s'annule jamais tout à fait, sans plancher elle
/// couvrirait toute la grille d'un voile.
///
/// `contour::isobands` rend toujours `bandes` bandes (une par paire de
/// seuils consécutifs), y compris vides si le champ n'y monte pas — c'est ce
/// que vérifie le test plus bas.
fn extraire_bandes(champ: &[f64], gn: usize, bandes: usize, famille: Option<i64>) -> Vec<Bande> {
    let max = champ.iter().cloned().fold(0.0, f64::max);
    if max <= 0.0 || bandes == 0 {
        return Vec::new();
    }
    let seuils: Vec<f64> = (0..=bandes)
        .map(|i| max * (0.10 + 0.85 * i as f64 / bandes as f64))
        .collect();

    let lo = -1.0 - MARGE;
    let pas = (2.0 + 2.0 * MARGE) / gn as f64;
    let constructeur = ContourBuilder::new(gn, gn, false)
        .x_origin(lo)
        .y_origin(lo)
        .x_step(pas)
        .y_step(pas);

    // Ne peut échouer qu'avec moins de deux seuils (`bandes == 0` est écarté
    // ci-dessus) ou un champ d'une autre taille que la grille — les deux
    // sont garantis par construction ici.
    let resultat = constructeur.isobands(champ, &seuils).unwrap_or_default();

    resultat
        .into_iter()
        .enumerate()
        .map(|(palier, bande)| {
            let polygones = bande
                .geometry()
                .0
                .iter()
                .map(|polygone| {
                    let mut anneaux = vec![anneau(polygone.exterior())];
                    anneaux.extend(polygone.interiors().iter().map(anneau));
                    anneaux
                })
                .collect();
            Bande {
                famille,
                palier,
                polygones,
            }
        })
        .collect()
}

fn anneau(ligne: &geo_types::LineString<f64>) -> Vec<[f64; 2]> {
    ligne.coords().map(|c| [c.x, c.y]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un amas net autour de l'origine, une seule famille : `isobands` doit
    /// rendre exactement `bandes` bandes par nappe (globale et famille), pas
    /// plus, pas moins — c'est le contrat de `contour::isobands`
    /// (`rings.windows(2)`, une bande par paire de seuils consécutifs).
    #[test]
    fn nombre_de_bandes_correspond_aux_seuils_demandes() {
        let mut points = Vec::new();
        let mut id = 0i64;
        for gx in -12..=12 {
            for gy in -12..=12 {
                points.push((id, gx as f32 * 0.015, gy as f32 * 0.015, 0i64));
                id += 1;
            }
        }
        let parametres = ParametresDensite {
            noyau: 0.06,
            resolution: 64,
            bandes: 5,
        };
        let resultat = calculer(&points, &parametres);

        let paliers_famille: std::collections::HashSet<_> = resultat
            .bandes
            .iter()
            .filter(|b| b.famille == Some(0))
            .map(|b| b.palier)
            .collect();
        assert_eq!(paliers_famille.len(), parametres.bandes);

        let paliers_globale: std::collections::HashSet<_> = resultat
            .bandes
            .iter()
            .filter(|b| b.famille.is_none())
            .map(|b| b.palier)
            .collect();
        assert_eq!(paliers_globale.len(), parametres.bandes);
    }

    /// Deux amas nettement séparés, deux familles : la bande la plus haute
    /// de chacune doit entourer un point proche de son propre centre, pas de
    /// l'autre — sinon le gain « densité maximale gagnante » n'a pas
    /// vraiment tranché entre les deux.
    #[test]
    fn le_recouvrement_se_resout_par_famille_gagnante() {
        let mut points = Vec::new();
        let mut id = 0i64;
        for gx in -8..=8 {
            for gy in -8..=8 {
                let dx = gx as f32 * 0.01;
                let dy = gy as f32 * 0.01;
                points.push((id, -0.3 + dx, -0.3 + dy, 0i64));
                id += 1;
                points.push((id, 0.3 + dx, 0.3 + dy, 1i64));
                id += 1;
            }
        }
        let parametres = ParametresDensite {
            noyau: 0.05,
            resolution: 96,
            bandes: 4,
        };
        let resultat = calculer(&points, &parametres);

        for (famille, centre) in [(0i64, [-0.3, -0.3]), (1i64, [0.3, 0.3])] {
            let sommet = resultat
                .bandes
                .iter()
                .filter(|b| b.famille == Some(famille) && b.palier == parametres.bandes - 1)
                .find(|b| !b.polygones.is_empty())
                .expect("la bande la plus haute doit exister près de chaque centre");
            let anneau = &sommet.polygones[0][0];
            let cx: f64 = anneau.iter().map(|p| p[0]).sum::<f64>() / anneau.len() as f64;
            let cy: f64 = anneau.iter().map(|p| p[1]).sum::<f64>() / anneau.len() as f64;
            assert!(
                (cx - centre[0]).abs() < 0.15 && (cy - centre[1]).abs() < 0.15,
                "bande sommitale de la famille {famille} centrée en ({cx}, {cy}), attendue près de {centre:?}"
            );
        }
    }

    /// Le champ global doit culminer là où les morceaux sont, et s'annuler
    /// loin d'eux — c'est tout ce que le relief lui demande.
    #[test]
    fn le_champ_global_culmine_sur_lamas() {
        let points: Vec<(i64, f32, f32, i64)> = (0..200)
            .map(|i| (i, 0.3 + (i % 7) as f32 * 0.002, -0.2, 0i64))
            .collect();
        let parametres = ParametresDensite {
            noyau: 0.03,
            resolution: 128,
            bandes: 4,
        };
        let champ = champ_global(&points, &parametres);
        assert_eq!(champ.len(), 128 * 128);

        let gn = 128usize;
        let lo = -1.0 - MARGE;
        let pas = (2.0 + 2.0 * MARGE) / gn as f64;
        let cellule = |x: f64, y: f64| -> usize {
            let gx = ((x - lo) / pas).floor().clamp(0.0, (gn - 1) as f64) as usize;
            let gy = ((y - lo) / pas).floor().clamp(0.0, (gn - 1) as f64) as usize;
            gy * gn + gx
        };
        // Le maximum vaut 1 quelque part — c'est le contrat de la
        // normalisation. Il ne tombe pas forcément sur la cellule de (0,3 ;
        // -0,2) : les morceaux s'étalent un peu et le flou déplace le sommet
        // d'une cellule ou deux.
        let max = champ.iter().cloned().fold(0.0, f64::max);
        assert!((max - 1.0).abs() < 1e-9, "maximum non normalisé : {max}");
        assert!(
            champ[cellule(0.3, -0.2)] > 0.9,
            "l'amas doit être tout près du sommet : {}",
            champ[cellule(0.3, -0.2)]
        );
        assert!(
            champ[cellule(-0.8, 0.8)] < 0.01,
            "le champ doit s'éteindre loin de l'amas"
        );
    }

    /// L'échantillonnage doit retrouver le sommet là où sont les morceaux, et
    /// zéro loin d'eux — mêmes attentes que `champ_global`, mais lues en
    /// coordonnées de carte plutôt qu'en indices de grille.
    #[test]
    fn lechantillonnage_suit_les_coordonnees_de_carte() {
        let points: Vec<(i64, f32, f32, i64)> =
            (0..200).map(|i| (i, -0.4, 0.55, 0i64)).collect();
        let parametres = ParametresDensite {
            noyau: 0.04,
            resolution: 128,
            bandes: 4,
        };
        let champ = champ_global(&points, &parametres);
        let sur = echantillonner(&champ, 128, -0.4, 0.55);
        let loin = echantillonner(&champ, 128, 0.7, -0.7);
        assert!(sur > 0.9, "sous l'amas : {sur}");
        assert!(loin < 0.01, "loin de l'amas : {loin}");
        // Hors domaine, on rend la valeur du bord plutôt qu'une panique.
        assert!(echantillonner(&champ, 128, -50.0, 50.0).is_finite());
        // Une grille incohérente ne doit pas faire tomber l'appelant.
        assert_eq!(echantillonner(&champ, 999, 0.0, 0.0), 0.0);
    }

    /// Douze familles, bien au-delà de `FAMILLES_MAX` (7) : au plus sept
    /// gardent leur propre territoire, le reste rejoint `AUTRES` — jamais
    /// treize territoires distincts.
    #[test]
    fn les_familles_en_trop_rejoignent_autres() {
        const N_FAMILLES: i64 = 12;
        let mut points = Vec::new();
        let mut id = 0i64;
        for famille in 0..N_FAMILLES {
            // Des effectifs délibérément inégaux, pour qu'un vrai partage
            // top-7 / reste ait lieu plutôt qu'une égalité arbitraire.
            let n = 5 + (famille as usize) * 3;
            let cx = -0.8 + (famille as f32 / N_FAMILLES as f32) * 1.6;
            for i in 0..n {
                let dy = (i as f32 % 9.0 - 4.0) * 0.008;
                points.push((id, cx, dy, famille));
                id += 1;
            }
        }
        let parametres = ParametresDensite {
            noyau: 0.03,
            resolution: 128,
            bandes: 4,
        };
        let resultat = calculer(&points, &parametres);

        let territoires: std::collections::HashSet<Option<i64>> =
            resultat.bandes.iter().map(|b| b.famille).collect();
        // Chaque territoire non vide vient soit d'une famille principale
        // (identifiant réel), soit d'`AUTRES` — jamais plus de
        // `FAMILLES_MAX` identifiants réels, plus `AUTRES`, plus `None`
        // (nappe globale).
        let reelles = territoires
            .iter()
            .filter(|f| !matches!(f, None | Some(AUTRES)))
            .count();
        assert!(reelles <= FAMILLES_MAX, "{reelles} familles réelles gardées, attendu au plus {FAMILLES_MAX}");
        assert!(
            territoires.contains(&Some(AUTRES)),
            "les familles en trop devraient former un territoire « autres »"
        );
    }
}
