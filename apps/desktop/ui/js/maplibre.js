import "./carte.js"; // l'ordre d'exécution : voir main.js
import {
  $, duree, fileAttente, invoke, isoler, navigation, remonter, signalerErreur, txt,
} from "./socle.js";
import {
  lama,
} from "./lama.js";
import {
  selectionner,
} from "./ecouter.js";
import {
  demarrerLecture, dessinerFile,
} from "./lecteur.js";
import {
  anneau, carte, chargerAnneau, cnv, CONTINUES, ctx, dansIntervalleAnnee, filtreAnnee,
  intervalleAnneeActif, rampe, retenu, surRampe, temps,
} from "./carte.js";
import {
  majSegmentsCouleur,
} from "./ecouter-familles.js";
import {
  aideCourante, bruitChemin, dessinerBornes, poserBorne,
} from "./fond-de-plan.js";
import {
  tracerItineraire,
} from "./itineraires.js";
import {
  cliquerVoie, dessinerBalade,
} from "./balade.js";
import { dessinerAura } from "./aura.js";
import { lecture } from "./etat-lecture.js";

/* ------------------------------------------------- carte MapLibre (module 2)
 *
 * Les tuiles vectorielles remplacent le nuage et la nappe dessinés à la main ;
 * le canevas reste, transparent, pour ce que les tuiles ne portent pas — les
 * chemins, le lasso, les bornes de départ et d'arrivée.
 *
 * **MapLibre n'écoute aucun événement** (`interactive: false`) : le canevas
 * est au-dessus et garde tous les gestionnaires, qu'il relaie. C'est ce qui
 * permet à `versEcran` et `versCarte` — les deux seules transformations de
 * coordonnées du fichier — de simplement déléguer, et à tout le reste (lasso,
 * tracés, pointage) de continuer sans être réécrit.
 *
 * La carte vivait dans une seconde fenêtre : MapLibre ne s'y initialisait
 * jamais, sans la moindre erreur. Dans la fenêtre principale, il démarre du
 * premier coup — voir `docs/carto-etapes.md`.
 */

/// Demi-étendue du domaine de la carte. **Doit valoir exactement celle de
/// `crates/carto/src/projection.rs`**, sinon la surcouche se décale des tuiles.
/// Ne sert que sur le monde fictif — voir `carte.villeReelle`.
const DEMI_ETENDUE = 1.08;


/// Ce sur quoi le bouton « Vue d'ensemble » ramène la caméra MapLibre :
/// `bounds` (l'emprise de la limite communale, `metadata["rusty:bounds"]` du
/// style) sur le plan de ville réel ; `center`/`zoom` d'accueil sinon, en
/// dernier repli. Rempli par `initialiserGL`.
let vueInitialeGL = null;


/// `true` quand ce sont les adresses réelles du plan de ville qui sont à
/// l'écran — donc le repère dans lequel `versEcran`/`versCarte` travaillent.
/// Le nuage t-SNE et le plan de ville coexistent : dans le nuage, on reste en
/// t-SNE même quand une ville est importée. Les modes de chemin qui raisonnent
/// à l'écran (direct, dessiné, lasso) passent ce drapeau au moteur pour qu'il
/// interpole dans le bon repère — sans lui, un trait tracé sur le nuage était
/// testé contre des positions parisiennes et n'attrapait rien.
const carteReelle = () => carte.affichage === "carte" && carte.villeReelle;

/// Le zoom maximal accordé à la caméra MapLibre — repris de `initialiserGL`
/// pour que `zoomer()` (molette, pincement) ne plafonne pas avant le zoom
/// auquel les tuiles ont vraiment quelque chose à montrer (le bâti n'apparaît
/// qu'au zoom 15 sur le plan de ville réel).
let zoomMax = 14;

/// L'instance MapLibre **si elle est aux commandes**, sinon `null`.
///
/// Le nuage t-SNE et la carte partagent le même canevas et les mêmes
/// gestionnaires ; c'est ce raccourci qui décide lequel des deux repères
/// gouverne les coordonnées, le déplacement et le zoom. Sans lui, ouvrir le
/// nuage laissait MapLibre projeter les points et le mode devenait
/// inatteignable.
function carteGL() {
  return carte.affichage === "carte" && carte.gl ? carte.gl : null;
}

/// `true` sur le streamgraph temporel — voir l'objet `temps` et
/// `dessinerTemps`. Ni embedding ni tuiles : un troisième repère, propre à ce
/// mode, jamais mêlé aux deux autres.
const modeTemps = () => carte.affichage === "temps";

/// `true` sur l'anneau de similarité par album — voir l'objet `anneau` et
/// `dessinerAnneau`. Un quatrième repère, encore différent des trois autres :
/// un cercle centré sur un album focal, pas un axe temps ni un embedding.
const modeAnneau = () => carte.affichage === "anneau";

/// La console d'une webview du système n'est pas lisible de l'extérieur : ce
/// qui compte repart vers le journal du processus.
function journalCarte(message, niveau = "log") {
  console[niveau === "warn" ? "warn" : "log"]("[carte] " + message);
  invoke("journal_carte", { niveau, message: String(message) }).catch(() => {});
}

function geoDepuisCarte(x, y) {
  // Plan de ville réel : `x`/`y` sont déjà des lon/lat (une adresse de rue),
  // pas un point du carré `[-1.08, 1.08]²` du monde fictif — rien à projeter.
  if (carte.villeReelle) return [x, y];
  const u = (x / DEMI_ETENDUE + 1) / 2;
  const v = (1 - y / DEMI_ETENDUE) / 2;
  return [(u * 2 - 1) * 180, (Math.atan(Math.sinh(Math.PI * (1 - 2 * v))) * 180) / Math.PI];
}

function carteDepuisGeo(lng, lat) {
  if (carte.villeReelle) return [lng, lat];
  const phi = (lat * Math.PI) / 180;
  const u = (lng / 180 + 1) / 2;
  const v = (1 - Math.log(Math.tan(phi) + 1 / Math.cos(phi)) / Math.PI) / 2;
  return [(u * 2 - 1) * DEMI_ETENDUE, (1 - v * 2) * DEMI_ETENDUE];
}

/// Combien de mètres au sol valent `px` pixels à l'écran, au centre de la vue
/// MapLibre courante. Sur le plan de ville réel, `direct` et `dessiné`
/// travaillent en mètres locaux côté moteur (`RepereLocal`) : le rayon de
/// cueillette doit être dans la même unité, pas dans le repère t-SNE.
function metresParPixels(px) {
  if (!carte.gl) return px;
  const r = cnv.getBoundingClientRect();
  const cx = r.width / 2;
  const cy = r.height / 2;
  const a = carte.gl.unproject([cx, cy]);
  const b = carte.gl.unproject([cx + px, cy]);
  const latM = (((a.lat + b.lat) / 2) * Math.PI) / 180;
  const dx = (((b.lng - a.lng) * Math.PI) / 180) * Math.cos(latM) * 6371000;
  const dy = (((b.lat - a.lat) * Math.PI) / 180) * 6371000;
  return Math.hypot(dx, dy);
}

/// L'emprise géographique de tous les points, `[[ouest, sud], [est, nord]]`,
/// ou `null` s'il n'y en a pas encore. Sert à `fitBounds` pour la vue
/// d'ensemble : sur le plan de ville `p.x/p.y` sont déjà des lon/lat, sur le
/// monde fictif `geoDepuisCarte` les projette d'abord.
function bornesGeoPoints() {
  let o = Infinity, s = Infinity, e = -Infinity, n = -Infinity;
  const ajoute = (lng, lat) => {
    if (!Number.isFinite(lng) || !Number.isFinite(lat)) return;
    if (lng < o) o = lng;
    if (lng > e) e = lng;
    if (lat < s) s = lat;
    if (lat > n) n = lat;
  };
  if (carte.villeReelle) {
    // Sur le plan de ville, seules les adresses réelles comptent. `p.x/p.y`
    // restent des coordonnées t-SNE (~[-1, 1]) : les passer pour des lon/lat
    // renverrait la caméra au large de l'Afrique — c'était la cause de
    // l'image grise. Les morceaux sans adresse sont simplement absents de
    // `positionsReelles`.
    for (const [lng, lat] of carte.positionsReelles.values()) ajoute(lng, lat);
  } else {
    for (const p of carte.points) {
      const [lng, lat] = geoDepuisCarte(p.x, p.y);
      ajoute(lng, lat);
    }
  }
  return o <= e && s <= n ? [[o, s], [e, n]] : null;
}

/// Les tuiles passent par `addProtocol`, sur le fil principal : le fil de
/// travail de MapLibre ne peut pas atteindre un schéma d'URI personnalisé.
if (window.maplibregl) {
  maplibregl.addProtocol("tuiles", async (params) => {
    const m = /^tuiles:\/\/[^/]*\/(carte|relief)\/(\d+)\/(\d+)\/(\d+)/.exec(params.url);
    if (!m) throw new Error("URL de tuile illisible : " + params.url);
    const octets = await invoke("tuile", { quoi: m[1], z: +m[2], x: +m[3], y: +m[4] });
    const donnees = octets instanceof ArrayBuffer ? octets : new Uint8Array(octets).buffer;
    // Un tableau vide veut dire « pas de tuile ici » : le cas ordinaire sur un
    // monde creux.
    return { data: donnees.byteLength ? donnees : null };
  });
}

async function initialiserGL() {
  if (carte.gl || !window.maplibregl) return;
  let style;
  try {
    style = await invoke("style_carte", { theme: carte.theme });
  } catch (e) {
    // Pas encore de tuiles : le nuage dessiné à la main prend le relais.
    console.warn("[carte] tuiles absentes :", e);
    return;
  }
  // Sur le plan de ville réel, `style` porte son propre `center`/`zoom`
  // (Paris, pas le centre du monde fictif) — voir `crate::style::construire`
  // côté Rust. Absents sur le monde fictif : on retombe alors sur les
  // anciennes valeurs, inchangées. `maxZoom` suit le même principe pour le
  // sur-zoom : 14 par défaut (le monde fictif s'arrête à 9, le sur-zoom
  // comble le reste), mais jamais moins que ce que les tuiles couvrent
  // vraiment — sur Paris, le bâti n'apparaît qu'au zoom 15.
  const maxZoomTuiles = style?.sources?.carte?.maxzoom;
  zoomMax = Math.max(14, maxZoomTuiles || 0);
  const bb = style?.metadata?.["rusty:bounds"];
  vueInitialeGL = {
    center: style.center || [0, 0],
    zoom: style.zoom || 1.6,
    bounds:
      Array.isArray(bb) && bb.length === 4 && bb.every(Number.isFinite)
        ? [[bb[0], bb[1]], [bb[2], bb[3]]]
        : null,
  };
  carte.gl = new maplibregl.Map({
    container: "carte-gl",
    style,
    center: style.center || [0, 0],
    zoom: style.zoom || 1.6,
    minZoom: 0,
    maxZoom: zoomMax,
    // Le canevas est au-dessus et relaie tout : MapLibre ne doit rien écouter.
    interactive: false,
    // ODbL : sur un plan de ville réel (`crates/osm`), les tuiles viennent
    // d'OpenStreetMap et l'attribution est obligatoire. Sur le monde
    // fictif, l'attribution par défaut ne dirait rien de faux — elle reste
    // affichée plutôt que testée au cas par cas ici.
    attributionControl: { compact: true, customAttribution: "© les contributeurs OpenStreetMap" },
    // Une carte inventée n'a ni nord ni horizon.
    renderWorldCopies: false,
    fadeDuration: 120,
    localIdeographFontFamily: "'Hiragino Sans', 'Noto Sans CJK JP', sans-serif",
  });
  carte.gl.on("load", () => {
    carte.glPret = true;
    journalCarte(
      "tuiles chargées : " + carte.gl.getStyle().layers.length + " couches, zoom " +
      carte.gl.getZoom().toFixed(2),
    );
    majCouleurGL();
    majFiltreGL();
    majPentesGL();
    majAffichageGL();
    poserVignetteCarte(carte.gl.getStyle());
    dessinerCarte();
  });
  carte.gl.on("zoom", majVignetteZoom);
  carte.gl.on("render", () => {
    synchroniserVue();
    dessinerSurcouche();
  });
  carte.gl.on("error", (e) => {
    const m = e && e.error ? e.error.message || String(e.error) : "";
    if (/204|empty|Not Found/i.test(m)) return; // tuile absente : ordinaire
    journalCarte(m, "warn");
  });
}

/// Combien d'unités de carte tient un pixel, à la vue courante.
function uniteParPixel() {
  const a = carte.gl.unproject([0, 0]);
  const b = carte.gl.unproject([1, 0]);
  return Math.abs(carteDepuisGeo(b.lng, b.lat)[0] - carteDepuisGeo(a.lng, a.lat)[0]);
}

/// Recopie la vue de MapLibre dans `carte.vue`.
///
/// Le reste du fichier lit `carte.vue.k` pour dimensionner ses points, ses
/// libellés et ses seuils de pointage. Plutôt que de traquer ces usages, on
/// tient `k` à jour : c'est le facteur qui, multiplié par `echelle(r)`, donne
/// les pixels par unité de carte — exactement ce qu'il valait avant.
function synchroniserVue() {
  if (!carteGL()) return;
  const r = cnv.getBoundingClientRect();
  const c = echelle(r);
  if (c <= 0) return;
  carte.vue.k = 1 / (uniteParPixel() * c);
  const z = $("zoom-val");
  if (z) z.textContent = `×${carte.vue.k.toFixed(1).replace(".", ",")}`;
}

/// Colorer par famille, année, tempo ou énergie : une expression de style sur
/// la couche des morceaux, au lieu d'un redessin complet du nuage.
/// Bascule entre les deux visualisations.
///
/// Le conteneur des tuiles se cache quand le nuage reprend la main : le
/// canevas redessine alors tout lui-même, avec son propre repère.
function majAffichageGL() {
  const conteneur = $("carte-gl");
  const enCarte = carte.affichage === "carte";
  if (conteneur) conteneur.hidden = !enCarte;
  // Le fond de plan ne concerne que les tuiles MapLibre : sur le nuage de
  // points, le canevas dessine son propre repère et ce choix n'a aucun effet.
  const blocFond = $("bloc-fond");
  if (blocFond) blocFond.hidden = !enCarte;
  majVisibiliteIntervalleAnnees();
  majSegmentsCouleur();
  if (enCarte && !carte.gl) {
    initialiserGL()
      .then(() => {
        majCouleurGL();
        majFiltreGL();
        dessinerCarte();
      })
      .catch((e) => journalCarte("tuiles indisponibles : " + e, "warn"));
    return;
  }
  if (enCarte && carte.gl) carte.gl.resize();
  const vignette = $("carte-vignette");
  if (vignette) vignette.style.display = enCarte ? "" : "none";
  dessinerCarte();
}

