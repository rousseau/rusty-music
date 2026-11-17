# Banc de super-résolution : AERO contre Apollo (G2)

Hors workspace. Fichiers lourds (venv, extraits, sorties, écoute) sur
`$RUSTY_DEV/banc-superres/` (`/Volumes/LaCieDock/rusty_music-dev/`).

## Montage

- `src/main.rs` : `banc-superres <modele.onnx> (entrée sortie)+`, passe
  chaque WAV par `rusty_music_superres` (le code de l'app, bouton « HD »)
  et écrit une ligne JSON `{entree, secondes, coupure_hz}`.
  `CARGO_TARGET_DIR=$RUSTY_DEV/banc-superres/target cargo build --release`.
- `banc.py <etape> <racine> …` (venv du banc : torch 2.11, onnxruntime,
  soundfile, lameenc) :
  - `preparer <copie de la base>` : 30 s (à partir du quart) de 20 MP3 de la
    bibliothèque (`selection.json` : 7 × 128, 6 × 160, 7 × 192 kbit/s,
    artistes distincts, graine 20261110) ; la bibliothèque n'a aucun couple
    MP3/FLAC du même morceau (22 FLAC sur 27 523 morceaux), donc 6 FLAC tirés
    au hasard sont encodés ici par LAME (128 et 192 kbit/s CBR) : la source
    sert de référence ;
  - `apollo <mps|cpu> [noms]` : Apollo (`JusperLee/Apollo`, cloné dans
    `apollo-src/`), morceaux de 10 s fondus sur 1 s ;
  - `mesurer` : coupure (−60 dB du pic du spectre moyen), énergie au-dessus
    de 16 kHz ; contre la référence, après alignement (délai du codeur) :
    LSD pleine bande, au-dessus et au-dessous de 16 kHz, SI-SDR ;
  - `ecoute` : 10 extraits de la bibliothèque, chaque version au même RMS,
    lettres tirées au hasard dans `ecoute/NN-X.wav` ; clé dans
    `ecoute-cle.json`, à ne pas ouvrir avant d'écouter.
- Piège : sur le LaCie (exFAT), libsndfile refuse d'ouvrir les MP3
  (« bad data offset ») qu'il lit sur l'APFS ; le banc décode en mémoire.

ViSQOL n'est pas installé (ni paquet ni binaire sur la machine) : mesures
spectrales et écoute à l'aveugle à la place.

## Résultats (10 nov. 2026)

**Apollo, 10 nov.** : son exécution (code cloné, poids en pickle) a été
refusée. **17 nov.** : par chargement sûr (décision du 10 nov.),
`apollo_sur.py` — réseau réécrit d'après le dépôt (CC BY-SA 4.0, commit
e84bcac), poids `pytorch_model.bin` lus par `torch.load(weights_only=True)`,
les globales de configuration (`omegaconf`, `typing.Any`, `TorchVersion`,
`defaultdict`) remplacées par une classe inerte ; chargement strict,
654 tenseurs, 16,54 M paramètres ; convertis en
`$RUSTY_DEV/banc-superres/poids/apollo.safetensors`.

    P=$B/venv/bin/python
    $P -I apollo_sur.py charger $B/poids/pytorch_model.bin $B/poids/apollo.safetensors
    $P -I apollo_sur.py traiter $B $B/poids/apollo.safetensors mps
    $P -I banc.py mesurer $B
    $P -I apollo_sur.py ecoute $B        # AERO contre Apollo, à l'aveugle

Licence des poids : la fiche Hugging Face `JusperLee/Apollo` déclare
**CC BY-SA 4.0** (entraînés sur MUSDB18) — la synthèse disait « non dite ».
Republiables convertis, sous la même licence, en citant.

AERO contre le MP3, 6 FLAC encodés (moyennes) :

| débit | LSD MP3 → AERO | LSD > 16 kHz | LSD < 16 kHz | SI-SDR (dB) | énergie > 16 kHz (réf. −44,4 dB) | coupure |
|---|---|---|---|---|---|---|
| 128 | 21,0 → 10,3 | 38,0 → 15,8 | 7,17 → 7,10 | 24,91 → 24,87 | −57,7 → −47,7 | 15,7 → 17,9 kHz |
| 192 | 18,0 → 11,1 | 33,8 → 20,3 | 3,69 → 3,69 | 29,86 → 29,76 | −50,0 → −45,6 | 16,6 → 18,6 kHz |

Apollo (17 nov., mêmes extraits) :

