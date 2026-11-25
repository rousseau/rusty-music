import "./palette.js"; // l'ordre d'exécution : voir main.js
import {
  $, fileAttente, isoler, pochette, remonter, txt,
} from "./socle.js";
import {
  spectreCourant, teteCourante,
} from "./lecteur.js";
import { couleursPochette } from "./couleurs-album.js";
import { lecture, transport } from "./etat-lecture.js";
import { deplacerLecture } from "./etabli-stems.js";
// Avant les écouteurs d'Échap ci-dessous : Échap sort d'abord du scratch.
import {
  basculerScratch, desarmerScratch, scratchArme, TOUR_MS,
} from "./scratch.js";

/* ------------------------------------------------- vue plein écran « En cours »
 *
 * Le morceau qui joue, en grand : un vinyle qui tourne, la pochette en
 * macaron, la progression en anneau cliquable, une lueur au bord ; titre,
 * artiste, album, temps et la suite de la file à côté (refonte du 23 nov. 2026) (`docs/plan-ecouter-v0.3.md`,
 * point 1 ; idée de Disc Player, `src/layout/VisualizerMode.vue`, licence MIT).
 * Un écran au-dessus des autres, comme la palette : le transport et la file
 * restent utilisables, Échap ferme. Pas un sixième mode du rail. On l'ouvre
 * par le bouton « en cours » du transport, la touche F, un clic sur la
 * pochette du transport ou ⌘K « en cours ».
 *
 * Une lueur fine au bord du disque suit l'intensité du morceau. Elle ne
 * prélève pas le son : elle lit, à la tête de lecture, la colonne du
 * spectrogramme que le transport a déjà calculé (`spectreCourant`), et en
 * lisse l'énergie moyenne dans le temps (24 nov. 2026 : un halo qui suivait
 * chaque bande, trop saccadé et trop marqué, l'a précédée).
 * Les fonds prennent les deux couleurs de la pochette, l'anneau et la lueur
 * une troisième, plus vive (`couleurs-album.js`) — l'exception de la Règle 3,
 * bornée à cette vue.
 * Avec `prefers-reduced-motion`, ni rotation ni lueur qui bouge (`style.css`, `boucle`).
 */

const vue = $("en-cours");
const disque = $("en-cours-disque");
const anneau = $("en-cours-anneau");
const avance = $("en-cours-avance");
const tete = $("en-cours-tete");
const platineVue = vue.querySelector(".en-cours__platine");
const bouton = $("bascule-en-cours");
const reduit = matchMedia("(prefers-reduced-motion: reduce)");

const etat = {
  chemin: undefined, // le morceau affiché
  image: 0, // requestAnimationFrame
  focusAvant: null,
  jeton: 0,
  suite: -1, // longueur de la file à la dernière pose de « À suivre »
  intensite: 0, // énergie lissée du morceau, 0..1 (la lueur du disque)
  instant: 0, // horodatage de la dernière image
};

const morceauEnCours = () => fileAttente.pistes.find((t) => t.path === lecture.courant) ?? null;

/// Pose titre, artiste, album, pochette et couleurs du morceau en cours.
async function poserMorceau(t) {
  etat.chemin = t?.path ?? null;
  const jeton = ++etat.jeton;
  $("en-cours-titre").textContent = t ? txt(t.title, "(sans titre)") : "Rien en lecture";
  $("en-cours-artiste").textContent = t ? txt(t.artist, "(sans artiste)") : "";
  $("en-cours-album").textContent = t
    ? [isoler(txt(t.album)), t.year].filter(Boolean).join(" · ")
    : "";
  poserSuite();
  const img = t ? await pochette(t.path) : null;
  if (jeton !== etat.jeton) return;
  const url = img ? `url("${img}")` : "";
  $("en-cours-macaron").style.backgroundImage = url;
  $("en-cours-fond").style.backgroundImage = url;
  await poserCouleurs(img, jeton);
}

/// Les fonds : les deux couleurs de la pochette, recalées pour le thème.
async function poserCouleurs(img, jeton = etat.jeton) {
  let c = null;
  try {
    c = await couleursPochette(img);
  } catch (e) {
    remonter(e, "couleurs d'album");
  }
  if (jeton !== etat.jeton) return;
  vue.style.setProperty("--album-haut", c?.haut ?? "var(--panel)");
  vue.style.setProperty("--album-bas", c?.bas ?? "var(--bg)");
  // Sans pochette, l'accent de l'application (repli de `style.css`).
  if (c?.trait) vue.style.setProperty("--album-trait", c.trait);
  else vue.style.removeProperty("--album-trait");
}

