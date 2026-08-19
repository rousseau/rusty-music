# Module 3 — le démixage : le problème et les voies possibles

État au 17 août 2026. Écrit après le sondage `experiments/burn-demucs/`, qui a
rendu une réponse plus intéressante qu'un simple oui ou non.

## Le problème, en une phrase

**HTDemucs s'importe correctement dans Burn, mais le seul backend assez rapide
pour l'exécuter — le GPU — rend des nombres faux.**

## Ce qui a été mesuré

Entrée : un mélange synthétique de 7,8 s (343 980 échantillons stéréo à
44,1 kHz — la longueur sur laquelle HTDemucs a été entraîné), quatre voix dans
quatre registres. Signal calculé, jamais lu depuis un fichier : la comparaison
ne doit dépendre ni du disque ni de la carte SD.

| Exécution | Conformité à ONNX Runtime | Durée | Rapport au temps réel |
|---|---|---|---|
| ONNX Runtime CPU (référence) | — | 822 ms | 9,5 × plus rapide |
| Burn `ndarray` (CPU) | **cosinus 1,000000000** | **272 s** | **35 × plus lent** |
| Burn `wgpu` (Metal) | cosinus 0,65 – 0,99 | 935 ms | 8,3 × plus rapide |

Par stem, l'écart du GPU :

| stem | RMS sous ORT | RMS Burn CPU | RMS Burn Metal | cosinus |
|---|---|---|---|---|
| batterie | 0,025603 | 0,025603 | 0,024974 | 0,986 |
| basse | 0,305751 | 0,305752 | 0,279525 | 0,982 |
| autre | 0,110720 | 0,110720 | **0,147554** | **0,879** |
| voix | 0,000374 | 0,000374 | 0,000452 | 0,648 |

Écart absolu maximal : 3,7 × 10⁻⁶ pour le CPU, **0,29** pour le GPU.

Trois conclusions se lisent directement dans ce tableau :

1. **La génération de code de `burn-onnx` est correcte.** Le CPU reproduit
   ONNX Runtime au millionième — c'est de l'arrondi `f32`, rien d'autre.
2. **Le backend wgpu se trompe** sur ce modèle. Sur CLAP, il donnait pourtant
   un cosinus de 1,0000000000 : le défaut est déclenché par quelque chose que
   HTDemucs emploie et que CLAP n'employait pas.
3. **Le CPU n'est pas un repli.** Trente-cinq fois le temps réel : démixer un
   morceau de quatre minutes demanderait deux heures et demie.

Ce n'est **ni `fusion` ni `autotune`** : les retirer donne exactement les mêmes
valeurs fausses. Le défaut est dans un noyau de base. (Au passage : `fusion`
coûtait 307 s de compilation de noyaux à la première passe contre 4,8 s sans,
pour un régime établi identique — 938 ms dans les deux cas.)

**Les suspects** sont les opérateurs présents chez HTDemucs et absents de
CLAP : `InstanceNormalization` (74 occurrences), `Split` (48), `Sigmoid` (48),
`LayerNormalization` (26), `ConvTranspose` (10), plus `Clip`, `Tile`, `Sin`,
`Cos`. Les isoler demande un modèle minimal par opérateur.

## Le défaut de fond : on demandait à Burn d'exécuter une FFT

Le sondage a révélé quelque chose de plus structurant que le bogue lui-même.

Le graphe ONNX publié compte 24 765 nœuds. **16 440 d'entre eux — 66 % —
portent `stft` dans leur nom.** Les deux tiers de ce qu'on demandait au GPU
d'exécuter ne sont pas un réseau de neurones : c'est une transformée de Fourier
à court terme, et son inverse, déroulées nœud par nœud par l'exportateur
PyTorch. D'où les 2 968 `Shape`, 697 `Range`, 684 `ScatterND` et le
`ReduceProd` sur une forme qui bloquait l'import.

Or HTDemucs est un modèle *hybride* : il travaille en parallèle sur la forme
d'onde et sur le spectrogramme. La STFT est sa porte d'entrée, pas une couche
apprise. La mettre dans le graphe, c'est confier à un moteur de tenseurs un
calcul que n'importe quelle bibliothèque de FFT fait mieux, plus vite et sans
approximation.

Le repliage par ONNX Runtime (`ORT_ENABLE_BASIC`) ramène le graphe à 1 453
nœuds et débloque l'import — mais il ne change pas la nature du problème : la
FFT reste dans le graphe, simplement mieux repliée.

## Ce que `demucs-rs` fait, et pourquoi ça compte

