import "./palette.js"; // l'ordre d'exécution : voir main.js
import {
  $, fileAttente, isoler, pochette, remonter, txt,
} from "./socle.js";
import {
  spectreCourant, teteCourante,
} from "./lecteur.js";
import {
  rampe,
} from "./carte.js";
import { couleursPochette } from "./couleurs-album.js";
import { lecture, transport } from "./etat-lecture.js";
import { deplacerLecture } from "./etabli-stems.js";

/* ------------------------------------------------- vue plein écran « En cours »
 *
 * Le morceau qui joue, en grand : un vinyle qui tourne, la pochette en
 * macaron, la progression en anneau cliquable, le spectre en couronne ; titre,
 * artiste, album, temps et la suite de la file à côté (refonte du 23 nov. 2026) (`docs/plan-ecouter-v0.3.md`,
 * point 1 ; idée de Disc Player, `src/layout/VisualizerMode.vue`, licence MIT).
 * Un écran au-dessus des autres, comme la palette : le transport et la file
 * restent utilisables, Échap ferme. Pas un sixième mode du rail. On l'ouvre
 * par le bouton « en cours » du transport, la touche F, un clic sur la
 * pochette du transport ou ⌘K « en cours ».
 *
 * Le spectre ne prélève pas le son : il lit, à la tête de lecture, la colonne
 * du spectrogramme que le transport a déjà calculé (`spectreCourant`, rangée 0
 * = aigus), lissée d'une image à l'autre. Sa couleur suit `--rampe` (Règle 3 :
 * une rampe, pas des familles). Les fonds prennent les deux couleurs de la
 * pochette (`couleurs-album.js`) — l'exception de la Règle 3, bornée à cette vue.
 * Avec `prefers-reduced-motion`, ni rotation ni spectre (`style.css`).
 */

const vue = $("en-cours");
const cnv = $("en-cours-spectre");
const ctx = cnv.getContext("2d");
const disque = $("en-cours-disque");
const anneau = $("en-cours-anneau");
const avance = $("en-cours-avance");
const bouton = $("bascule-en-cours");
const reduit = matchMedia("(prefers-reduced-motion: reduce)");

const etat = {
  chemin: undefined, // le morceau affiché
  bandes: [], // énergie lissée de chaque bande, 0..1
  image: 0, // requestAnimationFrame
  focusAvant: null,
  jeton: 0,
  suite: -1, // longueur de la file à la dernière pose de « À suivre »
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
  etat.bandes = [];
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
}

/// Table de la rampe, de 0 à 255, pour colorer chaque bande selon son énergie.
let tableRampe = null;
function couleurDe(v) {
  if (!tableRampe) {
    const etapes = rampe();
    tableRampe = Array.from({ length: 256 }, (_, i) => etapes[Math.min(etapes.length - 1, Math.floor((i / 256) * etapes.length))]);
  }
  return tableRampe[Math.max(0, Math.min(255, Math.round(v * 255)))];
}

/// Le spectre en anneau : les graves en haut, les aigus en bas, symétrique de
/// part et d'autre du disque. Une barre par bande du spectrogramme.
function dessinerSpectre() {
  const dpr = devicePixelRatio || 1;
  const taille = cnv.clientWidth;
  if (cnv.width !== Math.round(taille * dpr)) {
    cnv.width = cnv.height = Math.round(taille * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, taille, taille);
  const s = spectreCourant;
  if (!s || !s.pixels || lecture.courant !== etat.chemin) return;
  const col = Math.min(s.largeur - 1, Math.floor(teteCourante * s.largeur));
  const n = s.hauteur;
  for (let k = 0; k < n; k++) {
    // Rangée n-1 = graves : en haut de l'anneau.
    const cible = s.pixels[(n - 1 - k) * s.largeur + col] / 255;
    etat.bandes[k] = (etat.bandes[k] ?? 0) * 0.72 + cible * 0.28;
  }
  // Une couronne fine au-delà de l'anneau de progression : des traits courts,
  // un peu transparents, pour accompagner le disque sans le couvrir.
  const centre = taille / 2;
  const rayon = disque.clientWidth / 2 + 24;
  const long = centre - rayon - 2;
  const pas = Math.PI / n;
  ctx.lineCap = "round";
  ctx.lineWidth = Math.max(1.5, Math.min(3, rayon * pas * 0.35));
  ctx.globalAlpha = 0.8;
  for (let k = 0; k < n; k++) {
    const v = etat.bandes[k];
    ctx.strokeStyle = couleurDe(v);
    const l = 2 + long * v ** 1.6;
    for (const sens of [1, -1]) {
      const a = -Math.PI / 2 + sens * (k + 0.5) * pas;
      ctx.beginPath();
      ctx.moveTo(centre + rayon * Math.cos(a), centre + rayon * Math.sin(a));
      ctx.lineTo(centre + (rayon + l) * Math.cos(a), centre + (rayon + l) * Math.sin(a));
      ctx.stroke();
    }
  }
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
  anneau.setAttribute("aria-valuenow", String(Math.round(frac * 100)));
  const duree = t?.duration_ms ?? 0;
  $("en-cours-temps").textContent = duree ? `${mmss(frac * duree)} / ${mmss(duree)}` : "";
}

function boucle() {
  if (vue.hidden) return;
  const t = morceauEnCours();
  if ((t?.path ?? null) !== etat.chemin) poserMorceau(t).catch((e) => remonter(e, "en cours"));
  else if (etat.suite !== fileAttente.pistes.length) poserSuite();
  poserProgression(t);
  vue.classList.toggle("en-cours--joue", transport.veutJouer);
  if (!reduit.matches) dessinerSpectre();
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
// Le thème change les bornes des couleurs et la rampe.
document.addEventListener("theme", () => {
  tableRampe = null;
  if (vue.hidden || !etat.chemin) return;
  pochette(etat.chemin).then((img) => poserCouleurs(img)).catch(() => {});
});

export {
  basculerEnCours, fermerEnCours, ouvrirEnCours,
};
