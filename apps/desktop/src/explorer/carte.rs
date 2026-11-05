// SPDX-License-Identifier: GPL-3.0-or-later
//! Explorer : plan de ville, tuiles, itinéraires sur la voirie, chemins.

use super::*;

/// La carte du module 2 : tous les morceaux déjà placés.
///
/// Chargée d'un bloc — 27 000 points tiennent largement, et l'interface les
/// redemande seulement quand l'utilisateur le veut. Tant que l'analyse tourne,
/// chaque rappel en ramène davantage.
#[tauri::command(async)]
pub(crate) fn map_view(etat: State<Etat>) -> Result<Vec<MapPoint>, String> {
    let pts = verrou(&etat.lib)
        .map_view(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?;
    tracing::info!(n = pts.len(), "map_view");
    Ok(pts)
}

/// Le plan de ville à afficher, ou `None` pour retomber sur le monde procédural.
///
/// À côté de la base : soit importé par l'utilisateur (`carto ville --sortie`),
/// soit installé du paquet au démarrage ([`installer_plan_de_ville`]). Un
/// fichier vide — le repli d'un `cargo build` sans `ville-paris.db`
/// (`apps/desktop/build.rs`) qu'un paquet peut embarquer — compte pour absent.
pub(crate) fn plan_de_ville(db: &Path) -> Option<PathBuf> {
    let p = db.with_file_name("ville-paris.db");
    match std::fs::metadata(&p) {
        Ok(m) if m.is_file() && m.len() > 1_000_000 => Some(p),
        _ => None,
    }
}

/// Installe le plan de ville du paquet à côté de la base si l'utilisateur n'en
/// a pas déjà un. Copie unique ; un `carto ville` ultérieur écrase.
pub(crate) fn installer_plan_de_ville(app: &tauri::App, dossier: &Path) {
    let cible = dossier.join("ville-paris.db");
    if cible.exists() {
        return;
    }
    let Ok(res) = app.path().resource_dir() else {
        return;
    };
    let source = res.join("ville-paris.db");
    // Le paquet peut embarquer un `ville-paris.db` vide (repli de `build.rs`) :
    // on ne copie que du vrai.
    if std::fs::metadata(&source).map(|m| m.len()).unwrap_or(0) < 1_000_000 {
        return;
    }
    match std::fs::create_dir_all(dossier).and_then(|_| std::fs::copy(&source, &cible)) {
        Ok(_) => tracing::info!("plan de ville (Paris) installé depuis le paquet"),
        Err(e) => tracing::warn!(%e, "plan de ville du paquet non installé"),
    }
}

/// Charge `ville-paris.db` une fois, la garde en mémoire ensuite — relire une
/// vingtaine de mégaoctets à chaque « refaire les tuiles » serait un gâchis.
/// Pas d'invalidation : la ville se remplace en bloc (`carto ville`), jamais
/// en place, et un nouvel import survient hors session.
pub(crate) fn charger_ville(
    etat: &State<Etat>,
    chemin: &Path,
) -> Result<Arc<rusty_music_osm::Extrait>, String> {
    let mut cache = verrou(&etat.ville);
    if let Some(extrait) = &*cache {
        return Ok(extrait.clone());
    }
    let extrait = Arc::new(rusty_music_osm::base::lire(chemin).map_err(echec)?);
    *cache = Some(extrait.clone());
    Ok(extrait)
}

/// Le graphe routable du plan de ville, construit une fois puis gardé — même
/// principe que [`charger_ville`], et sur le même extrait.
pub(crate) fn charger_graphe_reel(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
) -> Result<Arc<rusty_music_carto::reseau_reel::Graphe>, String> {
    let mut cache = verrou(&etat.graphe_reel);
    if let Some(graphe) = &*cache {
        return Ok(graphe.clone());
    }
    let graphe = Arc::new(rusty_music_carto::reseau_reel::Graphe::construire(extrait));
    *cache = Some(graphe.clone());
    Ok(graphe)
}

/// La grille « aux abords d'un parc ou de l'eau », construite une fois par
/// session — ne dépend que de l'extrait (profil panoramique).
pub(crate) fn charger_agrement_voirie(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
) -> Result<Arc<rusty_music_carto::cout_itineraire::ProximiteAgrement>, String> {
    let mut cache = verrou(&etat.agrement_voirie);
    if let Some(a) = &*cache {
        return Ok(a.clone());
    }
    let a =
        Arc::new(rusty_music_carto::cout_itineraire::ProximiteAgrement::nouvelle(extrait, 120.0));
    *cache = Some(a.clone());
    Ok(a)
}

/// Un graphe de voirie pondéré par profil, mis en cache. Le jeu de sommets est
/// identique à celui de `graphe_reel` : seule la pondération d'arête change.
pub(crate) fn charger_graphe_voirie(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
    profil: rusty_music_carto::cout_itineraire::ProfilVoirie,
) -> Result<Arc<rusty_music_carto::reseau_reel::Graphe>, String> {
    use rusty_music_carto::cout_itineraire::friction_itineraire;

    if let Some(g) = verrou(&etat.graphes_voirie).get(&profil) {
        return Ok(g.clone());
    }
    let agrement = charger_agrement_voirie(etat, extrait)?;
    let graphe = Arc::new(rusty_music_carto::reseau_reel::Graphe::construire_pondere(
        extrait,
        friction_itineraire(profil, Some(&agrement)),
    ));
    verrou(&etat.graphes_voirie).insert(profil, graphe.clone());
    Ok(graphe)
}

/// Accroche chaque morceau (son adresse réelle, `positions.json`) au sommet de
/// voirie le plus proche. Bâti une fois sur `graphe_base` — l'indexation des
/// sommets ne dépend pas de la pondération, donc l'accrochage vaut pour tous
/// les graphes de voirie pondérés.
///
/// `Err` si aucune position réelle n'est disponible (pas de plan de ville, ou
/// tuiles pas encore générées) — l'appelant en fait un repli.
pub(crate) fn charger_accrochage_voirie(
    etat: &State<Etat>,
    app: &tauri::AppHandle,
    graphe_base: &rusty_music_carto::reseau_reel::Graphe,
) -> Result<Arc<AccrochageVoirie>, String> {
    {
        let cache = verrou(&etat.accrochage_voirie);
        if let Some(a) = &*cache {
            return Ok(a.clone());
        }
    }

    // `points_de_carte_effectifs(reel = true)` lit `positions.json` : `(id,
    // lon, lat)`. Sur le chemin fictif il retombe sur le t-SNE — sans rapport
    // avec une rue —, d'où le garde-fou `positions.json` ci-dessous.
    let dossier = tuiles::dossier(app).map_err(echec)?;
    if !dossier.join("positions.json").is_file() {
        return Err("carte réelle pas encore générée".into());
    }
    let points = points_de_carte_effectifs(etat, app, true)?;
    if points.is_empty() {
        return Err("aucune adresse réelle".into());
    }

    let a = AccrochageVoirie::construire(graphe_base, points);
    tracing::info!(
        accroches = a.sommet_de.len(),
        sommets = a.morceaux_a.len(),
        "morceaux accrochés à la voirie"
    );
    let a = Arc::new(a);
    *verrou(&etat.accrochage_voirie) = Some(a.clone());
    Ok(a)
}

/// Popularité par morceau, `[0, 1]` — proxy « nombre de morceaux gardés de
/// l'artiste », faute de compteur d'écoute (comme [`construire_reseau`]). Sert
/// de « dénivelé » le long d'un itinéraire.
pub(crate) fn popularites_par_artiste(vue: &[MapPoint]) -> std::collections::HashMap<i64, f32> {
    let mut par_artiste: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for p in vue {
        *par_artiste
            .entry(p.artist.clone().unwrap_or_default())
            .or_default() += 1;
    }
    let max = par_artiste.values().copied().max().unwrap_or(1).max(1) as f32;
    vue.iter()
        .map(|p| {
            let n = par_artiste[&p.artist.clone().unwrap_or_default()] as f32;
            (p.id, n / max)
        })
        .collect()
}

/// Assemble une [`rusty_music_carto::source::Source`] depuis le plan de
/// ville réel plutôt que depuis le monde engendré — voir
/// `rusty_music_carto::ville::rassembler`.
pub(crate) fn rassembler_ville(
    etat: &State<Etat>,
    extrait: &rusty_music_osm::Extrait,
) -> Result<rusty_music_carto::source::Source, String> {
    let modele = rusty_music_analysis::passe::MODELE;
    let lib = verrou(&etat.lib);
    let vue = lib.map_view(modele).map_err(echec)?;
    if vue.is_empty() {
        return Err("aucun morceau sur la carte : lancer l'analyse d'abord".into());
    }
    let noms_famille: std::collections::HashMap<i64, String> = lib
        .familles(modele)
        .map_err(echec)?
        .into_iter()
        .map(|(id, nom, _)| (id, nom))
        .collect();
    drop(lib);
    // Les empreintes, pour les pentes entre quartiers (P3 de la balade).
    let empreintes: std::collections::HashMap<i64, Vec<f32>> = charger_vecteurs(etat)?
        .iter()
        .map(|(id, v)| (*id, v.clone()))
        .collect();

    let r = rusty_music_carto::ville::rassembler(
        extrait,
        &vue,
        &noms_famille,
        Some(rusty_music_carto::ville::ILE_DE_LA_CITE),
        Some(&empreintes),
    );
    // Pas de `curiosites` sur le plan de ville : `style::couches_ville` ne les
    // rend plus (pastilles brunes plus grosses qu'un bâtiment). Le dispositif
    // monument/refuge/fondation reste propre au monde fictif.
    tracing::info!(
        adresses = r.adresses_posees,
        sans_adresse = r.morceaux_sans_adresse,
        artistes_ancres = r.artistes_ancres,
        batiments_peuples = r.batiments_peuples,
        cellules = r.cellules,
        cellules_habitees = r.cellules_habitees,
        albums = r.source.albums.len(),
        pentes = r.source.pentes.len(),
        "plan de ville réel assemblé"
    );
    Ok(r.source)
}

/// Fabrique les deux archives. Quelques secondes sur 27 000 morceaux — la
/// commande est `async`, donc hors du fil de l'interface.
///
/// Deux chemins : si `ville-paris.db` existe à côté de la bibliothèque
/// (importée via `carto ville`), la carte affiche le vrai plan de ville
/// ([`rassembler_ville`]) — pas de relief à ombrer, Paris est plat. Sinon,
/// repli sur le monde fictif engendré depuis la bibliothèque
/// ([`rassembler`]), inchangé.
#[tauri::command(async)]
pub(crate) fn engendrer_tuiles(app: tauri::AppHandle, etat: State<Etat>) -> Result<String, String> {
    let (src, paliers, ombrage_rapport) = if let Some(chemin_ville) = plan_de_ville(&etat.db) {
        let extrait = charger_ville(&etat, &chemin_ville)?;
        let src = rassembler_ville(&etat, &extrait)?;
        (src, rusty_music_carto::tuiles::Paliers::ville(), None)
    } else {
        tracing::info!(
            "aucune ville importée — carte procédurale de repli ; lancer \
             `carto ville <extrait.osm.pbf> --commune <nom>` pour Paris"
        );
        let (src, champ, resolution) = rassembler(&etat)?;
        let ombrage = rusty_music_carto::relief::Ombrage::default();
        let rr = rusty_music_carto::relief::ecrire(
            &champ,
            resolution,
            &ombrage,
            &tuiles::chemin_relief(&app).map_err(echec)?,
        )
        .map_err(echec)?;
        (src, rusty_music_carto::tuiles::Paliers::default(), Some(rr))
    };

    let chemin = tuiles::chemin_carte(&app).map_err(echec)?;
    let r = rusty_music_carto::tuiles::ecrire(&src, &paliers, &chemin).map_err(echec)?;

    // Le style part avec les tuiles : même source, mêmes paliers, aucune
    // dérive possible entre ce qui est dans les tuiles et ce qui s'affiche.
    // `construire` retire lui-même la source « relief » sur le plan de ville
    // réel (`Source::est_ville_reelle`) — rien à faire ici pour l'omettre.
    //
    // **Une variante de style par palette de fond de plan.** N'affecte que
    // `style.json`, jamais les tuiles : reconstruire le style est du pur JSON,
    // presque gratuit face à la `Source` (déjà bâtie ci-dessus). L'interface
    // bascule ensuite entre ces fichiers sans régénérer quoi que ce soit.
    // `osm-clair` prend le nom sans suffixe (le style lu par défaut).
    let dossier_style = tuiles::dossier(&app).map_err(echec)?;
    for palette in rusty_music_carto::Palette::toutes() {
        let style = rusty_music_carto::style::construire(&src, &paliers, &tuiles::base(), palette);
        let nom = if palette.id == "osm-clair" {
            "style.json".to_string()
        } else {
            format!("style-{}.json", palette.id)
        };
        std::fs::write(
            dossier_style.join(nom),
            serde_json::to_vec_pretty(&style).map_err(echec)?,
        )
        .map_err(echec)?;
    }

    // Les positions réelles (lon/lat) des morceaux, pour que la surcouche du
    // canevas (lasso, survol, chemin dessiné) retrouve chaque morceau là où
    // les tuiles le montrent. Sans ce fichier, l'interface (`ui/js/`) n'a que les
    // coordonnées t-SNE de `map_view` — sans aucun rapport avec une adresse
    // de rue une fois le plan de ville réel actif. Absent (donc en échec à
    // la lecture) sur le chemin fictif : c'est le signal que l'interface (`ui/js/`) utilise
    // pour savoir dans quel repère il travaille.
    let chemin_positions = tuiles::dossier(&app).map_err(echec)?.join("positions.json");
    if src.est_ville_reelle() {
        let positions: std::collections::HashMap<i64, [f32; 2]> =
            src.morceaux.iter().map(|m| (m.id, [m.x, m.y])).collect();
        std::fs::write(
            &chemin_positions,
            serde_json::to_vec(&positions).map_err(echec)?,
        )
        .map_err(echec)?;
    } else {
        std::fs::remove_file(&chemin_positions).ok();
    }

    // L'ancienne archive reste projetée en mémoire tant qu'on ne la lâche pas.
    app.state::<tuiles::Archives>().oublier();

    // Les tuiles viennent de changer : `positions.json` avec, donc tout ce qui
    // en dépend (l'accrochage des morceaux à la voirie, les graphes pondérés,
    // la grille d'agrément, et l'extrait / le graphe de base eux-mêmes en cas
    // de réimport). On repart de zéro à la prochaine demande d'itinéraire.
    *verrou(&etat.ville) = None;
    *verrou(&etat.graphe_reel) = None;
    *verrou(&etat.accrochage_voirie) = None;
    verrou(&etat.graphes_voirie).clear();
    *verrou(&etat.agrement_voirie) = None;

    match ombrage_rapport {
        Some(rr) => {
            tracing::info!(
                tuiles = r.tuiles,
                relief = rr.tuiles,
                secondes = r.duree.as_secs_f64() + rr.duree.as_secs_f64(),
                "tuiles engendrées"
            );
            Ok(format!(
                "{} tuiles ({:.1} Mo) et {} d'ombrage ({:.1} Mo) en {:.1} s",
                r.tuiles,
                r.octets as f64 / 1_048_576.0,
                rr.tuiles,
                rr.octets as f64 / 1_048_576.0,
                r.duree.as_secs_f64() + rr.duree.as_secs_f64()
            ))
        }
        None => {
            tracing::info!(
                tuiles = r.tuiles,
                secondes = r.duree.as_secs_f64(),
                "tuiles engendrées (plan de ville réel)"
            );
            Ok(format!(
                "{} tuiles ({:.1} Mo) en {:.1} s — plan de ville réel (© les contributeurs OpenStreetMap)",
                r.tuiles,
                r.octets as f64 / 1_048_576.0,
                r.duree.as_secs_f64()
            ))
        }
    }
}

/// Un itinéraire, tel que l'interface le reçoit.
#[derive(serde::Serialize)]
pub(crate) struct ItineraireVu {
    pistes: Vec<MapPoint>,
    duree_ms: u64,
    distance_sonique: f32,
    /// Le dénivelé : la popularité le long du trajet.
    popularite: Vec<f32>,
    classes: Vec<String>,
}

/// Trace un itinéraire dans le réseau de circulation.
///
/// `carto-google-maps.md` §3 : un seul graphe, plusieurs fonctions de coût. La
/// durée cible est la contrainte la plus utile côté utilisateur — « un
/// itinéraire de 40 minutes » est une vraie demande.
#[tauri::command(async)]
pub(crate) fn itineraire(
    etat: State<Etat>,
    depart: i64,
    arrivee: Option<i64>,
    profil: String,
    minutes: Option<u64>,
    familles: Option<Vec<i64>>,
) -> Result<Vec<ItineraireVu>, String> {
    use rusty_music_analysis::reseau::{Options, Profil};

    let modele = rusty_music_analysis::passe::MODELE;
    {
        // Construction paresseuse, une seule fois par session.
        let mut cache = verrou(&etat.reseau);
        if cache.is_none() {
            *cache = Some(construire_reseau(&etat, modele)?);
        }
    }
    let cache = verrou(&etat.reseau);
    let reseau = cache.as_ref().expect("réseau construit juste au-dessus");

    let mut o = Options::nouveau(
        depart,
        match profil.as_str() {
            "sentier" => Profil::Sentier,
            "panoramique" => Profil::Panoramique,
            _ => Profil::Autoroute,
        },
    );
    o.arrivee = arrivee;
    o.alternatives = 1;
    o.duree_cible_ms = minutes.map(|m| m * 60_000);
    // Familles isolées dans Explorer : le trajet n'en sort pas (le réseau est
    // construit une fois, la restriction se fait à chaque requête).
    if let Some(fs) = familles.filter(|fs| !fs.is_empty()) {
        o = o.dans_les_familles(fs);
    }

    let trajets = reseau.itineraires(&o).map_err(echec)?;
    drop(cache);

    let lib = verrou(&etat.lib);
    let par_id: std::collections::HashMap<i64, MapPoint> = lib
        .map_view(modele)
        .map_err(echec)?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    drop(lib);
    trajets
        .into_iter()
        .map(|t| {
            let pistes: Vec<MapPoint> = t
                .morceaux
                .iter()
                .filter_map(|id| par_id.get(id).cloned())
                .collect();
            Ok(ItineraireVu {
                pistes,
                duree_ms: t.duree_ms,
                distance_sonique: t.distance_sonique,
                popularite: t.popularite,
                classes: t
                    .classes
                    .iter()
                    .map(|c| format!("{c:?}").to_lowercase())
                    .collect(),
            })
        })
        .collect()
}

/// Un itinéraire routé sur la voirie réelle, tel que l'interface le reçoit.
#[derive(serde::Serialize)]
pub(crate) struct ItineraireVoirieVu {
    /// Les morceaux rencontrés le long des rues, dans l'ordre — la playlist.
    pistes: Vec<MapPoint>,
    /// La ligne du trajet, déjà routée sur les rues : `[lon, lat]`. L'interface
    /// n'a plus à rappeler `trace_rues`.
    polyligne: Vec<[f64; 2]>,
    /// Somme des durées des `pistes`.
    duree_ms: u64,
    /// Longueur de la `polyligne`, en mètres.
    distance_m: f64,
    /// Le dénivelé : la popularité le long du trajet, un `f32` par piste.
    popularite: Vec<f32>,
    /// Les classes de voie traversées, doublons consécutifs retirés.
    classes: Vec<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct ReponseItineraireVoirie {
    /// `Some(raison)` : l'interface affiche la raison et retombe sur
    /// l'itinéraire musical (`itineraire`).
    repli: Option<String>,
    trajets: Vec<ItineraireVoirieVu>,
}

impl ReponseItineraireVoirie {
    fn repli(raison: &str) -> ReponseItineraireVoirie {
        ReponseItineraireVoirie {
            repli: Some(raison.into()),
            trajets: Vec::new(),
        }
    }
}

/// Distance équirectangulaire entre deux points `[lon, lat]`, en mètres — même
/// approximation que `carto::reseau_reel` et `osm::Troncon::longueur_m`.
pub(crate) fn distance_lonlat_m(a: [f64; 2], b: [f64; 2]) -> f64 {
    let lat_moy = (a[1] + b[1]).to_radians() / 2.0;
    let dx = (b[0] - a[0]).to_radians() * lat_moy.cos() * 6_371_000.0;
    let dy = (b[1] - a[1]).to_radians() * 6_371_000.0;
    (dx * dx + dy * dy).sqrt()
}

/// Trace un itinéraire **sur les vraies rues** et compose la playlist des
/// morceaux qui les bordent.
///
/// `docs/carto-ville.md` (révisé) : sur le plan de ville, ce mode ne se
/// contente plus d'habiller le trait d'un chemin musical — c'est la voirie qui
/// choisit les morceaux. Le chemin musical (`itineraire`) reste le repli quand
/// il n'y a pas de ville, pas d'adresse, ou dans le nuage.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn itineraire_voirie(
    app: tauri::AppHandle,
    etat: State<Etat>,
    depart: i64,
    arrivee: Option<i64>,
    profil: String,
    minutes: Option<u64>,
    familles: Option<Vec<i64>>,
    rayon_m: Option<f64>,
) -> Result<ReponseItineraireVoirie, String> {
    use rusty_music_carto::accrochage::{destination_pour_duree, fin_de_trace, morceaux_le_long};
    use rusty_music_carto::cout_itineraire::ProfilVoirie;

    let Some(chemin_ville) = plan_de_ville(&etat.db) else {
        return Ok(ReponseItineraireVoirie::repli("aucune ville importée"));
    };
    let extrait = charger_ville(&etat, &chemin_ville)?;
    let graphe_base = charger_graphe_reel(&etat, &extrait)?;
    let accrochage = match charger_accrochage_voirie(&etat, &app, &graphe_base) {
        Ok(a) => a,
        Err(_) => {
            return Ok(ReponseItineraireVoirie::repli(
                "carte réelle pas encore générée — refaire les tuiles",
            ))
        }
    };
    let profil_v = ProfilVoirie::depuis_nom(&profil);
    let graphe = charger_graphe_voirie(&etat, &extrait, profil_v)?;

    // Positions réelles (lon/lat) des morceaux.
    let positions: std::collections::HashMap<i64, [f64; 2]> =
        points_de_carte_effectifs(&etat, &app, true)?
            .into_iter()
            .map(|(id, x, y)| (id, [x as f64, y as f64]))
            .collect();
    let Some(&depart_pos) = positions.get(&depart) else {
        return Ok(ReponseItineraireVoirie::repli(
            "ce morceau n'a pas d'adresse sur la carte",
        ));
    };
    if graphe.accrocher(depart_pos).is_none() {
        return Ok(ReponseItineraireVoirie::repli(
            "le départ n'est rattaché à aucune rue",
        ));
    }
    let arrivee_pos = match arrivee {
        Some(a) => match positions.get(&a) {
            Some(&p) if graphe.accrocher(p).is_some() => Some(p),
            _ => {
                return Ok(ReponseItineraireVoirie::repli(
                    "l'arrivée n'a pas d'adresse sur la carte",
                ))
            }
        },
        None => None,
    };

    let permis: Option<HashSet<i64>> = match familles.as_deref() {
        Some(fs) if !fs.is_empty() => Some(morceaux_des_familles(&etat, fs)?),
        _ => None,
    };

    // `map_view` une fois : hydratation, durées, popularité.
    let modele = rusty_music_analysis::passe::MODELE;
    let vue = {
        let lib = verrou(&etat.lib);
        lib.map_view(modele).map_err(echec)?
    };
    let par_id: std::collections::HashMap<i64, MapPoint> =
        vue.iter().map(|p| (p.id, p.clone())).collect();
    let duree_ms_de: std::collections::HashMap<i64, u64> = vue
        .iter()
        .map(|p| (p.id, p.duration_ms.unwrap_or(0).max(0) as u64))
        .collect();
    let pop_de = popularites_par_artiste(&vue);
    let duree = |id: i64| duree_ms_de.get(&id).copied().unwrap_or(0);

    // Cible = position d'arrivée, sinon un morceau assez loin pour qu'il y ait
    // ~`minutes` de musique en chemin.
    let cible_ms = minutes.filter(|m| *m > 0).map(|m| m * 60_000);
    let cible_pos = match (arrivee_pos, cible_ms) {
        (Some(p), _) => p,
        (None, Some(cible)) => {
            let cible_id = destination_pour_duree(
                &graphe,
                &accrochage,
                depart,
                depart_pos,
                permis.as_ref(),
                cible,
                &duree,
            );
            match cible_id.and_then(|id| positions.get(&id)) {
                Some(&p) => p,
                None => {
                    return Ok(ReponseItineraireVoirie::repli(
                        "aucun morceau accessible pour cette durée",
                    ))
                }
            }
        }
        (None, None) => {
            return Ok(ReponseItineraireVoirie::repli(
                "choisir une arrivée ou une durée",
            ));
        }
    };

    // Un seul trajet : le meilleur pour ce profil. Les « variantes » (Yen /
    // pénalité) ne donnaient que des versions dégradées du même profil — le
    // choix, c'est le profil lui-même.
    let Some((trace, _)) = graphe.chemin_sommets(depart_pos, cible_pos) else {
        return Ok(ReponseItineraireVoirie::repli("pas de route jusque-là"));
    };

    // Table classe par tronçon, pour nommer les voies traversées.
    let classe_de: std::collections::HashMap<i64, rusty_music_osm::Classe> =
        extrait.troncons.iter().map(|t| (t.id, t.classe)).collect();
    let rayon = rayon_m.unwrap_or(25.0).max(0.0);

    let couloir = graphe.couloir(&trace, rayon);
    let route = morceaux_le_long(
        &accrochage,
        &couloir,
        depart,
        arrivee,
        permis.as_ref(),
        cible_ms,
        &duree,
    );
    let pistes: Vec<MapPoint> = route
        .iter()
        .filter_map(|id| par_id.get(id).cloned())
        .collect();
    if pistes.len() < 2 {
        return Ok(ReponseItineraireVoirie::repli(
            "aucun morceau le long de cet itinéraire — essayer une autre durée ou un autre profil",
        ));
    }

    // **Avec une arrivée, le tracé va jusqu'à elle** — on garde tout. Sans
    // arrivée, `chemin_sommets` file jusqu'à une destination provisoire (~la
    // durée de musique plus loin) ; si les morceaux sont denses près du départ,
    // la playlist s'arrête bien avant, et dessiner la polyligne entière puis
    // rejoindre le dernier morceau (souvent revenu près du départ) faisait une
    // boucle. On tronque donc au dernier morceau retenu.
    let fin = if arrivee.is_some() {
        trace.len()
    } else {
        fin_de_trace(&route, &accrochage, &couloir, trace.len())
    };
    let trace = &trace[..fin];

    let polyligne: Vec<[f64; 2]> = trace.iter().map(|&s| graphe.point(s)).collect();
    let distance_m = polyligne
        .windows(2)
        .map(|w| distance_lonlat_m(w[0], w[1]))
        .sum();
    let duree_ms = pistes.iter().map(|p| duree(p.id)).sum();
    let popularite: Vec<f32> = pistes
        .iter()
        .map(|p| pop_de.get(&p.id).copied().unwrap_or(0.0))
        .collect();
    let mut classes: Vec<String> = Vec::new();
    for t in graphe.troncons_traverses(trace) {
        if let Some(c) = classe_de.get(&t) {
            let nom = c.nom().to_string();
            if classes.last() != Some(&nom) {
                classes.push(nom);
            }
        }
    }

    Ok(ReponseItineraireVoirie {
        repli: None,
        trajets: vec![ItineraireVoirieVu {
            pistes,
            polyligne,
            duree_ms,
            distance_m,
            popularite,
            classes,
        }],
    })
}

/// Rassemble ce qu'il faut au réseau et le construit.
pub(crate) fn construire_reseau(
    etat: &State<Etat>,
    modele: &str,
) -> Result<rusty_music_analysis::reseau::Reseau, String> {
    use rusty_music_analysis::reseau::{Morceau, Parametres, Reseau};
    use std::collections::HashMap;

    let lib = verrou(&etat.lib);
    let empreintes = lib.embeddings(modele).map_err(echec)?;
    let vue = lib.map_view(modele).map_err(echec)?;
    if vue.is_empty() {
        return Err("aucun morceau sur la carte".into());
    }
    let points = lib.map_points(modele).map_err(echec)?;
    let mut parametres = lib.parametres_carte().map_err(echec)?.parametres_densite();
    drop(lib);

    // La popularité dont on dispose : le nombre de morceaux gardés d'un
    // artiste. La base ne porte aucun compteur d'écoute.
    let mut par_artiste: HashMap<String, u32> = HashMap::new();
    for p in &vue {
        *par_artiste
            .entry(p.artist.clone().unwrap_or_default())
            .or_default() += 1;
    }
    let index: HashMap<&str, u32> = par_artiste
        .keys()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i as u32))
        .collect();
    let morceaux: Vec<Morceau> = vue
        .iter()
        .map(|p| {
            let a = p.artist.clone().unwrap_or_default();
            Morceau {
                id: p.id,
                duree_ms: p.duration_ms.unwrap_or(0).max(0) as u64,
                artiste: index[a.as_str()],
                famille: p.cluster,
                x: p.x,
                y: p.y,
                morceaux_de_lartiste: par_artiste[&a],
            }
        })
        .collect();

    parametres.noyau = rusty_music_carto::relief::NOYAU;
    let champ = rusty_music_core::density::champ_global(&points, &parametres);
    Ok(Reseau::construire(
        empreintes,
        &morceaux,
        &champ,
        parametres.resolution,
        &Parametres {
            fils: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(8),
            ..Default::default()
        },
    ))
}

