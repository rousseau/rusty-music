# Vocabulaire CLAP-texte

Deux fichiers générés hors ligne par
`experiments/clap-texte/preparer_vocabulaire.py`, à committer ici une fois
produits — voir `docs/nommage-familles.md` :

- `vocabulaire.bin` — table `N × 512` `f32` petit-boutien, les empreintes
  CLAP-texte du vocabulaire (une par genre MusicBrainz retenu).
- `vocabulaire.txt` — `genre<TAB>phrase` par ligne, même ordre.

Contrairement aux modèles de `models/` (des centaines de Mo, reconstruits
par `scripts/preparer-*.sh` et gitignorés), cette table pèse quelques
centaines de Ko : un produit fini, pas un poids de modèle, committé comme
n'importe quel autre fichier de données du dépôt.

Absents (avant la première exécution du script, ou tant qu'Ollama n'est pas
installé) : `crates/analysis/src/vocabulaire_texte.rs::charger()` rend alors
`None` sans erreur — le vote CLAP-texte est simplement absent, MusicBrainz et
Last.fm votent seuls le nom des familles.
