import "./lecteur.js"; // l'ordre d'exécution : voir main.js
import {
  $, fileAttente, invoke, navigation, remonter, signalerErreur,
} from "./socle.js";
import {
  inspecterAlbum,
} from "./ecouter.js";

import {
  chargerFamilles, dessinerCarte, modeAnneau, modeTemps, patienter,
} from "./maplibre.js";
import { lecture } from "./etat-lecture.js";

/* ------------------------------------------------------------- carte */

// Le nuage vit dans son propre repère, en [-1, 1]. `vue` porte la
// transformation vers les pixels : un facteur et un décalage, rien de plus.
const carte = {
  points: [],
  // Adresse réelle (lon/lat) de chaque morceau logé, `id -> [x, y]` — vide
  // sur le monde fictif. Voir `carte.villeReelle` et `chargerCarte`.
  positionsReelles: new Map(),
  vue: { k: 1, dx: 0, dy: 0 },
  isolees: new Set(), // familles mises en avant (multi-sélection), vide = tout
  survole: null,
  depart: null, // borne de départ d'un chemin
  arrivee: null, // borne d'arrivée
  route: null, // chemin tracé (les morceaux, un point par étape), ou null
  // Le TRAIT à dessiner entre les étapes de `route` — d'ordinaire `route`
  // lui-même (une droite d'étape en étape), sauf sur le plan de ville réel
  // où `tracerRouteSurCarte` l'enrichit après coup avec les vraies rues
  // entre chaque paire. `route` reste la vérité pour les repères posés à
  // chaque étape ; `routeTrace` ne sert qu'au tracé du trait.
  routeTrace: null,
  lasso: null, // contour en cours de tracé, en coordonnées de carte
  couleur: "famille", // famille, ou une clé de CONTINUES
  // Trois visualisations : le nuage t-SNE dessiné au canevas, la carte en
  // tuiles vectorielles, et le streamgraph temporel (voir l'objet `temps`
  // plus bas, qui porte ses propres coordonnées — temps × famille empilée,
  // rien à voir avec l'embedding ou le lon/lat d'ici).
  affichage: "points", // points (nuage t-SNE) | carte (tuiles) | temps (streamgraph)
  familles: null, // [[rang, nom, effectif]], chargé une fois
  bornes: {}, // min et max de chaque variable continue, pour la rampe
  filtre: "", // texte du filtre ; les exclus s'estompent, jamais ne disparaissent
  chemin: "direct", // direct | sonique | errance | dessine
  trace: null, // tracé en cours de dessin, en coordonnées de carte
  graine: 1, // graine de l'errance ; « Autre tirage » l'incrémente
  refaire: null, // de quoi rejouer le dernier chemin avec une autre graine
  // Le fond de plan de la carte MapLibre — eau, terre, voirie, bâti, toponymes.
  // Chaque thème est un `style-<id>.json` déjà écrit par `engendrer_tuiles` (côté
  // Rust, `rusty_music_carto::Palette`) ; basculer ne régénère aucune tuile.
  // `osm-clair` = le `style.json` sans suffixe. L'overlay de familles (nuage,
  // pastilles, territoires) ne dépend pas de ce choix.
  theme: localStorage.getItem("carte-theme") || "osm-clair",
  gl: null, // l'instance MapLibre, ou null tant que les tuiles manquent
  glPret: false,
  // `true` quand un plan de ville réel est actif (`positions_carte` a rendu
  // quelque chose côté Rust) : `carte.points[i].x/y` valent alors des lon/lat
  // réels — une adresse de rue, pas une position t-SNE — et
  // `geoDepuisCarte`/`carteDepuisGeo` doivent être l'identité plutôt que la
  // projection du monde fictif. Mis à jour par `chargerCarte`.
  villeReelle: false,
};

