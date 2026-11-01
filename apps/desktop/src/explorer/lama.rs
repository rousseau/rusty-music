// SPDX-License-Identifier: GPL-3.0-or-later
//! Explorer : Lama, du texte à la playlist (Ollama).

use super::*;

/// De quoi conduire le panneau Lama sans clavier ni souris : une webview du
/// système ne se pilote pas de l'extérieur, et macOS refuse à nos outils le
/// pilotage des clics. `RUSTY_MUSIC_AFFICHAGE=lama` ouvre l'affichage,
/// `RUSTY_MUSIC_PROMPT="…"` soumet une demande, `RUSTY_MUSIC_THEME=clair`
/// force le thème, `RUSTY_MUSIC_AUDIT=1` lance l'audit de gabarit de l'interface
/// — pour les essais seulement, sans effet en usage normal.
#[derive(serde::Serialize)]
pub(crate) struct EssaiLama {
    affichage: Option<String>,
    prompt: Option<String>,
    theme: Option<String>,
    audit: bool,
}

#[tauri::command(async)]
pub(crate) fn essai_lama() -> EssaiLama {
    let lire = |cle: &str| std::env::var(cle).ok().filter(|v| !v.is_empty());
    EssaiLama {
        affichage: lire("RUSTY_MUSIC_AFFICHAGE"),
        prompt: lire("RUSTY_MUSIC_PROMPT"),
        theme: lire("RUSTY_MUSIC_THEME"),
        audit: audit_actif(),
    }
}

/// Interprétation d'un prompt de playlist en texte libre (champ d'intention
/// d'Explorer), affichée puis éditable dans l'inspecteur avant composition —
/// voir `path_texte_interpreter` puis `path_texte`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PlanTexte {
    /// `None` si le prompt ne nomme aucun morceau ni artiste identifiable :
    /// `path_texte` choisit alors un départ par similarité à la première
    /// étape plutôt qu'un morceau arbitraire.
    depart: Option<TrackRow>,
    /// `None` si le prompt ne demande pas d'arrivée précise (cas le plus
    /// courant : une simple dérive depuis `depart`) — `path_texte` dérive
    /// alors sans viser de point d'arrivée, comme avant l'ajout de ce champ.
    /// Présent, `path_texte` termine la marche guidée par un raccordement
    /// exact vers ce morceau (`Graphe::sonique`), pour qu'un « arriver à X »
    /// explicite dans le prompt tienne sa promesse plutôt que de rester une
    /// simple étape textuelle diluée parmi d'autres.
    arrivee: Option<TrackRow>,
    /// Le nom que le LLM a cru reconnaître comme arrivée, **que la recherche
    /// l'ait trouvé ou non** — `None` seulement si le prompt ne demandait
    /// aucune arrivée. Sert à distinguer, dans l'inspecteur, « pas d'arrivée
    /// demandée » de « arrivée demandée mais introuvable » : régression du 14
    /// septembre (« RATM » cherché tel quel, jamais trouvé, la playlist
    /// dérivait quand même sans arrivée sans que rien ne le montre) —
    /// `arrivee` seul à `None` ne disait pas lequel des deux cas s'était
    /// produit.
    arrivee_demandee: Option<String>,
    /// Toujours au moins une étape — `ollama::interpreter` le garantit déjà.
    etapes: Vec<String>,
    n: usize,
    /// Durée totale voulue : prime sur `n`, la playlist s'arrête à cette
    /// durée plutôt qu'à un nombre de morceaux.
    #[serde(default)]
    duree_minutes: Option<u32>,
    /// Plafond de morceaux par artiste **demandé** par l'utilisateur — jamais
    /// relâché. Absent : un plafond par défaut, lui relâché au besoin
    /// (`filtres_playlist::plafonner_par_artiste`).
    #[serde(default)]
    plafond_par_artiste: Option<u32>,
    /// Les contraintes typées appliquées à la bibliothèque avant la marche
    /// (`filtres_playlist`). `#[serde(default)]` : un plan sans ce champ ne
    /// restreint rien.
    #[serde(default)]
    filtres: FiltresPlaylist,
    /// Une phrase où Ollama reformule la demande — affichée telle quelle, sous
    /// « Ollama a compris » : de quoi vérifier d'un coup d'œil qu'il a bien lu.
    #[serde(default)]
    reformulation: Option<String>,
    /// Les parties d'une playlist « X puis Y » (genre, énergie, durée chacune).
    /// Vide : une seule partie implicite — `etapes` et `filtres`. Éditables
    /// dans l'interface avant « Recomposer ».
    #[serde(default)]
    parties: Vec<rusty_music_core::ollama::PartieLlm>,
    /// La réponse du modèle, telle qu'il l'a écrite — pour le dépliant « réponse
    /// brute d'Ollama ».
    #[serde(default)]
    brut: String,
}

/// Ce que `path_texte` rend : la playlist, et ce qu'il faut en savoir.
#[derive(Debug, serde::Serialize)]
pub(crate) struct CompositionTexte {
    pistes: Vec<TrackRow>,
    /// Une phrase par critère abandonné ou limite rencontrée (trop peu de
    /// morceaux admissibles, marche écourtée, durée non atteinte) — montrées
    /// à l'utilisateur, jamais tues : une playlist « calme » qui ne l'est plus
    /// doit le dire.
    relaches: Vec<String>,
    /// Durée cumulée des pistes rendues.
    duree_ms: i64,
    /// Ce que le moteur a fait, partie par partie : l'« entonnoir » des filtres,
    /// le départ choisi et pourquoi, la durée obtenue contre la durée visée.
    /// Toujours au moins une entrée (une playlist sans partie en a une
    /// implicite) ; l'interface s'en sert pour expliquer le résultat.
    parties: Vec<PartieResultat>,
}

/// Une partie composée : sa place dans la file et ce qui l'a produite.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PartieResultat {
    libelle: String,
    /// Indices dans `pistes` : `debut` inclus, `fin` exclu.
    debut: usize,
    fin: usize,
    duree_ms: i64,
    cible_ms: Option<i64>,
    /// Morceaux admissibles avant la marche (hors ceux d'une partie précédente).
    n_admissibles: usize,
    /// `(critère, morceaux restants)` — voir `filtres_playlist::entonnoir`.
    entonnoir: Vec<(String, usize)>,
    depart_pourquoi: String,
    relaches: Vec<String>,
    /// Les admissibles, pour estomper le reste sur le mini-nuage. Vide : aucun
    /// filtre, tout est admissible.
    ids_admissibles: Vec<i64>,
}

/// Interprète un prompt de playlist en texte libre (modèle intégré ou Ollama),
/// et résout son
/// morceau ou artiste de départ dans la bibliothèque — sans encore calculer
/// d'empreinte CLAP-texte ni construire de trajet, pour rester rapide :
/// l'utilisateur doit pouvoir corriger l'interprétation dans l'inspecteur
/// avant qu'elle ne remplace sa file d'écoute (`path_texte` fait le reste).
///
/// `modele` : celui choisi par l'utilisateur via l'icône 🦙 (mémorisé côté
/// interface) — [`modele_local::NOM`] pour le modèle intégré, sinon un modèle
/// d'Ollama. `None` — rien choisi encore — prend le modèle intégré s'il est
/// téléchargé, sinon `ollama::modele_par_defaut`, pas un nom fixe : deviner
/// un modèle qui n'est pas installé rend un 404 qu'Ollama ne distingue pas
/// clairement d'un serveur injoignable (observé en pratique).
///
/// `seed` : un tirage frais côté interface à chaque interprétation, jamais
/// réutilisé — voir la documentation de [`resoudre_piste_nommee`] sur
/// pourquoi le départ/l'arrivée d'un artiste sans titre précis en a besoin.
#[tauri::command(async)]
pub(crate) fn path_texte_interpreter(
    etat: State<Etat>,
    prompt: String,
    n_defaut: usize,
    modele: Option<String>,
    seed: u64,
) -> Result<PlanTexte, String> {
    let modele = modele.filter(|m| !m.is_empty());
    let integre = match modele.as_deref() {
        Some(m) => m == modele_local::NOM,
        None => modele_local::disponibilite() == "pret",
    };
    // Les genres réels de la bibliothèque : montrés au LLM pour qu'il ne parle
    // que de ce qui existe, puis utilisés pour écarter ce qu'il inventerait
    // quand même. Illisible : on s'en passe (aucun filtre de genre) plutôt que
    // de refuser toute interprétation pour cela.
    let vocabulaire = verrou(&etat.lib)
        .vocabulaire_genres(usize::MAX)
        .unwrap_or_else(|e| {
            tracing::warn!(erreur = %e, "vocabulaire de genres illisible, interprétation sans");
            Vec::new()
        });
    let interpretation = if integre {
        modele_local::interpreter(&etat, &prompt, &vocabulaire)?
    } else {
        let hote = rusty_music_core::ollama::HOTE_DEFAUT;
        let modele = match modele {
            Some(m) => m,
            None => rusty_music_core::ollama::modele_par_defaut(hote).map_err(echec)?,
        };
        rusty_music_core::ollama::interpreter(hote, &modele, &prompt, &vocabulaire)
            .map_err(echec)?
    };

    let n = interpretation
        .n
        .map(|n| n as usize)
        .unwrap_or(n_defaut)
        .max(1);
    let depart = resoudre_piste_nommee(
        &etat,
        "départ",
        interpretation.seed_morceau.as_deref(),
        interpretation.seed_artiste.as_deref(),
        seed,
    )?;
    let arrivee_demandee = interpretation
        .arrivee_morceau
        .clone()
        .or_else(|| interpretation.arrivee_artiste.clone());
    // Graine décorrélée de celle du départ : sans ça, quand les deux listes
    // de candidats ont la même taille, les deux tirages `categorique`
    // choisiraient le même rang et retomberaient sur le même biais.
    let arrivee = resoudre_piste_nommee(
        &etat,
        "arrivée",
        interpretation.arrivee_morceau.as_deref(),
        interpretation.arrivee_artiste.as_deref(),
        seed.wrapping_add(1),
    )?;
    tracing::info!(
        depart = ?depart.as_ref().map(|t| (&t.title, &t.artist)),
        ?arrivee_demandee,
        arrivee = ?arrivee.as_ref().map(|t| (&t.title, &t.artist)),
        etapes = ?interpretation.etapes,
        n,
        "champ d'intention : plan prêt pour l'inspecteur",
    );
    if arrivee_demandee.is_some() && arrivee.is_none() {
        tracing::warn!(
            ?arrivee_demandee,
            "champ d'intention : arrivée demandée mais introuvable dans la bibliothèque",
        );
    }

    let filtres = interpretation.filtres();
    tracing::info!(
        ?filtres,
        duree_minutes = ?interpretation.duree_minutes,
        plafond_par_artiste = ?interpretation.plafond_par_artiste,
        "champ d'intention : contraintes extraites",
    );
    Ok(PlanTexte {
        depart,
        arrivee,
        arrivee_demandee,
        etapes: interpretation.etapes,
        n,
        duree_minutes: interpretation.duree_minutes,
        plafond_par_artiste: interpretation.plafond_par_artiste,
        filtres,
        reformulation: interpretation.reformulation,
        parties: interpretation.parties,
        brut: interpretation.brut,
    })
}