/// Les pentes entre quartiers (`style.rs`, `pentes_reelles`) : masquées dans le
/// style, montrées par l'option « Pentes ». L'option n'apparaît que si le
/// style en porte (plan de ville réel, empreintes calculées). À rejouer après
/// `setStyle`, comme la couleur.
const COUCHES_PENTES = ["pentes-lisere", "pentes-courbe", "pentes-tete"];
function majPentesGL() {
  const presentes = !!carte.gl?.getLayer("pentes-courbe");
  const ligne = $("ligne-pentes");
  if (ligne) ligne.hidden = !presentes;
  const caseAcocher = $("carte-pentes");
  if (caseAcocher) caseAcocher.checked = carte.pentes;
  if (!presentes) return;
  for (const couche of COUCHES_PENTES)
    if (carte.gl.getLayer(couche))
      carte.gl.setLayoutProperty(couche, "visibility", carte.pentes ? "visible" : "none");
}

/// `#RRGGBB` → `"r, g, b"`, ou `null`. Pour bâtir `rgb()`/`rgba()`.
function hexVersRgb(hex) {
  const m = /^#([0-9a-fA-F]{6})$/.exec(hex || "");
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return `${n >> 16 & 255}, ${n >> 8 & 255}, ${n & 255}`;
}

/// Le fondu de bordure façon maptoposter, **sur le plan de ville réel** : le
/// réseau se dissout dans le fond près des bords, au lieu de s'arrêter net sur
/// le périphérique (le halo de petite couronne lui donne de la matière). Teinte
/// = le fond du thème chargé ; force décroissante au zoom (`majVignetteZoom`).
function poserVignetteCarte(style) {
  const el = $("carte-vignette");
  if (!el) return;
  const couches = style?.layers || [];
  const couche = couches.find((l) => l.type === "background");
  const hex = couche && couche.id === "terre-reelle" && couche.paint?.["background-color"];
  const rgb = hexVersRgb(hex);
  if (!rgb) {
    el.style.background = "none";
    el.dataset.actif = "";
    return;
  }
  // Rampe linéaire, comme `create_gradient_fade` de maptoposter : la couleur du
  // fond, opaque au bord, transparente vers le centre. On interpole vers cette
  // même couleur à alpha 0 (`rgba(r,g,b,0)`), **jamais** vers `transparent` —
  // qui vaut `rgba(0,0,0,0)` et fait virer le milieu du dégradé au gris/noir.
  const plein = `rgb(${rgb})`, vide = `rgba(${rgb},0)`;
  const h = 30, c = 17; // % de fondu, vertical / horizontal
  el.style.background =
    `linear-gradient(to bottom, ${plein}, ${vide} ${h}%, ${vide} ${100 - h}%, ${plein}),` +
    `linear-gradient(to right, ${plein}, ${vide} ${c}%, ${vide} ${100 - c}%, ${plein})`;
  el.dataset.actif = "1";
  majVignetteZoom();
}

/// Opacité du fondu : pleine sous z13 (vue d'ensemble, esthétique poster),
/// éteinte à z16 (échelle de la façade, où un voile gênerait la lecture).
function majVignetteZoom() {
  const el = $("carte-vignette");
  if (!el || el.dataset.actif !== "1" || !carte.gl) return;
  el.style.opacity = String(Math.max(0, Math.min(1, (16 - carte.gl.getZoom()) / 3)));
}

function majCouleurGL() {
  if (!carte.gl || !carte.glPret) return;
  // Le plan de ville réel n'a pas de point de morceau (`style::couches`,
  // depuis que le bâtiment habité porte le morceau) — l'un des deux
  // existe selon le chemin, jamais les deux.
  const pointMorceaux = carte.gl.getLayer("morceaux-point");
  const batimentsMorceaux = carte.gl.getLayer("batiments-morceaux");
  if (!pointMorceaux && !batimentsMorceaux) return;
  const teintes = couleursFamillesCarte();
  let expr;
  if (carte.couleur === "famille") {
    const m = ["match", ["get", "famille"]];
    teintes.forEach((t, i) => m.push(i, t));
    m.push(autresCarte());
    expr = m;
  } else {
    const { champ } = CONTINUES[carte.couleur] ?? {};
    const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 1];
    const etapes = rampe();
    const i = ["interpolate", ["linear"], ["coalesce", ["get", champ], v0]];
    etapes.forEach((t, n) => i.push(v0 + ((v1 - v0) * n) / (etapes.length - 1), t));
    expr = i;
  }
  if (pointMorceaux) carte.gl.setPaintProperty("morceaux-point", "circle-color", expr);
  // Le bâtiment habité porte les attributs de son occupant (`palier` = famille,
  // `annee`, `bpm`, `energie` — `tuiles::Anneau`) : il se colore par famille
  // comme par année, tempo ou énergie. Passe par le même constructeur que
  // `majFiltreGL` : la coloration du bâti, l'isolement d'une famille et
  // l'intervalle d'années doivent rester cohérents.
  poserCouleurHabites();
  // Les aplats de quartier sont un lavis **de famille** : sous une coloration
  // continue (année, tempo, énergie) ils mentiraient sur la couleur de la
  // ville — la couleur vient alors des bâtiments et de leurs points.
  for (const couche of ["territoires", "territoires-reels", "territoires-reels-contour"]) {
    if (carte.gl.getLayer(couche))
      carte.gl.setLayoutProperty(couche, "visibility", carte.couleur === "famille" ? "visible" : "none");
  }
}

/// Gris du bâti — la même valeur que `BATI` dans `crates/carto/src/style.rs`.
/// Un bâtiment d'une famille non isolée y revient (choix utilisateur : la
/// famille mise en avant ressort, la trame de la ville reste lisible autour).
const GRIS_BATI = "#DEDAD2";

/// Couches que l'isolement de familles (`carte.isolees`) masque par un
/// **filtre**, avec le champ MVT qui porte la famille sur chacune (la plupart
/// `famille`, le bâti et les territoires réels la portent dans `palier`).
///
/// Un filtre, pas une opacité : la plupart de ces couches ont une opacité
/// **interpolée sur le zoom** dans le style, et MapLibre n'autorise `["zoom"]`
/// qu'au sommet d'un `interpolate`/`step` — la multiplier par un `["case"]`
/// (l'ancienne approche) produisait une expression invalide que
/// `setPaintProperty` rejetait en silence, d'où « cliquer une famille ne fait
/// rien ». `setFilter` n'a pas cette contrainte et masque proprement (pas de
/// collision d'étiquette fantôme non plus). Le bâti habité est à part : il
/// vire au gris (`couleurBatimentsMorceaux`) plutôt que de disparaître.
const CIBLES_FILTRE_GL = [
  ["territoires", "famille"],
  ["territoires-reels", "palier"],
  ["territoires-reels-contour", "palier"],
  ["morceaux-point", "famille"],
  ["morceaux-etiquette", "famille"],
  ["artistes-point", "famille"],
  ["artistes-etiquette", "famille"],
  ["albums-point", "famille"],
  ["albums-etiquette", "famille"],
  ["batiments-morceaux-bord", "palier"],
];

/// Parmi les couches ci-dessus, celles qui répondent en plus à l'intervalle
/// d'années (`filtreAnnee`) — un artiste (et le monument où il est ancré, le
/// même point) hors de l'intervalle de son premier morceau, et le bord du
/// bâti habité qui doit disparaître avec lui (le remplissage, lui, vire au
/// gris par la peinture — voir `couleurBatimentsMorceaux`). Le reste
/// (territoires, morceaux, albums) est de la géographie ou hors du périmètre
/// tranché pour ce chantier — `docs/carto-ville.md` : territoires, voirie,
/// hydrographie, espaces verts n'en dépendent jamais.
const CIBLES_FILTRE_ANNEE_GL = ["artistes-point", "artistes-etiquette", "batiments-morceaux-bord"];

/// L'expression MapLibre « cette entité est hors de l'intervalle », ou
/// `null` si l'intervalle n'est pas resserré (rien à exclure). Une entité
/// sans année (`coalesce` retombe alors sur `debut`, qui est toujours dans
/// l'intervalle puisque `debut <= fin`) ne peut jamais être hors intervalle —
/// même règle que `dansIntervalleAnnee` côté canevas.
function clauseAnneeHorsIntervalle() {
  if (!intervalleAnneeActif()) return null;
  const v = ["coalesce", ["get", "annee"], filtreAnnee.debut];
  return ["any", ["<", v, filtreAnnee.debut], [">", v, filtreAnnee.fin]];
}

/// Pose la couleur du bâti habité **et** celle de son point de dézoom
/// (`habites-point`, qui relaie le bâtiment tant qu'il est trop petit pour se
/// voir — `crates/carto/src/style.rs::habites_reels`) : la ville garde la
/// coloration choisie (famille, année, tempo, énergie) à tous les zooms.
function poserCouleurHabites() {
  if (!carte.gl || !carte.glPret) return;
  if (carte.gl.getLayer("batiments-morceaux"))
    carte.gl.setPaintProperty("batiments-morceaux", "fill-color", couleurBatimentsMorceaux("palier"));
  if (carte.gl.getLayer("habites-point"))
    carte.gl.setPaintProperty("habites-point", "circle-color", couleurBatimentsMorceaux("famille"));
}

/// Le bâti habité coloré par famille : la teinte de l'occupant (champ `palier`).
function couleurFamilleBatiment(gris, champ) {
  const m = ["match", ["get", champ]];
  couleursFamillesCarte().forEach((t, i) => m.push(i, t));
  m.push(gris);
  return m;
}

/// Champ des tuiles qui porte, sur un bâtiment, la variable continue de
/// `CONTINUES` (`champ` y désigne le champ du *morceau* : `year`, `bpm`,
/// `energy`).
const CHAMP_TUILE_BATIMENT = { annee: "annee", tempo: "bpm", energie: "energie" };

/// Le bâti habité coloré sur la rampe de la variable continue active, avec les
/// mêmes bornes (`carte.bornes`) et la même rampe que les points du nuage. Un
/// occupant sans valeur (année inconnue = -1, tag de tempo/énergie absent)
/// prend la teinte neutre « autres », jamais une extrémité de la rampe.
function couleurContinueBatiment() {
  const champ = CHAMP_TUILE_BATIMENT[carte.couleur];
  const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 1];
  if (!champ || !(v1 > v0)) return autresCarte();
  const valeur = ["coalesce", ["get", champ], -1];
  const etapes = rampe();
  const rampeExpr = ["interpolate", ["linear"], valeur];
  etapes.forEach((t, n) => rampeExpr.push(v0 + ((v1 - v0) * n) / (etapes.length - 1), t));
  // `-1` marque l'absence de valeur : aucune année, tempo ou énergie réels n'y
  // tombent (`valide` écarte les années ≤ 0 des bornes).
  return ["case", ["<", valeur, 0], autresCarte(), rampeExpr];
}

/// Couleur de remplissage du bâti habité (`batiments-morceaux`) : la teinte
/// de la famille de l'occupant (champ `palier`), sauf quand une famille est
/// isolée — les autres reviennent alors au gris du bâti vacant — ou quand
/// l'année de l'occupant tombe hors de l'intervalle choisi
/// (`filtreAnnee`) : le bâtiment retombe alors dans le même gris, exactement
/// le style « vacant » déjà en place, pas une teinte inventée pour l'occasion.
///
/// Un bâtiment n'a pas d'année propre ; c'est celle du morceau qui l'habite
/// (`crates/carto/src/tuiles.rs`, tag `annee`).
///
/// `champFamille` : `palier` sur le bâtiment, `famille` sur son point de
/// dézoom (couche `habites`, voir `poserCouleurHabites`) — deux tuiles, même
/// coloration.
function couleurBatimentsMorceaux(champFamille = "palier") {
  const gris = grisBatiCarte();
  const parPalier =
    carte.couleur === "famille" ? couleurFamilleBatiment(gris, champFamille) : couleurContinueBatiment();
  const conditions = [];
  if (carte.isolees.size > 0)
    conditions.push(["!", ["in", ["get", champFamille], ["literal", [...carte.isolees]]]]);
  const hors = clauseAnneeHorsIntervalle();
  if (hors) conditions.push(hors);
  if (!conditions.length) return parPalier;
  return ["case", conditions.length === 1 ? conditions[0] : ["any", ...conditions], gris, parPalier];
}

/// Filtre de chaque couche tel que `style::construire` l'a posé, capturé au
/// premier appel pour pouvoir le rétablir sans le reconstruire côté JS.
const filtreOriginal = new Map();
function filtreBase(layer) {
  if (!filtreOriginal.has(layer)) {
    filtreOriginal.set(layer, carte.gl.getFilter(layer) ?? null);
  }
  return filtreOriginal.get(layer);
}

/// Isole une famille sur les tuiles MapLibre — le pendant, en mode Carte, de
/// ce que `dessinerCarte()` fait sur le canevas 2D du nuage. Les autres
/// familles disparaissent (territoires, points et étiquettes) ; leur bâti
/// habité vire au gris.
function majFiltreGL() {
  if (!carte.gl || !carte.glPret) return;

  poserCouleurHabites();

  const hors = clauseAnneeHorsIntervalle();
  const dansIntervalle = hors && ["!", hors];
  for (const [layer, champ] of CIBLES_FILTRE_GL) {
    if (!carte.gl.getLayer(layer)) continue;
    const base = filtreBase(layer);
    const clauses = base ? [base] : [];
    if (carte.isolees.size > 0)
      clauses.push(["in", ["get", champ], ["literal", [...carte.isolees]]]);
    if (dansIntervalle && CIBLES_FILTRE_ANNEE_GL.includes(layer)) clauses.push(dansIntervalle);
    carte.gl.setFilter(
      layer,
      clauses.length === 0 ? null : clauses.length === 1 ? clauses[0] : ["all", ...clauses],
    );
  }
}

/// Montre ou cache le bloc d'intervalle : mode Explorer (les quatre
/// visualisations, pas seulement Carte) et au moins deux années distinctes à
/// délimiter.
function majVisibiliteIntervalleAnnees() {
  const bloc = $("bloc-intervalle-annees");
  if (bloc) bloc.hidden = !(navigation.mode === "explorer" && filtreAnnee.min != null);
}

/// Bornes de l'intervalle : les mêmes que celles déjà calculées pour la
/// rampe « Colorer par : Année » (`carte.bornes.annee`, voir `chargerCarte`)
/// — pas une seconde lecture de `carte.points`. Ouvre sur l'intervalle
/// complet : rien n'est filtré tant qu'on n'a pas resserré.
function initialiserFiltreAnnee() {
  const [min, max] = carte.bornes.annee ?? [0, 0];
  if (!(min < max)) {
    filtreAnnee.min = filtreAnnee.max = filtreAnnee.debut = filtreAnnee.fin = null;
    majVisibiliteIntervalleAnnees();
    return;
  }
  filtreAnnee.min = min;
  filtreAnnee.max = max;
  filtreAnnee.debut = min;
  filtreAnnee.fin = max;
  const de = $("annee-debut");
  const a = $("annee-fin");
  if (de) { de.min = String(min); de.max = String(max); de.value = String(min); }
  if (a) { a.min = String(min); a.max = String(max); a.value = String(max); }
  majEtiquetteIntervalleAnnees();
  majVisibiliteIntervalleAnnees();
}

