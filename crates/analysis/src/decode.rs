//! Décodage d'un fichier vers le format qu'attend le frontal log-mel.
//!
//! Le modèle veut du mono à 48 kHz ; la bibliothèque est en 44,1 kHz stéréo
//! pour l'essentiel. `rodio` sait rééchantillonner et remixer à la volée, ce
//! qui évite d'ajouter un rééchantillonneur au projet.
//!
//! **Le fichier est lu d'un bloc, puis décodé fenêtre par fenêtre depuis la
//! mémoire.** L'encodeur HTSAT prend dix secondes ; cinq fenêtres font
//! cinquante secondes d'un morceau qui en dure deux cent cinquante — 20 % de
//! l'audio. Ne décoder que celles-là rend la chaîne 7,3 × moins chère.
//!
//! Trois stratégies ont été mesurées sur la vraie bibliothèque, et le
//! classement n'est pas celui qu'on attendrait :
//!
//! | stratégie | s/morceau, 12 travailleurs, carte SD |
//! |---|---|
//! | tout décoder, en flux | 1,08 |
//! | **lire d'un bloc, positionner en mémoire** | **1,13** |
//! | positionner dans le fichier | 1,42 |
//!
//! Se positionner *dans le fichier* lit pourtant cinq fois moins d'octets — et
//! c'est le plus lent. Ce support facture l'accès, pas l'octet : sous douze
//! travailleurs concurrents, l'accès dispersé est exactement ce qu'il sert le
//! plus mal. La lecture d'un bloc est le seul motif qu'il honore à son débit,
//! et elle conserve l'économie de décodage — d'où ce choix, qui gagne aussi
//! sur un support rapide, où seul le décodage évité compte.
//!
//! **Le vrai plafond n'est pas là.** La carte donne 7,4 Mo/s en séquentiel, au
//! repos ; les 212 Go de la bibliothèque prennent donc ~8 h à traverser, quoi
//! qu'on décode ensuite. Aucune de ces stratégies ne change cet ordre de
//! grandeur.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::source::UniformSourceIterator;
use rodio::{Decoder, Source};

use crate::mel::{FENETRE_N, SR};

/// Nombre de fenêtres par morceau retenu pour la passe.
///
/// Cinq, et pas trois ni neuf : mesuré par l'exemple `couverture` sur 1 743
/// morceaux, jugé contre l'album et l'artiste. Le rapport au hasard passe de
/// 0,59 (une fenêtre) à 0,53 (trois) puis 0,51 (cinq), et **ne bouge plus** à
/// neuf. Cinq attrape donc tout le gain disponible pour la moitié du coût.
pub const FENETRES: usize = 5;

/// Les fenêtres restent entre ces bornes du morceau : ni l'intro ni la fin,
/// pour éviter les fondus et les silences de bord.
const DEBUT: f64 = 0.15;
const FIN: f64 = 0.85;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ouverture impossible de {path} : {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("format non décodable pour {path} : {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: rodio::decoder::DecoderError,
    },
    #[error(transparent)]
    Opus(#[from] rusty_music_core::Error),
}

/// Positions relatives des `n` fenêtres dans la plage utile.
///
/// Une seule fenêtre se prend au centre : le milieu d'un morceau est plus
/// représentatif que son quinzième.
pub fn fractions(n: usize) -> Vec<f64> {
    match n {
        0 | 1 => vec![0.5],
        n => (0..n)
            .map(|i| DEBUT + (FIN - DEBUT) * i as f64 / (n - 1) as f64)
            .collect(),
    }
}

/// Décodeur alimenté depuis la mémoire, et non au fil des besoins du disque.
type Lecteur = Decoder<std::io::Cursor<Vec<u8>>>;

/// Lit le fichier d'un seul tenant, puis ouvre un décodeur dessus.
///
/// Un morceau pèse 8 Mo en moyenne ; douze travailleurs en tiennent une
/// centaine de mégaoctets, ce qui ne se discute pas. Ce qui se discutait,
/// c'était de laisser le décodeur réclamer ses octets au support au fil de ses
/// positionnements — mesuré 26 % plus lent sur la carte.
fn ouvrir(path: &Path) -> Result<Lecteur, Error> {
    let octets = std::fs::read(path).map_err(|source| Error::Open {
        path: path.to_path_buf(),
        source,
    })?;
    Decoder::try_from(std::io::Cursor::new(octets)).map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })
}

fn uniforme(d: Lecteur) -> UniformSourceIterator<Lecteur> {
    UniformSourceIterator::new(
        d,
        1.try_into().expect("1 canal"),
        SR.try_into().expect("48 kHz"),
    )
}

