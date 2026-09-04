//! Tuiles vectorielles de la carte (module 2).
//!
//! Rust produit les tuiles, MapLibre les affiche. Ce crate ne dessine rien : il
//! traduit ce que la base contient — positions, familles, nappe de densité — en
//! une archive PMTiles que la webview consomme.
//!
//! Il ne dépend ni de Burn, ni de l'audio, ni de Tauri : il compile en quelques
//! secondes et se teste sans accélérateur.

pub mod affectation;
pub mod ancrage;
pub mod batiments;
pub mod cout_itineraire;
pub mod cout_voirie;
pub mod hydro;
pub mod palette;
pub mod peuplement;
pub mod projection;
pub mod source;
pub mod style;
pub mod relief;
pub mod reseau_reel;
pub mod tuiles;
pub mod ville;

pub use palette::Palette;

/// Lit le champ de densité sous un point de carte, borné à `[0, 1]`.
///
/// Un raccourci partagé : le relief, le peuplement et le tracé des routes s'en
/// servent, et deux copies de cette conversion finiraient par diverger d'une
/// demi-cellule.
pub fn densite_sous(champ: &[f64], gn: usize, x: f32, y: f32) -> f64 {
    rusty_music_core::density::echantillonner(champ, gn, x, y).clamp(0.0, 1.0)
}
