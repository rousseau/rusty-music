import "./pratiquer.js"; // l'ordre d'exécution : voir main.js
import { invoke, remonter } from "./socle.js";
import { lecture } from "./etat-lecture.js";
import {
  appliquerVitesses, choisirVariante, deplacerLecture, dessinerReglages, edition, majBoucleUI,
  reposerBoucle,
} from "./etabli.js";
import { majUsage } from "./pratiquer.js";

/* ------------------------------------------- la mémoire de Pratiquer */

// Ce que Pratiquer garde d'un morceau à l'autre, par morceau (E1 ;
// `docs/ui-spec-editeur.md`, question tranchée le 9 nov. 2026) : instrument,
// notation, décompte, variante de démixage, vitesse et vitesse atteinte,
// boucle posée et boucles nommées, position. Une table SQLite par migration
// (`crates/core/src/db/pratique.rs`), pas un format de projet. Créer n'y
// écrit rien : ses stems sur le disque lui suffisent (décision 3).
//
// Rendue une fois par morceau, quand ses stems sont chargés en Pratiquer ;
// gardée dès qu'un réglage change (relevé toutes les deux secondes), et au
// moment de quitter le morceau.

const memoire = {
  // Le morceau dont la mémoire a été rendue, et que `garder` peut écrire.
  path: null,
  // Le dernier état écrit, sans la position, et la position écrite.
  cle: null,
  position: null,
};

const RELEVE_MS = 2000;

const arrondi = (v) => (v === null || v === undefined ? null : Math.round(v * 1000) / 1000);

/// Ce que l'établi montre, sous la forme que garde le moteur
/// (`MemoirePratique`).
function instantane() {
  const b = edition.boucle;
  const boucles = b
    ? [{
        nom: null, debut_s: b.debut, fin_s: b.fin, mesures: b.mesures ?? null,
        vitesse_atteinte: b.atteinte ?? null,
      }]
    : [];
  return {
    instrument: edition.instrument,
    notation: edition.notation,
    decompte: edition.decompte,
    variante: edition.varianteStems,
    vitesse: arrondi(edition.vitesse),
    vitesse_atteinte: edition.vitesseAtteinte,
    mesures_boucle: edition.mesuresBoucle,
    accel_boucle: edition.accelBoucle,
    position_s: lecture.stems.charges ? arrondi(lecture.stems.positionMs / 1000) : memoire.position,
    // Les boucles nommées sont dans `edition` : l'établi les nomme, les
    // rappelle et les oublie (V17).
    boucles: [...boucles, ...edition.bouclesNommees.map((n) => ({ ...n }))],
  };
}

const cleDe = (m) => JSON.stringify({ ...m, position_s: null });

/// Écrit la mémoire du morceau ouvert si un réglage a changé — ou la
/// position, quand la lecture est en pause ou qu'on quitte le morceau
/// (`quitter`) : pendant qu'elle avance, on ne l'écrit pas toutes les deux
/// secondes.
async function garderMemoirePratique({ quitter = false } = {}) {
  const path = memoire.path;
  if (!path || edition.source?.path !== path || edition.usage !== "pratiquer") return;
  const m = instantane();
  const cle = cleDe(m);
  const p = m.position_s;
  const bouge = p !== null && (memoire.position === null || Math.abs(p - memoire.position) >= 1);
  if (cle === memoire.cle && !(bouge && (quitter || lecture.stems.enPause))) return;
  memoire.cle = cle;
  memoire.position = p;
  try {
    await invoke("pratique_garder", { path, memoire: m });
  } catch (e) {
    remonter(e, "mémoire de Pratiquer");
  }
}

/// Garde la mémoire du morceau qu'on quitte, puis l'oublie : le suivant
/// rendra la sienne. Appelée par `poserSourceEdition` avant le changement.
async function quitterMemoirePratique() {
  await garderMemoirePratique({ quitter: true });
  memoire.path = null;
  edition.bouclesNommees = [];
  memoire.cle = null;
  memoire.position = null;
  edition.vitesseAtteinte = null;
  edition.varianteStems = null;
}

/// Rend la séance du morceau ouvert, une fois, quand ses stems sont chargés
/// en Pratiquer. Sans mémoire, rien ne bouge : les réglages en place
/// deviennent ceux du morceau dès qu'on en change un.
async function rendreMemoirePratique() {
  const t = edition.source;
  if (!t || edition.usage !== "pratiquer" || !lecture.stems.charges || memoire.path === t.path) return;
  let m = null;
  try {
    m = await invoke("pratique_memoire", { path: t.path });
  } catch (e) {
    remonter(e, "mémoire de Pratiquer");
  }
  // Un autre morceau a été ouvert pendant la lecture de la base.
  if (edition.source?.path !== t.path || memoire.path === t.path) return;
  memoire.path = t.path;
  edition.bouclesNommees = (m?.boucles ?? [])
    .filter((b) => b.nom !== null)
    .sort((x, y) => x.debut_s - y.debut_s);
  if (m) await appliquer(m);
  else majBoucleUI();
  memoire.cle = cleDe(instantane());
  memoire.position = m?.position_s ?? null;
}

async function appliquer(m) {
  if (m.instrument && edition.stems.some((s) => s.nom === m.instrument)) edition.instrument = m.instrument;
  if (m.notation) edition.notation = m.notation;
  if (m.decompte !== null && m.decompte !== undefined) edition.decompte = m.decompte;
  // Un démixage lancé pendant la séance l'emporte sur la variante retenue.
  if (m.variante && !edition.varianteStems) {
    edition.varianteStems = m.variante;
    // Le sélecteur montre la variante des stems du morceau, sauf choix fait
    // à la main pour le prochain démixage.
    if (!edition.varianteChoisie) choisirVariante(m.variante);
  }
  if (m.vitesse) edition.vitesse = m.vitesse;
  edition.vitesseAtteinte = m.vitesse_atteinte ?? null;
  if (m.mesures_boucle) edition.mesuresBoucle = m.mesures_boucle;
  edition.accelBoucle = !!m.accel_boucle;
  const b = (m.boucles ?? []).find((x) => x.nom === null);
  edition.boucle = b
    ? { debut: b.debut_s, fin: b.fin_s, mesures: b.mesures ?? null, atteinte: b.vitesse_atteinte ?? null }
    : null;
  await appliquerVitesses();
  await reposerBoucle();
  // La position, seulement si rien ne joue : un morceau qu'on écoutait en
  // Écouter repart d'où il en était (`prendreLaMain`).
  if (lecture.stems.enPause && m.position_s !== null && m.position_s !== undefined && lecture.stems.dureeMs) {
    await deplacerLecture((m.position_s * 1000) / lecture.stems.dureeMs);
  }
  dessinerReglages();
  majBoucleUI();
  majUsage();
}

setInterval(() => {
  garderMemoirePratique().catch((e) => remonter(e, "mémoire de Pratiquer"));
}, RELEVE_MS);
// À la fermeture de la fenêtre : la dernière position, au mieux.
window.addEventListener("pagehide", () => {
  garderMemoirePratique({ quitter: true }).catch(() => {});
});

export { garderMemoirePratique, quitterMemoirePratique, rendreMemoirePratique };
