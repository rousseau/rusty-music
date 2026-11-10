// SPDX-License-Identifier: GPL-3.0-or-later
//! Du relevé d'erreurs au conseil (E4, tranche 3) : la table par mesure,
//! la boucle proposée sur les pires mesures et l'escalier de tempo
//! (`docs/recherche-coach-pratique.md`, « Du relevé d'erreurs au conseil :
//! une table par mesure »).
//!
//! À chaque passage, chaque mesure jouée donne un [`Releve`] : notes sûres
//! attendues, attrapées, manquées, en trop, sur la mauvaise pièce ; moyenne,
//! écart-type et pente des écarts (en temps du morceau d'origine, donc
//! comparables d'une vitesse à l'autre). [`cumuler`] l'ajoute à la ligne de
//! la mesure (`rusty_music_core::db::MesurePratique`) : comptes additionnés,
//! écarts et score d'erreur lissés par moyenne mobile exponentielle, d'un
//! passage et d'une séance à l'autre. [`pires_mesures`] en tire la boucle à
//! proposer ; [`Escalier`] la vitesse : départ vers 0,7, un cran de plus
//! après deux passages propres, un de moins après un échec. Tout est
//! **proposé**, jamais imposé : l'interface demande.
//!
//! Les notes incertaines (verdict [`Verdict::NonNotee`]) ne comptent nulle
//! part : ni attendues, ni manquées, ni dans les écarts.

use rusty_music_core::db::MesurePratique;
use serde::{Deserialize, Serialize};

use super::appariement::{Jugement, Verdict};

/// Ce qu'une mesure a donné à un passage.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Releve {
    /// Rang de la mesure dans la grille des premiers temps.
    pub mesure: u32,
    pub attendues: u32,
    pub attrapees: u32,
    pub manquees: u32,
    pub en_trop: u32,
    pub fausses: u32,
    /// Écart signé moyen des notes attrapées, en ms du morceau d'origine.
    pub ecart_moy_ms: Option<f64>,
    pub ecart_et_ms: Option<f64>,
    /// Pente de l'écart sur la position dans la mesure (0 au premier temps,
    /// 1 au suivant), en ms par mesure ; négative, le musicien presse.
    pub pente_ms: Option<f64>,
    /// Score d'erreur du passage ([`erreur`]).
    pub erreur: f64,
}

impl Releve {
    /// Rien à dire : aucune note sûre attendue, aucun coup en trop.
    pub fn vide(&self) -> bool {
        self.attendues == 0 && self.en_trop == 0
    }

    /// Le passage est-il propre : au moins 95 % des notes sûres attrapées,
    /// et un écart-type d'au plus 25 ms.
    pub fn propre(&self) -> bool {
        propre(self.attendues, self.attrapees, self.ecart_et_ms)
    }
}

/// Un passage propre (`recherche-coach-pratique.md` : « ≥ 95 % attrapées et
/// écart-type ≤ 25 ms »).
pub fn propre(attendues: u32, attrapees: u32, ecart_et_ms: Option<f64>) -> bool {
    attendues > 0
        && attrapees as f64 >= 0.95 * attendues as f64
        && ecart_et_ms.is_none_or(|e| e <= ECART_PROPRE_MS)
}

/// Écart-type au-delà duquel un passage n'est pas propre.
pub const ECART_PROPRE_MS: f64 = 25.0;

/// En dessous de cet écart (moyenne ou écart-type, en ms du morceau), le
/// timing ne pèse rien dans le score : c'est l'ordre du seuil perceptif.
const ECART_TOLERE_MS: f64 = 15.0;
/// Écart au-delà duquel le timing pèse tout son poids.
const ECART_PLEIN_MS: f64 = 50.0;

