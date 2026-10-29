// SPDX-License-Identifier: GPL-3.0-or-later
//! Corde et frette de chaque note. Une même hauteur se joue à plusieurs
//! endroits du manche ; le bon choix est celui qui économise la main. C'est un
//! plus court chemin sur la suite des notes (Viterbi), comme le recommande la
//! littérature sur la tablature d'une ligne monophonique
//! (`docs/recherche-editer-pratique-creation.md`).
//!
//! Le modèle est celui d'un professeur de basse : **la main a une position**
//! — l'index sur une frette, les quatre doigts couvrent quatre frettes
//! (« un doigt par case »). Une note frettée doit tomber sous la main ; une
//! corde à vide ne la déplace pas. Le chemin se cherche donc sur les couples
//! (position jouée, position de la main), et le coût d'un passage est :
//! - le déplacement de la main, en frettes — moins cher après un silence,
//!   qui laisse le temps de se replacer ;
//! - un changement de corde, très peu (0,1 par corde traversée) ;
//! - une main haute sur le manche, au-delà de la 9ᵉ position ;
//! - un a priori **appris** sur des tablatures de référence : les frettes et
//!   cordes où les bassistes jouent le plus ([`crate::positions`]).
//!
//! Version précédente (9 oct.) : un coût entre positions successives, sans
//! mémoire de la main. Une corde à vide « libérait » tout, et la main pouvait
//! sauter d'un bout du manche à l'autre autour d'elle — les doigtés
//! incohérents signalés à l'essai.

use serde::{Deserialize, Serialize};

use crate::{positions, Note};

/// La basse de celui qui joue, 4 ou 5 cordes : un réglage de l'utilisateur.
/// L'accordage, lui, est celui du disque ([`Accordage::pour`]) — une 4 cordes
/// passe en mi♭ si le morceau l'est, la tablature le montre, et l'on peut
/// transposer le morceau plutôt que désaccorder (29 oct.).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Basses {
    /// Les anciens noms du réglage (« disque », 4 cordes de préférence ;
    /// « quatre-et-cinq ») se relisent : les caches qui les portent sont
    /// reposés de toute façon (`VERSION_DOIGTES`).
    #[default]
    #[serde(alias = "disque")]
    Quatre,
    #[serde(alias = "quatre-et-cinq")]
    Cinq,
}

/// Un accordage, de la corde la plus grave à la plus aiguë, en hauteurs MIDI.
#[derive(Debug, Clone, PartialEq)]
pub struct Accordage {
    pub cordes: Vec<u8>,
    pub frettes: u8,
}

impl Accordage {
    /// Basse 4 cordes standard : mi1 la1 ré2 sol2, 20 frettes.
    pub fn basse4() -> Self {
        Self { cordes: vec![28, 33, 38, 43], frettes: 20 }
    }
    /// Basse 5 cordes : si0 en plus, en bas.
    pub fn basse5() -> Self {
        Self { cordes: vec![23, 28, 33, 38, 43], frettes: 20 }
    }

    /// Basse 4 cordes en drop D : la corde grave descend au ré0.
    pub fn drop_d() -> Self {
        Self { cordes: vec![26, 33, 38, 43], frettes: 20 }
    }

    /// Les accordages essayés par [`Accordage::choisir`] : standard, abaissés
    /// d'un demi-ton, d'un ton, de deux tons (le grunge : Nirvana), drop D
    /// et ses variantes, cinq cordes.
    pub fn candidats() -> Vec<Self> {
        Self::de([
            vec![28, 33, 38, 43],
            vec![27, 32, 37, 42],
            vec![26, 31, 36, 41],
            vec![24, 29, 34, 39],
            vec![26, 33, 38, 43],
            vec![25, 32, 37, 42],
            vec![24, 31, 36, 41],
            vec![23, 28, 33, 38, 43],
        ])
    }

    /// Les accordages d'une 5 cordes : standard, abaissés d'un demi-ton et
    /// d'un ton.
    pub fn candidats5() -> Vec<Self> {
        Self::de([vec![23, 28, 33, 38, 43], vec![22, 27, 32, 37, 42], vec![21, 26, 31, 36, 41]])
    }

