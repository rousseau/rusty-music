# Banc de transcription — jeux annotés

Vérité exacte, sans OMR. Ce banc complète celui des partitions publiées
(`experiments/partitions/`) : ce dernier classe les modèles, celui-ci mesure
leur justesse. Plan : `docs/plan-editer-pratique-creation.md`, section 0.4.

```
python banc_jeux.py slakh|mdb|guitarset|maestro [--limite N]
python piano_specialises.py      # Transkun et ByteDance, environnement à part
```

## Prérequis

- **Disque externe** : les données vivent dans `RUSTY_DEV`, par défaut
  `/Volumes/LaCieDock/rusty_music-dev`. Ce disque n'est pas toujours
  branché ; sans lui, le banc s'arrête avec un message clair.
- **Jeux de données** : téléchargés à la source dans `$RUSTY_DEV/jeux`, jamais
  versionnés.
  - BabySlakh : CC BY 4.0.
  - MDB Drums : CC BY-NC-SA 4.0.
  - GuitarSet : CC BY 4.0.
  - MAESTRO v3 : CC BY-NC-SA 4.0. On a tiré 20 pièces du jeu de test
    (`choix.json`) et on les a lues à distance dans l'archive de 108 Go,
    avec `remotezip`, sans la télécharger entière.
- **Cache** : `$RUSTY_DEV/banc-jeux`.
- **Binaires** :
  - `target/release/rusty-music` (démixage) ;
  - `target/release/examples/banc` (Basic Pitch, ADTOF) ;
  - `target/release/examples/muscriptor` (portage Burn, f16 + produit mince).
- **Python** : `mir_eval`, `pretty_midi`, `jams`, `pyyaml`.

## Mesures

- **Notes** : `mir_eval.transcription`, attaque à ±50 ms. On donne le F1 sans
  la fin de note, puis avec (fin à ±20 % de la durée).
- **Frappes** : `mir_eval.onset`, ±50 ms, F1 par classe.
- **Totaux** : pondérés par le nombre de notes de référence.

## Résultats (11 oct. 2026)

### BabySlakh

20 morceaux, 76 289 notes, instruments synthétisés depuis le MIDI.

| Entrée | MuScriptor | Basic Pitch |
|---|---|---|
| Mélange, toutes notes | **0,54** (0,23 avec fin) | 0,39 (0,16 avec fin) |
| Stem basse démixé | **0,59** | 0,44 |
| Mélange, instrument nommé : basse | 0,46 | — |
| Mélange, instrument nommé : guitare | 0,37 | — |
| Mélange, instrument nommé : piano | 0,36 | — |

**Batterie :**

| Pièce | ADTOF (stem démixé) | MuScriptor (mélange) |
|---|---|---|
| Grosse caisse | **0,84** | 0,65 |
| Caisse claire | **0,65** | 0,59 |
| Toms | 0,27 | 0,31 |
| Charleston | 0,07 | **0,60** |
| Cymbales | 0,00 | 0,11 |

### MDB Drums

23 extraits réels.

| Pièce | ADTOF (stem) | MuScriptor (stem) | MuScriptor (mélange) |
|---|---|---|---|
| Grosse caisse | 0,91 | **0,92** | 0,80 |
| Caisse claire | **0,74** | 0,72 | 0,61 |
| Cymbales | **0,84** | 0,75 | 0,50 |
| Toms | 0,44 | **0,56** | 0,51 |
| Charleston | **0,86** | 0,71 | 0,69 |

### GuitarSet

360 extraits, guitare seule, micro.

| Jeu | MuScriptor | Basic Pitch |
|---|---|---|
| Accompagnement | **0,73** (0,40 avec fin) | 0,72 (**0,43** avec fin) |
| Solo | **0,87** (0,57 avec fin) | 0,82 (**0,65** avec fin) |

### MAESTRO (piano)

20 pièces du jeu de test, 156 min, 85 889 notes, piano réel (Disklavier).
La fin de la note de référence est prolongée tant que la pédale forte tient,
selon la convention d'Onsets and Frames.

