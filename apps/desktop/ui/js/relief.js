import "./traces.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, navigation, remonter,
} from "./socle.js";
import { carte, cnv, ctx } from "./carte.js";
import {
  carteGL, dessinerCarte, poserChemin,
} from "./maplibre.js";
import { allumeeAura, ecouterAutour } from "./aura.js";
import { balade } from "./balade.js";

/* ------------------------------------------------- le relief musical (P6)
 *
 * `docs/plan-balade.md`, P6 : une altitude qui a un sens musical — l'énergie
 * par défaut, le tempo, la popularité ou la familiarité au choix —, ombrée
 * par MapLibre et coupée de courbes de niveau numérotées (l'emprunt à Qoyra).
 * Le moteur écrit les tuiles de la grandeur choisie (`relief_musical`) ;
 * l'interface pointe les sources `dem` et `courbes` dessus (`setTiles`), sans
 * toucher au plan.
 *
 * Les gestes :
 * - un clic **sur** une courbe la désigne : elle s'épaissit, son numéro se
 *   pose en pastille là où l'on a cliqué ;
 * - la molette, tant qu'une courbe est désignée, la suit d'un morceau à
 *   l'autre (le fuseau de Qoyra) ; l'aura, allumée, fait entendre autour ;
 * - un clic sur la pastille « ouvre » la courbe : la playlist de la bande
 *   qu'elle borde, dans ce qu'elle enferme ;
 * - Échap, ou un clic ailleurs, la relâche.
 */

const GRANDEURS = ["energie", "tempo", "popularite", "familiarite"];
/// Un clic à moins de tant de pixels d'un trait le désigne : les courbes sont
/// fines, un clic dans la rue d'à côté doit rester un clic sur la ville.
const COURBE_CLIC_PX = 4;
/// Rayon de la pastille du numéro, pixels.
const PASTILLE_PX = 11;

const relief = {
  actif: lireCase("carte-relief"),
  grandeur: lireChoix("relief-grandeur", GRANDEURS, "energie"),
  version: null, // la version des tuiles affichées, `null` : pas encore demandées
  choisie: null, // `courbe` : { ligne, niveau, sommet, points, le_long }
  pastille: null, // [lon, lat] : où se pose le numéro
  fuseau: -1, // rang dans `choisie.le_long`, -1 : pas encore avancé
};

function lireCase(cle) {
  try {
    return localStorage.getItem(cle) === "1";
  } catch {
    return false;
  }
}

function lireChoix(cle, valeurs, defaut) {
  try {
    const v = localStorage.getItem(cle);
    return valeurs.includes(v) ? v : defaut;
  } catch {
    return defaut;
  }
}

function garder(cle, valeur) {
  try {
    localStorage.setItem(cle, valeur);
  } catch {
    // Sans stockage, le réglage vaut pour la session.
  }
}

const COUCHES = ["relief-ombre", "courbes", "courbes-numero"];

/// Rejoue l'état du relief sur le style courant : au chargement de la carte,
/// après `setStyle`, et quand l'option ou la grandeur change.
async function majReliefGL() {
  const g = carte.gl;
  const present = !!(g && carte.glPret && g.getLayer("relief-ombre"));
  $("ligne-relief").hidden = !present;
  $("carte-relief").checked = relief.actif;
  $("relief-grandeur").hidden = !present || !relief.actif;
  document
    .querySelectorAll("#relief-grandeur [data-grandeur]")
    .forEach((b) => b.classList.toggle("segment--actif", b.dataset.grandeur === relief.grandeur));
  if (!present) return;
  if (relief.actif) {
    let v;
    try {
      v = await invoke("relief_musical", { grandeur: relief.grandeur });
    } catch (err) {
      remonter(err, "relief musical");
      return;
    }
    if (!v) return;
    if (v.version !== relief.version) {
      relief.version = v.version;
      relacher();
    }
    const tuiles = (genre) => [`tuiles://tuiles/${genre}-${relief.grandeur}/{z}/{x}/{y}?v=${v.version}`];
    g.getSource("dem")?.setTiles?.(tuiles("dem"));
    g.getSource("courbes")?.setTiles?.(tuiles("courbes"));
  } else {
    relacher();
  }
  for (const c of COUCHES)
    if (g.getLayer(c)) g.setLayoutProperty(c, "visibility", relief.actif ? "visible" : "none");
  majChoisieGL();
}

