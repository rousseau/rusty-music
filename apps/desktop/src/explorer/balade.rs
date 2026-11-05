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

use super::*;

use rusty_music_carto::balade::{pente_simple, suivants, Branche, EtatBalade};

/// Le suivant entre dans la file, et n'en bouge plus, à moins de 10 s de la
/// fin du morceau en cours.
const FIGE_AVANT_FIN: Duration = Duration::from_secs(10);
/// Branches proposées dans le graphe des voisins (repli).
const BRANCHES_VOISINS: usize = 3;

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
}

/// Ce qui survit à un redémarrage : de quoi reprendre au même pas.
#[derive(serde::Serialize, serde::Deserialize)]
struct Reprise {
    pistes: Vec<i64>,
    ville: Option<EtatBalade>,
    trace: Vec<[f64; 2]>,
}

fn chemin_reprise(etat: &Etat) -> PathBuf {
    etat.db.with_file_name("balade.json")
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
        }
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

    /// Décide le pas suivant (pente par défaut). `None` : plus rien devant.
    fn decider(&self, lib: &Library) -> Option<Pas> {
        let courant = self.courant_id();
        let (morceau, branche, troncon) = match &self.terrain {
            Terrain::Ville {
                graphe,
                accrochage,
                positions,
                etat,
            } => {
                let branches = suivants(etat, graphe, accrochage);
                let i = pente_simple(etat, &branches, graphe, |id| self.distance(id))?;
                let b = branches[i].clone();
                let mut troncon: Vec<[f64; 2]> = Vec::with_capacity(b.sommets.len() + 2);
                troncon.extend(positions.get(&courant).copied());
                troncon.extend(b.sommets.iter().map(|&s| graphe.point(s)));
                troncon.extend(positions.get(&b.morceau).copied());
                (b.morceau, Some(b), troncon)
            }
            Terrain::Voisins { graphe, vus } => {
                let libres = |k: usize| -> Vec<i64> {
                    graphe
                        .voisins(courant, k)
                        .into_iter()
                        .filter(|id| !vus.contains(id))
                        .collect()
                };
                let mut branches = libres(BRANCHES_VOISINS);
                if branches.is_empty() {
                    // Tous les proches déjà joués : on élargit avant d'abandonner.
                    branches = libres(rusty_music_analysis::chemin::K_VOISINS * 4);
                }
                let m = branches
                    .into_iter()
                    .min_by(|&a, &b| self.distance(a).total_cmp(&self.distance(b)))?;
                (m, None, Vec::new())
            }
        };
        let chemin = PathBuf::from(lib.track(morceau).ok()??.path);
        Some(Pas {
            morceau,
            chemin,
            branche,
            troncon,
            fige: false,
            precharge: false,
        })
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
    })
}

/// « Flâner d'ici » : lance la lecture de `depart` et la balade qui en part.
#[tauri::command(async)]
pub(crate) fn balade_demarrer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    depart: i64,
) -> Result<BaladeVue, String> {
    let mut balade = monter(&etat, &app, vec![depart], None, Vec::new())?;
    {
        let lib = verrou(&etat.lib);
        balade.suivant = balade.decider(&lib);
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
    if verrou(&etat.balade).take().is_some() {
        tracing::info!("balade arrêtée");
    }
    std::fs::remove_file(chemin_reprise(etat)).ok();
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

    if balade.suivant.is_none() {
        let decide = {
            let lib = verrou(&etat.lib);
            balade.decider(&lib)
        };
        match decide {
            Some(p) => balade.suivant = Some(p),
            None => {
                balade.fin = Some("plus aucun morceau devant".into());
                return;
            }
        }
    }

    // À moins de 10 s de la fin — ou plus rien ne joue (⏭, morceau illisible) :
    // le suivant entre dans la file et n'en bouge plus.
    let fin_proche = releve.restant.is_none_or(|r| r <= FIGE_AVANT_FIN);
    let pas = balade.suivant.as_mut().expect("décidé juste au-dessus");
    if pas.fige && releve.prets >= 2 {
        pas.precharge = true;
    }
    if pas.fige || !fin_proche {
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
    match monter(etat, app, r.pistes, r.ville, r.trace) {
        Ok(b) => {
            tracing::info!(pas = b.pistes.len(), "balade reprise");
            *verrou(&etat.balade) = Some(b);
        }
        Err(e) => tracing::warn!(erreur = %e, "balade non reprise"),
    }
}

/// `RUSTY_MUSIC_ESSAI_BALADE=<id>` : lance une balade depuis le morceau `id`
/// au démarrage, **sans le son** (volume à zéro), pour juger une longue balade
/// au journal (`balade : pas`). À lancer sur une copie des données
/// (`RUSTY_MUSIC_DONNEES`) : la session de l'essai remplace celle d'avant.
/// Sans effet en usage normal.
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
        match balade_demarrer(app.clone(), etat, depart) {
            Ok(v) => tracing::info!(depart, terrain = v.terrain, "essai de balade lancé"),
            Err(e) => tracing::warn!(erreur = %e, "essai de balade impossible"),
        }
    });
}