| Modèle | F1 | F1 avec fin |
|---|---|---|
| **Transkun 2** (MIT) | **0,985** | 0,48 |
| **ByteDance**, Kong et al. (Apache-2.0, poids CC BY 4.0) | 0,968 | **0,84** |
| MuScriptor | 0,83 | 0,38 |
| Basic Pitch | 0,67 | 0,22 |

- **Biais en faveur des spécialisés :** Transkun et ByteDance sont entraînés
  sur la partie d'entraînement de MAESTRO, donc sur le même piano et la même
  salle. Les pièces du jeu de test sont distinctes, mais il faudra confirmer
  ces chiffres sur MAPS ou SMD.
- **Écart de mesure sur les fins :** Transkun traite la pédale à part, ce qui
  explique sans doute son F1 avec fin plus bas.

## Lecture

- **Basse.** Sur le stem démixé, MuScriptor l'emporte nettement
  (0,59 contre 0,44). Cela confirme le mode « Qualité ».
  - Nommer l'instrument sur le mélange fait moins bien que transcrire le stem
    (0,46). L'ordre de l'application reste donc : démixer d'abord.
  - Les écarts d'un morceau à l'autre sont grands (0,17 à 0,90). Ils viennent
    en partie des rendus de Slakh : basses synthétiques, octaves déplacées.
- **Fins de note.** C'est le point faible de MuScriptor : sur GuitarSet, il est
  devant à l'attaque mais derrière Basic Pitch dès qu'on compte la fin. C'est
  peu gênant pour une partition, où la durée est quantifiée sur la grille de
  mesures, mais à surveiller.
- **Batterie.** ADTOF reste le bon choix sur le stem démixé (MDB : il gagne sur
  trois pièces sur cinq).
  - MuScriptor fait mieux sur les toms : à garder en tête pour 2.5 (LarsNet)
    ou une fusion.
  - **Piège de BabySlakh :** à 16 kHz, rien n'existe au-dessus de 8 kHz. ADTOF,
    entraîné à 44,1 kHz, ne voit alors ni charleston ni cymbale : il ne sort
    que grosse caisse, caisse claire et toms. Les chiffres de batterie de
    Slakh ne valent donc que pour ces trois pièces. Il faudrait Slakh2100 en
    pleine bande pour aller plus loin.
- **Piano.** Les modèles spécialisés sont d'un autre ordre :
  0,97-0,99 contre 0,83. Pour une partition de piano, on portera
  l'un d'eux plutôt que de compter sur MuScriptor.
  - ByteDance est un CRNN classique, exportable en ONNX pour `ort`.
  - Transkun décode par semi-CRF, ce qui est plus long à porter.
- **Boucles de MuScriptor** (révélées par MAESTRO).
  - **Symptôme :** une tranche de 5 s de Scriabine rendait 586 notes au lieu
    de 47, alors même qu'elle se terminait par un EOS.
  - **Correctif :** `jetons::debut_de_boucle` repère une même note rejouée
    trois fois à 20 ms ou moins d'intervalle. Le décodage s'arrête, et on garde
    le flux d'avant la boucle au lieu d'écarter le segment.
  - **Effet :** sur cette pièce, F1 0,38 → 0,53.
- **Correctif tiré du banc.** MuScriptor émettait parfois deux fois la même note,
  à moins de 20 ms d'écart (14 notes sur 391 en mode basse). `sans_doublons`
  les fusionne désormais dans `crates/transcription/src/muscriptor/mod.rs`.

## Pièges

- **Stems valides** : dans `metadata.yaml` de BabySlakh, `audio_rendered` est
  faux partout. On se fie à l'existence du wav du stem.
- **Octave rendue** : certains instruments sont rendus transposés par rapport
  à leur MIDI (basse une octave en dessous). `octave_rendue` passe Basic Pitch
  sur le stem isolé et choisit −12, 0 ou +12 ; le résultat est mis en cache.
- **MDB Drums** : la classe OT (autres) est ignorée, et l'audio est rééchantillonné
  à 44,1 kHz stéréo avant le démixage.
- **Reste à mesurer : la voix**. MedleyDB (accès sur demande) donnerait une
  mélodie annotée sur de vrais mélanges.