/// Faut-il lancer l'autotest de la carte ?
///
/// `RUSTY_MUSIC_AUTOTEST=1`. Une webview du système ne se pilote pas de
/// l'extérieur : sans ce banc, les interactions ne se vérifient qu'à la main,
/// donc ne se vérifient pas.
#[tauri::command(async)]
pub(crate) fn autotest_carte() -> bool {
    std::env::var("RUSTY_MUSIC_AUTOTEST").is_ok_and(|v| v != "0")
}

/// Renvoie au journal du processus ce que la console de la webview dit.
///
/// Une webview du système n'a pas d'inspecteur accessible depuis l'extérieur :
/// sans ce renvoi, une carte qui ne s'affiche pas ne dit rien du tout. C'est ce
/// qui a permis de trouver que MapLibre démarre dans la fenêtre principale et
/// pas dans une seconde.
#[tauri::command(async)]
pub(crate) fn journal_carte(niveau: String, message: String) {
    match niveau.as_str() {
        "error" => tracing::error!(target: "carte", "{message}"),
        "warn" => tracing::warn!(target: "carte", "{message}"),
        _ => tracing::info!(target: "carte", "{message}"),
    }
}

/// Les octets d'une tuile, pour `maplibregl.addProtocol`.
///
/// Rend un tableau vide quand la tuile n'existe pas : c'est le cas ordinaire
/// sur un monde creux, et MapLibre s'en accommode sans le compter comme une
/// erreur.
#[tauri::command]
pub(crate) async fn tuile(
    app: tauri::AppHandle,
    quoi: String,
    z: u8,
    x: u32,
    y: u32,
) -> Result<tauri::ipc::Response, String> {
    if !tuiles::archive_valide(&quoi) {
        return Err(format!("archive inconnue : {quoi}"));
    }
    let octets = tuiles::lire(&app, &quoi, z, x, y)
        .await
        .map_err(echec)?
        .unwrap_or_default();
    Ok(tauri::ipc::Response::new(octets))
}