/// Ce que Lama peut faire sur cette machine, sondé à l'entrée du panneau.
#[derive(serde::Serialize)]
pub(crate) struct DisponibiliteLama {
    /// Le modèle intégré : `pret`, `a_telecharger`, ou `absent` (version
    /// compilée sans) — voir [`modele_local::disponibilite`].
    local: &'static str,
    /// Ollama : `pret`, `sans_modele` (il répond, rien d'installé) ou `absent`.
    etat: &'static str,
    modeles: Vec<String>,
    /// Pourquoi Ollama est tenu pour absent, tel que le client l'a vu.
    detail: Option<String>,
}

/// Ollama répond-il, et avec quels modèles ? Ne lève jamais : un serveur
/// absent est un état du panneau, pas une erreur (Lama dit alors quoi faire
/// avant qu'on écrive une demande, au lieu d'échouer après).
#[tauri::command(async)]
pub(crate) fn lama_disponibilite() -> DisponibiliteLama {
    match rusty_music_core::ollama::modeles(rusty_music_core::ollama::HOTE_DEFAUT) {
        Ok(modeles) if modeles.is_empty() => DisponibiliteLama {
            local: modele_local::disponibilite(),
            etat: "sans_modele",
            modeles,
            detail: None,
        },
        Ok(modeles) => DisponibiliteLama {
            local: modele_local::disponibilite(),
            etat: "pret",
            modeles,
            detail: None,
        },
        Err(e) => DisponibiliteLama {
            local: modele_local::disponibilite(),
            etat: "absent",
            modeles: Vec::new(),
            detail: Some(e.to_string()),
        },
    }
}

/// Modèles Ollama déjà installés localement — sélecteur du champ d'intention
/// (icône 🦙).
#[tauri::command(async)]
pub(crate) fn ollama_modeles() -> Result<Vec<String>, String> {
    rusty_music_core::ollama::modeles(rusty_music_core::ollama::HOTE_DEFAUT).map_err(echec)
}

/// Retrouve dans la bibliothèque le morceau ou l'artiste que l'interprétation
/// a cru reconnaître pour un rôle (« départ » ou « arrivée », pour les
/// journaux seulement) — `None` si rien n'a été cité ou rien n'a été trouvé.
/// Partagée entre `depart` et `arrivee` : même ambiguïté à lever dans les deux
/// cas (un artiste sans titre précis résolu à l'un de ses morceaux).
///
/// **Choix au hasard parmi les morceaux trouvés, pas le plus central.**
/// Revu le 14 septembre : `chemin::parcours` retenait toujours le même pivot
/// (le morceau le plus proche du centroïde de l'artiste), donc un même
/// prompt ouvrait chaque fois sur le même morceau — contraire au but affiché
/// de l'application, redécouvrir la bibliothèque plutôt que la parcourir
/// toujours pareil. `graine` vient d'un tirage frais côté interface à chaque
/// interprétation (pas à chaque « Recomposer », qui rejoue le même plan) :
/// deux envois du même prompt peuvent donc ouvrir sur deux morceaux
/// différents du même artiste, mais rejouer un plan déjà interprété ne
/// change pas son départ.
pub(crate) fn resoudre_piste_nommee(
    etat: &State<Etat>,
    role: &str,
    morceau: Option<&str>,
    artiste: Option<&str>,
    graine: u64,
) -> Result<Option<TrackRow>, String> {
    let Some(requete) = morceau.or(artiste) else {
        return Ok(None);
    };

    let resultats = {
        let lib = verrou(&etat.lib);
        lib.search(requete, 50).map_err(echec)?
    };
    tracing::info!(
        %role,
        %requete,
        resultats = resultats.len(),
        "champ d'intention : recherche dans la bibliothèque",
    );

    let mut alea = rusty_music_analysis::alea::Alea::depuis(graine);

    if let Some(artiste) = artiste {
        let ids: Vec<i64> = resultats
            .iter()
            .filter(|t| {
                t.artist
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(artiste))
            })
            .map(|t| t.id)
            .collect();
        tracing::info!(
            %role,
            %artiste,
            morceaux_de_lartiste = ids.len(),
            "champ d'intention : filtrage par artiste cité",
        );
        if !ids.is_empty() {
            let id = ids[alea.categorique(&vec![1.0; ids.len()])];
            return Ok(pistes_de(etat, &[id])?.into_iter().next());
        }
    }
    if resultats.is_empty() {
        return Ok(None);
    }
    let i = alea.categorique(&vec![1.0; resultats.len()]);
    Ok(resultats.into_iter().nth(i))
}

/// Compose la playlist : filtres, puis une marche guidée dans le graphe des
/// voisins (`chemin::guidee`) **partie par partie**. Séparée de l'interprétation
/// LLM (`path_texte_interpreter`) : ce calcul-ci ne se relance que si
/// l'utilisateur confirme le plan affiché.
///
/// Les **filtres** du plan (genres, exclusions, années, tempo, énergie,
/// popularité — `filtres_playlist`) restreignent d'abord la bibliothèque ; la
/// marche ne parcourt que les morceaux admissibles. Départ et arrivée nommés
/// restent admis même s'ils contreviennent aux filtres : l'utilisateur les a
/// cités. Chaque partie est plafonnée par artiste puis ramenée à **sa** durée
/// ou à **son** nombre de morceaux.
#[tauri::command(async)]
pub(crate) fn path_texte(
    etat: State<Etat>,
    plan: PlanTexte,
    seed: u64,
    bruit: Option<f32>,
) -> Result<CompositionTexte, String> {
    if plan.etapes.is_empty() && plan.parties.is_empty() {
        return Err("aucune étape à composer".into());
    }

    let parties = {
        let mut garde = verrou(&etat.texte_modele);
        if garde.valeur.is_none() {
            garde.valeur = Some(rusty_music_analysis::EmbedderTexte::charger(None).map_err(echec)?);
        }
        let encodeur = garde.valeur.as_ref().expect("encodeur texte chargé");
        let parties = parties_du_plan(&plan, |phrase| encodeur.embed(phrase).map_err(echec))?;
        garde.touche = Some(Instant::now());
        parties
    };

    let vecteurs = charger_vecteurs(&etat)?;
    // Seuls les morceaux déjà analysés peuvent être placés : les filtres, leurs
    // percentiles et les durées se calculent sur ceux-là.
    let carac = {
        let analyses: HashSet<i64> = vecteurs.iter().map(|(id, _)| *id).collect();
        let mut c = verrou(&etat.lib).caracteristiques_pistes().map_err(echec)?;
        c.retain(|p| analyses.contains(&p.id));
        c
    };

    let bruit = bruit.unwrap_or(BRUIT_DEFAUT);
    let base = construire_graphe(&etat, &vecteurs)?;
    let composition = composer_route(&plan, &parties, &carac, &vecteurs, &base, seed, bruit)?;

    let pistes = pistes_de(&etat, &composition.route)?;
    let duree_ms = pistes.iter().map(|t| t.duration_ms.unwrap_or(0)).sum();
    for r in &composition.relaches {
        tracing::info!(relache = %r, "champ d'intention : contrainte assouplie");
    }
    for p in &composition.parties {
        tracing::info!(
            partie = %p.libelle, morceaux = p.fin - p.debut, duree_ms = p.duree_ms,
            cible_ms = ?p.cible_ms, admissibles = p.n_admissibles, depart = %p.depart_pourquoi,
            "champ d'intention : partie composée",
        );
    }
    Ok(CompositionTexte {
        pistes,
        relaches: composition.relaches,
        duree_ms,
        parties: composition.parties,
    })
}

