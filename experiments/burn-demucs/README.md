# Sondage : importer HTDemucs avec `burn-onnx`

Hors du workspace. Même manœuvre que `../burn-clap`, sur le modèle de démixage
du module 3 : `StemSplitio/htdemucs-onnx`, MIT, 316 Mo, 4 stems en un fichier.

## Réponse courte

**L'import marche et il est fidèle. Le backend GPU, lui, rend des nombres
faux — et le GPU n'est pas optionnel ici.** Le module 3 ne peut donc pas être
bâti sur Burn en l'état.

| | vs ONNX Runtime | Temps pour 7,8 s d'audio |
|---|---|---|
| ONNX Runtime (CPU, référence) | — | 822 ms |
| Burn `ndarray` (CPU) | **cosinus 1,000000000** (écart 3,7 × 10⁻⁶) | **272 s** |
| Burn `wgpu` (Metal) | cosinus 0,65 – 0,99 (écart 0,29) | **935 ms** |

Le CPU est juste mais met **35 × le temps réel** : inutilisable. Le GPU tient
8 × plus vite que le temps réel, et se trompe.

## Le chemin

### 1. Le modèle publié ne s'importe pas

Son entrée est pourtant déjà figée — `[1, 2, 343980]`, 7,8 s de stéréo à
44,1 kHz. Mais le graphe calcule ses formes à l'exécution, et la génération
s'arrête net :

```
Failed to parse ONNX file: Type inference failed:
Node 'reduceprod1' (ReduceProd): Type mismatch: expected Tensor, got Shape(3)
```

`ReduceProd` appliqué à une *forme* et non à un tenseur — l'idiome « produit
des dimensions » qui précède un `Reshape` dynamique. Même famille de blocage
que les marges calculées de CLAP.

### 2. Le repliage par ONNX Runtime le débloque

Même recette que pour CLAP, et elle est spectaculaire ici :

```bash
python -c "
import onnxruntime as ort
o = ort.SessionOptions()
o.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
o.optimized_model_filepath = 'models/htdemucs-fige.onnx'
ort.InferenceSession('models/htdemucs.onnx', o, providers=['CPUExecutionProvider'])"
```

| | avant | après |
|---|---|---|
| nœuds | 24 765 | **1 453** |
| types d'opérateurs | 38 | 23 |
| `ReduceProd` / `ScatterND` / `Range` / `Expand` / `Shape` | 1 / 684 / 697 / 701 / 2 968 | **0 partout** |

Sortie vérifiée identique sous ORT (RMS des quatre stems inchangés au
millionième), et 3 × plus rapide au passage — 822 ms contre 2 381.

`burn-onnx` importe ce graphe sans broncher.

### 3. Le GPU se trompe

Sur le même modèle importé, la même entrée :

| stem | RMS sous ORT | RMS Burn CPU | RMS Burn Metal |
|---|---|---|---|
| batterie | 0,025603 | 0,025603 | 0,024974 |
| basse | 0,305751 | 0,305752 | 0,279525 |
| autre | 0,110720 | 0,110720 | **0,147554** |
| voix | 0,000374 | 0,000374 | 0,000452 |

Le CPU reproduit ORT au millionième ; **la génération de code est donc
correcte**. C'est l'exécution wgpu qui dérive, de 33 % sur le stem « autre ».

**Ce n'est ni `fusion` ni `autotune`** : retirer les deux donne exactement les
mêmes valeurs fausses. Le défaut est dans un noyau de base.

**Les suspects** sont les opérateurs que HTDemucs emploie et que CLAP — qui,
lui, passe sur wgpu au cosinus 1,0000000000 — n'employait pas :
`InstanceNormalization` (74 occurrences), `ConvTranspose` (10), `Split` (48),
`Sigmoid` (48), `LayerNormalization` (26), `Clip`, `Tile`, `Sin`, `Cos`. Les
isoler demande un modèle minimal par opérateur : c'est le travail suivant, et
il a sa place en amont, chez Burn.

### Au passage : `fusion` ne sert à rien ici et coûte cher

Première passe **307 s** avec fusion, **4,8 s** sans. Régime établi identique
dans les deux cas : 938 ms. Cinq minutes de compilation de noyaux pour zéro
gain.

## Ce que ça implique pour le module 3

1. **Le chemin d'import est validé** — la recette « figer, replier, importer »
   marche sur les deux modèles du projet.
2. **La voie GPU de Burn n'est pas exploitable telle quelle** pour le
   démixage. Trois issues : trouver et signaler le noyau fautif ; attendre une
   version de Burn qui le corrige ; ou faire revenir ONNX Runtime **pour le
   seul module 3**, avec son fournisseur CoreML.
3. **Le CPU n'est pas un repli** : 35 × le temps réel.

## Reproduire

```bash
cd experiments/burn-demucs
python3 reference_demucs.py /tmp/stems-ort.txt ../../models/htdemucs-fige.onnx

cargo run --release -- /tmp/stems-metal.txt                              # wgpu
cargo run --release --no-default-features --features cpu -- /tmp/stems-cpu.txt
cargo run --release --no-default-features --features metal-nu -- /tmp/x  # sans fusion
```
