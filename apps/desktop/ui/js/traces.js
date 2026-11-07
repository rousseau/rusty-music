import "./aura.js"; // l'ordre d'exécution : voir main.js
import {
  $, fileAttente, invoke, navigation, remonter,
} from "./socle.js";
import { carte } from "./carte.js";
import { carteGL } from "./explorer/carte-gl.js";
import { dessinerCarte, versEcran } from "./explorer/nuage.js";
import {
  demarrerLecture, dessinerFile,
} from "./lecteur.js";
import { proposerGarder } from "./lieux.js";

/* ------------------------------------------------- traces et brouillard (P5)
 *
 * `docs/plan-balade.md`, P5 : la carte montre où l'on est passé et ce qui
 * reste inconnu.
 *
 * - **Brouillard** : un voile couleur papier sur le bâti habité par un morceau
 *   jamais écouté (couches `brouillard` et `brouillard-point`, posées masquées
 *   par `style::poser_traces`). Le voile se lève morceau par morceau par
 *   `feature-state` (`ecoute: true`), l'identifiant d'un bâtiment étant son
 *   morceau (`promoteId`) : ni tuile ni filtre à refaire, et chaque écoute
 *   notée (`ecoute-notee`, émis par le moteur) découvre son bâtiment aussitôt.
 * - **Sentiers battus** : les rues parcourues en balade, sous la voirie, plus
 *   épaisses et plus franches avec les passages (`sentiers_carte`).
 * - **Compteur de quartier** : au survol d'un territoire, combien de morceaux
 *   et quelle part jamais écoutée (`territoireSous`, lu par `explorer/nuage.js`).
 * - **Balades passées** : la liste du rail ; un clic redessine la balade, ▶ la
 *   rejoue comme une file ordinaire.
 */

/// Balades listées dans le rail : au-delà, la liste pousserait le zoom hors
/// de vue (Règle 5).
const BALADES_MONTREES = 6;

const traces = {
  brouillard: lireCase("carte-brouillard"),
  sentiers: lireCase("carte-sentiers"),
  ecoutes: new Set(), // morceaux écoutés au moins une fois
  ecoutesLues: false,
  comptes: null, // Map famille → { n, jamais }, refait quand les écoutes changent
  balades: [], // `balades_passees`
  dessinee: null, // { id, pistes, trace } : la balade passée redessinée
};

function lireCase(cle) {
  try {
    return localStorage.getItem(cle) === "1";
  } catch {
    return false;
  }
}

function garderCase(cle, oui) {
  try {
    localStorage.setItem(cle, oui ? "1" : "0");
  } catch {
    // Sans stockage, le réglage vaut pour la session.
  }
}

/* ------------------------------------------------- brouillard */

/// Lève le voile sur le bâtiment de `id` et sur son point de dézoom.
function decouvrir(g, id) {
  // Une source GeoJSON (le style des tests de fumée) n'a pas de couches.
  const couches = g.getSource("carte")?.type === "vector" ? ["batiments", "habites"] : [undefined];
  for (const sourceLayer of couches) {
    try {
      g.setFeatureState({ source: "carte", sourceLayer, id }, { ecoute: true });
    } catch {
      // Source absente (monde fictif) : rien à découvrir.
    }
  }
}

async function lireEcoutes() {
  try {
    traces.ecoutes = new Set(await invoke("morceaux_ecoutes"));
    traces.ecoutesLues = true;
    traces.comptes = null;
  } catch (err) {
    remonter(err, "morceaux écoutés");
  }
}

/// Rejoue l'état des traces sur le style courant : au chargement de la carte,
/// après `setStyle`, et quand une option change.
async function majTracesGL() {
  const g = carte.gl;
  if (!g || !carte.glPret || !g.getLayer("brouillard")) {
    majOptionsTraces(false);
    return;
  }
  majOptionsTraces(true);
  const voir = (couche, oui) => {
    if (g.getLayer(couche)) g.setLayoutProperty(couche, "visibility", oui ? "visible" : "none");
  };
  voir("brouillard", traces.brouillard);
  voir("brouillard-point", traces.brouillard);
  voir("sentiers", traces.sentiers);
  if (!traces.ecoutesLues) await lireEcoutes();
  for (const id of traces.ecoutes) decouvrir(g, id);
  if (traces.sentiers) await majSentiers();
}

