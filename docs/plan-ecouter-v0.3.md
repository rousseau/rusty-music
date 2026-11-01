# Mode Écouter — pistes pour la v0.3

Établi le 1ᵉʳ novembre 2026, une fois le plan 0.2 soldé (`plan-ecouter-v0.2.md`).
Même forme : l'état **constaté**, des **options** comparées, une
recommandation. Rien ici n'est engagé : c'est une liste à trancher.

Légende d'effort : **S** < 1 jour · **M** 1 à 3 jours · **L** > 3 jours.

## Idées reprises de Disc Player

[Disc Player](https://github.com/eudj1n/snowsky-disc-player) (MIT, © eudj1n)
est un lecteur web (Vue, Tailwind) pour la collection d'un baladeur. Trois de
ses idées sont déjà passées dans Rusty Music, sans son code ni son cadre
(`MODELES.md`) : la palette de recherche ⌘K, le thème clair atteignable avec
contraste vérifié (Règle 6), la typographie mesurée (Règle 11). Restent six
idées, examinées ici ; la sixième, le passage de la lecture entre le Mac et
l'iPhone, vit dans `plan-ios-compagnon.md` (« Passage de la lecture »).

Hors périmètre, à ne pas rouvrir : égaliseur et AutoEq, paroles et karaoké
(écartés en 0.2, points 9 et 10).

### 1. Vue plein écran « En cours » · **M**

**Chez Disc Player.** La pochette posée sur un disque qui tourne, un spectre
autour (`src/layout/VisualizerMode.vue`), les deux couleurs de la pochette en
fond (point 2).

**Constaté chez nous.** Le morceau en cours n'a pas d'écran à lui : le
transport (64 px) et l'inspecteur le montrent. `crates/player/src/spectre.rs`
calcule un **spectrogramme hors ligne** : il décode tout le fichier, rend une
image d'intensités sur un octet (axe log, 80 dB de plage), déjà mise en cache
pour le transport (`spectre_transport`). Ce n'est pas un spectre en direct :
aucun prélèvement de la sortie audio n'existe (`rodio`/`cpal` jouent hors de
la webview, où aucun `AnalyserNode` ne voit le son).

| Option | Effort | Pour | Contre |
|---|---|---|---|
| **A. Lire le spectrogramme déjà calculé à la tête de lecture** | S | rien de nouveau côté moteur : la colonne de l'image à la position courante donne l'énergie par bande, dessinée en anneau autour du disque ; synchronisée par la position du transport (200 ms, interpolée) ; marche aussi pour l'aperçu et le HD | ce n'est pas le son qui sort (le « E » et la normalisation ne s'y voient pas) ; une colonne = 46 ms, assez pour l'œil |
| B. Prélever la sortie audio (FFT sur le flux de `crates/player`) | M | le vrai son, effets compris | un fil et un canal d'évènements de plus, à 30-60 images/s entre Rust et la webview ; coût continu pendant toute l'écoute |
| C. Pochette seule qui tourne, sans spectre | S | le plus sobre | perd ce qui fait l'intérêt de la vue |

Contrat d'interface : un écran au-dessus des autres, comme la palette (le
transport reste en bas, Échap ferme), pas un sixième mode du rail ; la
rotation et le spectre s'arrêtent avec `prefers-reduced-motion` ; couleurs du
spectre : la rampe `--rampe` (Règle 3 : une rampe, pas des familles).

**Recommandation : A.** Le calcul existe et il est en cache ; B ne vaut que si
l'on veut voir l'effet du « E », ce que personne n'a demandé.

### 2. Couleurs d'album · **M** — décision à prendre (Règle 3)

**Chez Disc Player** (`src/domain/coverColours.ts`, `docs/themes.md`). Deux
couleurs plutôt que la moyenne des pixels (qui rend grises les pochettes
sombres ou bariolées) : une copie 48 × 48 rangée en cases de couleur ; le
quasi-noir, le quasi-blanc et le gris pèsent peu, la couleur fréquente et
saturée gagne, la seconde s'en écarte nettement (distance OKLab > 0,13) — à
défaut, une version plus claire et plus douce de la première. Chaque couleur
est ensuite **recalée en OKLCH pour le thème** (clarté 0,80-0,93 et chroma ≤
0,11 en clair ; clarté 0,24-0,36 et chroma ≤ 0,09 en sombre) pour que le texte
du thème garde 4,5:1. Elles colorent **les fonds seulement** ; les contrôles
gardent les couleurs du thème (« le bouton doit rester stable pour qu'on ne le
cherche pas »).

**Chez nous.** La méthode se porte telle quelle en JS (`pochette()` rend déjà
une image ; un canevas 48 × 48 suffit), ou en Rust à la lecture de la pochette
(mise en cache en base avec la pochette). Les fonds candidats : la vue « En
cours » (point 1), l'en-tête d'un album ouvert au centre, l'inspecteur.

