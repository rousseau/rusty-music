# Structure de la carte — placement hiérarchique en trois étages (abandonné)

> Archive : section retirée de `carto-google-maps.md` le 31 oct. 2026, historique seulement.
> Le texte est tel qu'il était, liens relatifs compris (ils visent `docs/`).

## 1. Structure de la carte

> ⚠️ **Cette section est remplacée par `carto-peuplement.md`** (modèle du peuplement : morceaux = habitants, placement chronologique, génération de monde 4X). Conservée ici pour l'historique du raisonnement et parce que les notions de territoire, de popularité et de sources de données restent valables.

### [Historique] Placement hiérarchique en trois étages

L'idée maîtresse : **fixer le gros une fois, placer le fin relativement au gros**. C'est ce qui donne la stabilité.

### Étage 1 — Continents et pays : les genres
- Graphe des genres : nœuds = genres présents dans la bibliothèque ; arêtes = co-occurrence (artistes/albums partageant des genres), pondérée.
- Disposition par force layout. **Le graphe est minuscule** (quelques dizaines de nœuds) : n'importe quel algorithme convient, le calcul est instantané, et la disposition peut être **corrigée à la main**. C'est une fonctionnalité, pas un défaut — cette couche doit être figée et mémorisable.
- Genres proches → familles → « continents ». Enregistrer les positions en base ; ne les recalculer que sur demande explicite.

### Étage 2 — Villes : les artistes
- Chaque artiste est rattaché à son genre dominant → placé dans le territoire correspondant.
- Position dans le territoire : force layout **contraint à l'intérieur du polygone**, avec
  - attraction : collaborations (relations MusicBrainz) + similarité audio,
  - répulsion proportionnelle à la popularité (une grande ville a besoin de place).
- **Taille de la ville = popularité.** Sources : ListenBrainz (`/1/popularity/...`, écoutes totales et pourcentage de popularité 0-100, indexé par MBID) + compteurs de lecture locaux.

### Étage 3 — Quartiers et rues : les morceaux
- Placés autour de leur artiste, regroupés par album, écartés selon la similarité audio.

### La stabilité, concrètement — politique graduée
« Rien ne bouge jamais » serait trop rigide. La règle est par étage :

| Étage | Politique |
|---|---|
| Genres (continents) | **Figé.** Recalcul uniquement sur demande explicite |
| Artistes (villes) | Déplacement **borné**, confiné à leur territoire |
| Morceaux (rues) | Libres de bouger localement |

Positions stockées en base, insertion incrémentale.

### Quand la carte doit vraiment changer : les éditions
Nouveau genre qui apparaît, bibliothèque qui double de volume → on ne l'interdit pas, on **versionne**. Une « nouvelle édition de la carte », déclenchée par l'utilisateur, avec transition animée entre les deux états. Comme une carte routière qui change d'édition. Conserver l'édition précédente pour pouvoir comparer.

### Territoires et relief
- Frontières : Voronoï pondéré par la popularité, puis lissage (approche GMap).
- Relief : champ de densité → ombrage (voir `carto-direction.md`).

### Échelle : pourquoi la hiérarchie REND possible les 25 000+ morceaux
La contrainte de stabilité n'est pas un frein à l'échelle, c'en est le moyen. Ce qui coûte cher et devient instable, c'est le force layout global sur 25 000 nœuds — l'approche à trois étages ne le calcule jamais.

Ordres de grandeur pour ~25 000 morceaux (≈ 1 500-3 000 artistes, 30-80 genres) :

| Étage | Taille du problème | Coût |
|---|---|---|
| Genres | 30-80 nœuds | instantané, calculé une fois |
| Artistes | 20-200 par territoire, **territoires indépendants** | quelques ms chacun, parallélisable (`rayon`) |
| Morceaux | 25 000, **placement déterministe** | O(1) par morceau, une passe linéaire |

**Point clé de l'étage 3 : pas de force layout du tout.** Placement déterministe autour de l'artiste (spirale phyllotaxique ou disque de Poisson, ordonné par album puis similarité) → stable par construction et linéaire.

Rendu : 25 000 points est modeste pour des tuiles vectorielles (la Map of GitHub en affiche 700 000). C'est la raison du choix MapLibre.

**Le vrai goulot d'étranglement à cette échelle n'est ni le placement ni le rendu : c'est l'analyse audio** (décodage + inférence sur 25 000 fichiers = plusieurs heures). C'est là qu'il faut reprise sur interruption, traitement par lots et parallélisation.