/// Ce que chaque mode attend de la souris, et ce qu'il fabrique. Phrase
/// entière dans le rail, sous les boutons ; rappel court en pied de carte, où
/// la place est comptée.
const AIDE_CHEMIN = {
  direct: [
    "Clic : le départ. Maj+clic : l'arrivée. Le trajet suit la droite entre les deux points de la carte, en cueillant au plus près.",
    "maj+clic : l'arrivée",
  ],
  sonique: [
    "Clic : le départ. Maj+clic : l'arrivée. Le trajet ne passe que d'un proche voisin sonore au suivant : plus long, sans à-coup à l'oreille — mais pas forcément droit à l'écran, la carte 2D déforme les distances. Le nombre de morceaux n'est ici qu'un plafond.",
    "maj+clic : l'arrivée",
  ],
  errance: [
    "Maj+clic : une promenade sonique part de ce morceau et dérive sans jamais revenir sur ses pas, en tirant à chaque saut parmi ses voisins sonores — les plus proches ont plus de chances, réglable dans Réglages.",
    "maj+clic : promenade",
  ],
  dessine: [
    "Maj+glisser : le trait cueille les morceaux qu'il touche. Ce qu'il traverse à vide reste vide.",
    "maj+glisser : tracer",
  ],
  itineraire: [
    "Maj+clic : le départ. Maj+clic à nouveau : une arrivée, facultative si une durée est fixée. Le trajet suit les vraies rues et la playlist est faite des morceaux qui les bordent ; le profil (grands axes / petites rues / parcs) le retrace aussitôt.",
    "maj+clic : départ / arrivée",
  ],
  voyage: [
    "Chercher un morceau puis Entrée : un voyage part de lui et n'avance que dans le temps, de voisin sonore en voisin sonore — il peut dériver vers une famille adjacente, jamais revenir en arrière.",
    "chercher + Entrée : départ",
  ],
};

/// Les variables continues qu'on peut porter sur la rampe.
///
/// Une seule mécanique pour les trois — bornes, dégradé, légende. Ajouter une
/// quatrième variable, c'est ajouter une ligne ici et un bouton dans le rail,
/// rien d'autre.
///
/// `tempo` et `energie` viennent de la passe `descripteurs` et peuvent manquer :
/// un morceau sans valeur reste tracé, en encre neutre.
const CONTINUES = {
  // `valide` écarte le bruit des tags avant qu'il n'entre dans les bornes :
  // sans lui, une poignée de morceaux à `year = 0` (année absente encodée en
  // « 0000 » plutôt qu'omise) tirait la borne basse à 0 et écrasait tout le
  // XXe-XXIe siècle sur les derniers pour cent de la rampe.
  annee: { champ: "year", format: (v) => String(Math.round(v)), valide: (v) => v > 0 },
  tempo: { champ: "bpm", format: (v) => `${Math.round(v)} BPM` },
  energie: { champ: "energy", format: (v) => v.toFixed(2) },
};

