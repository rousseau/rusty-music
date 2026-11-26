// SPDX-License-Identifier: GPL-3.0-or-later
//! Guitare : des accords sur six cordes. Là où la basse joue une note à la
//! fois ([`crate::tablature`]), la guitare plaque des accords : chaque groupe
//! de notes attaquées ensemble reçoit un doigté entier — une note par corde,
//! les frettes sous la main —, et le chemin entre doigtés successifs se
//! cherche comme pour la basse, par plus court chemin (Viterbi).
//!
//! Coût d'un doigté :
//! - l'écart de la main (frettes appuyées les plus éloignées), qui ne dépasse
//!   pas quatre frettes ([`ECART_MAX`]) ;
//! - une main haute sur le manche, au-delà de la 7ᵉ position ;
//! - un peu chaque note frettée plutôt qu'à vide : les accords ouverts
//!   d'abord, comme en joue un guitariste rythmique.
//!
//! Coût d'un passage : le déplacement de la main (sa frette la plus basse),
//! moitié moins cher après un silence. Les cordes à vide ne déplacent pas la
//! main : un doigté tout à vide garde la position de celui qui le précède sur
//! le chemin, sans quoi une corde à vide « libérait » tout (le défaut corrigé
//! sur la basse le 9 oct.).
//!
//! Premier jet (26 nov.), sans banc : à régler contre des tablatures
//! publiées comme l'a été celui de la basse.

use crate::Note;

/// Accordage standard, de la corde grave (mi2) à l'aiguë (mi4), en MIDI.
pub const STANDARD: [u8; 6] = [40, 45, 50, 55, 59, 64];
/// Frettes du manche.
pub const FRETTES: u8 = 22;
/// Notes attaquées à moins de cet écart : un même accord (un grattage n'est
/// pas instantané, d'une corde à l'autre).
pub const ECART_ACCORD_S: f32 = 0.04;
/// Écart maximal entre frettes appuyées d'un même doigté.
pub const ECART_MAX: u8 = 4;
/// Doigtés gardés par accord, les moins chers.
const CANDIDATS: usize = 48;

/// Les accords d'une suite de notes triées par début : les rangs des notes
/// attaquées ensemble, du grave à l'aigu.
fn accords(notes: &[Note]) -> Vec<Vec<usize>> {
    let mut groupes: Vec<Vec<usize>> = Vec::new();
    let mut debut = f32::NEG_INFINITY;
    for (i, n) in notes.iter().enumerate() {
        if n.debut_s - debut <= ECART_ACCORD_S && !groupes.is_empty() {
            groupes.last_mut().expect("non vide").push(i);
        } else {
            debut = n.debut_s;
            groupes.push(vec![i]);
        }
    }
    for g in &mut groupes {
        g.sort_by_key(|&i| notes[i].hauteur);
    }
    groupes
}

/// Ramène une hauteur sur le manche, à l'octave près.
fn sur_le_manche(mut h: u8, accordage: &[u8; 6]) -> u8 {
    let (bas, haut) = (accordage[0], accordage[5] + FRETTES);
    while h < bas {
        h += 12;
    }
    while h > haut {
        h -= 12;
    }
    h
}

/// Tous les doigtés d'un accord (hauteurs du grave à l'aigu) : une corde par
/// note, frettes appuyées dans [`ECART_MAX`]. Chaque doigté : `(corde,
/// frette)` au rang de sa note.
fn doigtes(hauteurs: &[u8], accordage: &[u8; 6]) -> Vec<Vec<(u8, u8)>> {
    fn chercher(
        k: usize,
        hauteurs: &[u8],
        accordage: &[u8; 6],
        prises: &mut [bool; 6],
        courant: &mut Vec<(u8, u8)>,
        sortie: &mut Vec<Vec<(u8, u8)>>,
    ) {
        if sortie.len() >= 4096 {
            return;
        }
        if k == hauteurs.len() {
            sortie.push(courant.clone());
            return;
        }
        for c in 0..6 {
            if prises[c] || hauteurs[k] < accordage[c] {
                continue;
            }
            let f = hauteurs[k] - accordage[c];
            if f > FRETTES {
                continue;
            }
            let appuyees = courant.iter().map(|p| p.1).chain([f]).filter(|&f| f > 0);
            let (min, max) = appuyees.fold((u8::MAX, 0), |(a, b), f| (a.min(f), b.max(f)));
            if max > 0 && max - min > ECART_MAX {
                continue;
            }
            prises[c] = true;
            courant.push((c as u8, f));
            chercher(k + 1, hauteurs, accordage, prises, courant, sortie);
            courant.pop();
            prises[c] = false;
        }
    }
    let mut sortie = Vec::new();
    chercher(
        0,
        hauteurs,
        accordage,
        &mut [false; 6],
        &mut Vec::new(),
        &mut sortie,
    );
    sortie
}

