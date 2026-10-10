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
<venv>/bin/python muscriptor_banc.py --taille medium   # muscriptor, pymupdf
<venv>/bin/python muscriptor_batterie.py
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

## Nirvana (10 oct., second corpus de basse)

*The Bass Guitar Collection* : 17 morceaux dans la bibliothèque (« Stain »
absent), Krist Novoselic au médiator. **11 sont accordés plus bas**
(demi-ton, ton, deux tons, drop D, drop D un demi-ton plus bas) : la
tablature est écrite relativement à l'accordage, la notation comme en
standard. `morceaux.json` donne l'accordage réel (`"accordage"`) ;
`reference.py` en déduit hauteurs et transposition.

| | F1 | F1tab | doigtés |
|---|---|---|---|
| Nirvana, accordage standard seul | 0,36 | 0,36 | 0,34 |
| Nirvana, accordage choisi par les notes | 0,36 | 0,36 | **0,60** (34 morceaux, deux corpus) |

`Accordage::choisir` essaie huit accordages : compatibles = presque aucune
note sous la corde grave et la corde grave à vide jouée ; entre eux, le
doigté le moins coûteux. **Juste pour 29 morceaux sur 34** ; les échecs sont
des cordes graves que Basic Pitch ne voit pas (do0 de « Blew », ré0 de « On a
Plain »). Le F1 de Nirvana est plus bas que celui de Flea : basse saturée
doublée par la guitare, octaves (11 %), « Polly » (acoustique, basse
discrète) à 0,04.

## Batterie (10 oct.)

Sept transcriptions de [thedrumninja.com](https://thedrumninja.com/drum-transcriptions)
(PDF Sibelius **vectoriels**) : `lire_batterie.py` lit directement glyphes
(têtes noires, en croix, fantômes, silences, « % » et « %% ») et traits
(hampes, ligatures, barres) — Audiveris, lui, rate les têtes en croix. Rythme
par voix (hampes montantes : mains, descendantes : pieds), durées par le
nombre de ligatures ; les hampes des petites notes sont écartées. Pièces
selon la clé standard, codées comme les cinq classes d'ADTOF.
`banc_batterie.py` aligne comme pour la basse.

| pièce | F1 | F1±1 |
|---|---|---|
| grosse caisse | 0,72 | 0,81 |
| caisse claire | 0,80 | 0,83 |
| toms | 0,42 | 0,43 |
| charleston | 0,77 | 0,82 |
| cymbales | 0,76 | 0,77 |

Par morceau, F1 de 0,92 (« Sunburn ») à 0,54 (« Paradise City », qui
accélère à la fin : la grille de mesures décroche). Les seuils de détection
d'ADTOF sont déjà au mieux (`regler_batterie.py` : moins de 0,02 à gagner
par pièce). Biais : une transcription Drum Ninja note le groove de chaque
section et le répète ; fills et variations ne sont pas tous écrits — les toms
surtout sont sous-notés.

## MuScriptor (10 oct.)

[MuScriptor](https://github.com/muscriptor/muscriptor) (Kyutai × Mirelo,
code MIT, poids CC BY-NC 4.0 et conditions d'usage acceptées sur Hugging
Face), taille *medium*, par son code Python de référence — évaluation
seulement, dans un environnement à part. `muscriptor_banc.py` transcrit le
stem de basse restreint aux basses, puis la chaîne de l'éditeur (exemple
`banc -- notes` : porte, monophonie, accordage, doigtés, mise en mesure) et
la même comparaison ; `muscriptor_batterie.py` fait de même pour la batterie
(groupe « drums », General MIDI → cinq pièces, `banc -- coups`).

| basse, 34 morceaux | F1 | F1tab | doigtés | accordage juste |
|---|---|---|---|---|
| Basic Pitch (chaîne actuelle) | 0,42 | 0,44 | 0,60 | 29/34 |
| MuScriptor medium | **0,55** | **0,57** | 0,62 | **31/34** |

Meilleur sur 29 morceaux sur 34 ; le gain est le plus fort sur Nirvana
(basse saturée doublée par la guitare) : « Mr. Moustache » 0,23 → 0,79,
« Come As You Are » 0,59 → 0,85, « Blew » 0,17 → 0,43. Flea : « Otherside »
0,64 → 0,77, « Get On Top » 0,26 → 0,41. Restent bas les mêmes
(« Porcelain », « I Like Dirt », « Polly »). La porte d'énergie ne change
presque rien (0,548 → 0,549) : MuScriptor ne transcrit pas les fuites.

**Défaut trouvé : boucles du décodage glouton.** Une tranche de 5 s qui
n'émet jamais sa fin de séquence répète la même note toutes les 10-20 ms
jusqu'à la limite de longueur — dans le silence (introduction tacet
d'« Under the Bridge » : 22 000 notes, 221 s de calcul) et les fins bruitées
(« Purple Stain »). `nettoyer()` écarte les tranches de plus de 150 notes ou
de plus de 10 doublons (un morceau sain reste sous 110). Un portage devra
faire de même (ou redécoder la tranche autrement).

Temps : ≈ 30 s par morceau sur la puce Apple (MPS), plus quand une tranche
boucle.

| batterie, 7 morceaux | GC | CC | toms | HH | CY |
|---|---|---|---|---|---|
| ADTOF | **0,72** | **0,80** | 0,42 | 0,77 | **0,76** |
| MuScriptor medium | 0,71 | 0,76 | **0,46** | 0,77 | 0,56 |

Batterie : pas mieux qu'ADTOF (cymbales nettement moins bonnes) — ADTOF
reste.

## Limites du banc

- La référence garde du bruit : mesures sans tablature (hauteurs d'Audiveris,
  octaves et accidents peu sûrs — « Scar Tissue » : des ré0 qui sont des ré1),
  rythmes mal reconnus (durées ramenées à 4/4).
- Les silences ne sont comptés que là où la partition écrit la portée de
  basse en silences ; un tacet sans portée est inconnu.
- L'alignement autorise les sauts : il est optimiste.
