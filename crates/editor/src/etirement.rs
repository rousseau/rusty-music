// SPDX-License-Identifier: GPL-3.0-or-later
//! Étirement temporel et transposition.
//!
//! **Vérifié avant d'écrire quoi que ce soit** : `wsola` fait déjà l'étirement
//! à hauteur préservée, en Rust pur et sans dépendance transitive. On s'en sert
//! plutôt que de maintenir un vocodeur de phase écrit à la main — c'était
//! cinq cents lignes de la partie la plus délicate du traitement du signal, et
//! ses artefacts de phase sur les transitoires n'étaient pas mesurables avec
//! les témoins qu'on savait calculer.
//!
//! La méthode de `wsola` — recouvrement-addition par similarité de forme
//! d'onde — est celle d'`atempo` chez ffmpeg et de VLC. Temporelle, donc sans
//! artefact de phase.
//!
//! Ce qui reste ici : la **transposition**, que `wsola` ne fait pas. Depuis E6
//! (9 nov. 2026), elle passe par Signalsmith Stretch, hors ligne, en réglage
//! juste (voir [`transposer`]). L'ancienne voie — étirer par `wsola` puis
//! rééchantillonner du même rapport — ne sert plus que de repli pour les
//! signaux trop courts.

/// Fréquence de travail de l'éditeur.
const SR: u32 = 44_100;

/// Étire un signal entrelacé sans changer sa hauteur.
///
/// `facteur` est un facteur de **durée** : 1,25 rend le morceau un quart plus
/// long. `wsola` raisonne en tempo, qui en est l'inverse.
pub fn etirer(signal: &[f32], canaux: usize, facteur: f32) -> Vec<f32> {
    if canaux == 0 || signal.is_empty() || (facteur - 1.0).abs() < 1e-6 {
        return signal.to_vec();
    }
    let tempo = (1.0 / facteur.clamp(0.25, 4.0)).clamp(0.25, 4.0);
    let out = wsola::stretch(signal, SR, canaux as u16, tempo).unwrap_or_default();
    // Un signal plus court que la fenêtre de l'étireur n'en ressort pas : il
    // n'y a pas de quoi chercher une similarité de forme d'onde. On rend alors
    // la matière telle quelle plutôt que le silence — la bibliothèque contient
    // des pistes d'une seconde.
    if out.is_empty() {
        return signal.to_vec();
    }
    out
}

/// Bloc d'analyse de Signalsmith pour la transposition, en trames (186 ms).
const BLOC_HAUTEUR: usize = 8192;
/// Pas d'analyse, en trames (5,1 ms). C'est le pas, plus que le bloc, qui
/// règle la justesse : le préréglage (bloc 5 292, pas 1 323) laissait
/// +22 cents à 82 Hz et +8 à 110 Hz ; 8192/224 tient 1,9 cent de 82 à
/// 220 Hz et 0,9 au-dessus (test `la_transposition_est_juste_au_cent`).
/// 16384/256 descend à 1,05 cent dans le grave, mais coûte 15 s par stem au
/// lieu de 8 (journal, E6, tranche 2).
const PAS_HAUTEUR: usize = 224;

/// Transpose de `demi_tons` sans changer la durée.
///
/// **Signalsmith Stretch, hors ligne et juste** (E6, décision de François du
/// 9 nov. : préféré à l'écoute d'E3 pour la hauteur). `exact` traite le
/// signal entier d'un coup, compense la latence du vocodeur et rend
/// exactement autant de trames qu'il en a reçu. Le pas fin coûte cher en
/// flux (4,5 ms de pire salve à 4 stems, au-delà de l'échéance de 128
/// trames) ; hors ligne, il ne coûte que du temps de calcul : 8,4 s pour un
/// stem de 224 s, contre une vingtaine par l'ancienne voie.
///
/// Un signal plus court que deux latences de sortie (≈ 0,2 s) ne passe pas
/// par `exact` : il prend l'ancienne voie, `wsola` puis rééchantillonnage.
pub fn transposer(signal: &[f32], canaux: usize, demi_tons: f32) -> Vec<f32> {
    if canaux == 0 || demi_tons.abs() < 1e-6 {
        return signal.to_vec();
    }
    let trames = signal.len() / canaux;
    let utile = &signal[..trames * canaux];
    let mut s = signalsmith_stretch::Stretch::new(canaux as u32, BLOC_HAUTEUR, PAS_HAUTEUR);
    s.set_transpose_factor_semitones(demi_tons, None);
    let mut out = vec![0.0; utile.len()];
    if s.exact(utile, &mut out) {
        return out;
    }
    transposer_par_wsola(utile, canaux, demi_tons)
}