/// Le style MapLibre, lu à côté des archives.
///
/// Il est **écrit en même temps que les tuiles**, et c'est délibéré : les deux
/// partagent les mêmes paliers et la même liste de familles, ils ne peuvent
/// plus diverger. Le recalculer à l'ouverture coûtait 42 secondes mesurées —
/// `Library::familles` refait tout l'arbitrage des genres MusicBrainz sur
/// 27 000 morceaux — pour un résultat identique à celui de la veille.
///
/// `theme` choisit la palette de fond de plan : `engendrer_tuiles` a écrit un
/// `style-<id>.json` par palette de [`rusty_music_carto::Palette`]. L'`id` est
/// filtré contre `Palette::par_id` (pas de `../`, pas de nom arbitraire), et on
/// retombe sur `style.json` si le fichier de thème manque — tuiles engendrées
/// avant cette fonctionnalité, ou thème `osm-clair`.
#[tauri::command(async)]
pub(crate) fn style_carte(
    app: tauri::AppHandle,
    theme: Option<String>,
) -> Result<serde_json::Value, String> {
    let dossier = tuiles::dossier(&app).map_err(echec)?;
    let chemin = match theme.as_deref() {
        Some(id)
            if id != "osm-clair"
                && rusty_music_carto::Palette::par_id(id).is_some()
                && dossier.join(format!("style-{id}.json")).is_file() =>
        {
            dossier.join(format!("style-{id}.json"))
        }
        _ => dossier.join("style.json"),
    };
    let texte = std::fs::read_to_string(&chemin)
        .map_err(|e| format!("style introuvable ({}) : {e}", chemin.display()))?;
    let mut style: serde_json::Value = serde_json::from_str(&texte).map_err(echec)?;
    // Un style écrit avant la Règle 11 garde des étiquettes sous 12 px : on les
    // remonte en le servant, sans régénérer les tuiles.
    rusty_music_carto::style::plancher_etiquettes(&mut style);
    Ok(style)
}

