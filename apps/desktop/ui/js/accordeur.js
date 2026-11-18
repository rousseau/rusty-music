import "./coach.js"; // l'ordre d'exécution : voir main.js
import { $, invoke, journal } from "./socle.js";
import { edition } from "./etabli.js";
import { transpositionDe } from "./etabli-reglages.js";
import { annoncer, arreter as arreterCoach, coach } from "./coach.js";
import { basseAffichee } from "./pratiquer.js";
import { aJouer, nomCorde } from "./accordage.js";
import { brancherChoixEntree, entreeAOuvrir, garderEntree, listerEntrees, montrerEntrees } from "./entree-basse.js";

/* ------------------------------------------- pratiquer : l'accordeur de basse */

// V36 : vérifier l'accord de la basse sans quitter Pratiquer. Le bouton au
// diapason du bandeau du coach ouvre, au centre, la note la plus proche, son
// écart en cents sur une jauge de −50 à +50, la corde visée (si grave, mi,
// la, ré, sol) et « ✓ juste » à ±3 cents — un signe, pas seulement une
// couleur. L'entrée est celle du coach de basse (`coach_basse_ecouter`, le
// canal retenu par V34), la hauteur celle du coach (pYIN), affinée par le
// moteur (`editor::coach::accordeur`) et lissée ici pour une aiguille stable.
// Aucun son n'est joué. L'accordeur et l'écoute du coach se coupent l'un
// l'autre, en le disant.
//
// La référence (décision de François, 10 nov.) : La 440 par défaut ; « sur le
// morceau », l'accord réel du disque mesuré sur le stem de basse
// (`coach_basse_accord_disque`), plus la transposition en cours — entière,
// elle ne change pas l'écart en cents. Le choix se garde dans le navigateur,
// comme la notation de la basse.

const accordeur = {
  ouvert: false,
  minuteur: null,
  occupe: false,
  // L'écart lissé de la note en cours (cents), et cette note.
  lisse: null,
  note: null,
  laHz: 440,
  // Le disque du morceau ouvert : son id, ce que le moteur en a mesuré.
  disque: null,
};

const NOMS = ["do", "do♯", "ré", "ré♯", "mi", "fa", "fa♯", "sol", "sol♯", "la", "la♯", "si"];
const nomNote = (h) => `${NOMS[h % 12]}${Math.floor(h / 12) - 1}`;
const CORDES = [23, 28, 33, 38, 43];

/// Les cordes visées (V37) : l'accordage à utiliser avec la hauteur en
/// cours — celui de la tablature, décalé de la transposition ; sans
/// tablature, si mi la ré sol.
function cordesVisees() {
  const cordes = basseAffichee()?.pret?.accordage;
  return cordes?.length ? aJouer(cordes, transpositionDe("bass")) : CORDES;
}

/// Lissage de l'aiguille : la part de la nouvelle mesure (toutes les 100 ms).
const LISSAGE = 0.35;
const JUSTE_CENTS = 3;

const CLE_MORCEAU = "accordeur-morceau";
function surMorceau() {
  try {
    return localStorage.getItem(CLE_MORCEAU) === "1";
  } catch {
    return false;
  }
}

/// Le bouton au diapason : là quand le bandeau du coach montre la basse.
function majBoutonAccordeur(basse) {
  const b = $("coach-accordeur");
  b.hidden = !basse;
  b.setAttribute("aria-pressed", String(accordeur.ouvert));
  if (!basse && accordeur.ouvert) fermerAccordeur();
}

async function ouvrirAccordeur() {
  if (accordeur.ouvert) return;
  if (coach.actif) {
    await arreterCoach();
    annoncer("Le coach cesse d'écouter : l'accordeur prend l'entrée.");
  }
  accordeur.ouvert = true;
  accordeur.lisse = null;
  accordeur.note = null;
  $("accordeur").hidden = false;
  $("coach-accordeur").setAttribute("aria-pressed", "true");
  $("accordeur-morceau").checked = surMorceau();
  afficher(null, "Ouverture de l'entrée…");
  // V42 : la même entrée que le coach (`entree-basse.js`), choisissable ici.
  const liste = await listerEntrees("accordeur");
  const nom = entreeAOuvrir(liste);
  montrerEntrees(liste, nom);
  if (!accordeur.ouvert) return;
  if (!nom) {
    afficher(null, "Aucune entrée audio.");
    return;
  }
  try {
    await invoke("coach_basse_ecouter", { entree: nom });
  } catch (e) {
    afficher(null, `Entrée audio injoignable : ${e}`);
    return;
  }
  // Gardée avec le défaut du moment, comme au coach : un défaut suivi
  // efface le choix fait sous l'ancien.
  garderEntree(nom);
  await poserReference();
  clearInterval(accordeur.minuteur);
  accordeur.minuteur = setInterval(sonder, 100);
}

/// Ferme l'accordeur ; `garderEntree` : le coach reprend l'entrée ouverte.
async function fermerAccordeur(garderEntree = false) {
  if (!accordeur.ouvert) return;
  accordeur.ouvert = false;
  clearInterval(accordeur.minuteur);
  accordeur.minuteur = null;
  $("accordeur").hidden = true;
  $("coach-accordeur").setAttribute("aria-pressed", "false");
  if (garderEntree) return;
  try {
    await invoke("coach_basse_ecouter", { entree: null });
  } catch (e) {
    journal("accordeur", `fermer : ${e}`, "warn");
  }
}

