# Sondage : super-résolution audio avec AERO

Hors du workspace (`exclude = ["experiments"]`). Fait suite au choix, dans
`docs/amelioration-audio.md`, de garder la super-résolution neuronale pour une
phase avancée : voici l'état de l'enquête sur **AERO**
(`slp-rl/aero`, ICASSP 2023), le candidat retenu.

**État (30 août 2026) : checkpoint musique récupéré, export ONNX validé, moteur
d'inférence tranché.**

- `scripts/preparer-aero.sh` produit `models/aero-11025-44100.onnx` — parité
  ORT vs PyTorch **2,8 × 10⁻⁵** (erreur L2 relative), graphe propre.
- Essai Rust (`cargo run --release` ici) sur un segment de 5 s, contre la
  référence PyTorch :

  | moteur | fidélité | temps (5 s d'audio) | note |
  |---|---|---|---|
  | **`tract`** (pur Rust) | **cos 0,68 — faux** | 2,5 s (×2 le temps réel) | erreur L2 relative 0,78 : un opérateur mal exécuté, comme wgpu sur demucs |
  | **`ort`** (ONNX Runtime) | **cos 1,000000** (rel 3 × 10⁻⁵) | 0,70 s (**×7 le temps réel**, CPU) | exact, rapide |

  **→ moteur retenu : `ort`.** Même conclusion que le module 3.

Reste : la STFT côté Rust, la boucle de segments, le crate et l'intégration
app (`docs/module3-superresolution.md`).

## Question posée

Le bouton « E » du mode Écouter fait aujourd'hui de l'excitation
psychoacoustique (harmoniques synthétisées, `crates/player/src/amelioration.rs`).
C'est un effet, pas une reconstruction. Un modèle appris peut-il
**reconstruire** un vrai haut du spectre structuré, en rendu hors-ligne
(« régénérer en HD »), et à quel prix d'intégration ?

## Réponse courte

**L'export ONNX du réseau seul marche et il est fidèle. Reste à écrire la STFT
Rust et à choisir le moteur d'inférence.**

1. `scripts/preparer-aero.sh` charge le checkpoint `musdb/…hl=256`, en extrait
   la classe et les kwargs (`pkg['models']['generator']`), retire deux
   irritants (voir plus bas), exporte **le seul réseau** (entrée/sortie =
   spectrogramme complexe 2 canaux `[1, 2, 256, T]`) et vérifie la parité
   contre PyTorch : **erreur L2 relative 2,8 × 10⁻⁵**, graphe de 628 nœuds,
   27 types, aucun opérateur exotique ni de domaine non standard.
2. AERO est de la lignée HDemucs. Le sondage `../burn-demucs` a établi que **le
   backend wgpu de Burn rend des nombres faux** sur cette famille, et AERO
   ajoute LSTM + attention + `Sin` (Snake) — tous suspects. La voie
   « `burn-onnx` + GPU » reste **une impasse**.
3. Voie viable : **STFT sortie du graphe, refaite en Rust (`rustfft`)** +
   moteur d'inférence sur le réseau seul. Deux candidats :
   - **`tract`** (Sonos, MIT/Apache) — inférence ONNX **pur Rust**, pas de lib
     native ni de `cmake`, dans l'esprit du projet (cf. `opus-decoder` vs
     `opus`). À valider : charge-t-il ce graphe ?
   - **`ort`** (ONNX Runtime) — la porte de sortie déjà identifiée pour le
     module 3, fournisseur CoreML, plus rapide, mais lib native + statut RC.
4. **Licence des poids** : voir § licence. Décidé (30 août) : on utilise tous
   les outils disponibles pour arriver à un logiciel fonctionnel ; la licence
   de distribution sera adaptée aux briques employées au moment de la
   publication (retrait des poids non redistribuables, ou ré-entraînement sur
   corpus libre).

### Les deux retouches à l'export