/// Le score d'erreur d'un passage sur une mesure : la part des notes sûres
/// ratées (manquées ou sur la mauvaise pièce, un coup en trop compte pour
/// moitié), plus au plus ½ pour le timing des notes attrapées (écart moyen
/// ou écart-type, ce qui est pire, au-delà de 15 ms ; plein à 50 ms). 0 sans
/// faute ; borné à 2.
pub fn erreur(r: &Releve) -> f64 {
    let fautes = (r.manquees + r.fausses) as f64 + 0.5 * r.en_trop as f64;
    let part = fautes / r.attendues.max(1) as f64;
    let pire = r
        .ecart_moy_ms
        .map(f64::abs)
        .into_iter()
        .chain(r.ecart_et_ms)
        .fold(0.0, f64::max);
    let timing = ((pire - ECART_TOLERE_MS) / (ECART_PLEIN_MS - ECART_TOLERE_MS)).clamp(0.0, 1.0);
    (part + 0.5 * timing).min(2.0)
}

/// Relève une mesure à un passage : `jugements` sont ceux de ses notes et
/// des coups en trop qui y tombent, chacun avec sa position dans la mesure
/// (0 à 1).
pub fn relever(mesure: u32, jugements: &[(Jugement, f64)]) -> Releve {
    let mut r = Releve {
        mesure,
        ..Default::default()
    };
    let mut points: Vec<(f64, f64)> = Vec::new();
    for (j, x) in jugements {
        match j.verdict {
            Verdict::Parfait | Verdict::Bon | Verdict::Avance | Verdict::Retard => {
                r.attendues += 1;
                r.attrapees += 1;
                if let Some(e) = j.ecart_musical_ms.filter(|e| e.is_finite()) {
                    points.push((*x, e));
                }
            }
            Verdict::Manquee => {
                r.attendues += 1;
                r.manquees += 1;
            }
            Verdict::MauvaisePiece => {
                r.attendues += 1;
                r.fausses += 1;
            }
            Verdict::EnTrop => r.en_trop += 1,
            Verdict::NonNotee => {}
        }
    }
    if !points.is_empty() {
        let n = points.len() as f64;
        let moy = points.iter().map(|p| p.1).sum::<f64>() / n;
        r.ecart_moy_ms = Some(moy);
        if points.len() >= 2 {
            let var = points.iter().map(|p| (p.1 - moy).powi(2)).sum::<f64>() / (n - 1.0);
            r.ecart_et_ms = Some(var.sqrt());
        }
        r.pente_ms = pente(&points);
    }
    r.erreur = erreur(&r);
    r
}

/// La pente des moindres carrés de y sur x, s'il y a au moins trois points
/// et que les positions s'étalent sur au moins un quart de mesure.
fn pente(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let (min, max) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p.0), b.max(p.0))
        });
    (max - min >= 0.25 && sxx > 0.0).then(|| sxy / sxx)
}

/// Poids du dernier passage dans les moyennes lissées.
pub const LISSAGE: f64 = 0.35;

/// Ajoute un passage à la ligne d'une mesure. La première fois, le passage
/// donne la ligne ; ensuite, comptes additionnés, écarts et score lissés
/// ([`LISSAGE`]).
pub fn cumuler(avant: Option<&MesurePratique>, r: &Releve) -> MesurePratique {
    let lisse = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => Some(a + LISSAGE * (b - a)),
        (a, b) => b.or(a),
    };
    match avant.filter(|a| a.passages > 0) {
        None => MesurePratique {
            mesure: r.mesure,
            passages: 1,
            attendues: r.attendues,
            attrapees: r.attrapees,
            manquees: r.manquees,
            en_trop: r.en_trop,
            fausses: r.fausses,
            ecart_moy_ms: r.ecart_moy_ms,
            ecart_et_ms: r.ecart_et_ms,
            pente_ms: r.pente_ms,
            score: r.erreur,
        },
        Some(a) => MesurePratique {
            mesure: r.mesure,
            passages: a.passages + 1,
            attendues: a.attendues + r.attendues,
            attrapees: a.attrapees + r.attrapees,
            manquees: a.manquees + r.manquees,
            en_trop: a.en_trop + r.en_trop,
            fausses: a.fausses + r.fausses,
            ecart_moy_ms: lisse(a.ecart_moy_ms, r.ecart_moy_ms),
            ecart_et_ms: lisse(a.ecart_et_ms, r.ecart_et_ms),
            pente_ms: lisse(a.pente_ms, r.pente_ms),
            score: a.score + LISSAGE * (r.erreur - a.score),
        },
    }
}