/// La référence : La 440, ou l'accord du disque (« sur le morceau »).
async function poserReference() {
  accordeur.laHz = 440;
  const dit = $("accordeur-disque");
  dit.textContent = "La 440";
  if (!surMorceau()) return;
  const id = edition.source?.id;
  if (id === undefined) {
    dit.textContent = "pas de morceau : La 440";
    return;
  }
  if (accordeur.disque?.id !== id) {
    dit.textContent = "mesure du disque…";
    try {
      accordeur.disque = { id, vue: await invoke("coach_basse_accord_disque", { id }) };
    } catch (e) {
      dit.textContent = `disque non mesuré : La 440`;
      journal("accordeur", `disque : ${e}`, "warn");
      return;
    }
  }
  const v = accordeur.disque.vue;
  if (!v.stem) {
    dit.textContent = "pas de stem de basse : La 440";
    return;
  }
  if (v.ecart_cents === null) {
    dit.textContent = `trop peu de notes sûres sur le stem : La 440`;
    return;
  }
  // La transposition entière déplace la grille d'autant de demi-tons :
  // ramené au demi-ton, l'écart reste celui du disque.
  const total = v.ecart_cents + 100 * transpositionDe("bass");
  const ecart = total - 100 * Math.round(total / 100);
  accordeur.laHz = 440 * 2 ** (ecart / 1200);
  const e = Math.round(ecart);
  dit.textContent = `le disque est à ${e > 0 ? "+" : e < 0 ? "−" : "±"}${Math.abs(e)} cent${Math.abs(e) > 1 ? "s" : ""} (La ${accordeur.laHz.toFixed(1).replace(".", ",")})`;
}

async function sonder() {
  if (accordeur.occupe || !accordeur.ouvert) return;
  accordeur.occupe = true;
  try {
    const v = await invoke("coach_basse_accorder", { laHz: accordeur.laHz, cordes: cordesVisees() });
    if (!accordeur.ouvert) return;
    if (v.erreur) afficher(null, `Entrée audio : ${v.erreur}`);
    else afficher(v.accord, "Jouez une corde.");
  } catch (e) {
    afficher(null, String(e));
  } finally {
    accordeur.occupe = false;
  }
}

/// La lecture : note, cents lissés, aiguille, corde visée ; sans note, le
/// message.
function afficher(a, message) {
  const aiguille = $("accordeur-aiguille");
  const juste = $("accordeur-juste");
  if (!a) {
    accordeur.lisse = null;
    accordeur.note = null;
    $("accordeur-note").textContent = "–";
    $("accordeur-cents").textContent = "";
    $("accordeur-corde").textContent = message;
    aiguille.hidden = true;
    juste.hidden = true;
    $("accordeur").classList.remove("accordeur--juste");
    return;
  }
  if (a.note !== accordeur.note || accordeur.lisse === null) accordeur.lisse = a.cents;
  else accordeur.lisse += LISSAGE * (a.cents - accordeur.lisse);
  accordeur.note = a.note;
  const c = accordeur.lisse;
  const r = Math.round(c);
  $("accordeur-note").textContent = nomNote(a.note);
  $("accordeur-cents").textContent = `${r > 0 ? "+" : r < 0 ? "−" : "±"}${Math.abs(r)} cent${Math.abs(r) > 1 ? "s" : ""}`;
  const ok = Math.abs(c) <= JUSTE_CENTS;
  juste.hidden = !ok;
  $("accordeur").classList.toggle("accordeur--juste", ok);
  aiguille.hidden = false;
  aiguille.style.left = `${50 + Math.max(-50, Math.min(50, c))}%`;
  const corde = nomCorde(a.corde, cordesVisees());
  const ec = Math.round(a.cents_corde);
  const sens = Math.abs(a.cents_corde) <= JUSTE_CENTS ? "juste" : a.cents_corde > 0 ? "trop haut : détendre" : "trop bas : tendre";
  $("accordeur-corde").textContent =
    Math.abs(a.cents_corde) < 100
      ? `corde de ${corde} (${nomNote(a.corde)}) : ${ec > 0 ? "+" : ec < 0 ? "−" : "±"}${Math.abs(ec)} cents, ${sens}`
      : `la plus proche : corde de ${corde} (${nomNote(a.corde)})`;
}

$("coach-accordeur").addEventListener("click", () => (accordeur.ouvert ? fermerAccordeur() : ouvrirAccordeur()));
$("accordeur-fermer").addEventListener("click", () => fermerAccordeur());
$("accordeur-morceau").addEventListener("change", async (e) => {
  try {
    localStorage.setItem(CLE_MORCEAU, e.target.checked ? "1" : "0");
  } catch {}
  accordeur.lisse = null;
  await poserReference();
});
// La hauteur des stems change : la référence « sur le morceau » la suit.
document.addEventListener("hauteur-appliquee", () => {
  if (accordeur.ouvert) poserReference();
});


// V42 : une autre entrée choisie (ici, dans le retour, ou au coach) pendant
// que l'accordeur écoute : il la rouvre.
brancherChoixEntree($("accordeur-entree"));
document.addEventListener("entree-basse", async (e) => {
  if (!accordeur.ouvert) return;
  e.detail.prise = true;
  accordeur.lisse = null;
  accordeur.note = null;
  try {
    await invoke("coach_basse_ecouter", { entree: e.detail.nom });
  } catch (err) {
    afficher(null, `Entrée audio injoignable : ${err}`);
  }
});

export { accordeur, fermerAccordeur, majBoutonAccordeur };