/// Étapes de la rampe séquentielle, lues dans la feuille de style.
function rampe() {
  return getComputedStyle(document.documentElement)
    .getPropertyValue("--rampe")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

/// Couleur d'un point sur la rampe, `t` entre 0 et 1.
function surRampe(etapes, t) {
  const i = Math.min(etapes.length - 1, Math.max(0, Math.round(t * (etapes.length - 1))));
  return etapes[i];
}

/// Vrai si le point passe le filtre courant — texte de recherche et
/// intervalle d'années (`filtreAnnee`).
function retenu(p) {
  if (!dansIntervalleAnnee(p.year)) return false;
  if (!carte.filtre) return true;
  const q = carte.filtre;
  return (
    (p.title || "").toLowerCase().includes(q) ||
    (p.artist || "").toLowerCase().includes(q) ||
    (p.album || "").toLowerCase().includes(q)
  );
}

/// Ordonne les bandes de familles pour minimiser la distance verticale totale
/// pondérée des arcs inter-familles — *minimum linear arrangement*, résolu
/// exactement par programmation dynamique sur les sous-ensembles de bits
/// (`docs/frise-filiations.md` § 2) : `dp[mask] = cut(mask) + min_{v∈mask}
/// dp[mask\{v}]`, où `cut(mask)` est le poids total entre `mask` et son
/// complément. O(2ⁿ·n) — pour n = 12 (le vocabulaire de familles par défaut),
/// quelques dizaines de milliers d'opérations, largement sous la
/// milliseconde ; reste praticable jusqu'à une vingtaine de familles. Calculé
/// une seule fois par chargement du réseau (`chargerTemps`), jamais à
/// l'image : aucun coût pour le rendu.
///
/// `poids` est une `Map` de `"idA,idB"` (idA < idB) vers un poids ≥ 0.
function ordonnerBandes(familles, poids) {
  const n = familles.length;
  if (n <= 2) return familles.slice();

  const w = (i, j) => {
    const a = familles[i], b = familles[j];
    return poids.get(a < b ? `${a},${b}` : `${b},${a}`) || 0;
  };

  // Au-delà d'une vingtaine de familles, 2ⁿ cesse d'être instantané : repli
  // glouton documenté (`docs/frise-filiations.md` § 2), jamais atteint avec
  // le vocabulaire par défaut.
  if (n > 20) return ordonnerBandesGlouton(familles, w, n);

  const bit = (x) => 31 - Math.clz32(x);
  const total = 1 << n;
  const degre = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    let s = 0;
    for (let j = 0; j < n; j++) if (j !== i) s += w(i, j);
    degre[i] = s;
  }
  // sumTotal(mask) = Σ degré des éléments de mask ; interne(mask) = Σ poids
  // des paires internes à mask. cut(mask) = sumTotal(mask) − 2·interne(mask)
  // — le poids entre mask et son complément, sans jamais énumérer ce dernier.
  const sumTotal = new Float64Array(total);
  const interne = new Float64Array(total);
  for (let mask = 1; mask < total; mask++) {
    const bas = mask & -mask;
    const i = bit(bas);
    const reste = mask ^ bas;
    sumTotal[mask] = sumTotal[reste] + degre[i];
    let ajout = 0;
    let m = reste;
    while (m) {
      const b = m & -m;
      ajout += w(i, bit(b));
      m ^= b;
    }
    interne[mask] = interne[reste] + ajout;
  }
  const cut = (mask) => sumTotal[mask] - 2 * interne[mask];

  const dp = new Float64Array(total);
  const choix = new Int8Array(total).fill(-1);
  for (let mask = 1; mask < total; mask++) {
    let meilleur = Infinity;
    let meilleurV = -1;
    let m = mask;
    while (m) {
      const b = m & -m;
      const v = bit(b);
      const c = dp[mask ^ b];
      if (c < meilleur) {
        meilleur = c;
        meilleurV = v;
      }
      m ^= b;
    }
    dp[mask] = cut(mask) + meilleur;
    choix[mask] = meilleurV;
  }

  // Rétropropagation : `choix[mask]` est le dernier élément ajouté pour
  // atteindre `mask`, donc la position la plus à droite parmi ceux de mask.
  const ordre = new Array(n);
  let mask = total - 1;
  for (let pos = n - 1; pos >= 0; pos--) {
    const v = choix[mask];
    ordre[pos] = familles[v];
    mask ^= 1 << v;
  }
  return ordre;
}

/// Repli glouton du *minimum linear arrangement*, pour un vocabulaire de
/// familles trop grand pour la DP exacte — voir `ordonnerBandes`. Part de la
/// paire la plus fortement connectée puis étend l'extrémité la plus
/// prometteuse à chaque pas. O(n²), toujours instantané.
function ordonnerBandesGlouton(familles, w, n) {
  const reste = new Set(familles.map((_, i) => i));
  let meilleurePaire = [0, Math.min(1, n - 1)];
  let meilleurPoids = -1;
  for (const i of reste) {
    for (const j of reste) {
      if (i >= j) continue;
      const p = w(i, j);
      if (p > meilleurPoids) {
        meilleurPoids = p;
        meilleurePaire = [i, j];
      }
    }
  }
  const ordre = [...meilleurePaire];
  for (const i of ordre) reste.delete(i);
  while (reste.size) {
    let meilleur = null;
    let poidsMax = -1;
    let avant = false;
    for (const i of reste) {
      const pd = w(ordre[ordre.length - 1], i);
      const pg = w(ordre[0], i);
      if (pd > poidsMax) {
        poidsMax = pd;
        meilleur = i;
        avant = false;
      }
      if (pg > poidsMax) {
        poidsMax = pg;
        meilleur = i;
        avant = true;
      }
    }
    reste.delete(meilleur);
    if (avant) ordre.unshift(meilleur);
    else ordre.push(meilleur);
  }
  return ordre.map((i) => familles[i]);
}

