// SPDX-License-Identifier: GPL-3.0-or-later
//! Le réseau hydrographique : ce qui manque le plus au réalisme.
//!
//! Une carte se reconnaît à ses rivières autant qu'à ses routes. Elles donnent
//! au relief une lecture immédiate — l'eau descend, donc voir où elle va, c'est
//! voir la forme du terrain — et elles cassent la régularité des nappes de
//! densité, qui sans elles se lisent comme des isobares.
//!
//! Technique classique du modelage de terrain, pas une invention : direction
//! d'écoulement en huit voisins (D8), accumulation de flux, puis extraction des
//! cours d'eau au-dessus d'un seuil. On la trouve dans toute la littérature de
//! SIG et chez Amit Patel, que `carto-peuplement.md` cite déjà pour les côtes.

/// Un cours d'eau : un tracé, et le débit qu'il porte à son embouchure.
#[derive(Debug, Clone)]
pub struct Riviere {
    /// En coordonnées de carte, de la source vers l'aval.
    pub points: Vec<[f32; 2]>,
    /// Nombre de cellules drainées à l'embouchure — fixe l'épaisseur du trait.
    pub debit: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Parametres {
    /// Débit minimal pour qu'un écoulement devienne une rivière visible, en
    /// part des cellules du domaine. Plus bas : un chevelu dense.
    pub seuil: f64,
    /// Longueur minimale d'un tracé, en cellules — sous quoi ce n'est qu'un
    /// ruisseau de deux pixels, qui salit la carte sans rien dire.
    pub longueur_min: usize,
    /// Nombre maximal de rivières gardées, les plus fortes d'abord.
    pub combien: usize,
}

impl Default for Parametres {
    fn default() -> Self {
        Self {
            seuil: 0.0006,
            longueur_min: 12,
            combien: 400,
        }
    }
}

/// Fait couler l'eau sur un champ d'altitude.
///
/// `champ` est la nappe normalisée dans `[0, 1]` : les sommets sont les amas de
/// morceaux, et l'eau en descend vers la mer. C'est le sens qu'on veut — les
/// rivières partent des hautes densités et rejoignent les côtes.
pub fn tracer(champ: &[f64], gn: usize, p: &Parametres) -> Vec<Riviere> {
    if gn < 4 || champ.len() != gn * gn {
        return Vec::new();
    }

    // 1. Direction d'écoulement : le plus fort dénivelé parmi les huit voisins.
    //    `usize::MAX` marque un puits — une cuvette ou le bord du domaine.
    let mut aval = vec![usize::MAX; gn * gn];
    for y in 0..gn {
        for x in 0..gn {
            let i = y * gn + x;
            let h = champ[i];
            let mut meilleur = (usize::MAX, 0.0f64);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= gn as i32 || ny >= gn as i32 {
                        continue;
                    }
                    let j = ny as usize * gn + nx as usize;
                    // Pente par unité de distance : sans cela les diagonales,
                    // plus longues, l'emporteraient à dénivelé égal et l'eau
                    // descendrait en zigzag.
                    let distance = if dx != 0 && dy != 0 {
                        std::f64::consts::SQRT_2
                    } else {
                        1.0
                    };
                    let pente = (h - champ[j]) / distance;
                    if pente > meilleur.1 {
                        meilleur = (j, pente);
                    }
                }
            }
            aval[i] = meilleur.0;
        }
    }

    // 2. Accumulation : chaque cellule pousse son eau vers l'aval, traitées de
    //    la plus haute à la plus basse pour qu'une cellule reçoive tout son
    //    amont avant d'être elle-même écoulée.
    let mut ordre: Vec<usize> = (0..gn * gn).collect();
    ordre.sort_unstable_by(|&a, &b| {
        champ[b]
            .partial_cmp(&champ[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut debit = vec![1u32; gn * gn];
    for &i in &ordre {
        let j = aval[i];
        if j != usize::MAX {
            debit[j] = debit[j].saturating_add(debit[i]);
        }
    }

    // 3. Les sources : cellules au-dessus du seuil dont aucun amont ne l'est —
    //    c'est là que la rivière commence à se voir.
    let seuil = ((gn * gn) as f64 * p.seuil).max(4.0) as u32;
    let mut amont_fort = vec![false; gn * gn];
    for (i, &d) in debit.iter().enumerate() {
        if d >= seuil {
            if let Some(&j) = aval.get(i).filter(|&&j| j != usize::MAX) {
                amont_fort[j] = true;
            }
        }
    }

    let lo = -1.0 - rusty_music_core::density::MARGE;
    let pas = (2.0 + 2.0 * rusty_music_core::density::MARGE) / gn as f64;
    let vers_carte = |i: usize| -> [f32; 2] {
        let (x, y) = (i % gn, i / gn);
        [
            (lo + (x as f64 + 0.5) * pas) as f32,
            (lo + (y as f64 + 0.5) * pas) as f32,
        ]
    };

    let mut rivieres = Vec::new();
    for i in 0..gn * gn {
        if debit[i] < seuil || amont_fort[i] {
            continue; // ce n'est pas une source
        }
        let mut points = vec![vers_carte(i)];
        let mut courant = i;
        let mut vus = std::collections::HashSet::from([i]);
        while let Some(&j) = aval.get(courant).filter(|&&j| j != usize::MAX) {
            // Un champ lissé peut receler une boucle numérique : sans cette
            // garde, on tournerait indéfiniment.
            if !vus.insert(j) {
                break;
            }
            points.push(vers_carte(j));
            courant = j;
        }
        if points.len() >= p.longueur_min {
            rivieres.push(Riviere {
                points,
                debit: debit[courant],
            });
        }
    }

    // Les plus forts débits d'abord ; à débit égal, le plus long. L'ordre doit
    // être stable pour que deux exécutions rendent les mêmes tuiles.
    rivieres.sort_by(|a, b| {
        b.debit
            .cmp(&a.debit)
            .then_with(|| b.points.len().cmp(&a.points.len()))
            .then_with(|| {
                a.points[0]
                    .partial_cmp(&b.points[0])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    rivieres.truncate(p.combien);
    rivieres
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un cône : l'eau doit descendre du sommet vers les bords, jamais l'inverse.
    fn cone(gn: usize) -> Vec<f64> {
        let mut champ = vec![0.0; gn * gn];
        let c = gn as f64 / 2.0;
        for y in 0..gn {
            for x in 0..gn {
                let d = ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt() / c;
                // Un peu de relief latéral, sinon toutes les directions se
                // valent et l'écoulement n'a rien à choisir.
                champ[y * gn + x] =
                    (1.0 - d).max(0.0) + 0.03 * ((x as f64 * 0.4).sin() + (y as f64 * 0.3).cos());
            }
        }
        champ
    }

    #[test]
    fn leau_descend_toujours() {
        let gn = 96;
        let champ = cone(gn);
        let r = tracer(&champ, gn, &Parametres::default());
        assert!(!r.is_empty(), "aucune rivière sur un cône");

        let lo = -1.0 - rusty_music_core::density::MARGE;
        let pas = (2.0 + 2.0 * rusty_music_core::density::MARGE) / gn as f64;
        let altitude = |p: &[f32; 2]| {
            let x = (((p[0] as f64 - lo) / pas) as usize).min(gn - 1);
            let y = (((p[1] as f64 - lo) / pas) as usize).min(gn - 1);
            champ[y * gn + x]
        };
        for riviere in &r {
            for paire in riviere.points.windows(2) {
                assert!(
                    altitude(&paire[1]) <= altitude(&paire[0]) + 1e-9,
                    "l'eau remonte : {:.4} → {:.4}",
                    altitude(&paire[0]),
                    altitude(&paire[1])
                );
            }
        }
    }

    #[test]
    fn les_rivieres_sont_ordonnees_et_bornees() {
        let gn = 96;
        let p = Parametres {
            combien: 5,
            ..Default::default()
        };
        let r = tracer(&cone(gn), gn, &p);
        assert!(r.len() <= 5);
        for f in r.windows(2) {
            assert!(f[0].debit >= f[1].debit, "les débits ne décroissent pas");
        }
        // Déterministe : deux exécutions rendent les mêmes tracés.
        let r2 = tracer(&cone(gn), gn, &p);
        assert_eq!(r.len(), r2.len());
        for (a, b) in r.iter().zip(&r2) {
            assert_eq!(a.points, b.points);
        }
    }

    #[test]
    fn un_champ_plat_na_pas_de_riviere() {
        let gn = 64;
        assert!(tracer(&vec![0.5; gn * gn], gn, &Parametres::default()).is_empty());
    }

    #[test]
    fn une_grille_incoherente_ne_fait_pas_tomber_lappelant() {
        assert!(tracer(&[0.0; 9], 64, &Parametres::default()).is_empty());
        assert!(tracer(&[], 0, &Parametres::default()).is_empty());
    }
}
