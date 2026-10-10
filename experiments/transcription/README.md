# Banc de la transcription de basse

- `parite_basic_pitch.py` — parité du port Rust de Basic Pitch avec le code
  d'origine de Spotify (activations, notes).
- `comparer_tablature.py` — compare une transcription (mesures quantifiées)
  à une tablature de référence au format JSON de Songsterr.
- `regler.py` — note une grille de réglages (`examples/regler.rs`) contre
  plusieurs références et les classe sur la moyenne.

**Les tablatures de référence ne sont pas dans le dépôt** : ce sont des
œuvres protégées, utilisées en local pour l'évaluation seulement. Pour les
récupérer, le JSON d'une partie Songsterr se trouve à
`https://dqsljvtekg760.cloudfront.net/<songId>/<revisionId>/<image>/<partId>.json`
(identifiants dans l'état de la page du morceau, compressé en gzip).

## Résultats (10 oct. 2026)

Références : « Love Foolosophy » (Jamiroquai, Songsterr 22839), « She's A Bad
Mama Jama » (Carl Carlton, 664247). Stems HTDemucs de la bibliothèque,
pulsation Beat This! corrigée.

| | Love Foolosophy | Mama Jama |
|---|---|---|
| attaques, rappel / précision | 72 % / 79 % (66 / 76 avant) | 55 % / 64 % (54 / 62) |
| hauteur juste (parmi les attaques) | 91 % (85) | 83 % (83) |
| erreur d'octave | 4 % (10) | 5 % (4) |
| doigté identique (parmi les hauteurs justes) | 71 % (47) | 78 % (81) |

Réglages retenus : seuil d'attaque 0,6, durée minimale 8 trames, correction
des harmoniques à 0,6 (`Reglages::basse`) ; changement de corde 0,6, pas de
préférence pour le bas du manche (`tablature::Couts`). Deux morceaux seulement
— à confirmer sur d'autres. « Black Crow » (Jamiroquai) n'est pas exploitable :
la pulsation détecte 75 BPM pour 120 dans la référence (balancement
ternaire ?), toute la grille est fausse. « Mama Jama » : la version de la
bibliothèque fait 178 mesures contre 130 dans la référence, l'alignement
pénalise.
