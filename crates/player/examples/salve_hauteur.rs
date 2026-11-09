// SPDX-License-Identifier: GPL-3.0-or-later
//! E6 : ce que coûte la transposition en flux dans le rappel audio.
//!
//! ```bash
//! cargo run --release -p rusty-music-player --features hauteur --example salve_hauteur
//! ```
//!
//! Le cas le plus lourd de l'établi : **quatre stems**, chacun ralenti par
//! `wsola` (vitesse 0,5) puis transposé par Signalsmith (±2 demi-tons), rendus
//! par blocs de 128 trames comme le rappel audio les demande. On mesure la
//! pire salve (le plus lent des blocs, minimum sur trois passes : une salve
//! isolée peut venir d'ailleurs que du calcul) et la charge moyenne. Aucun son
//! n'est joué.
//!
//! Plusieurs réglages de Signalsmith (bloc d'analyse, pas) : la justesse
//! dépend du pas (`hauteur.rs`), et le coût aussi.

use std::time::Instant;

use rusty_music_player::hauteur::Transpositeur;

const SR: u32 = 44_100;
const CANAUX: usize = 2;
const STEMS: usize = 4;
const BLOC: usize = 128;
const SECONDES: usize = 20;
const PASSES: usize = 3;
/// Échéance d'un tampon de 128 trames, et celle du tampon CoreAudio courant.
const ECHEANCE_128: f64 = BLOC as f64 / SR as f64 * 1000.0;

/// (nom, vitesse 0,5 par wsola, bloc et pas de Signalsmith).
type Reglage = (&'static str, bool, Option<(usize, usize)>);

struct Voix {
    etireur: wsola::TimeStretch,
    entree: usize,
    fifo: Vec<f32>,
    lus: usize,
    t: Transpositeur,
    brut: Vec<f32>,
    sortie: Vec<f32>,
}

fn main() {
    let mut graine = 0x2545_F491_4F6C_DD1Du64;
    let mut alea = || {
        graine ^= graine >> 12;
        graine ^= graine << 25;
        graine ^= graine >> 27;
        (graine.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 8_388_608.0 - 1.0
    };
    // Bruit à -6 dB : pas de silence que Signalsmith saurait sauter.
    let matiere: Vec<f32> = (0..SR as usize * SECONDES * CANAUX)
        .map(|_| 0.5 * alea())
        .collect();

    println!(
        "{STEMS} stems · blocs de {BLOC} trames (échéance {ECHEANCE_128:.2} ms) · \
         vitesse 0,5 (wsola) et/ou hauteur ±2 (Signalsmith)\n"
    );
    println!(
        "| réglage | latence | pire salve | 99e centile | médiane | charge moyenne |\n\
         |---|---|---|---|---|---|"
    );
    // (nom, vitesse 0,5 par wsola, réglage de Signalsmith)
    let reglages: [Reglage; 9] = [
        ("wsola seul", true, None),
        ("wsola + préréglage (5292/1323)", true, Some((5292, 1323))),
        ("wsola + 8192/512", true, Some((8192, 512))),
        ("wsola + 8192/256", true, Some((8192, 256))),
        ("wsola + 16384/512", true, Some((16384, 512))),
        ("Signalsmith seul, préréglage", false, Some((5292, 1323))),
        ("Signalsmith seul, 8192/512", false, Some((8192, 512))),
        ("Signalsmith seul, 8192/256", false, Some((8192, 256))),
        ("Signalsmith seul, 16384/512", false, Some((16384, 512))),
    ];
    for (nom, ralenti, reglage) in reglages {
        let mut pires = Vec::new();
        let mut temps_total = Vec::new();
        let mut toutes = Vec::new();
        let mut latence = 0;
        for _ in 0..PASSES {
            let mut voix: Vec<Voix> = (0..STEMS)
                .map(|i| {
                    let mut etireur = wsola::TimeStretch::new(SR, CANAUX as u16).expect("étireur");
                    etireur.set_tempo(0.5);
                    let mut t = match reglage {
                        Some((b, p)) => Transpositeur::avec(b, p),
                        None => Transpositeur::new(),
                    };
                    t.regler(if i % 2 == 0 { 2.0 } else { -2.0 });
                    latence = t.latence();
                    Voix {
                        etireur,
                        entree: 0,
                        fifo: Vec::with_capacity(1 << 16),
                        lus: 0,
                        t,
                        brut: vec![0.0; BLOC * CANAUX],
                        sortie: vec![0.0; BLOC * CANAUX],
                    }
                })
                .collect();
            let blocs = SR as usize * SECONDES / BLOC;
            let mut durees = Vec::with_capacity(blocs);
            let debut = Instant::now();
            for _ in 0..blocs {
                let t0 = Instant::now();
                for v in &mut voix {
                    if !ralenti {
                        // Vitesse 1 : la matière telle quelle, comme la voie
                        // directe de `Voix`.
                        let a = v.entree % (matiere.len() - BLOC * CANAUX);
                        v.brut.copy_from_slice(&matiere[a..a + BLOC * CANAUX]);
                        v.entree = a + BLOC * CANAUX;
                    }
                    // wsola : ce qu'il faut pour 128 trames de sortie.
                    let mut k = if ralenti { 0 } else { BLOC * CANAUX };
                    while k < BLOC * CANAUX {
                        if v.lus >= v.fifo.len() {
                            let a = v.entree % matiere.len();
                            let b = (a + BLOC * CANAUX).min(matiere.len());
                            v.etireur.push(&matiere[a..b]);
                            v.entree = b;
                            v.fifo = v.etireur.pull(usize::MAX);
                            v.lus = 0;
                            continue;
                        }
                        v.brut[k] = v.fifo[v.lus];
                        v.lus += 1;
                        k += 1;
                    }
                    if reglage.is_some() {
                        v.t.traiter(&v.brut, &mut v.sortie);
                    }
                    std::hint::black_box(&v.sortie);
                }
                durees.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            temps_total.push(debut.elapsed().as_secs_f64());
            pires.push(durees.iter().cloned().fold(0.0, f64::max));
            toutes.extend(durees);
        }
        toutes.sort_by(f64::total_cmp);
        let centile = |q: f64| toutes[((toutes.len() - 1) as f64 * q) as usize];
        let pire = pires.iter().cloned().fold(f64::MAX, f64::min);
        let charge = temps_total.iter().sum::<f64>() / (PASSES * SECONDES) as f64 * 100.0;
        println!(
            "| {nom} | {} | {pire:.2} ms | {:.3} ms | {:.3} ms | {charge:.1} % d'un cœur |",
            if reglage.is_some() {
                format!(
                    "{latence} trames ({:.0} ms)",
                    latence as f64 / SR as f64 * 1000.0
                )
            } else {
                "—".into()
            },
            centile(0.99),
            centile(0.5),
        );
    }
}
