import "./relief.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, navigation, remonter,
} from "./socle.js";
import { carte, cnv, ctx } from "./carte.js";
import { carteGL, dessinerCarte } from "./maplibre.js";
import { jouerPlaylist } from "./ecouter.js";
import { fermerMenuBalade } from "./balade.js";

/* ------------------------------------------------- des lieux à soi (P8)
 *
 * `docs/plan-balade.md`, P8 : ce que l'on pose soi-même sur la carte, sans
 * déplacer un morceau.
 *
 * - **Épingles** : clic droit « Épingler ici », un nom, une couleur, un emoji
 *   facultatif. Un repère à tous les zooms ; clic droit dessus : « Flâner
 *   d'ici » (depuis le morceau le plus proche) ou « Retirer l'épingle ».
 * - **Chaînes** : une balade passée ou un itinéraire, « Garder » sous un nom.
 *   C'est une playlist (elle paraît dans la vue Playlists) avec un tracé
 *   permanent sur la carte ; ▶ la rejoue.
 *
 * Les deux vivent en `[lon, lat]` dans la base et se dessinent sur la
 * surcouche : rien ne dépend des tuiles ni du style, qu'on peut régénérer.
 */

/// Six teintes au choix, lisibles sur les fonds clairs comme sur la nuit.
const NUANCIER = ["#C4552D", "#D9A21B", "#3E8E5B", "#2F6FB0", "#8A4FB3", "#4A4A4A"];
/// Un clic droit à moins de tant de pixels d'une épingle la vise.
const EPINGLE_PX = 12;

const lieux = {
  epingles: [],
  chaines: [],
  couleurEpingle: NUANCIER[0],
  couleurChaine: NUANCIER[3],
  ici: null, // [lon, lat] de l'épingle en cours de création
  aGarder: null, // { origine, morceaux, trace, nom } : la chaîne en cours de création
};

async function charger() {
  try {
    [lieux.epingles, lieux.chaines] = await Promise.all([invoke("epingles"), invoke("chaines")]);
  } catch (err) {
    remonter(err, "épingles et chaînes");
    return;
  }
  lieux.epingles ??= [];
  lieux.chaines ??= [];
  dessinerListeChaines();
  if (navigation.mode === "explorer") dessinerCarte();
}

/* ------------------------------------------------- géométrie */

/// Les épingles et les chaînes vivent en `[lon, lat]` : sur le plan de ville
/// seulement, où MapLibre projette lui-même.
const surLePlan = () => !!carteGL() && carte.villeReelle;

function ecran(ll) {
  const q = carte.gl.project(ll);
  return [q.x, q.y];
}

function peutEpingler() {
  return surLePlan();
}

function lonlatSous(mx, my) {
  const g = carteGL();
  if (!g) return null;
  const ll = g.unproject([mx, my]);
  return [ll.lng, ll.lat];
}

function epingleSous(mx, my) {
  if (!surLePlan()) return null;
  let meilleure = null;
  let d2 = EPINGLE_PX ** 2;
  for (const e of lieux.epingles) {
    const [x, y] = ecran([e.lon, e.lat]);
    const d = (x - mx) ** 2 + (y - my) ** 2;
    if (d <= d2) {
      d2 = d;
      meilleure = e;
    }
  }
  return meilleure;
}

/// Le morceau logé le plus proche d'une épingle : d'où part « Flâner d'ici ».
function morceauPres(e) {
  let meilleur = null;
  let d2 = Infinity;
  const kx = Math.cos((e.lat * Math.PI) / 180);
  for (const p of carte.points) {
    const q = carte.positionsReelles?.get(p.id);
    if (!q) continue;
    const d = ((q[0] - e.lon) * kx) ** 2 + (q[1] - e.lat) ** 2;
    if (d < d2) {
      d2 = d;
      meilleur = p;
    }
  }
  return meilleur;
}

/* ------------------------------------------------- nuancier */

function nuancier(conteneur, choisie, choisir) {
  conteneur.replaceChildren(
    ...NUANCIER.map((c) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "nuancier__teinte";
      b.style.background = c;
      b.dataset.couleur = c;
      b.setAttribute("aria-label", `Couleur ${c}`);
      b.setAttribute("aria-pressed", String(c === choisie));
      b.addEventListener("click", () => {
        choisir(c);
        conteneur
          .querySelectorAll("button")
          .forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.couleur === c)));
      });
      return b;
    }),
  );
}