/// Score sous lequel une mesure n'est pas « difficile » : rien à proposer.
pub const SCORE_DIFFICILE: f64 = 0.15;

/// La boucle à proposer : la pire mesure (score lissé le plus haut, au
/// moins [`SCORE_DIFFICILE`]), étendue à ses voisines difficiles elles
/// aussi, jusqu'à `max_mesures`. Rend les rangs de la première et de la
/// dernière mesure, ou `None` si rien n'est difficile.
pub fn pires_mesures(table: &[MesurePratique], max_mesures: u32) -> Option<(u32, u32)> {
    let difficile = |m: u32| {
        table
            .iter()
            .find(|x| x.mesure == m)
            .filter(|x| x.passages > 0 && x.score >= SCORE_DIFFICILE)
            .map(|x| x.score)
    };
    let pire = table
        .iter()
        .filter(|x| x.passages > 0 && x.score >= SCORE_DIFFICILE)
        .max_by(|a, b| a.score.total_cmp(&b.score).then(b.mesure.cmp(&a.mesure)))?;
    let (mut a, mut b) = (pire.mesure, pire.mesure);
    let max = max_mesures.max(1);
    while b - a + 1 < max {
        let gauche = a.checked_sub(1).and_then(difficile);
        let droite = difficile(b + 1);
        match (gauche, droite) {
            (Some(g), Some(d)) if g > d => a -= 1,
            (_, Some(_)) => b += 1,
            (Some(_), None) => a -= 1,
            (None, None) => break,
        }
    }
    Some((a, b))
}

/// Ce que l'escalier propose après un passage.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Proposition {
    pub vitesse: f64,
    /// Vrai pour un cran de plus, faux pour un cran de moins.
    pub monter: bool,
}

/// L'escalier de tempo : départ vers [`Escalier::DEPART`], un cran de plus
/// ([`Escalier::PAS`]) après [`Escalier::PROPRES`] passages propres de
/// suite, un de moins après un échec (moins de 80 % des notes sûres
/// attrapées). Il suit la vitesse jouée : si l'on en change à la main, le
/// compte des passages propres repart de zéro.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Escalier {
    vitesse: Option<f64>,
    propres: u32,
}

impl Escalier {
    pub const DEPART: f64 = 0.7;
    pub const PAS: f64 = 0.03;
    pub const PROPRES: u32 = 2;
    pub const PLANCHER: f64 = 0.4;
    /// Taux d'attrape sous lequel un passage est un échec.
    pub const ECHEC: f64 = 0.8;

