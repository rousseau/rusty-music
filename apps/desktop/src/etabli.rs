// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes de l'établi, communes à Pratiquer et Créer : démixage et stems,
//! vitesse et hauteur, export, lecture et boucles, pulsation, sections,
//! spectres.

use super::*;

/// Avancement du démixage, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct AvancementDemix {
    /// Le morceau en cours de traitement, pour que l'interface sache si les
    /// stems affichés sont bien les siens.
    pub(crate) source: String,
    /// Où en est le travail : « telechargement » (poids de la variante absents,
    /// récupérés d'abord) puis « separation ». Vide hors traitement.
    pub(crate) phase: String,
    /// Téléchargement des poids : octets reçus et attendus (taille connue
    /// d'avance, la barre est donc toujours graduée).
    pub(crate) octets_faits: u64,
    pub(crate) octets_total: u64,
    /// Segments audio traités et total, pour la barre de progression. `total`
    /// reste à zéro tant que le découpage n'est pas connu (décodage puis
    /// chauffe du modèle, ou morceau trop court pour être découpé) : la barre
    /// se montre alors indéterminée.
    pub(crate) segments_faits: usize,
    pub(crate) segments_total: usize,
    /// Variante affinée : le stem dont le réseau tourne en ce moment.
    pub(crate) stem: Option<String>,
    /// Stems produits : nom et chemin du WAV.
    pub(crate) stems: Vec<(String, String)>,
}
pub(crate) type EtatDemix = Passe<AvancementDemix>;

/// Où déposer les stems : à côté de la base, dans un sous-dossier par morceau.
///
/// Pas à côté du morceau d'origine : la bibliothèque est en lecture seule par
/// principe — c'est une source, pas un espace de travail — et elle vit ici sur
/// une carte SD lente.
pub(crate) fn dossier_stems(etat: &State<Etat>, source: &Path) -> PathBuf {
    let base = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "morceau".into());
    racine_stems(etat).join(base)
}

/// Racine du cache de stems, à côté de la base.
pub(crate) fn racine_stems(etat: &State<Etat>) -> PathBuf {
    etat.db.parent().unwrap_or(Path::new(".")).join("stems")
}

/// Vitesse de lecture des stems, appliquée immédiatement.
///
/// **Rien n'est rechargé** : la vitesse est un flottant que la lecture relit à
/// chaque trame, et la position ne bouge pas. C'est la différence avec
/// [`start_etirer`], qui réécrit un fichier et coûte des dizaines de secondes.
///
/// **La hauteur ne suit pas la vitesse.** L'étireur `wsola` travaille dans la
/// lecture elle-même — recouvrement-addition temporel, la méthode d'`atempo`.
/// Le commentaire qui figurait ici disait le contraire ; il datait d'avant que
/// l'étireur soit branché, quand la vitesse n'était qu'un rééchantillonnage.
#[tauri::command(async)]
pub(crate) fn stems_vitesse(etat: State<Etat>, vitesse: f32) -> Result<(), String> {
    if let Some(m) = verrou(&etat.stems).as_ref() {
        m.vitesse(vitesse);
    }
    Ok(())
}

/// Vitesse d'un seul stem — le réglage qui désynchronise.
///
/// **C'est un effet, pas un réglage**, et la spec le dit depuis sa décision 4 :
/// deux stems à des vitesses différentes ont deux têtes de lecture, l'écart
/// grandit tant que la lecture continue, et rien ne le rattrape tout seul.
/// L'interface a donc deux devoirs qu'elle n'a pas ailleurs : montrer la
/// dérive mesurée (`stems_state`) et offrir de réaligner.
///
/// Aussi immédiat que la vitesse globale : un flottant que la lecture relit.
#[tauri::command(async)]
pub(crate) fn stems_vitesse_stem(
    etat: State<Etat>,
    index: usize,
    vitesse: f32,
) -> Result<(), String> {
    if let Some(m) = verrou(&etat.stems).as_ref() {
        m.vitesse_stem(index, vitesse);
    }
    Ok(())
}

/// Applique une transposition à un jeu de stems, et rend les chemins traités.
///
/// **Une valeur par stem.** Transposer la basse d'une quinte sans toucher au
/// reste est un geste courant ; l'exiger sur les quatre à la fois ne l'était
/// pas. Contrairement à la vitesse, cela ne désynchronise rien : la
/// transposition rend la durée qu'elle a reçue.
///
/// **Le résultat est mis en cache sur le disque**, un dossier par valeur et par
/// stem. Revenir à un réglage déjà calculé est alors immédiat, l'aller-retour
/// entre deux valeurs — le geste naturel quand on cherche la bonne — ne
/// recalcule rien, et **changer la hauteur d'un seul stem ne recalcule que
/// celui-là**. Le sous-dossier reste invisible de `stems_existants`, qui ne lit
/// que le premier niveau.
///
/// Un stem à zéro demi-ton rend son chemin d'origine : il n'y a rien à
/// calculer, et écrire une copie identique serait 31 Mo pour rien.
/// Où va la version transposée d'un stem. `None` quand il n'y a rien à faire.
///
/// Séparé du calcul pour que l'on puisse répondre à « y a-t-il du travail ? »
/// sans rien calculer — c'est ce qui permet au cas déjà en cache de rester
/// instantané.
pub(crate) fn cible_transposee(chemin: &str, demi_ton: f32) -> Option<PathBuf> {
    if demi_ton.abs() < 1e-3 {
        return None;
    }
    let source = PathBuf::from(chemin);
    Some(
        source
            .parent()
            .unwrap_or(Path::new("."))
            .join("traites")
            .join(format!("t{:+03}", demi_ton as i32))
            .join(source.file_name().unwrap_or_default()),
    )
}