/// État du mode Explorer → Temps : la frise des filiations, un point par
/// album sur un axe chronologique, réparti en bandes de familles
/// (`docs/frise-filiations.md`). Remplace l'ancien streamgraph — même
/// réseau que le mode Anneau (`reseau_albums`), seule la position à l'écran
/// change (chronologique + bande, au lieu de radial).
///
/// Coordonnées propres — temps (x) et bande de famille (y) — jamais mêlées à
/// `carte.points`/`carte.vue`, qui portent des positions d'embedding ou de
/// lon/lat.
const temps = {
  charge: false,
  albums: [], // [{id, name, artist, famille, familleSecondaire, annee, sortants}]
  arcs: [], // [{a, b, distance}] — le même fond permanent que l'anneau
  bandes: [], // familles, dans l'ordre calculé par `ordonnerBandes` (haut → bas)
  posFamille: new Map(), // famille -> index dans `bandes`
  minAnnee: null,
  maxAnnee: null,
  aIncertain: false,
  // Seuil de marquage des hubs (§ 6) — initialisé au chargement à
  // moyenne + 2 écarts-types des degrés sortants, ajustable ensuite par le
  // curseur du rail (`#temps-seuil`, `temps.seuilHub` mis à jour en direct).
  seuilHub: 8,
  // Bande étendue verticalement (§ 3, zoom vertical ciblé) — un id de
  // famille, ou `null`. Basculée depuis la légende (`rendreFamilles`), pas un
  // zoom généraliste : l'axe horizontal (`vue.k` ci-dessous) et l'axe
  // vertical ont chacun leur propre contrôle, jamais couplés.
  bandeEtendue: null,
  // Un point par album effectivement dessiné à la dernière image, en
  // coordonnées écran — reconstruit à chaque `dessinerTemps`, c'est sur lui
  // que `survolerTemps` pointe pour retrouver l'album sous la souris.
  puces: [],
  survole: null,
  survoleCle: null,
  // Zoom et glisser sur les deux axes à la fois — un seul facteur `k` pour
  // le temps et les bandes ensemble (comme `carte.vue.k` sur le nuage), `dx`
  // et `dy` les décalages en pixels de chaque axe, reclampés à chaque image
  // dans `dessinerTemps` pour ne jamais faire défiler du vide. L'accordéon
  // (`bandeEtendue`) et `k` composent : étendre une bande donne un point de
  // départ, zoomer permet ensuite d'aller y regarder de près.
  vue: { k: 1, dx: 0, dy: 0 },
  // Bornes du dernier tracé (`{xDeb, xFin, xPasBase, n, yHaut, hauteur}`) —
  // c'est sur elles que `zoomerTemps` calcule le
  // nouveau décalage, sans dupliquer le calcul des marges de `dessinerTemps`.
  disposition: null,
};

/// Charge le réseau d'albums et calcule l'ordre des bandes au premier passage
/// en mode Temps — mêmes données pour toute la session, comme
/// `anneau.reseau`. Réutilise `reseau_albums` (le fond permanent du mode
/// Anneau) : aucun calcul de similarité neuf, voir `docs/frise-filiations.md`.
async function chargerTemps() {
  if (temps.charge) return;
  await chargerFamilles();
  let r;
  try {
    r = await invoke("reseau_albums", { limiteArcs: 4000 });
  } catch (e) {
    signalerErreur("fil-compte", "échec du chargement du réseau d'albums", e, "réseau d'albums (frise)");
    return;
  }

  const albums = r.albums.map((a) => ({
    id: a.id,
    name: a.name,
    artist: a.artist,
    famille: a.famille,
    familleSecondaire: a.famille_secondaire ?? null,
    annee: a.annee ?? null,
    sortants: 0,
  }));
  const parId = new Map(albums.map((a) => [a.id, a]));
  const arcs = r.arcs.filter((l) => parId.has(l.a) && parId.has(l.b));

  temps.albums = albums;
  temps.arcs = arcs;
  temps.aIncertain = albums.some((a) => a.annee == null);
  const annees = albums.map((a) => a.annee).filter((a) => a != null);
  temps.minAnnee = annees.length ? Math.min(...annees) : null;
  temps.maxAnnee = annees.length ? Math.max(...annees) : null;

  // Poids inter-familles : agrégat des arcs album-à-album du réseau existant,
  // groupés par paire de familles maîtresses — voir `ordonnerBandes` et
  // `docs/frise-filiations.md` § 2. Les arcs intra-famille ne comptent pas :
  // ils ne changent rien à une distance verticale nulle par construction.
  const poids = new Map();
  for (const l of arcs) {
    const fa = parId.get(l.a).famille;
    const fb = parId.get(l.b).famille;
    if (fa == null || fb == null || fa === fb) continue;
    const cle = fa < fb ? `${fa},${fb}` : `${fb},${fa}`;
    poids.set(cle, (poids.get(cle) || 0) + 1 / (1 + l.distance));
  }
  temps.bandes = ordonnerBandes(
    carte.familles.map(([c]) => c),
    poids,
  );
  temps.posFamille = new Map(temps.bandes.map((f, i) => [f, i]));

  // Degré sortant de chaque album — un arc compte pour son extrémité la plus
  // ancienne, vers un album plus tardif (§ 6). Deux albums de la même année
  // n'ont pas de sens directionnel : l'arc ne compte pour aucun des deux.
  for (const l of arcs) {
    const a = parId.get(l.a);
    const b = parId.get(l.b);
    if (a.annee == null || b.annee == null || a.annee === b.annee) continue;
    (a.annee < b.annee ? a : b).sortants++;
  }
  const degres = albums.map((a) => a.sortants).filter((d) => d > 0);
  if (degres.length) {
    const moyenne = degres.reduce((s, d) => s + d, 0) / degres.length;
    const variance = degres.reduce((s, d) => s + (d - moyenne) ** 2, 0) / degres.length;
    temps.seuilHub = Math.max(3, Math.round(moyenne + 2 * Math.sqrt(variance)));
    const curseur = $("temps-seuil");
    if (curseur) {
      curseur.value = String(Math.min(40, temps.seuilHub));
      $("temps-seuil-val").textContent = `${temps.seuilHub} connexion${temps.seuilHub > 1 ? "s" : ""}`;
    }
  }

  temps.charge = true;
  // Idem `chargerReseauAnneau` : forcer un nouveau passage du suivi de lecture
  // maintenant que la frise peut tracer.
  albumFocalLecture = null;
}

