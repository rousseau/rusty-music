import "./balade.js"; // l'ordre d'exécution : voir main.js
import { $, invoke, navigation, remonter } from "./socle.js";
import { carte, cnv } from "./carte.js";
import { journalCarte, modeAnneau, modeTemps, versEcran } from "./maplibre.js";
import { balade } from "./balade.js";
import { lecture, transport } from "./etat-lecture.js";

/* ------------------------------------------------- l'aura (P4)
 *
 * `docs/plan-balade.md`, P4 : entendre la carte. Les trois morceaux les plus
 * proches d'un point jouent ensemble, très bas, le plus proche dominant,
 * chacun placé à gauche ou à droite selon sa direction. Le point est le
 * curseur à son arrêt (exploration libre) ou le promeneur (balade).
 *
 * Les voisins se cherchent ici, sur les positions d'écran déjà chargées ; le
 * moteur (`aura`, `explorer/aura.rs`) ne fait que charger les extraits et
 * fondre les voix. Le haut-parleur de la carte l'allume ou l'éteint : éteint
 * par défaut hors balade (le clic reste silencieux), allumé en balade, sous le
 * morceau principal. Hors balade, un morceau qui joue a la main : l'aura se
 * tait.
 */

/// Au-delà, un morceau n'est plus un voisin.
const RAYON_PX = 140;
/// Distance où le gain tombe de moitié : `1 / (1 + (d / d0)²)`.
const D0_PX = 40;
/// Le curseur est arrêté quand il n'a pas bougé depuis…
const ARRET_MS = 120;
const VOIX = 3;

const aura = {
  /// Le choix fait au haut-parleur ; `null` : le défaut du moment.
  choisie: null,
  /// La balade marchait au dernier relevé : son départ et son arrêt
  /// rendent le haut-parleur à son défaut.
  enBalade: false,
  minuterie: null,
  /// La dernière demande envoyée, pour ne pas la renvoyer.
  envoyee: "",
  /// Mesures, relevées au journal toutes les 20 demandes.
  mesures: [],
};

function allumee() {
  return aura.choisie ?? !!balade.vue;
}

/// Hors balade, un morceau qui joue au premier plan fait taire l'aura.
function couverte() {
  return !balade.vue && transport.veutJouer && !!lecture.courant;
}

function surLaCarte() {
  return navigation.mode === "explorer" && !modeTemps() && !modeAnneau() && carte.points.length > 0;
}

/// Les voisins de `(x, y)` à l'écran, du plus proche au plus lointain.
function voisins(x, y, exclus) {
  const r = cnv.getBoundingClientRect();
  const proches = [];
  for (const p of carte.points) {
    if (exclus.has(p.id)) continue;
    if (carte.isolees.size > 0 && !carte.isolees.has(p.cluster)) continue;
    const [px, py] = versEcran(p, r);
    const d = Math.hypot(px - x, py - y);
    if (d <= RAYON_PX) proches.push({ id: p.id, d, dx: px - x });
  }
  proches.sort((a, b) => a.d - b.d);
  return proches.slice(0, VOIX).map(({ id, d, dx }) => ({
    id,
    gain: 1 / (1 + (d / D0_PX) ** 2),
    pan: Math.max(-1, Math.min(1, dx / RAYON_PX)),
  }));
}

function envoyer(voix, msVoisins = 0) {
  const cle = voix.map((v) => `${v.id}:${v.gain.toFixed(2)}:${v.pan.toFixed(2)}`).join(" ");
  if (cle === aura.envoyee) return;
  aura.envoyee = cle;
  const t = performance.now();
  invoke("aura", { voix })
    .then((msMoteur) => {
      if (!voix.length) return;
      aura.mesures.push([msVoisins, msMoteur, performance.now() - t + msVoisins]);
      if (aura.mesures.length >= 20) {
        const med = (i) => aura.mesures.map((m) => m[i]).sort((a, b) => a - b)[10];
        journalCarte(
          `aura : voisins ${med(0).toFixed(1)} ms, moteur ${med(1).toFixed(1)} ms, ` +
            `arrêt → voix ${med(2).toFixed(1)} ms (médianes sur 20)`,
        );
        aura.mesures = [];
      }
    })
    .catch((err) => remonter(err, "aura"));
}

function taire() {
  clearTimeout(aura.minuterie);
  envoyer([]);
}

/// Autour d'un point de l'écran, sans les morceaux `exclus`.
function ecouterAutour(x, y, exclus = new Set()) {
  const t = performance.now();
  const voix = voisins(x, y, exclus);
  envoyer(voix, performance.now() - t);
}

function majHautParleur() {
  const b = $("carte-aura");
  if (!b) return;
  // Le streamgraph et l'anneau n'ont pas de morceaux à entendre autour.
  b.hidden = modeTemps() || modeAnneau();
  const allume = allumee();
  b.setAttribute("aria-pressed", String(allume));
  b.classList.toggle("carte__aura--couverte", allume && couverte());
  b.title = !allume
    ? "Aura éteinte : entendre, très bas, les morceaux autour du curseur"
    : couverte()
      ? "Aura en attente : un morceau joue ; elle revient quand il s'arrête"
      : balade.vue
        ? "Aura : les voisins du promeneur, sous le morceau en cours"
        : "Aura : les morceaux autour du curseur, à son arrêt";
}

$("carte-aura")?.addEventListener("click", () => {
  aura.choisie = !allumee();
  majHautParleur();
  if (!allumee()) taire();
});

cnv.addEventListener("mousemove", (e) => {
  if (balade.vue || !allumee() || couverte() || !surLaCarte()) return;
  clearTimeout(aura.minuterie);
  const r = cnv.getBoundingClientRect();
  const x = e.clientX - r.left;
  const y = e.clientY - r.top;
  aura.minuterie = setTimeout(() => ecouterAutour(x, y), ARRET_MS);
});

cnv.addEventListener("mouseleave", () => {
  if (!balade.vue) taire();
});

/// Une fois par seconde : le promeneur avance, une lecture démarre, le mode
/// change. Le promeneur fait entendre ses voisins, sauf le morceau qu'il
/// écoute et celui où il va.
setInterval(() => {
  if (!!balade.vue !== aura.enBalade) {
    aura.enBalade = !!balade.vue;
    aura.choisie = null;
  }
  majHautParleur();
  if (!allumee() || couverte() || !surLaCarte()) {
    taire();
    return;
  }
  if (balade.vue && balade.promeneur) {
    const v = balade.vue;
    const exclus = new Set([v.pistes[v.pistes.length - 1], v.suivant].filter((id) => id != null));
    ecouterAutour(balade.promeneur[0], balade.promeneur[1], exclus);
  }
}, 1000);

export { aura };