    fn de<const N: usize>(cordes: [Vec<u8>; N]) -> Vec<Self> {
        cordes.into_iter().map(|cordes| Self { cordes, frettes: 20 }).collect()
    }

    /// L'accordage que les notes réclament. Une note sous la corde grave est
    /// impossible ; la corde grave à vide, elle, se joue (le rock accordé
    /// plus bas tourne autour d'elle). Parmi les accordages compatibles —
    /// presque aucune note dessous, la corde grave à vide jouée —, celui qui
    /// donne le doigté le moins coûteux : un morceau écrit pour un accordage
    /// y trouve ses cordes à vide. Sans corde grave jouée, le standard si les
    /// notes y tiennent.
    ///
    /// Banc du 10 oct. (`experiments/partitions/`, 17 morceaux de Nirvana dont
    /// 11 accordés plus bas) : avec le seul standard, les frettes de la
    /// tablature ne correspondaient à aucune des nôtres.
    pub fn choisir(notes: &[Note]) -> Self {
        Self::choisir_parmi(notes, &Self::candidats())
    }

    /// [`Accordage::choisir`] parmi des candidats donnés, le standard en
    /// premier.
    pub fn choisir_parmi(notes: &[Note], candidats: &[Self]) -> Self {
        let tolere = (notes.len() / 100).max(1);
        let sous = |l: u8| notes.iter().filter(|n| n.hauteur < l).count();
        let a_vide = |l: u8| notes.iter().filter(|n| n.hauteur == l).count();
        let standard = candidats.first().cloned().unwrap_or_else(Self::basse4);
        let mut valides: Vec<&Self> = candidats
            .iter()
            .filter(|a| sous(a.cordes[0]) <= tolere && a_vide(a.cordes[0]) >= tolere.max(2))
            .collect();
        if valides.is_empty() {
            if sous(standard.cordes[0]) <= tolere {
                return standard;
            }
            // Des notes sous le mi sans corde grave nette : l'accordage le plus
            // haut qui les contient toutes.
            let haut = candidats.iter().filter(|a| sous(a.cordes[0]) <= tolere).map(|a| a.cordes[0]).max();
            valides = candidats.iter().filter(|a| Some(a.cordes[0]) == haut).collect();
            if valides.is_empty() {
                // Rien ne contient tout : le plus grave.
                return candidats.iter().min_by_key(|a| a.cordes[0]).cloned().unwrap_or(standard);
            }
        }
        let cout = |a: &Self| {
            let mut x = notes.to_vec();
            poser_avec(&mut x, a, &Couts::default())
        };
        valides
            .into_iter()
            .map(|a| (cout(a), a))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, a)| a.clone())
            .unwrap_or(standard)
    }

    /// L'accordage du disque sur la basse de l'utilisateur : parmi ceux d'une
    /// 4 cordes (standard, abaissés, drop D…) ou d'une 5 cordes.
    pub fn pour(notes: &[Note], basses: Basses) -> Self {
        match basses {
            Basses::Quatre => {
                let quatre: Vec<Self> = Self::candidats().into_iter().filter(|a| a.cordes.len() == 4).collect();
                Self::choisir_parmi(notes, &quatre)
            }
            Basses::Cinq => Self::choisir_parmi(notes, &Self::candidats5()),
        }
    }

    fn positions(&self, hauteur: u8) -> Vec<(u8, u8)> {
        self.cordes
            .iter()
            .enumerate()
            .filter_map(|(c, &vide)| {
                let f = hauteur.checked_sub(vide)?;
                (f <= self.frettes).then_some((c as u8, f))
            })
            .collect()
    }

    /// La hauteur ramenée dans la tessiture par octaves — une note hors du
    /// manche est presque toujours une erreur d'octave de la transcription.
    fn ramener(&self, mut h: u8) -> u8 {
        let bas = *self.cordes.iter().min().unwrap_or(&28);
        let haut = self.cordes.iter().max().unwrap_or(&43) + self.frettes;
        while h < bas {
            h += 12;
        }
        while h > haut {
            h -= 12;
        }
        h
    }
}

/// Doigts disponibles : la main couvre `position..position + ETENDUE - 1`.
const ETENDUE: u8 = 4;

