# Le « Google Maps de la musique » — conception

Trois chantiers séparables : **la structure** (où sont les choses), **le réseau** (comment on circule), **le rendu** (à quoi ça ressemble). Le routage vient en quatrième, une fois le réseau construit.

## Principe fondateur : construire, pas projeter

On abandonne t-SNE/UMAP. Raison décisive : la **stabilité**. Ces algorithmes rebattent les cartes à chaque recalcul — ajouter un album déplacerait des territoires entiers. Une carte doit être permanente : on doit pouvoir la mémoriser, y revenir, s'y repérer.

Référence : **GMap** (Gansner, Hu & Kobourov, 2010, intégré à Graphviz) — carte géographique à partir de données relationnelles. Parti pris à reprendre : la carte n'a pas à être une représentation exacte, elle doit capturer les relations ; les sommets fortement liés sont groupés en régions.
Suite directe : **Gronemann & Jünger, *Drawing clustered graphs as topographic maps*** (2012).
Modèle opérationnel : **Map of GitHub** (anvaka) — force layout + MapLibre + noms de pays générés.

---

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

---

## 2. Réseau routier — la hiérarchie des liens

Graphe de base : k plus proches voisins sur les embeddings audio (k ≈ 8-16), coût d'arête = 1 − similarité.

Classification en hiérarchie routière, par importance des extrémités et **centralité d'intermédiarité** :

| Classe | Critère | Sens musical |
|---|---|---|
| Autoroute | relie les grands pôles, forte centralité | transitions évidentes entre artistes majeurs |
| Route nationale | relie les villes moyennes | voisinage solide |
| Route secondaire | intra-territoire | exploration d'un genre |
| Sentier | arêtes de faible poids, longue traîne | découverte, morceaux oubliés |
| Refuge isolé | nœud sans arête forte | morceau orphelin |

Astuce cartographique : faire suivre aux autoroutes la **ligne de crête** de densité (arbre couvrant minimal ou arbre de Steiner sur les pôles) — le réseau épouse alors le relief, comme une vraie carte routière.

---

## 3. Routage — un seul graphe, plusieurs profils

> **Révisé (septembre 2026).** Sur le plan de ville réel, le mode *itinéraire*
> de la Carte ne route plus sur le graphe musical décrit ici mais sur la
> **voirie OSM** (`crates/carto/src/reseau_reel.rs` +
> `cout_itineraire.rs` ; commande `itineraire_voirie`). Les trois profils
> ci-dessous sont réinterprétés comme des pondérations de rue (par le connu /
> redécouvrir / panoramique). Le routage musical (`itineraire`) subsiste en
> repli. Détail : `carto-ville.md` § « Le réseau et les itinéraires » et
> `carto-etapes.md`.

Comme OSRM : **le graphe ne change pas, seule la fonction de coût change.**

| Profil | Fonction de coût | Effet |
|---|---|---|
| **Autoroute** | distance ÷ popularité | trajet court par les morceaux connus |
| **Petit sentier** | distance × popularité | évite ce qui est déjà connu → redécouverte |
| **Points d'intérêt** | étapes imposées (favoris, jamais écoutés) | itinéraire à waypoints |
| **Panoramique** | maximise le nombre de territoires traversés | diversité de genres |

Options à la Google Maps, avec un sens musical direct :
- **« Éviter les autoroutes »** → éviter mes morceaux les plus écoutés.
- **Durée du trajet = durée de la playlist.** Un « itinéraire de 40 minutes » est une contrainte réelle sur la somme des durées : c'est le paramètre le plus naturel pour un utilisateur.
- **Dénivelé** = variation de popularité ou d'énergie le long du trajet → alimente le profil d'altitude déjà prévu.
- **Itinéraires alternatifs** : k plus courts chemins (Yen), comme les 2-3 trajets proposés par Google Maps.

### Briques à utiliser (ne rien réécrire)
- `pathfinding` : A*, Dijkstra, **Yen (k plus courts chemins)**, composantes connexes.
- `fast_paths` : Contraction Hierarchies, conçu pour les réseaux routiers — à garder en réserve si le graphe devient gros.
- `petgraph` : structure de graphe et centralités.

---

## 4. Sources de données

| Donnée | Source |
|---|---|
| Popularité mondiale (artiste, enregistrement) | **ListenBrainz** `/1/popularity/*` — écoutes totales, auditeurs, popularité 0-100, par MBID |
| Popularité personnelle | compteurs de lecture locaux |
| Collaborations, membres de groupe | **MusicBrainz** (`artist-rels`) |
| Genres | tags locaux + MusicBrainz |
| Similarité audio | embeddings du pipeline d'analyse |
