// SPDX-License-Identifier: GPL-3.0-or-later
//! Où trouver les poids des modèles, selon d'où on tourne.
//!
//! Deux exécutions n'ont pas la même idée de « à côté » :
//!
//! - **en développement**, on lance depuis la racine du dépôt, et les poids
//!   sont dans `models/` — un chemin relatif au dossier courant suffit ;
//! - **dans une application empaquetée**, le dossier courant est `/` quand on
//!   double-clique depuis le Finder. Un chemin relatif ne désigne alors rien,
//!   et un chemin absolu figé à la compilation désigne une machine qui n'est
//!   pas celle de l'utilisateur.
//!
//! Ce module cherche donc dans un ordre qui couvre les deux, et rend le
//! premier candidat qui existe. Il ne devine jamais : si rien n'est trouvé,
//! l'appelant reçoit `None` et peut dire précisément ce qui manque.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::{Error, Result};

/// Variable d'environnement qui l'emporte sur tout le reste.
///
/// Sert à faire tourner une application installée sur des poids rangés
/// ailleurs, sans la reconstruire.
pub const VARIABLE: &str = "RUSTY_MUSIC_MODELS";

/// Le dossier des poids **téléchargés par l'application** — sous ses données
/// utilisateur, seul endroit où une application empaquetée peut écrire. Posé une
/// fois au démarrage par l'application ; absent en ligne de commande.
static DOSSIER_UTILISATEUR: OnceLock<PathBuf> = OnceLock::new();

/// Désigne le dossier où ranger les poids qu'on télécharge. Sans effet au
/// second appel : il ne change pas en cours de route.
pub fn definir_dossier_utilisateur(dossier: PathBuf) {
    let _ = DOSSIER_UTILISATEUR.set(dossier);
}

/// Où écrire un poids téléchargé : le dossier forcé par [`VARIABLE`], sinon
/// celui de l'application, sinon `models/` — le dépôt, en ligne de commande, là
/// où `scripts/preparer-*.sh` les range aussi.
pub fn dossier_telechargement() -> PathBuf {
    if let Some(force) = std::env::var_os(VARIABLE) {
        return PathBuf::from(force);
    }
    DOSSIER_UTILISATEUR
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("models"))
}

/// Les dossiers où chercher, dans l'ordre de priorité.
pub fn dossiers() -> Vec<PathBuf> {
    let mut candidats = Vec::new();

    if let Some(force) = std::env::var_os(VARIABLE) {
        candidats.push(PathBuf::from(force));
    }

    if let Some(utilisateur) = DOSSIER_UTILISATEUR.get() {
        candidats.push(utilisateur.clone());
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Disposition d'un paquet macOS : `Rusty Music.app/Contents/MacOS/
            // rusty-music-desktop` a ses ressources dans `../Resources/`.
            candidats.push(dir.join("../Resources/models"));
            // Disposition simple : les poids à côté du binaire.
            candidats.push(dir.join("models"));
        }
    }

    // Développement : lancé depuis la racine du dépôt.
    candidats.push(PathBuf::from("models"));
    candidats
}

/// Cherche un fichier de poids par son nom.
pub fn trouver(nom: &str) -> Option<PathBuf> {
    dossiers()
        .into_iter()
        .map(|d| d.join(nom))
        .find(|p| p.is_file())
}

/// Télécharge un poids dans [`dossier_telechargement`] et rend son chemin.
///
/// En flux vers un fichier `.partiel`, renommé seulement **quand tout est
/// arrivé et vérifié** : un téléchargement interrompu ne laisse jamais un
/// fichier tronqué que la recherche prendrait pour un modèle (`trouver` ne
/// regarde que le nom final). Deux garde-fous sur la taille — celle annoncée
/// par le serveur, et `octets_minimum` quand il n'en annonce pas — car un poids
/// tronqué ne se plaint qu'au chargement, avec un message qui n'en dit pas la
/// cause.
///
/// `sha256_attendu` vérifie en plus l'intégrité du contenu, pas seulement sa
/// taille — même garantie que `scripts/preparer-*.sh` (`shasum -a 256`),
/// jusqu'ici absente de ce chemin de téléchargement à la demande. `None`
/// quand l'empreinte n'est pas connue (voir `editor::Variante::sha256`) :
/// mieux vaut télécharger sans cette garantie que refuser une variante
/// existante faute d'avoir mesuré son empreinte.
///
/// `avancer(octets_reçus, octets_attendus)` est rappelé à chaque lot.
pub fn telecharger(
    nom: &str,
    url: &str,
    octets_minimum: u64,
    sha256_attendu: Option<&str>,
    avancer: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    telecharger_dans(&dossier_telechargement(), nom, url, octets_minimum, sha256_attendu, avancer)
}

