# Quel noyau wgpu calcule faux ? — aucun de ceux-là

Hors du workspace. Fait suite à `../burn-demucs/` et à `docs/module3-demixage.md`.

## Ce qu'on cherchait

HTDemucs importé depuis ONNX rend des stems justes sur processeur et faux sur
Metal — écart de 33 % sur un stem, cosinus 0,65 sur un autre. La génération de
code étant correcte (le processeur reproduit ONNX Runtime au millionième), le
soupçon portait sur **un noyau GPU**, c'est-à-dire le petit programme qui
exécute une seule opération tensorielle.

Pour le trouver : un modèle ONNX par opérateur suspect, un seul nœud chacun,
exécuté sur les deux backends et comparé à une référence ONNX Runtime.
Comparer les deux backends entre eux aurait dit qu'ils divergent, pas lequel a
tort.

## La réponse : aucun

Quatorze cas, deux backends, écart maximal **4,1 × 10⁻⁷** — de l'arrondi `f32`.

| opérateur | écart processeur | écart Metal |
|---|---|---|
| Clip | 2,1e-9 | 2,1e-9 |
| ConvTranspose | 4,8e-9 | 2,9e-9 |
| Cos | 4,2e-9 | 2,5e-11 |
| InstanceNormalization | 1,5e-9 | 2,2e-9 |
| LayerNormalization | 2,6e-8 | 1,2e-8 |
| Sigmoid | 5,6e-9 | 4,7e-8 |
| Sin | 3,0e-9 | 1,5e-8 |
| Split | 2,4e-10 | 2,4e-10 |
| Tile | 3,8e-10 | 3,8e-10 |
| Gather | — | 1,5e-9 |
| Unsqueeze | — | 2,4e-10 |
| InstanceNormalization `[1,48,512,336]` | — | 4,1e-7 |
| ConvTranspose `[1,48,256,168]` | — | 1,3e-9 |
| LayerNormalization `[1,336,512]` | — | 3,3e-9 |

Les trois derniers sont aux dimensions réelles de HTDemucs : un noyau GPU peut
être juste sur un petit tenseur et faux sur un grand, le découpage en tuiles
changeant avec la taille. Ce n'est pas le cas ici non plus.

## Une erreur de méthode, en chemin

La première liste de suspects était tirée du graphe **non replié**, où les
opérateurs de données se noient dans des milliers de nœuds de calcul de formes.
Résultat : quatre des neuf premiers cas (`Clip`, `Tile`, `Sin`, `Cos`) ne
figurent même pas dans le graphe que Burn exécute, et deux vrais suspects
manquaient — dont `Gather`, exactement le genre d'opérateur dont une erreur
enverrait l'énergie dans le mauvais stem.

La comparaison qui vaut est entre graphes **repliés**, celui de CLAP servant de
témoin puisqu'il passe sur Metal au cosinus 1,0000000000 :

```
HTDemucs replié : 1 453 nœuds, 23 types
CLAP replié     :   882 nœuds, 22 types

Dans HTDemucs et pas dans CLAP :
  ConvTranspose 10 · Gather 3 · InstanceNormalization 74
  LayerNormalization 26 · Sigmoid 48 · Split 48 · Unsqueeze 6
```

Les sept sont testés ci-dessus. Tous justes.

## Ce que ça nous apprend quand même

**Le module 2 est mieux assuré qu'avant.** Il tourne sur ce backend, avec un
sous-ensemble de ces opérateurs, tous vérifiés individuellement contre ONNX
Runtime aux deux échelles. On est passé de « ça marche » à « ça marche, et
voici les quatorze contrôles qui le disent ».

**Le défaut de HTDemucs n'est pas au niveau de l'opérateur.** Il reste trois
explications, par ordre de vraisemblance :

1. **les paramètres exacts** des opérateurs dans HTDemucs — un `ConvTranspose`
   avec des groupes, un `Pad` asymétrique, une combinaison que ces cas
   minimaux ne reproduisent pas ;
2. **une accumulation sur la profondeur** : 1 453 nœuds enchaînés, chacun juste
   à 10⁻⁷ près, peuvent dériver — mais 33 % d'écart demanderait une
   amplification que rien ici ne suggère ;
3. **la structure du graphe importé** plutôt que ses opérateurs : un
   enchaînement que la génération traduit d'une façon que wgpu exécute mal.

Trancher demanderait de **bissecter le vrai graphe** : ajouter des sorties
intermédiaires à HTDemucs, et remonter jusqu'au premier point où processeur et
Metal divergent. C'est la méthode définitive, et c'est un chantier à part.

**Le risque pour le projet est borné.** Les deux modèles en production —
CLAP pour le module 2, `demucs-core` pour le module 3 — sont vérifiés sur wgpu
contre une référence ONNX Runtime, et `empreinte_reference` garde le premier
d'une dérive future. Le défaut ne concerne qu'une voie d'import qu'on
n'emprunte plus.

## Reproduire

```bash
cd experiments/wgpu-noyaux
python3 generer.py modeles          # 14 modèles + la référence ORT
cargo run --release                                       # Metal
cargo run --release --no-default-features --features cpu  # processeur
```