| Irritant | Cause | Correctif (dans `preparer-aero.sh`) |
|---|---|---|
| `EyeLike` non implémenté par ORT CPU | `LocalState` masque sa diagonale d'attention par `torch.eye(T, dtype=bool)` | remplacé par `(delta == 0)` — `delta` est déjà calculé dans la même fonction, opérateurs standard, résultat identique |
| `Aero.__init__` refuse certains kwargs | le paquet stocke des clés de config mortes (`channels_time`, `wiener_iters`, `time_stride`, `multi_freqs`…) | filtrées sur la signature de `Aero.__init__` |

## L'architecture d'AERO — config musique du checkpoint

Lue dans le paquet (`pkg['args'].experiment`), modèle
`hdemucs-snake-ftb-lstm-peg-concat`, classe `src.models.aero.Aero` :

- `lr_sr=11025`, `hr_sr=44100`, `spec_upsample=true` (ratio ×4).
- STFT : `nfft=512`. `hop_length` : **64** pour le signal d'entrée
  (`Aero.hop_length = 256 // scale = 256 // 4`) — la config affiche 256, mais
  c'est la valeur *avant* division. Fenêtre de Hann périodique,
  `normalized=True` (⇒ ×1/√nfft), `center=True`, `pad_mode='reflect'`, bin de
  Nyquist retiré (`[..., :-1, :]`) → **256 bins**. Complex-as-channels
  (`cac: true`) : le complexe devient 2 canaux réels.
- **U-Net sur spectrogramme complexe**, sans branche temporelle
  (`hybrid: false`). Profondeur 4, `strides=[4,4,2,2]` (sur l'axe fréquence),
  `channels=48`, `growth=2` → 48/96/192/384.
- Par couche : `Conv2d` / `ConvTranspose2d`, `GroupNorm` (4 groupes, à partir
  de la couche 2), `GELU`, `GLU`, embedding de fréquence appris.
- Branche résiduelle `DConv` (profondeur 2) avec, **à partir de la couche 2** :
  une **BLSTM** (`max_steps=200`, déroulage temporel) et une **attention
  locale** (`LocalState`).
- Activation `act_func: snake` — `x + sin²(αx)/α` : introduit `Sin`.
- Normalisation globale des entrées par moyenne/écart-type (dans le `forward` —
  reste dans le graphe exporté).
- ~331 tenseurs de poids, ONNX réseau-seul de **156 Mo**.

Poids : paquet `.th`, `pkg['models']['generator']` porte `class`, `kwargs`,
`state` — on reconstruit le modèle sans hydra.

Découpage de `predict.py` : segments de **10 s bout à bout, sans
recouvrement** — coutures possibles, à remplacer par un fondu-enchaîné
(overlap-add) côté Rust. `preparer-aero.sh` fige un segment de **5 s** (T=862
trames) ; à ajuster selon le coût mesuré.

## Voie d'intégration

STFT sortie du graphe, réseau seul exécuté par un moteur d'inférence,
segments avec recouvrement, rendu hors-ligne vers un cache. Détail dans
`docs/module3-superresolution.md`.

### La STFT à reproduire en Rust (`rustfft`, déjà là)

`spectro` d'AERO = `torch.stft` avec :
- `n_fft = 512`, `hop_length = 64` (valeur *après* division par `scale`),
  `win_length = 512`, fenêtre de **Hann périodique** (`torch.hann_window`
  défaut : `periodic=True` — `0.5 - 0.5·cos(2πn/N)`, pas `N-1`) ;
- `center = True` → l'entrée est **réfléchie** (`pad_mode='reflect'`) de
  `n_fft/2` de part et d'autre avant le fenêtrage ;
- `normalized = True` → chaque trame divisée par `√n_fft` ;
- `return_complex`, puis `[..., :-1, :]` : le **bin de Nyquist est jeté**
  → 256 bins.
- `_move_complex_to_channels_dim` : `[B, 1, 256, T]` complexe →
  `[B, 2, 256, T]` réel, ordre `(réel, imag)` sur l'axe canal.

`ispectro` = `torch.istft` symétrique (`F.pad(z, (0,0,0,1))` recolle un bin de
Nyquist nul, puis iSTFT normalisée, `center=True`, longueur cible
`length·scale`).

### Le moteur d'inférence — `ort` (mesuré)

`tract` charge le graphe sans broncher (pur Rust, séduisant) mais **le calcule
faux** : cos 0,68 contre PyTorch, un opérateur mal exécuté — exactement le
motif du sondage demucs sur wgpu. `ort` (ONNX Runtime) reproduit PyTorch au
`f32` près (cos 1,000000) et tourne à ×7 le temps réel sur CPU seul (une piste
de 4 min ≈ 35 s de rendu). C'est la porte de sortie déjà retenue pour le
module 3.

Coût : `ort` télécharge la bibliothèque ONNX Runtime (MIT) au build. Statut
`2.0.0-rc`. `ort-sys` tire `ureq` + `tar` + `flate2` pour ce téléchargement.
Sur mac, le fournisseur CoreML est disponible (non mesuré ici, CPU suffit).

## Licence des poids — décidé (30 août 2026)

Le checkpoint musique est entraîné sur **MUSDB18-HQ** (non commercial). La
décision du projet : **on utilise toutes les briques disponibles pour obtenir
d'abord un logiciel fonctionnel** ; la licence de distribution sera adaptée
aux outils employés au moment de la publication open source.

Options à la publication, si les poids restent non redistribuables :
- livrer l'app **sans les poids**, avec un script de récupération (comme
  `preparer-demucs.sh` le fait déjà pour HTDemucs) ;
