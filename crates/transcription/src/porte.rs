// SPDX-License-Identifier: GPL-3.0-or-later
//! Porte d'énergie : pas de note là où le stem se tait.
//!
//! Basic Pitch ignore le niveau : il transcrit aussi bien la fuite d'une
//! guitare, 60 dB sous la basse, que la basse elle-même. Sur « Under the
//! Bridge » (banc contre la partition publiée, 10 oct.), l'introduction et le
//! premier couplet — basse tacet, stem entre −60 et −70 dB sous son niveau de
//! jeu — recevaient une centaine de notes. On garde une note si le stem, au
//! début de la note, atteint un niveau proche de celui où il joue.

use crate::Note;

/// Trames d'énergie : 20 ms.
const TRAME_S: f32 = 0.02;
/// Le début d'une note : ce qu'on écoute pour décider.
const ATTAQUE_S: f32 = 0.1;

/// Le seuil retenu, en dB sous le niveau de jeu du stem.
pub const SEUIL_DB: f32 = 30.0;

/// Énergie (RMS) par trame de 20 ms.
fn energies(mono: &[f32], sr: u32) -> Vec<f32> {
    let n = ((sr as f32 * TRAME_S) as usize).max(1);
    mono.chunks(n).map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt()).collect()
}

/// Niveau de jeu : le 95ᵉ centile des trames — robuste aux silences, qui
/// peuvent occuper la moitié d'un morceau.
fn niveau_de_jeu(e: &[f32]) -> f32 {
    if e.is_empty() {
        return 0.0;
    }
    let mut v = e.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * 0.95) as usize]
}

/// Retire les notes dont l'attaque reste à plus de `seuil_db` sous le niveau
/// de jeu du stem.
pub fn filtrer(notes: Vec<Note>, mono: &[f32], sr: u32, seuil_db: f32) -> Vec<Note> {
    let e = energies(mono, sr);
    let seuil = niveau_de_jeu(&e) * 10f32.powf(-seuil_db / 20.0);
    notes
        .into_iter()
        .filter(|n| {
            let a = (n.debut_s / TRAME_S).max(0.0) as usize;
            let b = (((n.debut_s + ATTAQUE_S.min((n.fin_s - n.debut_s).max(TRAME_S))) / TRAME_S).ceil() as usize).min(e.len());
            e.get(a..b.max(a + 1).min(e.len())).is_some_and(|f| f.iter().any(|&x| x >= seuil))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(debut: f32) -> Note {
        Note { debut_s: debut, fin_s: debut + 0.3, hauteur: 40, amplitude: 0.5, corde: None, frette: None }
    }

    #[test]
    fn une_fuite_lointaine_est_retiree() {
        let sr = 1000;
        // 2 s de fuite à −60 dB, puis 2 s de basse.
        let mut mono: Vec<f32> = (0..2000).map(|i| 0.001 * (i as f32 * 0.3).sin()).collect();
        mono.extend((0..2000).map(|i| (i as f32 * 0.3).sin()));
        let gardees = filtrer(vec![note(0.5), note(1.2), note(2.5), note(3.1)], &mono, sr, SEUIL_DB);
        let debuts: Vec<f32> = gardees.iter().map(|n| n.debut_s).collect();
        assert_eq!(debuts, vec![2.5, 3.1]);
    }

    #[test]
    fn un_jeu_doux_reste() {
        let sr = 1000;
        // Un passage joué 20 dB plus doux que le reste : ce n'est pas un silence.
        let mut mono: Vec<f32> = (0..2000).map(|i| 0.1 * (i as f32 * 0.3).sin()).collect();
        mono.extend((0..6000).map(|i| (i as f32 * 0.3).sin()));
        assert_eq!(filtrer(vec![note(0.5), note(4.0)], &mono, sr, SEUIL_DB).len(), 2);
    }
}
