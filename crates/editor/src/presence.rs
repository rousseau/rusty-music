// SPDX-License-Identifier: GPL-3.0-or-later
//! Les stems presque vides (V38). BS-RoFormer SW sépare toujours six stems ;
//! sur un groupe sans clavier, « piano » et souvent « autres » ne reçoivent
//! presque rien, et encombrent la pile de l'établi. On mesure la part
//! d'énergie de chaque stem dans le mélange (la somme des stems, qui le
//! reconstitue à 26-34 dB près), sur les seuls passages où le mélange sonne.
//!
//! Banc du 12 nov. (`docs/journal.md`, « V38 ») : sur 14 morceaux démixés,
//! les stems vides tombent sous −36 dB (souvent −100 dB), les vrais restent
//! au-dessus de −27 dB (l'orgue discret de *Highway Star* dans « autres ») :
//! [`SEUIL_PRESQUE_VIDE_DB`] = −30 dB, au milieu de l'écart.

/// Sous cette part d'énergie (dB, relative au mélange), un stem est presque
/// vide.
pub const SEUIL_PRESQUE_VIDE_DB: f32 = -30.0;

/// Une trame de mesure : une demi-seconde.
const TRAME_S: f32 = 0.5;

/// Le mélange « sonne » sur une trame à moins de 40 dB de sa trame la plus
/// forte : les silences et les fondus ne comptent pas.
const SONNE_DB: f32 = 40.0;

/// Part d'énergie la plus basse rendue (un stem muet).
const PLANCHER_DB: f32 = -120.0;

/// La part d'énergie de chaque stem dans leur somme, en dB, sur les trames où
/// la somme sonne. `stems` : un signal mono (ou un canal) par stem, à `sr` Hz ;
/// les longueurs peuvent différer (la plus courte fait foi).
pub fn parts_db(stems: &[&[f32]], sr: f32) -> Vec<f32> {
    let n = stems.iter().map(|s| s.len()).min().unwrap_or(0);
    let t = ((sr * TRAME_S) as usize).max(1);
    let k = n / t;
    if k == 0 {
        return vec![PLANCHER_DB; stems.len()];
    }
    let energie = |x: &[f32]| x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    let mut melange = vec![0.0f64; k];
    let mut par_stem = vec![vec![0.0f64; k]; stems.len()];
    let mut somme = vec![0.0f32; t];
    for j in 0..k {
        somme.iter_mut().for_each(|v| *v = 0.0);
        for (i, s) in stems.iter().enumerate() {
            let trame = &s[j * t..(j + 1) * t];
            par_stem[i][j] = energie(trame);
            for (a, b) in somme.iter_mut().zip(trame) {
                *a += b;
            }
        }
        melange[j] = energie(&somme);
    }
    let max = melange.iter().cloned().fold(0.0, f64::max);
    if max <= 0.0 {
        return vec![PLANCHER_DB; stems.len()];
    }
    let seuil = max * 10f64.powf(-(SONNE_DB as f64) / 10.0);
    let sonne: Vec<usize> = (0..k).filter(|&j| melange[j] > seuil).collect();
    let total: f64 = sonne.iter().map(|&j| melange[j]).sum();
    par_stem
        .iter()
        .map(|e| {
            let part = sonne.iter().map(|&j| e[j]).sum::<f64>() / total;
            if part > 0.0 {
                ((10.0 * part.log10()) as f32).max(PLANCHER_DB)
            } else {
                PLANCHER_DB
            }
        })
        .collect()
}

/// Ce stem est-il presque vide ?
pub fn presque_vide(part_db: f32) -> bool {
    part_db < SEUIL_PRESQUE_VIDE_DB
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sinus(f: f32, a: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| a * (std::f32::consts::TAU * f * i as f32 / 8000.0).sin())
            .collect()
    }

    #[test]
    fn un_piano_absent_est_presque_vide_et_un_orgue_discret_ne_l_est_pas() {
        let n = 8000 * 20;
        let basse = sinus(55.0, 0.5, n);
        let guitare = sinus(220.0, 0.4, n);
        // Un résidu de guitare à −50 dB : presque vide.
        let piano = sinus(330.0, 0.5 * 10f32.powf(-50.0 / 20.0), n);
        // Un orgue à −25 dB, sur les dix dernières secondes seulement.
        let mut orgue = vec![0.0; n];
        orgue[n / 2..].copy_from_slice(&sinus(440.0, 0.5 * 10f32.powf(-22.0 / 20.0), n / 2));
        let p = parts_db(&[&basse, &guitare, &piano, &orgue], 8000.0);
        assert!(!presque_vide(p[0]) && !presque_vide(p[1]), "{p:?}");
        assert!(presque_vide(p[2]), "{p:?}");
        assert!(p[2] < -45.0, "{p:?}");
        assert!(!presque_vide(p[3]), "{p:?}");
        assert!(p[3] > -30.0 && p[3] < -20.0, "{p:?}");
    }

    #[test]
    fn les_silences_ne_comptent_pas_et_le_vide_ne_casse_rien() {
        // Un morceau à moitié silencieux : la part se mesure sur la moitié qui
        // sonne, comme si le silence n'existait pas.
        let n = 8000 * 10;
        let mut a = sinus(110.0, 0.5, n);
        let mut b = sinus(220.0, 0.05, n);
        a[..n / 2].iter_mut().for_each(|v| *v = 0.0);
        b[..n / 2].iter_mut().for_each(|v| *v = 0.0);
        let p = parts_db(&[&a, &b], 8000.0);
        assert!((p[1] - (-20.0)).abs() < 0.5, "{p:?}");
        let muet = vec![0.0f32; n];
        assert_eq!(parts_db(&[&muet, &muet], 8000.0), vec![-120.0, -120.0]);
        assert_eq!(parts_db(&[], 8000.0), Vec::<f32>::new());
        assert_eq!(parts_db(&[&[0.1; 10]], 8000.0), vec![-120.0]);
    }
}
