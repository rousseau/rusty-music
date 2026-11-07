import "../carte.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, navigation,
} from "../socle.js";
import {
  carte, cnv, CONTINUES, filtreAnnee, intervalleAnneeActif, rampe,
} from "../carte.js";
import { majSegmentsCouleur } from "../ecouter-familles.js";
import { majTracesGL } from "../traces.js";
import { majReliefGL } from "../relief.js";
import {
  dessinerCarte, dessinerSurcouche, echelle,
} from "./nuage.js";
import {
  autresCarte, couleursFamillesCarte, grisBatiCarte,
} from "../familles.js";

/* ------------------------------------------------- carte MapLibre (module 2)
 *
 * Les tuiles vectorielles remplacent le nuage et la nappe dessinés à la main ;
 * le canevas reste, transparent, pour ce que les tuiles ne portent pas — les
 * chemins, le lasso, les bornes de départ et d'arrivée.
 *
 * **MapLibre n'écoute aucun événement** (`interactive: false`) : le canevas
 * est au-dessus et garde tous les gestionnaires, qu'il relaie. C'est ce qui
 * permet à `versEcran` et `versCarte` (`explorer/nuage.js`), les deux seules
 * transformations de coordonnées, de simplement déléguer, et à tout le reste (lasso,
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
    const m = /^tuiles:\/\/[^/]*\/(carte|relief|(?:dem|courbes)-[a-z]+)\/(\d+)\/(\d+)\/(\d+)/.exec(params.url);
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
    majTracesGL();
    majReliefGL();
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

export {
  bornesGeoPoints, carteDepuisGeo, carteGL, carteReelle, geoDepuisCarte, GRIS_BATI,
  initialiserFiltreAnnee, journalCarte, majAffichageGL, majCouleurGL,
  majEtiquetteIntervalleAnnees, majFiltreGL, majPentesGL, majVisibiliteIntervalleAnnees,
  metresParPixels, poserVignetteCarte, rafraichirFiltreAnnee, synchroniserVue, vueInitialeGL,
  zoomMax,
};
