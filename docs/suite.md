# Où en est le projet, et ce qui reste

État au **19 nov. 2026**. Ce document dit où en est chaque brique et ce qui
reste, dans l'ordre ; le détail vit dans les documents cités. L'ancienne
version (plan d'août, dettes D1-D4, chantiers 1 à 9, presque tous faits) est
dans [`archives/suite-2026-08.md`](archives/suite-2026-08.md). Le plan en
cours, étape par étape : [`synthese-novembre-2026.md`](synthese-novembre-2026.md).
Ce qui a été fait et mesuré, au jour le jour : `journal.md`.

## Les briques

| Brique | État | Restes, et où les lire |
|---|---|---|
| Cœur d'ingestion | livré : scan, surveillance (`notify`), tags (`lofty`), base SQLite, décodage symphonia + Opus pur Rust ; `core` séparé en quatre crates le 7 nov. (`core`, `enrichissement`, `lama`, `valise` ; `gapless` passé dans `player`) : toucher Lama ou l'enrichissement recompile en 19 s au lieu de 96 | Opus décodé par `opus-pure` depuis le 8 nov. (G4) : ouverture d'un morceau de 2 à 6 min en 0,14 à 0,58 s au lieu de 8 à 38 s, échantillons identiques à libopus (l'ancien `opus-decoder` s'en écartait par pointes isolées, jusqu'à 0,17) ; sous 200 ms pour 4 morceaux sur 10 seulement, le lecteur décodant toute la piste avant de jouer. Mesuré le 8 nov. (V8) pour tous les formats : ~1,4 ms par seconde de son, 200 ms dépassé dès ~2 min 20 (MP3, M4A, FLAC, Opus), « E » ×3 ; les 5 premières secondes seules s'ouvrent en 6 à 9 ms : lecture progressive proposée, en attente d'un choix (« E » ne s'y prête pas à l'identique). Migrations versionnées depuis le 31 oct. : `db::schema::migrations`. Depuis le 8 nov. (V12), ce que la surveillance change en base est signalé à l'interface (`bibliotheque-modifiee`) : Écouter se met à jour sans relance, et dès la fin du scan d'« Analyser » ; reste à le voir dans l'app lancée |
| Enrichissement en ligne | livré : genres MusicBrainz (si adresse de contact), popularité ListenBrainz + Deezer, pochettes de repli (Cover Art Archive, Deezer), bios TheAudioDB, crédits Discogs (dumps), critiques CritiqueBrainz (liens Markdown rendus en liens, V33, 10 nov.), tags Last.fm (clé de l'utilisateur) ; vérification des pochettes locales | popularité et pochettes de repli coupables depuis V45 (19 nov. : deux cases des options de la chaîne, gardées d'un lancement à l'autre) ; les autres cases des sources (biographies, critiques, Discogs, Last.fm) reviennent cochées à chaque lancement (`enrichissement-lecteur.md`, `popularite.md`) |
| Module 1 — Écouter | livré, v0.1.0 ; plan 0.2 soldé (reprise de session, playlists, historique et favoris, « En cours de lecture » macOS, minuteur, enchaînement sans blanc) ; palette de recherche ⌘K (1ᵉʳ nov.) ; gérer les playlists (P1, 9 nov. : ajouter depuis une ligne, la file, l'inspecteur ou un album, retirer, réordonner, créer vide) | paroles et égaliseur écartés ; P1 : ajout depuis ⌘K et la case d'album de la grille, pas encore vu dans l'app installée ; P2 (enchaîner une playlist dans Pratiquer, 10 nov.) fait, en attente de l'essai de François ; batterie du Mac non lue pour le démixage d'avance ; un seul démixage à la fois (V31, 10 nov. : l'avance se met en pause devant un démixage lancé à la main, puis reprend) ; `plan-ecouter-v0.2.md` |
| Module 2 — Explorer | livré : carte de Paris (OpenStreetMap), croissance chronologique, ancrage des artistes populaires aux monuments, palettes de fond, itinéraires sur la voirie, familles nommées par vote, coloration tempo/énergie, Lama (texte → playlist, par parties ; modèle intégré Gemma 4 E4B ou Ollama au choix, phase 7) ; nuage stable (D5, 8 nov.) : la projection reprend les coordonnées en base et loge les nouveaux morceaux près de leurs voisins (déplacement médian des anciens 0,9 % contre 15 à 33 % en repartant de zéro) | nuage : reprise vérifiée sur une copie de la base de l'app (V3 : anciens déplacés de 0,66 % en médiane, puis « nuage inchangé ») ; le plafond de 13 % de conservation tenait au pas de t-SNE, pas à l'empreinte : un pas ×16 donne 27 % à temps égal et 25,7 % de pureté de genre (borne 512 d : 29,2 %), V10 : pas ×32 adopté le 8 nov. (29,3 % de conservation, 26,2 % de genre, 47,7 % de famille, même temps ; familles et qualité du plan inchangées) ; reprise D5 remesurée au nouveau pas : 0,47 % de déplacement médian, conservation 30,6 % ; la carte de l'app est refaite une fois, au premier « Recalculer la carte » (ou Scanner, Analyser), puis « Régénérer les tuiles » pour le plan (`journal.md`, « V3 », « V10 », « V10 (suite) » ; images dans `~/Exp/rusty_music-v10/`) ; Lama : raffinement multi-tours, fusion de graines, ordre lissé (`recherche-llm-playlist.md`) ; peuplement persisté et autocomplétion multi-type écartés pour l'instant |
| Module 3 — Pratiquer et Créer (ex-Éditer, deux modes du rail depuis le 5 nov.) | périmètre d'`ui-spec-editeur.md` couvert (démixage, vitesse, hauteur, réglage par stem, greffe calée sur les temps, export) ; *pratiquer* : pulsation Beat This!, boucle à la mesure, partition alphaTab, basse (Basic Pitch ou MuScriptor, tablature doigtée), batterie (ADTOF), sections (all-in-one) ; *créer* : greffe mesure par mesure, section sur section ; BS-RoFormer « SW » variante par défaut pour pratiquer ; stems presque vides (moins de −30 dB du mélange : le piano d'un trio rock) masqués dans la pile, toujours joués, montrés d'un geste (V38, 12 nov.) ; pile des stems nette à toute taille et densité (V39, 12 nov. : spectrogrammes suivis par `ResizeObserver` et `matchMedia`) ; temps estimé du démixage appris par variante sur la machine, corrigé par la progression (V40, 15 nov.) | `plan-editer-pratique-creation.md` : 1.1 autoroute de notes, 2.2 chercher le greffon (fait le 9 nov., F1 : lancé sur une copie de la vraie base, V23 ; poids du texte dans le préfiltre et éditions dédoublonnées, V26 ; reste à confronter la syncope à une vérité terrain), 2.3 COCOLA, 2.5-2.7 (LarsNet, GrooVAE, génération) ; publication des poids convertis sur Hugging Face (0.1, en partie faite) ; tonalité cible par stem (la mémoire par morceau est faite, E1 ; boucles nommées et vitesse atteinte à l'écran, V17 ; corrections de la basse transcrite et export MIDI / Guitar Pro, E2) |
| Interface (contrat et contrôles) | treize règles (`interface-guidelines.md`), dont les Règles 12 (une zone, un rôle) et 13 (le rail tient dans la fenêtre), écrites le 7 nov. ; table des écrans (`js/ecrans.js`) ; G8 (hauteur du rail) dans l'audit ; audit 288 cellules, 0 violation deux fois de suite le 10 nov. (V30 : un mot insécable des critiques coupé dans l'inspecteur ; G3 nomme désormais ce qui dépasse), G8 vert sur les 13 écrans ; fenêtre d'audit discrète depuis V32 (ni premier plan, ni focus, ni Dock ; `RUSTY_MUSIC_ESSAI_VISIBLE` pour la voir, d'office avec les captures) ; types générés de Rust (`tauri-specta`, en test seulement) pour 262 commandes sur 263 depuis V53 (20 nov. ; seule `tuile`, réponse binaire, y échappe), confrontés à `fixtures.js` par `verifier.sh` et la CI, champs en trop compris | le rail d'Explorer est plein (Règle 13, V6, 9 nov.) : un nouveau bloc doit en remplacer un ; G8 avertit sous 24 px de marge (Anneau 803 px pour 804) ; la palette ⌘K trouve les familles, leur nom entier se lit au survol et au clavier ; un G5 aléatoire vu une fois sur `ecoute:playlists` à 1 920 px (repère A–Z affiché), non reproduit, cause non trouvée ; une réponse figée qui rend l'état partagé (`e.balade`) est `any` et échappe au contrôle (`journal.md`, « V5 ») |
| Backend `burn/metal` | parité faite (31 oct.) : CLAP, all-in-one, BS-RoFormer, MuScriptor f32 exacts en MSL ; les trois HTDemucs exacts en MSL **et en WGSL** depuis V48 (19 nov. : deux noyaux faux de cubecl localisés et contournés dans le fork, `d851b07` ; WGSL, le backend livré, séparait à 16-40 dB de PyTorch) | décider la bascule de toute l'application en `burn/metal` (×1,2 à ×1,9, MuScriptor en f32) ; signaler les deux noyaux en amont (brouillon : `experiments/burn-metal/signalement-cubecl.md`) ; reprise : `experiments/burn-metal/README.md` |
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
  étapes (28 depuis V5, le 8 nov.) ; V4 (10 nov.) : `etabli.js` (2 032
  lignes) en trois modules, `etabli.js`, `etabli-reglages.js`,
  `etabli-stems.js` ; plus aucun fichier de `ui/js/` au-delà de 1 500
  lignes hors Lama et Bibliothèque (`ecouter.js` : 1 421).