async function majSentiers() {
  const g = carte.gl;
  const source = g?.getSource("sentiers");
  if (!source) return;
  try {
    source.setData(await invoke("sentiers_carte"));
  } catch (err) {
    remonter(err, "sentiers battus");
    return;
  }
  // La teinte du promeneur (`dessinerBalade`) : on reconnaît sa propre trace.
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
  if (accent) g.setPaintProperty("sentiers", "line-color", accent);
}

function majOptionsTraces(presentes) {
  const ligne = $("ligne-traces");
  if (ligne) ligne.hidden = !presentes;
  $("carte-brouillard").checked = traces.brouillard;
  $("carte-sentiers").checked = traces.sentiers;
}

$("carte-brouillard").addEventListener("change", (e) => {
  traces.brouillard = e.target.checked;
  garderCase("carte-brouillard", traces.brouillard);
  majTracesGL();
});
$("carte-sentiers").addEventListener("change", (e) => {
  traces.sentiers = e.target.checked;
  garderCase("carte-sentiers", traces.sentiers);
  majTracesGL();
});

// Une écoute vient d'être notée (30 s ou la moitié du morceau, `SuiviEcoute`) :
// son bâtiment sort du brouillard sans rien recharger.
window.__TAURI__?.event?.listen?.("ecoute-notee", (e) => {
  const id = e.payload;
  if (!Number.isFinite(id) || traces.ecoutes.has(id)) return;
  traces.ecoutes.add(id);
  traces.comptes = null;
  if (carte.gl && carte.glPret) decouvrir(carte.gl, id);
});

/* ------------------------------------------------- compteur de quartier */

/// Morceaux logés et jamais écoutés, par famille.
function comptes() {
  if (traces.comptes) return traces.comptes;
  const m = new Map();
  for (const p of carte.points) {
    if (!carte.positionsReelles?.has(p.id)) continue;
    const c = m.get(p.cluster) ?? { n: 0, jamais: 0 };
    c.n += 1;
    if (!traces.ecoutes.has(p.id)) c.jamais += 1;
    m.set(p.cluster, c);
  }
  traces.comptes = m;
  return m;
}

/// Le territoire sous le pixel `(mx, my)`, avec ses comptes : `null` hors du
/// plan de ville, ou quand les quartiers ne sont pas peints (coloration
/// continue, zoom de la façade).
function territoireSous(mx, my) {
  const g = carteGL();
  if (!g || !carte.villeReelle || !traces.ecoutesLues || !g.getLayer("territoires-reels")) return null;
  if (g.getLayoutProperty("territoires-reels", "visibility") === "none") return null;
  const f = g.queryRenderedFeatures([mx, my], { layers: ["territoires-reels"] })[0];
  const famille = f?.properties?.palier;
  if (famille == null || famille < 0) return null;
  const c = comptes().get(famille);
  if (!c?.n) return null;
  const nom = (carte.familles ?? []).find(([r]) => r === famille)?.[1] ?? "";
  return { famille, nom, ...c };
}

/// « 312 morceaux · 41 % jamais écoutés ».
function texteTerritoire(t) {
  const part = Math.round((100 * t.jamais) / t.n);
  return `${t.n} morceau${t.n > 1 ? "x" : ""} · ${part} % jamais écouté${t.jamais > 1 ? "s" : ""}`;
}

/* ------------------------------------------------- balades passées */

const formatDate = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric", month: "short", hour: "2-digit", minute: "2-digit",
});

async function chargerBalades() {
  try {
    traces.balades = await invoke("balades_passees");
  } catch (err) {
    remonter(err, "balades passées");
    traces.balades = [];
  }
  dessinerListeBalades();
}

