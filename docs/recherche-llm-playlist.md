# Texte → playlist : état de l'art et direction retenue

Recherche du 1ᵉʳ octobre 2026, pour le champ d'intention d'Explorer
(`ollama::interpreter`, `path_texte`). Question : comment relier la souplesse
d'un LLM à la richesse de la bibliothèque ?

## Principe retenu

**Le LLM traduit, le code exécute.** Le LLM transforme une demande en langage
naturel en une *spec* d'arguments typés et bornés ; du code déterministe applique
cette spec à la bibliothèque. Le LLM ne nomme **jamais** de morceaux à jouer :
il en invente (hallucination), et la bibliothèque est un catalogue fermé.

## Niveau de confiance des sources

- **Lus en détail** : Text2Playlist (HTML), MusicRecoIntent (HTML), TalkPlay (HTML),
  TalkPlay-Tools (HTML), doc Ollama sur les sorties structurées, code
  d'AudioMuse-AI (`tasks/ai/tools.py`, `app_chat.py`, résumés par un petit modèle).
- **Résumés seulement** : JAM, « From Queries to Playlists » (chiffres tirés de
  l'abstract).
- **Confiance faible** (extraction de PDF vague) : Melo, MuChator, WeMusic-Agent,
  Two Views One Voice, Reddit2Deezer.
- **Inaccessibles** : pages ACM (403), MusicSem.

## Cinq familles d'approches

| Famille | Exemples | Principe | Pour nous |
|---|---|---|---|
| Extraction de tags puis recherche | Text2Playlist (Deezer, arXiv 2501.05894) | Le LLM extrait tags explicites et implicites, un index les retrouve, un second LLM réordonne pour la diversité | Proche de nous ; demande des tags de qualité (MusicBrainz, Last.fm) |
| Appel d'outils | AudioMuse-AI, TalkPlay-Tools (2510.01698) | Le LLM choisit des outils et leurs arguments, le code les exécute | La plus souple |
| Recherche vectorielle sémantique | « From Queries to Playlists », JAM (2507.15826) | Requête et morceaux dans un même espace, plus proches voisins | C'est notre CLAP |
| Génération de jetons de morceaux | TalkPlay (2502.13713) | Un LLM entraîné produit des identifiants | Écarté : catalogue fermé, ré-entraînement à chaque ajout |
| LLM qui répond de mémoire | « direct LLM generation » | Le LLM propose des titres | Écarté : hallucinations |

« From Queries to Playlists » : précision@10 de 64 % (facettes), 74 % (génération
directe), 81 % (recherche vectorielle).

## Ce que chaque travail apprend

- **Text2Playlist** : trois étapes (tags, personnalisation, réordonnancement). En
  ligne, 45 % des playlists générées sont écoutées contre 27 % pour les playlists
  faites à la main. Limites reconnues : pas de dialogue, couverture des tags,
  aucun prompt ni ablation publiés.
- **MusicRecoIntent** (2 291 requêtes Reddit, arXiv 2602.12301) : une demande
  contient des descripteurs **désirés** (`+`), **indésirables** (`−`) et de
  **référence** (`~`, « comme X »). Les LLM testés via Ollama (Gemma 3, Qwen 3,
  Llama 3, Mistral) plafonnent à F1 ≈ 0,69 ; le contexte d'écoute (« pour
  travailler ») est le pire (F1 0,45) ; la négation mêlée à une intention
  positive est le piège classique. → Notre schéma doit exprimer l'exclusion et
  la référence, et on doit s'attendre à des erreurs d'un petit modèle
  (d'où l'inspecteur éditable).
