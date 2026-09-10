// SPDX-License-Identifier: GPL-3.0-or-later
//! Le vote CLAP-texte : charge la table de vocabulaire précalculée
//! (`vocabulaire/vocabulaire.bin` + `.txt`, produite hors ligne par
//! `experiments/clap-texte/preparer_vocabulaire.py` à partir des genres
//! MusicBrainz réels de la bibliothèque et de leur article Wikipédia — voir
//! `docs/nommage-familles.md`) et vote, par morceau, pour le genre dont la
//! phrase décrit le mieux son empreinte audio.
//!
//! **Aucun modèle à l'exécution.** La tour texte de CLAP (RoBERTa, 501 Mo)
//! n'est embarquée nulle part ici : elle a servi une fois, hors ligne, à
//! produire cette table. Ce module ne fait qu'un produit scalaire contre un
//! tableau `f32` fixe — la même conclusion que l'essai `experiments/
//! clap-texte` : « pour nommer les familles, on n'en a pas besoin à
//! l'exécution ».

use std::path::Path;

/// Une entrée du vocabulaire : le nom du genre MusicBrainz d'origine (sert
/// de libellé de vote, comparable à ceux de MusicBrainz et Last.fm — le
/// vocabulaire est construit *depuis* ces genres) et son empreinte
/// CLAP-texte, normalisée à l'unité.
pub struct Entree {
    pub genre: String,
    pub vecteur: Vec<f32>,
}

pub struct Vocabulaire {
    pub entrees: Vec<Entree>,
}

const DIM: usize = 512;

/// Charge la table depuis `crates/analysis/vocabulaire/` — `None` si elle
/// n'a pas encore été générée (le script hors ligne a besoin d'Ollama,
/// installé séparément) : le vote CLAP-texte est alors simplement absent, et
/// MusicBrainz et Last.fm restent seuls à voter le nom d'une famille.
pub fn charger() -> Option<Vocabulaire> {
    charger_depuis(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/vocabulaire/vocabulaire.bin")),
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/vocabulaire/vocabulaire.txt")),
    )
}

fn charger_depuis(bin: &Path, txt: &Path) -> Option<Vocabulaire> {
    let bin_data = std::fs::read(bin).ok()?;
    let txt_data = std::fs::read_to_string(txt).ok()?;
    // `genre\tphrase` par ligne, même ordre que la table binaire — seul le
    // genre sert ici, la phrase n'a servi qu'à produire l'empreinte.
    let genres: Vec<&str> = txt_data
        .lines()
        .map(|l| l.split('\t').next().unwrap_or(l))
        .collect();

    if bin_data.len() % (DIM * 4) != 0 {
        return None;
    }
    let n = bin_data.len() / (DIM * 4);
    if n == 0 || n != genres.len() {
        return None;
    }

    let mut entrees = Vec::with_capacity(n);
    for (i, genre) in genres.into_iter().enumerate() {
        let mut v = Vec::with_capacity(DIM);
        for d in 0..DIM {
            let o = (i * DIM + d) * 4;
            v.push(f32::from_le_bytes([
                bin_data[o],
                bin_data[o + 1],
                bin_data[o + 2],
                bin_data[o + 3],
            ]));
        }
        entrees.push(Entree {
            genre: genre.to_string(),
            vecteur: normaliser(&v),
        });
    }
    Some(Vocabulaire { entrees })
}

fn normaliser(v: &[f32]) -> Vec<f32> {
    let norme = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norme == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norme).collect()
}

/// Le genre gagnant, par morceau — `None` quand aucune phrase ne se détache
/// (score centré nul ou négatif partout : aucune n'est plus proche que la
/// moyenne du vocabulaire pour ce morceau).
///
/// **Calcul en lot, pas morceau par morceau.** Le calibrage a besoin du
/// score de tous les morceaux contre toute la table avant de centrer chaque
/// colonne (par phrase) — l'essai `experiments/clap-texte` l'a mesuré sur
/// Metallica : sans centrage, une poignée de phrases rafle toutes les
/// familles ; en centrant, mais en réduisant aussi par l'écart-type, le
/// résultat sur-corrige. Le centrage seul est le calibrage retenu.
pub fn voter(vecteurs: &[Vec<f32>], vocab: &Vocabulaire) -> Vec<Option<String>> {
    let n = vecteurs.len();
    let v = vocab.entrees.len();
    if n == 0 || v == 0 {
        return vec![None; n];
    }

    // Score brut : cosinus — les deux côtés sont normalisés à l'unité.
    let mut scores = vec![0f32; n * v];
    for (i, piste) in vecteurs.iter().enumerate() {
        let piste = normaliser(piste);
        for (j, entree) in vocab.entrees.iter().enumerate() {
            let s: f32 = piste.iter().zip(&entree.vecteur).map(|(a, b)| a * b).sum();
            scores[i * v + j] = s;
        }
    }

    for j in 0..v {
        let moyenne: f32 = (0..n).map(|i| scores[i * v + j]).sum::<f32>() / n as f32;
        for i in 0..n {
            scores[i * v + j] -= moyenne;
        }
    }

    (0..n)
        .map(|i| {
            (0..v)
                .map(|j| (j, scores[i * v + j]))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|(_, s)| *s > 0.0)
                .map(|(j, _)| vocab.entrees[j].genre.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(paires: &[(&str, [f32; 2])]) -> Vocabulaire {
        Vocabulaire {
            entrees: paires
                .iter()
                .map(|(g, v)| Entree {
                    genre: g.to_string(),
                    vecteur: normaliser(v),
                })
                .collect(),
        }
    }

    #[test]
    fn vote_pour_la_phrase_la_plus_proche_apres_centrage() {
        let vocab = vocab(&[("Rock", [1.0, 0.0]), ("Jazz", [0.0, 1.0])]);
        let vecteurs = vec![vec![1.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0]];
        let gagnants = voter(&vecteurs, &vocab);
        assert_eq!(gagnants[0].as_deref(), Some("Rock"));
        assert_eq!(gagnants[1].as_deref(), Some("Rock"));
        assert_eq!(gagnants[2].as_deref(), Some("Jazz"));
    }

    #[test]
    fn une_phrase_qui_ecrase_tout_ne_gagne_plus_une_fois_centree() {
        // « Enfant » reste, en score brut, la phrase la plus proche des deux
        // morceaux (son vecteur est plus proche des deux axes) — sans
        // centrage elle raflerait les deux votes, comme « a children's
        // song » raflait trois familles sur douze dans l'essai. Une fois
        // chaque colonne centrée, le premier morceau (aligné sur l'axe de
        // « Rock ») bascule vers « Rock ».
        let vocab = vocab(&[("Rock", [0.9, 0.1]), ("Enfant", [0.95, 0.2])]);
        let vecteurs = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let gagnants = voter(&vecteurs, &vocab);
        assert_eq!(gagnants[0].as_deref(), Some("Rock"));
    }

    #[test]
    fn vocabulaire_absent_ne_bloque_rien() {
        assert!(charger_depuis(Path::new("/nexiste/pas.bin"), Path::new("/nexiste/pas.txt"))
            .is_none());
    }
}
