// SPDX-License-Identifier: GPL-3.0-or-later
//! La grille de battements : où les temps tombent, et pas seulement à quelle
//! vitesse ils se suivent.
//!
//! **Ce que `descripteurs.rs` ne dit pas.** Il rend un tempo — 124 BPM — et
//! c'est une période, pas une position. Deux morceaux à 124 BPM peuvent pulser
//! en opposition de phase ; les caler l'un sur l'autre demande de savoir *où*
//! est le premier temps. C'est la différence entre « ils vont à la même
//! vitesse » et « ils tombent ensemble ».
//!
//! **Ce que ça débloque, et c'est deux choses, pas une.** La feuille de route
//! classait la grille comme prérequis du mixage DJ (`docs/suite.md`, chantier
//! 8). C'est aussi la moitié manquante de la greffe, déjà livrée : elle cale
//! les tempos et pas les temps forts, et le recours proposé à l'utilisateur —
//! rattraper à la main avec la vitesse par stem — est un aveu.
//!
//! **La méthode.** Le tempo vient d'ici même par l'autocorrélation à peigne
//! de `descripteurs.rs`. La phase s'obtient en glissant un peigne de Dirac de
//! cette période sur l'enveloppe d'attaques et en gardant le décalage qui
//! ramasse le plus d'énergie — c'est ce que fait `beattracking` d'aubio après
//! son autocorrélation, et Mixxx après la sienne.
//!
//! **Deux réserves, et la seconde est la plus lourde.**
//!
//! *Le tempo est supposé constant.* Une grille (période, phase) ne décrit pas
//! un batteur qui accélère ni un live. [`Grille::derive`] mesure ce que cette
//! hypothèse coûte, plutôt que de la laisser tacite.
//!
//! *La phase d'une batterie est presque indéterminée.* C'est la découverte de
//! ce chantier, et elle n'était pas prévue. Sur la batterie de « Hard as a
//! Rock », le meilleur décalage obtient une netteté de 2,96 et le suivant —
//! à 42 % de battement, presque le contretemps — 2,93. Rien ne les départage
//! sérieusement : la caisse claire du 2 et du 4 pèse autant que la grosse
//! caisse du 1 et du 3. [`candidats`] rend la liste pour qu'on le voie.
//!
//! **Conséquence pratique, et elle commande la manière de vérifier** :
//! remesurer la grille d'un signal calé ne prouve rien, on comparerait deux
//! tirages ambigus. C'est [`evaluer`] qu'il faut — poser la grille de
//! référence et regarder ce qu'elle ramasse. Mesuré ainsi sur une vraie
//! greffe : 2,19 avec calage, 1,08 sans, où 1,00 est le score d'une phase
//! tirée au hasard.

use crate::descripteurs::{self, Analyseur, TPS};

/// Finesse de la recherche de phase, en fractions de période. Le pas vaut donc
/// T/64 — à 120 BPM, 7,8 ms, sous le seuil où deux attaques s'entendent
/// séparées.
const PHASES: usize = 64;

/// **Retard du détecteur d'attaques, et il se dérive.** Le flux spectral compare
/// la fenêtre `i` à la précédente ; une attaque n'y apparaît qu'au moment où
/// elle entre dans la partie de `i` que `i-1` ne couvrait pas, c'est-à-dire ses
/// `HOP` derniers échantillons. La trame `i` étant datée de son début, l'attaque
/// est donc placée `(N_FFT - HOP)` échantillons trop tôt.
///
/// **Mesuré avant d'être appliqué** (`examples/latence.rs`) : sur des clics à
/// 150 BPM, où le tempo tombe juste, l'écart vaut −31 ms pour 32 ms dérivés.
/// Aux tempos où le tempo tombe moins juste, l'écart est plus grand — et ce
/// n'est pas la latence, c'est ce que [`affiner`] existe pour corriger.
const LATENCE_S: f32 = (N_FFT_ATTAQUE - HOP_ATTAQUE) as f32 / crate::mel::SR as f32;

/// Doivent rester d'accord avec `descripteurs.rs` — c'est son flux qu'on lit.
const N_FFT_ATTAQUE: usize = 2048;
const HOP_ATTAQUE: usize = 512;

/// Demi-largeur de la recherche fine de période, en proportion.
///
/// **Pourquoi elle est nécessaire.** `descripteurs::tempo` balaie 240 tempos
/// entre 60 et 200 BPM, géométriquement : le pas vaut 0,5 %. C'est sans
/// conséquence pour colorer une carte, et rédhibitoire pour une phase — 0,5 %
/// de 120 BPM, ce sont 2,5 ms par battement, donc 40 ms au bout de 16 secondes
/// et une demi-seconde au bout d'un morceau. ±1 % couvre largement le pas de
/// la grille grossière.
const BANDE: f32 = 0.01;
const PERIODES: usize = 41;

