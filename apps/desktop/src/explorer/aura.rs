// SPDX-License-Identifier: GPL-3.0-or-later
//! L'aura de la carte (`docs/plan-balade.md`, P4) : les voisins d'un point
//! jouent ensemble, très bas, autour du morceau principal ou seuls.
//!
//! L'interface calcule les voisins à l'écran (les positions sont déjà
//! chargées) et appelle [`aura`] à l'arrêt du curseur, ou quand le promeneur
//! change de voisins. Le moteur ne cherche aucun voisin : il charge les
//! extraits qui lui manquent et dit à la source de l'aura qui doit jouer
//! (`rusty_music_player::aura`).
//!
//! **Les extraits sont décodés à la volée**, avec un saut à la fenêtre la
//! plus représentative du morceau (`Library::fenetre_representative`) : le
//! banc `extraits_aura` l'a départagé du cache d'extraits Opus
//! (`docs/journal.md`, P4). Ils restent ensuite en mémoire, le temps de
//! quelques allers-retours du curseur.
//!
//! L'extrait d'une voie au carrefour (P2) passe par la même source : il prend
//! la place de l'aura le temps du survol, un peu plus fort, puis la rend.

use std::sync::atomic::AtomicU64;

use rusty_music_analysis::decode::{debut_extrait, EXTRAIT_S, FENETRES};
use rusty_music_player::aura::VoixAura;

use super::*;

/// Niveau de l'aura en balade : −18 dB sous le morceau principal.
const FACTEUR_BALADE: f32 = 0.126;
/// Niveau de l'aura seule, en exploration libre : elle est tout ce qu'on
/// entend, mais trois voix se somment.
const FACTEUR_SEULE: f32 = 0.5;
/// L'extrait d'une voie au carrefour : −9 dB sous le morceau principal, qui
/// ne baisse plus.
const FACTEUR_APERCU: f32 = 0.35;
/// L'extrait d'une voie ne dure que le temps d'une annonce.
const APERCU: Duration = Duration::from_secs(5);

/// Ce que l'application retient de l'aura, à côté de la source.
#[derive(Default)]
pub(crate) struct EtatAura {
    /// La dernière demande de l'interface, rejouée après un aperçu.
    demande: Vec<VoixAura>,
    /// L'extrait d'une voie de carrefour joue depuis cet instant.
    apercu: Option<Instant>,
}

/// Génération des demandes : celle qu'un décodage a dépassée ne joue pas.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Le niveau de l'aura selon qu'une balade marche ou non.
fn facteur(etat: &Etat) -> f32 {
    if verrou(&etat.balade).is_some() {
        FACTEUR_BALADE
    } else {
        FACTEUR_SEULE
    }
}

/// Décode, en parallèle, les extraits que l'aura n'a pas encore, et les lui
/// confie. Hors du verrou du lecteur, sauf pour les déposer.
fn charger(etat: &Etat, ids: &[i64]) -> Result<(), String> {
    let manquants: Vec<i64> = {
        let mut p = verrou(&etat.player);
        let a = p.aura();
        ids.iter().copied().filter(|id| !a.connait(*id)).collect()
    };
    if manquants.is_empty() {
        return Ok(());
    }
    let lus: Vec<(i64, PathBuf, Duration)> = {
        let lib = verrou(&etat.lib);
        manquants
            .iter()
            .filter_map(|&id| {
                let piste = lib.track(id).ok().flatten()?;
                let rang = lib
                    .fenetre_representative(id, rusty_music_analysis::passe::MODELE)
                    .ok()
                    .flatten()
                    .unwrap_or(FENETRES / 2);
                let duree = piste.duration_ms.unwrap_or(0).max(0) as f64 / 1000.0;
                let debut = Duration::from_secs_f64(debut_extrait(duree, FENETRES, rang));
                Some((id, PathBuf::from(piste.path), debut))
            })
            .collect()
    };
    // `gain_pour` prend lui-même le verrou de la base.
    let a_decoder: Vec<(i64, PathBuf, Duration, f32)> = lus
        .into_iter()
        .map(|(id, chemin, debut)| {
            let gain = gain_pour(etat, &chemin);
            (id, chemin, debut, gain)
        })
        .collect();
    let extraits: Vec<(i64, Vec<f32>)> = std::thread::scope(|s| {
        let fils: Vec<_> = a_decoder
            .iter()
            .map(|(id, chemin, debut, gain)| {
                s.spawn(move || {
                    let mono = rusty_music_player::extrait_mono(
                        chemin,
                        *debut,
                        Duration::from_secs_f64(EXTRAIT_S),
                    );
                    match mono {
                        Ok(mut m) if !m.is_empty() => {
                            m.iter_mut().for_each(|x| *x *= gain);
                            Some((*id, m))
                        }
                        Ok(_) => None,
                        Err(e) => {
                            tracing::warn!(id, erreur = %e, "extrait de l'aura illisible");
                            None
                        }
                    }
                })
            })
            .collect();
        fils.into_iter()
            .filter_map(|f| f.join().ok().flatten())
            .collect()
    });
    let mut p = verrou(&etat.player);
    let a = p.aura();
    for (id, mono) in &extraits {
        a.retenir(*id, mono);
    }
    Ok(())
}

