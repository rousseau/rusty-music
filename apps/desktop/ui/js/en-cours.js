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

/* ------------------------------------------------- vue plein écran « En cours »
 *
 * Le morceau qui joue, en grand : la pochette sur un disque qui tourne, le
 * spectre autour, titre, artiste et album dessous (`docs/plan-ecouter-v0.3.md`,
 * point 1 ; idée de Disc Player, `src/layout/VisualizerMode.vue`, licence MIT).
 * Un écran au-dessus des autres, comme la palette : le transport et la file
 * restent utilisables, Échap ferme. Pas un sixième mode du rail.
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
const reduit = matchMedia("(prefers-reduced-motion: reduce)");

const etat = {
  chemin: undefined, // le morceau affiché
  bandes: [], // énergie lissée de chaque bande, 0..1
  image: 0, // requestAnimationFrame
  focusAvant: null,
  jeton: 0,
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
  const img = t ? await pochette(t.path) : null;
  if (jeton !== etat.jeton) return;
  disque.style.backgroundImage = img ? `url("${img}")` : "";
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
  const centre = taille / 2;
  const rayon = disque.clientWidth / 2 + 6;
  const long = centre - rayon - 4;
  const pas = Math.PI / n;
  ctx.lineCap = "round";
  ctx.lineWidth = Math.max(2, (rayon * pas) * 0.55);
  for (let k = 0; k < n; k++) {
    const v = etat.bandes[k];
    ctx.strokeStyle = couleurDe(v);
    const l = 3 + long * v ** 1.4;
    for (const sens of [1, -1]) {
      const a = -Math.PI / 2 + sens * (k + 0.5) * pas;
      ctx.beginPath();
      ctx.moveTo(centre + rayon * Math.cos(a), centre + rayon * Math.sin(a));
      ctx.lineTo(centre + (rayon + l) * Math.cos(a), centre + (rayon + l) * Math.sin(a));
      ctx.stroke();
    }
  }
}

function boucle() {
  if (vue.hidden) return;
  const t = morceauEnCours();
  if ((t?.path ?? null) !== etat.chemin) poserMorceau(t).catch((e) => remonter(e, "en cours"));
  vue.classList.toggle("en-cours--joue", transport.veutJouer);
  if (!reduit.matches) dessinerSpectre();
  etat.image = requestAnimationFrame(boucle);
}

function ouvrirEnCours() {
  if (!vue.hidden) return;
  etat.focusAvant = document.activeElement;
  vue.hidden = false;
  etat.chemin = undefined; // force la pose du morceau
  $("en-cours-fermer").focus();
  boucle();
}

function fermerEnCours() {
  if (vue.hidden) return;
  vue.hidden = true;
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
