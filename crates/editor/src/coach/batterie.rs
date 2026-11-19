// SPDX-License-Identifier: GPL-3.0-or-later
//! La batterie électronique en MIDI : des messages aux coups (E4, tranche 2,
//! `docs/recherche-coach-pratique.md`, « La batterie : MIDI d'abord »).
//!
//! Une batterie électronique donne l'instant, le pad et la vélocité exacts ;
//! le seul travail est la **correspondance** entre ses notes et les cinq
//! classes de la transcription ADTOF de la référence. General MIDI ne suffit
//! pas : Roland, Alesis et Yamaha émettent des notes de zone (bord, cercle)
//! différentes, et le charleston s'ouvre en continu par le CC4. D'où des
//! profils éditables, gardés par périphérique ([`Profil`]), et un
//! [`Decodeur`] qui lit les messages bruts (ce que `midir` rend) avec l'état
//! du CC4.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Les cinq classes de la transcription ADTOF, dans son ordre et sous ses
/// noms sérialisés (`rusty_music_transcription::batterie::Piece`).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classe {
    GrosseCaisse,
    CaisseClaire,
    Toms,
    Charleston,
    Cymbales,
}

impl Classe {
    pub const TOUTES: [Classe; 5] = [
        Classe::GrosseCaisse,
        Classe::CaisseClaire,
        Classe::Toms,
        Classe::Charleston,
        Classe::Cymbales,
    ];
}

/// Le contrôleur continu de l'ouverture du charleston (« foot controller »).
pub const CC_CHARLESTON: u8 = 4;

/// Une correspondance notes → classes, pour un périphérique.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profil {
    pub nom: String,
    /// Note MIDI → classe ; une note absente est ignorée (pad d'effet,
    /// déclencheur auxiliaire).
    pub notes: BTreeMap<u8, Classe>,
    /// Le CC qui porte l'ouverture du charleston (4 sur presque tous les
    /// modules), `None` pour l'ignorer.
    #[serde(default)]
    pub cc_charleston: Option<u8>,
    /// Vélocité sous laquelle un coup est tenu pour un faux déclenchement
    /// (diaphonie entre pads, rebond). 1 : tout compte.
    #[serde(default = "velocite_min_defaut")]
    pub velocite_min: u8,
}

fn velocite_min_defaut() -> u8 {
    1
}

impl Default for Profil {
    fn default() -> Self {
        Self::general_midi()
    }
}

impl Profil {
    /// General MIDI (canal 10), complété des notes de zone que les modules
    /// courants (Roland TD, Alesis, Yamaha DTX) émettent : bords de
    /// charleston 22/26, cercle de tom grave 58, bords de cymbales 52/55/59.
    pub fn general_midi() -> Self {
        use Classe::*;
        let mut notes = BTreeMap::new();
        let mut poser = |ns: &[u8], c: Classe| {
            for &n in ns {
                notes.insert(n, c);
            }
        };
        // 35 grosse caisse acoustique, 36 grosse caisse 1.
        poser(&[35, 36], GrosseCaisse);
        // 37 bâton sur le cercle, 38 caisse claire, 39 claquement de mains
        // (souvent le cercle sur les modules), 40 caisse claire électrique
        // (le cercle chez Roland).
        poser(&[37, 38, 39, 40], CaisseClaire);
        // 41 43 toms basse, 45 47 toms médium, 48 50 toms aigus, 58 cercle
        // du tom grave (Roland).
        poser(&[41, 43, 45, 47, 48, 50, 58], Toms);
        // 42 fermé, 44 pédale, 46 ouvert ; 22 et 26 : bords fermé et ouvert
        // (Roland).
        poser(&[22, 26, 42, 44, 46], Charleston);
        // 49 57 crash, 52 chinoise, 55 splash, 51 59 ride, 53 cloche de ride.
        poser(&[49, 51, 52, 53, 55, 57, 59], Cymbales);
        Profil {
            nom: "General MIDI".into(),
            notes,
            cc_charleston: Some(CC_CHARLESTON),
            velocite_min: 1,
        }
    }

    /// Refuse un profil qu'on ne saurait pas lire.
    pub fn verifier(&self) -> Result<(), String> {
        if self.nom.trim().is_empty() {
            return Err("profil sans nom".into());
        }
        if let Some(n) = self.notes.keys().find(|&&n| n > 127) {
            return Err(format!("note MIDI hors bornes : {n}"));
        }
        if self.cc_charleston.is_some_and(|c| c > 127) || self.velocite_min > 127 {
            return Err("contrôleur ou vélocité hors bornes".into());
        }
        if self.notes.is_empty() {
            return Err("aucune note n'est affectée".into());
        }
        Ok(())
    }
}

/// Un coup reçu de la batterie.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frappe {
    /// Instant de réception, en secondes de l'horloge commune
    /// (`rusty_music_player::horloge::maintenant_s`).
    pub t_s: f64,
    pub classe: Classe,
    pub note: u8,
    pub velocite: u8,
    /// Ouverture du charleston au moment du coup (0 fermé, 127 ouvert),
    /// pour un coup de charleston dont le module a envoyé le CC.
    pub ouverture: Option<u8>,
}

