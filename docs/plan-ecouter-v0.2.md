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

### 1. Enchaînement sans blanc — **mesuré et corrigé (4 oct. 2026)**

**Méthode.** `scripts/gapless-fixtures.sh` coupe un balayage de fréquence à
l'échantillon près et encode chaque moitié dans chaque format ;
`crates/player/examples/verif_gapless.rs` ouvre les deux moitiés par `ouvrir`
(le chemin exact de la lecture, rééchantillonnage vers 48 kHz compris) et
mesure le blanc ou le saut au raccord. Seuil retenu : 5 ms.

**Avant correction** : WAV, FLAC, MP3 à en-tête LAME et Vorbis à 0,00 ms ;
**M4A +41,8 ms** (symphonia 0.5.5 lit la liste d'édition sans l'appliquer) ;
**Opus +13,5 ms** (notre décodeur ignorait la granule de fin).
**Après** : tous les formats à 0,00 ms, aux deux fréquences.

**Corrigé** : `crates/core/src/opus.rs` (troncature à la granule de fin) et
`crates/core/src/gapless.rs` (lecture d'`iTunSMPB` puis de la liste d'édition
`elst`, appliquée par `ouvrir` avant le rééchantillonnage). Sur la
bibliothèque réelle, 280 M4A sur 300 échantillonnés portent l'information
(retard iTunes, 2 112 trames).

**Limite assumée — MP3 sans en-tête Xing/Info.** Environ **la moitié des MP3**
de la bibliothèque n'en ont pas (1 516 sur 3 000 échantillonnés) : le fichier
ne dit pas son retard, aucun décodeur ne peut le rogner sans heuristique. Le
décodage laisse alors ~25 ms de retard d'encodeur en tête. Sur 300 vraies
pistes, le silence propre aux morceaux dépasse largement ce retard (médiane
130 à 240 ms en tête, 1 à 2 s en queue) : il ne s'entend que sur les pistes
qui s'enchaînent sans silence. **Décision : ne rien faire**, toute correction
serait une heuristique qui risque de couper de la musique. À revoir si
l'écoute d'albums continus (live, mix) le justifie — voir la validation ci-dessous.

**Validation sur trois albums réels** (blanc mesuré à chaque raccord, seuil
−60 dB) : Soul Coughing *Live 2024* — 17 raccords sur 20 à 0 ms, les trois
autres sont de vrais silences (3 s, 0,5 s, 0,2 s) ; Korn *Issues* — 15 sur 15
à 11 ms ou moins ; Sons of Kemet *Black to the Future* — 0 sur 10 sous 50 ms,
mais ce sont des silences voulus (fins de morceaux étirées, médiane 3,3 s) que
le lecteur doit respecter. Ces trois albums portent tous l'en-tête Xing/Info :
**ils ne testent pas la limite des MP3 sans tag**, qui reste non observée sur
un album continu.

**Décisions** : pas de fondu enchaîné en 0.2.
`examples/silences_bords.rs` chiffre le silence en tête et en queue d'une
liste de pistes.

### 2. Piste illisible ou disparue — **fait (4 oct. 2026)**

**Constaté avant.** Au préchargement, une piste qui ne s'ouvrait pas était
sautée (`a_precharger` avance avant de tenter) mais **sans que personne le
sache**. Pire : si c'était la piste de **départ** (clic, suivant, précédent),
`charger` renvoyait l'erreur sans rien lire et le préchargement remplissait
ensuite une sortie restée en pause — lecture muette.

**Décisions** : passer à la suivante et prévenir ; pas de mémoire en base
(message transitoire seulement) ; test Rust + contrôle manuel du message.

**Fait.**
- `Player::charger` essaie les pistes suivantes jusqu'à en trouver une
  lisible, chacune une fois au plus (la répétition ne peut pas faire boucler) ;
  si toutes échouent, l'erreur remonte et rien ne démarre.
- `Player` retient les pistes sautées (`Echec`, 32 au plus) ;
  `signaler_echec` pour le préchargement du desktop, `prendre_echecs` pour les
  relever une seule fois.
- `playback_state` expose `ignorees` ; l'interface écrit dans la ligne
  `#np-mesures` du transport, en rouge, 8 s : « Illisible : *titre* —
  suivante », ou « N pistes illisibles ignorées ». Pas de bandeau ni de
  toast, conformément à `interface-guidelines.md`.
- Test `une_piste_illisible_est_sautee_et_signalee` (ouvre la sortie audio :
  `--ignored`, comme `ouvre_la_sortie_par_defaut`) : fichier disparu +
  fichier corrompu en tête de file, saut par `jump_to`, file entièrement
  illisible, avec et sans répétition. Test pur de la borne de 32.
- `scripts/audit-interface.sh` : 180 cellules, 0 violation.

