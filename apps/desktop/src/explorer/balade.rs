// SPDX-License-Identifier: GPL-3.0-or-later
//! La balade (`docs/plan-balade.md`, P1) : la lecture est une marche dans les
//! rues. Le morceau suivant n'est pas dans une file calculée d'avance, il est
//! choisi quand on en a besoin, depuis la position courante.
//!
//! **File courte.** Le lecteur ne reçoit que le morceau en cours ; le suivant
//! est décidé tout de suite (pour que l'interface trace le pas du promeneur)
//! mais n'entre dans la file que dans les [`FIGE_AVANT_FIN`] dernières
//! secondes, où il est figé : jusque-là, P2 pourra le remplacer sans rien
//! couper, et l'enchaînement sans blanc garde la marge du préchargement.
//!
//! Le battement ([`balade_battement`]) tourne dans le fil natif de `main`, à
//! côté du préchargement : il relève le lecteur verrou tenu le temps d'une
//! copie, calcule hors de ce verrou (voir le commentaire de
//! `Player::completer`), et ne le reprend que pour poser le suivant.
//!
//! **Repli** sans ville importée (ou départ sans adresse) : la même balade
//! dans le graphe des voisins, où les branches sont les voisins non joués.
//!
//! **Carrefour** (P2). Quand plusieurs voies s'offrent, le pas décidé est la
//! pente par défaut, et l'interface montre deux ou trois voies
//! ([`rusty_music_carto::balade::proposer`]). Un geste ([`balade_choisir`])
//! remplace le suivant tant qu'il n'est pas figé. Ne rien faire, c'est
//! prendre la pente — sauf en mode *demander*, où la balade attend le choix,
//! dans le silence s'il le faut. [`balade_revenir`] retourne au carrefour
//! passé et repropose les voies non prises. Le compteur d'usage
//! (`balade-mesure.json`, à côté de la base) compte, à la sortie de chaque
//! carrefour montré, s'il y a eu un choix et s'il allait « ailleurs ».

use super::*;

use std::sync::atomic::AtomicU64;

use rusty_music_carto::balade::{
    deja_vu, pente_simple, proposer, suivants, Branche, EtatBalade, PROPOSEES_MAX,
};

/// Le suivant entre dans la file, et n'en bouge plus, à moins de 10 s de la
/// fin du morceau en cours.
const FIGE_AVANT_FIN: Duration = Duration::from_secs(10);
/// Branches proposées dans le graphe des voisins (repli).
const BRANCHES_VOISINS: usize = 3;
/// Carrefours passés qu'on garde pour y revenir.
const CARREFOURS_GARDES: usize = 32;
/// L'extrait d'une voie au survol : sa durée, et où il commence dans le
/// morceau (passé l'introduction).
const EXTRAIT_DUREE: Duration = Duration::from_secs(5);
const EXTRAIT_DEBUT: f64 = 0.3;

/// Le degré d'intervention (`docs/plan-balade.md`, le principe), réglé une
/// fois dans le rail d'Explorer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Intervention {
    /// Aucun panneau : le tracé des voies seulement, la pente décide.
    Porter,
    /// Le panneau, la pente prise sans geste.
    #[default]
    Proposer,
    /// La balade attend le choix au carrefour.
    Demander,
}

/// Une suite possible au carrefour : un morceau, et dans la ville la branche
/// qui y mène.
#[derive(Clone)]
struct Voie {
    morceau: i64,
    branche: Option<Branche>,
}

/// Un carrefour : présenté pendant le morceau en cours, ou passé.
struct Carrefour {
    /// Change à chaque présentation : l'interface y accroche Échap.
    numero: u64,
    /// La position d'où partent les voies (ville).
    depuis: Option<EtatBalade>,
    /// Les voies montrées, au plus trois.
    voies: Vec<Voie>,
    /// Distance à l'oreille de chaque voie au morceau du carrefour.
    distances: Vec<f32>,
    /// La pente par défaut.
    defaut: usize,
    /// La plus proche à l'oreille : un choix d'une autre est « ailleurs ».
    proche: usize,
    /// Choisie d'un geste ; `None` : la pente.
    choisie: Option<usize>,
    /// Voies déjà parcourues depuis ce carrefour.
    lues: HashSet<usize>,
    /// Longueur de la trace au carrefour : de quoi y revenir à pied.
    trace_len: usize,
    /// Représenté par « ← Carrefour précédent ».
    represente: bool,
    /// Le chemin du retour, alors : du morceau en cours au carrefour, par la
    /// trace (vide dans le graphe des voisins).
    retour: Vec<[f64; 2]>,
}

impl Carrefour {
    fn prise(&self) -> usize {
        self.choisie.unwrap_or(self.defaut)
    }
}

/// Le compteur d'usage du carrefour, gardé sur la machine
/// (`balade-mesure.json`) : de quoi juger, après une semaine, si l'on choisit.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct MesureCarrefours {
    /// Carrefours montrés (hors *se laisser porter*) et quittés.
    carrefours: u64,
    /// … où une voie a été choisie d'un geste.
    choix: u64,
    /// … et cette voie n'était pas la plus proche à l'oreille.
    ailleurs: u64,
    /// Retours au carrefour précédent.
    retours: u64,
}

