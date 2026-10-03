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
- **Le gain de la correction est modeste** (+12 genres sur 300 requêtes). Le
  diagnostic détaillé figure plus bas (« Genres ») : la grande majorité des
  genres cités sont écartés par `ancrer_genres` (le texte ne les nomme pas),
  pas par le vocabulaire — une première version de ce README l'affirmait à
  tort.
- Les genres gagnés sont légitimes (« jazz hip hop comme Ezra Collective »,
  « metal comme Lamb of God »). Quelques-uns viennent d'un départ mal reconnu
  par le modèle (« After Funk », « Adam's Blues » pris pour des artistes).
- Seule inversion de polarité relevée : « … apart from radiohead » → Radiohead
  en `seed_artiste`.
- `qwen3.8:27b-mlx` n'a pas pu tourner (Metal « Insufficient Memory » sur
  25,8 Go) : c'est pourquoi le modèle par défaut est désormais le **plus
  petit** installé.

## Énergie : ancrage dans le texte (2 oct. 2026)

Constat : le modèle posait un niveau d'énergie dans 24 % des demandes qui n'en
disaient aucun (« metal comme Lamb of God » → intense, « comme Norah Jones » →
calme, parfois sans lien : « R&B comme Ella Mai » → moyenne). Or c'est un
filtre dur sur les tiers d'énergie de la bibliothèque. Remède, comme pour les
genres : `InterpretationLlm::ancrer_energie` ne garde l'énergie (de la playlist
et des parties) que si le texte **dit** une énergie — lexique
`INDICES_ENERGIE` : ambiances (calme, chill, détendu), niveaux (énergique,
upbeat, dynamique), usages (sport, dormir, dîner).

| | Avant | Après |
|---|---|---|
| Énergie posée sans être dite (300 prompts) | 23 % (70) | **0,7 % (2)** |
| Énergie conservée quand l'ambiance est annotée (53 cas) | 100 % | 43 % (23/53) |
| Ancien banc `prompts-playlist` (64 cas stricts, gemma4) | — | 64/64 |
| Genres, entités, exclusions, JSON | — | inchangés (± 1, bruit) |

Les 30 énergies écartées « à tort » étaient des adjectifs d'humeur sans mot
d'énergie (« dreamy », « angry », « sad », « depressive », « sweet »).

### Extension aux humeurs et négation (même jour)

`INDICES_ENERGIE` accueille les humeurs dont la direction d'énergie est nette
(calme : sad, triste, melancholic, dreamy, depressive, gloomy, wistful,
somber ; intense : angry, furious, rage, brutal, euphoric). Restent hors
lexique les humeurs ambiguës (« romantic », « happy », « dark », « sweet »).
Un indice précédé d'une négation dans les deux mots qui le précèdent (« not
like sad », « pas calme », « isn't too calm ») ne compte plus.

| | Ancrage seul | + humeurs et négation |
|---|---|---|
| Énergie posée sans être dite | 0,7 % (2/300) | 0,7 % (2/300) |
| Énergie conservée quand l'ambiance est annotée | 43 % (23/53) | **56 % (30/54)** |
| Ancien banc `prompts-playlist` (gemma4) | 64/64 | 64/64 |

Les 7 énergies regagnées : 5 plausibles (angry → intense, dreamy → calme,
somber melancholic → calme, sad country → calme, sad rap → calme), 1 qui suit
le genre plutôt que l'humeur (« sad rock metal metalcore » → intense), 1
contradictoire : « depressive screamo » → calme alors que le screamo est
intense. Une humeur ne vérifie pas la **cohérence** du niveau posé par le
modèle avec le genre ; c'est la limite de cette approche. Les 24 humeurs
restantes sans énergie gardée n'ont aucun mot du lexique (« sweet »,
« romantic », « psychedelic »…) : l'humeur reste portée par les étapes CLAP.

## Genres : ramener à ce que le texte dit (2 oct. 2026)

Diagnostic des 285 genres cités par gemma4 (avant correction) : **93 gardés,
184 écartés parce que le texte ne les nomme pas** (`ancrer_genres`, voulu :
c'est la garde contre les genres déduits d'une ambiance ou d'un artiste), **6
hors vocabulaire** (disco, power metal, grindcore, black metal, world) et **2
graphies collées** (« triphop », « bigbeat »). « alternative rock », « heavy
metal », « nu metal », « trip hop » et « big beat » **sont** dans le
vocabulaire de la bibliothèque (232 genres) : ils n'étaient pas inconnus.

Deux défauts réels, corrigés dans `ollama.rs` :
- `genre_nomme` ne reconnaissait pas « trip hop » dans « triphop ».
- Le modèle sur-précise (« indie » → `indie rock`, « metal » → `heavy metal`)
  ou invente un sous-genre absent (`power metal`), et le genre était alors
  perdu bien que le texte en nomme un mot. `ramener_genres` cherche, parmi les
  sous-ensembles de ses mots, le plus grand qui soit **nommé par le texte** et
  **connu de la bibliothèque** (mêmes règles de mots que le filtre).
  Garde-fous : un sous-genre doit garder la **tête** du genre (son dernier mot)
  — un qualificatif seul n'est admis que s'il est un genre à lui seul ou
  qualifie au moins deux têtes (« indie » : rock, pop ; pas « heavy », qui ne
  qualifie que « heavy metal ») ; un **refus** n'est jamais élargi à un
  qualificatif (« pas trop heavy » est une ambiance, pas `exclure heavy
  metal`) ni à un parent quand le texte le nomme en entier (« sans black
  metal » ≠ « sans metal »). Le vocabulaire se teste par mots, comme le
  filtre, au lieu de l'égalité stricte.

| (gemma4, 300 prompts) | Avant | Après |
|---|---|---|
| Genre `+` retrouvé dans `genres` [spec] | 62 % (84/135) | **76 % (103/135)** |
| Genre `+` retenu quelque part | 90 % | 93 % |
| Genre inventé [spec] / exclusion inventée | 7 / 1 | 7 / 1 |
| Genre `−` dans `exclure_genres`, inversions | 12/18, 0 | 12/18, 0 |
| Ancien banc `prompts-playlist` | 64/64 | 64/64 |

(`noter.py` reconnaît désormais « rap » ≈ « hip hop » et les graphies
collées : « avant » est rénoté avec la même règle, 58 % → 62 %.) Au rejeu, le
modèle lui-même varie un peu d'une passe à l'autre (une requête change de
réponse sans rapport avec le code) : compter ± 1-2 requêtes de bruit.