**Contrôle visuel** (harnais navigateur, voir « Méthode de test de
l'interface » plus bas) : le message s'affiche dans le transport, **en rouge**
(le premier jet était gris : `.transport__now span` l'emportait en
spécificité), puis s'efface au bout de 8 s. **Reste à faire à la main** dans
l'application elle-même : renommer hors de l'application le fichier de la 4ᵉ
piste d'un album en lecture, avancer jusqu'à elle — la lecture doit continuer
sur la 5ᵉ.

**Limite connue** : les échecs sont relevés par le sondage de l'interface ; en
mode Éditer (stems), `battement` ne sonde pas `playback_state` et rien n'est
affiché.

## Priorité 2 — ce qui manque à un lecteur du quotidien

### 3. Reprise de session — **fait (4 oct. 2026)**

**Décisions** : table SQLite `session` ; au lancement, la file revient **en
pause** (jamais de son surprise) ; on restaure aussi le volume, l'aléatoire et
la répétition. Pas l'écran ni la vue ouverts : hors périmètre.

**Fait.**
- `crates/core/src/session.rs` + table `session` (une ligne, `schema.sql`) :
  la file en **identifiants** de pistes, `avant_melange` (l'ordre à rendre à
  la désactivation de l'aléatoire), rang, position, réglages. La file n'est
  réécrite que lorsqu'elle change ; sinon seule la position l'est.
- `Player::instantane` / `Player::restaurer` : la piste visée est chargée
  **sans démarrer le son** (`charger_depuis(…, false)`) — pas de `charger`
  puis `pause`, qui laisserait passer un instant de son. Une session qui
  arrive après que l'utilisateur a lancé autre chose s'efface.
- Desktop : un fil natif reprend la session au lancement puis la réécrit
  toutes les 5 s (comme le préchargement, côté natif : une fenêtre masquée
  ralentit les temporisateurs de la webview) ; dernière écriture à
  `RunEvent::Exit`. Une piste disparue de la bibliothèque est omise et le
  rang recalé (`retenir`) ; si c'était la piste en cours, la position repart de 0.
- Interface : la page n'a jamais vu la file restaurée, elle la demande au
  moteur (`file_courante`) la première fois qu'une piste qu'elle ne connaît pas
  joue, et cale le curseur de volume sur celui du lecteur.
- Tests : 8 (cœur : aller-retour, mise à jour de position, une seule ligne,
  traductions chemins ↔ ids, longue file par paquets), 2 de retenue (desktop),
  1 avec sortie audio (`restaurer_reprend_en_pause_a_la_bonne_position`).
  Vérifié de bout en bout sur l'application construite, sur une copie de la
  base : session semée → reprise → réécrite, id inexistant omis.

**Défaut trouvé au contrôle de l'interface, corrigé.** L'interface ne sonde le
lecteur que quand il joue (pour ne pas consommer du processeur au repos) : une
session reprise **en pause** n'était donc jamais affichée — transport sur
« Rien en lecture », volume à 100. Corrigé par un battement au démarrage
(`setTimeout(() => sonder(true), 0)`, il se coupe seul quand rien ne joue) et un
évènement `session-restauree` émis par le moteur si la reprise finit après le
chargement de la page. Vérifié dans le harnais : titre, position (01:01),
volume (40), aléatoire, répétition et file reprise, sondage arrêté ensuite.

**Non vérifié** : l'écriture à la fermeture (`RunEvent::Exit`), qui ne se
déclenche pas sur un arrêt par signal ; la sauvegarde de 5 s est le filet.

### 4. Playlists enregistrées — **fait, sauf l'export M3U8 (4 oct. 2026)**

**Décisions** : tables SQLite ; export M3U8 « ensuite » ; une playlist de Lama
garde ses morceaux et le **texte de la demande** (pas de spec rejouable) ; vue
« Playlists » à côté d'Artistes/Albums, gestion simple (créer, jouer, ouvrir,
renommer, supprimer — pas d'édition piste par piste en 0.2).

**Écart assumé avec la session** : le contenu est une liste de **chemins** et
non d'identifiants. L'identité d'un morceau est son chemin ; un fichier retiré
puis réinséré au même endroit (disque démonté, rescan) reçoit un nouvel
identifiant — avec des ids, la playlist le perdrait pour de bon, avec des
chemins elle le retrouve. Les chemins servent aussi tels quels à l'export M3U8.
Une piste absente reste dans la liste, comptée « introuvable ».

**Fait.**
- `crates/core/src/playlists.rs` + tables `playlist` et `playlist_piste`
  (`schema.sql`) : création, liste (récentes d'abord), pistes dans l'ordre,
  renommage, suppression ; nom nettoyé, borné à 120 caractères, jamais vide.
  5 tests.
- Commandes `playlists`, `creer_playlist`, `playlist_pistes`,
  `renommer_playlist`, `supprimer_playlist`.
- Interface : segment « Playlists » dans « Parcourir » ; bouton « enregistrer »
  et champ de nom dans le panneau de file (pas de fenêtre modale) ; liste avec
  ▶ (lire), clic (ouvrir comme un album, « ← Playlists »), ✎ (renommer sur
  place), ✕ (suppression en deux temps, sans boîte de dialogue) ; message
  d'accueil quand il n'y en a aucune.
- Origine : les cinq générateurs passent par `composerAlchimie({ origine })` —
  la demande faite à Lama, « Dans l'esprit de … », « Radio à partir de … »,
  « Playlist de … ». Elle propose le nom et se garde avec la playlist, mais
  seulement si la file n'a pas changé depuis (même longueur, même premier
  morceau).

**Reste** : l'export/import M3U8 (second temps décidé), et tout ce que la
décision a écarté — éditer une playlist enregistrée, la regénérer.

**Méthode de test de l'interface.** La fenêtre Tauri ne se capture pas, et
`node --check` ne voit pas les collisions de noms (JS accepte une fonction
redéclarée). Un harnais jetable — `ui/` servi par `http.server`, un
`mock.js` qui remplace `window.__TAURI__` par un backend en mémoire — a permis
de dérouler les gestes dans Chrome. Il a trouvé **quatre défauts** que les tests
Rust ne pouvaient pas voir : `dureeLongue` déjà définie plus bas (la mienne
était ignorée : « 222,2 h »), un faux surlignage « sélectionné » des lignes de
playlist (`undefined === undefined`), la reprise de session invisible, le
message rouge devenu gris. Le harnais n'est pas versionné ; si l'on veut le
garder, c'est un candidat pour `scripts/`.

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