/// Une grille : la période, l'origine, et ce qu'on peut en croire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grille {
    pub bpm: f32,
    /// Instant du premier battement, en secondes depuis le début du signal.
    /// Toujours dans `[0, 60/bpm)` — l'origine est le premier temps, pas un
    /// temps quelconque.
    pub phase_s: f32,
    /// Ce que le peigne ramasse au meilleur décalage, rapporté à ce qu'il
    /// ramasserait à un décalage quelconque. **1,0 = aucune phase**, le signal
    /// n'a pas de temps forts ; au-delà de 2, la pulsation est franche.
    pub nettete: f32,
}

impl Grille {
    /// Période d'un battement, en secondes.
    pub fn periode(&self) -> f32 {
        60.0 / self.bpm
    }

    /// Les instants des battements, en secondes, jusqu'à `duree_s`.
    pub fn battements(&self, duree_s: f32) -> impl Iterator<Item = f32> + '_ {
        let p = self.periode();
        (0..).map(move |i| self.phase_s + i as f32 * p).take_while(move |t| *t < duree_s)
    }

    /// Le battement le plus proche de `t`, en secondes. C'est ce qui sert à
    /// caler : on ne déplace pas au premier temps, on déplace au temps voisin.
    pub fn caler(&self, t: f32) -> f32 {
        let p = self.periode();
        self.phase_s + ((t - self.phase_s) / p).round().max(0.0) * p
    }
}

/// Enveloppe d'attaques d'un signal mono, prête pour l'autocorrélation.
///
/// `signal` est mono à la fréquence du module d'analyse (48 kHz). Passer une
/// autre fréquence décalerait tempo **et** phase sans rien signaler, d'où
/// [`grille_reechantillonnee`] pour les appelants qui n'y sont pas.
fn enveloppe(signal: &[f32], a: &Analyseur) -> Vec<f32> {
    let mut env = descripteurs::flux(&a.spectres(signal));
    descripteurs::centrer(&mut env);
    env
}

/// Ce qu'un peigne de Dirac de période `periode` et de décalage `phi` ramasse
/// sur l'enveloppe. Trames, moyenne par dent — une période longue pose moins
/// de dents, la somme brute la pénaliserait.
fn ramasse(env: &[f32], periode: f32, phi: f32) -> f32 {
    let mut somme = 0.0f32;
    let mut n = 0usize;
    let mut x = phi;
    while x < env.len() as f32 - 1.0 {
        // Interpolation linéaire : la période est fractionnaire, les
        // battements ne tombent pas sur des trames entières.
        let (j, t) = (x.floor() as usize, x.fract());
        somme += env[j] * (1.0 - t) + env[j + 1] * t;
        n += 1;
        x += periode;
    }
    if n == 0 {
        0.0
    } else {
        somme / n as f32
    }
}

/// Affine conjointement période et phase autour d'une période grossière.
///
/// **Conjointement, et c'est le point.** L'autocorrélation a donné une période
/// à 0,5 % près, ce qui est trop grossier pour une phase : l'erreur s'accumule
/// battement après battement, et le meilleur décalage global devient un
/// compromis entre le début et la fin du morceau. On cherche donc le couple
/// (période, phase) qui explique le mieux les attaques, au lieu de prendre la
/// période d'un critère et la phase d'un autre.
///
/// C'est aussi ce qui distingue cette recherche de l'autocorrélation : celle-ci
/// compare le signal à lui-même et est **aveugle à la phase par construction**.
///
/// Rend `(période en trames, phase en trames, netteté)`.
fn affiner(env: &[f32], grossiere: f32) -> (f32, f32, f32) {
    if grossiere <= 1.0 || env.is_empty() {
        return (grossiere, 0.0, 1.0);
    }
    let mut meilleur = (grossiere, 0.0f32, 0.0f32);
    let mut total = 0.0f32;
    let mut essais = 0usize;

    for p in 0..PERIODES {
        let f = 1.0 - BANDE + 2.0 * BANDE * p as f32 / (PERIODES - 1) as f32;
        let periode = grossiere * f;
        for i in 0..PHASES {
            let phi = periode * i as f32 / PHASES as f32;
            let score = ramasse(env, periode, phi);
            total += score;
            essais += 1;
            if score > meilleur.2 {
                meilleur = (periode, phi, score);
            }
        }
    }

    // Netteté : ce que le meilleur couple ramasse, rapporté à ce qu'un couple
    // quelconque ramasserait. 1,0 = aucune structure, le signal n'a pas de
    // temps forts.
    let moyenne = total / essais.max(1) as f32;
    let nettete = if moyenne > f32::EPSILON {
        meilleur.2 / moyenne
    } else {
        1.0
    };
    (meilleur.0, meilleur.1, nettete)
}

