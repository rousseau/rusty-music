# Le « Google Maps de la musique » — conception

Trois chantiers séparables : **la structure** (où sont les choses), **le réseau** (comment on circule), **le rendu** (à quoi ça ressemble). Le routage vient en quatrième, une fois le réseau construit.

## Principe fondateur : construire, pas projeter

On abandonne t-SNE/UMAP. Raison décisive : la **stabilité**. Ces algorithmes rebattent les cartes à chaque recalcul — ajouter un album déplacerait des territoires entiers. Une carte doit être permanente : on doit pouvoir la mémoriser, y revenir, s'y repérer.

Référence : **GMap** (Gansner, Hu & Kobourov, 2010, intégré à Graphviz) — carte géographique à partir de données relationnelles. Parti pris à reprendre : la carte n'a pas à être une représentation exacte, elle doit capturer les relations ; les sommets fortement liés sont groupés en régions.
Suite directe : **Gronemann & Jünger, *Drawing clustered graphs as topographic maps*** (2012).
Modèle opérationnel : **Map of GitHub** (anvaka) — force layout + MapLibre + noms de pays générés.

---

## 1. Structure de la carte

Abandonnée : remplacée par le peuplement (`archives/carto-peuplement.md`), puis par le plan de Paris et sa croissance chronologique (`carto-ville.md`). Le texte d'origine est dans [`archives/carto-google-maps-structure.md`](archives/carto-google-maps-structure.md).

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
