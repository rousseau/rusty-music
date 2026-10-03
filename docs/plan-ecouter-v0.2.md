# Mode Écouter — ce qu'il reste pour la v0.2

Établi après la v0.1.0 (3 oct. 2026), à partir de `ui-spec-lecteur.md`, du code
du lecteur (`crates/player`, `apps/desktop`) et du chapitre `livre/ecouter.qmd`.
Chaque chantier donne l'état **constaté**, des **options** comparées, et une
recommandation. Rien ici n'est engagé : c'est une liste à trancher.

Ce qui est livré et ne figure donc pas ci-dessous : transport complet
(précédent/suivant, seek par clic sur le spectrogramme, volume), file
(aléatoire, répétition, glisser-déposer), vues Albums / Artistes / univers de
l'artiste, pochettes avec repli réseau, bios, critiques, crédits, popularité,
touches média clavier, normalisation « N », enchaînement des pistes par
préchargement.

Légende d'effort : **S** < 1 jour · **M** 1 à 3 jours · **L** > 3 jours.

## Priorité 1 — fiabilité (à faire avant tout le reste)

### 1. Vérifier que l'enchaînement est réellement sans blanc — S puis M

**Constaté.** Le préchargement empile les pistes dans la sortie `rodio`
(`Player::a_precharger`, `PRECHARGE = 2`), donc il n'y a pas de trou
**d'ouverture de fichier**. Mais rien dans le code ne traite le **retard de
l'encodeur** (MP3, AAC : quelques dizaines de ms de silence en tête et en
queue) : le décodeur est appelé avec `Decoder::try_from`, sans option, et
aucun test ne mesure le raccord. Sur un album live ou électronique mixé, un
« clic » ou un blanc de 25-50 ms s'entend. **Non mesuré** : c'est le premier
travail.

| Option | Principe | Coût | Réserve |
|---|---|---|---|
| A. Banc de mesure d'abord | Un sinus continu coupé en deux MP3/AAC/FLAC, joués à la suite, enregistré en boucle logicielle ; on mesure l'écart de phase au raccord | S | Ne corrige rien, mais dit si les options B/C sont utiles |
| B. Réglage gapless du décodeur | Activer l'option de `rodio`/symphonia qui rogne le retard déclaré (tag LAME, `iTunSMPB`) | S si l'option existe dans la version épinglée | À vérifier dans rodio 0.22 |
| C. Rognage maison | Lire le retard dans les tags (`lofty`) et sauter les échantillons en tête/queue avant d'empiler | M | Réécrit une brique que B fournit peut-être |
| D. Fondu enchaîné court (20-50 ms) | Masque le clic sans le supprimer | M | Mauvais pour le live et le classique : coupe l'attaque |

**Recommandation : A, puis B si A trouve un défaut, C en dernier recours.**
D seulement comme réglage optionnel.

### 2. Piste illisible ou disparue : comportement à confirmer — S

**Constaté.** La spec exige « message clair, passage à la piste suivante »
(10 fichiers illisibles au moment de l'écriture, depuis réduits à 2 ;
fichier disparu pendant qu'il est dans la file). Je n'ai pas retrouvé de test
de ce chemin.

| Option | Principe | Coût |
|---|---|---|
| A. Test + message transitoire | Un test de `Player` (fichier supprimé en file) et un bandeau « Illisible : *titre* — suivante » | S |
| B. A + marquage en base | Mémorise l'échec (`illisible`), grise la piste dans les listes, la saute en aléatoire | M |

**Recommandation : A.** B n'a de sens que si les cas se multiplient (disque
réseau déconnecté).

## Priorité 2 — ce qui manque à un lecteur du quotidien

### 3. Reprise de session — M

**Constaté.** La file et la position ne survivent pas à la fermeture
(seuls les réglages E/N/Lama sont dans `localStorage`).