/// Une partie à composer, résolue à partir du plan : ses filtres, sa taille et
/// les cibles CLAP-texte que la marche vise.
pub(crate) struct PartiePlan {
    libelle: String,
    filtres: FiltresPlaylist,
    cible_ms: Option<i64>,
    n: Option<usize>,
    cibles: Vec<Vec<f32>>,
}

/// Ce que `composer_route` rend : la route (identifiants, dans l'ordre), tout
/// ce qui a été assoupli, et le détail partie par partie.
pub(crate) struct Composition {
    route: Vec<i64>,
    relaches: Vec<String>,
    parties: Vec<PartieResultat>,
}

/// Le plan en parties à composer. Sans partie explicite, une seule partie
/// implicite porte tout le plan (ses filtres, sa durée ou son nombre de
/// morceaux, toutes ses étapes comme cibles successives de la marche) — le
/// comportement d'avant les parties. `embed` : l'encodeur CLAP-texte, passé en
/// fonction pour se tester sans lui.
pub(crate) fn parties_du_plan(
    plan: &PlanTexte,
    embed: impl Fn(&str) -> Result<Vec<f32>, String>,
) -> Result<Vec<PartiePlan>, String> {
    use rusty_music_core::filtres_playlist::NiveauEnergie;
    if plan.parties.is_empty() {
        let cibles = plan
            .etapes
            .iter()
            .map(|e| embed(e))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(vec![PartiePlan {
            libelle: "Playlist".into(),
            filtres: plan.filtres.clone(),
            cible_ms: plan.duree_minutes.map(|m| i64::from(m) * 60_000),
            n: if plan.duree_minutes.is_some() {
                None
            } else {
                Some(plan.n.max(1))
            },
            cibles,
        }]);
    }
    let k = plan.parties.len();
    plan.parties
        .iter()
        .map(|p| {
            let mut filtres = plan.filtres.clone();
            // Le genre et l'énergie de la partie priment sur ceux de la playlist.
            filtres.genres = p.genres.clone();
            if let Some(e) = p.energie.as_deref().and_then(NiveauEnergie::depuis_texte) {
                filtres.energie = Some(e);
            }
            let libelle = if p.genres.is_empty() {
                let mut d: String = p.description.chars().take(40).collect();
                if let Some(c) = d.get_mut(0..1) {
                    c.make_ascii_uppercase();
                }
                d
            } else {
                let mut g = p.genres.join(" / ");
                if let Some(c) = g.get_mut(0..1) {
                    c.make_ascii_uppercase();
                }
                g
            };
            let cible_ms = p.duree_minutes.map(|m| i64::from(m) * 60_000);
            // Ni durée ni nombre : sa part du nombre de morceaux de la playlist.
            let n = match (cible_ms, p.n) {
                (Some(_), _) => None,
                (None, Some(n)) => Some(n as usize),
                (None, None) => Some((plan.n / k).max(3)),
            };
            Ok(PartiePlan {
                libelle,
                filtres,
                cible_ms,
                n,
                cibles: vec![embed(&p.description)?],
            })
        })
        .collect()
}

/// Ce que `composer_partie` partage entre les parties.
pub(crate) struct Contexte<'a> {
    carac: &'a [filtres_playlist::CaracteristiquesPiste],
    vecteurs: &'a [Empreinte],
    base: &'a Graphe,
    par_id: HashMap<i64, &'a [f32]>,
    artiste_de: HashMap<i64, String>,
    duree_de: HashMap<i64, i64>,
    seed: u64,
    bruit: f32,
}

/// Le cœur de `path_texte`, sans `State` : pour chaque partie, filtres, marche
/// guidée, plafond par artiste, longueur finale. Séparé de la commande pour se
/// tester sur des empreintes synthétiques.
///
/// `carac` ne doit contenir que des morceaux présents dans `vecteurs` : les
/// percentiles et les durées se calculent sur les morceaux placables.
///
/// Une partie ne reprend jamais un morceau d'une partie précédente. Son départ
/// est, après la première, **le morceau admissible le plus proche de la fin de
/// la précédente** : la transition la plus douce possible entre deux styles,
/// jamais un tirage.
pub(crate) fn composer_route(
    plan: &PlanTexte,
    parties: &[PartiePlan],
    carac: &[filtres_playlist::CaracteristiquesPiste],
    vecteurs: &[Empreinte],
    base: &Graphe,
    seed: u64,
    bruit: f32,
) -> Result<Composition, String> {
    let ctx = Contexte {
        carac,
        vecteurs,
        base,
        par_id: vecteurs.iter().map(|(id, v)| (*id, v.as_slice())).collect(),
        artiste_de: carac
            .iter()
            .filter_map(|p| p.artiste.clone().map(|a| (p.id, a)))
            .collect(),
        duree_de: carac
            .iter()
            .filter_map(|p| p.duree_ms.map(|d| (p.id, d)))
            .collect(),
        seed,
        bruit,
    };
    let multi = parties.len() > 1;
    let mut route: Vec<i64> = Vec::new();
    let mut utilises: HashSet<i64> = HashSet::new();
    let mut resultats: Vec<PartieResultat> = Vec::new();
    let mut relaches: Vec<String> = Vec::new();

    for (i, partie) in parties.iter().enumerate() {
        let precedent = route.last().copied();
        let derniere = i + 1 == parties.len();
        let (morceaux, mut resultat) =
            composer_partie(&ctx, plan, partie, i, derniere, precedent, &utilises)?;
        resultat.debut = route.len();
        route.extend_from_slice(&morceaux);
        resultat.fin = route.len();
        utilises.extend(morceaux);
        for r in &resultat.relaches {
            relaches.push(if multi {
                format!("{} — {r}", partie.libelle)
            } else {
                r.clone()
            });
        }
        resultats.push(resultat);
    }
    if route.is_empty() {
        return Err("aucune marche possible avec ces filtres".into());
    }
    Ok(Composition {
        route,
        relaches,
        parties: resultats,
    })
}

