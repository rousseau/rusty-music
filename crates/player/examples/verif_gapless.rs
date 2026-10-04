// SPDX-License-Identifier: GPL-3.0-or-later
//! Le raccord entre deux pistes est-il sans blanc ni recouvrement ?
//!
//! ```bash
//! scripts/gapless-fixtures.sh /tmp/gapless
//! cargo run --release -p rusty-music-player --example verif_gapless -- /tmp/gapless
//! ```
//!
//! Chaque format est ouvert par [`rusty_music_player::ouvrir`] — le chemin
//! exact de la lecture, rééchantillonnage vers la sortie compris — pour la
//! moitié A puis la moitié B d'un même balayage de fréquence, coupé à
//! l'échantillon près (voir le script). On mesure :
//!
//! - `L` : de combien le contenu d'une moitié arrive **en retard** sur son
//!   instant idéal (négatif : le début est rogné) — corrélation avec le
//!   balayage théorique, au pas de l'échantillon ;
//! - `fin A` : silence (positif) ou rognage (négatif) à la fin de A ;
//! - `raccord` = `fin A` + `L` de B : ce qui s'insère (blanc) ou se perd
//!   (saut) entre les deux dans la file. **Zéro, c'est le gapless.**
//!
//! Seuil de défaut retenu : 5 ms (`docs/plan-ecouter-v0.2.md`, point 1).

use std::path::Path;

use rodio::Source;

const SEUIL_MS: f64 = 5.0;
/// Fenêtre de corrélation : assez longue pour que le balayage soit net
/// (résolution ~ 1 / largeur de bande balayée dans la fenêtre).
const FENETRE: usize = 32_768;
const DEBUT_FENETRE: usize = 4_000;
const DECALAGE_MAX: i64 = 3_000;
const FORMATS: [&str; 6] = ["wav", "flac", "mp3", "m4a", "ogg", "opus"];

/// Balayage idéal à l'instant `t` (s) — même formule que le script.
fn ideal(t: f64) -> f64 {
    0.5 * (2.0 * std::f64::consts::PI * (200.0 * t + 1000.0 * t * t)).sin()
}

struct Piste {
    taux: u32,
    trames: usize,
    mono: Vec<f32>,
}

fn ouvrir(chemin: &Path) -> Option<Piste> {
    let source = rusty_music_player::ouvrir(chemin, 1.0).ok()?;
    let taux = source.sample_rate().get();
    let canaux = source.channels().get() as usize;
    let tout: Vec<f32> = source.collect();
    Some(Piste {
        taux,
        trames: tout.len() / canaux,
        mono: tout.iter().step_by(canaux).copied().collect(),
    })
}

/// Retard (en échantillons) du contenu de `p` sur le balayage idéal, dont il
/// est censé commencer à `origine_s` secondes.
fn retard(p: &Piste, origine_s: f64) -> i64 {
    let sr = f64::from(p.taux);
    let mut meilleur = (f64::MIN, 0_i64);
    for l in -DECALAGE_MAX..=DECALAGE_MAX {
        let mut somme = 0.0;
        for n in 0..FENETRE {
            let i = DEBUT_FENETRE + n;
            let t = origine_s + (i as f64 - l as f64) / sr;
            somme += f64::from(p.mono[i]) * ideal(t);
        }
        if somme > meilleur.0 {
            meilleur = (somme, l);
        }
    }
    meilleur.1
}

fn mesurer(dossier: &Path, ext: &str) -> Option<(f64, f64, f64, f64)> {
    let a = ouvrir(&dossier.join(format!("gapless-a.{ext}")))?;
    let b = ouvrir(&dossier.join(format!("gapless-b.{ext}")))?;
    if a.mono.len() < DEBUT_FENETRE + FENETRE || b.mono.len() < DEBUT_FENETRE + FENETRE {
        return None;
    }
    let sr = f64::from(a.taux);
    let ms = |ech: i64| ech as f64 / sr * 1000.0;
    let attendu = (3.0 * sr).round() as i64;
    let l_a = retard(&a, 0.0);
    let l_b = retard(&b, 3.0);
    let fin_a = a.trames as i64 - l_a - attendu;
    let raccord = fin_a + l_b;
    let _ = b.trames;
    Some((ms(l_a), ms(l_b), ms(fin_a), ms(raccord)))
}

fn main() {
    let dossier = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/gapless".into());
    let dossier = Path::new(&dossier);
    let mut defauts = 0;

    for sortie in [0_u32, 48_000] {
        rusty_music_player::enregistrer_taux_sortie(sortie);
        let titre = if sortie == 0 {
            "sortie : fréquence native".to_string()
        } else {
            format!("sortie : {sortie} Hz (rééchantillonnage)")
        };
        println!("\n{titre}");
        println!(
            "{:<6} {:>9} {:>9} {:>9} {:>10}   verdict",
            "format", "L(A) ms", "L(B) ms", "fin A ms", "raccord ms"
        );
        for ext in FORMATS {
            match mesurer(dossier, ext) {
                None => println!("{ext:<6} (fichiers absents ou illisibles)"),
                Some((l_a, l_b, fin_a, raccord)) => {
                    let ok = raccord.abs() <= SEUIL_MS;
                    if !ok {
                        defauts += 1;
                    }
                    println!(
                        "{ext:<6} {l_a:>9.2} {l_b:>9.2} {fin_a:>9.2} {raccord:>10.2}   {}",
                        if ok { "ok" } else { "DÉFAUT" }
                    );
                }
            }
        }
    }
    println!("\n{defauts} raccord(s) au-delà de {SEUIL_MS} ms");
    if defauts > 0 {
        std::process::exit(1);
    }
}
