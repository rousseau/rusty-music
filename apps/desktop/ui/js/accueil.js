import "./pratiquer-playlist.js"; // l'ordre d'exécution : voir main.js
// L'Accueil d'Écouter (`docs/plan-ecouter-accueil.md`, A2) : un album à la
// une, puis des rangées horizontales de pochettes, chacune titrée par la
// raison qui la fait exister. Le moteur compose la page (commande `accueil`,
// `crates/core/src/accueil.rs`) ; ici, on l'affiche et on relaie les « ↻ ».
//
// La vue s'insère dans la navigation d'Écouter comme Albums ou Playlists :
// `poser("accueil", …)`, un clic sur une pochette ouvre l'album avec retour à
// l'Accueil (`ouvrirAlbumDePiste`, qui prend l'instantané de `snapVue`).
import {
  $, ecoute, invoke, navigation, pochette, remonter, txt, vue,
} from "./socle.js";
import { carteAlbum, lireAlbum, ouvrirAlbumDePiste } from "./vues.js";
import { poser, texteCompte } from "./ecouter.js";
import { estRetenu } from "./ecouter-familles.js";
import { couleursPochette } from "./couleurs-album.js";

const vueAccueil = $("accueil-vue");

const accueil = {
  /// La dernière page reçue (`PageAccueil`), ou `null` avant la première.
  page: null,
  /// Le jour (AAAA-MM-JJ local) qui a servi de graine : la page reste la
  /// même toute la journée, et les « ↻ » repartent de zéro le lendemain.
  date: null,
  /// Nombre de « ↻ » par rangée, pour le jour courant.
  decalages: {},
  /// Numéro de la demande en cours : une réponse arrivée après une plus
  /// récente est ignorée.
  jeton: 0,
  /// Une page a été demandée au moins une fois.
  demandee: false,
  /// Cases affichées, avec l'album qu'elles portent : le filtre du rail les
  /// estompe sans les reconstruire.
  cases: [],
};