/// Les positions réelles (lon/lat) des morceaux sur le plan de ville, écrites
/// par `engendrer_tuiles` à côté du style. Échoue sur le chemin fictif — pas
/// d'erreur applicative, `ui/js/` lit cet échec comme « pas de plan de ville
/// réel actif » et retombe sur les coordonnées t-SNE de `map_view`.
#[tauri::command(async)]
pub(crate) fn positions_carte(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let chemin = tuiles::dossier(&app).map_err(echec)?.join("positions.json");
    let texte = std::fs::read_to_string(&chemin)
        .map_err(|e| format!("positions introuvables ({}) : {e}", chemin.display()))?;
    serde_json::from_str(&texte).map_err(echec)
}

/// Rassemble ce qu'il faut aux deux archives : morceaux, familles, bandes, et
/// le champ continu du relief.
pub(crate) fn rassembler(
    etat: &State<Etat>,
) -> Result<(rusty_music_carto::source::Source, Vec<f64>, usize), String> {
    use rusty_music_carto::source;
    let modele = rusty_music_analysis::passe::MODELE;
    let lib = verrou(&etat.lib);

    let vue = lib.map_view(modele).map_err(echec)?;
    if vue.is_empty() {
        return Err("aucun morceau sur la carte : lancer l'analyse d'abord".into());
    }
    let vue_gardee = vue.clone();
    let lib_ordre = lib.ordre_darrivee().map_err(echec)?;
    let empreintes_gardees: std::collections::HashMap<i64, Vec<f32>> =
        lib.embeddings(modele).map_err(echec)?.into_iter().collect();
    let familles: Vec<source::Famille> = lib
        .familles(modele)
        .map_err(echec)?
        .into_iter()
        .map(|(id, nom, effectif)| source::Famille {
            id,
            nom,
            effectif: effectif as usize,
        })
        .collect();
    let points = lib.map_points(modele).map_err(echec)?;
    let mut parametres = lib.parametres_carte().map_err(echec)?.parametres_densite();
    drop(lib);

    let nappe = rusty_music_core::density::calculer(&points, &parametres);
    // Le relief veut un champ plus lisse que les territoires : au noyau des
    // contours (0,02) l'ombrage ressemble à du papier froissé — vérifié à
    // l'œil sur la bibliothèque réelle.
    parametres.noyau = rusty_music_carto::relief::NOYAU;
    let champ = rusty_music_core::density::champ_global(&points, &parametres);

    // Le peuplement et le réseau entrent dans les tuiles au même titre que les
    // territoires : sans lieux ni routes, la carte n'est qu'une nappe.
    let ordre = lib_ordre;
    let par_id: std::collections::HashMap<i64, &MapPoint> =
        vue_gardee.iter().map(|p| (p.id, p)).collect();
    let arrivants: Vec<rusty_music_carto::peuplement::Arrivant> = ordre
        .iter()
        .filter_map(|a| {
            let p = par_id.get(&a.track_id)?;
            Some(rusty_music_carto::peuplement::Arrivant {
                track_id: a.track_id,
                x: p.x,
                y: p.y,
                empreinte: empreintes_gardees
                    .get(&a.track_id)
                    .cloned()
                    .unwrap_or_default(),
                famille: p.cluster,
                date: a.date,
                artiste: p.artist.clone().unwrap_or_default(),
            })
        })
        .collect();
    let peupl = rusty_music_carto::peuplement::peupler(
        &arrivants,
        &rusty_music_carto::peuplement::Parametres::default(),
    );
    tracing::info!(
        etablissements = peupl.rapport.etablissements,
        iles = peupl.rapport.iles,
        "peuplement"
    );

    // **La parcelle remplace la coordonnée t-SNE.** C'est ce qui fait que les
    // morceaux habitent la carte au lieu de flotter dessus. Le CLI le faisait
    // déjà ; ce chemin-ci ne le faisait pas, et le bouton « refaire les
    // tuiles » rendait donc une carte différente de celle du CLI.
    let parcelles: std::collections::HashMap<i64, (f32, f32)> = peupl
        .habitants
        .iter()
        .map(|h| (h.track_id, (h.x, h.y)))
        .collect();
    let morceaux: Vec<source::Morceau> = vue_gardee
        .iter()
        .filter_map(|p| {
            let &(x, y) = parcelles.get(&p.id)?;
            Some(source::Morceau {
                id: p.id,
                x,
                y,
                famille: p.cluster,
                titre: p.title.clone().unwrap_or_default(),
                artiste: p.artist.clone().unwrap_or_default(),
                annee: p.year.map(|a| a as i32),
                bpm: p.bpm,
                energie: p.energy,
            })
        })
        .collect();

    // Le réseau de circulation, construit ici plutôt que réutilisé depuis
    // l'état : `rassembler` tourne dans une commande qui tient déjà le verrou
    // de la bibliothèque, et le réseau met une trentaine de secondes — le
    // partager demanderait de réordonner les verrous pour rien.
    let reseau = construire_reseau(etat, modele)?;
    let etab_de: std::collections::HashMap<i64, u32> = peupl
        .habitants
        .iter()
        .map(|h| (h.track_id, h.etablissement))
        .collect();
    let centres: std::collections::HashMap<u32, (f32, f32)> = peupl
        .etablissements
        .iter()
        .map(|e| (e.id, (e.cx, e.cy)))
        .collect();
    let routes_gardees = source::reseau_entre_lieux(
        &reseau.troncons_identifies(),
        &etab_de,
        &centres,
        &champ,
        parametres.resolution,
    );

    let rivieres = rusty_music_carto::hydro::tracer(
        &champ,
        parametres.resolution,
        &rusty_music_carto::hydro::Parametres::default(),
    );
    let curiosites =
        source::curiosites(&morceaux, &peupl.etablissements, &reseau.refuges(0.995), 60);

    Ok((
        source::Source {
            morceaux,
            familles,
            bandes: nappe.bandes,
            routes: routes_gardees,
            etablissements: peupl.etablissements,
            rivieres,
            curiosites,
            ..Default::default()
        },
        champ,
        parametres.resolution,
    ))
}