| Option | Principe | Réserve |
|---|---|---|
| A. `localStorage` | La liste d'identifiants + rang + position, écrits par l'interface | Rapide ; fragile si la bibliothèque change (ids obsolètes), et propre à la webview |
| B. Table SQLite `session` | Ids de pistes, rang, position, aléa/répétition ; nettoyée si un id disparaît | Un peu plus de code, mais robuste et testable en Rust |
| C. Bouton « Reprendre » | Ne restaure rien seul : propose de relancer à la fermeture | Moins surprenant (pas de lecture au démarrage), un clic de plus |

**Recommandation : B, avec la restauration en pause** (jamais de son au
lancement), et C si on préfère ne rien démarrer sans geste.

### 4. Playlists enregistrées — M à L

**Constaté.** Les playlists (artiste, Lama, voisins) se jouent mais ne se
sauvegardent pas : aucune table de playlists dans `db.rs`. La file elle-même
ne s'enregistre pas.

| Option | Principe | Coût |
|---|---|---|
| A. Tables `playlist` / `playlist_piste` | « Enregistrer la file », renommer, rouvrir, supprimer ; liste dans le rail ou la Bibliothèque | M |
| B. Export / import M3U8 seul | Fichiers dans le dossier de musique ; interop Plex et autres lecteurs, pas d'UI de gestion | S |
| C. A + B | Les deux : base pour l'usage, M3U8 pour l'échange | M-L |

**Recommandation : A d'abord, B ensuite** (l'export M3U8 est peu coûteux une
fois A là). Point à trancher : une playlist générée par Lama garde-t-elle sa
**spec** (le texte et les parties) ou seulement ses morceaux ? Garder la spec
permet de la « rejouer » avec une bibliothèque qui a grandi.

### 5. Mémoire d'écoute : favoris et historique — M

**Constaté.** Aucun compteur de lecture, favori ou note n'existe en base.
Lama et « Sonne comme » ne peuvent donc pas s'appuyer sur ce que l'on écoute
vraiment.

| Option | Principe | Réserve |
|---|---|---|
| A. Historique local seul | `dernier_lu`, `nb_lectures` incrémentés quand une piste passe un seuil (30 s ou 50 %) ; vues « Récents », « Les plus écoutés » | Pas de geste utilisateur, mais déjà utile |
| B. Favori ♥ | Un bit par morceau, filtre dans les listes | Geste explicite, simple |
| C. A + B | | |
| D. Soumission ListenBrainz | Envoyer les écoutes à un compte (le client ListenBrainz existe déjà, en lecture de popularité) | Sort de la logique « tout local » : opt-in, jeton à gérer |

**Recommandation : C**, D en option activable plus tard. Attention à la
vie privée : l'historique reste dans la base locale, jamais envoyé sans
réglage explicite.

### 6. Intégration système macOS (« En cours de lecture ») — M