- **TalkPlay-Tools** (Qwen3-4B, 1 000 conversations synthétiques) : le LLM
  écrit du **SQL** → **27,4 %** de succès seulement. Hit@10 : 0,073 (BM25 seul)
  contre 0,082 (appel d'outils) — gain réel mais modeste. → Pas de SQL libre :
  arguments typés.
- **AudioMuse-AI** (le projet que Rusty Music allège) : un seul appel LLM, quatre
  outils — `seed_search` (union / alchemy / subtract / journey), `text_match`
  (CLAP), `knowledge_lookup`, `search_database`. Ce dernier a des arguments
  typés et bornés : genres pris dans le vocabulaire réel, énergie 0–1 convertie
  en **percentile de la bibliothèque**, `exclude_artists`/`exclude_genres`,
  durée, année, BPM. Autour du LLM, le code fait : troncature à la durée
  (`_trim_to_duration`, ×1,05), plafond par artiste relâché progressivement,
  échantillonnage proportionnel entre outils, ordre lissé par tempo/énergie/
  tonalité. **Aucun repli inventé** : rien trouvé → message qui invite à nommer un
  artiste ou un genre. Pas de dialogue multi-tours.
- **Biais** (arXiv 2508.20401) : les recommandations LLM en démarrage à froid
  montrent des biais culturels et de genre.

## Notre position

| | AudioMuse-AI | Rusty Music |
|---|---|---|
| Filtres typés (genre, BPM, énergie, année, durée) | oui | oui (depuis ce chantier) |
| Exclusions (artistes, genres) | oui | oui |
| Durée en minutes | oui | oui |
| Plafond par artiste | oui | oui |
| Ordre lissé tempo/énergie/tonalité | oui | non (plus tard) |
| Recherche par description (CLAP) | oui | oui |
| Trajet par étapes descriptives successives | non | **oui** (`Graphe::guidee`) |

## État de l'implémentation (1ᵉʳ octobre 2026)

Livré : spec typée (`InterpretationLlm`), schéma JSON dans `format` +
température 0, vocabulaire de genres réel dans la consigne, filtres
(`crates/core/src/filtres_playlist.rs`), `composer_route` (`apps/desktop/src/main.rs`) :
filtres → marche guidée → plafond par artiste → durée, relâchements affichés,
bloc « Contraintes » et champ « Durée » dans l'inspecteur. Détail et décisions :
`docs/ui-spec.md` (« Le LLM traduit, le code exécute »).

Essai de bout en bout (vrai Ollama `gemma4:e4b-mlx`, vrais CLAP-texte et
empreintes, 5 477 morceaux sous-échantillonnés sur 27 425) :
- « calme, 60 minutes, sans rock » → 11 morceaux, 59 min, aucun rock, énergie ≤ 0,18 ;
- « une heure d'énergique pour courir » → 14 morceaux, 56 min ;
- « jazz peu connus, 20 morceaux » → 20 morceaux, tous jazz.

Limites constatées :
- **« Commence doucement et monte vers… » n'est pas une courbe.** Seules les
  étapes CLAP orientent la marche ; l'énergie obtenue (0,04 → 0,27) monte à
  peu près, sans garantie. Une vraie courbe d'énergie (cible par position)
  reste à faire.
- **« Intense » est relatif** : le tiers haut des énergies de cette
  bibliothèque commence vers 0,26 (valeur efficace). Cohérent avec le choix
  d'un percentile, mais pas toujours ce que « pour courir » évoque.
- **Qualité des tags** : un morceau étiqueté « rock » à 175 BPM d'un groupe de
  musique éthérée remonte tel quel — la sélection ne vaut que ce que valent les
  genres résolus (MusicBrainz, Last.fm, tag du fichier).
- **Interface non vérifiée à l'écran** : le JavaScript passe `node --check`, la
  logique moteur est testée (29 tests du bureau, 15 du cœur), mais le bloc
  « Contraintes » n'a pas été vu dans l'application.

## Robustesse mesurée (1ᵉʳ octobre 2026)

Jeu de **53 prompts annotés** (`experiments/prompts-playlist/prompts.json` :
durée, exclusions, époques, énergie, tempo, popularité, genres, trajets,
plafond, combinés, anglais, fautes de frappe, injection, prompt vide de sens,
prompt de 70 mots). Deux bancs :

```bash
# 1. interprétation (vrai Ollama) — écrit res.json ; ROB_IDS=a,b pour en rejouer quelques-uns
cargo run --release -p rusty-music-core --example robustesse -- copie.db gemma4:e4b-mlx res.json
# 2. composition sur les interprétations de res.json (vrai CLAP-texte, vraies empreintes)
RUSTY_DB=copie.db RUSTY_RESULTATS=res.json cargo test -p rusty-music-desktop \
  robustesse_de_la_composition -- --ignored --nocapture
```

Toujours sur **une copie** de la base (l'ouverture migre le schéma).

| | 1ʳᵉ mesure | Après correctifs |
|---|---|---|
| Interprétation conforme (`gemma4:e4b-mlx`, T = 0) | 46/53, 6 erreurs | **51/53, 0 erreur** |
| Composition sans problème | 46/53 | **53/53**, 0 panique |
| Latence d'une interprétation | médiane 1,7 s | médiane 1,6 s, max 5,9 s |

Quatre défauts trouvés et corrigés grâce à ce banc :

1. **Boucle de décodage** (6 prompts sur 53 ; 4 sur 5 de ceux qui fixent une
   année) : le modèle écrit une spec correcte puis des *espaces* à l'infini à la
   place de la clé requise suivante, jusqu'au délai de 300 s (cache qui grossit,
   interface figée). Corrigé par `num_predict: 400` et `reparer_json_tronque`
   (referme le JSON coupé ; les clés absentes retombent sur `serde(default)`).
   Rendre les clés facultatives **aggrave** : le modèle n'en remplit presque
   plus (20/53 conformes), d'où des champs requis.
2. **« Sans rock » n'écartait que le genre exactement « rock »** : 8 morceaux sur
   20 étaient `alternative rock`, `hard rock`… Correspondance par mots
   (`genre_correspond`) : « rock » couvre sa famille, pas « rocksteady ».
3. **« Sans rap » laissait passer le hip hop** (et inversement) ; et un genre
   composé rendu par le modèle (« rap/hip hop ») était lu comme une conjonction.
   Alias rap ↔ hip hop, alternatives séparées par `/`.
4. **« Sans James Brown » laissait passer ses collaborations** (« James Brown &
   The Famous Flames »). Chaque artiste d'une mention est comparé séparément ;
   « Prince » n'écarte pas « Prince Royce ».

Écarts restants (non corrigés) :
- « Rythme lent » → `bpm_max` 120, qui n'est pas lent ; l'annotation en attend
  100. À surveiller plutôt qu'à durcir.
- Le prompt long (« doux, sans paroles trop présentes… ») fait **inventer** des
  genres (`ambient`, `classical`, `jazz`) que l'utilisateur n'a pas demandés.
  La composition les assouplit et le dit, et chacun se retire d'un clic dans
  l'inspecteur — mais c'est le type d'erreur le plus probable en usage réel.
- **Non mesuré** : un seul modèle (`gemma4:e4b-mlx`). `qwen3.8:27b-mlx` (18 Go)
  plante en mémoire GPU sur ce Mac de 24 Go (« Insufficient Memory ») même seul
  et avec `num_ctx` réduit : inutilisable ici, pas un défaut du code. Pas non plus
  de test de stabilité par paraphrase, ni de jugement musical sur les listes
  produites (les invariants vérifiés sont mécaniques : exclusions, durée,
  plafond, bornes, doublons).
- La composition a aussi été rejouée sur la **bibliothèque entière** (27 385
  morceaux placables, sans sous-échantillon) : 53/53 sans problème, 0 panique,
  composition de 10 ms à 1,4 s par playlist (graphe des voisins déjà bâti).

## Redécouverte (pistes, hors chantier actuel)

- `added_at` existe, mais **aucun historique d'écoute** n'est stocké : « morceaux
  oubliés » demanderait d'abord une table d'écoutes. En attendant, « peu connu »
  passe par `track_popularite.relative`.
- Étagères d'hypothèses (Spotify, arXiv 2607.25823) : le LLM propose des thèmes,
  le code les réalise sur la bibliothèque.
- Légende générée de la playlist : Deezer mesure un gain d'engagement en A/B
  (arXiv 2606.22460).
- Raffinement multi-tours (« plus calme ») : modifier la spec existante.

## À tester plutôt que supposer

Plan d'outils en tableau JSON contraint par `format` (ce que fait ce chantier)
contre appel d'outils natif d'Ollama : un ticket (ollama/ollama#8095) signale
qu'une sortie structurée combinée à des outils rend des `tool_calls` vides.
