# Banc d'empreinte (G1) : CLAP contre MuQ-MuLan et CLaMP 3

Le banc est commun à tous les modèles : un seul échantillon, un seul audio,
une seule façon de mesurer.

- **Échantillon** : 2 000 morceaux tirés au hasard (graine `0x61BA9C`) parmi
  les 22 636 dont le genre résolu figure dans le vocabulaire des familles.
  La famille y est donc le genre déclaré, jamais le repli acoustique sur
  CLAP, qui avantagerait CLAP. Rock 700, Électronique 317, Hip Hop 269,
  Folk 174, Metal 138, Jazz 126, Pop 90, Reggae 76, Soul · Funk 44,
  Classique 35, Monde 16, Chanson 15 ; 55 genres, 142 h.
- **Audio** : les cinq fenêtres de 10 s que CLAP a analysées
  (`decode::fenetres`, le décodeur de l'application), exportées telles
  quelles. MuQ-MuLan les encode une à une puis fait la moyenne, comme
  l'application pour CLAP ; CLaMP 3 reçoit les 50 s bout à bout.
- **Mesures** (`crates/analysis/examples/banc_empreinte.rs mesurer`) :
  - pureté de genre exact et de famille des 12 voisins, dans l'empreinte
    (cosinus), puis après t-SNE au réglage de l'application et au pas ×32
    (V10) ;
  - distance cosinus médiane entre morceaux d'une même famille pris dans
    des albums différents, et entre familles (P7), puis leur rapport. Les
    échelles de cosinus diffèrent d'un modèle à l'autre : seul le rapport
    se compare ;
  - texte → audio : les 10 premiers morceaux de chaque requête de
    `requetes.tsv` (30 demandes types du Lama, avec l'`etape` en anglais
    que le Lama donne à la tour texte). Tous les modèles sont fondus dans
    `data/annoter.tsv`, sans dire qui a rendu quoi ; `data/cle.tsv` garde
    les rangs par modèle.
  - coût par morceau sur ce Mac (`data/cout-*.txt`).

## Licences

| modèle | code | poids | à l'adoption |
|---|---|---|---|
| CLAP `laion/clap-htsat-unfused` | Apache-2.0 | Apache-2.0 | en place |
| MuQ-MuLan `OpenMuQ/MuQ-MuLan-large` | MIT | **CC BY-NC 4.0** | admis par la politique des poids (on adopte, on cite), jamais republiés |
| CLaMP 3 `sander-wood/clamp3` | MIT | MIT | mais l'audio passe d'abord par **MERT-v1-95M, CC BY-NC 4.0** : les termes NC viennent avec |

## Procédure

Tout ce qui se télécharge ou pèse (poids ≈ 3 Go pour MuQ-MuLan, 2,5 Go +
0,4 Go pour CLaMP 3 et MERT, fenêtres 9,6 Go, environnement Python) va sous
`RUSTY_DEV` (disque externe), jamais sur le SSD.

```bash
B=$RUSTY_DEV/banc-empreinte
# 1. échantillon et CLAP (fait le 8 nov. ; `data/` n'est pas versionné)
cargo run --release -p rusty-music-analysis --example banc_empreinte -- exporter <copie de la base> experiments/banc-empreinte/data
# 2. l'audio que CLAP a analysé (≈ 0,2 s par morceau)
cargo run --release -p rusty-music-analysis --example banc_empreinte -- fenetres experiments/banc-empreinte/data $B/fenetres
# 3. environnement
uv venv $B/venv --python 3.10 && . $B/venv/bin/activate
export UV_CACHE_DIR=<dossier sur le SSD> UV_LINK_MODE=copy   # voir les pièges
uv pip install torch torchaudio muq "transformers==4.40.0" "accelerate==0.34.0" \
  nnAudio==0.3.3 unidecode samplings abctoolkit mido soundfile tqdm
git clone https://github.com/sanderwood/clamp3 $B/clamp3-code
find $B/venv -name '._*' -delete
export HF_HOME=$RUSTY_DEV/hf WANDB_MODE=disabled
# 4. MuQ-MuLan
python experiments/banc-empreinte/muq_mulan.py experiments/banc-empreinte/data $B/fenetres
# 5. CLaMP 3 (voir l'en-tête de clamp3.py)
python experiments/banc-empreinte/clamp3.py preparer experiments/banc-empreinte/data $B/fenetres $B/clamp3
# 6. mesures et fichier à annoter
cargo run --release -p rusty-music-analysis --example banc_empreinte -- mesurer experiments/banc-empreinte/data clap muq-mulan clamp3
```

Pièges vus le 8 nov. (disque `RUSTY_DEV` en exFAT) :

- macOS y pose un fichier `._<nom>` à côté de chaque fichier écrit. Dans le
  venv, `transformers` les lit comme du Python (`UnicodeDecodeError`) ; dans
  `clamp3/wav`, ils passeraient pour de l'audio. Après chaque installation
  ou préparation : `find $B/venv $B/clamp3 -name '._*' -delete`.
- Le cache de `uv` ne s'y extrait pas (« does not appear to be a Python
  project ») : `UV_CACHE_DIR` sur le SSD (petit, effacé après) et
  `UV_LINK_MODE=copy`. L'installation dure ≈ 1 h (grappes de 256 Ko).
