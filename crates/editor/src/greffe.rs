// SPDX-License-Identifier: GPL-3.0-or-later
//! Greffe : mettre à la place d'un stem celui d'un autre morceau.
//!
//! C'est le premier geste de l'éditeur qui va chercher quelque chose dans la
//! bibliothèque — on cherche une batterie voisine sur la carte, et on l'échange
//! (`docs/ui-spec-editeur.md`). Le voisinage sonore est calculé ailleurs, dans
//! le module 2 ; ici on ne s'occupe que de faire tenir le greffon à la place de
//! ce qu'il remplace, ce qui demande trois choses et pas une de plus :
//!
//! 1. **le tempo.** Un stem greffé à 96 BPM sous un morceau à 124 flotte
//!    aussitôt. On étire le greffon du rapport des deux tempos ;
//! 2. **le départ.** Le stem remplacé ne commence pas forcément à zéro — une
//!    batterie entre après l'intro. Le greffon prend le même départ, sinon la
//!    batterie arrive trente secondes trop tôt ;
//! 3. **la longueur.** Le greffon est plus court ou plus long que la place à
//!    tenir : on le répète ou on le coupe.
//!
//! **Les temps forts, eux, se calent — quand on donne la grille.** C'est le
//! [`Cale`] optionnel. Sans lui, le greffon entre à la première attaque du stem
//! remplacé, ce qui met les deux matières au même tempo sans garantir qu'elles
//! tombent sur le même temps ; avec lui, le greffon entre sur un battement et
//! sa boucle est coupée à un nombre entier de battements, de sorte que chaque
//! répétition retombe juste.
//!
//! **La grille se calcule ailleurs** — `rusty_music_analysis::battements` —
//! et arrive ici en deux nombres. L'éditeur ne dépend pas du module 2 : ce
//! serait tirer CLAP, ses 117 Mo de poids et sa génération de code dans un
//! crate qui n'en a que faire. C'est l'application qui relie les deux.

use std::path::Path;

use crate::{decode, etirement, wav, Error, Result};

/// Fréquence de travail de l'éditeur — celle des stems produits.
const SR: u32 = 44_100;
const CANAUX: usize = 2;

/// Fondu aux jonctions de boucle, en secondes. Vingt millisecondes : assez
/// pour effacer le claquement d'une coupure nette, trop peu pour s'entendre
/// comme un fondu.
const FONDU_S: f32 = 0.02;

/// Ce qu'il a fallu faire pour que le greffon tienne la place. Rendu à
/// l'interface, qui le dit plutôt que de laisser deviner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Facteur de **durée** appliqué au greffon. 0,8 = un cinquième plus court.
    pub facteur: f32,
    /// Repliements d'octave du rapport de tempo : +1 = le greffon est joué à
    /// double tempo, −1 = à demi-tempo.
    pub octaves: i32,
    /// Retard imposé au greffon pour qu'il entre là où l'ancien stem entrait.
    pub retard_s: f32,
    /// Combien de fois la matière a été posée. 1 = pas de boucle.
    pub boucles: usize,
    /// Tempo effectif du greffon une fois étiré.
    pub bpm_rendu: f32,
    /// Le greffon est-il entré sur un battement, ou sur la première attaque ?
    /// **Rendu à l'interface plutôt que deviné** : c'est la différence entre
    /// une greffe qui tombe juste et une qu'il faudra rattraper à la main.
    pub cale_aux_temps: bool,
    /// Greffe mesure par mesure ([`greffer_par_mesures`]) : mesures posées,
    /// et combien ont trouvé une section de même nom chez le greffon. 0 et 0
    /// pour la greffe d'un bloc.
    pub mesures: usize,
    pub mesures_par_section: usize,
}

/// Où tombent les battements, de part et d'autre. Ce que [`greffer`] ne sait
/// pas calculer et qu'on lui donne.
///
/// Les deux phases sont **dans la durée d'origine de chaque fichier** : celle
/// du greffon est mise à l'échelle ici, après l'étirement, pour qu'un appelant
/// n'ait pas à savoir de quel facteur on l'aura étiré.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cale {
    /// Instant du premier battement du stem remplacé, en secondes.
    pub phase_remplace_s: f32,
    /// Instant du premier battement du greffon, en secondes, avant étirement.
    pub phase_greffon_s: f32,
    /// Période d'un battement du greffon, en secondes, avant étirement.
    pub periode_greffon_s: f32,
}

/// Rapport d'étirement pour amener `greffon` au tempo de `source`, replié à
/// l'octave la plus proche.
///
/// **Replier plutôt qu'étirer bêtement.** Une boucle à 70 BPM sous un morceau
/// à 140 n'a pas besoin d'être accélérée du double : ses temps tombent déjà un
/// sur deux. Étirer d'un facteur 2 détruirait la matière pour rien. On ramène
/// donc le rapport dans [1/√2, √2] par doublements successifs — c'est ce que
/// fait tout logiciel de mixage, et cela borne l'étirement à ±41 % au pire.
///
/// Rend le facteur de durée et le nombre de repliements.
pub fn tempo_replie(bpm_source: f32, bpm_greffon: f32) -> (f32, i32) {
    if !(bpm_source.is_finite() && bpm_greffon.is_finite())
        || bpm_source <= 0.0
        || bpm_greffon <= 0.0
    {
        return (1.0, 0);
    }
    // Facteur de durée : le greffon doit devenir plus court s'il est plus lent
    // que la source… non, l'inverse. Un greffon à 100 BPM sous une source à
    // 120 doit être accéléré, donc raccourci : 100/120 = 0,83.
    let mut facteur = bpm_greffon / bpm_source;
    let mut octaves = 0;
    const HAUT: f32 = std::f32::consts::SQRT_2;
    const BAS: f32 = 1.0 / std::f32::consts::SQRT_2;
    while facteur > HAUT && octaves < 4 {
        facteur /= 2.0;
        octaves += 1;
    }
    while facteur < BAS && octaves > -4 {
        facteur *= 2.0;
        octaves -= 1;
    }
    (facteur, octaves)
}

