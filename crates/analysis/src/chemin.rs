//! Chemins dans la bibliothèque : quatre façons de fabriquer un trajet.
//!
//! | mode | ce qu'on fournit | ce qu'on obtient |
//! |---|---|---|
//! | [`direct`] | deux morceaux | la droite entre eux, sur la carte |
//! | [`Graphe::sonique`] | deux morceaux | le trajet sans à-coup, de voisin en voisin |
//! | [`Graphe::errance`] | un morceau | une promenade au hasard qui dérive |
//! | [`dessine`] | un tracé à la souris | les morceaux sous le trait |
//!
//! **Direct et sonique ne sont pas deux réglages du même calcul.** Le direct
//! tire une droite à l'écran et cueille à chaque pas le morceau le plus proche
//! du point visé : il va d'un bout à l'autre sans détour visible. Le sonique
//! cherche le plus court chemin dans le graphe des *k* plus proches voisins :
//! chaque saut est par construction une transition entre proches, le trajet
//! est plus long mais ne surprend jamais l'oreille — lisse à l'oreille, pas
//! forcément à l'écran, où t-SNE ne préserve que les voisinages locaux : un
//! nom antérieur, « lisse », prêtait à confusion sur la carte pour cette
//! raison précise.
//!
//! **Deux modes raisonnent en coordonnées de carte, [`direct`] et [`dessine`],
//! et pour la même raison : l'utilisateur y désigne un geste à l'écran.** Les
//! deux autres calculent dans l'espace des empreintes, où les distances sont
//! celles du son. Le direct a d'abord été écrit ainsi, par interpolation
//! sphérique entre les deux empreintes ; le trajet était juste, mais il
//! zigzaguait sur la carte — une droite dans l'espace des empreintes n'en est
//! plus une après t-SNE — et un mode nommé « direct » qui serpente ne tient
//! pas sa promesse. Le geste l'a emporté sur le calcul : c'est un outil de
//! pointage, pas une mesure.

use crate::alea::Alea;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Un morceau et son empreinte. Les deux voyagent toujours ensemble : les
/// fonctions d'ici rendent des identifiants, jamais des indices de tableau.
pub type Empreinte = (i64, Vec<f32>);

/// Nombre de voisins par morceau dans le graphe. Assez pour que le graphe
/// reste connexe sur une bibliothèque réelle, assez peu pour qu'un saut reste
/// une transition entre proches.
pub const K_VOISINS: usize = 12;

/// Les quatre modes partagent un seul cadran `bruit` ∈ [0, 1] : 0 reproduit
/// le trajet exact (déjà le cas pour direct/sonique/dessiné avant ce
/// paramètre), plus haut dérive sans perdre le fil sonore. Chaque mode
/// traduit ce cadran dans son propre registre — ces constantes fixent
/// l'échelle de chacun. Choisies à l'œil/à l'oreille, pas dérivées : à
/// ajuster comme le reste des constantes de ce fichier.
///
/// Le principe vient du cadre académique des « Randomized Shortest Paths »
/// (Saerens, Yen, Achbany, Fouss, 2009), qui interpole plus court chemin et
/// marche aléatoire via une température ; on en retient l'approximation
/// pratique employée dans le routage GPS pour diversifier des itinéraires —
/// bruiter les arêtes puis lancer un Dijkstra ordinaire — plutôt que la
/// machinerie complète (inversion de matrice sur tout le graphe), hors de
/// portée d'un curseur temps réel sur 27 000 nœuds.
const TEMPERATURE_ECHELLE: f32 = 3.0; // errance : bruit [0,1] → température [0,3]
const FACTEUR_BRUIT_ARETE: f32 = 0.6; // sonique : swing multiplicatif max par arête
const FACTEUR_BRUIT_DIRECT: f32 = 0.9; // direct : écart-type max au milieu du pont, en fraction du pas d'interpolation

