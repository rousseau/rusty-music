// SPDX-License-Identifier: GPL-3.0-or-later
// Largeur du rail et du panneau droit (inspecteur et file), réglable à la
// souris ou au clavier : une poignée entre chaque panneau et le centre.
//
// Les largeurs restent les jetons du gabarit (`--largeur-rail`,
// `--largeur-droite`, Règle 10 de `docs/interface-guidelines.md`) : on ne
// change que leur valeur, posée sur `<html>`. Tout ce qui s'y aligne (la file
// qui recouvre exactement l'inspecteur, le panneau d'un outil de la carte, la
// vue « En cours » quand la file est ouverte) suit sans rien savoir des
// poignées. Mémorisées par appareil (`localStorage`), reposées avant la
// première peinture par `theme-avant.js` ; un double-clic rend la largeur
// d'origine.
//
// Hors de la chaîne de `main.js` : le module ne dépend d'aucun autre et
// n'ordonne rien.

export const CLE_PANNEAUX = "rusty-music.panneaux";

/// Bornes de chaque panneau (px). Sous le minimum, les blocs du rail et
/// l'inspecteur se tassent ; au-delà du maximum, le centre perd plus qu'ils
/// ne gagnent.
const BORNES = {
  rail: { jeton: "--largeur-rail", defaut: 232, min: 200, max: 420 },
  droite: { jeton: "--largeur-droite", defaut: 300, min: 260, max: 560 },
};
/// Le centre garde au moins cette largeur, quelle que soit la fenêtre.
const CENTRE_MIN = 440;
/// Pas des flèches du clavier (Maj : un pas plus grand).
const PAS = 16;
const PAS_MAJ = 64;

const racine = document.documentElement;

function lire() {
  try {
    const o = JSON.parse(localStorage.getItem(CLE_PANNEAUX) || "{}");
    return o && typeof o === "object" ? o : {};
  } catch {
    return {};
  }
}

function ecrire(largeurs) {
  try {
    localStorage.setItem(CLE_PANNEAUX, JSON.stringify(largeurs));
  } catch {
    /* stockage indisponible : la largeur vaut pour la session */
  }
}

/// Largeur choisie (mémorisée ou d'origine) avant contrainte de la fenêtre.
const choisies = lire();
const voulue = (cote) => {
  const v = Number(choisies[cote]);
  return Number.isFinite(v) ? v : BORNES[cote].defaut;
};

/// Borne une largeur : celle du panneau, puis la place laissée au centre par
/// l'autre panneau à sa largeur effective.
function borner(cote, v) {
  const b = BORNES[cote];
  const autre = cote === "rail" ? "droite" : "rail";
  const place = innerWidth - CENTRE_MIN - effective(autre);
  return Math.round(Math.max(b.min, Math.min(b.max, place, v)));
}

const effectives = { rail: BORNES.rail.defaut, droite: BORNES.droite.defaut };
const effective = (cote) => effectives[cote];

/// Pose la largeur `v` (déjà voulue) sur le jeton, bornée par la fenêtre.
function poser(cote, v) {
  const b = BORNES[cote];
  const w = borner(cote, v);
  effectives[cote] = w;
  if (w === b.defaut) racine.style.removeProperty(b.jeton);
  else racine.style.setProperty(b.jeton, `${w}px`);
  const p = poignees[cote];
  if (p) {
    p.setAttribute("aria-valuenow", String(w));
    p.setAttribute("aria-valuemax", String(Math.max(b.min, borner(cote, b.max))));
  }
  return w;
}

/// Prévient les vues dessinées (nuage, carte, frise, Lama…) qui écoutent déjà
/// `resize` : leur conteneur a changé de taille sans que la fenêtre bouge.
let prevenir = 0;
function prevenirVues() {
  if (prevenir) return;
  prevenir = requestAnimationFrame(() => {
    prevenir = 0;
    dispatchEvent(new Event("resize"));
  });
}
let enRedimension = false;

const poignees = {};

function brancher(cote, poignee) {
  const b = BORNES[cote];
  poignees[cote] = poignee;
  poignee.setAttribute("role", "separator");
  poignee.setAttribute("aria-orientation", "vertical");
  poignee.setAttribute("aria-valuemin", String(b.min));
  poignee.tabIndex = 0;
  // Le rail grandit vers la droite, le panneau droit vers la gauche.
  const sens = cote === "rail" ? 1 : -1;

  const changer = (v, memoriser) => {
    const w = poser(cote, v);
    if (memoriser) {
      choisies[cote] = w;
      if (w === b.defaut) delete choisies[cote];
      ecrire(choisies);
    }
    prevenirVues();
  };

  poignee.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    poignee.setPointerCapture(e.pointerId);
    const x0 = e.clientX;
    const w0 = effective(cote);
    document.body.classList.add("panneaux--glisse");
    poignee.classList.add("poignee--active");
    enRedimension = true;
    const bouger = (ev) => changer(w0 + sens * (ev.clientX - x0), false);
    const lacher = () => {
      poignee.removeEventListener("pointermove", bouger);
      poignee.removeEventListener("pointerup", lacher);
      poignee.removeEventListener("pointercancel", lacher);
      document.body.classList.remove("panneaux--glisse");
      poignee.classList.remove("poignee--active");
      enRedimension = false;
      changer(effective(cote), true);
    };
    poignee.addEventListener("pointermove", bouger);
    poignee.addEventListener("pointerup", lacher);
    poignee.addEventListener("pointercancel", lacher);
  });

  poignee.addEventListener("dblclick", () => changer(b.defaut, true));

  poignee.addEventListener("keydown", (e) => {
    const pas = e.shiftKey ? PAS_MAJ : PAS;
    let v = null;
    // Les flèches déplacent la poignée, pas la largeur : → élargit le rail
    // et rétrécit le panneau droit.
    if (e.key === "ArrowRight") v = effective(cote) + sens * pas;
    else if (e.key === "ArrowLeft") v = effective(cote) - sens * pas;
    else if (e.key === "Home") v = b.min;
    else if (e.key === "End") v = b.max;
    else if (e.key === "Enter") v = b.defaut;
    if (v == null) return;
    e.preventDefault();
    changer(v, true);
  });
}

function creerPoignee(cote, titre) {
  const p = document.createElement("div");
  p.className = `poignee poignee--${cote}`;
  p.setAttribute("aria-label", titre);
  p.title = `${titre} — glisser ; double-clic : largeur d'origine`;
  return p;
}

const atelier = document.querySelector(".atelier");
if (atelier) {
  const gauche = creerPoignee("rail", "Largeur du rail");
  const droite = creerPoignee("droite", "Largeur du panneau de droite");
  atelier.append(gauche, droite);
  brancher("rail", gauche);
  brancher("droite", droite);
  // Le rail d'abord, compté contre le panneau droit à son minimum : à la
  // fenêtre trop étroite pour les deux largeurs voulues, c'est le panneau
  // droit qui cède. Une fenêtre qui s'élargit rend les largeurs voulues.
  const ajuster = () => {
    effectives.droite = BORNES.droite.min;
    poser("rail", voulue("rail"));
    poser("droite", voulue("droite"));
  };
  ajuster();
  addEventListener("resize", () => {
    if (!enRedimension) ajuster();
  });
}
