# Mode Éditer — pratiquer un instrument, créer avec les stems : état de l'art

Recherche du 8 oct. 2026 (cinq axes : transcription basse, transcription
batterie, partition défilante, pulsation et structure, remplacement et
génération de stems), à partir de la demande : (1) **pratiquer** — couper un
stem et jouer avec le reste du groupe, partition de l'instrument qui défile ;
(2) **créer** — remplacer ou ajouter un stem (« une batterie plus funk, comme
dans tel morceau ») en respectant la structure d'origine. Outils cités au
départ : automashup, music-dissector et dj-structfreak (Taejun Kim),
beat_this, all-in-one. Le plan d'implémentation qui en découle est dans
`docs/plan-editer-pratique-creation.md`.

**Politique de licence appliquée** (`CLAUDE.md`, `docs/rust-audio-stack.md`) :
on adopte la licence de chaque outil et de chaque poids, NC comprise, et on s'y
conforme ; le code *lié* au binaire reste compatible GPL-3 ; les poids peuvent
être convertis pour Burn/`ort` et republiés sous leur licence d'origine quand
elle le permet ; tout est cité.

## Synthèse — bâtir la grille d'abord, transcrire et greffer ensuite

Pour les deux usages visés, le premier chantier est le même : **une grille temps/mesures fiable, puis une analyse en sections**. beat_this (CPJKU, code et poids MIT, déjà porté en Rust) fournit la grille dès maintenant. Pour les sections, all-in-one (mir-aidj, MIT) reste le meilleur rapport qualité/coût — il consomme nos stems HTDemucs et pèse 300 K paramètres — mais il faut le porter à la main en Burn ; SongFormer, plus précis, devient éligible et sert de référence.