function majChoisieGL() {
  const g = carte.gl;
  if (!g?.getLayer("courbes-choisie")) return;
  const id = relief.choisie?.ligne ?? -1;
  g.setFilter("courbes-choisie", ["==", ["get", "ligne"], id]);
  g.setLayoutProperty("courbes-choisie", "visibility", relief.actif && relief.choisie ? "visible" : "none");
}

$("carte-relief").addEventListener("change", (e) => {
  relief.actif = e.target.checked;
  garder("carte-relief", relief.actif ? "1" : "0");
  majReliefGL();
});

document.querySelectorAll("#relief-grandeur [data-grandeur]").forEach((b) =>
  b.addEventListener("click", async () => {
    if (b.dataset.grandeur === relief.grandeur) return;
    relief.grandeur = b.dataset.grandeur;
    garder("relief-grandeur", relief.grandeur);
    await majReliefGL();
    // Les allures du relief suivent la grandeur affichée.
    if (balade.vue) {
      try {
        const vue = await invoke("balade_regler", { grandeur: relief.grandeur });
        if (vue) balade.vue = vue;
      } catch (err) {
        remonter(err, "balade_regler");
      }
    }
  }),
);

/* ------------------------------------------------- la courbe désignée */

function relacher() {
  if (!relief.choisie) return;
  relief.choisie = null;
  relief.pastille = null;
  relief.fuseau = -1;
  majChoisieGL();
  $("fil-compte").textContent = "";
  dessinerCarte();
}

/// La courbe sous le pixel `(mx, my)`, ou `null`.
function courbeSous(mx, my) {
  const g = carteGL();
  if (!g || !relief.actif || !g.getLayer("courbes")) return null;
  const d = COURBE_CLIC_PX;
  const f = g.queryRenderedFeatures(
    [[mx - d, my - d], [mx + d, my + d]],
    { layers: ["courbes"] },
  )[0];
  return f?.properties?.ligne ?? null;
}

/// Le relief vit en `[lon, lat]` : MapLibre projette lui-même.
function lonlatEcran(ll) {
  const q = carte.gl.project(ll);
  return [q.x, q.y];
}

/// Le point de la courbe le plus proche de l'écran `(mx, my)`, `[lon, lat]` :
/// projeté sur le segment le plus proche, pas le sommet le plus proche.
function pointProche(points, mx, my, r) {
  let meilleur = points[0];
  let d2 = Infinity;
  for (let i = 1; i < points.length; i++) {
    const [ax, ay] = lonlatEcran(points[i - 1], r);
    const [bx, by] = lonlatEcran(points[i], r);
    const l2 = (bx - ax) ** 2 + (by - ay) ** 2;
    const t = l2 ? Math.max(0, Math.min(1, ((mx - ax) * (bx - ax) + (my - ay) * (by - ay)) / l2)) : 0;
    const d = (ax + t * (bx - ax) - mx) ** 2 + (ay + t * (by - ay) - my) ** 2;
    if (d < d2) {
      d2 = d;
      const [a, b] = [points[i - 1], points[i]];
      meilleur = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
    }
  }
  return meilleur;
}

async function designer(ligne, mx, my) {
  let c;
  try {
    c = await invoke("courbe", { ligne });
  } catch (err) {
    remonter(err, "courbe");
    return;
  }
  if (!c) return;
  const r = cnv.getBoundingClientRect();
  relief.choisie = c;
  relief.pastille = pointProche(c.points, mx, my, r);
  relief.fuseau = -1;
  majChoisieGL();
  const n = c.le_long.length;
  $("fil-compte").textContent =
    `Courbe ${c.niveau} · ${n} morceau${n > 1 ? "x" : ""} le long · ` +
    `molette : la suivre · clic sur ${c.niveau} : l'ouvrir · Échap : la relâcher`;
  dessinerCarte();
}

function surPastille(mx, my) {
  if (!relief.choisie || !relief.pastille) return false;
  const [x, y] = lonlatEcran(relief.pastille, cnv.getBoundingClientRect());
  return Math.hypot(x - mx, y - my) <= PASTILLE_PX + 3;
}

