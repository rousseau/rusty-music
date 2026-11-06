# Où en est le projet, et ce qui reste

État au **6 nov. 2026**. Ce document dit où en est chaque brique et ce qui
reste, dans l'ordre ; le détail vit dans les documents cités. L'ancienne
version (plan d'août, dettes D1-D4, chantiers 1 à 9, presque tous faits) est
dans [`archives/suite-2026-08.md`](archives/suite-2026-08.md). Ce qui a été
fait et mesuré, au jour le jour : `journal.md`.

## Les briques

| Brique | État | Restes, et où les lire |
|---|---|---|
| Cœur d'ingestion | livré : scan, surveillance (`notify`), tags (`lofty`), base SQLite, décodage symphonia + Opus pur Rust | décodage Opus lent : `opus-decoder` met 11 à 13 s pour ouvrir un morceau Opus de 2 min de la bibliothèque (Fingathing, 10 morceaux) ; `opus-pure` décode 15 s en 10,5 ms contre 787 ms (relevé par le banc de P4, `journal.md`, 6 nov.) ; à remplacer. Migrations versionnées depuis le 31 oct. : `db::schema::migrations` |
| Enrichissement en ligne | livré : genres MusicBrainz (si adresse de contact), popularité ListenBrainz + Deezer, pochettes de repli (Cover Art Archive, Deezer), bios TheAudioDB, crédits Discogs (dumps), critiques CritiqueBrainz, tags Last.fm (clé de l'utilisateur) ; vérification des pochettes locales | la popularité et les pochettes de repli n'ont pas de case pour les désactiver (`enrichissement-lecteur.md`, `popularite.md`) |
| Module 1 — Écouter | livré, v0.1.0 ; plan 0.2 soldé (reprise de session, playlists, historique et favoris, « En cours de lecture » macOS, minuteur, enchaînement sans blanc) ; palette de recherche ⌘K (1ᵉʳ nov.) | paroles et égaliseur écartés ; `plan-ecouter-v0.2.md` |
| Module 2 — Explorer | livré : carte de Paris (OpenStreetMap), croissance chronologique, ancrage des artistes populaires aux monuments, palettes de fond, itinéraires sur la voirie, familles nommées par vote, coloration tempo/énergie, Lama (texte → playlist, par parties ; modèle intégré Gemma 4 E4B ou Ollama au choix, phase 7) | Lama : raffinement multi-tours, fusion de graines, ordre lissé (`recherche-llm-playlist.md`) ; peuplement persisté et autocomplétion multi-type écartés pour l'instant |
| Module 3 — Pratiquer et Créer (ex-Éditer, deux modes du rail depuis le 5 nov.) | périmètre d'`ui-spec-editeur.md` couvert (démixage, vitesse, hauteur, réglage par stem, greffe calée sur les temps, export) ; *pratiquer* : pulsation Beat This!, boucle à la mesure, partition alphaTab, basse (Basic Pitch ou MuScriptor, tablature doigtée), batterie (ADTOF), sections (all-in-one) ; *créer* : greffe mesure par mesure, section sur section ; BS-RoFormer « SW » variante par défaut pour pratiquer | `plan-editer-pratique-creation.md` : 1.1 autoroute de notes, 1.5 corriger la transcription, 2.2 chercher le greffon, 2.3 COCOLA, 2.5-2.7 (LarsNet, GrooVAE, génération) ; publication des poids convertis sur Hugging Face (0.1, en partie faite) ; tonalité cible par stem ; ce que Pratiquer retient d'une séance à l'autre (`ui-spec-editeur.md`, questions ouvertes) |
| Backend `burn/metal` | parité faite (31 oct.) : CLAP, all-in-one, BS-RoFormer, MuScriptor f32 exacts en MSL ; htdemucs et htdemucs_ft aussi | htdemucs_6s faux en MSL, à localiser dans `demucs-core` avant de basculer toute l'application ; reprise : `experiments/burn-metal/README.md` |
| Super-résolution « HD » | livrée (`crates/superres`, AERO via ONNX Runtime) | — (`module3-superresolution.md`) |
| Compagnon iOS « Rusty Valise » | tourne sur l'iPhone (18 oct.) ; mode Valise du bureau (19 oct.) ; file et reprise (22 oct.) ; passage Mac → iPhone par fichier (2 nov.) ; passage dans les deux sens et valise par différence, par le Wi-Fi, appairage au câble (3 nov.) | étapes 1 (découper le code), 3 (sélection, en partie), 7 (rotation) ; essais sur l'iPhone (passages, valise par le Wi-Fi) ; mesures de batterie ; `plan-ios-compagnon.md` |

## Chantier en cours : assainir le dépôt (ouvert le 31 oct. 2026)

À la suite d'un audit : fonctionnellement abouti, couche applicative fragile.
Phases, dans l'ordre ; aucun changement de comportement visible voulu, sauf
mention contraire.

0. Point de départ : `scripts/verifier.sh` mesuré (`journal.md`, 31 oct.).
1. Hygiène de la documentation : README, `CLAUDE.md` ramené aux décisions et à
   l'index, historique sous `archives/`, ce document — fait.
2. Filet de sécurité : `crates/analysis` testable sans le modèle CLAP, CI
   GitHub Actions minimale (macOS), tests de fumée de l'interface (Chrome
   headless, `invoke` simulé) — fait.
3. Migrations versionnées de la base (`PRAGMA user_version`,
   `rusqlite_migration`) — fait.
4. Découper `apps/desktop/src/main.rs` : un module de commandes par mode, un
   type générique de passe de fond — fait ; progression par
   `tauri::ipc::Channel` (4c) abandonnée (`docs/journal.md`, phase 5b).
5. Découper `apps/desktop/ui/app.js` en modules ES natifs, un état par mode,
   une seule copie de l'état de lecture — fait.
6. Découper `crates/core/src/db.rs` par domaine (`crates/core/src/db/`) — fait.
7. Lama sans Ollama obligatoire — fait : modèle intégré (Gemma 4 E4B,
   llama.cpp lié, feature `lama-local`), qualité égale à Ollama sur
   MusicRecoIntent, 4,7 s contre 2,1 s (`docs/recherche-lama-sans-ollama.md`).
8. Préparer la publication de crates autonomes — fait
   (`docs/publication-crates.md`) : trois portages en Burn proposés
   (all-in-one, BS-RoFormer, MuScriptor). Décision : on garde les crates,
   sans publier.

## Chantier en cours : idées d'interface reprises de Disc Player (ouvert le 1ᵉʳ nov. 2026)

Des idées et des méthodes de [Disc Player](https://github.com/eudj1n/snowsky-disc-player)
(MIT), sans son code ni son cadre (Vue, Tailwind) ; citées dans `MODELES.md`.
Un commit par étape.

1. Palette de recherche (⌘K, « / ») au-dessus de l'écran courant ; `#q`
   devient un filtre qui estompe (Règle 2) — fait.
2. Thème clair atteignable (sombre / clair / système, sans flash) et contraste
   vérifié par `audit-interface.sh` (Règle 6) — fait.
3. Règle 11, typographie mesurée : jeu réduit de tailles et de graisses,
   rien sous 12 px, noms isolés pour le bidi et balisés `lang` — fait
   (21 tailles → 6 rôles et un décor ; 97 déclarations sous 12 px → 0).
4. Plan, sans coder : vue « En cours » plein écran, couleurs d'album, choix
   d'édition MusicBrainz face à face, état de la bibliothèque, passage
   Mac ↔ iPhone, captures sur collection fictive — fait :
   `plan-ecouter-v0.3.md` et `plan-ios-compagnon.md` (« Passage de la
   lecture »). Tranché le même jour : options recommandées partout ; couleurs
   d'album dans la seule vue « En cours » (exception écrite à la Règle 3) ;
   thème par défaut : sombre. Ordre : captures (faites, `docs/images/`), vue
   « En cours » et ses couleurs (faites, 1ᵉʳ nov.), bloc « État » (fait, et
   Bibliothèque repliée : Règles 5 et 8 ✅), éditions face à face (faites,
   pas encore essayées contre le vrai MusicBrainz), passage Mac → iPhone
   (option A faite le 2 nov. : « Emporter la lecture » dans Valise et ⌘K ;
   pas encore essayé sur l'iPhone). « Compléter » sous le bloc
   « État » (2 nov. : genres MusicBrainz puis tags Last.fm, journal en base,
   la dernière passe s'annule en bloc). Passage par le Wi-Fi (option B,
   3 nov.) : dans les deux sens, appairage par jeton déposé au câble, chaque
   pas accusé par l'autre côté ; pas encore essayé sur l'iPhone. Rien ne
   reste de ce chantier hors des essais sur l'appareil.

Relevé en route, corrigé sur décision : le filtre par famille d'Écouter
retirait des albums au lieu de les estomper (`interface-guidelines.md`,
Écouter, Règle 2).

## Chantier en cours : la balade musicale (ouvert le 5 nov. 2026)

Faire de la carte un terrain de promenade, pas seulement un plan
(`recherche-balade-musicale.md`, pistes et prompts dans `plan-balade.md`).

- P0, mesure de continuité — fait : hors album, une rue sonne presque comme
  le hasard ; P7 jugé prioritaire (`journal.md`, 5 nov.).
- P1, le promeneur — fait : « Flâner d'ici », file courte, reprise, repli
  dans le graphe des voisins.
- P7, sérier les cellules — essayé, sans effet net (−5 %, −9 % au mieux en
  renforçant le style) : une famille est trop large pour faire une
  progression. Reste un banc (`CR_SERIATION`), pas dans l'application.
- P2, le carrefour — fait : 2-3 voies à la fin du morceau, gestes 1/2/3,
  flèches, clic sur la rue, Échap ; se laisser porter / proposer / demander ;
  retour au carrefour précédent ; extrait de 5 s, son principal baissé. Un
  carrefour tous les quatre morceaux environ. À relever après une semaine :
  `balade-mesure.json` (choix faits, choix « ailleurs »).
- P3, les pentes — fait : la pente du carrefour mêle proximité CLAP,
  nouveauté (`ecoute`), déjà-visité, usure et relation MusicBrainz, en trois
  allures (rester / dériver / s'aventurer, dans le rail). Elle ne décide
  qu'aux carrefours, 1,6 fois par heure : sur une heure, dériver fait +4 points
  de jamais-écoutés sur une histoire simulée (rien à gagner sur l'histoire
  réelle, déjà neuve à 98 %). Flèches rouges entre quartiers voisins
  (216 sur Paris, 0 croisement), option « Pentes » du fond de plan ; les
  tuiles sont à régénérer pour les voir.
- P4, l'aura — fait : les trois voisins du curseur (à son arrêt) ou du
  promeneur jouent très bas, fondus en 300 ms, placés selon leur direction,
  repris où on les avait laissés dans les 30 s ; haut-parleur sur la carte
  (éteint hors balade, allumé en balade). Extraits décodés à la volée sur la
  fenêtre la plus représentative (21 ms pour 15 s, à froid), sans cache.
  L'extrait d'une voie au carrefour passe par l'aura, le son principal ne
  baisse plus. À lancer une fois : `rusty-music fenetres` (~45 min), sinon
  l'aura prend la fenêtre centrale des morceaux analysés avant.
- Restes, dans l'ordre : P5, P8, puis P6.

## Ce qui n'est pas prévu, et pourquoi

- **Mixage DJ de deux pistes** : sorti du périmètre du module 3 (une piste à
  la fois, `ui-spec-editeur.md`). La grille de battements le rendrait
  possible, mais rien n'est planifié.
- **Wikidata** pour les infos d'artiste : jamais implémenté, TheAudioDB le
  remplace (`enrichissement-lecteur.md`).
- **Deux morceaux hors carte** (un m4a, un mp3) : fichiers réellement
  corrompus.
