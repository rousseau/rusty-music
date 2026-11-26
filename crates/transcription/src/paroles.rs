// SPDX-License-Identifier: GPL-3.0-or-later
//! Les paroles sous la partition de voix : chaque ligne du texte, coupée en
//! syllabes, posée sur les notes chantées pendant cette ligne.
//!
//! Le texte vient d'ailleurs (tags du fichier, LRCLIB) ; ici, seulement sa
//! lecture et son placement :
//! 1. [`lire`] : des paroles datées (LRC, `[mm:ss.cc] texte`) ou du texte
//!    seul, une ligne par vers ; les en-têtes (`[Chorus]`) et les lignes
//!    vides tombent ;
//! 2. [`syllabes`] : la césure des dictionnaires TeX (`hyphenation`),
//!    anglais ou français selon les mots les plus courants du texte, minima
//!    du dictionnaire (réduits à une lettre, ils coupaient « y-ou ») ;
//! 3. [`poser`] : une ligne datée occupe les notes qui commencent entre son
//!    instant et celui de la suivante (un peu avant : les LRC datent souvent
//!    l'entrée du vers en avance) ; ses syllabes s'y répartissent de façon
//!    régulière — une note de trop reste sans texte (mélisme, vibrato coupé
//!    en deux notes), une syllabe de trop rejoint sa voisine sur la même
//!    note. Du texte seul se pose de même, phrase chantée par phrase quand
//!    les nombres de vers et de phrases concordent, sinon d'un bloc.
//!
//! Premier jet (26 nov.), sans banc.

use std::sync::OnceLock;

use hyphenation::{Hyphenator, Language, Load, Standard};
use serde::{Deserialize, Serialize};

use crate::Note;

/// Une ligne des paroles ; `debut_s` : son instant, si le texte est daté.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ligne {
    pub debut_s: Option<f32>,
    pub texte: String,
}

/// Une syllabe posée : le rang de sa note dans les notes données à
/// [`poser`] (celui de `quantification::Evenement::note`) et son texte, qui
/// finit par « - » quand le mot continue sur la note suivante.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Syllabe {
    pub note: u32,
    pub texte: String,
}

/// Avance tolérée d'une ligne datée sur le chant.
pub const AVANCE_S: f32 = 0.3;
/// Durée prêtée à la dernière ligne datée.
const DERNIERE_S: f32 = 8.0;
/// Silence qui sépare deux phrases chantées, pour du texte non daté.
const SOUFFLE_S: f32 = 0.8;

/// Lit des paroles : LRC si au moins une ligne porte un instant, sinon du
/// texte, une ligne par vers.
pub fn lire(texte: &str) -> Vec<Ligne> {
    let mut lignes = Vec::new();
    let mut datees = false;
    for brute in texte.lines() {
        let mut reste = brute.trim();
        let mut instants = Vec::new();
        while let Some(r) = reste.strip_prefix('[') {
            let Some(fin) = r.find(']') else { break };
            let balise = &r[..fin];
            reste = r[fin + 1..].trim_start();
            if let Some(t) = instant(balise) {
                instants.push(t);
            } else {
                // Métadonnée LRC (`[ar:…]`) ou en-tête de section (`[Chorus]`).
                instants.clear();
                reste = "";
                break;
            }
        }
        let propre = reste.trim();
        if propre.is_empty() {
            continue;
        }
        if instants.is_empty() {
            lignes.push(Ligne {
                debut_s: None,
                texte: propre.to_string(),
            });
        } else {
            datees = true;
            for t in instants {
                lignes.push(Ligne {
                    debut_s: Some(t),
                    texte: propre.to_string(),
                });
            }
        }
    }
    if datees {
        lignes.retain(|l| l.debut_s.is_some());
        lignes.sort_by(|a, b| a.debut_s.unwrap().total_cmp(&b.debut_s.unwrap()));
    }
    lignes
}

/// `mm:ss`, `mm:ss.cc` ou `mm:ss:cc` en secondes.
fn instant(balise: &str) -> Option<f32> {
    let (m, s) = balise.split_once(':')?;
    let m: f32 = m.trim().parse().ok()?;
    let s: f32 = s.trim().replacen(':', ".", 1).parse().ok()?;
    (m >= 0.0 && (0.0..60.0).contains(&s)).then_some(60.0 * m + s)
}

fn dictionnaire(langue: Language) -> Option<&'static Standard> {
    static ANGLAIS: OnceLock<Option<Standard>> = OnceLock::new();
    static FRANCAIS: OnceLock<Option<Standard>> = OnceLock::new();
    let cellule = if langue == Language::French {
        &FRANCAIS
    } else {
        &ANGLAIS
    };
    cellule
        .get_or_init(|| Standard::from_embedded(langue).ok())
        .as_ref()
}

