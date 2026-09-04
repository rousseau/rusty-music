# Super-résolution audio — « régénérer en HD »

État au 29 août 2026. Fait suite au bouton « E » du mode Écouter
(`docs/amelioration-audio.md`) : celui-ci *synthétise* un haut du spectre
plausible (excitation psychoacoustique) ; ce document couvre la voie qui le
*reconstruit* par un modèle appris, en rendu hors-ligne.

Sondage : `experiments/burn-aero/`. À lire d'abord.

## Le problème, en une phrase

**Un fichier compressé a perdu de l'information au-dessus de ~16 kHz — et sous
la coupure aussi, en finesse. Un modèle de bandwidth extension peut en
reconstruire une version crédible, mais c'est lourd, lent, et la licence des
poids existants bloque.**

## Ce qui est su (sondage `experiments/burn-aero/`, 30 août 2026)

- **AERO** (`slp-rl/aero`, MIT) : U-Net sur spectrogramme complexe, lignée
  HDemucs, une passe. Checkpoint musique 11,025→44,1 kHz récupéré
  (`musdb/aero-nfft=512-hl=256`, ~437 Mo).
- Export ONNX **du réseau seul** (STFT exclue) validé par
  `scripts/preparer-aero.sh` : parité `2,8 × 10⁻⁵`, graphe de 628 nœuds,
  standard, `models/aero-11025-44100.onnx` (156 Mo).
- **Moteur : `ort` (ONNX Runtime).** Mesuré : `tract` (pur Rust) charge le
  graphe mais le calcule **faux** (cos 0,68 — même motif que wgpu sur demucs).
  `ort` reproduit PyTorch au `f32` près (cos 1,000000) et tourne à **×7 le
  temps réel sur CPU** — une piste de 4 min ≈ 35 s de rendu. CoreML dispo sur
  mac si besoin.
- **Licence des poids** : entraînés sur MUSDB18-HQ (non commercial). Décidé
  (30 août) : on utilise la brique pour livrer d'abord un logiciel
  fonctionnel ; à la publication open source, soit l'app est livrée **sans les
  poids** avec un script de récupération (comme `preparer-demucs.sh`), soit la
  config est **ré-entraînée** sur corpus libre (MTG-Jamendo filtré CC BY, FMA).
  Voir `experiments/burn-aero/README.md`.

## Coût

| | Mesuré / estimé |
|---|---|
| Inférence `ort` CPU, segment 5 s | 0,70 s → **×7 le temps réel** ; piste de 4 min ≈ 35 s |
| `tract` (écarté) | ×2 le temps réel **et faux** |
| Modèle ONNX (réseau seul) | 156 Mo |
| Cache par piste (`.flac` HD) | ~10–40 Mo (4 min stéréo 44,1 kHz/16 bit) |
| Nouvelles dépendances | `ort` (+ `ort-sys` tire `ureq`/`tar`/`flate2` pour télécharger ONNX Runtime, MIT), `ndarray` |

Ce n'est **pas** une latence de lecture : rendu de fond d'une trentaine de
secondes par piste. « E » (excitation psychoacoustique) reste l'amélioration
immédiate ; ceci est un « régénérer en HD » qu'on lance et laisse tourner.

## Architecture

Calquée sur le démixage (`EtatDemix`, cache `stems/` à côté de la base,
commandes `start_demix` / `demix_state`, dock du mode Éditer).

### Moteur — nouveau crate `crates/superres`

- `preparer-aero.sh` produit `models/aero-11025-44100.onnx` (réseau seul),
  avec ses métadonnées (`rusty_music.nfft`, `.hop`, `.lr_sr`, `.hr_sr`,
  `.frames`, `.freq_bins`).
- STFT / iSTFT en Rust reproduisant `torch.stft` : Hann **périodique**,
  `normalized=True` (×1/√nfft), `center` + `reflect`, bin de Nyquist retiré
  (256 bins), ordre `(réel, imag)` sur l'axe canal. La brique FFT existe déjà
  dans `crates/player` (`spectre.rs`, `amelioration.rs`) — mais ces
  détails-là (périodique, normalized, reflect) doivent être exacts, à valider
  contre une référence PyTorch.
- Pipeline `regenerer(chemin) -> PathBuf` :
  1. décoder → à `lr_sr` = 11 025 Hz (rééchantillonnage `rubato`, déjà en
     dépendance du player) ;
  2. STFT → `[1, 2, 256, T]` ;
  3. segmenter en T fixes (862 pour 5 s) **avec recouvrement** (≥ 25 %) +
     fenêtre de fondu — `predict.py` d'AERO ne recouvre pas et laisse des
     coutures ;
  4. `ort::Session::run` par segment ;
  5. overlap-add des segments de sortie ;
  6. iSTFT → forme d'onde `hr_sr` = 44 100 Hz, un canal à la fois ;
  7. addition-recouvrement des segments avec fondu trapézoïdal, puis écriture
     d'un **WAV PCM 16 bits** dans le cache `hd/<hachage-du-chemin>.wav`
     (même format que le cache de stems ; `flacenc` produit un FLAC que
     `symphonia` ne relit pas).
