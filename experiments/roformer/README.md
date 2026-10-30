# Banc RoFormer — faut-il remplacer HTDemucs ?

Phase 2 du chantier décrit dans `docs/module3-roformer.md` (état de l'art,
choix des candidats, provenance des poids). Les candidats tournent par leur
code d'origine, en Python, au banc seulement.

## Candidats

| Nom au banc | Modèle | Stems | Poids |
|---|---|---|---|
| `bsrofo_sw` | BS-RoFormer « SW », 174,7 M paramètres | 6 | [`enerjazzer/BS-ROFO-SW-Fixed`](https://huggingface.co/enerjazzer/BS-ROFO-SW-Fixed), octet pour octet celui du miroir [`FractalSurfer/BS-RoFormer-SW-6stem`](https://huggingface.co/FractalSurfer/BS-RoFormer-SW-6stem) : `.ckpt` SHA-256 `24e7d35ee9c64415673d3fd33e06a67cac2c103c5df6267ba1576459c775916e`, `.yaml` `f9fada9f…4329b` ; licence non déclarée, provenance inconnue |
| `bsrofo_musdb` | BS-RoFormer MSST, appris sur MUSDB18HQ seul | 4 | release v1.0.12 de MSST |
| `scnet_xl_ihf` | SCNet XL IHF, MUSDB18HQ seul | 4 | release v1.0.15 de MSST |
| `htdemucs`, `htdemucs_ft`, `htdemucs_6s` | nos HTDemucs (`rusty-music demix`, Burn/Metal) | 4 / 4 / 6 | `set-soft/audio_separation` |

Réglages d'inférence : ceux de chaque configuration publiée (découpe,
recouvrement 2 ou 4, fondu de MSST), sauf SCNet, passé en lot de 1
(`SEP_LOT=1`) : son lot de 4 dépasse la mémoire du Mac.

## Mise en place

```
git clone https://github.com/ZFTurbo/Music-Source-Separation-Training $RUSTY_DEV/ext/mss-training   # révision 6fdad13
dot_clean -m $RUSTY_DEV/ext/mss-training                                                              # exFAT
UV_CACHE_DIR=$TMPDIR/uv-cache uv venv -p 3.11 $TMPDIR/roformer-env
uv pip install -p $TMPDIR/roformer-env/bin/python torch torchaudio numpy soundfile librosa ml_collections \
    omegaconf==2.2.3 beartype==0.14.1 rotary_embedding_torch==0.3.5 einops==0.8.1 tqdm pyyaml packaging
# poids et configurations dans $RUSTY_DEV/ext/roformer-poids (URL dans docs/module3-roformer.md)
```

Lancement :

```
python -I separer.py <modele> liste.tsv                     # séparation seule, mesure du temps et de la mémoire
banc_jeux.py medleydb-demix --modele htdemucs,htdemucs_ft,htdemucs_6s,bsrofo_sw,bsrofo_musdb,scnet_xl_ihf
DEMIX=bsrofo_sw banc_jeux.py slakh2100 --limite 50          # MuScriptor sur le stem de basse de SW
DEMIX=bsrofo_sw banc_jeux.py medleydb --famille basse
```

`banc_jeux.py` appelle `separer.py` par l'interpréteur `ROFORMER_PY`
(par défaut `$TMPDIR/roformer-env/bin/python`) pour tout modèle dont le nom
ne commence pas par `htdemucs`. `DEMIX` choisit le séparateur de la chaîne
de transcription ; ses stems et ses caches (`…@bsrofo_sw.json`) sont à
part de ceux de HTDemucs 6s.

## Résultats

### SDR sur MedleyDB (`medleydb-demix`, 30 oct. 2026)

100 morceaux retenus (ajustement des stems au mélange ≥ 12 dB), SDR global
du MDX, médiane par source. `scnet_xl_ihf` : à venir.
Journal : `$RUSTY_DEV/banc-jeux/roformer/demix-sw.log`.

| | basse V1 | basse V2 | batterie V1 | batterie V2 | voix V1 | voix V2 |
|---|---|---|---|---|---|---|
| n | 38 | 21 | 39 | 24 | 25 | 15 |
| `htdemucs` | 10,27 | 2,17 | 8,10 | 7,35 | 11,09 | 7,83 |
| `htdemucs_ft` | 10,79 | 2,88 | 8,34 | 7,40 | 11,57 | 7,65 |
| `htdemucs_6s` | 9,59 | 1,91 | 6,91 | 6,77 | 10,66 | 6,86 |
| **`bsrofo_sw`** | **13,43** | **4,41** | **11,61** | **10,50** | **13,66** | **11,97** |
| `bsrofo_musdb` | 11,46 | 2,69 | 9,82 | 9,05 | 13,73 | 10,41 |

Différence appariée SW − `htdemucs_ft`, morceau par morceau (médiane,
morceaux où SW gagne) :

| | V1 | V2 |
|---|---|---|
| basse | +2,4 dB, 35/38 | **+0,5 dB, 13/21** (pire −7,6) |
| batterie | +2,4 dB, 36/39 | **+2,4 dB, 21/24** |
| voix | +2,7 dB, 25/25 | +2,8 dB, 15/15 |

Guitare et piano (6 stems, contre `htdemucs_6s`) : V2 guitare 8,99 contre
4,30, piano 4,00 contre 0,65.

Lecture :

- **Le seuil est franchi sur la batterie V2** (+2,4 dB apparié, 21 morceaux
  sur 24), et la voix suit (+2,8 dB, 15 sur 15). V2 n'est dans aucun jeu
  public ; mais rien n'exclut que SW l'ait vu (provenance inconnue).
