// SPDX-License-Identifier: GPL-3.0-or-later
//! Explorer : points de carte, graphe des voisins, familles, doublons,
//! sélection, anneau d'albums.

use super::*;

/// Reprojette des positions `[lon, lat]` en mètres locaux (équirectangulaire).
///
/// `direct` et `dessine` cueillent, à chaque pas, le morceau **le plus proche**
/// du point visé, et `dessine` rééchantillonne le tracé à pas d'arc constant.
/// En degrés bruts, ces deux mesures sont faussées : à la latitude de Paris un
/// degré de longitude vaut ~0,66 degré de latitude, donc « le plus proche »
/// tirait vers le nord-sud et le trait ne suivait pas la ligne visée. On projette
/// d'abord en mètres — l'origine est quelconque (translation sans effet ni sur
/// « le plus proche » ni sur une longueur d'arc), seul compte le rapport des
/// axes.
pub(crate) struct RepereLocal {
    lon0: f64,
    lat0: f64,
    cos_lat0: f64,
}

impl RepereLocal {
    pub(crate) fn autour(points: &[(i64, f32, f32)]) -> RepereLocal {
        let n = points.len().max(1) as f64;
        let lon0 = points.iter().map(|p| p.1 as f64).sum::<f64>() / n;
        let lat0 = points.iter().map(|p| p.2 as f64).sum::<f64>() / n;
        RepereLocal {
            lon0,
            lat0,
            cos_lat0: lat0.to_radians().cos(),
        }
    }

    pub(crate) fn projeter(&self, lon: f32, lat: f32) -> (f32, f32) {
        const R: f64 = 6_371_000.0;
        (
            ((lon as f64 - self.lon0).to_radians() * self.cos_lat0 * R) as f32,
            ((lat as f64 - self.lat0).to_radians() * R) as f32,
        )
    }

    /// Reprojette une liste de positions en place — sans effet si elles ne
    /// ressemblent pas à des lon/lat (garde-fou : `points_de_carte_effectifs`
    /// retombe en silence sur le t-SNE si `positions.json` manque, et une
    /// latitude t-SNE tourne autour de 0, pas de 48,8).
    pub(crate) fn projeter_liste(points: &mut [(i64, f32, f32)]) {
        if !ressemble_a_lon_lat(points) {
            return;
        }
        let rep = RepereLocal::autour(points);
        for p in points.iter_mut() {
            let (x, y) = rep.projeter(p.1, p.2);
            (p.1, p.2) = (x, y);
        }
    }
}

/// Les positions ressemblent-elles à des lon/lat (plan de ville réel) plutôt
/// qu'à des coordonnées t-SNE ? La latitude d'une vraie ville est loin de 0 ;
/// le nuage t-SNE tient dans `[-1,1]²`.
pub(crate) fn ressemble_a_lon_lat(points: &[(i64, f32, f32)]) -> bool {
    let Some(&(_, _, lat)) = points.first() else {
        return false;
    };
    lat.abs() > 5.0 && lat.abs() < 89.0
}

