// SPDX-License-Identifier: GPL-3.0-or-later
//! Ce que les tuiles doivent contenir, avant toute question de format.
//!
//! Le crate ne lit pas la base : il reçoit des données déjà rassemblées, comme
//! `core::density::calculer` reçoit ses positions. C'est ce qui le rend
//! testable sans SQLite et sans bibliothèque de 27 000 morceaux.

use std::collections::HashMap;

use rusty_music_core::density::Bande;

/// Accumulateur par artiste : somme des x, somme des y, effectif, et le
/// nombre de morceaux par famille (pour trancher la famille dominante).
type CumulArtiste = (f64, f64, usize, HashMap<i64, usize>);

/// Un morceau placé sur la carte.
#[derive(Debug, Clone)]
pub struct Morceau {
    pub id: i64,
    pub x: f32,
    pub y: f32,
    /// Famille (grappe) d'appartenance. Négatif = sans famille.
    pub famille: i64,
    pub titre: String,
    pub artiste: String,
    pub annee: Option<i32>,
    pub bpm: Option<f32>,
    pub energie: Option<f32>,
}

/// Une famille et son étiquette — ce qui tient lieu de « genre » sur la carte.
#[derive(Debug, Clone)]
pub struct Famille {
    pub id: i64,
    pub nom: String,
    pub effectif: usize,
}

/// Une ville : un artiste, sa position et sa taille.
#[derive(Debug, Clone)]
pub struct Artiste {
    pub nom: String,
    pub x: f32,
    pub y: f32,
    pub famille: i64,
    /// Nombre de morceaux dans la bibliothèque. **C'est la popularité dont on
    /// dispose** : `carto-google-maps.md` prévoit ListenBrainz et les compteurs
    /// de lecture locaux, ni l'un ni l'autre n'existe aujourd'hui. Le compte de
    /// morceaux en est une approximation honnête et locale — une intégrale de
    /// ce qu'on a jugé bon de garder.
    pub effectif: usize,
    /// Nom du monument où cet artiste est ancré, s'il l'est — les plus
    /// populaires quittent leur quartier de famille pour un lieu iconique de
    /// Paris (`crate::ancrage`, `docs/carto-ville.md`). `None` = artiste
    /// ordinaire, posé sur sa rue par l'affectation.
    pub ancre: Option<String>,
}

/// Un tronçon du réseau de circulation, en coordonnées de carte.
///
/// Le réseau est calculé dans `analysis::reseau`, qui dépend de Burn ; ce crate
/// n'en reçoit que la géométrie. C'est ce qui permet à `carto` de rester léger
/// sans que la dépendance s'inverse.
#[derive(Debug, Clone)]
pub struct Route {
    /// Le tracé, du départ à l'arrivée. Au moins deux points ; davantage quand
    /// la route épouse le relief.
    pub points: Vec<[f32; 2]>,
    /// 0 autoroute, 1 nationale, 2 secondaire, 3 sentier.
    pub classe: u8,
}

impl Route {
    /// Un tronçon droit — le cas dégénéré.
    pub fn droite(a: [f32; 2], b: [f32; 2], classe: u8) -> Self {
        Route {
            points: vec![a, b],
            classe,
        }
    }
}

/// Un point remarquable : ce qu'une carte signale d'un symbole parce que cela
/// mérite le détour.
///
/// Sur un plan ordinaire ce sont les monuments, les gares, les points de vue.
/// Ici, trois espèces, et chacune répond à une question qu'on se pose en
/// explorant une discothèque.
#[derive(Debug, Clone)]
pub struct Curiosite {
    pub x: f32,
    pub y: f32,
    pub nom: String,
    pub espece: Espece,
    /// Année, quand elle a un sens.
    pub annee: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Espece {
    /// Le morceau le plus ancien d'un territoire — son monument historique.
    Monument,
    /// Un morceau dont même le plus proche voisin est loin : rien ne lui
    /// ressemble dans la bibliothèque. Le « refuge isolé » du document.
    Refuge,
    /// Le morceau qui a fondé une métropole. On vient y voir d'où c'est parti.
    Fondation,
}

impl Espece {
    pub fn indice(self) -> i64 {
        match self {
            Espece::Monument => 0,
            Espece::Refuge => 1,
            Espece::Fondation => 2,
        }
    }
    pub fn nom(self) -> &'static str {
        match self {
            Espece::Monument => "monument",
            Espece::Refuge => "refuge",
            Espece::Fondation => "fondation",
        }
    }
}

