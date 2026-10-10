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
//! - une main haute sur le manche : un peu dès la 1ʳᵉ position, nettement
//!   au-delà de la 5ᵉ.
//!
//! Version précédente (9 oct.) : un coût entre positions successives, sans
//! mémoire de la main. Une corde à vide « libérait » tout, et la main pouvait
//! sauter d'un bout du manche à l'autre autour d'elle — les doigtés
//! incohérents signalés à l'essai.

use crate::Note;

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

    /// L'accordage que les notes réclament. Une basse standard ne descend pas
    /// sous le mi0 ; si une part des notes tombe sous lui, le morceau est en
    /// drop D (ré0, ré♯0) ou joué sur une cinq cordes (jusqu'au si0). Sans ça,
    /// ces notes remontaient d'une octave (« Scar Tissue », en drop D :
    /// 83 notes une octave trop haut au banc du 10 oct.).
    pub fn choisir(notes: &[Note]) -> Self {
        let seuil = (notes.len() / 50).max(4);
        let sous = |a: u8, b: u8| notes.iter().filter(|n| (a..=b).contains(&n.hauteur)).count();
        if sous(23, 25) >= seuil {
            Self::basse5()
        } else if sous(26, 27) >= seuil {
            Self::drop_d()
        } else {
            Self::basse4()
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
}

/// Réglé le 10 oct. contre deux tablatures de référence : doigtés identiques
/// de 64 à 74 % en moyenne. Le premier réglage (corde 0,1, préférence pour le
/// bas du manche) traversait les cordes trop volontiers ; les bassistes de
/// référence restent sur une corde et dans une position (« Love
/// Foolosophy » : 378 notes sur 615 sur la corde de mi, en 7ᵉ position).
///
/// Puis contre les 15 tablatures du livre *Californication* (Flea,
/// `experiments/partitions/regler_doigtes.py`) : une corde à vide quand la
/// main est haute coûte (`vide_haut` 0,3) — Flea joue le sol en 10ᵉ case de
/// la corde de la plutôt que la corde de sol à vide. Doigtés identiques de
/// 51 à 61 % des notes ; la préférence pour les cordes graves n'apporte rien.
impl Default for Couts {
    fn default() -> Self {
        Self { corde: 0.6, deplacement: 1.0, pente_bas: 0.0, debut_haut: 12, pente_haut: 0.0, vide_haut: 0.3, corde_aigue: 0.0 }
    }
}

impl Couts {
    /// Le prix d'une main en `position`.
    fn hauteur_main(&self, position: u8) -> f32 {
        self.pente_bas * position.saturating_sub(1) as f32
            + self.pente_haut * position.saturating_sub(self.debut_haut) as f32
    }

    /// Le prix propre d'une position jouée (corde, frette) sous une main.
    fn jeu(&self, corde: u8, frette: u8, main: u8) -> f32 {
        let vide = if frette == 0 { self.vide_haut * main.saturating_sub(1) as f32 } else { 0.0 };
        vide + self.corde_aigue * corde as f32 + self.hauteur_main(main)
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
pub fn poser_avec(notes: &mut [Note], accordage: &Accordage, couts_doigte: &Couts) {
    if notes.is_empty() {
        return;
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

    let k = couts_doigte;
    let mut couts: Vec<Vec<f32>> = vec![etats[0].iter().map(|&(c, f, m)| k.jeu(c, f, m)).collect()];
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
                    (couts[i - 1][j] + main + k.corde * (c1 as f32 - c2 as f32).abs() + k.jeu(c2, f2, m2), j)
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
    for i in (0..notes.len()).rev() {
        if let Some(&(c, f, _)) = etats[i].get(e) {
            notes[i].corde = Some(c);
            notes[i].frette = Some(f);
        }
        e = retour[i].get(e).copied().unwrap_or(0);
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
    fn l_octave_d_une_corde_a_vide_se_prend_deux_cordes_plus_haut() {
        // Mi1 à vide puis mi2 : la forme d'octave (corde de ré, 2ᵉ case),
        // pas la 12ᵉ case de la corde de mi — une corde à vide sous une main
        // en 12ᵉ position coûte (`vide_haut`, réglage du 10 oct.).
        let mut n = vec![note(0.0, 28), note(0.3, 40)];
        poser(&mut n, &Accordage::basse4());
        assert_eq!((n[1].corde, n[1].frette), (Some(2), Some(2)));
    }

    #[test]
    fn en_position_haute_on_evite_la_corde_a_vide() {
        // Une phrase en 10ᵉ position (sol3, fa3) qui redescend sur sol2 : la
        // corde de la en 10ᵉ case, sous la main, plutôt que la corde de sol à
        // vide (Flea, « Parallel Universe »).
        let mut n: Vec<Note> = [55u8, 53, 43, 55, 53, 43].iter().enumerate().map(|(i, &h)| note(i as f32 * 0.2, h)).collect();
        poser(&mut n, &Accordage::basse4());
        assert!(n.iter().all(|x| x.frette != Some(0)), "{:?}", n.iter().map(|x| (x.corde, x.frette)).collect::<Vec<_>>());
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
    fn des_re_graves_reclament_le_drop_d() {
        let mut n: Vec<Note> = (0..40).map(|i| note(i as f32 * 0.3, if i % 4 == 0 { 26 } else { 38 })).collect();
        let a = Accordage::choisir(&n);
        assert_eq!(a, Accordage::drop_d());
        poser(&mut n, &a);
        assert_eq!((n[0].hauteur, n[0].corde, n[0].frette), (26, Some(0), Some(0)));
        // Quelques notes graves isolées (erreurs) ne changent pas l'accordage.
        let m: Vec<Note> = (0..100).map(|i| note(i as f32 * 0.3, if i == 5 { 26 } else { 40 })).collect();
        assert_eq!(Accordage::choisir(&m), Accordage::basse4());
    }

    #[test]
    fn la_cinquieme_corde_sert_les_graves() {
        let mut n = vec![note(0.0, 23)]; // si0
        poser(&mut n, &Accordage::basse5());
        assert_eq!((n[0].corde, n[0].frette), (Some(0), Some(0)));
    }
}
