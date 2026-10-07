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

### 4. Playlists enregistrées — **fait (4 oct. 2026)**

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

**Export M3U8 (fait, import écarté).** Un bouton ⇩ par playlist ouvre le
sélecteur de dossier déjà utilisé par l'export des stems (aucune permission
d'écriture ajoutée) et écrit `<nom>.m3u8` : `#EXTM3U`, une ligne `#EXTINF`
(durée arrondie, « artiste - titre ») et le chemin absolu par piste, UTF-8. Un
titre ne peut pas ouvrir une ligne parasite (retours à la ligne remplacés), le
nom de fichier est nettoyé, et **un fichier existant n'est jamais écrasé**
(« (2) », « (3) »…). Les pistes introuvables n'y figurent pas. L'import n'est
pas fait : les chemins d'un fichier venu d'une autre machine ne correspondent
pas à ceux de la bibliothèque. 4 tests.

**Reste** : tout ce que la décision a écarté — éditer une playlist
enregistrée, la regénérer, l'import M3U8.

**Méthode de test de l'interface.** La fenêtre Tauri ne se capture pas, et
`node --check` ne voit pas les collisions de noms (JS accepte une fonction
redéclarée). Un harnais jetable — `ui/` servi par `http.server`, un
`mock.js` qui remplace `window.__TAURI__` par un backend en mémoire — permet de
dérouler les gestes dans Chrome (par l'extension, ou par Playwright et le Chrome
installé quand l'extension n'est pas connectée). Il a trouvé **quatre défauts**
que les tests Rust ne pouvaient pas voir : `dureeLongue` déjà définie plus bas (la mienne
était ignorée : « 222,2 h »), un faux surlignage « sélectionné » des lignes de
playlist (`undefined === undefined`), la reprise de session invisible, le
message rouge devenu gris. Le harnais n'est pas versionné ; si l'on veut le
garder, c'est un candidat pour `scripts/`.

### 5. Mémoire d'écoute : historique et favoris — **fait (4 oct. 2026)**

**Décisions** : historique **et** favori ♥ ; une écoute compte après 30 s ou la
moitié de la piste, le premier seuil atteint ; trois listes calculées en tête
de la vue Playlists ; **tout reste local** — la soumission à ListenBrainz est
écartée de la 0.2.

**Fait.**
- Tables `ecoute` (un journal : une ligne par écoute) et `favori`, par
  **chemin** comme les playlists — un fichier réinséré au même endroit retrouve
  son historique. `crates/core/src/memoire.rs`, 6 tests.
- Comptage côté moteur, pas côté page (une fenêtre masquée ralentit les
  temporisateurs de la webview) : le fil de 500 ms qui précharge compte aussi le
  **temps réellement joué** (`SuiviEcoute::avancer`). Pause exclue ; **sauter à
  la fin d'une piste ne la fait pas compter** ; une piste qui recommence
  (répétition « une ») compte à chaque tour ; un battement très en retard
  (ordinateur endormi) est plafonné à 2 s. 6 tests.
- Listes calculées, seulement celles qui ne sont pas vides : « Récemment
  écoutés » (100, une fois chacun), « Les plus écoutés » (au moins deux
  écoutes, sinon ce ne serait que « récemment »), « Favoris ». Elles se lisent
  et s'ouvrent comme des playlists, sans renommer ni supprimer ; l'en-tête de la
  vue compte désormais des « listes ».
- ♥ sur la pochette de l'inspecteur (il suit la lecture, donc il vise le morceau
  qui joue) ; un ♥ discret marque les lignes de pistes favorites.

**Écarts et limites** : le ♥ n'est pas dans le transport ni cliquable sur les
lignes — l'inspecteur seul le bascule. L'historique n'alimente pas encore Lama
(c'était l'intérêt du journal, `suite.md`/point 5 du plan). Aucune purge ni
bouton « effacer l'historique » : à ajouter si l'on veut garder la main sur ce
qui est retenu.

### 6. Intégration système macOS (« En cours de lecture ») — **fait et éprouvé à la main (4 oct. 2026, contrôle le 7 oct.)**

**Décisions** : crate `souvlaki` ; infos du morceau **et** commandes ; les
raccourcis globaux sont retirés quand l'intégration démarre, gardés en repli.

**Constaté avant.** Les touches média passaient par des raccourcis globaux
(`global-hotkey`), au prix de l'autorisation « Surveillance des saisies » et
d'un échec silencieux si elle était refusée ; rien dans le centre de contrôle.

**Fait.** `apps/desktop/src/systeme_media.rs`.
- Annonce au système : titre, artiste, album, durée, position, pochette,
  état lecture/pause/arrêt. **La décision est pure** (`Annonce::decider`) et
  testée sur toutes les plateformes : fiche seulement pour un nouveau morceau,
  position ré-annoncée chaque seconde en lecture (pas plus), une seule fois en
  pause, tout de suite après un saut dans la piste, arrêt annoncé une fois.
  `souvlaki` pose le temps écoulé mais **pas la vitesse de lecture** : sans ce
  rafraîchissement, macOS ne ferait pas avancer la barre.
