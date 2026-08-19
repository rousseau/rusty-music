# Essai : la tour texte de CLAP — elle s'importe, et ce n'est pas elle qu'il faut embarquer

Hors du workspace (voir son `exclude`) : un essai doit pouvoir échouer sans
empêcher `cargo build` de passer sur le reste.

**Question posée** (`docs/suite.md`, chantier 7). Nos douze familles sortent
d'un regroupement acoustique ; aucune taxonomie de genres ne les nomme. L'une
est « voix féminine, trip-hop et pop-rock », une autre « chanson acoustique »,
et les genres MusicBrainz sortent « Folk · Children's » pour Regina Spektor,
Nina Simone et Agnes Obel. CLAP est un modèle texte-vers-audio dont nous
n'avons importé que la tour audio. Exporter la tour texte donnerait de quoi
comparer chaque empreinte à des **mots**, au morceau et non à l'artiste.

Le plan disait : « rien ne dit que la tour texte s'exporte aussi proprement que
l'audio, et c'est là que se sont logés les pièges ».

## Réponse courte

**Elle s'exporte et s'importe mieux que l'audio — du premier coup, sans rien
figer d'autre que la longueur de séquence, cosinus 0,9999994636 contre
PyTorch.** Et c'est le résultat le moins utile de l'essai.

Trois faits l'emportent :

1. **elle pèse 501 Mo**, quatre fois la tour audio, pour un RoBERTa-base ;
2. **elle ne tourne pas sur wgpu** — `burn-cubecl` n'implémente pas les
   tenseurs booléens natifs, et le masque d'attention en est un ;
3. **pour nommer les familles, on n'en a pas besoin à l'exécution.** Le
   vocabulaire est fixe : ses empreintes se calculent une fois et tiennent dans
   **102 400 octets**. Embarquer 501 Mo pour recalculer à chaque lancement
   cinquante vecteurs qui ne changent jamais serait absurde.

**Ce qui marche le mieux n'est pas ce qu'on cherchait.** La recherche par
description est excellente ; le nommage des familles est meilleur que les
genres sur la moitié d'entre elles, et **franchement faux sur deux**.

## 1. L'espace commun tient — c'est le préalable, et il est acquis

On classe les 27 042 empreintes de la base contre une phrase. Rien n'est
recalculé : ce sont les vecteurs que la carte utilise déjà.

| phrase | ce qui remonte |
|---|---|
| « a symphony orchestra » | Ravel, CSR Symphony Orchestra × 5 |
| « a rapper over a boom bap beat » | MR. GREEN, Cypress Hill, Blockhead, Quantic |
| « traditional celtic music with fiddle and flute » | Matt Molloy, Uña Ramos, Lúnasa |
| « a saxophone solo » | Steve Coleman × 3, Julien Lourau |
| « an accordion » | Slide, La Tordue, Yann Tiersen, Fred Guichen |
| « a live rock concert with a crowd » | Morricone (intro live de Metallica), Pearl Jam `[encore break]` |
| « a children's song » | Bernard Davois — Les Petites Marionnettes |

Deux échecs, et ils sont instructifs : « a reggae offbeat guitar and bass »
remonte Dead Can Dance et Gainsbourg ; « a jazz double bass and brushed drums »
met Cypress Hill en deuxième. CLAP entend des **textures**, pas des idiomes.

C'est le préalable de tout le reste, et il ne coûte rien à vérifier — un
produit matriciel de 27 042 × 512 par 512 × 16.

## 2. Le nommage : deux conditions, et un plafond

### Le vocabulaire décide, pas le modèle

Premier essai avec **le vocabulaire d'AudioMuse-AI à la lettre** — les 50 tags
Last.fm de MusiCNN, qui sont exactement ce qu'emploie l'instance relevée :

```
  4763  Mellow · funk           4734  Progressive rock · Mellow
  3535  instrumental · blues    3138  Hip-Hop · funk
  2129  blues                   1774  blues · Mellow
```

**« blues » et « Mellow » partout.** Un mot nu est un mauvais prompt CLAP : le
modèle a été entraîné sur des légendes, pas sur des étiquettes. Donner à CLAP
le vocabulaire de MusiCNN ne donne pas les résultats de MusiCNN.

Avec des phrases descriptives, la même mécanique rend des noms qui décrivent :

