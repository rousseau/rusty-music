import "./itineraires.js"; // l'ordre d'exécution : voir main.js
import {
  $, fileAttente, invoke, navigation, remonter,
} from "./socle.js";
import {
  carte, cnv, ctx,
} from "./carte.js";
import {
  carteGL, dessinerCarte, pointSous, tracerRouteSurCarte, versEcran,
} from "./maplibre.js";
import {
  demarrerLecture, dessinerFile,
} from "./lecteur.js";
import { lecture } from "./etat-lecture.js";

/* ------------------------------------------------- la balade (P1)
 *
 * `docs/plan-balade.md`, P1 : la lecture est une marche. Le moteur choisit le
 * morceau suivant depuis la position courante (`explorer/balade.rs`) ; ce
 * module ne fait que l'entrée (« Flâner d'ici » dans l'inspecteur et au clic
 * droit sur la carte), la sortie (« Arrêter la balade ») et le dessin : le
 * promeneur, qui avance le long de la rue au rythme de la lecture, et la trace
 * de ce qui a déjà été parcouru.
 *
 * Rien n'est calculé à chaque battement côté moteur : le sondage de lecture
 * existant (`battement`) donne la position, `balade_etat` n'est relu qu'au
 * changement de morceau et toutes les 2 s.
 */

const balade = {
  vue: null, // dernière réponse de `balade_etat`, `null` sans balade
  luA: 0, // `performance.now()` de la dernière lecture
  courantVu: null, // le chemin joué lors de la dernière lecture
  dessineA: 0,
};

/// Lance la balade depuis le morceau `id`.
async function flanerDici(id) {
  if (!Number.isFinite(id)) return;
  fermerMenuBalade();
  try {
    await demarrerLecture(async () => {
      balade.vue = await invoke("balade_demarrer", { depart: id });
    });
  } catch (err) {
    remonter(err, "balade_demarrer");
    return;
  }
  balade.luA = performance.now();
  // La route d'un itinéraire précédent n'a plus rien à dire : la balade trace
  // sa propre ligne.
  tracerRouteSurCarte(null);
  try {
    fileAttente.pistes = await invoke("file_courante");
    dessinerFile();
  } catch (err) {
    remonter(err, "file courante (balade)");
  }
  majBandeau();
  redessiner(true);
}

async function arreterBalade() {
  try {
    await invoke("balade_arreter");
  } catch (err) {
    remonter(err, "balade_arreter");
  }
  balade.vue = null;
  majBandeau();
  redessiner(true);
}

async function relireBalade() {
  balade.luA = performance.now();
  balade.courantVu = lecture.courant;
  try {
    balade.vue = await invoke("balade_etat");
  } catch (err) {
    remonter(err, "balade_etat");
    return;
  }
  majBandeau();
}

/// Appelé à chaque battement du lecteur.
function majBalade() {
  const maintenant = performance.now();
  const relire =
    lecture.courant !== balade.courantVu || (balade.vue && maintenant - balade.luA > 2000);
  if (relire) relireBalade().then(() => redessiner(true));
  else redessiner(false);
}

/// La carte, au plus une fois par seconde : le promeneur avance de quelques
/// mètres par minute, pas besoin de plus.
function redessiner(force) {
  if (navigation.mode !== "explorer") return;
  const maintenant = performance.now();
  if (!force && (!balade.vue || maintenant - balade.dessineA < 1000)) return;
  balade.dessineA = maintenant;
  dessinerCarte();
}

/// Le bandeau de la carte : où en est la balade, et de quoi l'arrêter.
function majBandeau() {
  const bandeau = $("carte-balade");
  const v = balade.vue;
  bandeau.hidden = !v;
  if (!v) return;
  const n = v.pistes.length;
  $("carte-balade-texte").textContent = v.fin
    ? `Balade finie : ${v.fin}`
    : `Balade · ${n} morceau${n > 1 ? "x" : ""}${v.terrain === "voisins" ? " · par les voisins" : ""}`;
}

/* ------------------------------------------------- dessin */

const parId = () => new Map(carte.points.map((p) => [p.id, p]));

/// Le point à la fraction `t` d'une ligne d'écran (longueur cumulée).
function lePointA(ecran, t) {
  if (ecran.length === 1) return ecran[0];
  const longueurs = [0];
  for (let i = 1; i < ecran.length; i++) {
    const [ax, ay] = ecran[i - 1];
    const [bx, by] = ecran[i];
    longueurs.push(longueurs[i - 1] + Math.hypot(bx - ax, by - ay));
  }
  const total = longueurs[longueurs.length - 1];
  if (total === 0) return ecran[0];
  const vise = Math.min(1, Math.max(0, t)) * total;
  let i = 1;
  while (i < longueurs.length - 1 && longueurs[i] < vise) i++;
  const f = (vise - longueurs[i - 1]) / Math.max(1e-9, longueurs[i] - longueurs[i - 1]);
  const [ax, ay] = ecran[i - 1];
  const [bx, by] = ecran[i];
  return [ax + (bx - ax) * f, ay + (by - ay) * f];
}