/// La trame où le signal commence vraiment.
///
/// **Pas le premier échantillon non nul** : un stem séparé n'est jamais
/// exactement silencieux, le modèle y laisse un fond à −60 dB. On cherche donc
/// la première fenêtre de dix millisecondes dont l'énergie dépasse un
/// vingtième de l'énergie moyenne du morceau — un seuil relatif, puisque les
/// stems n'ont pas tous le même niveau.
pub fn premiere_attaque(signal: &[f32], canaux: usize) -> usize {
    if canaux == 0 || signal.is_empty() {
        return 0;
    }
    let trames = signal.len() / canaux;
    let fenetre = (SR as usize / 100).max(1); // 10 ms
    let moyenne: f32 = signal.iter().map(|v| v * v).sum::<f32>() / signal.len() as f32;
    if moyenne <= f32::MIN_POSITIVE {
        return 0;
    }
    let seuil = moyenne / 20.0;

    let mut t = 0;
    while t < trames {
        let fin = (t + fenetre).min(trames);
        let energie: f32 = signal[t * canaux..fin * canaux].iter().map(|v| v * v).sum();
        let n = ((fin - t) * canaux).max(1);
        if energie / n as f32 > seuil {
            return t;
        }
        t = fin;
    }
    0
}

/// Pose le greffon dans une place de `cible` trames : retard, boucles, coupe.
///
/// Les jonctions de boucle sont fondues. Sans cela, chaque répétition marque
/// un claquement là où la fin de la matière rencontre son début.
pub fn assembler(
    greffon: &[f32],
    canaux: usize,
    cible: usize,
    retard: usize,
    fondu: usize,
) -> Vec<f32> {
    let mut sortie = vec![0.0f32; cible * canaux];
    if canaux == 0 || greffon.is_empty() || cible == 0 {
        return sortie;
    }
    let n = greffon.len() / canaux;
    if n == 0 {
        return sortie;
    }
    // Le fondu ne peut pas manger plus que la moitié de la matière : sur une
    // boucle très courte, il ne resterait rien de non fondu.
    let fondu = fondu.min(n / 2);
    // Chaque copie recouvre la précédente sur la durée du fondu.
    let pas = (n - fondu).max(1);

    let mut depart = retard;
    let mut boucles = 0;
    while depart < cible {
        for t in 0..n {
            let ou = depart + t;
            if ou >= cible {
                break;
            }
            // Fondu entrant sur chaque copie sauf, en pratique, la première —
            // qui n'a rien à recouvrir mais que l'on fond tout de même, pour
            // ne pas commencer sur un front.
            let mut gain = 1.0f32;
            if fondu > 0 {
                if t < fondu {
                    gain *= t as f32 / fondu as f32;
                }
                if t >= n - fondu {
                    gain *= (n - t) as f32 / fondu as f32;
                }
            }
            for c in 0..canaux {
                sortie[ou * canaux + c] += greffon[t * canaux + c] * gain;
            }
        }
        boucles += 1;
        depart += pas;
        // Garde-fou : une matière plus courte que le fondu boucherait la
        // sortie d'une répétition par trame.
        if boucles > cible {
            break;
        }
    }
    sortie
}

/// Coupe la matière pour qu'elle commence sur un battement et dure un nombre
/// entier de battements.
///
/// **Les deux moitiés servent à des choses différentes.** Commencer sur un
/// battement fait entrer le greffon au bon endroit ; durer un nombre entier de
/// battements fait que la *répétition* retombe juste. Sans la seconde, une
/// greffe qui boucle six fois se désaccorde six fois, un peu plus à chaque
/// tour — c'est le défaut le plus long à entendre et le plus sûr à survenir,
/// puisque la matière n'a aucune raison de mesurer un compte rond.
///
/// `phase_s` et `periode_s` sont dans la durée **déjà étirée**. Le fondu vient
/// en plus des battements entiers : [`assembler`] avance de `n - fondu` d'une
/// copie à l'autre, c'est donc ce pas-là qui doit tomber juste.
///
/// Rend la tranche et si la coupe a bien pu se faire — une matière plus courte
/// qu'un battement est rendue telle quelle.
pub fn decouper_aux_temps(
    matiere: &[f32],
    canaux: usize,
    phase_s: f32,
    periode_s: f32,
    fondu: usize,
) -> (&[f32], bool) {
    if canaux == 0 || matiere.is_empty() || !periode_s.is_finite() || periode_s <= 0.0 {
        return (matiere, false);
    }
    let periode = (periode_s * SR as f32).max(1.0);
    let tete = ((phase_s.max(0.0) * SR as f32) as usize * canaux).min(matiere.len());
    let reste = &matiere[tete..];
    let trames = reste.len() / canaux;

    let battements = (trames.saturating_sub(fondu) as f32 / periode).floor();
    if battements < 1.0 {
        return (reste, false);
    }
    let garde = ((battements * periode) as usize + fondu) * canaux;
    (&reste[..garde.min(reste.len())], true)
}