- **L'architecture compte, pas seulement les données inconnues** :
  `bsrofo_musdb`, appris sur les 100 morceaux de MUSDB18HQ seuls, bat déjà
  `htdemucs_ft` sur V2 en batterie (+1,6 dB) et en voix (+2,8 dB), mais pas
  en basse (−0,2 dB). C'est un repli de provenance connue si SW devait
  être écarté — sans le gain sur la basse.
- **Sur la basse V2, le gain est fragile** : +1,5 dB d'écart entre médianes,
  mais seulement +0,5 dB apparié, 13 morceaux sur 21. La basse de V2 est
  surtout de la contrebasse classique et jazz, où tous les modèles sont
  à 2-4 dB. Le banc de transcription (basse de Slakh et MedleyDB) tranche.

### Basse transcrite par MuScriptor, Slakh2100 (50 premiers morceaux)

`banc_jeux.py slakh2100 --limite 50`, F1 des notes (attaque et hauteur)
sur le stem de basse, 22 686 notes. Journaux : `basse-slakh-{sw,6s}.log`.

| Stem de basse de | MuScriptor F1 | avec fin | Basic Pitch F1 |
|---|---|---|---|
| `htdemucs_6s` (chaîne actuelle) | 0,649 | 0,320 | 0,519 |
| **`bsrofo_sw`** | **0,837** | **0,437** | **0,644** |
| *mélange, MuScriptor « instrument nommé »* | 0,350 | 0,178 | |

Apparié : +0,185 de F1 médian, SW meilleur sur 46 morceaux, égal sur 2,
jamais pire (48 morceaux communs). Réserve : Slakh est rendu par
synthétiseurs à partir de Lakh MIDI, et c'est un jeu d'entraînement courant
chez MVSep ; SW a pu le voir. La batterie (ADTOF) ne bouge pas.

### Basse transcrite par MuScriptor, MedleyDB (vrais enregistrements)

`banc_jeux.py medleydb --famille basse` : justesse de la ligne de basse
trame par trame (pas de partition de notes dans MedleyDB). Deux
références : les annotations corrigées à la main (10 morceaux), et pYIN
sur la piste de basse isolée (79 morceaux, plus bruitée). Journaux :
`basse-mdb-{sw,6s}.log`.

| Stem de basse de | justesse, corrigés à la main | justesse, pYIN | présence, pYIN |
|---|---|---|---|
| `htdemucs_6s` | 0,728 | 0,687 | 0,699 |
| **`bsrofo_sw`** | **0,797** | **0,705** | **0,711** |

Apparié (justesse par morceau) : corrigés à la main +0,05 médian, SW
meilleur sur 8 sur 10 ; pYIN +0,00 médian, 37 meilleurs, 26 pires (le pire
−0,47). **Sur de la vraie musique, le gain sur la basse existe mais il est
bien plus petit que sur Slakh**, ce qui va dans le sens d'un SW qui aurait
vu Slakh, ou simplement d'une basse de studio déjà bien séparée par
HTDemucs.

## Port Burn (phase 3)

`crates/editor/src/roformer/` : réseau générique sur le backend Burn, STFT
en f64 sur le processeur (`rustfft`), découpe en segments de `demix`. Le
`.ckpt` d'origine est lu tel quel par `burn-store` (archive PyTorch, rien à
convertir ni à republier) ; `scripts/preparer-roformer.sh` le télécharge à
la source, révision épinglée, empreinte vérifiée.

Parité contre MSST sur processeur (float32), 30 s de
`TheKitchenettes_Alive` (`reference.py`, puis
`examples/parite_roformer.rs`) : chaque étage est repris de la référence,
la première divergence dirait où chercher.

| Backend | spectre → 12 blocs → masques | segment, 6 stems | morceau découpé, pire stem |
|---|---|---|---|
| ndarray (processeur) | cosinus 1,0000000 partout | 1,0000000 (écart ≤ 3,1·10⁻⁶) | 1,0000000 |
| wgpu (WGSL, Metal) | 1,0000000 partout | 1,0000000 (≤ 2,8·10⁻⁶) | 1,0000000 |
| wgpu `burn/metal` (MSL) | 1,0000000 partout | 1,0000000 | 1,0000000 |

Le défaut wgpu de HTDemucs importé d'ONNX ne se reproduit pas : le réseau
est écrit à la main en modules Burn, sans graphe ONNX. Test :
`cargo test -p rusty-music-editor roformer` (sans poids), et
`ROFORMER_REF=… cargo test --release -p rusty-music-editor -- --ignored parite_sw`.

Vitesse (M4 Pro 16 cœurs GPU, segment de 13,4 s, `examples/vitesse_roformer.rs`) :

| | un bloc | têtes de masque | segment entier |
|---|---|---|---|
| premier jet, WGSL | 0,81 s | 0,99 s | ≈ 10,7 s |
| têtes regroupées (7 produits par stem au lieu de 124) | | 0,36 s | |
| linéaires aplaties en 2D (`Linear` diffusait sur 1 151 lots) | 0,78 s | | ≈ 9,7 s, 0,69 × le temps réel |
| même code, `burn/metal` (MSL) | 0,42 s | 0,17 s | 5,4 s, 1,24 × le temps réel |
| *PyTorch MPS, `separer.py`* | | | *≈ 4,2 s, 1,6 ×* |

L'attention fusionnée de Burn (`module::attention`) n'apporte rien ici
(674 ms contre 454 ms), la taille des tranches d'attention non plus. Le
reste du temps est dans les produits matriciels, à environ 0,9 TFLOP/s en
WGSL. **Passer l'application à `burn/metal` doublerait la vitesse**, mais
toucherait aussi HTDemucs, CLAP et MuScriptor : décision à part, avec leurs
parités à refaire.