/// Distance au carré — la racine ne change pas l'ordre, autant l'éviter.
fn distance2(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Bruit déterministe d'une arête `(i, j)`, uniforme dans [-1, 1] —
/// indépendant de l'ordre dans lequel Dijkstra la rencontre, contrairement à
/// un tirage puisé dans une suite séquentielle. Un simple mélange, pas un
/// hachage cryptographique : il n'a qu'à décorréler `i`, `j` et `graine`.
fn bruit_arete(graine: u64, i: u32, j: u32) -> f32 {
    let m = graine
        ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (j as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    Alea::depuis(m).reel() * 2.0 - 1.0
}

/// Décale un point visé d'un bruit gaussien 2D — même mécanisme pour
/// [`direct`] et [`dessine`], les deux modes qui raisonnent sur la carte ;
/// seule l'enveloppe (comment `ecart_type` varie le long du trajet) diffère
/// entre les deux, décidée par l'appelant.
fn bruiter(cx: f32, cy: f32, ecart_type: f32, alea: &mut Alea) -> (f32, f32) {
    if ecart_type <= 0.0 {
        return (cx, cy);
    }
    (cx + alea.normale() * ecart_type, cy + alea.normale() * ecart_type)
}

/// Les `k` morceaux les plus proches d'un morceau donné.
///
/// Balayage complet : 5 ms mesurées sur 27 044 vecteurs de 512 dimensions.
/// Un index approché serait de la complexité gratuite à cette échelle.
pub fn voisins(empreintes: &[Empreinte], id: i64, k: usize) -> Vec<i64> {
    let Some(cible) = empreintes.iter().find(|(i, _)| *i == id).map(|(_, v)| v) else {
        return Vec::new();
    };

    let mut classes: Vec<(i64, f32)> = empreintes
        .iter()
        .filter(|(i, _)| *i != id)
        .map(|(i, v)| (*i, distance2(v, cible)))
        .collect();
    // Tri partiel : on ne veut que les k premiers, pas l'ordre complet.
    let k = k.min(classes.len());
    if k == 0 {
        return Vec::new();
    }
    classes.select_nth_unstable_by(k - 1, |a, b| {
        a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
    });
    classes.truncate(k);
    classes.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    classes.into_iter().map(|(i, _)| i).collect()
}

/// Trace un chemin de `depart` à `arrivee`, en `etapes` morceaux au total.
///
/// **Une droite sur la carte.** On échantillonne le segment qui joint les deux
/// points à l'écran, et chaque échantillon cueille le morceau non encore pris
/// le plus proche. Le résultat commence par `depart`, finit par `arrivee`, et
/// ne répète jamais un morceau.
///
/// Deux différences avec [`dessine`], qui parcourt aussi la carte :
///
/// - **aucun rayon de cueillette.** Le trait dessiné est une intention
///   précise : ce qu'il traverse à vide doit rester vide. Ici l'utilisateur a
///   désigné deux morceaux et veut aller de l'un à l'autre — chaque pas doit
///   rendre quelque chose, quitte à cueillir au bord d'une région déserte ;
/// - **les deux extrémités sont garanties.** Ce sont les morceaux choisis, pas
///   le produit d'un geste.
///
/// `bruit` décale chaque point visé intermédiaire d'un **pont brownien** :
/// écart-type nul aux deux bouts, maximal au milieu (`FACTEUR_BRUIT_DIRECT`
/// fois le pas d'interpolation à `bruit = 1`) — les extrémités restent
/// exactement les morceaux cliqués, seul le trajet entre les deux ondule.
/// `bruit = 0` retrouve exactement la droite d'origine. L'écart-type est
/// relatif au **pas** (`distance(départ, arrivée) / (étapes - 1)`), pas à un
/// repère absolu de la carte : la densité de points en t-SNE varie énormément
/// d'un amas à l'autre, une constante absolue rendait donc le même curseur
/// tantôt invisible, tantôt disproportionné selon l'endroit de la carte.
pub fn direct(
    points: &[(i64, f32, f32)],
    depart: i64,
    arrivee: i64,
    etapes: usize,
    graine: u64,
    bruit: f32,
) -> Vec<i64> {
    let ou = |id: i64| {
        points
            .iter()
            .find(|(i, _, _)| *i == id)
            .map(|(_, x, y)| (*x, *y))
    };
    let (Some((ax, ay)), Some((bx, by))) = (ou(depart), ou(arrivee)) else {
        return Vec::new();
    };
    if depart == arrivee {
        return vec![depart];
    }
    let bruit = bruit.clamp(0.0, 1.0);
    let mut alea = Alea::depuis(graine);

    let etapes = etapes.max(2);
    // Le pas naturel de l'interpolation — pas la largeur de la carte. Une
    // trajectoire entre deux morceaux voisins doit onduler sur une échelle
    // bien plus fine qu'une trajectoire entre deux morceaux aux deux bouts
    // du nuage : sans ça, `bruit = 0.05` fait déjà sortir le trajet de son
    // quartier dans un amas dense.
    let pas = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt() / (etapes - 1) as f32;
    let mut route = vec![depart];
    let mut pris: HashSet<i64> = HashSet::from([depart, arrivee]);

    for i in 1..etapes - 1 {
        let t = i as f32 / (etapes - 1) as f32;
        let (cx, cy) = (ax + (bx - ax) * t, ay + (by - ay) * t);
        // 0 aux deux bouts, 1 au milieu — le pont brownien reste pincé aux
        // extrémités, qui ne passent d'ailleurs jamais par ce calcul (la
        // boucle exclut `i = 0` et `i = etapes - 1`).
        let enveloppe = 2.0 * (t * (1.0 - t)).sqrt();
        let (cx, cy) = bruiter(cx, cy, bruit * FACTEUR_BRUIT_DIRECT * pas * enveloppe, &mut alea);
        let meilleur = points
            .iter()
            .filter(|(id, _, _)| !pris.contains(id))
            .map(|(id, x, y)| (*id, (x - cx).powi(2) + (y - cy).powi(2)))
            .min_by(|u, v| u.1.partial_cmp(&v.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((id, _)) = meilleur {
            pris.insert(id);
            route.push(id);
        }
    }

    route.push(arrivee);
    route
}

/// Chemin suivant un tracé dessiné sur la carte.
///
/// Avec [`direct`], le mode qui raisonne en coordonnées de carte, et pour une
/// raison simple : l'utilisateur pointe le dessin, pas l'espace des
/// empreintes. Le tracé est rééchantillonné à pas d'arc constant — sans quoi
/// les portions dessinées lentement, où les points de la souris s'accumulent,
/// pèseraient plus que les autres.
///
/// `rayon` borne la cueillette : au-delà, l'échantillon ne rend rien plutôt
/// que d'aller chercher un morceau à l'autre bout de la carte. Un trait qui
/// traverse le vide produit donc un trou, pas une surprise.
///
/// `bruit` décale chaque point rééchantillonné du même bruit gaussien que
/// [`direct`] (voir `bruiter`), mais à écart-type **constant** — pas de pont
/// pincé ici, le tracé n'a pas d'extrémités à préserver au sens où `direct`
/// en a. L'échelle suit `rayon`, pas une constante absolue : elle reste donc
/// cohérente avec le zoom courant, dont `rayon` dépend déjà côté appelant.
/// `bruit = 0` retrouve exactement la cueillette d'origine.
pub fn dessine(
    points: &[(i64, f32, f32)],
    trace: &[(f32, f32)],
    etapes: usize,
    rayon: f32,
    graine: u64,
    bruit: f32,
) -> Vec<i64> {
    if points.is_empty() || trace.len() < 2 || etapes == 0 {
        return Vec::new();
    }
    let bruit = bruit.clamp(0.0, 1.0);
    let mut alea = Alea::depuis(graine);

    // Longueurs cumulées du tracé.
    let mut cumul = Vec::with_capacity(trace.len());
    cumul.push(0.0f32);
    for f in trace.windows(2) {
        let d = ((f[1].0 - f[0].0).powi(2) + (f[1].1 - f[0].1).powi(2)).sqrt();
        cumul.push(cumul.last().unwrap() + d);
    }
    let total = *cumul.last().unwrap();
    if total <= f32::EPSILON {
        return Vec::new();
    }

    let rayon2 = rayon * rayon;
    let mut route: Vec<i64> = Vec::new();
    let mut segment = 0usize;

    for i in 0..etapes {
        let vise = total * i as f32 / (etapes.max(2) - 1) as f32;
        while segment + 2 < cumul.len() && cumul[segment + 1] < vise {
            segment += 1;
        }
        let (a, b) = (trace[segment], trace[segment + 1]);
        let portee = cumul[segment + 1] - cumul[segment];
        let t = if portee > f32::EPSILON {
            ((vise - cumul[segment]) / portee).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cy) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let (cx, cy) = bruiter(cx, cy, bruit * rayon * 0.8, &mut alea);

        let meilleur = points
            .iter()
            .map(|(id, x, y)| (*id, (x - cx).powi(2) + (y - cy).powi(2)))
            .filter(|(id, d2)| *d2 <= rayon2 && !route.contains(id))
            .min_by(|u, v| u.1.partial_cmp(&v.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((id, _)) = meilleur {
            route.push(id);
        }
    }
    route
}

/// Clé de tas ordonnée sur un flottant. Les distances sont finies et
/// positives : `partial_cmp` ne rend jamais `None` ici.
///
/// `Ord` est inversé pour que `BinaryHeap`, qui est un tas-max, serve de
/// tas-min à Dijkstra.
#[derive(PartialEq)]
struct Cout(f32, u32);

impl Eq for Cout {}

impl Ord for Cout {
    fn cmp(&self, autre: &Self) -> std::cmp::Ordering {
        autre
            .0
            .partial_cmp(&self.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| autre.1.cmp(&self.1))
    }
}

impl PartialOrd for Cout {
    fn partial_cmp(&self, autre: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(autre))
    }
}

/// Graphe des *k* plus proches voisins, construit une fois puis réutilisé.
///
/// Le construire coûte un balayage complet ; s'en servir ne coûte plus rien.
/// Il porte les deux modes qui ont besoin d'un voisinage explicite — le
/// sonique et l'errance — et rend au passage `voisins` instantané.
pub struct Graphe {
    /// Rang → identifiant de morceau.
    ids: Vec<i64>,
    /// Identifiant → rang.
    rang: HashMap<i64, u32>,
    /// Par rang : les voisins, du plus proche au plus lointain.
    aretes: Vec<Vec<(u32, f32)>>,
}

impl Graphe {
    /// Combien de morceaux le graphe couvre.
    pub fn taille(&self) -> usize {
        self.ids.len()
    }

    /// Le plus proche voisin de chaque morceau et la distance au carré qui
    /// les sépare — sert au repérage des quasi-doublons (distance proche de
    /// zéro) et des morceaux isolés (mode Bibliothèque).
    ///
    /// Retrouve le minimum plutôt que de lire `aretes[r][0]` : les arcs
    /// retour, ajoutés après coup pour la connexité, ne respectent pas le tri
    /// par distance croissante que `construire` établit au départ — même
    /// piège que documenté dans [`Self::voisins`].
    pub fn plus_proches(&self) -> Vec<(i64, i64, f32)> {
        self.aretes
            .iter()
            .enumerate()
            .filter_map(|(rang, v)| {
                v.iter()
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|&(voisin, d2)| (self.ids[rang], self.ids[voisin as usize], d2))
            })
            .collect()
    }

    /// Rang → identifiant de morceau.
    pub fn identifiants(&self) -> &[i64] {
        &self.ids
    }

    /// Le rang d'un morceau, s'il est dans le graphe.
    pub fn rang_de(&self, id: i64) -> Option<u32> {
        self.rang.get(&id).copied()
    }

    /// Le voisinage d'un rang, tel qu'il est rangé — arcs retour compris, donc
    /// pas trié par distance croissante (voir [`Self::voisins`]).
    pub fn voisinage(&self, rang: u32) -> &[(u32, f32)] {
        self.aretes
            .get(rang as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Les arêtes vues comme non orientées : une seule fois par paire.
    ///
    /// Le voisinage des *k* plus proches n'est pas symétrique, et
    /// [`Self::construire`] ajoute des arcs retour pour la connexité : sans
    /// dédoublonnage, une paire compterait deux fois dans la centralité.
    pub fn aretes_uniques(&self) -> Vec<(u32, u32)> {
        let mut vues = std::collections::HashSet::new();
        let mut sortie = Vec::new();
        for (i, voisins) in self.aretes.iter().enumerate() {
            for &(j, _) in voisins {
                let paire = if (i as u32) < j {
                    (i as u32, j)
                } else {
                    (j, i as u32)
                };
                if paire.0 != paire.1 && vues.insert(paire) {
                    sortie.push(paire);
                }
            }
        }
        sortie
    }

    /// Construit le graphe sur `fils` fils.
    ///
    /// Balayage complet : n² distances. Sur 27 044 empreintes de 512
    /// dimensions cela fait 366 millions de paires — mesuré ci-dessous dans le
    /// README. Un index approché (HNSW) diviserait ce coût mais ajouterait une
    /// dépendance et une approximation pour un calcul fait une fois par
    /// session.
    pub fn construire(empreintes: &[Empreinte], k: usize, fils: usize) -> Self {
        Self::construire_suivi(empreintes, k, fils, &AtomicUsize::new(0))
    }

    /// Comme [`Self::construire`], en publiant l'avancement du balayage dans
    /// `fait` — une empreinte traitée par incrément, sur `empreintes.len()`.
    ///
    /// L'appli le lit d'un autre fil pour afficher une jauge pendant la
    /// vingtaine de secondes que coûte la première errance d'une session ; la
    /// CLI et les tests passent par [`Self::construire`], qui l'ignore.
    pub fn construire_suivi(
        empreintes: &[Empreinte],
        k: usize,
        fils: usize,
        fait: &AtomicUsize,
    ) -> Self {
        let n = empreintes.len();
        let ids: Vec<i64> = empreintes.iter().map(|(i, _)| *i).collect();
        let rang: HashMap<i64, u32> = ids
            .iter()
            .enumerate()
            .map(|(r, i)| (*i, r as u32))
            .collect();
        let mut aretes: Vec<Vec<(u32, f32)>> = vec![Vec::new(); n];
        if n < 2 {
            return Graphe { ids, rang, aretes };
        }
        let k = k.clamp(1, n - 1);

        // Découpage statique : chaque ligne coûte exactement le même balayage,
        // un curseur atomique pour distribuer le travail n'apporterait rien.
        // `fait`, lui, ne distribue rien : il ne fait que compter les lignes
        // traversées pour la jauge de l'appelant.
        let fils = fils.max(1);
        let taille = n.div_ceil(fils);
        std::thread::scope(|portee| {
            for (bloc, part) in aretes.chunks_mut(taille).enumerate() {
                let base = bloc * taille;
                portee.spawn(move || {
                    // Tampon réutilisé d'une ligne à l'autre : 27 044 paires
                    // font 216 Ko, les réallouer 27 044 fois serait absurde.
                    let mut tampon: Vec<(u32, f32)> = Vec::with_capacity(n);
                    for (decalage, sortie) in part.iter_mut().enumerate() {
                        let i = base + decalage;
                        let vi = &empreintes[i].1;
                        tampon.clear();
                        for (j, (_, vj)) in empreintes.iter().enumerate() {
                            if j != i {
                                tampon.push((j as u32, distance2(vi, vj)));
                            }
                        }
                        tampon.select_nth_unstable_by(k - 1, |a, b| {
                            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        tampon.truncate(k);
                        tampon.sort_by(|a, b| {
                            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        *sortie = tampon.clone();
                        fait.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });

        // Le voisinage n'est pas symétrique : B peut compter parmi les k plus
        // proches de A sans que l'inverse soit vrai. Sans les arcs retour,
        // Dijkstra buterait sur des culs-de-sac que rien ne justifie, et un
        // morceau isolé n'aurait aucune arête entrante.
        let retours: Vec<(u32, u32, f32)> = aretes
            .iter()
            .enumerate()
            .flat_map(|(i, v)| v.iter().map(move |(j, d)| (*j, i as u32, *d)))
            .collect();
        for (de, vers, d) in retours {
            let liste = &mut aretes[de as usize];
            if !liste.iter().any(|(j, _)| *j == vers) {
                liste.push((vers, d));
            }
        }

        Graphe { ids, rang, aretes }
    }

    /// Un sous-graphe limité aux morceaux `permis`.
    ///
    /// Mêmes arêtes, mais seules celles dont les deux extrémités sont permises
    /// subsistent. Sert au filtre par famille du mode Explorer : un chemin
    /// sonique ou une errance qui ne doit traverser qu'une seule famille se
    /// calcule sur `graphe.restreint(&famille)`, sans reconstruire le graphe
    /// complet (aucune distance n'est recalculée, juste l'adjacence filtrée).
    ///
    /// Le sous-graphe peut être disjoint là où le graphe complet était
    /// connexe : une famille dont deux amas ne communiquaient que par un
    /// morceau d'une autre famille. `sonique` rend alors vide et l'appelant
    /// retombe sur le mode direct, lui aussi filtré.
    pub fn restreint(&self, permis: &HashSet<i64>) -> Graphe {
        let ids: Vec<i64> = self.ids.iter().copied().filter(|id| permis.contains(id)).collect();
        let rang: HashMap<i64, u32> =
            ids.iter().enumerate().map(|(r, i)| (*i, r as u32)).collect();
        let aretes: Vec<Vec<(u32, f32)>> = ids
            .iter()
            .map(|id| {
                let ancien = self.rang[id] as usize;
                self.aretes[ancien]
                    .iter()
                    .filter_map(|(j, d)| rang.get(&self.ids[*j as usize]).map(|nr| (*nr, *d)))
                    .collect()
            })
            .collect();
        Graphe { ids, rang, aretes }
    }

    /// Les `k` voisins d'un morceau, du plus proche au plus lointain.
    ///
    /// Le graphe stocke les siens déjà triés : la réponse est immédiate. Au
    /// delà de son propre `k`, on rend ce qu'on a.
    pub fn voisins(&self, id: i64, k: usize) -> Vec<i64> {
        let Some(&r) = self.rang.get(&id) else {
            return Vec::new();
        };
        // Les arcs retour sont ajoutés en fin de liste, hors tri : on retrie.
        let mut liste = self.aretes[r as usize].clone();
        liste.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        liste
            .into_iter()
            .take(k)
            .map(|(j, _)| self.ids[j as usize])
            .collect()
    }

    /// Plus court chemin de `depart` à `arrivee` dans le graphe des voisins.
    ///
    /// Rend le trajet complet, du départ à l'arrivée. Vide si les deux
    /// morceaux tombent dans deux composantes disjointes — le cas existe :
    /// avec `K_VOISINS` voisins, un petit amas très à part peut n'avoir aucune
    /// arête vers le reste. L'appelant retombe alors sur le mode direct.
    ///
    /// `bruit` (voir `FACTEUR_BRUIT_ARETE`) perturbe la distance de chaque
    /// arête d'un facteur multiplicatif, avant de lancer un Dijkstra
    /// ordinaire — l'approximation practique des « Randomized Shortest
    /// Paths » retenue pour ce fichier (voir la note en tête de fichier), et
    /// une technique documentée du routage GPS pour diversifier des
    /// itinéraires. Le bruit d'une arête est **dérivé d'un hachage de la
    /// paire de rangs et de la graine** (`bruit_arete`), pas d'un tirage
    /// séquentiel : Dijkstra peut tester la même arête plusieurs fois, à des
    /// instants différents selon l'état du tas, et un tirage séquentiel y
    /// aurait donné une valeur différente à chaque fois — la même graine ne
    /// redonnerait alors pas le même trajet. `bruit = 0` laisse chaque
    /// facteur à 1 et retrouve exactement le plus court chemin d'origine.
    pub fn sonique(&self, depart: i64, arrivee: i64, graine: u64, bruit: f32) -> Vec<i64> {
        let (Some(&a), Some(&b)) = (self.rang.get(&depart), self.rang.get(&arrivee)) else {
            return Vec::new();
        };
        if a == b {
            return vec![depart];
        }
        let bruit = bruit.clamp(0.0, 1.0);

        let mut cout = vec![f32::INFINITY; self.ids.len()];
        let mut parent = vec![u32::MAX; self.ids.len()];
        let mut tas = BinaryHeap::new();
        cout[a as usize] = 0.0;
        tas.push(Cout(0.0, a));

        while let Some(Cout(d, r)) = tas.pop() {
            if r == b {
                break;
            }
            // Entrée périmée : un chemin plus court a été trouvé depuis.
            if d > cout[r as usize] {
                continue;
            }
            for (j, poids) in &self.aretes[r as usize] {
                // La racine ici : additionner des carrés donnerait un chemin
                // qui préfère un grand saut à deux petits, l'inverse de ce
                // qu'on cherche.
                let facteur = if bruit > 0.0 {
                    (1.0 + bruit * FACTEUR_BRUIT_ARETE * bruit_arete(graine, r, *j)).max(0.1)
                } else {
                    1.0
                };
                let suivant = d + poids.sqrt() * facteur;
                if suivant < cout[*j as usize] {
                    cout[*j as usize] = suivant;
                    parent[*j as usize] = r;
                    tas.push(Cout(suivant, *j));
                }
            }
        }

        if cout[b as usize].is_infinite() {
            return Vec::new();
        }
        let mut route = vec![b];
        while let Some(&p) = route.last() {
            if p == a {
                break;
            }
            route.push(parent[p as usize]);
        }
        route.reverse();
        route.into_iter().map(|r| self.ids[r as usize]).collect()
    }

    /// Errance sonique : marche aléatoire auto-évitante dans le graphe des
    /// voisins, pondérée par la proximité — plus un voisin est proche, plus
    /// il a de chances d'être tiré, sans que ce ne soit jamais une certitude.
    ///
    /// **Le principe vient de Song Alchemy, chez AudioMuse-AI** : au lieu
    /// d'un tirage uniforme parmi les voisins libres, chacun reçoit un poids
    /// `exp(-distance / (échelle locale × température))` — un softmax, comme
    /// leur `ALCHEMY_TEMPERATURE` sur un score de similarité. Basse, la
    /// température assèche la distribution et le tirage devient presque
    /// glouton (le plus proche presque à coup sûr) ; haute, elle l'aplatit.
    /// `bruit` est le cadran commun aux quatre modes de chemin, normalisé à
    /// [0, 1] — voir la constante `TEMPERATURE_ECHELLE` en tête de fichier
    /// pour son passage à la température du softmax. **Cas limite `bruit →
    /// 1` (température → 3) : le softmax s'aplatit, proche du tirage
    /// uniforme d'origine de cette fonction, qui ne pondérait pas du tout ;
    /// `bruit = 0` (température → 0⁺) est presque glouton — le plus proche
    /// voisin sort presque à coup sûr.** L'échelle locale (l'écart moyen des
    /// voisins libres au plus proche d'entre eux) rend la température
    /// relative à la densité du voisinage courant, pas à l'échelle absolue
    /// des distances CLAP — sans elle, un même bruit ferait un effet
    /// différent selon la région de la carte des empreintes.
    ///
    /// C'est l'auto-évitement, pas la pondération, qui produit la dérive :
    /// une marche qui s'autoriserait le retour tournerait en rond autour de
    /// son point de départ, pondérée ou non.
    ///
    /// La même graine et le même bruit redonnent la même promenade.
    /// S'arrête tôt si tous les voisins du morceau courant ont déjà été
    /// visités.
    pub fn errance(&self, depart: i64, pas: usize, graine: u64, bruit: f32) -> Vec<i64> {
        let Some(&debut) = self.rang.get(&depart) else {
            return Vec::new();
        };
        // `bruit = 0` doit rester tirable : plancher à 1e-3 plutôt que de
        // diviser par zéro. À cette borne, le tirage devient quasi glouton —
        // le cas limite naturel du softmax, pas un cas à part.
        let temperature = (bruit.clamp(0.0, 1.0) * TEMPERATURE_ECHELLE).max(1e-3);
        let mut alea = Alea::depuis(graine);
        let mut vus: HashSet<u32> = HashSet::from([debut]);
        let mut route = vec![debut];

        while route.len() < pas.max(1) {
            let courant = *route.last().unwrap();
            let libres: Vec<(u32, f32)> = self.aretes[courant as usize]
                .iter()
                .copied()
                .filter(|(j, _)| !vus.contains(j))
                .collect();
            if libres.is_empty() {
                break;
            }
            let d_min = libres
                .iter()
                .map(|&(_, d)| d)
                .fold(f32::INFINITY, f32::min);
            // Écart moyen au plus proche : l'« échelle » locale qui rend la
            // température comparable d'un voisinage dense à un voisinage
            // épars. Plancher à 1e-6 pour ne pas diviser par zéro quand tous
            // les voisins libres sont à la même distance.
            let echelle = (libres.iter().map(|&(_, d)| d - d_min).sum::<f32>()
                / libres.len() as f32)
                .max(1e-6);
            let poids: Vec<f32> = libres
                .iter()
                .map(|&(_, d)| (-(d - d_min) / (echelle * temperature)).exp())
                .collect();
            let choisi = libres[alea.categorique(&poids)].0;
            vus.insert(choisi);
            route.push(choisi);
        }
        route.into_iter().map(|r| self.ids[r as usize]).collect()
    }
}

/// Ordonne un ensemble de morceaux en un parcours de proche en proche.
///
/// Sert à la sélection au lasso : une zone de la carte donne des dizaines de
/// morceaux, et les enchaîner dans l'ordre où la base les rend produirait une
/// playlist qui saute d'un bout à l'autre de la zone. On part du morceau le
/// plus central — celui dont l'empreinte est la plus proche de la moyenne —
/// puis on prend à chaque fois le plus proche non encore pris.
///
/// Ce n'est pas le parcours optimal : le trouver serait un voyageur de
/// commerce. C'est le glouton, qui suffit à ce qu'aucune transition ne soit
/// brutale sans coûter davantage qu'un balayage par morceau.
pub fn parcours(empreintes: &[Empreinte], ids: &[i64]) -> Vec<i64> {
    let choisis: Vec<&Empreinte> = ids
        .iter()
        .filter_map(|id| empreintes.iter().find(|(i, _)| i == id))
        .collect();
    if choisis.len() < 2 {
        return choisis.iter().map(|(i, _)| *i).collect();
    }

    // Centre de la sélection, puis le morceau qui s'en approche le plus : un
    // départ pris au hasard donnerait un parcours qui commence par traverser
    // toute la zone.
    let dim = choisis[0].1.len();
    let mut centre = vec![0.0f32; dim];
    for (_, v) in &choisis {
        for (c, x) in centre.iter_mut().zip(v) {
            *c += x / choisis.len() as f32;
        }
    }

    let mut restants: Vec<usize> = (0..choisis.len()).collect();
    let depart = restants
        .iter()
        .copied()
        .min_by(|a, b| {
            distance2(&choisis[*a].1, &centre)
                .partial_cmp(&distance2(&choisis[*b].1, &centre))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("sélection non vide");
    restants.retain(|i| *i != depart);

    let mut route = vec![depart];
    while !restants.is_empty() {
        let courant = &choisis[*route.last().expect("route non vide")].1;
        let (rang, _) = restants
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                distance2(&choisis[**a].1, courant)
                    .partial_cmp(&distance2(&choisis[**b].1, courant))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("restants non vide");
        route.push(restants.remove(rang));
    }
    route.into_iter().map(|i| choisis[i].0).collect()
}

/// Réduit un trajet à au plus `max` morceaux, en gardant les extrémités.
///
/// Un plus court chemin entre deux amas éloignés peut compter des centaines de
/// sauts : utile comme trajet, ingérable comme file d'attente.
pub fn echantillonner(route: &[i64], max: usize) -> Vec<i64> {
    if route.len() <= max || max < 2 {
        return route.to_vec();
    }
    (0..max)
        .map(|i| route[i * (route.len() - 1) / (max - 1)])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Empreintes disposées le long d'un arc : le chemin de la première à la
    /// dernière doit les traverser dans l'ordre, sans sauter ni revenir.
    fn arc(n: usize) -> Vec<(i64, Vec<f32>)> {
        (0..n)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / (n - 1) as f32;
                (i as i64, vec![a.cos(), a.sin(), 0.0])
            })
            .collect()
    }

    /// Morceaux régulièrement posés sur la diagonale de la carte, plus des
    /// intrus loin de cette droite. Un mode qui promet une droite à l'écran ne
    /// doit ramasser que les premiers.
    fn diagonale(n: usize, intrus: usize) -> Vec<(i64, f32, f32)> {
        let mut pts: Vec<(i64, f32, f32)> = (0..n)
            .map(|i| {
                let t = -1.0 + 2.0 * i as f32 / (n - 1) as f32;
                (i as i64, t, t)
            })
            .collect();
        // Écartés de 0,7 de la diagonale, soit cinq fois le pas : jamais les
        // plus proches, sauf si le chemin s'égare.
        for k in 0..intrus {
            let t = -1.0 + 2.0 * k as f32 / intrus.max(2) as f32;
            pts.push((1000 + k as i64, t - 0.7, t + 0.7));
        }
        pts
    }

    #[test]
    fn le_chemin_direct_suit_la_droite_a_lecran() {
        let p = diagonale(21, 8);
        let route = direct(&p, 0, 20, 6, 1, 0.0);

        assert_eq!(route.first(), Some(&0), "doit partir du départ");
        assert_eq!(route.last(), Some(&20), "doit arriver à l'arrivée");
        assert_eq!(route.len(), 6);

        // Sur la diagonale, avancer vers l'arrivée veut dire des indices
        // croissants — et jamais un intrus, qui porte un identifiant ≥ 1000.
        for f in route.windows(2) {
            assert!(f[0] < f[1], "le chemin revient en arrière : {route:?}");
        }
        assert!(
            route.iter().all(|id| *id < 1000),
            "le chemin s'écarte de la droite : {route:?}"
        );
    }

    /// Le segment est échantillonné à pas constant : sur des morceaux
    /// régulièrement posés, les étapes doivent l'être aussi. C'est ce que
    /// l'ancienne version, qui interpolait entre les empreintes, ne pouvait
    /// pas garantir à l'écran — t-SNE ne conserve pas les distances.
    #[test]
    fn les_etapes_sont_regulierement_espacees() {
        let p = diagonale(41, 0);
        let route = direct(&p, 0, 40, 6, 1, 0.0);
        let ecarts: Vec<i64> = route.windows(2).map(|f| f[1] - f[0]).collect();
        let (mini, maxi) = (*ecarts.iter().min().unwrap(), *ecarts.iter().max().unwrap());
        assert!(
            maxi - mini <= 1,
            "pas irréguliers : {ecarts:?} (route {route:?})"
        );
    }

    #[test]
    fn aucun_morceau_en_double() {
        let p = diagonale(30, 4);
        let route = direct(&p, 0, 29, 12, 1, 0.0);
        let uniques: HashSet<_> = route.iter().collect();
        assert_eq!(uniques.len(), route.len(), "doublon dans {route:?}");
    }

    /// Le pont brownien reste pincé aux deux bouts, même à bruit maximal —
    /// ce sont les morceaux cliqués, pas le produit d'un tirage — et la
    /// même graine à bruit égal redonne le même trajet, deux graines
    /// distinctes des trajets différents.
    #[test]
    fn direct_bruite_est_reproductible_et_garde_ses_extremites() {
        let p = diagonale(41, 0);
        let a = direct(&p, 0, 40, 8, 42, 1.0);
        assert_eq!(a.first(), Some(&0), "le pont doit partir du départ");
        assert_eq!(a.last(), Some(&40), "le pont doit arriver à l'arrivée");
        assert_eq!(
            a,
            direct(&p, 0, 40, 8, 42, 1.0),
            "même graine, même bruit, même trajet"
        );
        assert_ne!(
            a,
            direct(&p, 0, 40, 8, 43, 1.0),
            "une autre graine doit dévier autrement"
        );
        assert_eq!(
            direct(&p, 0, 40, 8, 42, 0.0),
            direct(&p, 0, 40, 8, 1, 0.0),
            "bruit nul : la graine ne doit plus rien changer"
        );
    }

    #[test]
    fn les_voisins_sont_les_plus_proches_et_ordonnes() {
        let e = arc(21);
        // Le morceau 10 est au milieu : ses voisins sont 9 et 11, puis 8 et 12.
        let v = voisins(&e, 10, 4);
        assert_eq!(v.len(), 4);
        assert!(!v.contains(&10), "un morceau n'est pas son propre voisin");
        assert_eq!(
            v.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([9, 11, 8, 12]),
            "voisins inattendus : {v:?}"
        );
        // Et rendus du plus proche au plus lointain.
        assert!(v[0] == 9 || v[0] == 11, "le plus proche d'abord : {v:?}");
    }

    #[test]
    fn supporte_les_cas_degeneres() {
        let e = arc(5);
        let p = diagonale(5, 0);
        assert_eq!(direct(&p, 2, 2, 5, 1, 0.0), vec![2], "départ = arrivée");
        assert!(voisins(&e, 99, 3).is_empty(), "morceau inconnu");
        assert!(voisins(&e, 0, 0).is_empty(), "zéro voisin demandé");
        assert!(direct(&p, 0, 99, 5, 1, 0.0).is_empty(), "arrivée inconnue");
        assert!(direct(&[], 0, 1, 5, 1, 0.0).is_empty(), "carte vide");
        // Plus d'étapes que de morceaux : on rend ce qu'on peut, sans doublon.
        let route = direct(&p, 0, 4, 50, 1, 0.0);
        let uniques: HashSet<_> = route.iter().collect();
        assert_eq!(uniques.len(), route.len());
    }

    /* ------------------------------------------------------------ graphe */

    #[test]
    fn le_graphe_retrouve_les_memes_voisins_que_le_balayage() {
        let e = arc(40);
        let g = Graphe::construire(&e, 4, 3);
        assert_eq!(g.taille(), 40);
        for id in [0, 7, 20, 39] {
            assert_eq!(
                g.voisins(id, 3),
                voisins(&e, id, 3),
                "voisinage divergent pour {id}"
            );
        }
    }

    #[test]
    fn construire_suivi_compte_chaque_empreinte_une_fois() {
        let e = arc(40);
        let fait = AtomicUsize::new(0);
        let g = Graphe::construire_suivi(&e, 4, 3, &fait);
        // Le graphe est le même que sans suivi, et le compteur a vu passer
        // les 40 lignes — la jauge de l'appli s'appuie dessus.
        assert_eq!(g.taille(), 40);
        assert_eq!(fait.load(Ordering::Relaxed), 40);
    }

    #[test]
    fn plus_proches_couvre_tout_le_monde_et_trouve_le_vrai_minimum() {
        let mut e = arc(20);
        // Deux empreintes identiques : leur plus proche voisin mutuel doit
        // ressortir à distance nulle, quelle que soit sa position dans la
        // liste d'arêtes du rang — `plus_proches` doit chercher le minimum,
        // pas supposer un tri déjà là.
        e.push((100, e[5].1.clone()));
        let g = Graphe::construire(&e, 4, 3);
        let proches: HashMap<i64, (i64, f32)> = g
            .plus_proches()
            .into_iter()
            .map(|(id, voisin, d2)| (id, (voisin, d2)))
            .collect();
        assert_eq!(proches.len(), 21, "un plus proche par morceau du graphe");
        assert_eq!(proches[&100], (5, 0.0));
        assert_eq!(proches[&5].0, 100, "5 retrouve aussi son jumeau exact");
        assert_eq!(proches[&5].1, 0.0);
    }

    /// Chaque saut du chemin sonique doit être une arête du graphe : c'est la
    /// propriété qui le distingue du direct, et la seule qui compte.
    #[test]
    fn le_chemin_sonique_ne_saute_que_de_voisin_en_voisin() {
        let e = arc(60);
        let g = Graphe::construire(&e, 4, 4);
        let route = g.sonique(0, 59, 1, 0.0);

        assert_eq!(route.first(), Some(&0));
        assert_eq!(route.last(), Some(&59));
        for f in route.windows(2) {
            let proches = g.voisins(f[0], 8);
            assert!(
                proches.contains(&f[1]),
                "{} → {} n'est pas une arête du graphe",
                f[0],
                f[1]
            );
        }
    }

    /// Bruité, le sonique reste sur les arêtes du graphe — seul le coût
    /// change, jamais l'existence d'un saut — et redevient reproductible par
    /// graine, comme direct et errance.
    #[test]
    fn sonique_bruite_reste_sur_le_graphe_et_est_reproductible() {
        let e = arc(60);
        let g = Graphe::construire(&e, 4, 4);
        let a = g.sonique(0, 59, 42, 1.0);

        assert_eq!(a.first(), Some(&0));
        assert_eq!(a.last(), Some(&59));
        for f in a.windows(2) {
            assert!(
                g.voisins(f[0], 8).contains(&f[1]),
                "{} → {} n'est pas une arête du graphe, même bruité",
                f[0],
                f[1]
            );
        }
        assert_eq!(
            a,
            g.sonique(0, 59, 42, 1.0),
            "même graine, même bruit, même trajet"
        );
        assert_eq!(
            g.sonique(0, 59, 42, 0.0),
            g.sonique(0, 59, 1, 0.0),
            "bruit nul : la graine ne doit plus rien changer"
        );
    }

    /// Deux amas séparés par un vide, sans arête entre eux : le sonique doit
    /// le dire (route vide) plutôt que rendre un trajet impossible.
    #[test]
    fn le_sonique_rend_vide_entre_deux_composantes() {
        // Deux paquets diamétralement opposés sur la sphère.
        let mut e: Vec<(i64, Vec<f32>)> = Vec::new();
        for i in 0..8 {
            let d = i as f32 * 0.001;
            e.push((i, vec![1.0 - d, d, 0.0]));
            e.push((100 + i, vec![-1.0 + d, d, 0.0]));
        }
        let g = Graphe::construire(&e, 3, 2);
        // Chaque paquet a huit membres : trois voisins suffisent à rester
        // dedans, aucune arête ne franchit le vide.
        assert!(
            g.sonique(0, 100, 1, 0.0).is_empty(),
            "un chemin a été trouvé là où il n'y a pas d'arête"
        );
        assert!(
            !g.sonique(0, 7, 1, 0.0).is_empty(),
            "dans un même amas, il en faut un"
        );
    }

    #[test]
    fn lerrance_est_reproductible_et_sans_retour() {
        let e = arc(60);
        let g = Graphe::construire(&e, 5, 2);

        // bruit ≈ 1/3 : température ≈ 1,0 une fois mise à l'échelle — la
        // valeur sur laquelle ce test était calibré avant que `bruit` ne
        // remplace `temperature` comme paramètre public.
        let a = g.errance(30, 15, 42, 0.333);
        assert_eq!(a, g.errance(30, 15, 42, 0.333), "même graine, même promenade");
        assert_ne!(
            a,
            g.errance(30, 15, 43, 0.333),
            "graines distinctes, promenades distinctes"
        );

        assert_eq!(a.first(), Some(&30));
        assert_eq!(a.len(), 15);
        let uniques: HashSet<_> = a.iter().collect();
        assert_eq!(uniques.len(), a.len(), "la marche repasse : {a:?}");
        // Auto-évitante, elle s'éloigne : sur un arc, elle finit forcément
        // au-delà de ses cinq voisins immédiats.
        let ecart = a.iter().map(|x| (x - 30).abs()).max().unwrap();
        assert!(ecart > 5, "la marche n'a pas dérivé : {a:?}");
    }

    /// Preuve que la pondération a un effet mesurable, pas seulement déclaré
    /// en commentaire : à basse température, le premier pas de l'errance
    /// doit tomber sur le plus proche voisin bien plus souvent que le taux
    /// uniforme (1 sur 8 ici) que donnait l'ancien tirage.
    #[test]
    fn la_temperature_basse_favorise_le_plus_proche_voisin() {
        let e = arc(60);
        let g = Graphe::construire(&e, 8, 2);
        let depart = 30;
        let plus_proche = g.voisins(depart, 8)[0];

        let essais = 400;
        let mut fois_plus_proche = 0;
        for graine in 1..=essais {
            if g.errance(depart, 2, graine, 0.15).get(1) == Some(&plus_proche) {
                fois_plus_proche += 1;
            }
        }
        // Sous tirage uniforme (8 voisins), le taux attendu serait ~12,5 % ;
        // on demande nettement plus, sans coller à la valeur mesurée pour ne
        // pas figer un test sur un chiffre qui dépend du fixture.
        assert!(
            fois_plus_proche > essais / 4,
            "le plus proche voisin ne domine pas à basse température : \
             {fois_plus_proche}/{essais}"
        );
    }

    /// Le sous-graphe restreint ne garde que les morceaux permis, et un chemin
    /// sonique qui y court n'en traverse aucun autre.
    #[test]
    fn le_sous_graphe_restreint_ne_traverse_que_les_permis() {
        let e = arc(60);
        let g = Graphe::construire(&e, 6, 3);
        // Une famille : un morceau sur deux.
        let permis: HashSet<i64> = (0..60).filter(|i| i % 2 == 0).collect();
        let r = g.restreint(&permis);

        assert_eq!(r.taille(), 30);
        let route = r.sonique(0, 58, 1, 0.0);
        assert_eq!(route.first(), Some(&0));
        assert_eq!(route.last(), Some(&58));
        assert!(
            route.iter().all(|id| permis.contains(id)),
            "le chemin sort de la famille : {route:?}"
        );

        let promenade = r.errance(0, 10, 7, 0.3);
        assert!(
            promenade.iter().all(|id| permis.contains(id)),
            "l'errance sort de la famille : {promenade:?}"
        );
    }

    #[test]
    fn le_graphe_supporte_les_cas_degeneres() {
        let g = Graphe::construire(&[], 4, 2);
        assert_eq!(g.taille(), 0);
        assert!(g.sonique(0, 1, 1, 0.0).is_empty());
        assert!(g.errance(0, 5, 1, 1.0).is_empty());

        let un = Graphe::construire(&arc(1), 4, 2);
        assert_eq!(
            un.errance(0, 5, 1, 1.0),
            vec![0],
            "un seul morceau, un seul pas"
        );

        // Plus de voisins demandés que de morceaux disponibles.
        let deux = Graphe::construire(&arc(2), 40, 2);
        assert_eq!(deux.voisins(0, 40), vec![1]);
    }

    /* ------------------------------------------------------------ dessin */

    #[test]
    fn le_trace_cueille_les_points_sous_le_trait() {
        // Une grille : une rangée à y = 0, une autre bien plus haut.
        let mut pts: Vec<(i64, f32, f32)> = Vec::new();
        for i in 0..11 {
            pts.push((i, i as f32 * 0.1, 0.0));
            pts.push((100 + i, i as f32 * 0.1, 0.9));
        }

        let route = dessine(&pts, &[(0.0, 0.0), (1.0, 0.0)], 6, 0.08, 1, 0.0);
        assert_eq!(route.len(), 6);
        assert!(
            route.iter().all(|id| *id < 100),
            "le trait bas a attrapé la rangée haute : {route:?}"
        );
        // Le tracé va de gauche à droite : les abscisses doivent croître.
        for f in route.windows(2) {
            assert!(f[0] < f[1], "ordre du tracé non respecté : {route:?}");
        }
    }

    /// Le rééchantillonnage à pas d'arc constant : un tracé où la souris a
    /// traîné au départ ne doit pas surreprésenter le départ.
    #[test]
    fn le_trace_est_reechantillonne_a_pas_constant() {
        let pts: Vec<(i64, f32, f32)> = (0..21).map(|i| (i, i as f32 * 0.05, 0.0)).collect();
        // Dix points collés au début, puis un grand saut jusqu'au bout.
        let mut trace: Vec<(f32, f32)> = (0..10).map(|i| (i as f32 * 0.002, 0.0)).collect();
        trace.push((1.0, 0.0));

        let route = dessine(&pts, &trace, 5, 0.06, 1, 0.0);
        assert_eq!(route.first(), Some(&0));
        assert_eq!(route.last(), Some(&20));
        let ecarts: Vec<i64> = route.windows(2).map(|f| f[1] - f[0]).collect();
        assert!(
            ecarts.iter().all(|e| (4..=6).contains(e)),
            "pas irréguliers : {ecarts:?}"
        );
    }

    #[test]
    fn le_trace_troue_plutot_que_dinventer() {
        let pts = vec![(1i64, 0.0f32, 0.0f32), (2, 1.0, 0.0)];
        // Un trait qui traverse une zone vide : rien à cueillir au milieu.
        let route = dessine(&pts, &[(0.0, 0.0), (1.0, 0.0)], 9, 0.05, 1, 0.0);
        assert_eq!(route, vec![1, 2], "un morceau lointain s'est invité");

        assert!(
            dessine(&pts, &[(0.0, 0.0)], 4, 0.5, 1, 0.0).is_empty(),
            "trait d'un point"
        );
        assert!(
            dessine(&[], &[(0.0, 0.0), (1.0, 0.0)], 4, 0.5, 1, 0.0).is_empty(),
            "carte vide"
        );
    }

    /// Sur un arc, un parcours doit se dérouler sans revenir en arrière —
    /// c'est toute la différence avec l'ordre où la base rend les lignes.
    #[test]
    fn le_parcours_enchaine_les_proches() {
        let e = arc(21);
        // Volontairement mélangés : c'est l'ordre d'arrivée d'une sélection.
        let ids = vec![14, 2, 9, 5, 17, 11, 7];
        let route = parcours(&e, &ids);

        assert_eq!(route.len(), ids.len(), "aucun morceau perdu");
        assert_eq!(
            route.iter().copied().collect::<HashSet<_>>(),
            ids.iter().copied().collect::<HashSet<_>>(),
            "les mêmes morceaux, réordonnés"
        );
        // Sur un arc, avancer de proche en proche donne une suite monotone
        // une fois passé le premier pas.
        let ecarts: Vec<i64> = route.windows(2).map(|f| f[1] - f[0]).collect();
        let changements = ecarts
            .windows(2)
            .filter(|f| f[0].signum() != f[1].signum())
            .count();
        assert!(changements <= 1, "le parcours zigzague : {route:?}");
    }

    #[test]
    fn le_parcours_supporte_les_cas_degeneres() {
        let e = arc(5);
        assert!(parcours(&e, &[]).is_empty());
        assert_eq!(parcours(&e, &[3]), vec![3]);
        // Un identifiant inconnu est ignoré, pas fatal.
        assert_eq!(parcours(&e, &[99]).len(), 0);
        assert_eq!(parcours(&e, &[1, 99, 2]).len(), 2);
    }

    #[test]
    fn lechantillonnage_garde_les_extremites() {
        let route: Vec<i64> = (0..100).collect();
        let court = echantillonner(&route, 7);
        assert_eq!(court.len(), 7);
        assert_eq!(court.first(), Some(&0));
        assert_eq!(court.last(), Some(&99));
        // Rien à faire si le trajet tient déjà dans le budget.
        assert_eq!(echantillonner(&[1, 2, 3], 10), vec![1, 2, 3]);
    }
}