/// Un tronçon de rue **réelle** (OSM), en coordonnées géographiques —
/// distinct de [`Route`], qui porte le réseau sonique en coordonnées carte.
///
/// Contrairement à `Route`, un `TronconReel` n'a besoin ni d'astuce de crête
/// ni de filtre « trop long pour être dessiné » : c'est déjà une polyligne
/// courte et correctement formée (`docs/carto-etapes.md`).
#[derive(Debug, Clone)]
pub struct TronconReel {
    /// Sommets en `[lon, lat]`, degrés.
    pub points: Vec<[f64; 2]>,
    pub classe: rusty_music_osm::Classe,
    /// Nom inventé, affiché — type de voie OSM + artiste (`carto-ville.md`).
    pub nom: String,
    /// Nom OSM réel, caché : traçabilité et débogage, jamais rendu.
    pub nom_osm: Option<String>,
    pub famille: Option<i64>,
    pub artiste: Option<String>,
}

/// Un contour géographique — plan d'eau, espace vert — en `[lon, lat]`, tel
/// que `crates/osm` le fournit.
#[derive(Debug, Clone)]
pub struct ContourReel {
    pub points: Vec<[f64; 2]>,
}

/// Un quartier musical comme aplat — la zone du diagramme de puissance de
/// l'étage 1 attribuée à une famille, en `[lon, lat]`. Ce que la carte montre
/// **en dézoomant**, quand les bâtiments individuels ne sont pas encore
/// révélés : sans lui, Paris entier n'a plus aucune couleur de genre.
/// Vide sur le chemin fictif, où [`Source::bandes`] (nappe de densité) tient
/// ce rôle.
#[derive(Debug, Clone)]
pub struct TerritoireReel {
    pub famille: i64,
    /// Un ou plusieurs polygones, chacun : anneau extérieur puis trous.
    pub polygones: Vec<Vec<Vec<[f64; 2]>>>,
}

/// Un bâtiment réel, avec l'occupant qui l'habite le cas échéant.
///
/// Distinct de [`ContourReel`] parce qu'un bâtiment porte une information que
/// l'eau et les espaces verts n'ont pas : qui l'habite. C'est ce qui permet à
/// `tuiles`/`style` de **colorer le bâtiment entier** plutôt que d'y poser un
/// point pour son morceau — un point de quelques pixels se perdait sur la
/// carte (voir `carto-ville.md`) ; un bâtiment coloré se voit.
#[derive(Debug, Clone)]
pub struct BatimentReel {
    pub points: Vec<[f64; 2]>,
    /// Le morceau qui l'habite, `None` si le bâtiment est vacant.
    pub morceau_id: Option<i64>,
    /// La famille du morceau qui l'habite — c'est elle qui colore le
    /// bâtiment, comme `style::couleur_famille` colore un point de morceau
    /// ailleurs sur la carte. `None` si vacant.
    pub famille: Option<i64>,
}

/// Un repère réel notable — musée, monument, lieu de culte — en `[lon, lat]`,
/// tel que `crates/osm::PointRemarquable` le fournit. Copié tel quel, sans
/// passer par l'affectation (comme [`ContourReel`]) : ce n'est pas un
/// logement, juste une ancre visuelle.
#[derive(Debug, Clone)]
pub struct PointReel {
    pub point: [f64; 2],
    pub nom: String,
    pub genre: String,
    /// Artiste ancré sur ce monument, s'il y en a un (`crate::ancrage`). Le
    /// rendu accole alors son nom au symbole du monument.
    pub artiste: Option<String>,
}

