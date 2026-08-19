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
//! **Ce qu'on ne fait pas, et qu'il faut savoir.** Les temps forts ne sont pas
//! alignés. Cela demanderait une grille de battements, que `descripteurs.rs`
//! ne calcule pas — il rend un tempo, pas une phase, et le mixage DJ, seul
//! usage qui l'exigerait, est hors du périmètre du module 3. La conséquence
//! s'entend : les deux batteries pulsent au même tempo, sans garantie de
//! tomber sur le même temps. Caler à la main avec la vitesse par stem est le
//! recours, et c'est cohérent — les deux réglages sont arrivés ensemble.

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
        let mut energie = 0.0f32;
        for i in t * canaux..fin * canaux {
            energie += signal[i] * signal[i];
        }
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

    // 2. Le départ. La matière est rognée de son silence de tête, puis
    //    retardée de celui du stem qu'elle remplace : le greffon entre là où
    //    l'ancien entrait, pas là où lui-même commençait.
    let tete = premiere_attaque(&matiere, CANAUX);
    let matiere = &matiere[tete * CANAUX..];
    let retard = premiere_attaque(&entrelace(&ancien), CANAUX);

    // 3. La longueur.
    let fondu = (SR as f32 * FONDU_S) as usize;
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
    };
    tracing::info!(
        facteur = plan.facteur,
        octaves = plan.octaves,
        retard = plan.retard_s,
        boucles = plan.boucles,
        "greffe écrite"
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
    #[test]
    fn un_greffon_tres_court_ne_bloque_pas() {
        let bref = sinus(220.0, 0.005); // 5 ms, un quart du fondu
        let out = assembler(&bref, CANAUX, SR as usize / 10, 0, 441);
        assert_eq!(out.len(), (SR as usize / 10) * CANAUX);
        assert!(out.iter().any(|v| v.abs() > 1e-6), "sortie muette");
    }
}