/// Combien de fois la matière tient dans la place, une fois le retard pris.
fn compter_boucles(n: usize, cible: usize, retard: usize, fondu: usize) -> usize {
    if n == 0 || cible <= retard {
        return 0;
    }
    let fondu = fondu.min(n / 2);
    let pas = (n - fondu).max(1);
    (cible - retard).div_ceil(pas)
}

/// Écrit à `sortie` le stem `greffon` mis à la place de `remplace`.
///
/// `remplace` donne la durée à tenir et l'instant d'entrée ; les deux tempos
/// donnent l'étirement. Quand l'un des deux manque, le greffon est posé tel
/// quel et le plan le dit — un facteur inventé serait pire que pas de facteur.
pub fn greffer(
    remplace: &Path,
    greffon: &Path,
    bpm_source: Option<f32>,
    bpm_greffon: Option<f32>,
    cale: Option<Cale>,
    sortie: &Path,
) -> Result<Plan> {
    let ancien = decode::stereo(remplace)?;
    let neuf = decode::stereo(greffon)?;
    if neuf.gauche.is_empty() {
        return Err(Error::Decodage(decode::Error::Vide {
            path: greffon.to_path_buf(),
        }));
    }

    let cible = ancien.gauche.len();
    let entrelace = |s: &decode::Stereo| -> Vec<f32> {
        s.gauche
            .iter()
            .zip(&s.droite)
            .flat_map(|(g, d)| [*g, *d])
            .collect()
    };

    // 1. Le tempo.
    let (facteur, octaves) = match (bpm_source, bpm_greffon) {
        (Some(a), Some(b)) => tempo_replie(a, b),
        _ => (1.0, 0),
    };
    let mut matiere = entrelace(&neuf);
    if (facteur - 1.0).abs() > 1e-3 {
        matiere = etirement::etirer(&matiere, CANAUX, facteur);
    }

    let fondu = (SR as f32 * FONDU_S) as usize;

    // 2. Le départ, et 3. la longueur — les deux ensemble, parce que la grille
    //    de battements les décide ensemble quand on l'a.
    let (matiere, retard, cale_aux_temps) = match cale {
        // **Avec la grille.** Le greffon entre sur un battement, et sa matière
        // est coupée à un nombre entier de battements : sans quoi chaque
        // répétition de la boucle décalerait le suivant d'un reste, et la
        // greffe se désaccorderait toute seule au fil du morceau.
        Some(c) if c.periode_greffon_s > 0.0 => {
            let (coupee, entiere) = decouper_aux_temps(
                &matiere,
                CANAUX,
                c.phase_greffon_s * facteur,
                c.periode_greffon_s * facteur,
                fondu,
            );
            (
                coupee.to_vec(),
                (c.phase_remplace_s * SR as f32) as usize,
                entiere,
            )
        }
        // **Sans elle**, le repli d'hier : rogner le silence de tête du
        // greffon, et le retarder de celui du stem qu'il remplace. Les tempos
        // se rejoignent, les temps forts non.
        _ => {
            let tete = premiere_attaque(&matiere, CANAUX);
            (
                matiere[tete * CANAUX..].to_vec(),
                premiere_attaque(&entrelace(&ancien), CANAUX),
                false,
            )
        }
    };
    let matiere = matiere.as_slice();

    let boucles = compter_boucles(matiere.len() / CANAUX, cible, retard, fondu);
    let pose = assembler(matiere, CANAUX, cible, retard, fondu);

    let (g, d): (Vec<f32>, Vec<f32>) = pose
        .chunks_exact(CANAUX)
        .map(|c| (c[0].clamp(-1.0, 1.0), c[1].clamp(-1.0, 1.0)))
        .unzip();
    if let Some(dossier) = sortie.parent() {
        std::fs::create_dir_all(dossier)?;
    }
    wav::ecrire(sortie, &g, &d, SR)?;

    let plan = Plan {
        facteur,
        octaves,
        retard_s: retard as f32 / SR as f32,
        boucles,
        bpm_rendu: bpm_greffon.map(|b| b / facteur).unwrap_or(0.0),
        cale_aux_temps,
        mesures: 0,
        mesures_par_section: 0,
    };
    tracing::info!(
        facteur = plan.facteur,
        octaves = plan.octaves,
        retard = plan.retard_s,
        boucles = plan.boucles,
        cale = plan.cale_aux_temps,
        "greffe écrite"
    );
    Ok(plan)
}

/// Une section d'un morceau (all-in-one), en secondes.
#[derive(Debug, Clone, PartialEq)]
pub struct Partie {
    pub debut_s: f32,
    pub fin_s: f32,
    /// Étiquette de Harmonix : `intro`, `verse`, `chorus`… ; `start` et
    /// `end` sont le silence avant et après la musique.
    pub etiquette: String,
}

