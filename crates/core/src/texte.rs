// SPDX-License-Identifier: GPL-3.0-or-later
//! Rapprochement de textes : ce que la base et l'enrichissement partagent pour
//! reconnaître un titre d'album d'une source à l'autre.

/// Réduit un titre d'album à ce qui permet de le reconnaître.
///
/// Nos fichiers ne portent pas d'identifiant d'album : le rapprochement se
/// fait par le titre, et les titres divergent. « In Utero (super deluxe) » chez
/// nous, « In Utero » chez MusicBrainz ; « Songs of Freedom » contre « Songs Of
/// Freedom ». On retire donc les parenthèses et crochets de fin — qui portent
/// presque toujours une mention d'édition — les accents, la ponctuation et la
/// casse.
pub fn normaliser_titre(titre: &str) -> String {
    let mut s = titre.trim();
    // Répété : « Album (deluxe) [remaster] » porte deux mentions.
    loop {
        let coupe = match s.chars().last() {
            Some(')') => s.rfind('('),
            Some(']') => s.rfind('['),
            _ => None,
        };
        match coupe {
            Some(i) if i > 0 => s = s[..i].trim(),
            _ => break,
        }
    }
    s.chars()
        .filter_map(|c| {
            let c = sans_accent(c);
            c.is_alphanumeric()
                .then(|| c.to_lowercase().next().unwrap_or(c))
        })
        .collect()
}

/// Rabat les lettres accentuées les plus courantes sur leur base.
///
/// Écrit à la main plutôt qu'avec une bibliothèque de normalisation Unicode :
/// c'est vingt lignes contre une dépendance, et le besoin se limite aux
/// alphabets latins que portent des titres d'albums.
pub fn sans_accent(c: char) -> char {
    match c {
        'à'..='å' | 'À'..='Å' => 'a',
        'è'..='ë' | 'È'..='Ë' => 'e',
        'ì'..='ï' | 'Ì'..='Ï' => 'i',
        'ò'..='ö' | 'Ò'..='Ö' => 'o',
        'ù'..='ü' | 'Ù'..='Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        'ý' | 'ÿ' | 'Ý' => 'y',
        autre => autre,
    }
}
