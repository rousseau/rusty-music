# Mode Éditer — plan d'implémentation : pratiquer, créer

Établi le 8 oct. 2026 à partir de `docs/recherche-editer-pratique-creation.md`
(état de l'art, licences, sources) et du code du module 3 (`crates/editor`,
`crates/analysis/src/battements.rs`, `apps/desktop`). Même forme que
`plan-ecouter-v0.2.md` : pour chaque chantier, l'état **constaté**, les
**options**, une **recommandation**. Rien n'est engagé : c'est une liste à
trancher.

Deux usages, un seul établi (« une piste à la fois » tient toujours, voir
`ui-spec-editeur.md`, décision 10) :

- **Pratiquer** — couper le stem de son instrument (basse ou batterie), jouer
  avec le reste du groupe, lire sa partie qui défile, boucler un passage,
  ralentir.
- **Créer** — remplacer un stem par celui d'un autre morceau de la
  bibliothèque ou par un stem généré (« une batterie plus funk, comme dans tel
  morceau »), ou en ajouter un, en gardant la structure du morceau d'origine.

Légende d'effort : **S** < 1 jour · **M** 1 à 3 jours · **L** > 3 jours.

**Trois principes, tirés de la recherche :**

1. **La grille d'abord.** Temps, premiers temps de mesure, puis sections : les
   deux usages consomment la même ossature. Rien d'autre n'avance bien sans
   elle.
2. **Une seule horloge**, la position dans l'audio d'origine, tenue par Rust.
   Notes, mesures et sections sont en secondes d'origine ; l'étirement et les
   boucles ne changent que la position rapportée.
3. **Mesurer avant de choisir un modèle.** La licence n'écarte plus les
   meilleurs candidats ; seul un banc sur *nos* stems HTDemucs départage.
4. **Tout en Rust, sur Burn** (décidé le 8 oct. 2026). Pas de Python à
   l'exécution, pas de processus externe : les poids sont convertis
   (safetensors) et les réseaux portés en Burn, comme HTDemucs et CLAP.
   `ort` reste admis là où il est déjà (ONNX livré tel quel, ex. Basic Pitch).
   Python ne sert qu'à **préparer** (conversion des poids) et à **vérifier**
   (parité contre la référence, bancs), comme aujourd'hui dans `scripts/` et
   `experiments/`.

---

## Phase 0 — le socle commun

### 0.1 Modèles tiers : préparation, publication, remerciements — **S** — *en cours (8 oct.)*

**Fait** : `MODELES.md` (CLAP, HTDemucs, AERO, Beat This! — article, dépôt,
licences code et poids, conversion) ; `scripts/preparer-beat-this.sh` (ajouté
à `preparer-tout.sh`) ; `models/README.md`. **Reste** : écran « À propos »,
lecture depuis Hugging Face dans `telecharger-modeles.sh` (rien à republier
pour Beat This! : les ONNX de `beat-this-rs` sont déjà publics, MIT).

**Constaté.** Les modèles passent par `scripts/preparer-*.sh` (conversion
depuis la source) et `scripts/telecharger-modeles.sh` (fichiers déjà préparés,
release assets GitHub `modeles-v1`, empreintes SHA-256). Aucun fichier ne
recense les modèles tiers ni leurs licences ; il n'y a pas d'écran « À
propos ».

**À faire.**
- `MODELES.md` à la racine : pour chaque modèle — article, dépôt, lien des
  poids originaux, licence des poids, licence du code, conversion appliquée,
  où l'on republie. Y reporter d'emblée CLAP, HTDemucs, AERO.
- Un `scripts/preparer-<modele>.sh` par nouveau modèle, sur le modèle des
  existants : télécharge à la source, convertit (safetensors pour Burn, ONNX
  pour `ort`), vérifie la parité contre la référence Python.
- **Republication.** Les poids convertis dont la licence le permet (MIT,
  Apache, CC BY, CC BY-SA, CC BY-NC…) sont publiés sur **Hugging Face**, un
  dépôt par modèle, fiche reprenant licence d'origine, citation et lien —
  la fiche HF porte nativement la licence et la citation, ce que les release
  assets ne font pas. Les poids **sans licence déclarée** (COCOLA, GrooVAE,
  STAGE) ne sont pas republiés : leur script les prend à la source.
  `telecharger-modeles.sh` apprend à lire depuis HF.
- Écran « À propos » (Réglages) : la liste de `MODELES.md`, plus l'attribution
  OSM déjà due. Conditions d'usage particulières affichées là où le modèle
  sert (MuScriptor).