/// Les voix de l'aura, à la place des précédentes ; une liste vide la tait.
/// Rend le temps passé, en millisecondes, entre la demande et les voix
/// confiées à la source (décodage compris).
#[tauri::command(async)]
pub(crate) fn aura(etat: State<Etat>, voix: Vec<VoixAura>) -> Result<f64, String> {
    let t = Instant::now();
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    verrou(&etat.aura).demande = voix.clone();
    if verrou(&etat.aura).apercu.is_some() {
        // L'extrait d'une voie a la main ; la demande attend qu'il la rende.
        return Ok(0.0);
    }
    let ids: Vec<i64> = voix.iter().map(|v| v.id).collect();
    charger(&etat, &ids)?;
    if GENERATION.load(Ordering::SeqCst) != generation {
        return Ok(t.elapsed().as_secs_f64() * 1e3);
    }
    let facteur = facteur(&etat);
    let mut p = verrou(&etat.player);
    p.regler_aura(facteur);
    p.aura().jouer(&voix);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    tracing::debug!(ms, voix = voix.len(), "aura");
    Ok(ms)
}

/// L'extrait de 5 s d'une voie au carrefour (P2), par la source de l'aura,
/// sous le morceau en cours ; `None` le coupe et rend la main à l'aura.
pub(crate) fn apercu(etat: &Etat, morceau: Option<i64>) -> Result<(), String> {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(id) = morceau else {
        fin_apercu(etat);
        return Ok(());
    };
    charger(etat, &[id])?;
    if GENERATION.load(Ordering::SeqCst) != generation {
        return Ok(());
    }
    verrou(&etat.aura).apercu = Some(Instant::now());
    let mut p = verrou(&etat.player);
    p.regler_aura(FACTEUR_APERCU);
    p.aura().jouer(&[VoixAura {
        id,
        gain: 1.0,
        pan: 0.0,
    }]);
    Ok(())
}

/// L'aperçu cède la place à la dernière demande de l'interface.
fn fin_apercu(etat: &Etat) {
    let demande = {
        let mut a = verrou(&etat.aura);
        if a.apercu.take().is_none() {
            return;
        }
        a.demande.clone()
    };
    let facteur = facteur(etat);
    let mut p = verrou(&etat.player);
    p.regler_aura(facteur);
    // Les extraits de la demande sont restés en mémoire : un aperçu n'en
    // charge qu'un.
    p.aura().jouer(&demande);
}

/// Au battement natif : un aperçu de plus de 5 s se termine.
pub(crate) fn entretenir_aura(etat: &Etat) {
    let fini = verrou(&etat.aura)
        .apercu
        .is_some_and(|t| t.elapsed() >= APERCU);
    if fini {
        fin_apercu(etat);
    }
}
