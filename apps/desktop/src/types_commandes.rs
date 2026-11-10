// SPDX-License-Identifier: GPL-3.0-or-later
//! Essai C4 (synthèse de novembre, O7), étendu par V5 : les types que
//! reçoivent et renvoient les commandes contrôlées, exportés en TypeScript
//! dans `ui-tests/types/commandes.ts`. Les tests d'interface y confrontent
//! leurs réponses figées (`fixtures.js`, `ui-tests/types/controle.ts`), pour
//! qu'une forme qui change côté Rust fasse échouer la vérification au lieu de
//! dériver sans bruit.

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
            demix_variantes,
            // V5 : Explorer, ce qui a le plus changé depuis octobre.
            balade_demarrer,
            balade_etat,
            balade_arreter,
            balade_choisir,
            balade_revenir,
            balade_regler,
            balade_extrait,
            morceaux_ecoutes,
            sentiers_carte,
            balades_passees,
            balade_passee,
            balade_rejouer,
            epingles,
            creer_epingle,
            supprimer_epingle,
            chaines,
            garder_chaine,
            oublier_trace,
            aura,
            relief_musical,
            courbe,
            ouvrir_courbe,
            recompute_map,
            // E1 : la mémoire de Pratiquer par morceau.
            pratique_memoire,
            pratique_garder,
            // E2 : la partie corrigée, exportée.
            exporter_partie,
            // F1 : chercher le greffon.
            chercher_greffon,
            greffons_preparer,
            greffons_etat,
            // P1 : gérer les playlists.
            playlists,
            playlist_pistes,
            nouvelle_playlist,
            ajouter_a_playlist,
            retirer_de_playlist,
            remettre_dans_playlist,
            deplacer_dans_playlist,
            // E4 : le coach de Pratiquer, batterie MIDI.
            coach::coach_peripheriques,
            coach::coach_ecouter,
            coach::coach_garder_profil,
            coach::coach_calibrer,
            coach::coach_juger
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