/// Où la balade marche : dans les rues, ou dans le graphe des voisins.
enum Terrain {
    Ville {
        graphe: Arc<rusty_music_carto::reseau_reel::Graphe>,
        accrochage: Arc<AccrochageVoirie>,
        /// Adresses réelles (`positions.json`), `[lon, lat]`.
        positions: Arc<HashMap<i64, [f64; 2]>>,
        etat: EtatBalade,
    },
    Voisins {
        graphe: Arc<Graphe>,
        vus: HashSet<i64>,
    },
}

/// Le pas décidé : le morceau suivant et le chemin du promeneur jusqu'à lui.
struct Pas {
    morceau: i64,
    chemin: PathBuf,
    /// La branche prise (ville seulement), pour avancer l'état à l'arrivée.
    branche: Option<Branche>,
    /// Le tracé du promeneur pendant le morceau en cours : de son adresse à
    /// celle du suivant, le long des rues. Vide dans le graphe des voisins
    /// (l'interface trace alors la droite d'un point à l'autre).
    troncon: Vec<[f64; 2]>,
    /// Déjà confié au lecteur : figé.
    fige: bool,
    /// Ouvert par le préchargement avant la fin du morceau en cours : il
    /// s'enchaîne sans blanc. Relevé pour le journal.
    precharge: bool,
}

/// Ce que l'interface reçoit d'une voie du carrefour.
#[derive(serde::Serialize)]
pub(crate) struct VoieVue {
    morceau: i64,
    /// Distance cosinus au morceau en cours : la jauge proche ↔ ailleurs.
    distance: f32,
    defaut: bool,
    proche: bool,
    /// Déjà parcourue (retour au carrefour) : montrée, plus proposée.
    lue: bool,
    /// Le tracé de la voie depuis le carrefour, `[lon, lat]` (ville).
    rue: Vec<[f64; 2]>,
}

#[derive(serde::Serialize)]
pub(crate) struct CarrefourVue {
    numero: u64,
    voies: Vec<VoieVue>,
    /// La voie qui sera prise : la choisie, sinon la pente.
    prise: usize,
    choisie: bool,
    /// Représenté par « ← Carrefour précédent ».
    retour: bool,
}

pub(crate) struct Balade {
    terrain: Terrain,
    /// Empreintes, pour la pente par défaut (le plus proche à l'oreille).
    vecteurs: Arc<Vec<Empreinte>>,
    rang: HashMap<i64, usize>,
    /// Les morceaux joués, dans l'ordre ; le dernier est en cours.
    pistes: Vec<i64>,
    courant: PathBuf,
    suivant: Option<Pas>,
    /// La trace des morceaux déjà parcourus, `[lon, lat]` (ville).
    trace: Vec<[f64; 2]>,
    /// La balade s'est arrêtée d'elle-même (plus rien devant).
    fin: Option<String>,
    /// Le bout du dernier pas, pour vérifier que le suivant en repart.
    bout: Option<[f64; 2]>,
    intervention: Intervention,
    /// Le carrefour du pas décidé, s'il y en a un.
    carrefour: Option<Carrefour>,
    /// Les carrefours passés, le plus récent en dernier.
    passes: Vec<Carrefour>,
    numero: u64,
    mesure: MesureCarrefours,
}

/// Ce que l'interface reçoit.
#[derive(serde::Serialize)]
pub(crate) struct BaladeVue {
    /// `"ville"` ou `"voisins"`.
    terrain: &'static str,
    /// Les morceaux joués, dans l'ordre ; le dernier est en cours.
    pistes: Vec<i64>,
    suivant: Option<i64>,
    /// Le suivant est dans la file du lecteur et n'en bougera plus.
    fige: bool,
    /// Le pas du promeneur pendant le morceau en cours, `[lon, lat]`.
    troncon: Vec<[f64; 2]>,
    /// Ce qui a déjà été parcouru, `[lon, lat]`.
    trace: Vec<[f64; 2]>,
    /// `Some(raison)` : la balade s'est arrêtée d'elle-même.
    fin: Option<String>,
    intervention: Intervention,
    /// Le carrefour qui s'offre au bout du morceau en cours.
    carrefour: Option<CarrefourVue>,
    /// La balade attend un choix (mode *demander*).
    attend: bool,
    /// Un carrefour passé où revenir, avec au moins une voie non prise.
    revenir: bool,
    mesure: MesureCarrefours,
}

/// Ce qui survit à un redémarrage : de quoi reprendre au même pas. Les
/// carrefours passés n'y sont pas : on ne revient pas en arrière d'une
/// session à l'autre.
#[derive(serde::Serialize, serde::Deserialize)]
struct Reprise {
    pistes: Vec<i64>,
    ville: Option<EtatBalade>,
    trace: Vec<[f64; 2]>,
    #[serde(default)]
    intervention: Intervention,
}