/// Ce qu'une grille **imposée** vaut sur un signal.
///
/// **Pourquoi cette fonction existe.** Vérifier qu'une greffe est calée en
/// remesurant sa grille ne marche pas : la phase d'une batterie est presque
/// indéterminée (voir [`candidats`]), et l'on comparerait deux tirages
/// ambigus. Ce qu'on veut savoir est autre chose — *le signal pulse-t-il là où
/// on le lui a demandé ?* On pose donc la grille de référence et l'on regarde
/// ce qu'elle ramasse, rapporté à ce que ramasserait une phase quelconque.
///
/// Même échelle que [`Grille::nettete`] : 1,0 = cette grille ne vaut pas mieux
/// qu'une phase tirée au hasard.
pub fn evaluer(signal: &[f32], a: &Analyseur, bpm: f32, phase_s: f32) -> Option<f32> {
    // `is_finite` en plus du signe : un NaN passerait `<= 0.0`.
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let env = enveloppe(signal, a);
    if env.is_empty() {
        return None;
    }
    let periode = 60.0 * TPS / bpm;
    // La phase est décalée de la latence en sens inverse : `grille` l'ajoute
    // pour rendre un instant réel, on la retire pour retrouver la trame.
    let phi = ((phase_s - LATENCE_S) * TPS).rem_euclid(periode);

    let moyenne = (0..PHASES)
        .map(|i| ramasse(&env, periode, periode * i as f32 / PHASES as f32))
        .sum::<f32>()
        / PHASES as f32;
    if moyenne <= f32::EPSILON {
        return None;
    }
    Some(ramasse(&env, periode, phi) / moyenne)
}

/// Les meilleurs couples (période, phase), du plus fort au plus faible.
///
/// **Sert à voir ce que la grille ne dit pas.** Une batterie répond au peigne
/// sur le temps *et* sur le contretemps : la caisse claire du 2 et du 4 pèse
/// souvent autant que la grosse caisse du 1 et du 3. Le meilleur score peut
/// alors basculer d'un morceau à l'autre, et deux grilles justes se retrouver
/// à un demi-battement l'une de l'autre. Rien dans `nettete` ne le signale —
/// les deux sont nets, c'est le choix entre eux qui est arbitraire.
pub fn candidats(signal: &[f32], a: &Analyseur, k: usize) -> Vec<Grille> {
    let env = enveloppe(signal, a);
    let Some(grossier) = descripteurs::tempo(&env) else {
        return Vec::new();
    };
    let grossiere = 60.0 * TPS / grossier;

    let mut tous: Vec<(f32, f32, f32)> = Vec::new();
    let mut total = 0.0f32;
    for p in 0..PERIODES {
        let f = 1.0 - BANDE + 2.0 * BANDE * p as f32 / (PERIODES - 1) as f32;
        let periode = grossiere * f;
        for i in 0..PHASES {
            let phi = periode * i as f32 / PHASES as f32;
            let score = ramasse(&env, periode, phi);
            total += score;
            tous.push((periode, phi, score));
        }
    }
    let moyenne = total / tous.len().max(1) as f32;
    tous.sort_by(|a, b| b.2.total_cmp(&a.2));

    // Un seul représentant par phase : les 41 périodes voisines rendraient
    // quarante fois le même couple, à un cheveu près.
    let mut sortie: Vec<Grille> = Vec::new();
    for (periode, phi, score) in tous {
        let bpm = 60.0 * TPS / periode;
        let periode_s = 60.0 / bpm;
        let phase_s = (phi / TPS + LATENCE_S).rem_euclid(periode_s);
        if sortie.iter().any(|g| {
            let d = (g.phase_s - phase_s).abs();
            d.min(periode_s - d) < periode_s / 8.0
        }) {
            continue;
        }
        sortie.push(Grille {
            bpm,
            phase_s,
            nettete: if moyenne > f32::EPSILON {
                score / moyenne
            } else {
                1.0
            },
        });
        if sortie.len() >= k {
            break;
        }
    }
    sortie
}

