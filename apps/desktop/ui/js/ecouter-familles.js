import "./maplibre.js"; // l'ordre d'exécution : voir main.js
import {
  $, invoke, remonter, vue, vueEnGrille,
} from "./socle.js";
import {
  cleCarteGrille, dessinerGrille,
} from "./vues.js";
import {
  affecterGrilleDernierRang, grilleDernierRang,
} from "./lama.js";
import {
  construireIndexAlpha, construireIndexAnnees,
} from "./ecouter.js";
import {
  modeCourant,
} from "./lecteur.js";
import {
  carte,
} from "./carte.js";
import {
  chargerFamilles, dessinerCarte, majCouleurGL, rendreFamilles,
} from "./maplibre.js";
import {
  majLegendeContinue,
} from "./fond-de-plan.js";
import {
  affecterFamillesParArtiste, chargerFamillesParArtiste, dessinerFamillesDecouvrir,
  famillesParArtiste, filtreFamillesDecouvrir, rendreFilDecouvrir,
} from "./decouvrir.js";

/* ------------------------------------------- filtre par famille — mode Écoute
 *
 * La grille de pochettes ne montre que les albums dont la famille sonique
 * dominante est cochée. Ensemble vide = tout est montré. Même légende que le
 * mode Explorer (`rendreFamilles`), mais multi-sélection et sans lien avec la
 * carte. Les familles viennent du même clustering ; le filtre n'a de sens
 * qu'une fois la carte calculée.
 */

const filtreFamilles = new Set();
let famillesParAlbum = null; // Map "nom\nartiste" → cluster dominant

function cleAlbum(a) {
  return `${a.name}\n${a.artist ?? ""}`;
}

async function chargerFamillesParAlbum() {
  try {
    const paires = await invoke("album_families");
    famillesParAlbum = new Map(
      paires.map(([nom, artiste, c]) => [`${nom}\n${artiste ?? ""}`, c]),
    );
  } catch (e) {
    remonter(e, "familles des albums");
    famillesParAlbum = new Map();
  }
}

/// Les albums réellement affichés dans la grille : `vue.lignes` filtré par les
/// familles cochées. Sans filtre, ou sans données de famille, on rend tout.
function albumsAffiches() {
  if (filtreFamilles.size === 0 || !famillesParAlbum) return vue.lignes;
  return vue.lignes.filter((a) => filtreFamilles.has(famillesParAlbum.get(cleAlbum(a))));
}

/// Ordre de la grille d'albums. `alpha` est l'ordre rendu par le moteur
/// (`ORDER BY … COLLATE NOCASE`) — on le laisse tel quel. `annee`, `ajoutes` et
/// `alea` retrient une copie côté interface.
let triAlbums = "alpha";
/// Ordre de la grille d'artistes : `alpha` (celui du moteur) ou `alea` — pas
/// d'année, un artiste n'en a pas. Mémorisé à part de `triAlbums`.
let triArtistes = "alpha";
/// Sens de chaque ordre : `1` = croissant (A→Z, plus ancien d'abord), `-1` =
/// décroissant. Un second clic sur l'ordre actif l'inverse ; y revenir depuis un
/// autre le remet au sens d'origine — A–Z croissant, Année et Ajoutés
/// décroissants (le plus récent d'abord). Un état par grille : inverser les
/// albums ne touche pas l'ordre des artistes.
const SENS_ORIGINE = { alpha: 1, annee: -1, ajoutes: -1 };
const sensTri = { albums: { ...SENS_ORIGINE }, artistes: { alpha: 1 } };
const grilleDeTri = (quoi = vue.quoi) => (quoi === "artistes" ? "artistes" : "albums");
const sensDe = (tri, quoi = vue.quoi) => sensTri[grilleDeTri(quoi)][tri] ?? 1;

/// Le libellé d'un bouton d'ordre, qui dit le sens quand il est actif : « Z–A »,
/// « Année ↑ »… Inactif, il garde son nom simple.
function libelleTri(tri, actif) {
  const base = { alpha: "A–Z", annee: "Année", alea: "Aléatoire", ajoutes: "Ajoutés" }[tri];
  if (!actif || tri === "alea") return base;
  const sens = sensDe(tri);
  if (tri === "alpha") return sens === 1 ? "A–Z" : "Z–A";
  return `${base} ${sens === 1 ? "↑" : "↓"}`;
}
// clé de carte (voir `cleCarteGrille`) → tirage aléatoire, régénéré à chaque
// clic sur « Aléatoire ». Passer par la clé (et non l'objet) garde l'ordre
// stable quand le filtre familles réduit la liste affichée.
// Une table par grille : rebrasser les albums ne doit pas changer l'ordre
// aléatoire des artistes quand on y revient.
const grainesAlea = { artistes: new Map(), albums: new Map() };

