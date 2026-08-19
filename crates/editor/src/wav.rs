//! Écriture des stems en WAV.
//!
//! Écrit à la main plutôt qu'avec une bibliothèque : un en-tête RIFF PCM tient
//! en une quarantaine de lignes, et le projet n'ajoute pas de dépendance pour
//! quarante lignes.
//!
//! Format retenu : **PCM 16 bits**. Les stems sortent du modèle en `f32`, mais
//! ils sont destinés à être écoutés et rechargés dans un éditeur ; le 16 bits
//! divise le poids par deux et se lit partout. Un stem de quatre minutes pèse
//! ainsi 42 Mo au lieu de 84.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("écriture impossible de {path} : {source}")]
    Ecriture {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("canaux de longueurs différentes : {gauche} et {droite}")]
    Desaccord { gauche: usize, droite: usize },
}

/// Écrit un WAV stéréo PCM 16 bits.
pub fn ecrire(path: &Path, gauche: &[f32], droite: &[f32], sr: u32) -> Result<(), Error> {
    if gauche.len() != droite.len() {
        return Err(Error::Desaccord {
            gauche: gauche.len(),
            droite: droite.len(),
        });
    }
    let faute = |source| Error::Ecriture {
        path: path.to_path_buf(),
        source,
    };

    let n = gauche.len();
    let octets_donnees = (n * 2 * 2) as u32; // n trames × 2 canaux × 2 octets
    let mut f = BufWriter::new(std::fs::File::create(path).map_err(faute)?);

    // En-tête RIFF/WAVE, tout en petit-boutiste.
    let mut ecrire_tout = |bloc: &[u8]| -> Result<(), Error> { f.write_all(bloc).map_err(faute) };
    ecrire_tout(b"RIFF")?;
    ecrire_tout(&(36 + octets_donnees).to_le_bytes())?;
    ecrire_tout(b"WAVE")?;
    ecrire_tout(b"fmt ")?;
    ecrire_tout(&16u32.to_le_bytes())?; // taille du bloc fmt
    ecrire_tout(&1u16.to_le_bytes())?; // 1 = PCM entier
    ecrire_tout(&2u16.to_le_bytes())?; // canaux
    ecrire_tout(&sr.to_le_bytes())?;
    ecrire_tout(&(sr * 4).to_le_bytes())?; // octets par seconde
    ecrire_tout(&4u16.to_le_bytes())?; // alignement de trame
    ecrire_tout(&16u16.to_le_bytes())?; // bits par échantillon
    ecrire_tout(b"data")?;
    ecrire_tout(&octets_donnees.to_le_bytes())?;

    // Écrêtage explicite : un stem peut dépasser 1,0 là où le mélange
    // d'origine ne le faisait pas, et un dépassement silencieux s'entend.
    let mut tampon = Vec::with_capacity(n * 4);
    for (g, d) in gauche.iter().zip(droite) {
        for v in [*g, *d] {
            let e = (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            tampon.extend_from_slice(&e.to_le_bytes());
        }
    }
    f.write_all(&tampon).map_err(faute)?;
    f.flush().map_err(faute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn len_tete_et_donnees_concordent() {
        let dossier = std::env::temp_dir().join(format!("rm-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let f = dossier.join("essai.wav");

        let g: Vec<f32> = (0..1000).map(|i| (i as f32 / 100.0).sin()).collect();
        ecrire(&f, &g, &g, 44_100).unwrap();

        let octets = std::fs::read(&f).unwrap();
        assert_eq!(&octets[0..4], b"RIFF");
        assert_eq!(&octets[8..12], b"WAVE");
        // 44 octets d'en-tête + 1000 trames × 2 canaux × 2 octets.
        assert_eq!(octets.len(), 44 + 4000);
        let annonce = u32::from_le_bytes(octets[4..8].try_into().unwrap());
        assert_eq!(annonce as usize, octets.len() - 8, "taille RIFF");
        let donnees = u32::from_le_bytes(octets[40..44].try_into().unwrap());
        assert_eq!(donnees as usize, octets.len() - 44, "taille du bloc data");

        std::fs::remove_dir_all(&dossier).unwrap();
    }

    #[test]
    fn ecrete_au_lieu_de_replier() {
        let dossier = std::env::temp_dir().join(format!("rm-wav-c-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let f = dossier.join("fort.wav");

        // Un stem peut dépasser 1,0 : sans écrêtage, la conversion replierait
        // le signal et le craquement s'entendrait.
        ecrire(&f, &[3.0, -3.0], &[3.0, -3.0], 44_100).unwrap();
        let o = std::fs::read(&f).unwrap();
        let premier = i16::from_le_bytes(o[44..46].try_into().unwrap());
        let second = i16::from_le_bytes(o[48..50].try_into().unwrap());
        assert_eq!(premier, i16::MAX);
        assert_eq!(second, -i16::MAX);

        std::fs::remove_dir_all(&dossier).unwrap();
    }

    #[test]
    fn refuse_des_canaux_desaccordes() {
        let f = std::env::temp_dir().join("jamais-ecrit.wav");
        assert!(matches!(
            ecrire(&f, &[0.0; 3], &[0.0; 4], 44_100),
            Err(Error::Desaccord { .. })
        ));
    }
}
