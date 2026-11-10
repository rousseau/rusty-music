// SPDX-License-Identifier: GPL-3.0-or-later
//! Le coach de Pratiquer, étape 1 : l'horloge et la calibration (E4,
//! `docs/recherche-coach-pratique.md`).
//!
//! En accompagnement, l'application connaît tout ce que le suivi de partition
//! devine ailleurs : elle lance la lecture, choisit la vitesse r et les
//! bornes de la boucle. L'instant où l'on attend la note de référence τ (en
//! secondes du morceau d'origine) se calcule donc :
//!
//! ```text
//! t̂ = t_ancre + (τ − τ_ancre + k·(b − a)) / r + L
//! ```
//!
//! où l'ancre est publiée par le moteur (`rusty_music_player::horloge`), k le
//! passage de boucle [a, b) et L la latence mesurée par [`calibrer`]. Aucun
//! DTW : le seul inconnu est L.
//!
//! Étape 2 : la batterie MIDI ([`batterie`] : profils de correspondance,
//! cinq classes d'ADTOF) et l'apparieur ([`appariement`] : fenêtres en
//! temps musical, écarts signés, silence sur les notes incertaines, place
//! perdue).
//!
//! Étape 3 : du relevé au conseil ([`mesures`] : la table par mesure,
//! la boucle proposée sur les pires mesures, l'escalier de tempo).
//!
//! Étape 4 : dynamique et groove ([`groove`] : micro-timing contre le stem,
//! rapport de swing, vélocité et accents, conseils par motifs).

pub mod appariement;
pub mod batterie;
pub mod groove;
pub mod mesures;

/// L'horloge de pratique : l'ancre du moteur, la boucle et la latence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Horloge {
    /// Instant de l'horloge du moteur (`maintenant_s`) où `morceau_s` a été
    /// rendu.
    pub t_s: f64,
    pub morceau_s: f64,
    /// Vitesse de lecture, r (1 = tempo d'origine).
    pub vitesse: f64,
    /// La boucle [a, b), en secondes du morceau, si elle est posée.
    pub boucle: Option<(f64, f64)>,
    /// La latence calibrée du chemin d'entrée, en secondes.
    pub latence_s: f64,
}

/// Où tombe une note de référence : au passage `passage` de la boucle
/// (0 : celui de l'ancre ; négatif : un passage d'avant), à l'instant `t_s`
/// de l'horloge, latence comprise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attendu {
    /// Rang de la note dans la liste projetée.
    pub indice: usize,
    pub passage: i64,
    pub t_s: f64,
}

impl Horloge {
    /// La boucle, si elle agit sur la lecture à partir de l'ancre : le moteur
    /// ne ramène au début que si la tête **franchit** la fin, une ancre posée
    /// au-delà n'y revient jamais.
    fn boucle_active(&self) -> Option<(f64, f64)> {
        self.boucle
            .filter(|&(a, b)| b > a && self.morceau_s < b && a.is_finite() && b.is_finite())
    }

    fn r(&self) -> f64 {
        self.vitesse.max(1e-3)
    }

    /// L'instant où l'on attend `tau` au passage `passage`. `None` si ce
    /// passage n'existe pas : hors boucle (ou `tau` hors de ses bornes),
    /// seul le passage 0 existe ; ancre avant la boucle, aucun passage
    /// négatif.
    pub fn attendu(&self, tau: f64, passage: i64) -> Option<f64> {
        let base = self.t_s + self.latence_s + (tau - self.morceau_s) / self.r();
        match self.boucle_active() {
            Some((a, b)) if (a..b).contains(&tau) => {
                if passage < 0 && self.morceau_s < a {
                    return None;
                }
                Some(base + passage as f64 * (b - a) / self.r())
            }
            _ => (passage == 0).then_some(base),
        }
    }

    /// L'instant du morceau qu'on aurait dû jouer à l'instant `t_s` de
    /// l'horloge, latence ôtée, et son passage de boucle.
    pub fn morceau_a(&self, t_s: f64) -> (f64, i64) {
        let x = self.morceau_s + (t_s - self.latence_s - self.t_s) * self.r();
        match self.boucle_active() {
            Some((a, b)) if x >= b || (x < a && self.morceau_s >= a) => {
                let k = ((x - a) / (b - a)).floor();
                (a + (x - a) - k * (b - a), k as i64)
            }
            _ => (x, 0),
        }
    }

