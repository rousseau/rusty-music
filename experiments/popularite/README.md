# Sonde de popularité — ListenBrainz et Deezer valent-ils la peine ?

Hors du workspace. Sondage du 2 septembre 2026, phase 0 de `docs/popularite.md`.
Question : **avant d'écrire le moteur, quelle part de la bibliothèque une
popularité externe atteindrait-elle, et laquelle des deux sources ouvertes
(ListenBrainz, Deezer) mérite d'entrer en phase 1 ?**

Rappel de la contrainte : pas de clé d'API. Cela laisse ListenBrainz (endpoint
`/1/popularity/*` public, écoutes agrégées de la communauté, CC0) et Deezer
(API publique). Last.fm, Discogs, Spotify, YouTube demandent tous une clé ou un
jeton et sont écartés.

## L'échantillon

200 morceaux tirés au hasard (`random`, graine fixe) parmi les 24 826
enregistrements de la bibliothèque qui portent un `mb_recording_id` — soit 92 %
des 27 170 morceaux. Pour chacun, le release-group MusicBrainz est résolu par
`(mb_album_artist_id, titre normalisé)`, le même lien que le moteur utilise
déjà pour les genres (`Library::mb_albums`) : **186 / 200 (93 %)** trouvent
leur album.

- ListenBrainz s'interroge **par MBID**, par lots POST — enregistrement et
  release-group. Aucun rapprochement.
- Deezer s'interroge **par recherche** `artist:"…" track:"…"` (ou `album:"…"`),
  puis on ne retient un résultat que si le nom d'artiste rendu concorde.

## Couverture

| | couverts | % |
|---|---:|---:|
| release-group résolu (lien titre) | 186 / 200 | 93 % |
| **ListenBrainz — enregistrement** | 177 / 200 | **88 %** |
| **ListenBrainz — release-group** (sur 186) | 177 / 186 | **95 %** |
| **ListenBrainz — enreg. OU album** (échelon réel affiché) | 194 / 200 | **97 %** |
| Deezer — piste, artiste concordant, `rank > 0` | 159 / 200 | 80 % |
| Deezer — album, artiste concordant, `fans > 0` | 137 / 200 | 69 % |
| **au moins une source → jauge affichable** | 197 / 200 | **98,5 %** |
| aucune source → grisé | 3 / 200 | 1,5 % |

Ventilée par effectif d'artiste (quartiles), **ListenBrainz est uniforme**
(96–98 % partout, y compris les artistes dont on n'a que quelques morceaux).
Deezer décroche sur la longue traîne : 72 % pour le quartile le moins fourni,
92 % pour le plus fourni — attendu, son catalogue suit le grand public.

## Fiabilité du rapprochement Deezer

Sur les résultats retenus (artiste concordant) :

- **piste : 2 % d'artiste faux** (4 / 163). En resserrant à **artiste +
  titre concordants**, il reste **1 erreur sur 159** (« Forzh Penaos — Allez…
  gavotte ! » rapproché de « Kost Ar C'hoat » du même groupe) : **99,4 % des
  rapprochements acceptés sont justes**, pour 79 % de couverture.
- **album : 6 % d'artiste faux** (9 / 146), mais la plupart sont des variantes
  du même nom (« The Wailers » ↔ « Bob Marley & The Wailers », « Femi Kuti » ↔
  « Femi Anikulapo Kuti »). Titre resserré : 96 % justes, 66 % de couverture.

Les écarts bénins sont nombreux et sans conséquence : casse, apostrophe typo­
graphique, « feat. » retiré, mention d'édition (« … (Instrumental Version) »).

**Conclusion : le rapprochement Deezer est fiable à condition d'exiger la
concordance du titre en plus de l'artiste** — pas seulement l'artiste, comme
la sonde le faisait pour mesurer le taux d'erreur.

## Accord entre sources (Spearman sur les rangs)

| paire | n | ρ |
|---|---:|---:|
| ListenBrainz enregistrement ↔ Deezer `rank` piste | 144 | **0,55** |
| ListenBrainz release-group ↔ Deezer `fans` album | 129 | 0,48 |
| ListenBrainz enregistrement ↔ ListenBrainz release-group | 160 | 0,61 |

Corrélations **modérées**. C'est le résultat le plus intéressant : Deezer n'est
**pas redondant** avec ListenBrainz. Le public de ListenBrainz est petit et
orienté (prog, metal, indie) ; Deezer capte un grand public que l'autre rate.
Les deux ensemble décrivent mieux « la notoriété dans le monde » que l'un ou
l'autre seul — ce qui justifie le **mélange par rang** prévu au document.

L'accord enregistrement ↔ release-group (0,61) confirme aussi que le **repli
vers l'album est cohérent** : proche, sans être identique.

## Échelles brutes (min / médiane / max)

| | min | médiane | max |
|---|---:|---:|---:|
| LB enreg. écoutes | 1 | 2 337 | 1 721 020 |
| LB enreg. auditeurs | 1 | 584 | 196 121 |
| LB release-group écoutes | 1 | 3 300 | 779 536 |
| Deezer piste `rank` | 10 922 | 233 454 | 966 953 |
| Deezer album `fans` | 2 | 2 826 | 541 694 |

Quatre ordres de grandeur, des planchers et des plafonds propres à chaque
métrique (Deezer `rank` est borné à ~1 M et ne descend jamais à zéro). **On ne
mélange pas ces nombres — on mélange leurs rangs.** Confirme la « valeur
affichée » du document.

## Verdict

**Les deux sources entrent en phase 1**, avec un partage clair des rôles :

- **ListenBrainz est le socle.** 97 % de couverture à l'échelon
  enregistrement-ou-album, aucun rapprochement, aucune clé, licence alignée.
  Elle suffirait à elle seule.
- **Deezer est le second signal.** 80 % de couverture piste, fiable **si l'on
  exige artiste + titre concordants** (la sonde ne vérifiait que l'artiste — la
  passe devra faire les deux). Corrélation modérée avec ListenBrainz : il
  apporte de l'information, pas du bruit redondant. À l'échelon album, moins
  utile (69 %, un aller-retour de plus pour `fans`) — **piste seulement au
  départ**, album plus tard si besoin.

Ajustements à porter dans le plan (`docs/popularite.md`) :

1. `deezer.rs` : match **artiste + titre normalisé**, pas artiste seul.
2. Deezer se limite à l'échelon **piste** en phase 1 (le `rank` est dans la
   recherche, pas de second appel ; l'album demanderait `/album/{id}` pour
   `fans` et ne couvre que 66 %).
3. Rien à changer au reste : trois tables, mélange médian des rangs, repli
   enregistrement → release-group → grisé, jauge à 5 segments.

Spotify / YouTube ne sont **pas nécessaires** : 1,5 % de morceaux sans aucune
source, ce n'est pas ce qui justifierait une clé et un quota.

## Reproduire

```bash
python3 aspirer.py [chemin/vers/rusty-music.db]   # échantillon + fetch, ~3 min, reprend sur cache
python3 rapport.py                                # bilan, instantané
```

`aspirer.py` construit `echantillon.json` (reproductible, graine fixe) au
premier passage, puis remplit `cache.json` après chaque réponse. Les deux API
sont publiques ; les clients s'annoncent par un `User-Agent` lu de
`RUSTY_MUSIC_CONTACT`. `cache.json` et `echantillon.json` ne sont pas
versionnés — ils porteraient le contenu d'une vraie bibliothèque — et se
refont en quelques minutes.
