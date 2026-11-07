# Lama sans Ollama : un modèle exécuté dans le processus

Étude de la phase 7 (1ᵉʳ nov. 2026), puis, après accord, sondage et
intégration le même jour : **Lama a désormais un modèle intégré** (Gemma 4
E4B, llama.cpp lié), Ollama reste un choix. Les chiffres sont plus bas
(« Sondage »).

## Ce que Lama demande au modèle

Un appel unique par demande (`crates/lama/src/ollama.rs::interpreter`) :
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

## Sondage : les chiffres (300 demandes de MusicRecoIntent)

Même schéma, même consigne, même lecture de la réponse que par Ollama
(`ollama::depuis_reponse`), Gemma 4 E4B en GGUF Q4_0, M4 Pro 24 Go. Les deux
moteurs ont tourné l'un après l'autre sur le même code
(`crates/lama/examples/robustesse.rs`, `ROB_LOCAL=`), notés par
`experiments/musicrecointent/noter.py`.

| [spec] | Ollama (`gemma4:e4b-mlx`) | Intégré (Q4_0) |
|---|---|---|
| Erreurs JSON | 0 | 0 |
| Genre voulu dans `genres` | 82,2 % | 83,7 % |
| Genre voulu retenu (genres ou étapes) | 94,8 % | 97,0 % |
| Genre refusé exclu | 66,7 % | 66,7 % |
| Entité nommée → départ ou étape | 87,2 % | 87,8 % |
| Décennie exacte | 6/8 | 7/8 |
| Genre / énergie / exclusion inventés | 2,7 / 0,7 / 1,0 % | 2,3 / 0,7 / 0,3 % |
| Popularité inventée | 0,7 % | **13,3 %**, puis 0 % (voir ci-dessous) |
| Latence médiane / max | **2,1 s** / 17,6 s | 4,7 s / 11,4 s |

- **Qualité égale.** Le seul écart, la popularité déduite d'une référence
  (« like Elvis » → `connu`), est corrigé pour les deux moteurs par
  `ancrer_popularite` : rejoué sur les mêmes réponses (`ROB_REJOUER`),
  0 % des deux côtés, les deux popularités dites (« workout hits »,
  « most underrated ») gardées, aucune autre mesure changée.
- **Plus lent qu'Ollama** : 4,7 s contre 2,1 s. Le chemin pour y arriver :
  13,6 s au premier essai ; 8,9 s en gardant l'état du contexte après la
  consigne (2 600 jetons lus une seule fois, 4 à 5 s gagnées) ; 5,3 s avec
  llguidance au lieu de la grammaire GBNF de llama.cpp (mêmes réponses), sur
  cinq demandes ; 4,7 s sur les 300. Sans contrainte, la génération mesurée
  était de 54 jetons/s ; celle sous llguidance n'a pas été chronométrée seule.
- **Chargement** : 3,9 s depuis le SSD (90 à 160 s depuis un disque USB).
  **Mémoire** : 5,1 Go résidents pendant l'usage, rendus après 3 minutes
  d'inactivité. **Binaire** : 14,4 Mo de plus.
- **Gabarit de conversation** : `llama_chat_apply_template` ne connaît pas
  celui de Gemma 4 (un programme Jinja de 18 Ko). minijinja (Apache-2.0) le
  rend tel que le GGUF le fournit, rien n'est réécrit à la main.
- `cargo deny` : une licence de plus, MIT-0 (`borrow-or-share`, via
  llguidance), compatible GPL-3.

## Intégration

- `crates/lama/src/llm_local.rs` (feature `lama-local`) : chargement,
  génération contrainte, cache de la consigne.
- `apps/desktop/src/explorer/modele_local.rs` : téléchargement en passe de
  fond (révision `a69f018` épinglée, SHA-256 vérifié), chargement à la
  première demande, éviction après inactivité (`liberer_si_inactif`).
- Lama prend le modèle intégré quand rien n'est choisi et qu'il est
  téléchargé ; le sélecteur 🦙 le propose à côté des modèles d'Ollama.
  Sans aucun modèle utilisable, l'accueil propose de le télécharger.
- `scripts/release.sh` construit avec `--features lama-local` (cmake requis).
  La CI et `verifier.sh` ne la construisent pas : seul `cargo deny` voit ces
  dépendances.
- **Non vérifié** : le paquet universel (`release.sh --universal`, llama.cpp
  compilé pour x86_64 depuis arm64) ; Linux et CUDA.

## Sans modèle utilisable, dans l'interface

Avant, la demande partait, et une erreur de connexion revenait après coup.
Désormais :
- à l'entrée du panneau Lama et avant chaque demande, l'interface interroge
  `lama_disponibilite` (`apps/desktop/src/explorer/lama.rs`) : le modèle
  intégré (prêt, à télécharger, ou absent de cette version) et Ollama (prêt,
  sans modèle, absent) ;
- si aucun des deux ne peut servir, l'accueil dit quoi faire : télécharger le
  modèle intégré (bouton, avancement en Go), ou, dans une version compilée
  sans lui, installer Ollama (`ollama pull gemma4:e4b`, étiquette vérifiée
  dans le registre d'Ollama) ; « Réessayer » relance le sondage ;
- le champ reste visible, et une demande faite sans modèle ramène à cette note
  au lieu d'échouer.

Deux tests de fumée couvrent ces cas.
