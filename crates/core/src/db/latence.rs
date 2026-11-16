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
//! interface audio où joue la basse (migration n° 14, V34), et le temps de
//! démixage par variante (migration n° 16, V40).

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

/// Le son et les volumes du retour d'un instrument (E7).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RetourInstrument {
    /// « basse » pour l'instant.
    pub instrument: String,
    /// Nom du préréglage (`brut`, `rond`…).
    pub prereglage: String,
    /// Volume de l'instrument, 0 à 1.
    pub volume: f32,
    /// Balance morceau / instrument, −1 (morceau seul) à 1 (instrument seul).
    pub balance: f32,
}

impl Library {
    /// Le retour gardé pour `instrument`, s'il l'a été.
    pub fn retour_instrument(&self, instrument: &str) -> Result<Option<RetourInstrument>> {
        Ok(self
            .conn
            .query_row(
                "SELECT instrument, prereglage, volume, balance FROM retour_instrument
                 WHERE instrument = ?1",
                params![instrument],
                |r| {
                    Ok(RetourInstrument {
                        instrument: r.get(0)?,
                        prereglage: r.get(1)?,
                        volume: r.get::<_, f64>(2)? as f32,
                        balance: r.get::<_, f64>(3)? as f32,
                    })
                },
            )
            .optional()?)
    }

    /// Garde le retour d'un instrument (volumes ramenés dans leurs bornes).
    pub fn garder_retour_instrument(&self, r: &RetourInstrument) -> Result<()> {
        let borne = |v: f32, bas: f32| {
            if v.is_finite() {
                v.clamp(bas, 1.0)
            } else {
                0.0
            }
        };
        self.conn.execute(
            "INSERT INTO retour_instrument (instrument, prereglage, volume, balance, maj)
             VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))
             ON CONFLICT (instrument) DO UPDATE SET
                prereglage = excluded.prereglage, volume = excluded.volume,
                balance = excluded.balance, maj = excluded.maj",
            params![
                r.instrument,
                r.prereglage,
                f64::from(borne(r.volume, 0.0)),
                f64::from(borne(r.balance, -1.0))
            ],
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

/// Démixages au-delà desquels la moyenne devient glissante : les plus
/// récents pèsent alors 1/[`FENETRE_DUREE_DEMIX`] chacun (une machine change :
/// pilote, `burn/metal`, autre Mac).
pub const FENETRE_DUREE_DEMIX: u32 = 5;

impl Library {
    /// Secondes de calcul par seconde de morceau pour la variante de démixage
    /// `variante`, apprises sur cette machine, et le nombre de démixages
    /// comptés.
    pub fn duree_demix(&self, variante: &str) -> Result<Option<(f64, u32)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT facteur, mesures FROM duree_demix WHERE variante = ?1",
                [variante],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Compte un démixage de `variante` qui a pris `facteur` secondes par
    /// seconde de morceau : moyenne des [`FENETRE_DUREE_DEMIX`] premiers, puis
    /// moyenne glissante. Une valeur aberrante (≤ 0, infinie) est ignorée.
    pub fn noter_duree_demix(&self, variante: &str, facteur: f64) -> Result<()> {
        if !(facteur.is_finite() && facteur > 0.0) {
            return Ok(());
        }
        let (nouveau, mesures) = match self.duree_demix(variante)? {
            Some((ancien, n)) => {
                let poids = (n + 1).min(FENETRE_DUREE_DEMIX) as f64;
                (ancien + (facteur - ancien) / poids, n.saturating_add(1))
            }
            None => (facteur, 1),
        };
        self.conn.execute(
            "INSERT INTO duree_demix (variante, facteur, mesures, maj)
             VALUES (?1, ?2, ?3, strftime('%s','now'))
             ON CONFLICT (variante) DO UPDATE SET
                facteur = excluded.facteur, mesures = excluded.mesures, maj = excluded.maj",
            params![variante, nouveau, mesures],
        )?;
        Ok(())
    }

    /// La durée d'un morceau en base, en millisecondes, par son chemin.
    pub fn duree_par_chemin(&self, path: &str) -> Result<Option<u64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT duration_ms FROM tracks WHERE path = ?1",
                [path],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .and_then(|d| u64::try_from(d).ok())
            .filter(|&d| d > 0))
    }
}
