# Le peuplement — la carte comme organisation humaine

> **Mécanique** : `carto-peuplement-architecture.md` — traits, structures,
> schéma SQL, réglages et objections. Ce document-ci garde l'intention.

Extension du concept cartographique. **Un morceau est un habitant**, avec ses caractéristiques et son empreinte. Les agglomérations naissent de la densité de peuplement. Le monde peut être généré selon différents critères, à la manière des jeux 4X.

**Inversion par rapport à la première conception** : les genres ne sont plus la clé d'organisation imposée d'en haut. Le peuplement **émerge**, et le genre devient un critère de génération parmi d'autres.

## Principe central : le placement chronologique

Les morceaux sont placés **par ordre de date de sortie**. Les plus anciens fondent les premiers établissements ; les suivants rejoignent une agglomération proche ou fondent un hameau sur une terre vierge.

Conséquences, toutes structurantes :
- **La stabilité devient une propriété, pas une contrainte.** Placement purement incrémental : un nouveau morceau est un arrivant, il ne déplace personne. Le gel artificiel des étages devient inutile.
- **La carte a des strates.** On peut rejouer la croissance en animation, ou trancher dans le temps : « la carte en 1975 », « ce qui est apparu depuis 2010 ».
- **Les formes s'expliquent** : centre ancien dense, périphérie récente — comme une vraie ville, pour la même raison.
- Dynamique de colonisation par vagues, très proche de l'esprit 4X.

## La bifurcation : terrain d'abord ou peuplement d'abord ?

### Voie A — le terrain dérive du peuplement
Les morceaux se placent, la densité produit le relief. Simple. Limite : l'altitude ne dit rien d'autre que « il y a du monde ici ».

### Voie B — le terrain est généré, puis peuplé (logique 4X) — **recommandée**
On génère un monde à partir de propriétés musicales intrinsèques, puis les morceaux s'y installent selon leur affinité avec le terrain.
- L'altitude porte sa propre information (énergie, intensité), **indépendante de la population** → montagnes inhabitées, vallées surpeuplées, paysage beaucoup plus riche à lire.
- Permet plusieurs **générateurs de monde** : la même bibliothèque donne des continents différents selon le critère choisi.

**Réserve honnête** : la voie B est plus arbitraire. Le choix des axes est une décision de design, pas une vérité des données. C'est aussi ce qui la rend intéressante.

### Référence : génération de mondes
Travaux d'Amit Patel (Red Blob Games) sur la génération de cartes polygonales : Voronoï pour la structure, bruit pour les côtes.
Surtout : le **diagramme de Whittaker** — deux axes physiques (température, précipitations) déterminent le biome. Transposition : deux axes musicaux déterminent le paysage.
- Exemple d'axes : acoustique ↔ synthétique en abscisse, calme ↔ intense en ordonnée.
- Biomes musicaux : forêt, désert, toundra, marais… porteurs de sens, pas décoratifs.

## Générateurs de monde (critères interchangeables)
Chaque générateur définit : **deux axes de position**, une **troisième propriété portée par l'altitude**, et la **métrique d'affinité** qui rapproche les habitants.

**Le niveau de la mer n'est pas un seuil d'altitude** — corrigé après conception. Si l'altitude porte l'énergie, seuiller l'altitude noierait les morceaux calmes, c'est-à-dire ses propres habitants. C'est la **densité de peuplement** qui dessine la côte ; la troisième propriété dessine le relief au-dessus. Les habitants trop isolés pour tenir sur le continent deviennent des **îles**, jamais des noyés. Détail et formule : `carto-peuplement-architecture.md`, §1.5.

Pistes : par similarité audio (embeddings) · par genre déclaré · par époque · par tempo et énergie · par réseau de collaborations.
Le même corpus doit produire des mondes différents et cohérents selon le générateur choisi.

## Échelle des établissements
Découle de la taille des grappes, jamais du genre.

| Taille | Établissement |
|---|---|
| 1 | Ferme isolée |
| 2-5 | Hameau |
| 6-20 | Village |
| 21-60 | Bourg |
| 61-200 | Ville |
| 200+ | Métropole |

Chaque seuil déclenche un changement de symbole, de taille d'étiquette et de niveau de zoom d'apparition. C'est ce qui produit l'impression de carte IGN.

## Ce que ça change pour le reste du projet
- `carto-google-maps.md` section 1 (placement hiérarchique en trois étages) est **remplacé** par ce modèle. Les autres sections (réseau routier, profils de routage, sources de données) restent valables — le réseau se construit toujours sur les établissements et les affinités.
- L'échelle reste tenable : le placement chronologique est en O(n log n) avec un index spatial, et ne demande aucun force layout global.
- La mécanique est conçue dans `carto-peuplement-architecture.md`. Trois choses y sont tranchées et méritent d'être connues d'ici : la stabilité est **démontrée** (la parcelle d'un habitant ne dépend que du centre de son établissement et de son rang d'arrivée), l'insertion d'un morceau **ancien** acquis tard passe par une **nouvelle édition** de la carte, et les mesures sur la bibliothèque réelle contredisent trois hypothèses de ce document — voir ses objections O1, O5 et O6.
