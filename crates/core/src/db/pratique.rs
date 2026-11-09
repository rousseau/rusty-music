// SPDX-License-Identifier: GPL-3.0-or-later
//! La mémoire de Pratiquer, par morceau (migration n° 7) : ce qui est réglé
//! en Pratiquer est gardé, et rouvrir le morceau rend la séance
//! (`docs/ui-spec-editeur.md`, question tranchée le 9 nov. 2026).
//!
//! Une ligne de `pratique` par chemin de fichier, ses boucles dans
//! `pratique_boucle`. Les corrections de transcription (E2) ont leur table,
//! `pratique_correction`, et leurs méthodes ([`Library::corriger_pratique`]) :
//! la mémoire ne les touche pas. La table par mesure du coach (E4) existe,
//! vide.

use super::*;

/// Une boucle, bornes en secondes du morceau d'origine.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BouclePratique {
    /// `None` : la boucle posée quand on a quitté le morceau (au plus une).
    pub nom: Option<String>,
    pub debut_s: f64,
    pub fin_s: f64,
    /// Nombre de mesures ; `None` pour une boucle posée sur une section.
    pub mesures: Option<u32>,
    pub vitesse_atteinte: Option<f64>,
}

/// Ce que Pratiquer retient d'un morceau. `None` : rien de retenu, le défaut
/// de l'interface s'applique.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemoirePratique {
    /// `"bass"` ou `"drums"`.
    pub instrument: Option<String>,
    /// `"score+tabs"` ou `"tabs"`.
    pub notation: Option<String>,
    /// Mesures de décompte : 0, 1 ou 2.
    pub decompte: Option<u32>,
    /// Variante de démixage des stems du morceau.
    pub variante: Option<String>,
    pub vitesse: Option<f64>,
    pub vitesse_atteinte: Option<f64>,
    pub mesures_boucle: Option<u32>,
    pub accel_boucle: bool,
    pub position_s: Option<f64>,
    /// La boucle courante (sans nom) d'abord, puis les boucles nommées.
    pub boucles: Vec<BouclePratique>,
}

impl MemoirePratique {
    /// Refuse ce qu'aucun geste de l'interface ne produit : une valeur hors
    /// de son domaine ne doit pas revenir à la séance suivante.
    pub fn verifier(&self) -> std::result::Result<(), String> {
        let positif = |v: Option<f64>, nom: &str| match v {
            Some(x) if !(x.is_finite() && x > 0.0 && x <= 4.0) => {
                Err(format!("{nom} invalide : {x}"))
            }
            _ => Ok(()),
        };
        positif(self.vitesse, "vitesse")?;
        positif(self.vitesse_atteinte, "vitesse atteinte")?;
        if let Some(i) = &self.instrument {
            if i != "bass" && i != "drums" {
                return Err(format!("instrument inconnu : {i}"));
            }
        }
        if let Some(n) = &self.notation {
            if n != "score+tabs" && n != "tabs" {
                return Err(format!("notation inconnue : {n}"));
            }
        }
        if self.decompte.is_some_and(|d| d > 2) {
            return Err("décompte au-delà de deux mesures".into());
        }
        if self.mesures_boucle.is_some_and(|m| !(1..=32).contains(&m)) {
            return Err("boucle hors de 1 à 32 mesures".into());
        }
        if self
            .position_s
            .is_some_and(|p| !(p.is_finite() && p >= 0.0))
        {
            return Err("position invalide".into());
        }
        if self.boucles.iter().filter(|b| b.nom.is_none()).count() > 1 {
            return Err("plus d'une boucle courante".into());
        }
        for b in &self.boucles {
            if !(b.debut_s.is_finite()
                && b.fin_s.is_finite()
                && 0.0 <= b.debut_s
                && b.debut_s < b.fin_s)
            {
                return Err(format!("boucle invalide : {} → {}", b.debut_s, b.fin_s));
            }
            positif(b.vitesse_atteinte, "vitesse atteinte de boucle")?;
        }
        Ok(())
    }
}