/// Les coûts du chemin. [`Couts::default`] est le réglage retenu ; les autres
/// valeurs servent les bancs (`examples/regler.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Couts {
    /// Par corde traversée.
    pub corde: f32,
    /// Par frette de déplacement de la main.
    pub deplacement: f32,
    /// Préférence pour le bas du manche, par position dès la première.
    pub pente_bas: f32,
    /// Au-delà de cette position, chaque position coûte en plus `pente_haut`.
    pub debut_haut: u8,
    pub pente_haut: f32,
    /// Une corde à vide quand la main est haute : par position de la main.
    /// Un bassiste en 10ᵉ position ne va pas chercher la corde à vide.
    pub vide_haut: f32,
    /// Préférence pour les cordes graves, par corde au-dessus de la plus
    /// grave : le son plus rond d'une note jouée haut sur une corde grave.
    pub corde_aigue: f32,
    /// Poids de l'a priori de position appris ([`crate::positions`]) : où,
    /// sur le manche, les bassistes des tablatures de référence jouent.
    pub position: f32,
}

/// Réglé le 10 oct. contre deux tablatures de référence, puis contre les 15
/// tablatures du livre *Californication* (Flea) : une corde à vide quand la
/// main est haute coûte (`vide_haut`).
///
/// Réglage actuel (17 oct.) : banc des doigtés sur 331 tablatures (31 des
/// livres du banc, 300 parties de basse Songsterr ; exemple `doigtes`,
/// `experiments/partitions/regler_doigtes_loo.py`), validation croisée un
/// morceau de côté : 64 % de doigtés identiques avec l'ancien réglage, 73 %
/// avec les seuls coûts réglés à nouveau, **77 %** avec l'a priori de position
/// appris ([`crate::positions`]). Le pire de l'ancien réglage était
/// `vide_haut` 0,3, réglé sur Flea seul : il poussait la main vers le bas du
/// manche (« Love Foolosophy » : 41 % → 66 % sur la transcription réelle).
/// Le plateau est large (± 0,01 sur une vingtaine de réglages) ; l'écart
/// restant tient au style des bassistes : choisir le meilleur réglage
/// morceau par morceau donnerait 85 à 92 %.
impl Default for Couts {
    fn default() -> Self {
        Self { corde: 0.6, deplacement: 1.0, pente_bas: 0.0, debut_haut: 9, pente_haut: 0.1, vide_haut: 0.1, corde_aigue: 0.3, position: 0.3 }
    }
}

impl Couts {
    /// Le prix d'une main en `position`.
    fn hauteur_main(&self, position: u8) -> f32 {
        self.pente_bas * position.saturating_sub(1) as f32
            + self.pente_haut * position.saturating_sub(self.debut_haut) as f32
    }

    /// Le prix propre d'une position jouée (corde, frette) sous une main,
    /// sur un manche de `cordes` cordes.
    fn jeu(&self, corde: u8, frette: u8, main: u8, cordes: usize) -> f32 {
        let vide = if frette == 0 { self.vide_haut * main.saturating_sub(1) as f32 } else { 0.0 };
        let depuis_aigue = cordes.saturating_sub(1 + corde as usize).min(positions::CORDES - 1);
        let appris = self.position * positions::POSITIONS[depuis_aigue][(frette as usize).min(positions::FRETTES - 1)];
        vide + self.corde_aigue * corde as f32 + self.hauteur_main(main) + appris
    }
}

/// Positions de main (frette de l'index) compatibles avec une note.
fn mains(frette: u8, max: u8) -> Vec<u8> {
    if frette == 0 {
        // Corde à vide : la main reste où elle est — toute position convient.
        (1..=max).collect()
    } else {
        (frette.saturating_sub(ETENDUE - 1).max(1)..=frette.min(max)).collect()
    }
}

/// Pose corde et frette sur chaque note. Une note hors du manche est ramenée
/// d'une ou plusieurs octaves (et sa hauteur corrigée en conséquence).
pub fn poser(notes: &mut [Note], accordage: &Accordage) {
    poser_avec(notes, accordage, &Couts::default());
}