/// L'ancienne transposition : étirer puis rééchantillonner du même rapport.
/// L'inverse — rééchantillonner puis étirer — donnerait le même résultat en
/// théorie, mais ferait travailler l'étireur sur un signal déjà interpolé.
fn transposer_par_wsola(signal: &[f32], canaux: usize, demi_tons: f32) -> Vec<f32> {
    let rapport = 2f32.powf(demi_tons / 12.0);
    let etire = etirer(signal, canaux, rapport);
    reechantillonner_entrelace(&etire, canaux, rapport)
}

/// Rééchantillonne sans jamais désentrelacer.
///
/// **Cinq tampons pleins sont devenus un**, et sur des stems entiers cela se
/// compte : la version précédente séparait le signal en canaux, rééchantillonnait
/// chacun, puis réentrelaçait — trois copies complètes en plus de l'entrée et
/// de la sortie. Sur quatre stems de quatre minutes transposés **en parallèle**,
/// c'était la différence entre tenir en mémoire et faire pagineur la machine,
/// et une machine qui pagine fige l'interface aussi sûrement qu'un calcul dans
/// le fil principal.
///
/// La lecture se fait au pas de `canaux`, l'écriture est séquentielle : c'est
/// aussi meilleur pour le cache que le rassemblement-dispersion d'avant.
fn reechantillonner_entrelace(signal: &[f32], canaux: usize, rapport: f32) -> Vec<f32> {
    let trames = signal.len() / canaux.max(1);
    if canaux == 0 || trames < 4 || (rapport - 1.0).abs() < 1e-6 {
        return signal.to_vec();
    }
    let cible = (trames as f32 / rapport) as usize;
    let mut out = Vec::with_capacity(cible * canaux);
    let dernier = trames as isize - 1;

    for i in 0..cible {
        let x = i as f32 * rapport;
        let j = x.floor() as isize;
        let t = x - j as f32;
        for c in 0..canaux {
            // Catmull-Rom, comme avant : l'interpolation linéaire ternit tout
            // le haut du spectre.
            let e = |d: isize| signal[(j + d).clamp(0, dernier) as usize * canaux + c];
            let (p0, p1, p2, p3) = (e(-1), e(0), e(1), e(2));
            out.push(
                p1 + 0.5
                    * t
                    * (p2 - p0
                        + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3
                            + t * (3.0 * (p1 - p2) + p3 - p0))),
            );
        }
    }
    out
}

