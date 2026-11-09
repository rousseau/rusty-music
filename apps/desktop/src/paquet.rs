// SPDX-License-Identifier: GPL-3.0-or-later
//! Les poids que l'application installée doit trouver dans son paquet.
//!
//! Chaque modèle que l'app peut demander est soit **embarqué** (`bundle >
//! resources` de `tauri.conf.json`), soit **téléchargé au premier usage**
//! (`rusty_music_core::modeles::telecharger`, avec son adresse, sa taille et
//! son empreinte). Inventaire du 9 nov. 2026 (V22) :
//!
//! | modèle | fichier | sort |
//! |---|---|---|
//! | CLAP audio | `clap-audio-encoder-b5.bpk` | embarqué |
//! | CLAP texte | `clap-text-encoder.bpk` | embarqué |
//! | HTDemucs standard | `htdemucs.safetensors` | embarqué (et téléchargeable) |
//! | ADTOF (batterie) | `adtof_frame_rnn.onnx` | embarqué — converti à la main, aucune source à télécharger |
//! | HTDemucs 6 stems, affinée | `htdemucs_6s`, `htdemucs_ft` | téléchargés |
//! | BS-RoFormer SW | `BS-Rofo-SW-Fixed.ckpt` | téléchargé à la source |
//! | Basic Pitch (basse) | `basic_pitch_nmp.onnx` | téléchargé |
//! | Beat This! | `beat_this*.onnx` | téléchargés |
//! | all-in-one (sections) | `allinone-harmonix-fold*.safetensors` | téléchargés |
//! | AERO (« HD ») | `aero-11025-44100.onnx` | téléchargé |
//! | MuScriptor | `muscriptor-medium.safetensors` | téléchargé (jeton Hugging Face) |
//! | Gemma 4 E4B (Lama) | `gemma-4-E4B-it-Q4_0.gguf` | téléchargé |
//!
//! Le test ci-dessous fait échouer la CI quand un modèle **sans
//! téléchargement** sort des ressources ; `scripts/release.sh` refuse, lui,
//! de construire un paquet dont un fichier de ressources manque sur la
//! machine.

/// Les poids que rien ne télécharge : sans eux dans le paquet, la fonction
/// qui en dépend échoue dans l'application installée.
pub(crate) fn requis_sans_telechargement() -> [&'static str; 3] {
    [
        rusty_music_analysis::Embedder::POIDS,
        rusty_music_analysis::EmbedderTexte::POIDS,
        rusty_music_transcription::batterie::FICHIER,
    ]
}

#[test]
fn les_modeles_sans_telechargement_sont_dans_le_paquet() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json lisible");
    let ressources = conf["bundle"]["resources"]
        .as_object()
        .expect("bundle > resources : une table source → destination");
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