```
  1774  a banjo and a fiddle playing a reel · traditional celtic music
   808  a saxophone solo · a jazz double bass and brushed drums
   668  a symphony orchestra · an ambient drone with no rhythm
   388  a solo instrumental piano piece · an ambient drone
```

### Les scores ne se comparent pas d'une phrase à l'autre

« a children's song » sort à +0,738 sur son meilleur morceau, « a reggae
offbeat guitar and bass » à +0,523 sur le sien. L'argmax brut ne classe donc
pas les morceaux, **il classe les phrases** — et toujours dans le même ordre :
« a children's song » remportait trois familles sur douze, dont celle de
Morcheeba et Bob Marley.

Retirer à chaque colonne sa moyenne suffit. Réduire en plus par l'écart-type
sur-corrige : la famille de Metallica devient « a live rock concert ».

| calibrage | famille Metallica | famille Regina Spektor |
|---|---|---|
| brut | a punk rock band playing fast | a male singer with an acoustic guitar |
| **centré** | **a heavy metal band with screamed vocals** | **a female choir · a female singer with a piano** |
| réduit | a live rock concert with a crowd | a female choir · a spoken voice |

### Ce que ça vaut, famille par famille

Comparé au nom que les genres donnent aujourd'hui. Les artistes dominants sont
la troisième colonne indispensable : sans eux, les deux premières ne se jugent
pas.

| genres (aujourd'hui) | CLAP centré | qui est dedans | verdict |
|---|---|---|---|
| Reggae · Rock | *an african percussion ensemble* | La Ruda, Femi Kuti, Sinsémilia | **CLAP faux** |
| Rock · Alternative Metal | a heavy metal band with screamed vocals | Metallica, Nirvana, Korn | égal |
| Trip Hop · Pop | a slow downtempo track with a trip hop beat | Morcheeba, Bob Marley | égal |
| Hip Hop · Electronic | a rapper over a boom bap beat | Atmosphere, Cypress Hill | CLAP mieux |
| Electronic · Breakbeat | a drum machine and a synthesizer | Chemical Brothers, Amon Tobin | égal |
| Rock · Folk | a male singer with an acoustic guitar | Jack Johnson, Tracy Chapman | **CLAP mieux** |
| Traditional · Celtic | a banjo and a fiddle playing a reel | Lúnasa, Danú | CLAP mieux |
| **Folk · Children's** | a female choir singing in harmony · **a female singer with a piano** | Regina Spektor, Nina Simone, Agnes Obel | **CLAP répare, en deuxième mot** |
| Jazz · Rock | a saxophone solo · a jazz double bass | Miles Davis & Coltrane | CLAP mieux |
| Classical · Ambient | a symphony orchestra | Morricone, John Williams, Zimmer | CLAP mieux |
| Children's · Spoken & Audio | *a man rapping in french* | chant breton, contes de Gripari | **CLAP faux** |
| Classical · Jazz | a solo instrumental piano piece | Einaudi, Tiersen, Satie | **CLAP mieux** |

**Sept mieux, trois égales, deux fausses.** Le cas que la feuille de route
citait — Regina Spektor sous « Children's » — est réparé, mais par le second
mot : la tête de liste, « a female choir singing in harmony », est douteuse
pour une famille menée par trois chanteuses solistes. C'est le même défaut que
les deux échecs, en moins grave.

### Le plafond, et il n'a pas cédé

Les deux échecs ont la même cause : **une famille que rien dans le vocabulaire
ne décrit ne reste pas sans nom, elle en reçoit un faux.** Le ska français part
sous « an african percussion ensemble » (tiré par quelques Femi Kuti), le chant
breton a cappella sous « a man rapping in french ».

L'hypothèse évidente — élargir le vocabulaire — a été testée et **ne suffit
pas.** Douze entrées ajoutées, dont « a ska band with a horn section and
offbeat guitar » et « traditional singing without instruments » : les deux
familles fautives gardent leur nom, et la famille trip-hop **régresse** vers
« a woman singing a jazz standard ». Le score `part × log₂(sur-représentation)`
récompense la phrase rare, et chaque ajout redistribue tout.

Le nommage par **centroïde** (comparer le centre de la famille au vocabulaire,
au lieu de compter les votes des morceaux) donne des paires plus cohérentes
entre elles — « a jazz double bass · a jazz big band », « a symphony orchestra
· a string quartet » — mais deux familles reçoivent alors la même tête de
liste. Aucune des deux voies ne domine : le vote sépare mieux les familles, le
centroïde décrit mieux chacune.

## 3. L'export et l'import : ce qui était censé être le piège

Rien n'a bloqué. À comparer avec la tour audio, dont c'est tout le contraire.

|  | tour audio | tour texte |
|---|---|---|
| modèle publié importable ? | **non** — `Runtime pads are not supported` | oui, une fois la longueur de séquence figée |
| préparation | figer 4 dimensions, replier par ORT, écarter `onnxsim` | `torch.onnx.export` + repliage ORT |
| nœuds | 8 031 → 882 | 613 → **464** |
| Rust généré | 4 400 lignes | **2 081 lignes** |
| cosinus contre la référence | 1,0000000000 | **0,9999994636** |
| poids | 117 Mo | **501 Mo** |
| wgpu | oui | **non** |

La tour texte est un RoBERTa : pas de partitionnement en fenêtres, donc pas de
`Pad` calculé à l'exécution. Les seuls opérateurs douteux — `CumSum` (les
positions déduites du masque), `GatherElements`, `Erf` — sont tous supportés.

