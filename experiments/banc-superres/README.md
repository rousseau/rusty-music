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

**Apollo n'a pas tourné** : son exécution (code cloné, poids
`pytorch_model.bin` en pickle) a été refusée dans la session automatique.
Tout est prêt : `HF_HUB_DISABLE_TELEMETRY=1 PYTHONPATH=$B/apollo-src
$B/venv/bin/python -I banc.py apollo $B mps`, puis `mesurer` et `ecoute`
(qui prend alors trois versions par extrait).

Licence des poids : la fiche Hugging Face `JusperLee/Apollo` déclare
**CC BY-SA 4.0** (entraînés sur MUSDB18) — la synthèse disait « non dite ».
Republiables convertis, sous la même licence, en citant.

AERO contre le MP3, 6 FLAC encodés (moyennes) :

| débit | LSD MP3 → AERO | LSD > 16 kHz | LSD < 16 kHz | SI-SDR (dB) | énergie > 16 kHz (réf. −44,4 dB) | coupure |
|---|---|---|---|---|---|---|
| 128 | 21,0 → 10,3 | 38,0 → 15,8 | 7,17 → 7,10 | 24,91 → 24,87 | −57,7 → −47,7 | 15,7 → 17,9 kHz |
| 192 | 18,0 → 11,1 | 33,8 → 20,3 | 3,69 → 3,69 | 29,86 → 29,76 | −50,0 → −45,6 | 16,6 → 18,6 kHz |

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
- Apollo : non mesuré.

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
