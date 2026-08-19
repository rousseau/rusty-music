//! Enveloppe d'une piste, pour l'affichage.
//!
//! La maquette de direction 1a demande une onde qui « n'est plus un motif
//! décoratif » : enveloppe crête et noyau RMS, lus à la même échelle dans le
//! transport, l'inspecteur et — plus tard — les pistes de stems du module 3.
//!
//! Le calcul suppose de décoder tout le fichier : compter quelques secondes par
//! piste sur un support lent. À faire hors du chemin d'affichage, et à garder
//! en mémoire.

use std::path::{Path, PathBuf};

use rodio::{Decoder, Source};

use crate::{Error, Result};

/// Enveloppe réduite à `tranches` valeurs.
///
/// `peak` donne la silhouette, `rms` le corps du son : c'est l'écart entre les
/// deux qui rend une dynamique lisible, là où la seule crête écrase tout.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Waveform {
    pub peak: Vec<f32>,
    pub rms: Vec<f32>,
}

/// Calcule l'enveloppe d'un fichier.
///
/// `duree_ms` sert de repli quand le décodeur ne sait pas annoncer la durée :
/// sans longueur connue, impossible de répartir les échantillons en tranches
/// sans tout garder en mémoire (une piste de quatre minutes pèse ~80 Mo en
/// échantillons décodés).
pub fn compute(path: &Path, tranches: usize, duree_ms: Option<u64>) -> Result<Waveform> {
    let tranches = tranches.max(1);

    // Opus ne passe pas par rodio : le cœur le décode, `SamplesBuffer` le rend
    // mesurable comme n'importe quelle autre source. Voir `opus_en_memoire`.
    if let Some(buf) = crate::opus_en_memoire(path)? {
        return tailler(buf, tranches, duree_ms, path);
    }
    let file = std::fs::File::open(path).map_err(|source| Error::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let source = Decoder::try_from(file).map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })?;
    tailler(source, tranches, duree_ms, path)
}

/// Découpe une source en tranches crête/RMS. Séparé de [`compute`] pour que les
/// deux voies de décodage — rodio et Opus — partagent le même calcul.
fn tailler(
    source: impl Source,
    tranches: usize,
    duree_ms: Option<u64>,
    path: &Path,
) -> Result<Waveform> {
    // `rodio` 0.22 rend ces deux-là en `NonZero` : d'où le `get()`.
    let taux = u64::from(source.sample_rate().get());
    let canaux = u64::from(source.channels().get());
    let millis = source
        .total_duration()
        .map(|d| d.as_millis() as u64)
        .or(duree_ms)
        .unwrap_or(0);

    let total = taux * canaux * millis / 1000;
    if total == 0 {
        return Err(Error::DureeInconnue {
            path: path.to_path_buf(),
        });
    }
    let par_tranche = (total / tranches as u64).max(1);

    let mut peak = Vec::with_capacity(tranches);
    let mut rms = Vec::with_capacity(tranches);
    let (mut crete, mut somme, mut n) = (0f32, 0f64, 0u64);

    for e in source {
        let a = e.abs();
        if a > crete {
            crete = a;
        }
        somme += (e as f64) * (e as f64);
        n += 1;

        if n >= par_tranche {
            peak.push(crete.min(1.0));
            rms.push(((somme / n as f64).sqrt() as f32).min(1.0));
            crete = 0.0;
            somme = 0.0;
            n = 0;
            // La durée annoncée est parfois approximative : on s'arrête net
            // plutôt que de déborder du nombre de tranches demandé.
            if peak.len() == tranches {
                break;
            }
        }
    }
    // Dernière tranche partielle, et complément si le fichier est plus court
    // que sa durée annoncée.
    if n > 0 && peak.len() < tranches {
        peak.push(crete.min(1.0));
        rms.push(((somme / n as f64).sqrt() as f32).min(1.0));
    }
    peak.resize(tranches, 0.0);
    rms.resize(tranches, 0.0);

    Ok(Waveform { peak, rms })
}

/// Chemin et enveloppe, pour le cache de l'appelant.
pub type Cache = std::collections::HashMap<PathBuf, Waveform>;