fn chemin_reprise(etat: &Etat) -> PathBuf {
    etat.db.with_file_name("balade.json")
}

fn chemin_mesure(etat: &Etat) -> PathBuf {
    etat.db.with_file_name("balade-mesure.json")
}

fn lire_mesure(etat: &Etat) -> MesureCarrefours {
    std::fs::read(chemin_mesure(etat))
        .ok()
        .and_then(|o| serde_json::from_slice(&o).ok())
        .unwrap_or_default()
}

fn ecrire_mesure(etat: &Etat, mesure: &MesureCarrefours) {
    if let Ok(octets) = serde_json::to_vec(mesure) {
        if let Err(e) = std::fs::write(chemin_mesure(etat), octets) {
            tracing::warn!(erreur = %e, "balade : mesure non écrite");
        }
    }
}

impl Balade {
    fn vue(&self) -> BaladeVue {
        BaladeVue {
            terrain: match self.terrain {
                Terrain::Ville { .. } => "ville",
                Terrain::Voisins { .. } => "voisins",
            },
            pistes: self.pistes.clone(),
            suivant: self.suivant.as_ref().map(|p| p.morceau),
            fige: self.suivant.as_ref().is_some_and(|p| p.fige),
            troncon: self
                .suivant
                .as_ref()
                .map(|p| p.troncon.clone())
                .unwrap_or_default(),
            trace: self.trace.clone(),
            fin: self.fin.clone(),
            intervention: self.intervention,
            carrefour: self.carrefour.as_ref().map(|c| self.carrefour_vue(c)),
            attend: self.attend(),
            revenir: self.suivant.as_ref().is_none_or(|p| !p.fige)
                && self
                    .passes
                    .last()
                    .is_some_and(|c| self.voies_libres(c).next().is_some()),
            mesure: self.mesure.clone(),
        }
    }

    fn carrefour_vue(&self, c: &Carrefour) -> CarrefourVue {
        let rue = |v: &Voie| -> Vec<[f64; 2]> {
            let Terrain::Ville {
                graphe, positions, ..
            } = &self.terrain
            else {
                return Vec::new();
            };
            let Some(b) = &v.branche else {
                return Vec::new();
            };
            let mut r: Vec<[f64; 2]> = b.sommets.iter().map(|&s| graphe.point(s)).collect();
            r.extend(positions.get(&v.morceau).copied());
            r
        };
        CarrefourVue {
            numero: c.numero,
            voies: c
                .voies
                .iter()
                .enumerate()
                .map(|(i, v)| VoieVue {
                    morceau: v.morceau,
                    distance: c.distances[i],
                    defaut: i == c.defaut,
                    proche: i == c.proche,
                    lue: self.voie_lue(c, i),
                    rue: rue(v),
                })
                .collect(),
            prise: c.prise(),
            choisie: c.choisie.is_some(),
            retour: c.represente,
        }
    }

    /// Une voie déjà parcourue, ou dont le morceau a joué depuis.
    fn voie_lue(&self, c: &Carrefour, i: usize) -> bool {
        c.lues.contains(&i) || self.pistes.contains(&c.voies[i].morceau)
    }

    fn voies_libres<'a>(&'a self, c: &'a Carrefour) -> impl Iterator<Item = usize> + 'a {
        (0..c.voies.len()).filter(move |&i| !self.voie_lue(c, i))
    }

    /// Le mode *demander* attend un geste au carrefour.
    fn attend(&self) -> bool {
        self.intervention == Intervention::Demander
            && self.carrefour.as_ref().is_some_and(|c| c.choisie.is_none())
    }

    fn courant_id(&self) -> i64 {
        *self
            .pistes
            .last()
            .expect("une balade a toujours un morceau")
    }

    /// Distance cosinus entre le morceau en cours et `id` ; 2 (le maximum)
    /// pour un morceau sans empreinte.
    fn distance(&self, id: i64) -> f32 {
        let (Some(&a), Some(&b)) = (self.rang.get(&self.courant_id()), self.rang.get(&id)) else {
            return 2.0;
        };
        let (va, vb) = (&self.vecteurs[a].1, &self.vecteurs[b].1);
        let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
        for (x, y) in va.iter().zip(vb) {
            ab += x * y;
            aa += x * x;
            bb += y * y;
        }
        1.0 - ab / (aa.sqrt() * bb.sqrt()).max(1e-12)
    }

