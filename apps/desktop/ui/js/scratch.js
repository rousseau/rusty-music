import { invoke, journal } from "./socle.js";

/* ------------------------------------------------- le mode « scratch » du vinyle
 *
 * Un bouton discret posé sur le disque (visible au survol ou au focus) arme le
 * mode ; Échap ou un second clic en sort. Armé, le trackpad (molette, deltaX et
 * deltaY) ou un glisser au pointeur sur le disque est une main posée dessus :
 * le déplacement **tangentiel** autour de l'axe donne la vitesse et le sens de
 * lecture, marche arrière comprise. Main levée, le moteur du lecteur ramène le
 * disque à sa vitesse en ~350 ms (`crates/player/src/scratch.rs`).
 *
 * Le dessin suit l'angle réel : celui de la tête du moteur (33 ⅓ tours/min),
 * relevé à chaque geste, décalé de l'angle où le disque était à l'armement.
 * Une seule commande en vol à la fois : ce qui s'accumule entre deux réponses
 * part dans la suivante, sans file d'attente qui retarderait la main.
 *
 * Deux disques peuvent s'armer (la pochette de l'inspecteur, la vue « En
 * cours ») ; un seul à la fois. Avec `prefers-reduced-motion`, pas de bouton
 * (`style.css`).
 */

const TOUR_MS = 1800; // 33 ⅓ tours/min
const DEG_PAR_S = 360 / (TOUR_MS / 1000); // vitesse nominale du disque
/// Sans molette depuis ce délai, la main se lève (le trackpad ne dit pas
/// quand les doigts quittent la surface ; son inertie compte comme un lancer).
const MAIN_LEVEE_MS = 90;

const etat = {
  cible: null, // { zone, bouton, angle(), poser(deg), rendre(deg) } : le disque armé
  ouverture: false, // `scratch_entrer` en cours
  decalage: 0, // angle dessiné − angle de la tête du moteur
  angle: 0,
  accumule: 0, // degrés de main depuis le dernier envoi
  depuis: 0, // horodatage du dernier envoi
  molette: 0, // horodatage du dernier événement de molette
  glisse: null, // { id, angle } pendant un glisser au pointeur
  enVol: false,
  image: 0,
  latences: [], // geste → son (µs), pour le journal
  allers: [], // aller-retour d'une commande (µs)
};

const arme = () => etat.cible !== null;

/// Angle (degrés) du point `x, y` de l'écran autour du centre de `zone`.
function centre(zone) {
  const r = zone.getBoundingClientRect();
  return { x: r.left + r.width / 2, y: r.top + r.height / 2, rayon: r.width / 2 };
}

/// La part tangentielle d'un déplacement `(mx, my)` au point `(x, y)`, en
/// degrés de rotation (sens horaire positif). Près de l'axe, le bras de levier
/// est borné : un geste au centre ne fait pas tourner le disque à toute allure.
function tangentiel(zone, x, y, mx, my) {
  const c = centre(zone);
  let px = x - c.x;
  let py = y - c.y;
  let r = Math.hypot(px, py);
  const min = c.rayon * 0.35;
  if (r < min) {
    // Bras de levier fictif, vers le haut du disque : un geste horizontal y
    // reste lisible (droite = avant).
    if (r < 1) { px = 0; py = -min; } else { px *= min / r; py *= min / r; }
    r = min;
  }
  return ((px * my - py * mx) / (r * r)) * (180 / Math.PI);
}

function surMolette(e) {
  if (!arme() || !etat.cible.zone.contains(e.target)) return;
  e.preventDefault();
  // Défilement naturel de macOS : le contenu suit les doigts, le delta est à
  // l'opposé de leur mouvement.
  let mx = -e.deltaX;
  let my = -e.deltaY;
  if (e.deltaMode === 1) { mx *= 16; my *= 16; }
  etat.accumule += tangentiel(etat.cible.zone, e.clientX, e.clientY, mx, my);
  etat.molette = performance.now();
}

function surAppui(e) {
  if (!arme() || e.button !== 0 || !etat.cible.zone.contains(e.target)) return;
  // Les boutons posés sur le disque (▶ ✦ ♡ ＋, celui-ci) et l'anneau de
  // progression gardent leur clic.
  if (e.target.closest?.("button, circle")) return;
  e.preventDefault();
  const c = centre(etat.cible.zone);
  etat.glisse = { id: e.pointerId, angle: Math.atan2(e.clientY - c.y, e.clientX - c.x) };
  etat.cible.zone.setPointerCapture?.(e.pointerId);
  etat.cible.zone.classList.add("scratch--main");
}

function surGlisse(e) {
  if (!etat.glisse || e.pointerId !== etat.glisse.id) return;
  const c = centre(etat.cible.zone);
  const a = Math.atan2(e.clientY - c.y, e.clientX - c.x);
  let d = a - etat.glisse.angle;
  if (d > Math.PI) d -= 2 * Math.PI;
  else if (d < -Math.PI) d += 2 * Math.PI;
  etat.glisse.angle = a;
  etat.accumule += d * (180 / Math.PI);
}