[`nikhilunni/demucs-rs`](https://github.com/nikhilunni/demucs-rs) (Apache-2.0)
est Demucs sur **Burn**, avec Metal, Vulkan et WebGPU, livré en CLI, en
application web WASM et en **plugin VST3/CLAP**. Environ 9 000 lignes de Rust.

Il n'utilise **aucun ONNX**. Le modèle est écrit à la main en modules Burn —
`htdemucs.rs`, `conv.rs`, `transformer.rs`, ~70 Ko — et les poids sont chargés
depuis `safetensors`. **La STFT reste en Rust**, avec `realfft`, hors du
graphe.

C'est exactement le contraire de notre approche, et ça explique pourquoi la
leur marche : ils ne donnent à Burn que le réseau de neurones.

Deux détails de leur `Cargo.toml` recoupent, indépendamment, ce que nos propres
mesures avaient trouvé :

- ils prennent `burn` en `default-features = false, features = ["std"]` — la
  même conclusion que nous avons tirée en découvrant que sans `std`, Burn
  bascule sur ses maths de substitution et rend des nombres faux sans
  avertir ;
- ils écartent `autotune` par défaut, avec ce commentaire : *« sur les GPU
  mobiles Apple, autotune teste des dizaines de variantes de noyau par forme à
  la première inférence et fige l'appareil »*. Nos 307 s de compilation avec
  `fusion` disent la même chose.

Leur dépôt contient aussi `bench/kernel_compare.py` et
`bench/debug_transformer.py` : ils ont manifestement traqué ce genre de
divergence numérique eux-mêmes.

Leur API est précisément celle dont le module 3 a besoin : `Demucs<B>`,
découpage en fenêtres (`num_chunks`), fondu triangulaire et recouvrement,
lecture GPU groupée pour éviter douze transferts par morceau, et
`TRAINING_LENGTH = 343980` — la même longueur que celle figée dans l'ONNX.

## Est-il maintenu ? Honnêtement : à peine

| | |
|---|---|
| Créé | 25 février 2026 |
| Dernier commit | 1er août 2026 |
| Commits | **52 en février, 1 en mars, 2 en août** — rien entre mars et août |
| Contributeurs | nikhilunni (54 commits), un contributeur d'appoint (1) |
| Étoiles / forks | 138 / 19 |
| Licence | Apache-2.0, dépôt non archivé |
| Publié sur crates.io | **non** — il faudrait pointer une révision git |

Le projet a été bâti d'un trait en un mois, puis laissé dormant. Le réveil
d'août n'est pas de son auteur : il répond à une contribution extérieure
(PR #6), ouverte le 4 juillet et fusionnée le 1er août — **quatre semaines**.
L'issue #5, « pas de son dans les WAV produits par le CLI », est ouverte depuis
le 12 mars.

**Et surtout, ils n'ont jamais suivi Burn.** Ils sont figés en 0.20.1, alors
que Burn 0.21.0 est sorti le 7 mai — il y a trois mois — et que 0.22 est en
pré-version. C'est directement contraire à la consigne « dernière version de
Burn, pas de rétrogradation ».

Conclusion : **on ne peut pas dépendre d'eux tels quels.** La question devient
donc : combien coûte le portage en 0.21 ?

## Le portage en Burn 0.21 : mesuré, pas supposé

**Deux fichiers, une seule modification d'API, 62 tests verts.**

Le seul changement cassant qui les concerne est `PaddingConfig`, passé de
symétrique à asymétrique en 0.21 :

```rust
PaddingConfig1d::Explicit(n)      →  Explicit(n, n)           // (left, right)
PaddingConfig2d::Explicit(h, w)   →  Explicit(h, w, h, w)     // (top, left, bottom, right)
```

Dix sites dans `conv.rs`, `htdemucs.rs` et `weights/load.rs`. Substitution
mécanique. Ensuite : `cargo test -p demucs-core --features ndarray` → **58 + 4
tests passent**, zéro échec.

### La conformité, vérifiée sur le même signal

Leur suite couvre la DSP et les formes, pas la fidélité de bout en bout — leur
seul test end-to-end est `#[ignore]` et sa ressource audio n'est même pas dans
le dépôt. On l'a donc refaite : même mélange synthétique que
`experiments/burn-demucs`, comparé à la référence ONNX Runtime.

| stem | RMS sous ORT | Burn 0.21 CPU | Burn 0.21 Metal |
|---|---|---|---|
| drums | 0,025603 | 0,025603 | 0,025604 |
| bass | 0,305751 | 0,305751 | 0,305733 |
| other | 0,110720 | 0,110720 | 0,110719 |
| vocals | 0,000374 | 0,000374 | 0,000374 |

**Le GPU donne les bons stems.** Le bogue wgpu qui faussait le modèle importé
depuis ONNX **ne touche pas leur chemin de code** — la preuve la plus utile de
tout ce sondage. Somme des stems contre l'entrée : **SDR 35,6 dB**, là où leur
propre test exige 20.

### La vitesse

Pour 7,8 s d'audio :

| | Durée | vs temps réel | Juste ? |
|---|---|---|---|
| ONNX Runtime CPU | 822 ms | 9,5 × | oui |
| **demucs-rs, Burn 0.21, Metal** | **728 ms** | **10,7 ×** | **oui** |
| demucs-rs, Burn 0.21, CPU | 11,5 s | 1,5 × plus lent | oui |
| Burn depuis ONNX, Metal | 935 ms | 8,3 × | **non** |
| Burn depuis ONNX, CPU | 272 s | 35 × plus lent | oui |

Deux enseignements. D'abord, **leur modèle écrit à la main est 24 × plus rapide
que le même modèle importé depuis ONNX** sur le même processeur — la STFT hors
du graphe, exactement comme prévu. Ensuite, `fusion` et `autotune` sont
indispensables côté GPU : sans eux, Metal met 90 s. Avec eux, 4,5 s de chauffe
puis 728 ms par segment — d'où l'intérêt de leur méthode `warmup()`.


## Les possibilités, révisées

Le portage étant mesuré, les options ne sont plus celles d'avant le sondage.

### A′ — Porter `demucs-core` en 0.21 et en dépendre par un fork — **retenu, fait**

Le fork vit sur [`rousseau/demucs-rs`](https://github.com/rousseau/demucs-rs),
branche `burn-0.21`, révision `6020111`. `crates/editor` en dépend par cette
révision épinglée.

Publier le portage sur un fork, en dépendre par révision git, et **proposer le
même correctif en amont**. S'il est fusionné, on repointe sur l'original ;
sinon on garde le fork.

- **Pour** : validé de bout en bout ci-dessus, sur la dernière version de Burn,
  GPU compris. Le gros du coût est déjà payé. Licence Apache-2.0 sans clause
  gênante. On hérite du découpage, du fondu, des transferts GPU groupés et de
  `warmup()`.
- **Contre** : un fork à tenir, sur un projet au mainteneur intermittent. Mais
  le portage a coûté une substitution mécanique : le rattrapage des versions
  suivantes sera du même ordre, et on est désormais outillés pour le vérifier.
- **Coût** : fait pour l'essentiel. Une journée pour raccorder au module 3.

### B — Écrire notre propre modèle en modules Burn

Sans objet désormais. Le portage montre que leur code passe en 0.21 sans
difficulté ; réécrire 70 Ko de modèle validé pour éviter un fork serait payer
très cher une indépendance qu'on peut obtenir autrement.

### C — Isoler le noyau wgpu fautif

Toujours à faire, mais **plus comme voie de secours** : leur chemin de code
évite le défaut. Ça reste une contribution utile à Burn, et une assurance pour
le module 2, qui tourne sur ce même backend.

### D — ONNX Runtime pour le seul module 3

Écarté. Burn/Metal fait 728 ms là où ORT/CPU fait 822, avec les mêmes stems, et
sans réintroduire un second moteur d'inférence.

## Recommandation

**A′.** Le portage est fait, testé, conforme au millionième sur processeur et
au dix-millième sur GPU, et plus rapide qu'ONNX Runtime. Il respecte la
consigne « dernière version de Burn ».

Trois choses, dans l'ordre :

1. **Proposer le portage 0.21 en amont.** Dix lignes, et ça sonde la vivacité
   du projet : leur réponse à la PR #6 en quatre semaines suggère qu'ils
   fusionnent, lentement. Leur réaction dira s'il faut prévoir de vivre
   durablement sur un fork.
2. **Raccorder le module 3** sur ce socle : décodage par `symphonia` (déjà là),
   séparation par `demucs-core`, écriture des stems.
3. **Isoler le noyau wgpu fautif** et le signaler. Le module 2 tourne sur ce
   backend ; savoir ce qui se trompe, c'est savoir si ça peut mordre ailleurs.

Réserve à garder en tête : **rien de tout cela n'a été vérifié sur de la vraie
musique.** Le mélange de sinusoïdes prouve la conformité numérique entre
moteurs, pas la qualité de séparation perçue. Un contrôle à l'oreille sur
quelques morceaux de la bibliothèque est le premier jalon du module 3.