/// Ce qu'il faut savoir d'un côté de la greffe pour la faire mesure par
/// mesure : ses premiers temps (Beat This!), sa métrique, ses sections.
#[derive(Debug, Clone, Default)]
pub struct Grille {
    pub premiers_temps: Vec<f32>,
    pub metrique: Option<u32>,
    /// Vide : les mesures sont prises dans l'ordre, sans tenir compte des
    /// sections.
    pub sections: Vec<Partie>,
}

/// Les unités de la grille : mesures `[début, fin)`, regroupées par deux
/// (`octaves` = 1) ou coupées en deux (−1). La dernière mesure se prolonge
/// de la longueur de la précédente, sans dépasser `duree_s`.
fn unites(premiers: &[f32], octaves: i32, duree_s: f32) -> Vec<(f32, f32)> {
    let mut m: Vec<(f32, f32)> = premiers.windows(2).map(|w| (w[0], w[1])).collect();
    if let (Some(&(a, b)), Some(&der)) = (m.last(), premiers.last()) {
        let fin = (der + (b - a)).min(duree_s);
        if fin > der + 0.25 * (b - a) {
            m.push((der, fin));
        }
    }
    for _ in 0..octaves.max(0) {
        m = m.chunks(2).map(|c| (c[0].0, c[c.len() - 1].1)).collect();
    }
    for _ in 0..(-octaves).max(0) {
        m = m
            .iter()
            .flat_map(|&(a, b)| [(a, (a + b) / 2.0), ((a + b) / 2.0, b)])
            .collect();
    }
    m
}

fn etiquette_a(sections: &[Partie], t: f32) -> Option<&str> {
    sections
        .iter()
        .find(|s| t >= s.debut_s && t < s.fin_s)
        .map(|s| s.etiquette.as_str())
}

fn silence(etiquette: Option<&str>) -> bool {
    matches!(etiquette, Some("start") | Some("end"))
}

/// Suites d'unités consécutives de même étiquette : `(étiquette, indices)`.
fn suites<'a>(u: &[(f32, f32)], sections: &'a [Partie]) -> Vec<(Option<&'a str>, Vec<usize>)> {
    let mut out: Vec<(Option<&str>, Vec<usize>)> = Vec::new();
    for (i, &(a, b)) in u.iter().enumerate() {
        let e = etiquette_a(sections, (a + b) / 2.0);
        match out.last_mut() {
            Some((d, v)) if *d == e => v.push(i),
            _ => out.push((e, vec![i])),
        }
    }
    out
}

/// Quelle unité du greffon va sur chaque unité de la source.
///
/// La *k*-ième mesure d'un refrain de la source reçoit la *k*-ième mesure
/// d'un refrain du greffon (en boucle si celui-ci est plus court), le
/// *n*-ième refrain allant sur le *n*-ième, en boucle lui aussi. Le silence
/// de la source (`start`, `end`) reste silencieux. Une étiquette que le
/// greffon n'a pas prend ses mesures musicales dans l'ordre.
///
/// Rend la correspondance et le nombre d'unités appariées par section.
pub fn correspondance(
    source: &[(f32, f32)],
    sections_source: &[Partie],
    greffon: &[(f32, f32)],
    sections_greffon: &[Partie],
) -> (Vec<Option<usize>>, usize) {
    let suites_g = suites(greffon, sections_greffon);
    let musique: Vec<usize> = suites_g
        .iter()
        .filter(|(e, _)| !silence(*e))
        .flat_map(|(_, v)| v.iter().copied())
        .collect();
    let mut sortie = vec![None; source.len()];
    let mut appariees = 0;
    let mut vues: std::collections::HashMap<Option<&str>, usize> = Default::default();
    let mut libre = 0; // curseur des mesures prises dans l'ordre
    for (e, v) in suites(source, sections_source) {
        if silence(e) {
            continue;
        }
        let n = vues.entry(e).or_insert(0);
        let memes: Vec<&Vec<usize>> = suites_g
            .iter()
            .filter(|(eg, _)| e.is_some() && *eg == e)
            .map(|(_, v)| v)
            .collect();
        if let Some(suite) = (!memes.is_empty()).then(|| memes[*n % memes.len()]) {
            for (k, &j) in v.iter().enumerate() {
                sortie[j] = Some(suite[k % suite.len()]);
            }
            appariees += v.len();
        } else if !musique.is_empty() {
            for &j in &v {
                sortie[j] = Some(musique[libre % musique.len()]);
                libre += 1;
            }
        }
        *n += 1;
    }
    (sortie, appariees)
}