| débit | LSD AERO → Apollo | LSD > 16 kHz | LSD < 16 kHz (MP3 / Apollo) | SI-SDR Apollo | énergie > 16 kHz | coupure |
|---|---|---|---|---|---|---|
| 128 | 10,3 → 7,9 | 15,8 → 11,1 | 7,17 / 6,12 | 10,7 | −46,5 | 17,2 kHz |
| 192 | 11,1 → 7,5 | 20,3 → 11,8 | 3,69 / 4,79 | 13,3 | −47,6 | 17,2 kHz |

Apollo bat AERO sur la LSD pleine bande (11 extraits sur 12) et au-dessus
de 16 kHz (10 sur 12) ; sous 16 kHz il **améliore** le 128 kbit/s (6 sur 6)
mais **dégrade** le 192 (0 sur 6). Il ne garde pas la forme d'onde : SI-SDR
contre la source 10 à 13 dB (AERO 25 à 30), contre son entrée 10 à 15 dB,
alors que la LSD sous 4 kHz entre entrée et sortie reste vers 2 dB : il
resynthétise le signal plutôt qu'il ne complète la bande. Faute de lancer
l'original, une erreur de la réécriture n'est pas exclue, mais le
chargement est strict et la LSD contre le FLAC baisse nettement. MP3 de la
bibliothèque : coupure 19,6 / 20,7 / 20,3 kHz, énergie au-dessus de 16 kHz
−42,1 / −38,6 / −35,6 dB (128 / 160 / 192), du même ordre qu'AERO.

LSD au-dessus de 16 kHz meilleur sur 11 des 12 extraits ; SI-SDR à
0,1 dB près sur 10 sur 12. AERO **étend la bande** sans toucher au reste :
la bande sous 16 kHz, où sont les artefacts du codeur, ne bouge pas.

Les 20 MP3 de la bibliothèque (pas de référence) : coupure moyenne
17,1 → 19,7 kHz (128), 17,8 → 20,2 (160), 18,4 → 19,8 (192) ; énergie
au-dessus de 16 kHz −57,0 → −41,4 dB, −45,4 → −39,6, −38,9 → −33,3.

Coût, Apple M4 Pro :
- AERO, pipeline Rust complet (`ort`, CPU) : 12,0 s par extrait stéréo de
  30 s (11,6 à 12,4), soit **24 s par minute de son**.
- Réseau seul (onnxruntime 1.31 en Python, segment de 5 s, un canal) : CPU
  0,62 s ; CoreML (Metal, MLProgram, CPU + GPU) 0,73 s — 627 nœuds sur
  656 pris mais en 13 partitions, aucun gain ; CoreML « ALL » (ANE) ne
  compile pas (`generic_general_slice`). Metal n'apporte rien à AERO.
- Apollo (torch 2.11, réseau seul, morceaux de 10 s) : Metal (MPS)
  10,4 s par extrait de 30 s (9,2 à 13,5), soit **21 s par minute de
  son** ; CPU 862 à 878 s par extrait, soit **29 min par minute de son**
  (3 extraits) : Apollo n'est utilisable que sur le GPU.

## Avis sur un port Burn d'Apollo

Faisable : 16,5 M paramètres, découpe en bandes et modélisation par
séquences de bandes, les briques du BS-RoFormer déjà porté en Burn
(`docs/module3-roformer.md`) ; poids CC BY-SA 4.0 republiables convertis.
Mais il ne vaut la peine que si l'écoute le demande : Apollo est entraîné
à réparer le MP3 de 24 à 128 kbit/s, la bibliothèque est entre 128 et
192, et sous 16 kHz l'écart au FLAC est déjà faible à 192 kbit/s (LSD
3,7 dB). Le gain attendu est sur les 128 kbit/s (LSD 7,2 dB sous 16 kHz,
qu'AERO ne touche pas). Ordre proposé : faire tourner Apollo (accord
nécessaire), mesurer la bande basse, écouter ; porter seulement s'il gagne
nettement à 128.

Après les mesures du 17 nov. : Apollo gagne aux chiffres (bande haute, et
sous 16 kHz à 128 kbit/s), mais il réécrit tout le signal, perd sous 16 kHz
à 192 kbit/s et ne tourne en temps utile que sur le GPU (21 s par minute de
son sur Metal ; un port Burn passerait par `wgpu`). Le port ne se décide
qu'après l'écoute AERO contre Apollo (`$B/ecoute-aero-apollo/`).