/// État du mode Explorer → Anneau : un graphe circulaire à liens groupés
/// (hierarchical edge bundling, façon Eigenfactor) centré sur **un seul
/// album focal** — ses voisins soniques, scindés en antérieurs/postérieurs.
///
/// Coordonnées propres — un cercle centré sur le canevas — jamais mêlées à
/// `carte.points`/`carte.vue` ni à `temps.*` : ni un embedding, ni un axe du
/// temps. Zoom/glisser malgré tout : même principe qu'un cercle qu'on
/// grossirait et déplacerait à l'écran, un seul facteur `k` (voir `carte.vue`
/// sur le nuage) plutôt que deux comme la frise (pas de second axe à
/// préserver ici).
const anneau = {
  focal: null, // {id, name, artist, famille, path}, ou null tant que rien n'est choisi
  voisins: [], // [{id, name, artist, famille, path, avant}]
  k: 8, // nombre de voisins demandés au moteur
  // Tension du bundling hiérarchique (Holten) — voir `courbeBundle`. 1 :
  // suit exactement la hiérarchie genre/album ; 0 : droite directe.
  beta: 0.8,
  vue: { k: 1, dx: 0, dy: 0 }, // zoom/glisser — voir `zoomerAnneau`
  resultats: [], // dernier résultat de recherche d'album, pour la barre #q
  // Un point par nœud dessiné à la dernière image, en coordonnées écran —
  // reconstruit à chaque `dessinerAnneau`, c'est sur lui que `survolerAnneau`
  // pointe pour retrouver l'album sous la souris ou sous un clic.
  puces: [],
  survole: null,
  // Le fond permanent, chargé une fois par session (voir `chargerReseauAnneau`)
  // — tous les albums connus et un échantillon de leurs liens. C'est là que
  // vit la position de chaque album (`angleAlbum`), calculée une bonne fois
  // pour toutes : elle ne dépend jamais de l'album focal, voir
  // `docs/carto-anneau.md` § stabilité des positions.
  reseau: {
    charge: false,
    albums: [], // [{id, name, artist, famille, path, duree_ms, annee}]
    arcs: [], // [{a, b, distance}], le fond ténu
    arcsFamille: new Map(), // cluster -> {debut, fin}, mêmes bornes que la couronne
    angleAlbum: new Map(), // id -> angle, stable
  },
};

/// Intervalle d'années, commun aux quatre visualisations du mode Explorer
/// (Nuage, Carte, Temps, Anneau) — un seul réglage plutôt que quatre, pour
/// que « voir les morceaux entre 1990 et 2000 » ait le même sens partout.
/// Mêmes bornes que la rampe « Colorer par : Année » (`carte.bornes.annee`),
/// pas une seconde lecture. `min`/`max` restent `null` tant que les bornes ne
/// sont pas connues, ou s'il n'y a pas au moins deux années distinctes à
/// délimiter — le bloc reste alors caché (`majVisibiliteIntervalleAnnees`).
/// `debut`/`fin` couvrent tout par défaut : rien n'est filtré tant qu'on n'a
/// pas resserré l'intervalle.
const filtreAnnee = {
  min: null,
  max: null,
  debut: null,
  fin: null,
};