    /// Un passage de la boucle, joué à `vitesse`, vient de finir. Rend la
    /// vitesse à proposer, s'il y a lieu ; jamais au-delà de `cible` (le
    /// tempo d'origine, en général 1).
    pub fn apres(
        &mut self,
        vitesse: f64,
        attendues: u32,
        attrapees: u32,
        ecart_et_ms: Option<f64>,
        cible: f64,
    ) -> Option<Proposition> {
        if attendues == 0 || !vitesse.is_finite() {
            return None;
        }
        if self.vitesse.is_none_or(|v| (v - vitesse).abs() > 1e-3) {
            self.vitesse = Some(vitesse);
            self.propres = 0;
        }
        let arrondi = |v: f64| (v * 100.0).round() / 100.0;
        if propre(attendues, attrapees, ecart_et_ms) {
            self.propres += 1;
            if self.propres >= Self::PROPRES && vitesse < cible - 1e-3 {
                self.propres = 0;
                return Some(Proposition {
                    vitesse: arrondi((vitesse + Self::PAS).min(cible)),
                    monter: true,
                });
            }
            None
        } else {
            self.propres = 0;
            let taux = attrapees as f64 / attendues as f64;
            (taux < Self::ECHEC && vitesse > Self::PLANCHER + 1e-3).then(|| Proposition {
                vitesse: arrondi((vitesse - Self::PAS).max(Self::PLANCHER)),
                monter: false,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(verdict: Verdict, ecart: Option<f64>) -> Jugement {
        Jugement {
            reference: Some(0),
            passage: 0,
            frappe: None,
            attendu_s: None,
            ecart_ms: ecart,
            ecart_musical_ms: ecart,
            verdict,
        }
    }

    #[test]
    fn une_mesure_propre_ne_coute_rien() {
        let js: Vec<_> = (0..8)
            .map(|i| {
                (
                    j(Verdict::Parfait, Some(if i % 2 == 0 { 3.0 } else { -3.0 })),
                    i as f64 / 8.0,
                )
            })
            .chain([(j(Verdict::NonNotee, None), 0.0)])
            .collect();
        let r = relever(4, &js);
        assert_eq!((r.mesure, r.attendues, r.attrapees), (4, 8, 8));
        assert!(r.ecart_moy_ms.unwrap().abs() < 1e-9);
        assert!(r.ecart_et_ms.unwrap() < 4.0);
        assert_eq!(r.erreur, 0.0);
        assert!(r.propre());
    }

    #[test]
    fn presser_donne_une_pente_negative() {
        // Le musicien part juste et finit la mesure 40 ms en avance.
        let js: Vec<_> = (0..8)
            .map(|i| {
                let x = i as f64 / 8.0;
                (j(Verdict::Bon, Some(-40.0 * x / 0.875)), x)
            })
            .collect();
        let r = relever(0, &js);
        assert!((r.pente_ms.unwrap() + 45.7).abs() < 0.5, "{r:?}");
        // Écart-type de 15 ms à peine : le timing pèse un peu dans le score.
        assert!(r.erreur > 0.0 && r.erreur < 0.2, "{r:?}");
        // Trop peu de points, ou serrés : pas de pente.
        assert_eq!(relever(0, &js[..2]).pente_ms, None);
        let serres: Vec<_> = js.iter().map(|(j, x)| (*j, x * 0.2)).collect();
        assert_eq!(relever(0, &serres).pente_ms, None);
    }

    #[test]
    fn les_fautes_font_le_score() {
        let js = [
            (j(Verdict::Parfait, Some(0.0)), 0.0),
            (j(Verdict::Manquee, None), 0.25),
            (j(Verdict::MauvaisePiece, None), 0.5),
            (j(Verdict::Parfait, Some(0.0)), 0.75),
            (
                Jugement {
                    reference: None,
                    ..j(Verdict::EnTrop, None)
                },
                0.6,
            ),
        ];
        let r = relever(1, &js);
        assert_eq!(
            (r.attendues, r.attrapees, r.manquees, r.fausses, r.en_trop),
            (4, 2, 1, 1, 1)
        );
        assert!((r.erreur - 2.5 / 4.0).abs() < 1e-9);
        assert!(!r.propre());
        // Une mesure sans note sûre ni coup en trop ne dit rien.
        assert!(relever(2, &[(j(Verdict::NonNotee, None), 0.0)]).vide());
    }

    #[test]
    fn le_cumul_lisse_le_score() {
        let mauvais = Releve {
            mesure: 5,
            attendues: 8,
            attrapees: 4,
            manquees: 4,
            ecart_moy_ms: Some(30.0),
            erreur: 0.5,
            ..Default::default()
        };
        let bon = Releve {
            mesure: 5,
            attendues: 8,
            attrapees: 8,
            ecart_moy_ms: Some(0.0),
            erreur: 0.0,
            ..Default::default()
        };
        let a = cumuler(None, &mauvais);
        assert_eq!((a.passages, a.score), (1, 0.5));
        let b = cumuler(Some(&a), &bon);
        assert_eq!((b.passages, b.attendues, b.attrapees), (2, 16, 12));
        assert!((b.score - 0.5 * (1.0 - LISSAGE)).abs() < 1e-9);
        assert!((b.ecart_moy_ms.unwrap() - 30.0 * (1.0 - LISSAGE)).abs() < 1e-9);
        // Six passages propres plus tard, la mesure n'est plus difficile.
        let mut c = b;
        for _ in 0..6 {
            c = cumuler(Some(&c), &bon);
        }
        assert!(c.score < SCORE_DIFFICILE, "{c:?}");
        assert!(c.verifier().is_ok());
    }

    fn ligne(mesure: u32, score: f64) -> MesurePratique {
        MesurePratique {
            mesure,
            passages: 2,
            score,
            ..Default::default()
        }
    }

    #[test]
    fn la_boucle_proposee_prend_la_pire_et_ses_voisines() {
        let t = [
            ligne(0, 0.0),
            ligne(1, 0.2),
            ligne(2, 0.6),
            ligne(3, 0.3),
            ligne(4, 0.05),
            ligne(7, 0.4),
        ];
        assert_eq!(pires_mesures(&t, 1), Some((2, 2)));
        assert_eq!(pires_mesures(&t, 2), Some((2, 3)));
        assert_eq!(pires_mesures(&t, 4), Some((1, 3)));
        // La 4 n'est pas difficile : la boucle s'arrête avant.
        assert_eq!(pires_mesures(&t, 8), Some((1, 3)));
        assert_eq!(pires_mesures(&[ligne(0, 0.1), ligne(1, 0.0)], 4), None);
        assert_eq!(pires_mesures(&[], 4), None);
        // Une mesure jamais jouée ne compte pas.
        let jamais = MesurePratique {
            passages: 0,
            ..ligne(3, 0.9)
        };
        assert_eq!(pires_mesures(&[jamais], 4), None);
    }

    #[test]
    fn l_escalier_monte_apres_deux_passages_propres() {
        let mut e = Escalier::default();
        let v = Escalier::DEPART;
        assert_eq!(e.apres(v, 16, 16, Some(10.0), 1.0), None);
        let p = e.apres(v, 16, 16, Some(10.0), 1.0).unwrap();
        assert!(p.monter && (p.vitesse - 0.73).abs() < 1e-9, "{p:?}");
        // On accepte : à la nouvelle vitesse, le compte repart.
        assert_eq!(e.apres(0.73, 16, 16, Some(10.0), 1.0), None);
        // Un passage moyen (90 %) remet le compte à zéro sans descendre.
        assert_eq!(e.apres(0.73, 20, 18, Some(10.0), 1.0), None);
        assert_eq!(e.apres(0.73, 16, 16, Some(10.0), 1.0), None);
        assert!(e.apres(0.73, 16, 16, Some(10.0), 1.0).unwrap().monter);
        // Propre en notes mais irrégulier : pas propre.
        let mut e = Escalier::default();
        e.apres(0.8, 16, 16, Some(30.0), 1.0);
        assert_eq!(e.apres(0.8, 16, 16, Some(30.0), 1.0), None);
        // Un échec propose un cran de moins.
        let p = e.apres(0.8, 16, 10, Some(10.0), 1.0).unwrap();
        assert!(!p.monter && (p.vitesse - 0.77).abs() < 1e-9);
        // Jamais au-delà de la cible, ni sous le plancher.
        let mut e = Escalier::default();
        e.apres(0.99, 8, 8, None, 1.0);
        assert_eq!(e.apres(0.99, 8, 8, None, 1.0).unwrap().vitesse, 1.0);
        let mut e = Escalier::default();
        e.apres(1.0, 8, 8, None, 1.0);
        assert_eq!(e.apres(1.0, 8, 8, None, 1.0), None);
        let mut e = Escalier::default();
        assert_eq!(e.apres(Escalier::PLANCHER, 8, 0, None, 1.0), None);
        // Rien d'attendu : rien à dire.
        assert_eq!(e.apres(0.8, 0, 0, None, 1.0), None);
    }
}
