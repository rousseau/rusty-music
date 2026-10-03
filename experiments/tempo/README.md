# Banc de tempo : GTZAN

Jusqu'ici le tempo (`descripteurs::analyser`, utilisé par la carte, le lecteur et le
« BPM cible » de l'éditeur) n'avait **aucune vérité terrain**. Ce banc le mesure sur
GTZAN (1 000 extraits de 30 s, 10 genres proches de ceux de la bibliothèque) avec les
annotations de tempo de `TempoBeatDownbeat/gtzan_tempo_beat`.

GTZAN n'a pas de licence déclarée (jeu de recherche) : rien n'est copié dans le
dépôt, `data/` est ignoré, `preparer.sh` télécharge audio et annotations.

```bash
experiments/tempo/preparer.sh
cargo run --release -p rusty-music-analysis --example verif_tempo -- \
    experiments/tempo/data/genres > experiments/tempo/data/ours.csv      # ~100 s
python3 experiments/tempo/noter_tempo.py experiments/tempo/data/ours.csv
# comparaison à librosa (venv avec librosa), depuis le dossier data/ :
(cd experiments/tempo/data && python3 ../librosa_tempo.py)   # écrit data/librosa.csv
```

Métriques : **Acc1** (à ± 4 % de la référence) et **Acc2** (idem en tolérant les erreurs
d'octave et de mesure : facteurs 2, 3, 1/2, 1/3).

## Résultats (3 oct. 2026, 998 clips ; `jazz.00054` est corrompu dans GTZAN)

| | Acc1 | Acc2 |
|---|---|---|
| Rusty Music | 62,9 % | **88,3 %** |
| librosa (`feature.tempo`, référence) | 65,8 % | 84,0 % |

Par genre (Acc1 / Acc2) — Rusty Music : blues 54/87, classical 37/56, country 66/97,
disco 94/98, hiphop 70/96, jazz 49/70, metal 47/91, pop 87/95, reggae 58/99, rock 67/94.

Par tempo de référence (juste / ÷2 / ×2, en %) :

| Référence | < 80 | 80-100 | 100-120 | 120-140 | 140-170 | ≥ 170 |
|---|---|---|---|---|---|---|
| Rusty Music | 43/0/22 | 68/10/8 | 77/5/10 | 84/8/2 | 50/45/0 | 21/69/0 |
| librosa | 2/0/57 | 65/0/7 | 91/0/0 | 95/0/0 | 87/1/0 | 8/79/0 |

## Ce que ça dit

- **La périodicité est bien détectée** : Acc2 88 % (librosa 84 %). Les vrais ratés sont
  la musique sans pulsation nette (classique 56 %, jazz 70 %) et les confusions
  ternaire/binaire (×2/3, ×3/2, ×4/3 : ~4 %).
- **Les erreurs sont des erreurs d'octave** : 17,5 % de ÷2 (trop lent) et 7,1 % de ×2.
  Les ÷2 se concentrent sur les références rapides (45 % de 140 à 170, 69 % au-delà).
- **Ce n'est pas forcément un défaut : c'est un choix de convention.** Les règles
  d'octave (`BPM_SOUS_OCTAVE_RAPIDE`, `SEUIL_SOUS_OCTAVE`…) visent le tempo **perçu**
  (« Killpop » de Slipknot mesuré à 163 sur sa subdivision s'affiche à 82). GTZAN suit
  souvent le battement le plus rapide : 107 clips sont annotés à 170 BPM ou plus,
  jusqu'à 322 (un reggae à 237 BPM, que personne ne tape à cette vitesse). **Acc1 est donc
  biaisé contre notre convention pour les tempos rapides** ; Acc2 est la mesure neutre.
- **Pas de verdict sur l'octave sans second jeu de référence en tempo perçu** (ACM Mirum,
  Ballroom : non téléchargeables ici — le serveur d'ISMIR 2004 répond 403). Optimiser
  Acc1 sur GTZAN seul reviendrait à ajuster la convention à celle de ses annotateurs.
- Dans la plage où les deux conventions s'accordent (100-140 BPM), notre moteur se
  trompe d'octave bien plus que librosa (77 % et 84 % de justes contre 91 % et 95 %),
  signe de marge sur les corrections d'octave (surtout le ×2 : 10 % de 100 à 120).