/// La frette la plus basse appuyée : la position de la main ; `None` : tout
/// à vide.
fn main(d: &[(u8, u8)]) -> Option<u8> {
    d.iter().map(|p| p.1).filter(|&f| f > 0).min()
}

fn cout_propre(d: &[(u8, u8)]) -> f32 {
    let appuyees: Vec<u8> = d.iter().map(|p| p.1).filter(|&f| f > 0).collect();
    let ecart = match (appuyees.iter().min(), appuyees.iter().max()) {
        (Some(a), Some(b)) => (b - a) as f32,
        _ => 0.0,
    };
    let haute = main(d).map_or(0.0, |m| m.saturating_sub(7) as f32);
    0.3 * ecart + 0.3 * haute + 0.1 * appuyees.len() as f32
}

/// Prépare les notes d'un stem de guitare : ramenées sur le manche à
/// l'octave, triées, sans doublon dans un accord, et chaque accord réduit
/// jusqu'à avoir un doigté (on retire ses notes les plus faibles, puis les
/// plus aiguës : au plus six, une par corde).
pub fn preparer(mut notes: Vec<Note>, accordage: &[u8; 6]) -> Vec<Note> {
    for n in &mut notes {
        n.hauteur = sur_le_manche(n.hauteur, accordage);
    }
    notes.sort_by(|a, b| a.debut_s.total_cmp(&b.debut_s));
    let mut gardees = Vec::with_capacity(notes.len());
    for g in accords(&notes) {
        let mut accord: Vec<Note> = Vec::with_capacity(g.len());
        for i in g {
            let n = &notes[i];
            match accord.iter_mut().find(|a| a.hauteur == n.hauteur) {
                Some(a) => a.fin_s = a.fin_s.max(n.fin_s),
                None => accord.push(n.clone()),
            }
        }
        while accord.len() > 1 {
            let h: Vec<u8> = accord.iter().map(|n| n.hauteur).collect();
            if accord.len() <= 6 && !doigtes(&h, accordage).is_empty() {
                break;
            }
            // La plus faible, et à force égale la plus aiguë (l'harmonique).
            let i = (0..accord.len())
                .min_by(|&a, &b| {
                    accord[a]
                        .amplitude
                        .total_cmp(&accord[b].amplitude)
                        .then(accord[b].hauteur.cmp(&accord[a].hauteur))
                })
                .expect("non vide");
            accord.remove(i);
        }
        gardees.extend(accord);
    }
    gardees
}

