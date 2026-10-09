// SPDX-License-Identifier: GPL-3.0-or-later
//! Corde et frette de chaque note. Une même hauteur se joue à plusieurs
//! endroits du manche ; le bon choix est celui qui économise la main. C'est un
//! plus court chemin sur la suite des notes (Viterbi), comme le recommande la
//! littérature sur la tablature d'une ligne monophonique
//! (`docs/recherche-editer-pratique-creation.md`).
//!
//! Le coût d'un passage d'une position à la suivante :
//! - le déplacement de la main, en frettes — mais une corde à vide ne
//!   déplace rien, et un long silence laisse le temps de se replacer ;
//! - un changement de corde, un peu ;
//! - les positions hautes, au-delà de la 4ᵉ frette : à jouabilité égale, on
//!   reste en bas du manche. Sans ce terme (0,05 par frette au départ), la
//!   transcription de « Love Foolosophy » sautait à la 12ᵉ frette de la corde
//!   de mi plutôt que de jouer la même note en 2ᵉ frette de ré.

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

/// Le prix d'une position haute : rien jusqu'à la 4ᵉ frette.
fn hauteur_manche(frette: u8) -> f32 {
    0.1 * frette.saturating_sub(4) as f32
}

fn cout(prec: (u8, u8), suiv: (u8, u8), silence_s: f32) -> f32 {
    let (c1, f1) = prec;
    let (c2, f2) = suiv;
    // Une corde à vide ne dit rien de la position de la main.
    let main = if f1 == 0 || f2 == 0 { 0.5 } else { (f1 as f32 - f2 as f32).abs() };
    // Le temps de se replacer : passé une demi-seconde, le déplacement coûte
    // de moins en moins.
    let repli = 1.0 / (1.0 + (silence_s - 0.5).max(0.0) * 2.0);
    main * repli + 0.2 * (c1 as f32 - c2 as f32).abs() + hauteur_manche(f2)
}

/// Pose corde et frette sur chaque note. Une note hors du manche est ramenée
/// d'une ou plusieurs octaves (et sa hauteur corrigée en conséquence).
pub fn poser(notes: &mut [Note], accordage: &Accordage) {
    if notes.is_empty() {
        return;
    }
    for n in notes.iter_mut() {
        n.hauteur = accordage.ramener(n.hauteur);
    }
    let candidats: Vec<Vec<(u8, u8)>> = notes.iter().map(|n| accordage.positions(n.hauteur)).collect();

    // Viterbi : meilleur coût pour finir à chaque candidat, et d'où l'on vient.
    let mut couts: Vec<Vec<f32>> = vec![candidats[0].iter().map(|&(_, f)| hauteur_manche(f)).collect()];
    let mut retour: Vec<Vec<usize>> = vec![vec![0; candidats[0].len()]];
    for i in 1..notes.len() {
        let silence = notes[i].debut_s - notes[i - 1].fin_s;
        let mut c_i = Vec::with_capacity(candidats[i].len());
        let mut r_i = Vec::with_capacity(candidats[i].len());
        for &suiv in &candidats[i] {
            let (meilleur, d_ou) = candidats[i - 1]
                .iter()
                .enumerate()
                .map(|(k, &prec)| (couts[i - 1][k] + cout(prec, suiv, silence), k))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap_or((0.0, 0));
            c_i.push(meilleur);
            r_i.push(d_ou);
        }
        couts.push(c_i);
        retour.push(r_i);
    }
    let dernier = couts.len() - 1;
    let mut k = (0..couts[dernier].len())
        .min_by(|&a, &b| couts[dernier][a].total_cmp(&couts[dernier][b]))
        .unwrap_or(0);
    for i in (0..notes.len()).rev() {
        if let Some(&(c, f)) = candidats[i].get(k) {
            notes[i].corde = Some(c);
            notes[i].frette = Some(f);
        }
        k = retour[i].get(k).copied().unwrap_or(0);
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
    fn apres_une_corde_a_vide_on_ne_monte_pas_au_douzieme() {
        // Mi1 à vide puis mi2 : deuxième frette de ré, pas douzième de mi.
        let mut n = vec![note(0.0, 28), note(0.3, 40)];
        poser(&mut n, &Accordage::basse4());
        assert_eq!((n[1].corde, n[1].frette), (Some(2), Some(2)));
    }

    #[test]
    fn la_cinquieme_corde_sert_les_graves() {
        let mut n = vec![note(0.0, 23)]; // si0
        poser(&mut n, &Accordage::basse5());
        assert_eq!((n[0].corde, n[0].frette), (Some(0), Some(0)));
    }
}