/// Un album, posé au milieu des bâtiments de ses morceaux — l'échelon
/// intermédiaire entre l'artiste (une rue) et le morceau (un bâtiment).
/// Réel seulement : les pistes d'un album se logent contiguës le long d'une
/// rue par construction de `affectation::loger_dans_batiments` (ordre
/// album/piste), donc leur ancre commune tombe naturellement sur ce
/// tronçon-là plutôt qu'ailleurs dans le quartier.
#[derive(Debug, Clone)]
pub struct AlbumReel {
    pub point: [f64; 2],
    pub nom: String,
    pub artiste: String,
    pub famille: i64,
    /// Nombre de morceaux de cet album effectivement logés — sert à trier
    /// l'affichage, comme `Artiste::effectif`.
    pub effectif: usize,
}

/// Tout ce qui entre dans une archive de tuiles.
#[derive(Default)]
pub struct Source {
    pub morceaux: Vec<Morceau>,
    pub familles: Vec<Famille>,
    /// Bandes de densité, telles que rend `core::density::calculer`.
    /// Celles dont la famille vaut `None` sont la nappe globale : c'est
    /// **elle qui dessine le littoral**.
    ///
    /// Chemin fictif seulement — vide sur le plan de ville réel, où
    /// [`Source::frontiere`] tient ce rôle.
    pub bandes: Vec<Bande>,
    /// Le réseau sonique (kNN), en coordonnées carte. Chemin fictif
    /// seulement — vide sur le plan de ville réel, où
    /// [`Source::troncons_reels`] tient ce rôle.
    pub routes: Vec<Route>,
    /// Les établissements du peuplement. **C'est ce qui donne l'allure d'une
    /// carte d'état-major** : six rangs, six symboles, six seuils de zoom.
    ///
    /// Chemin fictif seulement — vide sur le plan de ville réel.
    pub etablissements: Vec<crate::peuplement::Etablissement>,
    /// Les cours d'eau, du plus fort débit au plus faible.
    ///
    /// Chemin fictif seulement — vide sur le plan de ville réel, où
    /// [`Source::eaux`] (la Seine, réelle) tient ce rôle.
    pub rivieres: Vec<crate::hydro::Riviere>,
    /// Les points remarquables.
    pub curiosites: Vec<Curiosite>,
    /// Les rues réelles d'un plan de ville importé (`crates/osm`), peuplées
    /// par l'affectation (`crate::ville`). Vide sur le chemin fictif.
    pub troncons_reels: Vec<TronconReel>,
    /// Bâtiments, vide sur le chemin fictif.
    pub batiments: Vec<BatimentReel>,
    /// Plans d'eau (la Seine), vide sur le chemin fictif.
    pub eaux: Vec<ContourReel>,
    /// Espaces verts (bois, parcs), vide sur le chemin fictif.
    pub verts: Vec<ContourReel>,
    /// La limite communale — remplace le rôle du littoral sur le plan de
    /// ville réel. `None` sur le chemin fictif.
    pub frontiere: Option<Vec<Vec<[f64; 2]>>>,
    /// Musées, monuments, lieux de culte réels — vide sur le chemin fictif,
    /// où [`Source::curiosites`] tient ce rôle (mais depuis la bibliothèque,
    /// pas depuis OSM).
    pub points_remarquables: Vec<PointReel>,
    /// Les albums, échelon de révélation entre l'artiste et le morceau.
    /// Vide sur le chemin fictif — aucun équivalent n'y existe aujourd'hui.
    pub albums: Vec<AlbumReel>,
    /// Les artistes déjà posés sur leur rue par l'affectation
    /// (`ville::rassembler`) — position sur la voirie qui porte leur nom,
    /// plutôt qu'au barycentre de leurs morceaux logés ([`Source::artistes`]),
    /// qui après un repli d'étage 3 tombe dans un vide. Vide sur le chemin
    /// fictif : `tuiles` retombe alors sur [`Source::artistes`].
    pub artistes_places: Vec<Artiste>,
    /// Les quartiers musicaux comme aplats, pour la carte dézoomée. Vide sur
    /// le chemin fictif.
    pub territoires_reels: Vec<TerritoireReel>,
}