/// Pose la matière du greffon unité par unité : chaque unité `i` est étirée
/// (WSOLA) sur la durée de l'unité `j` qu'elle remplace, et fondue sur
/// [`FONDU_S`] avec la suivante. Rend le signal entrelacé de `cible` trames
/// et la médiane des facteurs d'étirement.
pub fn poser_mesures(
    matiere: &[f32],
    canaux: usize,
    source: &[(f32, f32)],
    greffon: &[(f32, f32)],
    correspondance: &[Option<usize>],
    cible: usize,
) -> (Vec<f32>, f32) {
    let mut sortie = vec![0.0f32; cible * canaux];
    let trames = matiere.len() / canaux.max(1);
    let fondu_s = FONDU_S;
    let mut facteurs = Vec::new();
    for (j, i) in correspondance.iter().enumerate() {
        let Some(i) = *i else { continue };
        let ((a, b), (c, d)) = (source[j], greffon[i]);
        if b <= a || d <= c {
            continue;
        }
        let facteur = ((b - a) / (d - c)).clamp(0.5, 2.0);
        facteurs.push(facteur);
        // Un fondu de part et d'autre, **avant** le premier temps et après
        // la fin : le fondu d'entrée ne doit pas manger l'attaque du premier
        // temps, et celui de sortie recouvre l'entrée de l'unité suivante.
        let avance = fondu_s.min(c * facteur).min(a);
        let t0 = (((c - avance / facteur) * SR as f32) as usize).min(trames);
        let t1 = (((d + fondu_s / facteur) * SR as f32) as usize).min(trames);
        if t1 <= t0 {
            continue;
        }
        let morceau = etirement::etirer(&matiere[t0 * canaux..t1 * canaux], canaux, facteur);
        let n = morceau.len() / canaux;
        let depart = ((a - avance) * SR as f32) as usize;
        let fondu = ((fondu_s * SR as f32) as usize).min(n / 2).max(1);
        for t in 0..n {
            let ou = depart + t;
            if ou >= cible {
                break;
            }
            let mut gain = 1.0f32;
            if t < fondu {
                gain *= t as f32 / fondu as f32;
            }
            if t >= n - fondu {
                gain *= (n - t) as f32 / fondu as f32;
            }
            for k in 0..canaux {
                sortie[ou * canaux + k] += morceau[t * canaux + k] * gain;
            }
        }
    }
    facteurs.sort_by(f32::total_cmp);
    let mediane = facteurs.get(facteurs.len() / 2).copied().unwrap_or(1.0);
    (sortie, mediane)
}

/// Repliement d'octave entre deux grilles : de combien de doublements la
/// mesure du greffon s'écarte de celle de la source (médianes).
fn octaves_entre(source: &[f32], greffon: &[f32]) -> i32 {
    let med = |p: &[f32]| {
        let mut d: Vec<f32> = p
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|x| *x > 0.0)
            .collect();
        d.sort_by(f32::total_cmp);
        d.get(d.len() / 2).copied()
    };
    match (med(source), med(greffon)) {
        (Some(a), Some(b)) => (a / b).log2().round().clamp(-2.0, 2.0) as i32,
        _ => 0,
    }
}

/// Peut-on greffer mesure par mesure ? Deux grilles d'au moins quatre
/// mesures, et la même métrique : un 3/4 sous un 4/4 ne se cale pas mesure
/// à mesure, on le refuse plutôt que de le tordre.
pub fn mesure_a_mesure(source: &Grille, greffon: &Grille) -> std::result::Result<(), String> {
    if source.premiers_temps.len() < 4 || greffon.premiers_temps.len() < 4 {
        return Err("pas assez de mesures détectées".into());
    }
    match (source.metrique, greffon.metrique) {
        (Some(a), Some(b)) if a != b => Err(format!("métriques différentes ({b}/4 sous {a}/4)")),
        _ => Ok(()),
    }
}