/// L'intensité du morceau, pour la lueur au bord du disque : l'énergie
/// moyenne de la colonne du spectrogramme à la tête de lecture, lissée dans le
/// temps (montée en un quart de seconde, retombée en près d'une seconde), et
/// mesurée en temps écoulé plutôt qu'en images : même douceur à 60 ou 120 Hz.
/// Pas de bande par bande : une lueur qui suit chaque bande clignote.
function suivreIntensite(maintenant) {
  const dt = Math.min(0.1, Math.max(0, (maintenant - (etat.instant || maintenant)) / 1000));
  etat.instant = maintenant;
  let cible = 0;
  const s = spectreCourant;
  if (s?.pixels && lecture.courant === etat.chemin && transport.veutJouer) {
    const col = Math.min(s.largeur - 1, Math.floor(teteCourante * s.largeur));
    let somme = 0;
    for (let k = 0; k < s.hauteur; k++) somme += s.pixels[k * s.largeur + col];
    // Une énergie moyenne de spectrogramme dépasse rarement la moitié de
    // l'échelle : on l'étire, puis on adoucit le haut.
    cible = Math.min(1, (somme / (s.hauteur * 255)) * 1.8) ** 0.8;
  }
  const tau = cible > etat.intensite ? 0.25 : 0.9;
  etat.intensite += (cible - etat.intensite) * (1 - Math.exp(-dt / tau));
  platineVue.style.setProperty("--intensite", etat.intensite.toFixed(3));
}

const mmss = (ms) => {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
};

/// Les deux morceaux suivants de la file, pour savoir où va l'écoute.
function poserSuite() {
  const i = fileAttente.pistes.findIndex((t) => t.path === lecture.courant);
  const suite = i < 0 ? [] : fileAttente.pistes.slice(i + 1, i + 3);
  const liste = $("en-cours-suite-liste");
  liste.replaceChildren(...suite.map((t) => {
    const li = document.createElement("li");
    li.textContent = [txt(t.title, "(sans titre)"), txt(t.artist)].filter(Boolean).join(" · ");
    return li;
  }));
  $("en-cours-suite").hidden = suite.length === 0;
  etat.suite = fileAttente.pistes.length;
}

/// L'anneau et le temps, à la tête de lecture du transport.
function poserProgression(t) {
  const frac = t ? teteCourante : 0;
  avance.style.strokeDashoffset = String(1000 * (1 - frac));
  tete.setAttribute("transform", `rotate(${(frac * 360).toFixed(2)} 50 50)`);
  tete.style.opacity = t && frac > 0 ? "1" : "0";
  anneau.setAttribute("aria-valuenow", String(Math.round(frac * 100)));
  const duree = t?.duration_ms ?? 0;
  $("en-cours-temps").textContent = duree ? `${mmss(frac * duree)} / ${mmss(duree)}` : "";
}

function boucle(maintenant = performance.now()) {
  if (vue.hidden) return;
  const t = morceauEnCours();
  if ((t?.path ?? null) !== etat.chemin) poserMorceau(t).catch((e) => remonter(e, "en cours"));
  else if (etat.suite !== fileAttente.pistes.length) poserSuite();
  poserProgression(t);
  vue.classList.toggle("en-cours--joue", transport.veutJouer);
  $("en-cours-scratch").hidden = !t;
  if (!reduit.matches) suivreIntensite(maintenant);
  etat.image = requestAnimationFrame(boucle);
}

function ouvrirEnCours() {
  if (!vue.hidden) return;
  etat.focusAvant = document.activeElement;
  vue.hidden = false;
  bouton.setAttribute("aria-pressed", "true");
  etat.chemin = undefined; // force la pose du morceau
  $("en-cours-fermer").focus();
  boucle();
}

function fermerEnCours() {
  if (vue.hidden) return;
  if (scratchArme()) desarmerScratch();
  vue.hidden = true;
  bouton.setAttribute("aria-pressed", "false");
  cancelAnimationFrame(etat.image);
  etat.focusAvant?.focus?.();
  etat.focusAvant = null;
}

const basculerEnCours = () => (vue.hidden ? ouvrirEnCours() : fermerEnCours());