/// Transpose un stem et rend le chemin du résultat. Ne recalcule rien si le
/// fichier est déjà là.
pub(crate) fn transposer_un(chemin: &str, demi_ton: f32) -> Result<String, String> {
    use rusty_music_editor::{decode, etirement, wav};

    let Some(cible) = cible_transposee(chemin, demi_ton) else {
        return Ok(chemin.to_string());
    };
    if cible.exists() {
        return Ok(cible.display().to_string());
    }
    let dossier = cible.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dossier).map_err(echec)?;

    let source = PathBuf::from(chemin);
    let s = decode::stereo(&source).map_err(echec)?;
    let entrelace: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .flat_map(|(g, d)| [*g, *d])
        .collect();
    let out = etirement::transposer(&entrelace, 2, demi_ton);
    let (g, d): (Vec<f32>, Vec<f32>) = out.chunks_exact(2).map(|c| (c[0], c[1])).unzip();
    wav::ecrire(&cible, &g, &d, 44_100).map_err(echec)?;
    tracing::info!(chemin, demi_ton, "stem transposé");
    Ok(cible.display().to_string())
}

/// Où en est la transposition. Sondé par l'interface, comme le démixage.
#[derive(Default, Clone, serde::Serialize)]
pub(crate) struct EtatTranspose {
    pub(crate) en_cours: bool,
    /// Stems déjà traités, et combien il y en a en tout. Un stem de quatre
    /// minutes demande une vingtaine de secondes : un pourcentage global
    /// laisserait croire à un blocage entre deux.
    faits: usize,
    total: usize,
    /// Les chemins à jouer, une fois tout fini.
    stems: Vec<(String, String)>,
    pub(crate) erreur: Option<String>,
}

// `EtatTranspose` porte `erreur`, pas `resultat` — hors du patron commun,
// implémentation à la main.
impl EtatPasse for EtatTranspose {
    fn en_cours(&self) -> bool {
        self.en_cours
    }
    fn marquer_echec_interne(&mut self) {
        self.en_cours = false;
        self.erreur = Some("échec : panique interne".to_string());
    }
}

/// Lance la transposition des stems **dans son fil**.
///
/// **Pourquoi un fil, alors que la commande était déjà `async`.** Une commande
/// Tauri `async` tourne sur le runtime, pas sur le fil de l'interface — mais
/// une boucle de calcul qui ne rend jamais la main y monopolise un ouvrier du
/// runtime. Quatre stems à une vingtaine de secondes, et **toutes les autres
/// commandes attendent derrière** : le sondage du transport, l'état de lecture,
/// le moindre clic. L'interface ne gelait pas, elle faisait la queue — ce qui
/// se voit pareil.
///
/// `async` suffit pour une commande qui attend ; il ne suffit pas pour une
/// commande qui calcule. Le démixage l'avait déjà réglé ainsi, la transposition
/// ne l'avait pas suivi parce qu'elle était courte du temps du vocodeur de
/// phase — 0,84 s par stem. `wsola` l'a portée à 17,9 s sans que ce chemin-là
/// soit revu.
///
/// **Rien à calculer = rien à attendre.** Si tous les stems sont neutres ou
/// déjà en cache, l'état est rempli sur place et aucun fil n'est lancé : le
/// chargement d'un jeu de stems sans réglage reste immédiat.
#[tauri::command(async)]
pub(crate) fn start_etirer(
    app: tauri::AppHandle,
    etat: State<Etat>,
    stems: Vec<(String, String)>,
    demi_tons: Vec<f32>,
) -> Result<EtatTranspose, String> {
    let a_faire = stems
        .iter()
        .enumerate()
        .filter(|(i, (_, chemin))| {
            cible_transposee(chemin, demi_tons.get(*i).copied().unwrap_or(0.0))
                .is_some_and(|c| !c.exists())
        })
        .count();

    if a_faire == 0 {
        let mut sortie = Vec::with_capacity(stems.len());
        for (i, (nom, chemin)) in stems.iter().enumerate() {
            let demi_ton = demi_tons.get(i).copied().unwrap_or(0.0);
            sortie.push((nom.clone(), transposer_un(chemin, demi_ton)?));
        }
        let fini = EtatTranspose {
            en_cours: false,
            faits: 0,
            total: 0,
            stems: sortie,
            erreur: None,
        };
        *verrou(&etat.transpose) = fini.clone();
        return Ok(fini);
    }

    {
        let mut t = verrou(&etat.transpose);
        if t.en_cours {
            return Err("une transposition est déjà en cours".into());
        }
        *t = EtatTranspose {
            en_cours: true,
            faits: 0,
            total: a_faire,
            stems: Vec::new(),
            erreur: None,
        };
    }

    let depart = verrou(&etat.transpose).clone();
    std::thread::spawn(move || {
        let etat = app.state::<Etat>();
        let _garde = GardePasse::nouvelle(&etat.transpose);

        // **Un fil par stem.** Mesuré sur un morceau de 272 s : 92,0 s en file
        // contre 23,5 s en parallèle, soit 3,9× — l'étirement est le seul poste
        // qui compte (22,8 s des 23,0 s d'un stem) et il ne partage rien.
        //
        // Ce qui rendait cette parallélisation dangereuse avant, et ne l'est
        // plus : chaque transposition tenait cinq tampons pleins du signal, et
        // quatre à la fois faisaient pagineur la machine — ce qui fige
        // l'interface aussi sûrement qu'un calcul mal placé.
        // `reechantillonner_entrelace` les a ramenés à deux.
        let resultats: Vec<Result<String, String>> = std::thread::scope(|portee| {
            let taches: Vec<_> = stems
                .iter()
                .enumerate()
                .map(|(i, (_, chemin))| {
                    let demi_ton = demi_tons.get(i).copied().unwrap_or(0.0);
                    let etat = &etat;
                    portee.spawn(move || {
                        // Le compteur n'avance que sur les stems réellement
                        // calculés : un stem déjà en cache passe en quelques
                        // microsecondes, et le voir sauter ne dirait rien de
                        // juste.
                        let travail =
                            cible_transposee(chemin, demi_ton).is_some_and(|c| !c.exists());
                        let issue = transposer_un(chemin, demi_ton);
                        if travail && issue.is_ok() {
                            {
                                let mut t = verrou(&etat.transpose);
                                t.faits += 1;
                            }
                        }
                        issue
                    })
                })
                .collect();
            taches
                .into_iter()
                .map(|t| t.join().unwrap_or_else(|_| Err("fil interrompu".into())))
                .collect()
        });

        let mut sortie = Vec::with_capacity(stems.len());
        let mut erreur = None;
        for ((nom, _), issue) in stems.iter().zip(resultats) {
            match issue {
                Ok(c) => sortie.push((nom.clone(), c)),
                Err(e) => erreur = Some(e),
            }
        }

        let mut t = verrou(&etat.transpose);
        t.en_cours = false;
        t.stems = if erreur.is_some() { Vec::new() } else { sortie };
        t.erreur = erreur;
    });

    Ok(depart)
}

