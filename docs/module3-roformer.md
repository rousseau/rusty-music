# Module 3 — RoFormer : faut-il remplacer HTDemucs ?

État au 29 oct. 2026. Phase 1 (état de l'art) du chantier « RoFormer ».
Phase 2, le banc : `experiments/roformer/README.md`. Rien n'est porté ni
intégré tant que le banc n'a pas parlé.

## Pourquoi la question se pose

HTDemucs (Rouard, Massa, Défossez, ICASSP 2023) n'est plus l'état de l'art.
Depuis le Sound Demixing Challenge 2023, la famille **RoFormer** domine :
on découpe le spectrogramme en bandes de fréquence, puis un Transformer
alterne l'attention dans le temps et entre les bandes, avec des
embeddings rotatifs (RoPE). C'est la norme dans UVR et MVSep. Or tout le
mode Éditer travaille sur nos stems : transcription de basse et de
batterie, pulsation, greffe. Un meilleur stem de basse, c'est une meilleure
tablature.

## Les architectures

| | Article | Code de référence | Idée |
|---|---|---|---|
| **BS-RoFormer** | Lu, Wang, Kong, Hung, « Music Source Separation with Band-Split RoPE Transformer », arXiv 2309.02612 (SAMI-ByteDance, 1er à SDX23) | [lucidrains/BS-RoFormer](https://github.com/lucidrains/BS-RoFormer) (MIT) ; ByteDance n'a publié ni code ni poids | 62 bandes fixes (2 à 129 cases de fréquence), STFT 2048, attention temps puis fréquence, masque complexe par stem. 9,80 dB de moyenne sur MUSDB18HQ sans données en plus |
| **Mel-Band RoFormer** | Wang, Lu, Won, arXiv 2310.01809 (ISMIR 2023, démo tardive) | lucidrains, même dépôt | Bandes sur l'échelle mel, qui se recouvrent. Meilleur que BS-RoFormer sur voix, batterie et autres, **moins bon sur la basse** (les bandes mel sont larges dans le grave) |
| **SCNet** (point de comparaison) | Tong et al., « SCNet: Sparse Compression Network for Music Source Separation », arXiv 2401.13276 (ICASSP 2024) | [starrytong/SCNet](https://github.com/starrytong/SCNet) (MIT) | Pas un RoFormer : découpe en sous-bandes de résolution inégale, convolutions et LSTM double chemin. Léger, rapide sur GPU |

Ces architectures et leurs poids communautaires se ramassent dans
[ZFTurbo/Music-Source-Separation-Training](https://github.com/ZFTurbo/Music-Source-Separation-Training)
(« MSST », MIT, 1 600 étoiles, actif) : un même `demix` pour tous les
modèles (segments, fondu, recouvrement). C'est par ce code que le banc
lance les candidats.

## Les poids publiés

Le point dur n'est pas le code, c'est **la provenance des poids**.
Presque tous les modèles forts sont entraînés par la communauté UVR/MVSep,
sur des jeux privés, sans licence ni fiche. Le tableau ne retient que ce
qui produit des stems utiles au mode Éditer (basse, batterie, voix).
SDR « Multisong » : jeu d'évaluation privé de MVSep, chiffres publiés par
MVSep ou MSST (pas les nôtres).

| Modèle | Stems | Taille | Entraîné sur | Licence des poids | SDR Multisong basse / batterie / voix / autres |
|---|---|---|---|---|---|
| **BS-RoFormer « SW »** (`BS-Rofo-SW-Fixed.ckpt`) | 6 : bass, drums, other, vocals, guitar, piano | 175 M param., 699 Mo | **inconnu** | **non déclarée** (`license: unknown`) | **14,62 / 14,11 / 11,30 / 8,71** (guitare 9,05, piano 7,83) |
| BS-RoFormer MSST, MUSDB seul (`model_bs_roformer_ep_17_sdr_9.6568`, release v1.0.12) | 4 | dim 384, profondeur 8, 503 Mo | MUSDB18HQ, 100 morceaux d'entraînement | non déclarée (release d'un dépôt MIT) | 11,08 / 11,29 / 9,19 / 5,96 |
| SCNet XL IHF, MUSDB seul (`model_scnet_ep_36_sdr_10.0891`, v1.0.15) | 4 | 204 Mo | MUSDB18HQ | non déclarée (release d'un dépôt MIT) | 11,94 / 11,58 / 9,68 / 6,48 |
| Huge-SCNet-4stems (Aname-Tommy, HF) | 4 | 396 Mo | inconnu | Apache-2.0 déclarée | non publié |
| bs_roformer_4stems_ft (SYH99999, HF) | 4 | 527 Mo | inconnu | Apache-2.0 déclarée | non publié |
| Mini-BS-RoFormer-V2 (HiDolen, HF) | 4 | 46,8 M param. | MUSDB18HQ (train **et** test) | CC BY-NC 4.0 | MUSDB val : 9,68 / 10,58 / 10,86 / 8,99 |
| Mel-Band RoFormer 4 stems (MSST, v1.0.11) | 4 | jusqu'à 3 Go | MUSDB18HQ | non déclarée | moyenne MUSDB ≤ 8,9 : en dessous de HTDemucs |
| BS-RoFormer viperx `ep_317_sdr_12.9755`, Mel-Band Kim | 2 : voix / reste | 600 Mo | MUSDB18HQ + 500 morceaux | non déclarée | voix 10,87 / 10,98 |
| MVSep Mega 53 stems (MSST v1.0.21) | 53 | ≥ 16 Go de mémoire vidéo | inconnu | non déclarée | non publiés |
| *référence* `htdemucs_ft` | 4 | 333 Mo (4 réseaux) | MUSDB18HQ + 800 morceaux | MIT | 12,05 / 11,24 / 8,33 / 5,74 |
| *référence* `htdemucs_6s` | 6 | 84 Mo | idem + guitare/piano | MIT | 11,42 / 10,59 / 8,17 / 2,63 |

Ce qu'on lit dans ce tableau :

1. **Un seul modèle public dépasse nettement HTDemucs sur la basse et la
   batterie : BS-RoFormer SW.** Environ +2,6 dB sur la basse, +2,9 sur la
   batterie, +3 sur la voix par rapport à `htdemucs_ft`, avec en plus
   guitare et piano. C'est le 6 stems par défaut d'UVR et de MVSep.
2. **Sa provenance est inconnue.** Il a été remis en ligne par `jarredou`,
   dont le compte Hugging Face est supprimé ; le miroir actuel
   ([`enerjazzer/BS-ROFO-SW-Fixed`](https://huggingface.co/enerjazzer/BS-ROFO-SW-Fixed))
   déclare `license: unknown` ; personne ne sait qui l'a entraîné, ni sur quoi,
   ni ce que veut dire « SW » ([issue openmirlab/bs-roformer-infer #3](https://github.com/openmirlab/bs-roformer-infer/issues/3)).
   Les miroirs sérieux (lumabeat, FractalSurfer) écrivent la même chose.
   Selon notre règle, licence non déclarée = **pas de redistribution** :
   l'utilisateur le télécharge à la source, nous ne republions rien. Mais
   au-delà de la licence, on ne peut pas exclure qu'il soit appris sur des
   données qu'on n'avait pas le droit d'utiliser. **Décision à prendre par
   l'auteur du projet, pas par le banc.**
3. **Les modèles de provenance propre ne battent pas `htdemucs_ft`.**
   Appris sur les 100 morceaux de MUSDB18HQ, BS-RoFormer et SCNet XL font
   jeu égal avec lui (basse 11,1-11,9 contre 12,05). C'est logique :
   HTDemucs a vu 800 morceaux de plus. À données égales, l'architecture
   RoFormer vaut environ +1 dB ; mais nous n'avons pas ces données.
4. **Mel-Band RoFormer n'existe pas en 4 stems de bonne qualité public.**
   Ses bons modèles sont des spécialistes de la voix (Kim, becruily, Gabox).
   Pour la basse, ses bandes mel larges dans le grave sont un handicap
   reconnu par l'article.
5. **Les spécialistes basse ou batterie sont payants** : les modèles
   « MVSep Bass » (BS-RoFormer 12,49, SCNet XL 13,81) et « MVSep Drums »
   ne sont servis que par mvsep.com, sans poids publiés.

## Contamination du banc

`medleydb-demix` mesure V1 et V2 de MedleyDB. MUSDB18 contient 46
morceaux de MedleyDB V1 (répartis entre ses parties d'entraînement et de
test). Donc :

- **V1** est en partie vu par tous les modèles appris sur MUSDB18HQ
  (HTDemucs compris, MSST BS-RoFormer, SCNet) ;
- **V2** n'est dans aucun jeu public connu. C'est le juge ;
- pour **SW**, on ne sait rien : il a pu voir MedleyDB V1 et V2 (MedleyDB
  est accessible sur demande). Un très bon score sur V2 serait à lire
  avec cette réserve.

## Ports existants (à ne pas réécrire)

| Projet | Langage / moteur | Modèles | Licence | État |
|---|---|---|---|---|
| [`IronHpc/UVR-rs`](https://github.com/IronHpc/UVR-rs) (`uvr-core`, `uvr-roformer` sur crates.io) | **Rust, Burn 0.21** (`burn-flex`, processeur), OpenVINO en option | BS-RoFormer « 1296 » (voix seule) | MIT | 5 étoiles, un auteur, sept. 2026 ; CPU uniquement ; parité soignée avec PyTorch (STFT en float64 : en float32, l'arrondi de la STFT est amplifié par le RMSNorm des bandes de faible énergie) |
| [`chenmozhijin/BSRoformer.cpp`](https://github.com/chenmozhijin/BSRoformer.cpp) | C++/GGML (CPU, CUDA, Vulkan, Metal) | BS- et Mel-Band RoFormer, GGUF quantifiés | MIT | 43 étoiles, actif ; SW en q8_0 : 12 s d'audio en 2,75 s sur Metal (miroir FractalSurfer) |
| `charon-audio`, `hexastem` (crates.io) | Rust + ONNX Runtime | 6 stems SW par ONNX | MIT/Apache | jeunes, ONNX Runtime |
| Exports ONNX sur HF (elicwhite, Acapellas : SW 6 stems ; xycld : viperx) | ONNX | — | poids : licence d'origine | utilisables par `ort` |
| MLX (lumabeat, MrSimmo), CoreML | Swift | SW | — | Apple seulement |

`UVR-rs` est le plus proche de ce que demande le projet : Burn 0.21, poids
`.ckpt` lus directement, BS-RoFormer complet (projection des bandes,
Transformer temps/fréquence, RMSNorm, 62 têtes de masque). Il ne fait que
le processeur et un modèle à 2 stems, mais le réseau est le même que celui
de SW (seuls `num_stems`, `dim` et `depth` changent). En phase 3, on
partirait de lui plutôt que de zéro, comme pour `demucs-rs`.

**Le risque Metal.** Le défaut wgpu jamais expliqué sur HTDemucs importé
depuis ONNX (cosinus 0,65 sur les voix) visait des opérateurs du graphe
ONNX (`InstanceNormalization`, `LayerNormalization`, `Split`…). Un
RoFormer, c'est du RMSNorm, des produits matriciels, une attention
(softmax) et des sinus/cosinus pour le RoPE, en modules Burn écrits à la
main. MuScriptor (Transformer, RoPE) et all-in-one tournent déjà sur Metal par
cette voie, avec la parité vérifiée contre leur code d'origine. Le risque existe, mais il est mesurable et la
bissection couche par couche est prévue en phase 3.

## Candidats retenus pour le banc

1. **BS-RoFormer SW** (6 stems) : le seul à pouvoir franchir le seuil de
   +1 dB, sous réserve de provenance.
2. **BS-RoFormer MSST, MUSDB seul** (4 stems) : même architecture, de
   provenance propre ; il dit ce que vaut l'architecture sans données
   inconnues.
3. **SCNet XL IHF, MUSDB seul** (4 stems) : point de comparaison hors
   RoFormer, de provenance propre.

Écartés : Mel-Band 4 stems (trop faibles), Mega 53 stems (mémoire,
provenance), spécialistes voix seule (le mode Éditer a besoin de basse et
de batterie), modèles HF sans fiche (Huge-SCNet, bs_roformer_4stems_ft :
licence déclarée mais données inconnues, à reprendre si les deux derniers
déçoivent).

## Décision et intégration (30 oct. 2026)

Banc complet : `experiments/roformer/README.md`. **SW est retenu**, à la
demande de l'auteur du projet (« le meilleur algorithme »), comme
quatrième variante de démixage (`Variante::RoformerSw`, `bsrofo_sw`), à
côté des trois HTDemucs, qui ne changent pas.

- **Ce qu'il apporte** (MedleyDB V2, apparié contre `htdemucs_ft`) :
  batterie +2,4 dB (21 morceaux sur 24), voix +2,8 dB (15 sur 15), basse
  +0,5 dB (13 sur 21) ; guitare et piano très au-dessus de `htdemucs_6s`.
  Basse transcrite par MuScriptor : F1 0,649 → 0,837 sur Slakh, justesse
  0,728 → 0,797 sur MedleyDB annoté — mais l'essentiel de ce gain vient
  de quitter `htdemucs_6s` : `htdemucs_ft` fait déjà 0,770 et 0,789. SW
  reste premier sur tous les stems de V2 devant les deux témoins appris
  sur MUSDB seul (SCNet XL deuxième, BS-RoFormer troisième). Sur la basse
  transcrite de vraie musique (MedleyDB, pYIN), il n'a plus d'avance :
  égal à `htdemucs_ft`, un peu derrière SCNet XL (trop gourmand en
  mémoire pour l'application).
- **Repli** si la provenance de SW devenait un obstacle : `htdemucs_ft`
  pour pratiquer, pas `htdemucs_6s`.
- **Où il sert** : variante par défaut du mode Éditer quand l'usage est
  *pratiquer* ; *créer* reste sur `htdemucs`, bien plus rapide.
- **Provenance** : licence non déclarée, auteur et données inconnus. Le
  `.ckpt` est téléchargé à la source (révision épinglée, SHA-256
  vérifié), jamais converti ni republié ; l'interface le dit au moment du
  choix. Si une licence ou un auteur apparaît, mettre à jour `MODELES.md`.
- **Port** : `crates/editor/src/roformer/`, parité 1,0000000 sur
  processeur et Metal contre MSST. Le risque Metal ne s'est pas réalisé.
- **Coût** : 0,69 × le temps réel en WGSL sur un M4 Pro (environ 6 min
  pour un morceau de 4 min). La même chose compilée avec `burn/metal`
  (MSL) va deux fois plus vite, avec la même parité. Contre
  `htdemucs_ft` (2,0 × le temps réel), SW coûte 2,9 fois plus en WGSL et
  1,6 fois en MSL : le critère « au plus deux fois » n'est tenu qu'en
  `burn/metal`. Bascule de toute l'application à décider : mesures par
  réseau dans `experiments/roformer/README.md` (htdemucs plus fidèle en
  MSL qu'en WGSL ; reste la parité de MuScriptor).
