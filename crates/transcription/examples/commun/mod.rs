// SPDX-License-Identifier: GPL-3.0-or-later
//! Choix du backend des exemples MuScriptor, à l'exécution, par la variable
//! `MOTEUR` — pour comparer sans recompiler :
//! - `f16` (défaut de l'application) : `wgpu`, fusion, flottants 16 bits ;
//! - `f32` : `wgpu`, fusion, flottants 32 bits ;
//! - `f16-brut`, `f32-brut` : sans la fusion de Burn.
//!
//! Sans la fonctionnalité `gpu`, toujours le CPU (`NdArray`, f32).

/// Appelle `$f::<B>()` avec le backend choisi.
#[cfg(feature = "gpu")]
macro_rules! selon_moteur {
    ($f:ident) => {{
        use burn::backend::wgpu::{CubeBackend, WgpuRuntime};
        use burn::backend::Wgpu;
        use burn::tensor::f16;
        let moteur = std::env::var("MOTEUR").unwrap_or_else(|_| "f16".into());
        eprintln!("moteur : {moteur}");
        match moteur.as_str() {
            "f32" => $f::<Wgpu<f32>>(),
            "f16-brut" => $f::<CubeBackend<WgpuRuntime, f16, i32, u32>>(),
            "f32-brut" => $f::<CubeBackend<WgpuRuntime, f32, i32, u32>>(),
            _ => $f::<Wgpu<f16>>(),
        }
    }};
}

#[cfg(not(feature = "gpu"))]
macro_rules! selon_moteur {
    ($f:ident) => {
        $f::<burn::backend::NdArray>()
    };
}
