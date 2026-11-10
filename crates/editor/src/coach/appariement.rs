// SPDX-License-Identifier: GPL-3.0-or-later
//! L'apparieur du coach (E4, tranche 2) : chaque coup joué face à la note de
//! référence qu'il visait (`docs/recherche-coach-pratique.md`, « Comparer
//! une transcription à une transcription » et « Timing, dynamique et
//! groove »).
//!
//! Deux questions séparées : la note est-elle **attrapée** (fenêtre large),
//! et de combien est-elle **décalée** (écart signé d = t_joué − t̂, négatif
//! en avance). Les fenêtres s'expriment en temps musical : à vitesse r, une
//! fenêtre de 40 ms du morceau d'origine dure 40/r ms à l'horloge. Seul le
//! seuil du « parfait » garde un plancher absolu, celui de l'oreille :
//! max(10 ms, 2,5 % de l'intervalle entre notes) (Friberg & Sundberg 1995).
//!
//! La règle centrale : **ne rien signaler sur une note de référence
//! incertaine** (cymbales, notes fantômes, coups faibles de la
//! transcription). Elle reste dans l'appariement, pour qu'un coup qui la
//! joue ne compte pas « en trop », mais son verdict est [`Verdict::NonNotee`]
//! et elle n'est jamais « manquée ».

use serde::{Deserialize, Serialize};

use super::batterie::{Classe, Frappe};
use super::Horloge;

/// Une note de la référence (la transcription de la batterie).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    /// Instant dans le morceau d'origine, en secondes.
    pub tau_s: f64,
    pub classe: Classe,
    /// Fausse quand la transcription n'en est pas sûre : rien n'est signalé
    /// sur elle ([`reference_sure`]).
    pub sure: bool,
}

/// Activation d'ADTOF sous laquelle un coup de la référence est incertain :
/// les notes fantômes et les coups que le réseau devine à peine (ses seuils
/// de détection vont de 0,22 à 0,32).
pub const FORCE_SURE: f32 = 0.5;

/// Une note de la transcription est-elle assez sûre pour qu'on juge le jeu
/// sur elle ? Jamais les cymbales : ADTOF y mêle crash et ride, et un
/// batteur y varie librement. Sinon, une activation d'au moins
/// [`FORCE_SURE`].
pub fn reference_sure(classe: Classe, force: f32) -> bool {
    classe != Classe::Cymbales && force >= FORCE_SURE
}

/// Les fenêtres, en secondes **du morceau d'origine** (divisées par r à
/// l'horloge), sauf le plancher du parfait.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fenetres {
    /// Plancher absolu du parfait, en secondes d'horloge (seuil perceptif).
    pub parfait_min_s: f64,
    /// Part de l'intervalle à la note voisine sous laquelle c'est parfait.
    pub parfait_part: f64,
    /// Jusqu'où c'est « bon ».
    pub bon_s: f64,
    /// Jusqu'où la note est attrapée (en avance ou en retard) ; au-delà,
    /// manquée et en trop.
    pub attrape_s: f64,
}

impl Default for Fenetres {
    /// Indulgentes : 40 ms pour « bon » (mir_eval, Score Drummer), 150 ms
    /// pour « attrapée » (les études de tutorat vont de 100 à 200 ms).
    fn default() -> Self {
        Fenetres {
            parfait_min_s: 0.010,
            parfait_part: 0.025,
            bon_s: 0.040,
            attrape_s: 0.150,
        }
    }
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Parfait,
    Bon,
    Avance,
    Retard,
    /// La note attendue n'a pas été jouée.
    Manquee,
    /// Un coup qu'aucune note n'attendait.
    EnTrop,
    /// Un coup au bon moment, sur la mauvaise pièce : la note attendue à cet
    /// instant n'a pas été jouée, une autre l'a été.
    MauvaisePiece,
    /// Note de référence incertaine : rien n'est dit.
    NonNotee,
}

