// SPDX-License-Identifier: GPL-3.0-or-later
//! Les corrections de l'utilisateur sur une transcription de basse (E2,
//! `docs/plan-editer-pratique-creation.md` 1.5) : corde imposée, octave
//! ou demi-ton corrigés (V18), note supprimée ou ajoutée.
//!
//! Elles vivent dans la mémoire de Pratiquer (`pratique_correction`), pas dans
//! le cache de la transcription : une nouvelle transcription (autre version du
//! calcul, doigté refait, cache effacé par un nouveau démixage) les retrouve et
//! les réapplique **après** le doigté, si bien que le modèle ne réécrit jamais
//! une note corrigée. La note visée se retrouve par son instant et sa hauteur
//! transcrits — ce qu'une nouvelle transcription redonne, à quelques
//! millisecondes près ([`TOLERANCE_S`]).

use rusty_music_core::db::CorrectionPratique;
use serde::{Deserialize, Serialize};

use crate::{tablature::Accordage, Note};

/// Écart toléré entre l'instant d'une correction et l'attaque d'une note
/// retranscrite : bien moins qu'une double croche (125 ms à 120 BPM), assez
/// pour un modèle qui date une attaque d'une trame plus tôt ou plus tard.
pub const TOLERANCE_S: f32 = 0.03;

/// Ce que l'interface doit savoir d'une note pour la corriger : son identité
/// (instant et hauteur transcrits, ceux qu'une correction vise) et ce qui y
/// est déjà corrigé.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Identite {
    pub debut_s: f32,
    pub hauteur: u8,
    /// Ajoutée à la main : la supprimer l'oublie.
    #[serde(default)]
    pub ajoutee: bool,
    /// Octaves de la correction (0 sans correction).
    #[serde(default)]
    pub octaves: i8,
    /// Demi-tons de la correction (0 sans correction), en plus des octaves.
    #[serde(default)]
    pub demitons: i8,
    /// La corde est imposée par une correction.
    #[serde(default)]
    pub corde_imposee: bool,
}

impl Identite {
    pub fn corrigee(&self) -> bool {
        self.ajoutee || self.octaves != 0 || self.demitons != 0 || self.corde_imposee
    }
}

/// Une transcription corrigée.
#[derive(Debug, Clone, PartialEq)]
pub struct Corrigee {
    /// Les notes, triées par attaque.
    pub notes: Vec<Note>,
    /// Une par note, au même rang.
    pub identites: Vec<Identite>,
    /// Les corrections qui n'ont trouvé aucune note (la transcription a trop
    /// changé), ou qu'on ne peut pas jouer sur ce manche : laissées en base,
    /// sans effet.
    pub sans_effet: usize,
}