/// Trace un chemin et rend les pistes traversées, dans l'ordre du trajet.
///
/// `mode` choisit la fabrique — `chemin.rs` documente ce qui les distingue :
///
/// - `direct` : de `from` à `to`, en droite sur la carte, en `steps` morceaux ;
/// - `sonique` : de `from` à `to`, de voisin en voisin, longueur libre
///   plafonnée à `steps` ;
/// - `errance` : depuis `from` seul, `steps` morceaux tirés au sort.
///
/// Un mode inconnu retombe sur `direct` plutôt que d'échouer : l'interface est
/// la seule à appeler, une faute de frappe y est un bogue à voir, pas une
/// erreur à afficher.
///
/// `bruit` (0 à 1) est le cadran commun aux quatre modes — voir la note en
/// tête de `chemin.rs`. `0` reproduit le trajet exact ; les quatre fabriques
/// le traduisent chacune dans son propre registre (softmax pour l'errance,
/// arêtes bruitées pour le sonique, pont brownien pour le direct et le
/// dessiné). Réglable par un curseur du rail, visible dans les 4 modes.
pub(crate) const BRUIT_DEFAUT: f32 = 0.3;

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)] // interface de commande Tauri : chaque champ vient du JS
pub(crate) fn path(
    app: tauri::AppHandle,
    etat: State<Etat>,
    from: i64,
    to: Option<i64>,
    mode: Option<String>,
    steps: usize,
    seed: Option<u64>,
    bruit: Option<f32>,
    reel: Option<bool>,
    familles: Option<Vec<i64>>,
) -> Result<Vec<TrackRow>, String> {
    use rusty_music_analysis::chemin;
    let mode = mode.unwrap_or_else(|| "direct".into());
    let graine = seed.unwrap_or(1);
    let bruit = bruit.unwrap_or(BRUIT_DEFAUT);
    let reel = reel.unwrap_or(false);
    let debut = std::time::Instant::now();

    // Filtre par famille : quand des familles sont isolées dans Explorer, le
    // chemin ne doit traverser qu'elles. Le nuage passé au mode direct est
    // amputé des autres familles ; le graphe sonique est restreint.
    let permis = match familles.as_deref() {
        Some(fs) if !fs.is_empty() => Some(morceaux_des_familles(&etat, fs)?),
        _ => None,
    };
    let carte_filtree =
        |etat: &State<Etat>, app: &tauri::AppHandle| -> Result<Vec<(i64, f32, f32)>, String> {
            let mut p = points_de_carte_effectifs(etat, app, reel)?;
            // Sur le plan de ville réel, les positions sont des lon/lat : on les
            // projette en mètres pour que « le plus proche » se mesure comme à
            // l'écran (voir [`RepereLocal`]). Le nuage t-SNE, lui, est déjà isotrope.
            if reel {
                RepereLocal::projeter_liste(&mut p);
            }
            if let Some(ref ids) = permis {
                p.retain(|(id, _, _)| ids.contains(id));
            }
            Ok(p)
        };

    // Le direct raisonne sur la carte, les deux autres sur les empreintes : on
    // ne charge que ce que le mode demande. Les empreintes font 55 Mo.
    let route = match mode.as_str() {
        "errance" => {
            let vecteurs = charger_vecteurs(&etat)?;
            let base = construire_graphe(&etat, &vecteurs)?;
            let restreint;
            let graphe: &Graphe = match &permis {
                Some(ids) => {
                    restreint = base.restreint(ids);
                    &restreint
                }
                None => &base,
            };
            graphe.errance(from, steps, graine, bruit)
        }
        "sonique" => {
            let arrivee = to.ok_or("le mode sonique demande une arrivée")?;
            let vecteurs = charger_vecteurs(&etat)?;
            let base = construire_graphe(&etat, &vecteurs)?;
            let restreint;
            let graphe: &Graphe = match &permis {
                Some(ids) => {
                    restreint = base.restreint(ids);
                    &restreint
                }
                None => &base,
            };
            let complet = graphe.sonique(from, arrivee, graine, bruit);
            if complet.is_empty() {
                // Les deux morceaux ne communiquent pas dans le graphe : mieux
                // vaut la droite que rien du tout. Le journal le dit,
                // l'interface l'annonce.
                tracing::info!(from, arrivee, "sonique impossible, repli sur direct");
                chemin::direct(
                    &carte_filtree(&etat, &app)?,
                    from,
                    arrivee,
                    steps,
                    graine,
                    bruit,
                )
            } else {
                chemin::echantillonner(&complet, steps.max(2))
            }
        }
        _ => {
            let arrivee = to.ok_or("le mode direct demande une arrivée")?;
            chemin::direct(
                &carte_filtree(&etat, &app)?,
                from,
                arrivee,
                steps,
                graine,
                bruit,
            )
        }
    };

    let pistes = pistes_de(&etat, &route)?;
    tracing::info!(
        mode,
        n = pistes.len(),
        ms = debut.elapsed().as_millis(),
        "chemin tracé"
    );
    Ok(pistes)
}

