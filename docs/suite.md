# Où en est le projet, et ce qui reste

État au **31 oct. 2026**. Ce document dit où en est chaque brique et ce qui
reste, dans l'ordre ; le détail vit dans les documents cités. L'ancienne
version (plan d'août, dettes D1-D4, chantiers 1 à 9, presque tous faits) est
dans [`archives/suite-2026-08.md`](archives/suite-2026-08.md). Ce qui a été
fait et mesuré, au jour le jour : `journal.md`.

## Les briques

| Brique | État | Restes, et où les lire |
|---|---|---|
| Cœur d'ingestion | livré : scan, surveillance (`notify`), tags (`lofty`), base SQLite, décodage symphonia + Opus pur Rust | migrations versionnées de la base (chantier d'assainissement, ci-dessous) |
| Enrichissement en ligne | livré : genres MusicBrainz (si adresse de contact), popularité ListenBrainz + Deezer, pochettes de repli (Cover Art Archive, Deezer), bios TheAudioDB, crédits Discogs (dumps), critiques CritiqueBrainz, tags Last.fm (clé de l'utilisateur) ; vérification des pochettes locales | la popularité et les pochettes de repli n'ont pas de case pour les désactiver (`enrichissement-lecteur.md`, `popularite.md`) |
| Module 1 — Écouter | livré, v0.1.0 ; plan 0.2 soldé (reprise de session, playlists, historique et favoris, « En cours de lecture » macOS, minuteur, enchaînement sans blanc) | paroles et égaliseur écartés ; `plan-ecouter-v0.2.md` |
| Module 2 — Explorer | livré : carte de Paris (OpenStreetMap), croissance chronologique, ancrage des artistes populaires aux monuments, palettes de fond, itinéraires sur la voirie, familles nommées par vote, coloration tempo/énergie, Lama (texte → playlist, par parties) | Lama : raffinement multi-tours, fusion de graines, ordre lissé (`recherche-llm-playlist.md`) ; Lama dépend d'Ollama, processus externe (chantier d'assainissement, phase 7) ; peuplement persisté et autocomplétion multi-type écartés pour l'instant |
| Module 3 — Éditer | périmètre d'`ui-spec-editeur.md` couvert (démixage, vitesse, hauteur, réglage par stem, greffe calée sur les temps, export) ; *pratiquer* : pulsation Beat This!, boucle à la mesure, partition alphaTab, basse (Basic Pitch ou MuScriptor, tablature doigtée), batterie (ADTOF), sections (all-in-one) ; *créer* : greffe mesure par mesure, section sur section ; BS-RoFormer « SW » variante par défaut pour pratiquer | `plan-editer-pratique-creation.md` : 1.1 autoroute de notes, 1.5 corriger la transcription, 2.2 chercher le greffon, 2.3 COCOLA, 2.5-2.7 (LarsNet, GrooVAE, génération) ; publication des poids convertis sur Hugging Face (0.1, en partie faite) ; tonalité cible par stem |
| Backend `burn/metal` | parité faite (31 oct.) : CLAP, all-in-one, BS-RoFormer, MuScriptor f32 exacts en MSL ; htdemucs et htdemucs_ft aussi | htdemucs_6s faux en MSL, à localiser dans `demucs-core` avant de basculer toute l'application ; reprise : `experiments/burn-metal/README.md` |
| Super-résolution « HD » | livrée (`crates/superres`, AERO via ONNX Runtime) | — (`module3-superresolution.md`) |
| Compagnon iOS « Rusty Valise » | tourne sur l'iPhone (18 oct.) ; mode Valise du bureau (19 oct.) ; file et reprise (22 oct.) | étapes 1 (découper le code), 3 (sélection, en partie), 4B (transfert sans Xcode), 7 (rotation) ; mesures de batterie ; `plan-ios-compagnon.md` |

## Chantier en cours : assainir le dépôt (ouvert le 31 oct. 2026)

À la suite d'un audit : fonctionnellement abouti, couche applicative fragile.
Phases, dans l'ordre ; aucun changement de comportement visible voulu, sauf
mention contraire.

0. Point de départ : `scripts/verifier.sh` mesuré (`journal.md`, 31 oct.).
1. Hygiène de la documentation : README, `CLAUDE.md` ramené aux décisions et à
   l'index, historique sous `archives/`, ce document.
2. Filet de sécurité : `crates/analysis` testable sans le modèle CLAP, CI
   GitHub Actions minimale (macOS), tests de fumée de l'interface (Chrome
   headless, `invoke` simulé).
3. Migrations versionnées de la base (`PRAGMA user_version`).
4. Découper `apps/desktop/src/main.rs` : un module de commandes par mode, un
   type générique de passe de fond, progression par `tauri::ipc::Channel`.
5. Découper `apps/desktop/ui/app.js` en modules ES natifs, un magasin d'état
   par mode.
6. Découper `crates/core/src/db.rs` par domaine.
7. Lama sans Ollama obligatoire : étude d'un modèle exécuté dans le processus,
   rapport avant toute implémentation.
8. Préparer la publication de crates autonomes : `publication-crates.md`.

## Ce qui n'est pas prévu, et pourquoi

- **Mixage DJ de deux pistes** : sorti du périmètre du module 3 (une piste à
  la fois, `ui-spec-editeur.md`). La grille de battements le rendrait
  possible, mais rien n'est planifié.
- **Wikidata** pour les infos d'artiste : jamais implémenté, TheAudioDB le
  remplace (`enrichissement-lecteur.md`).
- **Deux morceaux hors carte** (un m4a, un mp3) : fichiers réellement
  corrompus.