/// Pose corde et frette sur des notes [`preparer`]ées : le chemin de doigtés
/// le moins cher sur tout le morceau.
pub fn poser(notes: &mut [Note], accordage: &[u8; 6]) {
    let groupes = accords(notes);
    if groupes.is_empty() {
        return;
    }
    let candidats: Vec<Vec<Vec<(u8, u8)>>> = groupes
        .iter()
        .map(|g| {
            let h: Vec<u8> = g.iter().map(|&i| notes[i].hauteur).collect();
            let mut c = doigtes(&h, accordage);
            c.sort_by(|a, b| cout_propre(a).total_cmp(&cout_propre(b)));
            c.truncate(CANDIDATS);
            c
        })
        .collect();
    // Viterbi : coût cumulé et prédécesseur de chaque candidat.
    let mut couts: Vec<Vec<f32>> = Vec::with_capacity(groupes.len());
    let mut retours: Vec<Vec<usize>> = Vec::with_capacity(groupes.len());
    // La position de la main après chaque candidat, sur son meilleur chemin.
    let mut mains: Vec<Vec<Option<u8>>> = Vec::with_capacity(groupes.len());
    couts.push(candidats[0].iter().map(|d| cout_propre(d)).collect());
    retours.push(vec![0; candidats[0].len()]);
    mains.push(candidats[0].iter().map(|d| main(d)).collect());
    for k in 1..groupes.len() {
        let fin_prec = groupes[k - 1]
            .iter()
            .map(|&i| notes[i].fin_s)
            .fold(f32::NEG_INFINITY, f32::max);
        let silence = notes[groupes[k][0]].debut_s - fin_prec > 0.25;
        let poids = if silence { 0.5 } else { 1.0 };
        let mut c_k = Vec::with_capacity(candidats[k].len());
        let mut r_k = Vec::with_capacity(candidats[k].len());
        let mut m_k = Vec::with_capacity(candidats[k].len());
        for d in &candidats[k] {
            let (mut meilleur, mut j_meilleur) = (f32::INFINITY, 0);
            for j in 0..candidats[k - 1].len() {
                let saut = match (mains[k - 1][j], main(d)) {
                    (Some(a), Some(b)) => (a as f32 - b as f32).abs(),
                    _ => 0.0,
                };
                let c = couts[k - 1][j] + poids * saut;
                if c < meilleur {
                    (meilleur, j_meilleur) = (c, j);
                }
            }
            c_k.push(meilleur + cout_propre(d));
            r_k.push(j_meilleur);
            m_k.push(main(d).or(mains[k - 1].get(j_meilleur).copied().flatten()));
        }
        couts.push(c_k);
        retours.push(r_k);
        mains.push(m_k);
    }
    let dernier = couts.last().expect("non vide");
    let mut j = (0..dernier.len())
        .min_by(|&a, &b| dernier[a].total_cmp(&dernier[b]))
        .unwrap_or(0);
    for k in (0..groupes.len()).rev() {
        if let Some(d) = candidats[k].get(j) {
            for (&i, &(corde, frette)) in groupes[k].iter().zip(d) {
                notes[i].corde = Some(corde);
                notes[i].frette = Some(frette);
            }
        }
        j = retours[k].get(j).copied().unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(debut: f32, hauteur: u8) -> Note {
        Note {
            debut_s: debut,
            fin_s: debut + 0.4,
            hauteur,
            amplitude: 1.0,
            corde: None,
            frette: None,
        }
    }

    #[test]
    fn un_accord_de_mi_majeur_se_joue_ouvert() {
        // Mi majeur : mi2 si2 mi3 sol#3 si3 mi4.
        let n: Vec<Note> = [40, 47, 52, 56, 59, 64]
            .iter()
            .map(|&h| note(0.0, h))
            .collect();
        let mut n = preparer(n, &STANDARD);
        poser(&mut n, &STANDARD);
        let frettes: Vec<u8> = n.iter().map(|n| n.frette.unwrap()).collect();
        assert_eq!(frettes, [0, 2, 2, 1, 0, 0]);
        let cordes: Vec<u8> = n.iter().map(|n| n.corde.unwrap()).collect();
        assert_eq!(cordes, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn un_accord_trop_large_perd_ses_notes_les_plus_faibles() {
        let mut n: Vec<Note> = (0..8).map(|k| note(0.0, 40 + 5 * k)).collect();
        n[7].amplitude = 0.2;
        let n = preparer(n, &STANDARD);
        assert!(n.len() <= 6);
        assert!(n.iter().all(|x| x.hauteur != 75));
    }

    #[test]
    fn hors_du_manche_ramene_a_l_octave() {
        let n = preparer(vec![note(0.0, 30), note(1.0, 100)], &STANDARD);
        assert_eq!(n[0].hauteur, 42);
        assert!(n[1].hauteur <= 64 + FRETTES && n[1].hauteur % 12 == 100 % 12);
    }

    #[test]
    fn une_ligne_reste_sous_la_main() {
        // Gamme de la mineur pentatonique en 5e position : la main ne
        // devrait pas courir le manche.
        let h = [45, 48, 50, 52, 55, 57, 60, 62, 64, 67, 69];
        let n: Vec<Note> = h
            .iter()
            .enumerate()
            .map(|(k, &h)| note(k as f32 * 0.25, h))
            .collect();
        let mut n = preparer(n, &STANDARD);
        poser(&mut n, &STANDARD);
        let appuyees: Vec<u8> = n
            .iter()
            .filter_map(|n| n.frette)
            .filter(|&f| f > 0)
            .collect();
        let (min, max) = (
            appuyees.iter().min().unwrap(),
            appuyees.iter().max().unwrap(),
        );
        assert!(max - min <= 5, "frettes {appuyees:?}");
    }
}
