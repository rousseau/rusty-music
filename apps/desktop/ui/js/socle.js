// Interface du mode Écoute. Pas de framework ni de bundler : `CLAUDE.md`
// retient HTML/CSS/JS simple, la carte WebGL du module 2 n'impose rien ici.

const { invoke } = window.__TAURI__.core;
// Ouvre un lien dans le navigateur du système (tauri-plugin-opener) — la
// webview ne navigue pas vers l'extérieur, et c'est voulu.
const ouvrirLien = (url) =>
  window.__TAURI__.opener?.openUrl(url)?.catch((e) => remonter(e, "lien externe"));

// Une exception ici resterait dans la console de la vue web, invisible depuis
// le terminal : on la renvoie au journal du processus.
const remonter = (message, source) =>
  invoke("js_error", { message: String(message), source: source ?? null }).catch(() => {});

window.addEventListener("error", (e) =>
  remonter(e.error?.stack || e.message, `${e.filename}:${e.lineno}`),
);
window.addEventListener("unhandledrejection", (e) =>
  remonter(e.reason?.stack || e.reason, "promesse non traitée"),
);

/// Une ligne au journal du processus, sans rien montrer à l'écran.
const journal = (source, message, niveau = "info") =>
  invoke("journal_ui", { niveau, source, message: String(message) }).catch(() => {});

const $ = (id) => document.getElementById(id);

/// Affiche `texte` dans le petit slot de statut `idStatut` — la convention
/// déjà en place ailleurs dans l'appli (`#chemin-aide`, `#demix-etat`,
/// `#dock-aide`, tous en classe `.aide`/`.scan-etat`) — et journalise
/// `erreur` via `remonter`. `remonter` seul ne montre jamais rien à l'écran,
/// seulement au journal Rust : un appel resté sur `remonter` nu échoue en
/// silence du point de vue de l'utilisateur. Généralise le motif déjà écrit
/// une fois pour `appliquerUn` (dock des stems) plutôt que d'en inventer un
/// nouveau — pas de bandeau ni de toast, la sobriété du contrat d'interface
/// (`docs/interface-guidelines.md`) s'applique aussi aux erreurs.
function signalerErreur(idStatut, texte, erreur, source) {
  const el = $(idStatut);
  if (el) el.textContent = texte;
  remonter(erreur, source ?? idStatut);
}

const LIGNE = 44; // hauteur d'une ligne — doit rester égale à `.ligne { height }` dans style.css

/* ------------------------------------------------------------------ outils */

function duree(ms) {
  if (!ms || ms <= 0) return "—";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function horloge(ms) {
  const s = Math.max(0, Math.round((ms || 0) / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

// Les titres viennent des tags : ils peuvent contenir n'importe quoi.
function txt(v, defaut = "") {
  return v === null || v === undefined || v === "" ? defaut : String(v);
}

/* -------------------------------------------------------- cache pochettes */

// Sans cache, chaque affichage relit le fichier : 50 à 210 ms sur la carte SD.
//
// Borné en octets, pas en nombre d'entrées : une pochette pèse de 50 à 600 Ko
// en `data:` URI, compter en entrées laisserait le poids varier d'un facteur
// douze. Sans plafond, une longue session de navigation accumule des centaines
// de mégaoctets.
const POCHETTES_MAX = 64 * 1024 * 1024;
const pochettes = new Map(); // chemin → { promesse, poids }
let pochettesPoids = 0;

async function pochette(path) {
  const connue = pochettes.get(path);
  if (connue) {
    // Remise en fin de Map : l'ordre d'insertion fait l'ordre d'éviction.
    pochettes.delete(path);
    pochettes.set(path, connue);
    return connue.promesse;
  }

  const entree = { promesse: invoke("cover", { path }).catch(() => null), poids: 0 };
  pochettes.set(path, entree);

  entree.promesse.then((img) => {
    entree.poids = img ? img.length : 0;
    pochettesPoids += entree.poids;
    // Évince les plus anciennes jusqu'à repasser sous le plafond.
    for (const [cle, e] of pochettes) {
      if (pochettesPoids <= POCHETTES_MAX) break;
      if (cle === path) continue; // jamais celle qu'on vient de demander
      pochettes.delete(cle);
      pochettesPoids -= e.poids;
    }
  });
  return entree.promesse;
}

/// Oublie toutes les pochettes : une a changé de source (« Image du dossier »).
function viderPochettes() {
  pochettes.clear();
  pochettesPoids = 0;
}

/* ------------------------------------------------------------------- état */

const vue = {
  quoi: "albums", // artistes | albums | pistes | recherche
  lignes: [],
  titre: "Albums",
  retour: null, // état à restaurer en remontant
};

// Dernière vue de premier niveau posée (Artistes ou Albums, `retour === null`).
// Sert de cible de retour à la recherche, qui ne connaît pas d'où l'on vient,
// et de vue par défaut de `charger()` quand elle est appelée sans argument.
let sommet = { quoi: "albums", titre: "Albums", lignes: [] };

// Artistes et Albums s'affichent tous deux dans la grille virtualisée
// (`dessinerGrille`) — une tuile carrée par entrée, pochette pour un album,
// mosaïque des pochettes de ses albums pour un artiste. Les pistes et la
// recherche restent en liste.
const vueEnGrille = (quoi = vue.quoi) => quoi === "albums" || quoi === "artistes";

// Une liste de morceaux avec jauge de popularité : les pistes d'un album ou
// les résultats de recherche (mêmes lignes `TrackRow`, même chargement en lot).
const estListePistes = (quoi = vue.quoi) => quoi === "pistes" || quoi === "recherche";

let fileCourante = []; // pistes envoyées au lecteur, pour l'affichage
// D'où vient la file en cours, quand l'interface le sait — la demande faite à
// Lama, « Dans l'esprit de … » : `{ texte, premier, n }`. `origineDeLaFile` ne
// la croit que si la file n'a pas changé depuis (même longueur, même premier
// morceau) : aucune remise à zéro à répartir sur tous les chemins de lecture.
let origineFile = null;

// Un bouton ✦ à la fois (case d'album ou inspecteur) : les deux partagent le
// graphe des voisins et le panneau de composition, deux lancements concurrents
// se marcheraient dessus.
let alchimieEnCours = false;
// Vrai pendant qu'une playlist ✦ se compose dans la file d'attente : `dessinerFile`
// s'efface alors, pour ne pas écraser l'animation de remplissage avec un rendu
// ordinaire au premier battement de sondage de la lecture.
let fileCompositionActive = false;

// Les autres modules ne peuvent pas affecter une liaison importée.
function affecterAlchimieEnCours(v) {
  return (alchimieEnCours = v);
}
function affecterFileCompositionActive(v) {
  return (fileCompositionActive = v);
}
function affecterFileCourante(v) {
  return (fileCourante = v);
}
function affecterOrigineFile(v) {
  return (origineFile = v);
}
function affecterSommet(v) {
  return (sommet = v);
}

export {
  $, affecterAlchimieEnCours, affecterFileCompositionActive, affecterFileCourante,
  affecterOrigineFile, affecterSommet, alchimieEnCours, duree, estListePistes,
  fileCompositionActive, fileCourante, horloge, invoke, journal, LIGNE, origineFile, ouvrirLien,
  pochette, remonter, signalerErreur, sommet, txt, viderPochettes, vue, vueEnGrille,
};