/// [`poser`] avec d'autres coûts.
/// Rend le coût du doigté retenu (ce que [`Accordage::choisir`] compare).
pub fn poser_avec(notes: &mut [Note], accordage: &Accordage, couts_doigte: &Couts) -> f32 {
    if notes.is_empty() {
        return 0.0;
    }
    for n in notes.iter_mut() {
        n.hauteur = accordage.ramener(n.hauteur);
    }
    let max_main = accordage.frettes.saturating_sub(ETENDUE - 1).max(1);
    // États de chaque note : (corde, frette, main).
    let etats: Vec<Vec<(u8, u8, u8)>> = notes
        .iter()
        .map(|n| {
            accordage
                .positions(n.hauteur)
                .into_iter()
                .flat_map(|(c, f)| mains(f, max_main).into_iter().map(move |m| (c, f, m)))
                .collect()
        })
        .collect();

    let (total, choix) = chemin(notes, &etats, couts_doigte, accordage.cordes.len());
    for (n, (e, &j)) in notes.iter_mut().zip(etats.iter().zip(&choix)) {
        if let Some(&(c, f, _)) = e.get(j) {
            n.corde = Some(c);
            n.frette = Some(f);
        }
    }
    total
}

/// Le plus court chemin (Viterbi) sur les états (corde, frette, main) de
/// chaque note : son coût et l'état retenu pour chacune.
fn chemin(notes: &[Note], etats: &[Vec<(u8, u8, u8)>], k: &Couts, n_cordes: usize) -> (f32, Vec<usize>) {
    if notes.is_empty() {
        return (0.0, Vec::new());
    }
    let mut couts: Vec<Vec<f32>> = vec![etats[0].iter().map(|&(c, f, m)| k.jeu(c, f, m, n_cordes)).collect()];
    let mut retour: Vec<Vec<usize>> = vec![vec![0; etats[0].len()]];
    for i in 1..notes.len() {
        let silence = notes[i].debut_s - notes[i - 1].fin_s;
        // Le temps de se replacer : passé une demi-seconde, un déplacement
        // coûte de moins en moins.
        let repli = 1.0 / (1.0 + (silence - 0.5).max(0.0) * 2.0);
        let (mut c_i, mut r_i) = (Vec::with_capacity(etats[i].len()), Vec::with_capacity(etats[i].len()));
        for &(c2, f2, m2) in &etats[i] {
            let (meilleur, d_ou) = etats[i - 1]
                .iter()
                .enumerate()
                .map(|(j, &(c1, _, m1))| {
                    let main = k.deplacement * (m1 as f32 - m2 as f32).abs() * repli;
                    // Traverser les cordes ne coûte presque rien à un bassiste ;
                    // déplacer la main, si.
                    (couts[i - 1][j] + main + k.corde * (c1 as f32 - c2 as f32).abs() + k.jeu(c2, f2, m2, n_cordes), j)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap_or((0.0, 0));
            c_i.push(meilleur);
            r_i.push(d_ou);
        }
        couts.push(c_i);
        retour.push(r_i);
    }
    let dernier = couts.len() - 1;
    let mut e = (0..couts[dernier].len())
        .min_by(|&a, &b| couts[dernier][a].total_cmp(&couts[dernier][b]))
        .unwrap_or(0);
    let total = couts[dernier].get(e).copied().unwrap_or(0.0);
    let mut choix = vec![0; notes.len()];
    for i in (0..notes.len()).rev() {
        choix[i] = e;
        e = retour[i].get(e).copied().unwrap_or(0);
    }
    (total, choix)
}

/// **Un motif se joue toujours de la même façon.** Le chemin de [`poser`]
/// doigte chaque passage selon ce qui l'entoure : le riff d'« About Damn
/// Time » sortait tantôt en 7-5-7 sur la corde de la♭, tantôt en 2-0-2 sur
/// celle de ré♭ (29 oct.) — aucun bassiste ne fait ça : dans les 331
/// tablatures du banc des doigtés, 96,6 % des motifs répétés n'ont qu'un
/// doigté. Les mesures aux mêmes notes, aux mêmes places, prennent ici un
/// seul doigté : parmi ceux que le chemin leur a donnés, le plus fréquent ;
/// à égalité, celui qui coûte le moins sur l'ensemble de leurs occurrences,
/// entrée et sortie de chaque mesure comprises.
///
/// Banc des doigtés (29 oct.) : 79,3 % → 79,5 % de doigtés identiques à la
/// référence ; choisir d'abord le moins coûteux ne gagnait rien (79,3 %).
///
/// `premiers` : les premiers temps des mesures, en secondes. À appeler après
/// [`poser`], sur le même accordage.
pub fn uniformiser(notes: &mut [Note], accordage: &Accordage, k: &Couts, premiers: &[f32]) {
    if premiers.len() < 2 || notes.len() < 4 {
        return;
    }
    let duree_type = (premiers[premiers.len() - 1] - premiers[0]) / (premiers.len() - 1) as f32;
    // Les notes de chaque mesure : un intervalle d'indices (notes triées).
    let mut mesures: Vec<std::ops::Range<usize>> = Vec::new();
    let mut i = notes.partition_point(|n| n.debut_s < premiers[0]);
    for (m, &debut) in premiers.iter().enumerate() {
        let fin = premiers.get(m + 1).copied().unwrap_or(debut + duree_type);
        let j = i + notes[i..].partition_point(|n| n.debut_s < fin);
        mesures.push(i..j);
        i = j;
    }
    // Clé d'une mesure : chaque note, hauteur et place à la double croche ;
    // avec elle, les mesures qui la partagent.
    type Cle = Vec<(u8, i32)>;
    let mut groupes: Vec<(Cle, Vec<usize>)> = Vec::new();
    for (m, r) in mesures.iter().enumerate() {
        if r.len() < 2 {
            continue;
        }
        let debut = premiers[m];
        let duree = premiers.get(m + 1).map_or(duree_type, |f| f - debut).max(1e-3);
        let cle: Vec<(u8, i32)> =
            notes[r.clone()].iter().map(|n| (n.hauteur, ((n.debut_s - debut) / duree * 16.0).round() as i32)).collect();
        match groupes.iter_mut().find(|g| g.0 == cle) {
            Some(g) => g.1.push(m),
            None => groupes.push((cle, vec![m])),
        }
    }
    let max_main = accordage.frettes.saturating_sub(ETENDUE - 1).max(1);
    let n_cordes = accordage.cordes.len();
    let doigte = |notes: &[Note], r: &std::ops::Range<usize>| -> Vec<(u8, u8)> {
        notes[r.clone()].iter().map(|n| (n.corde.unwrap_or(0), n.frette.unwrap_or(0))).collect()
    };
    for (_, occurrences) in groupes.iter().filter(|g| g.1.len() > 1) {
        let mut candidats: Vec<(Vec<(u8, u8)>, usize)> = Vec::new();
        for &m in occurrences {
            let d = doigte(notes, &mesures[m]);
            match candidats.iter_mut().find(|c| c.0 == d) {
                Some(c) => c.1 += 1,
                None => candidats.push((d, 1)),
            }
        }
        if candidats.len() < 2 {
            continue;
        }
        // Le coût d'un doigté imposé à une occurrence, avec la note d'avant et
        // celle d'après telles qu'elles sont posées : seule la main est libre.
        let cout = |notes: &[Note], m: usize, d: &[(u8, u8)]| -> f32 {
            let r = &mesures[m];
            let de = r.start.saturating_sub(1);
            let a = (r.end + 1).min(notes.len());
            let etats: Vec<Vec<(u8, u8, u8)>> = (de..a)
                .map(|i| {
                    let (c, f) = if r.contains(&i) { d[i - r.start] } else { (notes[i].corde.unwrap_or(0), notes[i].frette.unwrap_or(0)) };
                    mains(f, max_main).into_iter().map(|main| (c, f, main)).collect()
                })
                .collect();
            chemin(&notes[de..a], &etats, k, n_cordes).0
        };
        // Le plus fréquent ; à égalité, le moins coûteux sur toutes les
        // occurrences, puis le premier rencontré.
        let meilleur = candidats
            .iter()
            .map(|(d, n)| (occurrences.iter().map(|&m| cout(notes, m, d)).sum::<f32>(), *n, d))
            .min_by(|a, b| b.1.cmp(&a.1).then(a.0.total_cmp(&b.0)))
            .map(|(_, _, d)| d.clone())
            .expect("au moins deux candidats");
        for &m in occurrences {
            for (n, &(c, f)) in notes[mesures[m].clone()].iter_mut().zip(&meilleur) {
                (n.corde, n.frette) = (Some(c), Some(f));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(debut: f32, hauteur: u8) -> Note {
        Note { debut_s: debut, fin_s: debut + 0.2, hauteur, amplitude: 0.7, corde: None, frette: None }
    }

    #[test]
    fn une_gamme_reste_dans_une_position() {
        // Sol majeur depuis sol1 (MIDI 31) : la main reste en
        // troisième position, en traversant les cordes.
        let mut n: Vec<Note> = [31u8, 33, 35, 36, 38, 40, 42, 43].iter().enumerate().map(|(i, &h)| note(i as f32 * 0.25, h)).collect();
        poser(&mut n, &Accordage::basse4());
        let frettes: Vec<u8> = n.iter().map(|x| x.frette.unwrap()).collect();
        let max = *frettes.iter().filter(|&&f| f > 0).max().unwrap();
        let min = *frettes.iter().filter(|&&f| f > 0).min().unwrap();
        assert!(max - min <= 4, "la main tient en quatre frettes : {frettes:?}");
    }

    #[test]
    fn hors_du_manche_on_ramene_a_l_octave() {
        let mut n = vec![note(0.0, 16)]; // mi0, sous la corde grave
        poser(&mut n, &Accordage::basse4());
        assert_eq!(n[0].hauteur, 28);
        assert_eq!((n[0].corde, n[0].frette), (Some(0), Some(0)));
    }

    #[test]
    fn l_octave_d_une_corde_a_vide_reste_sous_la_main() {
        // Mi1 à vide puis mi2 : la forme d'octave (corde de ré, 2ᵉ case) ou
        // la 7ᵉ case de la corde de la — jamais la 12ᵉ case de la corde de mi.
        // Les deux se lisent dans les tablatures de référence ; l'a priori
        // appris (17 oct.) préfère la corde de la.
        let mut n = vec![note(0.0, 28), note(0.3, 40)];
        poser(&mut n, &Accordage::basse4());
        assert!(matches!((n[1].corde, n[1].frette), (Some(1), Some(7)) | (Some(2), Some(2))), "{:?}", (n[1].corde, n[1].frette));
    }

    #[test]
    fn un_riff_repete_se_doigte_pareil() {
        // Sol3, fa3, sol2, quatre fois (Flea, « Parallel Universe ») : les
        // reprises du milieu, loin des bords du chemin, sont identiques.
        let mut n: Vec<Note> = [55u8, 53, 43].repeat(4).iter().enumerate().map(|(i, &h)| note(i as f32 * 0.2, h)).collect();
        poser(&mut n, &Accordage::basse4());
        let d: Vec<_> = n.iter().map(|x| (x.corde, x.frette)).collect();
        assert_eq!(d[3..6], d[6..9], "{d:?}");
    }

    #[test]
    fn la_main_ne_bouge_pas_autour_d_une_corde_a_vide() {
        // Sol1 (mi, 3ᵉ frette), mi1 à vide, si1 : la main reste en première
        // position — si1 en 2ᵉ frette de la, pas en 7ᵉ de mi.
        let mut n = vec![note(0.0, 31), note(0.3, 28), note(0.6, 35)];
        poser(&mut n, &Accordage::basse4());
        assert_eq!((n[2].corde, n[2].frette), (Some(1), Some(2)));
    }

    #[test]
    fn une_ligne_haute_reste_en_position() {
        // Une phrase en 7ᵉ position (la2 si2 do3 ré3) reste sous la main :
        // pas d'allers-retours vers les cordes à vide.
        let mut n: Vec<Note> = [45u8, 47, 48, 50, 48, 47].iter().enumerate().map(|(i, &h)| note(i as f32 * 0.2, h)).collect();
        poser(&mut n, &Accordage::basse4());
        let f: Vec<u8> = n.iter().map(|x| x.frette.unwrap()).collect();
        let (mn, mx) = (*f.iter().min().unwrap(), *f.iter().max().unwrap());
        assert!(mx - mn <= 3, "tient sous la main : {f:?}");
    }

    #[test]
    fn un_morceau_accorde_un_demi_ton_plus_bas_est_reconnu() {
        // Mi♭0 à vide, la♭0, ré♭1 : les cordes à vide d'une basse en mi♭.
        let n: Vec<Note> = (0..48).map(|i| note(i as f32 * 0.3, [27u8, 27, 32, 37, 34, 27][i % 6])).collect();
        assert_eq!(Accordage::choisir(&n).cordes, vec![27, 32, 37, 42]);
    }

    #[test]
    fn des_re_graves_reclament_le_drop_d() {
        // Ré0 à vide, la0 et ré1 à vide : drop D plutôt que ré standard.
        let mut n: Vec<Note> = (0..40).map(|i| note(i as f32 * 0.3, [26u8, 33, 38, 26, 45][i % 5])).collect();
        let a = Accordage::choisir(&n);
        assert_eq!(a, Accordage::drop_d());
        poser(&mut n, &a);
        assert_eq!((n[0].hauteur, n[0].corde, n[0].frette), (26, Some(0), Some(0)));
        // Quelques notes graves isolées (erreurs) ne changent pas l'accordage.
        let m: Vec<Note> = (0..100).map(|i| note(i as f32 * 0.3, if i == 5 { 26 } else { 40 })).collect();
        assert_eq!(Accordage::choisir(&m), Accordage::basse4());
    }

    #[test]
    fn un_motif_se_doigte_toujours_pareil() {
        // Le riff d'« About Damn Time », en mi♭ : mi♭ mi♭ ré♭ sol♭ sol♭ fa |
        // mi♭ ré♭ mi♭. Suivi tantôt du mi♭ grave à vide, tantôt d'une phrase
        // aiguë : la fin de mesure était tirée de deux côtés.
        let riff = [(0.0, 27u8), (0.5, 27), (0.875, 37), (1.0, 42), (1.125, 42), (1.25, 41), (1.5, 39), (1.75, 37), (1.875, 39)];
        let mut n = Vec::new();
        for m in 0..6 {
            let t0 = m as f32 * 4.0;
            n.extend(riff.iter().map(|&(t, h)| note(t0 + t, h)));
            let suite: &[u8] = if m % 2 == 0 { &[27, 27, 27, 27] } else { &[51, 53, 54, 56] };
            n.extend(suite.iter().enumerate().map(|(i, &h)| note(t0 + 2.0 + i as f32 * 0.5, h)));
        }
        let premiers: Vec<f32> = (0..12).map(|m| m as f32 * 2.0).collect();
        let a = Accordage { cordes: vec![27, 32, 37, 42], frettes: 20 };
        let doigtes = |n: &[Note]| -> Vec<Vec<_>> { n.chunks(13).map(|c| c[..9].iter().map(|x| (x.corde, x.frette)).collect()).collect() };
        poser(&mut n, &a);
        let avant = doigtes(&n);
        uniformiser(&mut n, &a, &Couts::default(), &premiers);
        let apres = doigtes(&n);
        assert!(avant.windows(2).any(|w| w[0] != w[1]), "le cas ne teste rien : {avant:?}");
        assert!(apres.windows(2).all(|w| w[0] == w[1]), "{apres:?}");
    }

    #[test]
    fn quatre_ou_cinq_cordes_dans_l_accordage_du_disque() {
        // Mi♭0 à vide, la♭0, ré♭1 : un disque en mi♭.
        let grave: Vec<Note> = (0..48).map(|i| note(i as f32 * 0.3, [27u8, 27, 32, 37, 34, 27][i % 6])).collect();
        assert_eq!(Accordage::pour(&grave, Basses::Quatre).cordes, vec![27, 32, 37, 42]);
        // La 5 cordes standard le contient (corde de si, 4ᵉ case).
        assert_eq!(Accordage::pour(&grave, Basses::Cinq), Accordage::basse5());
        let mi: Vec<Note> = (0..40).map(|i| note(i as f32 * 0.3, [28u8, 40, 43][i % 3])).collect();
        assert_eq!(Accordage::pour(&mi, Basses::Quatre), Accordage::basse4());
        assert_eq!(Accordage::pour(&mi, Basses::Cinq), Accordage::basse5());
    }

    #[test]
    fn la_cinquieme_corde_sert_les_graves() {
        let mut n = vec![note(0.0, 23)]; // si0
        poser(&mut n, &Accordage::basse5());
        assert_eq!((n[0].corde, n[0].frette), (Some(0), Some(0)));
    }
}