    /// Les voies qui s'offrent depuis la position courante, la pente par
    /// défaut en tête. Plus d'une : c'est un carrefour.
    fn voies(&self, lib: &Library) -> Vec<Voie> {
        let courant = self.courant_id();
        match &self.terrain {
            Terrain::Ville {
                graphe,
                accrochage,
                etat,
                ..
            } => {
                let branches = suivants(etat, graphe, accrochage);
                let Some(defaut) = pente_simple(etat, &branches, graphe, |id| self.distance(id))
                else {
                    return Vec::new();
                };
                let modele = rusty_music_analysis::passe::MODELE;
                let famille = |id: i64| lib.cluster_de_piste(modele, id).ok().flatten();
                proposer(
                    &branches,
                    defaut,
                    |id| self.distance(id),
                    famille,
                    famille(courant),
                    |b| deja_vu(etat, graphe, b),
                )
                .into_iter()
                .map(|i| Voie {
                    morceau: branches[i].morceau,
                    branche: Some(branches[i].clone()),
                })
                .collect()
            }
            Terrain::Voisins { graphe, vus } => {
                let libres = |k: usize| -> Vec<i64> {
                    graphe
                        .voisins(courant, k)
                        .into_iter()
                        .filter(|id| !vus.contains(id))
                        .collect()
                };
                let mut ids = libres(BRANCHES_VOISINS);
                if ids.is_empty() {
                    // Tous les proches déjà joués : on élargit avant d'abandonner.
                    ids = libres(rusty_music_analysis::chemin::K_VOISINS * 4);
                }
                ids.sort_by(|&a, &b| self.distance(a).total_cmp(&self.distance(b)));
                ids.truncate(PROPOSEES_MAX);
                ids.into_iter()
                    .map(|morceau| Voie {
                        morceau,
                        branche: None,
                    })
                    .collect()
            }
        }
    }

    /// Le pas vers `voie`. `retour` : le chemin déjà fait pour revenir au
    /// carrefour (« ← Carrefour précédent »), d'où part alors le pas.
    fn pas_vers(&self, lib: &Library, voie: &Voie, retour: &[[f64; 2]]) -> Option<Pas> {
        let mut troncon: Vec<[f64; 2]> = Vec::new();
        if let (
            Terrain::Ville {
                graphe, positions, ..
            },
            Some(b),
        ) = (&self.terrain, &voie.branche)
        {
            if retour.is_empty() {
                troncon.extend(positions.get(&self.courant_id()).copied());
            } else {
                troncon.extend_from_slice(retour);
            }
            troncon.extend(b.sommets.iter().map(|&s| graphe.point(s)));
            troncon.extend(positions.get(&voie.morceau).copied());
        }
        let chemin = PathBuf::from(lib.track(voie.morceau).ok()??.path);
        Some(Pas {
            morceau: voie.morceau,
            chemin,
            branche: voie.branche.clone(),
            troncon,
            fige: false,
            precharge: false,
        })
    }

    /// Décide le pas suivant (pente par défaut) et, s'il y a plusieurs voies,
    /// présente le carrefour. `false` : plus rien devant.
    fn decider(&mut self, lib: &Library) -> bool {
        let voies = self.voies(lib);
        let Some(pas) = voies.first().and_then(|v| self.pas_vers(lib, v, &[])) else {
            return false;
        };
        self.carrefour = (voies.len() > 1).then(|| {
            let distances: Vec<f32> = voies.iter().map(|v| self.distance(v.morceau)).collect();
            let proche = (0..voies.len())
                .min_by(|&a, &b| distances[a].total_cmp(&distances[b]))
                .unwrap_or(0);
            self.numero += 1;
            Carrefour {
                numero: self.numero,
                depuis: match &self.terrain {
                    Terrain::Ville { etat, .. } => Some(etat.clone()),
                    Terrain::Voisins { .. } => None,
                },
                voies,
                distances,
                defaut: 0,
                proche,
                choisie: None,
                lues: HashSet::new(),
                trace_len: self.trace.len(),
                represente: false,
                retour: Vec::new(),
            }
        });
        self.suivant = Some(pas);
        true
    }

    /// Un geste au carrefour : la voie `i` remplace le suivant décidé.
    fn choisir(&mut self, lib: &Library, numero: u64, i: usize) -> Result<(), String> {
        if self.suivant.as_ref().is_some_and(|p| p.fige) {
            return Err("le morceau suivant est déjà en route".into());
        }
        let c = self
            .carrefour
            .as_ref()
            .filter(|c| c.numero == numero)
            .ok_or("ce carrefour est passé")?;
        if i >= c.voies.len() {
            return Err("voie inconnue".into());
        }
        if self.voie_lue(c, i) {
            return Err("cette voie a déjà été parcourue".into());
        }
        let pas = self
            .pas_vers(lib, &c.voies[i], &c.retour)
            .ok_or("morceau introuvable")?;
        self.suivant = Some(pas);
        self.carrefour.as_mut().expect("vérifié").choisie = Some(i);
        Ok(())
    }

