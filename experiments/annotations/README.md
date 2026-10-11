# Annotations publiées, audio de la bibliothèque

Des jeux annotés publient leurs annotations sans l'audio. On les apparie aux
morceaux de la bibliothèque : la vérité vient du jeu, l'audio est le nôtre,
donc de vrais mélanges sans rien télécharger d'autre que du texte. Plan :
`docs/plan-editer-pratique-creation.md`, section 0.4.

```
python apparier.py                                        # → $RUSTY_DEV/jeux/annotations/appariement.json
uv run --with mir_eval --with numpy python pulsation.py   # Beat This! contre les temps annotés
```

## Jeux

Dans `$RUSTY_DEV/jeux/annotations`, jamais versionnés.

| Jeu | Annotations | Appariés (11 oct.) |
|---|---|---|
| Isophonics (Beatles, Queen, Carole King, Zweieck, Michael Jackson) | sections, accords, tonalités ; temps (Beatles, Zweieck) | 66 sur 301 |
| SALAMI (`DDMAL/salami-data-public`) | sections, deux annotateurs | 24 sur 1 214 |
| Harmonix Set (`urinieto/harmonixset`) | temps, premiers temps, sections | 6 sur 912 |
| Raveform (Kim, TISMIR 2026) | temps, sections EDM | 1 seul morceau annoté en commun : écarté |

En tout, 96 morceaux : 33 avec des temps, 96 avec des sections, 30 avec
accords et tonalités.

- **Appariement** : par MBID d'enregistrement, sinon par artiste et titre
  normalisés. La durée doit tenir à ±3 s, sinon c'est une autre édition.
- **Calage** : nos fichiers sont souvent des remasters qui ne commencent
  pas au même instant (jusqu'à 3 s d'écart). Le décalage constant est
  cherché sur les temps **et** les premiers temps, à 150 ms : un décalage
  d'un temps entier laisse le F1 des temps intact. Il est pris au centre du
  plateau, puis affiné par la médiane des résidus. Ce calage favorise un peu
  le pisteur.

## Résultats (11 oct. 2026)

**Pulsation**, Beat This! complet, portage Rust, 33 morceaux :

| | F1 temps | F1 premiers temps | CMLt | AMLt |
|---|---|---|---|---|
| ces 33 morceaux | 81,4 | 77,6 | 82,6 | 82,6 |
| GTZAN, pour mémoire | 88,2 | 77,5 | 79,1 | 89,2 |

La plupart des morceaux des Beatles sont au-dessus de 0,95. Cinq morceaux
restent sous 0,45 : *Oh! Darling*, *I Am the Walrus*, *The Fool on the
Hill*, *Magical Mystery Tour* et *I Want You*. Leur meilleur F1 à 150 ms
plafonne vers 0,6, ce qui évoque une autre version (montage, vitesse) plutôt
qu'une erreur du pisteur. Il reste à le vérifier à l'oreille.

## Pièges

- **Disque exFAT** : copier des milliers de petits fichiers y est très lent,
  et macOS sème un `._*` par fichier, même avec `cp -X`. On les supprime par
  `dot_clean -m`, puis `find -name '._*' -delete`.
- **Billboard** (McGill) : l'archive téléchargée par le lien Dropbox est
  illisible. Elle est à reprendre quand on aura besoin des accords.
