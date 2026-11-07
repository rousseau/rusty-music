import "./maplibre.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, enStems, invoke, navigation, plier, remonter, repondAuFiltre, vue, vueEnGrille,
} from "./socle.js";
import {
  cleCarteGrille, dessiner, dessinerGrille,
} from "./vues.js";

import {
  construireIndexAlpha, construireIndexAnnees, texteCompte,
} from "./ecouter.js";

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
  chargerFamillesParArtiste, decouverte, dessinerFamillesDecouvrir, filtreFamillesDecouvrir,
  rendreFilDecouvrir,
} from "./decouvrir.js";

/* ------------------------------------------- filtre par famille — mode Écoute
 *
 * Les albums dont la famille sonique dominante n'est pas cochée s'estompent
 * dans la grille, à leur place (Règle 2 — comme Découvrir et le filtre du
 * rail) ; ensemble vide = aucun filtre. Même légende que le
 * mode Explorer (`rendreFamilles`), mais multi-sélection et sans lien avec la
 * carte. Les familles viennent du même clustering ; le filtre n'a de sens
 * qu'une fois la carte calculée.
 */

const filtreFamilles = new Set();

function cleAlbum(a) {
  return `${a.name}\n${a.artist ?? ""}`;
}

async function chargerFamillesParAlbum() {
  try {
    const paires = await invoke("album_families");
    ecoute.famillesParAlbum = new Map(
      paires.map(([nom, artiste, c]) => [`${nom}\n${artiste ?? ""}`, c]),
    );
  } catch (e) {
    remonter(e, "familles des albums");
    ecoute.famillesParAlbum = new Map();
  }
}

/// Vrai si l'album `a` passe le filtre par famille. Sans filtre, ou sans données
/// de famille, tout passe.
function passeFamille(a) {
  if (filtreFamilles.size === 0 || !ecoute.famillesParAlbum) return true;
  return filtreFamilles.has(ecoute.famillesParAlbum.get(cleAlbum(a)));
}

/// Vrai si `item` reste plein dans la vue affichée : il répond au filtre du rail
/// et, dans la grille d'albums, au filtre par famille. Sinon il s'estompe.
function estRetenu(item) {
  return repondAuFiltre(item) && (vue.quoi !== "albums" || passeFamille(item));
}

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
const triCourant = () => (vue.quoi === "artistes" ? ecoute.triArtistes : ecoute.triAlbums);

/// Le repère alphabétique n'a de sens que si l'ordre affiché est celui des noms.
const indexAlphaUtile = (quoi = vue.quoi) =>
  (quoi === "artistes" && ecoute.triArtistes === "alpha") ||
  (quoi === "albums" && ecoute.triAlbums === "alpha");

/// Le repère des années n'a de sens que si l'ordre affiché est celui des années.
const indexAnneesUtile = (quoi = vue.quoi) => quoi === "albums" && ecoute.triAlbums === "annee";

function rebrasserAlea(lignes) {
  const table = grainesAlea[vue.quoi === "artistes" ? "artistes" : "albums"];
  table.clear();
  for (const a of lignes) table.set(cleCarteGrille(a), Math.random());
}

/// La barre d'ordre sert aux grilles d'albums et d'artistes du mode Écoute ;
/// « Année » n'a de sens que pour les albums.
function majBarreTri(horsEcoute = navigation.mode !== "ecoute") {
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
    if (ecoute.triArtistes === "alpha") return sensDe("alpha") === 1 ? lignes : [...lignes].reverse();
    if (ecoute.triArtistes !== "alea") return lignes;
    const g = grainesAlea.artistes;
    return [...lignes].sort((a, b) => (g.get(cleCarteGrille(a)) ?? 0) - (g.get(cleCarteGrille(b)) ?? 0));
  }
  if (ecoute.triAlbums === "alpha") return sensDe("alpha") === 1 ? lignes : [...lignes].reverse();
  if (ecoute.triAlbums === "annee") return [...lignes].sort(parValeur((a) => a.year, sensDe("annee")));
  if (ecoute.triAlbums === "ajoutes") {
    return [...lignes].sort(parValeur((a) => a.ajoute_le, sensDe("ajoutes")));
  }
  if (ecoute.triAlbums === "alea") {
    const g = grainesAlea.albums;
    return [...lignes].sort((a, b) => (g.get(cleCarteGrille(a)) ?? 0) - (g.get(cleCarteGrille(b)) ?? 0));
  }
  return lignes;
}

/// Les lignes de la vue courante — les grilles d'albums et d'artistes peuvent
/// être retriées ; tout le reste (pistes d'un album, playlists) passe tel quel.
/// Rien n'est retiré : un filtre estompe (`estRetenu`).
function lignesCourantes() {
  if (vue.quoi === "albums") return trierAlbums(vue.lignes);
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
    navigation.mode === "ecoute" &&
    vue.quoi === "albums" &&
    ecoute.famillesParAlbum &&
    ecoute.famillesParAlbum.size > 0;
  $("bloc-familles-ecoute").hidden = !utile;
}

/// Recalcule la grille après un changement de filtre : nouveau compte, repère
/// alphabétique reconstruit sur la liste filtrée, bande de rangées redessinée.
function rafraichirGrille() {
  majBlocFamillesEcoute();
  if ($("grille").hidden) return;
  $("fil-compte").textContent = texteCompte(vue.quoi, lignesCourantes());
  construireIndexAlpha();
  if (indexAnneesUtile()) construireIndexAnnees();
  ecoute.grilleDernierRang = -1;
  dessinerGrille();
}

/// Le filtre du rail (`#q`) côté Écouter, Pratiquer et Créer : pose le texte plié et, si
/// la grille ou la liste est à l'écran, la redessine — ce qui n'y répond pas
/// s'estompe (`repondAuFiltre`), le compte devient « n sur N ».
function filtrerEcoute(q) {
  ecoute.filtre = plier(q.trim());
  if (navigation.mode !== "ecoute" && !enStems()) return;
  if (vueEnGrille()) {
    rafraichirGrille();
    return;
  }
  if ($("liste").hidden) return;
  dessiner();
  if (navigation.mode === "ecoute") $("fil-compte").textContent = texteCompte(vue.quoi, vue.lignes);
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
  ecoute.famillesParAlbum = null;
  decouverte.famillesParArtiste = null;
  filtreFamilles.clear();
  filtreFamillesDecouvrir.clear();
  await chargerFamillesParAlbum();
  if (navigation.mode === "ecoute") {
    await dessinerFamillesEcoute();
    rafraichirGrille();
  }
  if (navigation.mode === "decouvrir") {
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


export {
  chargerFamillesParAlbum, cleAlbum, dessinerFamillesEcoute, estRetenu, familleARecalculee,
  filtrerEcoute, grilleDeTri, indexAlphaUtile, indexAnneesUtile, lignesCourantes, majBarreTri,
  majBlocFamillesEcoute, majSegmentsCouleur, rafraichirGrille, rebrasserAlea, SENS_ORIGINE, sensDe,
  sensTri, triCourant,
};
