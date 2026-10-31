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
