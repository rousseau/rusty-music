// SPDX-License-Identifier: GPL-3.0-or-later
//! La latence calibrée du coach de Pratiquer (migration n° 10, E4) : une par
//! périphérique d'entrée et par chemin (`"audio"`, `"midi"`), gardée hors du
//! morceau — on calibre sa batterie une fois, pas à chaque chanson
//! (`docs/recherche-coach-pratique.md`, « En accompagnement, l'alignement se
//! réduit à une calibration »).
//!
//! La mesure elle-même (médiane des écarts frappe − clic) est
//! `rusty_music_editor::coach::calibrer` ; ici, seulement ce qui en reste.
//! Avec elle, le profil de correspondance d'une batterie électronique
//! (migration n° 11), gardé en JSON par périphérique, et le canal d'une
//! interface audio où joue la basse (migration n° 14, V34).

use super::*;

/// Une latence calibrée.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LatenceCalibree {
    pub peripherique: String,
    /// `"audio"` ou `"midi"`.
    pub chemin: String,
    /// Médiane des écarts frappe − clic, en millisecondes.
    pub latence_ms: f64,
    /// Écart-type robuste des frappes retenues, en millisecondes.
    pub dispersion_ms: Option<f64>,
    /// Frappes retenues pour la médiane.
    pub frappes: u32,
}

impl LatenceCalibree {
    /// Refuse ce qu'aucune calibration ne produit : une latence négative au-delà
    /// de 100 ms (on frapperait avant d'entendre) ou au-delà d'une seconde.
    pub fn verifier(&self) -> std::result::Result<(), String> {
        if self.peripherique.trim().is_empty() {
            return Err("périphérique sans nom".into());
        }
        if self.chemin != "audio" && self.chemin != "midi" {
            return Err(format!("chemin inconnu : {}", self.chemin));
        }
        if !(self.latence_ms.is_finite() && (-100.0..=1000.0).contains(&self.latence_ms)) {
            return Err(format!("latence invalide : {} ms", self.latence_ms));
        }
        if self
            .dispersion_ms
            .is_some_and(|d| !(d.is_finite() && d >= 0.0))
        {
            return Err("dispersion invalide".into());
        }
        if self.frappes == 0 {
            return Err("aucune frappe retenue".into());
        }
        Ok(())
    }
}

impl Library {
    /// La latence calibrée de `peripherique` sur `chemin`, si elle l'a été.
    pub fn latence(&self, peripherique: &str, chemin: &str) -> Result<Option<LatenceCalibree>> {
        Ok(self
            .conn
            .query_row(
                "SELECT peripherique, chemin, latence_ms, dispersion_ms, frappes
                 FROM latence WHERE peripherique = ?1 AND chemin = ?2",
                params![peripherique, chemin],
                ligne_latence,
            )
            .optional()?)
    }

    /// Toutes les latences calibrées, par périphérique puis chemin.
    pub fn latences(&self) -> Result<Vec<LatenceCalibree>> {
        let mut stmt = self.conn.prepare(
            "SELECT peripherique, chemin, latence_ms, dispersion_ms, frappes
             FROM latence ORDER BY peripherique, chemin",
        )?;
        let v = stmt
            .query_map([], ligne_latence)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Garde une calibration ; elle remplace la précédente du même
    /// périphérique et du même chemin.
    pub fn garder_latence(&self, l: &LatenceCalibree) -> Result<()> {
        self.conn.execute(
            "INSERT INTO latence (peripherique, chemin, latence_ms, dispersion_ms, frappes, maj)
             VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))
             ON CONFLICT (peripherique, chemin) DO UPDATE SET
                latence_ms = excluded.latence_ms, dispersion_ms = excluded.dispersion_ms,
                frappes = excluded.frappes, maj = excluded.maj",
            params![
                l.peripherique,
                l.chemin,
                l.latence_ms,
                l.dispersion_ms,
                l.frappes
            ],
        )?;
        Ok(())
    }

    /// Le profil MIDI gardé pour `peripherique`, en JSON (le coach le lit).
    pub fn profil_midi(&self, peripherique: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT profil FROM profil_midi WHERE peripherique = ?1",
                params![peripherique],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Garde le profil MIDI de `peripherique` (JSON), à la place du précédent.
    pub fn garder_profil_midi(&self, peripherique: &str, json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO profil_midi (peripherique, profil, maj)
             VALUES (?1, ?2, strftime('%s','now'))
             ON CONFLICT (peripherique) DO UPDATE SET
                profil = excluded.profil, maj = excluded.maj",
            params![peripherique, json],
        )?;
        Ok(())
    }
}

/// Le canal d'une interface audio où joue la basse : choisi à la main, ou
/// trouvé par le coach.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CanalEntree {
    pub peripherique: String,
    /// Compté depuis 0 : l'entrée 1 de l'interface.
    pub canal: u32,
    /// Choisi à la main (sinon : trouvé par le coach, « auto »).
    pub manuel: bool,
}

impl Library {
    /// Le canal gardé pour l'interface `peripherique`, s'il l'a été.
    pub fn canal_entree(&self, peripherique: &str) -> Result<Option<CanalEntree>> {
        Ok(self
            .conn
            .query_row(
                "SELECT peripherique, canal, manuel FROM canal_entree WHERE peripherique = ?1",
                params![peripherique],
                |r| {
                    Ok(CanalEntree {
                        peripherique: r.get(0)?,
                        canal: r.get(1)?,
                        manuel: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Garde le canal d'une interface, à la place du précédent.
    pub fn garder_canal_entree(&self, c: &CanalEntree) -> Result<()> {
        self.conn.execute(
            "INSERT INTO canal_entree (peripherique, canal, manuel, maj)
             VALUES (?1, ?2, ?3, strftime('%s','now'))
             ON CONFLICT (peripherique) DO UPDATE SET
                canal = excluded.canal, manuel = excluded.manuel, maj = excluded.maj",
            params![c.peripherique, c.canal, c.manuel],
        )?;
        Ok(())
    }
}

fn ligne_latence(r: &rusqlite::Row) -> rusqlite::Result<LatenceCalibree> {
    Ok(LatenceCalibree {
        peripherique: r.get(0)?,
        chemin: r.get(1)?,
        latence_ms: r.get(2)?,
        dispersion_ms: r.get(3)?,
        frappes: r.get(4)?,
    })
}
