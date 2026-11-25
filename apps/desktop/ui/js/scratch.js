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
 * Main posée, le dessin suit le geste et le son suit sa vitesse ; levée, le
 * dessin avance à la vitesse du moteur et se recale sur sa tête. Une seule
 * commande en vol à la fois, sans file d'attente qui retarderait la main.
 *
 * Deux disques peuvent s'armer (la pochette de l'inspecteur, la vue « En
 * cours ») ; un seul à la fois. Avec `prefers-reduced-motion`, pas de bouton
 * (`style.css`).
 */

const TOUR_MS = 1800; // 33 ⅓ tours/min
const DEG_PAR_S = 360 / (TOUR_MS / 1000); // vitesse nominale du disque
/// Sans geste depuis ce délai, la main se lève : le trackpad ne dit pas quand
/// les doigts quittent la surface. Entre deux événements plus proches, la
/// vitesse de la main est tenue (ils n'arrivent pas à chaque image).
const MAIN_LEVEE_MS = 60;
/// Au glisser, un pointeur immobile depuis ce délai tient le disque arrêté.
const IMMOBILE_MS = 40;
/// Événements de molette qui décroissent en suite géométrique : l'inertie de
/// macOS, pas les doigts. La main est levée dès qu'on la reconnaît — un disque
/// lancé garde sa vitesse et le moteur l'y ramène, il ne retombe pas à zéro
/// en suivant l'inertie du trackpad.
const INERTIE_SUITE = 5;

const etat = {
  cible: null, // { zone, bouton, angle(), anim(), rendre(deg) } : le disque armé
  libre: false, // l'animation tourne seule, à la vitesse du moteur
  ouverture: false, // `scratch_entrer` en cours
  decalage: 0, // angle dessiné − angle de la tête du moteur
  angle: 0, // angle dessiné
  dessin: 0, // degrés de main pas encore dessinés
  vMain: 0, // vitesse de la main, en degrés par seconde (lissée)
  dernier: 0, // horodatage du dernier geste
  type: null, // "molette" | "glisse" : la main posée, ou null
  glisse: null, // { id, angle } pendant un glisser au pointeur
  inertie: { suite: 0, precedent: 0, ignore: false }, // repérage de l'inertie
  moteur: { vitesse: 1, tete: 0, quand: 0 }, // dernier état rendu par le moteur
  image: 0,
  avant: 0, // horodatage de l'image précédente
  centre: null, // centre et rayon du disque, relevés à l'armement
  enVol: false,
  latences: [], // geste → son (µs), pour le journal
  allers: [], // aller-retour d'une commande (µs)
  releve: { debut: 0, images: 0, molettes: 0, ignores: 0, envois: 0 }, // diagnostic
};

const arme = () => etat.cible !== null;

/// Centre et rayon du disque armé. Relevés une fois (armement,
/// redimensionnement), pas à chaque événement : `getBoundingClientRect`
/// après une rotation posée force un calcul de mise en page, et sur un grand
/// disque la molette en envoie plus d'un par image.
function centre() {
  if (!etat.centre) {
    const r = etat.cible.zone.getBoundingClientRect();
    etat.centre = { x: r.left + r.width / 2, y: r.top + r.height / 2, rayon: r.width / 2 };
  }
  return etat.centre;
}
window.addEventListener("resize", () => { etat.centre = null; });

/// Un geste de `deg` degrés à l'instant `t` : le dessin le suit tout de suite
/// (la main tient le disque), et la vitesse de la main en est déduite.
function tourner(deg, t, type) {
  const dt = etat.type ? Math.min(60, Math.max(4, t - etat.dernier)) : 16;
  const inst = deg / (dt / 1000);
  // Lissée sur ~25 ms : les deltas du trackpad arrivent entiers et
  // irréguliers, et chaque saut de vitesse s'entendrait.
  const k = 1 - Math.exp(-dt / 25);
  etat.vMain = etat.type ? etat.vMain + (inst - etat.vMain) * k : inst;
  etat.dessin += deg;
  etat.dernier = t;
  etat.type = type;
}

