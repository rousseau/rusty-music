// SPDX-License-Identifier: GPL-3.0-or-later
//! Lama : une demande en texte libre (« du calme pour travailler, plutôt des
//! années 90 ») interprétée en filtres de playlist, puis appliquée à la
//! bibliothèque.
//!
//! L'interprétation passe par Ollama (`ollama`) ou par un modèle intégré
//! (`llm_local`, feature `lama-local` : llama.cpp compilé dans l'application,
//! il faut cmake) ; les filtres et leur relâchement sont dans
//! `filtres_playlist`. Séparé du cœur pour que llama.cpp, et toute
//! modification ici, ne recompilent ni le lecteur ni l'analyse.

pub mod filtres_playlist;
#[cfg(feature = "lama-local")]
pub mod llm_local;
pub mod ollama;