/// [`telecharger`] vers un dossier explicite.
pub fn telecharger_dans(
    dossier: &Path,
    nom: &str,
    url: &str,
    octets_minimum: u64,
    sha256_attendu: Option<&str>,
    mut avancer: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    std::fs::create_dir_all(dossier)?;
    let destination = dossier.join(nom);
    let partiel = dossier.join(format!("{nom}.partiel"));

    let (mut recus, mut attendus) = (0u64, None);
    let agent = crate::discogs::agent();
    let issue = crate::discogs::telecharger_avec_avancement(&agent, url, &partiel, |vus, total| {
        (recus, attendus) = (vus, total);
        avancer(vus, total);
    });
    let verifie = issue.and_then(|()| match attendus {
        Some(total) if recus != total => Err(Error::Reseau(format!(
            "téléchargement de {nom} interrompu : {recus} octets sur {total}"
        ))),
        _ if recus < octets_minimum => Err(Error::Reseau(format!(
            "téléchargement de {nom} incomplet : {recus} octets, {octets_minimum} au moins attendus"
        ))),
        _ => Ok(()),
    });
    let verifie = verifie.and_then(|()| match sha256_attendu {
        Some(attendu) => {
            let reel = sha256_fichier(&partiel)?;
            if reel.eq_ignore_ascii_case(attendu) {
                Ok(())
            } else {
                Err(Error::Reseau(format!(
                    "{nom} : empreinte SHA-256 inattendue ({reel}, attendu {attendu})"
                )))
            }
        }
        None => Ok(()),
    });
    if let Err(e) = verifie {
        let _ = std::fs::remove_file(&partiel);
        return Err(e);
    }
    std::fs::rename(&partiel, &destination)?;
    Ok(destination)
}

/// Empreinte SHA-256 de `chemin`, en flux (jamais tout le fichier en
/// mémoire à la fois — les poids pèsent jusqu'à quelques centaines de Mo).
fn sha256_fichier(chemin: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut fichier = std::fs::File::open(chemin)?;
    let mut hasheur = Sha256::new();
    let mut tampon = [0u8; 1024 * 1024];
    loop {
        let n = std::io::Read::read(&mut fichier, &mut tampon)?;
        if n == 0 {
            break;
        }
        hasheur.update(&tampon[..n]);
    }
    Ok(hasheur.finalize().iter().map(|o| format!("{o:02x}")).collect())
}

/// Message d'erreur qui dit où l'on a regardé.
///
/// Un « fichier introuvable » sans la liste des endroits visités oblige à lire
/// le code pour comprendre.
pub fn introuvable(nom: &str) -> String {
    let vus: Vec<String> = dossiers()
        .iter()
        .map(|d| d.join(nom).display().to_string())
        .collect();
    format!(
        "{nom} introuvable. Cherché dans :\n  {}\n\
         Poser {VARIABLE} pour désigner un autre dossier.",
        vus.join("\n  ")
    )
}