/// La part tangentielle d'un déplacement `(mx, my)` au point `(x, y)`, en
/// degrés de rotation (sens horaire positif). Près de l'axe, le bras de levier
/// est borné : un geste au centre ne fait pas tourner le disque à toute allure.
function tangentiel(x, y, mx, my) {
  const c = centre();
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

/// Vrai quand ce delta continue une décroissance régulière (l'inertie du
/// trackpad) ; un delta qui remonte, ou un silence, est une main à nouveau.
function inertie(amplitude, t) {
  const i = etat.inertie;
  if (t - etat.dernier > 2 * MAIN_LEVEE_MS) i.ignore = false;
  const rapport = i.precedent > 0 ? amplitude / i.precedent : 0;
  i.precedent = amplitude;
  if (rapport > 0.75 && rapport < 0.97) i.suite += 1;
  else i.suite = 0;
  if (i.ignore && rapport >= 1.05) i.ignore = false;
  if (!i.ignore && i.suite >= INERTIE_SUITE) i.ignore = true;
  return i.ignore;
}

function surMolette(e) {
  if (!arme() || !etat.cible.zone.contains(e.target)) return;
  e.preventDefault();
  etat.releve.molettes += 1;
  // Défilement naturel de macOS : le contenu suit les doigts, le delta est à
  // l'opposé de leur mouvement.
  let mx = -e.deltaX;
  let my = -e.deltaY;
  if (e.deltaMode === 1) { mx *= 16; my *= 16; }
  const t = performance.now();
  if (inertie(Math.hypot(mx, my), t)) {
    etat.releve.ignores += 1;
    etat.dernier = t;
    etat.type = null; // la main est levée : le disque garde sa vitesse
    return;
  }
  tourner(tangentiel(e.clientX, e.clientY, mx, my), t, "molette");
}

function surAppui(e) {
  if (!arme() || e.button !== 0 || !etat.cible.zone.contains(e.target)) return;
  // Les boutons posés sur le disque (▶ ✦ ♡ ＋, celui-ci) et l'anneau de
  // progression gardent leur clic.
  if (e.target.closest?.("button, circle")) return;
  e.preventDefault();
  const c = centre();
  etat.glisse = { id: e.pointerId, angle: Math.atan2(e.clientY - c.y, e.clientX - c.x) };
  etat.type = "glisse";
  etat.vMain = 0; // poser la main arrête le disque
  etat.dernier = performance.now();
  etat.cible.zone.setPointerCapture?.(e.pointerId);
  etat.cible.zone.classList.add("scratch--main");
}

function surGlisse(e) {
  if (!etat.glisse || e.pointerId !== etat.glisse.id) return;
  const c = centre();
  const a = Math.atan2(e.clientY - c.y, e.clientX - c.x);
  let d = a - etat.glisse.angle;
  if (d > Math.PI) d -= 2 * Math.PI;
  else if (d < -Math.PI) d += 2 * Math.PI;
  etat.glisse.angle = a;
  tourner(d * (180 / Math.PI), performance.now(), "glisse");
}

function surLacher(e) {
  if (!etat.glisse || e.pointerId !== etat.glisse.id) return;
  etat.glisse = null;
  etat.type = null;
  etat.cible?.zone.classList.remove("scratch--main");
}

/// La main est-elle posée, à l'instant `t` ? Met à jour sa vitesse.
function mainPosee(t) {
  if (etat.type === "glisse") {
    // Le pointeur tenu sans bouger tient le disque arrêté.
    if (t - etat.dernier > IMMOBILE_MS) etat.vMain *= 0.5;
    return true;
  }
  if (etat.type === "molette" && t - etat.dernier < MAIN_LEVEE_MS) return true;
  etat.type = null;
  return false;
}

/// Une image : le disque est dessiné, la main (ou son absence) part vers le
/// moteur. Main posée, le dessin suit le geste. Levée, il avance de lui-même
/// à la vitesse du moteur et se recale doucement sur sa tête : le fil audio ne
/// la publie qu'une fois par tampon de sortie, la recopier ferait avancer le
/// disque par à-coups.
function boucle(maintenant) {
  if (!arme()) return;
  etat.image = requestAnimationFrame(boucle);
  const dt = Math.min(0.1, Math.max(0, (maintenant - (etat.avant || maintenant)) / 1000));
  etat.avant = maintenant;
  diagnostic(maintenant);
  const main = mainPosee(maintenant);
  if (main) {
    etat.angle += etat.dessin;
  } else {
    const m = etat.moteur;
    etat.angle += m.vitesse * DEG_PAR_S * dt;
    const visee = m.tete + m.vitesse * DEG_PAR_S * ((maintenant - m.quand) / 1000) + etat.decalage;
    etat.angle += (visee - etat.angle) * Math.min(1, 4 * dt);
  }
  etat.dessin = 0;
  dessiner(main);
  if (etat.enVol) return; // la vitesse de la main est tenue jusqu'au prochain envoi
  const vitesse = main ? etat.vMain / DEG_PAR_S : null;
  etat.enVol = true;
  etat.releve.envois += 1;
  const envoi = performance.now();
  invoke("scratch_main", { vitesse })
    .then((r) => {
      etat.enVol = false;
      if (!r?.actif) return desarmer({ moteur: false });
      const recu = performance.now();
      if (main && r.latence_us) {
        const aller = (recu - envoi) * 1000;
        // La moitié de l'aller-retour pour l'interface, la relève du fil
        // audio, puis la sortie.
        etat.allers.push(aller);
        etat.latences.push(aller / 2 + r.latence_us + (r.latence_sortie_us ?? 0));
        if (etat.latences.length >= 240) noterLatence();
      }
      const tete = (r.position_ms / TOUR_MS) * 360;
      etat.moteur = { vitesse: r.vitesse, tete, quand: recu };
      // Main posée, le dessin mène : on retient ce qui le sépare du moteur,
      // pour qu'au lâcher il n'y ait pas de saut.
      if (main) etat.decalage = etat.angle - tete;
    })
    .catch((e) => {
      etat.enVol = false;
      journal("scratch", String(e), "warn");
      desarmer({ moteur: false });
    });
}

const modulo = (deg) => ((((deg / 360) % 1) + 1) % 1) * TOUR_MS;

/// Pose l'angle sur l'animation du disque. Main posée ou moteur qui change
/// de vitesse : en pause, l'angle fixé à chaque image. Moteur à vitesse
/// constante : l'animation tourne seule à cette vitesse, au rythme de l'écran
/// (120 Hz sur le MacBook, quand les rappels de WebKit plafonnent à 60), et
/// n'est recalée que si elle dérive de plus de 2°.
function dessiner(main) {
  const a = etat.cible.anim();
  if (!a) return;
  const v = etat.moteur.vitesse;
  const stable = !main && Math.abs(v - Math.round(v)) < 0.005 && (v === 0 || Math.round(v) === 1);
  if (!stable || Math.round(v) === 0) {
    if (etat.libre || a.playState === "running") a.pause();
    etat.libre = false;
    a.currentTime = modulo(etat.angle);
    return;
  }
  if (!etat.libre) {
    a.currentTime = modulo(etat.angle);
    a.playbackRate = 1;
    a.play();
    etat.libre = true;
    return;
  }
  // L'animation mène : l'angle dessiné est le sien, recalé s'il dérive.
  let ecart = ((a.currentTime ?? 0) / TOUR_MS) * 360 - modulo(etat.angle) / TOUR_MS * 360;
  ecart = ((ecart + 540) % 360) - 180;
  if (Math.abs(ecart) > 2) a.currentTime = modulo(etat.angle);
}

/// Toutes les deux secondes armé, si la vue passe sous 50 images/s : images
/// par seconde, molettes reçues et écartées (inertie), envois, tailles — de
/// quoi voir ce qui ralentit.
function diagnostic(maintenant) {
  const r = etat.releve;
  r.images += 1;
  if (!r.debut) r.debut = maintenant;
  const duree = (maintenant - r.debut) / 1000;
  if (duree < 2) return;
  const c = centre();
  if (r.images / duree < 50) journal("scratch", `${(r.images / duree).toFixed(0)} images/s, ${r.molettes} molettes `
    + `(${r.ignores} d'inertie), ${(r.envois / duree).toFixed(0)} envois/s, disque `
    + `${(2 * c.rayon).toFixed(0)} px, fenêtre ${innerWidth}×${innerHeight}`);
  etat.releve = { debut: maintenant, images: 0, molettes: 0, ignores: 0, envois: 0 };
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
/// `anim()` (l'animation Web Animations d'un tour, que ce module pilote),
/// `rendre(deg)` (rend le disque à son animation ordinaire, depuis cet angle).
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
  etat.dessin = 0;
  etat.vMain = 0;
  etat.type = null;
  etat.centre = null;
  etat.avant = 0;
  etat.moteur = { vitesse: r.vitesse, tete: (r.position_ms / TOUR_MS) * 360, quand: performance.now() };
  etat.inertie = { suite: 0, precedent: 0, ignore: false };
  etat.releve = { debut: 0, images: 0, molettes: 0, ignores: 0, envois: 0 };
  cible.bouton.setAttribute("aria-pressed", "true");
  cible.zone.classList.add("scratch--arme");
  etat.libre = false;
  dessiner(false);
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