**Constaté.** Les touches média passent par des **raccourcis globaux** (
`main.rs`, au prix de l'autorisation « Surveillance des saisies » demandée au
premier appui, et d'un échec silencieux si elle est refusée). Pas de
widget « En cours de lecture », pas de pilotage depuis l'écran verrouillé ou
les AirPods.

| Option | Principe | Réserve |
|---|---|---|
| A. Crate `souvlaki` | API unique : MPNowPlayingInfoCenter + commandes à distance (macOS), MPRIS (Linux), SMTC (Windows) | Licence à passer à `cargo deny` ; boucle d'évènements à brancher sur Tauri |
| B. Liaisons Objective-C directes (`objc2-media-player`) | Même résultat, sans intermédiaire | Plus de code, macOS seulement |
| C. Statu quo | Garder les raccourcis globaux | Pas de pochette/titre dans le centre de contrôle |

**Recommandation : A.** Bénéfice collatéral : supprime la dépendance à la
permission « Surveillance des saisies ».

## Priorité 3 — confort

### 7. Durée totale et temps restant de la file — S

Listé dans la spec (§ « Ce que l'interface demandera au moteur », point 5),
non affiché. Une ligne « 14 pistes · 1 h 02 » sous la file, calculée côté
interface à partir des `duration_ms`. Aucune alternative utile ;
**à faire**.

### 8. Minuteur d'arrêt — S

| Option | Principe |
|---|---|
| A. Durée fixe | 15 / 30 / 60 min, fondu de 10 s puis pause |
| B. Fin de piste ou d'album | S'arrête à la fin de ce qui joue |
| C. A + B | Un petit menu |

**Recommandation : C**, dans le menu du transport. Pas de nouveau moteur :
la pause existe, il faut un fondu de volume.

### 9. Paroles — M

**Constaté.** Rien dans le code.

| Option | Principe | Réserve |
|---|---|---|
| A. Local seul | Tag `USLT` (non synchronisé) ou fichier `.lrc` voisin (synchronisé), lus par `lofty` ; affichés dans l'inspecteur | Couverture dépend de la bibliothèque : à mesurer avant de se lancer |
| B. LRCLIB (API libre) | Paroles synchronisées par titre/artiste/durée | Réseau, appariement flou (le projet évite par principe le flou par nom), droits d'auteur des textes |
| C. Ne pas faire | | |

**Recommandation : mesurer la couverture locale (S), et ne faire A que si
elle dépasse quelques pourcents.** B est à écarter tant que le projet n'a
pas tranché la question des droits.

### 10. Égaliseur — M

Aucune trace. La normalisation « N » règle le niveau, pas le timbre.

| Option | Principe | Réserve |
|---|---|---|
| A. 5 à 10 bandes (filtres biquad dans `player`) | Curseurs + préréglages | Ajoute un étage au chemin audio, à placer avant le clamp final |
| B. Préampli + « Graves / Aigus » | Deux curseurs | Le plus petit pas utile |
| C. Ne pas faire | Le système (macOS) a ses propres réglages | |

**Recommandation : C pour la 0.2**, B si quelqu'un le demande. Ce n'est pas
ce qui distingue l'app.

## Priorité 4 — décisions de périmètre, pas de développement

### 11. Les boutons « E » et « HD » — décision, S

**Constaté.** Le chapitre `livre/ecouter.qmd` les range dans « Essais
abandonnés (mais disponibles) : le résultat n'a pas été concluant », alors
que `CLAUDE.md`, le README et `suite.md` disent « livré » et que les deux
boutons sont toujours dans le transport (`index.html`).

| Option | Principe |
|---|---|
| A. Garder, déplacer sous « Expérimental » | Réglages > Expérimental, retirés du transport |
| B. Retirer de l'interface, garder le code | Les crates `superres` et `amelioration` restent, désactivés à la compilation |
| C. Retirer tout | Gain de maintenance : AERO + `ort` sont une dépendance lourde |
| D. Statu quo | |

**Recommandation : A**, avec les docs alignées. Le transport est la zone la
plus chargée de l'écran ; deux boutons qu'on a jugés non concluants n'y ont
plus leur place. C si le poids de `ort`/AERO pèse sur l'installeur.

### 12. Vérification visuelle des cas limites — S

États limites listés dans la spec : artiste vide (« (sans artiste) » existe
dans `app.js`), titres très longs, écritures CJK (la police de secours est
celle du système : probablement bonne sur macOS, **non vérifiée**). À passer
en revue avec la bibliothèque réelle, puis lancer
`scripts/audit-interface.sh`.

## Ce qui est écarté pour la 0.2

- `tracks_of_artist()` : aucun défaut constaté, voir `ui-spec-lecteur.md`.
- Unifier le dessin de l'onde (transport / inspecteur / stems) : sans
  demande, coût réel.
- Crossfade par défaut : voir 1.D.

## Ordre proposé

1. **1** (mesure du gapless), **2**, **7**, **11** — peu coûteux, ils règlent
   un doute ou une incohérence.
2. **3** (reprise de session), **6** (En cours de lecture) — ce qu'on
   remarque dès la première semaine d'usage.
3. **4**, **5** — le gros morceau, à concevoir ensemble : playlists et
   historique partagent la même question de ce qu'on mémorise.
4. **8**, **9**, **10** si le temps le permet.
