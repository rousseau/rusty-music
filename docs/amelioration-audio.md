# Amélioration du son — bouton « E » du mode Écouter

Ce document trace la décision derrière le bouton « E » du transport et
l'enquête sur la super-résolution neuronale, écartée pour l'instant.

## Ce qui est livré

### Ligne de qualité

Sous le compteur de temps, la qualité du fichier en écoute :
`FLAC · 16 bit · 44,1 kHz`, `MP3 · 320 kb/s · 44,1 kHz`. Lue au scan
(`tags::read`, propriétés lofty), servie par la commande `qualite_piste(id)`
— calquée sur `descripteurs`. La profondeur de bits (`bit_depth`) est une
colonne ajoutée par migration ; `NULL` tant qu'un rescan « relire même les
fichiers inchangés » ne l'a pas remplie, comme `bitrate` / `codec` avant elle.

### Bouton « E » — excitation psychoacoustique

**Choix : excitateur par non-linéarité** (Aphex Aural Exciter ; Larsen & Aarts,
*Audio Bandwidth Extension*, Wiley 2004) — synthèse des 2ᵉ et 3ᵉ harmoniques
de la bande `[2,5 kHz, coupure]`, ajoutées **dans la région audible juste
sous la coupure** (présence, « air ») autant qu'au-dessus. Fait dans le
domaine STFT plutôt qu'en temporel : pas besoin de suréchantillonnage, pas de
repliement (on n'écrit rien au-dessus de Nyquist).

> Le premier essai ne recopiait qu'une octave translatée *au-dessus* de la
> coupure (~16 kHz pour un MP3 128) — quasi inaudible pour une oreille adulte.
> La synthèse d'harmoniques qui redescendent dans le médium-aigu est ce qui
> s'entend vraiment sur un fichier compressé.

Chaîne, dans `crates/player/src/amelioration.rs`, sur le tampon décodé en RAM
dans `ouvrir()` — donc **hors du verrou `Player`** :

1. `estimer_coupure` — périodogramme moyen (24 fenêtres de 4096), lissé sur
   ~9 raies ; la coupure est la plus haute fréquence encore ~30 dB au-dessus
   du niveau moyen du bas médium. `FC_MAX_HZ` (18 kHz) si l'estimation est
   ambiguë.
2. Si la coupure sort de `[3 kHz, 18 kHz)` → **passe-plat** : un FLAC ou un
   MP3 320 ressort strictement intact (test).
3. Sinon, STFT (Hann 2048, recouvrement 3/4) : pour chaque raie source `k` de
   `[2,5 kHz, fc]`, on ajoute aux raies `2k` et `3k` une contribution de
   module `|X_k|·gain·pente` et de phase `2·arg(X_k)` / `3·arg(X_k)` — la
   relation de phase d'une vraie harmonique. Symétrie hermitienne, iFFT,
   addition-recouvrement dans un tampon *humide*.
4. `sortie = sec + humide`, borné à ±1.

**Intensité réglable** (`0`..`1`) : `set_intensite` module les gains
d'harmoniques (`GAIN_H2_MAX`, `GAIN_H3_MAX`) par une courbe `intensité²` —
perceptible dès le quart de course, généreuse au bout. Défaut `0,6`. La
réglette de l'interface n'apparaît que quand « E » est actif.

État global au processus (`OnceLock` : `AtomicBool` + `AtomicU32` pour
l'intensité), pour ne pas faire transiter le réglage par la signature de
`ouvrir`.

Bascule / changement d'intensité en cours d'écoute : `set_amelioration` pose
drapeau et intensité, puis réouvre le morceau courant **en tâche de fond** et
le remet à la même position (`Player::remplacer_courant`) — le son continue
pendant le décodage, puis on bascule sans coupure. Le préchargement se
reconstruit au sondage suivant. La réglette n'agit qu'au relâché (`change`),
pas à chaque pixel.

Choix persistés côté interface (`localStorage`, clés `ameliorer` et
`ameliorer-force`).

**Limite connue** : la synthèse STFT peut légèrement lisser les transitoires
(cymbales, attaques) et, à forte intensité, durcir les sifflantes. Bornée par
la courbe de gain et le déclenchement sur coupure estimée. Un étage temporel
avec fort suréchantillonnage serait plus « analogique » mais réintroduit le
repliement que la voie fréquentielle élimine.

### Rééchantillonnage de qualité (toujours actif)

`rodio` 0.22 ne fait qu'une interpolation linéaire pour monter en fréquence
et jette des échantillons pour descendre (`conversions::SampleRateConverter`,
commentaire d'en-tête du crate), audible sur les rapports non entiers
(48 kHz → 44,1 kHz). Dans `ouvrir()`, si la fréquence du fichier diffère de
celle de la carte son (`Player::new` la mémorise), le tampon est
rééchantillonné avec `rubato` (`Fft` synchrone, `process_all`) avant d'être
rendu à `rodio` — dont le convertisseur devient alors un passe-plat.
Passe-plat aussi si les fréquences coïncident déjà.

## Super-résolution neuronale — sondage fait, chantier reporté

Objectif : reconstruire un vrai haut du spectre structuré (pas seulement des
harmoniques dérivées du bas), par un modèle appris, en rendu hors-ligne
« régénérer en HD ». **Reporté**, bloqué par la licence des poids.

- Sondage détaillé du modèle et des voies d'intégration :
  **`experiments/burn-aero/README.md`**.
- Architecture d'intégration dans l'app : **`docs/module3-superresolution.md`**.
- Script d'export ONNX (réseau seul) : **`scripts/preparer-aero.sh`**.

Résumé ci-dessous.

### Modèles

| Modèle | Code | Poids | Vitesse | Export ONNX |
|---|---|---|---|---|
| **AERO** (`slp-rl/aero`, ICASSP 2023) | MIT | Google Drive — **musique entraînée sur MUSDB18-HQ** | 1 passe (U-Net GAN sur spectrogramme complexe), mais attention/LSTM : lourd sur CPU | faisable (générateur unique ; STFT/iSTFT à refaire en Rust si opérateurs non supportés) |
| **AEROMamba** (LAMIR 2024) | MIT | idem AERO | ~15× plus rapide qu'AERO, ~5× moins de VRAM | ops SSM/Mamba pas garanties dans `ort` |
| **AudioSR** (`haoheliu/versatile_audio_super_resolution`) | MIT | HuggingFace, **licence non explicitée** | diffusion latente — plus lent que le temps réel même sur A100 (RTF ≈ 2,1), plusieurs min/morceau sur CPU | très difficile (pipeline multi-modèles + boucle d'échantillonnage) |

### Le blocage — licence des poids

- **MUSDB18-HQ** (dont dépendent les poids musique d'AERO / AEROMamba) est
  fourni « for educational purposes only », **non commercial** ; 46 de ses
  150 pistes sont en CC BY-NC-SA. `CLAUDE.md` exclut explicitement les
  licences CC BY-NC-*. Distribuer ces poids avec l'app n'est pas conforme.
- **AudioSR** : licence des poids non explicitée sur le dépôt → à clarifier
  avec l'auteur avant tout usage.
- Sortie propre : ré-entraîner AERO/AEROMamba sur un corpus librement licencié
  (FMA, MTG-Jamendo — CC BY) — 1 à 2 semaines et une boucle d'entraînement
  PyTorch. Contredit « ne jamais réécrire ce qui existe » ; à ne faire que si
  la super-résolution devient une priorité.

### Croquis d'intégration, si le blocage est levé

1. `torch.onnx.export` du générateur AERO (étape Python ponctuelle, hors
   dépôt) ; relever les opérateurs non supportés.
2. Inférence via `ort` (déjà la brique prévue pour les empreintes CLAP) ;
   `burn-import` plus risqué (opérateurs limités, opset ≥ 16).
3. STFT/iSTFT refaites côté Rust avec `rustfft` si non exportables.
4. Fenêtrage overlap-add sur le morceau (comme le découpage HTDemucs du
   module 3).
5. **Rendu hors-ligne asynchrone** : file de travaux, écriture d'un `.flac`
   amélioré dans un cache disque (~10–40 Mo/morceau), lecture ensuite de la
   version cache. « E » deviendrait « régénérer en HD » — barre de
   progression, état par morceau (original / en cours / HD disponible),
   détection GPU/CPU.

Conclusion : phase module-3, conditionnée à la résolution de la licence.

### Prototype

`scripts/preparer-aero.sh` fait l'export ONNX (réseau seul, STFT exclue,
figeage + repliage par ORT — recette `preparer-modele.sh`). Il attend un
checkpoint : les poids `4-16` (VCTK, CC BY) servent à roder le pipeline, la
musique demande le ré-entraînement (§ ci-dessus).

## Hors périmètre — normalisation de loudness (EBU R128)

Une normalisation (volume constant d'un morceau à l'autre) n'est pas une
amélioration : elle ira comme option du mode Bibliothèque (crate `ebur128`),
appliquée à l'ingestion ou à la lecture. Distincte de « E ».

## Références

- Larsen & Aarts, *Audio Bandwidth Extension* (Wiley, 2004) —
  <https://www.sps.tue.nl/rmaarts/RMA_papers/aar04pu6F.pdf>
- Spectral Band Replication — <https://en.wikipedia.org/wiki/Spectral_band_replication>
- AERO — <https://github.com/slp-rl/aero> · AEROMamba —
  <https://arxiv.org/abs/2411.07364>
- AudioSR — <https://github.com/haoheliu/versatile_audio_super_resolution>
- MUSDB18-HQ (licence) — <https://zenodo.org/records/3338373>
- `rubato` — <https://github.com/HEnquist/rubato>
