# Rusty Music v0.1.0

Suite musicale locale et autonome, écrite intégralement par Claude. Tout le
calcul se fait sur la machine : aucun service distant, aucune clé d'API.

## Ce que ça fait

- **Écouter** — lecture soignée : décodage en mémoire (plus de coupures),
  rééchantillonnage propre vers la carte son, pochettes, infos, une ligne de
  qualité et un excitateur psychoacoustique optionnel (« E »).
- **Explorer** — une carte 2D de la bibliothèque par similarité sonore
  (empreintes CLAP), affichée comme le **plan de Paris** : familles → quartiers,
  artistes → rues, morceaux → adresses. Chemins entre deux morceaux (quatre
  modes), itinéraires sur la voirie, filtres par tempo/énergie/année, familles
  nommées par genre.
- **Éditer** — démixage en stems (HTDemucs), vitesse et hauteur (globales ou par
  stem), greffe d'un stem d'un autre morceau calée sur les temps, export.
- **HD** — super-résolution audio hors ligne (AERO) : reconstruit le haut du
  spectre d'un fichier compressé.
- **Métadonnées** — genres MusicBrainz, descripteurs audio (tempo, tonalité,
  énergie), popularité générale (ListenBrainz + Deezer), fil « Découvrir »
  (nouveaux disques, collaborations, artistes voisins).

## Installation (macOS)

Le `.dmg` ci-dessous **n'est ni signé ni notarisé**. Au premier lancement :
**clic droit sur l'app → Ouvrir**, puis confirmer — ou
`xattr -dr com.apple.quarantine "/Applications/Rusty Music.app"`.

Au premier lancement, choisir le dossier de musique à surveiller. L'analyse
(empreintes, projection, descripteurs) tourne en tâche de fond et est
reprenable.

Modèles et plan de ville sont embarqués dans le paquet (~0,4 Go).

## Limites connues

- **macOS uniquement** pour l'instant.
- **Placement sur la carte en cours d'itération** : au niveau du bâtiment, les
  genres restent trop mêlés et le cœur de la ville trop régulier — un
  redécoupage en corridors est prévu.
- Le lecteur n'a pas encore aléatoire / répétition ni réordonnancement de la
  file.
- Pas de pochettes ni de bios enrichies (Cover Art Archive / Wikidata restent à
  câbler).

## Licences

- **Code** : GPL-3.0-or-later.
- **Plan de la carte** : © les contributeurs OpenStreetMap, sous **ODbL** —
  l'attribution est affichée dans le coin de la carte.
- **Poids des modèles**, distincts du code : CLAP (Apache-2.0), HTDemucs (MIT),
  AERO (voir `slp-rl/aero`).

## Construire soi-même

`README.md` → « Démarrer ». En résumé : Rust 1.82+, un compilateur C,
`./scripts/telecharger-modeles.sh && ./scripts/telecharger-ville.sh`, puis
`./scripts/release.sh`.