impl Source {
    /// `true` si cette source porte un vrai plan de ville plutôt que le monde
    /// engendré — c'est ce qui décide, dans `tuiles`/`style`, quel jeu de
    /// couches produire.
    pub fn est_ville_reelle(&self) -> bool {
        !self.troncons_reels.is_empty() || self.frontiere.is_some()
    }
}

impl Source {
    /// Les villes, dérivées des morceaux : un artiste, le barycentre de ses
    /// morceaux, son effectif.
    ///
    /// La famille retenue est celle où l'artiste a le plus de morceaux, et non
    /// celle de son barycentre : un artiste à cheval sur deux familles doit
    /// être rattaché à la plus fournie, pas à un milieu qui n'appartient à
    /// aucune des deux.
    pub fn artistes(&self) -> Vec<Artiste> {
        let mut cumul: HashMap<&str, CumulArtiste> = HashMap::new();
        for m in &self.morceaux {
            if m.artiste.is_empty() {
                continue;
            }
            let e = cumul.entry(m.artiste.as_str()).or_insert_with(|| {
                (0.0, 0.0, 0, HashMap::new())
            });
            e.0 += m.x as f64;
            e.1 += m.y as f64;
            e.2 += 1;
            *e.3.entry(m.famille).or_insert(0) += 1;
        }

        let mut villes: Vec<Artiste> = cumul
            .into_iter()
            .map(|(nom, (sx, sy, n, familles))| {
                let dominante = familles
                    .into_iter()
                    // À égalité, le plus petit identifiant : sinon l'ordre de
                    // parcours d'une table de hachage déciderait, et deux
                    // exécutions ne donneraient pas la même carte.
                    .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                    .map(|(f, _)| f)
                    .unwrap_or(-1);
                Artiste {
                    nom: nom.to_string(),
                    x: (sx / n as f64) as f32,
                    y: (sy / n as f64) as f32,
                    famille: dominante,
                    effectif: n,
                    ancre: None,
                }
            })
            .collect();
        villes.sort_by(|a, b| b.effectif.cmp(&a.effectif).then_with(|| a.nom.cmp(&b.nom)));
        villes
    }

    /// Où poser le nom d'une famille.
    ///
    /// Le barycentre serait le choix évident et il est faux : une famille en
    /// deux amas a son barycentre dans le vide entre les deux, et l'étiquette
    /// se pose alors sur le territoire du voisin. On prend donc le morceau le
    /// plus proche du barycentre — un médoïde approché, garanti d'être sur une
    /// terre peuplée.
    pub fn ancres_de_familles(&self) -> HashMap<i64, (f32, f32)> {
        let mut cumul: HashMap<i64, (f64, f64, usize)> = HashMap::new();
        for m in &self.morceaux {
            let e = cumul.entry(m.famille).or_insert((0.0, 0.0, 0));
            e.0 += m.x as f64;
            e.1 += m.y as f64;
            e.2 += 1;
        }
        let barycentres: HashMap<i64, (f32, f32)> = cumul
            .into_iter()
            .map(|(f, (sx, sy, n))| (f, ((sx / n as f64) as f32, (sy / n as f64) as f32)))
            .collect();

        let mut meilleur: HashMap<i64, (f32, (f32, f32))> = HashMap::new();
        for m in &self.morceaux {
            let Some(&(bx, by)) = barycentres.get(&m.famille) else {
                continue;
            };
            let d = (m.x - bx).powi(2) + (m.y - by).powi(2);
            let e = meilleur.entry(m.famille).or_insert((f32::MAX, (m.x, m.y)));
            if d < e.0 {
                *e = (d, (m.x, m.y));
            }
        }
        meilleur.into_iter().map(|(f, (_, p))| (f, p)).collect()
    }
}