- Dépendances : `ort` (`=2.0.0-rc.10`, télécharge ONNX Runtime MIT au build),
  `ndarray`. Traité comme le compromis déjà acté pour le module 3 : `ort`
  n'entre que dans ce crate. STFT sur `rustfft`, rééchantillonnage sur
  `rubato` (déjà au workspace).

### Application

- `struct EtatSuperres { en_cours, source, faits, total, resultat }` dans
  `Etat`, comme `EtatDemix` ; `superres_modele: Mutex<Option<Modele>>` (chargé
  à la première régénération, gardé) ; `hd: PathBuf` (= `db.parent()/hd`).
- Commandes : `start_superres(path)` (thread de fond), `superres_state()`
  (sondage : `faits`/`total`), `superres_disponible(path) -> bool`,
  `set_lecture_hd(actif)` (drapeau global + réouverture du morceau en cours,
  comme « E »), `superres_cache() -> (octets, n)`, `vider_cache_hd()`.
- **Aiguillage lecture** : `Player` porte un `resoudre: Fn(&Path) -> PathBuf`
  (identité par défaut). L'application l'installe sur
  `superres::resoudre(&hd, p)` — qui rend le cache si `lecture_hd()` est actif
  **et** que le WAV existe, sinon le chemin d'origine. `Player.queue` garde
  toujours les chemins d'origine (c'est eux que `current` rend, l'interface
  s'y repère) ; seule l'ouverture (`a_precharger`, `completer`, réouverture
  « E ») passe par `resoudre`.
- UI (mode Écouter) : `#hd` à côté de « E », trois états — *absent*
  (`start_superres`), *en cours* (`45 %`, désarmé), *disponible*
  (`aria-pressed`, bascule `set_lecture_hd`). La lecture HD est un drapeau de
  session ; seuls les morceaux régénérés en subissent l'effet.

### Ordre de mise en œuvre

| | Étape | État |
|---|---|---|
| 1 | Checkpoint musique + export ONNX réseau-seul (`preparer-aero.sh`) | **fait** — parité 2,8e-5 |
| 2 | Moteur d'inférence tranché (`ort`, ×7 le temps réel) | **fait** — `experiments/burn-aero/` |
| 3 | STFT/iSTFT Rust + parité contre `torch.stft` | **fait** — `crates/superres/src/stft.rs`, segment isolé rel 1e-5 |
| 4 | Boucle segments + recouvrement/fondu, parité contre `model()` sur un fichier | **fait** — `examples/verifier.rs`, rel **1,1 %** (part de la segmentation) |
| 5 | Crate `crates/superres` : `regenerer()` de bout en bout | **fait** — décode → `rubato` → segments → `ort` → iSTFT → WAV |
| 6 | Cache `hd/` + `EtatSuperres` + commandes + aiguillage lecture | **fait** — `Player::set_resolveur`, `resoudre` global, commandes `start_superres` / `superres_state` / `superres_disponible` / `set_lecture_hd` / `superres_cache` / `vider_cache_hd` |
| 7 | UI bouton « HD » (trois états) | **fait** — `#hd` dans le transport |
| — | Mélange HF : source sous sa coupure, modèle au-dessus — le HD ne peut plus ternir | **fait** — `melanger_hf` par **maximum spectral** (au-dessus de `fc`, la raie la plus forte des deux) : garantie « n'ajoute que » même si `fc` est sous-estimée. « Beng » 0–16 kHz à la décimale de l'original |
| — | Cache versionné : un correctif du son ne doit pas laisser d'anciens fichiers étouffés | **fait** — `VERSION_CACHE` dans le nom (`<hash>-v3.wav`), `purger_anciens` au démarrage et avant régénération. **À incrémenter dès que `regenerer` change.** |
| — | Axe du spectrogramme fixe (22 050 Hz) quelle que soit la fréquence de l'entrée | **fait** — `spectre::F_MAX` |
| — | Gating : prévenir quand la source est déjà pleine bande | **fait** — `regenerer` rend la coupure ; `start_superres` prévient au-dessus de 16 kHz |
| — | Barre de lecture = spectrogramme du son joué, ajout E/HD teinté | **fait** — `spectre_transport`, `#wave-cnv` |
| — | Écoute réelle : A/B MP3 96/128/192, recouvrement minimal sans couture | à faire |
| — | Fournisseur CoreML si le CPU est trop lent sur le parc visé | à faire |
| — | Licence : livrer sans les poids + script, ou ré-entraîner sur corpus libre | à la publication |

Le rééchantillonnage a demandé **deux** retouches à `rubato` 5.0, toutes deux
dans `crates/superres/src/lib.rs::reechantillonner` :

- `Fft::new` (défaut `sub_chunks ≈ 16`) donne une FFT de sortie de 64 points à
  44,1 → 11,025 kHz : coupure à 4,3 kHz au lieu de 5,3. AERO voit une entrée
  déjà sur-filtrée et rend un aigu étouffé (« Trawalc'h » de Startijenn — HD
  coupé à 8 kHz au lieu de 17). Corrigé : `new_custom` avec `sub_chunks = 1`,
  `chunk_size 16384`, fenêtre de Hann → grande FFT, coupure au bord.
- `process_all` laisse ~1 s d'amorce fausse — préfixe réfléchi jeté ensuite.

Écart au rééchantillonneur de référence (`torchaudio.resample`) : 5 × 10⁻⁴,
coupure identique (~5,3 kHz).
