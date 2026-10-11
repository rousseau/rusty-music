// SPDX-License-Identifier: GPL-3.0-or-later
//! Vocabulaire MT3 de MuScriptor et automate qui tire des notes du flux de
//! jetons — port de `tokenizer/notes.py`, `tokenizer/mt3.py` et de
//! `OpenNoteTracker` (`events.py`) du code d'origine (MIT).
//!
//! Disposition fixe : trois jetons spéciaux (PAD, EOS, UNK), 1 001 décalages
//! de temps (pas de 10 ms depuis le début du segment), 128 hauteurs, 2
//! vélocités (0 = fin, 1 = début), `tie`, 130 programmes, 128 frappes de
//! batterie — 1 393 jetons.

pub const EOS: u32 = 1;
const DECALAGE: u32 = 3;
const DECALAGES: u32 = 1001;
const HAUTEUR: u32 = DECALAGE + DECALAGES; // 1004
const VELOCITE: u32 = HAUTEUR + 128; // 1132
pub const TIE: u32 = VELOCITE + 2; // 1134
const PROGRAMME: u32 = TIE + 1; // 1135
const BATTERIE: u32 = PROGRAMME + 130; // 1265
/// Taille du vocabulaire : les logits au-delà sont masqués.
pub const VOCABULAIRE: u32 = BATTERIE + 128; // 1393

/// Trames par seconde des décalages.
pub const TRAMES_PAR_S: f32 = 100.0;
/// Programme General MIDI des frappes de batterie.
pub const PROGRAMME_BATTERIE: u8 = 128;
const DUREE_MIN_S: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evenement {
    Special,
    Decalage(u32),
    Hauteur(u8),
    Velocite(u8),
    Tie,
    Programme(u8),
    Batterie(u8),
}

pub fn evenement(jeton: u32) -> Evenement {
    match jeton {
        j if j < DECALAGE => Evenement::Special,
        j if j < HAUTEUR => Evenement::Decalage(j - DECALAGE),
        j if j < VELOCITE => Evenement::Hauteur((j - HAUTEUR) as u8),
        j if j < TIE => Evenement::Velocite((j - VELOCITE) as u8),
        TIE => Evenement::Tie,
        j if j < BATTERIE => Evenement::Programme((j - PROGRAMME) as u8),
        j if j < VOCABULAIRE => Evenement::Batterie((j - BATTERIE) as u8),
        _ => Evenement::Special,
    }
}

/// Groupes d'instruments du vocabulaire « MT3_FULL_PLUS » : nom, identifiant
/// de groupe (conditionnement), programme représentatif (celui que le modèle
/// écrit). `None` : la batterie, qui n'a pas de programme.
pub const GROUPES: &[(&str, u32, Option<u8>)] = &[
    ("acoustic_piano", 0, Some(0)),
    ("electric_piano", 1, Some(2)),
    ("chromatic_percussion", 2, Some(8)),
    ("organ", 3, Some(16)),
    ("acoustic_guitar", 4, Some(24)),
    ("clean_electric_guitar", 5, Some(26)),
    ("distorted_electric_guitar", 6, Some(29)),
    ("acoustic_bass", 7, Some(32)),
    ("electric_bass", 8, Some(33)),
    ("violin", 9, Some(40)),
    ("viola", 10, Some(41)),
    ("cello", 11, Some(42)),
    ("contrabass", 12, Some(43)),
    ("orchestral_harp", 13, Some(46)),
    ("timpani", 14, Some(47)),
    ("string_ensemble", 15, Some(48)),
    ("synth_strings", 16, Some(50)),
    ("voice", 17, Some(52)),
    ("orchestra_hit", 18, Some(55)),
    ("trumpet", 19, Some(56)),
    ("trombone", 20, Some(57)),
    ("tuba", 21, Some(58)),
    ("french_horn", 22, Some(60)),
    ("brass_section", 23, Some(61)),
    ("soprano_and_alto_sax", 24, Some(64)),
    ("tenor_sax", 25, Some(66)),
    ("baritone_sax", 26, Some(67)),
    ("oboe", 27, Some(68)),
    ("english_horn", 28, Some(69)),
    ("bassoon", 29, Some(70)),
    ("clarinet", 30, Some(71)),
    ("flutes", 31, Some(72)),
    ("synth_lead", 32, Some(80)),
    ("synth_pad", 33, Some(88)),
    ("drums", 36, None),
];

