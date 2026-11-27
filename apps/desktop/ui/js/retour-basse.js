import "./accordeur.js"; // l'ordre d'exécution : voir main.js
import { $, invoke, journal } from "./socle.js";
import { edition } from "./etabli.js";
import { brancherChoixEntree, entreeAOuvrir, garderEntree, listerEntrees, montrerEntrees } from "./entree-basse.js";

/* ------------------------------------------ pratiquer : entendre sa basse (E7) */

// La basse branchée sur l'interface (Scarlett, entrée instrument) ne
// s'entendait pas : le Mac jouait le morceau seul. Le bouton au haut-parleur
// du bandeau du coach (là où est le diapason) ouvre, au centre, de quoi
// l'entendre dans la sortie de l'ordinateur : un son parmi huit préréglages,
// son volume, la balance morceau / basse. Le moteur (`retour_basse.rs`)
// prend l'entrée du coach (le canal retenu par V34), toujours analysée
// brute ; le retour naît coupé et à mi-volume. Une entrée qui semble être
// un micro est signalée (larsen sur des enceintes) : il faut cocher une
// seconde fois pour l'entendre quand même.
//
// Le retour vit avec l'écoute de l'entrée : quand le coach ou l'accordeur la
// rouvre ou la ferme, le module la rouvre et rallume le son tant qu'il est
// voulu. Il s'éteint en quittant la basse dans Pratiquer.
//
// La guitare aussi (26 nov.) : le haut-parleur suit l'instrument du bandeau
// du coach (basse ou guitare), les textes nomment celui-là, et « comme le
// disque » prend son stem.

const retour = {
  ouvert: false,
  // Le son est demandé (case cochée).
  voulu: false,
  // L'avertissement de micro a été vu : la case cochée à nouveau passe outre.
  microAccepte: false,
  vue: null,
  minuteur: null,
  occupe: false,
  envoi: null,
};

/// L'entrée à écouter si personne ne l'a ouverte : celle du coach (V42 :
/// `entree-basse.js`).
async function ouvrirEntree() {
  const liste = await listerEntrees("retour");
  const nom = entreeAOuvrir(liste);
  montrerEntrees(liste, nom);
  if (!nom) return false;
  try {
    await invoke("coach_basse_ecouter", { entree: nom });
    garderEntree(nom);
    return true;
  } catch (e) {
    journal("retour", `écoute : ${e}`, "warn");
    return false;
  }
}

function reglages(actif) {
  return {
    actif,
    prereglage: $("retour-prereglage").value || "brut",
    volume: Number($("retour-volume").value) / 100,
    balance: Number($("retour-balance").value) / 100,
    id: edition.source?.id ?? null,
    instrument: nomInstrument === "guitare" ? "guitar" : "bass",
  };
}

/// L'instrument entendu, tel que les textes le nomment.
let nomInstrument = "basse";
/// Les textes d'origine (écrits pour la basse) : attributs et nœuds texte.
const originaux = new WeakMap();

/// Renomme la basse en guitare (ou l'inverse) dans le bouton et le panneau.
function renommer(nom) {
  if (nom === nomInstrument) return;
  nomInstrument = nom;
  const changer = (orig) => orig.replaceAll("basse", nom);
  for (const racine of [$("coach-retour"), $("retour-basse")]) {
    for (const el of [racine, ...racine.querySelectorAll("*")]) {
      if (!originaux.has(el)) {
        originaux.set(el, { "aria-label": el.getAttribute("aria-label"), title: el.getAttribute("title") });
      }
      for (const [a, v] of Object.entries(originaux.get(el))) if (v !== null) el.setAttribute(a, changer(v));
      for (const n of el.childNodes) {
        if (n.nodeType !== Node.TEXT_NODE) continue;
        if (!originaux.has(n)) originaux.set(n, n.textContent);
        n.textContent = changer(originaux.get(n));
      }
    }
  }
  if (retour.vue) $("retour-etat").textContent = etat(retour.vue);
  // « Comme le disque » change de stem.
  if (retour.voulu) regler(true);
}

/// Envoie les réglages ; `actif` : le son doit sortir.
async function regler(actif) {
  try {
    afficher(await invoke("retour_basse_regler", reglages(actif)));
  } catch (e) {
    $("retour-etat").textContent = `Retour injoignable : ${e}`;
    journal("retour", `régler : ${e}`, "warn");
  }
}

/// Le son demandé : ouvre l'entrée s'il le faut, puis le son, sauf micro
/// non accepté.
async function allumer() {
  let v = await invoke("retour_basse_etat");
  if (!v.ecoute) {
    if (!(await ouvrirEntree())) {
      retour.voulu = false;
      afficher(v, "Aucune entrée audio : branchez l'interface.");
      return;
    }
    v = await invoke("retour_basse_etat");
  }
  if (!v.disponible) {
    retour.voulu = false;
    afficher(v, `Aucune sortie audio pour la ${nomInstrument}.`);
    return;
  }
  if (v.micro && !retour.microAccepte) {
    retour.voulu = false;
    afficher(v);
    return;
  }
  await regler(true);
}

async function eteindre() {
  retour.voulu = false;
  await regler(false);
}

