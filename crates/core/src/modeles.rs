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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

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
    telecharger_dans(
        &dossier_telechargement(),
        nom,
        url,
        octets_minimum,
        sha256_attendu,
        avancer,
    )
}

/// [`telecharger`] vers un dossier explicite.
pub fn telecharger_dans(
    dossier: &Path,
    nom: &str,
    url: &str,
    octets_minimum: u64,
    sha256_attendu: Option<&str>,
    avancer: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    telecharger_dans_avec_jeton(
        dossier,
        nom,
        url,
        None,
        octets_minimum,
        sha256_attendu,
        avancer,
    )
}

/// [`telecharger`] d'un dépôt à accès restreint : `jeton` est envoyé au
/// serveur, jamais écrit nulle part.
pub fn telecharger_avec_jeton(
    nom: &str,
    url: &str,
    jeton: Option<&str>,
    octets_minimum: u64,
    sha256_attendu: Option<&str>,
    avancer: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    telecharger_dans_avec_jeton(
        &dossier_telechargement(),
        nom,
        url,
        jeton,
        octets_minimum,
        sha256_attendu,
        avancer,
    )
}

fn telecharger_dans_avec_jeton(
    dossier: &Path,
    nom: &str,
    url: &str,
    jeton: Option<&str>,
    octets_minimum: u64,
    sha256_attendu: Option<&str>,
    mut avancer: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    std::fs::create_dir_all(dossier)?;
    let destination = dossier.join(nom);

    // Le même fichier déjà en route (le téléchargement du premier lancement,
    // et l'utilisateur qui le demande aussi depuis les réglages, ou une passe
    // qui en a besoin) : on attend celui-là plutôt que d'écrire à deux dans le
    // même `.partiel`.
    let _vol = match Vol::prendre(&destination) {
        Ok(vol) => vol,
        Err(autre) => {
            autre.attendre();
            return if destination.is_file() {
                Ok(destination)
            } else {
                Err(Error::Reseau(format!(
                    "le téléchargement de {nom} déjà en cours a échoué"
                )))
            };
        }
    };
    let partiel = dossier.join(format!("{nom}.partiel"));

    let (mut recus, mut attendus) = (0u64, None);
    let agent = crate::telechargement::agent();
    let issue = crate::telechargement::telecharger_avec_jeton(
        &agent,
        url,
        jeton,
        &partiel,
        |vus, total| {
            (recus, attendus) = (vus, total);
            avancer(vus, total);
        },
    );
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

/// Les téléchargements en cours, par chemin de destination.
static EN_VOL: OnceLock<Mutex<HashMap<PathBuf, Arc<Signal>>>> = OnceLock::new();

fn en_vol() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Arc<Signal>>> {
    EN_VOL
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Levé quand un téléchargement se termine, réussi ou non.
#[derive(Default)]
struct Signal {
    fini: Mutex<bool>,
    cv: Condvar,
}

impl Signal {
    fn attendre(&self) {
        let mut fini = self.fini.lock().unwrap_or_else(|e| e.into_inner());
        while !*fini {
            fini = self.cv.wait(fini).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// Un téléchargement inscrit dans [`EN_VOL`] : retiré et signalé à sa
/// destruction, panique comprise.
struct Vol {
    destination: PathBuf,
    signal: Arc<Signal>,
}

impl Vol {
    /// Inscrit le téléchargement, ou rend celui qui est déjà en cours.
    fn prendre(destination: &Path) -> std::result::Result<Vol, Arc<Signal>> {
        let mut carte = en_vol();
        if let Some(autre) = carte.get(destination) {
            return Err(autre.clone());
        }
        let signal = Arc::new(Signal::default());
        carte.insert(destination.to_path_buf(), signal.clone());
        Ok(Vol {
            destination: destination.to_path_buf(),
            signal,
        })
    }
}

impl Drop for Vol {
    fn drop(&mut self) {
        en_vol().remove(&self.destination);
        *self.signal.fini.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.signal.cv.notify_all();
    }
}

/// Vrai si `nom` est en train d'arriver.
pub fn en_cours(nom: &str) -> bool {
    en_vol()
        .keys()
        .any(|d| d.file_name().is_some_and(|n| n == nom))
}

/// [`trouver`], en attendant d'abord la fin d'un téléchargement de `nom` en
/// cours : une passe lancée pendant le premier téléchargement des poids
/// (l'analyse d'une bibliothèque toute neuve) les attend au lieu d'échouer.
pub fn trouver_ou_attendre(nom: &str) -> Option<PathBuf> {
    let signaux: Vec<Arc<Signal>> = en_vol()
        .iter()
        .filter(|(d, _)| d.file_name().is_some_and(|n| n == nom))
        .map(|(_, s)| s.clone())
        .collect();
    for s in signaux {
        s.attendre();
    }
    trouver(nom)
}

/// Où se trouve `chemin` par rapport à l'application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Dans le dossier de l'application : téléchargé, peut être supprimé.
    Telecharge,
    /// Livré dans le paquet (`Resources/models`), en lecture seule.
    Paquet,
    /// Ailleurs : `models/` du dépôt, ou le dossier de [`VARIABLE`].
    Autre,
}

/// La provenance d'un poids trouvé par [`trouver`].
pub fn provenance(chemin: &Path) -> Provenance {
    if DOSSIER_UTILISATEUR
        .get()
        .is_some_and(|d| chemin.parent() == Some(d.as_path()))
    {
        Provenance::Telecharge
    } else if chemin
        .parent()
        .is_some_and(|d| d.ends_with("Resources/models"))
    {
        Provenance::Paquet
    } else {
        Provenance::Autre
    }
}

/// Supprime un poids téléchargé par l'application. Refuse ce qui est livré
/// dans le paquet ou rangé ailleurs (le dépôt) : ce n'est pas à l'application
/// d'y toucher. Rend le nombre d'octets libérés.
pub fn supprimer(nom: &str) -> Result<u64> {
    if en_cours(nom) {
        return Err(Error::Reseau(format!(
            "{nom} est en cours de téléchargement"
        )));
    }
    let Some(dossier) = DOSSIER_UTILISATEUR.get() else {
        return Ok(0);
    };
    let chemin = dossier.join(nom);
    match std::fs::metadata(&chemin) {
        Ok(m) => {
            std::fs::remove_file(&chemin)?;
            Ok(m.len())
        }
        Err(_) => Ok(0),
    }
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
    Ok(hasheur
        .finalize()
        .iter()
        .map(|o| format!("{o:02x}"))
        .collect())
}

/// Vrai quand l'exécutable tourne depuis un paquet macOS
/// (`….app/Contents/MacOS/`) : les scripts du dépôt n'y existent pas.
pub fn dans_un_paquet() -> bool {
    std::env::current_exe().is_ok_and(|exe| chemin_dans_un_paquet(&exe))
}

fn chemin_dans_un_paquet(exe: &Path) -> bool {
    let Some(macos) = exe.parent() else {
        return false;
    };
    let contents = macos.parent();
    let app = contents.and_then(Path::parent);
    macos.file_name().is_some_and(|n| n == "MacOS")
        && contents
            .and_then(Path::file_name)
            .is_some_and(|n| n == "Contents")
        && app
            .and_then(Path::extension)
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

/// Que faire quand un poids manque. En développement : le script du dépôt
/// qui le prépare. Depuis un paquet, où ce script n'existe pas : le poids
/// aurait dû être livré avec l'application ou téléchargé au premier usage,
/// il faut la réinstaller.
pub fn remede(script: &str) -> String {
    remede_selon(dans_un_paquet(), script)
}

fn remede_selon(paquet: bool, script: &str) -> String {
    if paquet {
        "Ce modèle manque à l'application installée : réinstaller Rusty Music.".into()
    } else {
        script.into()
    }
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

    /// Deux demandes du même poids en même temps : une seule requête part,
    /// la seconde attend la première et rend le même fichier.
    #[test]
    fn un_meme_poids_demande_deux_fois_ne_part_quune_fois() {
        let dossier = std::env::temp_dir().join("rusty-music-test-modeles-vol");
        let _ = std::fs::remove_dir_all(&dossier);
        // Serveur d'une seule réponse : une seconde requête resterait sans
        // réponse et échouerait.
        let url = serveur(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        );
        let (d, u) = (dossier.clone(), url.clone());
        let premier = std::thread::spawn(move || {
            telecharger_dans(&d, "vol.safetensors", &u, 10, None, |_, _| {
                std::thread::sleep(std::time::Duration::from_millis(50))
            })
        });
        // Laisse le premier s'inscrire.
        while !en_cours("vol.safetensors") && !dossier.join("vol.safetensors").exists() {
            std::thread::yield_now();
        }
        let second = telecharger_dans(&dossier, "vol.safetensors", &url, 10, None, |_, _| {});
        assert_eq!(second.expect("second"), dossier.join("vol.safetensors"));
        assert!(premier.join().unwrap().is_ok());
        assert!(!en_cours("vol.safetensors"));
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
        assert!(
            r.is_err(),
            "empreinte fausse : le téléchargement doit échouer"
        );
        assert!(!dossier.join("hash-mauvais.safetensors").exists());
        assert!(!dossier.join("hash-mauvais.safetensors.partiel").exists());
    }

    #[test]
    fn un_paquet_ne_renvoie_pas_aux_scripts_du_depot() {
        assert!(chemin_dans_un_paquet(Path::new(
            "/Applications/Rusty Music.app/Contents/MacOS/rusty-music-desktop"
        )));
        assert!(!chemin_dans_un_paquet(Path::new(
            "/Users/x/rusty_music/target/release/rusty-music-desktop"
        )));
        assert!(!chemin_dans_un_paquet(Path::new("/opt/Contents/MacOS/x")));
        let paquet = remede_selon(true, "./scripts/preparer-adtof.sh");
        assert!(!paquet.contains("scripts/"), "{paquet}");
        assert!(paquet.contains("réinstaller"), "{paquet}");
        assert_eq!(
            remede_selon(false, "./scripts/preparer-adtof.sh"),
            "./scripts/preparer-adtof.sh"
        );
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