- Commandes du système : lecture, pause, bascule, suivant, précédent
  repassent par l'évènement `touche-media` des anciens raccourcis (garde
  anti-double-appui de l'interface comprise) ; lecture et pause, distinctes pour
  le système (AirPods), ne déclenchent la bascule que si elle changerait
  quelque chose. Déplacement dans la piste (barre du centre de contrôle, avance/recul)
  appliqué directement au lecteur.
- Pochette : la pochette locale du morceau est écrite sous `en-cours/` (les deux
  fichiers les plus récents seulement) et passée en URL `file://`
  **percent-encodée** — le dossier de données contient une espace.
- Appels à `souvlaki` sur le fil principal (`run_on_main_thread`).
- `souvlaki` 0.8.3, MIT, **macOS seulement** (dépendance conditionnelle) ;
  `cargo deny check` : licences, interdictions, sources et avis de sécurité OK.
  7 tests.

**Vérifié** : l'application construite démarre sans erreur sur une copie de la
base ; la pochette de la piste reprise est écrite ; l'ancien avertissement des
touches média a disparu (les raccourcis globaux ne sont plus enregistrés).

**Contrôle à la main (7 oct. 2026) : conforme.** macOS désigne comme « application en
cours de lecture » la dernière qui a joué du son ; la liste de contrôle (centre de
contrôle : titre, artiste, pochette, barre de progression ; touches ▶⏸ ⏭ ⏮ ; AirPods ;
déplacement de la barre ; écran verrouillé) a été déroulée par l'utilisateur, qui
valide le comportement.

**Changement de comportement à connaître.** Avant, les touches média étaient
captées par l'application en permanence, même sans rien jouer. Maintenant elles
vont à la dernière application qui a joué du son : tant que Rusty Music n'a rien
joué dans la session, elles peuvent piloter autre chose (Musique, un navigateur).
**Issue** : `RUSTY_MUSIC_TOUCHES_GLOBALES=1` rétablit les raccourcis globaux.
Le repli « si le système refuse » ne se déclenche pas sur macOS : `souvlaki` y
rend toujours `Ok`, le système n'a pas de refus à signaler.

## Priorité 3 — confort

### 7. Durée totale et temps restant de la file — **fait (4 oct. 2026)**

**Décisions** : total **et** temps restant, dans l'en-tête du panneau de file
seulement (pas dans le transport, déjà dense).

**Fait.** Une ligne sous l'en-tête de la file : « 15 min · reste 10 min »
(`texteDureeFile`, pure). Le reste est celui de la piste en cours (position
comprise) plus les suivantes ; il se met à jour à chaque battement sans toucher
au DOM tant que le texte ne change pas, et rien n'est calculé panneau fermé.
Une piste sans durée n'est pas comptée et le total est alors précédé de « ≈ » ;
**pas de « reste » en répétition** (la file reboucle, il n'y a pas de fin) ;
rien en lecture : le total seul. Formats : « 35 s », « 41 min », « 1 h 02 ».
Vérifié sur huit cas limites et sur le rendu dans le harnais.

**Audit d'interface non concluant ce jour-là.** `scripts/audit-interface.sh`
s'est figé à 27-32 cellules sur 180, sans erreur et sans activité du processus,
**y compris sur `0a33ff6`, dont l'audit passait quelques heures plus tôt** : le
blocage ne vient donc pas des changements des points 6 et 7. Écartés : réseau
(Deezer, CAA, MusicBrainz répondent en < 0,3 s), charge de la machine, session
verrouillée ou en veille, mémoire, disque. Non écarté : la webview se suspend
quand sa fenêtre n'est pas visible (l'audit pilote une vraie fenêtre) ; amener
la fenêtre au premier plan par `osascript` n'a rien changé, mais n'est pas
prouvé efficace. À relancer par l'utilisateur, fenêtre au premier plan.

### 8. Minuteur d'arrêt — **fait (4 oct. 2026)**

**Décisions** : trois modes — « dans 15/30/60 min », « fin du morceau », « fin de
l'album » ; pour une durée fixe, le volume descend pendant 10 s puis pause à la
position atteinte, volume remis ; bouton dans le panneau de file, à côté
d'« enregistrer ». Compté en temps réel, pause comprise (comme un minuteur de
chevet) ; jamais conservé d'un lancement à l'autre.

**Fait.**
- `apps/desktop/src/minuteur.rs` : une machine d'états **pure**
  (`Minuteur::avancer`), qui reçoit une photo du lecteur et rend une action ;
  18 tests purs. Elle vit **côté moteur**, dans un fil à battement adaptatif
  (500 ms au repos, 100 ms armé, 20 ms dans les 2 dernières secondes d'un
  morceau) : une webview masquée ralentit ses temporisateurs, et un minuteur de
  coucher est précisément ce cas-là.
