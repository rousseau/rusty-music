//! Le peuplement : des habitants, des établissements, une carte qui se lit.
//!
//! Conception complète dans `docs/carto-peuplement-architecture.md`. Ce module
//! en implémente le cœur : les morceaux s'installent **par ordre de date de
//! sortie**, chacun rejoignant l'établissement le plus affine s'il en trouve un
//! à portée, en fondant un sinon.
//!
//! **La stabilité est une conséquence de l'algorithme, pas une contrainte
//! ajoutée.** La parcelle d'un habitant vaut
//! `phyllotaxie(centre_du_fondateur, rang_d'arrivée_dans_l'établissement)` :
//! ni l'un ni l'autre ne dépend de ce qui arrive ensuite, donc une position est
//! écrite une fois et n'est plus jamais recalculée.

use std::collections::HashMap;

use rusty_music_core::density;

/// Angle d'or, en radians. La spirale phyllotaxique répartit les parcelles sans
/// jamais en aligner deux — c'est la disposition des graines d'un tournesol.
const ANGLE_DOR: f32 = 2.399_963_2;

/// Ce qu'il faut savoir d'un morceau pour l'installer.
#[derive(Debug, Clone)]
pub struct Arrivant {
    pub track_id: i64,
    /// Position dans le monde, en unités de carte. **Fonction du morceau
    /// seul** : ici la projection déjà figée en base.
    pub x: f32,
    pub y: f32,
    /// Empreinte, pour l'affinité. Vide = rejoint sans discuter.
    pub empreinte: Vec<f32>,
    pub famille: i64,
    /// AAAAMMJJ.
    pub date: u32,
    pub artiste: String,
}

/// Les six rangs de la hiérarchie, et ce qui les distingue à l'écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Rang {
    Ferme,
    Hameau,
    Village,
    Bourg,
    Ville,
    Metropole,
}

impl Rang {
    /// Les seuils du document, tels quels.
    pub fn depuis_population(n: u32) -> Rang {
        match n {
            0..=1 => Rang::Ferme,
            2..=5 => Rang::Hameau,
            6..=20 => Rang::Village,
            21..=60 => Rang::Bourg,
            61..=200 => Rang::Ville,
            _ => Rang::Metropole,
        }
    }

    /// Rang numérique, pour les tuiles et le tri d'étiquettes.
    pub fn indice(self) -> i64 {
        match self {
            Rang::Ferme => 0,
            Rang::Hameau => 1,
            Rang::Village => 2,
            Rang::Bourg => 3,
            Rang::Ville => 4,
            Rang::Metropole => 5,
        }
    }

    /// Le zoom auquel il apparaît. C'est cette échelle-là qui produit
    /// l'impression de carte d'état-major : chaque niveau révèle une strate.
    pub fn zoom_apparition(self) -> f64 {
        match self {
            Rang::Metropole => 2.0,
            Rang::Ville => 4.0,
            Rang::Bourg => 5.5,
            Rang::Village => 7.0,
            Rang::Hameau => 8.5,
            Rang::Ferme => 10.0,
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Rang::Ferme => "ferme",
            Rang::Hameau => "hameau",
            Rang::Village => "village",
            Rang::Bourg => "bourg",
            Rang::Ville => "ville",
            Rang::Metropole => "métropole",
        }
    }
}

/// Un établissement, tel qu'il sort du peuplement.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Etablissement {
    pub id: u32,
    /// Centre, **figé à la fondation**. Ne bouge jamais.
    pub cx: f32,
    pub cy: f32,
    pub population: u32,
    pub fondation_date: u32,
    pub fondation_rang: u32,
    pub famille: i64,
    /// Toponyme : le nom de l'artiste fondateur. La seule source dont on
    /// dispose, et elle a du sens — un lieu porte le nom de qui l'a fondé.
    pub nom: String,
    /// Fondé hors du continent : les plus isolés deviennent des îles.
    pub ile: bool,
}

/// La parcelle d'un habitant.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Habitant {
    pub track_id: i64,
    pub etablissement: u32,
    /// Rang d'arrivée **dans** l'établissement.
    pub place: u32,
    /// Rang d'arrivée dans le monde entier — permet de rejouer la croissance.
    pub arrivee: u32,
    pub date: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Parametres {
    /// Cosinus minimal pour rejoindre plutôt que fonder.
    pub seuil_affinite: f32,
    /// Rayon de recrutement d'un établissement d'un seul habitant.
    pub rayon_base: f32,
    /// Plafond du rayon de recrutement.
    pub rayon_max: f32,
    /// Espacement des parcelles dans la spirale.
    pub pas_parcelle: f32,
    /// Quantile du champ d'habitabilité, évalué **aux positions des
    /// habitants**, sous lequel on est en mer.
    pub quantile_mer: f64,
}