/// Décode un fichier et en extrait `n` fenêtres à analyser.
///
/// Renvoie jusqu'à `n` blocs de [`FENETRE_N`] échantillons, mono à 48 kHz. Un
/// morceau plus court qu'une fenêtre est rendu tel quel — le frontal le
/// répétera.
pub fn fenetres(path: &Path, n: usize) -> Result<Vec<Vec<f32>>, Error> {
    // Opus ne passe pas par rodio : la voie rapide n'a rien à ouvrir.
    if est_opus(path) {
        return fenetres_integrales(path, n);
    }
    let fractions = fractions(n);

    // Voie rapide. Elle échoue proprement (`None`) sur un fichier dont la
    // durée n'est pas annoncée ou qui refuse le positionnement : le repli
    // n'est pas une précaution théorique, un MPEG sans en-tête Xing ne sait
    // pas dire où il va.
    if let Some(blocs) = par_position(path, &fractions)? {
        return Ok(blocs);
    }
    fenetres_integrales(path, n)
}

fn est_opus(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("opus"))
}

/// Le morceau entier, mono à [`SR`].
///
/// **Opus passe par le cœur, tout le reste par rodio.** symphonia ne décode pas
/// Opus, et un album entier de la bibliothèque de test restait hors de la
/// carte ; `rusty_music_core::opus` comble ce seul trou. Il rend déjà du 48 kHz
/// — la fréquence du modèle — donc rien à rééchantillonner, seulement à
/// ramener les canaux à un.
fn mono_complet(path: &Path) -> Result<Vec<f32>, Error> {
    if !est_opus(path) {
        return Ok(uniforme(ouvrir(path)?).collect());
    }
    let piste = rusty_music_core::opus::decoder(path)?;
    if piste.canaux <= 1 {
        return Ok(piste.echantillons);
    }
    Ok(piste
        .echantillons
        .chunks_exact(piste.canaux)
        .map(|c| c.iter().sum::<f32>() / piste.canaux as f32)
        .collect())
}

/// Se place sur chaque fenêtre au lieu de lire le fichier entier.
///
/// Un seul décodeur pour toutes les fenêtres, et des positions croissantes :
/// rouvrir le fichier à chaque fenêtre coûterait trois ouvertures là où une
/// suffit, et une ouverture se paie ~100 ms sur la carte SD.
fn par_position(path: &Path, fractions: &[f64]) -> Result<Option<Vec<Vec<f32>>>, Error> {
    let decodeur = ouvrir(path)?;
    let Some(duree) = decodeur.total_duration() else {
        return Ok(None);
    };
    let fenetre = FENETRE_N as f64 / SR as f64;
    let utile = duree.as_secs_f64() - fenetre;
    // Morceau plus court qu'une fenêtre : rien à découper, le repli le rendra
    // tel quel pour que le frontal le répète.
    if utile <= 0.0 {
        return Ok(None);
    }

    let mut src = uniforme(decodeur);
    let mut blocs = Vec::with_capacity(fractions.len());
    for f in fractions {
        if src.try_seek(Duration::from_secs_f64(utile * f)).is_err() {
            return Ok(None);
        }
        let bloc: Vec<f32> = src.by_ref().take(FENETRE_N).collect();
        // Une fenêtre tronquée fausserait le spectrogramme : on préfère le
        // repli, qui sait exactement où finit le morceau.
        if bloc.len() < FENETRE_N {
            return Ok(None);
        }
        blocs.push(bloc);
    }
    Ok(Some(blocs))
}

/// Repli : tout décoder, puis découper. Le comportement d'origine.
///
/// Reste publique pour que `cout_decodage` chiffre ce que le positionnement
/// évite : une optimisation qu'on ne sait plus mesurer est une optimisation
/// qu'on ne saura pas défendre.
pub fn fenetres_integrales(path: &Path, n: usize) -> Result<Vec<Vec<f32>>, Error> {
    let fractions = fractions(n);
    let mono = mono_complet(path)?;
    if mono.len() <= FENETRE_N {
        return Ok(vec![mono]);
    }
    Ok(fractions
        .iter()
        .map(|f| {
            let d = ((mono.len() - FENETRE_N) as f64 * f) as usize;
            mono[d..d + FENETRE_N].to_vec()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_fenetres_sont_reparties_dans_la_plage_utile() {
        assert_eq!(fractions(0), vec![0.5], "aucune fenêtre demandée");
        assert_eq!(fractions(1), vec![0.5], "une seule : au centre");
        assert_eq!(fractions(3), vec![DEBUT, 0.5, FIN]);

        let neuf = fractions(9);
        assert_eq!(neuf.len(), 9);
        assert_eq!(neuf.first(), Some(&DEBUT));
        assert_eq!(neuf.last(), Some(&FIN));
        // Régulièrement espacées, et jamais hors de la plage utile.
        for f in neuf.windows(2) {
            assert!((f[1] - f[0] - (FIN - DEBUT) / 8.0).abs() < 1e-9, "{neuf:?}");
        }
        assert!(neuf.iter().all(|f| (DEBUT..=FIN).contains(f)));
    }
}
