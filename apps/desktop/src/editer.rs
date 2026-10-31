// SPDX-License-Identifier: GPL-3.0-or-later
//! Commandes d'Éditer : démixage et stems, vitesse et hauteur, export,
//! pulsation, transcription, sections, greffe.

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

/// Lance les stems après un décompte de métronome (Pratiquer) : la musique
/// part de `debut`, en secondes du morceau, et `clics` donne chaque clic
/// (instant en secondes du morceau, premier temps ou non). L'interface les
/// place sur la pulsation ; le moteur les joue à l'échantillon près.
#[tauri::command(async)]
pub(crate) fn stems_decompter(
    etat: State<Etat>,
    debut: f64,
    clics: Vec<(f64, bool)>,
) -> Result<(), String> {
    if !debut.is_finite() || debut < 0.0 || clics.iter().any(|c| !c.0.is_finite()) {
        return Err(format!("décompte invalide : début {debut}"));
    }
    if let Some(m) = verrou(&etat.stems).as_ref() {
        m.decompter(std::time::Duration::from_secs_f64(debut), &clics);
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

/// Une transcription, telle que l'interface la consomme : les notes (pour
/// les corrections à venir) et les mesures quantifiées (pour la partition).
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct VueTranscription {
    version: u32,
    instrument: String,
    modele: String,
    notes: Vec<rusty_music_transcription::Note>,
    /// Les coups de batterie, pour la batterie.
    #[serde(default)]
    coups: Vec<rusty_music_transcription::batterie::Coup>,
    mesures: serde_json::Value,
    /// Accordage de la basse, de la corde grave à l'aiguë, en hauteurs MIDI
    /// (standard, drop D ou cinq cordes : `Accordage::pour`).
    #[serde(default)]
    accordage: Vec<u8>,
    /// Version du doigté de la basse ([`VERSION_DOIGTES`]).
    #[serde(default)]
    doigtes: u32,
    /// La basse du réglage (4 ou 5 cordes) sur laquelle le doigté a été posé.
    #[serde(default)]
    basses: tablature::Basses,
    /// Hauteurs transcrites, avant que le doigté ne ramène à l'octave ce qui
    /// sort du manche : changer de basse repart d'elles (une 4 cordes
    /// remonte le mi♭ grave qu'une 5 cordes joue).
    #[serde(default)]
    hauteurs: Vec<u8>,
    /// Version de la grille des mesures ([`VERSION_MESURES`]).
    #[serde(default)]
    grille: u32,
}

/// Version du cache de transcription : à monter dès que le calcul change.
pub(crate) const VERSION_TRANSCRIPTION: u32 = 5;

/// Version du doigté (`tablature::Couts`) : quand seul le doigté change, les
/// notes en cache sont reposées sans retranscrire (MuScriptor prend des
/// minutes). 1 : avant l'a priori appris ; 2 : a priori de position et coûts
/// réglés sur 89 tablatures (17 oct.) ; 3 : table apprise sur 331 tablatures ;
/// 4 : un motif, un doigté (`tablature::uniformiser`, 29 oct.) ; 5 : 4 ou
/// 5 cordes, toujours dans l'accordage du disque.
pub(crate) const VERSION_DOIGTES: u32 = 5;

/// Pose corde et frette sur les notes, pour les basses du réglage, puis donne
/// un seul doigté à chaque motif répété.
pub(crate) fn doigter(
    notes: &mut [rusty_music_transcription::Note],
    basses: tablature::Basses,
    premiers: &[f32],
) -> tablature::Accordage {
    let accordage = tablature::Accordage::pour(notes, basses);
    tablature::poser(notes, &accordage);
    tablature::uniformiser(notes, &accordage, &tablature::Couts::default(), premiers);
    accordage
}

/// Version de la grille des mesures (`Pulsation::regularisee`) : quand seule
/// elle change, les notes et coups en cache sont requantifiés sans
/// retranscrire. 1 : premiers temps parasites retirés (26 oct.) ; 2 :
/// anticipations coupées au premier temps recousues (29 oct.).
pub(crate) const VERSION_MESURES: u32 = 2;

/// Avancement d'une transcription MuScriptor, sondé par l'interface.
#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct EtatTranscription {
    pub(crate) en_cours: bool,
    /// Le morceau transcrit, pour que l'interface sache si l'avancement est
    /// le sien.
    id: i64,
    faits: usize,
    total: usize,
    /// Téléchargement des poids de MuScriptor en cours, et son avancement.
    telechargement: bool,
    octets_faits: u64,
    octets_total: u64,
}

#[tauri::command]
pub(crate) fn etat_transcription(etat: State<Etat>) -> EtatTranscription {
    verrou(&etat.transcription).clone()
}

/// Les partitions déjà calculées du morceau, sans rien calculer : « bass »,
/// « bass-muscriptor », « drums » selon les caches présents et à jour. Le
/// bloc « Partitions » du rail montre ainsi « prête » plutôt que « Calculer ».
#[tauri::command(async)]
pub(crate) fn transcriptions_en_cache(etat: State<Etat>, id: i64) -> Result<Vec<String>, String> {
    let chemin = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .map(|t| t.path)
        .ok_or_else(|| format!("morceau inconnu : {id}"))?;
    let dossier = dossier_stems(&etat, Path::new(&chemin));
    Ok(["bass", "bass-muscriptor", "drums"]
        .into_iter()
        .filter(|cle| {
            std::fs::read(dossier.join(format!("transcription-{cle}.json")))
                .ok()
                .and_then(|o| serde_json::from_slice::<VueTranscription>(&o).ok())
                .is_some_and(|t| t.version == VERSION_TRANSCRIPTION)
        })
        .map(String::from)
        .collect())
}

/// Vrai si les poids de MuScriptor sont sur la machine (dossier des modèles
/// ou cache Hugging Face) : l'utilisateur doit les télécharger lui-même,
/// après avoir accepté leurs conditions d'usage sur Hugging Face.
#[tauri::command]
pub(crate) fn muscriptor_disponible() -> bool {
    rusty_music_transcription::muscriptor::poids().is_some()
}

/// Vrai si un jeton Hugging Face est déjà posé sur la machine (`HF_TOKEN`
/// ou `hf auth login`) : l'interface ne le demande alors pas.
#[tauri::command]
pub(crate) fn muscriptor_jeton_present() -> bool {
    rusty_music_transcription::muscriptor::jeton_hugging_face().is_some()
}

/// Télécharge les poids de MuScriptor (1,2 Go) dans le dossier des modèles,
/// avec le jeton saisi, sinon celui de la machine. Le jeton n'est envoyé qu'à
/// Hugging Face et n'est conservé nulle part.
#[tauri::command(async)]
pub(crate) fn telecharger_muscriptor(
    etat: State<Etat>,
    jeton: Option<String>,
) -> Result<(), String> {
    use rusty_music_transcription::muscriptor;
    let jeton = jeton
        .map(|j| j.trim().to_string())
        .filter(|j| !j.is_empty())
        .or_else(muscriptor::jeton_hugging_face);
    {
        let mut t = verrou(&etat.transcription);
        if t.telechargement {
            return Err("téléchargement déjà en cours".into());
        }
        (t.telechargement, t.octets_faits, t.octets_total) = (true, 0, muscriptor::OCTETS);
    }
    let issue = muscriptor::telecharger(jeton.as_deref(), |vus, total| {
        let mut t = verrou(&etat.transcription);
        (t.octets_faits, t.octets_total) = (vus, total.unwrap_or(muscriptor::OCTETS));
    });
    verrou(&etat.transcription).telechargement = false;
    issue.map(|_| ()).map_err(echec)
}

/// Transcrit le stem `instrument` d'un morceau (`crates/transcription`) :
/// - « bass » : Basic Pitch, ou MuScriptor si `qualite` (une minute par
///   morceau, plus juste — banc `experiments/partitions/`), puis une note à
///   la fois, corde et frette ;
/// - « drums » : ADTOF, cinq pièces.
/// Puis quantification sur la pulsation. En cache à côté des stems ;
/// quelques secondes la première fois.
#[tauri::command(async)]
pub(crate) fn transcrire(
    etat: State<Etat>,
    id: i64,
    instrument: String,
    qualite: Option<bool>,
    basses: Option<tablature::Basses>,
) -> Result<VueTranscription, String> {
    use rusty_music_transcription::{
        basic_pitch, batterie, monophonie, muscriptor, porte, quantification,
    };
    let basses = basses.unwrap_or_default();
    if instrument != "bass" && instrument != "drums" {
        return Err(format!(
            "transcription de « {instrument} » pas encore disponible"
        ));
    }
    let qualite = qualite.unwrap_or(false) && instrument == "bass";
    let chemin = verrou(&etat.lib)
        .track(id)
        .map_err(echec)?
        .map(|t| t.path)
        .ok_or_else(|| format!("morceau inconnu : {id}"))?;
    let source = PathBuf::from(&chemin);
    let dossier = dossier_stems(&etat, &source);
    let cache = dossier.join(if qualite {
        format!("transcription-{instrument}-muscriptor.json")
    } else {
        format!("transcription-{instrument}.json")
    });
    if let Some(mut t) = std::fs::read(&cache)
        .ok()
        .and_then(|o| serde_json::from_slice::<VueTranscription>(&o).ok())
        .filter(|t| t.version == VERSION_TRANSCRIPTION)
    {
        let doigtes_a_jour =
            instrument != "bass" || (t.doigtes == VERSION_DOIGTES && t.basses == basses);
        if doigtes_a_jour && t.grille == VERSION_MESURES {
            return Ok(t);
        }
        // Seuls le doigté ou la grille des mesures ont changé : les notes en
        // cache sont reposées et requantifiées, sans retranscrire.
        let pulsation = pulsation_de(
            &etat,
            &source,
            &dossier.join(rusty_music_editor::pulsation::FICHIER_CACHE),
            rusty_music_editor::pulsation::Modele::Complet,
        )?;
        let metrique = pulsation.metrique().unwrap_or(4);
        t.mesures = if instrument == "bass" {
            if !doigtes_a_jour {
                // Un cache d'avant le champ : ses hauteurs ont été ramenées
                // sur l'accordage du disque, qui les contenait toutes.
                if t.hauteurs.len() != t.notes.len() {
                    t.hauteurs = t.notes.iter().map(|n| n.hauteur).collect();
                }
                for (n, &h) in t.notes.iter_mut().zip(&t.hauteurs) {
                    n.hauteur = h;
                }
                let accordage = doigter(&mut t.notes, basses, &pulsation.premiers_temps);
                t.accordage = accordage.cordes;
                t.doigtes = VERSION_DOIGTES;
                t.basses = basses;
            }
            let mesures = quantification::quantifier(
                &t.notes,
                &pulsation.temps,
                &pulsation.premiers_temps,
                metrique,
            );
            serde_json::to_value(&mesures).map_err(echec)?
        } else {
            let mesures = quantification::quantifier_coups(
                &t.coups,
                &pulsation.temps,
                &pulsation.premiers_temps,
                metrique,
            );
            serde_json::to_value(&mesures).map_err(echec)?
        };
        t.grille = VERSION_MESURES;
        if let Ok(o) = serde_json::to_vec(&t) {
            if let Err(e) = std::fs::write(&cache, o) {
                tracing::warn!(
                    "transcription : cache non écrit ({}) : {e}",
                    cache.display()
                );
            }
        }
        tracing::info!(
            notes = t.notes.len(),
            coups = t.coups.len(),
            "partition reposée sur la grille"
        );
        return Ok(t);
    }
    let (_, stem) = stems_du_dossier(&dossier)
        .into_iter()
        .find(|(n, _)| *n == instrument)
        .ok_or_else(|| format!("pas de stem « {instrument} » séparé pour ce morceau"))?;
    let pulsation = pulsation_de(
        &etat,
        &source,
        &dossier.join(rusty_music_editor::pulsation::FICHIER_CACHE),
        rusty_music_editor::pulsation::Modele::Complet,
    )?;

    let debut = std::time::Instant::now();
    let s = rusty_music_editor::decode::stereo(&stem).map_err(echec)?;
    if instrument == "drums" {
        let (carac, trames, bandes) = batterie::caracteristiques(&s.gauche, &s.droite);
        drop(s);
        let activations = {
            let mut garde = verrou(&etat.batteur);
            if garde.is_none() {
                *garde = Some(batterie::Batteur::charger_installe().map_err(echec)?);
            }
            garde
                .as_mut()
                .expect("chargé juste au-dessus")
                .activations(&carac, trames, bandes)
                .map_err(echec)?
        };
        let coups = batterie::coups(&activations, trames);
        let mesures = quantification::quantifier_coups(
            &coups,
            &pulsation.temps,
            &pulsation.premiers_temps,
            pulsation.metrique().unwrap_or(4),
        );
        tracing::info!(
            ms = debut.elapsed().as_millis() as u64,
            coups = coups.len(),
            mesures = mesures.len(),
            "transcription batterie"
        );
        let vue = VueTranscription {
            version: VERSION_TRANSCRIPTION,
            instrument,
            modele: "adtof-frame-rnn".into(),
            notes: Vec::new(),
            coups,
            mesures: serde_json::to_value(&mesures).map_err(echec)?,
            accordage: Vec::new(),
            doigtes: 0,
            basses: tablature::Basses::default(),
            hauteurs: Vec::new(),
            grille: VERSION_MESURES,
        };
        if let Ok(o) = serde_json::to_vec(&vue) {
            if let Err(e) = std::fs::write(&cache, o) {
                tracing::warn!(
                    "transcription : cache non écrit ({}) : {e}",
                    cache.display()
                );
            }
        }
        return Ok(vue);
    }
    let mono: Vec<f32> = s
        .gauche
        .iter()
        .zip(&s.droite)
        .map(|(g, d)| 0.5 * (g + d))
        .collect();
    drop(s);
    let brutes = if qualite {
        let poids = muscriptor::poids().ok_or_else(|| {
            "poids de MuScriptor absents : accepter leurs conditions sur huggingface.co/MuScriptor/muscriptor-medium, \
             puis les télécharger depuis le choix « Qualité »"
                .to_string()
        })?;
        {
            let mut t = verrou(&etat.transcription);
            (t.en_cours, t.id, t.faits, t.total) = (true, id, 0, 0);
        }
        // Le bilan n'arrive qu'à la fin (une minute) : sans cette ligne, une
        // transcription interrompue ne laissait aucune trace au journal.
        tracing::info!(id, "MuScriptor : transcription de la basse lancée");
        let issue = (|| -> Result<Vec<rusty_music_transcription::Note>, String> {
            let mut garde = verrou(&etat.muscriptor);
            if garde.valeur.is_none() {
                garde.valeur = Some(
                    muscriptor::Muscriptor::charger(&poids, &muscriptor::appareil())
                        .map_err(echec)?,
                );
            }
            let m = garde.valeur.as_ref().expect("chargé juste au-dessus");
            let (notes, bilan) = m
                .transcrire(
                    &mono,
                    rusty_music_editor::SR,
                    muscriptor::BASSES,
                    muscriptor::LOT,
                    |faits, total| {
                        let mut t = verrou(&etat.transcription);
                        (t.faits, t.total) = (faits, total);
                    },
                )
                .map_err(echec)?;
            garde.touche = Some(Instant::now());
            tracing::info!(segments = bilan.segments, jetons = bilan.jetons, boucles = ?bilan.boucles, "MuScriptor");
            Ok(notes)
        })();
        verrou(&etat.transcription).en_cours = false;
        issue?
    } else {
        basic_pitch::POIDS.telecharger(|_, _| {}).map_err(echec)?;
        let mut garde = verrou(&etat.transcripteur);
        if garde.is_none() {
            *garde = Some(basic_pitch::Transcripteur::charger_installe().map_err(echec)?);
        }
        let activations = garde
            .as_mut()
            .expect("chargé juste au-dessus")
            .activations(&mono, rusty_music_editor::SR)
            .map_err(echec)?;
        basic_pitch::notes(&activations, &basic_pitch::Reglages::basse())
    };
    // La porte retire ce que le stem ne joue pas vraiment (fuites d'autres
    // instruments, 60 dB sous la basse) avant de choisir les notes.
    let brutes = porte::filtrer(brutes, &mono, rusty_music_editor::SR, porte::SEUIL_DB);
    let mut notes = monophonie::monophonique(brutes);
    let hauteurs = notes.iter().map(|n| n.hauteur).collect();
    let accordage = doigter(&mut notes, basses, &pulsation.premiers_temps);
    let mesures = quantification::quantifier(
        &notes,
        &pulsation.temps,
        &pulsation.premiers_temps,
        pulsation.metrique().unwrap_or(4),
    );
    tracing::info!(
        ms = debut.elapsed().as_millis() as u64,
        notes = notes.len(),
        mesures = mesures.len(),
        "transcription"
    );
    let vue = VueTranscription {
        version: VERSION_TRANSCRIPTION,
        instrument,
        modele: if qualite {
            "muscriptor-medium"
        } else {
            "basic-pitch"
        }
        .into(),
        notes,
        coups: Vec::new(),
        mesures: serde_json::to_value(&mesures).map_err(echec)?,
        accordage: accordage.cordes,
        doigtes: VERSION_DOIGTES,
        basses,
        hauteurs,
        grille: VERSION_MESURES,
    };
    if let Ok(o) = serde_json::to_vec(&vue) {
        if let Err(e) = std::fs::write(&cache, o) {
            tracing::warn!(
                "transcription : cache non écrit ({}) : {e}",
                cache.display()
            );
        }
    }
    Ok(vue)
}

/// Sections d'un morceau, telles que l'interface les consomme.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct VueSections {
    version: u32,
    sections: Vec<rusty_music_transcription::sections::Section>,
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

// ---------------------------------------------------------------------------
// Mode Éditer : greffer le stem d'un autre morceau
// ---------------------------------------------------------------------------

/// Écart de tempo au-delà duquel un morceau n'est pas proposé pour une greffe.
///
/// **Dix pour cent, et le repliement est déjà passé** : un morceau à demi-tempo
/// n'est pas écarté, il est compté pour ce qu'il est. Ce qui reste est le vrai
/// étirement à appliquer, et `wsola` y est transparent. Plus loin, deux choses
/// se dégradent ensemble — la matière étirée, et la confiance qu'on peut avoir
/// dans un tempo mesuré à 6 % près (`docs/suite.md`, 6 bis).
pub(crate) const TOLERANCE_TEMPO: f32 = 0.10;

/// Combien de voisins soniques on examine avant de filtrer sur le tempo.
///
/// Le filtre en écarte beaucoup : sur une bibliothèque réelle, quelques
/// dizaines de voisins ne suffisent pas à en garder une poignée.
pub(crate) const VOISINS_EXAMINES: usize = 400;

/// Un morceau dont on pourrait prendre un stem.
#[derive(serde::Serialize)]
pub(crate) struct Candidat {
    id: i64,
    /// Le fichier, pour pouvoir le séparer s'il ne l'est pas encore.
    path: String,
    title: Option<String>,
    artist: Option<String>,
    bpm: f32,
    /// Facteur de durée qui l'amènerait au tempo du morceau ouvert.
    facteur: f32,
    /// Repliements d'octave : +1 = joué à double tempo, −1 = à demi-tempo.
    octaves: i32,
    /// Ce que l'étirement lui fera subir, en pourcentage.
    ecart: f32,
    /// Ses stems sont-ils déjà séparés ? Sinon il faut compter trente secondes.
    separe: bool,
}

/// Ce que la recherche a trouvé, **et ce qu'elle a écarté**.
///
/// Les deux comptes ne sont pas décoratifs : sans eux, une liste courte passe
/// pour une bibliothèque pauvre en voisins alors que c'est le tempo, ou son
/// absence, qui a trié.
#[derive(serde::Serialize)]
pub(crate) struct VoisinsStem {
    /// Tempo du morceau ouvert. Sans lui, rien n'est calable.
    bpm: Option<f32>,
    candidats: Vec<Candidat>,
    /// Voisins écartés parce que leur tempo est trop loin.
    ecartes: usize,
    /// Voisins écartés faute de tempo mesuré.
    sans_tempo: usize,
}

/// Les morceaux d'où l'on pourrait tirer un stem à greffer.
///
/// **Le voisinage se calcule sur le morceau entier, pas sur le stem.** C'est
/// une limite assumée : la bibliothèque ne contient que des empreintes de
/// mélanges complets, et comparer une batterie seule à des mélanges
/// compare deux choses différentes. Embarquer une empreinte par stem
/// supposerait de démixer les 27 000 morceaux — trente secondes chacun.
///
/// Le tempo, lui, est une contrainte dure et non un classement : une batterie
/// qui ne tombe pas juste ne sert à rien, si proche soit-elle.
#[tauri::command(async)]
pub(crate) fn voisins_de_stem(
    etat: State<Etat>,
    id: i64,
    count: usize,
) -> Result<VoisinsStem, String> {
    let vecteurs = charger_vecteurs(&etat)?;
    let proches = rusty_music_analysis::chemin::voisins(&vecteurs, id, VOISINS_EXAMINES);
    let pistes = pistes_de(&etat, &proches)?;

    let (bpm, tempos) = {
        let lib = verrou(&etat.lib);
        let bpm = lib.tempos(&[id]).map_err(echec)?.get(&id).copied();
        (bpm, lib.tempos(&proches).map_err(echec)?)
    };

    let Some(bpm_source) = bpm else {
        // Rien à caler sans tempo : on le dit plutôt que de proposer une liste
        // dont aucune entrée ne tiendrait.
        return Ok(VoisinsStem {
            bpm: None,
            candidats: Vec::new(),
            ecartes: 0,
            sans_tempo: pistes.len(),
        });
    };

    let mut candidats = Vec::new();
    let (mut ecartes, mut sans_tempo) = (0usize, 0usize);
    for piste in pistes {
        let Some(bpm) = tempos.get(&piste.id).copied() else {
            sans_tempo += 1;
            continue;
        };
        let (facteur, octaves) = rusty_music_editor::greffe::tempo_replie(bpm_source, bpm);
        let ecart = (facteur - 1.0).abs();
        if ecart > TOLERANCE_TEMPO {
            ecartes += 1;
            continue;
        }
        if candidats.len() < count {
            let dossier = dossier_stems(&etat, Path::new(&piste.path));
            candidats.push(Candidat {
                id: piste.id,
                path: piste.path,
                title: piste.title,
                artist: piste.artist,
                bpm,
                facteur,
                octaves,
                ecart: (facteur - 1.0) * 100.0,
                separe: !stems_du_dossier(&dossier).is_empty(),
            });
        }
    }

    tracing::info!(
        retenus = candidats.len(),
        ecartes,
        sans_tempo,
        "voisins pour une greffe"
    );
    Ok(VoisinsStem {
        bpm: Some(bpm_source),
        candidats,
        ecartes,
        sans_tempo,
    })
}

/// Ce qu'il faut pour proposer un **BPM cible** dans la barre d'outils plutôt
/// qu'un pourcentage de vitesse.
#[derive(serde::Serialize)]
pub(crate) struct TempoCible {
    /// Tempo du morceau entier, tel que la passe de descripteurs l'a mesuré.
    /// C'est plus sûr qu'une mesure sur un seul stem, et c'est le nombre que
    /// l'utilisateur reconnaît. `None` : morceau non mesuré.
    bpm: Option<f32>,
    /// Netteté de la pulsation, mesurée sur le stem `drums` de l'établi
    /// (`battements::Grille`). **Au-delà de 2, la pulsation est franche** ;
    /// l'interface ne propose le BPM cible qu'à cette condition. `None` : pas
    /// de stem `drums`, ou il ne se décode pas.
    nettete: Option<f32>,
}

/// Le tempo du morceau ouvert et la netteté de sa pulsation.
///
/// Le BPM cible n'a de sens que sur une matière qui pulse : une nappe ou un
/// morceau rubato n'a pas de tempo à viser, et lui en proposer un donnerait
/// une précision qu'on n'a pas. L'interface s'en sert pour choisir entre le
/// réglage en BPM et le réglage en pourcentage (`docs/ui-spec-editeur.md`).
///
/// La netteté se mesure sur le stem `drums` — le meilleur signal de pulsation
/// une fois le morceau séparé — par la même grille que la greffe
/// (`grilles_des_stems`). Le tempo, lui, vient de la base : mesuré sur le
/// mélange entier, il est plus robuste qu'une batterie seule.
#[tauri::command(async)]
pub(crate) fn tempo_cible(etat: State<Etat>, id: i64) -> Result<TempoCible, String> {
    let (bpm, chemin) = {
        let lib = verrou(&etat.lib);
        let bpm = lib.tempos(&[id]).map_err(echec)?.get(&id).copied();
        let chemin = lib.track(id).map_err(echec)?.map(|t| t.path);
        (bpm, chemin)
    };

    let nettete = chemin.and_then(|p| {
        let dossier = dossier_stems(&etat, Path::new(&p));
        let (_, drums) = stems_du_dossier(&dossier)
            .into_iter()
            .find(|(n, _)| n == "drums")?;
        let s = rusty_music_editor::decode::stereo(&drums).ok()?;
        let mono: Vec<f32> = s
            .gauche
            .iter()
            .zip(&s.droite)
            .map(|(g, d)| (g + d) * 0.5)
            .collect();
        let analyseur = rusty_music_analysis::descripteurs::Analyseur::new();
        rusty_music_analysis::battements::grille_reechantillonnee(
            &mono,
            rusty_music_editor::SR,
            &analyseur,
        )
        .map(|g| g.nettete)
    });

    Ok(TempoCible { bpm, nettete })
}

/// Un stem greffé, et ce qu'il a fallu lui faire.
#[derive(serde::Serialize)]
pub(crate) struct Greffon {
    /// Le WAV écrit, à mettre à la place du stem remplacé.
    chemin: String,
    /// Le morceau d'où il vient, tel qu'on l'affiche.
    origine: String,
    facteur: f32,
    octaves: i32,
    retard_s: f32,
    boucles: usize,
    /// Le greffon est-il entré sur un battement ? L'interface le dit plutôt
    /// que de laisser l'oreille le découvrir.
    cale_aux_temps: bool,
    /// Greffe mesure par mesure : mesures posées, dont appariées par section
    /// (refrain sur refrain). 0 pour la greffe d'un bloc.
    mesures: usize,
    mesures_par_section: usize,
    /// Pourquoi on s'est rabattu sur la greffe d'un bloc, s'il y a lieu.
    repli: Option<String>,
}

/// Les grilles de battements des deux stems.
///
/// **C'est ici que le module 2 rejoint le module 3.** `crates/editor` ne dépend
/// pas de `rusty-music-analysis` — ce serait tirer CLAP, ses 117 Mo de poids et
/// la génération de code de son `build.rs` dans un crate qui n'a que faire d'un
/// modèle d'empreintes. L'application dépend des deux, c'est donc elle qui les
/// relie, en trois nombres.
pub(crate) fn grilles_des_stems(
    remplace: &Path,
    greffon: &Path,
) -> Option<(
    rusty_music_analysis::battements::Grille,
    rusty_music_analysis::battements::Grille,
)> {
    use rusty_music_analysis::battements;

    let analyseur = rusty_music_analysis::descripteurs::Analyseur::new();
    let une = |chemin: &Path| -> Option<battements::Grille> {
        let s = rusty_music_editor::decode::stereo(chemin).ok()?;
        // Somme des deux voies : une batterie panoramée à gauche ne doit pas
        // rendre une phase différente d'une batterie centrée.
        let mono: Vec<f32> = s
            .gauche
            .iter()
            .zip(&s.droite)
            .map(|(g, d)| (g + d) * 0.5)
            .collect();
        battements::grille_reechantillonnee(&mono, rusty_music_editor::SR, &analyseur)
    };
    Some((une(remplace)?, une(greffon)?))
}

/// Met à la place d'un stem celui d'un autre morceau.
///
/// **Non destructif, comme tout le module** : la greffe est un fichier de plus
/// dans le cache, sous `greffes/`, et le stem d'origine reste où il est. Rouvrir
/// le morceau retrouve ses quatre stems séparés, pas la greffe — les stems sur
/// le disque sont la seule persistance, et une greffe n'en est pas un.
#[tauri::command(async)]
pub(crate) fn stems_greffer(
    etat: State<Etat>,
    id: i64,
    stem: String,
    remplace: String,
    voisin: i64,
) -> Result<Greffon, String> {
    let (source, autre, tempos) = {
        let lib = verrou(&etat.lib);
        let source = lib
            .track(id)
            .map_err(echec)?
            .ok_or_else(|| "morceau ouvert introuvable en base".to_string())?;
        let autre = lib
            .track(voisin)
            .map_err(echec)?
            .ok_or_else(|| "le morceau d'où greffer est introuvable en base".to_string())?;
        let tempos = lib.tempos(&[id, voisin]).map_err(echec)?;
        (source, autre, tempos)
    };

    let origine = format!(
        "{} — {}",
        autre.artist.clone().unwrap_or_else(|| "?".into()),
        autre.title.clone().unwrap_or_else(|| "?".into())
    );
    let dossier_autre = dossier_stems(&etat, Path::new(&autre.path));
    let Some((_, greffon)) = stems_du_dossier(&dossier_autre)
        .into_iter()
        .find(|(n, _)| *n == stem)
    else {
        return Err(format!(
            "{origine} n'a pas de stem « {stem} » — il faut d'abord le séparer"
        ));
    };

    let base = Path::new(&autre.path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "morceau".into());
    let sortie = dossier_stems(&etat, Path::new(&source.path))
        .join("greffes")
        .join(format!("{stem} — {base}.wav"));

    // **Les grilles se mesurent sur les stems, les tempos viennent de la base.**
    // Ce n'est pas une incohérence : la base a mesuré le morceau entier, ce qui
    // est plus sûr qu'une batterie seule, et c'est ce tempo-là qui a servi à
    // choisir le voisin. La grille n'apporte que la phase — mais elle apporte
    // aussi son propre tempo, plus fin, dont on se sert pour l'étirement quand
    // la base n'en a pas.
    // **Mesure par mesure d'abord** (plan 2.1 et 2.4) : les premiers temps
    // de Beat This! des deux côtés et, si les poids d'all-in-one sont là,
    // les sections — refrain sur refrain. Sinon, la greffe d'un bloc.
    use rusty_music_editor::greffe;
    let grille_de = |chemin: &str, tid: i64| -> Result<greffe::Grille, String> {
        let dossier = dossier_stems(&etat, Path::new(chemin));
        let p = pulsation_de(
            &etat,
            Path::new(chemin),
            &dossier.join(rusty_music_editor::pulsation::FICHIER_CACHE),
            rusty_music_editor::pulsation::Modele::Complet,
        )?;
        let sections = if sections_disponibles() {
            sections_de(&etat, tid)
                .map(|v| {
                    v.sections
                        .into_iter()
                        .map(|x| greffe::Partie {
                            debut_s: x.debut_s,
                            fin_s: x.fin_s,
                            etiquette: x.etiquette,
                        })
                        .collect()
                })
                .unwrap_or_else(|e| {
                    tracing::warn!("greffe : sections indisponibles : {e}");
                    Vec::new()
                })
        } else {
            Vec::new()
        };
        Ok(greffe::Grille {
            metrique: p.metrique(),
            premiers_temps: p.premiers_temps,
            sections,
        })
    };
    let par_mesures = grille_de(&source.path, id)
        .and_then(|a| Ok((a, grille_de(&autre.path, voisin)?)))
        .and_then(|(a, b)| greffe::mesure_a_mesure(&a, &b).map(|()| (a, b)));
    let (plan, repli) = match par_mesures {
        Ok((a, b)) => (
            greffe::greffer_par_mesures(Path::new(&remplace), &greffon, &a, &b, &sortie)
                .map_err(echec)?,
            None,
        ),
        Err(raison) => {
            tracing::info!("greffe d'un bloc : {raison}");
            let grilles = grilles_des_stems(Path::new(&remplace), &greffon);
            let plan = greffe::greffer(
                Path::new(&remplace),
                &greffon,
                tempos.get(&id).copied().or(grilles.map(|(a, _)| a.bpm)),
                tempos.get(&voisin).copied().or(grilles.map(|(_, b)| b.bpm)),
                grilles.map(|(a, b)| greffe::Cale {
                    phase_remplace_s: a.phase_s,
                    phase_greffon_s: b.phase_s,
                    periode_greffon_s: b.periode(),
                }),
                &sortie,
            )
            .map_err(echec)?;
            (plan, Some(raison))
        }
    };

    Ok(Greffon {
        chemin: sortie.display().to_string(),
        origine,
        facteur: plan.facteur,
        octaves: plan.octaves,
        retard_s: plan.retard_s,
        boucles: plan.boucles,
        cale_aux_temps: plan.cale_aux_temps,
        mesures: plan.mesures,
        mesures_par_section: plan.mesures_par_section,
        repli,
    })
}

// ---------------------------------------------------------------------------
// Réglages : source de la bibliothèque
// ---------------------------------------------------------------------------

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