/// `true` si l'intervalle est resserré — sinon rien à filtrer, quel que soit
/// le mode.
function intervalleAnneeActif() {
  return filtreAnnee.min != null && (filtreAnnee.debut > filtreAnnee.min || filtreAnnee.fin < filtreAnnee.max);
}

/// Un morceau/artiste/album sans année compte **toujours** comme dans
/// l'intervalle — l'absence de date ne doit jamais l'exclure d'une vue
/// (même règle que le filtre texte : on estompe un contenu qu'on écarte,
/// on ne fait jamais disparaître un contenu faute de savoir le classer).
function dansIntervalleAnnee(annee) {
  if (!intervalleAnneeActif()) return true;
  if (annee == null) return true;
  return annee >= filtreAnnee.debut && annee <= filtreAnnee.fin;
}

/// Cherche des albums pour la barre `#q` en mode Anneau, et remplit
/// `#anneau-resultats`. `anneau.resultats` garde la dernière liste pour que
/// Entrée (dans le gestionnaire de `#q`) puisse en retenir le premier sans
/// relancer une recherche.
async function chercherAlbums(q) {
  const hote = $("anneau-resultats");
  if (!q) {
    anneau.resultats = [];
    hote.hidden = true;
    hote.replaceChildren();
    return;
  }
  let resultats;
  try {
    resultats = await invoke("search_albums", { query: q, limit: 20 });
  } catch (e) {
    remonter(e, "recherche d'albums");
    return;
  }
  anneau.resultats = resultats;
  hote.replaceChildren();
  hote.hidden = resultats.length === 0;
  for (const a of resultats) {
    const el = document.createElement("button");
    el.className = "candidat";
    el.innerHTML = "<b></b><span></span>";
    el.children[0].textContent = a.name;
    el.children[1].textContent = a.artist;
    el.addEventListener("click", () => chargerAnneau(a.id));
    hote.appendChild(el);
  }
}

/// Fait de `id` l'album focal de l'anneau et recharge son voisinage — appelée
/// aussi bien depuis un résultat de recherche que depuis un clic sur un
/// voisin déjà affiché (« marcher » dans le graphe de ressemblance).
///
/// `discret` : appel de fond, quand c'est la lecture qui déplace le focal
/// (`suivreAlbumEnLecture`) et non un geste. On tait alors le mot d'attente
/// du pied de carte et, en cas d'échec (album sans empreinte), l'erreur dans
/// `#fil-compte` — ce n'est pas l'utilisateur qui a demandé ce calcul.
/// `avecInspecteur` : peuple l'inspecteur avec l'album (le défaut, comme un
/// clic) ; `false` en suivi de lecture, où l'inspecteur suit le morceau joué.
async function chargerAnneau(id, { discret = false, avecInspecteur = true } = {}) {
  if (!discret) patienter("calcul de l'anneau…");
  let vue;
  try {
    vue = await invoke("album_ring", { albumId: id, k: anneau.k });
  } catch (e) {
    if (discret) {
      // L'album joué n'a pas d'anneau calculable (jamais analysé) : plutôt
      // que de laisser la sélection sur le disque précédent, on l'efface —
      // l'affichage doit désigner ce qu'on écoute, ou rien.
      remonter(e, "anneau (suivi de lecture)");
      anneau.focal = null;
      anneau.voisins = [];
      dessinerCarte();
    } else {
      signalerErreur("fil-compte", "échec du calcul de l'anneau", e, "anneau");
    }
    return;
  } finally {
    if (!discret) patienter(null);
  }
  anneau.focal = vue.focal;
  anneau.voisins = vue.voisins;
  $("anneau-resultats").hidden = true;
  $("fil-compte").textContent = `${anneau.focal.name} — ${anneau.voisins.length} voisin(s)`;
  if (avecInspecteur) await inspecterAlbum(anneau.focal);
  dessinerCarte();
}