/// Le jugement d'une note attendue, d'un coup joué, ou des deux.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Jugement {
    /// Rang de la note dans les références, s'il y en a une.
    pub reference: Option<usize>,
    /// Passage de boucle de la note attendue (0 hors boucle).
    pub passage: i64,
    /// Rang du coup dans les frappes, s'il y en a un.
    pub frappe: Option<usize>,
    /// Instant attendu à l'horloge, latence comprise.
    pub attendu_s: Option<f64>,
    /// Écart signé t_joué − t̂, en millisecondes d'horloge ; négatif en
    /// avance.
    pub ecart_ms: Option<f64>,
    /// Le même écart en temps du morceau d'origine (× r) : comparable d'une
    /// vitesse à l'autre.
    pub ecart_musical_ms: Option<f64>,
    pub verdict: Verdict,
}

/// Juge les notes que la référence attendait entre `t0_s` et `t1_s` de
/// l'horloge, et les coups reçus dans le même intervalle qui n'en ont pris
/// aucune. Un coup juste avant `t0_s` peut prendre une note juste après :
/// l'appelant découpe le temps en fenêtres consécutives, sans les juger deux
/// fois ; il attend que la fin d'une fenêtre ait au moins une fenêtre
/// d'attrape de passé, pour que les coups en retard soient arrivés. Par classe, les paires les plus
/// proches d'abord (un glouton sur l'écart absolu : sur une voix d'une
/// classe, qui avance dans le temps, il donne l'appariement optimal du
/// hongrois sauf aux croisements, qu'une fenêtre plus étroite que
/// l'intervalle interdit). Puis, parmi les restes, un coup sur la mauvaise
/// pièce à une fenêtre « bon » d'une note manquée devient
/// [`Verdict::MauvaisePiece`].
///
/// Le tout est trié par instant (attendu, ou joué à défaut).
pub fn apparier(
    horloge: &Horloge,
    refs: &[Reference],
    frappes: &[Frappe],
    t0_s: f64,
    t1_s: f64,
    fenetres: &Fenetres,
) -> Vec<Jugement> {
    let r = horloge.vitesse.max(1e-3);
    let attrape = fenetres.attrape_s / r;
    let bon = fenetres.bon_s / r;

    // Les notes attendues, une marge d'attrape en deçà et au-delà : un coup
    // au bord de la fenêtre d'écoute trouve encore sa note.
    let taus: Vec<f64> = refs.iter().map(|x| x.tau_s).collect();
    let attendus = horloge.projeter(&taus, t0_s - attrape, t1_s + attrape);
    // Les coups de la marge peuvent prendre une note de la fenêtre ; seuls
    // ceux de la fenêtre sont jugés seuls (en trop).
    let candidates: Vec<usize> = (0..frappes.len())
        .filter(|&j| frappes[j].t_s >= t0_s - attrape && frappes[j].t_s < t1_s + attrape)
        .collect();
    let dedans = |j: usize| frappes[j].t_s >= t0_s && frappes[j].t_s < t1_s;

    // Intervalle à la note voisine (toutes classes), à l'horloge : il règle
    // le seuil du parfait et borne la fenêtre d'attrape, pour qu'un coup ne
    // saute pas par-dessus une note pour en prendre une autre de sa classe.
    let voisin = |k: usize, meme_classe: bool| -> f64 {
        let c = refs[attendus[k].indice].classe;
        let t = attendus[k].t_s;
        let mut d = f64::INFINITY;
        for (i, a) in attendus.iter().enumerate() {
            if i != k && (!meme_classe || refs[a.indice].classe == c) {
                d = d.min((a.t_s - t).abs());
            }
        }
        d
    };

    // Paires candidates (attendu k, frappe j) de même classe, dans la
    // fenêtre d'attrape, bornée à la demi-distance à la note voisine de
    // même classe.
    let mut paires: Vec<(f64, usize, usize)> = Vec::new();
    for (k, a) in attendus.iter().enumerate() {
        let classe = refs[a.indice].classe;
        let w = attrape.min(voisin(k, true) / 2.0);
        for &j in &candidates {
            let f = &frappes[j];
            let d = f.t_s - a.t_s;
            if f.classe == classe && d.abs() <= w {
                paires.push((d.abs(), k, j));
            }
        }
    }
    paires.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut k_pris = vec![None; attendus.len()];
    let mut j_pris = vec![false; frappes.len()];
    for &(_, k, j) in &paires {
        if k_pris[k].is_none() && !j_pris[j] {
            k_pris[k] = Some(j);
            j_pris[j] = true;
        }
    }

    let mut v = Vec::new();
    let mut manquees: Vec<usize> = Vec::new();
    for (k, a) in attendus.iter().enumerate() {
        let reference = &refs[a.indice];
        // Une note appartient à la fenêtre où elle est attendue : celles de
        // la marge ne servent qu'à l'appariement, la fenêtre voisine les
        // juge. Des fenêtres consécutives ne jugent donc jamais deux fois.
        if !(a.t_s >= t0_s && a.t_s < t1_s) {
            continue;
        }
        let base = Jugement {
            reference: Some(a.indice),
            passage: a.passage,
            frappe: None,
            attendu_s: Some(a.t_s),
            ecart_ms: None,
            ecart_musical_ms: None,
            verdict: Verdict::Manquee,
        };
        match k_pris[k] {
            Some(j) => {
                let d = frappes[j].t_s - a.t_s;
                let parfait = fenetres
                    .parfait_min_s
                    .max(fenetres.parfait_part * voisin(k, false).min(1.0));
                let verdict = if !reference.sure {
                    Verdict::NonNotee
                } else if d.abs() <= parfait {
                    Verdict::Parfait
                } else if d.abs() <= bon {
                    Verdict::Bon
                } else if d < 0.0 {
                    Verdict::Avance
                } else {
                    Verdict::Retard
                };
                v.push(Jugement {
                    frappe: Some(j),
                    ecart_ms: Some(d * 1000.0),
                    ecart_musical_ms: Some(d * r * 1000.0),
                    verdict,
                    ..base
                });
            }
            None if !reference.sure => v.push(Jugement {
                verdict: Verdict::NonNotee,
                ..base
            }),
            None => {
                manquees.push(v.len());
                v.push(base);
            }
        }
    }

    // Les coups restés seuls : sur la mauvaise pièce s'ils tombent près
    // d'une note manquée (la plus proche), en trop sinon — sauf près d'une
    // note incertaine de n'importe quelle classe, où l'on se tait.
    for &j in &candidates {
        if j_pris[j] || !dedans(j) {
            continue;
        }
        let f = &frappes[j];
        let pres = |a_s: f64| (f.t_s - a_s).abs() <= bon;
        let cible = manquees
            .iter()
            .copied()
            .filter(|&m| v[m].frappe.is_none() && pres(v[m].attendu_s.unwrap_or(f64::NAN)))
            .min_by(|&x, &y| {
                let dx = (f.t_s - v[x].attendu_s.unwrap_or(0.0)).abs();
                let dy = (f.t_s - v[y].attendu_s.unwrap_or(0.0)).abs();
                dx.total_cmp(&dy)
            });
        if let Some(m) = cible {
            let d = f.t_s - v[m].attendu_s.unwrap_or(f.t_s);
            v[m].frappe = Some(j);
            v[m].ecart_ms = Some(d * 1000.0);
            v[m].ecart_musical_ms = Some(d * r * 1000.0);
            v[m].verdict = Verdict::MauvaisePiece;
            continue;
        }
        let pres_incertaine = attendus
            .iter()
            .any(|a| !refs[a.indice].sure && (f.t_s - a.t_s).abs() <= attrape);
        v.push(Jugement {
            reference: None,
            passage: 0,
            frappe: Some(j),
            attendu_s: None,
            ecart_ms: None,
            ecart_musical_ms: None,
            verdict: if pres_incertaine {
                Verdict::NonNotee
            } else {
                Verdict::EnTrop
            },
        });
    }

    let instant = |x: &Jugement| {
        x.attendu_s
            .or(x.frappe.map(|j| frappes[j].t_s))
            .unwrap_or(0.0)
    };
    v.sort_by(|x, y| instant(x).total_cmp(&instant(y)));
    v
}