function trait(points) {
  ctx.beginPath();
  points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.stroke();
}

/// Appelé par la surcouche de la carte (`surcoucheSur`), sous le halo du
/// morceau en écoute. Sur le plan de ville, la trace et le pas du promeneur
/// suivent les rues ; sur le nuage (ou dans le graphe des voisins), la droite
/// d'un morceau au suivant — les points des rues sont des lon/lat, sans
/// rapport avec le repère t-SNE.
function dessinerBalade(r, encre, accent) {
  const v = balade.vue;
  if (!v || !carte.points.length) return;
  const index = parId();
  const rues = !!carteGL() && carte.villeReelle && v.terrain === "ville";
  const ecranDe = (p) => versEcran(p, r);
  const lonlat = (ll) => versEcran({ x: ll[0], y: ll[1] }, r);

  // La trace : ce qui a déjà été parcouru, en retrait.
  const trace = rues
    ? v.trace.map(lonlat)
    : v.pistes.map((id) => index.get(id)).filter(Boolean).map(ecranDe);
  ctx.save();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  if (trace.length > 1) {
    ctx.strokeStyle = encre;
    ctx.globalAlpha = 0.4;
    ctx.lineWidth = 4;
    trait(trace);
    ctx.strokeStyle = accent;
    ctx.globalAlpha = 0.7;
    ctx.lineWidth = 1.75;
    trait(trace);
  }

  // Le pas en cours : du morceau joué au suivant.
  const courant = index.get(v.pistes[v.pistes.length - 1]);
  const suivant = v.suivant != null ? index.get(v.suivant) : null;
  let pas = [];
  if (rues && v.troncon.length) pas = v.troncon.map(lonlat);
  else if (courant && suivant) pas = [ecranDe(courant), ecranDe(suivant)];
  else if (courant) pas = [ecranDe(courant)];
  if (pas.length > 1) {
    ctx.strokeStyle = accent;
    ctx.globalAlpha = 0.45;
    ctx.lineWidth = 1.5;
    trait(pas);
  }

  // Le promeneur : à la fraction jouée du morceau en cours. Le relevé de la
  // balade peut retarder d'un battement sur la lecture : si le suivant a déjà
  // commencé, il est au bout du pas, jamais revenu au début.
  const joue = carte.points.find((p) => p.path === lecture.courant);
  let t = null;
  if (joue && courant && joue.id === courant.id) {
    t = courant.duration_ms ? lecture.positionMs / courant.duration_ms : 0;
  } else if (joue && suivant && joue.id === suivant.id) {
    t = 1;
  }
  if (t != null && pas.length) {
    const [x, y] = lePointA(pas, t);
    ctx.globalAlpha = 1;
    ctx.fillStyle = accent;
    ctx.beginPath();
    ctx.arc(x, y, 6, 0, Math.PI * 2);
    ctx.fill();
    ctx.lineWidth = 2;
    ctx.strokeStyle = encre;
    ctx.stroke();
  }
  ctx.restore();
}

/* ------------------------------------------------- gestes */

$("insp-balade").addEventListener("click", () => {
  const id = Number($("insp-titre").dataset.id);
  if ($("insp-titre").dataset.id === "") return;
  flanerDici(id);
});

$("carte-balade-arreter").addEventListener("click", arreterBalade);

/// Le clic droit sur un morceau de la carte : un menu d'une ligne.
let cibleMenu = null;
cnv.addEventListener("contextmenu", (e) => {
  const r = cnv.getBoundingClientRect();
  const p = pointSous(e.clientX - r.left, e.clientY - r.top);
  if (!p) {
    fermerMenuBalade();
    return;
  }
  e.preventDefault();
  cibleMenu = p;
  const menu = $("carte-menu");
  const zone = cnv.parentElement.getBoundingClientRect();
  menu.style.left = `${e.clientX - zone.left}px`;
  menu.style.top = `${e.clientY - zone.top}px`;
  menu.hidden = false;
  $("carte-menu-flaner").focus();
});

function fermerMenuBalade() {
  $("carte-menu").hidden = true;
  cibleMenu = null;
}

$("carte-menu-flaner").addEventListener("click", () => {
  if (cibleMenu) flanerDici(cibleMenu.id);
});
document.addEventListener("pointerdown", (e) => {
  if (!$("carte-menu").hidden && !$("carte-menu").contains(e.target)) fermerMenuBalade();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("carte-menu").hidden) fermerMenuBalade();
});

// Une balade reprise au démarrage (`balade_reprendre`) : la relire une fois.
setTimeout(() => relireBalade().then(() => redessiner(true)), 1500);

export { arreterBalade, balade, dessinerBalade, flanerDici, majBalade };