impl Default for Parametres {
    fn default() -> Self {
        Self {
            // **Le seuil d'affinité n'est pas le paramètre qui décide.**
            // Balayé de 0,30 à 0,55 sur la bibliothèque réelle, il fait varier
            // le nombre d'établissements de 3 126 à 3 142 — rien. Ce qui lie,
            // c'est la géométrie : le rayon de recrutement.
            seuil_affinite: 0.55,
            // Calibré, pas deviné. À 0,012 — un espacement moyen entre
            // morceaux — 43 % des établissements étaient des fermes isolées et
            // il n'y avait qu'une métropole. À 0,024, **les six rangs sont
            // tous peuplés** : 196 fermes, 145 hameaux, 104 villages,
            // 126 bourgs, 178 villes, 8 métropoles, pour 757 établissements.
            // C'est cette hiérarchie complète qui fait marcher la révélation
            // par échelle.
            rayon_base: 0.024,
            rayon_max: 0.10,
            // **Calibré sur la part de sol bâti d'un vrai pays.** 757
            // établissements sur un disque de rayon 1 donnent à chacun une part
            // de rayon 0,036 ; le bâti doit en occuper quelques pour cent, pas
            // la totalité. À 0,0048 les agglomérations se touchaient et
            // formaient une nappe grise continue ; à 0,0015 une seule ville
            // couvrait le tiers de l'écran au zoom 5. À 0,0005, la plus grande
            // fait 0,011 — 30 % de sa part, soit environ 9 % de sa surface,
            // l'ordre de grandeur d'un pays habité.
            pas_parcelle: 0.0005,
            quantile_mer: 0.01,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Rapport {
    pub habitants: usize,
    pub etablissements: usize,
    pub iles: usize,
    pub par_rang: Vec<(String, usize)>,
    pub plus_grand: u32,
    pub ms: u128,
    pub niveau_mer: f64,
}

pub struct Peuplement {
    pub etablissements: Vec<Etablissement>,
    pub habitants: Vec<Habitant>,
    pub rapport: Rapport,
}

/// La parcelle d'un habitant : spirale phyllotaxique autour du centre.
///
/// `place = 0` place le fondateur exactement au centre. Aucun terme ne dépend
/// de ce qui arrivera ensuite — **c'est là que se joue la stabilité**.
pub fn parcelle(cx: f32, cy: f32, place: u32, pas: f32) -> (f32, f32) {
    let r = pas * (place as f32).sqrt();
    let a = place as f32 * ANGLE_DOR;
    (cx + r * a.cos(), cy + r * a.sin())
}

/// Le contour bâti d'un établissement, en coordonnées de carte.
///
/// **C'est ce qui manque le plus à une carte** : sur un plan, une agglomération
/// est une tache d'une couleur qui n'est pas celle de la campagne. Un simple
/// point, si gros soit-il, ne dit pas « ici, c'est la ville ».
///
/// Le rayon est celui de la spirale phyllotaxique — le bâti couvre exactement
/// les parcelles. Le contour est perturbé par un bruit déterministe tiré de
/// l'identifiant : un disque parfait se lit comme un symbole, pas comme un
/// lieu, et deux villes voisines auraient la même silhouette.
pub fn contour_bati(e: &Etablissement, pas: f32, cotes: usize) -> Vec<[f32; 2]> {
    let base = (pas * (e.population as f32).sqrt()).max(pas * 0.9);
    // Un peu plus large que le bâti strict : les faubourgs.
    let base = base * 1.45;
    let mut points = Vec::with_capacity(cotes);
    for i in 0..cotes {
        let a = i as f32 / cotes as f32 * std::f32::consts::TAU;
        // Deux harmoniques suffisent à casser le cercle sans le déformer.
        let graine = e.id as f32 * 0.7351;
        let bruit = 1.0
            + 0.18 * (a * 3.0 + graine).sin()
            + 0.10 * (a * 5.0 - graine * 1.7).sin();
        let r = base * bruit;
        points.push([e.cx + r * a.cos(), e.cy + r * a.sin()]);
    }
    points.push(points[0]);
    points
}

/// Rayon de recrutement : croît comme la racine de la population, donc l'aire
/// du bassin croît proportionnellement au nombre d'habitants.
fn rayon(population: u32, p: &Parametres) -> f32 {
    (p.rayon_base * (population as f32).sqrt()).min(p.rayon_max)
}

/// Grille uniforme sur le domaine borné : la requête est à rayon borné, un
/// R-tree n'apporterait rien.
struct Grille {
    cote: f32,
    n: usize,
    cases: Vec<Vec<u32>>,
}

impl Grille {
    fn neuve(rayon_max: f32) -> Self {
        let n = ((2.0 + 2.0 * density::MARGE as f32) / rayon_max).ceil() as usize;
        Grille {
            cote: rayon_max,
            n: n.max(1),
            cases: vec![Vec::new(); n.max(1) * n.max(1)],
        }
    }
    fn indice(&self, x: f32, y: f32) -> (usize, usize) {
        let lo = -1.0 - density::MARGE as f32;
        let i = (((x - lo) / self.cote) as isize).clamp(0, self.n as isize - 1) as usize;
        let j = (((y - lo) / self.cote) as isize).clamp(0, self.n as isize - 1) as usize;
        (i, j)
    }
    fn poser(&mut self, x: f32, y: f32, id: u32) {
        let (i, j) = self.indice(x, y);
        self.cases[j * self.n + i].push(id);
    }
    /// Les établissements des neuf cases autour d'un point.
    fn autour(&self, x: f32, y: f32) -> impl Iterator<Item = u32> + '_ {
        let (i, j) = self.indice(x, y);
        let (n, i, j) = (self.n as isize, i as isize, j as isize);
        (-1..=1)
            .flat_map(move |dj| (-1..=1).map(move |di| (di, dj)))
            .filter_map(move |(di, dj)| {
                let (a, b) = (i + di, j + dj);
                (a >= 0 && a < n && b >= 0 && b < n).then(|| (b * n + a) as usize)
            })
            .flat_map(move |c| self.cases[c].iter().copied())
    }
}

fn cosinus(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 1.0;
    }
    let (mut pv, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        pv += x * y;
        na += x * x;
        nb += y * y;
    }
    (pv / (na.sqrt() * nb.sqrt()).max(f32::EPSILON)).clamp(-1.0, 1.0)
}

/// Peuple le monde.
///
/// `arrivants` doit être **déjà trié** par ordre d'arrivée : c'est
/// `core::db::Library::ordre_darrivee` qui en décide, et lui seul.
pub fn peupler(arrivants: &[Arrivant], p: &Parametres) -> Peuplement {
    let debut = std::time::Instant::now();

    // Le champ d'habitabilité, et le niveau de la mer qui s'en déduit. On le
    // prend comme un **quantile évalué aux positions des habitants** et non
    // sur les cellules de grille : la part de terre émergée devient alors une
    // mesure, pas un réglage — et surtout personne ne se noie par surprise.
    let points: Vec<(i64, f32, f32, i64)> = arrivants
        .iter()
        .map(|a| (a.track_id, a.x, a.y, a.famille))
        .collect();
    let parametres = density::ParametresDensite {
        noyau: 0.05,
        resolution: 512,
        bandes: 4,
    };
    let champ = density::champ_global(&points, &parametres);
    let mut sous_les_pieds: Vec<f64> = arrivants
        .iter()
        .map(|a| density::echantillonner(&champ, parametres.resolution, a.x, a.y))
        .collect();
    let mut triees = sous_les_pieds.clone();
    triees.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let niveau_mer = if triees.is_empty() {
        0.0
    } else {
        triees[((triees.len() as f64 * p.quantile_mer) as usize).min(triees.len() - 1)]
    };

    let mut etablissements: Vec<Etablissement> = Vec::new();
    let mut centroides: Vec<Vec<f32>> = Vec::new();
    let mut habitants: Vec<Habitant> = Vec::with_capacity(arrivants.len());
    let mut grille = Grille::neuve(p.rayon_max);

    for (rang, a) in arrivants.iter().enumerate() {
        // Le meilleur établissement dont le bassin nous contient.
        let mut meilleur: Option<(u32, f32)> = None;
        for id in grille.autour(a.x, a.y) {
            let e = &etablissements[id as usize];
            let d = ((e.cx - a.x).powi(2) + (e.cy - a.y).powi(2)).sqrt();
            if d > rayon(e.population, p) {
                continue;
            }
            let aff = cosinus(&a.empreinte, &centroides[id as usize]);
            if aff >= p.seuil_affinite && meilleur.is_none_or(|(_, m)| aff > m) {
                meilleur = Some((id, aff));
            }
        }

        match meilleur {
            Some((id, _)) => {
                let e = &mut etablissements[id as usize];
                let place = e.population;
                e.population += 1;
                // Centroïde courant, mis à jour en ligne.
                let c = &mut centroides[id as usize];
                if c.len() == a.empreinte.len() {
                    let n = place as f32 + 1.0;
                    for (v, x) in c.iter_mut().zip(&a.empreinte) {
                        *v += (x - *v) / n;
                    }
                }
                let (x, y) = parcelle(e.cx, e.cy, place, p.pas_parcelle);
                habitants.push(Habitant {
                    track_id: a.track_id,
                    etablissement: id,
                    place,
                    arrivee: rang as u32,
                    date: a.date,
                    x,
                    y,
                });
            }
            None => {
                let id = etablissements.len() as u32;
                // Personne ne se noie : un habitant sous le niveau de la mer
                // fonde une **île**, ce que la typologie prévoit déjà avec la
                // ferme isolée.
                let ile = sous_les_pieds[rang] < niveau_mer;
                etablissements.push(Etablissement {
                    id,
                    cx: a.x,
                    cy: a.y,
                    population: 1,
                    fondation_date: a.date,
                    fondation_rang: rang as u32,
                    famille: a.famille,
                    nom: a.artiste.clone(),
                    ile,
                });
                centroides.push(a.empreinte.clone());
                grille.poser(a.x, a.y, id);
                habitants.push(Habitant {
                    track_id: a.track_id,
                    etablissement: id,
                    place: 0,
                    arrivee: rang as u32,
                    date: a.date,
                    x: a.x,
                    y: a.y,
                });
            }
        }
    }
    sous_les_pieds.clear();

    let mut comptes: HashMap<&str, usize> = HashMap::new();
    for e in &etablissements {
        *comptes
            .entry(Rang::depuis_population(e.population).nom())
            .or_default() += 1;
    }
    let mut par_rang: Vec<(String, usize)> =
        comptes.into_iter().map(|(n, c)| (n.to_string(), c)).collect();
    par_rang.sort_by_key(|(n, _)| {
        ["ferme", "hameau", "village", "bourg", "ville", "métropole"]
            .iter()
            .position(|x| x == n)
            .unwrap_or(9)
    });

    let rapport = Rapport {
        habitants: habitants.len(),
        etablissements: etablissements.len(),
        iles: etablissements.iter().filter(|e| e.ile).count(),
        plus_grand: etablissements.iter().map(|e| e.population).max().unwrap_or(0),
        par_rang,
        ms: debut.elapsed().as_millis(),
        niveau_mer,
    };
    Peuplement {
        etablissements,
        habitants,
        rapport,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrivant(id: i64, x: f32, y: f32, date: u32, e: &[f32]) -> Arrivant {
        Arrivant {
            track_id: id,
            x,
            y,
            empreinte: e.to_vec(),
            famille: 0,
            date,
            artiste: format!("artiste {id}"),
        }
    }

    /// **Le théorème de stabilité.** La parcelle d'un habitant ne dépend que du
    /// centre de son établissement et de son rang d'arrivée : ajouter des
    /// arrivants après lui ne doit rien changer à sa position.
    #[test]
    fn un_arrivant_ne_deplace_jamais_personne() {
        let mut v: Vec<Arrivant> = (0..40)
            .map(|i| arrivant(i, 0.01 * (i % 3) as f32, 0.0, 19_800_000 + i as u32, &[1.0, 0.0]))
            .collect();
        let avant = peupler(&v, &Parametres::default());

        for i in 40..80 {
            v.push(arrivant(i, 0.01 * (i % 3) as f32, 0.0, 20_000_000 + i as u32, &[1.0, 0.0]));
        }
        let apres = peupler(&v, &Parametres::default());

        for a in &avant.habitants {
            let b = apres
                .habitants
                .iter()
                .find(|h| h.track_id == a.track_id)
                .expect("habitant disparu");
            assert_eq!((a.x, a.y), (b.x, b.y), "morceau {} déplacé", a.track_id);
            assert_eq!(a.place, b.place);
            assert_eq!(a.etablissement, b.etablissement);
        }
    }

    /// Un morceau sans affinité fonde plutôt que de rejoindre.
    #[test]
    fn un_etranger_fonde_son_propre_etablissement() {
        let v = vec![
            arrivant(1, 0.0, 0.0, 19_700_000, &[1.0, 0.0]),
            arrivant(2, 0.001, 0.0, 19_710_000, &[1.0, 0.0]), // même son, même lieu
            arrivant(3, 0.002, 0.0, 19_720_000, &[0.0, 1.0]), // orthogonal
        ];
        let p = peupler(&v, &Parametres::default());
        let de = |id: i64| p.habitants.iter().find(|h| h.track_id == id).unwrap().etablissement;
        assert_eq!(de(1), de(2), "deux morceaux identiques devraient cohabiter");
        assert_ne!(de(1), de(3), "un son étranger devrait fonder ailleurs");
    }

    /// Les six rangs suivent les seuils du document.
    #[test]
    fn les_seuils_de_la_typologie_sont_ceux_du_document() {
        use Rang::*;
        for (n, attendu) in [
            (1u32, Ferme),
            (2, Hameau),
            (5, Hameau),
            (6, Village),
            (20, Village),
            (21, Bourg),
            (60, Bourg),
            (61, Ville),
            (200, Ville),
            (201, Metropole),
        ] {
            assert_eq!(Rang::depuis_population(n), attendu, "population {n}");
        }
        // Et les zooms d'apparition vont du grand au petit.
        let zooms: Vec<f64> = [Metropole, Ville, Bourg, Village, Hameau, Ferme]
            .iter()
            .map(|r| r.zoom_apparition())
            .collect();
        assert!(zooms.windows(2).all(|z| z[0] < z[1]), "{zooms:?}");
    }

    /// Le fondateur est au centre, et les parcelles s'écartent en spirale sans
    /// jamais se superposer.
    #[test]
    fn les_parcelles_secartent_en_spirale() {
        let pas = 0.0025;
        assert_eq!(parcelle(0.5, -0.2, 0, pas), (0.5, -0.2));
        let mut precedent = 0.0;
        for place in [1u32, 4, 16, 64, 200] {
            let (x, y) = parcelle(0.0, 0.0, place, pas);
            let r = (x * x + y * y).sqrt();
            assert!(r > precedent, "le rayon doit croître : {place}");
            precedent = r;
        }
        // 200 habitants tiennent dans un disque de rayon 0,035.
        let (x, y) = parcelle(0.0, 0.0, 200, pas);
        assert!((x * x + y * y).sqrt() < 0.036);
    }

    /// Le contour bâti doit envelopper les parcelles et n'être pas un cercle.
    #[test]
    fn le_bati_enveloppe_les_parcelles_sans_etre_un_cercle() {
        let e = Etablissement {
            id: 3,
            cx: 0.2,
            cy: -0.1,
            population: 100,
            fondation_date: 19_900_000,
            fondation_rang: 0,
            famille: 0,
            nom: "essai".into(),
            ile: false,
        };
        let pas = 0.0048;
        let c = contour_bati(&e, pas, 32);
        assert_eq!(c.len(), 33, "le contour doit être fermé");

        let rayons: Vec<f32> = c
            .iter()
            .map(|p| ((p[0] - e.cx).powi(2) + (p[1] - e.cy).powi(2)).sqrt())
            .collect();
        let min = rayons.iter().cloned().fold(f32::MAX, f32::min);
        let max = rayons.iter().cloned().fold(0.0, f32::max);
        assert!(max > min * 1.15, "contour trop régulier : {min} à {max}");

        // La parcelle la plus éloignée doit tomber dedans.
        let (px, py) = parcelle(e.cx, e.cy, e.population - 1, pas);
        let d = ((px - e.cx).powi(2) + (py - e.cy).powi(2)).sqrt();
        assert!(d < min, "une parcelle déborde du bâti : {d} contre {min}");

        // Déterministe : deux appels rendent le même contour.
        assert_eq!(c, contour_bati(&e, pas, 32));
    }

    #[test]
    fn tous_les_arrivants_sont_places() {
        let v: Vec<Arrivant> = (0..200)
            .map(|i| {
                let t = i as f32 / 200.0 * std::f32::consts::TAU;
                arrivant(i, t.cos() * 0.6, t.sin() * 0.6, 19_600_000 + i as u32, &[t.cos(), t.sin()])
            })
            .collect();
        let p = peupler(&v, &Parametres::default());
        assert_eq!(p.habitants.len(), 200);
        assert_eq!(p.rapport.habitants, 200);
        let uniques: std::collections::HashSet<i64> =
            p.habitants.iter().map(|h| h.track_id).collect();
        assert_eq!(uniques.len(), 200, "un morceau placé deux fois");
    }
}