/// Les coordonnées de carte de tous les morceaux placés.
///
/// Partagé par les deux modes qui raisonnent à l'écran, `direct` et `dessine`.
/// Bien plus léger que les empreintes — deux flottants par morceau contre 512.
pub(crate) fn points_de_carte(etat: &State<Etat>) -> Result<Vec<(i64, f32, f32)>, String> {
    let lib = verrou(&etat.lib);
    Ok(lib
        .map_points(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?
        .into_iter()
        .map(|(id, x, y, _famille)| (id, x, y))
        .collect())
}

/// Les identifiants des morceaux d'une ou plusieurs familles (clusters de la
/// carte).
///
/// Le filtre par famille du mode Explorer (`carte.isolees`, `ui/js/`) borne
/// aussi le calcul d'un chemin : quand des familles sont isolées, la
/// playlist ne doit contenir que des morceaux de ces familles — les points
/// hors familles sont retirés du nuage avant `chemin::direct`/`dessine`, et
/// le graphe sonique est restreint (voir [`Graphe::restreint`]).
pub(crate) fn morceaux_des_familles(
    etat: &State<Etat>,
    familles: &[i64],
) -> Result<HashSet<i64>, String> {
    let voulues: HashSet<i64> = familles.iter().copied().collect();
    let lib = verrou(&etat.lib);
    Ok(lib
        .map_points(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?
        .into_iter()
        .filter(|(_, _, _, f)| voulues.contains(f))
        .map(|(id, _, _, _)| id)
        .collect())
}

/// Les coordonnées **effectivement affichées** — celles que `versEcran`/
/// `versCarte` d'`js/maplibre.js` utilisent, dans le même repère.
///
/// Sur le plan de ville réel, un morceau n'habite plus sa position t-SNE
/// mais une adresse (`positions.json`, écrit par `engendrer_tuiles` à côté
/// des tuiles) : le lasso, le tracé dessiné et le mode direct doivent
/// raisonner sur cette même adresse, sinon ils testent un contour dessiné
/// sur Paris contre des positions qui n'ont plus aucun rapport avec l'écran.
///
/// **Mais uniquement quand c'est le plan de ville qui est à l'écran.** Le
/// nuage t-SNE et le plan de ville coexistent (bouton « Points » / « Carte »
/// du rail) : dans le nuage, `versEcran`/`versCarte` restent en t-SNE même
/// quand une ville est importée. L'interface passe donc `reel` — vrai
/// seulement quand `carte.affichage === "carte" && villeReelle` — et sur le
/// nuage on retombe sur les positions t-SNE, celles que le geste vise
/// réellement.
///
/// Retombe aussi sur [`points_de_carte`] (t-SNE) si `positions.json` est
/// absent ou vide — le chemin fictif, ou aucune génération de tuiles pas
/// encore lancée.
pub(crate) fn points_de_carte_effectifs(
    etat: &State<Etat>,
    app: &tauri::AppHandle,
    reel: bool,
) -> Result<Vec<(i64, f32, f32)>, String> {
    if reel {
        if let Ok(dossier) = tuiles::dossier(app) {
            if let Ok(texte) = std::fs::read_to_string(dossier.join("positions.json")) {
                if let Ok(reelles) =
                    serde_json::from_str::<std::collections::HashMap<i64, [f32; 2]>>(&texte)
                {
                    if !reelles.is_empty() {
                        return Ok(reelles.into_iter().map(|(id, [x, y])| (id, x, y)).collect());
                    }
                }
            }
        }
    }
    points_de_carte(etat)
}

/// Hydrate une suite d'identifiants en pistes complètes, dans le même ordre.
pub(crate) fn pistes_de(etat: &State<Etat>, route: &[i64]) -> Result<Vec<TrackRow>, String> {
    let lib = verrou(&etat.lib);
    let mut pistes = Vec::with_capacity(route.len());
    for id in route {
        if let Some(t) = lib.track(*id).map_err(echec)? {
            pistes.push(t);
        }
    }
    Ok(pistes)
}

/// Mélange en place, tirage non-déterministe (graine tirée de l'horloge à
/// chaque appel) — même principe que le xorshift64* de `crates/player`,
/// dupliqué ici faute d'être exporté, pour rester sans dépendance nouvelle.
pub(crate) fn melanger_non_deterministe<T>(tranche: &mut [T]) {
    let graine = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    let mut etat = graine;
    let mut suivant = || {
        etat ^= etat << 13;
        etat ^= etat >> 7;
        etat ^= etat << 17;
        etat
    };
    for i in (1..tranche.len()).rev() {
        let j = (suivant() % (i as u64 + 1)) as usize;
        tranche.swap(i, j);
    }
}

/// Construit le graphe des voisins si le nombre d'empreintes a changé.
///
/// Le balayage est complet — une dizaine de secondes sur la bibliothèque
/// entière. Tout se fait donc verrou relâché : ni la base ni le cache des
/// empreintes ne sont tenus pendant ce temps, et l'interface reste servie.
/// L'appelant qui veut éviter l'attente appelle `prepare_graph` en avance.
pub(crate) fn construire_graphe(
    etat: &State<Etat>,
    vecteurs: &[Empreinte],
) -> Result<Arc<Graphe>, String> {
    let n = vecteurs.len();
    // Touche aussi sur un coup au but : sans ça, un `graphe` très sollicité
    // mais jamais reconstruit (la taille ne bouge plus) semblerait inactif à
    // `liberer_si_inactif` après `SEUIL_EVICTION_POIDS`, alors qu'il sert en
    // continu — voir sa documentation.
    let en_cache = |etat: &State<Etat>| -> Option<Arc<Graphe>> {
        let mut c = verrou(&etat.graphe);
        let g = c
            .valeur
            .as_ref()
            .filter(|(taille, _)| *taille == n)
            .map(|(_, g)| Arc::clone(g));
        if g.is_some() {
            c.touche = Some(Instant::now());
        }
        g
    };
    if let Some(g) = en_cache(etat) {
        return Ok(g);
    }

    // Un seul balayage à la fois. Ceux qui attendent ici retrouvent, une fois
    // le verrou obtenu, le cache déjà rempli par le premier — d'où la seconde
    // vérification avant de se lancer à son tour.
    let _construction = verrou(&etat.graphe_construction);
    if let Some(g) = en_cache(etat) {
        return Ok(g);
    }

    let debut = std::time::Instant::now();
    etat.graphe_fait.store(0, Ordering::Relaxed);
    etat.graphe_total.store(n, Ordering::Relaxed);
    let neuf = Arc::new(Graphe::construire_suivi(
        vecteurs,
        rusty_music_analysis::chemin::K_VOISINS,
        coeurs_arriere_plan(),
        &etat.graphe_fait,
    ));
    etat.graphe_total.store(0, Ordering::Relaxed);
    tracing::info!(n, ms = debut.elapsed().as_millis(), "graphe des voisins");
    *verrou(&etat.graphe) = Cache {
        valeur: Some((n, Arc::clone(&neuf))),
        touche: Some(Instant::now()),
    };
    Ok(neuf)
}

/// Prépare le graphe des voisins sans rien tracer.
///
/// L'interface l'appelle en entrant dans le mode Explorer : le balayage a
/// alors le temps de tourner pendant que l'utilisateur regarde la carte,
/// plutôt que de le faire attendre au premier chemin demandé. Rend le nombre
/// de morceaux couverts.
#[tauri::command(async)]
pub(crate) fn prepare_graph(etat: State<Etat>) -> Result<usize, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    Ok(construire_graphe(&etat, &vecteurs)?.taille())
}

/// Les familles de la carte, nommées et comptées.
#[tauri::command(async)]
pub(crate) fn families(etat: State<Etat>) -> Result<Vec<(i64, String, i64)>, String> {
    verrou(&etat.lib)
        .familles(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// La famille sonique dominante de chaque album — le filtre par famille de la
/// grille de pochettes du mode Écoute (`ui/js/`), qui réutilise la légende des
/// familles du mode Explorer.
#[tauri::command(async)]
pub(crate) fn album_families(
    etat: State<Etat>,
) -> Result<Vec<(String, Option<String>, i64)>, String> {
    verrou(&etat.lib)
        .familles_des_albums(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// La famille sonique dominante de chaque artiste (`mb_album_artist_id`) — le
/// filtre par famille du fil du mode Découvrir (`ui/js/`), qui réutilise la
/// légende des familles du mode Explorer.
#[tauri::command(async)]
pub(crate) fn artist_families(etat: State<Etat>) -> Result<Vec<(String, i64)>, String> {
    verrou(&etat.lib)
        .familles_des_artistes(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

#[tauri::command(async)]
pub(crate) fn map_parameters(
    etat: State<Etat>,
) -> Result<rusty_music_core::db::ParametresCarte, String> {
    verrou(&etat.lib).parametres_carte().map_err(echec)
}

/// `cle` doit être un champ de [`rusty_music_core::db::ParametresCarte`] —
/// vérifié ici plutôt que laissé filer jusqu'à la base, qui accepterait
/// n'importe quelle chaîne sans jamais la relire.
#[tauri::command(async)]
pub(crate) fn set_map_parameter(etat: State<Etat>, cle: String, valeur: f64) -> Result<(), String> {
    const CLES: [&str; 7] = [
        "perplexite",
        "epoques",
        "familles",
        "iterations_kmeans",
        "densite_noyau",
        "densite_resolution",
        "densite_bandes",
    ];
    if !CLES.contains(&cle.as_str()) {
        return Err(format!("paramètre inconnu : {cle}"));
    }
    verrou(&etat.lib)
        .set_parametre_carte(&cle, valeur)
        .map_err(echec)
}

/// Le vocabulaire des familles par genre, dans l'ordre d'affichage.
#[tauri::command(async)]
pub(crate) fn vocabulaire_familles(
    etat: State<Etat>,
) -> Result<Vec<(String, Vec<String>)>, String> {
    verrou(&etat.lib).vocabulaire_familles().map_err(echec)
}

/// Remplace le vocabulaire en bloc. Une liste vide restaure les valeurs par
/// défaut — voir `Library::definir_vocabulaire_familles`.
///
/// Ne relance rien elle-même : comme pour `set_map_parameter`, c'est
/// `project` (rappelé par l'interface après coup) qui recalcule la carte
/// avec le nouveau vocabulaire.
#[tauri::command(async)]
pub(crate) fn definir_vocabulaire_familles(
    etat: State<Etat>,
    vocabulaire: Vec<(String, Vec<String>)>,
) -> Result<(), String> {
    verrou(&etat.lib)
        .definir_vocabulaire_familles(&vocabulaire)
        .map_err(echec)
}

/// `passe::Rapport` ne dérive pas `Serialize` — `rusty-music-analysis` ne
/// dépend pas de `serde`, et ce n'est pas ce seul retour de commande qui
/// justifie de le lui ajouter.
#[derive(serde::Serialize)]
pub(crate) struct RapportCarte {
    empreintes: usize,
    familles: usize,
    /// « refaite » (nouveau dessin : perplexité ou époques changées),
    /// « reprise » (nouveaux morceaux logés) ou « gardee » (nuage inchangé).
    projection: &'static str,
}

/// Rejoue la projection t-SNE et le clustering sur les empreintes déjà là,
/// avec les paramètres actuels — pas l'encodage CLAP, qui ne dépend
/// d'aucun d'eux. Bon marché : quelques secondes à quelques dizaines de
/// secondes sur toute la bibliothèque, pas besoin du fil séparé et du
/// sondage qu'`Analyser` demande.
///
/// Le nuage n'est redessiné que si la perplexité ou les époques ont changé
/// (`passe::projeter_tout`) : après un changement de vocabulaire, seules
/// les familles bougent.
#[tauri::command(async)]
pub(crate) fn recompute_map(etat: State<Etat>) -> Result<RapportCarte, String> {
    use rusty_music_analysis::passe::Projection;
    let lib = verrou(&etat.lib);
    let r = rusty_music_analysis::passe::projeter_tout(&lib, None, Projection::Reprise)
        .map_err(|e| e.to_string())?;
    Ok(RapportCarte {
        empreintes: r.empreintes,
        familles: r.familles,
        projection: match r.projection {
            Projection::Refaite => "refaite",
            Projection::Reprise => "reprise",
            Projection::Gardee => "gardee",
        },
    })
}

/// Sous ce carré de distance entre empreintes CLAP, deux morceaux sonnent au
/// point d'être indiscernables — mesuré sur la bibliothèque réelle : la
/// distance médiane entre plus proches voisins y vaut 0,044, son 1ᵉʳ
/// centile 0,0059. En dessous de ce seuil-ci, 66 paires sur 27 042 morceaux
/// — un ordre de grandeur sous le 1ᵉʳ centile, pas un effet de bord de la
/// distribution générale.
pub(crate) const SEUIL_DOUBLON_D2: f32 = 1e-3;

/// Tolérance de durée entre deux morceaux suspectés doublons. Un peu de jeu
/// pour l'arrondi des tags, pas assez pour confondre deux versions
/// réellement différentes (single édité / album, live).
pub(crate) const TOLERANCE_DUREE_MS: i64 = 3000;

#[derive(Clone, serde::Serialize)]
pub(crate) struct DoublonProbable {
    a: TrackRow,
    b: TrackRow,
    distance2: f32,
}

/// Paires de morceaux dont l'empreinte sonore est quasi identique — voir
/// [`SEUIL_DOUBLON_D2`]. Biaisé vers le silence plutôt que le faux
/// positif : un doublon manqué reste juste un doublon, un faux doublon
/// signalé use la confiance dans le reste de la liste.
#[tauri::command(async)]
pub(crate) fn probable_duplicates(etat: State<Etat>) -> Result<Vec<DoublonProbable>, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let graphe = construire_graphe(&etat, &vecteurs)?;
    let lib = verrou(&etat.lib);

    let mut vus: HashSet<(i64, i64)> = HashSet::new();
    let mut paires = Vec::new();
    for (id, voisin, d2) in graphe.plus_proches() {
        if d2 >= SEUIL_DOUBLON_D2 {
            continue;
        }
        // Chaque paire proche se présente deux fois (une fois depuis chaque
        // morceau) : la clé triée ne la garde qu'une.
        let cle = (id.min(voisin), id.max(voisin));
        if !vus.insert(cle) {
            continue;
        }
        let (Some(a), Some(b)) = (
            lib.track(cle.0).map_err(echec)?,
            lib.track(cle.1).map_err(echec)?,
        ) else {
            continue;
        };
        if let (Some(da), Some(db)) = (a.duration_ms, b.duration_ms) {
            if (da - db).abs() > TOLERANCE_DUREE_MS {
                continue;
            }
        }
        paires.push(DoublonProbable {
            a,
            b,
            distance2: d2,
        });
    }
    paires.sort_by(|x, y| x.distance2.total_cmp(&y.distance2));
    Ok(paires)
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct PointIsole {
    piste: TrackRow,
    plus_proche: TrackRow,
    distance2: f32,
}

/// Morceaux dont même le plus proche voisin reste sonoremement lointain —
/// des coins délaissés de la carte, sans repère pour y aller depuis un
/// autre morceau connu. Seuil auto-calibré sur la bibliothèque du moment
/// (99ᵉ centile des distances au plus proche voisin) plutôt qu'une
/// constante absolue : ce qui compte comme « loin » dépend de la densité
/// propre à chaque bibliothèque, pas d'un nombre choisi une fois pour
/// toutes.
#[tauri::command(async)]
pub(crate) fn isolated_points(etat: State<Etat>) -> Result<Vec<PointIsole>, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let graphe = construire_graphe(&etat, &vecteurs)?;
    let lib = verrou(&etat.lib);

    let mut proches = graphe.plus_proches();
    if proches.is_empty() {
        return Ok(Vec::new());
    }
    let mut distances: Vec<f32> = proches.iter().map(|(_, _, d2)| *d2).collect();
    distances.sort_by(f32::total_cmp);
    let seuil = distances[((distances.len() - 1) as f64 * 0.99) as usize];

    proches.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut points = Vec::new();
    for (id, voisin, d2) in proches {
        if d2 < seuil {
            break;
        }
        let (Some(piste), Some(plus_proche)) = (
            lib.track(id).map_err(echec)?,
            lib.track(voisin).map_err(echec)?,
        ) else {
            continue;
        };
        points.push(PointIsole {
            piste,
            plus_proche,
            distance2: d2,
        });
    }
    Ok(points)
}

/// Les morceaux d'une zone dessinée sur la carte.
///
/// `trace` est le contour, en coordonnées de carte. Les morceaux retenus sont
/// rendus **ordonnés en parcours de proche en proche** et non dans l'ordre de
/// la base : une zone donne des dizaines de morceaux, et les enchaîner au
/// hasard produirait une playlist qui saute d'un bout à l'autre.
#[tauri::command(async)]
pub(crate) fn selection(
    app: tauri::AppHandle,
    etat: State<Etat>,
    trace: Vec<(f32, f32)>,
    reel: Option<bool>,
    familles: Option<Vec<i64>>,
) -> Result<Vec<TrackRow>, String> {
    if trace.len() < 3 {
        return Ok(Vec::new());
    }
    let points = points_de_carte_effectifs(&etat, &app, reel.unwrap_or(false))?;
    let permis = match familles.as_deref() {
        Some(fs) if !fs.is_empty() => Some(morceaux_des_familles(&etat, fs)?),
        _ => None,
    };

    let dedans: Vec<i64> = points
        .iter()
        .filter(|(_, x, y)| dans_le_contour(&trace, *x, *y))
        .map(|(id, _, _)| *id)
        .filter(|id| permis.as_ref().is_none_or(|ids| ids.contains(id)))
        .collect();

    let vecteurs = charger_vecteurs(&etat)?;
    let ordonnes = rusty_music_analysis::chemin::parcours(&vecteurs, &dedans);
    tracing::info!(n = ordonnes.len(), "sélection au lasso");
    pistes_de(&etat, &ordonnes)
}

/// Un point est-il dans le contour ? Lancer de rayon, règle pair-impair.
///
/// On compte les côtés qu'une demi-droite partant du point traverse : un
/// nombre impair veut dire dedans. Tient les contours concaves, ce qu'un
/// lasso tracé à la main est presque toujours.
pub(crate) fn dans_le_contour(contour: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut dedans = false;
    let mut j = contour.len() - 1;
    for i in 0..contour.len() {
        let (xi, yi) = contour[i];
        let (xj, yj) = contour[j];
        // Le côté enjambe-t-il l'ordonnée du point ? La comparaison est
        // volontairement asymétrique : sans cela, un sommet exactement à la
        // hauteur du point serait compté deux fois.
        if (yi > y) != (yj > y) {
            let traverse = xi + (y - yi) / (yj - yi) * (xj - xi);
            if x < traverse {
                dedans = !dedans;
            }
        }
        j = i;
    }
    dedans
}

/// Les morceaux qui sonnent le plus comme celui-ci.
///
/// L'inspecteur du modèle « Atelier » les liste sous le morceau courant. Le
/// voisinage se calcule sur les empreintes, jamais sur la carte : t-SNE
/// déforme les distances qu'on cherche justement à comparer.
#[tauri::command(async)]
pub(crate) fn neighbours(
    etat: State<Etat>,
    id: i64,
    count: usize,
) -> Result<Vec<TrackRow>, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let proches = rusty_music_analysis::chemin::voisins(&vecteurs, id, count);
    pistes_de(&etat, &proches)
}

/// Un album trouvé par la recherche du mode Explorer → Anneau.
///
/// `id` voyage en décimal : un hachage 64 bits dépasse la précision d'un
/// `Number` JS. Calculé une fois côté cœur ([`rusty_music_core::db::Library::search_albums`]),
/// jamais recalculé côté interface — voir la note sur `hacher` dans
/// `crates/core/src/db.rs`.
#[derive(serde::Serialize)]
pub(crate) struct AlbumHit {
    id: String,
    name: String,
    artist: String,
}

/// Recherche d'albums pour la barre `#q` en mode Explorer → Anneau.
#[tauri::command(async)]
pub(crate) fn search_albums(
    etat: State<Etat>,
    query: String,
    limit: i64,
) -> Result<Vec<AlbumHit>, String> {
    Ok(verrou(&etat.lib)
        .search_albums(&query, limit)
        .map_err(echec)?
        .into_iter()
        .map(|(id, name, artist)| AlbumHit {
            id: id.to_string(),
            name,
            artist,
        })
        .collect())
}

/// Un nœud album, partagé par le mode Explorer → Anneau (focal ou voisin) et
/// la frise des filiations du mode Explorer → Temps.
#[derive(serde::Serialize)]
pub(crate) struct AnneauNoeud {
    id: String,
    name: String,
    artist: String,
    famille: Option<i64>,
    /// Famille secondaire d'un album à cheval sur deux familles, `None` sinon
    /// — voir [`rusty_music_core::db::Library::familles_secondaires_albums`].
    /// Toujours `None` pour un [`AnneauVoisin`] construit par [`album_ring`] :
    /// l'anneau n'a pas besoin de cette nuance, seule la frise colore un arc
    /// entrant par elle.
    famille_secondaire: Option<i64>,
    path: Option<String>,
    /// Somme des durées de ses morceaux, en ms — épaisseur de son arc sur le
    /// second anneau (voir `AlbumNoeud::duree_ms`).
    duree_ms: i64,
    /// Année de sortie, `None` tant qu'aucune date fiable n'est connue (voir
    /// `AlbumNoeud::date`, 0 = inconnue).
    annee: Option<i32>,
}

impl From<&AlbumNoeud> for AnneauNoeud {
    fn from(a: &AlbumNoeud) -> Self {
        Self {
            id: a.id.to_string(),
            name: a.name.clone(),
            artist: a.artist.clone(),
            famille: a.famille,
            famille_secondaire: None,
            path: a.path.clone(),
            duree_ms: a.duree_ms,
            annee: (a.date > 0).then_some((a.date / 10_000) as i32),
        }
    }
}

#[derive(serde::Serialize)]
pub(crate) struct AnneauVoisin {
    #[serde(flatten)]
    noeud: AnneauNoeud,
    /// Vrai si cet album précède le focal dans le temps, faux s'il le suit
    /// (ou à égalité — voir `Library::albums_annotes`, une date de
    /// résolution à la journée).
    avant: bool,
    /// Distance au carré entre les centroïdes d'empreinte du focal et de ce
    /// voisin — épaisseur du lien : plus proche, plus épais. Brute (pas de
    /// racine ni de normalisation, l'échelle n'a de sens que relative aux
    /// autres voisins du même focal) ; à l'interface de la ramener à une
    /// épaisseur de trait parmi les voisins affichés.
    distance: f32,
}

#[derive(serde::Serialize)]
pub(crate) struct AnneauVue {
    focal: AnneauNoeud,
    voisins: Vec<AnneauVoisin>,
}

/// Les k albums les plus proches d'un album focal par empreinte, scindés en
/// antérieurs/postérieurs — mode Explorer → Anneau.
///
/// Balayage linéaire sur les centroïdes d'album (voir `charger_centroides_albums`
/// et la doc de `Library::album_embeddings` sur pourquoi aucun graphe n'est
/// construit ici) : pas de mise en cache par requête, la bibliothèque compte
/// trop peu d'albums pour que ce soit utile.
#[tauri::command(async)]
pub(crate) fn album_ring(
    etat: State<Etat>,
    album_id: String,
    k: usize,
) -> Result<AnneauVue, String> {
    let id: i64 = album_id
        .parse()
        .map_err(|_| format!("identifiant d'album invalide : {album_id}"))?;
    let (centroides, noeuds) = charger_centroides_albums(&etat)?;

    let focal = noeuds
        .get(&id)
        .ok_or_else(|| "album inconnu ou sans morceau analysé".to_string())?;

    let vecteur = |i: i64| {
        centroides
            .iter()
            .find(|(vid, _)| *vid == i)
            .map(|(_, v)| v.as_slice())
    };
    let vecteur_focal = vecteur(id);

    let proches = rusty_music_analysis::chemin::voisins(&centroides, id, k);
    let voisins = proches
        .into_iter()
        .filter_map(|vid| Some((noeuds.get(&vid)?, vecteur(vid)?)))
        .map(|(v, vv)| AnneauVoisin {
            noeud: v.into(),
            avant: v.date < focal.date,
            distance: match vecteur_focal {
                Some(vf) => vf.iter().zip(vv).map(|(a, b)| (a - b) * (a - b)).sum(),
                None => 0.0,
            },
        })
        .collect();

    Ok(AnneauVue {
        focal: focal.into(),
        voisins,
    })
}

/// Centroïde moyen des albums de chaque artiste, à partir des centroïdes
/// d'album déjà chargés — pas de nouvelle requête SQL, une simple agrégation
/// d'un cran supplémentaire sur une donnée déjà en cache.
pub(crate) fn centroides_par_artiste(
    centroides: &CentroidesAlbums,
    noeuds: &NoeudsAlbums,
) -> HashMap<String, Vec<f32>> {
    let mut cumul: HashMap<String, (Vec<f32>, u32)> = HashMap::new();
    for (id, v) in centroides {
        let Some(noeud) = noeuds.get(id) else {
            continue;
        };
        let e = cumul
            .entry(noeud.artist.clone())
            .or_insert_with(|| (vec![0.0; v.len()], 0));
        for (a, b) in e.0.iter_mut().zip(v) {
            *a += b;
        }
        e.1 += 1;
    }
    cumul
        .into_iter()
        .map(|(nom, (somme, n))| {
            let n = (n.max(1)) as f32;
            (nom, somme.into_iter().map(|x| x / n).collect())
        })
        .collect()
}

/// Les k artistes les plus proches sonorement d'un artiste donné — panneau
/// central du mode Écoute, sous la grille d'albums de cet artiste.
///
/// Même esprit que [`album_ring`] un cran plus haut : réutilise les
/// centroïdes d'album déjà en cache, agrégés par artiste, puis un balayage
/// linéaire — une bibliothèque compte bien moins d'artistes que d'albums,
/// inutile d'y consacrer un graphe.
#[tauri::command(async)]
pub(crate) fn artistes_proches(
    etat: State<Etat>,
    artiste: String,
    k: usize,
) -> Result<Vec<(String, f32)>, String> {
    let (centroides, noeuds) = charger_centroides_albums(&etat)?;
    let par_artiste = centroides_par_artiste(&centroides, &noeuds);
    let Some(focal) = par_artiste.get(&artiste) else {
        return Ok(Vec::new());
    };

    let mut scores: Vec<(String, f32)> = par_artiste
        .iter()
        .filter(|(nom, _)| *nom != &artiste)
        .map(|(nom, v)| {
            let d = focal.iter().zip(v).map(|(a, b)| (a - b) * (a - b)).sum();
            (nom.clone(), d)
        })
        .collect();
    scores.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(k);
    Ok(scores)
}

/// Un lien du fond permanent de l'anneau — voir `reseau_albums`. `distance`
/// sert à trier/plafonner côté moteur ; l'interface s'en sert aussi pour
/// moduler l'opacité, à l'identique de `AnneauVoisin::distance`.
#[derive(serde::Serialize)]
pub(crate) struct AnneauArcBase {
    a: String,
    b: String,
    distance: f32,
}

#[derive(serde::Serialize)]
pub(crate) struct AnneauReseau {
    /// Chaque album qui a une famille connue — c'est sur cette liste que
    /// l'interface calcule une bonne fois pour toutes la position de chacun
    /// sur l'anneau (voir `docs/carto-anneau.md` § stabilité des positions) :
    /// jamais recalculée au changement d'album focal.
    albums: Vec<AnneauNoeud>,
    /// Un sous-ensemble représentatif des liens de tout le réseau — le plus
    /// proche voisin de chaque album, dédoublonné — affiché en permanence et
    /// très estompé, pour la texture tissée que la référence Eigenfactor a
    /// déjà même sans sélection (voir `docs/carto-anneau.md` § état de base).
    arcs: Vec<AnneauArcBase>,
}

/// Le fond permanent du mode Explorer → Anneau et de la frise des filiations
/// (mode Explorer → Temps) : tous les albums connus et un échantillon de
/// leurs liens de plus proche voisinage — voir `AnneauReseau`. Chargé une
/// fois par session par chacun des deux modes (voir `chargerReseauAnneau`/
/// `chargerReseauFrise` côté interface), jamais reconstruit au changement de
/// focal ou d'album survolé.
#[tauri::command(async)]
pub(crate) fn reseau_albums(etat: State<Etat>, limite_arcs: usize) -> Result<AnneauReseau, String> {
    let (centroides, noeuds) = charger_centroides_albums(&etat)?;
    let graphe = charger_graphe_albums(&etat, &centroides)?;
    let secondaires = verrou(&etat.lib)
        .familles_secondaires_albums(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;

    let albums = noeuds
        .values()
        .filter(|n| n.famille.is_some())
        .map(|n| {
            let mut noeud: AnneauNoeud = n.into();
            noeud.famille_secondaire = secondaires.get(&n.id).copied();
            noeud
        })
        .collect();

    let mut vues: HashSet<(i64, i64)> = HashSet::new();
    let mut arcs: Vec<(i64, i64, f32)> = Vec::new();
    for (id, voisin, d2) in graphe.plus_proches() {
        if id == voisin {
            continue;
        }
        let paire = if id < voisin {
            (id, voisin)
        } else {
            (voisin, id)
        };
        if vues.insert(paire) {
            arcs.push((id, voisin, d2));
        }
    }
    // Les liens les plus forts d'abord : si la limite tranche, elle retire
    // les plus faibles, jamais l'inverse.
    arcs.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    arcs.truncate(limite_arcs);
    let arcs = arcs
        .into_iter()
        .filter(|(a, b, _)| {
            noeuds.get(a).is_some_and(|n| n.famille.is_some())
                && noeuds.get(b).is_some_and(|n| n.famille.is_some())
        })
        .map(|(a, b, distance)| AnneauArcBase {
            a: a.to_string(),
            b: b.to_string(),
            distance,
        })
        .collect();

    Ok(AnneauReseau { albums, arcs })
}

/// Playlist « dans l'esprit de l'album » : une errance ordinaire, mais partie
/// du morceau le plus central de l'album plutôt que d'un seul morceau choisi
/// à la main. `chemin::parcours` donne ce pivot — le morceau dont l'empreinte
/// est la plus proche de la moyenne de l'album — et filtre déjà les morceaux
/// sans empreinte, donc un album partiellement analysé n'est pas un problème.
#[tauri::command(async)]
pub(crate) fn path_album(
    etat: State<Etat>,
    album: String,
    artist: Option<String>,
    steps: usize,
    seed: u64,
    bruit: Option<f32>,
) -> Result<Vec<TrackRow>, String> {
    let ids: Vec<i64> = {
        let lib = verrou(&etat.lib);
        lib.tracks_of_album(&album, artist.as_deref())
            .map_err(echec)?
            .into_iter()
            .map(|t| t.id)
            .collect()
    };

    let vecteurs = charger_vecteurs(&etat)?;
    let pivot = *rusty_music_analysis::chemin::parcours(&vecteurs, &ids)
        .first()
        .ok_or("aucun morceau analysé pour cet album")?;

    let route = construire_graphe(&etat, &vecteurs)?.errance(
        pivot,
        steps,
        seed,
        bruit.unwrap_or(BRUIT_DEFAUT),
    );
    pistes_de(&etat, &route)
}

/// Playlist « dans l'esprit de l'artiste » — même principe que [`path_album`],
/// mais le pivot est cherché sur l'ensemble des morceaux de l'artiste plutôt
/// qu'un seul album : le bouton « radio » du bandeau « autour de cet artiste »
/// (panneau central du mode Écoute), à côté de « Sonne comme ».
#[tauri::command(async)]
pub(crate) fn path_artist(
    etat: State<Etat>,
    artist: String,
    mbid: Option<String>,
    steps: usize,
    seed: u64,
    bruit: Option<f32>,
) -> Result<Vec<TrackRow>, String> {
    let ids: Vec<i64> = {
        let lib = verrou(&etat.lib);
        let albums = lib
            .albums_of_artist(mbid.as_deref(), &artist)
            .map_err(echec)?;
        let mut ids = Vec::new();
        for album in albums {
            ids.extend(
                lib.tracks_of_album(&album.name, album.artist.as_deref())
                    .map_err(echec)?
                    .into_iter()
                    .map(|t| t.id),
            );
        }
        ids
    };

    let vecteurs = charger_vecteurs(&etat)?;
    let pivot = *rusty_music_analysis::chemin::parcours(&vecteurs, &ids)
        .first()
        .ok_or("aucun morceau analysé pour cet artiste")?;

    let route = construire_graphe(&etat, &vecteurs)?.errance(
        pivot,
        steps,
        seed,
        bruit.unwrap_or(BRUIT_DEFAUT),
    );
    pistes_de(&etat, &route)
}

/// Playlist mélangée de tous les morceaux d'un artiste — un vrai tirage
/// aléatoire, renouvelé à chaque appel, pas une dérive sonique comme
/// `path_artist` : aucun besoin de vecteurs CLAP, fonctionne même pour un
/// artiste non analysé. Bouton « Playlist de cet artiste » du bandeau
/// « autour de cet artiste » (panneau central du mode Écoute).
#[tauri::command(async)]
pub(crate) fn shuffle_artist(
    etat: State<Etat>,
    artist: String,
    mbid: Option<String>,
) -> Result<Vec<TrackRow>, String> {
    let mut pistes = {
        let lib = verrou(&etat.lib);
        let albums = lib
            .albums_of_artist(mbid.as_deref(), &artist)
            .map_err(echec)?;
        let mut pistes = Vec::new();
        for album in albums {
            pistes.extend(
                lib.tracks_of_album(&album.name, album.artist.as_deref())
                    .map_err(echec)?,
            );
        }
        pistes
    };
    melanger_non_deterministe(&mut pistes);
    Ok(pistes)
}

/// La famille du morceau `id` — la plupart des vues de piste ne portent pas
/// déjà ce numéro (seuls les points de la carte l'ont) : l'inspecteur ne
/// l'appelle donc que faute de mieux, avant de lire le nom voté dans le
/// cache de `families_detail`. `None` sur un morceau pas encore projeté.
#[tauri::command(async)]
pub(crate) fn famille_piste(etat: State<Etat>, id: i64) -> Result<Option<i64>, String> {
    verrou(&etat.lib)
        .cluster_de_piste(rusty_music_analysis::passe::MODELE, id)
        .map_err(echec)
}

/// Le détail du vote de nommage des familles — trois propositions et un
/// drapeau de confiance par famille (`docs/nommage-familles.md`). `families`
/// reste le nom simple qu'utilisent la carte, l'anneau et la frise ; cette
/// commande sert le nouveau bloc de l'inspecteur (« nom incertain »),
/// chargée une fois et mise en cache côté JS comme `families`.
#[tauri::command(async)]
pub(crate) fn families_detail(
    etat: State<Etat>,
) -> Result<Vec<rusty_music_core::db::VoteFamille>, String> {
    verrou(&etat.lib)
        .familles_votees(rusty_music_analysis::passe::MODELE)
        .map_err(echec)
}

/// Affinage local suggéré dans l'inspecteur, jamais automatique
/// (`docs/nommage-familles.md`) : redécoupe les seuls morceaux de la
/// famille `cluster` par un k-means restreint, puis nomme chaque
/// sous-groupe par le même vote à trois sources. N'écrit rien dans
/// `features.cluster` — les identifiants de sous-groupe rendus n'ont de
/// sens que pour cet appel, jamais réutilisés par la carte, l'anneau ou la
/// frise.
#[tauri::command(async)]
pub(crate) fn subdiviser_famille(
    etat: State<Etat>,
    cluster: i64,
) -> Result<Vec<rusty_music_core::db::VoteFamille>, String> {
    const K: usize = 3;
    let lib = verrou(&etat.lib);
    let modele = rusty_music_analysis::passe::MODELE;
    let empreintes = lib.embeddings_du_cluster(modele, cluster).map_err(echec)?;
    if empreintes.is_empty() {
        return Ok(Vec::new());
    }
    let vecteurs: Vec<Vec<f32>> = empreintes.iter().map(|(_, v)| v.clone()).collect();
    let groupes = rusty_music_analysis::cluster::subdiviser(&vecteurs, K);
    let assignations: Vec<(i64, i64)> = empreintes
        .iter()
        .zip(groupes)
        .map(|((id, _), g)| (*id, g as i64))
        .collect();
    lib.sous_groupes_votes(modele, &assignations).map_err(echec)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un lasso tracé à la main est presque toujours concave : c'est le cas
    /// que la règle pair-impair doit tenir, et qu'une enveloppe convexe
    /// raterait.
    #[test]
    fn le_contour_tient_les_formes_concaves() {
        // Un « U » : ouvert vers le haut, avec un creux au milieu.
        let u = [
            (-1.0, -1.0),
            (-1.0, 1.0),
            (-0.5, 1.0),
            (-0.5, -0.5),
            (0.5, -0.5),
            (0.5, 1.0),
            (1.0, 1.0),
            (1.0, -1.0),
        ];
        assert!(dans_le_contour(&u, -0.75, 0.0), "branche gauche");
        assert!(dans_le_contour(&u, 0.75, 0.0), "branche droite");
        assert!(!dans_le_contour(&u, 0.0, 0.5), "le creux est dehors");
        assert!(dans_le_contour(&u, 0.0, -0.75), "le fond est dedans");
        assert!(!dans_le_contour(&u, 2.0, 0.0), "loin dehors");
    }

    #[test]
    fn un_carre_se_comporte_comme_un_carre() {
        let c = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
        assert!(dans_le_contour(&c, 0.5, 0.5));
        assert!(!dans_le_contour(&c, 1.5, 0.5));
        assert!(!dans_le_contour(&c, 0.5, 1.5));
        assert!(!dans_le_contour(&c, -0.5, 0.5));
    }

    #[test]
    fn repere_local_corrige_lanisotropie_lon_lat() {
        // Trois points autour de Notre-Dame : l'un ~730 m à l'est (Δlon), l'autre
        // ~1110 m au nord (Δlat). En degrés bruts, 0,01° dans les deux sens
        // paraîtraient à égale distance ; en mètres, l'est est bien plus proche.
        let ancre = (0i64, 2.35_f32, 48.85_f32);
        let est = (1i64, 2.36_f32, 48.85_f32);
        let nord = (2i64, 2.35_f32, 48.86_f32);
        let mut pts = vec![ancre, est, nord];

        // En degrés bruts, un écart de 0,01° pèse pareil au nord et à l'est —
        // c'est justement le biais que la projection corrige.
        let d2_deg =
            |a: (i64, f32, f32), b: (i64, f32, f32)| (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2);
        assert!((d2_deg(ancre, est) - d2_deg(ancre, nord)).abs() < 1e-6);

        RepereLocal::projeter_liste(&mut pts);
        let d2 = |a: usize, b: usize| (pts[a].1 - pts[b].1).powi(2) + (pts[a].2 - pts[b].2).powi(2);
        // 0 = ancre, 1 = est (~730 m), 2 = nord (~1110 m).
        assert!(
            d2(0, 1) < d2(0, 2),
            "après projection, l'est doit être le plus proche"
        );
        let est_m = d2(0, 1).sqrt();
        assert!(
            (est_m - 730.0).abs() < 40.0,
            "~730 m attendus, obtenu {est_m}"
        );
    }
}