impl Library {
    /// La mémoire de Pratiquer pour le morceau `path`, s'il en a une.
    pub fn memoire_pratique(&self, path: &str) -> Result<Option<MemoirePratique>> {
        let ligne = self
            .conn
            .query_row(
                "SELECT instrument, notation, decompte, variante, vitesse, vitesse_atteinte,
                        mesures_boucle, accel_boucle, position_s
                 FROM pratique WHERE path = ?1",
                [path],
                |r| {
                    Ok(MemoirePratique {
                        instrument: r.get(0)?,
                        notation: r.get(1)?,
                        decompte: r.get(2)?,
                        variante: r.get(3)?,
                        vitesse: r.get(4)?,
                        vitesse_atteinte: r.get(5)?,
                        mesures_boucle: r.get(6)?,
                        accel_boucle: r.get::<_, i64>(7)? != 0,
                        position_s: r.get(8)?,
                        boucles: Vec::new(),
                    })
                },
            )
            .optional()?;
        let Some(mut m) = ligne else {
            return Ok(None);
        };
        let mut req = self.conn.prepare(
            "SELECT nom, debut_s, fin_s, mesures, vitesse_atteinte
             FROM pratique_boucle WHERE path = ?1
             ORDER BY nom IS NOT NULL, rang",
        )?;
        m.boucles = req
            .query_map([path], |r| {
                Ok(BouclePratique {
                    nom: r.get(0)?,
                    debut_s: r.get(1)?,
                    fin_s: r.get(2)?,
                    mesures: r.get(3)?,
                    vitesse_atteinte: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Some(m))
    }

    /// Garde la mémoire de Pratiquer du morceau `path`, boucles comprises
    /// (elles remplacent les précédentes). Les corrections et la table par
    /// mesure ne sont pas touchées.
    pub fn garder_pratique(&self, path: &str, m: &MemoirePratique) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO pratique (path, instrument, notation, decompte, variante, vitesse,
                                   vitesse_atteinte, mesures_boucle, accel_boucle, position_s, maj)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, strftime('%s','now'))
             ON CONFLICT (path) DO UPDATE SET
                instrument = excluded.instrument, notation = excluded.notation,
                decompte = excluded.decompte, variante = excluded.variante,
                vitesse = excluded.vitesse, vitesse_atteinte = excluded.vitesse_atteinte,
                mesures_boucle = excluded.mesures_boucle, accel_boucle = excluded.accel_boucle,
                position_s = excluded.position_s, maj = excluded.maj",
            params![
                path,
                m.instrument,
                m.notation,
                m.decompte,
                m.variante,
                m.vitesse,
                m.vitesse_atteinte,
                m.mesures_boucle,
                m.accel_boucle,
                m.position_s,
            ],
        )?;
        tx.execute("DELETE FROM pratique_boucle WHERE path = ?1", [path])?;
        for (rang, b) in m.boucles.iter().enumerate() {
            tx.execute(
                "INSERT INTO pratique_boucle (path, rang, nom, debut_s, fin_s, mesures, vitesse_atteinte)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![path, rang as i64, b.nom, b.debut_s, b.fin_s, b.mesures, b.vitesse_atteinte],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// Une correction de transcription (E2) : la note visée se retrouve par son
/// instant et sa hauteur **transcrits** (`debut_s`, `hauteur`), qu'une
/// nouvelle transcription redonne ; une note ajoutée par les siens. Appliquée
/// par `rusty_music_transcription::corrections`.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CorrectionPratique {
    /// `"corde"`, `"octave"`, `"supprimer"`, `"ajouter"` ; `"retablir"` en
    /// demande seulement (oublie tout ce qui vise la note, rien n'est gardé).
    pub genre: String,
    pub debut_s: f64,
    pub hauteur: u8,
    /// Note ajoutée : sa durée.
    #[serde(default)]
    pub duree_s: Option<f64>,
    /// Corde imposée (0 = la plus grave), ou celle d'une note ajoutée.
    #[serde(default)]
    pub corde: Option<u8>,
    #[serde(default)]
    pub frette: Option<u8>,
    /// Octave corrigée : de combien d'octaves la note bouge, −2 à +2.
    #[serde(default)]
    pub octaves: Option<i8>,
}

/// L'ordre où les corrections s'appliquent : une note ajoutée peut ensuite
/// changer d'octave ou de corde ; la corde imposée vient en dernier, sur la
/// hauteur corrigée.
const ORDRE_GENRES: &str =
    "CASE genre WHEN 'ajouter' THEN 0 WHEN 'supprimer' THEN 1 WHEN 'octave' THEN 2 ELSE 3 END";

/// Deux instants transcrits sont la même note à moins d'une milliseconde
/// (ils font l'aller-retour par le JSON de l'interface).
const MEME_INSTANT_S: f64 = 1e-3;

impl CorrectionPratique {
    /// Refuse ce qu'aucun geste de l'interface ne produit.
    pub fn verifier(&self) -> std::result::Result<(), String> {
        if !(self.debut_s.is_finite() && self.debut_s >= 0.0) {
            return Err(format!("instant invalide : {}", self.debut_s));
        }
        if self.hauteur > 127 {
            return Err(format!("hauteur invalide : {}", self.hauteur));
        }
        match self.genre.as_str() {
            "supprimer" | "retablir" => Ok(()),
            "corde" if self.corde.is_some_and(|c| c < 6) => Ok(()),
            "corde" => Err("corde imposée absente ou hors du manche".into()),
            "octave" if self.octaves.is_some_and(|o| (-2..=2).contains(&o)) => Ok(()),
            "octave" => Err("octave hors de −2 à +2".into()),
            "ajouter" => match self.duree_s {
                Some(d) if d.is_finite() && d > 0.0 && d <= 30.0 => {
                    if self.corde.is_some_and(|c| c >= 6) {
                        Err("corde hors du manche".into())
                    } else {
                        Ok(())
                    }
                }
                _ => Err("note ajoutée sans durée valide".into()),
            },
            g => Err(format!("correction inconnue : {g}")),
        }
    }
}

impl Library {
    /// Les corrections de transcription du morceau `path` pour `instrument`,
    /// dans l'ordre où elles s'appliquent.
    pub fn corrections_pratique(
        &self,
        path: &str,
        instrument: &str,
    ) -> Result<Vec<CorrectionPratique>> {
        let mut req = self.conn.prepare(&format!(
            "SELECT genre, debut_s, hauteur, duree_s, corde, frette, octaves
             FROM pratique_correction WHERE path = ?1 AND instrument = ?2
             ORDER BY {ORDRE_GENRES}, id"
        ))?;
        let lignes = req
            .query_map([path, instrument], |r| {
                Ok(CorrectionPratique {
                    genre: r.get(0)?,
                    debut_s: r.get(1)?,
                    hauteur: r.get::<_, Option<u8>>(2)?.unwrap_or(0),
                    duree_s: r.get(3)?,
                    corde: r.get(4)?,
                    frette: r.get(5)?,
                    octaves: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(lignes)
    }

    /// Garde une correction de transcription. Une note n'a qu'une correction
    /// de chaque genre : la nouvelle remplace l'ancienne. Changer l'octave
    /// oublie la corde imposée (elle ne porte plus la même note) ; supprimer
    /// une note ajoutée l'oublie tout à fait ; « rétablir » oublie tout ce qui
    /// vise la note, qui redevient celle de la transcription. À vérifier
    /// d'abord ([`CorrectionPratique::verifier`]).
    pub fn corriger_pratique(
        &self,
        path: &str,
        instrument: &str,
        c: &CorrectionPratique,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        // La correction pend à la ligne du morceau (clé étrangère).
        tx.execute(
            "INSERT INTO pratique (path) VALUES (?1) ON CONFLICT (path) DO NOTHING",
            [path],
        )?;
        let meme_note = "path = ?1 AND instrument = ?2 AND abs(debut_s - ?3) < ?4 AND hauteur = ?5";
        let oublier = |genres: &str| -> rusqlite::Result<usize> {
            tx.execute(
                &format!(
                    "DELETE FROM pratique_correction WHERE {meme_note} AND genre IN ({genres})"
                ),
                params![path, instrument, c.debut_s, MEME_INSTANT_S, c.hauteur],
            )
        };
        let ajoutee: bool = tx.query_row(
            &format!(
                "SELECT EXISTS (SELECT 1 FROM pratique_correction WHERE {meme_note} AND genre = 'ajouter')"
            ),
            params![path, instrument, c.debut_s, MEME_INSTANT_S, c.hauteur],
            |r| r.get(0),
        )?;
        let tout = "'ajouter', 'supprimer', 'octave', 'corde'";
        let garder = match c.genre.as_str() {
            "retablir" => {
                oublier(tout)?;
                false
            }
            "supprimer" if ajoutee => {
                oublier(tout)?;
                false
            }
            "supprimer" => {
                oublier(tout)?;
                true
            }
            "octave" => {
                oublier("'octave', 'corde'")?;
                c.octaves != Some(0)
            }
            "corde" => {
                oublier("'corde'")?;
                true
            }
            "ajouter" => {
                oublier(tout)?;
                true
            }
            // Refusé plus haut par `verifier` : rien n'est gardé.
            _ => false,
        };
        if garder {
            tx.execute(
                "INSERT INTO pratique_correction
                    (path, instrument, genre, debut_s, hauteur, duree_s, corde, frette, octaves)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    path, instrument, c.genre, c.debut_s, c.hauteur, c.duree_s, c.corde, c.frette,
                    c.octaves
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
