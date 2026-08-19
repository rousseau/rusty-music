# Notre carte tient-elle face à AudioMuse-AI ? — oui, et leurs descripteurs moins qu'on l'espérait

Hors du workspace. Sondage mené le 17 août 2026 contre une instance
AudioMuse-AI en fonctionnement sur **la même bibliothèque** : 26 928 morceaux
chez eux, 27 044 chez nous, 25 335 appariés par artiste + titre (93,7 %).

## Ce qu'on cherchait

Deux questions, l'une de vérification, l'autre d'opportunité.

1. **Nos douze familles décrivent-elles quelque chose de réel ?** Elles sortent
   d'un k-means sur des empreintes CLAP projetées par t-SNE. Rien, jusqu'ici, ne
   les confrontait à un découpage obtenu autrement.
2. **Leur tempo et leur tonalité pourraient-ils débloquer le mixage (chantier 8)**
   (mixage de deux pistes), que `docs/suite.md` déclare bloqué faute de ces
   deux grandeurs ?

## 1. Les familles : corroborées

AudioMuse-AI bâtit sa propre projection (UMAP) sur ses propres descripteurs.
En y plaçant nos familles, **les douze sont plus compactes que la bibliothèque
entière** — distance moyenne entre deux morceaux tirés au hasard, rapportée à
la même mesure sur tout le corpus :

| notre famille | notre nom | n | dispersion chez eux |
|---|---|---:|---:|
| 0 | Traditional · Celtic | 1 673 | **0,36×** |
| 11 | Hip-Hop · Rap | 2 791 | 0,39× |
| 9 | Classical · Soundtrack | 868 | 0,42× |
| 8 | *(voir plus bas)* | 1 336 | 0,43× |
| 5 | Rock · Folk | 2 033 | 0,46× |
| 3 | Electronic · Jazz | 2 926 | 0,56× |
| 4 | Children's · Spoken & Audio | 351 | 0,56× |
| 1 | Reggae · Pop | 4 024 | 0,64× |
| 7 | Pop · R&B | 3 127 | 0,65× |
| 10 | Jazz · Big Band | 777 | 0,77× |
| 6 | Metal · Grunge | 2 173 | 0,88× |
| 2 | Metal · Rock | 3 256 | 0,89× |

Deux modèles différents, deux réductions différentes, la même partition tient.
Les deux dernières lignes disent aussi quelque chose : **les deux familles
metal sont les plus lâches**, ce qui est cohérent avec un découpage en deux
d'une région large — et c'est exactement le couple qui exigeait une règle
supplémentaire pour ne pas porter le même nom.

## 2. Les étiquettes : d'accord sur neuf familles sur douze

Leurs étiquettes sont *calculées* (un classifieur), les nôtres viennent des
tags des fichiers. Classées par le même score que `nommer_les_familles` :

| famille | notre nom (tags des fichiers) | leurs étiquettes (calculées) |
|---|---|---|
| 1 | Reggae · Pop | pop · funk · indie · soul |
| 2 | Metal · Rock | metal · punk · alternative · hard rock |
| 7 | Pop · R&B | pop · female vocalists · soul · indie |
| 3 | Electronic · Jazz | electronica · electronic · chillout · ambient |
| 11 | Hip-Hop · Rap | hip-hop · rnb · electronic · soul |
| 6 | Metal · Grunge | metal · hard rock · punk · alternative |
| 5 | Rock · Folk | folk · acoustic · indie · country |
| 0 | Traditional · Celtic | instrumental · folk · jazz · guitar |
| 9 | Classical · Soundtrack | ambient · instrumental · jazz · experimental |
| 10 | Jazz · Big Band | jazz · instrumental · blues · funk |
| **8** | **Children's · Pop** | **female vocalists · folk · indie · jazz** |
| **4** | **Children's · Spoken & Audio** | **hip-hop · experimental · blues · electronic** |

Les deux désaccords se tranchent en regardant les morceaux, et ils ne tombent
pas du même côté :

- **famille 8 — ils ont raison, notre nom est faux.** Regina Spektor, Agnes
  Obel, Nina Simone, Thomas Fersen, Jeff Buckley, Feist, Björk : voix et
  écriture intimiste. Notre libellé vient de 121 fichiers étiquetés
  « Children's » dans une bibliothèque qui en compte peu — assez pour gagner au
  score. **Le défaut est dans la donnée, pas dans la règle** : aucun classement
  ne rattrape une étiquette fausse.
- **famille 4 — nous avons raison, leur classifieur décroche.** 130 titres de
  chant breton *a cappella* (Nolùen Le Buhé, Yann-Fañch Kemener…), un conte lu
  par François Morel, de la chanson jeunesse. De la voix nue : cohérent. Y lire
  « hip-hop · blues » ne l'est pas.

## 3. Leur tempo et leur tonalité : inutilisables pour le mixage

C'était la vraie opportunité, et elle n'existe pas.

- **`scale` vaut `minor` pour les 26 928 morceaux.** Ce n'est pas une
  distribution, c'est un champ constant. Le mode est donc absent, et le mixage
  harmonique (roue de Camelot) resterait à moitié aveugle.
- **`tempo` ne prend que 37 valeurs distinctes**, espacées d'environ 6 %
  (…89, 94, 99, 104, 110, 117, 125, 134, 144, 156…) : une grille logarithmique
  de classifieur, pas un suivi de battements. Pour caler deux morceaux, il faut
  mieux que ±3 % — et surtout une phase, que ce champ ne porte pas du tout.

**Conclusion pour `docs/suite.md`, chantier 8 : le prérequis manquant le reste.**
Ces deux grandeurs sont à calculer chez nous, pas à emprunter.

Ce qui est solide chez eux, en revanche : `mood_vector` (étiquettes calculées
par morceau) et `other_features` (danceable, aggressive, happy, party, relaxed,
sad), tous deux renseignés à 100 %. C'est ce qui a servi au tableau ci-dessus.

## Ce qu'on en fait

**Rien n'est importé.** Le projet vise une suite autonome ; dépendre de
l'export d'un autre outil contredirait sa raison d'être. Ce sondage sert de
contrôle, une fois.

Il désigne en revanche une suite possible, et bon marché : **CLAP est un modèle
texte-vers-audio**, et nous n'en avons exporté que la tour audio. Exporter la
tour texte permettrait de comparer chaque empreinte à des mots — « reggae »,
« voix féminine », « instrumental » — et de nommer les familles sans dépendre
des tags des fichiers, qui viennent de se tromper sur la famille 8. Même
modèle, même format, aucune dépendance nouvelle. AudioMuse-AI fait déjà
exactement cela de son côté (`/api/clap/search`).

## Reproduire

```bash
export AM_URL=http://127.0.0.1:8000 AM_USER=… AM_PASS=…
python3 comparer.py [chemin/vers/rusty-music.db]
```

Les identifiants passent par l'environnement : ils n'ont rien à faire dans un
fichier versionné.