/// Où en est la transposition lancée par [`start_etirer`].
#[tauri::command(async)]
pub(crate) fn etirer_state(etat: State<Etat>) -> Result<EtatTranspose, String> {
    Ok(verrou(&etat.transpose).clone())
}

/// La racine surveillée qui contient `dossier`, s'il y en a une.
///
/// Comparaison composant par composant, jamais sur les chaînes : `/Musique2`
/// n'est pas dans `/Musique`, alors qu'un `starts_with` textuel le dirait.
pub(crate) fn sous_une_racine(dossier: &Path, racines: &[String]) -> Option<String> {
    racines
        .iter()
        .find(|r| dossier.starts_with(Path::new(r)))
        .cloned()
}

/// Exporte ce qu'on entend : le mélange des stems audibles, avec leurs niveaux,
/// leur vitesse et leur transposition.
///
/// **Une seule sortie, et c'est un choix.** La spec en prévoyait trois — un
/// stem, la sélection, le mélange. Mettre un stem en solo *est* la sélection :
/// un menu de plus n'aurait dit que ce que le dock montre déjà.
///
/// **Refuse d'écrire dans la bibliothèque surveillée.** Un fichier déposé sous
/// une racine serait ingéré, analysé et placé sur la carte : ce n'est pas un
/// morceau, c'est un rendu. Mieux vaut le dire tout de suite que laisser la
/// surprise arriver à la prochaine surveillance.
#[tauri::command(async)]
pub(crate) fn stems_exporter(
    etat: State<Etat>,
    stems: Vec<(String, String)>,
    niveaux: Vec<f32>,
    vitesses: Vec<f32>,
    demi_tons: Vec<f32>,
    destination: String,
    nom: String,
) -> Result<String, String> {
    use rusty_music_editor::{decode, etirement, wav};

    if stems.is_empty() {
        return Err("aucun stem à exporter".into());
    }
    let dossier = PathBuf::from(&destination);
    {
        let lib = verrou(&etat.lib);
        let racines: Vec<String> = lib
            .roots()
            .map_err(echec)?
            .into_iter()
            .map(|r| r.path)
            .collect();
        if let Some(racine) = sous_une_racine(&dossier, &racines) {
            return Err(format!(
                "{} est dans la bibliothèque surveillée ({racine}). Un rendu y \
                 serait ingéré comme un morceau — choisis un autre dossier.",
                dossier.display()
            ));
        }
    }

    // Somme des stems audibles, à leur niveau. Un niveau nul — coupé, ou non
    // retenu par un solo — ne contribue pas et n'a même pas besoin d'être lu.
    //
    // **Chaque stem est traité avant d'être sommé, et non l'inverse.** Tant que
    // la vitesse était globale, étirer le mélange revenait au même et coûtait
    // un étirement au lieu de quatre. Avec une vitesse par stem, ce n'est plus
    // vrai : c'est justement le décalage entre les stems qu'il faut rendre, et
    // il n'existe pas dans leur somme.
    let mut melange: Vec<f32> = Vec::new();
    for (i, (_, chemin)) in stems.iter().enumerate() {
        let niveau = niveaux.get(i).copied().unwrap_or(1.0);
        if niveau <= 1e-4 {
            continue;
        }
        let s = decode::stereo(Path::new(chemin)).map_err(echec)?;
        let mut piste: Vec<f32> = s
            .gauche
            .iter()
            .zip(&s.droite)
            .flat_map(|(g, d)| [*g * niveau, *d * niveau])
            .collect();

        // La vitesse est un tempo à la lecture ; à l'écrit, c'est une durée.
        let vitesse = vitesses.get(i).copied().unwrap_or(1.0);
        if (vitesse - 1.0).abs() > 1e-3 {
            piste = etirement::etirer(&piste, 2, 1.0 / vitesse);
        }
        let demi_ton = demi_tons.get(i).copied().unwrap_or(0.0);
        if demi_ton.abs() > 1e-3 {
            piste = etirement::transposer(&piste, 2, demi_ton);
        }

        // Les stems n'ont plus la même longueur dès que leurs vitesses
        // diffèrent : le plus long commande, les autres se taisent avant.
        if melange.len() < piste.len() {
            melange.resize(piste.len(), 0.0);
        }
        for (d, v) in melange.iter_mut().zip(&piste) {
            *d += v;
        }
    }
    if melange.is_empty() {
        return Err("tout est coupé : il n'y a rien à exporter".into());
    }

    std::fs::create_dir_all(&dossier).map_err(echec)?;
    let cible = dossier.join(format!("{nom}.wav"));
    // La somme des stems reconstitue un mélange qui frôlait déjà la pleine
    // échelle : sans écrêtage, un solo à plein niveau saturerait.
    let (g, d): (Vec<f32>, Vec<f32>) = melange
        .chunks_exact(2)
        .map(|c| (c[0].clamp(-1.0, 1.0), c[1].clamp(-1.0, 1.0)))
        .unzip();
    wav::ecrire(&cible, &g, &d, 44_100).map_err(echec)?;
    tracing::info!(fichier = %cible.display(), "export écrit");
    Ok(cible.display().to_string())
}