fn groupe(nom: &str) -> Option<&'static (&'static str, u32, Option<u8>)> {
    GROUPES.iter().find(|g| g.0 == nom)
}

/// Identifiants de groupe pour le conditionnement, dans l'ordre donné. Un nom
/// inconnu est ignoré.
pub fn groupes_de(instruments: &[&str]) -> Vec<u32> {
    instruments.iter().filter_map(|n| groupe(n).map(|g| g.1)).collect()
}

/// Jetons interdits quand seuls `instruments` peuvent apparaître : tout
/// programme qui n'est pas le représentant d'un groupe permis, et la batterie
/// si « drums » n'est pas demandée (`MT3Tokenizer.forbidden_token_ids`).
pub fn interdits(instruments: &[&str]) -> Vec<u32> {
    let batterie = instruments.contains(&"drums");
    let permis: Vec<u8> = instruments.iter().filter_map(|n| groupe(n).and_then(|g| g.2)).collect();
    let mut v: Vec<u32> = (0..130u32).filter(|p| !permis.contains(&(*p as u8))).map(|p| PROGRAMME + p).collect();
    if !batterie {
        v.extend(BATTERIE..VOCABULAIRE);
    }
    v
}

/// Prologue d'un segment : les notes encore tenues à sa frontière, triées par
/// (programme, hauteur), un jeton de programme par série, puis `tie`.
pub fn prologue(ouvertes: &[(u8, u8)]) -> Vec<u32> {
    let mut cles = ouvertes.to_vec();
    cles.sort();
    let mut v = Vec::new();
    let mut courant = None;
    for (p, h) in cles {
        if courant != Some(p) {
            v.push(PROGRAMME + p as u32);
            courant = Some(p);
        }
        v.push(HAUTEUR + h as u32);
    }
    v.push(TIE);
    v
}

/// Une note décodée, en secondes depuis le début de l'audio transcrit.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NoteBrute {
    pub debut_s: f32,
    pub fin_s: f32,
    pub hauteur: u8,
    /// Programme General MIDI ; [`PROGRAMME_BATTERIE`] pour une frappe.
    pub programme: u8,
}

/// L'automate de décodage, segment par segment (`OpenNoteTracker`). Sert
/// aussi au prélude forcé : [`Suivi::ouvertes`] à une frontière donne les
/// notes que le segment suivant doit déclarer tenues.
#[derive(Debug, Default)]
pub struct Suivi {
    /// (programme, hauteur) → début ; dans l'ordre d'ouverture.
    ouvertes: Vec<((u8, u8), f32)>,
    pub notes: Vec<NoteBrute>,
    debut_segment: f32,
    fin_segment: Option<f32>,
    tic_debut: u32,
    tic: u32,
    programme: Option<u8>,
    velocite: Option<u8>,
    prologue: bool,
    ignorer: bool,
    tenues: Vec<(u8, u8)>,
    commence: bool,
}

impl Suivi {
    pub fn new() -> Self {
        Self::default()
    }

    fn fermer(&mut self, cle: (u8, u8), t: f32) {
        if let Some(i) = self.ouvertes.iter().position(|(k, _)| *k == cle) {
            let (_, debut) = self.ouvertes.remove(i);
            self.notes.push(NoteBrute { debut_s: debut, fin_s: t, hauteur: cle.1, programme: cle.0 });
        }
    }

    fn tout_fermer(&mut self, t: f32) {
        for (cle, debut) in std::mem::take(&mut self.ouvertes) {
            self.notes.push(NoteBrute { debut_s: debut, fin_s: t, hauteur: cle.1, programme: cle.0 });
        }
    }

    /// Frontière d'un segment qui commence à `debut` (s) ; `fin` : début du
    /// suivant, au-delà duquel les événements sont ignorés.
    pub fn frontiere(&mut self, debut: f32, fin: Option<f32>) {
        if self.commence && self.prologue {
            self.tout_fermer(self.debut_segment);
        }
        self.debut_segment = debut;
        self.fin_segment = fin;
        self.tic_debut = (debut * TRAMES_PAR_S).round() as u32;
        self.tic = self.tic_debut;
        self.programme = None;
        self.velocite = None;
        self.prologue = true;
        self.ignorer = false;
        self.tenues.clear();
        self.commence = true;
    }