- D, finir la balade — fait (7-8 nov.) : D2 (P8), D4 (P6, relief regardé
  dans l'application le 8 nov.), D5 (nuage stable). **En attente d'usage** :
  D1 (relever `balade-mesure.json` ; les 8 et 12 nov., 3 balades et
  0 carrefour montré, aucune balade depuis le 6 nov. ; à relever après une
  dizaine de balades d'une heure, ~40 carrefours) et D3 (apprendre la pente des choix aux
  carrefours, s'il y en a au moins 50).
- Le plan suit le nuage — V16 fait (9 nov.) : une projection refaite
  (« Recalculer la carte » après un changement de réglage, ou les
  empreintes d'un scan) régénère le plan de Paris en tâche de fond (6 s sur
  27 861 morceaux), l'avancement sous « Régénérer les tuiles » ; une
  reprise ou un nuage gardé, rien. Le bouton manuel reste.
- Outillage — V11 fait (8 nov.) : `verifier.sh` ne lie plus les exemples et
  le profil `dev` ne garde que les tables de lignes : `target/debug` 12 Gio
  après un passage (28 avant), 66 s en régime établi (231 avant) ; routine
  de nettoyage dans le README. L'arrêt de l'audit d'interface après
  30 cellules est compris et levé (V13, 8 nov.) : session verrouillée,
  WebKit suspendait la page ; fenêtre d'audit sans ralentissement de fond,
  252 cellules en 441 s écran verrouillé. La carte, elle, ne se dessine pas
  écran verrouillé (aucune image) : l'audit le dit, et avec captures il
  sort en code 3 (« reporté ») ; relief et captures demandent un lancement
  écran déverrouillé (`journal.md`, « V13 »). Depuis V19 (9 nov.),
  `scripts/ecran-visible.sh` (session déverrouillée et affichage allumé)
  décide du report, et une pose non lue en 8 s le dit au lieu d'attendre
  le délai ; reste un cas imprévisible : la fenêtre d'audit ouverte
  derrière ne reçoit aucune image (`journal.md`, « V19 »).
- Sécurité : RUSTSEC-2026-0459 (SQLite < 3.54) levé par V20 (9 nov.) :
  `rusqlite` 0.41 (SQLite 3.54.1) ; `rusqlite_migration` repris de
  crates.io en 2.7.0 le 20 nov. (la copie de `vendor/` retirée) ; Rust 1.95
  minimum (`journal.md`, « V20 »). Installé (I11, 20 nov.).
- Modèles de l'app installée — V22 fait (9 nov.) : ADTOF (batterie de
  Pratiquer) embarqué dans le paquet, seul modèle que rien ne téléchargeait ;
  inventaire et test dans `apps/desktop/src/paquet.rs`, `release.sh` refuse
  un paquet dont une ressource manque, et depuis un `.app` le message d'un
  poids absent dit de réinstaller au lieu de citer un script du dépôt.
  Installé (I9, 9 nov., 5bfd59f). V25 (9 nov.) : Basic Pitch, Beat This! (mel
  et petit) et les 8 plis d'all-in-one embarqués aussi (21 Mo), pour que
  Pratiquer transcrive hors ligne dès le premier usage ; le téléchargement
  reste le repli. Paquet construit, pas encore installé (I10).
- Restent :
  - E, Pratiquer : mémoire par morceau faite (E1, 9 nov. : migration
    n° 7, la séance revient à la réouverture ; boucles nommées et vitesse
    atteinte à l'écran, V17 ; reste à le voir dans l'app installée), corriger la
    transcription faite (E2, 9 nov. : corde, octave, note supprimée ou
    ajoutée, gardées et réappliquées ; export MIDI et Guitar Pro ; V18 :
    demi-ton sur une note transcrite, barre sur une ligne à 1 400 px ; pas
    encore vue dans l'app réelle), banc d'étirement (E3, 9 nov. : coûts mesurés — Signalsmith
    transpose un stem de 184 s en 1,0 s contre 17 s pour wsola, et les deux
    vocodeurs transposent en flux ; écoute à l'aveugle faite : décision mixte,
    wsola garde la vitesse, Signalsmith prend la transposition — E6 fait le
    9 nov. : en flux, le préréglage était faux dans le grave et le réglage
    juste dépassait le budget du rappel ; sur décision de François, la
    transposition reste hors ligne et en cache, mais par Signalsmith `exact`
    (bloc 8192, pas 224 : ≤ 2 cents, 8,4 s par stem de 224 s contre ≈ 20 s
    par wsola ; V28 : stems transposés en parallèle sur les cœurs de
    performance moins un, 4 stems en 9,5 s, 6 en 7,9 s) ; reste à l'entendre
    dans l'app), coach (E4 commencé le 9 nov. :
    horloge de pratique et calibration de latence stockée par périphérique
    et chemin, migration n° 10 ; tranche 2 le 10 nov. : batterie MIDI par
    `midir`, profils par périphérique (migration n° 11), apparieur en temps
    musical muet sur les notes incertaines, place perdue, geste
    « calibrer » sur le décompte et notes entourées sur la partition,
    testés sans matériel ; tranche 3 le 10 nov. : table par mesure et par
    passage gardée dans `pratique_mesure` (migration n° 12, pente des
    écarts), score lissé, « boucler » les pires mesures à 70 % et escalier
    de tempo proposés dans la barre du coach, jamais imposés ; E5,
    tranche 1 le 10 nov. : groove de la batterie MIDI — écart au stem par
    pièce, décalage du disque à la grille, swing du disque et du jeu,
    régularité de vélocité et accents contre l'activation d'ADTOF, conseils
    par motifs dans la ligne du coach, muets sous les seuils ; E5,
    tranche 2 le 10 nov. : analyse de la basse en DI (attaque par flux
    spectral, hauteur par pYIN causal ≈ 70 ms après, puis ≈ 60, fausses notes et
    octaves contrôlées par l'énergie, référence vérifiée sur le stem par le
    même analyseur), testée sur des prises synthétiques ; tranche 3 le
    10 nov. : hauteur ≈ 60 ms après l'attaque dès le mi grave (trame
    courte), entrée audio par cpal datée sur l'horloge commune, permission
    micro, « jouer avec ma basse » dans la barre du coach (fausses notes et
    octaves entourées, calibration sur une corde étouffée) ; V34 le
    10 nov. : tous les canaux de l'interface écoutés, celui de la basse
    trouvé à la calibration ou dès qu'elle joue (Scarlett Solo : entrée 2),
    choix à la main gardé par interface, barre du coach visible sans
    partition avec « transcrire la basse pour jouer avec » ; V35 le
    10 nov. : la basse jugée et sa tablature affichée à la hauteur
    transposée (« hors manche » sous le mi grave), code des erreurs en
    couleur et en signe (← → la note jouée, 8, ×, pointillé, + pour les
    coups en trop), bandeau du coach sur aplat d'accent avec sa légende ;
    V36 le 10 nov. : accordeur de basse au diapason dans le bandeau
    (même entrée et même pYIN que le coach, affiné au cent, La 440 ou
    l'accord du disque mesuré sur le stem) ; V37 le 12 nov. : la
    tablature écrite pour l'accordage du disque ne bouge plus avec la
    hauteur (V35 défait pour l'affichage, le coach juge toujours à la
    hauteur transposée), accordages limités à standard, ½ ton et 1 ton
    dessous, drop D, 5 cordes, standard d'abord, choix à la main gardé
    par morceau (migration n° 15), accordage du disque et accordage à
    utiliser dans le bandeau, visés par l'accordeur — à regarder par
    François sur About Damn Time (porte) ; E7 (15-16 nov.) : entendre sa
    basse avec le morceau — huit préréglages en Rust, retour entre deux
    horloges (anneau sans verrou, rééchantillonnage adaptatif, 0 trou sur
    10 min simulées), branché sur l'écoute du coach (brut pour l'analyse),
    haut-parleur dans le bandeau, volume, balance, réglages gardés
    (migration n° 17), micro signalé ; restent l'installation et l'essai
    de François sur ses enceintes (porte) ;
    V42 (18 nov.) : tonalité d'ensemble gardée par morceau (migration
    n° 18), vitesse et tonalité remises au neutre sur un morceau sans
    réglage gardé, playlist comprise ; entrée de la basse qui suit le
    défaut de macOS (choix à la main gardé tant qu'il ne bouge pas),
    choisissable dans l'accordeur et le retour ; V43 (19 nov.) : la
    partition gagne 42 px coach démarré, 98 accordeur ouvert (stems sur
    une ligne, réglages de l'entrée repliés dans le bandeau) ;
    V44 (19 nov.) : les pads de la batterie MIDI se règlent à l'écran
    (« pads » dans le repli des réglages : le pad frappé, sa pièce, General
    MIDI) ; audit d'interface complet le 19 nov. après V44 (288 cellules,
    0 violation, Pratiquer compris) ; reste l'essai réel avec la batterie et
    la basse de François) ;
  - F, Créer : chercher le greffon (F1 fait le 9 nov. : préfiltre CLAP
    guidé par le texte, tempo, tonalité, métrique, groove de la batterie,
    raisons par candidat ; tourné de bout en bout sur une copie de la vraie
    base le 9 nov., V23 : classement fidèle à la syncope, mais préfiltre qui ne
    proposait que des voisins rock d'une référence rock ; V26 : similarités
    réduites avant le mélange, part du texte 0,85, quatre funk ou soul dans
    les dix premiers au lieu de deux faux positifs, une seule édition par
    titre ; reste la vérité terrain de la syncope), bancs de
    compatibilité (F2, sans Stem-JEPA faute de poids : 20 sources ×
    10 batteries démixées et greffées, CLAP et groove classés le 10 nov.,
    même premier choix 10 fois sur 20 ; COCOLA par une réécriture à
    chargement sûr le 17 nov. : l'origine bat le hasard 20/20, premier choix
    commun avec CLAP 4 fois, avec le groove 1 fois ; 47 greffes rendues à
    l'aveugle, l'écoute de François reste) et de fûts, génération ;
  - G, bancs de modèles avant tout remplacement : super-résolution
    (G2, 10 nov. : AERO mesuré, il étend la bande sans toucher sous 16 kHz,
    24 s par minute ; 17 nov. : Apollo par chargement sûr, meilleur en LSD
    contre le FLAC, mais il resynthétise le signal, perd sous 16 kHz à
    192 kbit/s et demande le GPU, 21 s par minute sur Metal ; l'écoute AERO
    contre Apollo de François décide du port Burn), Chromaprint ; G4 (`opus-pure`) fait le 8 nov. Empreinte (G1)
    close le 9 nov. : **CLAP gardé**. MuQ-MuLan gagne 17 % de pureté de
    famille mais perd 2,5 % sur le rapport intra / inter, donc ne passe pas
    le critère ; CLaMP 3 abandonné sans mesure ; texte → audio non annoté.
    La carte n'est pas à régénérer ; SongFormer (G3) abandonné. Les
    fichiers du banc (31 Go avec les poids MuQ) sont mis de côté sur
    `RUSTY_DEV` dans `_a-effacer-V27` (V27, 9 nov.), vidés le 19 nov.

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
  courbes. Corrigé le 9 nov. (V9) : sur fond sombre, courbes beige clair
  et ombrage qui éclaircit (contraste du trait en Nuit 3,1 → 6,3) ; courbes
  masquées sous z13, l'ombrage gardé ; les six poses après sont prises
  (V19, `~/Exp/rusty_music-captures/v9-apres/`), à regarder
  (`journal.md`, « V9 », « V19 »). Réparé en route :
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