/// L'ordre en vigueur pour la vue de premier niveau affichée.
const triCourant = () => (vue.quoi === "artistes" ? triArtistes : triAlbums);

/// Le repère alphabétique n'a de sens que si l'ordre affiché est celui des noms.
const indexAlphaUtile = (quoi = vue.quoi) =>
  (quoi === "artistes" && triArtistes === "alpha") || (quoi === "albums" && triAlbums === "alpha");

/// Le repère des années n'a de sens que si l'ordre affiché est celui des années.
const indexAnneesUtile = (quoi = vue.quoi) => quoi === "albums" && triAlbums === "annee";

function rebrasserAlea(lignes) {
  const table = grainesAlea[vue.quoi === "artistes" ? "artistes" : "albums"];
  table.clear();
  for (const a of lignes) table.set(cleCarteGrille(a), Math.random());
}

/// La barre d'ordre sert aux grilles d'albums et d'artistes du mode Écoute ;
/// « Année » n'a de sens que pour les albums.
function majBarreTri(horsEcoute = modeCourant !== "ecoute") {
  $("tri-albums").hidden = horsEcoute || !vueEnGrille();
  // « Année » et « Ajoutés » n'ont de sens que pour les albums.
  document.querySelector('[data-tri="annee"]').hidden = vue.quoi === "artistes";
  document.querySelector('[data-tri="ajoutes"]').hidden = vue.quoi === "artistes";
  document.querySelectorAll("[data-tri]").forEach((b) => {
    const actif = b.dataset.tri === triCourant();
    b.classList.toggle("tri__opt--actif", actif);
    b.textContent = libelleTri(b.dataset.tri, actif);
  });
}

/// Comparateur sur une valeur numérique (année, date d'ajout) dans le sens
/// `sens`. **Ce qui n'a pas de valeur vient toujours en dernier**, dans les deux
/// sens : inverser l'ordre ne doit pas faire remonter les albums sans année en
/// tête. À valeur égale, ordre alphabétique croissant.
function parValeur(cle, sens) {
  const nom = (x) => x.name || "";
  return (a, b) => {
    const va = cle(a);
    const vb = cle(b);
    if (va == null && vb == null) return nom(a).localeCompare(nom(b), "fr", { sensitivity: "base" });
    if (va == null) return 1;
    if (vb == null) return -1;
    if (va !== vb) return (va - vb) * sens;
    return nom(a).localeCompare(nom(b), "fr", { sensitivity: "base" });
  };
}

function trierAlbums(lignes) {
  if (vue.quoi === "artistes") {
    if (triArtistes === "alpha") return sensDe("alpha") === 1 ? lignes : [...lignes].reverse();
    if (triArtistes !== "alea") return lignes;
    const g = grainesAlea.artistes;
    return [...lignes].sort((a, b) => (g.get(cleCarteGrille(a)) ?? 0) - (g.get(cleCarteGrille(b)) ?? 0));
  }
  if (triAlbums === "alpha") return sensDe("alpha") === 1 ? lignes : [...lignes].reverse();
  if (triAlbums === "annee") return [...lignes].sort(parValeur((a) => a.year, sensDe("annee")));
  if (triAlbums === "ajoutes") return [...lignes].sort(parValeur((a) => a.ajoute_le, sensDe("ajoutes")));
  if (triAlbums === "alea") {
    const g = grainesAlea.albums;
    return [...lignes].sort((a, b) => (g.get(cleCarteGrille(a)) ?? 0) - (g.get(cleCarteGrille(b)) ?? 0));
  }
  return lignes;
}

/// Les lignes de la vue courante — la grille d'albums peut être filtrée et
/// retriée, celle d'artistes retriée ; tout le reste (pistes d'un album,
/// recherche) passe tel quel.
function lignesCourantes() {
  if (vue.quoi === "albums") return trierAlbums(albumsAffiches());
  if (vue.quoi === "artistes") return trierAlbums(vue.lignes);
  return vue.lignes;
}