    /// Toutes les notes de `taus` (secondes du morceau) attendues entre
    /// `t0_s` et `t1_s` de l'horloge, triées par instant.
    pub fn projeter(&self, taus: &[f64], t0_s: f64, t1_s: f64) -> Vec<Attendu> {
        let mut v = Vec::new();
        for (indice, &tau) in taus.iter().enumerate() {
            let Some(base) = self.attendu(tau, 0) else {
                continue;
            };
            match self.boucle_active() {
                Some((a, b)) if (a..b).contains(&tau) => {
                    let periode = (b - a) / self.r();
                    let mut k_min = ((t0_s - base) / periode).ceil() as i64;
                    if self.morceau_s < a {
                        k_min = k_min.max(0);
                    }
                    let k_max = ((t1_s - base) / periode).floor() as i64;
                    for passage in k_min..=k_max {
                        let t_s = base + passage as f64 * periode;
                        if t_s >= t0_s && t_s <= t1_s {
                            v.push(Attendu {
                                indice,
                                passage,
                                t_s,
                            });
                        }
                    }
                }
                _ => {
                    if base >= t0_s && base <= t1_s {
                        v.push(Attendu {
                            indice,
                            passage: 0,
                            t_s: base,
                        });
                    }
                }
            }
        }
        v.sort_by(|x, y| x.t_s.total_cmp(&y.t_s));
        v
    }
}

/// Ce qu'une calibration par tapotement mesure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    /// Médiane des écarts frappe − clic, en millisecondes : la latence L.
    pub latence_ms: f64,
    /// Écart-type robuste (1,4826 × écart absolu médian) des frappes
    /// retenues, en millisecondes.
    pub dispersion_ms: f64,
    pub retenues: usize,
    /// Frappes écartées : trop loin de la médiane, ou deux sur un même clic.
    pub ecartees: usize,
}

/// Frappes retenues qu'il faut au moins pour une médiane qui tienne.
pub const FRAPPES_MIN: usize = 8;
/// Au-delà, on a tapé à côté du clic, pas dessus : on ne garde rien.
pub const DISPERSION_MAX_MS: f64 = 40.0;

