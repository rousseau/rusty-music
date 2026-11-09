// SPDX-License-Identifier: GPL-3.0-or-later
//! La mémoire de Pratiquer, par morceau (migration n° 7) : ce qui est réglé
//! en Pratiquer est gardé, et rouvrir le morceau rend la séance
//! (`docs/ui-spec-editeur.md`, question tranchée le 9 nov. 2026).
//!
//! Une ligne de `pratique` par chemin de fichier, ses boucles dans
//! `pratique_boucle`. Les tables des corrections de transcription (E2) et de
//! la table par mesure du coach (E4) existent déjà, vides : elles ne passent
//! pas par ici.

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
