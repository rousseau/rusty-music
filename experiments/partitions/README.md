# Banc de la basse contre des partitions publiées

Transcription de basse de l'éditeur (`crates/transcription`) comparée aux
livres de partitions « Bass Recorded Versions » (Hal Leonard) : notation +
tablature écrites d'après l'enregistrement. **Usage local et d'évaluation
seulement** : les livres sont des œuvres protégées ; ni eux, ni leurs pages,
ni ce qui en est extrait (MusicXML, tablatures lues, `reference.json`) ne
sont versionnés. Seuls le code et la liste des morceaux (titres, pages) le
sont.

## Chaîne

1. **`omr.py`** — chaque page rendue à 300 dpi ; la tablature y est lue
   (`lire_tablature.py`) puis effacée (`nettoyer_page.py`) ; les pages
   nettoyées sont reconnues par [Audiveris](https://github.com/Audiveris/audiveris)
   5.11 (AGPL-3.0, outil externe, jamais lié) → MusicXML. Sans l'effacement,
   Audiveris prend l'interligne de la tablature pour celui des portées et rate
   toutes les ligatures (toutes les durées à la noire). Si une page fait
   échouer le livre, reprise page par page.
2. **`reference.py`** — la ligne de basse : la partie en clé de fa de chaque
   mesure (Audiveris ne garde pas une partie par instrument d'un système à
   l'autre), hauteur réelle = écrite − 12. Corrections :
   - **armure** : la plus chargée des portées d'un même système (Audiveris
     oublie des dièses : 0, 2, 3 ou 4 lus pour 4) ; accidents jusqu'à la fin
     de la mesure ;
   - **hauteurs et doigtés pris sur la tablature** : les mesures de la
     notation et de la tablature sont alignées (programmation dynamique sur la
     ressemblance des hauteurs), puis note à note. La tablature dit la corde
     et la frette — vérité des hauteurs (Audiveris rate les bécarres) et des
     doigtés. Un chiffre rattaché à la corde voisine (tablature et notation à
     une quarte juste d'écart, 4 % des notes) est remis sur sa corde ;
   - mesure **sans portée de basse** (« w/ Bass Fig. 1 », tacet) : inconnue,
     retirée de la référence ; mesure écrite en silences : silence.
3. **`banc.py`** — six stems (`rusty-music demix --modele htdemucs_6s`),
   pulsation sur le mélange (`verif_pulsation`), transcription du stem de
   basse (exemple `banc` du crate : la chaîne de l'éditeur), comparaison.
4. **`comparer.py`** — une partition écrite n'est pas une chronologie
   (reprises, D.S. al Coda, figures rejouées, silences de plusieurs mesures
   comptés pour une) : nos mesures sont alignées sur les mesures écrites par
   un Viterbi (mesure suivante gratuite, saut n'importe où pénalisé, rester
   sur une mesure vide, état « hors partition »), avec la phase du premier
   temps (0 à 3 temps) choisie au mieux. Notes : même double croche (ou ±1)
   et même hauteur.

Réglages : `regler_basic_pitch.py` (grille Basic Pitch × porte, exemple
`banc -- reglages`), `regler_doigtes.py` (grille des coûts de doigté,
`banc -- doigtes`).

```sh
AUDIVERIS=/chemin/Audiveris.app/Contents/MacOS/Audiveris python3 omr.py
python3 preparer_audio.py          # stems et pulsations (long)
python3 banc.py --refaire-transcription
python3 regler_basic_pitch.py
python3 regler_doigtes.py
```

Variables : `LIVRES` (défaut `~/Exp/tmp`), `CACHE` (`~/Exp/tmp/banc-partitions`),
`MUSIQUE` (`~/Music/MyMusic`), `RUSTY_DB`. Python : PyMuPDF, Pillow, NumPy,
SciPy.

## Livres

- *Red Hot Chili Peppers — Californication (Bass)* : scan CCITT 600 dpi, net.
  Les 15 morceaux de l'album, tous dans la bibliothèque. **Le corpus.**
- *Red Hot Chili Peppers — Greatest Hits (Bass Recorded Versions)* : scan
  basse résolution, chiffres de tablature de 15 à 20 px — 5/6 et 0/8/9
  indiscernables (essayé : modèles de police, regroupement des glyphes du
  livre). Notation seule, pour deux morceaux absents du premier livre
  (« Under the Bridge », « Give It Away »).

## Résultats (10 oct. 2026)

F1 : attaque à la même double croche et même hauteur. **F1tab** : sur les
seules mesures dont la hauteur vient de la tablature (référence la plus sûre).
Doigté : même corde et même frette, parmi les hauteurs justes. Silences :
mesures écrites en silences où nous écrivons des notes.

| | F1 | F1±1 | F1tab | doigté | silences |
|---|---|---|---|---|---|
| Avant (chaîne du 10 oct. matin) | 0,450 | 0,527 | 0,479 | 0,51 | — |
| Après | **0,48** | **0,55** | **0,51** | **0,61** | 6/89 |

Ce qui a changé, mesuré au banc :

- **Porte d'énergie** (`crates/transcription/src/porte.rs`) : Basic Pitch
  ignore le niveau et transcrit les fuites d'autres instruments dans le stem
  de basse. « Under the Bridge » : introduction et premier couplet (basse
  tacet, stem 60 à 70 dB sous son niveau de jeu) — **119 notes → 2**.
  « Love Foolosophy » : première note à 0,5 s → 14,9 s (entrée de la basse).
  F1 0,450 → 0,459.
- **Durée minimale** 8 → 6 trames : F1 0,459 → 0,468. Les seuils d'attaque et
  de trame n'y gagnent rien.
- **Doigtés** : Flea joue haut sur les cordes graves (sol2 en 10ᵉ case de la
  corde de la plutôt que la corde de sol à vide). Une corde à vide sous une
  main haute coûte (`vide_haut` 0,3) : doigtés identiques 51 → 61 %.
- **Accordage choisi** par les notes (standard, drop D, cinq cordes) — aucun
  morceau du corpus ne le déclenche.

Par morceau, F1 de 0,81 (« Parallel Universe ») à 0,17 (« Porcelain »,
« I Like Dirt », « Road Trippin' »). Les erreurs restantes, sur les notes de
tablature à la bonne double croche : hauteur juste 71 %, octave 6,5 %
(−12 : 4,2 % ; +12 : 2,3 %), quarte/quinte 6 %, seconde 4 %. Le rappel
(44 %) est la limite principale : Basic Pitch ne voit pas les notes répétées
rapides ni les notes étouffées.

## Autres livres (sondés le 10 oct.)

| Livre | Résolution | Tablature lisible | Intérêt |
|---|---|---|---|
| Nirvana — *The Bass Guitar Collection* | 300 dpi | oui | autre bassiste (médiator, toniques), accordages abaissés |
| Primus — *Sailing the Seas of Cheese* | 300 dpi | peu (guitare et basse) | à trier |
| Royal Blood — *Songbook* (epub) | ~160 dpi | à essayer | basse jouée comme une guitare |
| Jaco Pastorius, Primus *Anthology* | 75-100 dpi | non | — |
| Livres guitare, piano-voix (epub) | ~60 dpi | non | — |

## Limites du banc

- La référence garde du bruit : mesures sans tablature (hauteurs d'Audiveris,
  octaves et accidents peu sûrs — « Scar Tissue » : des ré0 qui sont des ré1),
  rythmes mal reconnus (durées ramenées à 4/4).
- Les silences ne sont comptés que là où la partition écrit la portée de
  basse en silences ; un tacet sans portée est inconnu.
- L'alignement autorise les sauts : il est optimiste.
