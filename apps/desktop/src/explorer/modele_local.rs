// SPDX-License-Identifier: GPL-3.0-or-later
//! Le modèle de langue intégré de Lama : Gemma 4 E4B (GGUF, Apache-2.0),
//! exécuté dans le processus (`rusty_music_core::llm_local`, feature
//! `lama-local`). Lama n'a plus besoin d'Ollama ; Ollama reste un choix.
//!
//! Téléchargé à la source au premier usage, chargé à la première demande,
//! déchargé après inactivité comme les autres poids (`liberer_si_inactif`).

use super::*;

/// Le fichier, sa source à une révision épinglée, son empreinte et sa taille
/// (`docs/recherche-lama-sans-ollama.md`).
const FICHIER: &str = "gemma-4-E4B-it-Q4_0.gguf";
const URL: &str = "https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/\
                   a69f018d7678a2b9f5e705f8903d6fb29b9b25dc/gemma-4-E4B-it-Q4_0.gguf";
const SHA256: &str = "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2";
const OCTETS: u64 = 4_590_807_392;

/// Le nom du modèle intégré dans le sélecteur de Lama (icône 🦙), à côté
/// des modèles d'Ollama.
pub(crate) const NOM: &str = "integre";

/// Avancement du téléchargement du modèle, en octets.
pub(crate) type EtatTelechargementLama = Passe<Compte>;

/// `pret` (fichier présent), `a_telecharger`, ou `absent` : cette version a
/// été compilée sans la feature `lama-local`.
pub(crate) fn disponibilite() -> &'static str {
    if !cfg!(feature = "lama-local") {
        "absent"
    } else if rusty_music_core::modeles::trouver(FICHIER).is_some() {
        "pret"
    } else {
        "a_telecharger"
    }
}

/// Télécharge le modèle intégré (4,6 Go), en passe de fond.
#[tauri::command(async)]
pub(crate) fn lama_telecharger(app: tauri::AppHandle) -> Result<(), String> {
    if !cfg!(feature = "lama-local") {
        return Err("cette version de Rusty Music n'intègre pas de modèle de langue".into());
    }
    passe::lancer(
        app,
        |e| &e.lama_telechargement,
        "le modèle est déjà en cours de téléchargement",
        Compte {
            faits: 0,
            total: OCTETS as usize,
        },
        |etat| {
            let issue = rusty_music_core::modeles::telecharger(
                FICHIER,
                URL,
                OCTETS,
                Some(SHA256),
                |vus, total| {
                    let mut e = verrou(&etat.lama_telechargement);
                    e.avancement.faits = vus as usize;
                    e.avancement.total = total.unwrap_or(OCTETS) as usize;
                },
            );
            match issue {
                Ok(chemin) => {
                    tracing::info!(chemin = %chemin.display(), "Lama : modèle intégré téléchargé");
                    "modèle intégré prêt".into()
                }
                Err(e) => format!("échec : {e}"),
            }
        },
    )
}

/// Où en est le téléchargement du modèle intégré.
#[tauri::command]
pub(crate) fn lama_telechargement_state(etat: State<Etat>) -> EtatTelechargementLama {
    verrou(&etat.lama_telechargement).clone()
}

/// Interprète `prompt` avec le modèle intégré, chargé au besoin.
///
/// Le modèle sort du verrou dans un `Arc` : la génération (quelques
/// secondes) ne bloque ni le fil d'éviction ni une autre demande, et une
/// éviction pendant la génération attend simplement qu'elle finisse.
#[cfg(feature = "lama-local")]
pub(crate) fn interpreter(
    etat: &Etat,
    prompt: &str,
    vocabulaire: &[String],
) -> Result<rusty_music_core::ollama::InterpretationLlm, String> {
    use rusty_music_core::llm_local::ModeleLocal;
    let chemin = rusty_music_core::modeles::trouver(FICHIER)
        .ok_or("modèle intégré absent : téléchargez-le depuis l'accueil de Lama")?;
    let modele = {
        let mut cache = verrou(&etat.lama_local);
        if cache.valeur.is_none() {
            let t = Instant::now();
            cache.valeur = Some(Arc::new(ModeleLocal::charger(&chemin).map_err(echec)?));
            tracing::info!(duree = ?t.elapsed(), "Lama : modèle intégré chargé");
        }
        cache.touche = Some(Instant::now());
        Arc::clone(cache.valeur.as_ref().expect("chargé juste au-dessus"))
    };
    let interpretation = modele.interpreter(prompt, vocabulaire).map_err(echec);
    verrou(&etat.lama_local).touche = Some(Instant::now());
    interpretation
}

#[cfg(not(feature = "lama-local"))]
pub(crate) fn interpreter(
    _: &Etat,
    _: &str,
    _: &[String],
) -> Result<rusty_music_core::ollama::InterpretationLlm, String> {
    Err("cette version de Rusty Music n'intègre pas de modèle de langue".into())
}