/// Lit les messages MIDI bruts d'un périphérique, avec l'état qu'ils
/// portent d'un message à l'autre (ouverture du charleston).
#[derive(Debug, Clone, Default)]
pub struct Decodeur {
    profil: Profil,
    ouverture: Option<u8>,
}

impl Decodeur {
    pub fn nouveau(profil: Profil) -> Self {
        Self {
            profil,
            ouverture: None,
        }
    }

    pub fn profil(&self) -> &Profil {
        &self.profil
    }

    /// La note et la vélocité d'un note-on frappé (vélocité non nulle, tous
    /// canaux), affecté ou non : ce que l'éditeur du profil montre.
    pub fn note_frappee(message: &[u8]) -> Option<(u8, u8)> {
        match message {
            [statut, note, velocite, ..] if statut & 0xF0 == 0x90 && velocite & 0x7F > 0 => {
                Some((note & 0x7F, velocite & 0x7F))
            }
            _ => None,
        }
    }

    /// Un message reçu à `t_s` : un coup s'il en est un (note-on de vélocité
    /// non nulle, note affectée, au-dessus du seuil), sinon `None`. Tous les
    /// canaux sont lus : les modules n'émettent pas tous sur le 10.
    pub fn lire(&mut self, t_s: f64, message: &[u8]) -> Option<Frappe> {
        let (&statut, donnees) = message.split_first()?;
        match statut & 0xF0 {
            0x90 => {
                let (&note, &velocite) = (donnees.first()?, donnees.get(1)?);
                if velocite == 0 || velocite < self.profil.velocite_min {
                    return None; // note-off déguisé, ou faux déclenchement
                }
                let classe = *self.profil.notes.get(&(note & 0x7F))?;
                Some(Frappe {
                    t_s,
                    classe,
                    note: note & 0x7F,
                    velocite: velocite & 0x7F,
                    ouverture: (classe == Classe::Charleston)
                        .then_some(self.ouverture)
                        .flatten(),
                })
            }
            0xB0 => {
                let (&cc, &valeur) = (donnees.first()?, donnees.get(1)?);
                if Some(cc) == self.profil.cc_charleston {
                    self.ouverture = Some(valeur & 0x7F);
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_midi_couvre_les_cinq_classes() {
        let p = Profil::general_midi();
        p.verifier().unwrap();
        for c in Classe::TOUTES {
            assert!(p.notes.values().any(|&x| x == c), "{c:?}");
        }
        assert_eq!(p.notes[&36], Classe::GrosseCaisse);
        assert_eq!(p.notes[&38], Classe::CaisseClaire);
        assert_eq!(p.notes[&44], Classe::Charleston);
        assert_eq!(p.notes[&51], Classe::Cymbales);
    }

    #[test]
    fn les_noms_suivent_ceux_d_adtof() {
        let json = serde_json::to_string(&Classe::TOUTES).unwrap();
        assert_eq!(
            json,
            r#"["grosse_caisse","caisse_claire","toms","charleston","cymbales"]"#
        );
    }

    #[test]
    fn le_decodeur_lit_notes_cc4_et_seuil() {
        let mut p = Profil::general_midi();
        p.velocite_min = 10;
        let mut d = Decodeur::nouveau(p);
        // Caisse claire sur le canal 10 (0x99).
        let f = d.lire(1.0, &[0x99, 38, 100]).unwrap();
        assert_eq!(
            (f.classe, f.velocite, f.ouverture),
            (Classe::CaisseClaire, 100, None)
        );
        // Note-on de vélocité nulle : un note-off.
        assert!(d.lire(1.1, &[0x99, 38, 0]).is_none());
        // Sous le seuil : diaphonie.
        assert!(d.lire(1.2, &[0x99, 38, 5]).is_none());
        // Note non affectée.
        assert!(d.lire(1.3, &[0x99, 81, 90]).is_none());
        // CC4 puis charleston : l'ouverture suit ; un autre CC n'y touche pas.
        assert!(d.lire(1.4, &[0xB9, 4, 90]).is_none());
        assert!(d.lire(1.45, &[0xB9, 7, 10]).is_none());
        let h = d.lire(1.5, &[0x99, 46, 70]).unwrap();
        assert_eq!((h.classe, h.ouverture), (Classe::Charleston, Some(90)));
        // Un autre canal (le 1) est lu aussi.
        assert_eq!(
            d.lire(1.6, &[0x90, 36, 64]).unwrap().classe,
            Classe::GrosseCaisse
        );
        // Messages tronqués ou d'autre nature : ignorés sans paniquer.
        assert!(d.lire(1.7, &[0x99, 38]).is_none());
        assert!(d.lire(1.7, &[]).is_none());
        assert!(d.lire(1.7, &[0xF8]).is_none());
    }

    #[test]
    fn un_profil_se_relit_en_json() {
        let p = Profil::general_midi();
        let j = serde_json::to_string(&p).unwrap();
        let q: Profil = serde_json::from_str(&j).unwrap();
        assert_eq!(p, q);
        // Un profil ancien sans seuil prend 1.
        let r: Profil =
            serde_json::from_str(r#"{"nom":"x","notes":{"36":"grosse_caisse"}}"#).unwrap();
        assert_eq!((r.velocite_min, r.cc_charleston), (1, None));
    }
}