/// Le dossier retenu pour un fichier donné, s'il existe.
pub fn dossier_de(nom: &str) -> Option<PathBuf> {
    trouver(nom).and_then(|p| p.parent().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_variable_denvironnement_passe_devant() {
        // On ne modifie pas l'environnement du processus de test — d'autres
        // tests tournent en parallèle. On vérifie l'ordre sur la liste telle
        // qu'elle est construite quand la variable est absente.
        let sans = dossiers();
        assert!(
            sans.last() == Some(&PathBuf::from("models")),
            "le repli de développement doit rester en dernier : {sans:?}"
        );
        assert!(
            sans.iter().any(|d| d.ends_with("Resources/models")),
            "la disposition d'un paquet doit être tentée : {sans:?}"
        );
    }

    #[test]
    fn un_telechargement_impossible_ne_laisse_aucun_fichier() {
        // Adresse qui ne répond pas : l'échec doit être net, et ni le poids ni
        // son `.partiel` ne doivent rester — sinon `trouver` ou une reprise
        // naïve prendrait un débris pour un modèle.
        let dossier = std::env::temp_dir().join("rusty-music-test-modeles");
        let _ = std::fs::remove_dir_all(&dossier);
        let r = telecharger_dans(
            &dossier,
            "essai.safetensors",
            "http://127.0.0.1:9/absent",
            1,
            None,
            |_, _| {},
        );
        assert!(r.is_err());
        assert!(!dossier.join("essai.safetensors").exists());
        assert!(!dossier.join("essai.safetensors.partiel").exists());
    }

    /// Un serveur d'une seule réponse, sur un port libre de la boucle locale.
    fn serveur(reponse: &'static [u8]) -> String {
        use std::io::{Read, Write};
        let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = ecoute.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut flux, _)) = ecoute.accept() {
                let mut requete = [0u8; 2048];
                let _ = flux.read(&mut requete);
                let _ = flux.write_all(reponse);
            }
        });
        format!("http://127.0.0.1:{port}/poids")
    }

    #[test]
    fn un_telechargement_complet_arrive_sous_son_nom_final() {
        let dossier = std::env::temp_dir().join("rusty-music-test-modeles-ok");
        let _ = std::fs::remove_dir_all(&dossier);
        let url = serveur(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        );
        let mut dernier = (0, None);
        let chemin = telecharger_dans(&dossier, "ok.safetensors", &url, 10, None, |v, t| {
            dernier = (v, t)
        })
        .expect("téléchargement");
        assert_eq!(chemin, dossier.join("ok.safetensors"));
        assert_eq!(std::fs::read(&chemin).unwrap(), b"0123456789");
        assert_eq!(dernier, (10, Some(10)), "l'avancement va jusqu'au bout");
        assert!(!dossier.join("ok.safetensors.partiel").exists());
    }

    #[test]
    fn un_telechargement_tronque_est_refuse() {
        let dossier = std::env::temp_dir().join("rusty-music-test-modeles-tronque");
        let _ = std::fs::remove_dir_all(&dossier);
        // Annonce 10 octets, en livre 5 puis ferme.
        let url =
            serveur(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n01234");
        assert!(telecharger_dans(&dossier, "coupe.safetensors", &url, 1, None, |_, _| {}).is_err());
        assert!(
            !dossier.join("coupe.safetensors").exists(),
            "pas de poids tronqué"
        );
        assert!(!dossier.join("coupe.safetensors.partiel").exists());
    }

    #[test]
    fn un_poids_trop_petit_est_refuse_meme_sans_taille_annoncee() {
        let dossier = std::env::temp_dir().join("rusty-music-test-modeles-petit");
        let _ = std::fs::remove_dir_all(&dossier);
        // Page d'erreur servie en 200, sans `Content-Length` : le corps se lit
        // jusqu'à la fermeture.
        let url = serveur(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n<html>oups</html>");
        assert!(
            telecharger_dans(&dossier, "petit.safetensors", &url, 1000, None, |_, _| {}).is_err()
        );
        assert!(!dossier.join("petit.safetensors").exists());
    }

    /// SHA-256 de `0123456789` (10 octets), calculé indépendamment pour ne
    /// pas retester `sha256_fichier` avec lui-même.
    const SHA256_0123456789: &str =
        "84d89877f0d4041efb6bf91a16f0248f2fd573e6af05c19f96bedb9f882f7882";

    #[test]
    fn une_empreinte_correcte_est_acceptee() {
        let dossier = std::env::temp_dir().join("rusty-music-test-modeles-hash-ok");
        let _ = std::fs::remove_dir_all(&dossier);
        let url = serveur(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        );
        let chemin = telecharger_dans(
            &dossier,
            "hash-ok.safetensors",
            &url,
            10,
            Some(SHA256_0123456789),
            |_, _| {},
        )
        .expect("empreinte correcte : le téléchargement doit passer");
        assert_eq!(std::fs::read(&chemin).unwrap(), b"0123456789");
    }

    #[test]
    fn une_empreinte_inattendue_est_refusee_sans_laisser_de_fichier() {
        let dossier = std::env::temp_dir().join("rusty-music-test-modeles-hash-mauvais");
        let _ = std::fs::remove_dir_all(&dossier);
        let url = serveur(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        );
        let r = telecharger_dans(
            &dossier,
            "hash-mauvais.safetensors",
            &url,
            10,
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
            |_, _| {},
        );
        assert!(r.is_err(), "empreinte fausse : le téléchargement doit échouer");
        assert!(!dossier.join("hash-mauvais.safetensors").exists());
        assert!(!dossier.join("hash-mauvais.safetensors.partiel").exists());
    }

    #[test]
    fn le_message_dit_ou_lon_a_cherche() {
        let m = introuvable("essai.bpk");
        assert!(m.contains("essai.bpk"));
        assert!(m.contains(VARIABLE), "le message doit citer l'échappatoire");
        // Autant de chemins listés que de dossiers candidats.
        assert_eq!(m.matches("essai.bpk").count(), dossiers().len() + 1);
    }
}