**Le conflit.** La Règle 3 dit **couleur = famille** : une seule palette, les
douze teintes de `--familles`, pour que la couleur veuille toujours dire la
même chose. Une couleur tirée de la pochette, posée sur un fond, dit autre
chose (« cet album ») — exactement la faute de cohérence que la règle interdit,
sauf à la borner.

| Option | Effort | Pour | Contre |
|---|---|---|---|
| A. Ne pas faire | — | la Règle 3 reste entière | Écouter reste neutre, quelle que soit la musique |
| B. **Exception bornée** : fonds d'Écouter seulement (vue « En cours », en-tête de l'album ouvert), jamais Explorer, jamais un contrôle, jamais un trait ou un point | M | l'écoute prend la couleur du disque ; la carte, seule à coder les familles par la couleur, n'est pas touchée ; même forme d'exception que `palette.rs` (bornée et écrite) | une deuxième source de couleur dans l'application ; à surveiller dans l'audit (un fond teinté sous une pastille de famille) |
| C. Seulement dans la vue « En cours » | S | l'exception la plus étroite : un écran à part, où aucune famille n'est affichée | le reste d'Écouter ne change pas |

Si B ou C est retenue, le contrôle de contraste (`scripts/contraste-interface.py`)
doit s'étendre au pire cas du recalage (texte du thème sur un fond de clarté
0,80 en clair, 0,36 en sombre), et la Règle 3 recevoir l'exception par écrit.

**Recommandation** : aucune — c'est une règle du contrat ; à trancher. C
permet d'essayer sans engager le reste d'Écouter.

### 3. Choisir l'édition MusicBrainz face à face · **M**

**Chez Disc Player** (`docs/images/readme-editions.jpg`, `src/layout/AlbumCompare.vue`).
Une boîte à deux colonnes : à gauche les éditions que propose MusicBrainz
(titre · label et référence · pays · format · année · nombre de pistes, une
coche quand le nombre de pistes concorde), à droite ce que porte l'album
local ; dessous, les deux listes de pistes alignées, durées comprises. En tête,
**une phrase qui dit ce qui part sur le réseau** : « envoie le titre et
l'artiste de l'album à musicbrainz.org et liste ses éditions ».

**Constaté chez nous.** L'identité d'une piste vient de ses tags
(`mb_release_id`, `mb_recording_id`… lus par `tags.rs`) ; l'enrichissement
part d'un MBID **déjà présent** (`musicbrainz.rs`) ; aucune recherche
d'identité manquante (`stats_sans_mbid` compte seulement). « Éditions
multiples » (`editions_multiples`) repère les albums présents en plusieurs
versions dans la bibliothèque, sans proposer d'en choisir une chez
MusicBrainz. Les crédits et labels Discogs se rattachent par édition (dump
mensuel). Rien n'est écrit dans les fichiers : la base porte tout.

**Ce qu'on ferait.** Depuis un album sans identité (ou depuis « Éditions
multiples »), « Chercher l'édition » : recherche `release` par titre et
artiste, les candidates en colonne, l'album local en face, pistes alignées par
rang et par durée (écart ≥ 3 s marqué, piste en trop ou manquante marquée),
**rien d'appliqué sans « Retenir »**. Retenir écrit le `release_id` en base
(jamais dans le fichier), relance l'enrichissement de cet album (genres,
crédits Discogs par édition). L'esprit est celui du **« nom incertain »** de
`nommage-familles.md` : quand rien ne concorde nettement, la boîte le dit
(« aucune édition ne concorde ») et liste sans choisir à la place de
l'utilisateur ; une coche n'est qu'une concordance, pas une décision.

| Option | Effort | Pour | Contre |
|---|---|---|---|
| **A. Boîte de comparaison, choix manuel** | M | lisible, rien d'automatique, une phrase réseau honnête ; réutilise l'adresse de contact MusicBrainz et son débit d'1 requête/s | un album à la fois |
| B. Appariement automatique au-dessus d'un seuil, la boîte seulement pour les cas douteux | L | passe sur toute la bibliothèque (point 4) | un mauvais appariement silencieux pollue genres, crédits et carte ; contraire à l'esprit « suggéré, jamais automatique » |
| C. Empreinte AcoustID / Chromaprint | L | identifie même sans tags | une dépendance et un service de plus (clé), à passer par `rust-audio-stack.md` et `cargo deny` |

Contrat d'interface : la boîte est une surimpression au-dessus de l'écran
(comme la palette), pas un panneau de plus ; l'inspecteur reste celui de ce
qui s'écoute (Règle 1).

**Recommandation : A**, branchée d'abord sur « Éditions multiples » et sur
l'album ouvert au centre ; C seulement si A laisse beaucoup d'albums sans
candidate.

### 4. État de la bibliothèque · **L**

**Chez Disc Player.** Un récapitulatif de ce qui manque, et une passe qui
complète, annulable.

**Ce que le code sait déjà** (recensé le 1ᵉʳ novembre 2026) — rien n'est
jamais écrit dans les fichiers audio (`lofty` en lecture seule), tout est en
base, **aucun journal d'annulation** :