/// Agrège le réseau sonore en un réseau **entre lieux**.
///
/// Les arêtes viennent du son : elles relient deux morceaux. Depuis que les
/// morceaux habitent leurs établissements, une arête va d'un lieu à un autre —
/// et c'est le lieu, non le morceau, qui a une place sur la carte. On regroupe
/// donc par couple d'établissements : une route, sa classe, et le nombre de
/// liens qu'elle porte.
///
/// **Le trafic fait le rang.** Un couloir emprunté cent cinquante fois est une
/// autoroute quelle que soit la classe de chaque brin ; c'est ce qui donne un
/// réseau hiérarchisé plutôt qu'un semis d'arêtes.
///
/// `champ` sert à faire épouser aux routes la ligne de crête, faute de quoi
/// elles rayonnent en étoile depuis chaque agglomération.
pub fn reseau_entre_lieux(
    aretes: &[(i64, i64, u8)],
    etablissement_de: &HashMap<i64, u32>,
    centres: &HashMap<u32, (f32, f32)>,
    champ: &[f64],
    gn: usize,
) -> Vec<Route> {
    /// Au-delà, un couloir est une autoroute ; puis une nationale.
    const SEUIL_AUTOROUTE: u32 = 150;
    const SEUIL_NATIONALE: u32 = 50;
    /// Détour maximal, en unités de carte.
    const AMPLITUDE: f32 = 0.035;

    let mut couloirs: HashMap<(u32, u32), (u8, u32)> = HashMap::new();
    for &(a, b, classe) in aretes {
        let (Some(&ea), Some(&eb)) = (etablissement_de.get(&a), etablissement_de.get(&b)) else {
            continue;
        };
        if ea == eb {
            continue; // un lien interne ne sort pas de la ville
        }
        let e = couloirs.entry((ea.min(eb), ea.max(eb))).or_insert((classe, 0));
        e.0 = e.0.min(classe);
        e.1 += 1;
    }

    let mut sortie: Vec<((u32, u32), Route)> = couloirs
        .iter()
        .filter_map(|(&(a, b), &(classe, liens))| {
            let (&(ax, ay), &(bx, by)) = (centres.get(&a)?, centres.get(&b)?);
            let classe = if liens >= SEUIL_AUTOROUTE {
                0
            } else if liens >= SEUIL_NATIONALE {
                classe.min(1)
            } else {
                classe.max(2)
            };
            Some((
                (a, b),
                Route {
                    points: crate::relief::epouser_le_relief(
                        [ax, ay],
                        [bx, by],
                        champ,
                        gn,
                        AMPLITUDE,
                    ),
                    classe,
                },
            ))
        })
        .collect();
    // Ordre stable : deux exécutions doivent rendre les mêmes tuiles.
    sortie.sort_by_key(|(cle, _)| *cle);
    sortie.into_iter().map(|(_, r)| r).collect()
}


