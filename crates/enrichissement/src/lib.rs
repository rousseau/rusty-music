// SPDX-License-Identifier: GPL-3.0-or-later
//! Enrichissement en ligne : ce que les sources publiques disent d'une
//! bibliothèque (genres, éditions, popularité, pochettes, biographies,
//! critiques, crédits Discogs), et les passes qui le versent dans la base.
//!
//! Toujours additif : une bibliothèque sans réseau reste entièrement
//! utilisable. Les requêtes SQL vivent dans `rusty_music_core::db` ; les
//! clients HTTP partagent `rusty_music_core::http`. Séparé du cœur pour
//! qu'une modification ici ne recompile ni le lecteur ni l'analyse.

pub mod biographies;
pub mod critiquebrainz;
pub mod critiques;
pub mod decouvrir;
pub mod deezer;
pub mod discogs;
pub mod discogs_import;
pub mod editions;
pub mod enrichir;
pub mod lastfm;
pub mod lastfm_pass;
pub mod listenbrainz;
pub mod musicbrainz;
pub mod pochette;
pub mod popularite;
pub mod theaudiodb;