/// Anglais ou français, aux mots outils les plus fréquents du texte.
pub fn langue(lignes: &[Ligne]) -> Language {
    const FR: &[&str] = &[
        "le", "la", "les", "de", "des", "et", "je", "tu", "il", "elle", "que", "qui", "pas", "est",
        "un", "une", "du", "en", "ne", "mon", "ma", "moi", "toi", "nous", "vous", "ce", "dans",
    ];
    const EN: &[&str] = &[
        "the", "and", "you", "i", "me", "to", "a", "of", "in", "my", "it", "is", "your", "on",
        "we", "be", "for", "that", "with", "all", "don't", "i'm", "it's",
    ];
    let (mut fr, mut en) = (0, 0);
    for l in lignes {
        for m in l.texte.split_whitespace() {
            let m = m
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase();
            fr += FR.contains(&m.as_str()) as usize;
            en += EN.contains(&m.as_str()) as usize;
        }
    }
    if fr > en {
        Language::French
    } else {
        Language::EnglishUS
    }
}

/// Les syllabes d'une ligne, dans l'ordre : chaque mot coupé selon le
/// dictionnaire, « - » au bout des syllabes que le mot continue.
pub fn syllabes(ligne: &str, langue: Language) -> Vec<String> {
    let mut sortie = Vec::new();
    for mot in ligne.split_whitespace() {
        // La césure se fait sur les lettres ; la ponctuation reste collée.
        let debut = mot.find(|c: char| c.is_alphanumeric());
        let fin = mot
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_alphanumeric())
            .map(|(i, c)| i + c.len_utf8());
        let (Some(debut), Some(fin)) = (debut, fin) else {
            // Que de la ponctuation (« - », « … ») : à la syllabe d'avant.
            if let Some(der) = sortie.last_mut() {
                let der: &mut String = der;
                der.push_str(mot);
            }
            continue;
        };
        let coeur = &mot[debut..fin];
        let coupures: Vec<usize> = match dictionnaire(langue) {
            // Coupé en minuscules : mêmes indices tant que la casse ne change
            // pas la longueur en octets, sinon sans césure.
            Some(d) if coeur.to_lowercase().len() == coeur.len() => d
                .hyphenate(&coeur.to_lowercase())
                .breaks
                .into_iter()
                .filter(|&i| coeur.is_char_boundary(i))
                .collect(),
            _ => Vec::new(),
        };
        let mut bornes = vec![0];
        bornes.extend(coupures);
        bornes.push(coeur.len());
        let n = bornes.len() - 1;
        for k in 0..n {
            let mut s = String::new();
            if k == 0 {
                s.push_str(&mot[..debut]);
            }
            s.push_str(&coeur[bornes[k]..bornes[k + 1]]);
            if k + 1 == n {
                s.push_str(&mot[fin..]);
            } else {
                s.push('-');
            }
            sortie.push(s);
        }
    }
    sortie
}

/// Pose les syllabes de `syl` sur les notes de rangs `notes` (dans l'ordre
/// du chant), régulièrement.
fn repartir(syl: &[String], notes: &[u32], sortie: &mut Vec<Syllabe>) {
    if syl.is_empty() || notes.is_empty() {
        return;
    }
    let (s, n) = (syl.len(), notes.len());
    let mut courante: Option<(u32, String)> = None;
    for (k, texte) in syl.iter().enumerate() {
        let note = notes[k * n / s];
        match &mut courante {
            Some((r, t)) if *r == note => {
                // Deux syllabes sur une note : le trait d'union tombe, le mot
                // se recolle ; deux mots gardent leur espace.
                if t.ends_with('-') {
                    t.pop();
                } else {
                    t.push(' ');
                }
                t.push_str(texte);
            }
            _ => {
                if let Some((r, t)) = courante.take() {
                    sortie.push(Syllabe { note: r, texte: t });
                }
                courante = Some((note, texte.clone()));
            }
        }
    }
    if let Some((r, t)) = courante {
        sortie.push(Syllabe { note: r, texte: t });
    }
}

