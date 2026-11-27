// SPDX-License-Identifier: GPL-3.0-or-later
//! La voix chantée, par suivi de fondamentale : pYIN (Mauch et Dixon 2014,
//! crate `pyin`, portage de celui de librosa, HMM compris) sur le stem de
//! voix, puis découpage en notes.
//!
//! Pourquoi pas MuScriptor : restreint au groupe « voice », il ne couvrait
//! que 45 % du temps chanté de « Burnin' Alive » (AC/DC), 26 trous de plus
//! d'une seconde en plein chant (26 nov.) — et les paroles, posées sur les
//! notes, se tassaient sur celles qui restaient. Une voix est monophonique :
//! un suivi de f0 la suit partout où elle chante.
//!
//! Découpage d'une plage voisée en notes :
//! - une hauteur qui s'écarte de plus de [`ECART_DT`] demi-ton de la note en
//!   cours pendant [`TENUE_S`] commence une note (le vibrato et les
//!   glissandos courts restent dans la note) ;
//! - à hauteur égale, un creux de [`ATTAQUE_DB`] sous le pic de la note puis
//!   une remontée d'autant commencent une syllabe (« no, no, no » sur la même
//!   note) ; la montée de la consonne à la voyelle, sans creux, n'en est pas
//!   une ;
//! - une note plus courte que [`DUREE_MIN_S`] tombe.
//!
//! Hauteur d'une note : la médiane de ses trames, arrondie au demi-ton.
//!
//! **Le chant crié** (26 nov., mesure 84 de « Burnin' Alive » : le refrain
//! repris en chœur) n'a pas de période : pYIN le dit non voisé. Là où le
//! stem reste fort (à [`CRI_DB`] du niveau de pointe du morceau) sans note
//! voisée pendant [`CRI_MIN_S`], [`crier`] pose des notes **criées** —
//! une par syllabe, au même découpage par le niveau —, de confiance nulle
//! (`amplitude` 0 : hauteur inconnue ; la partition les dessine en croix).
//! Leur hauteur est indicative : la médiane des rares trames voisées, sinon
//! la note chantée d'avant.

use crate::{Note, Result};

/// Fréquence de travail : celle de Basic Pitch, déjà rééchantillonnée ici.
const SR: u32 = 22_050;
const TRAME: usize = 2048;
const PAS: usize = 256;
/// Du mi2 d'une voix de basse au do6 d'une soprano.
const F_MIN: f64 = 75.0;
const F_MAX: f64 = 1100.0;
/// Écart de hauteur (demi-tons) qui commence une note, s'il dure.
pub const ECART_DT: f32 = 0.8;
/// Durée de l'écart avant de couper.
pub const TENUE_S: f32 = 0.05;
/// Remontée du niveau qui commence une syllabe à hauteur égale.
pub const ATTAQUE_DB: f32 = 6.0;
/// Note la plus courte gardée.
pub const DUREE_MIN_S: f32 = 0.07;
/// Trames non voisées tolérées au milieu d'une note (consonne brève).
const TROU_MAX: usize = 2;
/// Chant crié : un niveau à moins de tant de dB du niveau de pointe du
/// stem (95ᵉ centile des trames).
pub const CRI_DB: f32 = 18.0;
/// Durée minimale d'une plage criée, et d'une syllabe criée.
pub const CRI_MIN_S: f32 = 0.12;
/// Confiance des notes voisées, au moins : 0 est réservé au chant crié.
const CONFIANCE_MIN: f32 = 0.01;

fn midi(f: f32) -> f32 {
    69.0 + 12.0 * (f / 440.0).log2()
}

