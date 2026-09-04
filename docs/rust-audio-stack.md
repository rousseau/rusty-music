# Pile audio Rust — inventaire

Objectif : **utiliser les meilleurs outils existants plutôt que réécrire**. Le projet est publié sous **GPL-3.0-or-later**, ce qui lève toute contrainte de licence sur les dépendances open source.

## Politique de licence — DÉCIDÉE

Le projet est distribué sous **GPL-3.0-or-later**. Conséquences :
- **Toute dépendance open source est acceptable** : MIT, Apache-2.0, BSD, MPL-2.0, LGPL, GPL-2.0-or-later, GPL-3.0, AGPL-3.0.
- Une seule vraie exclusion demeure : les licences **non-commerciales ou non-libres** (CC BY-NC-*, « research only », « non-commercial use »). Elles ne sont pas compatibles GPL et ne sont pas open source.
- Point de vigilance restant : **GPL-2.0-only** (sans « or later ») est incompatible avec GPL-3.0. Cas rare, mais à vérifier.
- Attention aussi aux **poids de modèles**, dont la licence est distincte du code : plusieurs modèles du zoo Essentia sont en CC BY-NC-SA. À vérifier modèle par modèle.

`deny.toml` ne sert donc plus à filtrer les licences copyleft, mais à repérer les licences non libres, les avis de sécurité et les doublons de versions.

**Corollaire, plus important que la règle elle-même : ne pas réécrire ce qui existe.** Chaque brique ci-dessous marquée « à privilégier » remplace du code à ne pas écrire.

---

## Les briques débloquées par le passage en GPL

Toute la chaîne MIR classique, jusqu'ici hors de portée, devient disponible — et elle couvre une large part du module 2 :

| Brique | Ce qu'elle fait | Licence | Statut |
|---|---|---|---|
| **bliss-audio** (`bliss-rs`) | analyse de morceaux et playlists par similarité, **en Rust** — c'est notre module 2, déjà écrit | GPL-3.0-only | ⭐ **à évaluer en priorité** |
| **aubio** (`aubio-rs`) | onset, tempo, pitch, MFCC — standard du domaine | GPL-3.0 | ✅ disponible |
| **Essentia** (MTG) | référence MIR : BPM, tonalité, timbre, embeddings musicnn | AGPLv3 | ✅ disponible (approche AudioMuse redevenue reproductible) |
| **Rubber Band** | référence du time-stretch / pitch | GPL | ✅ disponible |
| **contour-isobands** | isobandes, plus rapide que `contour` | AGPL-3.0 | ✅ disponible |

**Conséquence directe** : la stratégie « descripteurs maison réimplémentés sur `rustfft`/`rubato` » n'a plus lieu d'être en première intention. Commencer par évaluer bliss-rs, et ne réécrire que ce qu'il ne couvre pas.

---

---

## Étage par étage

Colonne « vérifiée » : ✔ = licence lue dans une source lors de cette recherche · ? = à confirmer via `cargo deny`.

### Décodage et conteneurs
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `symphonia` | décodage pur Rust : FLAC, MP3, AAC, ALAC, OGG/Vorbis, WAV, AIFF, MP4/MKV | MPL-2.0 | ✔ |
| `hound` | lecture/écriture WAV | Apache-2.0 | ? |
| `claxon` | FLAC seul (plus léger que symphonia) | Apache-2.0 | ? |
| `lofty` | tags, agnostique au format (déjà utilisé dans le cœur) | MIT/Apache-2.0 | ? |

`symphonia` (MPL-2.0) : plus aucun problème, la question est close.

### Lecture et sortie audio
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `cpal` | accès bas niveau aux périphériques audio | Apache-2.0 | ? |
| `rodio` | lecture simple par-dessus cpal (module 1) | MIT **ou** Apache-2.0 | ✔ |
| `kira` | moteur audio orienté mixage, fondus, effets — meilleur candidat pour le module 3 | MIT/Apache-2.0 | ? |

### DSP, spectral, rééchantillonnage
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `rustfft` | FFT de référence en Rust | MIT/Apache-2.0 | ✔ (module 1 : spectrogramme, excitation « E ») |
| `realfft` | surcouche FFT réelle (2× plus rapide sur du signal réel) | MIT | ? |
| `rubato` | rééchantillonnage async/sync, SIMD | MIT | ✔ (module 1 : rééchantillonnage de qualité vers la carte son — `rodio` 0.22 ne fait que du linéaire) |
| `phastft` | FFT alternative, plus économe en mémoire | MIT **ou** Apache-2.0 | ✔ |
| `ndrustfft` | FFT sur `ndarray` | MIT | ✔ |
| `fundsp` | graphe DSP, filtres, générateurs (utile module 3) | MIT/Apache-2.0 | ? |
| `dasp` | primitives de traitement d'échantillons | MIT/Apache-2.0 | ? |

