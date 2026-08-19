# Essai : importer l'encodeur CLAP avec `burn-onnx`

Hors du workspace (voir son `exclude`) : un essai doit pouvoir échouer sans
empêcher `cargo build` de passer sur le reste.

**Question posée.** `CLAUDE.md` annonce « Calcul / ML : Rust + Burn, backends
CUDA/ROCm/Metal/Vulkan/WebGPU/CPU ». Le module 2 tourne en réalité sur ONNX
Runtime (`ort`), fournisseur CPU. Burn peut-il prendre la place ?

## Réponse courte

**Oui, mais pas sur le modèle tel qu'il est publié.** Il faut d'abord le figer
sur une forme d'entrée unique. Une fois fait, l'import est exact au bit près ou
presque — **cosinus 1,0000000000** contre `ort`, écart absolu maximal
6,8 × 10⁻⁷, soit de l'arrondi `f32`.

## Ce qui bloque, et pourquoi

Le modèle publié déclare son entrée comme
`['audio_batch_size', 'num_channels', 'height', 'width']` — tout est dynamique.
Ses douze blocs Swin ajustent alors leurs marges à l'exécution, et la
génération de code s'arrête net :

```
ERROR burn_onnx::logger: PANIC => panicked at burn-onnx-0.21.0/src/burn/node/pad.rs:22:17:
Runtime pads are not supported in burn-onnx
```

Ce n'est **pas** un opérateur manquant : les 34 types employés par ce graphe
figurent tous comme supportés dans `SUPPORTED-ONNX-OPS.md`. C'est la
distinction qui compte — un opérateur supporté n'est pas un graphe importable.
`burn-onnx` génère du Rust par inférence *statique* de formes ; ce graphe en
calcule 786 `Shape`, 180 `Range`, 225 `Expand`, 45 `ScatterND` à l'exécution.

## La fausse bonne idée : `onnxsim`

`onnx-simplifier` replie magnifiquement le graphe — 8 031 nœuds → ~1 275, les
douze `Pad` disparaissent, `ScatterND`/`Where`/`Equal`/`Expand`/`Range` tombent
à zéro. `burn-onnx` l'importe sans broncher.

**Et le résultat est inutilisable** : `onnx.checker` et ONNX Runtime refusent
tous deux le graphe produit.

```
[ShapeInferenceError] Inferred shape and existing shape differ in rank: (6) vs (0)
[ShapeInferenceError] Dimension could not be inferred: incompatible shapes
```

Les reshapes de partitionnement en fenêtres Swin sortent incohérents. Un import
qui réussit sur un graphe cassé ne prouve rien — d'où la vérification
systématique contre `ort` avant de conclure quoi que ce soit.

## Ce qui marche : l'outillage d'ONNX Runtime

Figer les dimensions, puis laisser ORT replier ses propres constantes. Il ne
produira pas un graphe qu'il refuse ensuite de charger.

```bash
# 1. figer les quatre dimensions (une invocation chacune)
python -m onnxruntime.tools.make_dynamic_shape_fixed \
    --dim_param audio_batch_size --dim_value 1 entree.onnx sortie.onnx
#    … puis num_channels=1, height=1001, width=64

# 2. replier les constantes, via ORT lui-même
python -c "
import onnxruntime as ort
o = ort.SessionOptions()
o.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
o.optimized_model_filepath = 'models/clap-audio-encoder-fige.onnx'
ort.InferenceSession('sortie.onnx', o, providers=['CPUExecutionProvider'])"
```

`ORT_ENABLE_BASIC` et pas `EXTENDED` : les fusions poussées introduisent des
opérateurs du domaine `com.microsoft`, que Burn ne connaît évidemment pas. Au
niveau basique, le graphe reste en ONNX standard — vérifié, aucun domaine non
standard parmi les 882 nœuds restants, et les douze `Pad` ont tous des marges
constantes.

Le modèle figé rend exactement la même empreinte que l'original sous `ort`
(`empreinte_ort` accepte un chemin de modèle en argument, précisément pour ce
contrôle).

## Résultats mesurés

Entrée déterministe et calculée — pas un fichier audio : la comparaison ne doit
dépendre ni du disque ni de la carte SD. `entree_test()` est dupliquée à
l'identique dans `src/main.rs` et dans `crates/analysis/examples/empreinte_ort.rs`.

| | ONNX Runtime (`ort`) | Burn `ndarray` (CPU) | Burn `wgpu` (Metal) |
|---|---|---|---|
| Inférence, une fenêtre | 88 ms | 192 ms | **36 ms** |
| Chargement du modèle | 0,7 s | 0,1 s | 0,4 s |
| Première passe | — | — | 25,6 s (compilation des noyaux) |
| Cosinus contre `ort` | — | 1,0000000000 | 1,0000000000 |
| Écart absolu maximal | — | 6,8 × 10⁻⁷ | 1,4 × 10⁻⁶ |

Trois enseignements :

1. **Sur CPU, Burn est 2,2 × plus lent qu'ONNX Runtime.** Attendu : les noyaux
   CPU d'ORT sont très travaillés.
2. **Sur Metal, Burn est 2,4 × plus rapide que la chaîne de production.** C'est
   ce que promettait le choix d'origine, et personne ne l'avait vérifié.
3. **La première passe coûte 25,6 s** — compilation des noyaux wgpu. À
   chronométrer à part, sinon on mesure le compilateur et non le calcul (déjà
   la leçon du jalon 1).

## Ce que l'adoption coûterait

- **Le modèle est figé sur une forme.** 1 × 1 × 1001 × 64 : plus de traitement
  par lots. La passe présente soumet ses cinq fenêtres en un appel ; il
  faudrait cinq appels, ou refiger le modèle à `batch = 5`.
- **Le code généré fait ~4 400 lignes** et se régénère à chaque build. À
  compiler une fois, mais ce n'est pas gratuit.
- **`burn-store` doit être déclaré à la main.** Le code généré fait
  `use burn_store::{BurnpackStore, ModuleSnapshot}` sans que `burn-onnx` tire
  la dépendance — quatre erreurs de compilation qui n'ont rien à voir avec le
  modèle.
- **La chaîne de préparation du modèle devient une dépendance Python.** Elle ne
  tourne qu'une fois, mais il faut la documenter et la rejouer à chaque
  changement de modèle.

## Reproduire

```bash
cargo run --release -p rusty-music-analysis --example empreinte_ort -- \
    models/clap-audio-encoder.onnx /tmp/v-ort.txt      # référence

cd experiments/burn-clap
cargo run --release -- /tmp/v-burn.txt                 # CPU (ndarray)
cargo run --release --no-default-features --features metal -- /tmp/v-metal.txt
```