function surLacher(e) {
  if (!etat.glisse || e.pointerId !== etat.glisse.id) return;
  etat.glisse = null;
  etat.cible?.zone.classList.remove("scratch--main");
}

/// Une image : la main (ou son absence) part vers le moteur, l'angle revient.
function boucle(maintenant) {
  if (!arme()) return;
  etat.image = requestAnimationFrame(boucle);
  if (etat.enVol) return; // ce qui s'accumule partira avec la prochaine
  const dt = Math.max(1, maintenant - (etat.depuis || maintenant)) / 1000;
  etat.depuis = maintenant;
  const main = etat.glisse !== null || maintenant - etat.molette < MAIN_LEVEE_MS;
  const vitesse = main ? etat.accumule / dt / DEG_PAR_S : null;
  etat.accumule = 0;
  etat.enVol = true;
  const envoi = performance.now();
  invoke("scratch_main", { vitesse })
    .then((r) => {
      etat.enVol = false;
      if (!r?.actif) return desarmer({ moteur: false });
      if (main && r.latence_us) {
        const aller = (performance.now() - envoi) * 1000;
        // Le geste part au début de l'image : la moitié de l'aller-retour pour
        // l'interface, la relève du fil audio, puis la sortie.
        etat.allers.push(aller);
        etat.latences.push(aller / 2 + r.latence_us + (r.latence_sortie_us ?? 0));
        if (etat.latences.length >= 240) noterLatence();
      }
      etat.angle = (r.position_ms / TOUR_MS) * 360 + etat.decalage;
      etat.cible?.poser(etat.angle);
    })
    .catch((e) => {
      etat.enVol = false;
      journal("scratch", String(e), "warn");
      desarmer({ moteur: false });
    });
}

/// La latence geste → son, médiane sur une rafale, au journal de l'interface.
function noterLatence() {
  const mediane = (v) => [...v].sort((a, b) => a - b)[Math.floor(v.length / 2)] / 1000;
  journal("scratch", `latence geste → son : médiane ${mediane(etat.latences).toFixed(1)} ms `
    + `(aller-retour de commande ${mediane(etat.allers).toFixed(1)} ms) sur ${etat.latences.length} gestes`);
  etat.latences = [];
  etat.allers = [];
}

/// Arme le disque `cible` (un seul à la fois). `cible` : `zone` (ce qui reçoit
/// les gestes), `bouton`, `angle()` (l'angle dessiné à l'armement),
/// `poser(deg)` (dessine l'angle), `rendre(deg)` (rend le disque à son
/// animation, depuis cet angle).
async function armer(cible) {
  if (etat.ouverture) return;
  if (arme()) await desarmer();
  etat.ouverture = true;
  cible.bouton.setAttribute("aria-busy", "true");
  let r = null;
  try {
    r = await invoke("scratch_entrer");
  } catch (e) {
    journal("scratch", String(e), "warn");
  } finally {
    etat.ouverture = false;
    cible.bouton.removeAttribute("aria-busy");
  }
  if (!r?.actif) return;
  etat.cible = cible;
  etat.decalage = cible.angle() - (r.position_ms / TOUR_MS) * 360;
  etat.angle = cible.angle();
  etat.accumule = 0;
  etat.depuis = 0;
  etat.molette = 0;
  cible.bouton.setAttribute("aria-pressed", "true");
  cible.zone.classList.add("scratch--arme");
  cible.poser(etat.angle);
  etat.image = requestAnimationFrame(boucle);
}

/// Sort du mode : la lecture reprend là où le disque s'est arrêté. `moteur :
/// false` quand le moteur en est déjà sorti de lui-même (autre piste, fin).
async function desarmer({ moteur = true } = {}) {
  const cible = etat.cible;
  if (!cible) return;
  etat.cible = null;
  cancelAnimationFrame(etat.image);
  etat.glisse = null;
  if (etat.latences.length >= 30) noterLatence();
  cible.bouton.setAttribute("aria-pressed", "false");
  cible.zone.classList.remove("scratch--arme", "scratch--main");
  cible.rendre(etat.angle);
  if (moteur) await invoke("scratch_sortir").catch((e) => journal("scratch", String(e), "warn"));
}

const basculer = (cible) => (etat.cible?.zone === cible.zone ? desarmer() : armer(cible));

window.addEventListener("wheel", surMolette, { passive: false, capture: true });
window.addEventListener("pointerdown", surAppui, true);
window.addEventListener("pointermove", surGlisse, true);
window.addEventListener("pointerup", surLacher, true);
window.addEventListener("pointercancel", surLacher, true);
// Échap sort du mode avant de fermer quoi que ce soit d'autre (la vue « En
// cours » écoute aussi Échap, en capture : ce module est importé avant elle).
window.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || !arme()) return;
  e.stopImmediatePropagation();
  e.preventDefault();
  desarmer();
}, true);

export {
  arme as scratchArme, basculer as basculerScratch, desarmer as desarmerScratch, TOUR_MS,
};