/// Compose **une** partie. Rend ses morceaux (éventuellement aucun, si tout ce
/// qui lui convenait est déjà pris — dit dans `relaches`) et son compte rendu.
pub(crate) fn composer_partie(
    ctx: &Contexte,
    plan: &PlanTexte,
    partie: &PartiePlan,
    i: usize,
    derniere: bool,
    precedent: Option<i64>,
    utilises: &HashSet<i64>,
) -> Result<(Vec<i64>, PartieResultat), String> {
    let (carac, vecteurs) = (ctx.carac, ctx.vecteurs);
    let mut relaches: Vec<String> = Vec::new();
    let entonnoir = filtres_playlist::entonnoir(carac, &partie.filtres);

    // Combien de morceaux faut-il ? D'abord une estimation sur la durée moyenne
    // de toute la bibliothèque (pour le seuil minimal de la sélection), affinée
    // plus bas sur la durée moyenne des seuls admissibles.
    let moyenne_globale = filtres_playlist::duree_moyenne_ms(carac, None).max(1);
    let estimation = match partie.cible_ms {
        Some(d) => ((d + moyenne_globale - 1) / moyenne_globale).max(2) as usize,
        None => partie.n.unwrap_or(plan.n).max(1),
    };

    let selection =
        filtres_playlist::selectionner(carac, &partie.filtres, (estimation / 2).clamp(5, 40));
    let mut permis: Option<HashSet<i64>> = match selection {
        None => None,
        Some(sel) => {
            relaches.extend(sel.relaches);
            if sel.ids.is_empty() {
                return Err(format!(
                    "{} : aucun morceau de la bibliothèque ne respecte ces exclusions — \
                     retirez-en une, ou nommez un artiste ou un genre",
                    partie.libelle
                ));
            }
            Some(sel.ids)
        }
    };
    // Une partie ne reprend pas un morceau déjà pris — et sans filtre, il faut
    // quand même l'écrire explicitement pour pouvoir les retirer.
    if !utilises.is_empty() {
        let tous = permis
            .take()
            .unwrap_or_else(|| carac.iter().map(|p| p.id).collect());
        permis = Some(
            tous.into_iter()
                .filter(|id| !utilises.contains(id))
                .collect(),
        );
    }
    // Départ et arrivée nommés : admis quoi qu'il arrive.
    if let Some(ids) = permis.as_mut() {
        if i == 0 {
            ids.extend(plan.depart.iter().map(|t| t.id));
        }
        if derniere {
            ids.extend(plan.arrivee.iter().map(|t| t.id));
        }
    }
    let n_admissibles = permis.as_ref().map_or(carac.len(), HashSet::len);
    let resultat =
        |depart_pourquoi: String, relaches: Vec<String>, permis: &Option<HashSet<i64>>| {
            let mut ids: Vec<i64> = permis.iter().flatten().copied().collect();
            ids.sort_unstable();
            PartieResultat {
                libelle: partie.libelle.clone(),
                debut: 0,
                fin: 0,
                duree_ms: 0,
                cible_ms: partie.cible_ms,
                n_admissibles,
                entonnoir: entonnoir.clone(),
                depart_pourquoi,
                relaches,
                ids_admissibles: ids,
            }
        };
    if permis.as_ref().is_some_and(HashSet::is_empty) {
        relaches.push("tout ce qui convenait est déjà pris par une partie précédente".into());
        return Ok((
            Vec::new(),
            resultat("aucun départ possible".into(), relaches, &permis),
        ));
    }

    let moyenne_ms = filtres_playlist::duree_moyenne_ms(carac, permis.as_ref()).max(1);
    let n_voulu = match partie.cible_ms {
        Some(d) => ((d + moyenne_ms - 1) / moyenne_ms).max(2) as usize,
        None => partie.n.unwrap_or(plan.n).max(1),
    };

    // Le départ : celui qui a été nommé ; sinon, pour la première partie, le
    // morceau le plus proche de sa description ; pour les suivantes, le plus
    // proche de la fin de la précédente — la transition la plus douce.
    let (depart, depart_pourquoi) = match (i, &plan.depart, precedent) {
        (0, Some(t), _) => (t.id, "départ nommé dans la demande".to_string()),
        (_, _, Some(dernier)) if i > 0 => {
            let v = ctx.par_id.get(&dernier).copied().unwrap_or(&[]);
            (
                meilleur_depart_par_description(vecteurs, permis.as_ref(), v)
                    .ok_or("aucun départ possible parmi les morceaux admissibles")?,
                "le plus proche de la fin de la partie précédente".to_string(),
            )
        }
        _ => (
            meilleur_depart_par_description(vecteurs, permis.as_ref(), &partie.cibles[0])
                .ok_or("aucun départ possible parmi les morceaux admissibles")?,
            "le plus proche de la description".to_string(),
        ),
    };

    // Une sélection large se découpe dans le graphe déjà construit (adjacence
    // filtrée, aucune distance recalculée) ; une sélection étroite y serait
    // en miettes — les voisins d'un morceau sont presque tous hors sélection —
    // et la marche s'arrêterait aussitôt : on bâtit alors le graphe des
    // voisins **des seuls morceaux admissibles**, dont le coût (n²) reste
    // modeste à cette taille.
    let dedie;
    let restreint;
    let graphe: &Graphe = match &permis {
        None => ctx.base,
        Some(ids) => {
            let presents = vecteurs.iter().filter(|(id, _)| ids.contains(id)).count();
            if presents * 2 < vecteurs.len() {
                let sous: Vec<Empreinte> = vecteurs
                    .iter()
                    .filter(|(id, _)| ids.contains(id))
                    .cloned()
                    .collect();
                dedie = Graphe::construire(
                    &sous,
                    rusty_music_analysis::chemin::K_VOISINS,
                    coeurs_arriere_plan(),
                );
                &dedie
            } else {
                restreint = ctx.base.restreint(ids);
                &restreint
            }
        }
    };

    // Un peu plus long que voulu : le plafond par artiste en retirera.
    let pas = (n_voulu * 13).div_ceil(10) + 2;
    let graine = ctx.seed.wrapping_add(i as u64);
    let mut route = graphe.guidee(vecteurs, depart, &partie.cibles, pas, graine, ctx.bruit);
    if route.is_empty() {
        relaches.push("aucune marche possible depuis ce départ avec ces filtres".into());
        return Ok((Vec::new(), resultat(depart_pourquoi, relaches, &permis)));
    }

    // Une arrivée précise a été demandée (« arriver à X ») : la marche guidée
    // ne fait que dériver vers ses cibles textuelles, qui sont des régions,
    // pas des points — elle ne garantit pas d'atterrir exactement sur un
    // morceau donné. Un raccordement dans le graphe des voisins, depuis là où
    // la dérive s'est arrêtée, tient la promesse là où `guidee` seule ne le
    // pouvait pas. Seulement à la dernière partie.
    let mut proteges: HashSet<i64> = HashSet::from([depart]);
    if let (true, Some(arrivee)) = (derniere, &plan.arrivee) {
        proteges.insert(arrivee.id);
        if route.last() != Some(&arrivee.id) {
            // La dérive a pu croiser l'arrivée en chemin : elle doit finir la
            // playlist, pas y figurer deux fois.
            route.retain(|id| *id != arrivee.id);
            let dernier = *route.last().unwrap_or(&depart);
            let jonction = graphe.sonique(dernier, arrivee.id, graine, ctx.bruit);
            match jonction.split_first() {
                Some((_, reste)) => route.extend_from_slice(reste),
                // Composantes non reliées dans le graphe des voisins (rare) :
                // l'arrivée est posée telle quelle plutôt que silencieusement
                // ignorée — une transition brutale tient mieux une demande
                // explicite qu'un « arriver à X » qui n'arrive nulle part.
                None => route.push(arrivee.id),
            }
        }
    }

    // Plafond par artiste : demandé, il tient ; sinon un défaut (au moins 3,
    // un huitième de la partie) qui monte d'un cran tant qu'il manque des
    // morceaux.
    let plafond_impose = plan.plafond_par_artiste.is_some();
    let plafond = plan
        .plafond_par_artiste
        .map(|p| p as usize)
        .unwrap_or_else(|| n_voulu.div_ceil(8).max(3));
    let atteint = route.len();
    route = filtres_playlist::plafonner_par_artiste(
        &route,
        &ctx.artiste_de,
        n_voulu,
        plafond,
        plafond_impose,
        &proteges,
    );
    if plafond_impose && route.len() < atteint.min(n_voulu) {
        relaches.push(format!(
            "plafond de {plafond} morceau(x) par artiste tenu : {} morceau(x) retiré(s)",
            atteint - route.len()
        ));
    }

    // Longueur finale : la durée visée, ou le nombre de morceaux — en gardant
    // les deux extrémités et l'allure du trajet (`echantillonner`).
    let duree_totale = |r: &[i64]| -> i64 {
        r.iter()
            .map(|id| {
                ctx.duree_de
                    .get(id)
                    .copied()
                    .filter(|d| *d > 0)
                    .unwrap_or(filtres_playlist::DUREE_DEFAUT_MS)
            })
            .sum()
    };
    let route = match partie.cible_ms {
        Some(cible) => {
            // Une arrivée nommée termine la dernière partie : on ne la coupe pas.
            let garder_fin = derniere && plan.arrivee.is_some();
            let r = filtres_playlist::ajuster_a_duree(
                &route,
                &ctx.duree_de,
                cible,
                echantillonner,
                garder_fin,
            );
            let total = duree_totale(&r);
            if total * 10 < cible * 9 {
                relaches.push(format!(
                    "durée visée {} min, mais la marche n'enchaîne que {} min de morceaux admissibles",
                    cible / 60_000,
                    total / 60_000
                ));
            }
            r
        }
        None if route.len() > n_voulu => echantillonner(&route, n_voulu),
        None => {
            if route.len() * 10 < n_voulu * 7 {
                relaches.push(format!(
                    "la marche s'arrête à {} morceau(x) sur {n_voulu} voulus : voisinage sonore trop restreint",
                    route.len()
                ));
            }
            route
        }
    };

    let mut r = resultat(depart_pourquoi, relaches, &permis);
    r.duree_ms = duree_totale(&route);
    Ok((route, r))
}