/// Retrouve l'identifiant d'anneau d'un album (nom + artiste) — d'abord dans
/// les réseaux déjà chargés (Temps, Anneau), qui portent l'identité exacte
/// calculée par le moteur, sinon par une recherche plein texte. Comparaison
/// insensible à la casse et aux espaces de bord : l'artiste d'une piste
/// (`tracks.artist`) n'est pas toujours l'artiste de regroupement dont le
/// moteur dérive l'identité du disque. `null` si l'album reste introuvable.
async function idAlbumPour(nom, artiste) {
  const eq = (x, y) => (x || "").trim().toLowerCase() === (y || "").trim().toLowerCase();
  for (const liste of [temps.albums, anneau.reseau.albums]) {
    if (!liste || liste.length === 0) continue;
    const exact = liste.find((a) => eq(a.name, nom) && eq(a.artist, artiste));
    if (exact) return exact.id;
    const parNom = liste.find((a) => eq(a.name, nom));
    if (parNom) return parNom.id;
  }
  try {
    const hits = await invoke("search_albums", { query: nom, limit: 20 });
    const exact = hits.find((h) => eq(h.name, nom) && eq(h.artist, artiste));
    if (exact) return exact.id;
    return hits.find((h) => eq(h.name, nom))?.id ?? null;
  } catch (e) {
    remonter(e, "recherche d'album (suivi de lecture)");
    return null;
  }
}

/// Album (nom + artiste) **effectivement affiché** comme focal de l'anneau /
/// de la frise parce qu'on l'écoute — `null` tant que rien ne l'est. N'est
/// posé qu'**après** confirmation que `anneau.focal` le reflète : si le
/// premier essai échoue (réseau pas encore chargé, `album_ring` en erreur),
/// il reste `null` et le battement suivant réessaie. C'était le bug « les
/// liens n'apparaissent qu'au 2ᵉ morceau » : la clé était posée avant le
/// succès, et la déduplication bloquait tout nouvel essai sur le 1ᵉʳ disque.
let albumFocalLecture = null;
/// Clé du disque en cours de résolution — anti-concurrence : le sondage
/// appelle `suivreAlbumEnLecture` à 5 Hz sans l'attendre.
let suiviAlbumEnVol = null;
const cleAlbumLecture = (nom, artiste) => JSON.stringify([nom ?? null, artiste ?? ""]);

/// Fait de l'album du morceau en cours de lecture le focal partagé de la
/// frise (Temps) et de l'anneau — « comme si on l'avait cliqué » : ses voisins
/// soniques, ses arcs mis en avant, et **rien d'autre**. Appelée à chaque
/// battement du sondage (idempotente : sort tout de suite si l'album focal est
/// déjà le bon) et à chaque entrée dans Temps/Anneau.
///
/// L'affichage désigne **toujours** le disque en cours d'écoute : la lecture
/// entre dans un autre album → il devient le focal ; le disque ne peut pas
/// être montré (pas de métadonnée, hors réseau, jamais analysé) → la sélection
/// est effacée, jamais laissée sur le disque précédent. Ne touche pas
/// l'inspecteur — en lecture il suit le morceau, pas le disque.
async function suivreAlbumEnLecture() {
  if (navigation.mode !== "explorer" || (!modeTemps() && !modeAnneau())) {
    albumFocalLecture = null;
    suiviAlbumEnVol = null;
    return;
  }
  // Morceau joué absent de la file : fenêtre courte après `remplacer_file` —
  // le morceau d'avant finit de jouer pendant que la nouvelle playlist prend
  // la main. État transitoire, on ne touche à rien (le prochain morceau de la
  // file résoudra).
  const piste = fileAttente.pistes.find((x) => x.path === lecture.courant);
  if (!piste) return;
  const nom = piste.album || null;
  const artiste = piste.artist || "";
  const cle = cleAlbumLecture(nom, artiste);

  // Déjà affiché pour ce disque, ou déjà en cours de résolution : rien à faire.
  if (albumFocalLecture && cleAlbumLecture(albumFocalLecture.nom, albumFocalLecture.artiste) === cle) return;
  if (suiviAlbumEnVol === cle) return;
  suiviAlbumEnVol = cle;
  try {
    // Morceau sans métadonnée d'album : rien de montrable, on efface — jamais
    // garder les connexions du disque précédent.
    if (!nom) {
      if (anneau.focal) {
        anneau.focal = null;
        anneau.voisins = [];
        dessinerCarte();
      }
      albumFocalLecture = { nom, artiste };
      return;
    }

    const id = await idAlbumPour(nom, artiste);
    if (suiviAlbumEnVol !== cle) return; // un autre disque a pris la main

    if (id == null) {
      // Introuvable — album pas indexé, ou (rare) `search_albums` qui a raté au
      // démarrage. On **n'enregistre pas** : le prochain battement réessaie.
      if (anneau.focal) {
        anneau.focal = null;
        anneau.voisins = [];
        dessinerCarte();
      }
      $("fil-compte").textContent = `${nom} — hors du réseau d'albums`;
      return;
    }

    if (!(anneau.focal && String(anneau.focal.id) === String(id))) {
      await chargerAnneau(id, { discret: true, avecInspecteur: false });
      if (suiviAlbumEnVol !== cle) return;
    } else {
      // Focal déjà bon mais on repasse ici (réseau qui vient d'arriver, par ex.)
      // : forcer un redessin pour que l'anneau trace enfin la sélection.
      dessinerCarte();
    }
    // `id` résolu : on enregistre, que `chargerAnneau` ait tracé l'anneau ou
    // qu'il ait échoué (album sans empreinte → `focal` remis à `null`,
    // affichage vide mais stable — « ce qu'on écoute, ou rien »). Le rendu
    // effectif, si le réseau finit encore de charger, est rattrapé par
    // `chargerReseauAnneau` (qui remet `albumFocalLecture` à `null` en fin de
    // chargement pour forcer un nouveau passage).
    albumFocalLecture = { nom, artiste };
  } finally {
    if (suiviAlbumEnVol === cle) suiviAlbumEnVol = null;
  }
}

