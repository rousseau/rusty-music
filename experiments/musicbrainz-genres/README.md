# Nommer les familles : tags des fichiers, MusicBrainz, ou modèle audio ?

Hors du workspace. Sondage du 17 août 2026, déclenché par un constat simple :
**les étiquettes des douze familles ne correspondent pas bien aux artistes
qu'on y trouve.**

## D'où viennent les étiquettes actuelles

Des **tags de genre des fichiers**, agrégés par famille et classés par
`part × log₂(sur-représentation)` (`nommer_les_familles`, `crates/core/src/db.rs`).
C'est déjà la première des deux sources qu'on pourrait envisager — et elle ne
suffit pas.

L'audit des douze familles le montre. Le **regroupement** est bon partout : ce
sont les libellés qui décrochent.

| n | artistes dominants | étiquette actuelle |
|---:|---|---|
| 4 321 | Femi Kuti, Bob Marley, Sinsémilia, La Ruda, James Brown | Reggae · Pop |
| 3 383 | Metallica, Mass Hysteria, Korn, Slipknot, Lofofora | Metal · Rock |
| 3 269 | Morcheeba, Sheryl Crow, Belleruche, Lamb, Alanis Morissette | **Pop · R&B** |
| 3 076 | Atmosphere, Cypress Hill, The Roots, Saïan Supa Crew | Hip-Hop · Rap |
| 3 040 | The Herbaliser, Chemical Brothers, Amon Tobin, The Prodigy | Electronic · Jazz |
| 2 273 | Nirvana, AC/DC, Soundgarden, Alice in Chains, Tool | Metal · Grunge |
| 2 151 | Jack Johnson, Tracy Chapman, Ben Harper, Moriarty | **Rock · Folk** |
| 1 766 | Lúnasa, Danú, Solas, Forzh Penaos | Traditional · Celtic |
| 1 444 | Regina Spektor, Agnes Obel, Nina Simone, Jeff Buckley, Feist | **Children's · Pop** |
| 924 | Ludovico Einaudi, Yann Tiersen, Erik Satie, Ennio Morricone | Classical · Soundtrack |
| 814 | Miles Davis & Coltrane, Jaco Pastorius, Steve Coleman | Jazz · Big Band |
| 570 | chant breton *a cappella*, conte lu, chanson jeunesse | Children's · Spoken & Audio |

**La cause n'est pas le classement, c'est le vocabulaire.** Les familles sortent
d'un regroupement *acoustique* (empreintes CLAP) ; les tags sont *éditoriaux*.
La famille de 3 269 est « voix féminine, trip-hop et pop-rock » ; celle de
2 151, « chanson acoustique ». Aucun genre ne nomme ça. Et pour celle de 1 444,
121 fichiers étiquetés « Children's » — étiquette rare ailleurs, donc gagnante —
suffisent à écraser Regina Spektor et Nina Simone.

## Ce qu'utilise AudioMuse-AI : ni l'un ni l'autre

Relevé sur l'instance (`/api/config`) :

```
mood_labels = ['rock', 'pop', 'alternative', 'indie', 'electronic',
               'female vocalists', 'dance', '00s', 'alternative rock', …]
top_n_moods = 5
```

42 étiquettes distinctes observées sur les 26 928 morceaux. **C'est le
vocabulaire des 50 tags Last.fm les plus fréquents du Million Song Dataset**,
sortie d'un réseau d'auto-étiquetage (MusiCNN) — pas les tags des fichiers, pas
MusicBrainz.

Et c'est là que se trouve l'enseignement : ce vocabulaire contient
`female vocalists`, `instrumental`, `acoustic`, `guitar`, `chillout`, `mellow`.
**Des descripteurs qui ne sont pas des genres** — voix, instrumentation,
texture. Exactement ce qui sépare nos familles 3 269, 2 151 et 1 444, et
exactement ce qu'aucune taxonomie de genres ne fournira jamais.

## MusicBrainz : mesuré, et nettement meilleur

L'appariement est gratuit : **92,6 % des morceaux portent déjà un
`mb_artist_id`** dans leurs tags (25 030 sur 27 044). Aucune correspondance
floue à faire, on interroge par identifiant.