/// Écrit à `sortie` le stem `greffon` mis à la place de `remplace`, **mesure
/// par mesure** : chaque mesure de la source reçoit une mesure du greffon,
/// prise dans une section de même nom ([`correspondance`]), étirée sur sa
/// durée exacte — un tempo qui dérive ne désaligne plus rien. Le tempo
/// moyen est replié à l'octave comme dans [`greffer`] : à demi-tempo, deux
/// mesures du greffon en font une.
///
/// À n'appeler que si [`mesure_a_mesure`] l'accepte.
pub fn greffer_par_mesures(
    remplace: &Path,
    greffon: &Path,
    source: &Grille,
    autre: &Grille,
    sortie: &Path,
) -> Result<Plan> {
    let ancien = decode::stereo(remplace)?;
    let neuf = decode::stereo(greffon)?;
    if neuf.gauche.is_empty() {
        return Err(Error::Decodage(decode::Error::Vide {
            path: greffon.to_path_buf(),
        }));
    }
    let cible = ancien.gauche.len();
    drop(ancien);
    let matiere: Vec<f32> = neuf
        .gauche
        .iter()
        .zip(&neuf.droite)
        .flat_map(|(g, d)| [*g, *d])
        .collect();
    let duree_greffon = neuf.gauche.len() as f32 / SR as f32;
    drop(neuf);

    let octaves = octaves_entre(&source.premiers_temps, &autre.premiers_temps);
    let us = unites(&source.premiers_temps, 0, cible as f32 / SR as f32);
    let ug = unites(&autre.premiers_temps, octaves, duree_greffon);
    let (corr, par_section) = correspondance(&us, &source.sections, &ug, &autre.sections);
    let (pose, facteur) = poser_mesures(&matiere, CANAUX, &us, &ug, &corr, cible);

    let (g, d): (Vec<f32>, Vec<f32>) = pose
        .chunks_exact(CANAUX)
        .map(|c| (c[0].clamp(-1.0, 1.0), c[1].clamp(-1.0, 1.0)))
        .unzip();
    if let Some(dossier) = sortie.parent() {
        std::fs::create_dir_all(dossier)?;
    }
    wav::ecrire(sortie, &g, &d, SR)?;

    let posees = corr.iter().flatten().count();
    let plan = Plan {
        facteur,
        octaves,
        retard_s: corr
            .iter()
            .position(Option::is_some)
            .map(|j| us[j].0)
            .unwrap_or(0.0),
        boucles: 1,
        bpm_rendu: 0.0,
        cale_aux_temps: true,
        mesures: posees,
        mesures_par_section: par_section,
    };
    tracing::info!(
        facteur,
        octaves,
        mesures = posees,
        par_section,
        "greffe mesure par mesure écrite"
    );
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le repliement doit ramener tout rapport de tempo dans [1/√2, √2] — la
    /// borne qui garantit qu'on n'étire jamais de plus de 41 %.
    #[test]
    fn le_tempo_se_replie_a_loctave() {
        // Même tempo : rien à faire.
        assert_eq!(tempo_replie(120.0, 120.0), (1.0, 0));

        // Moitié et double : les temps tombent déjà juste, on ne touche à rien.
        let (f, o) = tempo_replie(120.0, 60.0);
        assert!((f - 1.0).abs() < 1e-6, "60 sous 120 : facteur {f}");
        assert_eq!(o, -1);
        let (f, o) = tempo_replie(120.0, 240.0);
        assert!((f - 1.0).abs() < 1e-6, "240 sous 120 : facteur {f}");
        assert_eq!(o, 1);

        // Un écart réel : 96 sous 124, à peine un quart plus lent.
        let (f, _) = tempo_replie(124.0, 96.0);
        assert!((f - 96.0 / 124.0).abs() < 1e-6);

        // Et dans tous les cas la borne tient.
        for source in [60.0f32, 90.0, 124.0, 175.0] {
            for greffon in [58.0f32, 72.0, 96.0, 128.0, 174.0, 200.0] {
                let (f, _) = tempo_replie(source, greffon);
                assert!(
                    (0.707..=1.415).contains(&f),
                    "{greffon} sous {source} : facteur {f} hors borne"
                );
            }
        }

        // Absence ou absurdité : on ne touche à rien plutôt que d'inventer.
        assert_eq!(tempo_replie(0.0, 120.0), (1.0, 0));
        assert_eq!(tempo_replie(120.0, f32::NAN), (1.0, 0));
    }

    /// Un tempo replié reste un multiple ou un diviseur du tempo visé : c'est
    /// ce qui rend le repliement acceptable musicalement.
    #[test]
    fn le_tempo_rendu_est_un_multiple_du_tempo_vise() {
        for (source, greffon) in [(120.0f32, 60.0f32), (124.0, 250.0), (90.0, 178.0)] {
            let (f, _) = tempo_replie(source, greffon);
            let rendu = greffon / f;
            let rapport = rendu / source;
            let plus_proche = rapport.log2().round().exp2();
            assert!(
                (rapport - plus_proche).abs() < 0.06 * plus_proche,
                "{greffon} sous {source} rend {rendu}, rapport {rapport}"
            );
        }
    }

    fn sinus(hz: f32, secondes: f32) -> Vec<f32> {
        let n = (SR as f32 * secondes) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * hz * i as f32 / SR as f32).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    /// Le silence de tête doit être trouvé, et le fond de bruit d'un stem
    /// séparé ne doit pas passer pour une attaque.
    #[test]
    fn lattaque_se_trouve_apres_le_silence() {
        let mut s: Vec<f32> = vec![0.0; SR as usize * CANAUX / 2]; // 0,5 s
        s.extend(sinus(440.0, 1.0));
        let t = premiere_attaque(&s, CANAUX);
        let secondes = t as f32 / SR as f32;
        assert!(
            (secondes - 0.5).abs() < 0.03,
            "attaque trouvée à {secondes:.3} s au lieu de 0,5"
        );

        // Un fond à −60 dB n'est pas une attaque.
        let mut bruite: Vec<f32> = (0..SR as usize * CANAUX / 2)
            .map(|i| 0.001 * ((i % 7) as f32 - 3.0))
            .collect();
        bruite.extend(sinus(440.0, 1.0));
        let t = premiere_attaque(&bruite, CANAUX);
        assert!(
            (t as f32 / SR as f32 - 0.5).abs() < 0.03,
            "le fond de bruit a été pris pour une attaque"
        );

        // Un signal qui commence tout de suite commence à zéro.
        assert_eq!(premiere_attaque(&sinus(440.0, 0.2), CANAUX), 0);
        assert_eq!(premiere_attaque(&[], CANAUX), 0);
        assert_eq!(premiere_attaque(&[0.0; 100], CANAUX), 0);
    }

    /// L'assemblage rend exactement la place demandée : ni plus, ni moins.
    /// C'est ce qui permet au stem greffé de se substituer sans décaler la
    /// lecture des autres.
    #[test]
    fn lassemblage_tient_exactement_la_place() {
        let matiere = sinus(220.0, 1.0);
        for cible in [SR as usize / 2, SR as usize, SR as usize * 3] {
            let out = assembler(&matiere, CANAUX, cible, 0, 441);
            assert_eq!(out.len(), cible * CANAUX, "place tenue pour {cible}");
        }
    }

    /// Le retard laisse du silence devant, et le son commence bien après.
    #[test]
    fn le_retard_laisse_le_silence_devant() {
        let matiere = sinus(220.0, 0.5);
        let retard = SR as usize / 4; // 0,25 s
        let out = assembler(&matiere, CANAUX, SR as usize, retard, 441);

        let avant: f32 = out[..retard * CANAUX].iter().map(|v| v.abs()).sum();
        assert_eq!(avant, 0.0, "du son avant le retard");
        let apres: f32 = out[retard * CANAUX..].iter().map(|v| v.abs()).sum();
        assert!(apres > 1.0, "rien après le retard");
    }

    /// Une matière plus courte que la place est répétée jusqu'au bout ; une
    /// matière plus longue est coupée.
    #[test]
    fn la_matiere_boucle_ou_se_coupe() {
        let court = sinus(220.0, 0.5);
        let out = assembler(&court, CANAUX, SR as usize * 2, 0, 441);
        // La fin doit contenir du son : sans boucle, elle serait muette.
        let queue: f32 = out[out.len() - SR as usize..].iter().map(|v| v.abs()).sum();
        assert!(queue > 1.0, "la boucle ne va pas jusqu'au bout");
        assert_eq!(compter_boucles(SR as usize / 2, SR as usize * 2, 0, 441), 5);

        let long = sinus(220.0, 3.0);
        let out = assembler(&long, CANAUX, SR as usize, 0, 441);
        assert_eq!(out.len(), SR as usize * CANAUX, "matière longue non coupée");
        assert_eq!(compter_boucles(SR as usize * 3, SR as usize, 0, 441), 1);
    }

    /// Le fondu ne doit pas dépasser la moitié d'une matière courte, sans quoi
    /// il ne resterait rien de non fondu et la boucle avancerait d'une trame à
    /// la fois.
    /// Clics mono à une position connue, pour éprouver l'alignement.
    fn clics(bpm: f32, secondes: f32, retard_s: f32) -> Vec<f32> {
        let n = (SR as f32 * secondes) as usize;
        let pas = (SR as f32 * 60.0 / bpm) as usize;
        let mut s = vec![0.0f32; n];
        for d in ((SR as f32 * retard_s) as usize..n).step_by(pas) {
            for i in 0..(SR as usize / 100).min(n - d) {
                s[d + i] = 1.0 - i as f32 / (SR as f32 / 100.0);
            }
        }
        s
    }

    /// Les instants où une salve commence, en secondes.
    fn attaques(signal: &[f32], canaux: usize) -> Vec<f32> {
        let mut t = Vec::new();
        let mut dedans = false;
        for (i, trame) in signal.chunks_exact(canaux).enumerate() {
            let fort = trame.iter().any(|v| v.abs() > 0.3);
            if fort && !dedans {
                t.push(i as f32 / SR as f32);
            }
            dedans = fort;
        }
        t
    }

    /// **La preuve que la boucle retombe juste**, et pas seulement qu'elle
    /// entre juste. Une matière coupée à un compte rond de battements, répétée
    /// pour tenir la place, doit poser toutes ses attaques sur la grille —
    /// y compris celles des dernières répétitions, où une erreur de coupe se
    /// serait accumulée.
    #[test]
    fn une_matiere_coupee_aux_temps_boucle_sur_la_grille() {
        let bpm = 120.0;
        let periode_s = 60.0 / bpm;
        let fondu = (SR as f32 * FONDU_S) as usize;

        // 2,1 s de clics : quatre battements et un reste, exprès. C'est le
        // reste qui désaccorderait la boucle si on ne le coupait pas.
        let greffon = clics(bpm, 2.1, 0.13);
        let (coupe, entiere) = decouper_aux_temps(&greffon, 1, 0.13, periode_s, fondu);
        assert!(entiere, "2,1 s à 120 BPM contient quatre battements");

        let cible = (SR as f32 * 10.0) as usize;
        let retard = (SR as f32 * 0.4) as usize;
        let pose = assembler(coupe, 1, cible, retard, fondu);

        let grille: Vec<f32> = attaques(&pose, 1);
        assert!(grille.len() > 12, "trop peu d'attaques : {}", grille.len());
        for t in &grille {
            // Chaque attaque doit tomber sur 0,4 + k × 0,5 s.
            let depuis = t - 0.4;
            let ecart = (depuis / periode_s - (depuis / periode_s).round()).abs() * periode_s;
            assert!(
                ecart < 0.012,
                "attaque à {t:.3} s, {:.1} ms hors grille",
                ecart * 1000.0
            );
        }
    }

    /// Le contre-essai : sans la coupe, la même matière dérive. Sans lui, le
    /// test précédent ne prouverait pas que c'est la coupe qui aligne.
    #[test]
    fn sans_la_coupe_la_boucle_derive() {
        let bpm = 120.0;
        let periode_s = 60.0 / bpm;
        let fondu = (SR as f32 * FONDU_S) as usize;
        let greffon = clics(bpm, 2.1, 0.13);
        let brute = &greffon[(0.13 * SR as f32) as usize..];

        let cible = (SR as f32 * 10.0) as usize;
        let pose = assembler(brute, 1, cible, (SR as f32 * 0.4) as usize, fondu);

        let pire = attaques(&pose, 1)
            .iter()
            .map(|t| {
                let d = (t - 0.4) / periode_s;
                (d - d.round()).abs() * periode_s
            })
            .fold(0.0f32, f32::max);
        assert!(
            pire > 0.05,
            "la matière non coupée devrait dériver, pire écart {:.1} ms",
            pire * 1000.0
        );
    }

    /// Une matière plus courte qu'un battement se rend telle quelle plutôt que
    /// de disparaître à la coupe.
    #[test]
    fn une_matiere_plus_courte_quun_temps_survit_a_la_coupe() {
        let court = vec![0.5f32; (SR as f32 * 0.2) as usize];
        let (rendu, entiere) = decouper_aux_temps(&court, 1, 0.0, 0.5, 100);
        assert!(!entiere);
        assert_eq!(rendu.len(), court.len());
    }

    fn partie(a: f32, b: f32, e: &str) -> Partie {
        Partie {
            debut_s: a,
            fin_s: b,
            etiquette: e.into(),
        }
    }

    /// Un refrain va sur un refrain, à la même place dans la section ; le
    /// silence reste silencieux ; une étiquette absente du greffon prend ses
    /// mesures dans l'ordre.
    #[test]
    fn les_refrains_vont_sur_les_refrains() {
        // Source : 1 mesure de silence, 2 de couplet, 2 de refrain, 1 de pont, 2 de refrain.
        let us: Vec<(f32, f32)> = (0..8).map(|i| (i as f32, i as f32 + 1.0)).collect();
        let ss = [
            partie(0.0, 1.0, "start"),
            partie(1.0, 3.0, "verse"),
            partie(3.0, 5.0, "chorus"),
            partie(5.0, 6.0, "bridge"),
            partie(6.0, 8.0, "chorus"),
        ];
        // Greffon : 3 de couplet (0-2), 3 de refrain (3-5).
        let ug: Vec<(f32, f32)> = (0..6)
            .map(|i| (i as f32 * 2.0, i as f32 * 2.0 + 2.0))
            .collect();
        let sg = [partie(0.0, 6.0, "verse"), partie(6.0, 12.0, "chorus")];
        let (c, n) = correspondance(&us, &ss, &ug, &sg);
        assert_eq!(
            c,
            vec![
                None,
                Some(0),
                Some(1),
                Some(3),
                Some(4),
                Some(0),
                Some(3),
                Some(4)
            ]
        );
        assert_eq!(n, 6, "couplets et refrains appariés, pas le pont");
    }

    /// Deux mesures de greffon pour une de source quand le greffon est noté
    /// à double tempo ; coupées en deux dans l'autre sens.
    #[test]
    fn les_unites_se_replient_a_loctave() {
        let p = [0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(
            unites(&p, 0, 100.0).len(),
            5,
            "la dernière mesure prolongée"
        );
        assert_eq!(
            unites(&p, 1, 100.0),
            vec![(0.0, 2.0), (2.0, 4.0), (4.0, 5.0)]
        );
        assert_eq!(
            unites(&p[..3], -1, 2.0),
            vec![(0.0, 0.5), (0.5, 1.0), (1.0, 1.5), (1.5, 2.0)]
        );
        assert_eq!(
            octaves_entre(&[0.0, 2.0, 4.0, 6.0], &[0.0, 1.0, 2.0, 3.0]),
            1
        );
    }

    /// **La preuve de la greffe mesure par mesure** : un greffon à 100 BPM
    /// posé sous une source dont le tempo dérive de 120 à 132 BPM. Chaque
    /// premier temps du greffon doit tomber sur le premier temps de la
    /// source — un étirement global ne le pourrait pas.
    #[test]
    fn chaque_mesure_tombe_sur_sa_mesure() {
        // Greffon : un clic par mesure de 4 temps à 100 BPM (2,4 s), 10 mesures.
        let periode_g = 2.4f32;
        let greffon = clics(60.0 / periode_g, 24.0, 0.0);
        let pg: Vec<f32> = (0..10).map(|i| i as f32 * periode_g).collect();
        // Source : des mesures qui raccourcissent de 2,0 s à 1,82 s.
        let mut ps = vec![0.5f32];
        for i in 0..11 {
            let d = 2.0 - 0.018 * i as f32;
            ps.push(ps[i] + d);
        }
        let cible = ((ps[ps.len() - 1] + 1.0) * SR as f32) as usize;
        let us = unites(&ps, 0, cible as f32 / SR as f32);
        let ug = unites(&pg, 0, 24.0);
        let (c, _) = correspondance(&us, &[], &ug, &[]);
        let (pose, _) = poser_mesures(&greffon, 1, &us, &ug, &c, cible);
        let vues = attaques(&pose, 1);
        assert!(vues.len() >= 10, "trop peu d'attaques : {vues:?}");
        for t in &vues {
            let ecart = ps.iter().map(|p| (p - t).abs()).fold(f32::MAX, f32::min);
            assert!(
                ecart < 0.025,
                "attaque à {t:.3} s, {:.1} ms d'un premier temps",
                ecart * 1000.0
            );
        }
    }

    #[test]
    fn une_autre_metrique_est_refusee() {
        let g = |m| Grille {
            premiers_temps: (0..8).map(|i| i as f32).collect(),
            metrique: Some(m),
            sections: vec![],
        };
        assert!(mesure_a_mesure(&g(4), &g(4)).is_ok());
        assert!(mesure_a_mesure(&g(4), &g(3)).is_err());
    }

    #[test]
    fn un_greffon_tres_court_ne_bloque_pas() {
        let bref = sinus(220.0, 0.005); // 5 ms, un quart du fondu
        let out = assembler(&bref, CANAUX, SR as usize / 10, 0, 441);
        assert_eq!(out.len(), (SR as usize / 10) * CANAUX);
        assert!(out.iter().any(|v| v.abs() > 1e-6), "sortie muette");
    }
}