    pub fn jeton(&mut self, jeton: u32) {
        let e = evenement(jeton);
        if self.prologue {
            match e {
                Evenement::Tie => {
                    self.prologue = false;
                    self.velocite = None;
                    let finies: Vec<(u8, u8)> =
                        self.ouvertes.iter().map(|(k, _)| *k).filter(|k| !self.tenues.contains(k)).collect();
                    for k in finies {
                        self.fermer(k, self.debut_segment);
                    }
                }
                Evenement::Decalage(_) => {
                    // Pas de `tie` : segment mal formé, tout se ferme à la
                    // frontière et le reste du segment est ignoré.
                    self.prologue = false;
                    self.ignorer = true;
                    self.tout_fermer(self.debut_segment);
                }
                Evenement::Programme(p) => self.programme = Some(p),
                Evenement::Hauteur(h) => {
                    if let Some(p) = self.programme {
                        if !self.tenues.contains(&(p, h)) {
                            self.tenues.push((p, h));
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if self.ignorer {
            return;
        }
        let t = self.tic as f32 / TRAMES_PAR_S;
        match e {
            Evenement::Decalage(v) => {
                if v > 0 {
                    self.tic = self.tic_debut + v;
                }
            }
            Evenement::Programme(p) => self.programme = Some(p),
            Evenement::Velocite(v) => self.velocite = Some(v),
            Evenement::Batterie(h) => {
                if self.fin_segment.is_none_or(|f| t < f) {
                    self.notes.push(NoteBrute { debut_s: t, fin_s: t + DUREE_MIN_S, hauteur: h, programme: PROGRAMME_BATTERIE });
                }
            }
            Evenement::Hauteur(h) => {
                let (Some(p), Some(v)) = (self.programme, self.velocite) else { return };
                if self.fin_segment.is_some_and(|f| t >= f) {
                    return;
                }
                self.fermer((p, h), t);
                if v > 0 {
                    self.ouvertes.push(((p, h), t));
                }
            }
            _ => {}
        }
    }

    /// Notes encore ouvertes, triées : le prologue forcé du segment suivant.
    pub fn ouvertes(&self) -> Vec<(u8, u8)> {
        let mut v: Vec<(u8, u8)> = self.ouvertes.iter().map(|(k, _)| *k).collect();
        v.sort();
        v
    }

    /// Fin du flux : ferme ce qui reste ouvert.
    pub fn finir(mut self) -> Vec<NoteBrute> {
        if self.commence && self.prologue {
            self.tout_fermer(self.debut_segment);
        } else {
            for (cle, debut) in std::mem::take(&mut self.ouvertes) {
                self.notes.push(NoteBrute { debut_s: debut, fin_s: debut + DUREE_MIN_S, hauteur: cle.1, programme: cle.0 });
            }
        }
        self.notes
    }
}

/// Écart maximal, en trames, entre deux attaques d'une même note pour
/// qu'elles comptent comme une répétition : 20 ms, plus vite qu'aucun
/// instrumentiste ne rejoue une touche.
const REPRISE_TRAMES: u32 = 2;
/// Répétitions rapprochées d'une même note qui signent une boucle.
const REPRISES_BOUCLE: u32 = 2;

/// Indice du jeton où le décodage part en boucle — la même note rejouée au
/// même instant ou toutes les 10-20 ms (banc MAESTRO : un segment de 5 s
/// rendu en 586 notes au lieu de 47). Le flux avant cet indice reste bon.
/// Une répétition isolée ne compte pas : [`super::sans_doublons`] la fond.
pub fn debut_de_boucle(jetons: &[u32]) -> Option<usize> {
    let mut tic = 0u32;
    let mut programme: Option<u8> = None;
    let mut velocite = 1u8;
    let mut actif = false;
    // (programme, hauteur) → (dernière attaque, répétitions, indice de la première)
    let mut vues: std::collections::HashMap<(u8, u8), (u32, u32, usize)> = Default::default();
    for (i, &j) in jetons.iter().enumerate() {
        let cle = match evenement(j) {
            Evenement::Tie => {
                actif = true;
                continue;
            }
            Evenement::Decalage(v) => {
                actif = true;
                tic = v;
                continue;
            }
            Evenement::Programme(p) => {
                programme = Some(p);
                continue;
            }
            Evenement::Velocite(v) => {
                velocite = v;
                continue;
            }
            Evenement::Hauteur(h) if actif && velocite > 0 => (programme.unwrap_or(0), h),
            Evenement::Batterie(h) if actif => (PROGRAMME_BATTERIE, h),
            _ => continue,
        };
        match vues.get_mut(&cle) {
            Some((dernier, reprises, premiere)) => {
                if tic.abs_diff(*dernier) <= REPRISE_TRAMES {
                    if *reprises == 0 {
                        *premiere = i;
                    }
                    *reprises += 1;
                    if *reprises >= REPRISES_BOUCLE {
                        return Some(*premiere);
                    }
                } else {
                    *reprises = 0;
                }
                *dernier = tic;
            }
            None => {
                vues.insert(cle, (tic, 0, i));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decalage(trames: u32) -> u32 {
        DECALAGE + trames
    }
    fn hauteur(h: u8) -> u32 {
        HAUTEUR + h as u32
    }
    fn programme(p: u8) -> u32 {
        PROGRAMME + p as u32
    }
    const DEBUT: u32 = VELOCITE + 1;
    const FIN: u32 = VELOCITE;

    #[test]
    fn le_vocabulaire_a_la_taille_du_modele() {
        assert_eq!(VOCABULAIRE, 1393);
        assert_eq!(evenement(TIE), Evenement::Tie);
        assert_eq!(evenement(programme(33)), Evenement::Programme(33));
        assert_eq!(evenement(BATTERIE + 36), Evenement::Batterie(36));
    }

    #[test]
    fn une_note_s_ouvre_et_se_ferme() {
        let mut s = Suivi::new();
        s.frontiere(0.0, Some(5.0));
        for j in [TIE, decalage(10), programme(33), DEBUT, hauteur(40), decalage(60), FIN, hauteur(40)] {
            s.jeton(j);
        }
        assert_eq!(s.finir(), vec![NoteBrute { debut_s: 0.1, fin_s: 0.6, hauteur: 40, programme: 33 }]);
    }

    #[test]
    fn une_note_tenue_traverse_la_frontiere() {
        let mut s = Suivi::new();
        s.frontiere(0.0, Some(5.0));
        for j in [TIE, decalage(400), programme(33), DEBUT, hauteur(40)] {
            s.jeton(j);
        }
        assert_eq!(s.ouvertes(), vec![(33, 40)]);
        s.frontiere(5.0, None);
        for j in prologue(&s.ouvertes()) {
            s.jeton(j);
        }
        for j in [decalage(50), programme(33), FIN, hauteur(40)] {
            s.jeton(j);
        }
        assert_eq!(s.finir(), vec![NoteBrute { debut_s: 4.0, fin_s: 5.5, hauteur: 40, programme: 33 }]);
    }

    #[test]
    fn une_note_non_declaree_se_ferme_a_la_frontiere() {
        let mut s = Suivi::new();
        s.frontiere(0.0, Some(5.0));
        for j in [TIE, decalage(400), programme(33), DEBUT, hauteur(40)] {
            s.jeton(j);
        }
        s.frontiere(5.0, None);
        s.jeton(TIE);
        assert_eq!(s.finir(), vec![NoteBrute { debut_s: 4.0, fin_s: 5.0, hauteur: 40, programme: 33 }]);
    }

    #[test]
    fn seuls_les_programmes_de_basse_sont_permis() {
        let v = interdits(&["electric_bass", "acoustic_bass"]);
        assert!(!v.contains(&programme(33)) && !v.contains(&programme(32)));
        assert!(v.contains(&programme(0)) && v.contains(&(BATTERIE + 36)));
        assert_eq!(v.len(), 128 + 128);
        assert_eq!(groupes_de(&["electric_bass", "acoustic_bass"]), vec![8, 7]);
    }

    #[test]
    fn une_note_rejouee_en_rafale_est_une_boucle() {
        let mut v = vec![TIE, decalage(10), programme(0), DEBUT, hauteur(60), hauteur(64)];
        let boucle = v.len() + 1;
        for t in 11..20 {
            v.extend([decalage(t), hauteur(64)]);
        }
        assert_eq!(debut_de_boucle(&v), Some(boucle));
    }

    #[test]
    fn un_trille_ou_un_accord_n_est_pas_une_boucle() {
        let mut v = vec![TIE, programme(0), DEBUT];
        for k in 0..20 {
            // Note rejouée toutes les 50 ms, accord de trois notes.
            v.extend([decalage(5 * k), hauteur(60), hauteur(64), hauteur(67)]);
        }
        assert_eq!(debut_de_boucle(&v), None);
        // Une seule répétition au même instant : un doublon, pas une boucle.
        v.extend([decalage(200), hauteur(72), hauteur(72), decalage(210), hauteur(74)]);
        assert_eq!(debut_de_boucle(&v), None);
    }
}