async function ouvrir() {
  const c = relief.choisie;
  let pistes;
  try {
    pistes = await invoke("ouvrir_courbe", { ligne: c.ligne });
  } catch (err) {
    remonter(err, "ouvrir la courbe");
    return;
  }
  const n = c.niveau;
  const bande = c.sommet ? `au-dessus de ${n}` : `sous ${n}`;
  relacher();
  // Comme une sélection au lasso (`jouerSelection`) : la playlist part, son
  // parcours se trace.
  await poserChemin(pistes, `rien dans la bande ${bande}`);
  if (pistes?.length >= 2) $("fil-compte").textContent = `${pistes.length} morceaux ${bande}, dans la courbe`;
}

/// La molette suit la courbe : un cran, un morceau.
function avancer(sens) {
  const c = relief.choisie;
  if (!c?.le_long.length) return;
  const n = c.le_long.length;
  relief.fuseau = relief.fuseau < 0 ? (sens > 0 ? 0 : n - 1) : (relief.fuseau + sens + n) % n;
  const riv = c.le_long[relief.fuseau];
  const g = carteGL();
  const r = cnv.getBoundingClientRect();
  let [x, y] = lonlatEcran(riv.point, r);
  // Le fuseau reste dans le champ.
  if (g && (x < 40 || y < 40 || x > r.width - 40 || y > r.height - 40)) {
    g.panTo(riv.point, { duration: 300 });
    x = r.width / 2;
    y = r.height / 2;
  }
  const p = carte.points.find((q) => q.id === riv.morceau);
  if (p) {
    const info = $("carte-info");
    info.hidden = false;
    info.replaceChildren(
      Object.assign(document.createElement("b"), { textContent: p.title || "(sans titre)" }),
      Object.assign(document.createElement("span"), {
        textContent: `${p.artist || "(sans artiste)"} · ${relief.fuseau + 1} / ${n} sur la courbe ${c.niveau}`,
      }),
    );
    info.style.left = `${Math.min(x + 14, r.width - 290)}px`;
    info.style.top = `${y + 14}px`;
  }
  // L'aura suit le fuseau, si elle est allumée.
  if (allumeeAura()) ecouterAutour(x, y);
  dessinerCarte();
}

/* ------------------------------------------------- gestes */

// En capture sur le conteneur : avant les gestes de la carte (`maplibre.js`).
const zone = cnv.parentElement;

zone.addEventListener(
  "wheel",
  (e) => {
    if (!relief.choisie || e.target !== cnv) return;
    e.preventDefault();
    e.stopPropagation();
    if (Math.abs(e.deltaY) < 1) return;
    avancer(e.deltaY > 0 ? 1 : -1);
  },
  { capture: true, passive: false },
);

zone.addEventListener(
  "click",
  (e) => {
    if (e.target !== cnv || e.shiftKey || e.altKey || navigation.mode !== "explorer") return;
    const r = cnv.getBoundingClientRect();
    const mx = e.clientX - r.left;
    const my = e.clientY - r.top;
    if (surPastille(mx, my)) {
      e.stopPropagation();
      ouvrir();
      return;
    }
    const ligne = courbeSous(mx, my);
    if (ligne != null) {
      e.stopPropagation();
      designer(ligne, mx, my);
      return;
    }
    if (relief.choisie) {
      // Un clic ailleurs relâche la courbe, et ne fait rien d'autre.
      e.stopPropagation();
      relacher();
    }
  },
  { capture: true },
);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && relief.choisie) relacher();
});

/// Sur la surcouche de la carte : la pastille du numéro, et le fuseau.
function dessinerRelief(r, encre, accent) {
  const c = relief.choisie;
  if (!c || !relief.pastille || !carteGL()) return;
  ctx.save();
  if (relief.fuseau >= 0) {
    const [fx, fy] = lonlatEcran(c.le_long[relief.fuseau].point, r);
    ctx.fillStyle = accent;
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(fx, fy, 6, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }
  const [x, y] = lonlatEcran(relief.pastille, r);
  const fond = getComputedStyle(document.documentElement).getPropertyValue("--panel").trim() || "#fff";
  ctx.fillStyle = fond;
  ctx.strokeStyle = "#9C7C5E";
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.arc(x, y, PASTILLE_PX, 0, Math.PI * 2);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = encre;
  ctx.font = "600 13px system-ui";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(String(c.niveau), x, y + 0.5);
  ctx.restore();
}

export { dessinerRelief, majReliefGL, relief };