/// Applique `corrections` (dans l'ordre de `Library::corrections_pratique` :
/// ajouts, suppressions, octaves, demi-tons, cordes) aux notes d'une transcription déjà
/// doigtée. `transcrites` : les hauteurs transcrites, avant que le doigté ne
/// ramène à l'octave ce qui sort du manche (le champ `hauteurs` du cache) ;
/// les hauteurs des notes si elles manquent.
pub fn appliquer(
    notes: &[Note],
    transcrites: &[u8],
    corrections: &[CorrectionPratique],
    accordage: &Accordage,
) -> Corrigee {
    let mut lignes: Vec<(Note, Identite)> = notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let hauteur = if transcrites.len() == notes.len() {
                transcrites[i]
            } else {
                n.hauteur
            };
            (
                n.clone(),
                Identite {
                    debut_s: n.debut_s,
                    hauteur,
                    ajoutee: false,
                    octaves: 0,
                    demitons: 0,
                    corde_imposee: false,
                },
            )
        })
        .collect();
    let mut sans_effet = 0;
    for c in corrections {
        let debut = c.debut_s as f32;
        if c.genre == "ajouter" {
            let mut note = Note {
                debut_s: debut,
                fin_s: debut + c.duree_s.unwrap_or(0.25) as f32,
                hauteur: c.hauteur,
                amplitude: 1.0,
                corde: None,
                frette: None,
            };
            let voisine = lignes
                .iter()
                .filter(|(n, _)| n.debut_s <= debut)
                .max_by(|a, b| a.0.debut_s.total_cmp(&b.0.debut_s))
                .and_then(|(n, _)| n.frette);
            let place = match c.corde {
                Some(corde) => sur_la_corde(c.hauteur, corde, accordage),
                None => placer(c.hauteur, voisine, accordage),
            };
            let Some((corde, frette)) = place else {
                sans_effet += 1;
                continue;
            };
            (note.corde, note.frette) = (Some(corde), Some(frette));
            lignes.push((
                note,
                Identite {
                    debut_s: debut,
                    hauteur: c.hauteur,
                    ajoutee: true,
                    octaves: 0,
                    demitons: 0,
                    corde_imposee: c.corde.is_some(),
                },
            ));
            continue;
        }
        // La note visée : même hauteur transcrite, l'attaque la plus proche.
        let visee = lignes
            .iter()
            .enumerate()
            .filter(|(_, (_, id))| {
                id.hauteur == c.hauteur && (id.debut_s - debut).abs() < TOLERANCE_S
            })
            .min_by(|a, b| {
                (a.1 .1.debut_s - debut)
                    .abs()
                    .total_cmp(&(b.1 .1.debut_s - debut).abs())
            })
            .map(|(i, _)| i);
        let Some(i) = visee else {
            sans_effet += 1;
            continue;
        };
        if c.genre == "supprimer" {
            lignes.remove(i);
            continue;
        }
        let (note, id) = &mut lignes[i];
        // La note transposée de `ecart` demi-tons, reposée au plus près de la
        // main ; rien si elle sort du manche.
        let transposer = |note: &mut Note, ecart: i16| -> bool {
            let h = note.hauteur as i16 + ecart;
            match u8::try_from(h)
                .ok()
                .and_then(|h| placer(h, note.frette, accordage).map(|p| (h, p)))
            {
                Some((h, (corde, frette))) => {
                    note.hauteur = h;
                    (note.corde, note.frette) = (Some(corde), Some(frette));
                    true
                }
                None => false,
            }
        };
        let applique = match c.genre.as_str() {
            "octave" => {
                let o = c.octaves.unwrap_or(0);
                let fait = transposer(note, 12 * o as i16);
                if fait {
                    id.octaves = o;
                }
                fait
            }
            "demiton" => {
                let d = c.demitons.unwrap_or(0);
                let fait = transposer(note, d as i16);
                if fait {
                    id.demitons = d;
                }
                fait
            }
            "corde" => match c
                .corde
                .and_then(|corde| sur_la_corde(note.hauteur, corde, accordage))
            {
                Some((corde, frette)) => {
                    (note.corde, note.frette) = (Some(corde), Some(frette));
                    id.corde_imposee = true;
                    true
                }
                None => false,
            },
            _ => false,
        };
        if applique {
            // Corrigée, la note est sûre : la partition ne l'estompe plus, et
            // une attaque voisine sur la même double croche ne la remplace pas.
            note.amplitude = 1.0;
        } else {
            sans_effet += 1;
        }
    }
    lignes.sort_by(|a, b| a.0.debut_s.total_cmp(&b.0.debut_s));
    let (notes, identites) = lignes.into_iter().unzip();
    Corrigee {
        notes,
        identites,
        sans_effet,
    }
}

/// La frette de `hauteur` sur `corde`, si elle tombe sur le manche.
fn sur_la_corde(hauteur: u8, corde: u8, accordage: &Accordage) -> Option<(u8, u8)> {
    let vide = *accordage.cordes.get(corde as usize)?;
    let frette = hauteur.checked_sub(vide)?;
    (frette <= accordage.frettes).then_some((corde, frette))
}