    /// « ← Carrefour précédent » : la balade retourne au dernier carrefour
    /// passé et en repropose les voies non prises ; celle qu'on avait prise
    /// est montrée comme lue. Le morceau en cours continue : c'est le pas
    /// suivant qui part du carrefour, par le chemin du retour.
    fn revenir(&mut self, lib: &Library) -> Result<(), String> {
        if self.suivant.as_ref().is_some_and(|p| p.fige) {
            return Err("le morceau suivant est déjà en route".into());
        }
        let mut c = self.passes.pop().ok_or("aucun carrefour passé")?;
        let libres: Vec<usize> = self.voies_libres(&c).collect();
        let Some(&defaut) = libres
            .iter()
            .min_by(|&&a, &&b| c.distances[a].total_cmp(&c.distances[b]))
        else {
            self.passes.push(c);
            return Err("toutes les voies de ce carrefour ont été prises".into());
        };
        // Le chemin du retour : la trace, à rebours, jusqu'au carrefour.
        let mut retour: Vec<[f64; 2]> = self.trace[c.trace_len.min(self.trace.len())..].to_vec();
        retour.reverse();
        let pas = self
            .pas_vers(lib, &c.voies[defaut], &retour)
            .ok_or("morceau introuvable")?;
        if let (Terrain::Ville { etat, .. }, Some(avant)) = (&mut self.terrain, &c.depuis) {
            etat.revenir(avant);
        }
        self.numero += 1;
        c.numero = self.numero;
        c.defaut = defaut;
        c.choisie = None;
        c.represente = true;
        c.retour = retour;
        c.trace_len = self.trace.len();
        self.carrefour = Some(c);
        self.suivant = Some(pas);
        self.mesure.retours += 1;
        Ok(())
    }

    /// Le suivant vient de commencer : il devient le morceau en cours.
    fn arriver(&mut self) {
        let Some(pas) = self.suivant.take() else {
            return;
        };
        // Le promeneur ne saute pas : le pas qui commence part d'où le
        // précédent s'est arrêté (l'adresse du morceau en cours).
        let saut_m = match (self.bout, pas.troncon.first()) {
            (Some(a), Some(&b)) => rusty_music_carto::reseau_reel::distance_m(a, b),
            _ => 0.0,
        };
        tracing::info!(
            pas = self.pistes.len(),
            morceau = pas.morceau,
            precharge = pas.precharge,
            repete = self.pistes.contains(&pas.morceau),
            saut_m,
            longueur_m = pas
                .troncon
                .windows(2)
                .map(|w| rusty_music_carto::reseau_reel::distance_m(w[0], w[1]))
                .sum::<f64>(),
            "balade : pas"
        );
        self.bout = pas.troncon.last().copied();
        if let Some(mut c) = self.carrefour.take() {
            let prise = c.prise();
            if self.intervention != Intervention::Porter {
                self.mesure.carrefours += 1;
                if c.choisie.is_some() {
                    self.mesure.choix += 1;
                    if prise != c.proche {
                        self.mesure.ailleurs += 1;
                    }
                }
            }
            tracing::info!(
                voies = c.voies.len(),
                prise,
                defaut = c.defaut,
                choisie = c.choisie.is_some(),
                ailleurs = prise != c.proche,
                retour = c.represente,
                "balade : carrefour"
            );
            c.lues.insert(prise);
            c.represente = false;
            c.retour.clear();
            self.passes.push(c);
            if self.passes.len() > CARREFOURS_GARDES {
                self.passes.remove(0);
            }
        }
        match &mut self.terrain {
            Terrain::Ville { etat, .. } => {
                if let Some(b) = &pas.branche {
                    etat.avancer(b);
                }
            }
            Terrain::Voisins { vus, .. } => {
                vus.insert(pas.morceau);
            }
        }
        if matches!(self.terrain, Terrain::Ville { .. }) {
            self.trace.extend(pas.troncon.iter().copied());
        }
        self.pistes.push(pas.morceau);
        self.courant = pas.chemin;
    }

    fn reprise(&self) -> Reprise {
        Reprise {
            pistes: self.pistes.clone(),
            ville: match &self.terrain {
                Terrain::Ville { etat, .. } => Some(etat.clone()),
                Terrain::Voisins { .. } => None,
            },
            trace: self.trace.clone(),
            intervention: self.intervention,
        }
    }
}

fn sauver(etat: &Etat, balade: &Balade) {
    let chemin = chemin_reprise(etat);
    match serde_json::to_vec(&balade.reprise()) {
        Ok(octets) => {
            if let Err(e) = std::fs::write(&chemin, octets) {
                tracing::warn!(erreur = %e, "balade : reprise non écrite");
            }
        }
        Err(e) => tracing::warn!(erreur = %e, "balade : reprise non sérialisée"),
    }
}

/// Ce que demande une balade dans la ville : graphe, accrochage, adresses.
/// `None` (avec la raison) quand il faut se replier sur les voisins.
type Ville = (
    Arc<rusty_music_carto::reseau_reel::Graphe>,
    Arc<AccrochageVoirie>,
    Arc<HashMap<i64, [f64; 2]>>,
);

fn preparer_ville(etat: &State<Etat>, app: &tauri::AppHandle) -> Result<Ville, String> {
    let chemin = plan_de_ville(&etat.db).ok_or("aucune ville importée")?;
    let extrait = charger_ville(etat, &chemin)?;
    let graphe = charger_graphe_reel(etat, &extrait)?;
    let accrochage = charger_accrochage_voirie(etat, app, &graphe)?;
    let positions: HashMap<i64, [f64; 2]> = points_de_carte_effectifs(etat, app, true)?
        .into_iter()
        .map(|(id, x, y)| (id, [x as f64, y as f64]))
        .collect();
    Ok((graphe, accrochage, Arc::new(positions)))
}

