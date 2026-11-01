# Lama sans Ollama : un modèle exécuté dans le processus

Étude de la phase 7 (1ᵉʳ nov. 2026). **Rien n'est implémenté côté modèle** :
ce document décrit l'état de l'art et la recommandation. L'implémentation attend un
accord. Seule l'absence d'Ollama est désormais propre dans l'interface (voir
la fin du document).

## Ce que Lama demande au modèle

Un appel unique par demande (`crates/core/src/ollama.rs::interpreter`) :
- **décodage contraint par un schéma JSON** (`format` : 20 clés requises,
  enums, types nullables `["string", "null"]`, tableau d'objets `parties`).
  C'est indispensable, parce qu'un petit modèle à qui le schéma laisse le choix omet
  des clés : 20 prompts conformes sur 53, contre 46 avec toutes les clés
  requises ;
- température 0, 700 jetons de sortie au plus, contexte de 6 144 jetons
  (consigne, 60 genres et schéma) ;
- un petit modèle : gemma4:e4b sur le banc (`experiments/musicrecointent` :
  300 prompts, 0 erreur JSON, 2,1 s médiane avec Ollama sur Apple Silicon).

Un remplaçant doit donc fournir **le décodage contraint par schéma**, et pas
seulement « du JSON ».

## Les candidats

Versions et licences relevées sur crates.io et Hugging Face le 1ᵉʳ nov.

| | Licence | Version | Metal / CUDA | GGUF | Schéma JSON | Gemma 4 E4B | Construction |
|---|---|---|---|---|---|---|---|
| `llama-cpp-2` (liaison de llama.cpp) | MIT ou Apache-2.0 | 0.1.160 (15 oct.), publication hebdomadaire | oui / oui | oui | oui : `json_schema_to_grammar` (GBNF, feature `common`) et llguidance (feature `llguidance`) | oui (GGUF officiels `ggml-org`) | C++ par cmake |
| `mistralrs` | MIT | 0.8.1 sur crates.io (2 avr.) ; v0.9.5 sur GitHub (19 oct.) | oui / oui | oui | oui (llguidance) | oui depuis 0.8.0 | Rust (base candle), arbre de dépendances lourd |
| `candle` | MIT ou Apache-2.0 | 0.11.1 (31 oct.) | oui / oui | oui, sauf Gemma 4 | **non** : il faudrait brancher `llguidance` soi-même | poids non quantifiés seulement | Rust |
| Burn | MIT et Apache-2.0 | — | — | non | non | non : `llama-burn` a quitté tracel-ai/models | — |
| `kalosm` | MIT/Apache-2.0 | 0.4.0 (9 févr. 2025), en réécriture | wgpu | oui | parseurs propres | non vérifié | Rust |

Écartés :
- **Burn** (pas de modèle de langue maintenu, ni de décodage contraint) ;
- **candle seul** (ni Gemma 4 quantifié, ni décodage contraint : ce serait réécrire ce que mistral.rs fait déjà) ;
- **kalosm** (sans version depuis 2025).

### Les poids

- **Gemma 4 E4B : Apache-2.0**, sans accès restreint (`google/gemma-4-E4B-it`).
  Les conditions Gemma propres aux versions 1 à 3 ne s'appliquent plus.
  Téléchargement au premier usage sans difficulté, et republication possible.
  GGUF officiels (`ggml-org/gemma-4-E4B-it-GGUF`) : **Q4_0 4,6 Go**, Q8_0
  8,0 Go, BF16 15,1 Go.
- Qwen2.5-3B-Instruct : « Qwen RESEARCH LICENSE », non commerciale. La politique
  du projet l'admet, mais ce n'est pas un bon défaut. Qwen3 1,7 B et 4 B sont
  sous Apache-2.0.

## Recommandation : `llama-cpp-2`, Gemma 4 E4B en GGUF

1. **Décodage contraint par deux voies.** Si la grammaire GBNF bute sur
   notre schéma, llguidance prend le relais sans changer de bibliothèque. Le
   convertisseur GBNF de llama.cpp ignore **sans avertir** ce qu'il ne gère pas
   (documenté : `properties` mêlé à `anyOf`, `$ref` imbriqués, nombres non
   entiers). Les enums et les types nullables de notre schéma sont à éprouver.
2. **Le format des modèles qu'on trouve partout** (GGUF) : Gemma 4 E4B en
   version officielle, Qwen3 en repli.
3. **La liaison la plus suivie**, publiée au rythme de llama.cpp.

**mistral.rs** est l'alternative sérieuse : tout Rust, llguidance natif,
Gemma 4 pris en charge. Mais la version publiée a sept mois de retard sur le
dépôt (il faudrait une dépendance `git`), son API change d'une version à
l'autre, et ses dépendances sont nombreuses.

**Une réserve.** Je n'ai pas vérifié qu'Ollama passe par llama.cpp pour ces
modèles : le modèle installé ici est `gemma4:e4b-mlx`, servi par MLX. Les
2,1 s et le 0 erreur JSON du banc ne se transposent donc pas tels quels. Il
faut les mesurer.

## Risques

- **Construction.** `llama-cpp-2` compile llama.cpp en C++ (cmake ; nvcc pour
  CUDA). Cela touche la CI, la compilation croisée aarch64 et le temps de
  compilation. Taille ajoutée au binaire : non mesurée.
- **Licences liées.** `cargo deny check` reste à passer sur ces dépendances.
- **Mémoire.** Un modèle de 4,6 Go (Q4_0) chargé dans le processus de
  l'application. Il faut le charger à la demande et le libérer après un temps
  d'inactivité.
- **Déterminisme.** La température 0 rend les sorties probablement stables,
  sans garantie d'un backend à l'autre (Metal, CUDA).
- **Qualité.** La quantification Q4_0 peut dégrader l'interprétation par rapport
  au modèle servi par Ollama. Seul le banc le dira.

## Plan proposé, si tu donnes ton accord

1. **Sondage** (`experiments/lama-en-processus/`, hors workspace) : un
   exemple Rust avec `llama-cpp-2` (`metal`, `common`, `llguidance`), Gemma 4
   E4B en Q4_0 puis Q8_0, **notre schéma tel quel**, sur les 300 prompts de
   `musicrecointent`. On compare à Ollama les erreurs JSON, les notes de
   `noter.py` et `noter_playlist.py`, la latence et la mémoire. On mesure aussi
   la taille du binaire et on passe `cargo deny`. Décision prise sur ces
   chiffres.
2. **Intégration** (si le sondage est concluant) :
   - un second moteur derrière `ollama::interpreter` : même consigne, même
     schéma, même `normaliser` ;
   - les poids téléchargés à la source au premier usage, inscrits dans
     `MODELES.md` et dans « À propos » ;
   - Ollama reste un choix possible (icône 🦙), le moteur embarqué devient le
     défaut ;
   - une feature de compilation, pour que la CI et les machines sans
     compilateur C++ s'en passent.

## Déjà fait : l'absence d'Ollama dans l'interface

Avant, la demande partait, et une erreur de connexion revenait après coup.
Désormais :
- à l'entrée du panneau Lama et avant chaque demande, l'interface interroge
  `lama_disponibilite` (`apps/desktop/src/explorer/lama.rs`) ;
- si Ollama ne répond pas, ou n'a aucun modèle, l'accueil dit quoi faire
  (`ollama pull gemma4:e4b`, étiquette vérifiée dans le registre d'Ollama)
  avec un bouton « Réessayer » ;
- le champ reste visible, et une demande faite sans Ollama ramène à cette note
  au lieu d'échouer.

Un test de fumée couvre ce cas.