/// Sépare un signal entrelacé en canaux. **Réservé aux tests** depuis que la
/// transposition rééchantillonne sans désentrelacer : c'est ce qui permet de
/// vérifier que les deux voies restent alignées.
#[cfg(test)]
fn separer(signal: &[f32], canaux: usize) -> Vec<Vec<f32>> {
    (0..canaux)
        .map(|c| signal.iter().skip(c).step_by(canaux).copied().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::{num_complex::Complex, FftPlanner};

    const TAU: f32 = std::f32::consts::TAU;
    const HZ: f32 = SR as f32;

    fn sinus(hz: f32, secondes: f32) -> Vec<f32> {
        let n = (HZ * secondes) as usize;
        (0..n).map(|i| (TAU * hz * i as f32 / HZ).sin()).collect()
    }

    /// Fréquence dominante, par transformée sur la partie centrale.
    fn dominante(signal: &[f32]) -> f32 {
        let n = 8192;
        assert!(signal.len() >= n, "signal trop court pour être mesuré");
        let debut = (signal.len() - n) / 2;
        let mut buf: Vec<Complex<f32>> = (0..n)
            .map(|i| {
                let w = 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos();
                Complex::new(signal[debut + i] * w, 0.0)
            })
            .collect();
        FftPlanner::new().plan_fft_forward(n).process(&mut buf);
        let k = (1..n / 2)
            .max_by(|a, b| buf[*a].norm().total_cmp(&buf[*b].norm()))
            .unwrap_or(0);
        k as f32 * HZ / n as f32
    }

    /// Ce que l'étirement promet : la durée change, la hauteur non.
    #[test]
    fn letirement_change_la_duree_pas_la_hauteur() {
        let s = sinus(440.0, 2.0);
        for facteur in [0.5f32, 1.5, 2.0] {
            let out = etirer(&s, 1, facteur);
            let attendu = s.len() as f32 * facteur;
            let ecart = (out.len() as f32 - attendu).abs() / attendu;
            assert!(ecart < 0.05, "durée ×{facteur} : {ecart:.3} d'écart");

            let hz = dominante(&out);
            assert!(
                (hz - 440.0).abs() < 12.0,
                "hauteur déplacée à {hz:.0} Hz pour un étirement ×{facteur}"
            );
        }
    }

    /// Et la transposition l'inverse : la hauteur change, la durée non.
    #[test]
    fn la_transposition_change_la_hauteur_pas_la_duree() {
        let s = sinus(220.0, 2.0);
        for (demi_tons, attendu) in [(12.0f32, 440.0f32), (-12.0, 110.0), (7.0, 329.6)] {
            let out = transposer(&s, 1, demi_tons);
            let hz = dominante(&out);
            let ecart = (hz - attendu).abs() / attendu;
            assert!(
                ecart < 0.04,
                "{demi_tons} demi-tons : {hz:.0} Hz au lieu de {attendu:.0}"
            );

            let duree = (out.len() as f32 - s.len() as f32).abs() / s.len() as f32;
            assert!(duree < 0.08, "durée déplacée de {:.1} %", duree * 100.0);
        }
    }

    /// Un facteur neutre rend le signal tel quel : on ne fait rien passer dans
    /// l'étireur quand il n'y a rien à étirer.
    #[test]
    fn un_facteur_neutre_rend_le_signal() {
        let s = sinus(440.0, 0.5);
        assert_eq!(etirer(&s, 1, 1.0), s);
        assert_eq!(transposer(&s, 1, 0.0), s);
    }

    /// La bibliothèque contient des pistes plus courtes qu'une fenêtre.
    #[test]
    fn un_signal_trop_court_ne_panique_pas() {
        assert!(!etirer(&[0.1; 100], 1, 2.0).is_empty());
        assert!(etirer(&[], 0, 2.0).is_empty());
        assert_eq!(transposer(&[0.1; 100], 1, 0.0).len(), 100);
    }

    /// Le stéréo doit rester du stéréo, et de la même longueur des deux côtés.
    #[test]
    fn le_stereo_reste_aligne() {
        let g = sinus(440.0, 1.0);
        let d = sinus(660.0, 1.0);
        let entrelace: Vec<f32> = g.iter().zip(&d).flat_map(|(a, b)| [*a, *b]).collect();
        let out = etirer(&entrelace, 2, 1.5);
        assert_eq!(out.len() % 2, 0, "sortie stéréo impaire");
        let canaux = separer(&out, 2);
        assert_eq!(canaux[0].len(), canaux[1].len());
        assert!((dominante(&canaux[0]) - 440.0).abs() < 12.0);
        assert!((dominante(&canaux[1]) - 660.0).abs() < 12.0);
    }

    /// Fréquence par passages par zéro interpolés, sur la moitié centrale :
    /// sur une sinusoïde, bien plus fin que la transformée (5 Hz par case).
    fn frequence(x: &[f32]) -> f64 {
        let (a, b) = (x.len() / 4, 3 * x.len() / 4);
        let mut passages = Vec::new();
        for i in a + 1..b {
            if x[i - 1] <= 0.0 && x[i] > 0.0 {
                passages.push((i - 1) as f64 + (-x[i - 1] / (x[i] - x[i - 1])) as f64);
            }
        }
        let n = passages.len();
        (n - 1) as f64 * SR as f64 / (passages[n - 1] - passages[0])
    }

    /// Le critère d'E6 : |erreur| ≤ 2 cents de 82 Hz à 4 kHz, ≤ 1 à partir
    /// de 220 Hz, à ±1, ±2 et ±5 demi-tons.
    #[test]
    fn la_transposition_est_juste_au_cent() {
        let mut ecarts = Vec::new();
        for hz in [82.4f32, 110.0, 220.0, 441.0, 1000.0, 4000.0] {
            let x: Vec<f32> = sinus(hz, 3.0).iter().map(|v| 0.5 * v).collect();
            for d in [-5.0f32, -2.0, -1.0, 1.0, 2.0, 5.0] {
                let y = transposer(&x, 1, d);
                assert_eq!(y.len(), x.len(), "la durée a bougé");
                let attendu = hz as f64 * 2f64.powf(d as f64 / 12.0);
                ecarts.push((hz, d, 1200.0 * (frequence(&y) / attendu).log2()));
            }
        }
        for (hz, _, _) in ecarts.iter().step_by(6) {
            let ligne: Vec<String> = ecarts
                .iter()
                .filter(|e| e.0 == *hz)
                .map(|e| format!("{:+.2}", e.2))
                .collect();
            eprintln!("{hz} Hz (−5 −2 −1 +1 +2 +5) : {}", ligne.join(" "));
        }
        let hors = |e: &&(f32, f32, f64)| e.2.abs() > if e.0 < 220.0 { 2.0 } else { 1.0 };
        let fautes: Vec<_> = ecarts.iter().filter(hors).collect();
        assert!(fautes.is_empty(), "{fautes:?}");
    }

    /// La latence du vocodeur est compensée : une note qui commence à 1 s
    /// commence encore à 1 s une fois transposée, sinon le stem se décale
    /// des autres.
    #[test]
    fn la_transposition_ne_decale_pas() {
        let mut x = vec![0.0f32; SR as usize];
        x.extend(sinus(440.0, 1.0).iter().map(|v| 0.5 * v));
        x.extend(vec![0.0f32; SR as usize]);
        let y = transposer(&x, 1, 2.0);
        let attaque = |s: &[f32]| s.iter().position(|v| v.abs() > 0.25).unwrap_or(0);
        let ecart = attaque(&y) as f64 - attaque(&x) as f64;
        let ms = ecart * 1000.0 / SR as f64;
        assert!(ms.abs() < 10.0, "attaque décalée de {ms:.1} ms");
    }

    /// Stéréo transposé : même longueur, et chaque canal garde sa note.
    #[test]
    fn la_transposition_garde_le_stereo() {
        let g = sinus(440.0, 2.0);
        let d = sinus(660.0, 2.0);
        let entrelace: Vec<f32> = g.iter().zip(&d).flat_map(|(a, b)| [*a, *b]).collect();
        let out = transposer(&entrelace, 2, 12.0);
        assert_eq!(out.len(), entrelace.len());
        let canaux = separer(&out, 2);
        assert!((dominante(&canaux[0]) - 880.0).abs() < 12.0);
        assert!((dominante(&canaux[1]) - 1320.0).abs() < 12.0);
    }

    /// Trop court pour `exact` : l'ancienne voie prend le relais.
    #[test]
    fn un_signal_court_se_transpose_quand_meme() {
        let s = sinus(440.0, 0.1);
        let out = transposer(&s, 1, 2.0);
        assert!(!out.is_empty());
    }
}