### Time-stretch / pitch (modules 1 et 3)
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| **`wsola`** | étirement temporel à hauteur préservée (recouvrement-addition), temps réel, pur Rust, zéro dépendance transitive | MIT | ✔ **retenu** (module 1 : vitesse de lecture ; module 3 : `editor/src/etirement.rs`, greffe). Hauteur inchangée à 441 Hz de 0,5× à 2×. |
| `signalsmith-stretch` / `ssstretch` | qualité production, C++ lié | MIT | écarté : `wsola` suffit et reste pur Rust |
| **Rubber Band** (`rubberband-sys`) | référence du domaine | GPL | non retenu — `wsola` couvre le besoin |

La transposition, que `wsola` ne fait pas, s'obtient en étirant puis en
rééchantillonnant du même rapport (`rubato`). Un vocodeur de phase maison
(~500 lignes) avait été écrit avant de vérifier `wsola` ; il a été retiré.

### Inférence ML
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `burn` (+ `burn-onnx`) | framework tensoriel, backends CUDA/Metal/Vulkan/WebGPU/CPU + WASM | MIT **et** Apache-2.0 | ✔ |
| `ort` (+ `ort-sys`) | bindings ONNX Runtime — voie la plus courte pour un modèle pré-entraîné | MIT **ou** Apache-2.0 | ✔ (module 1 : super-résolution AERO, `docs/module3-superresolution.md`) |
| `tract` | inférence ONNX **pur Rust** | MIT **ou** Apache-2.0 | ✘ — charge le graphe AERO mais le calcule **faux** (cos 0,68), comme wgpu sur demucs (`experiments/burn-aero/`) |
| `candle` / `candle-onnx` | alternative Hugging Face, pur Rust | MIT **ou** Apache-2.0 | ✔ |

`ort` lie ONNX Runtime (C++). L'objectif « 100 % Rust » est abandonné au profit de « ne pas réécrire » : lier du C++ éprouvé est préférable à réimplémenter. Deux sondages le confirment sur des modèles audio réels — HTDemucs (`experiments/burn-demucs/` : wgpu faux) et AERO (`experiments/burn-aero/` : `tract` faux, `ort` exact à ×7 le temps réel).

### Recherche de voisins, clustering, projection
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `instant-distance` | HNSW pur Rust — voisins soniques, chemins entre morceaux | MIT **et** Apache-2.0 | ✔ |
| `hnswlib-rs` / `hnsw` | autres implémentations HNSW pur Rust | MIT/Apache-2.0 | ? |
| `linfa` | K-Means, DBSCAN | MIT/Apache-2.0 | ? |
| `bhtsne` | t-SNE Barnes-Hut pur Rust | MIT | ? |
| *UMAP* | — | pas d'implémentation Rust mature | ⚠️ point ouvert |

La projection 2D reste le maillon faible côté Rust. Démarrer en t-SNE (`bhtsne`) et garder UMAP comme amélioration ultérieure.

### Rendu topographique de la carte (module 2)
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `contour` (mthh/contour-rs) | isolignes + **isobandes** par marching squares, portage de d3-contour | MIT/Apache-2.0 | ✔ |
| `kiddo` / `kdtree` | k-d tree pour l'estimation de densité par noyau | MIT/Apache-2.0 | ? |
| `contour-isobands` | isobandes seules, plus rapide, calcul parallèle | AGPL-3.0 | ✅ disponible |

### Rendu et coquille applicative
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `wgpu` | WebGPU/WebGL, cible native et navigateur | MIT/Apache-2.0 | ? |
| `tauri` | app de bureau, backend Rust + frontend web | MIT/Apache-2.0 | ? |
| `egui` | UI immédiate Rust (repli si Tauri ne convient pas) | MIT/Apache-2.0 | ? |

### Démixage (module 3)
Retenu : **`demucs-core`** (fork de `demucs-rs` épinglé, Apache-2.0), poids HTDemucs
(Meta, MIT). La STFT reste en Rust, Burn ne reçoit que le réseau. L'export ONNX
+ `ort`/`burn-onnx` a été **écarté** : il déroulait la transformée de Fourier en
milliers de nœuds (66 % du graphe) et le backend GPU rendait un résultat faux.
Détail : `docs/module3-demixage.md`.

### Super-résolution (bouton « HD »)
Retenu : **AERO** via **`ort`** (ONNX Runtime, C++). `tract` (ONNX pur Rust)
charge le graphe mais le calcule faux (cos 0,68). `crates/superres`,
`docs/module3-superresolution.md`.

---

## Ce qui a été tranché

1. **Pipeline d'analyse : écrit ici, pas bliss-rs.** Descripteurs maison (flux
   spectral + autocorrélation, chroma Krumhansl-Schmuckler) — voir `suite.md` §6 bis.
2. **Modèle d'empreintes : CLAP** (`laion/clap-htsat-unfused`, poids Apache-2.0),
   traduit d'ONNX par `burn-onnx`.
3. **Réduction de dimension : t-SNE Barnes-Hut** (`bhtsne`, pur Rust).