/// La grille d'un signal mono à 48 kHz.
///
/// `None` quand rien ne pulse — le silence, en pratique, comme pour le tempo.
/// **La même réserve vaut ici** : un conte lu reçoit un tempo qui ne veut rien
/// dire, il recevra donc une phase qui n'en veut pas davantage. C'est
/// [`Grille::nettete`] qui sert à ne pas y croire, pas l'absence de valeur.
pub fn grille(signal: &[f32], a: &Analyseur) -> Option<Grille> {
    let env = enveloppe(signal, a);
    let grossier = descripteurs::tempo(&env)?;
    let (periode, phi, nettete) = affiner(&env, 60.0 * TPS / grossier);
    let bpm = 60.0 * TPS / periode;

    // La latence du détecteur place les attaques trop tôt ; on la rend. Puis on
    // ramène l'origine dans la première période — une phase est définie modulo
    // la période, et une origine négative surprendrait tout appelant.
    let periode_s = 60.0 / bpm;
    let phase_s = (phi / TPS + LATENCE_S).rem_euclid(periode_s);

    Some(Grille {
        bpm,
        phase_s,
        nettete,
    })
}

/// La même, pour un signal qui n'est pas à 48 kHz — le cas de l'éditeur, dont
/// les stems sont en 44,1 kHz stéréo.
///
/// **Rééchantillonner plutôt que de recalculer les constantes.** Le module
/// d'analyse fige `N_FFT`, `HOP` et `TPS` pour 48 kHz ; les rendre variables
/// ferait dépendre le tempo mesuré de la fréquence du fichier, ce qui est
/// exactement le genre d'écart qu'on ne verrait jamais. L'interpolation
/// linéaire suffit : on cherche des attaques, pas des harmoniques.
pub fn grille_reechantillonnee(signal: &[f32], sr: u32, a: &Analyseur) -> Option<Grille> {
    const CIBLE: u32 = crate::mel::SR;
    if signal.is_empty() {
        return None;
    }
    if sr == CIBLE {
        return grille(signal, a);
    }
    let rapport = sr as f64 / CIBLE as f64;
    let n = (signal.len() as f64 / rapport) as usize;
    let mono: Vec<f32> = (0..n)
        .map(|i| {
            let x = i as f64 * rapport;
            let (j, t) = (x.floor() as usize, (x.fract()) as f32);
            let a = signal[j.min(signal.len() - 1)];
            let b = signal[(j + 1).min(signal.len() - 1)];
            a * (1.0 - t) + b * t
        })
        .collect();
    grille(&mono, a)
}

