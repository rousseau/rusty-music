// SPDX-License-Identifier: GPL-3.0-or-later
import { invoke, journal } from "./socle.js";

/* ------------------------------ pratiquer : l'entrée audio de la basse (V42) */

// Le coach, l'accordeur et le retour (« Entendre ma basse ») écoutent la même
// entrée ; ce module dit laquelle ouvrir, et garde le choix fait à la main.
//
// La règle : l'entrée par défaut de macOS, sauf choix fait à la main **tant
// que ce défaut n'a pas bougé**. Le choix se garde avec le défaut du moment ;
// si macOS a changé de défaut depuis (la Scarlett branchée et choisie dans
// les réglages du Mac), c'est le nouveau défaut qui s'ouvre. Avant V42, le
// nom gardé l'emportait toujours : le micro du Mac, gardé une fois, passait
// devant l'interface choisie dans macOS.
//
// Chaque ouverture garde l'entrée ouverte avec le défaut du moment
// (`garderEntree`) : un défaut suivi efface ainsi le choix fait sous l'ancien,
// et revenir ensuite au premier défaut ne ressuscite pas ce choix.
//
// Le moteur met l'entrée par défaut en tête de `coach_basse_entrees`
// (`coach_basse.rs`) : la première de la liste est le défaut du moment.
//
// Module déclaratif, hors de la chaîne de `main.js` : il n'écoute rien à son
// chargement.

const CLE = "coach-entree";
const CLE_DEFAUT = "coach-entree-defaut";

// Le défaut de macOS à la dernière liste lue.
let defautVu = null;
// Les sélecteurs d'entrée de l'accordeur et du retour (celui du coach sert
// aussi à la batterie MIDI : le coach le tient lui-même).
const choix = [];

function lire(cle) {
  try {
    return localStorage.getItem(cle);
  } catch {
    return null;
  }
}

/// Les entrées audio, le défaut de macOS en tête ; `[]` si l'audio ne répond pas.
async function listerEntrees(qui) {
  try {
    const liste = await invoke("coach_basse_entrees");
    defautVu = liste[0] ?? null;
    return liste;
  } catch (e) {
    journal(qui, `entrées : ${e}`, "warn");
    return [];
  }
}

/// L'entrée à ouvrir parmi `liste` (lue de `coach_basse_entrees`) : le choix
/// gardé si le défaut de macOS est celui du moment où il a été fait, sinon le
/// défaut. Retient ce défaut pour `garderEntree`.
function entreeAOuvrir(liste, garde = lire(CLE), defautAlors = lire(CLE_DEFAUT)) {
  const defaut = liste[0] ?? null;
  defautVu = defaut;
  if (garde && liste.includes(garde) && defautAlors === defaut) return garde;
  return defaut;
}

/// Garde `nom`, avec le défaut de macOS du moment.
function garderEntree(nom) {
  try {
    localStorage.setItem(CLE, nom);
    localStorage.setItem(CLE_DEFAUT, defautVu ?? "");
  } catch {}
}

/// Remplit les sélecteurs d'entrée : `liste`, `nom` choisi. Une seule entrée
/// se montre aussi (on voit ce qui est écouté), sans choix possible.
function montrerEntrees(liste, nom) {
  for (const s of choix) {
    s.replaceChildren(...liste.map((n) => new Option(n, n)));
    if (nom) s.value = nom;
    s.disabled = liste.length < 2;
    s.closest("label").hidden = !liste.length;
  }
}

/// L'entrée a été choisie à la main, d'un des sélecteurs : gardée, montrée
/// partout, puis rouverte par celui qui écoute (`entree-basse` : le coach ou
/// l'accordeur la prennent en posant `detail.prise`) ; sinon, si le retour
/// écoute seul, rouverte ici.
async function choisirEntree(nom) {
  garderEntree(nom);
  for (const s of choix) s.value = nom;
  const e = new CustomEvent("entree-basse", { detail: { nom, prise: false } });
  document.dispatchEvent(e);
  if (e.detail.prise) return;
  try {
    const v = await invoke("retour_basse_etat");
    if (v.ecoute) await invoke("coach_basse_ecouter", { entree: nom });
  } catch (err) {
    journal("entrée", `${nom} : ${err}`, "warn");
  }
}

/// Branche un sélecteur d'entrée (dans un `<label>`) sur le choix commun.
function brancherChoixEntree(select) {
  choix.push(select);
  select.addEventListener("change", () => choisirEntree(select.value));
}

export { brancherChoixEntree, choisirEntree, entreeAOuvrir, garderEntree, listerEntrees, montrerEntrees };