fn mediane(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Calibre une latence : `clics`, les instants où le moteur a rendu chaque
/// clic, et `frappes`, ceux où l'entrée a reçu chaque coup du musicien qui
/// tapait dessus, tous en secondes de la même horloge.
///
/// Chaque frappe est rapportée au clic qui la précède d'un quart de période à
/// trois quarts : la latence est positive (on entend après le rendu, on
/// frappe, l'entrée reçoit après) et reste sous la demi-période pour un clic
/// lent. On prend la médiane, on réapparie autour d'elle, puis on écarte ce
/// qui s'en éloigne de plus de max(25 ms, 3 écarts-types robustes) et les
/// doublons sur un même clic. Une médiane, pas une moyenne : les premières
/// frappes, avant qu'on ait pris le pas, ne la déplacent pas.
pub fn calibrer(clics: &[f64], frappes: &[f64]) -> Result<Calibration, String> {
    if frappes.iter().any(|f| !f.is_finite()) || clics.iter().any(|c| !c.is_finite()) {
        return Err("instant non fini".into());
    }
    if clics.len() < 2 || clics.windows(2).any(|w| w[1] <= w[0]) {
        return Err("il faut au moins deux clics, dans l'ordre".into());
    }
    let mut ecarts: Vec<f64> = clics.windows(2).map(|w| w[1] - w[0]).collect();
    let periode = mediane(&mut ecarts);

    // Écart de chaque frappe à son clic, dans la fenêtre [centre − P/2,
    // centre + P/2) ; et le clic retenu.
    let apparier = |centre: f64| -> Vec<(usize, f64)> {
        frappes
            .iter()
            .filter_map(|&f| {
                let mut meilleur: Option<(usize, f64)> = None;
                for (i, &c) in clics.iter().enumerate() {
                    let d = f - c;
                    if d >= centre - periode / 2.0 && d < centre + periode / 2.0 {
                        meilleur = Some((i, d));
                    }
                }
                meilleur
            })
            .collect()
    };

    let premiers = apparier(periode / 4.0);
    if premiers.len() < FRAPPES_MIN {
        return Err(format!(
            "{} frappes sur les clics, il en faut {FRAPPES_MIN}",
            premiers.len()
        ));
    }
    let centre = mediane(&mut premiers.iter().map(|p| p.1).collect::<Vec<_>>());
    let paires = apparier(centre);
    let mut ds: Vec<f64> = paires.iter().map(|p| p.1).collect();
    let med = mediane(&mut ds);
    let mut abs: Vec<f64> = paires.iter().map(|p| (p.1 - med).abs()).collect();
    let sigma = 1.4826 * mediane(&mut abs);
    let seuil = (3.0 * sigma).max(0.025);

    // Un clic, une frappe : la plus proche de la médiane.
    let mut par_clic: Vec<Option<f64>> = vec![None; clics.len()];
    for &(i, d) in &paires {
        if (d - med).abs() > seuil {
            continue;
        }
        match par_clic[i] {
            Some(e) if (e - med).abs() <= (d - med).abs() => {}
            _ => par_clic[i] = Some(d),
        }
    }
    let mut gardes: Vec<f64> = par_clic.into_iter().flatten().collect();
    let retenues = gardes.len();
    if retenues < FRAPPES_MIN {
        return Err(format!(
            "{retenues} frappes régulières, il en faut {FRAPPES_MIN}"
        ));
    }
    let latence = mediane(&mut gardes);
    let mut abs: Vec<f64> = gardes.iter().map(|d| (d - latence).abs()).collect();
    let dispersion_ms = 1.4826 * mediane(&mut abs) * 1000.0;
    if dispersion_ms > DISPERSION_MAX_MS {
        return Err(format!(
            "frappes trop irrégulières ({dispersion_ms:.0} ms) : recommencer"
        ));
    }
    Ok(Calibration {
        latence_ms: latence * 1000.0,
        dispersion_ms,
        retenues,
        ecartees: frappes.len() - retenues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proche(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// Sans boucle, à vitesse 1 : t̂ = t_ancre + (τ − τ_ancre) + L.
    #[test]
    fn sans_boucle_une_note_tombe_une_fois() {
        let h = Horloge {
            t_s: 100.0,
            morceau_s: 10.0,
            vitesse: 1.0,
            boucle: None,
            latence_s: 0.03,
        };
        assert!(proche(h.attendu(12.0, 0).unwrap(), 102.03));
        assert_eq!(h.attendu(12.0, 1), None);
        let (tau, k) = h.morceau_a(102.03);
        assert!(proche(tau, 12.0) && k == 0);
        let p = h.projeter(&[9.0, 12.0, 30.0], 99.0, 110.0);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].indice, 0);
        assert!(proche(p[1].t_s, 102.03));
    }

    /// Ralenti à 0,8 et en boucle [a, b) : chaque passage décale de
    /// (b − a)/r, la formule de la recherche (t̂ = t_début + (τ − τ_boucle)/r
    /// + L) en est le cas d'une ancre au début de boucle.
    #[test]
    fn en_boucle_ralentie_chaque_passage_decale_d_une_periode() {
        let (a, b, r, l) = (20.0, 28.0, 0.8, 0.05);
        let h = Horloge {
            t_s: 50.0,
            morceau_s: a,
            vitesse: r,
            boucle: Some((a, b)),
            latence_s: l,
        };
        for k in 0..4 {
            let t = h.attendu(22.0, k).unwrap();
            let t_debut = 50.0 + k as f64 * (b - a) / r;
            assert!(proche(t, t_debut + (22.0 - a) / r + l), "passage {k}");
            let (tau, kk) = h.morceau_a(t);
            assert!((tau - 22.0).abs() < 1e-9 && kk == k, "{tau} {kk}");
        }
        // Hors des bornes de la boucle : une seule fois.
        assert_eq!(h.attendu(30.0, 1), None);
        // Trois passages entiers projetés : 3 occurrences de chaque note.
        let p = h.projeter(&[21.0, 27.0], 50.0, 50.0 + 3.0 * (b - a) / r);
        assert_eq!(p.len(), 6);
        assert!(p.windows(2).all(|w| w[0].t_s <= w[1].t_s));
        assert_eq!(p.iter().map(|x| x.passage).max(), Some(2));
    }

    /// Une ancre avant la boucle (la lecture y arrive) : le premier tour
    /// est le passage 0, et rien avant ; au-delà de la fin, la boucle
    /// n'agit plus.
    #[test]
    fn une_ancre_avant_la_boucle_y_entre_au_passage_zero() {
        let h = Horloge {
            t_s: 0.0,
            morceau_s: 15.0,
            vitesse: 1.0,
            boucle: Some((20.0, 24.0)),
            latence_s: 0.0,
        };
        assert!(proche(h.attendu(21.0, 0).unwrap(), 6.0));
        assert!(proche(h.attendu(21.0, 1).unwrap(), 10.0));
        assert_eq!(h.attendu(21.0, -1), None);
        assert_eq!(h.morceau_a(9.5), (24.0 - 4.0 + 0.5, 1));
        assert!(h.projeter(&[21.0], -100.0, 7.0).len() == 1);
        let apres = Horloge {
            morceau_s: 30.0,
            ..h
        };
        assert_eq!(apres.attendu(21.0, 1), None);
        assert_eq!(apres.morceau_a(1.0), (31.0, 0));
    }

    /// Un décompte se projette comme une note : un clic à τ = −0,5 (avant le
    /// départ), sur l'échelle de l'ancre.
    #[test]
    fn un_clic_de_decompte_se_projette_comme_une_note() {
        let h = Horloge {
            t_s: 3.0,
            morceau_s: -1.0,
            vitesse: 0.5,
            boucle: None,
            latence_s: 0.0,
        };
        assert!(proche(h.attendu(-0.5, 0).unwrap(), 4.0));
    }

    /// Un pseudo-aléa reproductible, uniforme sur [−1, 1].
    fn alea(graine: &mut u64) -> f64 {
        *graine = graine
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*graine >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }

    /// Seize clics à 100 BPM, une batterie à 38 ms de latence et ±8 ms de
    /// geste ; trois frappes à côté, une en double, les deux premiers clics
    /// manqués : la médiane retrouve 38 ms à 3 ms près.
    #[test]
    fn la_calibration_retrouve_la_latence_malgre_les_frappes_a_cote() {
        let p = 0.6;
        let clics: Vec<f64> = (0..16).map(|i| 5.0 + i as f64 * p).collect();
        let mut g = 7;
        let mut frappes: Vec<f64> = clics[2..]
            .iter()
            .map(|c| c + 0.038 + 0.008 * alea(&mut g))
            .collect();
        frappes.push(clics[5] + 0.25); // à côté
        frappes.push(clics[9] - 0.2); // en avance, très
        frappes.push(clics[12] + 0.045); // doublon d'un clic déjà frappé
        frappes.sort_by(f64::total_cmp);
        let c = calibrer(&clics, &frappes).unwrap();
        assert!((c.latence_ms - 38.0).abs() < 3.0, "{c:?}");
        assert_eq!(c.retenues, 14);
        assert_eq!(c.ecartees, 3);
        assert!(c.dispersion_ms < 12.0, "{c:?}");
    }

    /// Un casque Bluetooth : 230 ms, au-delà du quart d'un clic à 120 BPM
    /// (125 ms) mais sous les trois quarts. Et un batteur qui frappe un peu
    /// avant le clic (−20 ms) : la latence peut être négative.
    #[test]
    fn la_calibration_tient_les_latences_longues_et_negatives() {
        for latence in [0.230, -0.020] {
            let clics: Vec<f64> = (0..12).map(|i| i as f64 * 0.5).collect();
            let mut g = 3;
            let frappes: Vec<f64> = clics
                .iter()
                .map(|c| c + latence + 0.005 * alea(&mut g))
                .collect();
            let c = calibrer(&clics, &frappes).unwrap();
            assert!((c.latence_ms - latence * 1000.0).abs() < 3.0, "{c:?}");
        }
    }

    #[test]
    fn la_calibration_refuse_ce_qui_ne_mesure_rien() {
        let clics: Vec<f64> = (0..16).map(|i| i as f64 * 0.6).collect();
        // Trop peu de frappes.
        assert!(calibrer(&clics, &[0.04, 0.64, 1.24]).is_err());
        // Frappes au hasard, sans rapport avec les clics.
        let mut g = 11;
        let hasard: Vec<f64> = (0..16)
            .map(|i| i as f64 * 0.6 + 0.25 * alea(&mut g))
            .collect();
        assert!(calibrer(&clics, &hasard).is_err());
        // Clics dans le désordre.
        assert!(calibrer(&[1.0, 0.5], &[0.6; 10]).is_err());
    }
}