/// Choisit les points remarquables d'une carte.
///
/// Trois espèces, et rien de plus : une carte couverte de symboles ne signale
/// plus rien. Le nombre est borné par espèce, les plus notables d'abord.
pub fn curiosites(
    morceaux: &[Morceau],
    etablissements: &[crate::peuplement::Etablissement],
    refuges: &[i64],
    par_espece: usize,
) -> Vec<Curiosite> {
    use std::collections::HashMap;
    let par_id: HashMap<i64, &Morceau> = morceaux.iter().map(|m| (m.id, m)).collect();
    let mut sortie = Vec::new();

    // Les monuments : le plus ancien morceau de chaque famille. C'est par lui
    // que le territoire a commencé.
    let mut plus_ancien: HashMap<i64, &Morceau> = HashMap::new();
    for m in morceaux {
        let Some(a) = m.annee else { continue };
        let e = plus_ancien.entry(m.famille).or_insert(m);
        if a < e.annee.unwrap_or(i32::MAX) {
            *e = m;
        }
    }
    let mut monuments: Vec<&Morceau> = plus_ancien.into_values().collect();
    monuments.sort_by_key(|m| (m.annee.unwrap_or(i32::MAX), m.id));
    for m in monuments.into_iter().take(par_espece) {
        sortie.push(Curiosite {
            x: m.x,
            y: m.y,
            nom: format!("{} — {}", m.artiste, m.titre),
            espece: Espece::Monument,
            annee: m.annee,
        });
    }

    // Les refuges : rien ne leur ressemble. On les signale parce que ce sont
    // eux qu'on ne trouverait jamais autrement.
    for id in refuges.iter().take(par_espece) {
        if let Some(m) = par_id.get(id) {
            sortie.push(Curiosite {
                x: m.x,
                y: m.y,
                nom: format!("{} — {}", m.artiste, m.titre),
                espece: Espece::Refuge,
                annee: m.annee,
            });
        }
    }

    // Les fondations des métropoles : d'où c'est parti.
    let mut grandes: Vec<&crate::peuplement::Etablissement> = etablissements
        .iter()
        .filter(|e| {
            crate::peuplement::Rang::depuis_population(e.population)
                == crate::peuplement::Rang::Metropole
        })
        .collect();
    grandes.sort_by_key(|e| (e.fondation_date, e.id));
    for e in grandes.into_iter().take(par_espece) {
        sortie.push(Curiosite {
            x: e.cx,
            y: e.cy,
            nom: e.nom.clone(),
            espece: Espece::Fondation,
            annee: Some((e.fondation_date / 10_000) as i32),
        });
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn morceau(id: i64, x: f32, y: f32, famille: i64, artiste: &str) -> Morceau {
        Morceau {
            id,
            x,
            y,
            famille,
            titre: format!("titre {id}"),
            artiste: artiste.to_string(),
            annee: Some(2000),
            bpm: None,
            energie: None,
        }
    }

    #[test]
    fn un_artiste_se_pose_au_barycentre_de_ses_morceaux() {
        let s = Source {
            morceaux: vec![
                morceau(1, 0.0, 0.0, 0, "A"),
                morceau(2, 1.0, 1.0, 0, "A"),
                morceau(3, -0.5, 0.0, 1, "B"),
            ],
            ..Default::default()
        };
        let villes = s.artistes();
        assert_eq!(villes.len(), 2);
        let a = villes.iter().find(|v| v.nom == "A").unwrap();
        assert!((a.x - 0.5).abs() < 1e-6 && (a.y - 0.5).abs() < 1e-6);
        assert_eq!(a.effectif, 2);
        // Classement par effectif décroissant : A d'abord.
        assert_eq!(villes[0].nom, "A");
    }

    /// Un artiste à cheval doit suivre la famille où il pèse le plus, pas
    /// celle qui tombe sous son barycentre.
    #[test]
    fn un_artiste_a_cheval_suit_sa_famille_dominante() {
        let s = Source {
            morceaux: vec![
                morceau(1, 0.0, 0.0, 7, "A"),
                morceau(2, 0.1, 0.0, 7, "A"),
                morceau(3, 5.0, 0.0, 3, "A"),
            ],
            ..Default::default()
        };
        assert_eq!(s.artistes()[0].famille, 7);
    }

    /// Le cas qui condamne le barycentre : deux amas, un vide au milieu.
    /// L'ancre doit tomber sur un morceau, jamais dans le vide.
    #[test]
    fn lancre_dune_famille_bimodale_reste_sur_du_peuple() {
        let mut morceaux = Vec::new();
        for i in 0..10 {
            morceaux.push(morceau(i, -0.8 + (i as f32) * 0.001, 0.0, 0, "gauche"));
            morceaux.push(morceau(100 + i, 0.8 + (i as f32) * 0.001, 0.0, 0, "droite"));
        }
        let s = Source { morceaux: morceaux.clone(), ..Default::default() };
        let (ax, _) = s.ancres_de_familles()[&0];
        assert!(
            ax.abs() > 0.5,
            "l'ancre est tombée dans le vide entre les deux amas : {ax}"
        );
        assert!(
            morceaux.iter().any(|m| (m.x - ax).abs() < 1e-6),
            "l'ancre doit coïncider avec un morceau réel"
        );
    }
}