/// Les syllabes des paroles `lignes`, posées sur `notes` (celles de la
/// transcription de la voix, dans leur ordre : le rang d'une note est celui
/// que la partition connaît).
pub fn poser(lignes: &[Ligne], notes: &[Note]) -> Vec<Syllabe> {
    let langue = langue(lignes);
    let mut ordre: Vec<u32> = (0..notes.len() as u32).collect();
    ordre.sort_by(|&a, &b| {
        notes[a as usize]
            .debut_s
            .total_cmp(&notes[b as usize].debut_s)
    });
    let mut sortie = Vec::new();
    if lignes.iter().all(|l| l.debut_s.is_some()) {
        for (i, l) in lignes.iter().enumerate() {
            let debut = l.debut_s.unwrap() - AVANCE_S;
            let fin = lignes
                .get(i + 1)
                .and_then(|s| s.debut_s)
                .map_or(debut + AVANCE_S + DERNIERE_S, |t| t - AVANCE_S);
            let dedans: Vec<u32> = ordre
                .iter()
                .copied()
                .filter(|&r| (debut..fin).contains(&notes[r as usize].debut_s))
                .collect();
            repartir(&syllabes(&l.texte, langue), &dedans, &mut sortie);
        }
        return sortie;
    }
    // Du texte seul : les phrases chantées, séparées par un souffle.
    let mut phrases: Vec<Vec<u32>> = Vec::new();
    let mut fin_prec = f32::NEG_INFINITY;
    for &r in &ordre {
        let n = &notes[r as usize];
        if n.debut_s - fin_prec > SOUFFLE_S || phrases.is_empty() {
            phrases.push(Vec::new());
        }
        phrases.last_mut().expect("non vide").push(r);
        fin_prec = fin_prec.max(n.fin_s);
    }
    if phrases.len() == lignes.len() {
        for (l, p) in lignes.iter().zip(&phrases) {
            repartir(&syllabes(&l.texte, langue), p, &mut sortie);
        }
    } else {
        let tout: Vec<String> = lignes
            .iter()
            .flat_map(|l| syllabes(&l.texte, langue))
            .collect();
        repartir(&tout, &ordre, &mut sortie);
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(debut: f32) -> Note {
        Note {
            debut_s: debut,
            fin_s: debut + 0.3,
            hauteur: 64,
            amplitude: 1.0,
            corde: None,
            frette: None,
        }
    }

    #[test]
    fn une_lrc_se_lit_et_se_trie() {
        let l = lire(
            "[ar:AC/DC]\n[00:12.50]Back in black\n\n[00:10.00][00:30.00]I hit the sack\n[Chorus]",
        );
        let t: Vec<(f32, &str)> = l
            .iter()
            .map(|l| (l.debut_s.unwrap(), l.texte.as_str()))
            .collect();
        assert_eq!(
            t,
            [
                (10.0, "I hit the sack"),
                (12.5, "Back in black"),
                (30.0, "I hit the sack")
            ]
        );
        let l = lire("[Verse 1]\nBonjour toi\n\nAu revoir");
        assert!(l.iter().all(|l| l.debut_s.is_none()) && l.len() == 2);
    }

    #[test]
    fn les_mots_se_coupent_en_syllabes() {
        let s = syllabes("Thunderstruck, yeah!", Language::EnglishUS);
        assert!(s.len() >= 3, "{s:?}");
        assert!(s[0].ends_with('-') && s[0].starts_with('T'));
        assert_eq!(s.concat().replace('-', ""), "Thunderstruck,yeah!");
        assert_eq!(s.last().unwrap(), "yeah!");
        let f = syllabes("Donne-moi ça", Language::French);
        assert_eq!(f.concat().replace('-', ""), "Donnemoiça");
    }

    #[test]
    fn la_langue_se_devine() {
        let fr = lire("Je ne sais pas\nce que tu veux de moi");
        assert_eq!(langue(&fr), Language::French);
        let en = lire("I want you to know\nthat it's all for me");
        assert_eq!(langue(&en), Language::EnglishUS);
    }

    #[test]
    fn une_ligne_datee_se_pose_sur_ses_notes() {
        // Deux lignes, quatre notes chacune ; la première a trois syllabes,
        // la seconde six.
        let notes: Vec<Note> = [1.0, 1.5, 2.0, 2.5, 5.0, 5.5, 6.0, 6.5]
            .iter()
            .map(|&t| note(t))
            .collect();
        let l = lire("[00:01.10]Back in black\n[00:04.90]hit the sack, been too long");
        let s = poser(&l, &notes);
        let a: Vec<(u32, &str)> = s.iter().map(|s| (s.note, s.texte.as_str())).collect();
        // Trois syllabes sur quatre notes : la dernière note sans texte.
        assert_eq!(&a[..3], [(0, "Back"), (1, "in"), (2, "black")]);
        // Six syllabes sur quatre notes : certaines notes en portent deux.
        assert!(a[3..].iter().all(|&(n, _)| (4..8).contains(&n)));
        let texte: Vec<&str> = a[3..].iter().map(|x| x.1).collect();
        assert_eq!(texte.join(" "), "hit the sack, been too long");
    }

    #[test]
    fn du_texte_seul_se_pose_phrase_par_phrase() {
        let notes: Vec<Note> = [0.0, 0.4, 3.0, 3.4].iter().map(|&t| note(t)).collect();
        let s = poser(&lire("Hey you\nGo home"), &notes);
        let a: Vec<(u32, &str)> = s.iter().map(|s| (s.note, s.texte.as_str())).collect();
        assert_eq!(a, [(0, "Hey"), (1, "you"), (2, "Go"), (3, "home")]);
    }
}
