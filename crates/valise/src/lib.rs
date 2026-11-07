// SPDX-License-Identifier: GPL-3.0-or-later
//! Valise : ce que le compagnon iOS emporte de la bibliothèque.
//!
//! La sélection et l'export (`valise`), l'état d'un envoi entre le Mac et le
//! téléphone (`passage`), et l'appairage par le Wi-Fi (`appairage`). Séparée
//! de `rusty-music-core` pour qu'une modification ici ne recompile ni le
//! lecteur ni l'analyse ; le compagnon iOS en dépend comme le bureau.

pub mod appairage;
pub mod passage;
pub mod valise;
