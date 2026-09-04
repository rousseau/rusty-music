// SPDX-License-Identifier: GPL-3.0-or-later
//! Construction Tauri, plus une chose que `tauri_build` ne fait pas : dire à
//! Cargo de **recompiler `main.rs`** quand l'interface change.
//!
//! **Sans cela, le binaire sert un `app.js` périmé.** Les fichiers de `ui/` sont
//! embarqués par la macro `tauri::generate_context!` au moment où `main.rs`
//! compile. Or `cargo:rerun-if-changed` ne fait que relancer *ce script* ; il
//! ne recompile `main.rs` que si la **sortie** du script change. On modifiait
//! donc l'interface, on recompilait, on lançait — et l'on testait la version de
//! la veille, sans le moindre avertissement. Le piège a coûté plusieurs heures
//! et, pire, un rapport faux : une fonctionnalité déclarée cassée alors qu'elle
//! marchait, le binaire testé étant antérieur au correctif.
//!
//! La parade : hacher tout `ui/` et publier ce hachage en variable
//! d'environnement de compilation. `main.rs` la lit (`env!`), donc tout
//! changement d'un fichier d'interface change la variable, invalide `main.rs`,
//! et force la ré-expansion de `generate_context!`. Plus besoin de
//! `cargo clean -p rusty-music-desktop`.

use std::path::Path;

fn main() {
    let mut h = Hachage::new();
    surveiller(Path::new("ui"), &mut h);
    println!("cargo:rustc-env=RUSTY_UI_HASH={:016x}", h.valeur());
    completer_ressources_de_paquet();
    tauri_build::build();
}

/// `tauri_build::build()` refuse de tourner si une ressource déclarée dans
/// `tauri.conf.json` manque — même pour un simple `cargo build`/`clippy`, où
/// l'on n'empaquette rien.
///
/// Deux d'entre elles ne sont pas dans le dépôt : `htdemucs.safetensors`
/// (téléchargée par `scripts/telecharger-modeles.sh`) et
/// `clap-audio-encoder-b5.bpk` (produite par `crates/analysis/build.rs`, mais
/// seulement en profil `release`, et sans garantie d'ordre entre scripts de
/// build). Pour que la compilation aboutisse quand même, on pose un fichier
/// vide en dernier recours. Un build `release` — le seul qui empaquette —
/// écrit le vrai `.bpk` par-dessus (`analysis/build.rs`), et
/// `scripts/telecharger-modeles.sh` fournit le vrai `.safetensors`.
fn completer_ressources_de_paquet() {
    for nom in ["clap-audio-encoder-b5.bpk", "htdemucs.safetensors"] {
        let chemin = Path::new("../../models").join(nom);
        if chemin.exists() {
            continue;
        }
        if let Some(dir) = chemin.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::File::create(&chemin) {
            Ok(_) => println!(
                "cargo:warning=ressource `{nom}` absente — fichier vide posé. \
                 Un build release ou `scripts/telecharger-modeles.sh` fournit la vraie."
            ),
            Err(e) => println!("cargo:warning=impossible de poser `{nom}` : {e}"),
        }
    }
}

/// Déclare chaque fichier de `ui/` à Cargo (pour relancer ce script) et le
/// verse dans le hachage (pour invalider `main.rs`). Récursif :
/// `cargo:rerun-if-changed` sur un dossier ne couvre que son contenu immédiat.
fn surveiller(dossier: &Path, h: &mut Hachage) {
    println!("cargo:rerun-if-changed={}", dossier.display());
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return;
    };
    // Tri : `read_dir` ne garantit aucun ordre, et le hachage doit être stable
    // d'une machine à l'autre.
    let mut chemins: Vec<_> = entrees.flatten().map(|e| e.path()).collect();
    chemins.sort();
    for chemin in chemins {
        if chemin.is_dir() {
            surveiller(&chemin, h);
        } else {
            println!("cargo:rerun-if-changed={}", chemin.display());
            h.avaler(chemin.to_string_lossy().as_bytes());
            if let Ok(contenu) = std::fs::read(&chemin) {
                h.avaler(&contenu);
            }
        }
    }
}

/// FNV-1a 64 bits — même construction que `core::db::hacher`. Pas besoin de
/// résistance cryptographique : on veut juste qu'un octet différent donne un
/// hachage différent.
struct Hachage(u64);

impl Hachage {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn avaler(&mut self, octets: &[u8]) {
        for &o in octets {
            self.0 ^= o as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn valeur(&self) -> u64 {
        self.0
    }
}