Sur les artistes interrogés, **75 % ont au moins un genre** chez MusicBrainz.
Le même score que le moteur, appliqué aux deux sources — seule la source change.
Relevé à 72,9 % de couverture en morceaux ; **le tableau ne bouge plus depuis
62,6 %**, les artistes restants ne pesant que quelques morceaux chacun :

| n | tags des fichiers (actuel) | MusicBrainz |
|---:|---|---|
| 4 321 | Reggae · Pop | **ska · reggae** |
| 3 383 | Metal · Rock | **nu metal · alternative rock** |
| 3 269 | Pop · R&B | **trip hop · pop** |
| 3 076 | Hip-Hop · Rap | **hip hop · boom bap** |
| 3 040 | Electronic · Jazz | **electronic · breakbeat** |
| 2 273 | Metal · Grunge | alternative rock · grunge |
| 2 151 | Rock · Folk | folk · rock |
| 1 766 | Traditional · Celtic | folk · celtic |
| 1 444 | Children's · Pop | **folk · singer-songwriter** |
| 924 | Classical · Soundtrack | classical · *amapiano* |
| 814 | Jazz · Big Band | **jazz · funk** |
| 570 | Children's · Spoken & Audio | *hip hop · singer-songwriter* |

**Neuf familles y gagnent, une est équivalente, deux y perdent.** Le vocabulaire
est bien plus spécifique — `boom bap`, `nu metal`, `trip hop`, `roots reggae`,
`afrobeat`, `anti-folk`, `chamber pop`, `nu jazz` là où les fichiers disent
« Rock » et « Pop ».

Les deux échecs s'expliquent, et aucun n'est un défaut de méthode :

- **`amapiano` sur la famille d'Einaudi et Satie** : un tag erroné sur le seul
  Yann Tiersen. MusicBrainz est contributif, donc bruité — un contrôle par le
  nombre de votes est nécessaire, la donnée existe (`count`).
- **la famille de 570** — chant breton *a cappella*, conte lu, chanson jeunesse :
  ses artistes n'ont **aucun** genre chez MusicBrainz. Là où les tags des
  fichiers, eux, disaient quelque chose de juste.

## Ce qu'il faut en conclure

**Les trois sources ne se remplacent pas, elles se complètent**, et chacune
répond à une question différente :

| source | couverture | granularité | vocabulaire |
|---|---|---|---|
| tags des fichiers | 90 % des morceaux | album, saisi à la main | 300 valeurs, grossières |
| MusicBrainz | 92,6 % ont un identifiant, 75 % des artistes ont un genre | **artiste** | curé, spécifique, un peu bruité |
| modèle audio | 100 % | **morceau** | inclut voix, texture, instrumentation |

Ordre proposé :

1. **MusicBrainz d'abord.** C'est le meilleur rapport résultat/effort : les
   identifiants sont déjà là, l'API est gratuite, le cache est définitif, et ça
   corrige neuf familles sur douze. Retenir les genres par nombre de votes pour
   écarter le bruit, et **garder le tag du fichier en repli** quand MusicBrainz
   ne sait rien — c'est précisément le cas de la famille bretonne.
2. **Le modèle audio ensuite**, pour ce qu'aucune taxonomie ne donne. La tour
   texte de CLAP suffit et n'ajoute aucune dépendance (`docs/suite.md`,
   chantier 7).

**Limite structurelle de MusicBrainz, à connaître avant de s'engager** : ses
genres sont attachés à l'*artiste*, pas au morceau. Bob Marley apparaît dans
deux de nos familles — la groove et la pop-rock à voix — et recevra les mêmes
étiquettes dans les deux. Seul un modèle audio distingue deux morceaux d'un même
artiste. C'est le plafond de cette voie, et la raison de ne pas s'y arrêter.

## Reproduire

```bash
python3 aspirer.py [chemin/vers/rusty-music.db]   # ~30 min, reprend sur cache
python3 nommer.py  [chemin/vers/rusty-music.db]   # instantané
```

`aspirer.py` respecte la limite d'une requête par seconde et s'annonce par un
agent identifiant l'application et un contact — deux conditions d'accès à
MusicBrainz, pas des recommandations. Il interroge les artistes les plus
représentés en premier : la couverture en morceaux monte alors bien plus vite
que la couverture en artistes, et le sondage devient exploitable avant d'être
terminé. Le cache `genres.json` est écrit après chaque réponse.