/* ------------------------------------------------- épingles */

function ouvrirFormEpingle(ici) {
  lieux.ici = ici;
  $("carte-menu-form").hidden = false;
  for (const id of ["carte-menu-flaner", "carte-menu-epingler", "carte-menu-retirer"]) $(id).hidden = true;
  $("epingle-nom").value = "";
  $("epingle-emoji").value = "";
  nuancier($("epingle-nuancier"), lieux.couleurEpingle, (c) => (lieux.couleurEpingle = c));
  $("epingle-nom").focus();
}

function fermerFormEpingle() {
  $("carte-menu-form").hidden = true;
  lieux.ici = null;
}

async function poserEpingle() {
  if (!lieux.ici) return;
  const [lon, lat] = lieux.ici;
  try {
    const e = await invoke("creer_epingle", {
      nom: $("epingle-nom").value,
      couleur: lieux.couleurEpingle,
      emoji: $("epingle-emoji").value || null,
      lon,
      lat,
    });
    if (e) lieux.epingles.push(e);
  } catch (err) {
    remonter(err, "creer_epingle");
  }
  fermerFormEpingle();
  fermerMenuBalade();
  dessinerCarte();
}

async function retirerEpingle(e) {
  try {
    await invoke("supprimer_epingle", { id: e.id });
  } catch (err) {
    remonter(err, "supprimer_epingle");
    return;
  }
  lieux.epingles = lieux.epingles.filter((x) => x.id !== e.id);
  dessinerCarte();
}

$("epingle-poser").addEventListener("click", poserEpingle);
for (const id of ["epingle-nom", "epingle-emoji"])
  $(id).addEventListener("keydown", (e) => {
    if (e.key === "Enter") poserEpingle();
  });

/* ------------------------------------------------- chaînes */

/// Ouvre le formulaire « Garder » pour une balade passée ou un itinéraire.
/// `trace` en `[lon, lat]` ; `morceaux` : leurs identifiants, dans l'ordre.
function proposerGarder({ origine, morceaux, trace, nom }) {
  lieux.aGarder = { origine, morceaux, trace };
  const f = $("carte-garder");
  f.hidden = false;
  $("garder-nom").value = nom;
  nuancier($("garder-nuancier"), lieux.couleurChaine, (c) => (lieux.couleurChaine = c));
  $("garder-nom").focus();
  $("garder-nom").select();
}

function fermerGarder() {
  $("carte-garder").hidden = true;
  lieux.aGarder = null;
}

async function garder() {
  const g = lieux.aGarder;
  if (!g) return;
  try {
    await invoke("garder_chaine", {
      nom: $("garder-nom").value,
      couleur: lieux.couleurChaine,
      origine: g.origine,
      morceaux: g.morceaux,
      trace: g.trace,
    });
  } catch (err) {
    remonter(err, "garder_chaine");
    return;
  }
  fermerGarder();
  await charger();
}

$("garder-ok").addEventListener("click", garder);
$("garder-annuler").addEventListener("click", fermerGarder);
$("garder-nom").addEventListener("keydown", (e) => {
  if (e.key === "Enter") garder();
  if (e.key === "Escape") fermerGarder();
});

/// L'itinéraire affiché (`carte.route`, `carte.routeTrace`), en `[lon, lat]`.
function itineraireAffiche() {
  if (!carte.route || carte.route.length < 2 || !carte.villeReelle) return null;
  const ll = (p) => (p.id != null ? carte.positionsReelles?.get(p.id) : [p.x, p.y]);
  const trace = (carte.routeTrace ?? carte.route).map(ll).filter(Boolean);
  return { morceaux: carte.route.map((p) => p.id), trace };
}

$("chemin-garder").addEventListener("click", () => {
  const it = itineraireAffiche();
  if (!it) return;
  proposerGarder({ origine: "itineraire", ...it, nom: "Itinéraire" });
});

function majBoutonGarderItineraire() {
  const b = $("chemin-garder");
  if (b) b.hidden = !itineraireAffiche();
}

