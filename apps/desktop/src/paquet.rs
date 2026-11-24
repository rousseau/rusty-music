// SPDX-License-Identifier: GPL-3.0-or-later
//! Les poids que l'application installée doit trouver dans son paquet.
//!
//! Chaque modèle que l'app peut demander est soit **embarqué** (`bundle >
//! resources` de `tauri.conf.json`), soit **téléchargé** (au premier
//! lancement ou au premier usage, `rusty_music_core::modeles::telecharger`,
//! avec son adresse, sa taille et son empreinte). La liste complète, et ce
//! que l'utilisateur en voit, est dans `modeles.rs` (Bibliothèque → Modèles).
//! Inventaire du 24 nov. 2026 :
//!
//! | modèle | fichier | sort |
//! |---|---|---|
//! | CLAP audio et texte | `clap-audio-encoder-b5.bpk`, `clap-text-encoder.bpk` | téléchargés **au premier lancement** (`rousseau/clap-htsat-unfused-burn`) |
//! | ADTOF (batterie) | `adtof_frame_rnn.onnx` | embarqué — converti à la main, aucune source à télécharger |
//! | HTDemucs (4 et 6 stems, affinée) | `htdemucs*.safetensors` | téléchargés au premier usage |
//! | BS-RoFormer SW | `BS-Rofo-SW-Fixed.ckpt` | téléchargé à la source |
//! | Basic Pitch (basse) | `basic_pitch_nmp.onnx` | embarqué (et téléchargeable) — V25 |
//! | Beat This! mel et petit | `beat_this_mel.onnx`, `beat_this_small.onnx` | embarqués (et téléchargeables) — V25 |
//! | Beat This! complet | `beat_this.onnx` | téléchargé (83 Mo) |
//! | all-in-one (sections) | `allinone-harmonix-fold*.safetensors` | embarqués (et téléchargeables) — V25 |
//! | AERO (« HD ») | `aero-11025-44100.onnx` | téléchargé |
//! | MuScriptor | `muscriptor-medium.safetensors` | téléchargé (jeton Hugging Face) |
//! | Gemma 4 E4B (Lama) | `gemma-4-E4B-it-Q4_0.gguf` | téléchargé |
//!
//! CLAP (590 Mo) et HTDemucs (84 Mo) sont sortis du paquet le 24 nov. 2026 :
//! le `.dmg` passait de 712 Mo à une centaine. Les petits modèles de
//! Pratiquer (≈ 21 Mo) restent embarqués pour que la basse, les temps et les
//! sections marchent hors ligne dès le premier usage ; leur téléchargement
//! reste le repli (`modeles::trouver` cherche d'abord dans `Resources/models`).
//!
//! Les tests ci-dessous font échouer la CI quand un modèle **requis** sort des
//! ressources, ou quand `scripts/telecharger-modeles.sh` (qui les apporte à
//! `release.sh`) ne les prend plus à la même source et à la même empreinte ; `scripts/release.sh` refuse, lui,
//! de construire un paquet dont un fichier de ressources manque sur la
//! machine.

/// Les poids que rien ne télécharge : sans eux dans le paquet, la fonction
/// qui en dépend échoue dans l'application installée.
pub(crate) fn requis_sans_telechargement() -> [&'static str; 1] {
    [rusty_music_transcription::batterie::FICHIER]
}

/// Les poids embarqués pour Pratiquer, qui gardent leur téléchargement comme
/// repli (V25) : Basic Pitch, Beat This! (mel et petit), all-in-one.
pub(crate) fn embarques_avec_repli() -> Vec<(&'static str, &'static str, &'static str)> {
    use rusty_music_editor::pulsation::{Modele, MEL};
    let mut v = vec![
        {
            let p = rusty_music_transcription::basic_pitch::POIDS;
            (p.nom, p.url, p.sha256)
        },
        (MEL.nom, MEL.url, MEL.sha256),
        {
            let p = Modele::Petit.poids();
            (p.nom, p.url, p.sha256)
        },
    ];
    v.extend(
        rusty_music_transcription::sections::POIDS
            .iter()
            .map(|p| (p.nom, p.url, p.sha256)),
    );
    v
}

#[cfg(test)]
fn ressources() -> serde_json::Map<String, serde_json::Value> {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json lisible");
    conf["bundle"]["resources"]
        .as_object()
        .expect("bundle > resources : une table source → destination")
        .clone()
}

#[test]
fn les_petits_modeles_de_pratiquer_sont_dans_le_paquet() {
    let ressources = ressources();
    let modeles = embarques_avec_repli();
    assert_eq!(
        modeles.len(),
        11,
        "Basic Pitch, Beat This! mel et petit, 8 plis"
    );
    for (nom, _, _) in modeles {
        assert_eq!(
            ressources
                .get(&format!("../../models/{nom}"))
                .and_then(|v| v.as_str()),
            Some(format!("models/{nom}").as_str()),
            "{nom} : embarqué pour Pratiquer hors ligne (V25)"
        );
    }
}

#[test]
fn le_script_apporte_les_petits_modeles_a_la_meme_source() {
    let script = include_str!("../../../scripts/telecharger-modeles.sh");
    for (nom, url, sha) in embarques_avec_repli() {
        assert!(
            script.contains(&format!("{nom}) echo {url} ;;")),
            "telecharger-modeles.sh : {nom} doit venir de {url}"
        );
        assert!(
            script.contains(&format!("{nom}) echo {sha} ;;")),
            "telecharger-modeles.sh : empreinte de {nom} attendue {sha}"
        );
    }
}

#[test]
fn les_modeles_sans_telechargement_sont_dans_le_paquet() {
    let ressources = ressources();
    for nom in requis_sans_telechargement() {
        let destination = format!("models/{nom}");
        let source = format!("../../models/{nom}");
        assert_eq!(
            ressources.get(&source).and_then(|v| v.as_str()),
            Some(destination.as_str()),
            "{nom} n'a aucun téléchargement : il doit être embarqué \
             (tauri.conf.json, bundle > resources)"
        );
    }
}