function majEtiquetteIntervalleAnnees() {
  const sd = $("annee-debut-val");
  const sf = $("annee-fin-val");
  if (sd) sd.textContent = filtreAnnee.debut == null ? "—" : String(filtreAnnee.debut);
  if (sf) sf.textContent = filtreAnnee.fin == null ? "—" : String(filtreAnnee.fin);
  const reinit = $("annee-reinit");
  if (reinit) reinit.hidden = !intervalleAnneeActif();
}

/// Répercute un changement d'intervalle sur la visualisation active — la
/// carte MapLibre (tuiles déjà construites, un filtre/une couleur suffit) ou
/// le canevas (Nuage/Temps/Anneau, un simple redessin : `retenu`/
/// `dansIntervalleAnnee` font le tri à chaque image).
function rafraichirFiltreAnnee() {
  majFiltreGL();
  dessinerCarte();
}

function dimensionner() {
  const r = cnv.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  cnv.width = Math.max(1, Math.round(r.width * dpr));
  cnv.height = Math.max(1, Math.round(r.height * dpr));
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return r;
}

/// Demi-côté utile du canevas : le nuage vit en [-1, 1], on lui laisse une
/// marge de 28 px pour que les points de bord ne collent pas au cadre.
function echelle(r) {
  return Math.min(r.width, r.height) / 2 - 28;
}

function versEcran(p, r) {
  const g = carteGL();
  if (g) {
    // Sur le plan de ville réel, l'adresse (si ce morceau en a une) prime
    // sur la position t-SNE — c'est elle que les tuiles montrent.
    const [x, y] = (carte.villeReelle && carte.positionsReelles.get(p.id)) || [p.x, p.y];
    const q = g.project(geoDepuisCarte(x, y));
    return [q.x, q.y];
  }
  // Le nuage, lui, reste toujours en t-SNE — `p.x`/`p.y` n'ont jamais été
  // touchés, que la ville soit active ou non.
  const { k, dx, dy } = carte.vue;
  const c = echelle(r);
  return [r.width / 2 + (p.x * c) * k + dx, r.height / 2 + (p.y * c) * k + dy];
}

/// L'inverse de `versEcran` : du pixel vers le repère du nuage. Le dessin en a
/// besoin — c'est le seul endroit où l'on part de l'écran pour aller vers les
/// données, et non l'inverse.
function versCarte(mx, my, r) {
  const g = carteGL();
  if (g) {
    const p = g.unproject([mx, my]);
    return carteDepuisGeo(p.lng, p.lat);
  }
  const { k, dx, dy } = carte.vue;
  const c = echelle(r) * k;
  return [(mx - r.width / 2 - dx) / c, (my - r.height / 2 - dy) / c];
}

/// Les douze teintes de famille, lues dans la feuille de style.
///
/// Sert au **nuage de points**, à la **légende** et au mode Écoute — tout ce
/// qui suit le thème clair/sombre de l'application. Sur la carte MapLibre,
/// voir `couleursFamillesCarte` : les familles s'y calent sur le fond de plan.
function couleursFamilles() {
  return getComputedStyle(document.documentElement)
    .getPropertyValue("--familles")
    .split(",")
    .map((c) => c.trim())
    .filter(Boolean);
}

/// Palettes de familles **de la carte**, calées sur chaque fond de plan.
/// Miroir de `crates/carto/src/palette.rs` — les deux doivent nommer une
/// famille de la même couleur (comme `--familles` ↔ `style.rs` auparavant) :
/// Rust les cuit dans `style-<id>.json`, le JS les rejoue après `setStyle`
/// pour rester cohérent avec l'isolement d'une famille (`majFiltreGL`).
const FAMILLES_VIVES = [
  "#EF8891", "#EC9066", "#D99E46", "#B7AF47", "#88BC6A", "#4EC497",
  "#0CC3C3", "#38BBE6", "#73AEF8", "#A39FF6", "#C892E1", "#E289BD",
];
const FAMILLES_CARTE = {
  sepia: {
    familles: [
      "#B24B58", "#B05323", "#9E6300", "#7E7400", "#4C8227", "#00895D",
      "#00888A", "#0080AC", "#3472BE", "#6B63BC", "#8F56A7", "#A74D83",
    ],
    autres: "#8A7A60",
    bati: "#EADBC4",
  },
  encre: {
    familles: [
      "#B06A70", "#AE7455", "#A08346", "#7C8347", "#579156", "#2E9480",
      "#33908F", "#4C86A0", "#6981AC", "#7D74A6", "#94709E", "#B06C94",
    ],
    autres: "#8C867A",
    bati: "#ECE6DB",
  },
  nuit: { familles: FAMILLES_VIVES, autres: "#8C8C90", bati: "#333333" },
  "bleu-plan": {
    familles: [
      "#E6A6AD", "#E3AB8A", "#D4B47C", "#BCC088", "#9CCA94", "#74CDB4",
      "#6CC9D2", "#86C6E2", "#A2C8F5", "#C0BEF5", "#D6B0E7", "#E6ADD0",
    ],
    autres: "#7C93AE",
    bati: "#234870",
  },
};

/// Le jeu de familles pour la carte : celui du fond de plan actif, ou celui de
/// la feuille de style (`osm-clair`, qui suit le thème de l'appli).
function couleursFamillesCarte() {
  return FAMILLES_CARTE[carte.theme]?.familles ?? couleursFamilles();
}
/// Le gris « fourre-tout » (`#6E6656` par défaut) accordé au fond de plan.
function autresCarte() {
  return FAMILLES_CARTE[carte.theme]?.autres ?? "#6E6656";
}
/// Le gris du bâti (vacant, ou d'une famille non isolée) accordé au fond.
function grisBatiCarte() {
  return FAMILLES_CARTE[carte.theme]?.bati ?? GRIS_BATI;
}

/// Pastilles pré-dessinées, une par couleur.
///
/// 27 000 appels à `arc()` par image rendraient le survol poussif. On dessine
/// chaque pastille **une fois** dans un canevas minuscule, puis on la recopie
/// — `drawImage` d'une petite image est bien moins cher qu'un tracé de
/// chemin. Et les recopies se superposent : là où les morceaux s'entassent,
/// l'opacité s'accumule et la densité se voit, ce qu'un carré opaque cachait.
const pastilles = new Map();
function pastille(couleur, rayon, alpha) {
  const cle = `${couleur}|${rayon}|${alpha}`;
  const connue = pastilles.get(cle);
  if (connue) return connue;

  const d = Math.max(2, Math.ceil(rayon * 2) + 2);
  const c = document.createElement("canvas");
  c.width = d;
  c.height = d;
  const g = c.getContext("2d");
  g.globalAlpha = alpha;
  g.fillStyle = couleur;
  g.beginPath();
  g.arc(d / 2, d / 2, rayon, 0, Math.PI * 2);
  g.fill();
  pastilles.set(cle, c);
  return c;
}

/// Le nuage de points ordinaire — fond estompé d'abord, sélection par-dessus,
/// pour qu'elle ne soit jamais recouverte. C'était tout le corps de
/// `dessinerCarte` avant que la densité ne lui donne une alternative.
function dessinerNuage(r) {
  const style = getComputedStyle(document.documentElement);
  const encre = style.getPropertyValue("--txt").trim() || "#EDE8DC";
  const rayon = Math.max(1.1, 1.9 * Math.sqrt(carte.vue.k));
  const etapes = rampe();
  const teintes = couleursFamilles();
  const continu = CONTINUES[carte.couleur];
  const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 0];
  const neutre = carte.isolees.size === 0 && !carte.filtre;

  for (const avant of [false, true]) {
    for (const p of carte.points) {
      const vise = (carte.isolees.size === 0 || carte.isolees.has(p.cluster)) && retenu(p);
      if (vise !== avant) continue;

      const [x, y] = versEcran(p, r);
      if (x < -8 || y < -8 || x > r.width + 8 || y > r.height + 8) continue;

      let couleur;
      if (continu) {
        const v = p[continu.champ];
        couleur = v != null && v1 > v0 ? surRampe(etapes, (v - v0) / (v1 - v0)) : encre;
      } else {
        couleur = teintes[p.cluster % teintes.length] ?? encre;
      }
      // L'opacité fait double emploi : elle écarte ce qui est filtré, et elle
      // révèle la densité par superposition.
      const alpha = avant ? (neutre ? 0.62 : 0.95) : 0.07;
      const rr = avant && !neutre ? rayon * 1.5 : rayon;
      const img = pastille(couleur, rr, alpha);
      ctx.drawImage(img, x - img.width / 2, y - img.height / 2);
    }
  }
}

// Les tuiles vectorielles remplacent le nuage et la nappe de densité
// dessinés à la main (§ plus haut, `regenerer-tuiles`) : le pavage par
// famille et la coloration continue s'engendrent désormais côté Rust
// (`crates/core::density`, `crates/carto`) et arrivent déjà peints dans les
// tuiles que MapLibre affiche. Les helpers de couleur qui suivent
// (`hexRGB`/`hexHSL`/`hslRGB`) restent utilisés ailleurs (`desature`, mode
// Explorer → Anneau), sans rapport avec la densité.

