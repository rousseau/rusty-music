// SPDX-License-Identifier: GPL-3.0-or-later
//! Écran « À propos » (chantier 0.1 de `docs/plan-editer-pratique-creation.md`) :
//! version, licence, et les remerciements des modèles.
//!
//! Le tableau de `MODELES.md` est la seule source : il est lu à la
//! compilation et découpé ici, pour qu'un modèle ajouté au fichier apparaisse
//! dans l'application sans rien écrire de plus.

use serde::Serialize;

const MODELES: &str = include_str!("../../../MODELES.md");

#[derive(Serialize, Debug, PartialEq)]
pub struct Lien {
    pub texte: String,
    pub url: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Modele {
    pub nom: String,
    pub role: String,
    pub article: String,
    pub licence_code: String,
    pub licence_poids: String,
    /// Tous les liens de la ligne (code, poids, conditions), sans doublon.
    pub liens: Vec<Lien>,
}

#[derive(Serialize)]
pub struct APropos {
    pub version: &'static str,
    pub modeles: Vec<Modele>,
}

/// Les liens Markdown `[texte](url)` d'une cellule, dans l'ordre.
fn liens(cellule: &str) -> Vec<Lien> {
    let mut out = Vec::new();
    let mut reste = cellule;
    while let Some(debut) = reste.find('[') {
        let apres = &reste[debut + 1..];
        let Some(fin_texte) = apres.find("](") else {
            break;
        };
        let Some(fin_url) = apres[fin_texte + 2..].find(')') else {
            break;
        };
        let url = &apres[fin_texte + 2..fin_texte + 2 + fin_url];
        if url.starts_with("http") {
            out.push(Lien {
                texte: texte_brut(&apres[..fin_texte]),
                url: url.to_string(),
            });
        }
        reste = &apres[fin_texte + 2 + fin_url..];
    }
    out
}

/// Une cellule sans sa mise en forme : liens réduits à leur texte, ni gras
/// ni italique ni code.
fn texte_brut(cellule: &str) -> String {
    let mut s = String::new();
    let mut reste = cellule;
    while let Some(debut) = reste.find('[') {
        let apres = &reste[debut + 1..];
        let lien = apres
            .find("](")
            .and_then(|ft| apres[ft + 2..].find(')').map(|fu| (ft, ft + 2 + fu + 1)));
        s.push_str(&reste[..debut]);
        match lien {
            Some((fin_texte, fin)) => {
                s.push_str(&apres[..fin_texte]);
                reste = &apres[fin..];
            }
            None => {
                s.push('[');
                reste = apres;
            }
        }
    }
    s.push_str(reste);
    s.replace(['`', '*'], "").trim().to_string()
}

/// Les lignes du tableau des modèles (celles dont le nom est en gras).
fn modeles(texte: &str) -> Vec<Modele> {
    texte
        .lines()
        .filter(|l| l.starts_with("| **"))
        .filter_map(|l| {
            let c: Vec<&str> = l.trim().trim_matches('|').split(" | ").collect();
            if c.len() < 7 {
                return None;
            }
            let mut tous: Vec<Lien> = Vec::new();
            for lien in c.iter().flat_map(|x| liens(x)) {
                if !tous.iter().any(|l| l.url == lien.url) {
                    tous.push(lien);
                }
            }
            Some(Modele {
                nom: texte_brut(c[0]),
                role: texte_brut(c[1]),
                article: texte_brut(c[2]),
                licence_code: texte_brut(c[4]),
                licence_poids: texte_brut(c[5]),
                liens: tous,
            })
        })
        .collect()
}

#[tauri::command]
pub fn a_propos() -> APropos {
    APropos {
        version: env!("CARGO_PKG_VERSION"),
        modeles: modeles(MODELES),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_modele_de_modeles_md_est_lu() {
        let m = modeles(MODELES);
        assert!(m.len() >= 8, "{} modèles", m.len());
        let clap = m.iter().find(|x| x.nom.starts_with("CLAP")).expect("CLAP");
        assert_eq!(clap.licence_poids, "Apache-2.0");
        assert!(clap
            .liens
            .iter()
            .any(|l| l.url == "https://github.com/LAION-AI/CLAP"));
        assert!(m
            .iter()
            .all(|x| !x.nom.contains("**") && !x.role.is_empty()));
    }

    #[test]
    fn la_mise_en_forme_est_retiree() {
        assert_eq!(
            texte_brut("**X** `y` [dépôt](https://a.b/c) (fin)"),
            "X y dépôt (fin)"
        );
        assert_eq!(
            liens("[a](https://x) et [b](https://y)"),
            vec![
                Lien {
                    texte: "a".into(),
                    url: "https://x".into()
                },
                Lien {
                    texte: "b".into(),
                    url: "https://y".into()
                },
            ]
        );
    }
}
