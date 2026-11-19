# Parité en `burn/metal` (MSL) — faut-il basculer l'application ?

L'application compile Burn sur wgpu avec des noyaux WGSL. L'option
`burn/metal` compile les mêmes noyaux en MSL (cubecl-msl), environ deux
fois plus vite pour le démixage (`experiments/roformer/README.md`,
« Toute l'application en `burn/metal` ? »). Ici : chaque réseau Burn de
l'application contre son code d'origine, en WGSL et en MSL (31 oct. 2026,
M4 Pro).

Binaires MSL : `cargo build --release … --features burn/metal --target-dir <autre>`
(l'option touche tous les crates à la fois). Références sous
`$RUSTY_DEV/banc-jeux/parite-metal/`.

## Résultats

| Réseau | Référence | WGSL (actuel) | MSL |
|---|---|---|---|
| CLAP audio (`empreinte_reference`) | ONNX Runtime, valeurs figées | conforme, 6·10⁻⁷ | conforme, 6·10⁻⁷ |
| CLAP texte (`empreinte_texte_reference`) | PyTorch, valeurs figées | conforme, 1,3·10⁻⁷ | conforme, 1,3·10⁻⁷ |
| all-in-one (`sections_parite`, 120 s) | `experiments/structure/parite_ref.py` | logits à 3·10⁻⁶, 8 sections sur 8 | idem |
| BS-RoFormer SW (`parite_roformer`) | MSST (`experiments/roformer/reference.py`) | cosinus 1,0000000 | cosinus 1,0000000 |
| MuScriptor f32 (`parite_muscriptor`, 2 min de basse) | `experiments/muscriptor/reference.py` | 522 jetons, 93 notes sur 93 | 522 jetons, 93 notes sur 93 |
| MuScriptor f16 (réglage de l'application) | idem | logits à 4·10⁻², 93 notes sur 93 | logits à 3·10⁻², **89 sur 93** (un segment bifurque) |
| htdemucs | `reference_htdemucs.py` | **25-38 dB** | 69-70 dB |
| htdemucs_ft | idem | **25-39 dB** | 69-70 dB |
| htdemucs_6s | idem | **16-40 dB** | **18-32 dB** |

Pour HTDemucs, l'écart est rapporté à l'énergie du mélange (`comparer.py`) :
70 dB, c'est la précision du float32 ; 20-30 dB, une erreur audible de
près sur un stem isolé. Sur processeur (ndarray), les trois variantes
sont à 70 dB : **le port (demucs-core) est juste, ce sont des noyaux GPU
qui ne le sont pas.** PyTorch lui-même n'est pas sensible au backend :
processeur contre MPS, plus de 109 dB, 6 stems compris.

Lecture :

- **WGSL, le backend livré, sépare faux avec les trois HTDemucs.** La
  qualité mesurée n'en souffre pas visiblement (SDR contre les vrais stems
  de MedleyDB à ±0,6 dB, `experiments/roformer/README.md`), mais ce n'est
  pas le réseau d'origine. Ce n'est pas le défaut wgpu du modèle importé
  d'ONNX (`docs/module3-demixage.md`, révisé) : celui-ci avait été jugé
  absent du port à la main sur la foi du seul RMS des stems.
- **MSL corrige htdemucs et htdemucs_ft, pas htdemucs_6s.** Le défaut du
  6 stems en MSL tient au réseau, pas au découpage : un seul segment de 7 s
  au milieu du morceau donne le même écart (21-30 dB), alors que le
  processeur y est à 70 dB. À localiser couche par couche dans demucs-core
  (fork `rousseau/demucs-rs`) avant toute bascule : htdemucs_6s n'est plus
  la variante pour pratiquer, mais il reste au menu.
- **MuScriptor** : exact en f32 sur les deux backends. En f16, aucun des
  deux n'est exact (logits à quelques 10⁻²) ; WGSL tombe juste sur cet
  extrait, MSL bifurque sur un segment. En MSL, le f32 va aussi vite que
  le f16 actuel en WGSL (8,3 s contre 8,3 s pour 522 jetons) : basculer en
  MSL permettrait de passer MuScriptor en f32, exact, sans rien perdre.
- Les extraits presque muets trompent : sur les 7 premières secondes de
  `TheKitchenettes_Alive`, même le processeur paraît s'écarter si l'on
  rapporte l'erreur au stem plutôt qu'au mélange.

## htdemucs_6s faux en MSL : localisé (19 nov. 2026)

**Le défaut est un noyau de produit matriciel de cubecl en MSL**, pas le
port. Méthode : `crates/editor/examples/couches_demucs.rs` relève min, max,
moyenne et écart-type de chaque étage (le `DebugListener` de demucs-core)
sur `extrait60.wav`, un segment ; processeur contre MSL.

- Les encodeurs sont justes (≈ 10⁻⁵, comme pour le 4 stems).
- L'écart naît dans la **première auto-attention de la branche temporelle**
  du transformeur : entrée normalisée, Q, K et V justes à 10⁻⁵, mais
  `Q·Kᵀ` faux (maximum 17,83 au lieu de 17,01), puis softmax, contexte et
  tout ce qui suit. La branche fréquentielle (2 688 positions) est juste.
- Ce qui ne dépend que du 6 stems : pas de sur-échantillonnage des canaux
  avant le transformeur, donc d_model 384 sur 8 têtes, **têtes de 48**
  (le 4 stems passe à 512 : têtes de 64).

Reproducteur hors du modèle : `crates/editor/examples/formes_attention.rs`
(entrées aléatoires, référence en f64 écrite à la main).

- MSL : `Q·Kᵀ` faux (écart jusqu'à 70 % du maximum) avec des têtes de 48 et
  1 280 à 1 536 positions, Q vue (`reshape` puis `swap_dims`) ou non ;
  juste à 672, 1 000, 1 024, 2 688 positions, et avec des têtes de 32 ou 64.
  Même résultat aux quatre niveaux d'autotune (`CUBECL_AUTOTUNE_LEVEL`,
  cache coupé) et **sans fusion** (pire : le cas contigu casse aussi) :
  c'est un noyau faux, pas un choix de l'autotune ni la fusion.
- WGSL : un autre défaut, `poids·V` contigu faux (27 %) à 1 000 positions
  et têtes de 48 ; le reste juste sur ces formes.
- Le processeur est juste partout.

**Contournement** (fork `rousseau/demucs-rs`, `model/transformer.rs`,
`attention`) : l'attention écrite avec les poids de `MultiHeadAttention`,
têtes complétées de zéros jusqu'au multiple de 32 supérieur (48 → 64), le
contexte recoupé ensuite. Mathématiquement identique ; dans le
reproducteur, juste à toutes les formes (3·10⁻⁷).

Mesuré avec le contournement (fork local, `--config patch`, protocole de
« Refaire » : 120 s de `TheKitchenettes_Alive`, contre PyTorch) :

| MSL | bass | drums | guitar | other | piano | vocals |
|---|---|---|---|---|---|---|
| htdemucs_6s avant | 22,4 | 31,6 | 18,5 | 21,7 | 22,9 | 22,4 |
| htdemucs_6s après | **70,2** | **70,6** | **70,1** | **71,4** | **69,4** | **70,3** |
| htdemucs avant / après | 70,0 / 70,0 | 70,0 / 70,0 | | 69,3 / 69,3 | | 70,0 / 70,0 |

htdemucs_6s devient exact en MSL ; htdemucs ne bouge pas ; temps inchangé
(8,3 à 8,4 s contre 8,6 à 10,1 s pour 120 s).

## WGSL faux sur les trois HTDemucs : localisé et contourné (19 nov. 2026)

Même méthode (`couches_demucs` et une trace dans `conv.rs`) : en WGSL,
l'écart naît au **2ᵉ encodeur temporel** ([1, 192, 5375]), dans la
première couche DConv : `conv1` et `norm1` justes, **`conv2` faux** — une
convolution 1×1 de 24 vers 384 canaux. Reproducteur :
`crates/editor/examples/formes_conv.rs` : en WGSL, 24 canaux d'entrée sur
5 375 ou 5 376 échantillons donnent jusqu'à 69 % d'écart (4 096 : juste) ; 32 canaux,
ou la même convolution en lot de 32 sur 336 positions (branche
fréquentielle), sont justes. MSL est juste à ces formes.

Le chiffre WGSL bouge d'une exécution à l'autre selon l'état du cache
d'autotune (`~/.cache/cubecl`) : htdemucs donnait 34,8 / 38,3 / 25,3 / 26,2 dB
le 31 oct., 26,6 / 31,9 / 25,4 / 39,1 le 19 nov. avec le même binaire.

**Contournement** (fork, `model/conv.rs`, `conv1x1`) : les convolutions 1×1
(`conv2` des DConv, `rewrite` des encodeurs temporels) en produit matriciel,
canaux d'entrée complétés de zéros jusqu'au multiple de 32.

**Résultat, les deux contournements ensemble** (fork `d851b07`, branche
`attention-tetes-completees`, épinglée par `crates/editor/Cargo.toml`) :

| contre PyTorch, dB | WGSL avant | WGSL après | MSL avant | MSL après |
|---|---|---|---|---|
| htdemucs | 25-38 | **69,3-70,0** | 69,3-70,0 | 69,3-70,0 |
| htdemucs_ft | 25-39 | **69,3-70,2** | 69,3-70,2 | 69,3-70,2 |
| htdemucs_6s | 16-40 | **69,4-71,4** | 18-32 | **69,4-71,4** |

Temps inchangé (WGSL, htdemucs, 120 s : 15,7 / 16,5 s avant, 15,7 / 16,9 s
après). **Les points 1 et 3 de « Reprendre » sont levés** ; reste la
décision de bascule (point 2), et le signalement en amont
(`signalement-cubecl.md`, brouillon non envoyé).

## Refaire

```
# environnement de référence (à part, rien pour l'application)
uv venv -p 3.11 $TMPDIR/parite-env
uv pip install -p $TMPDIR/parite-env/bin/python "git+https://github.com/muscriptor/muscriptor" demucs \
    librosa scikit-learn mir_eval soundfile cython hydra-core omegaconf timm huggingface_hub matplotlib
uv pip install -p $TMPDIR/parite-env/bin/python --no-build-isolation "git+https://github.com/CPJKU/madmom"
uv pip install -p $TMPDIR/parite-env/bin/python --no-deps allin1

python -I experiments/muscriptor/reference.py <basse.wav> <ref>/muscriptor --debut 30 --duree 120 --taille <muscriptor-medium.safetensors>
(cd experiments/structure && HF_HOME=$RUSTY_DEV/hf PYTHONPATH=natten_pur python parite_ref.py <stems> <ref>/allinone --secondes 120)
TORCH_HOME=$RUSTY_DEV/torch python -I experiments/burn-metal/reference_htdemucs.py <mix.wav> htdemucs_6s <ref>/demucs-ref-htdemucs_6s
<target>/examples/sur_fichier <mix.wav> htdemucs_6s 120 <ref>/demucs-msl-htdemucs_6s
python -I experiments/burn-metal/comparer.py <mix.wav> <ref>/demucs-ref-htdemucs_6s <ref>/demucs-msl-htdemucs_6s
```

## Reprendre (état au 31 oct. 2026)

Tous les chiffres sont dans ce fichier et dans `experiments/roformer/README.md` ;
rien n'est à recalculer pour décider. Ce qui reste :

1. **Localiser le défaut MSL de htdemucs_6s**, couche par couche, dans
   demucs-core (fork [`rousseau/demucs-rs`](https://github.com/rousseau/demucs-rs),
   révision épinglée dans `crates/editor/Cargo.toml`) : sorties
   intermédiaires du réseau sur un segment de 7 s (`extrait60.wav`), processeur
   contre MSL, comme `examples/parite_roformer.rs` pour RoFormer. Le 4 stems
   est exact en MSL : chercher ce qui ne dépend que du nombre de sources
   (dernières couches des décodeurs, découpe des sorties en stems).
2. **Décider la bascule en `burn/metal`** : gains ×1,2 à ×1,9 selon le réseau ;
   htdemucs et htdemucs_ft deviennent exacts ; MuScriptor peut passer en f32 sans
   perte de vitesse ; SW tient alors le critère « au plus deux fois
   `htdemucs_ft` » (1,6 ×, contre 2,9 × en WGSL). Bloquant : le point 1.
3. Indépendamment de la bascule : WGSL sépare faux avec les trois HTDemucs
   (25-40 dB). À signaler en amont (Burn / cubecl-wgpu) une fois le noyau isolé.

Ce qui ne vit que sur le disque LaCie (`$RUSTY_DEV` = `/Volumes/LaCieDock/rusty_music-dev`),
à rebrancher pour reprendre :

| Chemin | Contenu |
|---|---|
| `banc-jeux/parite-metal/` | références de parité : `muscriptor/`, `allinone/`, `demucs-ref-*` (PyTorch, 120 s), `demucs-{wgsl,msl}-*`, `seg60-*` et `extrait60.wav` (7 s au milieu de `TheKitchenettes_Alive`) |
| `banc-jeux/roformer/` | journaux du banc RoFormer, `suite.sh` (file reprenable) et ses `listes/`, `parite-sw/` (références de RoFormer), `chrono-htdemucs.txt` |
| `banc-jeux/medleydb/*/stems-*`, `banc-jeux/slakh2100/*/stems-*` | stems des six séparateurs, caches de transcription `…@<séparateur>.json` |
| `target-banc/{wgsl,msl,cpu}/` | binaires de mesure par backend (≈ reconstruits en quelques minutes ; supprimables) |
| `torch/`, `hf/` | poids PyTorch de HTDemucs et d'all-in-one pour les références |
| `ext/mss-training`, `ext/roformer-poids` | code et poids MSST des candidats RoFormer/SCNet |

Perdus à chaque redémarrage du Mac (dans `$TMPDIR`) et à refaire : les
environnements `roformer-env` (`experiments/roformer/README.md`) et
`parite-env` (ci-dessus, « Refaire »).
