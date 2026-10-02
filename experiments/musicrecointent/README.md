# MusicRecoIntent → banc d'essai de Lama

Mesure, sur de vraies requêtes, si `ollama::interpreter` traduit bien une
demande en filtres typés — surtout **la polarité** (voulu `+`, rejeté `−`,
« comme X » `~`).

Données : [MusicRecoIntent](https://github.com/deezer/MusicRecoIntent-NLP4MusA26)
(Deezer, NLP4MusA 2026, arXiv 2602.12301) : 2 291 requêtes Reddit en anglais,
3 935 descripteurs annotés. **Licence non précisée** par le dépôt : rien n'est
copié ici, `data/` est ignoré par git, `preparer.py` télécharge le CSV.

```bash
python3 experiments/musicrecointent/preparer.py 300            # data/cas.json (46 négations + 254 au hasard)
cp <base>.db /tmp/lib.db                                       # jamais la vraie base
ROB_CAS=experiments/musicrecointent/data/cas.json \
  cargo run --release -p rusty-music-core --example robustesse -- \
  /tmp/lib.db <modele> experiments/musicrecointent/data/sortie-<modele>.json
python3 experiments/musicrecointent/noter.py experiments/musicrecointent/data/sortie-*.json
```

`noter.py` ne note que ce que le schéma exprime (Genre ±, NE ±/~, Decade) et
lit chaque sortie de deux façons : **[brut]** ce que le modèle a dit,
**[spec]** ce qui reste après `normaliser` (genres vidés si un départ est
nommé, puis filtrés par le vocabulaire de la bibliothèque).

## Résultats — gemma4:e4b (le plus petit modèle installé), 2 oct. 2026

300 prompts, 0 erreur JSON, 2,1 s médiane. « Avant » / « après » = la règle
`est_trajet` de `normaliser` corrigée (un **départ** seul vidait `genres` ;
désormais seule une **arrivée** nommée en fait un trajet). Sorties :
`data/sortie-gemma4-e4b-avant.json` et `-apres.json`.

| Mesure ([spec], ce qui reste après `normaliser`) | Avant | Après |
|---|---|---|
| Genre `+` retrouvé dans `genres` | 55 % (74/135) | **59 % (80/135)** |
| Genres cités par le modèle qui atteignent les filtres | 88/283 | **100/282** |
| Requêtes qui gagnent / perdent un genre | — | 10 / 0 |
| Genre `+` retenu quelque part (genres ou étapes) | 88 % | 88 % |
| Genre `−` dans `exclure_genres` | 67 % (12/18) | 67 % |
| Genre `−` rendu en positif (inversion) | 0/18 | 0/18 |
| Entité nommée → seed ou étape (≥ 1) | 87 % (156/180) | 86 % (155/180) |
| Entité `+`/`~` exclue à tort | 0/343 | 0/343 |
| Entité `−` dans `exclure_artistes` | 2/4 | 2/4 |
| Décennie exacte (±1 an) | 6/8 | 6/8 |
| Exclusion / seed / année / durée inventés | 1 / 1 / 0 / 0 sur 300 | idem |
| Énergie posée sans être dite | 24 % (71/300) | 23 % (70/300) |

À retenir :
- **Les négations sont rares** (46 requêtes sur 2 291) : les taux `−` reposent
  sur 18 genres et 4 entités, à lire comme des indices.
- **Le gain de la correction est modeste** (+12 genres sur 300 requêtes) : la
  plupart des genres cités n'atteignent pas les filtres à cause de deux autres
  garde-fous voulus — `ancrer_genres` (un genre que le texte ne nomme pas est
  écarté) et `restreindre_au_vocabulaire` (un genre inconnu de la
  bibliothèque viderait la sélection : « power metal », « grindcore »,
  « disco », « big beat », « trip hop » ne sont pas dans le vocabulaire
  réel). Un premier constat de ce README (« 93 requêtes sur 151 perdent leurs
  genres par `est_trajet` ») était exagéré : beaucoup de ces genres auraient été
  écartés par ces deux garde-fous de toute façon.
- Les genres gagnés sont légitimes (« jazz hip hop comme Ezra Collective »,
  « metal comme Lamb of God »). Quelques-uns viennent d'un départ mal reconnu
  par le modèle (« After Funk », « Adam's Blues » pris pour des artistes).
- **Énergie** : souvent une déduction plausible (grindcore → intense, Norah
  Jones → calme), parfois non (R&B → moyenne) ; c'est un filtre dur, non traité.
- Seule inversion de polarité relevée : « … apart from radiohead » → Radiohead
  en `seed_artiste`.
- `qwen3.8:27b-mlx` n'a pas pu tourner (Metal « Insufficient Memory » sur
  25,8 Go) : c'est pourquoi le modèle par défaut est désormais le **plus
  petit** installé.
