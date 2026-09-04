// SPDX-License-Identifier: GPL-3.0-or-later
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
use tracing::warn;

use crate::mel::{FENETRE_N, SR};

/// Taille de fichier au-delà de laquelle on refuse de le charger en mémoire.
///
/// Une piste ne pèse jamais ça : c'est le signe d'un fichier corrompu, ou
/// d'autre chose que de la musique rangé sous une extension audio (voir
/// `AUDIO_EXTS`). Sans ce plafond, `std::fs::read` en charge l'intégralité
/// avant même que le décodeur ne se prononce.
const TAILLE_MAX: u64 = 1_000_000_000;

/// Échantillons au-delà desquels le repli "tout décoder" s'arrête, même si le
/// flux continue d'en produire.
///
/// Quatre heures à [`SR`], largement au-dessus de tout morceau réel (le plus
/// long ne dépasse pas l'heure). Un flux dégénéré — en-tête corrompu que le
/// rééchantillonneur interprète de travers — peut sinon produire un nombre
/// d'échantillons sans rapport avec la durée réelle du fichier et épuiser la
/// mémoire avant qu'on s'en aperçoive.
const ECHANTILLONS_MAX: usize = SR as usize * 4 * 3600;

/// Pause imposée après un délai dépassé qui a survécu à toutes ses tentatives.
///
/// Rencontré en pratique — à deux reprises — sous la forme d'une véritable
/// panique noyau (`pcie-sdreader`, timeout de complétion PCIe) plutôt que
/// d'une simple erreur applicative : le contrôleur montrait déjà des signes
/// de détresse avant l'échec final. Enchainer aussitôt sur le fichier suivant
/// le sollicite à nouveau au pire moment ; cette pause lui laisse le temps de
/// se stabiliser avant la prochaine lecture.
const REPOS_APRES_TIMEOUT: Duration = Duration::from_secs(10);

/// Temps maximal accordé à une tentative de lecture avant de l'abandonner.
///
/// Rencontré en pratique : `std::fs::read` peut rester bloqué **deux heures**
/// sans jamais renvoyer d'erreur, jusqu'à ce que le noyau lui-même panique. Ce
/// délai n'est donc pas un raffinement — sans lui, aucune reprise ni aucune
/// pause n'a jamais la main : l'appel ne revient tout simplement pas.
const DELAI_LECTURE: Duration = Duration::from_secs(45);

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
    let octets = lire_borne(path)?;
    Decoder::try_from(std::io::Cursor::new(octets)).map_err(|source| Error::Decode {
        path: path.to_path_buf(),
        source,
    })
}

/// Vérifie la taille avant de charger, puis lit — avec reprise sur un support
/// qui s'est fait attendre.
///
/// Une carte SD ou un partage réseau sous forte charge concurrente peut
/// renvoyer un délai dépassé (`ETIMEDOUT`) sans que le fichier soit en cause :
/// deux nouvelles tentatives, espacées, suffisent le plus souvent à passer un
/// engorgement passager plutôt que de compter le morceau en échec pour rien.
fn lire_borne(path: &Path) -> Result<Vec<u8>, Error> {
    let taille = std::fs::metadata(path)
        .map_err(|source| Error::Open {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if taille > TAILLE_MAX {
        return Err(Error::Open {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{taille} octets, plafond {TAILLE_MAX}"),
            ),
        });
    }

    const TENTATIVES: u32 = 3;
    for tentative in 0..TENTATIVES {
        match lire_avec_delai(path, DELAI_LECTURE) {
            Ok(octets) => return Ok(octets),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut && tentative + 1 < TENTATIVES => {
                warn!(path = %path.display(), tentative, "lecture en délai dépassé, nouvel essai");
                std::thread::sleep(Duration::from_millis(300 * (tentative as u64 + 1)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                warn!(
                    path = %path.display(),
                    "délai dépassé à bout de tentatives, pause avant de continuer"
                );
                std::thread::sleep(REPOS_APRES_TIMEOUT);
                return Err(Error::Open {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
            Err(source) => {
                return Err(Error::Open {
                    path: path.to_path_buf(),
                    source,
                })
            }
        }
    }
    unreachable!("la boucle rend toujours avant d'épuiser ses tentatives")
}

/// Lit `path`, sans jamais attendre `std::fs::read` plus que [`DELAI_LECTURE`].
///
/// Rencontré en pratique : un support en détresse peut laisser l'appel bloqué
/// des **heures** sans jamais renvoyer d'erreur — le noyau lui-même finit par
/// paniquer plutôt que de rendre la main. Aucune reprise ne peut aider si
/// l'appel ne revient jamais ; la lecture part donc sur un fil à part, et on
/// ne l'attend qu'un temps fixe. Le fil oublié au-delà du délai continue
/// d'exister, bloqué dans le noyau comme il l'aurait été de toute façon — mais
/// il ne bloque plus, lui, l'avancement de la passe.
fn lire_avec_delai(path: &Path, delai: Duration) -> std::io::Result<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let chemin = path.to_path_buf();
    std::thread::spawn(move || {
        // Le récepteur peut être parti (délai déjà expiré côté appelant) :
        // l'envoi échoue alors silencieusement, ce qui est très bien.
        let _ = tx.send(std::fs::read(&chemin));
    });
    rx.recv_timeout(delai)
        .unwrap_or_else(|_| Err(std::io::Error::from(std::io::ErrorKind::TimedOut)))
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
        // `+1` pour distinguer un flux qui s'arrête pile au plafond de celui
        // qui continuait — seul ce dernier mérite l'avertissement.
        let echantillons: Vec<f32> = uniforme(ouvrir(path)?).take(ECHANTILLONS_MAX + 1).collect();
        return Ok(tronquer_si_demesure(echantillons, path));
    }
    let piste = rusty_music_core::opus::decoder(path)?;
    let mono = if piste.canaux <= 1 {
        piste.echantillons
    } else {
        piste
            .echantillons
            .chunks_exact(piste.canaux)
            .map(|c| c.iter().sum::<f32>() / piste.canaux as f32)
            .collect()
    };
    Ok(tronquer_si_demesure(mono, path))
}

/// Coupe à [`ECHANTILLONS_MAX`] un flux qui le dépasserait, en le signalant :
/// un morceau réel ne l'atteint jamais, seul un flux mal décodé le peut.
fn tronquer_si_demesure(mut echantillons: Vec<f32>, path: &Path) -> Vec<f32> {
    if echantillons.len() > ECHANTILLONS_MAX {
        warn!(
            path = %path.display(),
            echantillons = echantillons.len(),
            "flux anormalement long, tronqué à 4 h"
        );
        echantillons.truncate(ECHANTILLONS_MAX);
    }
    echantillons
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