function hexRGB(hex) {
  const n = parseInt((hex || "#9A9284").replace("#", ""), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/// Teinte → HSL, pour faire varier la clarté et la saturation avec la
/// densité plutôt que la seule opacité.
function hexHSL(hex) {
  const [r, g, b] = hexRGB(hex).map((v) => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0, s = 0;
  const d = max - min;
  if (d > 1e-6) {
    s = d / (1 - Math.abs(2 * l - 1));
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return [h, s, l];
}

function hslRGB(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}


/// La frise des filiations du mode Explorer → Temps : un point par album sur
/// un axe chronologique, réparti en bandes de familles dont l'ordre minimise
/// la distance verticale totale des arcs inter-familles — voir l'objet
/// `temps`, `ordonnerBandes`, et `docs/frise-filiations.md`. Remplace
/// l'ancien streamgraph (bandes empilées par bucket temporel), jugé
/// insatisfaisant — voir `docs/carto-anneau.md`.
function dessinerTemps(r, encre, accent) {
  if (!temps.charge) {
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.6;
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Chargement de la frise…", r.width / 2, r.height / 2);
    ctx.globalAlpha = 1;
    return;
  }
  if (temps.albums.length === 0 || temps.minAnnee == null) {
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.6;
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Aucune date exploitable pour l'instant.", r.width / 2, r.height / 2);
    ctx.globalAlpha = 1;
    return;
  }

  const teintes = couleursFamilles();
  const noms = new Map(carte.familles.map(([c, nom]) => [c, nom]));
  const nBandes = Math.max(1, temps.bandes.length);
  const aIncertain = temps.aIncertain;
  const largeurIncertain = aIncertain ? 34 : 0;
  const marge = { haut: 10, bas: 28, gauche: aIncertain ? 20 + largeurIncertain + 20 : 20, droite: 92 };
  const xDeb = marge.gauche;
  const xFin = r.width - marge.droite;
  const yHaut = marge.haut;
  const yBas = r.height - marge.bas;
  const hauteur = Math.max(1, yBas - yHaut);

  // Zoom vertical (même `temps.vue.k` que l'horizontal, § plus haut) : même
  // reclampage que le zoom horizontal ci-dessous, appliqué à la hauteur
  // totale des bandes plutôt qu'à la largeur du temps. `hauteurEchelle` est
  // ce que l'accordéon (juste après) répartit entre les bandes — étendre une
  // bande puis zoomer dedans composent naturellement, l'un donne un point de
  // départ, l'autre permet d'aller y regarder de près.
  const hauteurEchelle = hauteur * temps.vue.k;
  temps.vue.dy =
    hauteurEchelle <= hauteur
      ? (hauteur - hauteurEchelle) / 2
      : Math.min(0, Math.max(hauteur - hauteurEchelle, temps.vue.dy));

  // Bande étendue (§ 3, accordéon depuis la légende — `temps.bandeEtendue`,
  // basculé par `rendreFamilles`) : cette bande seule prend `FRACTION_ETENDUE`
  // de la hauteur (zoomée), les autres se partagent le reste à parts égales.
  const FRACTION_ETENDUE = 0.55;
  const iEtendue =
    temps.bandeEtendue != null ? temps.posFamille.get(temps.bandeEtendue) : undefined;
  const hauteursBandes = new Float64Array(nBandes);
  if (iEtendue !== undefined) {
    const reste = (hauteurEchelle * (1 - FRACTION_ETENDUE)) / Math.max(1, nBandes - 1);
    for (let i = 0; i < nBandes; i++) hauteursBandes[i] = i === iEtendue ? hauteurEchelle * FRACTION_ETENDUE : reste;
  } else {
    hauteursBandes.fill(hauteurEchelle / nBandes);
  }
  const y0Bandes = new Float64Array(nBandes);
  {
    let cumul = yHaut + temps.vue.dy;
    for (let i = 0; i < nBandes; i++) {
      y0Bandes[i] = cumul;
      cumul += hauteursBandes[i];
    }
  }

  const minA = temps.minAnnee;
  const maxA = temps.maxAnnee;
  const nAnnees = Math.max(1, maxA - minA);
  const xPasBase = (xFin - xDeb) / nAnnees; // pixels par année, à k = 1

  // Reclampe le décalage à chaque image, pas seulement au moment du geste :
  // un redimensionnement de la fenêtre ou un changement de l'intervalle
  // connu (nouvelle analyse) doit lui aussi retomber sur une vue sans vide.
  const xPas = xPasBase * temps.vue.k;
  const largeurDisponible = xFin - xDeb;
  const largeurTotale = nAnnees * xPas;
  temps.vue.dx =
    largeurTotale <= largeurDisponible
      ? (largeurDisponible - largeurTotale) / 2
      : Math.min(0, Math.max(largeurDisponible - largeurTotale, temps.vue.dx));
  temps.disposition = { xDeb, xFin, xPasBase, n: nAnnees + 1, yHaut, hauteur };

  const xi = 20; // colonne « incertain »
  const xDeAnnee = (annee) => xDeb + (annee - minA) * xPas + temps.vue.dx;
  const xDeAlbum = (a) => (a.annee == null ? xi + largeurIncertain / 2 : xDeAnnee(a.annee));

  // Jitter vertical déterministe (empreinte de l'id) — évite que deux albums
  // de la même année et de la même famille se superposent exactement, sans
  // jamais dépendre de qui d'autre est affiché à côté : la position d'un
  // album ne bouge jamais d'une session à l'autre (stabilité, § 1).
  const jitter = (id) => {
    let h = 2166136261;
    for (let i = 0; i < id.length; i++) {
      h ^= id.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return ((h >>> 0) / 4294967295) * 2 - 1; // [-1, 1]
  };
  const yDeAlbum = (a) => {
    const pos = temps.posFamille.get(a.famille) ?? 0;
    const h = hauteursBandes[pos] ?? hauteur / nBandes;
    const centre = y0Bandes[pos] + h / 2;
    return centre + jitter(a.id) * h * 0.32;
  };

  // Bandes de familles — jamais redistribuées par l'affichage lui-même (seule
  // l'accordéon de la légende change leur hauteur, § 3) : un fond ténu pour
  // situer les groupes, un séparateur entre chacune, son nom en bordure
  // droite (hors zone de zoom horizontal, l'axe des bandes ne bouge jamais).
  temps.bandes.forEach((f, i) => {
    const estompee = carte.isolees.size > 0 && !carte.isolees.has(f);
    ctx.fillStyle = teintes[f % teintes.length] ?? accent;
    ctx.globalAlpha = estompee ? 0.02 : 0.06;
    ctx.fillRect(xDeb - 4, y0Bandes[i], xFin - xDeb + 8, hauteursBandes[i]);
  });
  ctx.globalAlpha = 1;
  ctx.strokeStyle = encre;
  ctx.lineWidth = 1;
  for (let i = 0; i <= nBandes; i++) {
    const y = i < nBandes ? y0Bandes[i] : yBas;
    ctx.globalAlpha = 0.08;
    ctx.beginPath();
    ctx.moveTo(xDeb - 4, y);
    ctx.lineTo(xFin + 4, y);
    ctx.stroke();
  }
  ctx.textAlign = "left";
  ctx.font = "12px system-ui"; // Règle 11 : rien sous 12 px, canevas compris
  temps.bandes.forEach((f, i) => {
    const estompee = carte.isolees.size > 0 && !carte.isolees.has(f);
    ctx.fillStyle = teintes[f % teintes.length] ?? encre;
    ctx.globalAlpha = estompee ? 0.25 : 0.85;
    ctx.fillText(noms.get(f) || `famille ${f + 1}`, xFin + 8, y0Bandes[i] + hauteursBandes[i] / 2 + 4);
  });
  ctx.globalAlpha = 1;

  // Graduation temporelle : trait fin tous les 6 mois, trait épais tous les
  // 10 ans — l'année ne s'imprime jamais en continu, seulement au survol
  // (voir `survolerTemps`), § 5.
  const anneeDeb = Math.floor(minA) - 1;
  const anneeFin = Math.ceil(maxA) + 1;
  ctx.strokeStyle = encre;
  for (let demi = anneeDeb * 2; demi <= anneeFin * 2; demi++) {
    const annee = demi / 2;
    const x = xDeAnnee(annee);
    if (x < xDeb - 2 || x > xFin + 2) continue;
    const decennie = Number.isInteger(annee) && annee % 10 === 0;
    ctx.lineWidth = decennie ? 1.5 : 1;
    ctx.globalAlpha = decennie ? 0.35 : 0.12;
    ctx.beginPath();
    ctx.moveTo(x, yBas);
    ctx.lineTo(x, yBas + (decennie ? 8 : 4));
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // L'intervalle d'années (`filtreAnnee`) : hors intervalle, un voile plutôt
  // qu'une disparition (même règle que l'isolement d'une famille).
  if (intervalleAnneeActif()) {
    const xGauche = xDeAnnee(filtreAnnee.debut);
    const xDroite = xDeAnnee(filtreAnnee.fin);
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#14120E";
    ctx.globalAlpha = 0.65;
    if (xGauche > xDeb) ctx.fillRect(xDeb, yHaut - 4, Math.min(xGauche, xFin) - xDeb, hauteur + 8);
    if (xDroite < xFin) ctx.fillRect(Math.max(xDroite, xDeb), yHaut - 4, xFin - Math.max(xDroite, xDeb), hauteur + 8);
    ctx.globalAlpha = 1;
  }

  // La colonne « dates incertaines » — ces albums n'ont pas de place légitime
  // sur l'axe du temps, mieux vaut le dire que les répartir au hasard dans
  // une année (même traitement que l'ancien streamgraph).
  if (aIncertain) {
    ctx.globalAlpha = 0.25;
    ctx.strokeStyle = encre;
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.moveTo(xi + largeurIncertain + 8, yHaut);
    ctx.lineTo(xi + largeurIncertain + 8, yBas);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.5;
    ctx.font = "12px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("incertain", xi + largeurIncertain / 2, yBas + 14);
    ctx.globalAlpha = 1;
  }

  const parId = new Map(temps.albums.map((a) => [a.id, a]));
  const filtre = (carte.filtre || "").toLowerCase();
  const correspond = (a) =>
    !filtre || a.name.toLowerCase().includes(filtre) || a.artist.toLowerCase().includes(filtre);

  // Fond permanent de liens : même mécanique que l'anneau (`dessinerAnneau`
  // § fond permanent de liens) — un trait ténu et **de force constante** pour
  // tout le réseau, coloré par la famille du premier album de l'arc. Ce n'est
  // que la sélection (plus bas) qui code la force par l'épaisseur/opacité :
  // mélanger les deux langages sur la même couche est ce qui rendait la frise
  // « moins jolie que l'anneau ». Un arc dont l'arrivée correspond à la
  // famille secondaire de l'album cible (hybridation, § 3) ressort à peine —
  // un indice, pas un second langage.
  for (const l of temps.arcs) {
    const a = parId.get(l.a);
    const b = parId.get(l.b);
    if (!a || !b) continue;
    const renforce = b.familleSecondaire != null && b.familleSecondaire === a.famille;
    ctx.strokeStyle = teintes[a.famille % teintes.length] ?? encre;
    ctx.lineWidth = renforce ? 0.9 : 0.6;
    ctx.globalAlpha = renforce ? 0.09 : 0.05;
    ctx.beginPath();
    ctx.moveTo(xDeAlbum(a), yDeAlbum(a));
    ctx.quadraticCurveTo(
      (xDeAlbum(a) + xDeAlbum(b)) / 2,
      (yDeAlbum(a) + yDeAlbum(b)) / 2 - Math.abs(xDeAlbum(b) - xDeAlbum(a)) * 0.08,
      xDeAlbum(b),
      yDeAlbum(b),
    );
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // Sélection : les voisins du focal ressortent par-dessus le fond — même
  // mécanique que l'anneau (`dessinerAnneau` § sélection), le même état
  // partagé (`anneau.focal`/`anneau.voisins`, peuplé par `chargerAnneau` —
  // voir le gestionnaire de clic) plutôt qu'un focal propre à la frise. Les
  // vrais voisins soniques (`album_ring`), pas seulement l'arête éparse du
  // fond permanent : épaisseur et opacité codent la force, couleur = famille
  // du voisin (la provenance), jamais de tiret ni de pointillé.
  const focal = anneau.focal ? parId.get(anneau.focal.id) : null;
  if (focal && anneau.voisins.length) {
    const distances = anneau.voisins.map((v) => v.distance);
    const dMin = Math.min(...distances);
    const dMax = Math.max(...distances);
    const forceDe = (d) => (dMax <= dMin ? 0.5 : 1 - (d - dMin) / (dMax - dMin));
    const xf = xDeAlbum(focal);
    const yf = yDeAlbum(focal);
    for (const v of anneau.voisins) {
      const cible = parId.get(v.id);
      if (!cible) continue;
      const force = forceDe(v.distance);
      const xv = xDeAlbum(cible);
      const yv = yDeAlbum(cible);
      ctx.strokeStyle = teintes[v.famille % teintes.length] ?? accent;
      ctx.globalAlpha = 0.45 + force * 0.55;
      ctx.lineWidth = 1.5 + force * 4;
      ctx.beginPath();
      ctx.moveTo(xf, yf);
      ctx.quadraticCurveTo((xf + xv) / 2, (yf + yv) / 2 - Math.abs(xv - xf) * 0.08, xv, yv);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1;
  }

  // Albums : un point par album, teinté par sa famille maîtresse. Un anneau
  // supplémentaire marque les hubs (§ 6) — un signal explicite dans le rendu
  // du point lui-même, visible même quand ses arcs sont estompés à l'état de
  // base (règle « estomper, jamais masquer », `docs/carto-anneau.md`). Le
  // focal, lui, ignore l'estompage : une sélection reste lisible quel que
  // soit le filtre.
  temps.puces = [];
  for (const a of temps.albums) {
    const x = xDeAlbum(a);
    if (a.annee != null && (x < xDeb - 20 || x > xFin + 20)) continue;
    const y = yDeAlbum(a);
    // Culling vertical, symétrique de l'horizontal ci-dessus — utile dès que
    // le zoom ou l'accordéon poussent une bande hors du cadre visible.
    if (y < yHaut - 20 || y > yBas + 20) continue;
    const estFocal = !!focal && a.id === focal.id;
    const estompee =
      !estFocal &&
      ((carte.isolees.size > 0 && !carte.isolees.has(a.famille)) ||
        !dansIntervalleAnnee(a.annee) ||
        (!!filtre && !correspond(a)));
    const hub = a.sortants >= temps.seuilHub;
    const rayon = (hub ? 3.2 : 2.2) + (estFocal ? 1 : 0);
    ctx.fillStyle = teintes[a.famille % teintes.length] ?? accent;
    ctx.globalAlpha = estompee ? 0.15 : 0.85;
    ctx.beginPath();
    ctx.arc(x, y, rayon, 0, Math.PI * 2);
    ctx.fill();
    if (hub) {
      ctx.strokeStyle = teintes[a.famille % teintes.length] ?? accent;
      ctx.globalAlpha = estompee ? 0.15 : 0.75;
      ctx.lineWidth = 1.2;
      ctx.beginPath();
      ctx.arc(x, y, rayon + 2.5, 0, Math.PI * 2);
      ctx.stroke();
      ctx.lineWidth = 1;
    }
    if (estFocal) {
      ctx.strokeStyle = encre;
      ctx.globalAlpha = 0.9;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(x, y, rayon + (hub ? 5 : 2.5), 0, Math.PI * 2);
      ctx.stroke();
      ctx.lineWidth = 1;
    }
    temps.puces.push({ x, y, album: a });
  }
  ctx.globalAlpha = 1;

  // Pas de tracé de la playlist en cours ici : sur Temps/Anneau, l'affichage
  // ne montre **que** l'album du morceau écouté et ses voisins soniques (le
  // focal, plus haut, piloté par `suivreAlbumEnLecture`). Superposer le
  // trajet de toute la file — qui part de l'album d'origine d'une playlist
  // « alchimie » — ajoutait un second jeu de connexions étranger au disque en
  // cours. `parCle` reste : le halo « album en écoute » juste dessous en a
  // besoin.
  const parCle = new Map(temps.albums.map((a) => [`${a.name} ${a.artist}`, a]));

  // L'album en écoute : même halo que le morceau en écoute sur Nuage/Carte
  // (`surcoucheSur`) — le morceau en cours se ramène à son album par la
  // même clé que `parCle` ci-dessus, `fileAttente.pistes` n'ayant que le nom.
  const pisteEnLecture = fileAttente.pistes.find((x) => x.path === lecture.courant);
  const albumEnLecture =
    pisteEnLecture && parCle.get(`${pisteEnLecture.album || ""} ${pisteEnLecture.artist || ""}`);
  if (albumEnLecture) {
    const x = xDeAlbum(albumEnLecture);
    const y = yDeAlbum(albumEnLecture);
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 9, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 0.35;
    ctx.beginPath();
    ctx.arc(x, y, 14, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1;
  }

  // Le point survolé : un halo, comme sur l'ancien streamgraph.
  if (temps.survole) {
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(temps.survole.x, temps.survole.y, 6, 0, Math.PI * 2);
    ctx.stroke();
    ctx.lineWidth = 1;
  }
}

/// L'anneau du mode Explorer → Anneau : un graphe circulaire à liens groupés
/// centré sur l'album focal (`anneau.focal`) — voir l'objet `anneau` et
/// `Library::album_embeddings`/`album_ring` côté cœur. Inspiré du rendu
/// d'Eigenfactor (Moritz Stefaner) : un anneau extérieur pour les familles,
/// nommées, un second anneau intérieur pour les albums.
///
/// Anneau extérieur = les 12 familles, longueur d'arc ∝ effectif
/// (`carte.familles`, déjà chargé pour la légende du rail), nom affiché en
/// bordure. Anneau intérieur = le focal et ses voisins, répartis sur le
/// sous-arc de leur famille pour ne jamais se chevaucher, chacun un arc dont
/// l'**épaisseur radiale ∝ la durée totale de l'album** (`duree_ms`) —
/// demande explicite : « proche de l'esprit Eigenfactor ». Liens = une
/// courbe quadratique par voisin, tirée vers le centre (simplification à un
/// seul foyer d'un bundling hiérarchique complet : le focal est déjà la
/// racine, pas besoin d'ancêtre commun intermédiaire) dont l'**épaisseur ∝
/// la proximité d'empreinte** (`v.distance`, normalisée parmi les voisins
/// affichés — l'échelle absolue d'une distance CLAP n'a pas de sens en soi).
/// Chemin dans la hiérarchie genre/album entre deux feuilles — voir
/// `docs/carto-anneau.md` § diagnostic. Deux niveaux seulement (racine →
/// famille → album), mais suffisants pour un vrai bundling hiérarchique :
/// deux albums d'une même famille partagent son nœud comme plus proche
/// ancêtre commun (chemin à 3 points), deux albums de familles distinctes ne
/// partagent que la racine (chemin à 5 points, en passant par les deux
/// nœuds de famille). C'est ce partage qui fait converger les liens vers une
/// même famille avant qu'ils ne divergent vers leurs feuilles respectives —
/// pas un simple rayonnement depuis le centre.
function cheminAncetres(posA, familleA, posB, familleB, hubs, racine) {
  if (familleA === familleB) return [posA, hubs.get(familleA), posB];
  return [posA, hubs.get(familleA), racine, hubs.get(familleB), posB];
}

/// Tension de Holten (*Hierarchical Edge Bundles*, 2006) : à `beta = 1`, la
/// courbe suit exactement le chemin par la hiérarchie (`chemin`) ; à `beta =
/// 0`, elle redevient la droite entre les deux feuilles. Chaque point
/// intermédiaire est le mélange des deux, comme `d3.curveBundle.beta` — les
/// extrémités, elles, restent toujours exactement les feuilles. Réglable
/// dans le rail (`anneau.beta`) : l'effet se juge à l'œil, pas par un calcul.
function courbeBundle(chemin, beta) {
  const n = chemin.length;
  if (n < 3) return chemin;
  const [dx, dy] = chemin[0];
  const [fx, fy] = chemin[n - 1];
  return chemin.map((p, i) => {
    if (i === 0 || i === n - 1) return p;
    const t = i / (n - 1);
    const tx = dx + (fx - dx) * t;
    const ty = dy + (fy - dy) * t;
    return [beta * p[0] + (1 - beta) * tx, beta * p[1] + (1 - beta) * ty];
  });
}

/// Une ligne lissée par une courbe quadratique passant par le milieu de
/// chaque paire de points consécutifs — même technique que `dessinerTemps`,
/// pour un faisceau organique plutôt qu'une polyligne à coudes.
function tracerLigneLissee(pts) {
  if (pts.length < 2) return;
  ctx.moveTo(pts[0][0], pts[0][1]);
  for (let i = 1; i < pts.length - 1; i++) {
    const mx = (pts[i][0] + pts[i + 1][0]) / 2;
    const my = (pts[i][1] + pts[i + 1][1]) / 2;
    ctx.quadraticCurveTo(pts[i][0], pts[i][1], mx, my);
  }
  const dernier = pts[pts.length - 1];
  ctx.lineTo(dernier[0], dernier[1]);
}

/// Désature une teinte de famille pour le fond (anneaux), en gardant sa
/// teinte et sa clarté — même principe que la carte topographique
/// (`docs/carto-direction.md`) : la couleur reste rare et porte un sens,
/// elle ne doit pas crier. Les liens, eux, gardent la teinte pleine : c'est
/// ce qu'on est venu regarder, pas le fond.
function desature(hex, facteur = 0.4) {
  const [h, s, l] = hexHSL(hex);
  const [r, g, b] = hslRGB(h, s * facteur, l);
  return `rgb(${r}, ${g}, ${b})`;
}

/// Trait de rappel + étiquette externe pour un voisin du focal sélectionné —
/// révélation par sélection, pas un nom permanent sur chaque segment (voir
/// `docs/carto-anneau.md` § étiquettes). Comme les noms de famille, mais
/// radial depuis la position réelle du nœud plutôt que le milieu d'un arc.
function dessinerRappel(pos, nom, cx, cy, rayonExt, encre) {
  const angle = Math.atan2(pos[1] - cy, pos[0] - cx);
  const bout = [cx + Math.cos(angle) * (rayonExt + 26), cy + Math.sin(angle) * (rayonExt + 26)];
  ctx.strokeStyle = encre;
  ctx.globalAlpha = 0.45;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(pos[0], pos[1]);
  ctx.lineTo(bout[0], bout[1]);
  ctx.stroke();

  const gauche = Math.cos(angle) < 0;
  ctx.save();
  ctx.translate(bout[0], bout[1]);
  ctx.rotate(gauche ? angle + Math.PI : angle);
  ctx.fillStyle = encre;
  ctx.globalAlpha = 0.9;
  ctx.font = "12px system-ui";
  ctx.textAlign = gauche ? "right" : "left";
  ctx.fillText(nom, 0, 0);
  ctx.restore();
  ctx.globalAlpha = 1;
}

function dessinerAnneau(r, encre, accent) {
  // Le fond permanent n'est pas encore revenu de `chargerReseauAnneau` —
  // rien à dessiner tant qu'on n'a pas au moins la couronne des familles.
  if (!anneau.reseau.charge || !carte.familles || carte.familles.length === 0) {
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.6;
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Chargement du réseau…", r.width / 2, r.height / 2);
    ctx.globalAlpha = 1;
    return;
  }

  const teintes = couleursFamilles();
  const cx = r.width / 2 + anneau.vue.dx;
  const cy = r.height / 2 + anneau.vue.dy;
  // Marge plus large qu'un simple anneau : les noms de famille et les
  // étiquettes de rappel débordent au dehors de `rayonExt`. Mise à l'échelle
  // par `anneau.vue.k` : tous les rayons qui en découlent (album, hub) grossissent
  // avec elle, comme un cercle qu'on zoome — les épaisseurs de trait restent
  // fixes, en pixels écran, lisibles à tout niveau de zoom.
  const rayonExt = (Math.min(r.width, r.height) / 2 - 54) * anneau.vue.k;
  const epaisseurFamille = 10;
  const rayonAlbum = rayonExt - epaisseurFamille - 20;
  // Le nœud de famille du bundling hiérarchique — entre l'anneau des albums
  // et le centre, jamais confondu avec `rayonAlbum` (une position de calcul,
  // pas un rayon dessiné).
  const rayonHub = rayonAlbum * 0.45;
  const racine = [cx, cy];
  const EPAISSEUR_ALBUM_MIN = 1;
  const EPAISSEUR_ALBUM_MAX = 9;

  // Couronne des familles — angles stables (`anneau.reseau.arcsFamille`,
  // fixés par `calculerAnglesAnneau`), désaturée : c'est le fond, pas ce
  // qu'on est venu lire (voir `desature`).
  const arcsFamille = anneau.reseau.arcsFamille;
  const hubsFamille = new Map(); // cluster -> [x, y], nœud de la hiérarchie
  ctx.lineWidth = epaisseurFamille;
  ctx.textBaseline = "middle";
  for (const [cluster, nom, n] of carte.familles) {
    const bornes = arcsFamille.get(cluster);
    if (!bornes) continue;
    const { debut, fin } = bornes;
    const milieu = (debut + fin) / 2;
    hubsFamille.set(cluster, [cx + Math.cos(milieu) * rayonHub, cy + Math.sin(milieu) * rayonHub]);

    ctx.strokeStyle = desature(teintes[cluster % teintes.length] ?? accent);
    ctx.beginPath();
    ctx.arc(cx, cy, rayonExt - epaisseurFamille / 2, debut, fin);
    ctx.stroke();

    // Le nom, radial — retourné sur la moitié gauche du cercle pour ne
    // jamais se lire la tête en bas, comme sur un cadran ordinaire. Permanent
    // pour la famille (12 noms, lisibles) — pas pour chaque album, voir plus
    // bas.
    const gauche = Math.cos(milieu) < 0;
    ctx.save();
    ctx.translate(cx + Math.cos(milieu) * (rayonExt + 8), cy + Math.sin(milieu) * (rayonExt + 8));
    ctx.rotate(gauche ? milieu + Math.PI : milieu);
    ctx.fillStyle = encre;
    ctx.globalAlpha = 0.8;
    ctx.font = "12px system-ui";
    ctx.textAlign = gauche ? "right" : "left";
    ctx.fillText(nom || `famille ${cluster + 1}`, 0, 0);
    ctx.restore();
  }
  ctx.lineWidth = 1;
  ctx.globalAlpha = 1;

  // Position de chaque album — stable, jamais recalculée au changement de
  // focal (voir `docs/carto-anneau.md` § stabilité des positions) : c'est
  // l'angle mémorisé par `calculerAnglesAnneau` qui est fixe, seuls `cx`/`cy`/
  // les rayons (dépendants de la taille du canevas) sont recalculés ici.
  const positions = new Map();
  const parAlbum = new Map(anneau.reseau.albums.map((a) => [a.id, a]));
  for (const a of anneau.reseau.albums) {
    const angle = anneau.reseau.angleAlbum.get(a.id);
    if (angle === undefined) continue;
    positions.set(a.id, [cx + Math.cos(angle) * rayonAlbum, cy + Math.sin(angle) * rayonAlbum]);
  }

  // La largeur angulaire d'un album, par famille — même division que
  // `calculerAnglesAnneau`, recalculée ici seulement pour savoir combien de
  // place laisser entre deux arcs voisins (le « seam »).
  const comptesFamille = new Map();
  for (const a of anneau.reseau.albums) {
    if (a.famille == null || !arcsFamille.has(a.famille)) continue;
    comptesFamille.set(a.famille, (comptesFamille.get(a.famille) || 0) + 1);
  }
  const pasParFamille = new Map();
  for (const [cluster, n] of comptesFamille) {
    const { debut, fin } = arcsFamille.get(cluster);
    pasParFamille.set(cluster, (fin - debut) / n);
  }

  // Anneau intérieur : **chaque album connu**, en trait fin et désaturé —
  // c'est la texture tissée permanente qui manquait (voir
  // `docs/carto-anneau.md` § pourquoi ça manque de richesse). Le focal et ses
  // voisins ressortent par-dessus, plus loin dans cette fonction.
  const durees = anneau.reseau.albums.map((a) => a.duree_ms || 0);
  const dureeMin = Math.min(...durees);
  const dureeMax = Math.max(...durees);
  const epaisseurAlbum = (duree) => {
    if (dureeMax <= dureeMin) return (EPAISSEUR_ALBUM_MIN + EPAISSEUR_ALBUM_MAX) / 2;
    const t = (duree - dureeMin) / (dureeMax - dureeMin);
    return EPAISSEUR_ALBUM_MIN + t * (EPAISSEUR_ALBUM_MAX - EPAISSEUR_ALBUM_MIN);
  };

  anneau.puces = [];
  for (const a of anneau.reseau.albums) {
    const pos = positions.get(a.id);
    const angle = anneau.reseau.angleAlbum.get(a.id);
    if (!pos || angle === undefined) continue;
    const pas = pasParFamille.get(a.famille) || 0.02;
    const marge = pas * 0.1;

    ctx.strokeStyle = desature(teintes[a.famille % teintes.length] ?? accent);
    ctx.lineWidth = epaisseurAlbum(a.duree_ms || 0);
    // Même règle que le filtre par famille : un album hors de l'intervalle
    // d'années s'estompe, il ne disparaît jamais.
    ctx.globalAlpha = dansIntervalleAnnee(a.annee) ? 0.55 : 0.08;
    ctx.beginPath();
    ctx.arc(cx, cy, rayonAlbum, angle - pas / 2 + marge, angle + pas / 2 - marge);
    ctx.stroke();

    anneau.puces.push({ id: a.id, x: pos[0], y: pos[1], rayon: 6, noeud: a });
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // Fond permanent de liens : un sous-ensemble représentatif de tout le
  // réseau (le plus proche voisin de chaque album, voir `reseau_albums`),
  // très ténu — la densité tissée que la sélection vient ensuite isoler,
  // pas remplacer (même règle que le filtre par famille de la carte :
  // estomper, jamais masquer).
  for (const arc of anneau.reseau.arcs) {
    const posA = positions.get(arc.a);
    const posB = positions.get(arc.b);
    if (!posA || !posB) continue;
    const familleA = parAlbum.get(arc.a)?.famille;
    const familleB = parAlbum.get(arc.b)?.famille;
    if (familleA == null || familleB == null) continue;
    const chemin = cheminAncetres(posA, familleA, posB, familleB, hubsFamille, racine);
    ctx.strokeStyle = teintes[familleA % teintes.length] ?? accent;
    ctx.globalAlpha = 0.05;
    ctx.lineWidth = 0.6;
    ctx.beginPath();
    tracerLigneLissee(courbeBundle(chemin, anneau.beta));
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.lineWidth = 1;

  // Sélection : les flux du focal ressortent par-dessus le fond — épaisseur
  // et opacité montent nettement, couleur du voisin (la provenance), le
  // reste reste visible mais nettement plus discret. Un seul langage pour la
  // force : jamais de tiret ni de pointillé.
  if (anneau.focal) {
    const posFocal = positions.get(anneau.focal.id);
    if (posFocal && anneau.voisins.length) {
      const distances = anneau.voisins.map((v) => v.distance);
      const dMin = Math.min(...distances);
      const dMax = Math.max(...distances);
      const forceDe = (d) => (dMax <= dMin ? 0.5 : 1 - (d - dMin) / (dMax - dMin)); // 1 : le plus proche
      for (const v of anneau.voisins) {
        const posV = positions.get(v.id);
        if (!posV) continue;
        const chemin = cheminAncetres(posFocal, anneau.focal.famille, posV, v.famille, hubsFamille, racine);
        const force = forceDe(v.distance);
        ctx.strokeStyle = teintes[v.famille % teintes.length] ?? accent;
        ctx.globalAlpha = 0.45 + force * 0.55;
        ctx.lineWidth = 1.5 + force * 4;
        ctx.beginPath();
        tracerLigneLissee(courbeBundle(chemin, anneau.beta));
        ctx.stroke();

        dessinerRappel(posV, v.name, cx, cy, rayonExt, encre);
      }
      ctx.globalAlpha = 1;
      ctx.lineWidth = 1;

      // Le focal lui-même : son arc redessiné en pleine teinte + surépaisseur,
      // pour qu'il ressorte du fond désaturé.
      const angleFocal = anneau.reseau.angleAlbum.get(anneau.focal.id);
      if (angleFocal !== undefined) {
        const pasFocal = pasParFamille.get(anneau.focal.famille) || 0.02;
        ctx.strokeStyle = teintes[anneau.focal.famille % teintes.length] ?? accent;
        ctx.lineWidth = epaisseurAlbum(anneau.focal.duree_ms || 0) + 2;
        ctx.beginPath();
        ctx.arc(cx, cy, rayonAlbum, angleFocal - pasFocal / 2, angleFocal + pasFocal / 2);
        ctx.stroke();
        ctx.lineWidth = 1;
      }
    }
  }

  // Pas de tracé de playlist ici : sur Temps/Anneau, l’affichage ne montre
  // **que** l’album du morceau écouté et ses voisins soniques (le focal, plus
  // haut, piloté par `suivreAlbumEnLecture`). Superposer le trajet de toute la
  // file — qui part de l’album d’origine d’une playlist « alchimie » —
  // ajoutait un second jeu de connexions étranger au disque en cours. `parCle`
  // reste : le halo « album en écoute » juste dessous en a besoin.
  const parCle = new Map(anneau.reseau.albums.map((a) => [`${a.name} ${a.artist}`, a]));
  // L'album en écoute : même halo que le morceau en écoute sur Nuage/Carte
  // (`surcoucheSur`) et que sur la frise (`dessinerTemps`) — même clé
  // `parCle` que ci-dessus pour ramener le morceau en cours à son album.
  const pisteEnLecture = fileAttente.pistes.find((x) => x.path === lecture.courant);
  const albumEnLecture =
    pisteEnLecture && parCle.get(`${pisteEnLecture.album || ""} ${pisteEnLecture.artist || ""}`);
  const posEnLecture = albumEnLecture && positions.get(albumEnLecture.id);
  if (posEnLecture) {
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(posEnLecture[0], posEnLecture[1], 9, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 0.35;
    ctx.beginPath();
    ctx.arc(posEnLecture[0], posEnLecture[1], 14, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1;
  }

  // Le point survolé : un halo, pour le retrouver dans le fouillis des arcs.
  if (anneau.survole) {
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(anneau.survole.x, anneau.survole.y, anneau.survole.rayon, 0, Math.PI * 2);
    ctx.stroke();
    ctx.lineWidth = 1;
  }
}

/// Survol de l'anneau : retrouve le nœud le plus proche du curseur (voir
/// `anneau.puces`, reconstruit à chaque `dessinerAnneau`) et montre son
/// identité — un clic dessus (voir le gestionnaire `click` du canevas) en
/// fait le nouvel album focal.
function survolerAnneau(mx, my, r) {
  let meilleur = null;
  let d2min = 14 * 14;
  for (const p of anneau.puces) {
    const d2 = (p.x - mx) ** 2 + (p.y - my) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = p;
    }
  }
  if (meilleur === anneau.survole) return;
  anneau.survole = meilleur;
  const info = $("carte-info");
  if (meilleur) {
    info.hidden = false;
    info.innerHTML = "<b></b><span></span>";
    // Le rôle (focal / antérieur / postérieur) n'a de sens que par rapport à
    // la sélection courante — un album du fond permanent, hors sélection,
    // n'en a aucun.
    const estFocal = anneau.focal && meilleur.id === anneau.focal.id;
    const voisin = anneau.voisins.find((v) => v.id === meilleur.id);
    const role = estFocal ? "album focal" : voisin ? (voisin.avant ? "antérieur" : "postérieur") : null;
    info.children[0].textContent = meilleur.noeud.name;
    info.children[1].textContent = [meilleur.noeud.artist, role, duree(meilleur.noeud.duree_ms)]
      .filter(Boolean)
      .join(" — ");
    info.style.left = `${Math.min(mx + 14, r.width - 290)}px`;
    info.style.top = `${my + 14}px`;
  } else {
    info.hidden = true;
  }
  dessinerCarte();
}

function dessinerCarte() {
  // Le Lama n'a pas de canevas de carte : rien à redessiner (la playlist qui
  // se compose y appelle `tracerRouteSurCarte`, qui rappelle ceci).
  if (carte.affichage === "lama") return;
  // En mode carte, les tuiles portent le fond : le canevas ne garde que la
  // surcouche. En mode nuage, il dessine tout, comme avant.
  const g = carteGL();
  if (g) {
    g.resize();
    dessinerSurcouche();
    return;
  }
  const r = dimensionner();
  const style = getComputedStyle(document.documentElement);
  const encre = style.getPropertyValue("--txt").trim() || "#EDE8DC";
  const accent = style.getPropertyValue("--accent").trim() || "#C07C4A";

  // Le streamgraph est un troisième dessin sur le même canevas, pas une
  // variante du nuage : ses axes (temps × famille empilée) n'ont rien à voir
  // avec l'embedding que `dessinerNuage`/`surcoucheSur` savent tracer.
  if (modeTemps()) {
    ctx.clearRect(0, 0, r.width, r.height);
    dessinerTemps(r, encre, accent);
    return;
  }

  // L'anneau, de même : un cercle centré sur un album focal, encore un autre
  // repère que l'embedding, le lon/lat ou l'axe du temps.
  if (modeAnneau()) {
    ctx.clearRect(0, 0, r.width, r.height);
    dessinerAnneau(r, encre, accent);
    return;
  }

  ctx.clearRect(0, 0, r.width, r.height);
  if (carte.points.length === 0) {
    ctx.fillStyle = style.getPropertyValue("--mut").trim() || "#9A9284";
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Aucun morceau analysé pour l'instant.", r.width / 2, r.height / 2);
    return;
  }

  // Le nuage t-SNE, tel qu'il a toujours été.
  dessinerNuage(r);
  surcoucheSur(r, encre, accent);
}

/// Ce que le canevas dessine par-dessus les tuiles : le lasso en cours, le
/// tracé à la souris, l'itinéraire, les bornes, le survol. Tout ce que les
/// tuiles ne portent pas parce que cela change à chaque geste.
function dessinerSurcouche() {
  if (!carteGL()) return;
  const r = dimensionner();
  const style = getComputedStyle(document.documentElement);
  ctx.clearRect(0, 0, r.width, r.height);
  surcoucheSur(
    r,
    style.getPropertyValue("--txt").trim() || "#EDE8DC",
    style.getPropertyValue("--accent").trim() || "#C07C4A",
  );
}

function surcoucheSur(r, encre, accent) {
  // Le lasso en cours : contour fermé et zone assombrie, pour qu'on voie ce
  // qu'on attrape avant de lâcher.
  if (carte.lasso && carte.lasso.length > 1) {
    ctx.beginPath();
    carte.lasso.forEach(([x, y], i) => {
      const [ex, ey] = versEcran({ x, y }, r);
      if (i === 0) ctx.moveTo(ex, ey);
      else ctx.lineTo(ex, ey);
    });
    ctx.closePath();
    ctx.fillStyle = accent;
    ctx.globalAlpha = 0.12;
    ctx.fill();
    ctx.globalAlpha = 0.9;
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([4, 3]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }

  // Le trait en cours de dessin : pointillé, pour le distinguer d'un chemin.
  if (carte.trace && carte.trace.length > 1) {
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.setLineDash([5, 4]);
    ctx.beginPath();
    carte.trace.forEach(([x, y], i) => {
      const [ex, ey] = versEcran({ x, y }, r);
      if (i === 0) ctx.moveTo(ex, ey);
      else ctx.lineTo(ex, ey);
    });
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Le chemin, par-dessus le nuage : un trait continu et ses étapes.
  //
  // Un simple trait à l'accent s'y perdait : plusieurs familles portent des
  // teintes proches de l'accent, et à cette épaisseur le trait devenait un
  // point de plus dans le nuage. Un liseré dans l'encre du texte — le ton le
  // plus contrasté sur le fond, dans les deux thèmes — le fait ressortir
  // quel que soit ce qu'il traverse, comme un tracé de route sur une carte
  // routière ; l'accent reste au-dessus, plus fin, pour l'identité du trait.
  if (carte.route && carte.route.length > 1) {
    ctx.lineJoin = "round";
    ctx.lineCap = "round";

    // Le trait suit `routeTrace` (les vraies rues, une fois habillé) — mais
    // seulement en mode Carte : ses points intermédiaires sont des lon/lat,
    // qui n'ont aucun sens passés tels quels dans le repère t-SNE du Nuage.
    // Les repères ci-dessous restent posés sur `route` (une entrée par
    // morceau) — `routeTrace` peut porter bien plus de points que d'étapes.
    const trait = (carteGL() && carte.routeTrace) || carte.route;
    const tracer = () => {
      ctx.beginPath();
      trait.forEach((p, i) => {
        const [x, y] = versEcran(p, r);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.stroke();
    };

    ctx.strokeStyle = encre;
    ctx.globalAlpha = 0.55;
    ctx.lineWidth = 4.5;
    tracer();

    // Une playlist composée par le Lama garde ses parties : le trait et les
    // repères y prennent la couleur de leur partie (la même que dans le panneau),
    // pour retrouver sur la carte ce que le récit a expliqué.
    const parties =
      lama.fileComposee && lama.fileComposee === fileAttente.pistes && lama.partieDeId.size > 1;
    const racine = getComputedStyle(document.documentElement);
    const couleurPartie = (id) =>
      parties && lama.partieDeId.has(id)
        ? racine.getPropertyValue(`--partie-${(lama.partieDeId.get(id) % 6) + 1}`).trim() || accent
        : accent;
    if (parties && trait === carte.route) {
      ctx.globalAlpha = 0.95;
      ctx.lineWidth = 2.25;
      for (let i = 1; i < trait.length; i++) {
        ctx.strokeStyle = couleurPartie(trait[i].id);
        ctx.beginPath();
        const [x0, y0] = versEcran(trait[i - 1], r);
        const [x1, y1] = versEcran(trait[i], r);
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
        ctx.stroke();
      }
    } else {
      ctx.strokeStyle = accent;
      ctx.globalAlpha = 0.95;
      ctx.lineWidth = 2.25;
      tracer();
    }

    for (const p of carte.route) {
      const [x, y] = versEcran(p, r);
      ctx.beginPath();
      ctx.arc(x, y, 4.5, 0, Math.PI * 2);
      ctx.fillStyle = couleurPartie(p.id);
      ctx.globalAlpha = 1;
      ctx.fill();
      ctx.lineWidth = 1.2;
      ctx.strokeStyle = encre;
      ctx.globalAlpha = 0.8;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // Les bornes du chemin : anneaux creux, l'un plein pour le départ.
  for (const [borne, plein] of [[carte.depart, true], [carte.arrivee, false]]) {
    if (!borne) continue;
    const [x, y] = versEcran(borne, r);
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(x, y, 5.5, 0, Math.PI * 2);
    ctx.stroke();
    if (plein) {
      ctx.fillStyle = accent;
      ctx.beginPath();
      ctx.arc(x, y, 2, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // La balade en cours : sa trace et le promeneur (`balade.js`).
  dessinerBalade(r, encre, accent);

  // Les voix de l'aura : un anneau autour de chaque morceau entendu (`aura.js`).
  dessinerAura(r, accent);

  // Le morceau en écoute : un halo qui le retrouve dans 27 000 points.
  const joue = carte.points.find((p) => p.path === lecture.courant);
  if (joue) {
    const [x, y] = versEcran(joue, r);
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 9, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 0.35;
    ctx.beginPath();
    ctx.arc(x, y, 14, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // Le morceau survolé : un anneau, lisible quel que soit le fond.
  if (carte.survole) {
    const [x, y] = versEcran(carte.survole, r);
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 7, 0, Math.PI * 2);
    ctx.stroke();
  }
}

/// Point sous le curseur — sur le plan réel, vise d'abord le bâtiment que
/// MapLibre a effectivement peint sous ce pixel (`queryRenderedFeatures`) :
/// depuis que le morceau est un bâtiment entier, pas un point (voir
/// `carto-etapes.md`), un simple rayon autour du centroïde manquait tout
/// clic vers le bord d'un grand bâtiment. Retombe sur le point le plus
/// proche dans un rayon raisonnable si rien n'est peint là (bâtiment vacant,
/// zoom trop large, ou monde fictif sans bâti).
function pointSous(mx, my) {
  // Le streamgraph n'a pas de position d'écran par morceau — seulement des
  // bandes et des fils agrégés (voir `dessinerTemps`) : rien à pointer.
  // Chercher un morceau puis Entrée reste la façon de poser un départ.
  // L'anneau non plus : ses nœuds sont des albums, pas des morceaux, pointés
  // par `survolerAnneau`/`anneau.puces`, pas par `carte.points`.
  if (modeTemps() || modeAnneau()) return null;
  const g = carteGL();
  if (g && carte.villeReelle && g.getLayer("batiments-morceaux")) {
    const feats = g.queryRenderedFeatures([mx, my], { layers: ["batiments-morceaux"] });
    const id = feats[0]?.properties?.morceau;
    if (id != null && id >= 0 && (carte.isolees.size === 0 || carte.isolees.has(feats[0].properties.palier))) {
      const p = carte.points.find((pt) => pt.id === id);
      if (p) return p;
    }
  }

  const r = cnv.getBoundingClientRect();
  let meilleur = null;
  let d2min = 14 * 14;
  for (const p of carte.points) {
    if (carte.isolees.size > 0 && !carte.isolees.has(p.cluster)) continue;
    const [x, y] = versEcran(p, r);
    const d2 = (x - mx) ** 2 + (y - my) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = p;
    }
  }
  return meilleur;
}

/// Survol du streamgraph : retrouve le point de fil d'artiste le plus proche
/// du curseur (voir `temps.puces`, reconstruit à chaque `dessinerTemps`) et
/// montre son détail par album — c'est la seule façon de descendre du genre
/// à l'album sur ce mode, faute d'une position à cliquer par morceau.
/// Survol de la frise : l'album le plus proche du curseur, ou — à défaut —
/// l'année sous le curseur quand il traîne sur la bande de graduation, seule
/// façon de lire une année puisqu'elle ne s'imprime plus en continu (§ 5).
function survolerTemps(mx, my, r) {
  let meilleur = null;
  let d2min = 12 * 12;
  for (const p of temps.puces) {
    const d2 = (p.x - mx) ** 2 + (p.y - my) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = p;
    }
  }

  let anneeSurvolee = null;
  const d = temps.disposition;
  if (!meilleur && d && d.n > 1) {
    const yBas = r.height - 28;
    if (my >= yBas - 2 && my <= yBas + 14 && mx >= d.xDeb - 4 && mx <= d.xFin + 4) {
      anneeSurvolee = Math.round(
        temps.minAnnee + (mx - d.xDeb - temps.vue.dx) / (d.xPasBase * temps.vue.k),
      );
    }
  }

  const cle = meilleur ? `a:${meilleur.album.id}` : anneeSurvolee != null ? `y:${anneeSurvolee}` : null;
  if (cle === temps.survoleCle) return;
  temps.survoleCle = cle;
  // La puce entière, pas seulement ses coordonnées : le gestionnaire de clic
  // (`cnv.addEventListener("click")`) lit `temps.survole.album` pour peupler
  // l'inspecteur partagé — même schéma que `anneau.survole`.
  temps.survole = meilleur;

  const info = $("carte-info");
  if (meilleur) {
    const a = meilleur.album;
    const noms = new Map(carte.familles.map(([c, nom]) => [c, nom]));
    const fam = noms.get(a.famille) || `famille ${a.famille + 1}`;
    const secondaire = a.familleSecondaire != null ? ` + ${noms.get(a.familleSecondaire) || ""}` : "";
    const hub = a.sortants >= temps.seuilHub ? " · hub" : "";
    info.hidden = false;
    info.innerHTML = "<b></b><span></span>";
    info.children[0].textContent = `${isoler(a.name)} — ${isoler(a.artist)}`;
    info.children[1].textContent = `${a.annee ?? "date incertaine"} · ${fam}${secondaire}${hub}`;
    info.style.left = `${Math.min(mx + 14, r.width - 290)}px`;
    info.style.top = `${my + 14}px`;
  } else if (anneeSurvolee != null) {
    info.hidden = false;
    info.innerHTML = "<b></b>";
    info.children[0].textContent = String(anneeSurvolee);
    info.style.left = `${Math.min(mx + 10, r.width - 100)}px`;
    info.style.top = `${my - 24}px`;
  } else {
    info.hidden = true;
  }
  dessinerCarte();
}

cnv.addEventListener("mousemove", (e) => {
  const r = cnv.getBoundingClientRect();
  const mx = e.clientX - r.left;
  const my = e.clientY - r.top;

  // Lasso et tracé de chemin se suivent de la même façon : un point tous les
  // 3 px, pour ne pas accumuler des centaines de points confondus.
  const encours = carte.lasso ?? carte.trace;
  if (encours) {
    const p = versCarte(mx, my, r);
    const d = encours[encours.length - 1];
    const seuil = 3 / (echelle(r) * carte.vue.k);
    if (Math.hypot(p[0] - d[0], p[1] - d[1]) > seuil) encours.push(p);
    dessinerCarte();
    return;
  }

  if (glisse) {
    const g = carteGL();
    if (g) {
      // Le canevas relaie : MapLibre n'écoute rien lui-même.
      g.panBy([-e.movementX, -e.movementY], { duration: 0 });
    } else if (modeTemps()) {
      // Les deux axes, comme Nuage/Carte juste en dessous — le temps
      // (`dx`) et les bandes (`dy`, zoom vertical continu). Reclampés au
      // prochain dessin, voir `temps.vue`.
      temps.vue.dx += e.movementX;
      temps.vue.dy += e.movementY;
    } else if (modeAnneau()) {
      anneau.vue.dx += e.movementX;
      anneau.vue.dy += e.movementY;
    } else {
      carte.vue.dx += e.movementX;
      carte.vue.dy += e.movementY;
    }
    dessinerCarte();
    return;
  }

  if (modeTemps()) {
    survolerTemps(mx, my, r);
    return;
  }

  if (modeAnneau()) {
    survolerAnneau(mx, my, r);
    return;
  }

  const p = pointSous(mx, my);
  if (p !== carte.survole) {
    carte.survole = p;
    const info = $("carte-info");
    if (p) {
      info.hidden = false;
      info.innerHTML = "<b></b><span></span>";
      info.children[0].textContent = txt(p.title, "(sans titre)");
      info.children[1].textContent = txt(p.artist, "(sans artiste)");
      info.style.left = `${Math.min(mx + 14, r.width - 290)}px`;
      info.style.top = `${my + 14}px`;
    } else {
      info.hidden = true;
    }
    dessinerCarte();
  }
});

let glisse = false;
// Un dessin ou un lasso se termine par un `mouseup`, donc par un `click` :
// sans ce drapeau, relâcher le trait relancerait la lecture du point survolé.
let vientDeDessiner = false;
// Un déplacement de la carte se termine lui aussi par un `click`. Sans cette
// mesure, faire glisser la vue changeait le morceau en écoute — le point sous
// le curseur à l'arrivée n'a rien demandé.
let departGlisse = null;
const SEUIL_GLISSE = 4; // px ; en deçà, c'est un clic tremblant, pas un glissé

cnv.addEventListener("mousedown", (e) => {
  // Le clic droit ouvre le menu d'un morceau (`balade.js`) : ni glisser ni
  // lasso.
  if (e.button === 2) return;
  // Un relâchement hors du canevas ne produit pas de `click` : sans cette
  // remise à zéro, le drapeau survivrait et avalerait le clic suivant.
  vientDeDessiner = false;

  // Le streamgraph n'a ni lasso ni tracé à la souris — chercher un morceau
  // puis Entrée reste le seul geste de départ du mode Temps — mais il se
  // glisse horizontalement comme les deux autres affichages (voir la
  // branche `modeTemps()` de `mousemove`, plus bas).
  if (modeTemps()) {
    glisse = true;
    departGlisse = [e.clientX, e.clientY];
    return;
  }

  // L'anneau n'a ni lasso, ni tracé — juste zoomer/glisser (comme le nuage,
  // voir `zoomerAnneau`) et le clic (voir le gestionnaire `click` plus bas) :
  // chercher un album, ou en cliquer un déjà affiché.
  if (modeAnneau()) {
    glisse = true;
    departGlisse = [e.clientX, e.clientY];
    return;
  }

  // Alt+glisser : lasso. Disponible dans tous les modes de chemin — c'est une
  // sélection, pas un chemin, et rien ne justifie de la cacher derrière un
  // mode.
  if (e.altKey) {
    const r = cnv.getBoundingClientRect();
    carte.lasso = [versCarte(e.clientX - r.left, e.clientY - r.top, r)];
    carte.route = null;
    carte.survole = null;
    $("carte-info").hidden = true;
    return;
  }

  if (carte.chemin === "dessine" && e.shiftKey) {
    const r = cnv.getBoundingClientRect();
    carte.trace = [versCarte(e.clientX - r.left, e.clientY - r.top, r)];
    carte.route = null;
    carte.survole = null;
    $("carte-info").hidden = true;
    return;
  }
  glisse = true;
  departGlisse = [e.clientX, e.clientY];
});

window.addEventListener("mouseup", (e) => {
  if (departGlisse) {
    const bouge =
      Math.hypot(e.clientX - departGlisse[0], e.clientY - departGlisse[1]) > SEUIL_GLISSE;
    departGlisse = null;
    if (bouge) vientDeDessiner = true; // le clic qui suit est à ignorer
  }
});

window.addEventListener("mouseup", async () => {
  glisse = false;
  if (carte.lasso) {
    const contour = carte.lasso;
    carte.lasso = null;
    vientDeDessiner = true;
    dessinerCarte();
    await jouerSelection(contour);
    return;
  }
  if (!carte.trace) return;
  const trace = carte.trace;
  carte.trace = null;
  vientDeDessiner = true;
  await tracerDessin(trace);
});

cnv.addEventListener("mouseleave", () => {
  carte.survole = null;
  temps.survole = null;
  anneau.survole = null;
  $("carte-info").hidden = true;
  dessinerCarte();
});

/// Applique un facteur de zoom autour d'un point de l'écran.
/// Zoom horizontal du streamgraph, autour de `cx` (pixel écran, l'axe des
/// familles empilées ne zoome jamais). Même principe que le zoom du nuage :
/// on retrouve le bucket sous le curseur avant le geste, on change d'échelle,
/// puis on décale pour l'y remettre — sans quoi zoomer ferait fuir le point
/// qu'on visait plutôt que s'en rapprocher.
/// Zoom de la frise — un seul facteur (`temps.vue.k`) pour les deux axes à la
/// fois, exactement comme `zoomer` sur le nuage (`carte.vue.k` juste plus
/// bas) : un geste de molette/pincement zoome le temps et les bandes
/// ensemble, glisser déplace les deux (voir le gestionnaire `mousemove`).
/// Deux axes, donc deux pivots à préserver sous le curseur — `cx` pour
/// l'année (repère `xDeb`/`xPasBase`), `cy` pour la bande (repère `yHaut`) —
/// mais un seul `k`, recalculé une fois, jamais deux facteurs qui pourraient
/// diverger l'un de l'autre.
function zoomerTemps(f, cx, cy) {
  const d = temps.disposition;
  if (!d) return;
  const xSousCurseur = d.n >= 2 ? (cx - d.xDeb - temps.vue.dx) / (d.xPasBase * temps.vue.k) : null;
  const ySousCurseur = (cy - d.yHaut - temps.vue.dy) / temps.vue.k;
  temps.vue.k = Math.min(40, Math.max(1, temps.vue.k * f));
  if (xSousCurseur != null) {
    temps.vue.dx = cx - d.xDeb - xSousCurseur * d.xPasBase * temps.vue.k;
  }
  temps.vue.dy = cy - d.yHaut - ySousCurseur * temps.vue.k;
  $("zoom-val").textContent = `×${temps.vue.k.toFixed(1).replace(".", ",")}`;
  dessinerCarte();
}

/// Zoom de l'anneau — même principe que le repli sans MapLibre du nuage
/// (`carte.vue.k` juste plus bas) : on retrouve le point sous le curseur
/// avant le geste, on change d'échelle, puis on décale pour l'y remettre.
function zoomerAnneau(f, cx, cy) {
  const r = cnv.getBoundingClientRect();
  const mx = (cx ?? r.width / 2) - r.width / 2;
  const my = (cy ?? r.height / 2) - r.height / 2;
  const avant = anneau.vue.k;
  anneau.vue.k = Math.min(6, Math.max(0.5, anneau.vue.k * f));
  const reel = anneau.vue.k / avant;
  anneau.vue.dx = mx - (mx - anneau.vue.dx) * reel;
  anneau.vue.dy = my - (my - anneau.vue.dy) * reel;
  $("zoom-val").textContent = `×${anneau.vue.k.toFixed(1).replace(".", ",")}`;
  dessinerCarte();
}

function zoomer(f, cx, cy) {
  if (modeTemps()) {
    const r = cnv.getBoundingClientRect();
    zoomerTemps(f, cx ?? r.width / 2, cy ?? r.height / 2);
    return;
  }
  if (modeAnneau()) {
    zoomerAnneau(f, cx, cy);
    return;
  }
  const g = carteGL();
  if (g) {
    // Zoomer autour du curseur : on note le point du monde qui s'y trouve,
    // on change d'échelle, puis on recentre pour l'y remettre. Les boutons
    // +/− appellent sans coordonnées : `unproject([undefined, …])` lève dans
    // MapLibre — on retombe alors sur le centre du canevas.
    const r0 = cnv.getBoundingClientRect();
    if (cx == null) cx = r0.width / 2;
    if (cy == null) cy = r0.height / 2;
    const avant = g.unproject([cx, cy]);
    const z = Math.min(zoomMax, Math.max(0, g.getZoom() + Math.log2(f)));
    g.setZoom(z);
    const apres = g.unproject([cx, cy]);
    const c = g.getCenter();
    g.setCenter([c.lng + (avant.lng - apres.lng), c.lat + (avant.lat - apres.lat)]);
    synchroniserVue();
    return;
  }
  const r = cnv.getBoundingClientRect();
  const mx = (cx ?? r.width / 2) - r.width / 2;
  const my = (cy ?? r.height / 2) - r.height / 2;
  const avant = carte.vue.k;
  carte.vue.k = Math.min(60, Math.max(0.5, carte.vue.k * f));
  // Le facteur réellement appliqué, après butée : sans cela, le décalage
  // continuerait de bouger une fois le zoom bloqué.
  const reel = carte.vue.k / avant;
  carte.vue.dx = mx - (mx - carte.vue.dx) * reel;
  carte.vue.dy = my - (my - carte.vue.dy) * reel;
  $("zoom-val").textContent = `×${carte.vue.k.toFixed(1).replace(".", ",")}`;
  dessinerCarte();
}

/// L'image hors-écran de la densité est bâtie pour un niveau de zoom donné
/// : la redessiner à chaque cran de molette la
/// referait des dizaines de fois par seconde pour rien, un délai court
/// après la fin du geste suffit — même principe que le bruit des chemins.
let attenteZoomDensite = null;

cnv.addEventListener("wheel", (e) => {
  e.preventDefault();
  const r = cnv.getBoundingClientRect();
  const mx = e.clientX - r.left;
  const my = e.clientY - r.top;

  // Proportionnel à l'ampleur du geste, et non un pas fixe par évènement :
  // un trackpad en émet des dizaines par centimètre de doigt, là où une
  // molette en émet un par cran. Le pas fixe rendait le zoom inutilisable au
  // trackpad. Le facteur est borné pour qu'une inertie brutale ne fasse pas
  // traverser toute la plage d'un coup. Un seul geste, les deux axes à la
  // fois — `zoomer` sait déjà distinguer les affichages, voir `zoomerTemps`,
  // qui zoome le temps et les bandes ensemble sur la frise, exactement comme
  // `carte.vue.k` le fait sur le nuage.
  const f = Math.exp(-Math.max(-40, Math.min(40, e.deltaY)) * 0.0035);
  zoomer(f, mx, my);
}, { passive: false });

$("zoom-plus").addEventListener("click", () => zoomer(1.4));
$("zoom-moins").addEventListener("click", () => zoomer(1 / 1.4));
$("zoom-reset").addEventListener("click", () => {
  if (modeTemps()) {
    temps.vue = { k: 1, dx: 0, dy: 0 };
    $("zoom-val").textContent = "×1,0";
    dessinerCarte();
    return;
  }
  if (modeAnneau()) {
    anneau.vue = { k: 1, dx: 0, dy: 0 };
    $("zoom-val").textContent = "×1,0";
    dessinerCarte();
    return;
  }
  const g = carteGL();
  if (g) {
    // Priorité à la limite communale (`vueInitialeGL.bounds`) : prise sur la
    // frontière réelle, elle ne se fait pas tirer au loin par un morceau mal
    // géocodé. À défaut — monde fictif — l'emprise des points. Le `zoom`
    // d'accueil du style (14, niveau du quartier) n'est qu'un dernier repli.
    const b = vueInitialeGL?.bounds || bornesGeoPoints();
    let applique = null;
    if (b) {
      // On calcule la caméra sans bouger, on la vérifie, puis on l'applique :
      // une emprise dégénérée (coordonnées parasites) donnerait un zoom
      // minuscule et une carte vide — mieux vaut alors le centre d'accueil.
      const cam = g.cameraForBounds(b, { padding: 40 });
      if (cam && cam.zoom >= 6) {
        g.jumpTo(cam);
        applique = `bounds → zoom ${cam.zoom.toFixed(2)}`;
      }
    }
    if (!applique && vueInitialeGL) {
      g.jumpTo({ center: vueInitialeGL.center, zoom: vueInitialeGL.zoom });
      applique = `centre d'accueil (bounds ${b ? "rejetées" : "absentes"})`;
    }
    journalCarte("vue d'ensemble : " + (applique || "aucune cible"));
    synchroniserVue();
    dessinerCarte();
    return;
  }
  carte.vue = { k: 1, dx: 0, dy: 0 };
  $("zoom-val").textContent = "×1,0";
  dessinerCarte();
});

cnv.addEventListener("click", async (e) => {
  if (vientDeDessiner) {
    vientDeDessiner = false;
    return;
  }

  // Anneau : cliquer n'importe quel album du fond (pas seulement un voisin
  // déjà affiché) en fait le nouvel album focal — « marcher » dans le graphe
  // de ressemblance. Cliquer le focal lui-même ne fait rien, il l'est déjà.
  if (modeAnneau()) {
    if (anneau.survole && anneau.survole.id !== anneau.focal?.id) {
      await chargerAnneau(anneau.survole.id);
    }
    return;
  }

  // Temps (frise des filiations) : même geste que l'anneau, même état focal
  // partagé (`anneau.focal`/`anneau.voisins`) — `chargerAnneau` peuple à la
  // fois la sélection dessinée (§ 2) et l'inspecteur partagé (§ 1), pas un
  // composant propre à la frise.
  if (modeTemps()) {
    if (temps.survole && temps.survole.album.id !== anneau.focal?.id) {
      await chargerAnneau(temps.survole.album.id);
    }
    return;
  }

  // Au carrefour d'une balade, un clic sur une rue tracée la prend
  // (`balade.js`) : il passe avant le morceau sous le curseur.
  {
    const r = cnv.getBoundingClientRect();
    if (!e.shiftKey && cliquerVoie(e.clientX - r.left, e.clientY - r.top)) return;
  }

  const p = carte.survole;
  if (!p) return;

  // Maj est le modificateur « chemin » dans tous les modes ; ce qu'il déclenche
  // dépend du mode choisi dans le rail.
  if (e.shiftKey) {
    if (carte.chemin === "dessine") return; // le dessin passe par le glisser
    await poserBorne(p);
    return;
  }

  // Sans modificateur : le morceau devient le départ proposé et peuple
  // l'inspecteur — silencieux, jamais un `play` direct. Même geste que le
  // clic sur l'Anneau/la Frise (`chargerAnneau`, qui ne joue pas non plus) :
  // cliquer navigue/sélectionne, écouter est un geste à part (bouton ▶ de
  // l'inspecteur, `insp-lecture`) — cohérence voulue entre les quatre
  // visualisations plutôt qu'un clic qui coupe la lecture en cours ici et pas
  // là (`docs/interface-guidelines.md` § 1).
  carte.depart = p;
  carte.arrivee = null;
  carte.route = null;
  dessinerBornes();
  selectionner(p);
  dessinerCarte();
});

/// Combien de morceaux le chemin doit compter — plafond pour le mode
/// sonique, dont la longueur naturelle est celle du graphe.
function longueurChemin() {
  const n = Number.parseInt($("chemin-n").value, 10);
  return Number.isFinite(n) ? Math.min(120, Math.max(2, n)) : 12;
}

/// Demande un chemin au moteur et l'envoie au lecteur.
///
/// `spec` est conservé tel quel pour que « Autre tirage » puisse rejouer le
/// même geste avec une graine différente.
async function tracerChemin(spec) {
  carte.refaire = spec;
  patienter("calcul du chemin…");
  let pistes;
  try {
    // Le voyage n'a ni arrivée ni notion de familles isolées ou de plan de
    // ville — un axe temporel, pas un espace à filtrer — d'où une commande
    // à lui, plutôt qu'un mode de plus dans `path`.
    pistes =
      spec.mode === "voyage"
        ? await invoke("voyage", {
            from: spec.from,
            steps: longueurChemin(),
            seed: carte.graine,
            bruit: bruitChemin,
          })
        : await invoke("path", {
            ...spec,
            steps: longueurChemin(),
            seed: carte.graine,
            bruit: bruitChemin,
            reel: carteReelle(),
            familles: [...carte.isolees],
          });
  } catch (e) {
    // Sans ce catch, l'échec ne laissait qu'un rappel de gestes générique en
    // pied de carte — rien qui dise qu'un calcul a réellement échoué.
    signalerErreur("chemin-aide", "échec du calcul du chemin", e, "chemin");
    return;
  } finally {
    patienter(null);
  }
  poserChemin(pistes);
}

/// Chemin dessiné : le tracé part en coordonnées de carte, avec le rayon de
/// cueillette que vaut le zoom courant. 24 px à l'écran, quel que soit le
/// facteur — c'est ce que l'utilisateur croit toucher avec son trait.
///
/// `trace` et `rayon` sont mémorisés dans `carte.refaire` (forme distincte de
/// celle de `tracerChemin`, voir `rejouerChemin`) : contrairement aux trois
/// autres modes, il n'y a pas de `{from, to, mode}` à rejouer, juste ce
/// tracé-ci, avec un nouveau bruit ou une nouvelle graine.
async function tracerDessin(trace) {
  const r = cnv.getBoundingClientRect();
  // Sur le plan de ville, le tracé et les points sont en lon/lat projetés en
  // mètres côté moteur : le rayon aussi (24 px du zoom courant, en mètres).
  // Sur le nuage t-SNE, le rayon reste dans le repère de la vue.
  const rayon = carteReelle()
    ? metresParPixels(24)
    : 24 / (echelle(r) * carte.vue.k);
  carte.refaire = { dessine: true, trace, rayon };
  patienter("cueillette sous le trait…");
  let pistes;
  try {
    pistes = await invoke("path_drawn", {
      trace,
      steps: longueurChemin(),
      radius: rayon,
      seed: carte.graine,
      bruit: bruitChemin,
      reel: carteReelle(),
      familles: [...carte.isolees],
    });
  } finally {
    patienter(null);
  }
  // Le trait dessiné EST l'intention : sur le plan de ville, on l'affiche tel
  // quel, sans le remplacer par un routage de rues entre les morceaux cueillis.
  poserChemin(pistes, "le trait n'a touché aucun morceau", carteReelle() ? trace : null);
}

/// Rejoue le dernier chemin tracé — bouton « Autre tirage » ou curseur de
/// bruit. `carte.refaire` prend deux formes : celle de `tracerChemin`
/// (`{from, to, mode}`, pour direct/sonique/errance) ou celle de
/// `tracerDessin` (`{dessine: true, trace, rayon}`), qui n'a pas de bornes à
/// repasser à `path` — chacune retrouve son propre chemin d'appel.
async function rejouerChemin() {
  if (!carte.refaire) return;
  if (carte.refaire.dessine) {
    const { trace, rayon } = carte.refaire;
    patienter("cueillette sous le trait…");
    let pistes;
    try {
      pistes = await invoke("path_drawn", {
        trace,
        steps: longueurChemin(),
        radius: rayon,
        seed: carte.graine,
        bruit: bruitChemin,
        reel: carteReelle(),
        familles: [...carte.isolees],
      });
    } catch (e) {
      remonter(e, "chemin dessiné");
      return;
    } finally {
      patienter(null);
    }
    poserChemin(pistes, "le trait n'a touché aucun morceau", carteReelle() ? trace : null);
  } else {
    await tracerChemin(carte.refaire);
  }
}

/// Joue les morceaux d'une zone dessinée au lasso.
///
/// Le moteur les rend déjà ordonnés en parcours de proche en proche : une zone
/// donne des dizaines de morceaux, et les enchaîner dans l'ordre de la base
/// produirait une playlist qui saute d'un bout à l'autre de la sélection.
async function jouerSelection(contour) {
  if (contour.length < 3) return;
  patienter("sélection…");
  let pistes;
  try {
    pistes = await invoke("selection", {
      trace: contour,
      reel: carteReelle(),
      familles: [...carte.isolees],
    });
  } finally {
    patienter(null);
  }
  if (!pistes || pistes.length === 0) {
    $("fil-compte").textContent = "le lasso n'a rien attrapé";
    return;
  }
  // Le tracé de la zone laisse la place au parcours qu'on va suivre.
  carte.refaire = null;
  await poserChemin(pistes, "sélection vide");
  $("fil-compte").textContent = `${pistes.length} morceaux de la zone`;
}

/// Affiche un chemin reçu du moteur et le met en lecture.
/// Reflète une file sur la carte : le chemin entre ses morceaux, s'il y en a
/// au moins deux — un seul morceau est déjà signalé par le halo de lecture
/// (`lecture.courant`), indépendant de `carte.route`. Sans effet tant que la carte
/// n'a pas chargé ses points ; `basculerMode` rejoue l'appel pour la file en
/// cours en entrant dans Explorer, pour rattraper une file lancée avant.
let traceRuesGraine = 0; // écarte une réponse `trace_rues` périmée par un trajet plus récent

function tracerRouteSurCarte(pistes, polyligne = null) {
  if (!carte.points.length) return;
  const parId = new Map(carte.points.map((p) => [p.id, p]));
  carte.route =
    pistes && pistes.length >= 2
      ? pistes.map((t) => parId.get(t.id)).filter(Boolean)
      : null;
  // Le trait suit d'abord la droite d'étape en étape — même donnée que les
  // repères, tant que rien de mieux n'est arrivé.
  carte.routeTrace = carte.route;
  traceRuesGraine++;

  // L'itinéraire sur voirie (et le tracé dessiné) livrent déjà leur ligne : on
  // l'affiche telle quelle, sans repasser par `trace_rues` (qui router-ait
  // entre les morceaux dans l'ordre de la playlist et pourrait boucler).
  // `{x, y}` = lon/lat, comme les segments de `trace_rues`.
  if (carte.villeReelle && polyligne && polyligne.length && carte.route) {
    carte.routeTrace =
      polyligne.length >= 2
        ? [
            carte.route[0],
            ...polyligne.map(([x, y]) => ({ x, y })),
            carte.route[carte.route.length - 1],
          ]
        : carte.route; // polyligne dégénérée : la droite entre morceaux, mais pas d'habillage
    if (navigation.mode === "explorer") dessinerCarte();
    return;
  }

  if (navigation.mode === "explorer") dessinerCarte();

  // Sur le plan de ville réel, habiller ce trait avec les vraies rues entre
  // chaque paire consécutive — **le choix des morceaux ne change pas**,
  // seule la ligne dessinée. Asynchrone et non bloquant : le trait droit
  // reste affiché jusqu'à ce que les rues arrivent, puis le remplace.
  if (carte.villeReelle && carte.route && carte.route.length >= 2) {
    const ids = carte.route.map((p) => p.id);
    const graine = traceRuesGraine;
    invoke("trace_rues", { ids })
      .then((segments) => {
        // Un autre trajet est arrivé entre-temps, ou la route a été effacée :
        // cette réponse ne concerne plus ce qui est à l'écran.
        if (graine !== traceRuesGraine || !carte.route) return;
        const habille = [carte.route[0]];
        segments.forEach((segment, i) => {
          for (const [x, y] of segment) habille.push({ x, y });
          habille.push(carte.route[i + 1]);
        });
        carte.routeTrace = habille;
        if (navigation.mode === "explorer") dessinerCarte();
      })
      .catch((e) => journalCarte("habillage du trait par les rues : " + e, "warn"));
  }
}

async function poserChemin(pistes, vide = "aucun chemin trouvé", polyligne = null) {
  if (!pistes || pistes.length < 2) {
    $("fil-compte").textContent = vide;
    carte.route = null;
    dessinerCarte();
    return;
  }

  tracerRouteSurCarte(pistes, polyligne);
  fileAttente.pistes = pistes;
  // Sans ce redessin, le panneau « file » resté ouvert continue de montrer
  // l'ancienne liste : rien d'autre ne le rafraîchit ici tant que le premier
  // morceau ne change pas — exactement le cas quand on ajuste la pondération
  // de l'errance depuis le même départ, où seule la suite change.
  if (!$("file").hidden) dessinerFile();
  // `set_queue`, pas `play` : si le premier morceau ne change pas — un
  // simple réglage du curseur de bruit ou « Autre tirage » sur le même
  // départ —, la lecture en cours n'a aucune raison de repartir de zéro.
  await demarrerLecture(() => invoke("set_queue", { paths: pistes.map((t) => t.path) }));
  selectionner(pistes[0]);
  dessinerCarte();
  $("fil-compte").textContent = `chemin de ${pistes.length} morceaux`;
  $("chemin-rejouer").hidden = !carte.refaire;
}

/// Signale un calcul en cours dans le pied de carte, ou l'efface.
///
/// Le premier chemin sonique ou errant construit le graphe des voisins : une
/// dizaine de secondes sur la bibliothèque entière. Sans ce mot, la carte
/// paraîtrait figée.
function patienter(texte) {
  $("carte-aide").textContent = texte ?? aideCourante();
}

window.addEventListener("resize", () => {
  if (!$("carte-vue").hidden) dessinerCarte();
});

/// Charge `carte.familles` (`[[cluster, nom, effectif]]`) au premier besoin —
/// mêmes données pour la légende d'Explorer et le filtre de l'Écoute.
///
/// Les noms viennent du moteur. Ni le genre le plus fréquent — « Rock » domine
/// six familles sur douze et ne les distinguerait pas — ni le plus
/// caractéristique, qui nommait « Ska Rock » une famille de 4 321 morceaux
/// menée par Bob Marley. Les deux à la fois : voir `nommer_les_familles`.
async function chargerFamilles() {
  if (!carte.familles) {
    try {
      carte.familles = await invoke("families");
    } catch (e) {
      remonter(e, "familles");
      carte.familles = [];
    }
  }
  // Détail du vote à trois sources (`docs/nommage-familles.md`), pour le
  // bloc « Famille » de l'inspecteur — best-effort : son échec n'empêche pas
  // la légende de la carte, déjà servie par `carte.familles` ci-dessus.
  // Rechargé dès qu'il est absent, indépendamment de `carte.familles` : une
  // passe Last.fm invalide le vote sans toucher aux noms de la légende
  // (`carte.familleVotes = null` seul, voir `passeLastfm`).
  if (!carte.familleVotes) {
    try {
      carte.familleVotes = await invoke("families_detail");
    } catch {
      carte.familleVotes = [];
    }
  }
}

/// Rendu commun de la légende des familles : une pastille teintée, un nom, un
/// effectif par famille. `estActive(cluster)` décide du filet d'accent,
/// `auClic(cluster)` réagit au clic. Même geste de multi-sélection (cocher/
/// décocher) dans les trois modes : Explorer isole un ensemble de familles
/// sur la carte, l'Écoute et Découvrir cochent/décochent une famille de leur
/// filtre — cohérence d'interface (`docs/interface-guidelines.md`, principe
/// Nielsen).
///
/// `extension`, optionnel : sur la frise des filiations seulement (§ 3), un
/// second contrôle par ligne — étendre/réduire cette bande verticalement
/// (`estActive`/`auClic` propres à ce contrôle, indépendants de l'isolement).
/// Toujours une ligne (`.famille-ligne`) même sans `extension`, pour que
/// l'ajout du second bouton ne redistribue jamais la mise en page existante.
function rendreFamilles(hote, estActive, auClic, extension) {
  const teintes = couleursFamilles();
  hote.replaceChildren();
  for (const [c, nom, n] of carte.familles ?? []) {
    const ligne = document.createElement("div");
    ligne.className = "famille-ligne";

    const el = document.createElement("button");
    el.className = "famille" + (estActive(c) ? " famille--isolee" : "");
    el.innerHTML = `<span class="famille__pastille"></span>
                    <span></span><span class="famille__n"></span>`;
    el.children[0].style.background = teintes[c % teintes.length] ?? "currentColor";
    // Une famille dont aucun genre ne ressort garde son numéro : mieux vaut un
    // nom neutre qu'un nom faux.
    el.children[1].textContent = nom || `famille ${c + 1}`;
    el.children[1].title = nom || "";
    el.children[2].textContent = n.toLocaleString("fr-FR");
    el.addEventListener("click", () => auClic(c));
    ligne.appendChild(el);

    if (extension) {
      const actif = extension.estActive(c);
      const bouton = document.createElement("button");
      bouton.className = "famille__etendre" + (actif ? " famille__etendre--actif" : "");
      bouton.title = actif ? "Réduire cette bande" : "Étendre cette bande verticalement";
      bouton.textContent = "↕";
      bouton.addEventListener("click", () => extension.auClic(c));
      ligne.appendChild(bouton);
    }

    hote.appendChild(ligne);
  }
}

/// Le filtre par famille borne aussi le trajet : quand les familles isolées
/// changent, ce qui est déjà tracé se recalcule. Un itinéraire (voirie ou
/// musical) n'a pas de `carte.refaire` : on ne le rejoue que s'il y a un départ
/// et un tracé à l'écran, pour ne pas lancer un calcul que personne n'a demandé.
function rejouerSelonFamilles() {
  if (carte.chemin === "itineraire") {
    if (carte.depart && carte.route) tracerItineraire().catch((e) => remonter(e, "itinéraire"));
  } else if (carte.refaire) {
    rejouerChemin().catch((e) => remonter(e, "chemin"));
  }
}

async function dessinerFamilles() {
  await chargerFamilles();
  rendreFamilles(
    $("familles"),
    (c) => carte.isolees.has(c),
    (c) => {
      if (carte.isolees.has(c)) carte.isolees.delete(c);
      else carte.isolees.add(c);
      dessinerFamilles();
      dessinerCarte();
      majFiltreGL();
      // Le filtre par famille borne aussi le chemin : un chemin déjà tracé se
      // recalcule pour ne garder que les familles isolées (ou les relâcher).
      rejouerSelonFamilles();
    },
    // Zoom vertical ciblé (§ 3) : seulement sur la frise, où une bande peut
    // compresser plusieurs milliers d'albums (Rock · Grunge, par exemple).
    // Ailleurs (Nuage, Carte, Anneau), aucune notion de « bande » n'existe.
    modeTemps()
      ? {
          estActive: (c) => temps.bandeEtendue === c,
          auClic: (c) => {
            temps.bandeEtendue = temps.bandeEtendue === c ? null : c;
            dessinerFamilles();
            dessinerCarte();
          },
        }
      : undefined,
  );
  $("familles-tout").hidden = carte.isolees.size === 0;
}

$("familles-tout").addEventListener("click", () => {
  carte.isolees.clear();
  dessinerFamilles();
  dessinerCarte();
  majFiltreGL();
  rejouerSelonFamilles();
});


export {
  bornesGeoPoints, carteGL, carteReelle, chargerFamilles, couleursFamilles,
  couleursFamillesCarte, dessinerCarte,
  dessinerFamilles, initialiserFiltreAnnee, journalCarte, longueurChemin, majAffichageGL,
  majCouleurGL, majEtiquetteIntervalleAnnees, majFiltreGL, majPentesGL, majVisibiliteIntervalleAnnees,
  modeAnneau, modeTemps, patienter, pointSous, poserChemin, poserVignetteCarte,
  rafraichirFiltreAnnee, rejouerChemin, rendreFamilles, tracerChemin, tracerRouteSurCarte,
  versCarte, versEcran, vueInitialeGL, zoomer,
};