/// Monte une balade dont `pistes` est le passé (le dernier en cours).
fn monter(
    etat: &State<Etat>,
    app: &tauri::AppHandle,
    pistes: Vec<i64>,
    reprise_ville: Option<EtatBalade>,
    trace: Vec<[f64; 2]>,
    intervention: Intervention,
) -> Result<Balade, String> {
    let depart = *pistes.last().ok_or("balade sans morceau")?;
    let courant = {
        let lib = verrou(&etat.lib);
        PathBuf::from(
            lib.track(depart)
                .map_err(echec)?
                .ok_or("morceau inconnu")?
                .path,
        )
    };
    let vecteurs = charger_vecteurs(etat)?;
    let rang: HashMap<i64, usize> = vecteurs
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();

    let ville = preparer_ville(etat, app).and_then(|(graphe, accrochage, positions)| {
        let sommet = *accrochage
            .sommet_de
            .get(&depart)
            .ok_or("ce morceau n'est rattaché à aucune rue")?;
        Ok((graphe, accrochage, positions, sommet))
    });
    let terrain = match ville {
        Ok((graphe, accrochage, positions, sommet)) => {
            let etat_b = reprise_ville.unwrap_or_else(|| {
                let mut e = EtatBalade::depuis(sommet, depart);
                e.morceaux_vus.extend(pistes.iter().copied());
                e
            });
            Terrain::Ville {
                graphe,
                accrochage,
                positions,
                etat: etat_b,
            }
        }
        Err(raison) => {
            tracing::info!(raison, "balade dans le graphe des voisins");
            Terrain::Voisins {
                graphe: construire_graphe(etat, &vecteurs)?,
                vus: pistes.iter().copied().collect(),
            }
        }
    };
    Ok(Balade {
        terrain,
        vecteurs,
        rang,
        pistes,
        courant,
        suivant: None,
        trace,
        fin: None,
        bout: None,
        intervention,
        carrefour: None,
        passes: Vec::new(),
        numero: 0,
        mesure: lire_mesure(etat),
    })
}

/// « Flâner d'ici » : lance la lecture de `depart` et la balade qui en part.
#[tauri::command(async)]
pub(crate) fn balade_demarrer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    depart: i64,
    intervention: Option<Intervention>,
) -> Result<BaladeVue, String> {
    let mut balade = monter(
        &etat,
        &app,
        vec![depart],
        None,
        Vec::new(),
        intervention.unwrap_or_default(),
    )?;
    {
        let lib = verrou(&etat.lib);
        balade.decider(&lib);
    }
    verrou(&etat.player)
        .play(std::slice::from_ref(&balade.courant))
        .map_err(echec)?;
    sauver(&etat, &balade);
    let vue = balade.vue();
    tracing::info!(depart, terrain = vue.terrain, "balade démarrée");
    *verrou(&etat.balade) = Some(balade);
    Ok(vue)
}

/// La balade en cours, `None` s'il n'y en a pas.
#[tauri::command(async)]
pub(crate) fn balade_etat(etat: State<Etat>) -> Option<BaladeVue> {
    verrou(&etat.balade).as_ref().map(Balade::vue)
}

/// « Arrêter la balade » : la lecture continue, plus rien n'est ajouté.
#[tauri::command(async)]
pub(crate) fn balade_arreter(etat: State<Etat>) {
    arreter(&etat);
}

fn arreter(etat: &Etat) {
    if let Some(b) = verrou(&etat.balade).take() {
        tracing::info!(mesure = ?b.mesure, "balade arrêtée");
    }
    std::fs::remove_file(chemin_reprise(etat)).ok();
}

/// Un geste au carrefour `numero` : la voie `voie` devient le pas suivant.
/// Refusé dans les 10 dernières secondes, quand le suivant est figé.
#[tauri::command(async)]
pub(crate) fn balade_choisir(
    etat: State<Etat>,
    numero: u64,
    voie: usize,
) -> Result<BaladeVue, String> {
    let mut garde = verrou(&etat.balade);
    let balade = garde.as_mut().ok_or("aucune balade")?;
    {
        let lib = verrou(&etat.lib);
        balade.choisir(&lib, numero, voie)?;
    }
    Ok(balade.vue())
}

/// « ← Carrefour précédent ».
#[tauri::command(async)]
pub(crate) fn balade_revenir(etat: State<Etat>) -> Result<BaladeVue, String> {
    let mut garde = verrou(&etat.balade);
    let balade = garde.as_mut().ok_or("aucune balade")?;
    {
        let lib = verrou(&etat.lib);
        balade.revenir(&lib)?;
    }
    ecrire_mesure(&etat, &balade.mesure);
    Ok(balade.vue())
}

/// Le degré d'intervention, pour la balade en cours (l'interface le garde
/// avec les autres réglages de la carte et le redonne à chaque départ).
#[tauri::command(async)]
pub(crate) fn balade_regler(etat: State<Etat>, intervention: Intervention) -> Option<BaladeVue> {
    let mut garde = verrou(&etat.balade);
    let balade = garde.as_mut()?;
    balade.intervention = intervention;
    sauver(&etat, balade);
    Some(balade.vue())
}

