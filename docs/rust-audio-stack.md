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
| `rustfft` | FFT de référence en Rust | MIT/Apache-2.0 | ? |
| `realfft` | surcouche FFT réelle (2× plus rapide sur du signal réel) | MIT | ? |
| `rubato` | rééchantillonnage async/sync, SIMD | MIT | ? |
| `phastft` | FFT alternative, plus économe en mémoire | MIT **ou** Apache-2.0 | ✔ |
| `ndrustfft` | FFT sur `ndarray` | MIT | ✔ |
| `fundsp` | graphe DSP, filtres, générateurs (utile module 3) | MIT/Apache-2.0 | ? |
| `dasp` | primitives de traitement d'échantillons | MIT/Apache-2.0 | ? |

### Time-stretch / pitch (module 3)
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `signalsmith-stretch` / `ssstretch` | étirement temporel et transposition, qualité production | MIT annoncée | ? |
| **Rubber Band** (`rubberband-sys`) | référence du domaine, meilleure qualité | GPL | ⭐ **à privilégier** |

### Inférence ML
| Crate | Rôle | Licence | Vérifiée |
|---|---|---|---|
| `burn` (+ `burn-onnx`) | framework tensoriel, backends CUDA/Metal/Vulkan/WebGPU/CPU + WASM | MIT **et** Apache-2.0 | ✔ |
| `ort` (+ `ort-sys`) | bindings ONNX Runtime — voie la plus courte pour un modèle pré-entraîné | MIT **ou** Apache-2.0 | ✔ |
| `candle` / `candle-onnx` | alternative Hugging Face, pur Rust | MIT **ou** Apache-2.0 | ✔ |

`ort` lie ONNX Runtime (C++). L'objectif « 100 % Rust » est abandonné au profit de « ne pas réécrire » : lier du C++ éprouvé est préférable à réimplémenter. `burn-onnx` reste une option si le pur Rust simplifie le déploiement.

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
Pas de crate Rust dédiée. Voie retenue : **HTDemucs exporté en ONNX (MIT)** exécuté via `ort` ou `burn-onnx`. ~316 Mo, GPU conseillé, découpage overlap-add à gérer soi-même.

---

## Ce qu'il reste à décider

1. **bliss-rs remplace-t-il notre pipeline d'analyse ?** Question n°1 : à évaluer avant d'écrire une ligne de descripteur.
2. **Choix du modèle d'embedding** (musicnn vs CLAP) — CLAP permet en plus la recherche texte→audio. Vérifier la licence des **poids**.
3. **Réduction de dimension** : t-SNE (`bhtsne`) au départ ; UMAP reste le maillon faible côté Rust.