/// Le jour local, AAAA-MM-JJ : le moteur ne connaît pas le fuseau.
function dateLocale(d = new Date()) {
  const deux = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${deux(d.getMonth() + 1)}-${deux(d.getDate())}`;
}

/// Ce que `vue.lignes` porte pour l'Accueil : les albums de la page, à la une
/// compris, sous la forme des cases de la grille (`name`, `artist`…). Sert au
/// compte du fil et au filtre du rail.
function albumsDe(page) {
  if (!page) return [];
  return [...(page.une ? [page.une.album] : []), ...page.rangees.flatMap((r) => r.albums)];
}

/// Ouvre l'Accueil : la vue s'affiche tout de suite (la page déjà reçue, ou un
/// mot d'attente), puis la page du jour arrive du moteur.
async function chargerAccueil() {
  const date = dateLocale();
  if (date !== accueil.date) {
    accueil.date = date;
    accueil.decalages = {};
  }
  accueil.demandee = true;
  ++ecoute.jetonVue;
  poser("accueil", "Accueil", albumsDe(accueil.page));
  await demanderPage();
}

/// Demande la page au moteur. Le premier calcul d'une session peut prendre
/// quelques secondes (familles, empreintes : `docs/journal.md`, A1) ; les
/// suivants, quelques dizaines de millisecondes.
async function demanderPage() {
  const jeton = ++accueil.jeton;
  const attente = setTimeout(() => {
    if (jeton === accueil.jeton && !accueil.page) {
      $("accueil-etat").textContent = "Préparation de la page d'accueil…";
    }
  }, 300);
  let page;
  try {
    page = await invoke("accueil", { date: accueil.date, decalages: accueil.decalages });
  } catch (e) {
    clearTimeout(attente);
    if (jeton !== accueil.jeton) return;
    $("accueil-etat").textContent = "La page d'accueil n'a pas pu être préparée.";
    remonter(e, "accueil");
    return;
  }
  clearTimeout(attente);
  if (jeton !== accueil.jeton) return;
  accueil.page = page;
  rendre();
}

/// Reprend la page affichée sans rien redemander au moteur — le retour depuis
/// un album (`poser` avec la vue Accueil), un filtre qui change.
function reafficherAccueil(scroll = null) {
  if (!accueil.demandee) {
    chargerAccueil().catch((e) => remonter(e, "accueil"));
    return;
  }
  appliquerFiltreAccueil();
  if (scroll !== null) vueAccueil.scrollTop = scroll;
}

/* ---------------------------------------------------------------- rendu */

/// Les cases ne demandent leur pochette qu'une fois visibles, comme dans la
/// grille : une page porte plus d'une centaine d'albums.
const visibles = new IntersectionObserver(
  (entrees) => {
    for (const e of entrees) {
      if (!e.isIntersecting) continue;
      visibles.unobserve(e.target);
      const charger = chargeurs.get(e.target);
      chargeurs.delete(e.target);
      charger?.();
    }
  },
  { root: vueAccueil, rootMargin: "200px" },
);
const chargeurs = new WeakMap();
function differer(el, charger) {
  chargeurs.set(el, charger);
  visibles.observe(el);
}

function ouvrir(album) {
  return ouvrirAlbumDePiste({ album: album.name, artist: album.artist ?? null });
}

function rendre() {
  const page = accueil.page;
  accueil.cases = [];
  rendreUne(page.une);

  // Une rangée dont les albums n'ont pas changé garde ses cases et son
  // défilement : après un « ↻ », seule la rangée tirée de nouveau se refait.
  const avant = new Map(
    [...$("accueil-rangees").children].map((el) => [el.dataset.signature, el]),
  );
  const rangees = page.rangees.map((r) => {
    const signature = `${r.id}|${r.titre}|${r.albums.map((a) => `${a.name}\n${a.artist ?? ""}`).join("|")}`;
    const el = avant.get(signature) ?? rendreRangee(r);
    el.dataset.signature = signature;
    el._albums.forEach((a, i) => accueil.cases.push([el._cases[i], a]));
    return el;
  });
  $("accueil-rangees").replaceChildren(...rangees);

  const vide = !page.une && page.rangees.length === 0;
  $("accueil-etat").textContent = vide
    ? "Rien à proposer pour l'instant : la page se remplit quand la bibliothèque a des albums."
    : "";

  if (vue.quoi === "accueil") {
    vue.lignes = albumsDe(page);
    appliquerFiltreAccueil();
  }
}

function rendreUne(une) {
  const section = $("accueil-une");
  section.hidden = !une;
  if (!une) return;
  const a = une.album;
  accueil.cases.push([section, a]);
  // Le même album qu'au rendu précédent (un « ↻ » ne change pas la une) :
  // pochette, couleurs et photo sont déjà posées.
  const cle = `${a.name}\n${a.artist ?? ""}`;
  if (section.dataset.album === cle) return;
  section.dataset.album = cle;
  $("accueil-une-raison").textContent = une.raison;
  $("accueil-une-titre").textContent = a.name;
  $("accueil-une-artiste").textContent = [txt(a.artist, "(sans artiste)"), a.year].filter(Boolean).join(" · ");
  $("accueil-une-lire").onclick = () => lireAlbum(a).catch((e) => remonter(e, "lireAlbum"));
  $("accueil-une-ouvrir").onclick = () => ouvrir(a).catch((e) => remonter(e, "ouvrir l'album"));

  // Le fond : les deux couleurs de la pochette, recalées pour le thème
  // (`couleurs-album.js`) ; à défaut, les fonds du thème.
  const image = section.querySelector(".une__pochette");
  image.style.backgroundImage = "";
  section.style.removeProperty("--album-haut");
  section.style.removeProperty("--album-bas");
  const jeton = accueil.jeton;
  poserPhoto(section, a, jeton);
  pochette(a.path)
    .then(async (img) => {
      if (!img || jeton !== accueil.jeton) return;
      image.style.backgroundImage = `url("${img}")`;
      const c = await couleursPochette(img);
      if (!c || jeton !== accueil.jeton) return;
      section.style.setProperty("--album-haut", c.haut);
      section.style.setProperty("--album-bas", c.bas);
    })
    .catch((e) => remonter(e, "couleurs d'album"));
}

/// A3 : l'image large de l'artiste (TheAudioDB, par MBID) derrière l'album à
/// la une, sous un voile aux couleurs de la pochette (`style.css`, `.une__fond`).
/// Sans MBID, sans image ou sans réseau : le dégradé seul, sans message.
function poserPhoto(section, a, jeton) {
  const fond = section.querySelector(".une__fond");
  fond.style.backgroundImage = "";
  section.classList.remove("une--photo");
  $("accueil-une-credit").hidden = true;
  if (!a.artist_mbid) return;
  invoke("accueil_fond", { mbid: a.artist_mbid })
    .then((img) => {
      if (!img || jeton !== accueil.jeton) return;
      fond.style.backgroundImage = `url("${img}")`;
      section.classList.add("une--photo");
      $("accueil-une-credit").hidden = false;
    })
    // Une panne réseau n'a rien à dire à l'écran : le dégradé suffit.
    .catch(() => {});
}

function rendreRangee(r) {
  const section = document.createElement("section");
  section.className = "rangee";

  const tete = document.createElement("header");
  tete.className = "rangee__tete";
  const titre = document.createElement("h2");
  titre.className = "rangee__titre";
  titre.textContent = r.titre;
  titre.title = r.raison;
  const autre = document.createElement("button");
  autre.type = "button";
  autre.className = "rangee__autre";
  autre.textContent = "↻";
  autre.title = "Un autre tirage pour cette rangée";
  autre.setAttribute("aria-label", `Un autre tirage : ${r.titre}`);
  autre.addEventListener("click", () => {
    accueil.decalages[r.id] = (accueil.decalages[r.id] ?? 0) + 1;
    demanderPage().catch((e) => remonter(e, "accueil"));
  });
  tete.append(titre, autre);

  const bande = document.createElement("div");
  bande.className = "rangee__bande";
  bande.tabIndex = 0;
  bande.setAttribute("aria-label", r.titre);
  bande.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    // Les flèches font défiler la rangée, pas avancer la lecture.
    e.preventDefault();
    e.stopPropagation();
    const pas = bande.clientWidth * 0.8;
    bande.scrollBy({ left: e.key === "ArrowLeft" ? -pas : pas, behavior: "smooth" });
  });
  const cases = r.albums.map((a) => carteAlbum(a, { ouvrir, note: a.note, differer }));
  bande.append(...cases);

  section.append(tete, bande);
  section._albums = r.albums;
  section._cases = cases;
  return section;
}

/// Le filtre du rail estompe ce qui n'y répond pas (Règle 2), et le compte du
/// fil le dit.
function appliquerFiltreAccueil() {
  for (const [el, a] of accueil.cases) el.classList.toggle("album--estompe", !estRetenu(a));
  if (navigation.mode === "ecoute" && vue.quoi === "accueil") {
    $("fil-compte").textContent = texteCompte("accueil", vue.lignes);
  }
}

export {
  appliquerFiltreAccueil, chargerAccueil, reafficherAccueil, vueAccueil,
};