| Manque | Détecté | Complété | Exposé |
|---|---|---|---|
| Identités | `stats_sans_mbid` (artiste sans MBID, seulement) | non : l'enrichissement part d'un MBID présent | statistique de Bibliothèque |
| Pochettes | `pochettes_verif::suspectes` (JPEG tronqué, contrôle par dossier) | repli Cover Art Archive puis Deezer, en cache ; choix de l'image du dossier | « Pochettes suspectes », ignorer (drapeau réversible) |
| Tags | `stats_genres` (sans genre), `suspect_genres` (genre isolé dans sa famille) | genres MusicBrainz, date, genres proposés par Ollama (en base) | « Genres suspects » |
| Doublons | `editions_multiples`, `probable_duplicates` (distance acoustique + durée) | non : ni fusion ni suppression | « Éditions multiples », « Chercher les doublons » |
| Illisibles | table `scan_failures` (exclus de l'analyse) | non : seulement un nouveau scan | « Échecs de scan », écarter (en base) |

N'existe pas : recherche d'identité manquante, décompte des albums **sans
aucune** pochette, contrôle des numéros de piste et des années manquants,
fusion de doublons, une passe unique qui enchaîne les compléments, et
l'annulation.

**Ce qu'on ferait.** En tête de Bibliothèque, un bloc « État » de cinq lignes
(un compte par manque, chacun menant à sa liste déjà existante — Règle 8 :
le détail attend le geste), puis « Compléter » : une passe qui enchaîne ce qui
existe (pochettes de repli, genres, dates ; plus tard les éditions du point 3)
et dit avant de partir ce qui ira sur le réseau. **Annulable en bloc** : chaque
écriture de la passe est notée dans une table `passe_ecriture` (passe, table,
clé, valeur d'avant), « Annuler cette passe » les rejoue à l'envers ; possible
parce que tout est en base, jamais dans les fichiers.

| Option | Effort | Pour | Contre |
|---|---|---|---|
| **A. Bloc « État » + passe « Compléter » journalisée** | L | un seul endroit pour savoir ce qui manque ; annulation réelle ; répond à la Règle 5 dans Bibliothèque (aujourd'hui dix blocs ouverts d'un coup, ❌ à l'audit) | une migration (`passe_ecriture`) ; chaque écriture de l'enrichissement doit passer par le journal |
| B. Bloc « État » seulement, sans passe | M | rend visible sans rien écrire de nouveau | ne complète rien ; l'utilisateur relance les passes une à une |
| C. Écrire aussi dans les fichiers (tags, pochettes) | L | la bibliothèque s'améliore hors de Rusty Music | irréversible sans copie, touche aux fichiers de l'utilisateur : contraire au parti « la base porte tout » |

**Recommandation : B d'abord, A ensuite.** B range Bibliothèque (Règle 5) et
montre où sont les manques avant d'écrire une passe ; C écartée.

### 5. Captures sur collection fictive · **S**

**Chez Disc Player.** Les captures du README et de la documentation sont faites
par Playwright sur une **collection fictive** (`tests/e2e/mock-collection.mjs`),
jamais sur une vraie bibliothèque, dans les deux thèmes.

**Constaté chez nous.** Les tests de fumée servent déjà l'interface réelle avec
un Tauri simulé et une bibliothèque fictive (`apps/desktop/ui-tests/`,
`fixtures.js` : deux artistes, trois albums) ; les étapes 1 à 3 ci-dessus ont
été vérifiées par des captures faites ainsi. Le README n'a aucune image. La
page https://rousseau.github.io/rusty-music/ est publiée depuis `livre/`
(Quarto, `.github/workflows/livre.yml`) — **`livre/` ne se modifie pas sans
demande**.

| Option | Effort | Pour | Contre |
|---|---|---|---|
| **A. `ui-tests/captures.spec.js` + collection fictive étoffée** | S | mêmes outils que les tests de fumée ; une vingtaine d'albums aux pochettes engendrées (aplats, pas d'œuvres réelles), carte et nuage factices ; écrans × deux thèmes vers `docs/images/` ; relancé à la main (`npx playwright test captures`), pas en CI | les vraies pochettes et la vraie carte de Paris n'y sont pas : l'aperçu est schématique |
| B. Captures de l'application réelle sur la vraie bibliothèque | S | fidèles | montrent la collection de l'auteur (droits sur les pochettes, vie privée) ; la webview ne se capture pas hors de son Espace macOS |

Le README pointerait vers `docs/images/` ; pour la page du livre, les images
sont prêtes, l'auteur décide de les y mettre.

**Recommandation : A.**

### Ordre proposé

1. **5** (captures) — peu coûteux, sert la documentation de tout le reste.
2. **1** (vue « En cours », option A) — sans décision de contrat.
3. **4**, option B (bloc « État »), puis **3** (éditions face à face), qui
   alimentera la passe de l'option A du point 4.
4. **2** (couleurs d'album) une fois la Règle 3 tranchée.
5. Passage de la lecture Mac ↔ iPhone : avec l'étape 4B du compagnon
   (`plan-ios-compagnon.md`).