/// Charge le fond permanent de l'anneau une fois par session — tous les
/// albums connus et un échantillon de leurs liens de plus proche voisinage
/// (voir `reseau_albums` côté cœur). Après quoi `calculerAnglesAnneau` fixe
/// la position de chacun, une bonne fois pour toutes.
async function chargerReseauAnneau() {
  if (anneau.reseau.charge) return;
  await chargerFamilles();
  let r;
  try {
    r = await invoke("reseau_albums", { limiteArcs: 1500 });
  } catch (e) {
    remonter(e, "réseau d'albums");
    return;
  }
  anneau.reseau.albums = r.albums;
  anneau.reseau.arcs = r.arcs;
  calculerAnglesAnneau();
  anneau.reseau.charge = true;
  // Le réseau vient d'arriver : un album focal posé avant (par le suivi de
  // lecture, alors que `dessinerAnneau` ne pouvait encore rien tracer) doit
  // être redessiné. On force un nouveau passage du suivi.
  albumFocalLecture = null;
}

/// Fixe l'angle de chaque famille (mêmes bornes que la couronne extérieure)
/// et de chaque album (réparti sur le sous-arc de sa famille, dans un ordre
/// stable — par identifiant, pas par l'ordre de retour du moteur). Appelée
/// une seule fois par chargement du réseau : c'est cette stabilité qui
/// garantit qu'un album ne bouge jamais quand on change d'album focal — voir
/// `docs/carto-anneau.md` § stabilité des positions.
function calculerAnglesAnneau() {
  const arcsFamille = new Map();
  const total = carte.familles.reduce((s, [, , n]) => s + n, 0) || 1;
  let angleCourant = -Math.PI / 2;
  for (const [cluster, , n] of carte.familles) {
    const largeur = (n / total) * Math.PI * 2;
    arcsFamille.set(cluster, { debut: angleCourant, fin: angleCourant + largeur });
    angleCourant += largeur;
  }

  const parFamille = new Map();
  for (const a of anneau.reseau.albums) {
    if (a.famille == null || !arcsFamille.has(a.famille)) continue;
    if (!parFamille.has(a.famille)) parFamille.set(a.famille, []);
    parFamille.get(a.famille).push(a);
  }
  for (const groupe of parFamille.values()) {
    groupe.sort((x, y) => (x.id < y.id ? -1 : x.id > y.id ? 1 : 0));
  }

  const angleAlbum = new Map();
  for (const [cluster, groupe] of parFamille) {
    const { debut, fin } = arcsFamille.get(cluster);
    const pas = (fin - debut) / groupe.length;
    groupe.forEach((a, i) => angleAlbum.set(a.id, debut + pas * (i + 0.5)));
  }

  anneau.reseau.arcsFamille = arcsFamille;
  anneau.reseau.angleAlbum = angleAlbum;
}

const cnv = $("carte");
const ctx = cnv.getContext("2d");

export {
  AIDE_CHEMIN, anneau, carte, chargerAnneau, chargerReseauAnneau, chargerTemps, chercherAlbums,
  cnv, CONTINUES, ctx, dansIntervalleAnnee, filtreAnnee, intervalleAnneeActif, rampe, retenu,
  suivreAlbumEnLecture, surRampe, temps,
};
