// SPDX-License-Identifier: GPL-3.0-or-later
//! Essai C4 (synthèse de novembre, O7) : les types que renvoient quelques
//! commandes, exportés en TypeScript dans `ui-tests/types/commandes.ts`. Les
//! tests d'interface y confrontent leurs réponses figées (`fixtures.js`,
//! `ui-tests/types/controle.ts`), pour qu'une forme qui change côté Rust
//! fasse échouer la vérification au lieu de dériver sans bruit.

use super::*;

#[test]
fn exporter_les_types_des_commandes() {
    // Les entiers 64 bits (identifiants, comptes, octets) deviennent des
    // `number` : c'est déjà ce que fait le JSON de Tauri à l'exécution.
    let constructeur = tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            roots,
            artists,
            albums,
            library_stats,
            demix_variantes
        ])
        .dangerously_cast_bigints_to_number()
        // Les noms d'`invoke` tels quels, et la valeur de retour nue : une
        // erreur rejette la promesse, comme pour `invoke` lui-même.
        .function_casing(tauri_specta::Casing::SnakeCase)
        .error_handling(tauri_specta::ErrorHandlingMode::Throw);
    let sortie = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui-tests/types/commandes.ts");
    std::fs::create_dir_all(sortie.parent().unwrap()).unwrap();
    constructeur
        .export(specta_typescript::Typescript::default(), &sortie)
        .expect("export des types");
}