/// Génération de l'extrait : un survol quitté avant la fin du décodage ne
/// doit pas lancer son extrait après coup.
static EXTRAIT: AtomicU64 = AtomicU64::new(0);

/// L'extrait de 5 s d'une voie survolée ; `None` le coupe. Le son principal
/// est baissé le temps de l'extrait (`Player::jouer_extrait`) : le mélange à
/// deux voix viendra avec l'aura (P4).
#[tauri::command(async)]
pub(crate) fn balade_extrait(etat: State<Etat>, morceau: Option<i64>) -> Result<(), String> {
    let generation = EXTRAIT.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(id) = morceau else {
        verrou(&etat.player).arreter_extrait();
        return Ok(());
    };
    let piste = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .ok_or("morceau inconnu")?;
    let chemin = PathBuf::from(&piste.path);
    let duree = piste.duration_ms.unwrap_or(0).max(0) as f64 / 1000.0;
    let debut = Duration::from_secs_f64(duree * EXTRAIT_DEBUT);
    // Décodé hors du verrou du lecteur.
    let source = rusty_music_player::ouvrir_extrait(
        &chemin,
        debut,
        EXTRAIT_DUREE,
        gain_pour(&etat, &chemin),
    )
    .map_err(echec)?;
    if EXTRAIT.load(Ordering::SeqCst) == generation {
        verrou(&etat.player).jouer_extrait(source);
    }
    Ok(())
}

/// Ce que le battement relève du lecteur, verrou tenu le temps d'une copie.
struct Releve {
    courant: Option<PathBuf>,
    restant: Option<Duration>,
    /// Pistes prêtes dans la sortie, celle en cours comprise.
    prets: usize,
}

fn relever(etat: &Etat) -> Releve {
    let p = verrou(&etat.player);
    let courant = p.current().map(Path::to_path_buf);
    let restant = p.duree_courante().map(|d| d.saturating_sub(p.position()));
    Releve {
        courant,
        restant,
        prets: p.remaining(),
    }
}

/// Le battement de la balade : avance quand le suivant commence, décide le
/// pas d'après, le fige et le confie au lecteur dans les dernières secondes.
/// Appelé toutes les 500 ms par le fil natif de `main`.
pub(crate) fn balade_battement(etat: &Etat) {
    let releve = relever(etat);
    let mut garde = verrou(&etat.balade);
    let Some(balade) = garde.as_mut() else {
        return;
    };
    if balade.fin.is_some() {
        return;
    }

    let suivant_commence = balade
        .suivant
        .as_ref()
        .is_some_and(|p| releve.courant.as_deref() == Some(p.chemin.as_path()));
    if suivant_commence {
        balade.arriver();
        sauver(etat, balade);
    } else if releve
        .courant
        .as_deref()
        .is_some_and(|c| c != balade.courant.as_path())
    {
        // On joue autre chose : ▶ ailleurs, ⏮, un clic dans la file. La
        // balade s'efface sans rien toucher à la lecture.
        drop(garde);
        arreter(etat);
        return;
    }

    if suivant_commence {
        ecrire_mesure(etat, &balade.mesure);
    }

    if balade.suivant.is_none() {
        let decide = {
            let lib = verrou(&etat.lib);
            balade.decider(&lib)
        };
        if !decide {
            balade.fin = Some("plus aucun morceau devant".into());
            return;
        }
    }

    // À moins de 10 s de la fin — ou plus rien ne joue (⏭, morceau illisible) :
    // le suivant entre dans la file et n'en bouge plus. En mode *demander*,
    // pas avant le choix : la balade attend, dans le silence s'il le faut.
    let fin_proche = releve.restant.is_none_or(|r| r <= FIGE_AVANT_FIN);
    let attend = balade.attend();
    let pas = balade.suivant.as_mut().expect("décidé juste au-dessus");
    if pas.fige && releve.prets >= 2 {
        pas.precharge = true;
    }
    if pas.fige || !fin_proche || attend {
        return;
    }
    let mut player = verrou(&etat.player);
    let resultat = match player.current().map(Path::to_path_buf) {
        Some(c) if c == balade.courant => {
            // `set_queue` garde ce qui est déjà confié à la sortie et ne
            // touche pas au disque : sûr verrou tenu.
            let rang = player.rang_courant().unwrap_or(0);
            let mut file: Vec<PathBuf> = player.queue()[..=rang].to_vec();
            file.push(pas.chemin.clone());
            player.set_queue(&file)
        }
        // Plus rien ne joue : le suivant part tout de suite.
        None => {
            tracing::warn!(
                morceau = pas.morceau,
                "balade : file vide, le suivant repart"
            );
            player.play(std::slice::from_ref(&pas.chemin))
        }
        // Autre chose a commencé entre le relevé et maintenant : le prochain
        // battement arrêtera la balade.
        Some(_) => return,
    };
    match resultat {
        Ok(()) => pas.fige = true,
        Err(e) => tracing::warn!(erreur = %e, "balade : suivant refusé par le lecteur"),
    }
}