/// Voyage dans le temps depuis `from` : le seul mode de chemin du mode
/// Explorer → Temps (voir `MODES_CHEMIN.temps` côté `ui/js/`) — une marche
/// sonique qui n'avance jamais en arrière chronologiquement, voir
/// [`rusty_music_analysis::chemin::Graphe::voyage`]. `bruit` et `seed`
/// jouent le même rôle que pour [`path`].
#[tauri::command(async)]
pub(crate) fn voyage(
    etat: State<Etat>,
    from: i64,
    steps: usize,
    seed: Option<u64>,
    bruit: Option<f32>,
) -> Result<Vec<TrackRow>, String> {
    let graine = seed.unwrap_or(1);
    let bruit = bruit.unwrap_or(BRUIT_DEFAUT);
    let debut = std::time::Instant::now();

    let vecteurs = charger_vecteurs(&etat)?;
    let graphe = construire_graphe(&etat, &vecteurs)?;

    let ordre: HashMap<i64, u32> = verrou(&etat.lib)
        .ordre_darrivee()
        .map_err(echec)?
        .into_iter()
        .enumerate()
        .map(|(rang, a)| (a.track_id, rang as u32))
        .collect();
    let familles: HashMap<i64, i64> = verrou(&etat.lib)
        .map_points(rusty_music_analysis::passe::MODELE)
        .map_err(echec)?
        .into_iter()
        .map(|(id, _, _, f)| (id, f))
        .collect();

    let route = graphe.voyage(from, &ordre, &familles, steps, bruit, graine);
    let pistes = pistes_de(&etat, &route)?;
    tracing::info!(
        n = pistes.len(),
        ms = debut.elapsed().as_millis(),
        "voyage tracé"
    );
    Ok(pistes)
}