- `muq` 0.1.0 casse avec `transformers` 5 (`EasyDict … _attn_implementation`) :
  `transformers==4.40.0`, la version que CLaMP 3 demande aussi, avec
  `accelerate==0.34.0` ; un seul venv sert aux deux.
- Les fenêtres prennent ≈ 0,85 s par morceau vers ce disque (28 min), pas
  0,2 s.

## Critère (fixé d'avance)

Adopter seulement si **les deux** gains dépassent 10 % sur CLAP, au même
échantillon : la pureté de famille du voisinage, et le rapport distance
intra-famille / inter-familles (plus bas vaut mieux). Puis, parmi ce qui
passe, la recherche texte → audio (rang du premier morceau jugé pertinent)
et le coût par morceau.

## Résultats

### CLAP (8 nov. 2026)

| | genre | famille |
|---|---|---|
| empreinte (512 d) | 27,0 % | 44,0 % |
| t-SNE, réglage de l'application | 24,7 % | 41,2 % |
| t-SNE, pas ×32 | 25,7 % | 42,3 % |

Conservation des 12 voisins : 32,7 % (application), 38,4 % (×32). Distance
médiane intra-famille (albums différents) 0,139, inter-familles 0,192,
rapport **0,725** (0,152 / 0,205 = 0,741 sur toute la base en P7).

Plus bas que sur toute la base (49,7 % de famille dans l'empreinte en V3) :
12 voisins parmi 2 000 sont plus lointains que parmi 27 800. Le hasard
(somme des carrés des parts de famille) donne 18,6 %.

### MuQ-MuLan (8-9 nov. 2026)

| | genre | famille |
|---|---|---|
| empreinte (512 d) | 32,3 % | 51,6 % |
| t-SNE, réglage de l'application | 31,0 % | 50,2 % |
| t-SNE, pas ×32 | 31,5 % | 50,8 % |

Conservation des 12 voisins : 45,2 % (application), 45,1 % (×32).
Intra-famille 0,444, inter-familles 0,598, rapport **0,743**. Coût :
0,51 s par morceau (5 × 10 s, MPS, hors décodage).

Le même `mesurer`, relancé, donne pour CLAP après t-SNE 42,8 % (application)
et 42,6 % (×32) au lieu de 41,2 % et 42,3 % : le réglage de l'application est
désormais le pas ×32 (V10), et t-SNE varie d'un lancement à l'autre de
≈ 1 point. L'empreinte (44,0 %) et le rapport (0,725) n'ont pas bougé.

**Critère** : pureté de famille +17 % (51,6 / 44,0 dans l'empreinte ;
+17 % aussi après t-SNE), mais rapport intra / inter **moins bon** de 2,5 %
(0,743 contre 0,725). Les deux gains devaient dépasser 10 % : MuQ-MuLan ne
passe pas.

### CLaMP 3 : non mesuré, abandonné (9 nov. 2026)

Environnement et WAV préparés (`$B/clamp3/wav`, 2 000 fichiers), mais le
lancement automatique n'a pas eu le droit d'exécuter le code cloné de
CLaMP 3 (garde-fou « code externe »). François a choisi de l'abandonner
plutôt que de le lancer à la main : son audio passe de toute façon par
MERT (poids CC BY-NC).

### Décision (9 nov. 2026)

**CLAP reste l'empreinte de la carte.** MuQ-MuLan est rejeté par le critère
fixé d'avance (rapport intra / inter 0,743 contre 0,725, gain exigé de
10 %), malgré +17 % de pureté de famille ; CLaMP 3 n'est pas mesuré. La
recherche texte → audio n'est pas jugée : `data/annoter.tsv` reste sans
annotation. SongFormer (G3), qui n'avait d'intérêt que si MuQ entrait dans
l'application, est abandonné avec.

Les fichiers lourds restent sur `RUSTY_DEV` (`$B` =
`/Volumes/LaCieDock/rusty_music-dev/banc-empreinte`, et le cache
`/Volumes/LaCieDock/rusty_music-dev/hf`) : fenêtres audio (9,8 Go),
WAV de CLaMP 3, environnement Python, poids MuQ-MuLan ; à effacer quand
François le décidera.