/// Où jouer `hauteur` : la position la plus proche de la frette `pres` (celle
/// de la note d'avant la correction, ou de la note précédente), la plus basse
/// sans repère.
pub fn placer(hauteur: u8, pres: Option<u8>, accordage: &Accordage) -> Option<(u8, u8)> {
    (0..accordage.cordes.len() as u8)
        .filter_map(|c| sur_la_corde(hauteur, c, accordage))
        .min_by_key(|&(_, f)| (pres.map_or(f as i16, |p| (f as i16 - p as i16).abs()), f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{quantification, tablature};

    fn note(debut_s: f32, hauteur: u8) -> Note {
        Note {
            debut_s,
            fin_s: debut_s + 0.4,
            hauteur,
            amplitude: 0.5,
            corde: None,
            frette: None,
        }
    }

    fn c(genre: &str, debut_s: f32, hauteur: u8) -> CorrectionPratique {
        CorrectionPratique {
            genre: genre.into(),
            debut_s: debut_s as f64,
            hauteur,
            duree_s: None,
            corde: None,
            frette: None,
            octaves: None,
            demitons: None,
        }
    }

    /// Une « transcription » : quatre noires à 120 BPM, doigtées comme le
    /// fait l'application.
    fn transcrire(decalage: f32) -> (Vec<Note>, Vec<u8>) {
        let mut notes: Vec<Note> = [40u8, 45, 33, 43]
            .iter()
            .enumerate()
            .map(|(i, &h)| note(0.5 + 0.5 * i as f32 + decalage, h))
            .collect();
        let hauteurs = notes.iter().map(|n| n.hauteur).collect();
        tablature::poser(&mut notes, &tablature::Accordage::basse4());
        (notes, hauteurs)
    }

    fn corrections() -> Vec<CorrectionPratique> {
        // Dans l'ordre de la base : ajout, suppression, octave, corde.
        vec![
            CorrectionPratique {
                duree_s: Some(0.25),
                ..c("ajouter", 2.75, 38)
            },
            c("supprimer", 1.5, 33),
            CorrectionPratique {
                octaves: Some(-1),
                ..c("octave", 1.0, 45)
            },
            // V18 : sol2 transcrit, corrigé en fa♯2.
            CorrectionPratique {
                demitons: Some(-1),
                ..c("demiton", 2.0, 43)
            },
            CorrectionPratique {
                corde: Some(1),
                ..c("corde", 0.5, 40)
            },
        ]
    }

    #[test]
    fn les_quatre_corrections_s_appliquent() {
        let a = tablature::Accordage::basse4();
        let (notes, hauteurs) = transcrire(0.0);
        let r = appliquer(&notes, &hauteurs, &corrections(), &a);
        assert_eq!(r.sans_effet, 0);
        let vues: Vec<(f32, u8, Option<u8>, Option<u8>)> = r
            .notes
            .iter()
            .map(|n| (n.debut_s, n.hauteur, n.corde, n.frette))
            .collect();
        assert_eq!(
            vues,
            vec![
                // mi2 imposé sur la corde de la (frette 7).
                (0.5, 40, Some(1), Some(7)),
                // la2 descendu d'une octave : la1, au plus près de la main
                // (5ᵉ frette de mi plutôt que la corde à vide).
                (1.0, 33, Some(0), Some(5)),
                // la1 supprimé ; sol2 corrigé en fa♯2, une frette plus bas.
                (2.0, 42, Some(1), Some(9)),
                // ré2 ajouté, posé près de la note d'avant.
                (2.75, 38, Some(0), Some(10)),
            ]
        );
        assert!(r.identites[0].corde_imposee);
        assert_eq!(r.identites[1].octaves, -1);
        // L'identité reste la hauteur transcrite.
        assert_eq!(r.identites[1].hauteur, 45);
        assert_eq!(r.identites[2].demitons, -1);
        assert_eq!(r.identites[2].hauteur, 43);
        assert!(r.identites[3].ajoutee);
        assert_eq!(r.notes[0].amplitude, 1.0);
        assert_eq!(r.notes[2].amplitude, 1.0);
    }

    /// Le critère d'E2 : une nouvelle transcription (les attaques datées
    /// 12 ms plus tard, le doigté refait) garde chaque correction.
    #[test]
    fn une_nouvelle_transcription_garde_les_corrections() {
        let a = tablature::Accordage::basse4();
        let (notes, hauteurs) = transcrire(0.0);
        let avant = appliquer(&notes, &hauteurs, &corrections(), &a);
        let (notes, hauteurs) = transcrire(0.012);
        let apres = appliquer(&notes, &hauteurs, &corrections(), &a);
        assert_eq!(apres.sans_effet, 0);
        let jeu = |r: &Corrigee| -> Vec<(u8, Option<u8>, Option<u8>)> {
            r.notes
                .iter()
                .map(|n| (n.hauteur, n.corde, n.frette))
                .collect()
        };
        assert_eq!(jeu(&avant), jeu(&apres));
        // Quantifiées sur la même grille, les deux partitions sont les mêmes,
        // et chaque note jouée renvoie à son rang.
        let temps: Vec<f32> = (0..16).map(|i| 0.5 + 0.5 * i as f32).collect();
        let premiers = [0.5, 2.5, 4.5, 6.5];
        let q = |r: &Corrigee| {
            quantification::quantifier(&r.notes, &temps, &premiers, 4)
                .into_iter()
                .flat_map(|m| m.evenements)
                .map(|e| (e.seiziemes, e.jeu, e.note))
                .collect::<Vec<_>>()
        };
        assert_eq!(q(&avant), q(&apres));
        let rangs: Vec<u32> = q(&apres).into_iter().filter_map(|e| e.2).collect();
        assert_eq!(rangs, vec![0, 1, 2, 3]);
    }

    #[test]
    fn une_correction_sans_note_reste_sans_effet() {
        let a = tablature::Accordage::basse4();
        let (notes, hauteurs) = transcrire(0.0);
        let r = appliquer(
            &notes,
            &hauteurs,
            &[
                // Plus de note à cet instant.
                c("supprimer", 9.0, 40),
                // mi1 n'a pas de corde plus grave que la sienne.
                CorrectionPratique {
                    octaves: Some(-2),
                    ..c("octave", 0.5, 40)
                },
                // Onze demi-tons sous la1 : sous la corde à vide de mi.
                CorrectionPratique {
                    demitons: Some(-11),
                    ..c("demiton", 1.5, 33)
                },
                // Hors du manche sur la corde de sol.
                CorrectionPratique {
                    corde: Some(3),
                    ..c("corde", 1.5, 33)
                },
            ],
            &a,
        );
        assert_eq!(r.sans_effet, 4);
        assert_eq!(r.notes, notes);
    }
}