/// Le morceau dont l'empreinte est la plus proche d'une cible — départ « par
/// description » quand le prompt ne nomme ni morceau ni artiste, même
/// principe que la recherche par description sondée dans
/// `experiments/clap-texte`.
pub(crate) fn meilleur_depart_par_description(
    vecteurs: &[Empreinte],
    permis: Option<&HashSet<i64>>,
    cible: &[f32],
) -> Option<i64> {
    vecteurs
        .iter()
        .filter(|(id, _)| permis.is_none_or(|p| p.contains(id)))
        .max_by(|(_, a), (_, b)| {
            produit_scalaire(a, cible)
                .partial_cmp(&produit_scalaire(b, cible))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(id, _)| *id)
}

pub(crate) fn produit_scalaire(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::Path;

    use std::time::Instant;

    // ---- composition d'une playlist par texte (`composer_route`) ----------

    use rusty_music_analysis::chemin::{Empreinte, Graphe};
    use rusty_music_core::db::TrackRow;
    use rusty_music_core::filtres_playlist::{
        CaracteristiquesPiste, FiltresPlaylist, NiveauEnergie,
    };

    const N: usize = 300;
    const TROIS_MIN_MS: i64 = 180_000;

    /// 300 morceaux le long d'un arc de cercle (voisins sonores = indices
    /// voisins), un artiste pour 4 morceaux consécutifs, énergie croissante,
    /// rock aux indices pairs et jazz aux impairs, tous de 1990 et de 3 min.
    fn bibliotheque_synthetique() -> (Vec<Empreinte>, Vec<CaracteristiquesPiste>, Graphe) {
        let vecteurs: Vec<Empreinte> = (0..N)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / (N - 1) as f32;
                (i as i64, vec![a.cos(), a.sin(), 0.0])
            })
            .collect();
        let carac = (0..N)
            .map(|i| CaracteristiquesPiste {
                id: i as i64,
                artiste: Some(format!("artiste {}", i / 4)),
                annee: Some(1990),
                duree_ms: Some(TROIS_MIN_MS),
                bpm: Some(100.0),
                energie: Some(i as f32 / N as f32),
                popularite: None,
                jours_depuis_ecoute: None,
                genres: vec![if i % 2 == 0 {
                    "rock".into()
                } else {
                    "jazz".into()
                }],
            })
            .collect();
        let graphe = Graphe::construire(&vecteurs, 8, 1);
        (vecteurs, carac, graphe)
    }

    fn plan_de(n: usize) -> PlanTexte {
        PlanTexte {
            depart: None,
            arrivee: None,
            arrivee_demandee: None,
            etapes: vec!["vers la fin de l'arc".into()],
            n,
            duree_minutes: None,
            plafond_par_artiste: None,
            filtres: FiltresPlaylist::default(),
            reformulation: None,
            parties: Vec::new(),
            brut: String::new(),
        }
    }

    /// Une partie de test : genre, durée en minutes.
    fn partie(genre: &str, minutes: u32) -> rusty_music_core::ollama::PartieLlm {
        rusty_music_core::ollama::PartieLlm {
            description: format!("{genre} music"),
            genres: vec![genre.into()],
            energie: None,
            duree_minutes: Some(minutes),
            n: None,
        }
    }

    fn piste(id: i64) -> TrackRow {
        TrackRow {
            id,
            path: format!("/p/{id}.flac"),
            title: Some(format!("t{id}")),
            artist: Some(format!("artiste {}", id / 4)),
            album: None,
            track_no: None,
            year: Some(1990),
            duration_ms: Some(TROIS_MIN_MS),
            artist_mbid: None,
        }
    }

    fn composer_complet(plan: &PlanTexte) -> Result<Composition, String> {
        let (vecteurs, carac, graphe) = bibliotheque_synthetique();
        // Un « encodeur » de poche : le rock vise le début de l'arc, tout le
        // reste (jazz, étapes ordinaires) la fin.
        let parties = parties_du_plan(plan, |t| {
            Ok(if t.contains("rock") {
                vec![1.0, 0.0, 0.0]
            } else {
                vec![0.0, 1.0, 0.0]
            })
        })?;
        composer_route(plan, &parties, &carac, &vecteurs, &graphe, 7, BRUIT_DEFAUT)
    }

    fn composer(plan: &PlanTexte) -> Result<(Vec<i64>, Vec<String>), String> {
        composer_complet(plan).map(|c| (c.route, c.relaches))
    }

    fn doublons(route: &[i64]) -> usize {
        route.len() - route.iter().collect::<HashSet<_>>().len()
    }

    #[test]
    fn sans_filtre_la_route_a_la_longueur_demandee() {
        // Départ au début de l'arc, cible à l'autre bout : sans départ nommé,
        // « le plus proche de la cible » serait le bout de l'arc lui-même, d'où
        // une marche auto-évitante vite acculée — un artefact de ce décor
        // unidimensionnel, pas du produit.
        let mut plan = plan_de(20);
        plan.depart = Some(piste(0));
        let (route, relaches) = composer(&plan).unwrap();
        assert!(
            route.len() >= 15 && route.len() <= 20,
            "{} morceaux",
            route.len()
        );
        assert_eq!(doublons(&route), 0);
        assert!(relaches.is_empty(), "{relaches:?}");
    }

    #[test]
    fn un_plafond_demande_par_artiste_est_tenu() {
        let mut plan = plan_de(20);
        plan.plafond_par_artiste = Some(2);
        let (route, relaches) = composer(&plan).unwrap();
        let mut par_artiste: HashMap<i64, usize> = HashMap::new();
        for id in &route {
            *par_artiste.entry(id / 4).or_default() += 1;
        }
        // Le départ est protégé : il peut seul dépasser le plafond.
        let depart = route[0] / 4;
        for (a, n) in &par_artiste {
            assert!(*n <= 2 || *a == depart, "artiste {a} : {n} morceaux");
        }
        assert!(
            !relaches.is_empty(),
            "des morceaux ont été retirés : il faut le dire"
        );
    }

    #[test]
    fn une_duree_demandee_est_approchee() {
        let mut plan = plan_de(10);
        plan.duree_minutes = Some(30); // 10 morceaux de 3 min
        let (route, relaches) = composer(&plan).unwrap();
        let total = route.len() as i64 * TROIS_MIN_MS;
        assert!(
            (total - 30 * 60_000).abs() <= TROIS_MIN_MS,
            "{} morceaux, {} min",
            route.len(),
            total / 60_000
        );
        assert!(
            relaches.iter().all(|r| !r.contains("durée visée")),
            "{relaches:?}"
        );
    }

    #[test]
    fn une_duree_hors_de_portee_est_signalee() {
        let mut plan = plan_de(10);
        plan.duree_minutes = Some(1000); // 333 morceaux : la bibliothèque n'en a que 300
        let (_, relaches) = composer(&plan).unwrap();
        assert!(
            relaches.iter().any(|r| r.contains("durée visée")),
            "{relaches:?}"
        );
    }

    /// Large sélection : découpée dans le graphe complet (`restreint`).
    #[test]
    fn exclure_un_genre_ne_laisse_aucun_morceau_de_ce_genre() {
        let mut plan = plan_de(15);
        plan.filtres.exclure_genres = vec!["rock".into()];
        let (route, _) = composer(&plan).unwrap();
        assert!(!route.is_empty());
        assert!(
            route.iter().all(|id| id % 2 == 1),
            "un rock s'est glissé : {route:?}"
        );
    }

    /// Sélection étroite (un tiers) : graphe des voisins bâti sur les seuls
    /// admissibles, faute de quoi la marche s'arrêterait aussitôt.
    #[test]
    fn un_niveau_denergie_restreint_la_marche_aux_morceaux_calmes() {
        let mut plan = plan_de(15);
        plan.filtres.energie = Some(NiveauEnergie::Calme);
        let (route, relaches) = composer(&plan).unwrap();
        assert!(route.len() >= 10, "{} morceaux : {relaches:?}", route.len());
        // Tiers bas des énergies = indices 0..100.
        assert!(route.iter().all(|id| *id <= 100), "{route:?}");
    }

    #[test]
    fn une_arrivee_nommee_termine_la_playlist_sans_doublon() {
        let mut plan = plan_de(15);
        plan.arrivee = Some(piste(250));
        plan.depart = Some(piste(10));
        let (route, _) = composer(&plan).unwrap();
        assert_eq!(route.first(), Some(&10));
        assert_eq!(route.last(), Some(&250));
        assert_eq!(doublons(&route), 0);
    }

    /// Le départ nommé reste admis même s'il contrevient aux filtres :
    /// l'utilisateur l'a cité.
    #[test]
    fn un_depart_nomme_echappe_aux_filtres() {
        let mut plan = plan_de(10);
        plan.depart = Some(piste(10)); // rock
        plan.filtres.exclure_genres = vec!["rock".into()];
        let (route, _) = composer(&plan).unwrap();
        assert_eq!(route.first(), Some(&10));
        assert!(route[1..].iter().all(|id| id % 2 == 1));
    }

    #[test]
    fn des_exclusions_qui_vident_la_bibliotheque_sont_une_erreur_claire() {
        let mut plan = plan_de(10);
        plan.filtres.exclure_genres = vec!["rock".into(), "jazz".into()];
        let e = composer(&plan).unwrap_err();
        assert!(e.contains("exclusions"), "{e}");
    }

    #[test]
    fn un_critere_desiré_sans_resultat_est_assoupli_et_dit() {
        let mut plan = plan_de(10);
        plan.filtres.energie = Some(NiveauEnergie::Calme);
        plan.filtres.annee_min = Some(2000); // aucun morceau n'est de 2000
        let (route, relaches) = composer(&plan).unwrap();
        assert!(!route.is_empty());
        assert!(
            relaches.iter().any(|r| r.contains("période")),
            "{relaches:?}"
        );
    }

    // ---- parties : « X pendant 12 minutes, puis Y pendant 20 minutes » ----

    fn genre_de(id: i64) -> &'static str {
        if id % 2 == 0 {
            "rock"
        } else {
            "jazz"
        }
    }

    fn duree_min(route: &[i64]) -> i64 {
        route.len() as i64 * TROIS_MIN_MS / 60_000
    }

    /// La demande qui a révélé le défaut : avant les parties, les durées se
    /// perdaient et « rock, hip hop » devenait un filtre commun.
    #[test]
    fn deux_parties_respectent_chacune_leur_genre_et_leur_duree() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 12), partie("jazz", 21)];
        let c = composer_complet(&plan).unwrap();
        assert_eq!(c.parties.len(), 2);
        let (a, b) = (&c.parties[0], &c.parties[1]);
        // Chaque partie ne contient que son genre.
        assert!(
            c.route[a.debut..a.fin]
                .iter()
                .all(|id| genre_de(*id) == "rock"),
            "{:?}",
            &c.route[a.debut..a.fin]
        );
        assert!(
            c.route[b.debut..b.fin]
                .iter()
                .all(|id| genre_de(*id) == "jazz"),
            "{:?}",
            &c.route[b.debut..b.fin]
        );
        // Les durées visées, à un morceau près.
        assert!(
            (a.duree_ms / 60_000 - 12).abs() <= 3,
            "partie 1 : {} min",
            a.duree_ms / 60_000
        );
        assert!(
            (b.duree_ms / 60_000 - 21).abs() <= 3,
            "partie 2 : {} min",
            b.duree_ms / 60_000
        );
        assert_eq!(a.fin, b.debut, "les parties se suivent sans trou");
        assert_eq!(b.fin, c.route.len());
        assert_eq!(
            doublons(&c.route),
            0,
            "une partie ne reprend pas un morceau d'une autre"
        );
        assert_eq!(duree_min(&c.route), (a.duree_ms + b.duree_ms) / 60_000);
    }

    /// La transition entre deux parties est le morceau admissible le plus
    /// proche de la fin de la précédente — jamais un tirage au hasard.
    #[test]
    fn le_depart_dune_partie_est_le_voisin_de_la_fin_de_la_precedente() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 12), partie("jazz", 12)];
        let c = composer_complet(&plan).unwrap();
        let fin_rock = c.route[c.parties[0].fin - 1];
        let debut_jazz = c.route[c.parties[1].debut];
        assert!(
            (fin_rock - debut_jazz).abs() <= 2,
            "fin {fin_rock} → début {debut_jazz}"
        );
        assert!(c.parties[1]
            .depart_pourquoi
            .contains("fin de la partie précédente"));
        assert!(c.parties[0].depart_pourquoi.contains("description"));
    }

    #[test]
    fn les_exclusions_globales_valent_pour_chaque_partie() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 9), partie("jazz", 9)];
        plan.filtres.exclure_artistes = vec!["artiste 0".into(), "artiste 1".into()]; // ids 0..=7
        let c = composer_complet(&plan).unwrap();
        assert!(c.route.iter().all(|id| *id >= 8), "{:?}", c.route);
    }

    #[test]
    fn chaque_partie_rend_son_entonnoir_et_ses_admissibles() {
        let mut plan = plan_de(10);
        plan.parties = vec![partie("rock", 9), partie("jazz", 9)];
        plan.filtres.annee_min = Some(1980);
        let c = composer_complet(&plan).unwrap();
        for p in &c.parties {
            assert_eq!(
                p.entonnoir.first().map(|(l, n)| (l.as_str(), *n)),
                Some(("Bibliothèque", N))
            );
            assert!(
                p.entonnoir.iter().any(|(l, _)| l.starts_with("genre")),
                "{:?}",
                p.entonnoir
            );
            assert!(p.n_admissibles > 0 && p.n_admissibles <= N);
            assert_eq!(p.ids_admissibles.len(), p.n_admissibles);
        }
        // Les deux parties admettent des morceaux différents (rock / jazz).
        assert!(c.parties[0]
            .ids_admissibles
            .iter()
            .all(|id| genre_de(*id) == "rock"));
    }

    /// Une partie dont plus rien ne convient (tout est pris, ou aucun morceau)
    /// n'est pas une erreur : la suivante se compose, le manque est dit.
    #[test]
    fn une_partie_sans_morceau_disponible_est_signalee_pas_fatale() {
        let mut plan = plan_de(10);
        let mut vide = partie("rock", 12);
        vide.genres = vec!["rock".into()];
        plan.parties = vec![partie("jazz", 9), vide];
        plan.filtres.annee_max = Some(1980); // tous de 1990 : le critère s'assouplit
        let c = composer_complet(&plan).unwrap();
        assert!(!c.route.is_empty());
        assert!(
            c.relaches
                .iter()
                .any(|r| r.contains("assoupli") || r.contains("abandonné")),
            "{:?}",
            c.relaches
        );
    }

    #[test]
    fn sans_partie_explicite_la_playlist_est_une_seule_partie_implicite() {
        let mut plan = plan_de(12);
        plan.depart = Some(piste(0));
        let c = composer_complet(&plan).unwrap();
        assert_eq!(c.parties.len(), 1);
        assert_eq!((c.parties[0].debut, c.parties[0].fin), (0, c.route.len()));
        assert_eq!(c.parties[0].libelle, "Playlist");
    }

    /// Les parties sans taille se partagent le nombre de morceaux voulu.
    #[test]
    fn des_parties_sans_taille_se_partagent_n() {
        let mut plan = plan_de(12);
        let mut a = partie("rock", 0);
        let mut b = partie("jazz", 0);
        a.duree_minutes = None;
        b.duree_minutes = None;
        plan.parties = vec![a, b];
        let c = composer_complet(&plan).unwrap();
        for p in &c.parties {
            assert!(
                (p.fin - p.debut) as i64 >= 3 && (p.fin - p.debut) <= 8,
                "{} morceaux",
                p.fin - p.debut
            );
        }
    }

    /// Bout en bout sur la **vraie** bibliothèque (une copie — ouvrir la base
    /// de l'application migrerait son schéma) : vrai Ollama, vrai encodeur
    /// CLAP-texte, vraies empreintes, vrai graphe. Sous-échantillonné pour que
    /// le graphe (n²) reste raisonnable en debug. Ignoré par défaut :
    /// `RUSTY_DB=copie.db RUSTY_SOUS_ECH=6000 OLLAMA_MODELE=gemma4:e4b-mlx \
    ///  cargo test -p rusty-music-desktop composition_sur_la_vraie -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn composition_sur_la_vraie_bibliotheque() {
        use rusty_music_core::ollama;
        let base = std::env::var("RUSTY_DB").expect("RUSTY_DB=chemin/vers/copie.db");
        let m: usize = std::env::var("RUSTY_SOUS_ECH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6000);
        let modele = std::env::var("OLLAMA_MODELE").unwrap_or_else(|_| "gemma4:e4b-mlx".into());
        let lib = rusty_music_core::db::Library::open(Path::new(&base)).expect("base");

        let tout = lib
            .embeddings(rusty_music_analysis::passe::MODELE)
            .expect("empreintes");
        let pas = (tout.len() / m).max(1);
        let vecteurs: Vec<Empreinte> = tout.into_iter().step_by(pas).collect();
        let analyses: HashSet<i64> = vecteurs.iter().map(|(id, _)| *id).collect();
        let mut carac = lib.caracteristiques_pistes().expect("caractéristiques");
        carac.retain(|p| analyses.contains(&p.id));
        let t = Instant::now();
        let graphe = Graphe::construire(
            &vecteurs,
            rusty_music_analysis::chemin::K_VOISINS,
            super::coeurs_arriere_plan(),
        );
        eprintln!(
            "{} morceaux, graphe en {:.1} s",
            vecteurs.len(),
            t.elapsed().as_secs_f64()
        );
        let encodeur = rusty_music_analysis::EmbedderTexte::charger(None).expect("CLAP texte");
        let vocab = lib.vocabulaire_genres(usize::MAX).unwrap();

        for prompt in [
            "Une playlist calme de 60 minutes, sans rock",
            "Une heure de musique énergique pour courir, pas plus de 2 morceaux par artiste",
            "Des morceaux de jazz peu connus, 20 morceaux",
            "Une playlist qui commence doucement et monte vers du rock énergique, 15 morceaux",
        ] {
            let i =
                ollama::interpreter(ollama::HOTE_DEFAUT, &modele, prompt, &vocab).expect("Ollama");
            let plan = PlanTexte {
                depart: None,
                arrivee: None,
                arrivee_demandee: None,
                etapes: i.etapes.clone(),
                n: i.n.map(|n| n as usize).unwrap_or(20),
                duree_minutes: i.duree_minutes,
                plafond_par_artiste: i.plafond_par_artiste,
                filtres: i.filtres(),
                reformulation: i.reformulation.clone(),
                parties: i.parties.clone(),
                brut: String::new(),
            };
            let parties = parties_du_plan(&plan, |e| Ok(encodeur.embed(e).unwrap())).unwrap();
            let t = Instant::now();
            let resultat =
                composer_route(&plan, &parties, &carac, &vecteurs, &graphe, 3, BRUIT_DEFAUT)
                    .map(|c| (c.route, c.relaches));
            eprintln!(
                "\n« {prompt} »\n  filtres : {:?} · durée {:?} · n {}",
                plan.filtres, plan.duree_minutes, plan.n
            );
            match resultat {
                Err(e) => eprintln!("  ERREUR : {e}"),
                Ok((route, relaches)) => {
                    let total: i64 = route
                        .iter()
                        .map(|id| {
                            carac
                                .iter()
                                .find(|p| p.id == *id)
                                .and_then(|p| p.duree_ms)
                                .unwrap_or(0)
                        })
                        .sum();
                    eprintln!(
                        "  {} morceaux, {} min, en {} ms ; assouplis : {relaches:?}",
                        route.len(),
                        total / 60_000,
                        t.elapsed().as_millis()
                    );
                    for id in route.iter().take(12) {
                        let c = carac.iter().find(|p| p.id == *id).unwrap();
                        let t = lib.track(*id).unwrap().unwrap();
                        eprintln!(
                            "    {:<28} {:<26} énergie {:.2} bpm {:>5.0} {:?}",
                            t.artist
                                .unwrap_or_default()
                                .chars()
                                .take(28)
                                .collect::<String>(),
                            t.title
                                .unwrap_or_default()
                                .chars()
                                .take(26)
                                .collect::<String>(),
                            c.energie.unwrap_or(-1.0),
                            c.bpm.unwrap_or(-1.0),
                            c.genres
                        );
                    }
                }
            }
        }
    }

    /// Exporte de **vraies** données pour vérifier l'interface hors de Tauri
    /// (une webview du système ne se pilote pas) : pour chaque prompt, le plan
    /// interprété par Ollama et la composition sur la vraie bibliothèque, tels
    /// que l'interface les reçoit, plus les points de la carte. Ignoré par
    /// défaut : `RUSTY_DB=copie.db RUSTY_EXPORT=dossier cargo test -p
    /// rusty-music-desktop exporter_pour_maquette -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn exporter_pour_maquette() {
        use rusty_music_core::ollama;
        let base = std::env::var("RUSTY_DB").expect("RUSTY_DB");
        let dossier =
            std::path::PathBuf::from(std::env::var("RUSTY_EXPORT").expect("RUSTY_EXPORT"));
        let modele = std::env::var("OLLAMA_MODELE").unwrap_or_else(|_| "gemma4:e4b-mlx".into());
        let lib = rusty_music_core::db::Library::open(Path::new(&base)).expect("base");
        let vecteurs: Vec<Empreinte> = lib
            .embeddings(rusty_music_analysis::passe::MODELE)
            .expect("empreintes");
        let analyses: HashSet<i64> = vecteurs.iter().map(|(id, _)| *id).collect();
        let mut carac = lib.caracteristiques_pistes().expect("caractéristiques");
        carac.retain(|p| analyses.contains(&p.id));
        let graphe = Graphe::construire(
            &vecteurs,
            rusty_music_analysis::chemin::K_VOISINS,
            super::coeurs_arriere_plan(),
        );
        let encodeur = rusty_music_analysis::EmbedderTexte::charger(None).expect("CLAP texte");
        let vocab = lib.vocabulaire_genres(usize::MAX).unwrap();

        let points = lib
            .map_view(rusty_music_analysis::passe::MODELE)
            .expect("map_view");
        std::fs::write(
            dossier.join("points.json"),
            serde_json::to_string(&points).unwrap(),
        )
        .unwrap();
        let familles = lib
            .familles(rusty_music_analysis::passe::MODELE)
            .expect("familles");
        std::fs::write(
            dossier.join("familles.json"),
            serde_json::to_string(&familles).unwrap(),
        )
        .unwrap();

        for (nom, prompt) in [
            (
                "parties",
                "Rock, pendant 12 minutes, puis hip hop pendant 20 minutes",
            ),
            (
                "duree",
                "Une playlist calme de 60 minutes, sans rock, années 70",
            ),
            (
                "trajet",
                "Commencer par du Portishead, finir par du Massive Attack, 15 morceaux",
            ),
        ] {
            let i =
                ollama::interpreter(ollama::HOTE_DEFAUT, &modele, prompt, &vocab).expect("Ollama");
            let plan = PlanTexte {
                depart: None,
                arrivee: None,
                arrivee_demandee: None,
                etapes: i.etapes.clone(),
                n: i.n.map(|n| n as usize).unwrap_or(20),
                duree_minutes: i.duree_minutes,
                plafond_par_artiste: i.plafond_par_artiste,
                filtres: i.filtres(),
                reformulation: i.reformulation.clone(),
                parties: i.parties.clone(),
                brut: i.brut.clone(),
            };
            let parties = parties_du_plan(&plan, |e| Ok(encodeur.embed(e).unwrap())).unwrap();
            let c = composer_route(&plan, &parties, &carac, &vecteurs, &graphe, 3, BRUIT_DEFAUT)
                .expect("composition");
            let pistes: Vec<TrackRow> = c
                .route
                .iter()
                .filter_map(|id| lib.track(*id).unwrap())
                .collect();
            let duree_ms = pistes.iter().map(|t| t.duration_ms.unwrap_or(0)).sum();
            let r = super::CompositionTexte {
                pistes,
                relaches: c.relaches,
                duree_ms,
                parties: c.parties,
            };
            std::fs::write(
                dossier.join(format!("{nom}-plan.json")),
                serde_json::to_string(&plan).unwrap(),
            )
            .unwrap();
            std::fs::write(
                dossier.join(format!("{nom}-compo.json")),
                serde_json::to_string(&r).unwrap(),
            )
            .unwrap();
            eprintln!(
                "{nom} : {} morceaux, {} min",
                r.pistes.len(),
                r.duree_ms / 60_000
            );
        }
    }

    /// Robustesse de la **composition** : relit les interprétations écrites par
    /// l'exemple `robustesse` (`RUSTY_RESULTATS=res.json`) et compose chacune
    /// sur la vraie bibliothèque (une copie), sans Ollama. Vérifie des
    /// invariants, pas la musique : exclusions respectées, durée, plafond par
    /// artiste, bornes d'années, absence de doublon, jamais de panique.
    ///
    /// `RUSTY_DB=copie.db RUSTY_RESULTATS=res.json RUSTY_SOUS_ECH=6000 \
    ///  cargo test -p rusty-music-desktop robustesse_de_la_composition -- --ignored --nocapture`
    ///
    /// `RUSTY_SORTIE=compositions.json` écrit en plus le détail de chaque
    /// playlist (morceaux, genres, années, énergie, assouplissements, nombre de
    /// morceaux admissibles) — voir `experiments/musicrecointent/noter_playlist.py`,
    /// qui la juge contre des annotations.
    #[test]
    #[ignore]
    fn robustesse_de_la_composition() {
        let base = std::env::var("RUSTY_DB").expect("RUSTY_DB");
        let res = std::env::var("RUSTY_RESULTATS").expect("RUSTY_RESULTATS");
        let m: usize = std::env::var("RUSTY_SOUS_ECH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6000);
        let lib = rusty_music_core::db::Library::open(Path::new(&base)).expect("base");
        let tout = lib
            .embeddings(rusty_music_analysis::passe::MODELE)
            .expect("empreintes");
        let pas = (tout.len() / m).max(1);
        let vecteurs: Vec<Empreinte> = tout.into_iter().step_by(pas).collect();
        let analyses: HashSet<i64> = vecteurs.iter().map(|(id, _)| *id).collect();
        let mut carac = lib.caracteristiques_pistes().expect("caractéristiques");
        carac.retain(|p| analyses.contains(&p.id));
        let graphe = Graphe::construire(
            &vecteurs,
            rusty_music_analysis::chemin::K_VOISINS,
            super::coeurs_arriere_plan(),
        );
        let encodeur = rusty_music_analysis::EmbedderTexte::charger(None).expect("CLAP texte");
        let par_id: HashMap<i64, &CaracteristiquesPiste> =
            carac.iter().map(|p| (p.id, p)).collect();
        eprintln!("{} morceaux placables", vecteurs.len());

        let fichier: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(res).expect("résultats")).expect("json");
        let (mut ok, mut total, mut erreurs_moteur, mut paniques) = (0, 0, 0, 0);
        let mut sorties: Vec<serde_json::Value> = Vec::new();
        for r in fichier["resultats"].as_array().expect("resultats") {
            let id = r["id"].as_str().unwrap_or("?");
            let Some(spec) = r.get("spec") else { continue }; // interprétation échouée : rien à composer
            let i: rusty_music_core::ollama::InterpretationLlm =
                serde_json::from_value(spec.clone()).expect("spec");
            let plan = PlanTexte {
                depart: None,
                arrivee: None,
                arrivee_demandee: None,
                etapes: i.etapes.clone(),
                n: i.n.map(|n| n as usize).unwrap_or(20),
                duree_minutes: i.duree_minutes,
                plafond_par_artiste: i.plafond_par_artiste,
                filtres: i.filtres(),
                reformulation: i.reformulation.clone(),
                parties: i.parties.clone(),
                brut: String::new(),
            };
            total += 1;
            let parties = parties_du_plan(&plan, |e| Ok(encodeur.embed(e).unwrap())).unwrap();
            let t = Instant::now();
            let sortie = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                composer_route(&plan, &parties, &carac, &vecteurs, &graphe, 3, BRUIT_DEFAUT)
                    .map(|c| (c.route, c.relaches, c.parties))
            }));
            let ms = t.elapsed().as_millis();
            let (route, relaches, parties_res) = match sortie {
                Err(_) => {
                    paniques += 1;
                    eprintln!("✗ {id:<22} PANIQUE");
                    sorties.push(serde_json::json!({"id": id, "panique": true}));
                    continue;
                }
                Ok(Err(e)) => {
                    erreurs_moteur += 1;
                    eprintln!("· {id:<22} erreur moteur : {e}");
                    sorties.push(serde_json::json!({"id": id, "erreur": e.to_string()}));
                    continue;
                }
                Ok(Ok(v)) => v,
            };
            sorties.push(serde_json::json!({
                "id": id,
                "n_voulu": plan.n,
                "route": route.iter().filter_map(|i| par_id.get(i)).map(|p| serde_json::json!({
                    "id": p.id, "artiste": p.artiste, "annee": p.annee, "energie": p.energie,
                    "bpm": p.bpm, "genres": p.genres,
                })).collect::<Vec<_>>(),
                "relaches": relaches,
                "admissibles": parties_res.iter().map(|p| p.n_admissibles).collect::<Vec<_>>(),
            }));

            let mut pb: Vec<String> = Vec::new();
            let artiste_de = |id: &i64| -> String {
                par_id
                    .get(id)
                    .and_then(|p| p.artiste.clone())
                    .unwrap_or_default()
                    .to_lowercase()
            };
            // Exclusions dures : aucun genre ni artiste ne contient le mot (mot entier).
            let mot_dans = |texte: &str, mot: &str| -> bool {
                texte
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|m| m == mot)
                    || (mot.contains(' ') && texte.contains(mot))
            };
            if let Some(mots) = r["exclusions_dures"].as_array() {
                for mot in mots.iter().filter_map(|m| m.as_str()) {
                    let n_genre = route
                        .iter()
                        .filter(|id| {
                            par_id
                                .get(id)
                                .is_some_and(|p| p.genres.iter().any(|g| mot_dans(g, mot)))
                        })
                        .count();
                    let n_art = route
                        .iter()
                        .filter(|id| artiste_de(id).contains(mot))
                        .count();
                    if n_genre + n_art > 0 {
                        pb.push(format!(
                            "« {mot} » : {n_genre} morceau(x) de ce genre, {n_art} de cet artiste"
                        ));
                    }
                }
            }
            let mut vus = HashSet::new();
            if route.iter().any(|id| !vus.insert(*id)) {
                pb.push("doublon".into());
            }
            let duree: i64 = route
                .iter()
                .map(|id| par_id.get(id).and_then(|p| p.duree_ms).unwrap_or(0))
                .sum();
            if let Some(d) = plan.duree_minutes {
                let cible = i64::from(d) * 60_000;
                let ecart = (duree - cible).abs() as f64 / cible as f64;
                let dit = relaches.iter().any(|r| r.contains("durée visée"));
                if ecart > 0.12 && !dit {
                    pb.push(format!(
                        "durée {} min pour {d} visées, sans le dire",
                        duree / 60_000
                    ));
                }
            }
            if let Some(p) = plan.plafond_par_artiste {
                let mut compte: HashMap<String, usize> = HashMap::new();
                for id in &route {
                    *compte.entry(artiste_de(id)).or_default() += 1;
                }
                if let Some((a, n)) = compte.iter().find(|(_, n)| **n > p as usize) {
                    pb.push(format!("plafond {p} dépassé : {a} ×{n}"));
                }
            }
            if let Some(min) = plan.filtres.annee_min {
                let hors = route
                    .iter()
                    .filter(|id| {
                        par_id
                            .get(id)
                            .and_then(|p| p.annee)
                            .is_some_and(|a| a < min)
                    })
                    .count();
                if hors > 0 && !relaches.iter().any(|r| r.contains("période")) {
                    pb.push(format!("{hors} morceau(x) avant {min}, sans le dire"));
                }
            }
            if let Some(max) = plan.filtres.annee_max {
                let hors = route
                    .iter()
                    .filter(|id| {
                        par_id
                            .get(id)
                            .and_then(|p| p.annee)
                            .is_some_and(|a| a > max)
                    })
                    .count();
                if hors > 0 && !relaches.iter().any(|r| r.contains("période")) {
                    pb.push(format!("{hors} morceau(x) après {max}, sans le dire"));
                }
            }
            if route.len() < 5 {
                pb.push(format!("seulement {} morceau(x)", route.len()));
            }
            // Parties : se suivent sans trou, couvrent toute la route, chacune
            // dans son genre quand elle en a un, et près de sa durée.
            let mut attendu_debut = 0;
            for (k, pr) in parties_res.iter().enumerate() {
                if pr.debut != attendu_debut {
                    pb.push(format!(
                        "partie {k} : début {} au lieu de {attendu_debut}",
                        pr.debut
                    ));
                }
                attendu_debut = pr.fin;
                if let Some(partie) = plan.parties.get(k) {
                    let dit = pr.relaches.iter().any(|r| {
                        r.contains("abandonn")
                            || r.contains("déjà pris")
                            || r.contains("durée visée")
                    });
                    if !partie.genres.is_empty() && !dit {
                        let hors = route[pr.debut..pr.fin]
                            .iter()
                            .filter(|id| {
                                // Départ nommé exclu : seuls comptent les morceaux choisis.
                                !par_id.get(id).is_some_and(|p| {
                                    partie.genres.iter().any(|g| {
                                        p.genres.iter().any(|pg| {
                                            pg.to_lowercase()
                                                .split(|c: char| !c.is_alphanumeric())
                                                .any(|m| {
                                                    g.to_lowercase()
                                                        .split(|c: char| !c.is_alphanumeric())
                                                        .any(|x| x == m)
                                                })
                                        })
                                    })
                                })
                            })
                            .count();
                        if hors > 0 {
                            pb.push(format!("partie {k} ({:?}) : {hors} morceau(x) d'un autre genre, sans le dire", partie.genres));
                        }
                    }
                    if let Some(m) = partie.duree_minutes {
                        let cible = i64::from(m) * 60_000;
                        let ecart = (pr.duree_ms - cible).abs() as f64 / cible as f64;
                        if ecart > 0.25 && !dit {
                            pb.push(format!(
                                "partie {k} : {} min pour {m} visées, sans le dire",
                                pr.duree_ms / 60_000
                            ));
                        }
                    }
                }
            }
            if attendu_debut != route.len() {
                pb.push(format!(
                    "les parties couvrent {attendu_debut} morceaux sur {}",
                    route.len()
                ));
            }
            if pb.is_empty() {
                ok += 1;
                eprintln!(
                    "✓ {id:<22} {:>3} morceaux, {:>3} min, {ms} ms{}",
                    route.len(),
                    duree / 60_000,
                    if relaches.is_empty() {
                        String::new()
                    } else {
                        format!(" — assouplis : {}", relaches.len())
                    }
                );
                if parties_res.len() > 1 || !relaches.is_empty() {
                    for pr in &parties_res {
                        eprintln!(
                            "      · {:<18} {:>2} morceaux, {:>3} min (visé {}), {} admissibles, départ : {}",
                            pr.libelle, pr.fin - pr.debut, pr.duree_ms / 60_000,
                            pr.cible_ms.map_or("—".into(), |c| format!("{} min", c / 60_000)),
                            pr.n_admissibles, pr.depart_pourquoi
                        );
                        for r in &pr.relaches {
                            eprintln!("          ↳ {r}");
                        }
                    }
                }
            } else {
                eprintln!(
                    "✗ {id:<22} {:>3} morceaux, {:>3} min",
                    route.len(),
                    duree / 60_000
                );
                for p in pb {
                    eprintln!("      {p}");
                }
            }
        }
        eprintln!("\n== composition : {ok}/{total} sans problème, {erreurs_moteur} refus clairs du moteur, {paniques} paniques");
        if let Ok(chemin) = std::env::var("RUSTY_SORTIE") {
            let artistes: std::collections::BTreeSet<&str> =
                carac.iter().filter_map(|p| p.artiste.as_deref()).collect();
            let sortie = serde_json::json!({
                "compositions": sorties,
                "vocabulaire": lib.vocabulaire_genres(usize::MAX).unwrap_or_default(),
                "artistes": artistes,
                "morceaux_placables": carac.len(),
            });
            std::fs::write(chemin, serde_json::to_string(&sortie).unwrap())
                .expect("écriture de RUSTY_SORTIE");
        }
    }
}
