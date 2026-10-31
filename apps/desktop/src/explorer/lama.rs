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
    pub(crate) depart: Option<TrackRow>,
    /// `None` si le prompt ne demande pas d'arrivée précise (cas le plus
    /// courant : une simple dérive depuis `depart`) — `path_texte` dérive
    /// alors sans viser de point d'arrivée, comme avant l'ajout de ce champ.
    /// Présent, `path_texte` termine la marche guidée par un raccordement
    /// exact vers ce morceau (`Graphe::sonique`), pour qu'un « arriver à X »
    /// explicite dans le prompt tienne sa promesse plutôt que de rester une
    /// simple étape textuelle diluée parmi d'autres.
    pub(crate) arrivee: Option<TrackRow>,
    /// Le nom que le LLM a cru reconnaître comme arrivée, **que la recherche
    /// l'ait trouvé ou non** — `None` seulement si le prompt ne demandait
    /// aucune arrivée. Sert à distinguer, dans l'inspecteur, « pas d'arrivée
    /// demandée » de « arrivée demandée mais introuvable » : régression du 14
    /// septembre (« RATM » cherché tel quel, jamais trouvé, la playlist
    /// dérivait quand même sans arrivée sans que rien ne le montre) —
    /// `arrivee` seul à `None` ne disait pas lequel des deux cas s'était
    /// produit.
    pub(crate) arrivee_demandee: Option<String>,
    /// Toujours au moins une étape — `ollama::interpreter` le garantit déjà.
    pub(crate) etapes: Vec<String>,
    pub(crate) n: usize,
    /// Durée totale voulue : prime sur `n`, la playlist s'arrête à cette
    /// durée plutôt qu'à un nombre de morceaux.
    #[serde(default)]
    pub(crate) duree_minutes: Option<u32>,
    /// Plafond de morceaux par artiste **demandé** par l'utilisateur — jamais
    /// relâché. Absent : un plafond par défaut, lui relâché au besoin
    /// (`filtres_playlist::plafonner_par_artiste`).
    #[serde(default)]
    pub(crate) plafond_par_artiste: Option<u32>,
    /// Les contraintes typées appliquées à la bibliothèque avant la marche
    /// (`filtres_playlist`). `#[serde(default)]` : un plan sans ce champ ne
    /// restreint rien.
    #[serde(default)]
    pub(crate) filtres: FiltresPlaylist,
    /// Une phrase où Ollama reformule la demande — affichée telle quelle, sous
    /// « Ollama a compris » : de quoi vérifier d'un coup d'œil qu'il a bien lu.
    #[serde(default)]
    pub(crate) reformulation: Option<String>,
    /// Les parties d'une playlist « X puis Y » (genre, énergie, durée chacune).
    /// Vide : une seule partie implicite — `etapes` et `filtres`. Éditables
    /// dans l'interface avant « Recomposer ».
    #[serde(default)]
    pub(crate) parties: Vec<rusty_music_core::ollama::PartieLlm>,
    /// La réponse du modèle, telle qu'il l'a écrite — pour le dépliant « réponse
    /// brute d'Ollama ».
    #[serde(default)]
    pub(crate) brut: String,
}

/// Ce que `path_texte` rend : la playlist, et ce qu'il faut en savoir.
#[derive(Debug, serde::Serialize)]
pub(crate) struct CompositionTexte {
    pub(crate) pistes: Vec<TrackRow>,
    /// Une phrase par critère abandonné ou limite rencontrée (trop peu de
    /// morceaux admissibles, marche écourtée, durée non atteinte) — montrées
    /// à l'utilisateur, jamais tues : une playlist « calme » qui ne l'est plus
    /// doit le dire.
    pub(crate) relaches: Vec<String>,
    /// Durée cumulée des pistes rendues.
    pub(crate) duree_ms: i64,
    /// Ce que le moteur a fait, partie par partie : l'« entonnoir » des filtres,
    /// le départ choisi et pourquoi, la durée obtenue contre la durée visée.
    /// Toujours au moins une entrée (une playlist sans partie en a une
    /// implicite) ; l'interface s'en sert pour expliquer le résultat.
    pub(crate) parties: Vec<PartieResultat>,
}

/// Une partie composée : sa place dans la file et ce qui l'a produite.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PartieResultat {
    pub(crate) libelle: String,
    /// Indices dans `pistes` : `debut` inclus, `fin` exclu.
    pub(crate) debut: usize,
    pub(crate) fin: usize,
    pub(crate) duree_ms: i64,
    pub(crate) cible_ms: Option<i64>,
    /// Morceaux admissibles avant la marche (hors ceux d'une partie précédente).
    pub(crate) n_admissibles: usize,
    /// `(critère, morceaux restants)` — voir `filtres_playlist::entonnoir`.
    pub(crate) entonnoir: Vec<(String, usize)>,
    pub(crate) depart_pourquoi: String,
    pub(crate) relaches: Vec<String>,
    /// Les admissibles, pour estomper le reste sur le mini-nuage. Vide : aucun
    /// filtre, tout est admissible.
    pub(crate) ids_admissibles: Vec<i64>,
}

/// Interprète un prompt de playlist en texte libre via Ollama, et résout son
/// morceau ou artiste de départ dans la bibliothèque — sans encore calculer
/// d'empreinte CLAP-texte ni construire de trajet, pour rester rapide :
/// l'utilisateur doit pouvoir corriger l'interprétation dans l'inspecteur
/// avant qu'elle ne remplace sa file d'écoute (`path_texte` fait le reste).
///
/// `modele` : celui choisi par l'utilisateur via l'icône 🦙 (mémorisé côté
/// interface). `None` — rien choisi encore — retombe sur
/// `ollama::modele_par_defaut`, pas sur un nom fixe : deviner un modèle qui
/// n'est pas installé rend un 404 qu'Ollama ne distingue pas clairement d'un
/// serveur injoignable (observé en pratique).
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
    let modele = match modele.filter(|m| !m.is_empty()) {
        Some(m) => m,
        None => rusty_music_core::ollama::modele_par_defaut(rusty_music_core::ollama::HOTE_DEFAUT)
            .map_err(echec)?,
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
    let interpretation = rusty_music_core::ollama::interpreter(
        rusty_music_core::ollama::HOTE_DEFAUT,
        &modele,
        &prompt,
        &vocabulaire,
    )
    .map_err(echec)?;

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
    pub(crate) route: Vec<i64>,
    pub(crate) relaches: Vec<String>,
    pub(crate) parties: Vec<PartieResultat>,
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