/// Poids d'un dossier, sous-dossiers compris.
///
/// La récursion n'est pas gratuite ici : les versions étirées et transposées
/// vivent un niveau plus bas, et chacune pèse autant que l'original. Les
/// ignorer sous-estimerait le cache d'un facteur quelconque.
pub(crate) fn poids(dossier: &Path) -> u64 {
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return 0;
    };
    entrees
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                poids(&p)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Taille du cache de stems et nombre de morceaux séparés.
///
/// **Un jeu de quatre stems pèse 124 Mo** en WAV — quinze morceaux séparés font
/// deux gigaoctets. Sans ce compte, c'est une fuite qu'on ne découvre qu'en
/// cherchant pourquoi le disque se remplit (`docs/ui-spec-editeur.md`).
#[tauri::command(async)]
pub(crate) fn stems_cache(etat: State<Etat>) -> Result<(u64, usize), String> {
    let racine = racine_stems(&etat);
    let Ok(entrees) = std::fs::read_dir(&racine) else {
        return Ok((0, 0));
    };
    let mut octets = 0u64;
    let mut morceaux = 0usize;
    for morceau in entrees.flatten() {
        if !morceau.path().is_dir() {
            continue;
        }
        morceaux += 1;
        octets += poids(&morceau.path());
    }
    Ok((octets, morceaux))
}

/// Vide le cache de stems.
///
/// Coupe d'abord la lecture multipiste : effacer les fichiers d'un jeu en train
/// de jouer laisserait la sortie audio sur des données disparues.
#[tauri::command(async)]
pub(crate) fn stems_cache_vider(etat: State<Etat>) -> Result<(), String> {
    *verrou(&etat.stems) = None;
    let racine = racine_stems(&etat);
    if racine.is_dir() {
        std::fs::remove_dir_all(&racine).map_err(echec)?;
    }
    tracing::info!(dossier = %racine.display(), "cache de stems vidé");
    Ok(())
}

/// Lance le démixage d'un morceau.
///
/// Tourne dans son fil : une séparation demande une trentaine de secondes pour
/// un morceau de quatre minutes, et le premier lancement y ajoute la
/// compilation des noyaux GPU.
#[tauri::command(async)]
pub(crate) fn start_demix(
    app: tauri::AppHandle,
    etat: State<Etat>,
    path: String,
    variant: String,
) -> Result<(), String> {
    let source = PathBuf::from(&path);
    if !source.is_file() {
        return Err(format!("{path} est introuvable"));
    }
    let variante = rusty_music_editor::Variante::analyser(&variant)
        .ok_or_else(|| format!("variante inconnue : {variant}"))?;
    let sortie = dossier_stems(&etat, &source);
    let depart = AvancementDemix {
        source: path.clone(),
        ..Default::default()
    };
    passe::lancer(
        app,
        |e| &e.demix,
        "un démixage est déjà en cours",
        depart,
        move |etat| {
            // Les poids de la variante manquent (la première fois, typiquement pour
            // la variante affinée, 336 Mo) : on les télécharge, plutôt que de
            // renvoyer l'utilisateur à un script. La phase et la progression sont
            // posées **avant** la première réponse du serveur, pour que
            // l'interface annonce le téléchargement tout de suite.
            if !variante.presente() {
                {
                    let mut e = verrou(&etat.demix);
                    e.avancement.phase = "telechargement".into();
                    e.avancement.octets_total = variante.octets();
                }
            }
            let poids = rusty_music_editor::Demixeur::assurer_poids(variante, |vus, total| {
                let mut e = verrou(&etat.demix);
                e.avancement.octets_faits = vus;
                e.avancement.octets_total = total.unwrap_or_else(|| variante.octets());
            });
            {
                let mut e = verrou(&etat.demix);
                e.avancement.phase = "separation".into();
                e.avancement.octets_faits = 0;
                e.avancement.octets_total = 0;
            }

            let issue = poids
                .and_then(|_| rusty_music_editor::Demixeur::charger(None, variante))
                .and_then(|d| {
                    d.chauffer();
                    d.separer_fichier_suivi(&source, &sortie, |p| {
                        let mut e = verrou(&etat.demix);
                        e.avancement.segments_faits = p.segments_faits;
                        e.avancement.segments_total = p.segments_total;
                        e.avancement.stem = p.stem.map(str::to_string);
                    })
                });

            let (stems, bilan) = match issue {
                Ok(fichiers) => {
                    oublier_derives_des_stems(&sortie, &fichiers);
                    let noms: Vec<(String, String)> = fichiers
                        .iter()
                        .map(|f| {
                            let nom = f
                                .file_stem()
                                .map(|s| {
                                    s.to_string_lossy()
                                        .rsplit('—')
                                        .next()
                                        .unwrap_or("")
                                        .trim()
                                        .to_string()
                                })
                                .unwrap_or_default();
                            (nom, f.display().to_string())
                        })
                        .collect();
                    let n = noms.len();
                    (noms, format!("{n} stems écrits"))
                }
                Err(e) => (Vec::new(), format!("échec : {e}")),
            };
            tracing::info!(%bilan, "démixage terminé");

            {
                let mut d = verrou(&etat.demix);
                d.avancement.phase = String::new();
                d.avancement.stem = None;
                d.avancement.stems = stems;
            }
            bilan
        },
    )
}