/// Ce que l'hypothèse de tempo constant coûte sur ce morceau-ci.
///
/// On refait la grille sur la première et la dernière moitié, et on regarde de
/// combien les deux phases auraient divergé au bout du morceau. **C'est la
/// mesure que la spec du module 3 réclamait sans la nommer** : « les deux
/// batteries pulsent au même tempo, sans garantie de tomber sur le même
/// temps ».
impl Grille {
    /// Décalage accumulé, en secondes, entre cette grille et `autre` au bout
    /// de `duree_s`. Zéro si les deux tempos sont égaux.
    pub fn derive(&self, autre: &Grille, duree_s: f32) -> f32 {
        let battements = duree_s / self.periode();
        (self.periode() - autre.periode()).abs() * battements
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mel::SR;

    /// Clics réguliers avec un silence devant : la seule vérité terrain
    /// fabricable sans jeu annoté, et la seule qui teste la phase — un signal
    /// qui commence sur un clic aurait toujours la phase 0.
    fn clics(bpm: f32, secondes: f32, retard_s: f32) -> Vec<f32> {
        let n = (SR as f32 * secondes) as usize;
        let periode = (SR as f32 * 60.0 / bpm) as usize;
        let debut = (SR as f32 * retard_s) as usize;
        let mut s = vec![0.0f32; n];
        for d in (debut..n).step_by(periode) {
            let duree = (SR as usize / 50).min(n - d);
            for i in 0..duree {
                let t = i as f32 / duree as f32;
                s[d + i] = (1.0 - t) * (((i * 7919) % 2001) as f32 / 1000.0 - 1.0);
            }
        }
        s
    }

    #[test]
    fn la_grille_retrouve_une_phase_connue() {
        let a = Analyseur::new();
        let bpm = 120.0;
        let periode = 60.0 / bpm;
        for retard in [0.0f32, 0.12, 0.31] {
            let g = grille(&clics(bpm, 16.0, retard), &a).expect("pulsation régulière");
            // La phase est définie modulo la période : un retard de 0,62 s à
            // 120 BPM (période 0,5 s) est la phase 0,12.
            let attendue = retard % periode;
            let brut = (g.phase_s - attendue).abs();
            let ecart = brut.min(periode - brut);
            // **La borne dit ce que la méthode atteint, pas ce qu'on espère.**
            // Le balayage pose `PHASES` décalages par période : le pas vaut
            // T/64, soit 7,8 ms à 120 BPM, et rien ne peut faire mieux. Deux
            // pas de tolérance. Mesuré à ce jour : au pire 7,6 ms d'erreur,
            // tempos et retards confondus (`examples/latence.rs`).
            assert!(
                ecart < periode / 32.0,
                "retard {retard} → phase {:.3}, attendue {attendue:.3} ({:.1} ms)",
                g.phase_s,
                ecart * 1000.0
            );
        }
    }

    /// Une pulsation franche doit se voir dans la netteté. Sans ce chiffre,
    /// rien ne distinguerait une grille juste d'une grille posée sur du bruit.
    #[test]
    fn la_nettete_separe_la_pulsation_du_bruit() {
        let a = Analyseur::new();
        let franche = grille(&clics(120.0, 16.0, 0.1), &a).expect("clics");

        // Bruit blanc : aucune structure temporelle. `tempo` rend tout de même
        // une valeur — c'est sa limite connue — mais la phase ne doit rien
        // ressortir.
        let bruit: Vec<f32> = (0..SR as usize * 16)
            .map(|i| ((i * 7919 % 2003) as f32 / 1000.0) - 1.0)
            .collect();
        let flou = grille(&bruit, &a);

        assert!(
            franche.nettete > 2.0,
            "clics : netteté {:.2}, attendue > 2",
            franche.nettete
        );
        if let Some(flou) = flou {
            assert!(
                flou.nettete < franche.nettete,
                "bruit {:.2} ≥ clics {:.2}",
                flou.nettete,
                franche.nettete
            );
        }
    }

    #[test]
    fn caler_ramene_au_battement_voisin() {
        let g = Grille {
            bpm: 120.0,
            phase_s: 0.1,
            nettete: 3.0,
        };
        // Période 0,5 s, battements à 0,1 · 0,6 · 1,1 · 1,6…
        assert!((g.caler(0.7) - 0.6).abs() < 1e-5);
        assert!((g.caler(0.9) - 1.1).abs() < 1e-5);
        // Avant le premier temps, on ne recule pas dans le négatif.
        assert!((g.caler(0.0) - 0.1).abs() < 1e-5);
    }

    #[test]
    fn les_battements_couvrent_la_duree() {
        let g = Grille {
            bpm: 120.0,
            phase_s: 0.1,
            nettete: 3.0,
        };
        let t: Vec<f32> = g.battements(2.0).collect();
        assert_eq!(t.len(), 4);
        assert!((t[0] - 0.1).abs() < 1e-5 && (t[3] - 1.6).abs() < 1e-5);
    }

    /// Le rééchantillonnage ne doit changer ni le tempo ni la phase : c'est
    /// exactement le genre d'écart silencieux que la fonction existe pour
    /// éviter.
    #[test]
    fn le_reechantillonnage_conserve_la_grille() {
        let a = Analyseur::new();
        let attendu = grille(&clics(120.0, 16.0, 0.15), &a).expect("clics");

        // Les mêmes clics fabriqués à 44 100 Hz.
        let sr = 44_100u32;
        let n = (sr as f32 * 16.0) as usize;
        let periode = (sr as f32 * 60.0 / 120.0) as usize;
        let debut = (sr as f32 * 0.15) as usize;
        let mut s = vec![0.0f32; n];
        for d in (debut..n).step_by(periode) {
            let duree = (sr as usize / 50).min(n - d);
            for i in 0..duree {
                let t = i as f32 / duree as f32;
                s[d + i] = (1.0 - t) * (((i * 7919) % 2001) as f32 / 1000.0 - 1.0);
            }
        }
        let obtenu = grille_reechantillonnee(&s, sr, &a).expect("clics à 44,1 kHz");
        assert!(
            (obtenu.bpm - attendu.bpm).abs() / attendu.bpm < 0.03,
            "{} contre {}",
            obtenu.bpm,
            attendu.bpm
        );
        assert!(
            (obtenu.phase_s - attendu.phase_s).abs() < 0.03,
            "phase {:.3} contre {:.3}",
            obtenu.phase_s,
            attendu.phase_s
        );
    }
}