**Tranché le 8 oct.** : compte Hugging Face **`rousseau`**
(https://huggingface.co/rousseau), un dépôt de modèle par poids converti
(`rousseau/<modele>-burn` par exemple). Jeton dans `~/.config`, jamais dans
le dépôt. Les poids d'accès restreint (MuScriptor : formulaire d'acceptation
des conditions) ne sont **pas** republiés — republier contournerait la
condition d'accès ; le script de préparation les télécharge avec le jeton de
l'utilisateur, après acceptation, et les convertit localement.

### 0.2 Grille de mesures — beat_this — **M** — *fait côté moteur (8 oct.)*

**Fait.** `crates/editor/src/pulsation.rs` (et non `crates/analysis` : seul
l'éditeur la consomme, et il a déjà le décodeur stéréo) : `Pisteur` (crate
`beat-this` 1.1, licence MIT vérifiée, runtime `rten`), `Pulsation { temps,
premiers_temps }`, `bpm`, `metrique`, `indice_temps`/`instant`, `position`
(mesure, temps ; anacrouse = −1), `mesure_autour`, `aimanter`, cache JSON
versionné ; poids téléchargés au premier usage (empreintes vérifiées). 6 tests.
Commande Tauri `pulsation(id)`, cache `pulsation.json` à côté des stems.
**Modèle complet retenu** : sur 4 morceaux, le petit se trompait de métrique
une fois sur quatre. **Coût** : environ 4 à 5 s par morceau sur un fil (le
réseau tourne dans le pool de 8 fils de `rten`, partagé par tout le
processus — paralléliser les morceaux n'accélère rien). Banc :
`crates/editor/examples/verif_pulsation.rs` + `experiments/pulsation/`
(`preparer.sh`, `noter.py`, mir_eval, tolérance 70 ms). **Banc GTZAN complet
(998 clips, un fil, 8 oct.) : F1 temps 88,2, premiers temps 77,5, CMLt 79,1,
AMLt 89,2** — l'article annonce 89,1 / 78,3 / 79,8 : le portage Rust et nos
décodage et rééchantillonnage le reproduisent à moins d'un point. 1,7 s par
clip de 30 s. Points faibles : classique (64,7 / 50,4), blues et reggae en
premiers temps (64 / 67). **Reste** : le Viterbi tempo/mesure — pas
justifié par ce banc, à reconsidérer si les boucles tombent mal à l'usage, et brancher `Cale` de la greffe sur les premiers
temps (chantier 2.1).

**Constaté.** `battements.rs` donne une grille **à tempo constant** (`bpm`,
`phase_s`, `nettete`), à ±8 ms, sans premier temps de mesure, avec une
ambiguïté d'un demi-battement sur les batteries (`suite.md` §5 ter). `greffer`
s'en sert via `Cale` (une phase et une période par côté).

**Options.**
- (a) la crate `beat-this` 1.0.0 (rten pur Rust, `ort` en option, parité
  F = 1,0 avec Python) ;
- (b) un export ONNX via `ort`, déjà dépendance de `crates/superres` ;
- (c) import `burn-onnx` pour profiter de Metal comme CLAP — non testé.

**Recommandation.** (a) si son LICENSE confirme MIT (c'est du code lié), sinon
(b). Modèle *small* (~2 M paramètres) d'abord. Nouveau module
`crates/analysis/src/pulsation.rs` :

```rust
pub struct Pulsation {
    pub temps: Vec<f32>,          // secondes d'origine
    pub premiers_temps: Vec<f32>, // downbeats
    pub metrique: u8,             // temps par mesure, déduit
}
```

plus un Viterbi tempo/phase de mesure maison (le DBN de madmom sert de
référence de parité, pas de dépendance), et un `temps_vers_position(t) ->
(mesure, temps, fraction)` par interpolation linéaire par morceaux — c'est la
fonction que partition, boucles et greffe partageront. Calcul **à l'ouverture
d'un morceau dans Éditer**, sur le mélange, mis en cache à côté des stems
(`dossier_stems`) ; pas de passe sur toute la bibliothèque tant que le temps
de calcul n'est pas mesuré. `Grille` reste pour la carte et `tempo_cible`.

**Vérification.** Banc GTZAN existant (Acc1/Acc2 contre l'estimateur actuel) ;
F1 de downbeat sur un échantillon annoté ; temps de calcul d'un morceau de
4 min sur le Mac.

### 0.3 Boucle calée sur les mesures — **S/M** — *fait (8 oct.), à vérifier à l'oreille*

**Fait.** `Multipiste::boucler`/`boucle`/`passages` (`crates/player`) : retour
au début quand la position de référence **franchit** la fin (un déplacement
au-delà n'y ramène pas), têtes réalignées, étireurs vidés, 5 ms de fondu de
part et d'autre. 3 tests (retour, déplacement, raccord sans saut d'amplitude).
Commande `stems_boucle`, `sans_boucle` dans `stems_transport`, `boucle` et
`passages` dans `stems_state`. Interface : réglage « boucler » dans la barre
d'outils (visible une fois la pulsation connue), longueur en mesures (±),
« +2 %/passage » jusqu'à 100 %, bande sur la pile ; la boucle se repose quand
les stems sont rechargés (transposition, greffe). **Reste** : essai dans
l'application (clic de raccord, latence de l'étireur à vitesse réduite),
raccourci clavier, sélection d'une boucle à la souris sur la pile.

**Constaté.** Le transport des stems (`stems_transport`) sait lire, arrêter,
se placer ; il n'a pas de boucle A-B.

**À faire.** Une boucle tenue **côté Rust** (retour à A quand la position
passe B, sans blanc), bornes aimantées aux premiers temps de 0.2. Sélection à
la souris sur la pile de stems ou par « ces 4 mesures ». Option Soundslice
« accélérer à chaque passage » (+2 % de vitesse jusqu'à 100 %) : peu de code,
très utile à l'entraînement. Sert aussi à l'écoute d'une greffe.

### 0.4 Banc de transcription — **M**

**Constaté.** Aucun chiffre publié sur des stems sortis de Demucs. Le projet a
déjà l'habitude des bancs (`banc-tempo-gtzan`, `experiments/`).

**Fait côté basse (10 oct.) : banc contre des partitions publiées**
(`experiments/partitions/`). Les 15 morceaux de *Californication* (livre
« Bass Recorded Versions », notation + tablature) : Audiveris lit la notation
(rythmes) une fois les tablatures effacées, nos propres scripts lisent la
tablature (hauteurs, cordes, frettes), un Viterbi aligne nos mesures sur les
mesures écrites (reprises, codas, figures rejouées). Chaîne de l'éditeur :
F1 0,48 (0,51 sur les mesures adossées à la tablature), doigtés identiques
61 %. Usage local, rien d'extrait des livres n'est versionné.

**Reste à faire.** `experiments/banc-transcription/` :
- données : Slakh2100 (basse), MDB-Drums et ENST (batterie), rapatriées de
  Garage en local (`rclone sync`, jamais streamées) ;
- protocole : remixer, séparer par **notre** HTDemucs, transcrire, comparer à
  la référence — F1 de note avec et sans offset (50 ms), par classe pour la
  batterie, temps de calcul ;
- candidats basse : Basic Pitch, YourMT3+, MuScriptor ; batterie : ADTOF
  (± LarsNet), ADT_STR, YourMT3+ ;
- la référence Python des modèles suffit ici : le banc décide **quel** modèle
  porter, avant d'en porter un.

---

## Phase 1 — pratiquer

### 1.1 Autoroute de notes — **M** — *rétrogradée le 9 oct.*

La partition au centre a été préférée sur maquette (décision 11 de
`ui-spec-editeur.md`) : l'autoroute devient une **vue de repli**, à ne faire
qu'après 1.4, et seulement si la gravure d'une transcription peu sûre se
révèle illisible. Ce qui suit reste valable pour ce cas.

**Recommandation de la recherche** : commencer par une autoroute (couloirs par
corde avec numéros de frette, couloirs par fût), pas par une partition — elle
n'exige aucune quantification et une erreur de transcription y reste une note
mal placée, pas un rythme faux.

**À faire.**
- Canvas dans l'établi, sous ou à la place du spectrogramme du stem coupé.
  Placement à concevoir avec `interface-guidelines.md` (et
  `scripts/audit-interface.sh` après) ; proposition : un bouton « Pratiquer »
  sur la ligne du stem, qui le coupe et ouvre l'autoroute.
- Défilement en **temps** d'origine, barres de mesure sur les premiers temps
  (0.2), interpolation `requestAnimationFrame` entre deux relevés de position,
  latence de sortie retranchée.
- Commande `transcrire(stem) -> Transcription` (travail en fil, avancement
  comme `EtatDemix`) ; `Transcription { notes: Vec<Note>, instrument, modele }`,
  `Note { debut_s, fin_s, hauteur | piece, velocite, corde, frette, confiance }`.
  Cache JSON à côté des stems.
- Confiance affichée (opacité) : cymbales et notes douteuses moins appuyées.

### 1.2 Transcription basse — **M**

**Fait (9 oct.) — première version de bout en bout.** Nouveau crate
`crates/transcription` (15 tests) : `basic_pitch` (ONNX Runtime, fenêtres de
2 s recouvrantes, port de `note_creation` sans pitch bends ; l'« astuce
Melodia » par tas plutôt que par balayages), `monophonie`, `tablature`
(Viterbi : main, changement de corde 0,2, positions au-delà de la 4ᵉ frette
0,1 par frette), `quantification` (doubles croches sur les temps mesurés,
découpe aux premiers temps, liaisons). **Parité avec le code de Spotify sur
le stem de basse de « Love Foolosophy »** : activations à 1,5 × 10⁻⁷, 832/832
notes identiques (`experiments/transcription/parite_basic_pitch.py`). 224 s
de basse : réseau 1,2 s, notes 5 ms. Commande `transcrire(id, instrument)`
(cache `transcription-bass.json`) ; dans l'interface, la tablature transcrite
remplace la grille dès qu'elle arrive (`texTablature`).
**Reste** : contrôle d'octave (alternances si1/si2 visibles — réelles ou
erreurs ? à trancher au banc), triolets, découpage des durées sur les temps
(silences pointés peu lisibles), MuScriptor en mode qualité, corrections
manuelles (1.5).

**Après l'essai du 9 oct.** (curseur irrégulier, notes mal placées, doigtés
incohérents) :
- *curseur* : un point de synchro **par temps** et non plus par mesure
  (alphaTab avançait au tempo moyen dans la mesure puis sautait à la barre),
  et une position **lissée** (avance à la vitesse de lecture, se recale de
  15 % de l'écart par tick, saute seulement au-delà de 250 ms) ;
- *placement* : mesuré sur « Love Foolosophy », les attaques (basse **et**
  batterie) tombent ~50 ms derrière les temps détectés, près d'une
  demi-double croche — la moitié des notes allaient sur la case voisine.
  `quantification::decalage_de_jeu` estime ce décalage par morceau (± une
  demi-double croche, à égalité « derrière le temps ») et le retranche.
  Par ailleurs, `beat-this` place les temps **12 ms en avance** sur les
  annotations GTZAN (53 554 temps, tous genres) : corrigé à la source
  (`pulsation::CORRECTION_S`, cache en version 2) ;
- *doigtés* : modèle de **position de main** (l'index sur une frette, quatre
  frettes sous les doigts, une corde à vide ne déplace pas la main ;
  traverser une corde coûte 0,1, déplacer la main une frette 1). Sur « Love
  Foolosophy », la main reste en 2ᵉ position au lieu de sauter.

**Contre des tablatures de référence (10 oct.)** — Songsterr, « Love
Foolosophy » et « She's A Bad Mama Jama » (`experiments/transcription/`) :
réglage de Basic Pitch (seuil d'attaque 0,6, durée minimale 8 trames),
**correction des harmoniques** (erreurs dominantes : +12, +19, +24 demi-tons ;
une note dont la fondamentale supposée est active à 60 % de sa propre
activation est ramenée dessus) et coûts de doigtés (changement de corde 0,6,
sans préférence pour le bas du manche : les bassistes restent sur une corde).
« Love Foolosophy » : attaques 66 → 72 % (rappel), hauteurs justes 85 → 91 %,
octaves 10 → 4 %, doigtés identiques 47 → 71 %. Limite trouvée : « Black
Crow » (pulsation à 75 BPM pour 120 — ternaire ?), grille fausse.

**Contre 15 partitions publiées (10 oct.)** — `experiments/partitions/`
(banc 0.4). Signalé à l'essai : des notes là où la basse ne joue pas. Cause :
Basic Pitch ignore le niveau et transcrit les fuites d'autres instruments
dans le stem (« Under the Bridge » : 119 notes pendant l'introduction et le
couplet tacet, stem 60 à 70 dB sous son niveau de jeu). **Porte d'énergie**
(`porte.rs`, 30 dB sous le 95ᵉ centile du stem) : 119 → 2 notes ; « Love
Foolosophy » commence enfin à l'entrée de la basse. Durée minimale 6 trames,
**coût d'une corde à vide sous une main haute** (`vide_haut`), accordage
choisi par les notes (standard, drop D, cinq cordes). F1 0,450 → 0,48,
doigtés 51 → 61 %. Limite : le rappel (44 %) — notes répétées rapides,
notes étouffées ; erreurs d'octave 6,5 % des notes bien placées. C'est là
que MuScriptor doit faire mieux.

**Recommandation.** Nouveau crate `crates/transcription` (Burn + `ort`, même
règle de backend que `analysis` et `editor`). Modèle par défaut **Basic Pitch**
(Apache-2.0, ONNX livré, < 17 000 paramètres) via `ort` — rapide, toujours
disponible. **MuScriptor** (Kyutai/Mirelo/IRCAM, 2026 ; décidé le 8 oct. :
on l'essaie) en mode qualité : Transformer décodeur seul, **porté en Burn**
(poids safetensors convertis localement, voir 0.1 ; `muscriptor.cpp` sert de
seconde référence d'implémentation), taille *small* ou *medium* d'abord.
Conditions d'usage affichées au premier emploi. Le banc 0.4 dit lequel des
deux sert par défaut et si le port vaut sa taille.

**Chaîne.**
1. stem basse → 22 050 Hz mono (`rubato`) → Basic Pitch ;
2. port de `note_creation.py` (activations → notes) ;
3. contrainte monophonique (Viterbi sur les activations) ;
4. contrôle d'octave par une F0 indépendante (pYIN) — le défaut n° 1 connu ;
5. **corde/frette** : plus court chemin (déplacement de la main, cordes à
   vide, changement de corde), accordage en paramètre (4/5 cordes, drop D) ;
   une corde imposée par l'utilisateur relance le calcul sous contrainte.

**Vérification.** Le banc 0.4 ; écoute de quelques lignes de basse connues.

### 1.3 Transcription batterie — **M/L**

**Fait (9 oct.) — ADTOF de bout en bout.** `crates/transcription::batterie` :
spectrogramme de madmom réécrit en Rust (mono 16 bits tronqué, trames de 2048
centrées à 100/s, Hann ÷ 32767, 84 bandes triangulaires normalisées,
`log10(1+x)`), réseau ADTOF « Frame_RNN » en ONNX (architecture réécrite pour
l'export, poids chargés et vérifiés, `scripts/preparer-adtof.sh` +
`experiments/batterie/exporter_adtof.py`), choix de pics de madmom
(`NotePeakPickingProcessor`, seuils d'ADTOF par classe). **Parité avec la
référence Python sur 30 s de « Love Foolosophy »** : caractéristiques 2,4e-7,
sorties 7,8e-7, 207/207 coups identiques (`examples/batterie_parite.rs`).
Morceau entier (224 s) : 1,2 s. `quantification::quantifier_coups` (pièces
d'une même double croche réunies, décalage de jeu retranché), commande
`transcrire(id, "drums")`, partition de percussion (`texBatterie`,
articulations GP7). **Reste** : LarsNet (crash/ride, vélocité, ghost notes),
deux voix (pieds en bas, mains en haut), banc MDB-Drums.


**Recommandation.** Vainqueur du banc entre :
- **ADTOF** — CRNN image par image, 5 classes, entraîné sur 359 h de vraie
  musique ; licence CC BY-NC-SA 4.0 **code compris**, donc : poids convertis en
  safetensors (republiables sous BY-NC-SA), **inférence réécrite en Burn** —
  simple pour un CRNN, et c'est ce que la licence exige pour ne pas lier leur
  code ;
- **ADT_STR** — 26 classes, vélocité, poids safetensors CC BY-SA 4.0 ; décodeur
  autorégressif à boucler en Rust.

Ensuite : **LarsNet** (poids CC BY-NC 4.0) pour séparer crash/ride et estimer
la vélocité par fût (méthode Riley & Dixon) ; ghost notes par la vélocité.
Les descripteurs de groove symboliques (densité, syncope, swing) tombent ici
et servent 2.2.

### 1.4 Partition et tablature — alphaTab — **M/L** — *vue principale de la pratique (9 oct.)*

**Prototype (9 oct.) — ce qui est établi.** alphaTab 1.8.4 embarqué
(`apps/desktop/ui/vendor/alphatab/`, MPL-2.0 sans clause d'incompatibilité,
police Bravura OFL, chargé seulement quand on pratique). Sélecteur
Pratiquer / Créer dans le rail ; en Pratiquer, la partition prend le centre et
la pile se réduit à une rangée de niveaux. Faute de transcription, la partition
porte la **grille mesurée** (`texGrille` : une note par temps, `\ts` par
mesure, `\sync (mesure 0 ms)` sur chaque premier temps). Mesuré hors de
l'application (Chrome sans fenêtre, même CSP que la webview) :
- l'alphaTex de la grille passe l'analyseur pour les **998 grilles GTZAN**,
  une mesure par premier temps, sans exception ;
- **132 mesures (6 min) rendues en 60-70 ms** sur le fil principal ; le worker
  d'alphaTab ne démarre pas hors d'un serveur → `useWorkers: false` ;
- syntaxe 1.8 : arguments de métadonnées **entre parenthèses** (`\ts (4 4)`,
  `\sync (0 0 1200)`), pas de point séparateur ; basse en `\clef F4` avec
  `\displaytranspose 12` (mi à vide sur la 1ʳᵉ ligne supplémentaire) ;
- alphaTab émet **des `seekTo` de lui-même** au chargement : on ne suit que
  ceux qui suivent un clic dans la partition (moins de 1,5 s).

**Essai dans l'application (9 oct.)** : le suivi et le clic dans la partition
fonctionnent. Corrigé après l'essai : la vue ne descendait pas avec la
lecture — le défilement d'alphaTab ne suit pas un lecteur externe ; il est
fait par `suivreCurseur` (mesure jouée tirée de notre pulsation, position
verticale de `boundsLookup`, pas de rappel pendant 3 s après un défilement à
la molette). Préférences : basse en **tablature seule** (`\staff {tabs}`),
batterie en **portée de percussion** (`\instrument percussion`,
`\articulation defaults`, `\clef neutral`, noms Guitar Pro « Hi-Hat
(closed) »…) ; l'instrument affiché se choisit dans le rail, sans lien avec
S/M. Corrigé aussi : le « panneau blanc » — alphaTab était rechargé à chaque
retour en Pratiquer (`chargerScript` le charge désormais une fois) et l'aide
remplaçait son contenu (`#partition-aide` est maintenant à côté de
`#partition-rendu`). Vérifié avec un harnais (vraie interface, backend
simulé, Chrome sans fenêtre) : basse, batterie, défilement à 2:30,
allers-retours Pratiquer/Créer. **Reste** : la
boucle vue sur la partition, le comportement à vitesse réduite. Constat sur la
grille : une intro sans pulsation nette donne des mesures irrégulières (1/4,
2/4, 5/4) — la grille dit ce qu'elle a mesuré.

**À faire.**
- Quantificateur Rust : position de temps (0.2) → grille double croche ou
  triolet par temps (règle `(4, 3)` de music21, pénalité de faux triolet),
  découpe aux premiers temps, liaisons ; batterie : articulations GP7, deux
  voix.
- Sortie **alphaTex** avec un `\sync` par mesure pris sur les premiers temps :
  la synchronisation est exacte par construction.
- alphaTab (MPL-2.0, paquet npm embarqué hors ligne avec ses polices SMuFL)
  piloté par `IExternalMediaHandler` : chaque `play`/`seekTo` devient un
  `invoke` vers le transport des stems, la position remonte par
  `updatePosition`.
- **Prototype jetable d'abord** (S) : alphaTab dans la WKWebView de Tauri,
  lecteur externe, boucle, morceau de 6 min — rien de cela n'est vérifié.

Bascule autoroute ↔ partition par l'utilisateur ; l'autoroute reste le
défaut pour une transcription automatique.

**Rendu, d'après les livres publiés (10 oct.).** Silences de plusieurs
mesures regroupés (`\multibarrest`) ; une note suivie d'un silence d'une
double croche le garde (`legato` : la transcription coupe court, une
partition écrit la note jusqu'à la suivante) ; accordage donné par le moteur.
**Proposé, à trancher** : la notation en clé de fa au-dessus de la tablature,
comme dans les livres (`NOTATION_BASSE` dans `app.js` ; la tablature seule
reste le défaut, préférence du 9 oct.). Encore absents des livres : les
accords chiffrés, les sections (Intro, Verse, Chorus — port d'all-in-one,
2.4), les reprises et figures (« Bass Fig. 1 ») qui raccourcissent la
lecture.

### 1.5 Corriger la transcription — **S/M**

Corde imposée, octave corrigée, note supprimée ou ajoutée : écrites dans le
JSON de cache, jamais perdues à la transcription suivante (le modèle ne
réécrit pas une note corrigée). Export MIDI et alphaTex/Guitar Pro pour
emporter sa partie.

---

## Phase 2 — créer

### 2.1 Greffe mesure par mesure — **M**

**Constaté.** `greffer` étire **globalement** (`tempo_replie`, replié à
l'octave), cale l'entrée sur un battement (`Cale`, `decouper_aux_temps`), puis
boucle ou coupe. Le tempo est supposé constant des deux côtés ;
`demi_tons_rendu` est déjà au programme (`ui-spec-editeur.md`).

**À faire.** Avec les premiers temps de 0.2 des deux côtés : la mesure *i* du
greffon est étirée par WSOLA sur la durée de la mesure *j* de la source, avec
fondus aux jonctions — un tempo qui dérive (morceau joué sans clic) ne
désaligne plus rien. `Plan` gagne `mesures_calees` et `demi_tons_rendu`.
Métrique différente (3/4 sous 4/4) : refuser et le dire, plutôt que tordre.

### 2.2 Chercher le bon greffon — **M**

**Constaté.** `voisins_de_stem` rend les voisins CLAP **du morceau entier**
dont le tempo se cale. La bibliothèque n'a pas d'empreintes par stem (il
faudrait démixer 27 000 morceaux).

**À faire.**
1. **pré-filtre** CLAP, éventuellement guidé par du texte via la tour texte
   déjà là (`encodeur_texte.rs`) : « funk drums, syncopated » ;
2. **filtres** : tempo replié, métrique, distance de tonalité pour les stems
   à hauteur ;
3. **démixage à la demande** des 5 à 10 premiers (≈ 30 s chacun à 7,8 × le
   temps réel), en tâche de fond, dans le cache existant ;
4. **classement** : descripteurs de groove (densité, syncope, swing) sur le
   stem batterie des candidats — « plus funk » = plus syncopé à densité
   voisine —, puis COCOLA (2.3) quand il est là.

L'interface montre pourquoi un candidat est classé là (syncope, tempo,
tonalité), comme la greffe dit déjà ce qu'il a fallu lui faire.

### 2.3 COCOLA — **M**

Score de compatibilité appris, asymétrique : stem candidat → reste du mix de
la source, jamais l'inverse. Poids sans licence déclarée : script de
préparation qui télécharge le point de contrôle à la source et l'exporte en
ONNX localement (pas de republication) ; `ort`. Récupérer le point de
contrôle **tôt** (hébergé sur Google Drive) et en garder une copie privée sur
Garage.

### 2.4 Sections — port d'all-in-one — **L**

**Constaté.** Aucune section aujourd'hui. all-in-one (MIT, poids MIT,
~300 K paramètres) consomme les 4 stems Demucs — les nôtres. Il dépend de
noyaux NATTEN (pas d'ONNX) et du DBN de madmom.

**À faire.** Port Burn : attention de voisinage dilatée réécrite par gather +
softmax (dimension 24, faisable), un seul des 8 plis d'abord, parité contre
Python. Poids convertis en safetensors et republiés sur HF (MIT). Sorties :
frontières + labels (intro, couplet, refrain, pont…). SongFormer (plus
précis, mais gros backbones) sert de référence de qualité.

**Repli** si le port coince : matrice d'auto-similarité sur CLAP et chroma
par mesure + noyau de Foote — des lettres A/B/C au lieu de noms.

**Ce que ça débloque.** Créer : greffer refrain sur refrain, couplet sur
couplet (appariement par label, ou par similarité à la manière de DJ
StructFreak). Pratiquer : boucler « le pont », naviguer par section.

### 2.5 Ne remplacer qu'un fût — LarsNet — **M**

Découpe du stem batterie en grosse caisse, caisse claire, toms, charleston,
cymbales (poids CC BY-NC 4.0, convertis, republiables sous la même licence).
Permet « garder la batterie, changer le charleston » et des descripteurs de
groove par fût. Partagé avec 1.3.

### 2.6 Batterie symbolique — GrooVAE — **L**

Transcrire la batterie (1.3), transformer le pattern (humaniser, transférer
le groove d'un morceau de référence), le rendre avec un kit d'échantillons
libres. La structure est préservée par construction : le pattern vit sur la
grille d'origine. Points de contrôle GrooVAE de Magenta (dépôt archivé en
janvier 2026, poids sans licence déclarée : script qui les prend à la source,
conversion TF → safetensors localement), petit LSTM-VAE porté en Burn. Plan B :
ré-entraîner sur le Groove MIDI Dataset (CC BY 4.0).

### 2.7 Générer un stem — port Burn — **L**

**Candidats** : MusicGen-Stem (régénérer un stem étant donné les autres),
STAGE/DARC (batterie d'accompagnement conditionnée par le mix, rythme imposé
par tapotement), ACE-Step 1.5 « lego » (style donné par un audio de
référence), RAVE (garder le jeu, changer le son du kit).

**Pas de Python** (principe 4) : chaque modèle retenu est porté en Burn. Ce
qui rend la chose raisonnable : **MusicGen-Stem, STAGE et DARC partagent la
même architecture** (MusicGen : codec EnCodec + modèle de langage sur ses
jetons ; STAGE est un MusicGen-Small affiné, DARC un adaptateur sur STAGE).
Un seul port couvre les trois ; seuls les poids et le conditionnement
changent. Ordre recommandé :
1. **écoute d'abord**, références Python dans `experiments/` sur une dizaine
   de morceaux de la bibliothèque, pour choisir ce qui mérite un port ;
2. **port MusicGen** en Burn (EnCodec + décodeur autorégressif, conditionnement
   par le mix) — couvre STAGE (batterie d'accompagnement, ~0,4 B) puis
   MusicGen-Stem ; parité jeton par jeton contre Python ;
3. **RAVE** (encodeur/décodeur convolutif, plus petit) si le transfert de
   timbre plaît à l'écoute ;
4. **ACE-Step** en dernier : DiT 2 B + VAE + modèle de langage, le plus lourd ;
   le portage C++ du « lego » (`qvac-fabric-speech.cpp`) sert de référence.

La sortie repasse par l'alignement mesure par mesure de 2.1. Dans
l'interface, « Générer » devient une source de greffon à côté de
« Bibliothèque ».

---

### I. Interface des deux usages — itérations — **M, en continu** — *première maquette (8 oct.)*

`ui/prototype/maquette-editer-usages.html` : quatre états à comparer —
A·Pratiquer (bascule dans la barre d'outils, autoroute au centre, stems réduits
à leurs niveaux), A·Créer (pile complète, candidats et raisons dans
l'inspecteur, écoute A/B), B (« pratiquer » sur la ligne d'un stem, autoroute
dépliée sous la ligne), C (usage choisi dans le rail, partition + tablature au
centre). **Tranché le 9 oct. : variante C, la partition au centre**
(`ui-spec-editeur.md`, décision 11). **L'usage se choisit dans le rail**
(décision 12, 9 oct.) — le centre reste le plus clair possible.

**Décidé le 8 oct.** : on itère sur les possibilités avant de figer. Pratiquer
et créer ne demandent pas le même établi : l'un lit en continu une partie qui
défile, l'autre compare et remplace des stems.

**Méthode.** Maquettes HTML jetables dans `ui/prototype/` (il y a déjà
`maquette-editeur.html`), deux ou trois variantes à la fois, comparées à
l'usage puis tranchées dans `ui-spec-editeur.md` — comme l'établi du 10 sept.
Contraintes : `interface-guidelines.md`, `scripts/audit-interface.sh`.

**Pistes à maquetter.**
- un sélecteur *Pratiquer / Créer* dans l'établi, ou deux états de plus après
  « séparer » ;
- pour pratiquer : autoroute sous la ligne du stem coupé, ou autoroute en
  pleine zone centrale avec la pile de stems réduite à des boutons de
  niveau ; place de la partition alphaTab ; commandes de boucle et de vitesse
  au clavier ;
- pour créer : où vivent les candidats de greffe (inspecteur, liste à côté du
  stem), comment montrer les raisons du classement, comment comparer
  original / greffon / généré à l'écoute (bascule A/B).

Premières maquettes avec le jalon A, pour que l'autoroute (1.1) et la
recherche de greffons (2.2) arrivent dans une interface déjà essayée.

## Ordre proposé

| Jalon | Chantiers | Ce qu'on peut faire à la fin |
|---|---|---|
| **A** — socle | 0.1, 0.2, 0.3, I | boucler 4 mesures, ralentir, accélérer à chaque passage ; premières maquettes |
| **B** — décider | 0.4 | savoir quels modèles porter, chiffres à l'appui |
| **C** — pratiquer la basse | prototype alphaTab, 1.2, 1.4 (basse) | couper la basse, lire sa partition / tablature au centre, curseur et boucle sur les mesures |
| **D** — greffer juste | 2.1, 2.2 | « une batterie plus funk » trouvée dans la bibliothèque, calée mesure par mesure |
| **E** — pratiquer la batterie | 1.3, 2.5 | idem pour la batterie ; groove mesuré sur les notes |
| **F** — corriger, exporter | 1.4 (batterie), 1.5, 1.1 si besoin | partition batterie, corrections, export ; autoroute de repli |
| **G** — structure | 2.4, 2.3 | sections nommées, greffe refrain sur refrain, classement COCOLA |
| **H** — matière nouvelle | 2.6, 2.7 | batterie transformée ou générée, en Rust |

A et B se mènent en parallèle. C et D sont indépendants une fois A fait. G est
le plus gros risque technique (port d'all-in-one) et n'est requis par rien
avant lui : la greffe mesure par mesure n'a besoin que des premiers temps.

## Décisions du 8 oct. 2026

- **Hugging Face** : compte `rousseau` (voir 0.1).
- **MuScriptor** : on l'essaie, porté en Burn, conditions d'usage affichées
  (voir 1.2).
- **Pas de Python à l'exécution** : poids convertis, réseaux portés en Burn ;
  Python seulement pour préparer et vérifier (principe 4, 2.7).
- **Interface** : itérations par maquettes avant de figer (chantier I).
- **Pulsation** : calculée pour le morceau ouvert dans Éditer et pour les
  candidats de greffe au moment où on les démixe (2.2) — **pas de passe sur
  toute la bibliothèque**, qui n'aurait servi qu'à filtrer les candidats par
  métrique avant démixage ; le tempo déjà en base suffit à ce filtre.
