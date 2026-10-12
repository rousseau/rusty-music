# Sections : all-in-one contre un repli sans apprentissage

Avant de porter all-in-one en Burn (`docs/plan-editer-pratique-creation.md`,
2.4), on mesure sa référence Python sur de vrais morceaux de la bibliothèque,
annotés par des jeux publiés (`experiments/annotations/`). Le repli prévu au
plan (Foote) est mesuré sur les mêmes morceaux, avec les mêmes mesures.

```
PYTHONPATH=natten_pur <env>/bin/python allinone_ref.py [--limite N]
<env>/bin/python repli.py
```

`<env>` : un environnement à part avec torch, madmom (réinstallé
`--no-build-isolation` contre le numpy de l'environnement), demucs, librosa,
scikit-learn, mir_eval et `allin1` sans ses dépendances (`--no-deps`). Rien de
tout ça ne sert à l'exécution de l'application.

- **Stems** : les nôtres (`rusty-music demix`, HTDemucs de l'application),
  rangés comme all-in-one les attend : `demix/htdemucs/<nom>/{bass,drums,other,vocals}.wav`.
- **NATTEN** n'a pas de roue pour macOS. `natten_pur/` réécrit en PyTorch
  pur les quatre noyaux qu'utilise all-in-one (`natten1dqkrpb`, `natten1dav`,
  et leurs versions 2D), avec la sémantique de NATTEN 0.14 (voisinage
  recentré aux bords, dilatation par classes de résidus, biais relatif).
  C'est aussi la spécification du portage Burn.
- **Repli** (`repli.py`) : chroma CQT et MFCC moyennés par temps (Beat
  This!, portage Rust), matrice de récurrence lissée, nouveauté par damier
  gaussien de 32 temps, étiquettes A à F par regroupement agglomératif.
- **Mesures** (mir_eval) : frontières à 0,5 et 3 s, F par paires
  (accord des étiquettes, quel que soit leur nom), sur la durée commune.
- **Calage** : par les temps annotés s'il y en a, sinon par le début du son
  (somme des stems) contre la fin du silence initial annoté.

Données, stems, sorties : `$RUSTY_DEV/banc-annotations/{demix,spec,allinone,repli}`.

## Résultats (11 oct. 2026)

89 morceaux (Isophonics 65, SALAMI 24), Harmonix noté à part :

| | F0,5 | F3 | Paires |
|---|---|---|---|
| **all-in-one** (8 plis, nos stems) | **0,487** | **0,630** | **0,601** |
| repli (Foote, sans apprentissage) | 0,174 | 0,487 | 0,576 |

| all-in-one par jeu | F0,5 | F3 | Paires |
|---|---|---|---|
| Isophonics (65) | 0,492 | 0,642 | 0,600 |
| SALAMI (24) | 0,473 | 0,597 | 0,605 |
| Harmonix (5, hors total) | 0,243 | 0,527 | 0,618 |

- **all-in-one l'emporte nettement sur les frontières**, beaucoup moins sur
  les étiquettes. Le portage Burn se justifie ; le repli ne sert que s'il
  coince.
- Pour mémoire, l'article annonce une précision des étiquettes de 0,738
  (paires) sur Harmonix, en validation croisée. Les annotations d'Isophonics et de SALAMI suivent
  d'autres conventions, plus grossières : l'écart ne dit pas que le réseau
  tourne mal.
- **Biais de convention** : sur les Beatles, le décalage qui maximiserait
  les frontières est d'environ −0,4 s, de façon systématique. all-in-one
  pose ses frontières sur les premiers temps, l'annotation un peu après.
  F0,5 y est très sensible ; F3 et les paires ne le sont pas. On ne corrige
  pas ce biais dans la notation.
- **Harmonix hors total** : c'est la base d'entraînement d'all-in-one, et
  son calage n'est sûr qu'à une mesure près. Sur trois morceaux sur cinq, le
  meilleur décalage tombe à 2 ou 4 s de celui des temps.

## Port Burn (12 oct. 2026)

`crates/transcription/src/sections/`, poids convertis par
`exporter_safetensors.py`, référence de parité par `parite_ref.py` :

```
<env>/bin/python exporter_safetensors.py                  # → $RUSTY_DEV/modeles/allinone
PYTHONPATH=natten_pur <env>/bin/python parite_ref.py <stems> <sortie> [--secondes 45]
MOTEUR=f32 cargo run --release -p rusty-music-transcription --example sections_parite -- <sortie> $RUSTY_DEV/modeles/allinone
MOTEUR=f32 cargo run --release -p rusty-music-transcription --example sections -- $RUSTY_DEV/modeles/allinone <sortie> <stems>...
```

Logits à moins de 1e-5 de Python, mêmes sections. Sur les 94 morceaux du
banc, mêmes scores (0,487 / 0,630 / 0,601), 6 s par morceau. En f16, les
logits divergent : f32 obligatoire.

## Pièges

- **Noms** : all-in-one résout les liens symboliques et nomme tout d'après le
  nom du fichier réel. S'il ne trouve pas les stems sous ce nom, il lance
  Demucs Python lui-même. Deux fichiers de même nom sont écartés.
- **Calage par les temps** : un décalage faux d'une ou deux mesures garde un
  bon F1 de temps, puisque temps et premiers temps sont périodiques. Sans
  silence annoté, rien ne lève l'ambiguïté.
- **Calage par le silence** : à mesurer sur le mélange, jamais sur un stem.
  La basse entre souvent après les autres : sur la basse seule, les
  décalages montaient à 9 s et F0,5 tombait à 0.
- **Bornes en double** : all-in-one sort parfois deux frontières au même
  instant, et mir_eval refuse les intervalles de durée nulle.
- **MSAF** ne s'importe plus avec un scipy récent (`scipy.inf`) : d'où
  `repli.py`.
