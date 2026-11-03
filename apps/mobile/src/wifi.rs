// SPDX-License-Identifier: GPL-3.0-or-later
//! Le Mac sur le Wi-Fi local (étape 4B, passage option B). Le Mac a déposé
//! `appairage.json` au câble : son jeton et les adresses où le chercher
//! (`rusty_music_core::appairage`). L'app le joint au lancement et à chaque
//! retour au premier plan, pour prendre un passage qu'il tient en attente
//! (`passage.rs`), et quand on lui rend la lecture ([`rendre`]).

use std::time::Duration;

use rusty_music_core::appairage::{self, Appairage, Bonjour, Client};
use rusty_music_core::passage::{Passage, VERSION};
use rusty_music_core::Error;
use serde::Serialize;

use crate::{echec, en_cours, lecture_courante, verrou, Etat};

/// Le Mac appairé, s'il répond. Un refus (jeton oublié par le Mac) efface
/// l'appairage : le bouton disparaît jusqu'au prochain câble.
pub(crate) fn joindre(etat: &Etat) -> Option<(Client, Bonjour)> {
    let chemin = etat.dossier.join(appairage::FICHIER);
    let a = Appairage::lire(&chemin).ok()?;
    let preferee = verrou(&etat.adresse_mac).clone();
    match Client::joindre(&a, preferee.as_deref()) {
        Ok((client, bonjour)) => {
            *verrou(&etat.adresse_mac) = Some(client.adresse().to_string());
            Some((client, bonjour))
        }
        Err(Error::AccesRefuse(raison)) => {
            tracing::warn!(%raison, "appairage refusé par le Mac, effacé");
            let _ = std::fs::remove_file(&chemin);
            None
        }
        Err(e) => {
            tracing::debug!(erreur = %e, "Mac injoignable");
            None
        }
    }
}

/// Ce que le Mac a repris de la lecture rendue.
#[derive(Serialize)]
pub(crate) struct Rendu {
    mac: String,
    pistes: usize,
    absents: usize,
    courante_presente: bool,
    /// L'iPhone s'est mis en pause (la lecture n'a pas changé entre-temps).
    en_pause: bool,
}

/// Rend la lecture au Mac, qui la charge en pause ; l'iPhone ne se tait
/// qu'une fois l'accusé du Mac reçu, et seulement s'il joue encore ce qu'il a
/// confié.
pub(crate) fn rendre(etat: &Etat) -> Result<Rendu, String> {
    let (client, bonjour) = joindre(etat).ok_or("le Mac ne répond pas sur ce réseau")?;
    let i = verrou(&etat.player)
        .as_ref()
        .and_then(rusty_music_player::Player::instantane)
        .ok_or("rien en lecture")?;
    let courant = i.file.get(i.rang).cloned();
    let chemins = |v: &[std::path::PathBuf]| -> Vec<String> {
        v.iter().map(|p| p.display().to_string()).collect()
    };
    let passage = Passage {
        version: VERSION,
        emis_le: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as i64,
        file: chemins(&i.file),
        avant_melange: chemins(&i.avant_melange),
        rang: i.rang,
        position_ms: i.position.as_millis() as u64,
        alea: i.alea,
        repetition: i.repetition.nom().to_string(),
        volume: i.volume,
    };
    let accuse = client.rendre(&passage).map_err(echec)?;
    let en_pause = verrou(&etat.player).as_ref().is_some_and(|p| {
        let meme = p.current() == courant.as_deref();
        if meme {
            p.pause();
        }
        meme
    });
    en_cours::afficher(lecture_courante(etat).as_ref());
    tracing::info!(pistes = accuse.pistes, en_pause, "lecture rendue au Mac");
    Ok(Rendu {
        mac: bonjour.mac,
        pistes: accuse.pistes,
        absents: accuse.absents,
        courante_presente: accuse.courante_presente,
        en_pause,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex, RwLock};

    /// Un Mac qui a oublié cet iPhone : l'appairage s'efface, le bouton
    /// « Sur le Mac » disparaît jusqu'au prochain câble.
    #[test]
    fn un_mac_qui_refuse_le_jeton_efface_l_appairage() {
        let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = ecoute.local_addr().unwrap().port();
        let fil = std::thread::spawn(move || {
            let (mut flux, _) = ecoute.accept().unwrap();
            let mut tampon = [0u8; 1024];
            let _ = flux.read(&mut tampon);
            flux.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });

        let dossier =
            std::env::temp_dir().join(format!("rusty-wifi-mobile-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let fichier = dossier.join(appairage::FICHIER);
        Appairage {
            version: appairage::VERSION,
            mac: "Mac".into(),
            jeton: "oublie".into(),
            port,
            adresses: vec!["127.0.0.1".into()],
        }
        .ecrire(&fichier)
        .unwrap();
        let etat = Etat {
            dossier: dossier.clone(),
            valise: Mutex::new(None),
            player: Mutex::new(None),
            fichiers: Arc::new(RwLock::new(HashMap::new())),
            gains: Arc::new(RwLock::new(HashMap::new())),
            precharge: Mutex::new(()),
            adresse_mac: Mutex::new(None),
            dernier_passage: Mutex::new(0),
        };
        assert!(joindre(&etat).is_none());
        assert!(!fichier.exists());
        fil.join().unwrap();
        std::fs::remove_dir_all(&dossier).unwrap();
    }
}