/// Ce qu'a donné une mesure à un passage.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Bilan {
    /// Notes sûres attendues.
    pub attendues: u32,
    /// Notes sûres attrapées (parfait, bon, avance, retard).
    pub attrapees: u32,
    pub manquees: u32,
    pub en_trop: u32,
    pub mauvaise_piece: u32,
}

impl Bilan {
    pub fn de(jugements: &[Jugement]) -> Bilan {
        let mut b = Bilan::default();
        for j in jugements {
            match j.verdict {
                Verdict::Parfait | Verdict::Bon | Verdict::Avance | Verdict::Retard => {
                    b.attendues += 1;
                    b.attrapees += 1;
                }
                Verdict::Manquee => {
                    b.attendues += 1;
                    b.manquees += 1;
                }
                Verdict::MauvaisePiece => {
                    b.attendues += 1;
                    b.mauvaise_piece += 1;
                }
                Verdict::EnTrop => b.en_trop += 1,
                Verdict::NonNotee => {}
            }
        }
        b
    }

    /// Part des notes sûres attrapées, 1 si la mesure n'en attend aucune.
    pub fn taux(&self) -> f64 {
        if self.attendues == 0 {
            1.0
        } else {
            self.attrapees as f64 / self.attendues as f64
        }
    }
}

/// Le musicien a-t-il perdu sa place ? Si ses coups, décalés d'une mesure
/// entière (en avance ou en retard), attrapent nettement plus de notes
/// qu'à leur place, la cascade de manquées et d'en trop n'est pas faite de
/// fautes : il joue la bonne musique au mauvais endroit. Rend le décalage en
/// mesures (−1 : il joue la mesure d'avant, en retard d'une mesure ; +1 :
/// celle d'après), ou `None`.
///
/// `mesure_s` : durée de la mesure dans le morceau d'origine. Il faut au
/// moins quatre notes sûres, moins de la moitié attrapées à leur place et au
/// moins les trois quarts, et le double, une fois décalées.
pub fn place_perdue(
    horloge: &Horloge,
    refs: &[Reference],
    frappes: &[Frappe],
    t0_s: f64,
    t1_s: f64,
    mesure_s: f64,
    fenetres: &Fenetres,
) -> Option<i32> {
    if !(mesure_s.is_finite() && mesure_s > 0.0) {
        return None;
    }
    let ici = Bilan::de(&apparier(horloge, refs, frappes, t0_s, t1_s, fenetres));
    if ici.attendues < 4 || ici.taux() >= 0.5 {
        return None;
    }
    let r = horloge.vitesse.max(1e-3);
    let mut meilleur: Option<(i32, f64)> = None;
    for k in [-1i32, 1] {
        // Jouer la mesure d'avant à la place de celle-ci, c'est jouer chaque
        // coup une mesure trop tard : on les ramène d'autant.
        let dt = -(k as f64) * mesure_s / r;
        let decales: Vec<Frappe> = frappes
            .iter()
            .map(|f| Frappe {
                t_s: f.t_s + dt,
                ..*f
            })
            .collect();
        let b = Bilan::de(&apparier(
            horloge,
            refs,
            &decales,
            t0_s + dt,
            t1_s + dt,
            fenetres,
        ));
        let taux = b.taux();
        if b.attendues >= 4
            && taux >= 0.75
            && taux >= 2.0 * ici.taux()
            && meilleur.is_none_or(|(_, t)| taux > t)
        {
            meilleur = Some((k, taux));
        }
    }
    meilleur.map(|(k, _)| -k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn horloge(vitesse: f64, boucle: Option<(f64, f64)>) -> Horloge {
        Horloge {
            t_s: 100.0,
            morceau_s: 0.0,
            vitesse,
            boucle,
            latence_s: 0.0,
        }
    }

    /// Un rythme rock sur deux mesures à 120 (0,5 s le temps) : grosse
    /// caisse 1 et 3, caisse claire 2 et 4, charleston en croches, une crash
    /// au premier temps (incertaine par nature).
    fn rock() -> Vec<Reference> {
        let mut v = Vec::new();
        for m in 0..2 {
            let t0 = m as f64 * 2.0;
            for b in 0..4 {
                let t = t0 + b as f64 * 0.5;
                let c = if b % 2 == 0 {
                    Classe::GrosseCaisse
                } else {
                    Classe::CaisseClaire
                };
                v.push(Reference {
                    tau_s: t,
                    classe: c,
                    sure: true,
                });
            }
            for c in 0..8 {
                v.push(Reference {
                    tau_s: t0 + c as f64 * 0.25,
                    classe: Classe::Charleston,
                    sure: true,
                });
            }
        }
        v.push(Reference {
            tau_s: 0.0,
            classe: Classe::Cymbales,
            sure: false,
        });
        v.sort_by(|a, b| a.tau_s.total_cmp(&b.tau_s));
        v
    }

    /// Les frappes d'un batteur qui joue la référence avec `ecart(i)`
    /// secondes (à l'horloge) sur la note i, à la vitesse de l'horloge.
    fn jouer(h: &Horloge, refs: &[Reference], ecart: impl Fn(usize) -> Option<f64>) -> Vec<Frappe> {
        let mut v: Vec<Frappe> = refs
            .iter()
            .enumerate()
            .filter_map(|(i, x)| {
                let t = h.attendu(x.tau_s, 0)?;
                ecart(i).map(|d| Frappe {
                    t_s: t + d,
                    classe: x.classe,
                    note: 0,
                    velocite: 90,
                    ouverture: None,
                })
            })
            .collect();
        v.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
        v
    }

    fn compter(js: &[Jugement], v: Verdict) -> usize {
        js.iter().filter(|j| j.verdict == v).count()
    }

    #[test]
    fn un_jeu_juste_est_parfait_et_la_crash_n_est_pas_notee() {
        let h = horloge(1.0, None);
        let refs = rock();
        let f = jouer(&h, &refs, |i| Some(0.003 * ((i % 3) as f64 - 1.0)));
        let js = apparier(&h, &refs, &f, 99.0, 105.0, &Fenetres::default());
        assert_eq!(compter(&js, Verdict::Parfait), 24);
        assert_eq!(compter(&js, Verdict::NonNotee), 1);
        assert_eq!(js.len(), 25);
        let b = Bilan::de(&js);
        assert_eq!((b.attendues, b.attrapees, b.en_trop), (24, 24, 0));
    }

    #[test]
    fn ecarts_signes_et_fenetres_en_temps_musical() {
        // À vitesse 0,5, un retard de 60 ms à l'horloge vaut 30 ms du
        // morceau : « bon », pas « en retard ».
        let h = horloge(0.5, None);
        let refs = vec![
            Reference {
                tau_s: 1.0,
                classe: Classe::CaisseClaire,
                sure: true,
            },
            Reference {
                tau_s: 2.0,
                classe: Classe::CaisseClaire,
                sure: true,
            },
            Reference {
                tau_s: 3.0,
                classe: Classe::CaisseClaire,
                sure: true,
            },
        ];
        let ecarts = [0.060, -0.120, 0.250];
        let f = jouer(&h, &refs, |i| Some(ecarts[i]));
        let js = apparier(&h, &refs, &f, 90.0, 120.0, &Fenetres::default());
        let v: Vec<_> = js.iter().map(|j| j.verdict).collect();
        assert_eq!(v, [Verdict::Bon, Verdict::Avance, Verdict::Retard]);
        assert!((js[0].ecart_ms.unwrap() - 60.0).abs() < 1e-6);
        assert!((js[0].ecart_musical_ms.unwrap() - 30.0).abs() < 1e-6);
        assert!((js[1].ecart_ms.unwrap() + 120.0).abs() < 1e-6);
        // Le même retard de 250 ms à vitesse 1 sort de la fenêtre :
        // manquée et en trop.
        let h1 = horloge(1.0, None);
        let f1 = jouer(&h1, &refs, |i| Some(ecarts[i]));
        let js1 = apparier(&h1, &refs, &f1, 90.0, 120.0, &Fenetres::default());
        assert_eq!(compter(&js1, Verdict::Manquee), 1);
        assert_eq!(compter(&js1, Verdict::EnTrop), 1);
    }

    #[test]
    fn manquees_en_trop_et_mauvaise_piece() {
        let h = horloge(1.0, None);
        let refs = rock();
        // On oublie la caisse claire du temps 2 (indice dans refs), on joue
        // un tom à la place de celle du temps 4, et on ajoute un coup isolé.
        let i_cc2 = refs
            .iter()
            .position(|x| x.classe == Classe::CaisseClaire && x.tau_s == 0.5)
            .unwrap();
        let i_cc4 = refs
            .iter()
            .position(|x| x.classe == Classe::CaisseClaire && x.tau_s == 1.5)
            .unwrap();
        let mut f = jouer(&h, &refs, |i| (i != i_cc2 && i != i_cc4).then_some(0.0));
        f.push(Frappe {
            t_s: h.attendu(1.5, 0).unwrap() + 0.01,
            classe: Classe::Toms,
            note: 45,
            velocite: 80,
            ouverture: None,
        });
        f.push(Frappe {
            t_s: h.attendu(2.875, 0).unwrap(),
            classe: Classe::GrosseCaisse,
            note: 36,
            velocite: 80,
            ouverture: None,
        });
        f.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
        let js = apparier(&h, &refs, &f, 99.0, 105.0, &Fenetres::default());
        assert_eq!(compter(&js, Verdict::Manquee), 1);
        assert_eq!(compter(&js, Verdict::MauvaisePiece), 1);
        assert_eq!(compter(&js, Verdict::EnTrop), 1);
        assert_eq!(compter(&js, Verdict::Parfait), 22);
        let m = js.iter().find(|j| j.verdict == Verdict::Manquee).unwrap();
        assert_eq!(m.reference, Some(i_cc2));
        let b = Bilan::de(&js);
        assert_eq!(
            (
                b.attendues,
                b.attrapees,
                b.manquees,
                b.mauvaise_piece,
                b.en_trop
            ),
            (24, 22, 1, 1, 1)
        );
    }

    #[test]
    fn rien_n_est_signale_sur_une_note_incertaine() {
        let h = horloge(1.0, None);
        // Une note fantôme incertaine de caisse claire, une crash.
        let refs = vec![
            Reference {
                tau_s: 1.0,
                classe: Classe::CaisseClaire,
                sure: false,
            },
            Reference {
                tau_s: 2.0,
                classe: Classe::Cymbales,
                sure: false,
            },
            Reference {
                tau_s: 3.0,
                classe: Classe::CaisseClaire,
                sure: true,
            },
        ];
        // Le fantôme est omis ; la crash jouée en retard de 120 ms ; un coup
        // de charleston près de la crash (un batteur qui varie).
        let mut f = jouer(&h, &refs, |i| match i {
            0 => None,
            1 => Some(0.120),
            _ => Some(0.0),
        });
        f.push(Frappe {
            t_s: h.attendu(2.0, 0).unwrap() + 0.02,
            classe: Classe::Charleston,
            note: 42,
            velocite: 60,
            ouverture: None,
        });
        let js = apparier(&h, &refs, &f, 99.0, 105.0, &Fenetres::default());
        assert_eq!(compter(&js, Verdict::Parfait), 1);
        assert_eq!(compter(&js, Verdict::NonNotee), 3, "{js:?}");
        for v in [
            Verdict::Manquee,
            Verdict::EnTrop,
            Verdict::Retard,
            Verdict::MauvaisePiece,
        ] {
            assert_eq!(compter(&js, v), 0, "{v:?}");
        }
        assert!(reference_sure(Classe::CaisseClaire, 0.8));
        assert!(!reference_sure(Classe::CaisseClaire, 0.3));
        assert!(!reference_sure(Classe::Cymbales, 0.95));
    }

    #[test]
    fn un_coup_ne_saute_pas_sur_la_note_voisine() {
        // Croches de charleston à 0,25 s : un coup à mi-chemin (125 ms) ne
        // prend aucune des deux, même si la fenêtre d'attrape est de 150 ms.
        let h = horloge(1.0, None);
        let refs: Vec<_> = (0..4)
            .map(|i| Reference {
                tau_s: i as f64 * 0.25,
                classe: Classe::Charleston,
                sure: true,
            })
            .collect();
        let mut f = jouer(&h, &refs, |i| (i != 1).then_some(0.0));
        f.push(Frappe {
            t_s: h.attendu(0.25, 0).unwrap() + 0.126,
            classe: Classe::Charleston,
            note: 42,
            velocite: 60,
            ouverture: None,
        });
        f.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
        let js = apparier(&h, &refs, &f, 99.0, 102.0, &Fenetres::default());
        assert_eq!(compter(&js, Verdict::Parfait), 3);
        assert_eq!(compter(&js, Verdict::Manquee), 1);
        assert_eq!(compter(&js, Verdict::EnTrop), 1);
    }

    #[test]
    fn en_boucle_chaque_passage_est_juge() {
        // Boucle sur la première mesure [0, 2) à vitesse 0,8, trois passages.
        let h = horloge(0.8, Some((0.0, 2.0)));
        let refs: Vec<_> = rock().into_iter().filter(|x| x.tau_s < 2.0).collect();
        let mut f = Vec::new();
        for passage in 0..3 {
            for x in &refs {
                let t = h.attendu(x.tau_s, passage).unwrap();
                // Le deuxième passage traîne de 30 ms à l'horloge (24 ms du
                // morceau : « bon »).
                let d = if passage == 1 { 0.030 } else { 0.0 };
                f.push(Frappe {
                    t_s: t + d,
                    classe: x.classe,
                    note: 0,
                    velocite: 90,
                    ouverture: None,
                });
            }
        }
        f.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
        let periode = 2.0 / 0.8;
        let js = apparier(
            &h,
            &refs,
            &f,
            100.0 - 0.1,
            100.0 + 3.0 * periode - 0.1,
            &Fenetres::default(),
        );
        for passage in 0..3 {
            let n = js
                .iter()
                .filter(|j| j.passage == passage && j.reference.is_some())
                .count();
            assert_eq!(n, refs.len(), "passage {passage}");
        }
        assert_eq!(compter(&js, Verdict::Bon), 12);
        assert_eq!(compter(&js, Verdict::Parfait), 24);
    }

    #[test]
    fn une_mesure_de_retard_se_voit_comme_une_place_perdue() {
        // Neuf mesures à 120 (2 s), trois motifs en croches qui ne se
        // recouvrent pas, en cycle : A, B, C, A, B, C…
        let motifs: [[(Classe, [usize; 2]); 2]; 3] = [
            [
                (Classe::GrosseCaisse, [0, 4]),
                (Classe::CaisseClaire, [2, 6]),
            ],
            [
                (Classe::GrosseCaisse, [1, 5]),
                (Classe::CaisseClaire, [3, 7]),
            ],
            [
                (Classe::GrosseCaisse, [2, 6]),
                (Classe::CaisseClaire, [0, 4]),
            ],
        ];
        let h = horloge(1.0, None);
        let mut refs = Vec::new();
        for m in 0..9 {
            for (classe, croches) in motifs[m % 3] {
                for c in croches {
                    refs.push(Reference {
                        tau_s: m as f64 * 2.0 + c as f64 * 0.25,
                        classe,
                        sure: true,
                    });
                }
            }
        }
        refs.sort_by(|a, b| a.tau_s.total_cmp(&b.tau_s));
        let jouer_mesure = |source: usize, ou: usize| -> Vec<Frappe> {
            refs.iter()
                .filter(|x| (source as f64 * 2.0..(source + 1) as f64 * 2.0).contains(&x.tau_s))
                .map(|x| Frappe {
                    t_s: h
                        .attendu(x.tau_s + (ou as f64 - source as f64) * 2.0, 0)
                        .unwrap(),
                    classe: x.classe,
                    note: 0,
                    velocite: 90,
                    ouverture: None,
                })
                .collect()
        };
        let t0 = h.attendu(6.0, 0).unwrap() - 0.05;
        let t1 = h.attendu(8.0, 0).unwrap() - 0.05;
        let f = Fenetres::default();
        // Pendant la mesure 3, il rejoue la mesure 2 : en retard d'une mesure.
        assert_eq!(
            place_perdue(&h, &refs, &jouer_mesure(2, 3), t0, t1, 2.0, &f),
            Some(-1)
        );
        // Il y joue déjà la mesure 4 : en avance d'une mesure.
        assert_eq!(
            place_perdue(&h, &refs, &jouer_mesure(4, 3), t0, t1, 2.0, &f),
            Some(1)
        );
        // À sa place, rien ; des coups de toms sur les contretemps, rien non
        // plus : ce n'est pas la bonne musique au mauvais endroit.
        assert_eq!(
            place_perdue(&h, &refs, &jouer_mesure(3, 3), t0, t1, 2.0, &f),
            None
        );
        let bruit: Vec<Frappe> = (0..8)
            .map(|c| Frappe {
                t_s: h.attendu(6.0 + c as f64 * 0.25 + 0.125, 0).unwrap(),
                classe: Classe::Toms,
                note: 0,
                velocite: 90,
                ouverture: None,
            })
            .collect();
        assert_eq!(place_perdue(&h, &refs, &bruit, t0, t1, 2.0, &f), None);
    }
    #[test]
    fn des_fenetres_consecutives_jugent_chaque_note_une_fois() {
        let h = horloge(0.9, Some((0.0, 4.0)));
        let refs: Vec<_> = rock().into_iter().filter(|x| x.tau_s < 4.0).collect();
        let mut f = Vec::new();
        for passage in 0..2 {
            for (i, x) in refs.iter().enumerate() {
                let d = 0.02 * ((i % 5) as f64 - 2.0);
                let t = h.attendu(x.tau_s, passage).unwrap() + d;
                f.push(Frappe {
                    t_s: t,
                    classe: x.classe,
                    note: 0,
                    velocite: 90,
                    ouverture: None,
                });
            }
        }
        f.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
        let fin = 100.0 + 2.0 * 4.0 / 0.9 - 0.2;
        let entier = apparier(&h, &refs, &f, 99.9, fin, &Fenetres::default());
        // Des fenêtres de 0,25 s, coupées n'importe où.
        let mut morceaux = Vec::new();
        let mut t = 99.9;
        while t < fin {
            let t1 = (t + 0.25).min(fin);
            morceaux.extend(apparier(&h, &refs, &f, t, t1, &Fenetres::default()));
            t = t1;
        }
        let cle = |j: &Jugement| (j.reference, j.passage, j.frappe, j.verdict);
        let mut a: Vec<_> = entier.iter().map(cle).collect();
        let mut b: Vec<_> = morceaux.iter().map(cle).collect();
        a.sort_by_key(|x| format!("{x:?}"));
        b.sort_by_key(|x| format!("{x:?}"));
        assert_eq!(a, b);
        assert_eq!(
            compter(&entier, Verdict::EnTrop) + compter(&entier, Verdict::Manquee),
            0
        );
    }
}