/// Après un nouveau démixage, retire ce qui venait des stems précédents : un
/// démixage écrase le précédent (un seul jeu de stems par morceau).
/// - les stems que la nouvelle variante ne produit pas (`guitar` et `piano`
///   d'un 6 stems quand on revient à 4) ;
/// - les partitions en cache (`transcription-*.json`), calculées sur l'ancien
///   stem — sinon « Calculer » rendait la partition d'avant ;
/// - `traites/`, les versions transposées des anciens stems.
///
/// La pulsation et les sections viennent du mélange : elles restent. Les
/// greffes aussi (`greffes/`), qui sont des stems venus d'ailleurs.
pub(crate) fn oublier_derives_des_stems(dossier: &Path, gardes: &[PathBuf]) {
    for (nom, p) in stems_du_dossier(dossier) {
        if !gardes.iter().any(|g| g.file_name() == p.file_name()) {
            tracing::info!(stem = %nom, "ancien stem retiré");
            let _ = std::fs::remove_file(&p);
        }
    }
    if let Ok(entrees) = std::fs::read_dir(dossier) {
        for p in entrees.filter_map(|e| e.ok()).map(|e| e.path()) {
            let nom = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if nom.starts_with("transcription-") && nom.ends_with(".json") {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    let _ = std::fs::remove_dir_all(dossier.join("traites"));
}

/// Ce que l'interface doit savoir d'une variante avant d'en proposer le bouton.
#[cfg_attr(test, derive(specta::Type))]
#[derive(serde::Serialize)]
pub(crate) struct VarianteDispo {
    nom: &'static str,
    /// Poids déjà sur la machine : sinon le premier démixage les télécharge.
    presente: bool,
    megaoctets: u32,
}

/// Les variantes de séparation et leur disponibilité locale — pour annoncer le
/// téléchargement **avant** le clic, comme l'analyse annonce son coût.
#[tauri::command(async)]
#[cfg_attr(test, specta::specta)]
pub(crate) fn demix_variantes() -> Vec<VarianteDispo> {
    use rusty_music_editor::Variante;
    [
        Variante::Standard,
        Variante::SixStems,
        Variante::Affinee,
        Variante::RoformerSw,
    ]
    .into_iter()
    .map(|v| VarianteDispo {
        nom: v.nom(),
        presente: v.presente(),
        megaoctets: v.megaoctets(),
    })
    .collect()
}

/// Avancement du démixage. L'interface sonde, faute de rapport intermédiaire.
#[tauri::command(async)]
pub(crate) fn demix_state(etat: State<Etat>) -> Result<EtatDemix, String> {
    Ok(verrou(&etat.demix).clone())
}

/// Lecture ordinaire et lecture des stems s'excluent : une seule des deux
/// sonne. Cette fonction est le côté « le lecteur reprend la main » — à appeler
/// par toute commande qui fait sonner le lecteur du module 1.
///
/// Rend vrai si des stems jouaient. Dans ce cas `stems_play` avait mis le
/// lecteur en pause : à l'appelant de le relancer si sa commande ne le fait pas
/// déjà (`Player::play` et `previous` le font, `skip` non).
///
/// **Garantie posée côté moteur, pas seulement par l'interface.** L'interface
/// arrêtait bien les stems avant de lancer un morceau, mais ⏮ (`previous`),
/// ⏭ et les touches média ne passaient pas par là : le morceau d'origine
/// repartait par-dessus ses stems. Un verrou à la fois, jamais les deux — pas
/// d'inversion possible avec `stems_play`, qui ne tient pas `stems` en même
/// temps que `player`.
pub(crate) fn couper_stems(etat: &Etat) -> bool {
    verrou(&etat.stems).take().is_some()
}

/// Charge un jeu de stems, **en pause**, à leurs niveaux.
///
/// Remplace le multipiste précédent s'il y en avait un : deux jeux de stems à
/// la fois n'auraient aucun sens, et chacun tient une sortie audio.
///
/// **Né silencieux, avec ses niveaux.** Il sonnait dès le chargement, tous
/// stems à pleine échelle : un muet (la batterie, en pratique) ne s'appliquait
/// qu'une fois vitesses et niveaux envoyés par l'interface, et le jeu entier
/// s'entendait entre-temps — à chaque chargement, donc aussi à chaque
/// changement de hauteur. `niveaux` porte le solo et les coupures dès la
/// première trame, et l'interface donne l'ordre de sonner (`reprendre`) quand
/// tout est en place.
#[tauri::command(async)]
pub(crate) fn stems_play(
    etat: State<Etat>,
    stems: Vec<(String, String)>,
    niveaux: Option<Vec<f32>>,
) -> Result<Vec<String>, String> {
    let pistes: Vec<(String, PathBuf)> = stems
        .into_iter()
        .map(|(nom, chemin)| (nom, PathBuf::from(chemin)))
        .collect();
    let niveaux = niveaux.unwrap_or_default();
    let multi = rusty_music_player::Multipiste::preparer(&pistes, &niveaux).map_err(echec)?;
    let noms = multi.noms().to_vec();

    // Le lecteur du module 1 se tait : deux sorties audio superposées, ce
    // serait le morceau d'origine par-dessus ses propres stems. On lui reprend
    // au passage son volume : le curseur de la barre du bas commande la même
    // écoute, les stems ne doivent pas repartir à fond.
    {
        let p = verrou(&etat.player);
        multi.set_volume(p.volume());
        p.pause();
    }
    *verrou(&etat.stems) = Some(multi);
    tracing::info!(n = noms.len(), "stems chargés");
    Ok(noms)
}

/// Règle le niveau de chaque piste — c'est ce que font solo et coupure.
///
/// **Refuse un compte qui ne correspond pas** au jeu chargé plutôt que
/// d'appliquer ce qui tombe juste : les niveaux sont posés par rang, et un
/// décalage muterait le mauvais stem sans rien dire.
#[tauri::command(async)]
pub(crate) fn stems_gain(etat: State<Etat>, levels: Vec<f32>) -> Result<(), String> {
    let garde = verrou(&etat.stems);
    if let Some(m) = garde.as_ref() {
        if levels.len() != m.noms().len() {
            return Err(format!(
                "{} niveaux pour {} stems chargés",
                levels.len(),
                m.noms().len()
            ));
        }
        for (i, n) in levels.iter().enumerate() {
            m.regler(i, *n);
        }
    }
    Ok(())
}

/// Transport du multipiste : pause, reprise, déplacement.
#[tauri::command(async)]
pub(crate) fn stems_transport(
    etat: State<Etat>,
    action: String,
    position: Option<f64>,
) -> Result<(), String> {
    let garde = verrou(&etat.stems);
    let Some(m) = garde.as_ref() else {
        return Ok(());
    };
    match action.as_str() {
        "pause" => m.pause(),
        "reprendre" => m.reprendre(),
        "deplacer" => m.deplacer(std::time::Duration::from_secs_f64(position.unwrap_or(0.0))),
        // Réaligner ne touche pas aux vitesses : on garde l'effet, on efface
        // l'écart déjà pris. Les deux gestes sont distincts, et il faut les
        // deux — égaliser les vitesses arrête la dérive mais laisse le décalage.
        "realigner" => m.realigner(),
        // Ôter la boucle — la poser passe par `stems_boucle`, qui a deux
        // bornes à recevoir.
        "sans_boucle" => m.boucler(None),
        "arreter" => {
            drop(garde);
            *verrou(&etat.stems) = None;
        }
        _ => return Err(format!("action inconnue : {action}")),
    }
    Ok(())
}

/// Pose une boucle A-B sur les stems en écoute, bornes en secondes du
/// morceau d'origine. L'aimantation sur les mesures est faite par l'interface,
/// qui tient la pulsation : le moteur boucle où on lui dit.
#[tauri::command(async)]
pub(crate) fn stems_boucle(etat: State<Etat>, debut: f64, fin: f64) -> Result<(), String> {
    if !(debut.is_finite() && fin.is_finite() && fin > debut && debut >= 0.0) {
        return Err(format!("bornes de boucle invalides : {debut} – {fin}"));
    }
    if let Some(m) = verrou(&etat.stems).as_ref() {
        m.boucler(Some((
            std::time::Duration::from_secs_f64(debut),
            std::time::Duration::from_secs_f64(fin),
        )));
    }
    Ok(())
}

/// La pulsation d'un morceau, telle que l'interface la consomme.
#[derive(serde::Serialize)]
pub(crate) struct VuePulsation {
    /// Chaque temps, en secondes du fichier d'origine.
    temps: Vec<f32>,
    /// Les premiers temps de mesure, en secondes.
    premiers_temps: Vec<f32>,
    bpm: Option<f32>,
    /// Temps par mesure.
    metrique: Option<u32>,
}

/// Temps et premiers temps de mesure d'un morceau (Beat This!).
///
/// En cache à côté de ses stems (`pulsation.json`) : quelques secondes de
/// calcul la première fois, rien ensuite. Les poids (83 Mo) se téléchargent
/// au premier usage, empreintes vérifiées, comme les variantes de démixage.
/// Le pisteur reste chargé une fois qu'il l'a été.
#[tauri::command(async)]
pub(crate) fn pulsation(etat: State<Etat>, id: i64) -> Result<VuePulsation, String> {
    use rusty_music_editor::pulsation::{Modele, FICHIER_CACHE};

    let chemin = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .map(|t| t.path)
        .ok_or_else(|| format!("morceau inconnu : {id}"))?;
    let source = PathBuf::from(&chemin);
    let dossier = dossier_stems(&etat, &source);
    std::fs::create_dir_all(&dossier).map_err(echec)?;
    let cache = dossier.join(FICHIER_CACHE);
    let modele = Modele::Complet;

    let p = pulsation_de(&etat, &source, &cache, modele)?;
    Ok(VuePulsation {
        bpm: p.bpm(),
        metrique: p.metrique(),
        temps: p.temps,
        premiers_temps: p.premiers_temps,
    })
}

/// La pulsation de `source`, depuis le cache ou calculée (et mise en cache).
pub(crate) fn pulsation_de(
    etat: &State<Etat>,
    source: &Path,
    cache: &Path,
    modele: rusty_music_editor::pulsation::Modele,
) -> Result<rusty_music_editor::pulsation::Pulsation, String> {
    use rusty_music_editor::pulsation::{Pisteur, Pulsation};
    let p = match Pulsation::lire(cache, modele) {
        Some(p) => p,
        None => {
            if !modele.present() {
                tracing::info!("pulsation : téléchargement des poids Beat This!");
                modele.telecharger(|_, _| {}).map_err(echec)?;
            }
            let mut garde = verrou(&etat.pisteur);
            if garde.is_none() {
                *garde = Some(Pisteur::charger(modele).map_err(echec)?);
            }
            let debut = std::time::Instant::now();
            let p = garde
                .as_mut()
                .expect("chargé juste au-dessus")
                .analyser_avec_cache(source, cache)
                .map_err(echec)?;
            tracing::info!(
                ms = debut.elapsed().as_millis() as u64,
                temps = p.temps.len(),
                "pulsation calculée"
            );
            p
        }
    };
    Ok(p)
}

/// Sections d'un morceau, telles que l'interface les consomme.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct VueSections {
    version: u32,
    pub(crate) sections: Vec<rusty_music_transcription::sections::Section>,
}

/// Version du cache des sections : à monter dès que le calcul change.
pub(crate) const VERSION_SECTIONS: u32 = 1;

/// Vrai si les sections peuvent se calculer : les 8 plis d'all-in-one sont
/// là, ou viennent d'être téléchargés (10 Mo, MIT, depuis
/// huggingface.co/rousseau/allinone-harmonix). Hors ligne et sans les plis :
/// faux, le bandeau ne s'affiche pas.
#[tauri::command(async)]
pub(crate) fn sections_disponibles() -> bool {
    use rusty_music_transcription::sections;
    sections::installes().is_some()
        || sections::telecharger(|_, _| {})
            .map_err(|e| tracing::warn!("sections : poids d'all-in-one non téléchargés : {e}"))
            .is_ok()
}

/// Sections d'un morceau démixé (intro, couplet, refrain…) par all-in-one,
/// sur ses stems HTDemucs : six secondes environ, en cache à côté des
/// stems. Avec six stems, guitare et piano rejoignent « other », comme le
/// modèle à quatre stems sur lequel all-in-one a appris.
#[tauri::command(async)]
pub(crate) fn sections(etat: State<Etat>, id: i64) -> Result<VueSections, String> {
    sections_de(&etat, id)
}

pub(crate) fn sections_de(etat: &State<Etat>, id: i64) -> Result<VueSections, String> {
    use rusty_music_transcription::sections::Sections;
    let chemin = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .map(|t| t.path)
        .ok_or_else(|| format!("morceau inconnu : {id}"))?;
    let dossier = dossier_stems(etat, Path::new(&chemin));
    let cache = dossier.join("sections.json");
    if let Some(v) = std::fs::read(&cache)
        .ok()
        .and_then(|o| serde_json::from_slice::<VueSections>(&o).ok())
        .filter(|v| v.version == VERSION_SECTIONS)
    {
        return Ok(v);
    }
    let stems = stems_du_dossier(&dossier);
    if stems.is_empty() {
        return Err("morceau pas encore séparé".into());
    }
    let debut = std::time::Instant::now();
    // bass, drums, other, vocals ; tout stem d'un autre nom va dans « other ».
    let mut quatre: [(Vec<f32>, Vec<f32>); 4] = Default::default();
    for (nom, wav) in &stems {
        let i = match nom.as_str() {
            "bass" => 0,
            "drums" => 1,
            "vocals" => 3,
            _ => 2,
        };
        let s = rusty_music_editor::decode::stereo(wav).map_err(echec)?;
        let (g, d) = &mut quatre[i];
        if g.len() < s.gauche.len() {
            g.resize(s.gauche.len(), 0.0);
            d.resize(s.droite.len(), 0.0);
        }
        g.iter_mut().zip(&s.gauche).for_each(|(a, b)| *a += b);
        d.iter_mut().zip(&s.droite).for_each(|(a, b)| *a += b);
    }
    // Un stem absent reste silencieux, à la longueur des autres.
    let n = quatre.iter().map(|(g, _)| g.len()).max().unwrap_or(0);
    for (g, d) in &mut quatre {
        g.resize(n, 0.0);
        d.resize(n, 0.0);
    }
    let mut garde = verrou(&etat.sections);
    if garde.valeur.is_none() {
        garde.valeur = Some(Sections::charger_installes(&Default::default()).map_err(echec)?);
    }
    let (sections, _) = garde
        .valeur
        .as_ref()
        .expect("chargé juste au-dessus")
        .analyser(std::array::from_fn(|i| {
            (quatre[i].0.as_slice(), quatre[i].1.as_slice())
        }));
    garde.touche = Some(Instant::now());
    drop(garde);
    tracing::info!(
        ms = debut.elapsed().as_millis() as u64,
        sections = sections.len(),
        "sections"
    );
    let vue = VueSections {
        version: VERSION_SECTIONS,
        sections,
    };
    if let Ok(o) = serde_json::to_vec(&vue) {
        if let Err(e) = std::fs::write(&cache, o) {
            tracing::warn!("sections : cache non écrit ({}) : {e}", cache.display());
        }
    }
    Ok(vue)
}

/// État du multipiste, sondé par l'interface pour animer sa barre.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct EtatStems {
    /// Un multipiste est chargé et n'est pas au bout. Arrivé au bout, il se
    /// remet en pause au début (`arreter_au_bout`) : `actif` et `en_pause`
    /// tous deux faux ne veulent dire que « rien de chargé ».
    actif: bool,
    en_pause: bool,
    position_ms: u64,
    duree_ms: u64,
    /// Noms des stems chargés, dans l'ordre des niveaux : de quoi vérifier que
    /// l'interface et le moteur parlent du même jeu.
    noms: Vec<String>,
    niveaux: Vec<f32>,
    /// Vitesse de chaque stem. L'interface les tient déjà, mais c'est le
    /// moteur qui les borne : les relire évite d'afficher une valeur que la
    /// lecture n'applique pas.
    vitesses: Vec<f32>,
    /// De combien le stem le plus éloigné s'est écarté de la référence.
    /// **Mesuré, pas prédit** — « les stems ont dérivé de 1,4 s » se vérifie à
    /// l'oreille, « ils peuvent se désynchroniser » ne dit rien.
    derive_ms: u64,
    /// Boucle A-B posée, en secondes de référence.
    boucle: Option<(f64, f64)>,
    /// Retours au début de boucle depuis qu'elle est posée — l'interface
    /// s'en sert pour accélérer d'un cran à chaque passage.
    passages: u32,
    /// Latence de la sortie audio : ce que l'on entend a ce retard sur
    /// `position_ms` (plus de 200 ms en Bluetooth). L'interface la retranche
    /// pour que le curseur de la partition suive l'oreille.
    latence_ms: u64,
}

#[tauri::command(async)]
pub(crate) fn stems_state(etat: State<Etat>) -> Result<EtatStems, String> {
    let etat_stems = {
        let garde = verrou(&etat.stems);
        // Le sondage fait aussi la fin de morceau : arrivés au bout, les
        // stems restent chargés, en pause au début (`arreter_au_bout`).
        if let Some(m) = garde.as_ref() {
            m.arreter_au_bout();
        }
        match garde.as_ref() {
            Some(m) => EtatStems {
                actif: !m.fini(),
                en_pause: m.en_pause(),
                position_ms: m.position().as_millis() as u64,
                duree_ms: m.duree().as_millis() as u64,
                noms: m.noms().to_vec(),
                niveaux: m.niveaux(),
                vitesses: m.vitesses(),
                derive_ms: m.derive().as_millis() as u64,
                boucle: m.boucle().map(|(a, b)| (a.as_secs_f64(), b.as_secs_f64())),
                passages: m.passages(),
                latence_ms: rusty_music_player::latence_sortie().as_millis() as u64,
            },
            None => EtatStems::default(),
        }
    };
    // Filet de sécurité de l'exclusion (voir `couper_stems`) : des stems qui
    // sonnent et le morceau d'origine aussi, c'est le défaut qu'on ne veut plus
    // entendre, quelle qu'en soit la cause. Le sondage y veille à chaque
    // battement. `try_lock` : le lecteur peut être tenu par un préchargement,
    // le sondage n'a pas à attendre — le prochain battement reviendra.
    if etat_stems.actif && !etat_stems.en_pause {
        if let Ok(p) = etat.player.try_lock() {
            if !p.is_paused() {
                p.pause();
                tracing::warn!("morceau d'origine relancé pendant les stems : remis en pause");
            }
        }
    }
    Ok(etat_stems)
}

/// Spectrogramme d'un stem, en intensités sur un octet.
///
/// La coloration appartient à l'interface : elle applique la rampe
/// séquentielle déjà retenue pour la carte, plutôt qu'une palette importée.
#[tauri::command(async)]
pub(crate) fn stem_spectre(
    path: String,
    width: usize,
    height: usize,
) -> Result<SpectreVue, String> {
    let s =
        rusty_music_player::spectre::calculer(Path::new(&path), width, height).map_err(echec)?;
    Ok(SpectreVue {
        largeur: s.largeur,
        hauteur: s.hauteur,
        pixels: s.pixels,
    })
}

#[derive(serde::Serialize)]
pub(crate) struct SpectreVue {
    largeur: usize,
    hauteur: usize,
    pixels: Vec<u8>,
}

/// Spectrogramme du **son réellement joué** pour le morceau à `path` :
///
/// - version HD du cache si la lecture HD est active et qu'elle existe ;
/// - sinon le fichier d'origine.
///
/// Quand ce qui joue diffère de l'original (HD), rend **aussi** le
/// spectrogramme de l'original (`pixels_ref`) : l'interface peut alors teinter
/// ce que le HD a ajouté. Rend la coupure estimée de la source, pour tracer le
/// trait « au-dessus, c'est reconstruit ».
#[tauri::command(async)]
pub(crate) fn spectre_transport(
    etat: State<Etat>,
    path: String,
    width: usize,
    height: usize,
) -> Result<SpectreTransport, String> {
    let origine = PathBuf::from(&path);
    let joue = rusty_music_superres::resoudre(&etat.hd, &origine);
    let hd = joue != origine;
    let e = rusty_music_player::amelioration().actif();

    // Le son joué : version HD du cache, sinon l'original ; puis l'excitateur
    // « E » par-dessus s'il est actif (il n'existe qu'en mémoire).
    let s = if e {
        rusty_music_player::spectre_ameliore(&joue, width, height).map_err(echec)?
    } else {
        rusty_music_player::spectre::calculer(&joue, width, height).map_err(echec)?
    };

    // Référence à teinter : l'original brut, dès que le son joué en diffère.
    let modifie = hd || e;
    let pixels_ref = if modifie {
        rusty_music_player::spectre::calculer(&origine, width, height)
            .ok()
            .filter(|r| r.pixels.len() == s.pixels.len())
            .map(|r| r.pixels)
    } else {
        None
    };
    Ok(SpectreTransport {
        largeur: s.largeur,
        hauteur: s.hauteur,
        pixels: s.pixels,
        pixels_ref,
        hd: modifie,
    })
}

#[derive(serde::Serialize)]
pub(crate) struct SpectreTransport {
    largeur: usize,
    hauteur: usize,
    pixels: Vec<u8>,
    pixels_ref: Option<Vec<u8>>,
    hd: bool,
}

/// Les WAV de stems d'un dossier, avec le nom de leur stem.
///
/// Le nom est ce qui suit le dernier tiret cadratin : `Die Oros — drums.wav`
/// donne `drums`. **Le premier niveau seulement** — `traites/` porte les
/// versions transposées et `greffes/` les stems venus d'ailleurs, qui sont des
/// dérivés du morceau, pas des stems de plus.
pub(crate) fn stems_du_dossier(dossier: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return Vec::new();
    };
    let mut trouves: Vec<(String, PathBuf)> = entrees
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .map(|p| {
            let nom = p
                .file_stem()
                .map(|s| {
                    s.to_string_lossy()
                        .rsplit('—')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();
            (nom, p)
        })
        .collect();
    trouves.sort();
    trouves
}

/// Les stems déjà calculés pour un morceau, s'il y en a.
///
/// Permet de retrouver un démixage d'une session précédente sans le refaire —
/// une séparation coûte trente secondes, on ne la rejoue pas pour rien.
#[tauri::command(async)]
pub(crate) fn stems_existants(
    etat: State<Etat>,
    path: String,
) -> Result<Vec<(String, String)>, String> {
    let dossier = dossier_stems(&etat, Path::new(&path));
    Ok(stems_du_dossier(&dossier)
        .into_iter()
        .map(|(nom, p)| (nom, p.display().to_string()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    /// Un rendu ne doit jamais atterrir sous une racine surveillée : il y
    /// serait ingéré, analysé et placé sur la carte alors que ce n'est pas un
    /// morceau.
    #[test]
    fn un_rendu_ne_sort_pas_dans_la_bibliotheque() {
        let racines = vec![
            "/Volumes/CarteSD/Music".to_string(),
            "/Users/x/Musique".to_string(),
        ];
        let sous = |p: &str| sous_une_racine(Path::new(p), &racines);

        assert!(
            sous("/Volumes/CarteSD/Music").is_some(),
            "la racine elle-même"
        );
        assert!(
            sous("/Volumes/CarteSD/Music/rendus").is_some(),
            "un sous-dossier"
        );
        assert!(sous("/Users/x/Musique/a/b").is_some());

        assert!(sous("/Users/x/Bureau").is_none());
        assert!(sous("/Volumes/CarteSD").is_none(), "au-dessus de la racine");
        // Le piège d'une comparaison de chaînes : ce dossier-là est ailleurs.
        assert!(
            sous("/Users/x/Musique2").is_none(),
            "préfixe textuel trompeur"
        );
    }
}
