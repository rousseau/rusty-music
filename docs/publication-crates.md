# Publier des crates autonomes

Préparation de la phase 8 (1ᵉʳ nov. 2026). **Décision (même jour) : on
garde les crates dans le workspace, sans rien publier.** Ce document reste
comme l'état des lieux à reprendre si la question revient : quoi publier, ce
qu'il resterait à faire et à décider.
Une publication sur crates.io est définitive : une version peut être
retirée (`yank`), jamais effacée.

## L'état actuel

Les douze crates du workspace sont marquées `publish = false`. Aucune n'a
`repository`, `readme`, `keywords` ni `categories`. Toutes les autres,
sauf `osm` et `superres`, dépendent de `rusty-music-core` par chemin, sans
version.
`editor` dépend de `demucs-core` par `git` (fork `rousseau/demucs-rs`),
ce que crates.io refuse. Licence de tout le workspace : GPL-3.0-or-later.
Rust 1.82 au minimum, Burn 0.21. Le dépôt GitHub `rousseau/rusty-music` est
public.

## Ce qui vaut d'être publié

Les crates de l'application (`core`, `player`, `analysis`, `carto`,
`editor`, `cli`, `desktop`, `mobile`) servent l'application : leur API suit
ses besoins, et `analysis` traduit CLAP au build depuis des poids absents de
crates.io. **Elles restent privées.**

Ce qui n'existe nulle part ailleurs en Rust, ce sont trois portages en Burn,
presque détachés du reste. Chacun n'emprunte au projet que
`rusty_music_core::modeles`, pour trouver ou télécharger ses poids.

| Portage | Où | Taille | Code d'origine | Poids | Tests sans poids | Déjà sur crates.io |
|---|---|---|---|---|---|---|
| **BS-RoFormer « SW »** (6 stems) | `crates/editor/src/roformer/` | 949 lignes | MIT (lucidrains, MSST) | licence non déclarée : à télécharger à la source, jamais redistribués | 3 (+1 ignoré, poids requis) | `uvr-roformer` 0.1.2 : modèle « 1296 » de voix seul, processeur seul |
| **MuScriptor** medium (transcription de basse) | `crates/transcription/src/muscriptor/` | 1 817 lignes | MIT (Kyutai × Mirelo) | CC BY-NC 4.0, accès après acceptation des conditions sur Hugging Face | 10 | rien |
| **all-in-one** (sections d'un morceau) | `crates/transcription/src/sections/` | 904 lignes | MIT (mir-aidj) | MIT ; convertis en safetensors et republiés (`rousseau/allinone-harmonix`) | 5 | rien |

Chacun a sa parité mesurée contre le code d'origine (`MODELES.md` : cosinus
1,0000000 pour BS-RoFormer ; 639 jetons sur 639 pour MuScriptor ; mêmes
sections sur 94 morceaux pour all-in-one), avec ses exemples
(`examples/parite_*`).

Noms libres sur crates.io au 1ᵉʳ nov. : `bs-roformer`, `muscriptor`,
`allinone`, `all-in-one` (et leurs variantes `burn-*`).

**Seconde vague, intérêt moindre** :
- `superres` (AERO par ONNX Runtime, 765 lignes) : une enveloppe mince
  d'`ort`, avec des poids sous condition non commerciale (MUSDB18-HQ) ;
- `osm` (import d'un extrait OpenStreetMap, 1 178 lignes, aucune
  dépendance au projet) : son schéma SQLite sert la carte du projet.

**demucs** : plutôt qu'une crate de plus, proposer les corrections du fork
`rousseau/demucs-rs` à l'original (`nikhilunni/demucs-rs`). Tant que
`editor` dépend d'un `git`, il ne peut de toute façon pas être publié.

## Décisions à prendre (les tiennes)

1. **Licence des crates publiées.** Le code est le tien, et celui qu'il
   porte est sous MIT. Deux choix :
   - **GPL-3.0-or-later**, comme le projet. Cohérent, mais beaucoup de
     projets Rust ne peuvent pas lier du GPL : l'usage en sera limité ;
   - **MIT ou Apache-2.0**, l'usage de l'écosystème Rust et la licence des
     originaux. L'application, sous GPL-3, peut toujours les utiliser.
2. **Langue de l'API et de la documentation.** Les identifiants publics sont
   en français (`Roformer::charger`, `separer`, `masques`…), comme tout le
   projet. Pour crates.io, il faut au minimum une documentation en anglais,
   et décider si l'API reste en français.
3. **Noms.** Proposition : `bs-roformer`, `muscriptor`, `allinone`.
4. **Compte crates.io** propriétaire des crates : le tien ; un jeton de
   publication que tu gardes, jamais dans le dépôt.

## Le travail avant publication, par crate

1. **Extraire** chaque portage en crate du workspace (`crates/bs-roformer/`,
   …). L'application la reprend comme dépendance par chemin.
2. **Couper le lien avec `rusty_music_core::modeles`.** La crate reçoit le
   chemin du fichier de poids ; trouver ou télécharger reste à l'application.
   Deux autres emprunts : MuScriptor prend `basic_pitch::reechantillonner` à
   sa crate, all-in-one prend `VersFlottant` à MuScriptor. Les recopier, ou
   publier all-in-one en dépendance de MuScriptor.
3. **Métadonnées** : `description`, `repository`, `readme`, `keywords`,
   `categories`, `rust-version`, `license`. `include` aussi, pour rester
   sous la limite de 10 Mo et ne pas embarquer de fichiers de test lourds.
4. **API.** Elle expose des types de Burn (`Tensor<B, 3>`, `Backend`) : chaque
   version majeure de Burn (0.21 aujourd'hui) imposera une version majeure de
   la crate. Le noter dans le README. Faire passer les features de backend
   (`ndarray`, `wgpu`, `metal`) sans en imposer une.
5. **Documentation** : README avec un exemple minimal, où trouver les poids
   et leur licence (pour MuScriptor, les conditions d'usage à accepter) ;
   `#![warn(missing_docs)]`.
6. **Contrôles** : tests sans poids dans la CI, exemples de parité gardés
   (poids requis) ; `cargo package --list` et `cargo publish --dry-run` ;
   `cargo semver-checks` à partir de la deuxième version.

## L'ordre proposé

1. Tes quatre décisions ci-dessus.
2. all-in-one d'abord : le plus petit, et des poids sous MIT déjà republiés.
   C'est lui qui éprouve le circuit (extraction, API, CI, `--dry-run`).
3. BS-RoFormer, puis MuScriptor.
4. Pour chacune : `cargo publish --dry-run` relu ensemble, puis publication
   **seulement après ton accord**, crate par crate.
