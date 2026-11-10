# Banc de compatibilité de stems (F2) : COCOLA contre CLAP seul et le groove

But : classer des batteries candidates pour un morceau source (« laquelle
irait avec ce mélange ? ») par un modèle appris, et confronter ce classement
à la proximité CLAP seule (le préfiltre de F1) puis à l'oreille. Rien n'est
porté en Burn avant ce jugement.

## Licences et poids (lus le 9 nov. 2026)

| | code | poids | où | entrée |
|---|---|---|---|---|
| **COCOLA** (gladia-research-group/cocola) | MIT | `COCOLA_HP_v1` (MoisesDB, Slakh2100, CocoChorales) : **licence non déclarée** | Google Drive, lien du README du dépôt | 5 s mono à 16 kHz ; scores harmonique, percussif ou les deux (`EmbeddingMode`) ; PyTorch 2.2, Lightning 2.2 |
| **Stem-JEPA** (SonyCSLParis/Stem-JEPA, ISMIR 2024) | LGPL-3.0 | **aucun poids publié** : ni lien dans le README, ni *release*, ni dépôt Hugging Face | — | WAV 4 canaux (basse, batterie, autre, voix) à 16 kHz pour l'entraînement ; instrument donné par l'indice du canal |
| **Stem-JEPA zéro-shot** (arXiv 2411.19806) | **pas de code trouvé** | **aucun poids** | article seul (CC BY 4.0) | instrument donné en texte |

Conséquences, d'après la politique du dépôt (`CLAUDE.md`, `MODELES.md`) :

- COCOLA est utilisable. Le code MIT peut être lié. Les poids, sans licence
  déclarée, se téléchargent à la source et ne se redistribuent jamais. Ils
  sont archivés sur le disque de dev, parce que le Google Drive peut
  disparaître (`docs/recherche-editer-pratique-creation.md`, risques).
- Stem-JEPA ne peut pas être évalué tel quel. Il faudrait l'entraîner
  nous-mêmes : données multipistes (MoisesDB et autres, chacune sous sa
  licence), GPU (la PGX), Hydra/Dora/Lightning. Ce n'est plus un banc de
  F2, c'est un chantier en soi. La question est posée au relecteur.

## Décision (9 nov. 2026)

Stem-JEPA n'ayant pas de poids, le banc compare **COCOLA**, **CLAP seul**
(l'ordre des voisins) et **le groove de F1** (`greffon::noter_groove` sans
intention, ajouté au rang sonore, comme l'application). 20 sources ×
10 batteries candidates, puis les premiers choix rendus pour une écoute à
l'aveugle.

## Procédure

Tout ce qui pèse est sur le disque de dev (`$F` =
`/Volumes/LaCieDock/rusty_music-dev/essais/F2`) ; la base lue est une copie
(`essais/V23/donnees/rusty-music.db`). COCOLA tourne dans un environnement
Python à part (`ext/cocola-venv`, dépôt cloné dans `ext/cocola`, poids dans
`ext/cocola-poids/`), pour le banc seulement : rien n'est porté en Burn
avant l'écoute.

1. `choisir.py <base> $F/plan-sources.json 28` : 28 sources candidates, une
   par famille CLAP et par tour, les plus populaires ; pas de paires.
2. `banc_compat` (exemple de `rusty-music-editor`) sur ce plan : démixage
   HTDemucs (la variante de Créer) et pulsation Beat This! des sources.
3. `batteries.py $F/plan-sources.json $F $F/exclus.txt 0.20` : écarte les
   sources dont la batterie démixée porte moins de 20 % du RMS (guitare-
   voix, interlude, percussions légères). Les 20 premières gardées vont
   dans `$F/sources.txt`.
4. `CHOISIR_SOURCES=$F/sources.txt choisir.py <base> $F/plan.json 20` :
   10 candidats par source, plus proches au cosinus CLAP, tempo replié à
   ±10 %, autre artiste, une édition par titre.
5. `banc_compat $F/plan.json $F` : démixe les candidats, écrit
   `greffes/<source>/contexte.wav` (la source sans batterie) et
   `greffes/<source>/<candidat>.wav` (la batterie du candidat posée mesure
   par mesure, `greffer_par_mesures`, sans sections ; d'un bloc si la
   métrique diffère).
6. `groove_stems` (exemple de `rusty-music-analysis`) sur `$F/stems` :
   `$F/groove.md`.
7. `cocola.py` : score COCOLA (`BOTH` et `PERCUSSIVE`) de chaque batterie
   posée contre le contexte, huit fenêtres de 5 s entre 20 et 80 % du
   morceau ; la batterie d'origine est notée aussi, en témoin.
8. `classer.py` : les trois classements, leurs accords (`classement.md`),
   et `ecoute/` : un extrait de 30 s par premier choix distinct, lettres
   tirées au sort, l'extrait d'origine (`NN-ref.wav`) pour situer ; la clé
   est dans `$F/cle.json`, hors du dossier d'écoute.
9. Après l'écoute : `correler.py $F` lit `ecoute/notes.tsv`.

## État (10 nov. 2026)

Étapes 1 à 6 faites, et le classement CLAP · groove (`classer.py` avec un
`cocola.json` vide) : même premier choix pour 10 sources sur 20, tau de
Kendall moyen +0,61. COCOLA n'a pas tourné : l'exécution du code cloné et
du checkpoint (pickle) attend un accord explicite. Ensuite : `cocola.py`
(environnement `/Volumes/cocola-env/venv`, image
`ext/cocola-env.sparsebundle` à monter), `classer.py`, écoute.