/// Reprise de session : si le lecteur a repris sur le morceau où la balade
/// s'était arrêtée, elle repart de là. Appelé par le fil natif tant que
/// `etat.balade_reprise` n'est pas tranchée (le lecteur se restaure dans un
/// autre fil, au démarrage).
pub(crate) fn balade_reprendre(app: &tauri::AppHandle, etat: &State<Etat>) {
    if etat.balade_reprise.load(Ordering::Relaxed) {
        return;
    }
    let chemin = chemin_reprise(etat);
    let Ok(octets) = std::fs::read(&chemin) else {
        etat.balade_reprise.store(true, Ordering::Relaxed);
        return;
    };
    let Some(courant) = relever(etat).courant else {
        return; // le lecteur n'a pas encore repris : on retentera
    };
    etat.balade_reprise.store(true, Ordering::Relaxed);
    let Ok(r) = serde_json::from_slice::<Reprise>(&octets) else {
        std::fs::remove_file(&chemin).ok();
        return;
    };
    let Some(&dernier) = r.pistes.last() else {
        return;
    };
    let meme = verrou(&etat.lib)
        .track(dernier)
        .ok()
        .flatten()
        .is_some_and(|t| Path::new(&t.path) == courant);
    if !meme {
        std::fs::remove_file(&chemin).ok();
        return;
    }
    match monter(etat, app, r.pistes, r.ville, r.trace, r.intervention) {
        Ok(b) => {
            tracing::info!(pas = b.pistes.len(), "balade reprise");
            *verrou(&etat.balade) = Some(b);
        }
        Err(e) => tracing::warn!(erreur = %e, "balade non reprise"),
    }
}

/// `RUSTY_MUSIC_ESSAI_BALADE=<id>` : lance une balade depuis le morceau `id`
/// au démarrage, **sans le son** (volume à zéro), pour juger une longue balade
/// au journal (`balade : pas`, `balade : carrefour`). À lancer sur une copie
/// des données (`RUSTY_MUSIC_DONNEES`) : la session de l'essai remplace celle
/// d'avant, et son compteur d'usage aussi. Avec `RUSTY_MUSIC_ESSAI_CHOIX=1`,
/// l'essai fait aussi les gestes du carrefour ([`essai_choisir`]). Sans effet
/// en usage normal.
pub(crate) fn essai_balade(app: &tauri::AppHandle) {
    let Some(depart) = std::env::var("RUSTY_MUSIC_ESSAI_BALADE")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
    else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        // Laisse la reprise de session passer avant : elle remplacerait la file.
        std::thread::sleep(Duration::from_secs(5));
        let etat = app.state::<Etat>();
        verrou(&etat.player).set_volume(0.0);
        match balade_demarrer(app.clone(), etat, depart, None) {
            Ok(v) => tracing::info!(depart, terrain = v.terrain, "essai de balade lancé"),
            Err(e) => {
                tracing::warn!(erreur = %e, "essai de balade impossible");
                return;
            }
        }
        if std::env::var_os("RUSTY_MUSIC_ESSAI_CHOIX").is_none() {
            return;
        }
        let mut revenu = u64::MAX;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            essai_choisir(&app.state::<Etat>(), &mut revenu);
        }
    });
}

/// Les gestes de l'essai, à 25 s de la fin de chaque morceau qui débouche sur
/// un carrefour : la voie la plus lointaine à l'oreille (« ailleurs ») ; et
/// au morceau qui suit un carrefour sur deux, un retour à ce carrefour. De quoi
/// vérifier au journal qu'un choix remplace le suivant avant son
/// préchargement, sans blanc.
fn essai_choisir(etat: &Etat, revenu: &mut u64) {
    if relever(etat)
        .restant
        .is_none_or(|r| r > Duration::from_secs(25))
    {
        return;
    }
    let mut garde = verrou(&etat.balade);
    let Some(b) = garde.as_mut() else { return };
    if b.suivant.as_ref().is_none_or(|p| p.fige) {
        return;
    }
    let lib = verrou(&etat.lib);
    if b.mesure.carrefours % 2 == 1 && *revenu != b.mesure.carrefours && !b.passes.is_empty() {
        *revenu = b.mesure.carrefours;
        match b.revenir(&lib) {
            Ok(()) => tracing::info!("essai : retour au carrefour précédent"),
            Err(e) => tracing::info!(raison = e, "essai : retour refusé"),
        }
        return;
    }
    let Some(c) = b.carrefour.as_ref().filter(|c| c.choisie.is_none()) else {
        return;
    };
    let Some(cible) = (0..c.voies.len())
        .filter(|&i| i != c.defaut && !b.voie_lue(c, i))
        .max_by(|&x, &y| c.distances[x].total_cmp(&c.distances[y]))
    else {
        return;
    };
    let numero = c.numero;
    match b.choisir(&lib, numero, cible) {
        Ok(()) => tracing::info!(voie = cible, "essai : choix"),
        Err(e) => tracing::info!(raison = e, "essai : choix refusé"),
    }
}
