# Où en est le projet, et ce qui reste

État au **8 nov. 2026**. Ce document dit où en est chaque brique et ce qui
reste, dans l'ordre ; le détail vit dans les documents cités. L'ancienne
version (plan d'août, dettes D1-D4, chantiers 1 à 9, presque tous faits) est
dans [`archives/suite-2026-08.md`](archives/suite-2026-08.md). Le plan en
cours, étape par étape : [`synthese-novembre-2026.md`](synthese-novembre-2026.md).
Ce qui a été fait et mesuré, au jour le jour : `journal.md`.

## Les briques

| Brique | État | Restes, et où les lire |
|---|---|---|
| Cœur d'ingestion | livré : scan, surveillance (`notify`), tags (`lofty`), base SQLite, décodage symphonia + Opus pur Rust ; `core` séparé en quatre crates le 7 nov. (`core`, `enrichissement`, `lama`, `valise` ; `gapless` passé dans `player`) : toucher Lama ou l'enrichissement recompile en 19 s au lieu de 96 | décodage Opus lent : `opus-decoder` met 11 à 13 s pour ouvrir un morceau Opus de 2 min de la bibliothèque (Fingathing, 10 morceaux) ; `opus-pure` décode 15 s en 10,5 ms contre 787 ms (relevé par le banc de P4, `journal.md`, 6 nov.) ; à remplacer (G4 de la synthèse). Migrations versionnées depuis le 31 oct. : `db::schema::migrations` |
| Enrichissement en ligne | livré : genres MusicBrainz (si adresse de contact), popularité ListenBrainz + Deezer, pochettes de repli (Cover Art Archive, Deezer), bios TheAudioDB, crédits Discogs (dumps), critiques CritiqueBrainz, tags Last.fm (clé de l'utilisateur) ; vérification des pochettes locales | la popularité et les pochettes de repli n'ont pas de case pour les désactiver (`enrichissement-lecteur.md`, `popularite.md`) |
| Module 1 — Écouter | livré, v0.1.0 ; plan 0.2 soldé (reprise de session, playlists, historique et favoris, « En cours de lecture » macOS, minuteur, enchaînement sans blanc) ; palette de recherche ⌘K (1ᵉʳ nov.) | paroles et égaliseur écartés ; `plan-ecouter-v0.2.md` |
| Module 2 — Explorer | livré : carte de Paris (OpenStreetMap), croissance chronologique, ancrage des artistes populaires aux monuments, palettes de fond, itinéraires sur la voirie, familles nommées par vote, coloration tempo/énergie, Lama (texte → playlist, par parties ; modèle intégré Gemma 4 E4B ou Ollama au choix, phase 7) ; nuage stable (D5, 8 nov.) : la projection reprend les coordonnées en base et loge les nouveaux morceaux près de leurs voisins (déplacement médian des anciens 0,9 % contre 15 à 33 % en repartant de zéro) | nuage : la première reprise sur la base de l'app viendra avec la prochaine analyse ; une échelle de départ quatre fois plus grande porte la conservation des voisins à 24,7 %, à creuser avant G1 (`journal.md`, « D5 ») ; Lama : raffinement multi-tours, fusion de graines, ordre lissé (`recherche-llm-playlist.md`) ; peuplement persisté et autocomplétion multi-type écartés pour l'instant |
| Module 3 — Pratiquer et Créer (ex-Éditer, deux modes du rail depuis le 5 nov.) | périmètre d'`ui-spec-editeur.md` couvert (démixage, vitesse, hauteur, réglage par stem, greffe calée sur les temps, export) ; *pratiquer* : pulsation Beat This!, boucle à la mesure, partition alphaTab, basse (Basic Pitch ou MuScriptor, tablature doigtée), batterie (ADTOF), sections (all-in-one) ; *créer* : greffe mesure par mesure, section sur section ; BS-RoFormer « SW » variante par défaut pour pratiquer | `plan-editer-pratique-creation.md` : 1.1 autoroute de notes, 1.5 corriger la transcription, 2.2 chercher le greffon, 2.3 COCOLA, 2.5-2.7 (LarsNet, GrooVAE, génération) ; publication des poids convertis sur Hugging Face (0.1, en partie faite) ; tonalité cible par stem ; ce que Pratiquer retient d'une séance à l'autre (`ui-spec-editeur.md`, questions ouvertes) |
| Interface (contrat et contrôles) | treize règles (`interface-guidelines.md`), dont les Règles 12 (une zone, un rôle) et 13 (le rail tient dans la fenêtre), écrites le 7 nov. ; table des écrans (`js/ecrans.js`) ; G8 (hauteur du rail) dans l'audit ; audit 252 cellules, 0 violation, G8 vert sur les 13 écrans ; types générés de Rust (`tauri-specta`, en test seulement) pour 5 commandes, confrontés à `fixtures.js` par `verifier.sh` et la CI | l'Anneau tient au ras (803 px pour 804) : un bloc de plus dans son rail le ferait déborder ; les 206 autres commandes à passer aux types générés, par étapes (`ui-tests/README.md`) ; un champ que Rust a retiré mais que `fixtures.js` garde n'est pas vu (`journal.md`, « C4 ») |
| Backend `burn/metal` | parité faite (31 oct.) : CLAP, all-in-one, BS-RoFormer, MuScriptor f32 exacts en MSL ; htdemucs et htdemucs_ft aussi | htdemucs_6s faux en MSL, à localiser dans `demucs-core` avant de basculer toute l'application ; reprise : `experiments/burn-metal/README.md` |
| Super-résolution « HD » | livrée (`crates/superres`, AERO via ONNX Runtime) | — (`module3-superresolution.md`) |
| Compagnon iOS « Rusty Valise » | tourne sur l'iPhone (18 oct.) ; mode Valise du bureau (19 oct.) ; file et reprise (22 oct.) ; passage Mac → iPhone par fichier (2 nov.) ; passage dans les deux sens et valise par différence, par le Wi-Fi, appairage au câble (3 nov.) | étapes 1 (découper le code), 3 (sélection, en partie), 7 (rotation) ; essais sur l'iPhone (passages, valise par le Wi-Fi) ; mesures de batterie ; `plan-ios-compagnon.md` |

## Chantier en cours : synthèse de novembre (ouvert le 7 nov. 2026)

Le point du 7 nov. sur tous les chantiers, et son plan en sept phases
([`synthese-novembre-2026.md`](synthese-novembre-2026.md), § 5 ; tableau
« Avancement » étape par étape, prompts au § 6).

- A, consolider — fait (7 nov.) : séparation Pratiquer/Créer, P5, P6, P8,
  chacun son commit ; `interface-guidelines.md` remis au code (six modes).
- B, l'interface — fait (7 nov.) : six défauts corrigés (I1-I5, I10), chacun
  son test de fumée ; table des écrans (`js/ecrans.js`, une seule fonction
  écrit `hidden`) ; Règles 12 et 13 et leur contrôle G8 (`audit.js`) ; rail
  désencombré (outils posés sur la carte, réglages de la balade dans son
  bandeau, aides derrière « ? ») : G8 vert sur les 13 écrans.
- C, ranger le code sans changement visible — fait (7 nov.) : `maplibre.js`,
  `editer.js` et `pratiquer.js` découpés (`explorer/`, `etabli.js`,
  `pratiquer.js`, `creer.js`), `editer.rs` et `bibliotheque.rs` aussi ;
  `core` en quatre crates ; types générés pour 5 commandes, adoptés par
  étapes.
- D, finir la balade — fait (7-8 nov.) : D2 (P8), D4 (P6, relief regardé
  dans l'application le 8 nov.), D5 (nuage stable). **En attente d'usage** :
  D1 (relever `balade-mesure.json` ; le 8 nov., 3 balades et 0 carrefour
  quitté, rien à relever) et D3 (apprendre la pente des choix aux
  carrefours, s'il y en a au moins 50).
- Restent :
  - E, Pratiquer : mémoire par morceau, corriger la transcription, banc
    d'étirement (WSOLA contre Signalsmith Stretch et Rubber Band), coach ;
  - F, Créer : chercher le greffon, bancs de compatibilité (COCOLA contre
    Stem-JEPA) et de fûts, génération ;
  - G, bancs de modèles avant tout remplacement : empreinte (MuQ-MuLan,
    CLaMP 3), super-résolution (Apollo), `opus-pure`, Chromaprint. G1 tôt :
    s'il retient une autre empreinte, la carte est à régénérer, D3 et D5 à
    remesurer.

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
- P5, traces et brouillard — fait : chaque pas de balade est gardé en base
  (migration n° 5, `balade`, `balade_pas`) ; options « Brouillard » et
  « Sentiers battus » du fond de plan (éteintes par défaut : 98,5 % de la
  ville est sous le brouillard) ; le voile se lève à chaque écoute par
  `feature-state`, sans régénérer ; compteur de quartier au survol ; balades
  passées dans le rail, redessinées d'un clic, rejouées par ▶.
- P6, le relief d'énergie — fait : option « Relief » du fond de plan
  (énergie, tempo, popularité, familiarité), `raster-dem` ombré par
  MapLibre et courbes de niveau numérotées, tuiles écrites à la demande
  (0,13 s) sans régénérer le plan ; une courbe se désigne d'un clic, se suit
  à la molette (l'aura suit), s'ouvre par son numéro ; allures *à niveau*,
  *gravir*, *redescendre*. *À niveau* : écart-type d'énergie −31 % contre
  *dériver* (0,116 contre 0,167) ; gravir et redescendre, effet faible.
  Regardé dans l'application le 8 nov. (D4) : l'audit voit le relief et
  pose la carte à z12, z14, z16 en OSM clair et en Nuit
  (`RUSTY_AUDIT_CAPTURES`). En OSM clair, le plan reste lisible sous les
  courbes. Pas corrigé, à décider : en Nuit, le relief est presque
  invisible (courbes et ombrage bruns sur le noir) ; à z12, les courbes
  s'empilent en anneau au bord de la zone peuplée. Réparé en route :
  changer de fond ne rejouait plus le relief, les couleurs, le filtre, les
  pentes ni les traces (`journal.md`, « D4 »).
- P8, des lieux à soi — fait : épingles (clic droit « Épingler ici », nom,
  teinte, emoji, « Flâner d'ici » depuis l'épingle) et chaînes (une balade
  passée ou un itinéraire gardé : une playlist plus son tracé, rejouable).
  Elles sont en lon/lat et par chemin, et survivent aux tuiles, aux
  réanalyses et aux rescans. Elles suivront les playlists dans la Valise
  quand la règle « Playlist » y viendra.
- Restes : P9 (facultatif, après un usage réel du carrefour) ; avant lui,
  D1 et D3 de la synthèse, qui attendent que la balade ait servi ; les
  allures du relief dans une vraie balade.

## Ce qui n'est pas prévu, et pourquoi

- **Mixage DJ de deux pistes** : sorti du périmètre du module 3 (une piste à
  la fois, `ui-spec-editeur.md`). La grille de battements le rendrait
  possible, mais rien n'est planifié.
- **Wikidata** pour les infos d'artiste : jamais implémenté, TheAudioDB le
  remplace (`enrichissement-lecteur.md`).
- **Deux morceaux hors carte** (un m4a, un mp3) : fichiers réellement
  corrompus.