/// Chemin suivant un tracé dessiné à la souris sur la carte.
///
/// `trace` est la suite des points parcourus, dans le repère de la carte
/// (`[-1, 1]` sur les deux axes — ou en lon/lat sur le plan de ville réel,
/// voir [`points_de_carte_effectifs`]) ; `radius` la distance au-delà de
/// laquelle on ne cueille rien. L'interface la calcule depuis le zoom
/// courant, pour que le trait attrape exactement ce qu'il touche à l'écran.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)] // interface de commande Tauri : chaque champ vient du JS
pub(crate) fn path_drawn(
    app: tauri::AppHandle,
    etat: State<Etat>,
    trace: Vec<(f32, f32)>,
    steps: usize,
    radius: f32,
    seed: Option<u64>,
    bruit: Option<f32>,
    reel: Option<bool>,
    familles: Option<Vec<i64>>,
) -> Result<Vec<TrackRow>, String> {
    let reel = reel.unwrap_or(false);
    let mut points = points_de_carte_effectifs(&etat, &app, reel)?;
    let mut trace = trace;
    // Plan de ville réel : positions ET tracé sont des lon/lat. On les projette
    // dans le même repère métrique (voir [`RepereLocal`]) ; `radius` arrive déjà
    // en mètres depuis l'interface (`tracerDessin`, branche `carteReelle`).
    if reel && ressemble_a_lon_lat(&points) {
        let rep = RepereLocal::autour(&points);
        for p in &mut points {
            let (x, y) = rep.projeter(p.1, p.2);
            (p.1, p.2) = (x, y);
        }
        for t in &mut trace {
            let (x, y) = rep.projeter(t.0, t.1);
            *t = (x, y);
        }
    }
    // Filtre par famille : le trait ne cueille que dans les familles isolées.
    if let Some(fs) = familles.as_deref().filter(|fs| !fs.is_empty()) {
        let ids = morceaux_des_familles(&etat, fs)?;
        points.retain(|(id, _, _)| ids.contains(id));
    }
    let route = rusty_music_analysis::chemin::dessine(
        &points,
        &trace,
        steps,
        radius,
        seed.unwrap_or(1),
        bruit.unwrap_or(BRUIT_DEFAUT),
    );
    let pistes = pistes_de(&etat, &route)?;
    tracing::info!(n = pistes.len(), points = trace.len(), "chemin dessiné");
    Ok(pistes)
}