### Le seul vrai obstacle : wgpu ne prend pas les booléens

```
not implemented: Unsupported dtype for `bool_from_data` Bool(Native)
  burn-cubecl-0.21.0/src/ops/bool_tensor.rs:46
```

Le masque d'attention est un tenseur booléen constant du graphe ; le backend
CPU le crée, `burn-cubecl` non. **Contrairement à la chasse au noyau fautif de
`../wgpu-noyaux/`, qui n'avait rien trouvé, il y a ici quelque chose à
signaler en amont.** Sur processeur : **91 ms par phrase** en `--release`, une
phrase à la fois — assez pour une barre de recherche, sans accélérateur.

## 4. Ce que l'essai change au chantier

**Deux livrables, et ils n'ont pas le même prix.**

| | ce qu'il faut embarquer | ce que ça donne |
|---|---|---|
| **nommer les familles** | une table de 50 × 512 `f32` = **102 400 octets**, aucun modèle, aucun tokeniseur | sept familles mieux nommées, deux fausses |
| **chercher par description** | **501 Mo** de poids, un tokeniseur BPE RoBERTa, processeur seulement | ce qui marche le mieux dans tout l'essai |

Le premier ne coûte presque rien et son bénéfice est **mesuré et partiel**. Le
second coûte cher et son bénéfice est le plus net qu'on ait vu — mais il
quadruple la taille de l'application pour une fonction que rien n'exige.

Ce que l'essai ne tranche pas, et qui appartient à l'interface : un nom de
famille doit tenir dans une légende. « a female choir singing in harmony · a
female singer with a piano » n'est pas un libellé. La voie évidente est un
vocabulaire de **couples** — la phrase pour CLAP, un libellé court pour
l'affichage — mais elle n'a pas été sondée.

## Refaire l'essai

```bash
python3 -m venv "$TMPDIR/rusty-music-clap-texte"
"$TMPDIR/rusty-music-clap-texte/bin/pip" install torch transformers onnx onnxruntime numpy
V="$TMPDIR/rusty-music-clap-texte/bin/python"

$V sonder.py espace --top 5                    # l'espace commun tient-il ?
$V sonder.py familles --vocabulaire tags       # le vocabulaire d'AudioMuse-AI
$V sonder.py comparer --vocabulaire large      # nom CLAP + artistes dominants
$V sonder.py centroide                         # l'autre voie de nommage
$V sonder.py table                             # les 102 Ko qu'on embarquerait
$V sonder.py export && $V sonder.py reference  # le modèle et sa référence
cargo run --release                            # l'import Burn, contre PyTorch
cargo run --features metal --no-default-features   # échoue : booléens
```

La base lue est celle de l'application installée, en lecture seule — l'essai ne
touche à rien. `torch` et `transformers` ne sont d'aucune façon des dépendances
du projet : ils ne servent qu'ici.

**Une remarque d'outillage** : l'interpréteur termine sur
`recursive_mutex lock failed` quand torch et onnxruntime sont chargés ensemble
sur macOS. C'est à la sortie, après que tout a été écrit — sans conséquence,
mais il ne faut pas le prendre pour un échec de l'export.