function dessinerListeChaines() {
  const liste = $("chaines");
  $("bloc-chaines").hidden = !lieux.chaines.length;
  liste.replaceChildren(
    ...lieux.chaines.map((c) => {
      const li = document.createElement("li");
      li.className = "balade-passee";
      const voir = document.createElement("button");
      voir.type = "button";
      voir.className = "balade-passee__voir";
      const pastille = document.createElement("span");
      pastille.className = "chaine__teinte";
      pastille.style.background = c.couleur;
      voir.append(pastille, `${c.nom} · ${c.nb_pistes} morceaux`);
      voir.title = "Cadrer la chaîne sur la carte";
      voir.addEventListener("click", () => cadrer(c.trace));
      const jouer = document.createElement("button");
      jouer.type = "button";
      jouer.className = "balade-passee__jouer";
      jouer.textContent = "▶";
      jouer.setAttribute("aria-label", `Rejouer « ${c.nom} »`);
      jouer.title = "Rejouer cette chaîne";
      jouer.addEventListener("click", () =>
        jouerPlaylist({ id: c.playlist_id }).catch((err) => remonter(err, "rejouer la chaîne")),
      );
      const oublier = document.createElement("button");
      oublier.type = "button";
      oublier.className = "balade-passee__jouer";
      oublier.textContent = "✕";
      oublier.setAttribute("aria-label", `Retirer « ${c.nom} » de la carte`);
      oublier.title = "Retirer le tracé de la carte (la playlist reste)";
      oublier.addEventListener("click", async () => {
        try {
          await invoke("oublier_trace", { id: c.playlist_id });
        } catch (err) {
          remonter(err, "oublier_trace");
          return;
        }
        await charger();
      });
      li.append(voir, jouer, oublier);
      return li;
    }),
  );
}

function cadrer(trace) {
  const g = carteGL();
  if (!g || trace.length < 2) return;
  const lons = trace.map((p) => p[0]);
  const lats = trace.map((p) => p[1]);
  g.fitBounds(
    [[Math.min(...lons), Math.min(...lats)], [Math.max(...lons), Math.max(...lats)]],
    { padding: 80, maxZoom: 16, duration: 600 },
  );
}

/* ------------------------------------------------- dessin */

/// Sur la surcouche de la carte, sous les balades : les chaînes, puis les
/// épingles par-dessus, à tous les zooms.
function dessinerLieux(r, encre) {
  majBoutonGarderItineraire();
  if (!surLePlan()) return;
  ctx.save();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  for (const c of lieux.chaines) {
    if (c.trace.length < 2) continue;
    const pts = c.trace.map(ecran);
    ctx.beginPath();
    pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.strokeStyle = encre;
    ctx.globalAlpha = 0.3;
    ctx.lineWidth = 6;
    ctx.stroke();
    ctx.strokeStyle = c.couleur;
    ctx.globalAlpha = 0.85;
    ctx.lineWidth = 3;
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.font = "12px system-ui"; // Règle 11 : rien sous 12 px
  ctx.textBaseline = "middle";
  const fond = getComputedStyle(document.documentElement).getPropertyValue("--panel").trim() || "#fff";
  for (const e of lieux.epingles) {
    const [x, y] = ecran([e.lon, e.lat]);
    if (x < -20 || y < -20 || x > r.width + 20 || y > r.height + 20) continue;
    ctx.fillStyle = e.couleur;
    ctx.strokeStyle = fond;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 9, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
    ctx.textAlign = "center";
    ctx.fillStyle = "#fff";
    ctx.fillText(e.emoji || e.nom.charAt(0).toUpperCase(), x, y + 0.5);
    // Le nom, sur un halo du fond.
    ctx.textAlign = "left";
    ctx.lineWidth = 3;
    ctx.strokeStyle = fond;
    ctx.strokeText(e.nom, x + 13, y);
    ctx.fillStyle = encre;
    ctx.fillText(e.nom, x + 13, y);
  }
  ctx.restore();
}

charger();

export {
  dessinerLieux, epingleSous, fermerFormEpingle, lieux, lonlatSous, morceauPres, ouvrirFormEpingle,
  peutEpingler, proposerGarder, retirerEpingle,
};