function afficher(v, message) {
  if (!v) return;
  retour.vue = v;
  const choix = $("retour-prereglage");
  if (choix.options.length !== v.prereglages.length) {
    choix.replaceChildren(
      ...v.prereglages.map((p) => {
        const o = document.createElement("option");
        o.value = p.nom;
        o.textContent = p.libelle;
        return o;
      }),
    );
  }
  if (document.activeElement !== choix) choix.value = v.prereglage;
  if (document.activeElement !== $("retour-volume")) $("retour-volume").value = String(Math.round(v.volume * 100));
  if (document.activeElement !== $("retour-balance")) $("retour-balance").value = String(Math.round(v.balance * 100));
  $("retour-actif").checked = v.actif || retour.voulu;
  $("coach-retour").setAttribute("aria-pressed", String(v.actif));
  const avert = $("retour-avertissement");
  avert.hidden = !v.micro;
  avert.textContent = v.micro
    ? retour.microAccepte || v.actif
      ? "Cette entrée semble être un micro : gare au larsen sur des enceintes, un casque est plus sûr."
      : "Cette entrée semble être un micro : sur des enceintes, gare au larsen. Cochez à nouveau pour l'entendre quand même (un casque est plus sûr)."
    : "";
  $("retour-etat").textContent = message ?? etat(v);
}

function etat(v) {
  const nom = nomInstrument;
  if (!v.ecoute) return `La ${nom} s'entend dès que l'entrée est ouverte.`;
  if (!v.actif) return v.disponible ? "Coupé." : `Aucune sortie audio pour la ${nom}.`;
  const morceau =
    v.prereglage === "comme-le-disque" && v.stem_disque === false
      ? `pas de stem de ${nom} : son brut · `
      : v.prereglage === "comme-le-disque" && v.jeu_s != null
        ? v.jeu_s > 0
          ? `égalisé d'après ${Math.round(v.jeu_s)} s de votre jeu · `
          : "jouez quelques secondes : le son s'ajuste · "
        : "";
  const latence = v.latence_ms == null ? "" : `latence ≈ ${Math.round(v.latence_ms)} ms`;
  const trous = v.trous ? ` · ${v.trous} trou${v.trous > 1 ? "s" : ""}` : "";
  return `${morceau}${latence}${trous}`;
}

/// Toutes les secondes, tant que le panneau est ouvert ou le son voulu :
/// l'état, et le son rallumé si l'écoute a été rouverte (coach, accordeur).
async function sonder() {
  if (retour.occupe) return;
  retour.occupe = true;
  try {
    const v = await invoke("retour_basse_etat");
    if (retour.voulu && !v.actif && (!v.micro || retour.microAccepte)) {
      if (!v.ecoute) await ouvrirEntree();
      await regler(true);
    } else afficher(v);
  } catch (e) {
    journal("retour", `état : ${e}`, "warn");
  } finally {
    retour.occupe = false;
  }
  if (!retour.ouvert && !retour.voulu) arreterSonde();
}

function lancerSonde() {
  if (!retour.minuteur) retour.minuteur = setInterval(sonder, 1000);
}
function arreterSonde() {
  clearInterval(retour.minuteur);
  retour.minuteur = null;
}

async function ouvrirPanneau() {
  retour.ouvert = true;
  $("retour-basse").hidden = false;
  // L'entrée, choisissable ici aussi (V42) ; celle qu'on ouvrirait, ou que
  // le coach ou l'accordeur écoutent déjà.
  listerEntrees("retour").then((liste) => montrerEntrees(liste, entreeAOuvrir(liste)));
  try {
    afficher(await invoke("retour_basse_etat"));
  } catch (e) {
    $("retour-etat").textContent = `Retour injoignable : ${e}`;
  }
  lancerSonde();
}

function fermerPanneau() {
  retour.ouvert = false;
  $("retour-basse").hidden = true;
  if (!retour.voulu) arreterSonde();
}

$("coach-retour").addEventListener("click", () => (retour.ouvert ? fermerPanneau() : ouvrirPanneau()));
$("retour-fermer").addEventListener("click", fermerPanneau);
brancherChoixEntree($("retour-entree"));
$("retour-actif").addEventListener("change", async (e) => {
  if (e.target.checked) {
    if (retour.vue?.micro) retour.microAccepte = !$("retour-avertissement").hidden || retour.microAccepte;
    retour.voulu = true;
    await allumer();
    lancerSonde();
  } else await eteindre();
});
$("retour-prereglage").addEventListener("change", () => regler(retour.voulu));
// Les curseurs : un envoi par geste, au plus toutes les 60 ms.
for (const id of ["retour-volume", "retour-balance"]) {
  $(id).addEventListener("input", () => {
    clearTimeout(retour.envoi);
    retour.envoi = setTimeout(() => regler(retour.voulu), 60);
  });
}

// Le haut-parleur suit le bandeau du coach : là quand il montre la basse ou
// la guitare (son instrument : `data-instrument`, posé par `coach.js`). Il
// disparaît : le son s'éteint, le panneau se ferme.
const bandeau = $("partition-coach");
new MutationObserver(() => {
  const instrument = bandeau.hidden ? "" : bandeau.dataset.instrument;
  const entendu = instrument === "bass" || instrument === "guitar";
  $("coach-retour").hidden = !entendu;
  if (entendu) renommer(instrument === "guitar" ? "guitare" : "basse");
  else {
    if (retour.ouvert) fermerPanneau();
    if (retour.voulu) eteindre();
  }
}).observe(bandeau, { attributes: true, attributeFilter: ["hidden", "data-instrument"] });

export { retour };