$("en-cours-fermer").addEventListener("click", fermerEnCours);
const art = $("transport-art");
art.addEventListener("click", basculerEnCours);
art.addEventListener("keydown", (e) => {
  if (e.key !== "Enter") return;
  e.preventDefault();
  basculerEnCours();
});
bouton.addEventListener("click", basculerEnCours);

// L'anneau : un clic y déplace la lecture, à l'angle cliqué (midi = début).
anneau.addEventListener("click", (e) => {
  const r = anneau.getBoundingClientRect();
  const x = e.clientX - (r.left + r.width / 2);
  const y = e.clientY - (r.top + r.height / 2);
  // Seulement près du trait : l'anneau couvre aussi le disque, qui reste inerte.
  if (Math.hypot(x, y) < r.width / 2 - 26) return;
  const angle = Math.atan2(x, -y); // 0 à midi, dans le sens des aiguilles
  const frac = (angle < 0 ? angle + 2 * Math.PI : angle) / (2 * Math.PI);
  deplacerLecture(frac).catch((err) => remonter(err, "en cours"));
});
// Au clavier : ← et → reculent ou avancent de 5 s — sur l'anneau seulement ;
// ailleurs, ces touches changent de piste (`lecteur.js`).
anneau.addEventListener("keydown", (e) => {
  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
  e.preventDefault();
  e.stopPropagation();
  const duree = morceauEnCours()?.duration_ms;
  if (!duree) return;
  const pas = (e.key === "ArrowRight" ? 5000 : -5000) / duree;
  deplacerLecture(teteCourante + pas).catch((err) => remonter(err, "en cours"));
});

// F ouvre ou ferme la vue, hors d'un champ de saisie et sans modificateur.
const dansUnChamp = (el) =>
  el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement ||
  el instanceof HTMLSelectElement || el?.isContentEditable;
document.addEventListener("keydown", (e) => {
  if (e.repeat || e.metaKey || e.ctrlKey || e.altKey || e.key.toLowerCase() !== "f") return;
  if (dansUnChamp(e.target) || !$("palette").hidden) return;
  e.preventDefault();
  basculerEnCours();
});

// Échap ferme la vue, et rien d'autre : la file d'attente, ouverte par-dessus,
// garde son propre Échap pour un second appui.
window.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || vue.hidden || !$("palette").hidden || !$("file").hidden) return;
  e.stopPropagation();
  fermerEnCours();
}, true);
// Le thème change les bornes des couleurs.
document.addEventListener("theme", () => {
  if (vue.hidden || !etat.chemin) return;
  pochette(etat.chemin).then((img) => poserCouleurs(img)).catch(() => {});
});

/* ------------------------------------------ la pochette de l'inspecteur
 *
 * Quand le morceau inspecté est celui qui joue (ou attend en pause), sa
 * pochette se change en vinyle : `.pochette--disque` lance la métamorphose
 * (CSS, 0,6 s), puis la platine tourne. Sa vitesse n'est pas un interrupteur :
 * elle monte en une seconde environ à la lecture et redescend à la pause,
 * comme une platine (Web Animations, `playbackRate` lissé). Au retour en
 * carré, le disque finit son tour jusqu'à l'endroit avant de se redresser.
 * Avec `prefers-reduced-motion`, la pochette reste un carré immobile.
 */
const platine = {
  el: $("pochette-platine"),
  hote: $("pochette"),
  tour: null, // l'animation de rotation (Web Animations)
  vitesse: 0, // playbackRate courant, 0..1
  scratch: false, // le mode scratch tient l'angle (`scratch.js`)
  image: 0, // requestAnimationFrame de la rampe
  sortie: null, // l'animation qui ramène le disque à l'endroit
};

function disqueVoulu() {
  if (reduit.matches || !lecture.courant || lecture.fini) return false;
  return $("insp-titre").dataset.path === lecture.courant && !$("insp").hidden;
}

function rampeVitesse() {
  if (platine.scratch) {
    platine.image = 0;
    return;
  }
  const cible = transport.veutJouer ? 1 : 0;
  platine.vitesse += (cible - platine.vitesse) * 0.06;
  if (Math.abs(cible - platine.vitesse) < 0.003) platine.vitesse = cible;
  if (platine.tour) {
    platine.tour.playbackRate = platine.vitesse;
    if (platine.vitesse === 0) platine.tour.pause();
    else if (platine.tour.playState !== "running") platine.tour.play();
  }
  platine.image = platine.vitesse === cible ? 0 : requestAnimationFrame(rampeVitesse);
}