function dessinerListeBalades() {
  const bloc = $("bloc-balades-passees");
  const liste = $("balades-passees");
  const montrees = traces.balades.slice(0, BALADES_MONTREES);
  bloc.hidden = !montrees.length;
  liste.replaceChildren(
    ...montrees.map((b) => {
      const li = document.createElement("li");
      li.className = "balade-passee";
      const voir = document.createElement("button");
      voir.type = "button";
      voir.className = "balade-passee__voir";
      voir.setAttribute("aria-pressed", String(traces.dessinee?.id === b.id));
      const minutes = Math.round(b.duree_ms / 60_000);
      voir.textContent = `${formatDate.format(new Date(b.debut * 1000))} · ${b.pas} morceaux · ${minutes} min`;
      voir.title = b.titre
        ? `Partie de « ${b.titre} »${b.artiste ? ` — ${b.artiste}` : ""}. Cliquer pour la redessiner.`
        : "Cliquer pour la redessiner.";
      voir.addEventListener("click", () => basculerDessin(b.id));
      const jouer = document.createElement("button");
      jouer.type = "button";
      jouer.className = "balade-passee__jouer";
      jouer.textContent = "▶";
      jouer.setAttribute("aria-label", "Rejouer cette balade");
      jouer.title = "Rejouer cette balade, dans le même ordre";
      jouer.addEventListener("click", () => rejouer(b.id));
      const garderB = document.createElement("button");
      garderB.type = "button";
      garderB.className = "balade-passee__jouer";
      garderB.textContent = "☆";
      garderB.setAttribute("aria-label", "Garder cette balade sur la carte");
      garderB.title = "Garder cette balade sous un nom : une playlist, et son tracé sur la carte";
      garderB.addEventListener("click", () => garderBalade(b));
      li.append(voir, jouer, garderB);
      return li;
    }),
  );
}

async function basculerDessin(id) {
  if (traces.dessinee?.id === id) {
    traces.dessinee = null;
  } else {
    try {
      traces.dessinee = { id, ...(await invoke("balade_passee", { id })) };
    } catch (err) {
      remonter(err, "balade passée");
      return;
    }
    cadrer(traces.dessinee);
  }
  dessinerListeBalades();
  dessinerCarte();
}

/// Amène la balade redessinée dans le champ.
function cadrer(d) {
  const g = carteGL();
  if (!g) return;
  const points = d.trace.length
    ? d.trace
    : d.pistes.map((id) => carte.positionsReelles?.get(id)).filter(Boolean);
  if (points.length < 2) return;
  const lons = points.map((p) => p[0]);
  const lats = points.map((p) => p[1]);
  g.fitBounds(
    [[Math.min(...lons), Math.min(...lats)], [Math.max(...lons), Math.max(...lats)]],
    { padding: 80, maxZoom: 16, duration: 600 },
  );
}

/// « Garder » une balade passée (P8) : elle devient une chaîne.
async function garderBalade(b) {
  let d;
  try {
    d = await invoke("balade_passee", { id: b.id });
  } catch (err) {
    remonter(err, "balade passée");
    return;
  }
  const jour = new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "long" }).format(
    new Date(b.debut * 1000),
  );
  proposerGarder({ origine: "balade", morceaux: d.pistes, trace: d.trace, nom: `Balade du ${jour}` });
}

async function rejouer(id) {
  try {
    await demarrerLecture(() => invoke("balade_rejouer", { id }));
    fileAttente.pistes = await invoke("file_courante");
    dessinerFile();
  } catch (err) {
    remonter(err, "rejouer la balade");
  }
}

/// La balade passée redessinée, sur la surcouche de la carte : un trait
/// pointillé, pour ne pas la confondre avec la balade en cours.
function dessinerBaladePassee(r, encre, accent, ctx) {
  const d = traces.dessinee;
  if (!d || navigation.mode !== "explorer") return;
  const parId = new Map(carte.points.map((p) => [p.id, p]));
  const ecran = d.trace.length && carteGL()
    ? d.trace.map((ll) => versEcran({ x: ll[0], y: ll[1] }, r))
    : d.pistes.map((id) => parId.get(id)).filter(Boolean).map((p) => versEcran(p, r));
  if (ecran.length < 2) return;
  ctx.save();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.setLineDash([6, 5]);
  ctx.strokeStyle = encre;
  ctx.globalAlpha = 0.35;
  ctx.lineWidth = 4;
  ctx.beginPath();
  ecran.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.stroke();
  ctx.strokeStyle = accent;
  ctx.globalAlpha = 0.9;
  ctx.lineWidth = 2;
  ctx.stroke();
  ctx.restore();
}

/// Une balade vient de finir ou de s'arrêter : la liste et les sentiers en
/// tiennent compte.
function rafraichirTraces() {
  chargerBalades();
  if (traces.sentiers) majSentiers();
}

chargerBalades();

export {
  dessinerBaladePassee, majTracesGL, rafraichirTraces, territoireSous, texteTerritoire, traces,
};