async function dessinerFamillesEcoute() {
  await chargerFamilles();
  rendreFamilles(
    $("familles-ecoute"),
    (c) => filtreFamilles.has(c),
    (c) => {
      if (filtreFamilles.has(c)) filtreFamilles.delete(c);
      else filtreFamilles.add(c);
      dessinerFamillesEcoute();
      rafraichirGrille();
    },
  );
  $("familles-ecoute-tout").hidden = filtreFamilles.size === 0;
}

/// Affiche le bloc « Familles » de l'Écoute quand il a un sens : mode Écoute,
/// grille d'albums à l'écran, et carte déjà calculée (sinon aucune famille à
/// proposer).
function majBlocFamillesEcoute() {
  const utile =
    modeCourant === "ecoute" &&
    vue.quoi === "albums" &&
    famillesParAlbum &&
    famillesParAlbum.size > 0;
  $("bloc-familles-ecoute").hidden = !utile;
}

/// Recalcule la grille après un changement de filtre : nouveau compte, repère
/// alphabétique reconstruit sur la liste filtrée, bande de rangées redessinée.
function rafraichirGrille() {
  majBlocFamillesEcoute();
  if ($("grille").hidden) return;
  $("fil-compte").textContent = `${lignesCourantes().length} ${vue.quoi === "artistes" ? "artistes" : "albums"}`;
  construireIndexAlpha();
  if (indexAnneesUtile()) construireIndexAnnees();
  affecterGrilleDernierRang(-1);
  dessinerGrille();
}

$("familles-ecoute-tout").addEventListener("click", () => {
  filtreFamilles.clear();
  dessinerFamillesEcoute();
  rafraichirGrille();
});

/// À rappeler quand le clustering a changé (recalcul de la carte, genres
/// aspirés) : les numéros de famille et l'appartenance des albums ont bougé,
/// le filtre courant ne veut plus rien dire.
async function familleARecalculee() {
  carte.familles = null;
  carte.familleVotes = null;
  famillesParAlbum = null;
  affecterFamillesParArtiste(null);
  filtreFamilles.clear();
  filtreFamillesDecouvrir.clear();
  await chargerFamillesParAlbum();
  if (modeCourant === "ecoute") {
    await dessinerFamillesEcoute();
    rafraichirGrille();
  }
  if (modeCourant === "decouvrir") {
    await chargerFamillesParArtiste();
    await dessinerFamillesDecouvrir();
    rendreFilDecouvrir();
  }
}

/// Les trois colorations continues (année, tempo, énergie) sont disponibles sur
/// le plan de ville réel comme sur le nuage : le bâtiment habité porte les
/// attributs de son occupant (`couleurContinueBatiment`). Gardée comme point
/// d'entrée (appelée à chaque bascule d'affichage) pour réactiver les boutons
/// qu'une ancienne session aurait laissés désactivés.
function majSegmentsCouleur() {
  document.querySelectorAll("[data-couleur]").forEach((b) => {
    b.disabled = false;
    b.title = "";
  });
}

document.querySelectorAll("[data-couleur]").forEach((b) =>
  b.addEventListener("click", () => {
    carte.couleur = b.dataset.couleur;
    majCouleurGL();
    document
      .querySelectorAll("[data-couleur]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
    majLegendeContinue();
    // Les familles n'ont de sens qu'en coloration par famille.
    $("bloc-familles").hidden = carte.couleur !== "famille";
    // Le pavage par territoires (Rust) ne dépend pas de « Colorer par » — il
    // n'y a qu'une seule nappe continue à refaire ici.
    dessinerCarte();
  }),
);

// Les autres modules ne peuvent pas affecter une liaison importée.
function affecterFamillesParAlbum(v) {
  return (famillesParAlbum = v);
}
function affecterTriAlbums(v) {
  return (triAlbums = v);
}
function affecterTriArtistes(v) {
  return (triArtistes = v);
}

export {
  affecterFamillesParAlbum, affecterTriAlbums, affecterTriArtistes, chargerFamillesParAlbum,
  cleAlbum, dessinerFamillesEcoute, familleARecalculee, famillesParAlbum, grilleDeTri,
  indexAlphaUtile, indexAnneesUtile, lignesCourantes, majBarreTri, majBlocFamillesEcoute,
  majSegmentsCouleur, rafraichirGrille, rebrasserAlea, SENS_ORIGINE, sensDe, sensTri, triAlbums,
  triArtistes, triCourant,
};