function entrerDisque() {
  platine.sortie?.cancel();
  platine.sortie = null;
  platine.hote.classList.add("pochette--disque");
  if (!platine.tour) {
    platine.tour = platine.el.animate(
      [{ transform: "rotate(0turn)" }, { transform: "rotate(1turn)" }],
      { duration: TOUR_MS, iterations: Infinity },
    );
    platine.tour.pause();
    platine.vitesse = 0;
  }
}

function sortirDisque() {
  cancelAnimationFrame(platine.image);
  platine.image = 0;
  const tour = platine.tour;
  platine.tour = null;
  platine.vitesse = 0;
  if (!tour) {
    platine.hote.classList.remove("pochette--disque");
    return;
  }
  // L'angle atteint, puis un dernier bout de tour jusqu'à l'endroit : le
  // carré ne se redresse jamais de travers.
  const angle = ((tour.currentTime ?? 0) % TOUR_MS) / TOUR_MS;
  tour.cancel();
  if (angle < 0.001 || reduit.matches) {
    platine.hote.classList.remove("pochette--disque");
    return;
  }
  platine.sortie = platine.el.animate(
    [{ transform: `rotate(${angle}turn)` }, { transform: "rotate(1turn)" }],
    { duration: 500 + 500 * (1 - angle), easing: "cubic-bezier(.2, .7, .3, 1)" },
  );
  platine.sortie.onfinish = () => {
    platine.sortie = null;
    platine.hote.classList.remove("pochette--disque");
  };
}

/// Un relevé léger, quatre fois par seconde : l'inspecteur, la lecture et
/// l'intention changent par bien des chemins, aucun n'a à prévenir ici.
function suivrePlatine() {
  const voulu = disqueVoulu();
  if (!voulu && platine.scratch) desarmerScratch();
  const actif = platine.hote.classList.contains("pochette--disque") && !platine.sortie;
  if (voulu && !actif) entrerDisque();
  else if (!voulu && actif) sortirDisque();
  if (voulu && !platine.image && platine.vitesse !== (transport.veutJouer ? 1 : 0)) {
    platine.image = requestAnimationFrame(rampeVitesse);
  }
}
setInterval(suivrePlatine, 250);

/* ------------------------------------------------- les deux disques, en scratch */

const tourner = (deg) => ((((deg / 360) % 1) + 1) % 1) * TOUR_MS;

/// Le vinyle de l'inspecteur : l'animation de rotation (Web Animations), en
/// pause, tient l'angle ; rendue, elle repart de là à la vitesse du transport.
const scratchInspecteur = {
  zone: platine.hote,
  bouton: $("insp-scratch"),
  angle: () => ((platine.tour?.currentTime ?? 0) % TOUR_MS) / TOUR_MS * 360,
  poser(deg) {
    platine.scratch = true;
    if (!platine.tour) return;
    platine.tour.pause();
    platine.tour.currentTime = tourner(deg);
  },
  rendre(deg) {
    this.poser(deg);
    platine.scratch = false;
    if (!platine.tour) return;
    platine.vitesse = transport.veutJouer ? 1 : 0;
    platine.tour.playbackRate = platine.vitesse || 1;
    if (platine.vitesse) platine.tour.play();
  },
};
$("insp-scratch").addEventListener("click", () => basculerScratch(scratchInspecteur));

/// Le disque de la vue « En cours » : animation CSS ; armé, un angle posé à
/// la main ; rendu, l'animation reprend à cet angle (délai négatif).
const scratchVue = {
  zone: platineVue,
  bouton: $("en-cours-scratch"),
  angle() {
    const m = getComputedStyle(disque).transform;
    const v = m.startsWith("matrix(") ? m.slice(7, -1).split(",").map(Number) : null;
    return v ? Math.atan2(v[1], v[0]) * (180 / Math.PI) : 0;
  },
  poser(deg) {
    if (disque.style.animationName !== "none") disque.style.animation = "none";
    disque.style.transform = `rotate(${deg.toFixed(2)}deg)`;
  },
  rendre(deg) {
    disque.style.animation = "";
    disque.style.transform = "";
    disque.style.animationDelay = `${(-tourner(deg) / 1000).toFixed(3)}s`;
  },
};
$("en-cours-scratch").addEventListener("click", () => basculerScratch(scratchVue));

export {
  basculerEnCours, fermerEnCours, ouvrirEnCours,
};