- **Fondu** : volume × (reste/10 s)², le carré parce qu'une descente linéaire
  paraît tomber d'un coup à la fin ; borné à la durée du minuteur s'il est plus
  court que 10 s ; le volume ne remonte jamais. Pause d'abord, volume remis
  ensuite (l'inverse ferait entendre la reprise). **Le volume d'avant est
  retenu pour la session** : sans cela, un arrêt de l'application en plein fondu
  l'aurait enregistré presque éteint.
- **Visée de la fin d'un morceau.** Attendre le changement de morceau laissait
  entendre **35 ms du suivant** (mesuré, stable) : le lecteur audio met quelques
  dizaines de ms à appliquer une pause, et c'est un « tic » sur une attaque
  franche. On met donc en pause à **moins de 40 ms de la fin**, sur le même
  morceau (mesuré : pause à 1,49 s d'un morceau de 1,5 s, **rien du suivant
  n'est entendu**) ; reprendre rejoue ces quelques ms puis enchaîne.
  Pour cela le lecteur expose la **durée réellement décodée**
  (`Player::duree_courante`, de pair avec `charges`), pas celle des tags — qui
  peut être fausse (retard d'encodeur, MP3 sans en-tête). Sans durée connue,
  le changement de morceau reste le filet (35 ms, veille à 20 ms).
- « Fin de l'album » : la suite ininterrompue de pistes du même album dans la
  file, morceau en cours en tête. Si l'on quitte l'album **avant** son dernier
  morceau (autre lecture lancée), le minuteur s'efface sans rien arrêter ; sans
  album connu, il retombe sur « fin du morceau ».
- Avec « répéter un morceau », « fin du morceau » s'arrête à la fin du tour en
  cours ; une file épuisée d'elle-même efface le minuteur.
- Interface : bouton « minuteur » (allumé quand armé), menu en ligne, état
  « Arrêt dans 24 min » / « Arrêt à la fin du morceau (1 min) », « annuler » ;
  le moteur refuse en clair quand rien ne joue. 4 boutons tiennent sur une
  rangée du panneau (mesuré : 193 px de texte pour 263 px utiles).
- Tests avec sortie audio : pause juste avant la fin du morceau, filet sans
  durée, fondu complet (volume jamais remonté, fin < 0,05, volume remis).

**Limites.** Quitter un morceau par « suivant » ou « précédent » pendant
« fin du morceau » déclenche la pause tout de suite (le morceau « est fini »).
Le décompte affiché n'avance que pendant la lecture (le battement de la page
s'arrête en pause) ; il se remet à jour à l'ouverture du panneau. Audit
d'interface non relancé (voir point 7).

### 9. Paroles — **écarté de la 0.2 (4 oct. 2026)**

**Mesuré sur 3 000 MP3 de la bibliothèque** : paroles dans les tags (`USLT`
non vide) pour **22 morceaux, soit 0,7 %** ; aucun fichier `.lrc` voisin. Une
version purement locale n'afficherait donc des paroles que pour moins de 1
morceau sur 100.

**Décision** : ne pas faire en 0.2. Reste ouverte pour plus tard : LRCLIB
(service libre, sans clé, appariement par titre/artiste/album/durée, paroles
synchronisées), à activer dans les réglages, avec cache — au prix du « tout
local » et d'une zone grise de droits d'auteur.

### 10. Égaliseur — **écarté (4 oct. 2026)**

Décision : pas d'égaliseur, pas besoin. La normalisation « N » règle le niveau ;
le réglage du timbre est laissé au système.

## Priorité 4 — décisions de périmètre, pas de développement

### 11. Les boutons « E » et « HD » — **conservés (4 oct. 2026)**

Décision : on ne les retire pas, ils sont historiques. Ils restent dans le
transport, avec leur code (`crates/superres`, excitateur). Le chapitre
`livre/ecouter.qmd` les range dans « essais abandonnés (mais disponibles) » —
c'est le sens de « conservés » : réalisés, au résultat jugé non concluant, mais
gardés. Aucun changement de code.

### 12. Cas limites visuels — **fait (4 oct. 2026)**

Passés en revue dans le harnais (1200×761 et 960×588), avec : un titre de
220 caractères, de l'écriture japonaise (`芸能山城組`), des diacritiques
(`(həd) p.e.`, `Kanañ a ri!`), titre, artiste, album, année et durée vides,
un nom d'artiste de 160 caractères.

- **Écriture non latine** : rendue correctement (police de repli du système) ;
  la crainte du plan est levée.
- **Champs vides** : « (sans titre) », « (sans artiste) », « — » ; aucune ligne
  ne se déforme.
- **Titres longs** : ellipse dans la liste, le transport et la file.
- **Défaut trouvé et corrigé** : un nom d'artiste très long n'était pas tronqué
  (`.ligne__sec` en `flex: none`) et chassait le titre et la durée hors de la
  ligne. Il se tronque maintenant à 40 % de la largeur ; plus aucun élément ne
  dépasse du viewport.

Audit d'interface non relancé (voir point 7).

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