fn mediane(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Les notes chantées d'un signal mono à `frequence` Hz.
pub fn notes(mono: &[f32], frequence: u32) -> Result<Vec<Note>> {
    let x = if frequence == SR {
        mono.to_vec()
    } else {
        crate::basic_pitch::reechantillonner(mono, frequence, SR)?
    };
    let mut p = pyin::PYINExecutor::new(F_MIN, F_MAX, SR, TRAME, None, Some(PAS), None);
    let (temps, f0, voisees, probas) = p.pyin(
        &x,
        f32::NAN,
        pyin::Framing::Center(pyin::PadMode::Constant(0.0)),
    );
    // Niveau de chaque trame (centrée), en dB.
    let db: Vec<f32> = temps
        .iter()
        .map(|&t| {
            let c = (t * SR as f64) as usize;
            let a = c.saturating_sub(PAS);
            let b = (c + PAS).min(x.len());
            if b <= a {
                return -120.0;
            }
            let e = x[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32;
            10.0 * e.max(1e-12).log10()
        })
        .collect();
    let trames: Vec<Option<f32>> = f0
        .iter()
        .zip(&voisees)
        .map(|(&f, &v)| (v && f.is_finite() && f > 0.0).then(|| midi(f)))
        .collect();
    let mut notes = decouper(&temps, &trames, &db, &probas);
    let criees = crier(&temps, &trames, &db, &notes);
    notes.extend(criees);
    notes.sort_by(|a, b| a.debut_s.total_cmp(&b.debut_s));
    Ok(notes)
}

/// Les notes criées : les plages fortes du stem que nulle note voisée ne
/// couvre, découpées en syllabes par le niveau (un creux de [`ATTAQUE_DB`]
/// puis une remontée d'autant).
fn crier(temps: &[f64], trames: &[Option<f32>], db: &[f32], chantees: &[Note]) -> Vec<Note> {
    if temps.len() < 2 {
        return Vec::new();
    }
    let pas_s = (temps[1] - temps[0]) as f32;
    let mut niveaux: Vec<f32> = db.iter().copied().filter(|&d| d > -80.0).collect();
    if niveaux.is_empty() {
        return Vec::new();
    }
    niveaux.sort_by(f32::total_cmp);
    let seuil = niveaux[niveaux.len() * 95 / 100] - CRI_DB;
    let couverte = |t: f32| {
        chantees
            .iter()
            .any(|n| t >= n.debut_s - 0.02 && t <= n.fin_s + 0.02)
    };
    let libre: Vec<bool> = temps
        .iter()
        .zip(db)
        .map(|(&t, &d)| d >= seuil && !couverte(t as f32))
        .collect();
    let min = (CRI_MIN_S / pas_s).ceil() as usize;
    let mut sortie: Vec<Note> = Vec::new();
    let mut i = 0;
    while i < libre.len() {
        if !libre[i] {
            i += 1;
            continue;
        }
        // La plage, trous de [`TROU_MAX`] trames compris.
        let debut = i;
        let mut fin = i;
        let mut j = i;
        while j < libre.len() && j <= fin + TROU_MAX + 1 {
            if libre[j] {
                fin = j;
            }
            j += 1;
        }
        i = fin + 1;
        if fin + 1 - debut < min {
            continue;
        }
        // Les syllabes : un creux sous le pic puis une remontée.
        let mut coupes = vec![debut];
        let (mut pic, mut creux) = (db[debut], db[debut]);
        for (k, &d) in db.iter().enumerate().take(fin + 1).skip(debut + 1) {
            let longue = k - coupes.last().expect("non vide") >= min;
            if longue && pic - creux >= ATTAQUE_DB && d - creux >= ATTAQUE_DB {
                coupes.push(k);
                (pic, creux) = (d, d);
            } else if d > pic {
                (pic, creux) = (d, d);
            } else {
                creux = creux.min(d);
            }
        }
        coupes.push(fin + 1);
        let avant = chantees
            .iter()
            .rev()
            .find(|n| n.debut_s < temps[debut] as f32)
            .map(|n| n.hauteur);
        let voisees = |a: usize, b: usize| -> Option<f32> {
            let mut h: Vec<f32> = trames[a..b].iter().flatten().copied().collect();
            (!h.is_empty()).then(|| mediane(&mut h))
        };
        let plage = voisees(debut, fin + 1);
        for w in coupes.windows(2) {
            let (a, b) = (w[0], w[1]);
            let hauteur = voisees(a, b)
                .or(plage)
                .map(|h| h.round().clamp(0.0, 127.0) as u8)
                .or(avant)
                .unwrap_or(64);
            sortie.push(Note {
                debut_s: temps[a] as f32,
                fin_s: temps[b - 1] as f32 + pas_s,
                hauteur,
                amplitude: 0.0,
                corde: None,
                frette: None,
            });
        }
    }
    sortie
}

/// Le découpage, séparé du suivi pour se tester : par trame, son instant,
/// sa hauteur (MIDI fractionnaire, `None` : non voisée), son niveau (dB) et
/// sa probabilité de voisement.
fn decouper(temps: &[f64], trames: &[Option<f32>], db: &[f32], probas: &[f32]) -> Vec<Note> {
    let pas_s = if temps.len() > 1 {
        (temps[1] - temps[0]) as f32
    } else {
        PAS as f32 / SR as f32
    };
    let tenue = (TENUE_S / pas_s).ceil().max(1.0) as usize;
    let mut notes = Vec::new();
    // La note en cours : première trame, hauteurs, probabilités, pic de
    // niveau et creux depuis ce pic.
    struct Cours {
        debut: usize,
        fin: usize,
        hauteurs: Vec<f32>,
        probas: Vec<f32>,
        pic: f32,
        creux: f32,
    }
    let fermer = |c: Cours, notes: &mut Vec<Note>| {
        let debut_s = temps[c.debut] as f32;
        let fin_s = temps[c.fin] as f32 + pas_s;
        if fin_s - debut_s < DUREE_MIN_S || c.hauteurs.is_empty() {
            return;
        }
        let mut h = c.hauteurs.clone();
        let hauteur = mediane(&mut h).round().clamp(0.0, 127.0) as u8;
        notes.push(Note {
            debut_s,
            fin_s,
            hauteur,
            amplitude: (c.probas.iter().sum::<f32>() / c.probas.len() as f32).max(CONFIANCE_MIN),
            corde: None,
            frette: None,
        });
    };
    let mut cours: Option<Cours> = None;
    let mut trou = 0;
    let mut i = 0;
    while i < trames.len() {
        let Some(m) = trames[i] else {
            trou += 1;
            if trou > TROU_MAX {
                if let Some(c) = cours.take() {
                    fermer(c, &mut notes);
                }
            }
            i += 1;
            continue;
        };
        trou = 0;
        let Some(c) = cours.as_mut() else {
            cours = Some(Cours {
                debut: i,
                fin: i,
                hauteurs: vec![m],
                probas: vec![probas[i]],
                pic: db[i],
                creux: db[i],
            });
            i += 1;
            continue;
        };
        let mut recents: Vec<f32> = c.hauteurs.iter().rev().take(8).copied().collect();
        let centre = mediane(&mut recents);
        // Un écart qui dure : les `tenue` trames voisées à venir s'écartent
        // toutes du même côté.
        let ecart = |k: usize| trames.get(k).copied().flatten().map(|h| h - centre);
        let change = (i..i + tenue).all(|k| {
            ecart(k).is_some_and(|e| e.abs() > ECART_DT && e.signum() == (m - centre).signum())
        });
        let longue = (i - c.debut) as f32 * pas_s >= DUREE_MIN_S;
        let attaque = longue && c.pic - c.creux >= ATTAQUE_DB && db[i] - c.creux >= ATTAQUE_DB;
        if change || attaque {
            let fini = cours.take().expect("en cours");
            fermer(fini, &mut notes);
            cours = Some(Cours {
                debut: i,
                fin: i,
                hauteurs: vec![m],
                probas: vec![probas[i]],
                pic: db[i],
                creux: db[i],
            });
        } else {
            c.fin = i;
            c.hauteurs.push(m);
            c.probas.push(probas[i]);
            if db[i] > c.pic {
                (c.pic, c.creux) = (db[i], db[i]);
            } else {
                c.creux = c.creux.min(db[i]);
            }
        }
        i += 1;
    }
    if let Some(c) = cours {
        fermer(c, &mut notes);
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une voix synthétique : une suite de (hauteur MIDI, durée), avec
    /// vibrato, quelques harmoniques et un silence entre les phrases.
    fn chant(phrases: &[&[(f32, f32)]]) -> Vec<f32> {
        let mut x = Vec::new();
        let mut phase = 0f32;
        for phrase in phrases {
            for &(m, d) in *phrase {
                let n = (d * SR as f32) as usize;
                for j in 0..n {
                    let t = j as f32 / SR as f32;
                    let vib = 0.3 * (2.0 * std::f32::consts::PI * 5.5 * t).sin();
                    let f = 440.0 * 2f32.powf((m + vib - 69.0) / 12.0);
                    phase += 2.0 * std::f32::consts::PI * f / SR as f32;
                    // Chaque note commence par une consonne (30 ms, 20 dB
                    // plus bas), puis attaque sur 20 ms.
                    let c = 0.03 * SR as f32;
                    let env = if (j as f32) < c {
                        0.1
                    } else {
                        ((j as f32 - c) / (0.02 * SR as f32)).clamp(0.1, 1.0)
                    };
                    x.push(
                        env * 0.3
                            * (phase.sin()
                                + 0.5 * (2.0 * phase).sin()
                                + 0.25 * (3.0 * phase).sin()),
                    );
                }
            }
            x.extend(std::iter::repeat_n(0.0, (0.4 * SR as f32) as usize));
        }
        x
    }

    #[test]
    fn une_melodie_chantee_se_decoupe_en_notes() {
        let x = chant(&[
            &[(64.0, 0.4), (67.0, 0.4), (69.0, 0.6)],
            &[(62.0, 0.5), (60.0, 0.8)],
        ]);
        let n = notes(&x, SR).unwrap();
        let h: Vec<u8> = n.iter().map(|n| n.hauteur).collect();
        assert_eq!(h, [64, 67, 69, 62, 60], "{n:#?}");
        // Les débuts à 70 ms près (la consonne de 30 ms et la tenue de
        // l'écart, 50 ms).
        let attendus = [0.0, 0.4, 0.8, 1.8, 2.3];
        for (n, a) in n.iter().zip(attendus) {
            assert!((n.debut_s - a).abs() < 0.07, "{} au lieu de {a}", n.debut_s);
        }
    }

    #[test]
    fn un_cri_entre_deux_phrases_fait_des_notes_criees() {
        // Une phrase chantée, puis trois syllabes criées (du bruit, sans
        // période), puis une note chantée.
        let mut x = chant(&[&[(64.0, 0.5)]]);
        let mut graine = 7u32;
        for k in 0..3 {
            let n = (0.25 * SR as f32) as usize;
            for j in 0..n {
                graine = graine.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let bruit = (graine >> 16) as f32 / 32768.0 - 1.0;
                // Une syllabe : monte vite, retombe ; 30 ms de quasi-silence
                // entre deux.
                let t = j as f32 / n as f32;
                let env = if t < 0.12 { 0.02 } else { (1.0 - t).max(0.05) };
                x.push(0.4 * env * bruit);
            }
            let _ = k;
        }
        x.extend(chant(&[&[(67.0, 0.5)]]));
        let n = notes(&x, SR).unwrap();
        let criees: Vec<&Note> = n.iter().filter(|n| n.amplitude == 0.0).collect();
        assert!((2..=4).contains(&criees.len()), "{n:#?}");
        // Hauteur indicative : la note chantée d'avant.
        assert!(criees.iter().all(|n| n.hauteur == 64), "{criees:#?}");
        let chantees: Vec<u8> = n
            .iter()
            .filter(|n| n.amplitude > 0.0)
            .map(|n| n.hauteur)
            .collect();
        assert_eq!(chantees, [64, 67]);
    }

    #[test]
    fn la_meme_note_repetee_fait_plusieurs_syllabes() {
        // « no, no, no » : trois attaques sur le même la, sans silence.
        let x = chant(&[&[(69.0, 0.3), (69.0, 0.3), (69.0, 0.3)]]);
        let n = notes(&x, SR).unwrap();
        assert_eq!(n.len(), 3, "{n:#?}");
        assert!(n.iter().all(|n| n.hauteur == 69));
    }
}
