# Banc de compatibilité de stems (F2) : COCOLA contre Stem-JEPA

But : classer des batteries candidates pour un morceau source (« laquelle
irait avec ce mélange ? ») par un modèle appris, et confronter ce classement
à la proximité CLAP seule (le préfiltre de F1) puis à l'oreille. Rien n'est
porté en Burn avant ce jugement.

## Licences et poids (lus le 9 nov. 2026)

| | code | poids | où | entrée |
|---|---|---|---|---|
| **COCOLA** (gladia-research-group/cocola) | MIT | `COCOLA_HP_v1` (MoisesDB, Slakh2100, CocoChorales) : **licence non déclarée** | Google Drive, lien du README du dépôt | 5 s mono à 16 kHz ; scores harmonique, percussif ou les deux (`EmbeddingMode`) ; PyTorch 2.2, Lightning 2.2 |
| **Stem-JEPA** (SonyCSLParis/Stem-JEPA, ISMIR 2024) | LGPL-3.0 | **aucun poids publié** : ni lien dans le README, ni *release*, ni dépôt Hugging Face | — | WAV 4 canaux (basse, batterie, autre, voix) à 16 kHz pour l'entraînement ; instrument donné par l'indice du canal |
| **Stem-JEPA zéro-shot** (arXiv 2411.19806) | **pas de code trouvé** | **aucun poids** | article seul (CC BY 4.0) | instrument donné en texte |

Conséquences, d'après la politique du dépôt (`CLAUDE.md`, `MODELES.md`) :

- COCOLA est utilisable. Le code MIT peut être lié. Les poids, sans licence
  déclarée, se téléchargent à la source et ne se redistribuent jamais. Ils
  sont archivés sur le disque de dev, parce que le Google Drive peut
  disparaître (`docs/recherche-editer-pratique-creation.md`, risques).
- Stem-JEPA ne peut pas être évalué tel quel. Il faudrait l'entraîner
  nous-mêmes : données multipistes (MoisesDB et autres, chacune sous sa
  licence), GPU (la PGX), Hydra/Dora/Lightning. Ce n'est plus un banc de
  F2, c'est un chantier en soi. La question est posée au relecteur.

## État

Licences lues et écrites. Le banc lui-même (20 sources × 10 candidates,
greffes à l'aveugle) attend la décision sur Stem-JEPA.