Pour la pratique d'un instrument, la licence ne tranche plus : c'est **un banc mesuré sur nos propres stems HTDemucs** qui doit départager. Basse : Basic Pitch (minuscule, ONNX) en premier, MuScriptor (état de l'art 2026, portage ggml/Metal existant) comme palier de qualité — sous réserve de sa clause d'usage sur les droits. Batterie : ADTOF (entraîné sur 359 h de vraie musique) entre en concurrence directe avec ADT_STR (synthétique), et LarsNet permet de redécouper le stem batterie en fûts. L'affichage commence par une « autoroute » de notes, puis alphaTab piloté par le moteur Rust.

Pour la création, la politique de licence du projet ouvre le paysage : **MusicGen-Stem** (régénérer un stem à partir des autres) et **STAGE/DARC** (batterie d'accompagnement conditionnée par le mix) font exactement le geste visé. La greffe par recherche dans la bibliothèque reste la voie principale — rapide, contrôlable, propre à notre atout de 27 000 morceaux — et elle peut s'appuyer sur COCOLA (poids publiés) pour classer, et la génération devient un second outil réaliste, porté en Burn (décision du 8 oct. : pas de Python à l'exécution).

## La grille beat_this et les sections all-in-one portent les deux usages

Les deux usages consomment la même ossature temporelle. La partition qui défile a besoin des temps et des premiers temps de mesure (downbeats) pour quantifier les notes, découper en mesures et poser les boucles calées sur les barres. La greffe a besoin des mêmes downbeats pour étirer mesure par mesure plutôt que d'un bloc, et des sections pour envoyer le refrain de B sur le refrain de A. L'estimateur de tempo maison (Acc1 63 % sur GTZAN) ne fournit ni downbeat ni mesure. C'est la brique manquante numéro un.

**beat_this** (Foscarin, Schlüter, Widmer, ISMIR 2024) est le bon choix pour les temps et les downbeats. Le code et les poids publiés sont sous MIT ([GitHub CPJKU/beat_this](https://github.com/CPJKU/beat_this)). Sur GTZAN, il obtient un **F1 beat de 89,1 et un F1 downbeat de 78,3**, et sa petite variante (~2 M paramètres) reste à 88,8 / 77,2 ([arXiv 2407.21658](https://arxiv.org/html/2407.21658)). Il existe déjà une crate Rust `beat-this` 1.0.0 : runtime rten pur Rust par défaut, `ort` en option, décodage symphonia et rééchantillonnage rubato (deux briques déjà dans la pile du projet), **parité F = 1,0 avec Python** pour le modèle complet ([docs.rs beat-this](https://docs.rs/crate/beat-this/1.0.0)). Un export ONNX indépendant est aussi exact à 7e-5 près ([HF musetric/beat-this-onnx](https://huggingface.co/musetric/beat-this-onnx)).

Son point faible est assumé : il n'a pas de DBN de post-traitement, donc sa continuité métrique est plus basse (CMLt 79,8 contre 81,2 chez Hung et al.), et un DBN la remonte ([arXiv 2407.21658](https://arxiv.org/html/2407.21658)). Les modèles de madmom (CC BY-NC-SA 4.0, [GitHub CPJKU/madmom](https://github.com/CPJKU/madmom)) ne sont plus exclus, mais madmom est en Python/Cython : il faut de toute façon écrire en Rust un petit Viterbi tempo/phase de mesure, et madmom sert de **référence de parité** pour le valider. Le tempo se déduira de la médiane des intervalles entre temps. Reste à mesurer sur le banc GTZAN existant s'il corrige les erreurs d'octave de l'estimateur actuel.

**all-in-one** (Kim et Nam, WASPAA 2023) produit en une passe le tempo, les temps, les downbeats, les frontières et 10 labels fonctionnels (intro, verse, chorus, bridge…). Il consomme directement les 4 stems Demucs, donc nos stems HTDemucs sans second démixage. Avec environ **300 K paramètres**, il atteint sur Harmonix un F1 beat de 0,958, un F1 downbeat de 0,915 et une précision de labels (PWF) de 0,738 ([arXiv 2307.16425](https://arxiv.org/html/2307.16425)). Le code est sous MIT et la fiche HF des poids indique « License: mit » ([HF taejunkim/allinone](https://huggingface.co/taejunkim/allinone)). Deux obstacles techniques empêchent un simple export ONNX. Le code appelle exclusivement des noyaux NATTEN (`natten1dqkrpb`, `natten1dav`…), sans repli PyTorch ([dinat.py](https://raw.githubusercontent.com/mir-aidj/all-in-one/main/src/allin1/models/dinat.py)). Et les temps passent par le DBN madmom (même Viterbi Rust que ci-dessus). Vu la taille (dimension 24), réécrire en Burn l'attention de voisinage dilatée par gather + softmax est réaliste. Une version dense est exclue : environ 30 000 images pour 5 minutes à 100 FPS. Les poids `.pth` se convertissent en safetensors, sur le modèle de demucs-core.

**SongFormer** (ASLP-lab, 2025) devient éligible : il fait mieux qu'all-in-one sur Harmonix dans ses propres tableaux (précision de labels 0,807 contre 0,740) et va plus vite sur GPU ([GitHub SongFormer](https://github.com/ASLP-lab/SongFormer)). Mais il repose sur deux gros backbones auto-supervisés (dont MuQ, poids CC BY-NC 4.0) qui ne réutilisent pas nos stems et seraient lourds à porter. Il sert donc de **référence de qualité** pour juger le port d'all-in-one, et de recours (port Burn) si les labels d'all-in-one déçoivent hors pop. MSAF (MIT) ne donne que des frontières et des lettres A/B/C ([GitHub urinieto/msaf](https://github.com/urinieto/msaf)) : c'est le **repli sans apprentissage** (matrice d'auto-similarité sur CLAP et chroma par mesure, puis noyau de Foote). Music Dissector et DJ StructFreak, cités par l'utilisateur, sont des démos web construites sur all-in-one, sans code publié ([taejun.kim](https://taejun.kim/)). DJ StructFreak cherche dans un morceau B le segment qui correspond le mieux à un point choisi dans A, à l'aide d'embeddings de structure ([ISMIR 2023 LBD 328](https://ismir2023program.ismir.net/lbd_328.html)). C'est exactement le geste de l'usage 2. On peut le reproduire avec les embeddings par stem d'all-in-one (24 dimensions × 4 stems), agrégés par mesure.

## Basse et batterie : un banc sur nos stems départage

Aucun banc publié ne mesure ces modèles sur des stems sortis de Demucs. Maintenant que la licence n'élimine plus les meilleurs candidats, **le banc devient l'outil de décision** — sur le modèle du banc tempo GTZAN. Basse : Slakh2100 remixé puis séparé par notre HTDemucs, comparé au MIDI de référence. Batterie : MDB-Drums (et ENST) séparés de même. Mesure du F1 de note avec et sans offset (tolérance 50 ms), par classe pour la batterie, plus le temps de calcul sur Mac.

### Basse

Aucun modèle récent, ouvert et spécialisé basse n'existe : la recherche 2024-2026 sur la tablature porte sur la guitare. Une basse isolée par HTDemucs est cependant presque toujours monophonique, et c'est précisément le cas d'usage recommandé de **Basic Pitch** (« works best on one instrument at a time »). Code et modèle sous Apache-2.0, livré en ONNX, plage de fréquences réglable ([GitHub spotify/basic-pitch](https://github.com/spotify/basic-pitch)). Avec **moins de 17 000 paramètres et moins de 20 Mo de mémoire**, il tourne plus vite que le temps réel ([Spotify Engineering](https://engineering.atspotify.com/2022/6/meet-basic-pitch)). `ort` est déjà dans le dépôt pour `crates/superres`. C'est le premier candidat : coût d'intégration minimal.

**MuScriptor** (Kyutai, Mirelo, IRCAM, 2026) est le palier de qualité. Transformer décodeur seul de 103 M à 1,4 G paramètres, 36 groupes d'instruments dont la basse ; sur son propre test, Onset F1 60,4 contre 32,5 pour YourMT3+ ([HF MuScriptor](https://huggingface.co/MuScriptor/muscriptor-large)). Il tourne déjà en local via `muscriptor.cpp` (ggml/GGUF, **Metal**), utilisé par NeuralNote v2 ([GitHub NeuralNote](https://github.com/DamRsn/NeuralNote)) — seconde référence d'implémentation pour un port Burn (décidé le 8 oct. : on l'essaie). Ses poids sont CC BY-NC 4.0, **assortis de conditions supplémentaires** : interdiction de transcrire des œuvres sans en détenir les droits, indemnisation de Mirelo et Kyutai, accès conditionné au partage de coordonnées. Se conformer à cette licence veut dire : ne proposer MuScriptor qu'en option, conditions affichées, et laisser l'utilisateur juger s'il détient les droits sur ce qu'il transcrit. YourMT3+ (code GPL-3, poids Apache-2.0, Onset F1 basse **93,2** sur Slakh, 30 à 46 M paramètres) reste le troisième point de comparaison.

Le défaut principal à corriger, quel que soit le modèle, est l'erreur d'octave. Les auteurs de YourMT3+ ont constaté que la plupart des basses de Slakh sortaient une octave trop haut ([arXiv 2407.04822v3](https://arxiv.org/html/2407.04822v3)). Le travail en Rust tient donc moins au modèle qu'au post-traitement : port de `note_creation.py` (Basic Pitch), contrainte monophonique, vérification d'octave par une F0 de type pYIN, alignement des attaques sur la grille.

La tablature ne réclame aucun modèle appris. Pour une ligne monophonique, l'affectation corde/frette est un plus court chemin classique : programmation dynamique qui minimise le déplacement de la main ([arXiv 2510.10619](https://arxiv.org/html/2510.10619v1)). On l'écrit en quelques centaines de lignes de Rust. Le coût combine distance de frette pondérée par le temps écoulé, préférence pour les cordes à vide et les positions basses, et pénalité de changement de corde, avec l'accordage en paramètre (4 ou 5 cordes, drop D). L'utilisateur peut imposer la corde d'une note, et le Viterbi se recalcule sous cette contrainte, dans l'esprit « suggéré, jamais imposé » du projet. Les modèles neuronaux (Fretting-Transformer, TART) n'ont publié ni code ni poids. Ordre de grandeur : même l'état de l'art guitare plafonne à **56 % de Tab F1 de bout en bout** ([arXiv 2609.11904](https://arxiv.org/html/2609.11904v1)).

### Batterie

La batterie est plus difficile, et c'est là que l'éventail de modèles utilisables est le plus large. Trois candidats principaux, à départager au banc :

- **ADTOF** (Zehren, Alunno, Bientinesi) : CRNN entraîné sur **359 h de vraie musique** annotée par la communauté des jeux de rythme, 5 classes (BD, SD, TT, HH, CY+RD), variante PyTorch disponible ([GitHub ADTOF](https://github.com/MZehren/ADTOF)). Modèle image par image, donc **export ONNX trivial** et pas de boucle de décodage. Licence CC BY-NC-SA 4.0 pour tout le dépôt, code compris : on garde les poids, on réécrit l'inférence en Rust (un CRNN) plutôt que de lier leur code. Son entraînement sur de la vraie musique est un argument fort contre l'écart synthétique→réel documenté ([arXiv 2407.19823](https://arxiv.org/abs/2407.19823)).
- **ADT_STR** (Melucci et al., janvier 2026) : Transformer encodeur-décodeur à 26 classes avec **vélocité** (ghost notes), F1 0,79 sur MDB en mode batterie seule ([arXiv 2601.09520](https://arxiv.org/html/2601.09520v1)), poids safetensors CC BY-SA 4.0 chargeables par Burn ([GitHub ADT_STR](https://github.com/pier-maker92/ADT_STR)). Entraînement purement synthétique, fenêtres de 2,56 s à recoudre, boucle autorégressive à écrire en Rust.
- **YourMT3+** : F1 onset de 85,9 à 87,3 sur ENST avec accompagnement ([arXiv 2407.04822](https://arxiv.org/html/2407.04822)), plus lourd.

En complément, **LarsNet** (Politecnico di Milano, 2024 ; poids CC BY-NC 4.0, 562 Mo, plus rapide que le temps réel) redécoupe le stem batterie en **grosse caisse, caisse claire, toms, charleston, cymbales** ([GitHub larsnet](https://github.com/polimi-ispl/larsnet)). Riley & Dixon (2025) s'en servent pour passer ADTOF de 5 à 7 classes (crash et ride séparés) et estimer la vélocité ([arXiv 2509.24853](https://arxiv.org/abs/2509.24853)). LarsNet sert aussi l'usage 2 : remplacer seulement le charleston, ou mesurer le groove fût par fût. Omnizart et Magenta OaF Drums (poids sans licence déclarée) deviennent des replis légers. Partout, **les cymbales restent le point faible** (CY+RD à 0,49-0,52 pour ADT_STR ; principal problème non résolu selon l'analyse ISMIR 2024, [poster 68](https://ismir2024program.ismir.net/poster_68.html)) : l'interface doit les afficher avec une confiance moindre.

## Une seule horloge, une autoroute d'abord, alphaTab ensuite

Le principe de synchronisation est simple : **une seule horloge, la position dans l'audio d'origine, détenue par Rust**. Les notes sont exprimées en coordonnées de temps (interpolation linéaire par morceaux sur la grille beat_this), si bien que l'étirement WSOLA et les boucles ne touchent jamais la partition : seule la position rapportée change. Côté interface, on interpole avec `requestAnimationFrame` entre les événements Tauri et on retranche la latence du tampon cpal.

L'affichage par défaut d'une transcription automatique devrait être une **autoroute de notes** en canvas/WebGL : couloirs par corde avec numéros de frette pour la basse, couloirs par fût pour la batterie, défilement en temps, barres de mesure posées sur les downbeats. Elle n'exige ni quantification ni gravure. Elle se dégrade donc gracieusement, là où une portée transforme chaque erreur de transcription en rythme faux visible. Rocksmith procède ainsi, et Ubisoft reconnaît que la tablature gère mieux les techniques étendues et les motifs répétés ([Ubisoft Rocksmith](https://ubisoft.com/en-us/game/rocksmith/plus/news-updates/5qGm6PztsPk9IXFej8TdFD/rocksmith-reference-staff-notation)). Le modèle d'expérience à viser pour « je coupe ma partie et je joue par-dessus » est celui de Songsterr et de Soundslice : curseur sur la tablature, boucles calées sur les barres, ralenti, et accélération à chaque passage de boucle chez Soundslice ([Soundslice](https://www.soundslice.com/blog/199/introducing-enhanced-slowdown-and-perfect-looping/)).

La vue partition/tablature passe par **alphaTab** (MPL-2.0). C'est la seule bibliothèque qui réunit tablature, portée en clé de fa, notation de batterie (depuis la 1.4), défilement fluide et rendu paresseux ([GitHub alphaTab](https://github.com/CoderLine/alphaTab) ; [notes 1.8](https://alphatab.net/docs/releases/release1_8)). Depuis la 1.6, elle accepte un **lecteur externe** via `IExternalMediaHandler` (`play`, `pause`, `seekTo`, `playbackRate`), alimenté par `updatePosition(ms)` ([guide de synchro](https://alphatab.net/docs/guides/audio-video-sync)). Cette API correspond presque exactement à notre architecture : alphaTab n'émet aucun son, chaque commande devient un `invoke` vers le lecteur Rust. Le quantificateur Rust écrit directement de l'**alphaTex** (texte) avec un `\sync <mesure> 0 <ms>` par mesure, pris sur les downbeats ([sync points](https://alphatab.net/docs/alphatex/sync-points)). La synchronisation est ainsi exacte par construction, même quand le tempo dérive.

La quantification elle-même tient en peu de code. Chaque note est convertie en position de temps puis aimantée sur une grille de doubles croches ou de triolets (la règle `(4, 3)` de music21), avec une pénalité contre les faux triolets ([music21 via velog](https://velog.io/@clayryu328/Music211-Quantization)). Ensuite on découpe aux downbeats et on lie les notes qui franchissent une barre. Pour la batterie, on ajoute un mappage vers les articulations GP7 et deux voix (pieds hampes en bas, mains en haut). Les quantificateurs appris (PM2S, MIT, piano) restent une piste d'amélioration ([arXiv 2508.19262](https://arxiv.org/html/2508.19262v1)). Replis si alphaTab déçoit : OSMD (BSD-3), qui gère mal la batterie, ou Verovio (LGPLv3, crédit visible obligatoire, pas de lecteur).

## Greffer par recherche d'abord, générer en second outil

AutoMashup (ax-le, IMT Atlantique, GRETSI 2025, code BSD-3) valide la chaîne à suivre pour la greffe : Demucs, puis allin1 (temps, downbeats, sections, tonalité), alignement, étirement/transposition, et classement par COCOLA. Il en tire deux enseignements décisifs. La compatibilité est **asymétrique** : elle dépend de quel morceau fournit quel stem. Et **CLAP et MERT ne reproduisent pas la cohérence perceptive** mesurée par COCOLA ([arXiv 2508.06516](https://arxiv.org/abs/2508.06516) ; [GitHub ax-le/automashup](https://github.com/ax-le/automashup)). Classer des candidats par similarité CLAP seule est donc une erreur. CLAP doit rester un pré-filtre de style (k plus proches voisins, éventuellement guidés par un texte comme « funk drums, syncopated ») pour réduire les 27 000 morceaux à 50-200 candidats. Viennent ensuite des filtres par règles (rapport de tempo replié à l'octave, métrique, distance de tonalité pour les stems à hauteur définie), puis un classement.

Pour donner un sens mesurable à « plus funk », une étude d'écoute sur 248 patterns montre que **la syncope et la densité d'événements prédisent le groove ressenti, contrairement au microtiming** ([PLoS ONE 2018](https://pmc.ncbi.nlm.nih.gov/articles/PMC6025871/)). En v1, on calcule des approximations directement sur le stem batterie (ou sur les fûts LarsNet) : densité d'attaques, indice de syncope, ratio de swing. Dès que la transcription batterie existe, on passe aux mêmes descripteurs calculés sur les notes : cette brique sert aussi l'usage 1. Le classeur appris est **COCOLA** : code MIT, point de contrôle `COCOLA_HP_v1` publié (licence non déclarée, entraîné sur MoisesDB, Slakh2100 et CocoChorales), entrée 5 s mono à 16 kHz, scores harmonique et percussif séparés ([GitHub cocola](https://github.com/gladia-research-group/cocola)). **Plus besoin de ré-entraînement** : on l'exporte en ONNX pour `ort` et on note toujours stem → reste du mix, jamais symétriquement. Stem-JEPA (code LGPL-3.0) n'a toujours pas publié de poids ([GitHub Stem-JEPA](https://github.com/SonyCSLParis/Stem-JEPA)).

La greffe structurée découle directement du socle commun. Les sections de A et de B sont appariées par label all-in-one, ou par similarité d'embeddings à la manière de DJ StructFreak. Chaque mesure de B est étirée par WSOLA sur la mesure correspondante de A, plutôt que d'appliquer un étirement global au tempo cible comme le fait `greffer` aujourd'hui (`crates/editor/src/greffe.rs`). Le calage de tonalité (`demi_tons_rendu`) reste le chantier déjà identifié.

**Voie symbolique.** Transcrire la batterie, transformer le pattern, puis le rendre avec un kit d'échantillons : la structure est préservée par construction puisque le pattern vit sur la grille d'origine. Les points de contrôle **GrooVAE** de Magenta (`groovae_2bar_humanize`, `groovae_2bar_tap_fixed_velocity`, `groovae_4bar`… ; licence non déclarée) sont téléchargeables ([magenta music_vae](https://github.com/magenta/magenta/blob/main/magenta/models/music_vae/README.md)) ; c'est un petit LSTM-VAE, portable en Burn avec conversion des poids TensorFlow. Le dépôt Magenta est archivé depuis janvier 2026 : récupérer les points de contrôle tôt. Le ré-entraînement sur le Groove MIDI Dataset (CC BY 4.0) n'est plus qu'un plan B.

**Voie générative audio** :

- **MusicGen-Stem** (Meta, [arXiv 2501.01757](https://arxiv.org/html/2501.01757v1)) génère basse, batterie et « autre », et sait **régénérer un stem étant donné les autres** — c'est exactement « refaire la batterie de ce morceau ». Poids CC BY-NC 4.0 ([HF musicgen-stem-6cb](https://huggingface.co/facebook/musicgen-stem-6cb/blob/main/README.md)). Pas de conditionnement par un audio de référence documenté : le style passe par le texte.
- **STAGE** (2025, MusicGen-Small affiné, ~0,4 B paramètres, code et poids sur GitHub/Drive) génère une **batterie d'accompagnement conditionnée par le mix** et/ou une piste de pulsation ([arXiv 2504.05690](https://arxiv.org/html/2504.05690v2) ; [GitHub stage](https://github.com/giorgioskij/stage)). **DARC** (janvier 2026) y ajoute un contrôle rythmique fin par beatbox ou tapotement ([arXiv 2601.02357](https://arxiv.org/html/2601.02357)) — un « fais-moi cette batterie, mais avec ce rythme » très proche du besoin.
- **ACE-Step 1.5** (MIT) : tâche « lego » qui génère une piste isolée suivant le morceau source, avec **audio de référence pour le style** — le seul à prendre directement « comme ce morceau-là ». DiT 2B, modèle de base à ~50 pas, moins de 4 Go de VRAM, MPS/MLX, portage C++ du lego existant ([GitHub ACE-Step-1.5](https://github.com/ace-step/ACE-Step-1.5) ; [PR qvac](https://github.com/tetherto/qvac-fabric-speech.cpp/pull/164)). Isolation pas toujours « sèche » ([PR song-maker #369](https://github.com/azerothl/song-maker/pull/369)).
- **JASCO** (Meta, poids CC BY-NC 4.0) : conditionnement par accords, batterie, mélodie, mais sorties de 10 s ([HF jasco](https://huggingface.co/facebook/jasco-chords-drums-melody-1B)) — moins adapté. **Stable Audio Open Small** (341 M, tourne sur CPU Arm ; Stability Community License, gratuite sous 1 M$ de revenus) et SA-ControlNet sont utilisables pour générer des boucles ou des sons. **RAVE** (IRCAM, CC BY-NC 4.0, export TorchScript) permet le **transfert de timbre** : garder le jeu de la batterie d'origine en lui donnant le son d'un autre kit ([GitHub RAVE](https://github.com/acids-ircam/RAVE)) — une troisième manière, distincte de la greffe et de la génération.
- Toujours non publiés : StemGen, Diff-A-Riff, SingSong.

*(Décision du 8 oct. 2026 : pas de Python à l'exécution — ces modèles seront portés en Burn, voir le chantier 2.7 du plan ; MusicGen-Stem, STAGE et DARC partagent l'architecture MusicGen, un seul port les couvre. Le paragraphe suivant est l'analyse d'origine.)* Tous ces modèles sont en Python/PyTorch et lourds à porter en Burn. La forme d'intégration la plus rapide serait un **processus externe facultatif** (environnement Python préparé par un script, comme les modèles actuels), appelé par l'éditeur avec les stems et la grille, et dont la sortie repasse par le même alignement mesure par mesure que la greffe. Le choix entre MusicGen-Stem/STAGE et ACE-Step se fait à l'écoute sur une dizaine de morceaux de la bibliothèque.

## Feuille de route

Détaillée, avec l'état du code et les efforts, dans
`docs/plan-editer-pratique-creation.md`. En bref : socle commun (grille
beat_this, boucles par mesure, modèles et remerciements) → **banc** de
transcription sur nos stems → basse (Basic Pitch + tablature) et greffe
mesure par mesure → batterie (ADTOF ou ADT_STR, LarsNet) → partition alphaTab
→ sections (port d'all-in-one) et COCOLA → batterie symbolique (GrooVAE) et
génération en outil externe (MusicGen-Stem, STAGE/DARC, ACE-Step, RAVE).

## Licences du code et des poids

Colonne « Republier converti » : peut-on mettre les poids convertis (safetensors,
ONNX) en release asset ou sur Hugging Face ? Oui sous la licence d'origine quand
elle l'autorise ; **non** quand aucune licence n'est déclarée (télécharger à la
source et convertir localement, ou demander l'accord des auteurs).

| Brique | Code | Poids | Republier converti | Usage retenu |
|---|---|---|---|---|
| beat_this (CPJKU) | MIT | MIT | oui (MIT) | grille commune |
| crate `beat-this` (danigb) | MIT selon docs.rs, NOASSERTION selon GitHub | ONNX dérivés | — | code **lié** : LICENSE à lire |
| all-in-one (mir-aidj) | MIT | MIT (fiche HF) | oui (MIT) | sections, port Burn |
| madmom | BSD | CC BY-NC-SA 4.0 | oui (BY-NC-SA) | référence de parité du DBN |
| SongFormer | CC BY 4.0 | MuQ CC BY-NC 4.0 | oui (BY-NC) | référence de qualité, recours externe |
| MSAF | MIT | — | — | repli sans apprentissage |
| Basic Pitch | Apache-2.0 | Apache-2.0 | oui | basse, défaut |
| MuScriptor | MIT | CC BY-NC 4.0 + conditions (droits, indemnisation, accès sur formulaire) | à vérifier (conditions d'accès) | basse, option qualité, conditions affichées |
| YourMT3+ | GPL-3.0 | Apache-2.0 | oui | candidat au banc |
| Omnizart, MT3 | MIT / Apache-2.0 | non précisés | non | replis |
| ADTOF | CC BY-NC-SA 4.0 | idem | oui (BY-NC-SA) | batterie ; inférence réécrite (code NC non lié) |
| ADT_STR | CC BY-SA 4.0 | CC BY-SA 4.0 (README) | oui (BY-SA) | candidat batterie |
| LarsNet | non précisé | CC BY-NC 4.0 | oui (BY-NC) | découpe du stem batterie en fûts |
| Magenta OaF Drums | Apache-2.0 | non précisé | non | repli léger |
| DrumSep (MDX23C) | MIT | non documenté, dépôt disparu | non | introuvable |
| alphaTab | MPL-2.0 | — | — | partition (paquet npm, hors `cargo deny`) |
| VexFlow / OSMD / Verovio | MIT / BSD-3 / LGPLv3 | — | — | replis |
| AutoMashup (ax-le) | BSD-3 | — | — | référence de méthode |
| COCOLA | MIT | non déclarée (MoisesDB…) | non | classement des greffons |
| Stem-JEPA | LGPL-3.0 | non publiés | — | inutilisable en l'état |
| GrooVAE (Magenta) | Apache-2.0 | non déclarée | non | batterie symbolique |
| Groove MIDI Dataset / E-GMD | — | données CC BY 4.0 | — | plan B d'entraînement |
| MusicGen-Stem, JASCO | MIT | CC BY-NC 4.0 | oui (BY-NC) | génération, port Burn |
| STAGE, DARC | non précisé | non précisés (dérivés MusicGen) | non | génération, port Burn |
| ACE-Step 1.5 | MIT | MIT | oui | génération par référence, port Burn |
| Stable Audio Open / Small / 3.0 | — | Stability Community License | selon licence | boucles, sons |
| RAVE | CC BY-NC 4.0 | idem | oui (BY-NC) | transfert de timbre, processus séparé (code NC) |
| Magenta RealTime | Apache-2.0 | CC BY 4.0 | oui | inadapté (n'accompagne pas de stems) |

Deux conséquences de conformité à retenir : ADTOF et RAVE ont un **code** NC —
on utilise leurs poids sans lier leur code (inférence réécrite en Rust pour
ADTOF, processus séparé pour RAVE) ; et les poids sans licence déclarée
(COCOLA, GrooVAE, STAGE, OaF) ne se republient pas, ils se préparent
localement à partir de la source.

## Points restant à vérifier

| Point | Pourquoi | Comment |
|---|---|---|
| ~~Clause d'usage de MuScriptor~~ | **Tranché le 8 oct.** : on l'essaie, conditions affichées | — |
| LICENSE du dépôt `beat-this-rs` | Code lié au binaire ; docs.rs dit MIT, GitHub NOASSERTION | Lire le fichier avant de vendoriser |
| Import `burn-onnx` de beat_this (opset 17, rotary) | Profiter de Metal comme pour CLAP | Essai local ; `ort`/rten en attendant |
| Temps d'inférence (beat_this, Basic Pitch, MuScriptor, ADTOF, ADT_STR, LarsNet, all-in-one) sur Mac | Aucun chiffre publié | Banc de l'étape 2 |
| Entrées/sorties de `nmp.onnx` (CQT intégrée ? plage depuis La0 ?) | Prétraitement Rust, 5 cordes | Inspection du graphe |
| Disponibilité des poids ADTOF (PyTorch) et données sur demande (Zenodo) | Conditionne le banc batterie | Dépôt GitHub, contact auteurs |
| Récupération des points de contrôle GrooVAE et COCOLA | Magenta archivé, COCOLA sur Google Drive : risque de disparition | Télécharger et archiver sur Garage tôt |
| Parité du port all-in-one (padding et biais relatif de NATTEN) | Le port n'est pas testé | Lecture du code NATTEN, comparaison avec Python |
| En-tête MPL-2.0 d'alphaTab (« Incompatible With Secondary Licenses » ?) | Compatibilité GPL du code embarqué | Lire le LICENSE |
| alphaTab en WKWebView : lecteur externe, boucles, taille, polices hors ligne | Rien n'est testé dans Tauri | Prototype jetable |
| Syntaxe alphaTex d'accordage basse 4/5 cordes | Non vérifiée | Documentation alphaTex |
| Précision des labels all-in-one hors pop | Entraîné sur Harmonix | Écoute sur un échantillon ; comparaison SongFormer |
| MusicGen-Stem : taille, durée max, conditionnement par audio de référence | Non documentés dans la recherche | Fiche HF, essai |
| Qualité d'ACE-Step lego, STAGE, MusicGen-Stem sur nos morceaux | Aucune mesure indépendante | Écoute comparée hors moteur |
| Fidélité des descripteurs de groove calculés sur l'audio | Définis symboliquement dans la littérature | Comparaison avec la transcription batterie |
| `MODELES.md` + écran « À propos » | Remerciements et liens, condition de la politique de licence | À créer avec le premier poids tiers ajouté |

## Conclusion

La recherche déplace le centre de gravité du mode Éditer. La difficulté n'est pas dans les modèles spectaculaires, transcription et génération : elle est dans **l'analyse métrique et structurelle**, que les deux usages consomment. Une fois la grille et les sections en place, la transcription est surtout du post-traitement Rust autour d'un réseau, et la « greffe qui respecte la structure » devient un problème d'appariement de mesures et de sections.

Adopter les licences plutôt que trier par licence change deux choses. Pour la pratique, il remplace un choix dicté par la licence par un **choix mesuré** : le banc sur nos stems HTDemucs devient l'étape décisive, avec ADTOF et MuScriptor comme concurrents sérieux. Pour la création, il supprime la seule décision de principe qui restait ouverte (plus besoin de ré-entraîner COCOLA ni GrooVAE) et rend la génération audio réaliste — MusicGen-Stem, STAGE/DARC, ACE-Step — en outil externe facultatif à côté de la greffe par recherche. La greffe reste néanmoins la voie principale : elle est rapide, contrôlable, et exploite ce que les modèles génératifs n'ont pas, 27 000 morceaux réels déjà séparés et indexés.