- **ré-entraîner** la config musique sur un corpus libre — `train.py` fourni,
  candidats **MTG-Jamendo** (filtré CC BY / BY-SA) ou **FMA** (`small` /
  `medium`, CC BY / CC0). N'importe quelle musique pleine bande sert
  d'exemple, la version basse résolution se fabrique par filtrage. Quelques
  jours de GPU + validation ViSQOL/écoute.

## Suite — faite

- STFT/iSTFT Rust reproduisant `torch.stft` : `crates/superres/src/stft.rs`.
  Segment isolé, pipeline complet Rust vs PyTorch — **erreur L2 relative
  1 × 10⁻⁵**. (Détails `torch.stft` qui comptaient : Hann **périodique**,
  `normalized` ⇒ ×1/√nfft, `center`+`reflect`, Nyquist retiré ; `_spec`
  `hop=64 win=128`, `_ispec` `hop=256 win=512`.)
- Boucle segments (5 s) + recouvrement + fondu trapézoïdal :
  `crates/superres/src/lib.rs`. Fichier entier Rust vs `model()` PyTorch —
  **1,1 %**, la part de la segmentation (normalisation par segment), pas un
  défaut. `examples/verifier.rs` + `examples/reference_pipeline.py`.
- Retouche `rubato` : `Fft::process_all` laisse ~1 s d'amorce fausse au
  début ; corrigé par un préfixe réfléchi (écart au rééchantillonneur de
  référence 12 % → 5 × 10⁻⁴).
- Crate `crates/superres` (dans le workspace), cache `hd/`, commandes,
  aiguillage lecture (`Player::set_resolveur`), bouton « HD » —
  `docs/module3-superresolution.md`.
- Test réel : MP3 128 de 7 min régénéré en **164 s** (×2,6 le temps réel,
  stéréo), WAV 44,1 kHz 16 bits, 75 Mo.

## Reste

- Écoute réelle : A/B MP3 96/128/192 contre l'exciter « E » et contre
  l'original ; recouvrement minimal sans couture.
- Fournisseur CoreML si le CPU est trop lent sur le parc visé.
- Licence : à la publication, livrer sans les poids + script de récupération,
  ou ré-entraîner sur corpus libre (§ ci-dessus).

## Reproduire

```bash
# checkpoint musique (~437 Mo) via gdown ou l'interstitiel "virus scan" de Drive
./scripts/preparer-aero.sh --checkpoint musdb-hl256.th --segment 5
#  → models/aero-11025-44100.onnx   (parité L2 rel. 2,8e-5)

git clone --depth 1 https://github.com/slp-rl/aero
less aero/src/models/aero.py          # forward, _spec / _ispec
less aero/src/models/spec.py          # spectro / ispectro (torch.stft)
```