/// Les segments de rues réelles entre chaque paire consécutive d'un
/// itinéraire déjà choisi — habille le trait dessiné sur le plan de ville
/// réel, sans toucher au choix des morceaux. Ce choix reste entièrement
/// celui du réseau **sonique** (`itineraire` ci-dessus, ou `path`/
/// `path_drawn` pour direct/dessiné) : la question ici n'est pas « quels
/// morceaux » mais « quel trait dessiner entre eux ».
///
/// `ids` est la liste ordonnée des morceaux d'un chemin déjà tracé ; le
/// résultat a un élément de moins, un `Vec` de points `[lon, lat]` par paire
/// consécutive. Un `Vec` vide en position `i` marque un segment sans chemin
/// trouvé (l'une des deux adresses n'a pas de position réelle, ou les deux
/// n'appartiennent pas à la même composante routable) — `ui/js/` retombe
/// alors sur le trait synthétique existant pour ce segment précis, pas pour
/// tout l'itinéraire.
#[tauri::command(async)]
pub(crate) fn trace_rues(
    app: tauri::AppHandle,
    etat: State<Etat>,
    ids: Vec<i64>,
) -> Result<Vec<Vec<[f64; 2]>>, String> {
    if ids.len() < 2 {
        return Ok(Vec::new());
    }
    let Some(chemin_ville) = plan_de_ville(&etat.db) else {
        return Ok(Vec::new());
    };
    let extrait = charger_ville(&etat, &chemin_ville)?;
    let graphe = charger_graphe_reel(&etat, &extrait)?;
    // Habillage du trait par les vraies rues : n'a de sens que sur le plan de
    // ville, et raisonne sur les adresses réelles quelle que soit la vue.
    let points = points_de_carte_effectifs(&etat, &app, true)?;
    let position = |id: i64| {
        points
            .iter()
            .find(|(i, _, _)| *i == id)
            .map(|(_, x, y)| [*x as f64, *y as f64])
    };

    let segments: Vec<Vec<[f64; 2]>> = ids
        .windows(2)
        .map(|paire| match (position(paire[0]), position(paire[1])) {
            (Some(a), Some(b)) => graphe.chemin(a, b).unwrap_or_default(),
            _ => Vec::new(),
        })
        .collect();
    tracing::info!(
        n = segments.iter().filter(|s| !s.is_empty()).count(),
        sur = segments.len(),
        "trait habillé de rues réelles"
    );
    Ok(segments)
}
