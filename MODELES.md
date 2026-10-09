# Modèles et remerciements

Rusty Music s'appuie sur des modèles publiés par d'autres. Ce fichier dit
d'où vient chacun, sous quelle licence sont son code et ses poids, ce que
nous en avons fait (conversion, portage), et où nous republions les poids
convertis. Politique du projet : on adopte la licence de chaque modèle et on
s'y conforme, et on cite chaque travail (`CLAUDE.md`, `docs/rust-audio-stack.md`).

Les poids ne sont jamais dans le dépôt Git : `scripts/preparer-*.sh` les
reconstruit depuis la source, `scripts/telecharger-modeles.sh` récupère ceux
déjà préparés, et certains se téléchargent à la demande dans l'application.
Les poids convertis par nous seront republiés sur Hugging Face, sous le compte
[`rousseau`](https://huggingface.co/rousseau), avec leur licence d'origine,
quand celle-ci le permet.

| Modèle | Rôle dans l'application | Article | Code d'origine | Licence du code | Licence des poids | Ce que nous en faisons |
|---|---|---|---|---|---|---|
| **CLAP** `laion/clap-htsat-unfused` | empreintes audio et texte (Explorer, carte, Lama) | Wu, Chen, Zhang, Hui, Berg-Kirkpatrick, Dubnov, « Large-scale Contrastive Language-Audio Pretraining with Feature Fusion and Keyword-to-Caption Augmentation », ICASSP 2023 | [LAION-AI/CLAP](https://github.com/LAION-AI/CLAP), poids [HF](https://huggingface.co/laion/clap-htsat-unfused) | CC0-1.0 | Apache-2.0 | export ONNX, forme figée (`preparer-modele.sh`), traduit en Rust par `burn-onnx` au build |
| **HTDemucs** (4 stems, 6 stems, affiné) | démixage (Éditer) | Rouard, Massa, Défossez, « Hybrid Transformers for Music Source Separation », ICASSP 2023 | [facebookresearch/demucs](https://github.com/facebookresearch/demucs) ; portage Rust [`demucs-rs`](https://github.com/nikhilunni/demucs-rs) (Nikhil Unni) (fork [`rousseau/demucs-rs`](https://github.com/rousseau/demucs-rs)) | MIT ; portage Apache-2.0 | MIT | poids safetensors redistribués par [set-soft](https://huggingface.co/set-soft/audio_separation), exécutés sur Burn |
| **AERO** | super-résolution, bouton « HD » | Mandel, Tal, Adi, « AERO: Audio Super Resolution in the Spectral Domain », ICASSP 2023 | [slp-rl/aero](https://github.com/slp-rl/aero) | MIT | checkpoint du dépôt, entraîné sur MUSDB18-HQ (usage éducatif / non commercial) | export ONNX du générateur (`preparer-aero.sh`), exécuté par ONNX Runtime |
| **Beat This!** (`final0`, `small1`) | temps et premiers temps de mesure (Éditer : boucles à la mesure, puis partition et greffe) | Foscarin, Schlüter, Widmer, « Beat This! Accurate and Generalizable Beat Tracking », ISMIR 2024 | [CPJKU/beat_this](https://github.com/CPJKU/beat_this) ; portage Rust [`beat-this-rs`](https://github.com/danigb/beat-this-rs) (danigb) | MIT ; portage MIT | MIT | exports ONNX de `beat-this-rs` (révision `1ae768e` et release `model-large`), exécutés par `rten` (pur Rust) ; téléchargés au premier usage ou par `preparer-beat-this.sh` |
| **Basic Pitch** (`icassp_2022/nmp.onnx`) | transcription de la basse (Éditer → Pratiquer : tablature) | Bittner, Bosch, Rubinstein, Meseguer-Brocal, Ewert, « A Lightweight Instrument-Agnostic Model for Polyphonic Note Transcription and Multipitch Estimation », ICASSP 2022 | [spotify/basic-pitch](https://github.com/spotify/basic-pitch) | Apache-2.0 | Apache-2.0 | ONNX du dépôt, tel quel (révision `fa5997a`), exécuté par ONNX Runtime ; création de notes portée en Rust (`crates/transcription`, parité 832/832 notes avec le code d'origine) ; téléchargé au premier usage |

## À venir

Les modèles prévus par `docs/plan-editer-pratique-creation.md` (MuScriptor, ADTOF, ADT_STR, LarsNet, all-in-one, COCOLA, GrooVAE, MusicGen-Stem,
STAGE/DARC, RAVE, ACE-Step) seront ajoutés ici **au moment où ils entrent dans
le code**, avec leur licence et leurs conditions d'usage. Le tableau des
licences de la recherche est dans `docs/recherche-editer-pratique-creation.md`.

## Données d'évaluation (jamais distribuées)

Les bancs d'`experiments/` utilisent des jeux sans licence déclarée,
téléchargés en local et jamais commités : GTZAN ([`marsyas/gtzan`](https://huggingface.co/datasets/marsyas/gtzan))
et ses annotations de tempo, de temps et de premiers temps
([`TempoBeatDownbeat/gtzan_tempo_beat`](https://github.com/TempoBeatDownbeat/gtzan_tempo_beat)).
