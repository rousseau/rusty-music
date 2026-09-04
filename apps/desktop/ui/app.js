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

const $ = (id) => document.getElementById(id);
const LIGNE = 44; // hauteur d'une ligne, en accord avec la CSS

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

let fileCourante = []; // pistes envoyées au lecteur, pour l'affichage

// Un bouton ✦ à la fois (case d'album ou inspecteur) : les deux partagent le
// graphe des voisins et le panneau de composition, deux lancements concurrents
// se marcheraient dessus.
let alchimieEnCours = false;
// Vrai pendant qu'une playlist ✦ se compose dans la file d'attente : `dessinerFile`
// s'efface alors, pour ne pas écraser l'animation de remplissage avec un rendu
// ordinaire au premier battement de sondage de la lecture.
let fileCompositionActive = false;

/* ------------------------------------------------- liste virtualisée */

const liste = $("liste");
const socle = document.createElement("div");
socle.className = "liste__socle";
const fenetre = document.createElement("div");
fenetre.className = "liste__fenetre";
socle.appendChild(fenetre);
liste.appendChild(socle);

function dessiner() {
  const n = vue.lignes.length;
  socle.style.height = `${n * LIGNE}px`;

  // On ne pose dans le DOM que ce qui est visible, plus une marge : 3 543
  // artistes en une fois figeraient la fenêtre.
  const haut = Math.max(0, Math.floor(liste.scrollTop / LIGNE) - 6);
  const bas = Math.min(n, Math.ceil((liste.scrollTop + liste.clientHeight) / LIGNE) + 6);

  fenetre.style.transform = `translateY(${haut * LIGNE}px)`;
  fenetre.replaceChildren();

  for (let i = haut; i < bas; i++) {
    fenetre.appendChild(ligne(vue.lignes[i], i));
  }
  majIndexActif();
}

function ligne(item, index) {
  const el = document.createElement("div");
  el.className = "ligne";
  el.dataset.index = index;

  el.innerHTML = `<span class="ligne__no"></span>
                  <span class="ligne__nom"></span>
                  <span class="ligne__sec"></span>
                  <span class="ligne__cpt"></span>`;
  el.children[0].textContent = item.track_no ?? "";
  el.children[1].textContent = txt(item.title, "(sans titre)");
  el.children[2].textContent = txt(item.artist);
  el.children[3].textContent = duree(item.duration_ms);
  if (item.path === enLecture) el.classList.add("ligne--joue");

  // Jauge de popularité — pour une liste de morceaux (pistes d'un album,
  // résultats de recherche), pas pour une liste d'artistes.
  if (vue.quoi === "pistes" && Number.isFinite(item.id)) {
    const cell = document.createElement("span");
    cell.className = "ligne__pop";
    cell.appendChild(jaugePop(popParPiste.get(item.id)));
    el.insertBefore(cell, el.children[3]); // juste avant la durée
  }

  el.addEventListener("click", () => activer(item));
  return el;
}

liste.addEventListener("scroll", dessiner, { passive: true });
window.addEventListener("resize", dessiner);

/* --------------------------------------------------- jauge de popularité
 *
 * La popularité générale d'un morceau (ListenBrainz + Deezer, remplie par la
 * passe du même nom) — un rang dans la bibliothèque, pas un compteur d'écoutes.
 * Chargée par lot pour ce qui est visible (`popularites`), gardée en cache, et
 * rendue en cinq segments dans la file d'attente et les listes de pistes.
 */

/// `id → {relative, echelon}` ; `null` = demandé mais le morceau n'en a pas
/// (jauge grisée) ; absent = pas encore demandé. Vidé après chaque passe.
const popParPiste = new Map();

/// Charge la popularité des `ids` encore inconnus en une requête, puis
/// `apres()` pour repeindre — seulement s'il y avait quelque chose à charger,
/// sinon le repaint rappellerait cette fonction sans fin.
async function chargerPopularites(ids, apres) {
  const manquants = [...new Set(ids)].filter(
    (id) => Number.isFinite(id) && !popParPiste.has(id),
  );
  if (manquants.length === 0) return;
  let lignes = [];
  try {
    lignes = await invoke("popularites", { ids: manquants });
  } catch (e) {
    remonter(e, "popularité");
  }
  const vus = new Set();
  for (const [id, relative, echelon] of lignes) {
    popParPiste.set(id, { relative, echelon });
    vus.add(id);
  }
  for (const id of manquants) if (!vus.has(id)) popParPiste.set(id, null);
  if (apres) apres();
}

const MOTS_POP = ["très faible", "faible", "moyenne", "élevée", "très élevée"];

/// Une jauge à cinq segments. `pop` : `{relative, echelon}` ∈ [0,1], ou une
/// valeur fausse = inconnu (contour seul, jamais une valeur inventée — même
/// règle que les descripteurs de l'inspecteur).
function jaugePop(pop) {
  const el = document.createElement("span");
  el.className = "jauge-pop";
  // Au moins un segment dès qu'on a une mesure : « connu et peu populaire »
  // ne doit pas se confondre avec « inconnu ».
  const remplis = pop ? Math.max(1, Math.round(pop.relative * 5)) : 0;
  if (!pop) el.classList.add("jauge-pop--inconnu");
  for (let i = 0; i < 5; i++) {
    const seg = document.createElement("i");
    if (i < remplis) seg.className = "jauge-pop__on";
    el.appendChild(seg);
  }
  el.title = pop
    ? `popularité : ${MOTS_POP[Math.min(4, Math.floor(pop.relative * 5))]} · ${
        pop.echelon === "release-group" ? "mesurée sur l'album" : "mesurée sur le morceau"
      }`
    : "popularité inconnue — lancez la passe de popularité";
  return el;
}

/// Vide le cache et recharge ce qui est à l'écran — après une passe de
/// popularité, les rangs de toute la bibliothèque ont bougé. `dessinerFile`
/// et `chargerPopularites` se chargent eux-mêmes du repaint quand les
/// nouvelles valeurs arrivent.
function popARecalculee() {
  popParPiste.clear();
  if (!$("file").hidden && !fileCompositionActive) dessinerFile();
  if (!$("liste").hidden && vue.quoi === "pistes") {
    dessiner();
    chargerPopularites(
      vue.lignes.map((l) => l.id),
      () => {
        if (vue.quoi === "pistes") dessiner();
      },
    );
  }
}

/// La ligne d'alerte du mode Bibliothèque : n'apparaît que si de la popularité
/// a été récupérée **et** qu'une partie date de plus de 90 jours — une
/// notoriété bouge lentement, on ne relance pas une passe de deux heures pour
/// rien. Le bouton « Rafraîchir » coche la case et relance la passe.
async function chargerPopulariteFraicheur() {
  const ligne = $("popularite-fraicheur");
  let couverts;
  let plusAncienne;
  let perimes;
  try {
    [couverts, plusAncienne, perimes] = await invoke("popularite_fraicheur");
  } catch (e) {
    remonter(e, "popularité");
    ligne.hidden = true;
    return;
  }
  if (couverts === 0 || perimes === 0) {
    ligne.hidden = true;
    return;
  }
  $("popularite-fraicheur-txt").textContent =
    `Popularité : ${perimes.toLocaleString("fr-FR")} entité${perimes > 1 ? "s" : ""} ` +
    `de plus de 90 jours` +
    (plusAncienne ? ` (la plus ancienne remonte à ${depuisTexte(plusAncienne)}).` : ".") +
    " ";
  ligne.hidden = false;
}

$("popularite-rafraichir").addEventListener("click", async () => {
  if ($("lancer-scan").disabled) return; // une passe tourne déjà
  $("pop-rafraichir").checked = true;
  verrouillerActualisation(true);
  try {
    await passePopularite(contactMb(), "", true);
    $("scan-etat").textContent = "Popularité rafraîchie.";
  } catch (e) {
    remonter(e, "popularité");
    $("scan-etat").textContent = String(e);
  } finally {
    $("scan-jauge").hidden = true;
    verrouillerActualisation(false);
  }
});

/* --------------------------------------------------- grille d'albums */

// Grille de pochettes, virtualisée par rangée sur le même principe que la
// liste : 1 986 albums, chacun avec une image à charger paresseusement, ne
// tiennent pas dans le DOM à la fois sans que la grille se traîne.
// Chaque constante doit valoir exactement ce que le CSS rend : la grille est
// virtualisée, seule une bande de rangées existe dans le DOM, translatée à la
// place qu'auraient occupée les rangées qui la précèdent. Un écart avec le
// CSS ferait dériver cette place au fil du défilement.
const ALBUM_LARG = 140; // largeur d'une case = hauteur de la pochette (carrée)
const ALBUM_TXT = 40; // `.album__pochette` margin-bottom(8) + nom(16) + sec margin-top(2) + sec(14)
const ALBUM_ECART = 14; // `gap` de `.grille__fenetre`
const ALBUM_HAUT = ALBUM_LARG + ALBUM_TXT + ALBUM_ECART;
const GRILLE_PAD = 26; // `padding` horizontal de `.grille`

const grille = $("grille");
const grilleSocle = document.createElement("div");
grilleSocle.className = "grille__socle";
const grilleFenetre = document.createElement("div");
grilleFenetre.className = "grille__fenetre";
grilleSocle.appendChild(grilleFenetre);
grille.appendChild(grilleSocle);

function colonnesGrille() {
  const larg = grille.clientWidth - 2 * GRILLE_PAD;
  return Math.max(1, Math.floor((larg + ALBUM_ECART) / (ALBUM_LARG + ALBUM_ECART)));
}

function dessinerGrille() {
  const lignes = lignesCourantes();
  const n = lignes.length;
  const cols = colonnesGrille();
  const rangs = Math.max(1, Math.ceil(n / cols));
  grilleSocle.style.height = `${rangs * ALBUM_HAUT}px`;

  const rangHaut = Math.max(0, Math.floor(grille.scrollTop / ALBUM_HAUT) - 2);
  const rangBas = Math.min(rangs, Math.ceil((grille.scrollTop + grille.clientHeight) / ALBUM_HAUT) + 2);

  grilleFenetre.style.transform = `translateY(${rangHaut * ALBUM_HAUT}px)`;
  grilleFenetre.style.gridTemplateColumns = `repeat(${cols}, ${ALBUM_LARG}px)`;
  grilleFenetre.replaceChildren();

  const carte = vue.quoi === "artistes" ? carteArtiste : carteAlbum;
  for (let i = rangHaut * cols; i < Math.min(n, rangBas * cols); i++) {
    grilleFenetre.appendChild(carte(lignes[i]));
  }
  majIndexActif();
}

function carteAlbum(item) {
  const el = document.createElement("div");
  el.className = "album";
  el.innerHTML = `<div class="album__pochette">
                    <button class="album__lecture" aria-label="Lire l'album" title="Lire l'album">▶</button>
                    <button class="album__alchimie" aria-label="Playlist dans l'esprit de l'album" title="Playlist dans l'esprit de l'album">✦</button>
                  </div>
                  <div class="album__nom"></div>
                  <div class="album__sec"></div>`;
  const image = el.children[0];
  el.children[1].textContent = item.name;
  el.children[2].innerHTML = `<span class="album__artiste"></span> · <span></span>`;
  const artiste = el.children[2].children[0];
  artiste.textContent = txt(item.artist, "(sans artiste)");
  el.children[2].children[1].textContent = item.year ?? "————";
  el.addEventListener("click", () => activer(item));

  // Le nom de l'artiste ouvre tous ses albums ; `stopPropagation` évite que
  // le clic remonte à la case et ouvre la liste de pistes de cet album.
  // `AlbumRow` n'a pas de MBID : le nom seul, avec le repli de
  // `Library::albums_of_artist`.
  if (item.artist) {
    artiste.classList.add("album__artiste--lien");
    artiste.addEventListener("click", (e) => {
      e.stopPropagation();
      ouvrirAlbumsArtiste(item.artist, null).catch((err) => remonter(err, "ouvrirAlbumsArtiste"));
    });
  }

  // Les deux boutons agissent directement depuis la case ; le reste de la
  // case ouvre sa liste de pistes. `stopPropagation` sépare les gestes, sans
  // quoi le clic remonterait et déclencherait aussi l'ouverture.
  image.querySelector(".album__lecture").addEventListener("click", (e) => {
    e.stopPropagation();
    lireAlbum(item).catch((err) => remonter(err, "lireAlbum"));
  });

  const boutonAlchimie = image.querySelector(".album__alchimie");
  boutonAlchimie.addEventListener("click", (e) => {
    e.stopPropagation();
    genererAlchimie(item, boutonAlchimie);
  });

  // Chargement paresseux, cache partagé avec l'inspecteur et le transport :
  // la pochette n'est demandée qu'à l'affichage de la case, et ne sera pas
  // relue si elle l'a déjà été ailleurs.
  pochette(item.path).then((img) => {
    if (!img) return;
    image.style.backgroundImage = `url("${img}")`;
    image.classList.add("album__pochette--pleine");
  });

  return el;
}

/* ------------------------------------------------- tuile d'artiste */

// artiste (mbid|nom) → Promise<string[]> de chemins d'albums avec pochette. La
// grille est virtualisée et reconstruit ses tuiles à chaque cran de
// défilement : sans ce cache, revenir sur une rangée relancerait la commande
// et la lecture des tags de chaque album. Les images, elles, passent par
// `pochette()` — même cache borné que la grille d'albums.
const mosaiques = new Map();

function cheminsArtiste(item) {
  const cle = `${item.mbid ?? ""}|${item.name}`;
  let p = mosaiques.get(cle);
  if (!p) {
    p = invoke("artist_covers", { name: item.name, mbid: item.mbid ?? null, max: 4 }).catch(
      (err) => {
        remonter(err, "artist_covers");
        return [];
      },
    );
    mosaiques.set(cle, p);
  }
  return p;
}

// Découpes de puzzle injectées dans un <svg><defs> au chargement : une pièce
// par pochette, aux bords à tenons et mortaises qui s'emboîtent. Chaque frontière
// interne est tracée par ses deux pièces avec les mêmes extrémités et le même
// sens de tenon — le cran étant symétrique, les deux tracés rendent la même
// courbe, donc ni jour ni recouvrement.
(function injecterDecoupesPuzzle() {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("width", "0");
  svg.setAttribute("height", "0");
  svg.setAttribute("aria-hidden", "true");
  svg.style.cssText = "position:absolute;width:0;height:0;overflow:hidden";
  const defs = document.createElementNS(NS, "defs");
  svg.appendChild(defs);

  // Un cran de puzzle le long du segment (ax,ay)→(bx,by), forcément axis-aligné,
  // bombé au milieu dans la direction (tx,ty). Coordonnées 0..1 de la boîte
  // englobante (`clipPathUnits="objectBoundingBox"`), donc indépendantes de la
  // taille rendue. Le profil (col resserré, bulbe, col) est symétrique : le
  // tracer de b vers a donne la même courbe.
  const cran = (ax, ay, bx, by, tx, ty) => {
    const len = Math.abs(bx - ax) + Math.abs(by - ay);
    const ex = (bx - ax) / len;
    const ey = (by - ay) / len;
    const perp = Math.min(len, 0.5);
    const P = (s, q) =>
      `${(ax + ex * len * s + tx * perp * q).toFixed(4)} ${(
        ay +
        ey * len * s +
        ty * perp * q
      ).toFixed(4)}`;
    return [
      `L ${P(0.36, 0)}`,
      `C ${P(0.34, 0.06)} ${P(0.3, 0.08)} ${P(0.3, 0.19)}`,
      `C ${P(0.3, 0.3)} ${P(0.4, 0.34)} ${P(0.5, 0.34)}`,
      `C ${P(0.6, 0.34)} ${P(0.7, 0.3)} ${P(0.7, 0.19)}`,
      `C ${P(0.7, 0.08)} ${P(0.66, 0.06)} ${P(0.64, 0)}`,
      `L ${P(1, 0)}`,
    ].join(" ");
  };

  const clip = (id, d) => {
    const cp = document.createElementNS(NS, "clipPath");
    cp.id = id;
    cp.setAttribute("clipPathUnits", "objectBoundingBox");
    const path = document.createElementNS(NS, "path");
    path.setAttribute("d", d);
    cp.appendChild(path);
    defs.appendChild(cp);
  };

  clip("puz-1-0", "M0 0 H1 V1 H0 Z");
  // 2 pièces : refend vertical, tenon vers la droite.
  clip("puz-2-0", `M0 0 L0.5 0 ${cran(0.5, 0, 0.5, 1, 1, 0)} L0 1 Z`);
  clip("puz-2-1", `M0.5 0 L1 0 L1 1 L0.5 1 ${cran(0.5, 1, 0.5, 0, 1, 0)} Z`);
  // 3 pièces : bande gauche pleine hauteur, colonne droite coupée en deux.
  clip(
    "puz-3-0",
    `M0 0 L0.5 0 ${cran(0.5, 0, 0.5, 0.5, -1, 0)} ${cran(0.5, 0.5, 0.5, 1, -1, 0)} L0 1 Z`,
  );
  clip(
    "puz-3-1",
    `M0.5 0 L1 0 L1 0.5 ${cran(1, 0.5, 0.5, 0.5, 0, -1)} ${cran(0.5, 0.5, 0.5, 0, -1, 0)} Z`,
  );
  clip(
    "puz-3-2",
    `M0.5 0.5 ${cran(0.5, 0.5, 1, 0.5, 0, -1)} L1 1 L0.5 1 ${cran(0.5, 1, 0.5, 0.5, -1, 0)} Z`,
  );
  // 4 pièces : refends en croix, un tenon par demi-refend.
  clip(
    "puz-4-0",
    `M0 0 L0.5 0 ${cran(0.5, 0, 0.5, 0.5, 1, 0)} ${cran(0.5, 0.5, 0, 0.5, 0, 1)} L0 0 Z`,
  );
  clip(
    "puz-4-1",
    `M0.5 0 L1 0 L1 0.5 ${cran(1, 0.5, 0.5, 0.5, 0, -1)} ${cran(0.5, 0.5, 0.5, 0, 1, 0)} Z`,
  );
  clip(
    "puz-4-2",
    `M0 0.5 ${cran(0, 0.5, 0.5, 0.5, 0, 1)} ${cran(0.5, 0.5, 0.5, 1, -1, 0)} L0 1 Z`,
  );
  clip(
    "puz-4-3",
    `M0.5 0.5 ${cran(0.5, 0.5, 0.5, 1, -1, 0)} L1 1 L1 0.5 ${cran(1, 0.5, 0.5, 0.5, 0, -1)} Z`,
  );

  document.body.appendChild(svg);
})();

// Découpe de chaque pièce selon le nombre de pochettes (1 à 4).
const MOSAIQUE = {
  1: ["puz-1-0"],
  2: ["puz-2-0", "puz-2-1"],
  3: ["puz-3-0", "puz-3-1", "puz-3-2"],
  4: ["puz-4-0", "puz-4-1", "puz-4-2", "puz-4-3"],
};

/// Initiales d'un artiste pour la tuile de repli (aucune pochette) : une ou
/// deux lettres, « The » écarté pour ne pas toujours retomber sur « T ».
function initiales(nom) {
  const mots = (nom || "?")
    .replace(/^(the|les|le|la)\s+/i, "")
    .split(/\s+/)
    .filter(Boolean);
  const lettres = (mots.length > 1 ? mots[0][0] + mots[1][0] : (mots[0] || "?").slice(0, 2))
    .toUpperCase();
  return lettres || "?";
}

/// Teinte stable dérivée du nom, pour que la tuile de repli d'un artiste garde
/// la même couleur d'une session à l'autre.
function teinteNom(nom) {
  let h = 0;
  for (const c of nom || "") h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

function carteArtiste(item) {
  const el = document.createElement("div");
  el.className = "album album--artiste";
  el.innerHTML = `<div class="album__pochette"><div class="mosaique"></div></div>
                  <div class="album__nom"></div>
                  <div class="album__sec"></div>`;
  el.children[1].textContent = item.name;
  const alb = item.albums === 1 ? "1 album" : `${item.albums} albums`;
  const morc = item.tracks === 1 ? "1 morceau" : `${item.tracks} morceaux`;
  el.children[2].textContent = `${alb} · ${morc}`;
  el.addEventListener("click", () => activer(item));

  const mos = el.querySelector(".mosaique");
  cheminsArtiste(item).then((chemins) => {
    if (!mos.isConnected) return;
    if (chemins.length === 0) {
      mos.classList.add("mosaique--vide");
      mos.style.setProperty("--teinte", teinteNom(item.name));
      mos.textContent = initiales(item.name);
      return;
    }
    const n = Math.min(chemins.length, 4);
    MOSAIQUE[n].forEach((clip, i) => {
      const piece = document.createElement("div");
      piece.className = "mosaique__piece";
      piece.style.clipPath = `url(#${clip})`;
      mos.appendChild(piece);
      pochette(chemins[i]).then((img) => {
        if (img && piece.isConnected) piece.style.backgroundImage = `url("${img}")`;
      });
    });
  });

  return el;
}

/// Lit l'album entier, dans l'ordre du disque, sans passer par sa liste de
/// pistes — le geste que propose le bouton posé sur la pochette.
async function lireAlbum(item) {
  const pistes = await invoke("tracks_of_album", { album: item.name, artist: item.artist ?? null });
  if (pistes.length === 0) return;
  inspecter(pistes[0]);
  fileCourante = pistes;
  tracerRouteSurCarte(pistes);
  await demarrerLecture(() => invoke("play", { paths: pistes.map((t) => t.path) }));
}

const ALCHIMIE_PISTES = 20;

/// Playlist « dans l'esprit de l'album » : dérive depuis son morceau le plus
/// central vers des morceaux soniquement proches, ailleurs dans la
/// bibliothèque — l'équivalent local du « Song Alchemy » d'AudioMuse-AI.
///
/// Une graine neuve à chaque clic : le bouton est pensé pour surprendre, pas
/// pour rejouer toujours la même dérive sur le même album.
function genererAlchimie(item, bouton) {
  return composerAlchimie({
    bouton,
    chemin: () =>
      invoke("path_album", {
        album: item.name,
        artist: item.artist ?? null,
        steps: ALCHIMIE_PISTES,
        seed: Math.floor(Math.random() * 2 ** 31),
        bruit: bruitChemin,
      }),
    // Le cas courant d'échec est un album pas encore analysé (`path_album`
    // échoue alors côté moteur) : `composerAlchimie` le journalise et remet
    // la file en état, sans casser la lecture en cours.
    demarrer: (pistes) => invoke("play", { paths: pistes.map((t) => t.path) }),
  });
}

/// Fabrique commune aux deux boutons ✦ (case d'album, inspecteur).
///
/// Deux attentes se cachaient derrière la roulette système, muettes : le
/// graphe des voisins d'abord — un balayage complet, une vingtaine de
/// secondes la première fois d'une session, rien ensuite — puis l'errance
/// elle-même, quasi instantanée. On les nomme toutes deux dans le panneau de
/// file d'attente, avec une jauge, et on y fait défiler la playlist à mesure
/// qu'elle se pose plutôt que de la livrer d'un bloc.
///
/// `chemin()` rend la promesse des pistes (le trajet), `demarrer(pistes)`
/// celle de l'envoi au lecteur. La lecture part dès que le trajet est là ;
/// le défilement qui suit n'est qu'un habillage.
async function composerAlchimie({ bouton, chemin, demarrer }) {
  if (alchimieEnCours) return;
  alchimieEnCours = true;
  const fileEtaitOuverte = !$("file").hidden;
  let pistes = null;

  bouton.disabled = true;
  bouton.classList.add("alchimie--travail");
  demarrerCompositionFile(ALCHIMIE_PISTES);
  basculerFile(true);
  const jauge = sonderGrapheProgres();

  try {
    await preparerGraphe();
    clearInterval(jauge);
    phaseCompositionFile("Composition de la playlist…");

    pistes = await chemin();
    if (!pistes || pistes.length === 0) return;

    inspecter(pistes[0]);
    fileCourante = pistes;
    tracerRouteSurCarte(pistes);
    // Lecture tout de suite ; le sondage d'état qui démarre avec elle
    // voudra redessiner la file, `fileCompositionActive` l'en empêche
    // jusqu'à la fin du défilement.
    demarrerLecture(() => demarrer(pistes)).catch((e) => remonter(e, "composerAlchimie"));
    await revelerFile(pistes);
  } catch (e) {
    remonter(e, "composerAlchimie");
  } finally {
    clearInterval(jauge);
    finirCompositionFile();
    bouton.disabled = false;
    bouton.classList.remove("alchimie--travail");
    alchimieEnCours = false;
    if (pistes && pistes.length > 0) dessinerFile();
    else if (!fileEtaitOuverte) basculerFile(false);
    else dessinerFile();
  }
}

/// Ouvre la file sur la longueur cible : autant d'emplacements vides que de
/// pistes attendues, la phase en cours au-dessus. `revelerFile` viendra
/// ensuite y poser les vraies pistes une à une.
function demarrerCompositionFile(cible) {
  fileCompositionActive = true;
  $("file-compo").hidden = false;
  $("file-compo-phase").textContent = "Préparation du graphe des voisins…";
  majBarreCompo(0, 0);
  $("file-compte").textContent = `0 / ${cible}`;
  const hote = $("file-liste");
  hote.replaceChildren();
  for (let i = 0; i < cible; i++) hote.appendChild(ligneSquelette(i));
}

function phaseCompositionFile(texte) {
  $("file-compo-phase").textContent = texte;
  majBarreCompo(0, 0); // indéterminé le temps de la composition
}

function finirCompositionFile() {
  fileCompositionActive = false;
  $("file-compo").hidden = true;
}

/// `fait`/`total` de la jauge de composition. `total` à 0 : va-et-vient
/// indéterminé (graphe déjà en cache, ou composition en cours).
function majBarreCompo(fait, total) {
  const barre = $("file-compo-barre");
  if (total > 0) {
    barre.classList.remove("file__compo-barre--indetermine");
    barre.style.marginLeft = "0";
    barre.style.width = `${Math.round((fait / total) * 100)}%`;
  } else {
    barre.classList.add("file__compo-barre--indetermine");
    barre.style.marginLeft = "";
    barre.style.width = "";
  }
}

/// Sonde `graphe_progress` pendant l'attente du graphe des voisins. Rend
/// l'identifiant d'intervalle, à couper dès que `preparerGraphe` a rendu.
function sonderGrapheProgres() {
  return setInterval(async () => {
    try {
      const [fait, total] = await invoke("graphe_progress");
      if (total > 0) {
        $("file-compo-phase").textContent = "Préparation du graphe des voisins…";
        majBarreCompo(fait, total);
      }
    } catch {
      /* le sondage suivant réessaiera */
    }
  }, 200);
}

function ligneSquelette(i) {
  const el = document.createElement("div");
  el.className = "file__ligne file__ligne--squelette";
  el.innerHTML = `<span class="file__rang"></span>
                  <span class="file__txt"><b></b><span></span></span>
                  <span class="file__duree"></span>`;
  el.children[0].textContent = i + 1;
  return el;
}

/// Une piste posée dans la file en composition — même gabarit que
/// `dessinerFile`, avec l'animation d'entrée et, pour la première, la marque
/// « graine » : c'est le morceau d'où part la dérive.
function ligneComposee(t, i) {
  const el = document.createElement("div");
  el.className = "file__ligne file__ligne--pose";
  if (i === 0) el.classList.add("file__ligne--graine");
  el.innerHTML = `<span class="file__rang"></span>
                  <span class="file__txt"><b></b><span></span></span>
                  <span class="file__duree"></span>`;
  el.children[0].textContent = i === 0 ? "✦" : i + 1;
  el.children[1].children[0].textContent = txt(t.title, "(sans titre)");
  el.children[1].children[1].textContent =
    i === 0
      ? `graine · ${txt(t.artist, "(sans artiste)")}`
      : txt(t.artist, "(sans artiste)");
  el.children[2].textContent = duree(t.duration_ms);
  el.addEventListener("click", async () => {
    await demarrerLecture(() => invoke("jump_to", { index: i }));
  });
  return el;
}

/// Remplace les emplacements vides par les vraies pistes, une par battement,
/// pour donner à voir la playlist se construire. Le trajet est déjà entier —
/// l'errance est instantanée — c'est un rythme d'affichage, pas de calcul, et
/// l'ordre est le vrai ordre de la dérive.
async function revelerFile(pistes) {
  const hote = $("file-liste");
  // La longueur cible affichée d'emblée peut différer du trajet rendu
  // (échantillonnage côté moteur) : on réajuste les emplacements.
  while (hote.children.length < pistes.length)
    hote.appendChild(ligneSquelette(hote.children.length));
  while (hote.children.length > pistes.length) hote.lastChild.remove();

  for (let i = 0; i < pistes.length; i++) {
    hote.replaceChild(ligneComposee(pistes[i], i), hote.children[i]);
    $("file-compte").textContent = `${i + 1} / ${pistes.length}`;
    majBarreCompo(i + 1, pistes.length);
    await new Promise((r) => setTimeout(r, 55));
  }
}

// Le défilement émet des dizaines d'évènements par seconde, et `dessinerGrille`
// reconstruit toute la bande de cases visibles à chaque appel (DOM neuf,
// écouteurs, demande de pochette). Sans ce garde-fou, la grille se traînait au
// défilement et le bouton ▶ d'une case disparaissait sous le pointeur au
// moindre coup de molette juste avant le clic — d'où l'impression qu'il ne
// répondait pas. On ne redessine qu'une fois par trame, et seulement quand la
// bande de rangées visibles a réellement changé (un cran = `ALBUM_HAUT`).
let grilleRedessinArme = false;
let grilleDernierRang = -1;
function dessinerGrilleAuDefilement() {
  if (grilleRedessinArme) return;
  grilleRedessinArme = true;
  requestAnimationFrame(() => {
    grilleRedessinArme = false;
    const rang = Math.floor(grille.scrollTop / ALBUM_HAUT);
    if (rang === grilleDernierRang) {
      majIndexActif();
      return;
    }
    grilleDernierRang = rang;
    dessinerGrille();
  });
}

grille.addEventListener("scroll", dessinerGrilleAuDefilement, { passive: true });
window.addEventListener("resize", () => {
  if (!grille.hidden) {
    grilleDernierRang = -1;
    dessinerGrille();
  }
});

/* ---------------------------------------------------------- navigation */

// Le défilement d'où l'on vient : lu par `activer()` juste avant de
// descendre dans un artiste ou un album, pour que « ← » retrouve la ligne
// quittée plutôt que de remonter en haut de la liste.
function scrollActuel() {
  return (vueEnGrille() ? grille : liste).scrollTop;
}

function poser(quoi, titre, lignes, retour = null, scroll = 0) {
  vue.quoi = quoi;
  vue.titre = titre;
  vue.lignes = lignes;
  vue.retour = retour;
  if (retour === null) sommet = { quoi, titre, lignes };

  // Hors mode Écoute, l'en-tête appartient à l'autre mode (« Découvrir »,
  // « Bibliothèque ») : `poser` met à jour `vue` mais n'y touche pas.
  const horsEcoute = modeCourant !== "ecoute";
  if (!horsEcoute) {
    $("fil-titre").textContent = titre;
    // La grille d'albums peut être filtrée par famille : le compte suit ce qui
    // est réellement montré, pas le total.
    const compte = quoi === "albums" ? lignesCourantes().length : lignes.length;
    $("fil-compte").textContent = `${compte} ${quoi === "artistes" ? "artistes" : quoi === "albums" ? "albums" : "morceaux"}`;
  }
  $("retour").hidden = retour === null;
  $("retour").textContent = `← ${retour ? retour.titre : ""}`;

  // Artistes et albums partagent le même geste de premier niveau ; le
  // commutateur du rail suit celui qui a produit la vue affichée, y compris
  // en descendant depuis un artiste vers ses albums.
  if (quoi === "artistes" || quoi === "albums") {
    document.querySelectorAll("[data-vuelib]").forEach((b) =>
      b.classList.toggle("segment--actif", b.dataset.vuelib === quoi),
    );
  }

  const enGrille = vueEnGrille(quoi);
  // `charger("albums")` court en fond au démarrage et se termine parfois après
  // `basculerMode` (mode imposé par `RUSTY_MUSIC_MODE`, ou clic rapide) : on
  // prépare alors les données sans remontrer grille ni liste.
  $("liste").hidden = horsEcoute || enGrille;
  $("grille").hidden = horsEcoute || !enGrille;
  // Le choix d'ordre ne concerne que la grille d'albums du mode Écoute.
  $("tri-albums").hidden = horsEcoute || quoi !== "albums";
  if (enGrille) {
    // Pas de `preparerGraphe()` ici : la grille est la vue par défaut de
    // l'Écoute, donc ce qu'on construirait à *chaque* lancement de l'appli,
    // même pour qui n'ouvre jamais Explorer ni le bouton ✦. Signalé
    // directement — ce préchauffage saturait tous les cœurs dès le
    // démarrage et rendait la lecture saccadée. Le bouton ✦
    // (`genererAlchimie`) et l'entrée dans Explorer (`poserModeChemin`)
    // continuent de le construire à la demande ; le premier geste de la
    // session qui en a besoin paie simplement le prix une fois.
    grille.scrollTop = scroll;
    grilleDernierRang = -1;
    dessinerGrille();
  } else {
    liste.scrollTop = scroll;
    dessiner();
  }

  // Repère alphabétique : seulement là où l'ordre affiché est celui des
  // noms — pas la liste des pistes d'un album (ordre du disque), ni une
  // recherche (ordre de pertinence).
  const avecIndex = quoi === "artistes" || (quoi === "albums" && triAlbums === "alpha");
  $("index-alpha").hidden = !avecIndex;
  if (avecIndex) construireIndexAlpha();

  // Liste de morceaux : la popularité se charge en lot, puis on repeint les
  // lignes visibles (la liste est virtualisée, `dessiner` relit le cache).
  if (quoi === "pistes") {
    chargerPopularites(
      lignes.map((l) => l.id),
      () => {
        if (vue.quoi === "pistes") dessiner();
      },
    );
  }

  majBlocFamillesEcoute();
}

/* -------------------------------------------------------- index alphabétique */

const LETTRES_INDEX = ["#", ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"];
const indexAlphaHote = $("index-alpha");
LETTRES_INDEX.forEach((l) => {
  const b = document.createElement("button");
  b.className = "index-alpha__lettre";
  b.dataset.lettre = l;
  b.textContent = l;
  b.addEventListener("click", () => sauterALettre(l));
  indexAlphaHote.appendChild(b);
});

/// Première lettre d'un nom, ramenée à une des 26 majuscules ou à `#` pour
/// tout le reste (chiffres, ponctuation, écritures non latines) — un repère
/// à 27 entrées reste lisible, un par caractère Unicode ne le serait pas.
function premiereLettre(nom) {
  const c = (nom || "").trim().charAt(0).toUpperCase();
  return LETTRES_INDEX.includes(c) ? c : "#";
}

// Lettre → rang de sa première entrée dans `vue.lignes`. Reconstruit à
// chaque `poser()` d'une vue indexable : Artistes et Albums sont déjà triés
// par nom côté moteur (`ORDER BY … COLLATE NOCASE`), un simple passage
// suffit donc à trouver ces rangs.
let indexAlpha = {};
function construireIndexAlpha() {
  indexAlpha = {};
  lignesCourantes().forEach((item, i) => {
    const l = premiereLettre(item.name);
    if (!(l in indexAlpha)) indexAlpha[l] = i;
  });
  indexAlphaHote.querySelectorAll(".index-alpha__lettre").forEach((b) => {
    b.classList.toggle("index-alpha__lettre--vide", !(b.dataset.lettre in indexAlpha));
  });
  majIndexActif();
}

/// Rang affiché en haut de la fenêtre visible, pour savoir quelle lettre
/// grossir dans le repère.
function rangVisible() {
  if (vueEnGrille()) return Math.floor(grille.scrollTop / ALBUM_HAUT) * colonnesGrille();
  return Math.floor(liste.scrollTop / LIGNE);
}

function majIndexActif() {
  if ($("index-alpha").hidden) return;
  const lignes = lignesCourantes();
  const i = Math.min(lignes.length - 1, Math.max(0, rangVisible()));
  const lettre = lignes[i] ? premiereLettre(lignes[i].name) : null;
  indexAlphaHote.querySelectorAll(".index-alpha__lettre").forEach((b) =>
    b.classList.toggle("index-alpha__lettre--actif", b.dataset.lettre === lettre),
  );
}

function sauterALettre(l) {
  const rang = indexAlpha[l];
  if (rang === undefined) return;
  if (vueEnGrille()) {
    grille.scrollTop = Math.floor(rang / colonnesGrille()) * ALBUM_HAUT;
    grilleDernierRang = -1;
    dessinerGrille();
  } else {
    liste.scrollTop = rang * LIGNE;
    dessiner();
  }
}

document.querySelectorAll("[data-vuelib]").forEach((b) =>
  b.addEventListener("click", () => charger(b.dataset.vuelib)),
);

document.querySelectorAll("[data-tri]").forEach((b) =>
  b.addEventListener("click", () => choisirTriAlbums(b.dataset.tri)),
);

/// Change l'ordre de la grille d'albums. Un clic sur « Aléatoire » rebrasse à
/// chaque fois, même s'il est déjà actif. Le repère alphabétique n'a de sens
/// que pour l'ordre `alpha` — masqué pour les deux autres.
function choisirTriAlbums(tri) {
  if (tri === "alea") rebrasserAlea(vue.lignes);
  else if (tri === triAlbums) return;
  triAlbums = tri;
  document.querySelectorAll("[data-tri]").forEach((b) =>
    b.classList.toggle("tri__opt--actif", b.dataset.tri === tri),
  );
  const avecIndex = vue.quoi === "albums" && tri === "alpha";
  $("index-alpha").hidden = !avecIndex;
  if (avecIndex) construireIndexAlpha();
  grille.scrollTop = 0;
  rafraichirGrille();
}

async function activer(item) {
  if (vue.quoi === "artistes") {
    // Les deux sont nécessaires : un artiste réunit ses pistes étiquetées
    // MusicBrainz et les autres.
    const albums = await invoke("albums", { mbid: item.mbid ?? null, artist: item.name });
    poser("albums", item.name, albums, {
      quoi: "artistes", titre: "Artistes", lignes: vue.lignes, scroll: scrollActuel(),
    });
  } else if (vue.quoi === "albums") {
    const pistes = await invoke("tracks_of_album", { album: item.name, artist: item.artist ?? null });
    poser("pistes", item.name, pistes, {
      quoi: vue.quoi, titre: vue.titre, lignes: vue.lignes, retour: vue.retour, scroll: scrollActuel(),
    });
  } else {
    inspecter(item);
    // Lire depuis la piste choisie : la suite de la liste forme la file.
    const depart = vue.lignes.indexOf(item);
    fileCourante = vue.lignes.slice(depart);
    tracerRouteSurCarte(fileCourante);
    await demarrerLecture(() => invoke("play", { paths: fileCourante.map((t) => t.path) }));
  }
}

$("retour").addEventListener("click", () => {
  const r = vue.retour;
  if (r) poser(r.quoi, r.titre, r.lignes, r.retour ?? null, r.scroll ?? 0);
});

/* ---------------------------------------------------------- inspecteur */

async function inspecter(t) {
  $("insp-vide").hidden = true;
  $("insp").hidden = false;
  // Sert à savoir, au retour d'un calcul, si l'inspecteur montre encore le
  // même morceau.
  $("insp-titre").dataset.path = t.path;
  $("insp-titre").dataset.id = t.id;
  $("insp-titre").textContent = txt(t.title, "(sans titre)");
  $("insp-artiste").textContent = txt(t.artist, "(sans artiste)");
  // Repris par le clic sur le nom, ci-dessous : `artist_mbid` manque pour un
  // point de la carte (`MapPoint`, pas `TrackRow`), le nom seul suffit alors
  // à retrouver l'artiste, avec le même repli que `Library::albums_of_artist`.
  $("insp-artiste").dataset.artiste = t.artist ?? "";
  $("insp-artiste").dataset.mbid = t.artist_mbid ?? "";
  $("insp-album").textContent = txt(t.album, "—");
  $("insp-album").dataset.album = t.album ?? "";
  $("insp-album").dataset.artiste = t.artist ?? "";
  $("insp-annee").textContent = t.year ?? "—";
  $("insp-piste").textContent = t.track_no ?? "—";
  $("insp-duree").textContent = duree(t.duration_ms);

  const img = await pochette(t.path);
  const el = $("pochette");
  el.style.backgroundImage = img ? `url("${img}")` : "";
  el.classList.toggle("pochette--pleine", Boolean(img));

  montrerDescripteurs(t);
  montrerVoisins(t);
}

/// Ouvre au centre la grille de tous les albums d'un artiste — le geste
/// partagé par le nom de l'artiste dans l'inspecteur et dans le transport.
async function ouvrirAlbumsArtiste(artiste, mbid) {
  if (!artiste) return;
  const albums = await invoke("albums", { artist: artiste, mbid: mbid || null });
  // « Au centre » suppose le mode Écoute : depuis Explorer ou Éditer, le
  // centre montre la carte ou le dock, pas la grille.
  if (modeCourant !== "ecoute") await basculerMode("ecoute");
  poser("albums", artiste, albums, sommet);
}

/// Le nom de l'artiste, dans l'inspecteur, ouvre ses albums au centre — le
/// même geste que cliquer l'artiste depuis la liste « Artistes », mais depuis
/// n'importe quel morceau inspecté (piste, voisin sonique, point de la carte).
$("insp-artiste").addEventListener("click", () =>
  ouvrirAlbumsArtiste($("insp-artiste").dataset.artiste, $("insp-artiste").dataset.mbid),
);

/// Le nom de l'album, dans l'inspecteur, ouvre ses pistes au centre — même
/// geste et même principe que le nom de l'artiste juste au-dessus.
$("insp-album").addEventListener("click", async () => {
  const album = $("insp-album").dataset.album;
  if (!album) return;
  const artiste = $("insp-album").dataset.artiste || null;
  const pistes = await invoke("tracks_of_album", { album, artist: artiste });
  if (modeCourant !== "ecoute") await basculerMode("ecoute");
  poser("pistes", album, pistes, sommet);
});

/// Playlist « dans l'esprit de ce morceau » — même mécanisme que le bouton ✦
/// d'une case d'album (`genererAlchimie`), mais partie d'un seul morceau déjà
/// connu de la carte : une errance sonique depuis son point, pas besoin de
/// lui chercher un centre au préalable.
$("insp-alchimie").addEventListener("click", () => {
  const id = Number($("insp-titre").dataset.id);
  if (!Number.isFinite(id)) return;
  composerAlchimie({
    bouton: $("insp-alchimie"),
    chemin: () =>
      invoke("path", {
        from: id,
        mode: "errance",
        steps: ALCHIMIE_PISTES,
        seed: Math.floor(Math.random() * 2 ** 31),
        bruit: bruitChemin,
      }),
    // `remplacer_file`, pas `set_queue` : la playlist doit devenir la file
    // dès le morceau suivant. `set_queue` gardait le préchargement de la
    // file précédente (les résultats de recherche), qui s'intercalait avant
    // la playlist. Le morceau en cours, lui, continue sans coupure.
    demarrer: (pistes) => invoke("remplacer_file", { paths: pistes.map((t) => t.path) }),
  });
});

/// Tempo et tonalité sous le titre du transport.
///
/// Séparé de l'inspecteur, qui suit la **sélection** : le transport suit ce
/// qu'on **écoute**, et les deux divergent dès qu'on explore la carte sans
/// changer de morceau.
async function mesuresDuTransport(t) {
  const vise = t.path;
  let d;
  try {
    d = await invoke("descripteurs", { id: t.id });
  } catch {
    return;
  }
  if (!d || enLecture !== vise) return;
  const bouts = [];
  if (d.bpm) bouts.push(`${Math.round(d.bpm)} BPM`);
  const ton = tonaliteFr(d.tonalite);
  if (ton) bouts.push(ton);
  $("np-mesures").textContent = bouts.join(" · ");
}

/// Conteneurs sans perte : on y affiche la profondeur de bits plutôt que le
/// débit (qui n'y est pas constant et ne dit rien de la qualité).
const CODECS_SANS_PERTE = new Set(["FLAC", "WAV", "AIFF", "APE", "WavPack", "ALAC"]);

/// 44100 → « 44,1 kHz » ; 48000 → « 48 kHz ».
function khz(hz) {
  const v = hz / 1000;
  return `${(Number.isInteger(v) ? v : v.toFixed(1)).toString().replace(".", ",")} kHz`;
}

/// Ligne de qualité du transport : « FLAC · 16 bit · 44,1 kHz »,
/// « MP3 · 320 kb/s · 44,1 kHz ». Segments omis quand la donnée manque.
function formatQualite(q) {
  const bouts = [];
  if (q.codec) bouts.push(q.codec);
  const sansPerte = q.codec && CODECS_SANS_PERTE.has(q.codec);
  if (sansPerte && q.bit_depth) bouts.push(`${q.bit_depth} bit`);
  else if (!sansPerte && q.bitrate) bouts.push(`${q.bitrate} kb/s`);
  if (q.sample_rate) bouts.push(khz(q.sample_rate));
  if (q.channels === 1) bouts.push("mono");
  return bouts.join(" · ");
}

/// Qualité du fichier sous le compteur de temps. Comme `mesuresDuTransport` :
/// suit ce qu'on **écoute**, chargé à la volée, vide si le morceau n'a pas été
/// scanné pour son format.
async function qualiteDuTransport(t) {
  const vise = t.path;
  let q;
  try {
    q = await invoke("qualite_piste", { id: t.id });
  } catch {
    return;
  }
  if (!q || enLecture !== vise) return;
  $("np-qualite").textContent = formatQualite(q);
}

/// Noms français des douze classes de hauteur.
///
/// **La base note à l'anglaise** — `C`, `F#`, `A` — parce que c'est ce
/// qu'écrivent les profils de Krumhansl-Schmuckler et tout le domaine. Ici on
/// affiche, donc on traduit. Les altérations restent des dièses : la mesure ne
/// distingue pas un fa dièse d'un sol bémol, et choisir l'un des deux
/// prétendrait le contraire.
const NOTES_FR = {
  C: "Do", "C#": "Do♯", D: "Ré", "D#": "Ré♯", E: "Mi", F: "Fa",
  "F#": "Fa♯", G: "Sol", "G#": "Sol♯", A: "La", "A#": "La♯", B: "Si",
};

/// « F min » devient « Fa mineur ». Rend la chaîne telle quelle si elle ne se
/// lit pas — mieux vaut afficher ce qu'on a que de le perdre.
function tonaliteFr(t) {
  if (!t) return null;
  const [note, mode] = String(t).split(/\s+/);
  const fr = NOTES_FR[note];
  if (!fr) return t;
  return `${fr} ${mode === "min" ? "mineur" : mode === "maj" ? "majeur" : (mode ?? "")}`.trim();
}

/// Tempo et tonalité du morceau inspecté.
///
/// **Un tiret quand ce n'est pas mesuré, jamais une valeur par défaut.** La
/// passe couvre 15 847 morceaux sur 27 044 ; afficher « 120 BPM » sur le reste
/// donnerait une mesure qu'on n'a pas.
async function montrerDescripteurs(t) {
  const vise = t.path;
  $("insp-bpm").textContent = "—";
  $("insp-tonalite").textContent = "—";
  $("insp-timbre").textContent = "—";
  let d;
  try {
    d = await invoke("descripteurs", { id: t.id });
  } catch {
    return;
  }
  // L'inspecteur a pu changer de morceau pendant l'appel.
  if (!d || $("insp-titre").dataset.path !== vise) return;
  if (d.bpm) $("insp-bpm").textContent = `${Math.round(d.bpm)} BPM`;
  const ton = tonaliteFr(d.tonalite);
  if (ton) $("insp-tonalite").textContent = ton;
  // Descripteurs timbraux — mêmes réserves que tempo/tonalité : un tiret
  // tant qu'un morceau n'est pas passé dans la passe, jamais une valeur
  // inventée.
  const timbre = [];
  if (d.centroide_moy) timbre.push(`centroïde ${Math.round(d.centroide_moy)} Hz`);
  if (d.flatness_moy != null) timbre.push(`aplatissement ${d.flatness_moy.toFixed(2)}`);
  if (d.zcr != null) timbre.push(`ZCR ${d.zcr.toFixed(2)}`);
  if (timbre.length) $("insp-timbre").textContent = timbre.join(" · ");
}

/// Les morceaux les plus proches à l'oreille du moteur.
///
/// Seuls les morceaux analysés en ont : tant que la passe tourne, la plupart
/// n'y figurent pas encore, et le bloc reste caché plutôt que vide.
async function montrerVoisins(t) {
  const bloc = $("bloc-voisins");
  const hote = $("voisins");
  const vise = t.path;
  bloc.hidden = true;

  let proches = [];
  try {
    proches = await invoke("neighbours", { id: t.id, count: 6 });
  } catch {
    return;
  }
  // L'inspecteur a pu changer de morceau pendant le calcul.
  if (proches.length === 0 || $("insp-titre").dataset.path !== vise) return;

  hote.replaceChildren();
  for (const v of proches) {
    const el = document.createElement("button");
    el.className = "voisin";
    el.innerHTML = "<b></b><span></span>";
    el.children[0].textContent = txt(v.title, "(sans titre)");
    el.children[1].textContent = txt(v.artist, "(sans artiste)");
    el.addEventListener("click", async () => {
      inspecter(v);
      fileCourante = [v];
      tracerRouteSurCarte(fileCourante);
      await demarrerLecture(() => invoke("play", { paths: [v.path] }));
    });
    hote.appendChild(el);
  }
  bloc.hidden = false;
}

/* ---------------------------------------------------------- recherche */

let modeCourant = "ecoute";
let minuteur;
// Entrée dans la barre de recherche, en mode Explorer : le premier morceau
// filtré devient une borne du chemin.
//
// `ui-spec.md` retenait « le choix des 2 morceaux via la barre de recherche »
// sans dire comment. La barre y sert déjà à filtrer la carte ; lui ajouter un
// second rôle plutôt qu'un second champ garde le rail lisible, et le geste est
// d'une touche : on cherche, on valide, la borne se pose.
$("q").addEventListener("keydown", async (e) => {
  if (e.key !== "Enter" || modeCourant !== "explorer") return;
  const candidats = carte.points.filter(retenu);
  if (candidats.length === 0) {
    $("fil-compte").textContent = "rien à poser comme borne";
    return;
  }
  // Le plus proche du centre de la carte parmi les retenus : sur une
  // recherche large, prendre le premier de la liste tomberait n'importe où.
  const t = candidats.reduce((a, b) =>
    a.x * a.x + a.y * a.y <= b.x * b.x + b.y * b.y ? a : b,
  );
  await poserBorne(t);
  inspecter(t);
});

$("q").addEventListener("input", (e) => {
  clearTimeout(minuteur);
  const q = e.target.value.trim();
  minuteur = setTimeout(async () => {
    // Sur la carte, chercher ne remplace pas la vue : les morceaux qui ne
    // correspondent pas s'estompent et restent en fond. `ui-spec.md` le
    // tranche ainsi — le contexte de la bibliothèque ne doit pas disparaître.
    if (modeCourant === "explorer") {
      carte.filtre = q.toLowerCase();
      const n = carte.points.filter(retenu).length;
      $("fil-compte").textContent = q
        ? `${n.toLocaleString("fr-FR")} sur ${carte.points.length.toLocaleString("fr-FR")}`
        : `${carte.points.length.toLocaleString("fr-FR")} morceaux`;
      dessinerCarte();
      return;
    }
    if (!q) return charger();
    const r = await invoke("search", { query: q, limit: 200 });
    poser("pistes", `« ${q} »`, r, sommet);
  }, 180);
});

/* ------------------------------------------------------ file d'attente */

// L'interface connaît déjà la file : c'est elle qui l'a envoyée au moteur.
// Inutile de la redemander, `current` suffit à situer la lecture.
function dessinerFile() {
  // La composition d'une playlist ✦ tient la main sur le panneau : elle y
  // fait défiler ses pistes elle-même (`revelerFile`), un rendu ordinaire
  // par-dessus effacerait l'animation.
  if (fileCompositionActive) return;
  const hote = $("file-liste");
  $("file-compte").textContent = `${fileCourante.length} morceaux`;

  if (fileCourante.length === 0) {
    hote.innerHTML = '<p class="file__vide">Rien en file. Choisissez un morceau.</p>';
    return;
  }

  const rangCourant = fileCourante.findIndex((t) => t.path === enLecture);
  hote.replaceChildren();

  fileCourante.forEach((t, i) => {
    const el = document.createElement("div");
    el.className = "file__ligne";
    if (i === rangCourant) el.classList.add("file__ligne--joue");
    else if (rangCourant >= 0 && i < rangCourant) el.classList.add("file__ligne--passe");

    el.innerHTML = `<span class="file__rang"></span>
                    <span class="file__txt"><b></b><span></span></span>
                    <span class="file__pop"></span>
                    <span class="file__duree"></span>`;
    el.children[0].textContent = i === rangCourant ? "▶" : i + 1;
    el.children[1].children[0].textContent = txt(t.title, "(sans titre)");
    el.children[1].children[1].textContent = txt(t.artist, "(sans artiste)");
    el.children[2].appendChild(jaugePop(popParPiste.get(t.id)));
    el.children[3].textContent = duree(t.duration_ms);

    // Sauter conserve les pistes précédentes : on peut revenir en arrière.
    el.addEventListener("click", async () => {
      await demarrerLecture(() => invoke("jump_to", { index: i }));
    });
    hote.appendChild(el);
  });

  // La popularité, en lot puis repaint — comme les pochettes de la grille.
  chargerPopularites(
    fileCourante.map((t) => t.id),
    () => {
      if (!$("file").hidden && !fileCompositionActive) dessinerFile();
    },
  );
}

function basculerFile(ouvrir) {
  const panneau = $("file");
  const visible = ouvrir ?? panneau.hidden;
  panneau.hidden = !visible;
  $("bascule-file").setAttribute("aria-expanded", String(visible));
  // Sans ça, le bouton restait étiqueté « file » même une fois le panneau
  // ouvert — rien ne distinguait alors « l'ouvrir » de « le fermer ».
  $("bascule-file").textContent = visible ? "fermer" : "file";
  if (visible) dessinerFile();
}

$("bascule-file").addEventListener("click", () => basculerFile());
$("file-fermer").addEventListener("click", () => basculerFile(false));
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("file").hidden) basculerFile(false);
});

/* ---------------------------------------------------------- transport */

let enLecture = null;

// Le libellé bascule tout de suite, sans attendre l'aller-retour : sur un
// clic, un décalage de quelques dizaines de millisecondes se voit.
// `ignorerEtatJusqua` empêche un sondage déjà en vol de le remettre à l'ancien
// état avec une réponse antérieure au clic.
let ignorerEtatJusqua = 0;
// Ce que l'utilisateur veut entendre, indépendamment de ce que le moteur a eu
// le temps de faire. **Le DOM ne peut pas servir de mémoire ici** : le sondage
// réécrit l'icône, et un second clic lisait alors l'état d'avant le premier —
// il envoyait l'action inverse, et la lecture repartait au lieu de s'arrêter.
let veutJouer = false;
let clicEnVol = false;

/// Pose l'icône du transport **et** l'intention, ensemble.
///
/// Les deux ne peuvent pas diverger si un seul endroit les écrit. Divergentes,
/// elles produisaient un clic qui envoyait l'action inverse.
function poserLecture(joue) {
  veutJouer = joue;
  $("lecture").textContent = joue ? "⏸" : "▶";
}

/// Lance une lecture en affichant l'intention **avant** que le moteur réponde.
///
/// `demarrer` renvoie la promesse de l'`invoke("play")` / `invoke("set_queue")`.
/// Le moteur décode la première piste en mémoire avant de rendre la main —
/// jusqu'à ~1 s sur support lent. Poser l'icône après cet `await` (l'ancien
/// schéma) laissait le bouton sur ▶ pendant tout ce temps : le clic sur la
/// pochette paraissait ignoré. On bascule donc l'icône tout de suite ;
/// `ignorerEtatJusqua` empêche un battement de sondage de remettre ▶ tant que
/// le moteur n'a pas confirmé la lecture.
///
/// Le sondage, lui, ne démarre qu'**après** que le moteur a répondu : lancé
/// avant, son premier battement voit le lecteur encore `finished` (l'ancienne
/// file épuisée) et se coupe aussitôt (`if (e.finished) sonder(false)`) —
/// plus rien n'interrogeait alors le moteur et le panneau d'écoute restait
/// figé sur « Rien en lecture ».
async function demarrerLecture(demarrer) {
  poserLecture(true);
  ignorerEtatJusqua = Date.now() + 2000;
  try {
    await demarrer();
  } catch (e) {
    poserLecture(false);
    throw e;
  }
  sonder(true);
}

/// Un clic à la fois : les suivants sont ignorés, pas empilés. Empilés, ils
/// s'annulaient deux à deux. Partagé par le bouton et le raccourci clavier —
/// le second doit passer par la même garde, pas la contourner.
async function lireOuPauser() {
  if (clicEnVol) return;
  clicEnVol = true;
  try {
    await basculerLecture();
  } finally {
    clicEnVol = false;
  }
}

$("lecture").addEventListener("click", lireOuPauser);

async function basculerLecture() {
  const versPause = veutJouer;
  poserLecture(!versPause);
  ignorerEtatJusqua = Date.now() + 400;

  if (edition.enLecture) {
    // **On n'interroge pas le moteur avant d'agir.** L'aller-retour ajoutait
    // un appel à la file d'un pool déjà encombré par les sondages, et le clic
    // attendait derrière eux : on cliquait trois fois avant que la lecture ne
    // s'arrête. L'intention est connue — c'est ce que le bouton affichait.
    await invoke("stems_transport", {
      action: versPause ? "pause" : "reprendre",
      position: null,
    });
    return;
  }

  const pause = await invoke("toggle_pause");
  poserLecture(!pause);
  sonder(!pause);
}
async function pistePrecedente() {
  await invoke("previous");
  sonder(true);
}
async function pisteSuivante() {
  await invoke("skip");
  sonder(true);
}
$("precedent").addEventListener("click", pistePrecedente);
$("suivant").addEventListener("click", pisteSuivante);
$("volume").addEventListener("input", (e) => invoke("set_volume", { volume: e.target.value / 100 }));

// Bouton « E » : amélioration du son (excitation psychoacoustique). Choix
// retenu entre deux lancements, comme le bruit du chemin ou le mode de
// contact. La réglette d'intensité n'apparaît que quand « E » est actif.
const btnAmeliorer = $("ameliorer");
const forceAmeliorer = $("ameliorer-force");
let ameliorationActive = localStorage.getItem("ameliorer") === "1";
let ameliorationForce = clamp01(parseInt(localStorage.getItem("ameliorer-force"), 10), 60);
forceAmeliorer.value = ameliorationForce;

function clamp01(v, defaut) {
  return Number.isFinite(v) ? Math.min(100, Math.max(0, v)) : defaut;
}

function refletAmeliorer() {
  btnAmeliorer.setAttribute("aria-pressed", String(ameliorationActive));
  forceAmeliorer.hidden = !ameliorationActive;
}
refletAmeliorer();

// Applique l'état courant au moteur puis relance le sondage : le moteur
// réouvre le morceau en cours en tâche de fond (~1 s) et reconstruit le
// préchargement avec la nouvelle version. On désarme les commandes le temps
// de la bascule, sans figer l'UI.
async function poserAmelioration() {
  btnAmeliorer.disabled = true;
  forceAmeliorer.disabled = true;
  try {
    await invoke("set_amelioration", {
      actif: ameliorationActive,
      intensite: ameliorationForce / 100,
    });
  } finally {
    setTimeout(() => {
      btnAmeliorer.disabled = false;
      forceAmeliorer.disabled = false;
    }, 800);
  }
  sonder(true);
  rafraichirSpectreTransport();
}

// Au démarrage, si actif : on ne fait que reposer le drapeau (rien ne joue
// encore, `poserAmelioration` réouvrirait dans le vide).
if (ameliorationActive) {
  invoke("set_amelioration", {
    actif: true,
    intensite: ameliorationForce / 100,
  }).catch(() => {});
}

btnAmeliorer.addEventListener("click", () => {
  ameliorationActive = !ameliorationActive;
  localStorage.setItem("ameliorer", ameliorationActive ? "1" : "0");
  refletAmeliorer();
  poserAmelioration();
});

// `change` et non `input` : on ne réouvre le morceau qu'au relâché, pas à
// chaque pixel du glissement.
forceAmeliorer.addEventListener("change", () => {
  ameliorationForce = clamp01(parseInt(forceAmeliorer.value, 10), 60);
  localStorage.setItem("ameliorer-force", String(ameliorationForce));
  if (ameliorationActive) poserAmelioration();
});

// Bouton « HD » : super-résolution neuronale (AERO), rendue hors ligne.
const btnHd = $("hd");
let lectureHd = localStorage.getItem("lecture-hd") === "1";
let hdSonde = null; // intervalle de suivi d'une régénération en cours

// Le fond de plan de la carte MapLibre — eau, terre, voirie, bâti, toponymes.
// Chaque thème est un `style-<id>.json` déjà écrit par `engendrer_tuiles` (côté
// Rust, `rusty_music_carto::Palette`) ; basculer ne régénère aucune tuile.
// `osm-clair` = le `style.json` sans suffixe. L'overlay de familles (nuage,
// pastilles, territoires) ne dépend pas de ce choix. Déclaré ici, avec les
// autres préférences en `localStorage`, parce qu'`initialiserGL` le lit.
let carteTheme = localStorage.getItem("carte-theme") || "osm-clair";

if (lectureHd) invoke("set_lecture_hd", { actif: true }).catch(() => {});

/// Met le bouton « HD » à l'état du morceau `t` : absent / disponible / en cours.
async function majHd(t) {
  clearInterval(hdSonde);
  hdSonde = null;
  btnHd.classList.remove("travaille");
  btnHd.disabled = false;
  btnHd.textContent = "HD";
  btnHd.setAttribute("aria-pressed", "false");
  if (!t) {
    btnHd.disabled = true;
    return;
  }
  // Une régénération est-elle en cours pour ce morceau ?
  try {
    const s = await invoke("superres_state");
    if (s.en_cours && s.source === t.path) return suivreHd(t);
  } catch {}
  // Sinon : le cache existe-t-il ?
  try {
    const dispo = await invoke("superres_disponible", { path: t.path });
    btnHd.setAttribute("aria-pressed", String(dispo && lectureHd));
    btnHd.dataset.dispo = dispo ? "1" : "0";
  } catch {}
}

/// Suit l'avancement d'une régénération et rebascule le bouton à la fin.
function suivreHd(t) {
  btnHd.classList.add("travaille");
  btnHd.disabled = true;
  clearInterval(hdSonde);
  hdSonde = setInterval(async () => {
    let s;
    try {
      s = await invoke("superres_state");
    } catch {
      return;
    }
    if (s.source !== t.path) return;
    if (s.en_cours) {
      const pct = s.total ? Math.round((100 * s.faits) / s.total) : 0;
      btnHd.textContent = `${pct}%`;
    } else {
      clearInterval(hdSonde);
      hdSonde = null;
      // enLecture peut avoir changé entre-temps.
      const cour = fileCourante.find((x) => x.path === enLecture);
      majHd(cour);
      spectres.clear(); // le cache HD vient d'apparaître
      if (lectureHd) rafraichirSpectreTransport();
    }
  }, 700);
}

btnHd.addEventListener("click", async () => {
  const t = fileCourante.find((x) => x.path === enLecture);
  if (!t || btnHd.disabled) return;
  if (btnHd.dataset.dispo === "1") {
    // Le cache existe : bascule lecture originale ↔ HD.
    lectureHd = !lectureHd;
    localStorage.setItem("lecture-hd", lectureHd ? "1" : "0");
    btnHd.setAttribute("aria-pressed", String(lectureHd));
    btnHd.disabled = true;
    try {
      await invoke("set_lecture_hd", { actif: lectureHd });
    } finally {
      setTimeout(() => (btnHd.disabled = false), 800);
    }
    sonder(true);
    rafraichirSpectreTransport();
  } else {
    // Pas de cache : lancer la régénération.
    try {
      await invoke("start_superres", { path: t.path });
      suivreHd(t);
    } catch (e) {
      remonter(e, "start_superres");
    }
  }
});

// Espace : lecture/pause. ← / → : piste précédente/suivante. Appelle les
// mêmes fonctions que les boutons du transport, jamais `.click()` : un focus
// resté sur un bouton (album, mode, « retour »…) fait sinon parfois doubler
// l'action, le navigateur activant le bouton focalisé en plus de notre
// gestionnaire. `repeat` écarte l'auto-répétition d'une touche maintenue —
// sans lui, un appui un peu long envoyait des dizaines d'appels d'affilée.
// Inerte dans un champ de saisie (recherche, réglages numériques, volume) :
// un espace tapé dans la recherche ne doit pas couper la lecture.
document.addEventListener("keydown", (e) => {
  if (e.repeat) return;
  if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
  if (e.key === " ") {
    e.preventDefault();
    lireOuPauser();
  } else if (e.key === "ArrowRight") {
    e.preventDefault();
    pisteSuivante();
  } else if (e.key === "ArrowLeft") {
    e.preventDefault();
    pistePrecedente();
  }
});

// Les touches média du clavier (▶⏸, précédent, suivant) ne passent jamais par
// le DOM : macOS les intercepte au niveau système. `main.rs` les capte en
// raccourcis globaux et relaie chaque appui ici, par évènement plutôt que par
// commande — pour rejouer exactement le même geste que le bouton ou le
// raccourci clavier, garde anti-double-appui comprise.
window.__TAURI__.event.listen("touche-media", (e) => {
  if (e.payload === "lecture") lireOuPauser();
  else if (e.payload === "suivant") pisteSuivante();
  else if (e.payload === "precedent") pistePrecedente();
});

// Barre de progression : spectrogramme du son réellement joué (version HD du
// cache si le HD est actif, sinon l'original), avec tête de lecture par
// dessus. Ce que le HD a ajouté au-dessus de l'original est teinté de
// l'accent. Le calcul décode tout le fichier — quelques secondes — donc il
// tourne en tâche de fond et l'image apparaît quand elle est prête ; d'ici là
// un fond neutre sert de repère.
const wave = $("wave");
const waveCnv = $("wave-cnv");
const waveCtx = waveCnv.getContext("2d");
const HAUT_SPECTRE = 42;

const spectres = new Map(); // clé path+hd → { fond: canvas } déjà peint
let spectreCourant = null; // le { fond } affiché
let teteCourante = 0;

/// Recompose : image de fond + assombrissement de la partie écoulée + trait.
function peindreTransport(frac) {
  teteCourante = Math.min(1, Math.max(0, frac || 0));
  const w = waveCnv.width || 1;
  const h = waveCnv.height || 1;
  waveCtx.clearRect(0, 0, w, h);
  if (spectreCourant) {
    waveCtx.drawImage(spectreCourant.fond, 0, 0, w, h);
  }
  const x = Math.round(teteCourante * w);
  waveCtx.fillStyle = "rgba(0,0,0,.34)";
  waveCtx.fillRect(0, 0, x, h);
  waveCtx.fillStyle =
    getComputedStyle(document.documentElement).getPropertyValue("--txt").trim() || "#EDE8DC";
  waveCtx.fillRect(x, 0, 1, h);
}

/// Peint le spectrogramme hors écran, une fois — teinte l'ajout du HD.
function composerSpectre(s) {
  const { largeur: w, hauteur: h, pixels, pixels_ref, hd } = s;
  const fond = document.createElement("canvas");
  fond.width = w;
  fond.height = h;
  const fctx = fond.getContext("2d");
  const img = fctx.createImageData(w, h);
  const table = rampeRGB();
  const acc = (
    getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#c98a3b"
  )
    .replace("#", "")
    .match(/../g)
    .map((v) => parseInt(v, 16));
  for (let i = 0; i < pixels.length; i++) {
    const v = pixels[i];
    let c = table[v];
    // Là où le HD a mis nettement plus d'énergie que l'original, on vire vers
    // l'accent — d'autant plus fort que le gain est net.
    if (hd && pixels_ref) {
      const gain = v - pixels_ref[i];
      if (gain > 16) {
        const k = Math.min(1, (gain - 16) / 90);
        c = [
          c[0] + (acc[0] - c[0]) * k,
          c[1] + (acc[1] - c[1]) * k,
          c[2] + (acc[2] - c[2]) * k,
        ];
      }
    }
    img.data[i * 4] = c[0];
    img.data[i * 4 + 1] = c[1];
    img.data[i * 4 + 2] = c[2];
    img.data[i * 4 + 3] = 255;
  }
  fctx.putImageData(img, 0, 0);
  return { fond };
}

/// Demande le spectrogramme du son joué ; le fichier décodé prend quelques
/// secondes, on réessaie sans marteler.
async function chargerSpectreTransport(t) {
  spectreCourant = null;
  peindreTransport(teteCourante);
  if (!t) return;

  const largeur = Math.max(160, Math.round(wave.getBoundingClientRect().width));
  waveCnv.width = largeur;
  waveCnv.height = HAUT_SPECTRE;

  const hd = lectureHd ? 1 : 0;
  const cle = `${t.path}#${hd}`;
  if (spectres.has(cle)) {
    spectreCourant = spectres.get(cle);
    return peindreTransport(teteCourante);
  }

  const vise = t.path;
  const echeance = Date.now() + 120_000;
  let attente = 250;
  while (Date.now() < echeance && enLecture === vise) {
    let s;
    try {
      s = await invoke("spectre_transport", { path: vise, width: largeur, height: HAUT_SPECTRE });
    } catch (e) {
      remonter(e, "spectre_transport");
      return;
    }
    if (s && s.pixels && s.pixels.length) {
      const compose = composerSpectre(s);
      spectres.set(cle, compose);
      if (enLecture === vise) {
        spectreCourant = compose;
        peindreTransport(teteCourante);
      }
      return;
    }
    await new Promise((r) => setTimeout(r, attente));
    attente = Math.min(attente * 1.4, 3000);
  }
}

/// À rappeler quand le son joué change sans changer de morceau (E ou HD).
function rafraichirSpectreTransport() {
  const t = fileCourante.find((x) => x.path === enLecture);
  chargerSpectreTransport(t);
}

wave.addEventListener("click", async (e) => {
  const r = wave.getBoundingClientRect();
  await deplacerLecture((e.clientX - r.left) / r.width);
});

// Le sondage ne tourne que lorsqu'il y a quelque chose à suivre. Laissé en
// continu, il coûtait ~5 % de processeur en permanence, fenêtre au repos et
// file vide comprises — soit 28 minutes de CPU en 9 heures.
let sondage = null;
function sonder(actif) {
  if (actif && !sondage) {
    sondage = setInterval(battement, 200); // 5 Hz : assez pour la progression
    battement();
  } else if (!actif && sondage) {
    clearInterval(sondage);
    sondage = null;
  }
}

// Une pochette peut coûter 200 ms de lecture disque, soit toute la période du
// sondage : sans ce verrou les appels s'empilent et retardent les commandes de
// transport, qui attendent alors derrière eux.
let battementEnVol = false;

async function battement() {
  if (edition.enLecture) return battementStems();
  if (battementEnVol) return;
  battementEnVol = true;
  let e;
  try {
    e = await invoke("playback_state");
  } catch {
    return;
  } finally {
    battementEnVol = false;
  }

  if (e.current !== enLecture) {
    enLecture = e.current;
    const t = fileCourante.find((x) => x.path === enLecture);
    $("np-titre").textContent = t ? txt(t.title, "(sans titre)") : "Rien en lecture";
    $("np-artiste").textContent = t ? txt(t.artist, "(sans artiste)") : "Choisissez un morceau";
    // Hors du chemin critique, comme la pochette : la ligne se remplit quand
    // la mesure arrive, et reste vide si le morceau n'est pas mesuré.
    $("np-mesures").textContent = "";
    if (t) mesuresDuTransport(t);
    $("np-qualite").textContent = "";
    if (t) qualiteDuTransport(t);
    majHd(t);
    // L'inspecteur suit la lecture au fil de la file, pas seulement le
    // morceau sur lequel on a cliqué en premier : sans ça, il restait figé
    // sur le départ d'un album ou d'une playlist « dans l'esprit de » pendant
    // que la lecture avançait. Ce bloc ne s'exécute qu'au changement de
    // morceau joué (`e.current !== enLecture` ci-dessus), pas à chaque
    // sondage : inspecter autre chose en explorant, pendant que la lecture se
    // poursuit ailleurs, n'est donc pas immédiatement écrasé — seulement au
    // prochain vrai changement de morceau.
    if (t) inspecter(t);
    // Le morceau en écoute devient le départ proposé pour un chemin — pas
    // seulement au clic sur la carte, mais à chaque avancée de la lecture,
    // pour pouvoir explorer « à partir d'ici » à tout moment. L'arrivée et
    // le trajet déjà tracé restent en place : seul le départ suit.
    if (t) {
      const point = carte.points.find((p) => p.id === t.id);
      if (point) {
        carte.depart = point;
        dessinerBornes();
      }
    }
    $("transport-art").style.backgroundImage = "";
    if (t) {
      const vise = t.path;
      pochette(vise).then((img) => {
        // La piste a pu changer entre-temps.
        if (img && enLecture === vise) {
          $("transport-art").style.backgroundImage = `url("${img}")`;
        }
      });
    }
    chargerSpectreTransport(t);
    dessiner(); // met à jour la ligne surlignée
    if (!$("file").hidden) dessinerFile();
    // Sans ça, le halo « en écoute » de la carte reste figé sur le premier
    // morceau : rien d'autre ne redessine la carte au fil d'une playlist qui
    // avance toute seule, hors interaction avec elle.
    if (modeCourant === "explorer") dessinerCarte();
  }

  if (Date.now() >= ignorerEtatJusqua) {
    poserLecture(!(e.paused || e.finished));
  }

  const t = fileCourante.find((x) => x.path === enLecture);
  const total = t?.duration_ms ?? 0;
  $("tc").textContent = `${horloge(e.position_ms)} / ${horloge(total)}`;

  const frac = total ? Math.min(1, e.position_ms / total) : 0;
  peindreTransport(frac);
  // Les spectrogrammes suivent aussi la lecture ordinaire : tant que les
  // stems ne jouent pas, ils montrent où en est le morceau d'origine.
  if (modeCourant === "editer") poserTete(frac);

  // Plus rien ne bouge : inutile de continuer à interroger le moteur. Toute
  // action de transport relance le sondage.
  if (e.finished || e.paused) sonder(false);
}

/* ------------------------------------------------------------- carte */

// Le nuage vit dans son propre repère, en [-1, 1]. `vue` porte la
// transformation vers les pixels : un facteur et un décalage, rien de plus.
const carte = {
  points: [],
  // Adresse réelle (lon/lat) de chaque morceau logé, `id -> [x, y]` — vide
  // sur le monde fictif. Voir `villeReelle` et `chargerCarte`.
  positionsReelles: new Map(),
  vue: { k: 1, dx: 0, dy: 0 },
  isolee: null, // famille mise en avant, ou null
  survole: null,
  depart: null, // borne de départ d'un chemin
  arrivee: null, // borne d'arrivée
  route: null, // chemin tracé (les morceaux, un point par étape), ou null
  // Le TRAIT à dessiner entre les étapes de `route` — d'ordinaire `route`
  // lui-même (une droite d'étape en étape), sauf sur le plan de ville réel
  // où `tracerRouteSurCarte` l'enrichit après coup avec les vraies rues
  // entre chaque paire. `route` reste la vérité pour les repères posés à
  // chaque étape ; `routeTrace` ne sert qu'au tracé du trait.
  routeTrace: null,
  lasso: null, // contour en cours de tracé, en coordonnées de carte
  couleur: "famille", // famille, ou une clé de CONTINUES
  // Deux visualisations, et deux seulement : le nuage t-SNE dessiné au
  // canevas, et la carte en tuiles vectorielles. Ce sont deux lectures
  // différentes de la même projection — l'une montre les morceaux tels quels,
  // l'autre en fait un pays.
  affichage: "points", // points (nuage t-SNE) | carte (tuiles)
  familles: null, // [[rang, nom, effectif]], chargé une fois
  bornes: {}, // min et max de chaque variable continue, pour la rampe
  filtre: "", // texte du filtre ; les exclus s'estompent, jamais ne disparaissent
  chemin: "direct", // direct | sonique | errance | dessine
  trace: null, // tracé en cours de dessin, en coordonnées de carte
  graine: 1, // graine de l'errance ; « Autre tirage » l'incrémente
  refaire: null, // de quoi rejouer le dernier chemin avec une autre graine
};

/// Ce que chaque mode attend de la souris, et ce qu'il fabrique. Phrase
/// entière dans le rail, sous les boutons ; rappel court en pied de carte, où
/// la place est comptée.
const AIDE_CHEMIN = {
  direct: [
    "Clic : le départ. Maj+clic : l'arrivée. Le trajet suit la droite entre les deux points de la carte, en cueillant au plus près.",
    "maj+clic : l'arrivée",
  ],
  sonique: [
    "Clic : le départ. Maj+clic : l'arrivée. Le trajet ne passe que d'un proche voisin sonore au suivant : plus long, sans à-coup à l'oreille — mais pas forcément droit à l'écran, la carte 2D déforme les distances. Le nombre de morceaux n'est ici qu'un plafond.",
    "maj+clic : l'arrivée",
  ],
  errance: [
    "Maj+clic : une promenade sonique part de ce morceau et dérive sans jamais revenir sur ses pas, en tirant à chaque saut parmi ses voisins sonores — les plus proches ont plus de chances, réglable dans Réglages.",
    "maj+clic : promenade",
  ],
  dessine: [
    "Maj+glisser : le trait cueille les morceaux qu'il touche. Ce qu'il traverse à vide reste vide.",
    "maj+glisser : tracer",
  ],
  itineraire: [
    "Maj+clic : le départ. Maj+clic à nouveau : une arrivée, facultative si une durée est fixée. Le trajet suit les vraies rues et la playlist est faite des morceaux qui les bordent ; le profil (grands axes / petites rues / parcs) le retrace aussitôt.",
    "maj+clic : départ / arrivée",
  ],
};

/// Les variables continues qu'on peut porter sur la rampe.
///
/// Une seule mécanique pour les trois — bornes, dégradé, légende. Ajouter une
/// quatrième variable, c'est ajouter une ligne ici et un bouton dans le rail,
/// rien d'autre.
///
/// `tempo` et `energie` viennent de la passe `descripteurs` et peuvent manquer :
/// un morceau sans valeur reste tracé, en encre neutre.
const CONTINUES = {
  // `valide` écarte le bruit des tags avant qu'il n'entre dans les bornes :
  // sans lui, une poignée de morceaux à `year = 0` (année absente encodée en
  // « 0000 » plutôt qu'omise) tirait la borne basse à 0 et écrasait tout le
  // XXe-XXIe siècle sur les derniers pour cent de la rampe.
  annee: { champ: "year", format: (v) => String(Math.round(v)), valide: (v) => v > 0 },
  tempo: { champ: "bpm", format: (v) => `${Math.round(v)} BPM` },
  energie: { champ: "energy", format: (v) => v.toFixed(2) },
};

/// Étapes de la rampe séquentielle, lues dans la feuille de style.
function rampe() {
  return getComputedStyle(document.documentElement)
    .getPropertyValue("--rampe")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

/// Couleur d'un point sur la rampe, `t` entre 0 et 1.
function surRampe(etapes, t) {
  const i = Math.min(etapes.length - 1, Math.max(0, Math.round(t * (etapes.length - 1))));
  return etapes[i];
}

/// Vrai si le point passe le filtre courant.
function retenu(p) {
  if (!carte.filtre) return true;
  const q = carte.filtre;
  return (
    (p.title || "").toLowerCase().includes(q) ||
    (p.artist || "").toLowerCase().includes(q) ||
    (p.album || "").toLowerCase().includes(q)
  );
}

const cnv = $("carte");
const ctx = cnv.getContext("2d");

/* ------------------------------------------------- carte MapLibre (module 2)
 *
 * Les tuiles vectorielles remplacent le nuage et la nappe dessinés à la main ;
 * le canevas reste, transparent, pour ce que les tuiles ne portent pas — les
 * chemins, le lasso, les bornes de départ et d'arrivée.
 *
 * **MapLibre n'écoute aucun événement** (`interactive: false`) : le canevas
 * est au-dessus et garde tous les gestionnaires, qu'il relaie. C'est ce qui
 * permet à `versEcran` et `versCarte` — les deux seules transformations de
 * coordonnées du fichier — de simplement déléguer, et à tout le reste (lasso,
 * tracés, pointage) de continuer sans être réécrit.
 *
 * La carte vivait dans une seconde fenêtre : MapLibre ne s'y initialisait
 * jamais, sans la moindre erreur. Dans la fenêtre principale, il démarre du
 * premier coup — voir `docs/carto-etapes.md`.
 */

/// Demi-étendue du domaine de la carte. **Doit valoir exactement celle de
/// `crates/carto/src/projection.rs`**, sinon la surcouche se décale des tuiles.
/// Ne sert que sur le monde fictif — voir `villeReelle`.
const DEMI_ETENDUE = 1.08;

let gl = null; // l'instance MapLibre, ou null tant que les tuiles manquent
let glPret = false;

/// Ce sur quoi le bouton « Vue d'ensemble » ramène la caméra MapLibre :
/// `bounds` (l'emprise de la limite communale, `metadata["rusty:bounds"]` du
/// style) sur le plan de ville réel ; `center`/`zoom` d'accueil sinon, en
/// dernier repli. Rempli par `initialiserGL`.
let vueInitialeGL = null;

/// `true` quand un plan de ville réel est actif (`positions_carte` a rendu
/// quelque chose côté Rust) : `carte.points[i].x/y` valent alors des lon/lat
/// réels — une adresse de rue, pas une position t-SNE — et
/// `geoDepuisCarte`/`carteDepuisGeo` doivent être l'identité plutôt que la
/// projection du monde fictif. Mis à jour par `chargerCarte`.
let villeReelle = false;

/// `true` quand ce sont les adresses réelles du plan de ville qui sont à
/// l'écran — donc le repère dans lequel `versEcran`/`versCarte` travaillent.
/// Le nuage t-SNE et le plan de ville coexistent : dans le nuage, on reste en
/// t-SNE même quand une ville est importée. Les modes de chemin qui raisonnent
/// à l'écran (direct, dessiné, lasso) passent ce drapeau au moteur pour qu'il
/// interpole dans le bon repère — sans lui, un trait tracé sur le nuage était
/// testé contre des positions parisiennes et n'attrapait rien.
const carteReelle = () => carte.affichage === "carte" && villeReelle;

/// Le zoom maximal accordé à la caméra MapLibre — repris de `initialiserGL`
/// pour que `zoomer()` (molette, pincement) ne plafonne pas avant le zoom
/// auquel les tuiles ont vraiment quelque chose à montrer (le bâti n'apparaît
/// qu'au zoom 15 sur le plan de ville réel).
let zoomMax = 14;

/// L'instance MapLibre **si elle est aux commandes**, sinon `null`.
///
/// Le nuage t-SNE et la carte partagent le même canevas et les mêmes
/// gestionnaires ; c'est ce raccourci qui décide lequel des deux repères
/// gouverne les coordonnées, le déplacement et le zoom. Sans lui, ouvrir le
/// nuage laissait MapLibre projeter les points et le mode devenait
/// inatteignable.
function carteGL() {
  return carte.affichage === "carte" && gl ? gl : null;
}

/// La console d'une webview du système n'est pas lisible de l'extérieur : ce
/// qui compte repart vers le journal du processus.
function journalCarte(message, niveau = "log") {
  console[niveau === "warn" ? "warn" : "log"]("[carte] " + message);
  invoke("journal_carte", { niveau, message: String(message) }).catch(() => {});
}

function geoDepuisCarte(x, y) {
  // Plan de ville réel : `x`/`y` sont déjà des lon/lat (une adresse de rue),
  // pas un point du carré `[-1.08, 1.08]²` du monde fictif — rien à projeter.
  if (villeReelle) return [x, y];
  const u = (x / DEMI_ETENDUE + 1) / 2;
  const v = (1 - y / DEMI_ETENDUE) / 2;
  return [(u * 2 - 1) * 180, (Math.atan(Math.sinh(Math.PI * (1 - 2 * v))) * 180) / Math.PI];
}

function carteDepuisGeo(lng, lat) {
  if (villeReelle) return [lng, lat];
  const phi = (lat * Math.PI) / 180;
  const u = (lng / 180 + 1) / 2;
  const v = (1 - Math.log(Math.tan(phi) + 1 / Math.cos(phi)) / Math.PI) / 2;
  return [(u * 2 - 1) * DEMI_ETENDUE, (1 - v * 2) * DEMI_ETENDUE];
}

/// Combien de mètres au sol valent `px` pixels à l'écran, au centre de la vue
/// MapLibre courante. Sur le plan de ville réel, `direct` et `dessiné`
/// travaillent en mètres locaux côté moteur (`RepereLocal`) : le rayon de
/// cueillette doit être dans la même unité, pas dans le repère t-SNE.
function metresParPixels(px) {
  if (!gl) return px;
  const r = cnv.getBoundingClientRect();
  const cx = r.width / 2;
  const cy = r.height / 2;
  const a = gl.unproject([cx, cy]);
  const b = gl.unproject([cx + px, cy]);
  const latM = (((a.lat + b.lat) / 2) * Math.PI) / 180;
  const dx = (((b.lng - a.lng) * Math.PI) / 180) * Math.cos(latM) * 6371000;
  const dy = (((b.lat - a.lat) * Math.PI) / 180) * 6371000;
  return Math.hypot(dx, dy);
}

/// L'emprise géographique de tous les points, `[[ouest, sud], [est, nord]]`,
/// ou `null` s'il n'y en a pas encore. Sert à `fitBounds` pour la vue
/// d'ensemble : sur le plan de ville `p.x/p.y` sont déjà des lon/lat, sur le
/// monde fictif `geoDepuisCarte` les projette d'abord.
function bornesGeoPoints() {
  let o = Infinity, s = Infinity, e = -Infinity, n = -Infinity;
  const ajoute = (lng, lat) => {
    if (!Number.isFinite(lng) || !Number.isFinite(lat)) return;
    if (lng < o) o = lng;
    if (lng > e) e = lng;
    if (lat < s) s = lat;
    if (lat > n) n = lat;
  };
  if (villeReelle) {
    // Sur le plan de ville, seules les adresses réelles comptent. `p.x/p.y`
    // restent des coordonnées t-SNE (~[-1, 1]) : les passer pour des lon/lat
    // renverrait la caméra au large de l'Afrique — c'était la cause de
    // l'image grise. Les morceaux sans adresse sont simplement absents de
    // `positionsReelles`.
    for (const [lng, lat] of carte.positionsReelles.values()) ajoute(lng, lat);
  } else {
    for (const p of carte.points) {
      const [lng, lat] = geoDepuisCarte(p.x, p.y);
      ajoute(lng, lat);
    }
  }
  return o <= e && s <= n ? [[o, s], [e, n]] : null;
}

/// Les tuiles passent par `addProtocol`, sur le fil principal : le fil de
/// travail de MapLibre ne peut pas atteindre un schéma d'URI personnalisé.
if (window.maplibregl) {
  maplibregl.addProtocol("tuiles", async (params) => {
    const m = /^tuiles:\/\/[^/]*\/(carte|relief)\/(\d+)\/(\d+)\/(\d+)/.exec(params.url);
    if (!m) throw new Error("URL de tuile illisible : " + params.url);
    const octets = await invoke("tuile", { quoi: m[1], z: +m[2], x: +m[3], y: +m[4] });
    const donnees = octets instanceof ArrayBuffer ? octets : new Uint8Array(octets).buffer;
    // Un tableau vide veut dire « pas de tuile ici » : le cas ordinaire sur un
    // monde creux.
    return { data: donnees.byteLength ? donnees : null };
  });
}

async function initialiserGL() {
  if (gl || !window.maplibregl) return;
  let style;
  try {
    style = await invoke("style_carte", { theme: carteTheme });
  } catch (e) {
    // Pas encore de tuiles : le nuage dessiné à la main prend le relais.
    console.warn("[carte] tuiles absentes :", e);
    return;
  }
  // Sur le plan de ville réel, `style` porte son propre `center`/`zoom`
  // (Paris, pas le centre du monde fictif) — voir `crate::style::construire`
  // côté Rust. Absents sur le monde fictif : on retombe alors sur les
  // anciennes valeurs, inchangées. `maxZoom` suit le même principe pour le
  // sur-zoom : 14 par défaut (le monde fictif s'arrête à 9, le sur-zoom
  // comble le reste), mais jamais moins que ce que les tuiles couvrent
  // vraiment — sur Paris, le bâti n'apparaît qu'au zoom 15.
  const maxZoomTuiles = style?.sources?.carte?.maxzoom;
  zoomMax = Math.max(14, maxZoomTuiles || 0);
  const bb = style?.metadata?.["rusty:bounds"];
  vueInitialeGL = {
    center: style.center || [0, 0],
    zoom: style.zoom || 1.6,
    bounds:
      Array.isArray(bb) && bb.length === 4 && bb.every(Number.isFinite)
        ? [[bb[0], bb[1]], [bb[2], bb[3]]]
        : null,
  };
  gl = new maplibregl.Map({
    container: "carte-gl",
    style,
    center: style.center || [0, 0],
    zoom: style.zoom || 1.6,
    minZoom: 0,
    maxZoom: zoomMax,
    // Le canevas est au-dessus et relaie tout : MapLibre ne doit rien écouter.
    interactive: false,
    // ODbL : sur un plan de ville réel (`crates/osm`), les tuiles viennent
    // d'OpenStreetMap et l'attribution est obligatoire. Sur le monde
    // fictif, l'attribution par défaut ne dirait rien de faux — elle reste
    // affichée plutôt que testée au cas par cas ici.
    attributionControl: { compact: true, customAttribution: "© les contributeurs OpenStreetMap" },
    // Une carte inventée n'a ni nord ni horizon.
    renderWorldCopies: false,
    fadeDuration: 120,
    localIdeographFontFamily: "'Hiragino Sans', 'Noto Sans CJK JP', sans-serif",
  });
  gl.on("load", () => {
    glPret = true;
    journalCarte(
      "tuiles chargées : " + gl.getStyle().layers.length + " couches, zoom " +
      gl.getZoom().toFixed(2),
    );
    majCouleurGL();
    majFiltreGL();
    majAffichageGL();
    poserVignetteCarte(gl.getStyle());
    dessinerCarte();
  });
  gl.on("zoom", majVignetteZoom);
  gl.on("render", () => {
    synchroniserVue();
    dessinerSurcouche();
  });
  gl.on("error", (e) => {
    const m = e && e.error ? e.error.message || String(e.error) : "";
    if (/204|empty|Not Found/i.test(m)) return; // tuile absente : ordinaire
    journalCarte(m, "warn");
  });
}

/// Combien d'unités de carte tient un pixel, à la vue courante.
function uniteParPixel() {
  const a = gl.unproject([0, 0]);
  const b = gl.unproject([1, 0]);
  return Math.abs(carteDepuisGeo(b.lng, b.lat)[0] - carteDepuisGeo(a.lng, a.lat)[0]);
}

/// Recopie la vue de MapLibre dans `carte.vue`.
///
/// Le reste du fichier lit `carte.vue.k` pour dimensionner ses points, ses
/// libellés et ses seuils de pointage. Plutôt que de traquer ces usages, on
/// tient `k` à jour : c'est le facteur qui, multiplié par `echelle(r)`, donne
/// les pixels par unité de carte — exactement ce qu'il valait avant.
function synchroniserVue() {
  if (!carteGL()) return;
  const r = cnv.getBoundingClientRect();
  const c = echelle(r);
  if (c <= 0) return;
  carte.vue.k = 1 / (uniteParPixel() * c);
  const z = $("zoom-val");
  if (z) z.textContent = `×${carte.vue.k.toFixed(1).replace(".", ",")}`;
}

/// Colorer par famille, année, tempo ou énergie : une expression de style sur
/// la couche des morceaux, au lieu d'un redessin complet du nuage.
/// Bascule entre les deux visualisations.
///
/// Le conteneur des tuiles se cache quand le nuage reprend la main : le
/// canevas redessine alors tout lui-même, avec son propre repère.
function majAffichageGL() {
  const conteneur = $("carte-gl");
  const enCarte = carte.affichage === "carte";
  if (conteneur) conteneur.hidden = !enCarte;
  // Le fond de plan ne concerne que les tuiles MapLibre : sur le nuage de
  // points, le canevas dessine son propre repère et ce choix n'a aucun effet.
  const blocFond = $("bloc-fond");
  if (blocFond) blocFond.hidden = !enCarte;
  majSegmentsCouleur();
  if (enCarte && !gl) {
    initialiserGL()
      .then(() => {
        majCouleurGL();
        majFiltreGL();
        dessinerCarte();
      })
      .catch((e) => journalCarte("tuiles indisponibles : " + e, "warn"));
    return;
  }
  if (enCarte && gl) gl.resize();
  const vignette = $("carte-vignette");
  if (vignette) vignette.style.display = enCarte ? "" : "none";
  dessinerCarte();
}

/// `#RRGGBB` → `"r, g, b"`, ou `null`. Pour bâtir `rgb()`/`rgba()`.
function hexVersRgb(hex) {
  const m = /^#([0-9a-fA-F]{6})$/.exec(hex || "");
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return `${n >> 16 & 255}, ${n >> 8 & 255}, ${n & 255}`;
}

/// Le fondu de bordure façon maptoposter, **sur le plan de ville réel** : le
/// réseau se dissout dans le fond près des bords, au lieu de s'arrêter net sur
/// le périphérique (le halo de petite couronne lui donne de la matière). Teinte
/// = le fond du thème chargé ; force décroissante au zoom (`majVignetteZoom`).
function poserVignetteCarte(style) {
  const el = $("carte-vignette");
  if (!el) return;
  const couches = style?.layers || [];
  const couche = couches.find((l) => l.type === "background");
  const hex = couche && couche.id === "terre-reelle" && couche.paint?.["background-color"];
  const rgb = hexVersRgb(hex);
  if (!rgb) {
    el.style.background = "none";
    el.dataset.actif = "";
    return;
  }
  // Rampe linéaire, comme `create_gradient_fade` de maptoposter : la couleur du
  // fond, opaque au bord, transparente vers le centre. On interpole vers cette
  // même couleur à alpha 0 (`rgba(r,g,b,0)`), **jamais** vers `transparent` —
  // qui vaut `rgba(0,0,0,0)` et fait virer le milieu du dégradé au gris/noir.
  const plein = `rgb(${rgb})`, vide = `rgba(${rgb},0)`;
  const h = 30, c = 17; // % de fondu, vertical / horizontal
  el.style.background =
    `linear-gradient(to bottom, ${plein}, ${vide} ${h}%, ${vide} ${100 - h}%, ${plein}),` +
    `linear-gradient(to right, ${plein}, ${vide} ${c}%, ${vide} ${100 - c}%, ${plein})`;
  el.dataset.actif = "1";
  majVignetteZoom();
}

/// Opacité du fondu : pleine sous z13 (vue d'ensemble, esthétique poster),
/// éteinte à z16 (échelle de la façade, où un voile gênerait la lecture).
function majVignetteZoom() {
  const el = $("carte-vignette");
  if (!el || el.dataset.actif !== "1" || !gl) return;
  el.style.opacity = String(Math.max(0, Math.min(1, (16 - gl.getZoom()) / 3)));
}

function majCouleurGL() {
  if (!gl || !glPret) return;
  // Le plan de ville réel n'a pas de point de morceau (`style::couches`,
  // depuis que le bâtiment habité porte le morceau) — l'un des deux
  // existe selon le chemin, jamais les deux.
  const pointMorceaux = gl.getLayer("morceaux-point");
  const batimentsMorceaux = gl.getLayer("batiments-morceaux");
  if (!pointMorceaux && !batimentsMorceaux) return;
  const teintes = couleursFamillesCarte();
  let expr;
  if (carte.couleur === "famille") {
    const m = ["match", ["get", "famille"]];
    teintes.forEach((t, i) => m.push(i, t));
    m.push(autresCarte());
    expr = m;
  } else {
    const { champ } = CONTINUES[carte.couleur] ?? {};
    const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 1];
    const etapes = rampe();
    const i = ["interpolate", ["linear"], ["coalesce", ["get", champ], v0]];
    etapes.forEach((t, n) => i.push(v0 + ((v1 - v0) * n) / (etapes.length - 1), t));
    expr = i;
  }
  if (pointMorceaux) gl.setPaintProperty("morceaux-point", "circle-color", expr);
  // Seule la coloration par famille se transpose au bâtiment : les modes
  // continus (année, tempo, énergie) n'ont pas d'attribut correspondant sur
  // un bâtiment (`palier` n'y porte que la famille de l'occupant) — le
  // bâtiment garde alors sa couleur de famille par défaut plutôt que de
  // virer à une teinte plate erronée.
  if (batimentsMorceaux && carte.couleur === "famille") {
    // Passe par le même constructeur que `majFiltreGL` : la coloration du
    // bâti et l'isolement d'une famille doivent rester cohérents.
    gl.setPaintProperty("batiments-morceaux", "fill-color", couleurBatimentsMorceaux());
  }
  if (gl.getLayer("territoires")) {
    gl.setLayoutProperty(
      "territoires",
      "visibility",
      carte.couleur === "famille" ? "visible" : "none",
    );
  }
}

/// Gris du bâti — la même valeur que `BATI` dans `crates/carto/src/style.rs`.
/// Un bâtiment d'une famille non isolée y revient (choix utilisateur : la
/// famille mise en avant ressort, la trame de la ville reste lisible autour).
const GRIS_BATI = "#DEDAD2";

/// Couches que l'isolement d'une famille (`carte.isolee`) masque par un
/// **filtre**, avec le champ MVT qui porte la famille sur chacune (la plupart
/// `famille`, le bâti et les territoires réels la portent dans `palier`).
///
/// Un filtre, pas une opacité : la plupart de ces couches ont une opacité
/// **interpolée sur le zoom** dans le style, et MapLibre n'autorise `["zoom"]`
/// qu'au sommet d'un `interpolate`/`step` — la multiplier par un `["case"]`
/// (l'ancienne approche) produisait une expression invalide que
/// `setPaintProperty` rejetait en silence, d'où « cliquer une famille ne fait
/// rien ». `setFilter` n'a pas cette contrainte et masque proprement (pas de
/// collision d'étiquette fantôme non plus). Le bâti habité est à part : il
/// vire au gris (`couleurBatimentsMorceaux`) plutôt que de disparaître.
const CIBLES_FILTRE_GL = [
  ["territoires", "famille"],
  ["territoires-reels", "palier"],
  ["territoires-reels-contour", "palier"],
  ["morceaux-point", "famille"],
  ["morceaux-etiquette", "famille"],
  ["artistes-point", "famille"],
  ["artistes-etiquette", "famille"],
  ["albums-point", "famille"],
  ["albums-etiquette", "famille"],
  ["batiments-morceaux-bord", "palier"],
];

/// Couleur de remplissage du bâti habité (`batiments-morceaux`) : la teinte
/// de la famille de l'occupant (champ `palier`), sauf quand une famille est
/// isolée — les autres reviennent alors au gris du bâti vacant.
function couleurBatimentsMorceaux() {
  const teintes = couleursFamillesCarte();
  const gris = grisBatiCarte();
  const parPalier = ["match", ["get", "palier"]];
  teintes.forEach((t, i) => parPalier.push(i, t));
  parPalier.push(gris);
  return carte.isolee === null
    ? parPalier
    : ["case", ["==", ["get", "palier"], carte.isolee], parPalier, gris];
}

/// Filtre de chaque couche tel que `style::construire` l'a posé, capturé au
/// premier appel pour pouvoir le rétablir sans le reconstruire côté JS.
const filtreOriginal = new Map();
function filtreBase(layer) {
  if (!filtreOriginal.has(layer)) {
    filtreOriginal.set(layer, gl.getFilter(layer) ?? null);
  }
  return filtreOriginal.get(layer);
}

/// Isole une famille sur les tuiles MapLibre — le pendant, en mode Carte, de
/// ce que `dessinerCarte()` fait sur le canevas 2D du nuage. Les autres
/// familles disparaissent (territoires, points et étiquettes) ; leur bâti
/// habité vire au gris.
function majFiltreGL() {
  if (!gl || !glPret) return;

  if (gl.getLayer("batiments-morceaux")) {
    gl.setPaintProperty("batiments-morceaux", "fill-color", couleurBatimentsMorceaux());
  }

  for (const [layer, champ] of CIBLES_FILTRE_GL) {
    if (!gl.getLayer(layer)) continue;
    const base = filtreBase(layer);
    if (carte.isolee === null) {
      gl.setFilter(layer, base);
    } else {
      const seulement = ["==", ["get", champ], carte.isolee];
      gl.setFilter(layer, base ? ["all", base, seulement] : seulement);
    }
  }
}


function dimensionner() {
  const r = cnv.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  cnv.width = Math.max(1, Math.round(r.width * dpr));
  cnv.height = Math.max(1, Math.round(r.height * dpr));
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return r;
}

/// Demi-côté utile du canevas : le nuage vit en [-1, 1], on lui laisse une
/// marge de 28 px pour que les points de bord ne collent pas au cadre.
function echelle(r) {
  return Math.min(r.width, r.height) / 2 - 28;
}

function versEcran(p, r) {
  const g = carteGL();
  if (g) {
    // Sur le plan de ville réel, l'adresse (si ce morceau en a une) prime
    // sur la position t-SNE — c'est elle que les tuiles montrent.
    const [x, y] = (villeReelle && carte.positionsReelles.get(p.id)) || [p.x, p.y];
    const q = g.project(geoDepuisCarte(x, y));
    return [q.x, q.y];
  }
  // Le nuage, lui, reste toujours en t-SNE — `p.x`/`p.y` n'ont jamais été
  // touchés, que la ville soit active ou non.
  const { k, dx, dy } = carte.vue;
  const c = echelle(r);
  return [r.width / 2 + (p.x * c) * k + dx, r.height / 2 + (p.y * c) * k + dy];
}

/// L'inverse de `versEcran` : du pixel vers le repère du nuage. Le dessin en a
/// besoin — c'est le seul endroit où l'on part de l'écran pour aller vers les
/// données, et non l'inverse.
function versCarte(mx, my, r) {
  const g = carteGL();
  if (g) {
    const p = g.unproject([mx, my]);
    return carteDepuisGeo(p.lng, p.lat);
  }
  const { k, dx, dy } = carte.vue;
  const c = echelle(r) * k;
  return [(mx - r.width / 2 - dx) / c, (my - r.height / 2 - dy) / c];
}

/// Les douze teintes de famille, lues dans la feuille de style.
///
/// Sert au **nuage de points**, à la **légende** et au mode Écoute — tout ce
/// qui suit le thème clair/sombre de l'application. Sur la carte MapLibre,
/// voir `couleursFamillesCarte` : les familles s'y calent sur le fond de plan.
function couleursFamilles() {
  return getComputedStyle(document.documentElement)
    .getPropertyValue("--familles")
    .split(",")
    .map((c) => c.trim())
    .filter(Boolean);
}

/// Palettes de familles **de la carte**, calées sur chaque fond de plan.
/// Miroir de `crates/carto/src/palette.rs` — les deux doivent nommer une
/// famille de la même couleur (comme `--familles` ↔ `style.rs` auparavant) :
/// Rust les cuit dans `style-<id>.json`, le JS les rejoue après `setStyle`
/// pour rester cohérent avec l'isolement d'une famille (`majFiltreGL`).
const FAMILLES_VIVES = [
  "#EF8891", "#EC9066", "#D99E46", "#B7AF47", "#88BC6A", "#4EC497",
  "#0CC3C3", "#38BBE6", "#73AEF8", "#A39FF6", "#C892E1", "#E289BD",
];
const FAMILLES_CARTE = {
  sepia: {
    familles: [
      "#B24B58", "#B05323", "#9E6300", "#7E7400", "#4C8227", "#00895D",
      "#00888A", "#0080AC", "#3472BE", "#6B63BC", "#8F56A7", "#A74D83",
    ],
    autres: "#8A7A60",
    bati: "#EADBC4",
  },
  encre: {
    familles: [
      "#B06A70", "#AE7455", "#A08346", "#7C8347", "#579156", "#2E9480",
      "#33908F", "#4C86A0", "#6981AC", "#7D74A6", "#94709E", "#B06C94",
    ],
    autres: "#8C867A",
    bati: "#ECE6DB",
  },
  nuit: { familles: FAMILLES_VIVES, autres: "#8C8C90", bati: "#333333" },
  "bleu-plan": {
    familles: [
      "#E6A6AD", "#E3AB8A", "#D4B47C", "#BCC088", "#9CCA94", "#74CDB4",
      "#6CC9D2", "#86C6E2", "#A2C8F5", "#C0BEF5", "#D6B0E7", "#E6ADD0",
    ],
    autres: "#7C93AE",
    bati: "#234870",
  },
};

/// Le jeu de familles pour la carte : celui du fond de plan actif, ou celui de
/// la feuille de style (`osm-clair`, qui suit le thème de l'appli).
function couleursFamillesCarte() {
  return FAMILLES_CARTE[carteTheme]?.familles ?? couleursFamilles();
}
/// Le gris « fourre-tout » (`#6E6656` par défaut) accordé au fond de plan.
function autresCarte() {
  return FAMILLES_CARTE[carteTheme]?.autres ?? "#6E6656";
}
/// Le gris du bâti (vacant, ou d'une famille non isolée) accordé au fond.
function grisBatiCarte() {
  return FAMILLES_CARTE[carteTheme]?.bati ?? GRIS_BATI;
}

/// Pastilles pré-dessinées, une par couleur.
///
/// 27 000 appels à `arc()` par image rendraient le survol poussif. On dessine
/// chaque pastille **une fois** dans un canevas minuscule, puis on la recopie
/// — `drawImage` d'une petite image est bien moins cher qu'un tracé de
/// chemin. Et les recopies se superposent : là où les morceaux s'entassent,
/// l'opacité s'accumule et la densité se voit, ce qu'un carré opaque cachait.
const pastilles = new Map();
function pastille(couleur, rayon, alpha) {
  const cle = `${couleur}|${rayon}|${alpha}`;
  const connue = pastilles.get(cle);
  if (connue) return connue;

  const d = Math.max(2, Math.ceil(rayon * 2) + 2);
  const c = document.createElement("canvas");
  c.width = d;
  c.height = d;
  const g = c.getContext("2d");
  g.globalAlpha = alpha;
  g.fillStyle = couleur;
  g.beginPath();
  g.arc(d / 2, d / 2, rayon, 0, Math.PI * 2);
  g.fill();
  pastilles.set(cle, c);
  return c;
}

/// Le nuage de points ordinaire — fond estompé d'abord, sélection par-dessus,
/// pour qu'elle ne soit jamais recouverte. C'était tout le corps de
/// `dessinerCarte` avant que la densité ne lui donne une alternative.
function dessinerNuage(r) {
  const style = getComputedStyle(document.documentElement);
  const encre = style.getPropertyValue("--txt").trim() || "#EDE8DC";
  const rayon = Math.max(1.1, 1.9 * Math.sqrt(carte.vue.k));
  const etapes = rampe();
  const teintes = couleursFamilles();
  const continu = CONTINUES[carte.couleur];
  const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 0];
  const neutre = carte.isolee === null && !carte.filtre;

  for (const avant of [false, true]) {
    for (const p of carte.points) {
      const vise = (carte.isolee === null || p.cluster === carte.isolee) && retenu(p);
      if (vise !== avant) continue;

      const [x, y] = versEcran(p, r);
      if (x < -8 || y < -8 || x > r.width + 8 || y > r.height + 8) continue;

      let couleur;
      if (continu) {
        const v = p[continu.champ];
        couleur = v != null && v1 > v0 ? surRampe(etapes, (v - v0) / (v1 - v0)) : encre;
      } else {
        couleur = teintes[p.cluster % teintes.length] ?? encre;
      }
      // L'opacité fait double emploi : elle écarte ce qui est filtré, et elle
      // révèle la densité par superposition.
      const alpha = avant ? (neutre ? 0.62 : 0.95) : 0.07;
      const rr = avant && !neutre ? rayon * 1.5 : rayon;
      const img = pastille(couleur, rr, alpha);
      ctx.drawImage(img, x - img.width / 2, y - img.height / 2);
    }
  }
}

/* ---------------------------------------------------- carte, densité */

// Alternative au nuage : une nappe de densité, comme un relevé topographique
// — teinte par famille (ou par la rampe continue), lignes de niveau
// par-dessus. Elle rend lisibles deux choses que le nuage noie sous 27 000
// points : les creux à faible densité, où tracer un chemin ne croise
// personne, et les cols entre deux modes d'une distribution, où deux familles
// se touchent sans se confondre.
//
// Calculée sur une grille basse résolution dans le repère de la carte
// (indépendante du zoom), mise en cache tant que les points, le regroupement
// ou la variable de coloration ne changent pas. Le tracé par image se contente
// ensuite d'un `drawImage` mis à l'échelle de la vue courante — le panoramique
// et le zoom sont donc gratuits, sans reconstruire la grille à chaque geste.

// Le regroupement par famille (genre) a migré côté Rust
// (`crates/core::density`) : noyau gaussien par famille + carré marchant →
// isobandes, densité maximale gagnante entre familles avant même d'en
// extraire les bandes — les territoires se pavent sans se recouvrir, plutôt
// que d'être contourés séparément puis mélangés à l'affichage. Ce fichier ne
// refait plus ce calcul : il construit les tracés vectoriels une fois par
// résultat reçu, les peint une fois dans une image hors-écran (relief +
// ombre portée courte, comme du papier découpé), puis recopie cette image à
// l'échelle de la vue à chaque image — zoom et panoramique restent gratuits.
//
// La coloration par variable continue (année/tempo/énergie) n'entre pas
// dans ce chantier : elle reste calculée ici, en JS, comme avant — une seule
// nappe, pas de recouvrement entre familles à résoudre, donc aucun besoin du
// vecteur Rust.


/* ---------------------------------------- densité, variable continue */

// Hors chantier : la coloration par année/tempo/énergie garde son ancien
// calcul, entièrement en JS — une seule nappe, jamais de recouvrement entre
// familles à résoudre, donc aucun besoin du pavage vectoriel ci-dessus.


function hexRGB(hex) {
  const n = parseInt((hex || "#9A9284").replace("#", ""), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/// Teinte → HSL, pour faire varier la clarté et la saturation avec la
/// densité plutôt que la seule opacité — utilisé aussi bien par le pavage
/// par famille ci-dessus que par la nappe continue ci-dessous.
function hexHSL(hex) {
  const [r, g, b] = hexRGB(hex).map((v) => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0, s = 0;
  const d = max - min;
  if (d > 1e-6) {
    s = d / (1 - Math.abs(2 * l - 1));
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return [h, s, l];
}

function hslRGB(h, s, l) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}


function dessinerCarte() {
  // En mode carte, les tuiles portent le fond : le canevas ne garde que la
  // surcouche. En mode nuage, il dessine tout, comme avant.
  const g = carteGL();
  if (g) {
    g.resize();
    dessinerSurcouche();
    return;
  }
  const r = dimensionner();
  const style = getComputedStyle(document.documentElement);
  const encre = style.getPropertyValue("--txt").trim() || "#EDE8DC";
  const accent = style.getPropertyValue("--accent").trim() || "#C07C4A";

  ctx.clearRect(0, 0, r.width, r.height);
  if (carte.points.length === 0) {
    ctx.fillStyle = style.getPropertyValue("--mut").trim() || "#9A9284";
    ctx.font = "13px system-ui";
    ctx.textAlign = "center";
    ctx.fillText("Aucun morceau analysé pour l'instant.", r.width / 2, r.height / 2);
    return;
  }

  // Le nuage t-SNE, tel qu'il a toujours été.
  dessinerNuage(r);
  surcoucheSur(r, encre, accent);
}

/// Ce que le canevas dessine par-dessus les tuiles : le lasso en cours, le
/// tracé à la souris, l'itinéraire, les bornes, le survol. Tout ce que les
/// tuiles ne portent pas parce que cela change à chaque geste.
function dessinerSurcouche() {
  if (!carteGL()) return;
  const r = dimensionner();
  const style = getComputedStyle(document.documentElement);
  ctx.clearRect(0, 0, r.width, r.height);
  surcoucheSur(
    r,
    style.getPropertyValue("--txt").trim() || "#EDE8DC",
    style.getPropertyValue("--accent").trim() || "#C07C4A",
  );
}

function surcoucheSur(r, encre, accent) {
  // Le lasso en cours : contour fermé et zone assombrie, pour qu'on voie ce
  // qu'on attrape avant de lâcher.
  if (carte.lasso && carte.lasso.length > 1) {
    ctx.beginPath();
    carte.lasso.forEach(([x, y], i) => {
      const [ex, ey] = versEcran({ x, y }, r);
      if (i === 0) ctx.moveTo(ex, ey);
      else ctx.lineTo(ex, ey);
    });
    ctx.closePath();
    ctx.fillStyle = accent;
    ctx.globalAlpha = 0.12;
    ctx.fill();
    ctx.globalAlpha = 0.9;
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([4, 3]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }

  // Le trait en cours de dessin : pointillé, pour le distinguer d'un chemin.
  if (carte.trace && carte.trace.length > 1) {
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.setLineDash([5, 4]);
    ctx.beginPath();
    carte.trace.forEach(([x, y], i) => {
      const [ex, ey] = versEcran({ x, y }, r);
      if (i === 0) ctx.moveTo(ex, ey);
      else ctx.lineTo(ex, ey);
    });
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Le chemin, par-dessus le nuage : un trait continu et ses étapes.
  //
  // Un simple trait à l'accent s'y perdait : plusieurs familles portent des
  // teintes proches de l'accent, et à cette épaisseur le trait devenait un
  // point de plus dans le nuage. Un liseré dans l'encre du texte — le ton le
  // plus contrasté sur le fond, dans les deux thèmes — le fait ressortir
  // quel que soit ce qu'il traverse, comme un tracé de route sur une carte
  // routière ; l'accent reste au-dessus, plus fin, pour l'identité du trait.
  if (carte.route && carte.route.length > 1) {
    ctx.lineJoin = "round";
    ctx.lineCap = "round";

    // Le trait suit `routeTrace` (les vraies rues, une fois habillé) — mais
    // seulement en mode Carte : ses points intermédiaires sont des lon/lat,
    // qui n'ont aucun sens passés tels quels dans le repère t-SNE du Nuage.
    // Les repères ci-dessous restent posés sur `route` (une entrée par
    // morceau) — `routeTrace` peut porter bien plus de points que d'étapes.
    const trait = (carteGL() && carte.routeTrace) || carte.route;
    const tracer = () => {
      ctx.beginPath();
      trait.forEach((p, i) => {
        const [x, y] = versEcran(p, r);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.stroke();
    };

    ctx.strokeStyle = encre;
    ctx.globalAlpha = 0.55;
    ctx.lineWidth = 4.5;
    tracer();

    ctx.strokeStyle = accent;
    ctx.globalAlpha = 0.95;
    ctx.lineWidth = 2.25;
    tracer();

    for (const p of carte.route) {
      const [x, y] = versEcran(p, r);
      ctx.beginPath();
      ctx.arc(x, y, 4.5, 0, Math.PI * 2);
      ctx.fillStyle = accent;
      ctx.globalAlpha = 1;
      ctx.fill();
      ctx.lineWidth = 1.2;
      ctx.strokeStyle = encre;
      ctx.globalAlpha = 0.8;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // Les bornes du chemin : anneaux creux, l'un plein pour le départ.
  for (const [borne, plein] of [[carte.depart, true], [carte.arrivee, false]]) {
    if (!borne) continue;
    const [x, y] = versEcran(borne, r);
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(x, y, 5.5, 0, Math.PI * 2);
    ctx.stroke();
    if (plein) {
      ctx.fillStyle = accent;
      ctx.beginPath();
      ctx.arc(x, y, 2, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // Le morceau en écoute : un halo qui le retrouve dans 27 000 points.
  const joue = carte.points.find((p) => p.path === enLecture);
  if (joue) {
    const [x, y] = versEcran(joue, r);
    ctx.strokeStyle = encre;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 9, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 0.35;
    ctx.beginPath();
    ctx.arc(x, y, 14, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // Le morceau survolé : un anneau, lisible quel que soit le fond.
  if (carte.survole) {
    const [x, y] = versEcran(carte.survole, r);
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 7, 0, Math.PI * 2);
    ctx.stroke();
  }
}

/// Point sous le curseur — sur le plan réel, vise d'abord le bâtiment que
/// MapLibre a effectivement peint sous ce pixel (`queryRenderedFeatures`) :
/// depuis que le morceau est un bâtiment entier, pas un point (voir
/// `carto-etapes.md`), un simple rayon autour du centroïde manquait tout
/// clic vers le bord d'un grand bâtiment. Retombe sur le point le plus
/// proche dans un rayon raisonnable si rien n'est peint là (bâtiment vacant,
/// zoom trop large, ou monde fictif sans bâti).
function pointSous(mx, my) {
  const g = carteGL();
  if (g && villeReelle && g.getLayer("batiments-morceaux")) {
    const feats = g.queryRenderedFeatures([mx, my], { layers: ["batiments-morceaux"] });
    const id = feats[0]?.properties?.morceau;
    if (id != null && id >= 0 && (carte.isolee === null || feats[0].properties.palier === carte.isolee)) {
      const p = carte.points.find((pt) => pt.id === id);
      if (p) return p;
    }
  }

  const r = cnv.getBoundingClientRect();
  let meilleur = null;
  let d2min = 14 * 14;
  for (const p of carte.points) {
    if (carte.isolee !== null && p.cluster !== carte.isolee) continue;
    const [x, y] = versEcran(p, r);
    const d2 = (x - mx) ** 2 + (y - my) ** 2;
    if (d2 < d2min) {
      d2min = d2;
      meilleur = p;
    }
  }
  return meilleur;
}

cnv.addEventListener("mousemove", (e) => {
  const r = cnv.getBoundingClientRect();
  const mx = e.clientX - r.left;
  const my = e.clientY - r.top;

  // Lasso et tracé de chemin se suivent de la même façon : un point tous les
  // 3 px, pour ne pas accumuler des centaines de points confondus.
  const encours = carte.lasso ?? carte.trace;
  if (encours) {
    const p = versCarte(mx, my, r);
    const d = encours[encours.length - 1];
    const seuil = 3 / (echelle(r) * carte.vue.k);
    if (Math.hypot(p[0] - d[0], p[1] - d[1]) > seuil) encours.push(p);
    dessinerCarte();
    return;
  }

  if (glisse) {
    const g = carteGL();
    if (g) {
      // Le canevas relaie : MapLibre n'écoute rien lui-même.
      g.panBy([-e.movementX, -e.movementY], { duration: 0 });
    } else {
      carte.vue.dx += e.movementX;
      carte.vue.dy += e.movementY;
    }
    dessinerCarte();
    return;
  }

  const p = pointSous(mx, my);
  if (p !== carte.survole) {
    carte.survole = p;
    const info = $("carte-info");
    if (p) {
      info.hidden = false;
      info.innerHTML = "<b></b><span></span>";
      info.children[0].textContent = txt(p.title, "(sans titre)");
      info.children[1].textContent = txt(p.artist, "(sans artiste)");
      info.style.left = `${Math.min(mx + 14, r.width - 290)}px`;
      info.style.top = `${my + 14}px`;
    } else {
      info.hidden = true;
    }
    dessinerCarte();
  }
});

let glisse = false;
// Un dessin ou un lasso se termine par un `mouseup`, donc par un `click` :
// sans ce drapeau, relâcher le trait relancerait la lecture du point survolé.
let vientDeDessiner = false;
// Un déplacement de la carte se termine lui aussi par un `click`. Sans cette
// mesure, faire glisser la vue changeait le morceau en écoute — le point sous
// le curseur à l'arrivée n'a rien demandé.
let departGlisse = null;
const SEUIL_GLISSE = 4; // px ; en deçà, c'est un clic tremblant, pas un glissé

cnv.addEventListener("mousedown", (e) => {
  // Un relâchement hors du canevas ne produit pas de `click` : sans cette
  // remise à zéro, le drapeau survivrait et avalerait le clic suivant.
  vientDeDessiner = false;

  // Alt+glisser : lasso. Disponible dans tous les modes de chemin — c'est une
  // sélection, pas un chemin, et rien ne justifie de la cacher derrière un
  // mode.
  if (e.altKey) {
    const r = cnv.getBoundingClientRect();
    carte.lasso = [versCarte(e.clientX - r.left, e.clientY - r.top, r)];
    carte.route = null;
    carte.survole = null;
    $("carte-info").hidden = true;
    return;
  }

  if (carte.chemin === "dessine" && e.shiftKey) {
    const r = cnv.getBoundingClientRect();
    carte.trace = [versCarte(e.clientX - r.left, e.clientY - r.top, r)];
    carte.route = null;
    carte.survole = null;
    $("carte-info").hidden = true;
    return;
  }
  glisse = true;
  departGlisse = [e.clientX, e.clientY];
});

window.addEventListener("mouseup", (e) => {
  if (departGlisse) {
    const bouge =
      Math.hypot(e.clientX - departGlisse[0], e.clientY - departGlisse[1]) > SEUIL_GLISSE;
    departGlisse = null;
    if (bouge) vientDeDessiner = true; // le clic qui suit est à ignorer
  }
});

window.addEventListener("mouseup", async () => {
  glisse = false;
  if (carte.lasso) {
    const contour = carte.lasso;
    carte.lasso = null;
    vientDeDessiner = true;
    dessinerCarte();
    await jouerSelection(contour);
    return;
  }
  if (!carte.trace) return;
  const trace = carte.trace;
  carte.trace = null;
  vientDeDessiner = true;
  await tracerDessin(trace);
});

cnv.addEventListener("mouseleave", () => {
  carte.survole = null;
  $("carte-info").hidden = true;
  dessinerCarte();
});

/// Applique un facteur de zoom autour d'un point de l'écran.
function zoomer(f, cx, cy) {
  const g = carteGL();
  if (g) {
    // Zoomer autour du curseur : on note le point du monde qui s'y trouve,
    // on change d'échelle, puis on recentre pour l'y remettre. Les boutons
    // +/− appellent sans coordonnées : `unproject([undefined, …])` lève dans
    // MapLibre — on retombe alors sur le centre du canevas.
    const r0 = cnv.getBoundingClientRect();
    if (cx == null) cx = r0.width / 2;
    if (cy == null) cy = r0.height / 2;
    const avant = g.unproject([cx, cy]);
    const z = Math.min(zoomMax, Math.max(0, g.getZoom() + Math.log2(f)));
    g.setZoom(z);
    const apres = g.unproject([cx, cy]);
    const c = g.getCenter();
    g.setCenter([c.lng + (avant.lng - apres.lng), c.lat + (avant.lat - apres.lat)]);
    synchroniserVue();
    return;
  }
  const r = cnv.getBoundingClientRect();
  const mx = (cx ?? r.width / 2) - r.width / 2;
  const my = (cy ?? r.height / 2) - r.height / 2;
  const avant = carte.vue.k;
  carte.vue.k = Math.min(60, Math.max(0.5, carte.vue.k * f));
  // Le facteur réellement appliqué, après butée : sans cela, le décalage
  // continuerait de bouger une fois le zoom bloqué.
  const reel = carte.vue.k / avant;
  carte.vue.dx = mx - (mx - carte.vue.dx) * reel;
  carte.vue.dy = my - (my - carte.vue.dy) * reel;
  $("zoom-val").textContent = `×${carte.vue.k.toFixed(1).replace(".", ",")}`;
  dessinerCarte();
}

/// L'image hors-écran de la densité est bâtie pour un niveau de zoom donné
/// : la redessiner à chaque cran de molette la
/// referait des dizaines de fois par seconde pour rien, un délai court
/// après la fin du geste suffit — même principe que le bruit des chemins
/// ou la force de l'ombre.
let attenteZoomDensite = null;

cnv.addEventListener("wheel", (e) => {
  e.preventDefault();
  // Proportionnel à l'ampleur du geste, et non un pas fixe par évènement :
  // un trackpad en émet des dizaines par centimètre de doigt, là où une
  // molette en émet un par cran. Le pas fixe rendait le zoom inutilisable au
  // trackpad. Le facteur est borné pour qu'une inertie brutale ne fasse pas
  // traverser toute la plage d'un coup.
  const f = Math.exp(-Math.max(-40, Math.min(40, e.deltaY)) * 0.0035);
  const r = cnv.getBoundingClientRect();
  zoomer(f, e.clientX - r.left, e.clientY - r.top);
}, { passive: false });

$("zoom-plus").addEventListener("click", () => zoomer(1.4));
$("zoom-moins").addEventListener("click", () => zoomer(1 / 1.4));
$("zoom-reset").addEventListener("click", () => {
  const g = carteGL();
  if (g) {
    // Priorité à la limite communale (`vueInitialeGL.bounds`) : prise sur la
    // frontière réelle, elle ne se fait pas tirer au loin par un morceau mal
    // géocodé. À défaut — monde fictif — l'emprise des points. Le `zoom`
    // d'accueil du style (14, niveau du quartier) n'est qu'un dernier repli.
    const b = vueInitialeGL?.bounds || bornesGeoPoints();
    let applique = null;
    if (b) {
      // On calcule la caméra sans bouger, on la vérifie, puis on l'applique :
      // une emprise dégénérée (coordonnées parasites) donnerait un zoom
      // minuscule et une carte vide — mieux vaut alors le centre d'accueil.
      const cam = g.cameraForBounds(b, { padding: 40 });
      if (cam && cam.zoom >= 6) {
        g.jumpTo(cam);
        applique = `bounds → zoom ${cam.zoom.toFixed(2)}`;
      }
    }
    if (!applique && vueInitialeGL) {
      g.jumpTo({ center: vueInitialeGL.center, zoom: vueInitialeGL.zoom });
      applique = `centre d'accueil (bounds ${b ? "rejetées" : "absentes"})`;
    }
    journalCarte("vue d'ensemble : " + (applique || "aucune cible"));
    synchroniserVue();
    dessinerCarte();
    return;
  }
  carte.vue = { k: 1, dx: 0, dy: 0 };
  $("zoom-val").textContent = "×1,0";
  dessinerCarte();
});

cnv.addEventListener("click", async (e) => {
  if (vientDeDessiner) {
    vientDeDessiner = false;
    return;
  }
  const p = carte.survole;
  if (!p) return;

  // Maj est le modificateur « chemin » dans tous les modes ; ce qu'il déclenche
  // dépend du mode choisi dans le rail.
  if (e.shiftKey) {
    if (carte.chemin === "dessine") return; // le dessin passe par le glisser
    await poserBorne(p);
    return;
  }

  // Sans modificateur : on écoute, et le morceau devient le départ proposé.
  carte.depart = p;
  carte.arrivee = null;
  carte.route = null;
  dessinerBornes();
  inspecter(p);
  fileCourante = [p];
  await demarrerLecture(() => invoke("play", { paths: [p.path] }));
  dessinerCarte();
});

/// Combien de morceaux le chemin doit compter — plafond pour le mode
/// sonique, dont la longueur naturelle est celle du graphe.
function longueurChemin() {
  const n = Number.parseInt($("chemin-n").value, 10);
  return Number.isFinite(n) ? Math.min(120, Math.max(2, n)) : 12;
}

/// Demande un chemin au moteur et l'envoie au lecteur.
///
/// `spec` est conservé tel quel pour que « Autre tirage » puisse rejouer le
/// même geste avec une graine différente.
async function tracerChemin(spec) {
  carte.refaire = spec;
  patienter("calcul du chemin…");
  let pistes;
  try {
    pistes = await invoke("path", {
      ...spec,
      steps: longueurChemin(),
      seed: carte.graine,
      bruit: bruitChemin,
      reel: carteReelle(),
      famille: carte.isolee,
    });
  } finally {
    patienter(null);
  }
  poserChemin(pistes);
}

/// Chemin dessiné : le tracé part en coordonnées de carte, avec le rayon de
/// cueillette que vaut le zoom courant. 24 px à l'écran, quel que soit le
/// facteur — c'est ce que l'utilisateur croit toucher avec son trait.
///
/// `trace` et `rayon` sont mémorisés dans `carte.refaire` (forme distincte de
/// celle de `tracerChemin`, voir `rejouerChemin`) : contrairement aux trois
/// autres modes, il n'y a pas de `{from, to, mode}` à rejouer, juste ce
/// tracé-ci, avec un nouveau bruit ou une nouvelle graine.
async function tracerDessin(trace) {
  const r = cnv.getBoundingClientRect();
  // Sur le plan de ville, le tracé et les points sont en lon/lat projetés en
  // mètres côté moteur : le rayon aussi (24 px du zoom courant, en mètres).
  // Sur le nuage t-SNE, le rayon reste dans le repère de la vue.
  const rayon = carteReelle()
    ? metresParPixels(24)
    : 24 / (echelle(r) * carte.vue.k);
  carte.refaire = { dessine: true, trace, rayon };
  patienter("cueillette sous le trait…");
  let pistes;
  try {
    pistes = await invoke("path_drawn", {
      trace,
      steps: longueurChemin(),
      radius: rayon,
      seed: carte.graine,
      bruit: bruitChemin,
      reel: carteReelle(),
      famille: carte.isolee,
    });
  } finally {
    patienter(null);
  }
  // Le trait dessiné EST l'intention : sur le plan de ville, on l'affiche tel
  // quel, sans le remplacer par un routage de rues entre les morceaux cueillis.
  poserChemin(pistes, "le trait n'a touché aucun morceau", carteReelle() ? trace : null);
}

/// Rejoue le dernier chemin tracé — bouton « Autre tirage » ou curseur de
/// bruit. `carte.refaire` prend deux formes : celle de `tracerChemin`
/// (`{from, to, mode}`, pour direct/sonique/errance) ou celle de
/// `tracerDessin` (`{dessine: true, trace, rayon}`), qui n'a pas de bornes à
/// repasser à `path` — chacune retrouve son propre chemin d'appel.
async function rejouerChemin() {
  if (!carte.refaire) return;
  if (carte.refaire.dessine) {
    const { trace, rayon } = carte.refaire;
    patienter("cueillette sous le trait…");
    let pistes;
    try {
      pistes = await invoke("path_drawn", {
        trace,
        steps: longueurChemin(),
        radius: rayon,
        seed: carte.graine,
        bruit: bruitChemin,
        reel: carteReelle(),
      });
    } catch (e) {
      remonter(e, "chemin dessiné");
      return;
    } finally {
      patienter(null);
    }
    poserChemin(pistes, "le trait n'a touché aucun morceau", carteReelle() ? trace : null);
  } else {
    await tracerChemin(carte.refaire);
  }
}

/// Joue les morceaux d'une zone dessinée au lasso.
///
/// Le moteur les rend déjà ordonnés en parcours de proche en proche : une zone
/// donne des dizaines de morceaux, et les enchaîner dans l'ordre de la base
/// produirait une playlist qui saute d'un bout à l'autre de la sélection.
async function jouerSelection(contour) {
  if (contour.length < 3) return;
  patienter("sélection…");
  let pistes;
  try {
    pistes = await invoke("selection", { trace: contour, reel: carteReelle(), famille: carte.isolee });
  } finally {
    patienter(null);
  }
  if (!pistes || pistes.length === 0) {
    $("fil-compte").textContent = "le lasso n'a rien attrapé";
    return;
  }
  // Le tracé de la zone laisse la place au parcours qu'on va suivre.
  carte.refaire = null;
  await poserChemin(pistes, "sélection vide");
  $("fil-compte").textContent = `${pistes.length} morceaux de la zone`;
}

/// Affiche un chemin reçu du moteur et le met en lecture.
/// Reflète une file sur la carte : le chemin entre ses morceaux, s'il y en a
/// au moins deux — un seul morceau est déjà signalé par le halo de lecture
/// (`enLecture`), indépendant de `carte.route`. Sans effet tant que la carte
/// n'a pas chargé ses points ; `basculerMode` rejoue l'appel pour la file en
/// cours en entrant dans Explorer, pour rattraper une file lancée avant.
let traceRuesGraine = 0; // écarte une réponse `trace_rues` périmée par un trajet plus récent

function tracerRouteSurCarte(pistes, polyligne = null) {
  if (!carte.points.length) return;
  const parId = new Map(carte.points.map((p) => [p.id, p]));
  carte.route =
    pistes && pistes.length >= 2
      ? pistes.map((t) => parId.get(t.id)).filter(Boolean)
      : null;
  // Le trait suit d'abord la droite d'étape en étape — même donnée que les
  // repères, tant que rien de mieux n'est arrivé.
  carte.routeTrace = carte.route;
  traceRuesGraine++;

  // L'itinéraire sur voirie (et le tracé dessiné) livrent déjà leur ligne : on
  // l'affiche telle quelle, sans repasser par `trace_rues` (qui router-ait
  // entre les morceaux dans l'ordre de la playlist et pourrait boucler).
  // `{x, y}` = lon/lat, comme les segments de `trace_rues`.
  if (villeReelle && polyligne && polyligne.length && carte.route) {
    carte.routeTrace =
      polyligne.length >= 2
        ? [
            carte.route[0],
            ...polyligne.map(([x, y]) => ({ x, y })),
            carte.route[carte.route.length - 1],
          ]
        : carte.route; // polyligne dégénérée : la droite entre morceaux, mais pas d'habillage
    if (modeCourant === "explorer") dessinerCarte();
    return;
  }

  if (modeCourant === "explorer") dessinerCarte();

  // Sur le plan de ville réel, habiller ce trait avec les vraies rues entre
  // chaque paire consécutive — **le choix des morceaux ne change pas**,
  // seule la ligne dessinée. Asynchrone et non bloquant : le trait droit
  // reste affiché jusqu'à ce que les rues arrivent, puis le remplace.
  if (villeReelle && carte.route && carte.route.length >= 2) {
    const ids = carte.route.map((p) => p.id);
    const graine = traceRuesGraine;
    invoke("trace_rues", { ids })
      .then((segments) => {
        // Un autre trajet est arrivé entre-temps, ou la route a été effacée :
        // cette réponse ne concerne plus ce qui est à l'écran.
        if (graine !== traceRuesGraine || !carte.route) return;
        const habille = [carte.route[0]];
        segments.forEach((segment, i) => {
          for (const [x, y] of segment) habille.push({ x, y });
          habille.push(carte.route[i + 1]);
        });
        carte.routeTrace = habille;
        if (modeCourant === "explorer") dessinerCarte();
      })
      .catch((e) => journalCarte("habillage du trait par les rues : " + e, "warn"));
  }
}

async function poserChemin(pistes, vide = "aucun chemin trouvé", polyligne = null) {
  if (!pistes || pistes.length < 2) {
    $("fil-compte").textContent = vide;
    carte.route = null;
    dessinerCarte();
    return;
  }

  tracerRouteSurCarte(pistes, polyligne);
  fileCourante = pistes;
  // Sans ce redessin, le panneau « file » resté ouvert continue de montrer
  // l'ancienne liste : rien d'autre ne le rafraîchit ici tant que le premier
  // morceau ne change pas — exactement le cas quand on ajuste la pondération
  // de l'errance depuis le même départ, où seule la suite change.
  if (!$("file").hidden) dessinerFile();
  // `set_queue`, pas `play` : si le premier morceau ne change pas — un
  // simple réglage du curseur de bruit ou « Autre tirage » sur le même
  // départ —, la lecture en cours n'a aucune raison de repartir de zéro.
  await demarrerLecture(() => invoke("set_queue", { paths: pistes.map((t) => t.path) }));
  inspecter(pistes[0]);
  dessinerCarte();
  $("fil-compte").textContent = `chemin de ${pistes.length} morceaux`;
  $("chemin-rejouer").hidden = !carte.refaire;
}

/// Signale un calcul en cours dans le pied de carte, ou l'efface.
///
/// Le premier chemin sonique ou errant construit le graphe des voisins : une
/// dizaine de secondes sur la bibliothèque entière. Sans ce mot, la carte
/// paraîtrait figée.
function patienter(texte) {
  $("carte-aide").textContent = texte ?? aideCourante();
}

window.addEventListener("resize", () => {
  if (!$("carte-vue").hidden) dessinerCarte();
});

/// Charge `carte.familles` (`[[cluster, nom, effectif]]`) au premier besoin —
/// mêmes données pour la légende d'Explorer et le filtre de l'Écoute.
///
/// Les noms viennent du moteur. Ni le genre le plus fréquent — « Rock » domine
/// six familles sur douze et ne les distinguerait pas — ni le plus
/// caractéristique, qui nommait « Ska Rock » une famille de 4 321 morceaux
/// menée par Bob Marley. Les deux à la fois : voir `nommer_les_familles`.
async function chargerFamilles() {
  if (carte.familles) return;
  try {
    carte.familles = await invoke("families");
  } catch (e) {
    remonter(e, "familles");
    carte.familles = [];
  }
}

/// Rendu commun de la légende des familles : une pastille teintée, un nom, un
/// effectif par famille. `estActive(cluster)` décide du filet d'accent,
/// `auClic(cluster)` réagit au clic. Explorer isole une famille sur la carte ;
/// l'Écoute coche/décoche une famille du filtre de la grille de pochettes.
function rendreFamilles(hote, estActive, auClic) {
  const teintes = couleursFamilles();
  hote.replaceChildren();
  for (const [c, nom, n] of carte.familles ?? []) {
    const el = document.createElement("button");
    el.className = "famille" + (estActive(c) ? " famille--isolee" : "");
    el.innerHTML = `<span class="famille__pastille"></span>
                    <span></span><span class="famille__n"></span>`;
    el.children[0].style.background = teintes[c % teintes.length] ?? "currentColor";
    // Une famille dont aucun genre ne ressort garde son numéro : mieux vaut un
    // nom neutre qu'un nom faux.
    el.children[1].textContent = nom || `famille ${c + 1}`;
    el.children[1].title = nom || "";
    el.children[2].textContent = n.toLocaleString("fr-FR");
    el.addEventListener("click", () => auClic(c));
    hote.appendChild(el);
  }
}

async function dessinerFamilles() {
  await chargerFamilles();
  rendreFamilles(
    $("familles"),
    (c) => carte.isolee === c,
    (c) => {
      carte.isolee = carte.isolee === c ? null : c;
      dessinerFamilles();
      dessinerCarte();
      majFiltreGL();
      // Le filtre par famille borne aussi le chemin : un chemin déjà tracé se
      // recalcule pour ne garder que la famille isolée (ou la relâcher).
      if (carte.refaire) rejouerChemin().catch((e) => remonter(e, "chemin"));
    },
  );
}

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
/// (`ORDER BY … COLLATE NOCASE`) — on le laisse tel quel. `annee` et `alea`
/// retrient une copie côté interface.
let triAlbums = "alpha";
// clé d'album → tirage aléatoire, régénéré à chaque clic sur « Aléatoire ».
// Passer par la clé (et non l'objet) garde l'ordre stable quand le filtre
// familles réduit la liste affichée.
let grainesAlea = new Map();

function rebrasserAlea(lignes) {
  grainesAlea = new Map();
  for (const a of lignes) grainesAlea.set(cleAlbum(a), Math.random());
}

function trierAlbums(lignes) {
  if (triAlbums === "annee") {
    return [...lignes].sort((a, b) => {
      const ya = a.year ?? -Infinity;
      const yb = b.year ?? -Infinity;
      if (ya !== yb) return yb - ya;
      return (a.name || "").localeCompare(b.name || "", "fr", { sensitivity: "base" });
    });
  }
  if (triAlbums === "alea") {
    return [...lignes].sort(
      (a, b) => (grainesAlea.get(cleAlbum(a)) ?? 0) - (grainesAlea.get(cleAlbum(b)) ?? 0),
    );
  }
  return lignes;
}

/// Les lignes de la vue courante — la grille d'albums peut être filtrée et
/// retriée, tout le reste (liste d'artistes, pistes d'un album, recherche)
/// passe tel quel.
function lignesCourantes() {
  return vue.quoi === "albums" ? trierAlbums(albumsAffiches()) : vue.lignes;
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
  $("fil-compte").textContent = `${lignesCourantes().length} albums`;
  construireIndexAlpha();
  grilleDernierRang = -1;
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
  famillesParAlbum = null;
  famillesParArtiste = null;
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

/// Sur le plan de ville réel, un bâtiment ne sait se colorer que par famille
/// (`majCouleurGL`) — les modes continus (année/tempo/énergie) resteraient
/// des boutons actifs sans aucun effet visible, une incohérence plutôt
/// qu'une limite honnête. Désactivés dans ce cas, avec un repli sur
/// « Famille » si l'un d'eux était choisi au moment de la bascule.
function majSegmentsCouleur() {
  const desactives = carte.affichage === "carte" && villeReelle;
  let bascule = false;
  document.querySelectorAll("[data-couleur]").forEach((b) => {
    const continu = b.dataset.couleur !== "famille";
    b.disabled = desactives && continu;
    b.title = b.disabled ? "Un bâtiment ne sait se colorer que par famille sur le plan de ville réel" : "";
    if (b.disabled && b.classList.contains("segment--actif")) bascule = true;
  });
  if (bascule) document.querySelector('[data-couleur="famille"]')?.click();
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

/* ------------------------------------------------------ fond de plan (thème) */

function majBoutonsTheme() {
  document
    .querySelectorAll("#carte-theme [data-theme]")
    .forEach((b) => b.classList.toggle("segment--actif", b.dataset.theme === carteTheme));
}
majBoutonsTheme();

document.querySelectorAll("#carte-theme [data-theme]").forEach((b) =>
  b.addEventListener("click", async () => {
    if (b.dataset.theme === carteTheme) return;
    carteTheme = b.dataset.theme;
    localStorage.setItem("carte-theme", carteTheme);
    majBoutonsTheme();
    // Pas d'instance MapLibre encore : `initialiserGL` relira `carteTheme`.
    if (!gl) return;
    try {
      const style = await invoke("style_carte", { theme: carteTheme });
      // `setStyle` diffe : mêmes sources, mêmes couches, mêmes ids — seules les
      // couleurs de peinture changent, donc pas de rechargement de tuile et la
      // caméra ne bouge pas.
      gl.setStyle(style);
      // Les mutations de style au runtime (couleur par famille, isolement,
      // visibilité des territoires) portaient sur l'ancien style : les rejouer
      // une fois le nouveau prêt. `setStyle` peut émettre plusieurs `styledata`
      // avant que le style soit complet — on attend `isStyleLoaded`.
      const rejouer = () => {
        if (!gl || !gl.isStyleLoaded()) return;
        gl.off("styledata", rejouer);
        majCouleurGL();
        majFiltreGL();
        majAffichageGL();
        poserVignetteCarte(gl.getStyle());
      };
      gl.on("styledata", rejouer);
      rejouer();
    } catch (e) {
      remonter(e, "thème de la carte");
    }
  }),
);

/// Les modes de chemin qui ont un sens dans chaque affichage.
///
/// L'itinéraire suit de vraies rues — rien à suivre sur un nuage t-SNE.
/// Sonique et errance sautent d'un voisin sonore au suivant sans égard pour
/// la géographie : sur la carte, ce sont des zigzags qui ne servent pas
/// l'exploration du plan, contrairement au nuage où c'est justement le
/// point.
const MODES_CHEMIN = {
  points: ["direct", "sonique", "errance", "dessine"],
  carte: ["direct", "dessine", "itineraire"],
};

/// Montre/cache les réglages propres à chaque mode de chemin. « morceaux » et
/// « bruit » disparaissent pour l'itinéraire (qui a ses propres réglages :
/// profil, durée) ; le bouton « Tracer » ne reste que hors plan de ville, où le
/// calcul musical est trop lent pour se déclencher tout seul.
function majReglagesChemin() {
  const estItin = carte.chemin === "itineraire";
  if ($("bloc-itineraire")) $("bloc-itineraire").hidden = !estItin;
  if ($("reglette-morceaux")) $("reglette-morceaux").hidden = estItin;
  if ($("bloc-bruit")) $("bloc-bruit").hidden = estItin;
  if ($("itin-tracer")) $("itin-tracer").hidden = estItin && carteReelle();
  if (estItin && $("chemin-rejouer")) $("chemin-rejouer").hidden = true;
}

/// Affiche les boutons de mode de chemin qui ont un sens dans l'affichage
/// courant, et bascule sur « direct » si le mode actif n'en fait plus
/// partie.
function majModesChemin() {
  const disponibles = MODES_CHEMIN[carte.affichage] || MODES_CHEMIN.points;
  document.querySelectorAll("[data-chemin]").forEach((b) => {
    b.hidden = !disponibles.includes(b.dataset.chemin);
  });
  if (!disponibles.includes(carte.chemin)) {
    poserModeChemin("direct");
  } else {
    majReglagesChemin();
  }
}

document.querySelectorAll("[data-affichage]").forEach((b) =>
  b.addEventListener("click", () => {
    carte.affichage = b.dataset.affichage;
    // Un tracé dessiné dans un repère (t-SNE / lon-lat) n'a plus de sens dans
    // l'autre : on repart propre plutôt que de rejouer des coordonnées
    // étrangères au repère courant.
    carte.refaire = null;
    carte.route = null;
    carte.routeTrace = null;
    majAffichageGL();
    majModesChemin();
    document
      .querySelectorAll("[data-affichage]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
    dessinerCarte();
  }),
);

/// Rappel du geste attendu, en pied de carte.
function aideCourante() {
  const [, court] = AIDE_CHEMIN[carte.chemin];
  return `molette : zoom · glisser : déplacer · clic : écouter · ${court} · alt+glisser : lasso`;
}

/// Montre ou cache la légende en dégradé, et y inscrit les bornes de la
/// variable active. Une variable dont aucun morceau ne porte la valeur — les
/// descripteurs avant leur passe — laisse la légende cachée plutôt que d'en
/// afficher une vide.
function majLegendeContinue() {
  const continu = CONTINUES[carte.couleur];
  const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 0];
  $("legende-continue").hidden = !continu || !(v1 > v0);
  if (continu && v1 > v0) {
    $("continue-min").textContent = continu.format(v0);
    $("continue-max").textContent = continu.format(v1);
  }
}

/// Affiche les bornes du chemin.
///
/// Elles étaient mémorisées sans être montrées : rien ne disait ce qui était
/// déjà choisi, ni comment le corriger autrement qu'en recliquant.
function dessinerBornes() {
  for (const [role, t] of [["depart", carte.depart], ["arrivee", carte.arrivee]]) {
    const el = $(`borne-${role}`);
    el.classList.toggle("borne--pose", !!t);
    el.querySelector(".borne__nom").textContent = t
      ? `${txt(t.artist, "?")} — ${txt(t.title, "?")}`
      : "—";
  }
  majDureeItin();
}

/// Une arrivée posée prime sur la durée : « va jusque-là » l'emporte. On grise
/// alors le curseur de durée pour que ce soit visible.
function majDureeItin() {
  const e = $("itin-minutes");
  if (!e) return;
  const inactif = !!carte.arrivee;
  e.disabled = inactif;
  const l = $("itin-minutes")?.closest(".reglette");
  if (l) l.style.opacity = inactif ? 0.4 : "";
  const out = $("itin-minutes-val");
  if (out) out.textContent = inactif ? "→ arrivée" : +e.value > 0 ? `${e.value} min` : "libre";
}

document.querySelectorAll("[data-borne]").forEach((b) =>
  b.addEventListener("click", () => {
    carte[b.dataset.borne] = null;
    carte.route = null;
    dessinerBornes();
    dessinerCarte();
    retracerItineraireSiPret();
  }),
);

/// Pose une borne et trace dès que les deux sont là.
///
/// L'errance n'a qu'une borne : elle part dès le départ posé.
async function poserBorne(t) {
  if (carte.chemin === "errance") {
    carte.depart = t;
    carte.arrivee = null;
    dessinerBornes();
    carte.graine = 1;
    await tracerChemin({ from: t.id, mode: "errance" });
    return;
  }
  if (!carte.depart) carte.depart = t;
  else carte.arrivee = t;
  dessinerBornes();
  if (carte.chemin === "itineraire") {
    // Sur le plan de ville, l'itinéraire se trace tout seul dès le départ posé
    // (l'arrivée est facultative si une durée est fixée) — comme `direct`.
    // Sans plan de ville, le calcul musical est trop lent pour ça : le bouton
    // « Tracer » reste visible et prend la main.
    if (carteReelle()) await tracerItineraire();
  } else if (carte.depart && carte.arrivee) {
    await tracerChemin({
      from: carte.depart.id,
      to: carte.arrivee.id,
      mode: carte.chemin,
    });
  }
}

function poserModeChemin(mode) {
  carte.chemin = mode;
  majReglagesChemin();
  carte.trace = null;
  // Change de mode sans changer de trajet : « Autre tirage » et le curseur
  // de bruit rejoueraient sinon la spécification de l'ancien mode sous le
  // nom du nouveau. Le tracé déjà à l'écran, lui, reste visible tel quel
  // jusqu'au prochain chemin calculé — seul le rejeu est désarmé.
  carte.refaire = null;
  document
    .querySelectorAll("[data-chemin]")
    .forEach((s) => s.classList.toggle("segment--actif", s.dataset.chemin === mode));
  $("chemin-aide").textContent =
    `${AIDE_CHEMIN[mode][0]} Ou : chercher puis Entrée pour poser une borne.`;
  $("carte-aide").textContent = aideCourante();
  // En itinéraire, la visibilité du bouton dépend des variantes — gérée par
  // `majReglagesChemin` ci-dessus.
  if (mode !== "itineraire") $("chemin-rejouer").hidden = !carte.refaire;
  dessinerBornes();
  dessinerCarte();

  // Sonique et errance passent par le graphe des voisins : on le prépare dès
  // le choix du mode, pour que le premier chemin ne paie pas la construction.
  //
  // **Le dire est indispensable.** Le calcul sature tous les cœurs ; muet, il
  // se lit comme un plantage, ventilateurs compris. On l'annonce donc dans le
  // rail et en pied de carte, et on efface dès que c'est prêt.
  if (mode === "sonique" || mode === "errance") {
    const attente = "Préparation du graphe des voisins…";
    $("chemin-aide").textContent = attente;
    patienter(attente);
    preparerGraphe().finally(() => {
      // Le mode a pu changer entre-temps : on réaffiche l'aide du mode
      // courant, pas celle de celui qui avait lancé la préparation.
      $("chemin-aide").textContent =
        `${AIDE_CHEMIN[carte.chemin][0]} Ou : chercher puis Entrée pour poser une borne.`;
      patienter();
    });
  }

  // Le départ (et l'arrivée) choisis restent d'un mode à l'autre — voir
  // `carte.depart`, jamais effacé ci-dessus. Changer de mode doit donc
  // aussitôt retracer avec ces bornes-là, sans obliger à recliquer : c'est
  // la trajectoire qui change, pas le point d'où l'on veut explorer.
  if (mode === "errance") {
    if (carte.depart) tracerChemin({ from: carte.depart.id, mode: "errance" }).catch((e) => remonter(e, "chemin"));
  } else if (mode === "itineraire") {
    if (carte.depart && carteReelle()) tracerItineraire().catch((e) => remonter(e, "itinéraire"));
  } else if (mode !== "dessine" && carte.depart && carte.arrivee) {
    tracerChemin({ from: carte.depart.id, to: carte.arrivee.id, mode }).catch((e) => remonter(e, "chemin"));
  }
}

document
  .querySelectorAll("[data-chemin]")
  .forEach((b) => b.addEventListener("click", () => poserModeChemin(b.dataset.chemin)));

$("chemin-rejouer").addEventListener("click", async () => {
  if (!carte.refaire) return;
  carte.graine += 1;
  await rejouerChemin();
});

/// Bruit commun aux quatre modes de chemin — un curseur du rail, toujours
/// visible en Explorer. 0 : trajet exact. Plus haut : dérive davantage, dans
/// le registre propre à chaque mode (voir la note en tête de `chemin.rs`
/// côté moteur). Mémorisé en local : ce n'est pas une donnée de la
/// bibliothèque, juste une préférence de cette installation.
const BRUIT_DEFAUT = 0.3;
let bruitChemin = Number(localStorage.getItem("bruit-chemin")) || BRUIT_DEFAUT;
$("bruit-chemin").value = bruitChemin;
$("bruit-valeur").textContent = bruitChemin.toFixed(2).replace(".", ",");

// Un délai court, pas un aller-retour à chaque cran du curseur : on laisse
// le geste se terminer avant de relancer le calcul et la lecture, sinon
// glisser le curseur bombarderait le moteur de dizaines de chemins.
let attenteBruit = null;
$("bruit-chemin").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  bruitChemin = Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : BRUIT_DEFAUT;
  $("bruit-valeur").textContent = bruitChemin.toFixed(2).replace(".", ",");
  localStorage.setItem("bruit-chemin", String(bruitChemin));

  // Un chemin est déjà tracé : on le rejoue au nouveau bruit, à graine
  // égale — seul le paramètre qu'on ajuste doit bouger, pas aussi le
  // tirage, sans quoi l'effet du curseur se confondrait avec celui d'un
  // nouveau tirage.
  if (!carte.refaire) return;
  clearTimeout(attenteBruit);
  attenteBruit = setTimeout(() => {
    rejouerChemin().catch((e) => remonter(e, "réglage du bruit"));
  }, 250);
});

/// Construit le graphe des voisins en tâche de fond, une seule fois à la
/// fois. Nu : ne dit rien de l'attente, c'est à l'appelant de le faire dans
/// son propre contexte — `poserModeChemin` pour la carte, `poser` pour la
/// grille d'albums.
///
/// Le balayage est complet : une vingtaine de secondes sur la bibliothèque
/// entière, davantage à mesure que l'analyse la remplit. Rien n'attend le
/// résultat — la commande met le graphe en cache côté moteur, les appels
/// suivants (`path` en mode sonique/errance, `path_album`) le trouvent prêt.
let graphePret = null;
function preparerGraphe() {
  if (graphePret) return graphePret;
  graphePret = invoke("prepare_graph")
    .catch((e) => remonter(e, "préparation du graphe"))
    .finally(() => {
      graphePret = null;
    });
  return graphePret;
}

async function chargerCarte() {
  const [points] = await Promise.all([
    invoke("map_view"),
  ]);
  carte.points = points;

  // Plan de ville réel : la vraie adresse (lon/lat) de chaque morceau logé,
  // **à côté** de `p.x`/`p.y` — jamais à la place. `p.x`/`p.y` restent la
  // position t-SNE partout : c'est ce que lit le mode Nuage, qui doit rester
  // correct que la ville soit active ou non. `versEcran` choisit laquelle
  // utiliser selon `villeReelle` et l'affichage courant. `positions_carte`
  // échoue sur le chemin fictif : c'est le signal, pas une erreur à
  // remonter.
  let positionsReelles = null;
  try {
    positionsReelles = await invoke("positions_carte");
  } catch {
    positionsReelles = null;
  }
  villeReelle = !!(positionsReelles && Object.keys(positionsReelles).length > 0);
  carte.positionsReelles = villeReelle ? new Map(Object.entries(positionsReelles).map(([id, p]) => [+id, p])) : new Map();
  majSegmentsCouleur();
  if (villeReelle) {
    const sansAdresse = carte.points.filter((p) => !carte.positionsReelles.has(p.id)).length;
    if (sansAdresse) {
      // Un morceau que l'affectation n'a pas logé n'a pas d'entrée ici :
      // `versEcran` retombe alors sur sa position t-SNE plutôt que de le
      // faire disparaître.
      journalCarte(`${sansAdresse} morceau(x) sans adresse réelle, position t-SNE conservée`, "warn");
    }
  }

  const [faits, total] = await invoke("map_progress");

  // Les bornes de chaque variable, une fois pour toutes au chargement.
  for (const [cle, { champ, valide }] of Object.entries(CONTINUES)) {
    const vs = carte.points
      .map((p) => p[champ])
      .filter((v) => v != null && (!valide || valide(v)));
    carte.bornes[cle] = vs.length ? [Math.min(...vs), Math.max(...vs)] : [0, 0];
  }
  majLegendeContinue();
  $("fil-titre").textContent = "Carte";
  $("fil-compte").textContent =
    faits < total
      ? `${carte.points.length.toLocaleString("fr-FR")} placés · ${(total - faits).toLocaleString("fr-FR")} en attente`
      : `${carte.points.length.toLocaleString("fr-FR")} morceaux`;
  dessinerFamilles();
  majAffichageGL();
  majModesChemin();
}

/* ---------------------------------------------------------- modes */

async function basculerMode(mode) {
  const explorer = mode === "explorer";
  const editer = mode === "editer";
  const bibliotheque = mode === "bibliotheque";
  const decouvrir = mode === "decouvrir";
  // Quitter Découvrir éteint les pastilles « nouveau » : ce qu'on vient de
  // voir n'est plus une nouveauté au prochain passage.
  if (modeCourant === "decouvrir" && mode !== "decouvrir") {
    invoke("decouvrir_tout_vu").catch((e) => remonter(e, "découvrir"));
  }
  document.querySelectorAll(".mode").forEach((b) =>
    b.classList.toggle("mode--actif", b.dataset.mode === mode),
  );
  modeCourant = mode;
  // Entrer dans l'éditeur avec des stems déjà affichés doit les rendre
  // audibles : c'est ce qu'on vient y faire.
  if (editer) prendreLaMain().catch((e) => remonter(e, "stems"));
  $("carte-vue").hidden = !explorer;
  $("bibliotheque-vue").hidden = !bibliotheque;
  $("decouvrir-vue").hidden = !decouvrir;
  $("liste").hidden = explorer || bibliotheque || decouvrir || vueEnGrille();
  $("grille").hidden = explorer || bibliotheque || decouvrir || !vueEnGrille();
  // L'ordre de la grille d'albums n'existe qu'en Écoute : `poser` le rétablit
  // en y revenant, mais ne court pas pour les autres modes.
  $("tri-albums").hidden = mode !== "ecoute" || vue.quoi !== "albums";
  $("retour").hidden = explorer || bibliotheque || decouvrir || vue.retour === null;
  $("index-alpha").hidden = $("index-alpha").hidden || bibliotheque || decouvrir;
  $("bloc-vue-lib").hidden = explorer || editer || bibliotheque || decouvrir;
  // « Chercher » (recherche globale de la bibliothèque) alimente #liste, masquée
  // en Bibliothèque et Découvrir : le champ n'y ferait rien de visible. En
  // Explorer il change de rôle (filtre de la carte), on le garde.
  $("bloc-chercher").hidden = bibliotheque || decouvrir;
  $("bloc-colorer").hidden = !explorer;
  $("bloc-chemin").hidden = !explorer;
  $("bloc-familles").hidden = !explorer || carte.couleur !== "famille";
  // Le filtre par famille de l'Écoute : `majBlocFamillesEcoute` le rallume si
  // la grille d'albums est à l'écran et la carte calculée.
  $("bloc-familles-ecoute").hidden = true;
  // Idem pour le filtre par famille de Découvrir (`majBlocFamillesDecouvrir`).
  if (!decouvrir) $("bloc-familles-decouvrir").hidden = true;
  $("bloc-demix").hidden = !editer;
  $("bloc-decouvrir").hidden = !decouvrir;
  $("dock").hidden = !editer || edition.stems.length === 0;

  // Sortir de l'édition rend la sortie au lecteur ordinaire : garder les
  // stems chargés tiendrait 186 Mo et une sortie audio pour rien.
  if (!editer && edition.enLecture) await arreterStems();

  if (explorer) {
    poserModeChemin(carte.chemin);
    await chargerCarte();
    // Rattrape une file lancée depuis l'Écoute avant la première visite
    // d'Explorer : la carte n'avait alors pas encore ses points pour y
    // tracer quoi que ce soit.
    tracerRouteSurCarte(fileCourante);
  } else if (bibliotheque) {
    $("fil-titre").textContent = "Bibliothèque";
    $("fil-compte").textContent = "";
    await dessinerRacines();
    majCache().catch((e) => remonter(e, "cache"));
    chargerDossierDonnees().catch((e) => remonter(e, "dossier de données"));
    chargerStatsBibliotheque().catch((e) => remonter(e, "statistiques"));
    reprendreActualisationEnCours().catch((e) => remonter(e, "actualisation"));
    chargerVerifications().catch((e) => remonter(e, "vérifications"));
    chargerParametresCarte().catch((e) => remonter(e, "paramètres de la carte"));
    chargerVocabulaireFamilles().catch((e) => remonter(e, "vocabulaire des familles"));
    chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));
  } else if (decouvrir) {
    $("fil-titre").textContent = "Découvrir";
    $("fil-compte").textContent = "";
    chargerArtistesDecouvrir().catch((e) => remonter(e, "découvrir"));
    entrerDecouvrir().catch((e) => remonter(e, "découvrir"));
    // Filtre par famille du fil : les données au premier passage, le rendu à
    // chaque entrée (les familles ont pu être renommées depuis).
    {
      const rendre = () => {
        dessinerFamillesDecouvrir();
        rendreFilDecouvrir();
      };
      if (famillesParArtiste) rendre();
      else
        chargerFamillesParArtiste()
          .then(rendre)
          .catch((e) => remonter(e, "familles des artistes"));
    }
  } else {
    poser(vue.quoi, vue.titre, vue.lignes, vue.retour);
    // Le mode Éditer travaille sur la sélection courante : on la relit à
    // chaque entrée plutôt que de la mémoriser, elle a pu changer depuis.
    if (editer) poserSourceEdition();
    // Filtre par famille de l'Écoute : les données au premier passage, le
    // rendu à chaque entrée (les familles ont pu être renommées depuis).
    if (mode === "ecoute") {
      const rendre = () => {
        dessinerFamillesEcoute();
        rafraichirGrille();
      };
      if (famillesParAlbum) rendre();
      else chargerFamillesParAlbum().then(rendre).catch((e) => remonter(e, "familles des albums"));
    }
  }
}

document.querySelectorAll(".mode").forEach((b) => {
  if (!b.disabled) b.addEventListener("click", () => basculerMode(b.dataset.mode));
});

/* ------------------------------------------------------ mode Découvrir */

/// Tous les artistes de la bibliothèque, chargés une fois à l'entrée dans
/// le mode et filtrés côté client à chaque frappe — un millier de lignes,
/// pas de quoi justifier une requête par lettre tapée. Seuls ceux qui ont
/// un identifiant MusicBrainz peuvent être cherchés : c'est la clé que
/// l'API demande, rien à faire sans elle.
let artistesDecouvrables = [];

async function chargerArtistesDecouvrir() {
  if (artistesDecouvrables.length === 0) {
    artistesDecouvrables = (await invoke("artists")).filter((a) => a.mbid);
  }
}

/// L'adresse de contact MusicBrainz : une seule valeur, exigée dans le
/// User-Agent pour toute requête à leur API. Deux fonctions s'en servent —
/// les collaborations d'artiste (ce mode) et la passe « Genres MusicBrainz »
/// du mode Bibliothèque — d'où deux champs, tenus synchronisés sur une même
/// clé de stockage.
const CONTACT_MB_CLE = "mb-contact";
const CHAMPS_CONTACT_MB = ["decouvrir-contact", "analyse-contact"];

function contactMb() {
  return (localStorage.getItem(CONTACT_MB_CLE) || "").trim();
}

(function initContactMb() {
  // Reprise de l'ancienne clé, du temps où seul Découvrir gardait l'adresse
  // (et où le champ de Bibliothèque n'était pas persisté du tout).
  const ancien = localStorage.getItem("decouvrir-contact");
  if (ancien && !localStorage.getItem(CONTACT_MB_CLE)) {
    localStorage.setItem(CONTACT_MB_CLE, ancien.trim());
  }
  localStorage.removeItem("decouvrir-contact");

  const valeur = contactMb();
  for (const id of CHAMPS_CONTACT_MB) {
    const champ = $(id);
    if (!champ) continue;
    champ.value = valeur;
    champ.addEventListener("change", (e) => {
      const v = e.target.value.trim();
      localStorage.setItem(CONTACT_MB_CLE, v);
      for (const autre of CHAMPS_CONTACT_MB) {
        if (autre === id) continue;
        const c = $(autre);
        if (c) c.value = v;
      }
    });
  }
})();

$("decouvrir-recherche").addEventListener("input", (e) => {
  const q = e.target.value.trim().toLowerCase();
  if (!q) {
    dessinerListeVerif("decouvrir-suggestions", []);
    return;
  }
  const trouves = artistesDecouvrables
    .filter((a) => a.name.toLowerCase().includes(q))
    .slice(0, 20);
  dessinerListeVerif(
    "decouvrir-suggestions",
    trouves.map((a) => ({
      ligne: a.name,
      onClick: () => {
        $("decouvrir-recherche").value = "";
        dessinerListeVerif("decouvrir-suggestions", []);
        decouvrirFil = [];
        naviguerDecouvrir(a.mbid, a.name).catch((e) => remonter(e, "découvrir"));
      },
    })),
  );
});

/// La pile des artistes visités, du premier au centre courant — le fil
/// d'Ariane. Vidée à chaque nouvelle recherche : on ne mélange pas deux
/// explorations.
let decouvrirFil = [];

/// Affiche les collaborateurs de `mbid` au centre, sans toucher au fil —
/// c'est [`naviguerDecouvrir`] et le fil d'Ariane qui décident quand y
/// ajouter ou y revenir.
async function afficherArtisteDecouvrir(mbid, nom) {
  $("decouvrir-vide").hidden = true;
  $("decouvrir-centre").hidden = false;
  $("decouvrir-nom").textContent = nom;
  dessinerListeVerif("decouvrir-liens", []);
  $("decouvrir-etat").textContent = "Recherche des collaborations…";

  let liens;
  try {
    liens = await invoke("artist_links", { mbid, contact: decouvrirContact() });
  } catch (e) {
    $("decouvrir-etat").textContent = String(e);
    remonter(e, "découvrir");
    return;
  }
  $("decouvrir-etat").textContent = liens.length
    ? ""
    : "Aucune collaboration connue de MusicBrainz pour cet artiste.";

  dessinerListeVerif(
    "decouvrir-liens",
    liens.map(([dstMbid, dstNom, relation]) => {
      const item = {
        ligne: dstNom,
        detail: relation,
        onClick: () =>
          naviguerDecouvrir(dstMbid, dstNom).catch((e) => remonter(e, "découvrir")),
      };
      // Un collaborateur déjà dans la bibliothèque mène à ses albums, en
      // plus de pouvoir recentrer dessus — deux gestes, deux cibles : le
      // bouton doit donc arrêter le clic avant qu'il ne remonte à la ligne.
      const local = artistesDecouvrables.find((a) => a.mbid === dstMbid);
      if (local) {
        const bouton = document.createElement("button");
        bouton.className = "verif__albums";
        bouton.textContent = "Albums";
        bouton.addEventListener("click", async (e) => {
          e.stopPropagation();
          const albums = await invoke("albums", { artist: local.name, mbid: local.mbid });
          if (modeCourant !== "ecoute") await basculerMode("ecoute");
          poser("albums", local.name, albums, sommet);
        });
        item.action = bouton;
      }
      return item;
    }),
  );
}

function decouvrirContact() {
  return contactMb();
}

/// Pousse `mbid` sur le fil d'Ariane et l'affiche au centre — le geste de
/// navigation normal, en cliquant un collaborateur.
async function naviguerDecouvrir(mbid, nom) {
  decouvrirFil.push({ mbid, nom });
  dessinerFilDecouvrir();
  await afficherArtisteDecouvrir(mbid, nom);
}

function dessinerFilDecouvrir() {
  const hote = $("decouvrir-fil");
  hote.replaceChildren();
  hote.hidden = decouvrirFil.length <= 1;
  decouvrirFil.forEach((a, i) => {
    if (i > 0) hote.appendChild(document.createTextNode(" › "));
    if (i === decouvrirFil.length - 1) {
      const span = document.createElement("span");
      span.style.color = "var(--txt)";
      span.textContent = a.nom;
      hote.appendChild(span);
    } else {
      const bouton = document.createElement("button");
      bouton.textContent = a.nom;
      bouton.addEventListener("click", async () => {
        // Revenir en arrière tronque le fil plutôt que d'y repousser une
        // entrée déjà là — sinon un aller-retour répété l'allongerait sans
        // fin pour les mêmes artistes.
        decouvrirFil = decouvrirFil.slice(0, i + 1);
        dessinerFilDecouvrir();
        await afficherArtisteDecouvrir(a.mbid, a.nom);
      });
      hote.appendChild(bouton);
    }
  });
}

/* ---------------------------------------- mode Découvrir : le fil d'actualité */

/// Au bout de combien d'heures sans passe on en relance une à l'ouverture du
/// mode. Douze heures : assez pour ne pas interroger MusicBrainz à chaque
/// aller-retour, assez peu pour que « récent » le reste.
const DECOUVRIR_PEREMPTION_H = 12;

/// Vrai tant qu'une passe tourne, pour ne pas en lancer deux.
let decouvrirEnCours = false;

/// La date (epoch s) de la dernière passe, retenue du dernier `decouvrir_feed`.
let decouvrirDernierePasse = null;

/// Un bouton stylé comme un lien, avec son action.
function boutonLien(texte, action) {
  const b = document.createElement("button");
  b.className = "lien";
  b.textContent = texte;
  b.addEventListener("click", action);
  return b;
}

/// L'onglet ouvert dans le panneau central — mémorisé d'une visite à l'autre.
let decouvrirOnglet = "sorties";
try {
  decouvrirOnglet = localStorage.getItem("decouvrir-onglet") || "sorties";
} catch {
  /* stockage indisponible : on garde le défaut */
}

/// Pochettes du fil, par identifiant de release-group. Bornées de fait par le
/// nombre de sorties d'une passe (~60), pas d'éviction à prévoir. `null` =
/// pas de pochette connue, pour ne pas la redemander à chaque rendu.
const pochettesCaa = new Map();
function pochetteDecouvrir(rgMbid) {
  let p = pochettesCaa.get(rgMbid);
  if (!p) {
    p = invoke("decouvrir_pochette", { rgMbid }).catch(() => null);
    pochettesCaa.set(rgMbid, p);
  }
  return p;
}

/// URL d'une page Last.fm — les espaces en `+`, le reste encodé. Un album
/// inconnu y tombe sur une page « introuvable » avec une recherche : acceptable.
function lienLastfm(...segments) {
  const enc = (s) => encodeURIComponent(s).replace(/%20/g, "+");
  return "https://www.last.fm/music/" + segments.map(enc).join("/");
}

/// Entrée dans le mode : on affiche le fil tel qu'il est, puis on lance une
/// passe si la dernière est ancienne (et si une adresse de contact est là).
async function entrerDecouvrir() {
  await chargerFilDecouvrir();
  const passe = decouvrirDernierePasse;
  const vieux =
    passe === null || Date.now() / 1000 - passe > DECOUVRIR_PEREMPTION_H * 3600;
  if (vieux && contactMb().includes("@")) {
    await lancerPasseDecouvrir();
  }
}

async function chargerFilDecouvrir() {
  const fil = await invoke("decouvrir_feed");
  filDecouvrir = fil;
  decouvrirDernierePasse = fil.derniere_passe ?? null;

  $("decouvrir-fraicheur").textContent = fil.derniere_passe
    ? `Actualisé ${depuisTexte(fil.derniere_passe)}.`
    : "Jamais actualisé.";

  rendreFilDecouvrir();
}

/// Le dernier fil reçu de `decouvrir_feed`, gardé pour le re-rendre à chaque
/// changement du filtre par famille sans refaire la requête.
let filDecouvrir = null;

/// La famille sonique de l'artiste-ancre d'une sortie — celle inscrite dans le
/// fil ne bouge pas, seul le filtre change. Ensemble vide, ou familles pas
/// encore chargées : tout passe.
function sortiePasseFamille(s) {
  if (filtreFamillesDecouvrir.size === 0 || !famillesParArtiste) return true;
  return filtreFamillesDecouvrir.has(famillesParArtiste.get(s.artiste_mbid));
}

/// Un voisin passe si l'une de ses ancres (`src_mbids`) est dans une famille
/// cochée — il a pu être proposé par plusieurs artistes de familles différentes.
function voisinPasseFamille(v) {
  if (filtreFamillesDecouvrir.size === 0 || !famillesParArtiste) return true;
  return (v.src_mbids ?? []).some((m) => filtreFamillesDecouvrir.has(famillesParArtiste.get(m)));
}

/// Applique le filtre par famille au fil courant et redessine les trois
/// onglets. Les compteurs suivent le filtre ; « Pas encore de nouveautés » et
/// « Tout marquer comme vu » restent calés sur le fil brut — ce qui est masqué
/// par un filtre reste du contenu.
function rendreFilDecouvrir() {
  const fil = filDecouvrir;
  if (!fil) return;

  const sorties = fil.sorties.filter(sortiePasseFamille);
  const collaborations = fil.collaborations.filter(sortiePasseFamille);
  const voisins = fil.voisins.filter(voisinPasseFamille);

  rendreListeSorties("decouvrir-sorties", "decouvrir-vide-sorties", sorties);
  rendreListeSorties("decouvrir-collabs", "decouvrir-vide-collabs", collaborations);
  rendreListeVoisins(voisins);

  const n = (id, v) => ($(id).textContent = v ? ` ${v}` : "");
  n("decouvrir-n-sorties", sorties.length);
  n("decouvrir-n-collabs", collaborations.length);
  n("decouvrir-n-voisins", voisins.length);

  const vide =
    fil.sorties.length === 0 && fil.collaborations.length === 0 && fil.voisins.length === 0;
  $("decouvrir-actus-vide").hidden = !vide;
  $("decouvrir-onglets").hidden = vide;
  poserOngletDecouvrir(decouvrirOnglet);

  const nouveaux =
    [...fil.sorties, ...fil.collaborations, ...fil.voisins].some((x) => !x.vu);
  $("decouvrir-vu-tout").hidden = !nouveaux;

  majBlocFamillesDecouvrir();
}

/* ------------------------------------------ filtre par famille — mode Découvrir
 *
 * Même légende que le mode Explorer (`rendreFamilles`), même multi-sélection
 * que le filtre de l'Écoute. Une sortie, une collaboration ou un voisin est
 * rangé dans la famille sonique de son artiste-ancre (celui de la
 * bibliothèque). Ensemble vide = tout est montré. N'a de sens qu'une fois la
 * carte calculée.
 */

const filtreFamillesDecouvrir = new Set();
let famillesParArtiste = null; // Map mb_album_artist_id → cluster dominant

async function chargerFamillesParArtiste() {
  try {
    const paires = await invoke("artist_families");
    famillesParArtiste = new Map(paires);
  } catch (e) {
    remonter(e, "familles des artistes");
    famillesParArtiste = new Map();
  }
}

async function dessinerFamillesDecouvrir() {
  await chargerFamilles();
  rendreFamilles(
    $("familles-decouvrir"),
    (c) => filtreFamillesDecouvrir.has(c),
    (c) => {
      if (filtreFamillesDecouvrir.has(c)) filtreFamillesDecouvrir.delete(c);
      else filtreFamillesDecouvrir.add(c);
      dessinerFamillesDecouvrir();
      rendreFilDecouvrir();
    },
  );
  $("familles-decouvrir-tout").hidden = filtreFamillesDecouvrir.size === 0;
}

/// Affiche le bloc « Familles » de Découvrir quand il a un sens : mode
/// Découvrir, familles des artistes chargées, et carte déjà calculée.
function majBlocFamillesDecouvrir() {
  const utile =
    modeCourant === "decouvrir" &&
    famillesParArtiste &&
    famillesParArtiste.size > 0 &&
    (carte.familles?.length ?? 0) > 0;
  $("bloc-familles-decouvrir").hidden = !utile;
}

$("familles-decouvrir-tout").addEventListener("click", () => {
  filtreFamillesDecouvrir.clear();
  dessinerFamillesDecouvrir();
  rendreFilDecouvrir();
});

function poserOngletDecouvrir(onglet) {
  decouvrirOnglet = onglet;
  try {
    localStorage.setItem("decouvrir-onglet", onglet);
  } catch {
    /* rien : la préférence ne survivra pas à la session, sans plus */
  }
  for (const b of $("decouvrir-onglets").children) {
    b.classList.toggle("segment--actif", b.dataset.onglet === onglet);
  }
  $("decouvrir-panneau-sorties").hidden = onglet !== "sorties";
  $("decouvrir-panneau-collabs").hidden = onglet !== "collabs";
  $("decouvrir-panneau-voisins").hidden = onglet !== "voisins";
}

$("decouvrir-onglets").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-onglet]");
  if (b) poserOngletDecouvrir(b.dataset.onglet);
});

/// « il y a 3 jours », « il y a 5 h », « à l'instant » — à partir d'un epoch s.
function depuisTexte(epochS) {
  const s = Math.max(0, Math.round(Date.now() / 1000 - epochS));
  if (s < 3600) return "il y a moins d'une heure";
  if (s < 86400) return `il y a ${Math.round(s / 3600)} h`;
  const j = Math.round(s / 86400);
  return j <= 1 ? "hier" : `il y a ${j} jours`;
}

/// Badge de type + « il y a N jours » à partir d'une date 'YYYY-MM-DD' partielle.
function ageSortie(date) {
  if (!date) return "";
  const complet = date.length === 4 ? `${date}-01-01` : date.length === 7 ? `${date}-01` : date;
  const j = Math.round((Date.now() - new Date(complet).getTime()) / 86400000);
  if (!Number.isFinite(j)) return date;
  if (j <= 0) return "aujourd'hui";
  if (j === 1) return "hier";
  if (j < 31) return `il y a ${j} jours`;
  return date;
}

/// Ouvre l'explorateur de collaborations sur un artiste, depuis le fil.
function explorerDepuisFil(mbid, nom) {
  decouvrirFil = [];
  naviguerDecouvrir(mbid, nom).catch((e) => remonter(e, "découvrir"));
}

/// Une ligne du fil : pochette (facultative, chargée à la volée), un corps
/// texte, une colonne de liens. Compacte — plusieurs par écran.
function ligneDecouvrir({ nouveau, rgMbid, titre, sous, collab, liens }) {
  const ligne = document.createElement("div");
  ligne.className = "decouvrir-ligne" + (nouveau ? " decouvrir-ligne--nouveau" : "");

  if (rgMbid !== undefined) {
    const pochette = document.createElement("div");
    pochette.className = "decouvrir-ligne__pochette";
    ligne.appendChild(pochette);
    pochetteDecouvrir(rgMbid).then((uri) => {
      if (!uri) return;
      const img = document.createElement("img");
      img.src = uri;
      img.alt = "";
      pochette.appendChild(img);
      pochette.classList.add("decouvrir-ligne__pochette--pleine");
    });
  }

  const corps = document.createElement("div");
  corps.className = "decouvrir-ligne__corps";
  const t = document.createElement("div");
  t.className = "decouvrir-ligne__titre";
  t.textContent = titre;
  t.title = titre;
  corps.appendChild(t);
  corps.appendChild(sous);
  if (collab) {
    const c = document.createElement("div");
    c.className = "decouvrir-ligne__collab";
    c.textContent = `avec ${collab}`;
    c.title = collab;
    corps.appendChild(c);
  }
  ligne.appendChild(corps);

  const col = document.createElement("div");
  col.className = "decouvrir-ligne__liens";
  for (const l of liens) col.appendChild(l);
  ligne.appendChild(col);

  return ligne;
}

function rendreListeSorties(idListe, idVide, sorties) {
  const hote = $(idListe);
  hote.replaceChildren();
  $(idVide).hidden = sorties.length > 0;
  for (const s of sorties) {
    const sous = document.createElement("div");
    sous.className = "decouvrir-ligne__sous";
    const artiste = boutonLien(s.artiste_nom, () =>
      explorerDepuisFil(s.artiste_mbid, s.artiste_nom),
    );
    artiste.classList.add("lien--inline");
    sous.append(artiste, document.createTextNode(
      [s.type_primaire, ageSortie(s.date_sortie)].filter(Boolean).map((x) => ` · ${x}`).join(""),
    ));

    hote.appendChild(
      ligneDecouvrir({
        nouveau: !s.vu,
        rgMbid: s.rg_mbid,
        titre: s.titre,
        sous,
        collab: s.collaborateurs,
        liens: [
          boutonLien("MusicBrainz", () =>
            ouvrirLien(`https://musicbrainz.org/release-group/${s.rg_mbid}`),
          ),
          boutonLien("Last.fm", () => ouvrirLien(lienLastfm(s.artiste_nom, s.titre))),
        ],
      }),
    );
  }
}

function rendreListeVoisins(voisins) {
  const hote = $("decouvrir-voisins");
  hote.replaceChildren();
  $("decouvrir-vide-voisins").hidden = voisins.length > 0;
  for (const v of voisins) {
    const sous = document.createElement("div");
    sous.className = "decouvrir-ligne__sous";
    sous.textContent = v.portes.length
      ? `proche de ${v.portes.slice(0, 3).join(", ")}`
      : "artiste voisin";

    hote.appendChild(
      ligneDecouvrir({
        nouveau: !v.vu,
        titre: v.dst_nom,
        sous,
        liens: [
          boutonLien("MusicBrainz", () =>
            ouvrirLien(`https://musicbrainz.org/artist/${v.dst_mbid}`),
          ),
          boutonLien("Explorer ▸", () => explorerDepuisFil(v.dst_mbid, v.dst_nom)),
        ],
      }),
    );
  }
}

/// Lance la passe et suit son avancement (barre + texte), puis rafraîchit le fil.
async function lancerPasseDecouvrir() {
  if (decouvrirEnCours) return;
  const contact = contactMb();
  if (!contact.includes("@")) {
    $("decouvrir-fraicheur").textContent =
      "Renseignez une adresse de contact MusicBrainz dans le rail.";
    return;
  }
  decouvrirEnCours = true;
  $("decouvrir-actualiser").disabled = true;
  const ligne = $("decouvrir-avancement");
  const jauge = $("decouvrir-jauge");
  ligne.hidden = false;
  jauge.hidden = false;
  jauge.removeAttribute("value"); // barre indéterminée tant qu'on n'a pas de total
  ligne.textContent = "Actualisation… (recherche des sorties récentes)";
  try {
    await invoke("start_decouvrir", { contact });
    await attendreFin("decouvrir_state", 1500, (d) => {
      if (!d.en_cours) return;
      if (d.total) {
        jauge.max = d.total;
        jauge.value = d.artistes;
        const reste = Math.max(0, d.total - d.artistes);
        ligne.textContent =
          `${d.artistes.toLocaleString("fr-FR")} / ${d.total.toLocaleString("fr-FR")} étapes` +
          ` — ${d.sorties_neuves} sorties, ${d.voisins_neufs} voisins` +
          (reste ? ` — reste ~${dureeLongue(reste * SECONDES_PAR_ARTISTE)}` : "");
      }
    });
  } catch (e) {
    ligne.textContent = String(e);
    remonter(e, "découvrir");
  } finally {
    decouvrirEnCours = false;
    $("decouvrir-actualiser").disabled = false;
    ligne.hidden = true;
    jauge.hidden = true;
    jauge.value = 0;
    await chargerFilDecouvrir().catch((e) => remonter(e, "découvrir"));
    const r = (await invoke("decouvrir_state").catch(() => null))?.resultat;
    if (r && r.startsWith("échec")) $("decouvrir-fraicheur").textContent = r;
  }
}

$("decouvrir-actualiser").addEventListener("click", () =>
  lancerPasseDecouvrir().catch((e) => remonter(e, "découvrir")),
);

$("decouvrir-vu-tout").addEventListener("click", async () => {
  await invoke("decouvrir_tout_vu").catch((e) => remonter(e, "découvrir"));
  await chargerFilDecouvrir().catch((e) => remonter(e, "découvrir"));
});

/* --------------------------------------------------- mode Bibliothèque */

/// Trois requêtes d'agrégation, rejouées à chaque entrée dans le mode — pas
/// de sondage : `library_stats` lit la base telle qu'elle est, elle ne
/// mesure rien.
async function chargerStatsBibliotheque() {
  const s = await invoke("library_stats");
  $("stats-total").textContent = `${s.total.toLocaleString("fr-FR")} morceaux.`;
  dessinerBarresGenres(s.genres);
  dessinerHistogramme("stats-tempo", s.tempo, (i) => `${s.tempo.min + i * s.tempo.pas}–${s.tempo.min + (i + 1) * s.tempo.pas} BPM`);
  dessinerAxe("stats-tempo-axe", s.tempo, (borne) => `${borne}`);
  $("stats-tempo-note").textContent = notesHistogramme(s.tempo, "mesurés");
  // `horloge`, pas `duree` : cette dernière rend « — » pour 0, pertinent pour
  // une piste de durée inconnue mais pas pour la borne d'une tranche.
  dessinerHistogramme("stats-durees", s.durees, (i) => horloge(s.durees.min + i * s.durees.pas));
  dessinerAxe("stats-durees-axe", s.durees, (borne) => horloge(borne));
  $("stats-durees-note").textContent = notesHistogramme(s.durees, "chronométrés");

  dessinerBarresCodecs(s.codecs);
  dessinerHistogramme("stats-bitrate", s.bitrate, (i) => `${s.bitrate.min + i * s.bitrate.pas}–${s.bitrate.min + (i + 1) * s.bitrate.pas} kb/s`);
  dessinerAxe("stats-bitrate-axe", s.bitrate, (borne) => `${borne}`);
  const bouts = [];
  // Au-delà de la dernière tranche, ce n'est pas une anomalie comme pour le
  // tempo ou la durée : plus le débit est haut, meilleure est la qualité —
  // et les formats sans perte (FLAC…) y vivent en permanence.
  if (s.bitrate.hors_gamme) {
    bouts.push(`${s.bitrate.hors_gamme.toLocaleString("fr-FR")} à ${s.bitrate.min + s.bitrate.comptes.length * s.bitrate.pas} kb/s ou plus, formats sans perte compris`);
  }
  if (s.bitrate.sans_valeur) {
    bouts.push(`${s.bitrate.sans_valeur.toLocaleString("fr-FR")} sans débit mesuré`);
  }
  $("stats-bitrate-note").textContent = bouts.join(" · ");

  dessinerBarres("stats-humeur", s.humeur, ["non mesuré"]);

  const completude = $("stats-completude");
  completude.replaceChildren();
  for (const [libelle, n] of [
    ["Sans MusicBrainz", s.sans_mbid],
    ["Sans genre identifié", s.genres.find(([g]) => g === "—")?.[1] ?? 0],
    ["Sans tempo mesuré", s.tempo.sans_valeur],
  ]) {
    const dt = document.createElement("dt");
    dt.textContent = libelle;
    const dd = document.createElement("dd");
    dd.textContent = `${n.toLocaleString("fr-FR")} (${((n / s.total) * 100).toFixed(1).replace(".", ",")} %)`;
    completude.append(dt, dd);
  }
}

/// Graduations de l'abscisse d'un histogramme, espacées en pourcentage de la
/// largeur plutôt qu'une par tranche — [`Histogramme`] en a jusqu'à 18, les
/// nommer toutes serait illisible. `libellerBorne(valeur)` met en forme une
/// borne (BPM brut pour le tempo, `m:ss` pour la durée).
function dessinerAxe(id, h, libellerBorne) {
  const hote = $(id);
  hote.replaceChildren();
  const tranches = h.comptes.length;
  const CIBLE = 6;
  const pas = Math.max(1, Math.round(tranches / CIBLE));
  const bornes = [];
  for (let i = 0; i <= tranches; i += pas) bornes.push(i);
  if (bornes[bornes.length - 1] !== tranches) bornes.push(tranches);

  for (const i of bornes) {
    const pct = (i / tranches) * 100;
    const el = document.createElement("span");
    el.textContent = libellerBorne(h.min + i * h.pas);
    if (i === 0) {
      el.style.left = "0";
    } else if (i === tranches) {
      el.style.left = "100%";
      el.style.transform = "translateX(-100%)";
    } else {
      el.style.left = `${pct}%`;
      el.style.transform = "translateX(-50%)";
    }
    hote.appendChild(el);
  }
}

/// Rend une liste `[nom, compte]` en barres horizontales — sert aux genres
/// comme aux formats de fichier, seul le pliage en amont diffère.
/// `nomsGris` fait ressortir en gris/italique les lignes qui ne décrivent
/// pas une vraie catégorie (« Sans genre identifié », « non mesuré »…), pour
/// qu'elles ne se confondent pas avec un genre ou un format minoritaire.
function dessinerBarres(id, lignes, nomsGris = []) {
  const hote = $(id);
  hote.replaceChildren();
  if (lignes.length === 0) return;
  const max = Math.max(...lignes.map(([, c]) => c));

  for (const [nom, compte] of lignes) {
    const el = document.createElement("div");
    el.className = "barre-genre" + (nomsGris.includes(nom) ? " barre-genre--indetermine" : "");
    el.innerHTML = `<span class="barre-genre__nom"></span>
                    <div class="barre-genre__piste"><div class="barre-genre__remplissage"></div></div>
                    <span class="barre-genre__valeur"></span>`;
    el.children[0].textContent = nom;
    el.children[0].title = nom;
    el.children[1].firstElementChild.style.width = `${(compte / max) * 100}%`;
    el.children[2].textContent = compte.toLocaleString("fr-FR");
    hote.appendChild(el);
  }
}

/// Les `n` premiers rangs affichés tels quels, le reste plié dans « Autres ».
const RANGS_AFFICHES = 12;

function plierEnAutres(lignes, unite) {
  const gardes = lignes.slice(0, RANGS_AFFICHES);
  const reste = lignes.slice(RANGS_AFFICHES).reduce((n, [, c]) => n + c, 0);
  return reste > 0
    ? [...gardes, [`Autres (${lignes.length - RANGS_AFFICHES} ${unite})`, reste]]
    : gardes;
}

function dessinerBarresGenres(genres) {
  // « — » (aucun genre résolu, ni par MusicBrainz ni par le tag du fichier)
  // n'est pas un genre parmi d'autres : le noyer dans « Autres » masquait le
  // seul chiffre qui réponde à « pourquoi autant de genres non identifiés ».
  // Il sort donc du classement et se trace à part, en dernier, quel que soit
  // son rang.
  const identifie = genres.filter(([g]) => g !== "—");
  const sansGenre = genres.find(([g]) => g === "—");
  const lignes = plierEnAutres(identifie, "genres");
  if (sansGenre) lignes.push(["Sans genre identifié", sansGenre[1]]);
  dessinerBarres("stats-genres", lignes, ["Sans genre identifié"]);
}

function dessinerBarresCodecs(codecs) {
  dessinerBarres("stats-codecs", plierEnAutres(codecs, "formats"), ["non mesuré"]);
}

/* ------------------------------------------------------ vérifications */

/// Une ligne de vérification : `ligne` en gras, `detail` en dessous en gris,
/// `mesure` alignée à droite (une distance, une date…), `action` un bouton
/// optionnel (retirer, par exemple). Même gabarit pour les cinq listes du
/// mode Bibliothèque — genres suspects, éditions, doublons, isolés, échecs.
function dessinerListeVerif(id, items) {
  const hote = $(id);
  hote.replaceChildren();
  for (const it of items) {
    const el = document.createElement("div");
    el.className = "verif" + (it.onClick ? " verif--clic" : "");
    if (it.onClick) el.addEventListener("click", it.onClick);
    const corps = document.createElement("div");
    corps.className = "verif__corps";
    const ligne = document.createElement("div");
    ligne.className = "verif__ligne";
    ligne.textContent = it.ligne;
    ligne.title = it.ligne;
    corps.appendChild(ligne);
    if (it.detail) {
      const detail = document.createElement("div");
      detail.className = "verif__detail";
      detail.textContent = it.detail;
      corps.appendChild(detail);
    }
    el.appendChild(corps);
    if (it.mesure) {
      const mesure = document.createElement("span");
      mesure.className = "verif__mesure";
      mesure.textContent = it.mesure;
      el.appendChild(mesure);
    }
    if (it.action) el.appendChild(it.action);
    hote.appendChild(el);
  }
}

/* --------------------------------------------- paramètres de la carte */

const CHAMPS_PARAMETRES_CARTE = {
  "param-perplexite": "perplexite",
  "param-epoques": "epoques",
  "param-familles": "familles",
  "param-iterations": "iterations_kmeans",
};

async function chargerParametresCarte() {
  const p = await invoke("map_parameters");
  $("param-perplexite").value = p.perplexite;
  $("param-epoques").value = p.epoques;
  $("param-familles").value = p.familles;
  $("param-iterations").value = p.iterations_kmeans;

  $("densite-resolution").value = String(p.densite_resolution);
  $("densite-noyau").value = p.densite_noyau;
  $("densite-noyau-valeur").textContent = p.densite_noyau.toFixed(3);
  $("densite-bandes").value = p.densite_bandes;
  $("densite-bandes-valeur").textContent = p.densite_bandes;
}

for (const [id, cle] of Object.entries(CHAMPS_PARAMETRES_CARTE)) {
  $(id).addEventListener("change", async (e) => {
    const valeur = Number(e.target.value);
    if (!Number.isFinite(valeur)) return;
    try {
      await invoke("set_map_parameter", { cle, valeur });
    } catch (err) {
      remonter(err, "paramètre de la carte");
    }
  });
}

/* --------------------------------- vocabulaire des familles par genre */

// Une ligne par famille : « Nom: genre1, genre2, genre3 ». Format texte
// plutôt qu'une liste éditable élément par élément — plus rapide à relire et
// à corriger d'un coup pour douze familles, et rien ici n'a besoin de
// glisser-déposer.
function serialiserVocabulaire(vocabulaire) {
  return vocabulaire.map(([nom, genres]) => `${nom}: ${genres.join(", ")}`).join("\n");
}

function analyserVocabulaire(texte) {
  const vocabulaire = [];
  for (const ligne of texte.split("\n")) {
    const deux = ligne.indexOf(":");
    if (deux < 0) continue; // ligne vide ou incomplète, ignorée sans erreur
    const nom = ligne.slice(0, deux).trim();
    const genres = ligne
      .slice(deux + 1)
      .split(",")
      .map((g) => g.trim().toLowerCase())
      .filter(Boolean);
    if (nom && genres.length) vocabulaire.push([nom, genres]);
  }
  return vocabulaire;
}

async function chargerVocabulaireFamilles() {
  const v = await invoke("vocabulaire_familles");
  $("vocabulaire-familles").value = serialiserVocabulaire(v);
}

$("enregistrer-vocabulaire").addEventListener("click", async () => {
  const bouton = $("enregistrer-vocabulaire");
  bouton.disabled = true;
  $("vocabulaire-etat").textContent = "Enregistrement…";
  try {
    const vocabulaire = analyserVocabulaire($("vocabulaire-familles").value);
    await invoke("definir_vocabulaire_familles", { vocabulaire });
    $("vocabulaire-etat").textContent =
      `${vocabulaire.length} famille(s) enregistrée(s) — « Recalculer la carte » pour l'appliquer.`;
  } catch (e) {
    remonter(e, "vocabulaire des familles");
    $("vocabulaire-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

$("reinitialiser-vocabulaire").addEventListener("click", async () => {
  const bouton = $("reinitialiser-vocabulaire");
  bouton.disabled = true;
  try {
    // Liste vide = restaure les valeurs par défaut côté base, voir
    // `Library::definir_vocabulaire_familles`.
    await invoke("definir_vocabulaire_familles", { vocabulaire: [] });
    await chargerVocabulaireFamilles();
    $("vocabulaire-etat").textContent =
      "Valeurs par défaut restaurées — « Recalculer la carte » pour l'appliquer.";
  } catch (e) {
    remonter(e, "vocabulaire des familles");
    $("vocabulaire-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

$("recalculer-carte").addEventListener("click", async () => {
  const bouton = $("recalculer-carte");
  bouton.disabled = true;
  $("carte-parametres-etat").textContent = "Recalcul en cours…";
  try {
    const r = await invoke("recompute_map");
    $("carte-parametres-etat").textContent =
      `${r.empreintes.toLocaleString("fr-FR")} morceaux replacés, ${r.familles.toLocaleString("fr-FR")} familles.`;
    // La carte affichée, si elle l'est, montre des positions périmées ; les
    // familles nommées et le filtre par famille de l'Écoute le sont tout autant.
    await familleARecalculee();
    if (modeCourant === "explorer") {
      await chargerCarte();
      await dessinerFamilles();
    }
  } catch (e) {
    remonter(e, "recalcul de la carte");
    $("carte-parametres-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

// Régénère les tuiles vectorielles (module `carto`), et rouvre MapLibre sur
// le résultat. Bascule automatiquement sur le plan de ville réel si
// `ville-paris.db` existe (`main.rs::engendrer_tuiles`) : rien à choisir ici.
//
// **Aucun autre bouton n'appelait `engendrer_tuiles`** avant celui-ci — la
// commande existait côté Rust, mais rien côté interface ne la déclenchait ;
// la carte affichée restait celle de la dernière génération, quel que soit
// le plan de ville importé depuis.
$("regenerer-tuiles").addEventListener("click", async () => {
  const bouton = $("regenerer-tuiles");
  bouton.disabled = true;
  $("tuiles-etat").textContent = "Génération en cours…";
  try {
    const r = await invoke("engendrer_tuiles");
    $("tuiles-etat").textContent = r;
    // Les tuiles ont changé sous l'instance MapLibre en place : la relancer
    // à neuf plutôt que d'espérer qu'elle redemande ce qu'elle a déjà en
    // cache.
    if (gl) {
      gl.remove();
      gl = null;
      glPret = false;
    }
    if (modeCourant === "explorer" && carte.affichage === "carte") {
      majAffichageGL();
    }
  } catch (e) {
    remonter(e, "génération des tuiles");
    $("tuiles-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

/* ------------------------------------------------- paramètres de densité */

// Résolution, noyau, bandes : mêmes principe que les réglages de la carte
// juste au-dessus — un changement n'écrit que la valeur, `recalculer-densite`
// rejoue. Automatiser le recalcul à chaque cran de curseur bombarderait le
// moteur (150 à 550 ms par appel, mesuré) sans que rien ne le demande.
const CHAMPS_PARAMETRES_DENSITE = {
  "densite-resolution": "densite_resolution",
  "densite-noyau": "densite_noyau",
  "densite-bandes": "densite_bandes",
};

for (const [id, cle] of Object.entries(CHAMPS_PARAMETRES_DENSITE)) {
  $(id).addEventListener("change", async (e) => {
    const valeur = Number(e.target.value);
    if (!Number.isFinite(valeur)) return;
    try {
      await invoke("set_map_parameter", { cle, valeur });
    } catch (err) {
      remonter(err, "paramètre de densité");
    }
  });
}

// Les deux curseurs affichent leur valeur au fil du geste, sans attendre le
// commit — la résolution (une liste) n'en a pas besoin.
$("densite-noyau").addEventListener("input", (e) => {
  $("densite-noyau-valeur").textContent = Number(e.target.value).toFixed(3);
});
$("densite-bandes").addEventListener("input", (e) => {
  $("densite-bandes-valeur").textContent = e.target.value;
});

$("recalculer-densite").addEventListener("click", async () => {
  const bouton = $("recalculer-densite");
  bouton.disabled = true;
  $("densite-parametres-etat").textContent = "Recalcul en cours…";
  try {
    await invoke("recompute_density");
    $("densite-parametres-etat").textContent = "Nappe de densité à jour.";
    if (modeCourant === "explorer") {
          dessinerCarte();
    }
  } catch (e) {
    remonter(e, "recalcul de la densité");
    $("densite-parametres-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});


/// Genres suspects et éditions multiples : peu coûteux (une requête
/// d'agrégation), rejoués à chaque entrée dans le mode comme le reste des
/// statistiques.
async function chargerVerifications() {
  const suspects = await invoke("suspect_genres");
  dessinerListeVerif(
    "genres-suspects",
    suspects.map(([, ligne, genre, dominants]) => ({
      ligne,
      detail: `Genre : ${genre} · Famille : ${dominants}`,
    })),
  );

  const editions = await invoke("multiple_editions");
  dessinerListeVerif(
    "editions-multiples",
    editions.map(([artiste, titre, versions]) => ({
      ligne: `${artiste} — ${titre}`,
      detail: versions.map(([album, n]) => `${album} (${n})`).join(" · "),
    })),
  );

  const echecs = await invoke("scan_failures");
  $("echecs-scan-vide").textContent = echecs.length
    ? `${echecs.length.toLocaleString("fr-FR")} fichier${echecs.length > 1 ? "s" : ""} qu'un scan n'a pas su lire.`
    : "Aucun échec de scan connu.";
  dessinerListeVerif(
    "echecs-scan",
    echecs.map(([chemin, raison, at]) => {
      const bouton = document.createElement("button");
      bouton.className = "bouton bouton--danger";
      bouton.textContent = "Retirer";
      bouton.addEventListener("click", async () => {
        await invoke("dismiss_scan_failure", { path: chemin });
        await chargerVerifications();
      });
      return {
        ligne: chemin.split("/").pop(),
        detail: `${raison} — ${new Date(at * 1000).toLocaleDateString("fr-FR")}`,
        action: bouton,
      };
    }),
  );
}

/// Le texte d'une piste dans une liste de vérification : artiste — titre,
/// avec le nom de fichier en repli pour un morceau sans titre en tag.
function ligneTrack(t) {
  return `${txt(t.artist, "?")} — ${txt(t.title, t.path.split("/").pop())}`;
}

/// Doublons probables et points isolés partagent le même graphe des
/// voisins, coûteux à construire (~15 s sur toute la bibliothèque) mais
/// gardé en mémoire côté moteur — d'où le geste explicite plutôt qu'un
/// calcul silencieux à chaque entrée dans le mode.
$("chercher-doublons").addEventListener("click", async () => {
  const bouton = $("chercher-doublons");
  bouton.disabled = true;
  $("graphe-attente").textContent = "Calcul en cours — jusqu'à une trentaine de secondes la première fois…";
  try {
    const doublons = await invoke("probable_duplicates");
    dessinerListeVerif(
      "doublons-probables",
      doublons.map((d) => ({
        ligne: `${ligneTrack(d.a)}  ↔  ${ligneTrack(d.b)}`,
        mesure: `d² = ${d.distance2.toFixed(5)}`,
      })),
    );
    $("graphe-attente").textContent = doublons.length
      ? `${doublons.length.toLocaleString("fr-FR")} paire${doublons.length > 1 ? "s" : ""} probable${doublons.length > 1 ? "s" : ""}.`
      : "Aucun doublon probable trouvé.";

    const isoles = await invoke("isolated_points");
    dessinerListeVerif(
      "points-isoles",
      isoles.map((p) => ({
        ligne: ligneTrack(p.piste),
        detail: `Plus proche : ${ligneTrack(p.plus_proche)}`,
        mesure: `d² = ${p.distance2.toFixed(3)}`,
      })),
    );
  } catch (e) {
    remonter(e, "doublons et points isolés");
  } finally {
    bouton.disabled = false;
  }
});

/// `libeller(i)` nomme la tranche `i` pour l'infobulle — le texte diffère
/// entre tempo (BPM) et durée (m:ss), la forme des barres non.
function dessinerHistogramme(id, h, libeller) {
  const hote = $(id);
  hote.replaceChildren();
  const max = Math.max(1, ...h.comptes);
  h.comptes.forEach((compte, i) => {
    const col = document.createElement("div");
    col.className = "histogramme__colonne" + (compte === 0 ? " histogramme__colonne--vide" : "");
    col.style.height = `${Math.max(2, (compte / max) * 100)}%`;
    col.title = `${libeller(i)} : ${compte.toLocaleString("fr-FR")}`;
    hote.appendChild(col);
  });
}

function notesHistogramme(h, participes) {
  const bouts = [];
  if (h.hors_gamme) bouts.push(`${h.hors_gamme.toLocaleString("fr-FR")} au-delà de la dernière tranche`);
  if (h.sans_valeur) bouts.push(`${h.sans_valeur.toLocaleString("fr-FR")} jamais ${participes}`);
  return bouts.join(" · ");
}

async function dessinerRacines() {
  const racines = await invoke("roots");
  const hote = $("racines");
  hote.replaceChildren();

  if (racines.length === 0) {
    hote.innerHTML = '<p class="file__vide">Aucun dossier surveillé.</p>';
    return;
  }

  for (const r of racines) {
    const el = document.createElement("div");
    el.className = "racine";
    el.innerHTML = `<span class="racine__chemin"></span>
                    <span class="racine__cpt"></span>
                    <button class="bouton racine__analyser">Analyser</button>
                    <button class="bouton bouton--danger">Oublier</button>`;
    el.children[0].textContent = r.path;
    el.children[1].textContent = `${r.tracks.toLocaleString("fr-FR")} morceaux`;
    el.children[2].addEventListener("click", () => {
      analyserRacine(r.path).catch((e) => remonter(e, "analyse de la racine"));
    });
    el.children[3].addEventListener("click", async () => {
      // Opération destructrice : elle emporte les morceaux de la racine.
      const ok = confirm(
        `Oublier ${r.path} ?\n\nLes ${r.tracks.toLocaleString("fr-FR")} morceaux ` +
          `qui en dépendent seront retirés de la bibliothèque. Les fichiers ne ` +
          `sont pas touchés.`,
      );
      if (!ok) return;
      await invoke("forget_root", { path: r.path });
      await dessinerRacines();
      await charger();
    });
    hote.appendChild(el);
  }
}

// Sélecteur natif : on ne demande pas à l'utilisateur de connaître ses chemins.
$("parcourir").addEventListener("click", async () => {
  const choisi = await window.__TAURI__.dialog.open({
    directory: true,
    multiple: false,
    title: "Choisir le dossier de musique",
  });
  if (choisi) $("nouveau-dossier").value = choisi;
});

/// « Scanner » : le point de départ du mode. Avec un dossier saisi, il
/// l'ajoute et lui fait faire toute la chaîne ([`analyserRacine`] — scan,
/// empreintes, tempo/tonalité/énergie, genres) ; sans, il rattrape ce qui
/// manque sur toutes les racines déjà surveillées (`lancerChaineComplete`).
/// Les deux chaînes reprennent où elles s'étaient arrêtées, et se grisent
/// elles-mêmes le temps de tourner (`verrouillerActualisation`).
$("lancer-scan").addEventListener("click", async () => {
  const chemin = $("nouveau-dossier").value.trim();
  try {
    if (chemin) {
      $("nouveau-dossier").value = "";
      await analyserRacine(chemin);
    } else if ((await invoke("roots")).length === 0) {
      $("scan-etat").textContent = "Choisissez d'abord un dossier de musique.";
    } else {
      await lancerChaineComplete();
    }
  } catch (e) {
    remonter(e, "scan");
    $("scan-etat").textContent = String(e);
  }
});

/* ---------------------------------------------------------- analyse */

/// 1,1 s/morceau, mesuré sur la carte SD — le stockage interne va bien plus
/// vite, l'estimation est donc un plafond. Sert aux durées annoncées pendant
/// la chaîne « Analyser » d'une racine ([`analyserRacine`]).
const SECONDES_PAR_MORCEAU = 1.1;

/// Jauge de progression d'une passe de fond (analyse, descripteurs, genres) —
/// le texte dit déjà le compte, la barre le donne à voir d'un coup d'œil
/// sans le lire. Cachée hors passe : une barre à 0 immobile ne dit rien
/// qu'une absence ne dise mieux.
function majJauge(id, enCours, faits, total) {
  const el = $(id);
  el.hidden = !enCours || !total;
  if (total) {
    el.max = total;
    el.value = faits;
  }
}

/// Durée en heures ou minutes, pour annoncer une passe qui dure.
function dureeLongue(s) {
  if (s < 90) return `${Math.round(s)} s`;
  if (s < 5400) return `${Math.round(s / 60)} min`;
  return `${(s / 3600).toFixed(1)} h`.replace(".", ",");
}

/// Sonde `invoke(commande)` jusqu'à ce que `.en_cours` devienne faux, en
/// laissant `surProgres` mettre à jour l'affichage à chaque tour. Factorise
/// les passes qu'[`analyserRacine`] enchaîne — scan, empreintes,
/// descripteurs, genres —, qui partagent toutes la même forme d'état
/// (`en_cours`/`faits`/`total`/`resultat`).
function attendreFin(commande, pas, surProgres) {
  return new Promise((resolve, reject) => {
    const id = setInterval(async () => {
      let etat;
      try {
        etat = await invoke(commande);
      } catch (e) {
        clearInterval(id);
        reject(e);
        return;
      }
      surProgres(etat);
      if (!etat.en_cours) {
        clearInterval(id);
        resolve(etat);
      }
    }, pas);
  });
}

/// Le bouton « Analyser » d'une racine, tout ce que la bibliothèque sait
/// faire sur ses fichiers, enchaîné : scan (relit tags/format/débit),
/// empreintes CLAP, tempo/tonalité/énergie, genres MusicBrainz si une adresse
/// de contact est renseignée, puis popularité générale (ListenBrainz +
/// Deezer, sans clé). Un seul bouton par racine plutôt que cinq sections
/// séparées à faire tourner soi-même dans l'ordre — c'est toujours le même
/// ordre, autant l'écrire une fois.
///
/// Les passes restent globales côté moteur (pas de filtre par racine) : dans
/// l'usage courant — une racine qu'on vient d'ajouter ou de changer — ce qui
/// est « en attente » est justement ce qu'on vient de scanner, donc le
/// résultat correspond à l'intention même sans filtrage explicite.
async function analyserRacine(chemin) {
  verrouillerActualisation(true);
  const force = $("analyse-force").checked;
  const contact = contactMb();
  const etat = $("scan-etat");
  const jauge = $("scan-jauge");
  try {
    etat.textContent = `${chemin} — scan…`;
    jauge.hidden = true;
    await invoke("start_scan", { path: chemin, force });
    await attendreFin("scan_state", 1000, (s) => {
      etat.textContent = s.en_cours
        ? `${chemin} — scan : ${s.morceaux.toLocaleString("fr-FR")} morceaux en base`
        : (etat.textContent = s.resultat ?? "");
    });

    etat.textContent = `${chemin} — empreintes…`;
    await invoke("start_analysis");
    await attendreFin("analysis_state", 2000, (a) => {
      majJauge("scan-jauge", a.en_cours, a.faits, a.total);
      if (a.en_cours) {
        const reste = Math.max(0, a.total - a.faits);
        etat.textContent = a.total
          ? `${chemin} — empreintes : ${a.faits.toLocaleString("fr-FR")} / ${a.total.toLocaleString("fr-FR")} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
          : `${chemin} — empreintes…`;
      }
    });
    // La projection a replacé tous les points : la carte affichée est
    // périmée, y compris ses familles et le filtre par famille de l'Écoute.
    if (modeCourant === "explorer") await chargerCarte();
    await familleARecalculee();

    etat.textContent = `${chemin} — tempo, tonalité, énergie…`;
    await invoke("start_descripteurs", { force });
    await attendreFin("descripteurs_state", 2000, (d) => {
      majJauge("scan-jauge", d.en_cours, d.faits, d.total);
      if (d.en_cours) {
        etat.textContent = d.total
          ? `${chemin} — mesures : ${d.faits.toLocaleString("fr-FR")} / ${d.total.toLocaleString("fr-FR")}`
          : `${chemin} — mesures…`;
      }
    });

    if (contact.includes("@")) {
      etat.textContent = `${chemin} — genres…`;
      await invoke("start_enrichment", { contact });
      await attendreFin("enrichment_state", 3000, (e) => {
        majJauge("scan-jauge", e.en_cours, e.artistes, e.total);
        if (e.en_cours) {
          const reste = Math.max(0, e.total - e.artistes);
          etat.textContent = e.total
            ? `${chemin} — genres : ${e.artistes.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")} artistes — reste ${dureeLongue(reste * SECONDES_PAR_ARTISTE)}`
            : `${chemin} — genres…`;
        }
      });
      // Nommées à la volée : il suffit de les redemander. Les numéros de
      // famille ne bougent pas (le clustering est intact), le filtre par
      // famille de l'Écoute reste donc valide — on ne rafraîchit que les noms.
      carte.familles = null;
      famillesParAlbum = null;
      await chargerFamillesParAlbum();
      if (modeCourant === "explorer") await dessinerFamilles();
      else if (modeCourant === "ecoute") {
        await dessinerFamillesEcoute();
        rafraichirGrille();
      }
    }

    // Popularité générale (ListenBrainz + Deezer). Sans condition d'adresse :
    // les deux API sont publiques ; le contact ne sert qu'au User-Agent.
    etat.textContent = `${chemin} — popularité…`;
    await invoke("start_popularite", { contact, rafraichir: $("pop-rafraichir").checked });
    await attendreFin("popularite_state", 3000, (p) => {
      majJauge("scan-jauge", p.en_cours, p.faits, p.total);
      if (p.en_cours) {
        etat.textContent = p.total
          ? `${chemin} — popularité : ${p.faits.toLocaleString("fr-FR")} / ${p.total.toLocaleString("fr-FR")}`
          : `${chemin} — popularité…`;
      }
    });
    popARecalculee();
    await chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));

    jauge.hidden = true;
    etat.textContent = `${chemin} — terminé.`;
    await dessinerRacines();
    await chargerStatsBibliotheque();
    await chargerVerifications();
  } catch (e) {
    remonter(e, "analyse de la racine");
    etat.textContent = String(e);
  } finally {
    verrouillerActualisation(false);
  }
}

/* ----------------------------------------- chaîne de passes de fond */

/// Les passes de fond, enchaînées par « Scanner » quand il n'y a pas de
/// dossier neuf à ajouter (`lancerChaineComplete`). Même moteur que la chaîne
/// « Analyser » d'une racine ([`analyserRacine`]), mais sur toutes les racines
/// à la fois. Les commandes sont globales côté moteur ; le scan, seul à
/// prendre une racine, est rejoué sur chacune à la suite.

/// Une ligne d'avancement dans le panneau : la barre (visible et graduée dès
/// qu'un total est connu, cachée sinon) et le texte, préfixé de l'étape quand
/// c'est la chaîne complète qui tourne (« Étape 2/4 — … »).
function avancementActu(phase, texte, faits, total) {
  const jauge = $("scan-jauge");
  jauge.hidden = !total;
  if (total) {
    jauge.max = total;
    jauge.value = faits;
  }
  $("scan-etat").textContent = phase + texte;
}

const pourcent = (a, b) => (b ? ` (${Math.round((a / b) * 100)} %)` : "");

async function passeScan(force, phase) {
  const racines = await invoke("roots");
  if (racines.length === 0) {
    $("scan-etat").textContent = phase + "aucun dossier surveillé.";
    return;
  }
  for (let i = 0; i < racines.length; i++) {
    const r = racines[i];
    const ou = racines.length > 1 ? ` (dossier ${i + 1}/${racines.length})` : "";
    avancementActu(phase, `scan${ou} : démarrage…`, 0, 0);
    await invoke("start_scan", { path: r.path, force });
    await attendreFin("scan_state", 1000, (s) => {
      // Le scan n'annonce pas de total : le compte qui monte est la seule
      // mesure d'avancement possible, sans barre.
      if (s.en_cours) {
        avancementActu(phase, `scan${ou} : ${s.morceaux.toLocaleString("fr-FR")} morceaux vus`, 0, 0);
      }
    });
  }
  await charger();
}

async function passeEmpreintes(phase) {
  avancementActu(phase, "empreintes : démarrage…", 0, 0);
  await invoke("start_analysis");
  await attendreFin("analysis_state", 1500, (a) => {
    if (!a.en_cours) return;
    const reste = Math.max(0, a.total - a.faits);
    avancementActu(
      phase,
      a.total
        ? `empreintes : ${a.faits.toLocaleString("fr-FR")} / ${a.total.toLocaleString("fr-FR")}${pourcent(a.faits, a.total)} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
        : "empreintes : démarrage…",
      a.faits,
      a.total,
    );
  });
  // La projection a replacé tous les points — mêmes suites qu'`analyserRacine`.
  if (modeCourant === "explorer") await chargerCarte();
  await familleARecalculee();
}

async function passeDescripteurs(force, phase) {
  avancementActu(phase, "tempo, tonalité, énergie : démarrage…", 0, 0);
  await invoke("start_descripteurs", { force });
  await attendreFin("descripteurs_state", 1500, (d) => {
    if (!d.en_cours) return;
    const reste = Math.max(0, d.total - d.faits);
    avancementActu(
      phase,
      d.total
        ? `tempo, tonalité, énergie : ${d.faits.toLocaleString("fr-FR")} / ${d.total.toLocaleString("fr-FR")}${pourcent(d.faits, d.total)} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
        : "tempo, tonalité, énergie : démarrage…",
      d.faits,
      d.total,
    );
  });
}

async function passeGenres(contact, phase) {
  avancementActu(phase, "genres : démarrage…", 0, 0);
  await invoke("start_enrichment", { contact });
  await attendreFin("enrichment_state", 3000, (e) => {
    if (!e.en_cours) return;
    const reste = Math.max(0, e.total - e.artistes);
    avancementActu(
      phase,
      e.total
        ? `genres : ${e.artistes.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")} artistes${pourcent(e.artistes, e.total)} — reste ${dureeLongue(reste * SECONDES_PAR_ARTISTE)}`
        : "genres : démarrage…",
      e.artistes,
      e.total,
    );
  });
  carte.familles = null;
  famillesParAlbum = null;
  await chargerFamillesParAlbum();
  if (modeCourant === "explorer") await dessinerFamilles();
  else if (modeCourant === "ecoute") {
    await dessinerFamillesEcoute();
    rafraichirGrille();
  }
}

/// La popularité générale (ListenBrainz + Deezer). Ne dépend d'aucune clé ni
/// adresse : `contact` ne sert qu'au User-Agent de ListenBrainz. `rafraichir`
/// réinterroge aussi ce qui date de plus de 90 jours (case du rail).
async function passePopularite(contact, phase, rafraichir = false) {
  avancementActu(phase, "popularité : démarrage…", 0, 0);
  await invoke("start_popularite", { contact, rafraichir });
  await attendreFin("popularite_state", 3000, (p) => {
    if (!p.en_cours) return;
    avancementActu(
      phase,
      p.total
        ? `popularité : ${p.faits.toLocaleString("fr-FR")} / ${p.total.toLocaleString("fr-FR")}${pourcent(p.faits, p.total)}`
        : "popularité : démarrage…",
      p.faits,
      p.total,
    );
  });
  popARecalculee();
  chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));
}

/// La chaîne complète — scan, empreintes, tempo/tonalité/énergie, genres,
/// popularité — sur toutes les racines surveillées, l'étape en cours affichée
/// en clair. C'est ce que « Scanner » lance quand aucun dossier neuf n'est
/// saisi ; chaque passe reprend où elle s'était arrêtée.
async function lancerChaineComplete() {
  const force = $("analyse-force").checked;
  const contact = contactMb();
  const etat = $("scan-etat");
  verrouillerActualisation(true);
  try {
    await passeScan(force, "Étape 1/5 — ");
    await passeEmpreintes("Étape 2/5 — ");
    await passeDescripteurs(force, "Étape 3/5 — ");
    if (contact.includes("@")) await passeGenres(contact, "Étape 4/5 — ");
    else etat.textContent = "Étape 4/5 — genres sautés (pas d'adresse de contact MusicBrainz)";
    await passePopularite(contact, "Étape 5/5 — ", $("pop-rafraichir").checked);
    etat.textContent = `${etat.textContent} — terminé.`;
  } catch (e) {
    remonter(e, "actualisation");
    etat.textContent = String(e);
  } finally {
    $("scan-jauge").hidden = true;
    $("scan-jauge").value = 0;
    verrouillerActualisation(false);
    await dessinerRacines();
    await chargerStatsBibliotheque();
    await chargerVerifications();
  }
}

/// Une passe ne tourne qu'à un exemplaire côté moteur : tant qu'elle est là,
/// on grise tout ce qui pourrait en relancer une — « Scanner » comme les
/// « Analyser » des racines.
function verrouillerActualisation(occupe) {
  document
    .querySelectorAll("#lancer-scan, .racine__analyser")
    .forEach((b) => (b.disabled = occupe));
}

/// Une passe lancée avant de quitter le mode continue de tourner : en
/// revenant dans Bibliothèque, on raccroche l'affichage à son état plutôt que
/// de laisser la ligne muette pendant qu'elle avance.
async function reprendreActualisationEnCours() {
  const sondes = [
    ["scan_state", "scan"],
    ["analysis_state", "empreintes"],
    ["descripteurs_state", "tempo, tonalité, énergie"],
    ["enrichment_state", "genres"],
    ["popularite_state", "popularité"],
  ];
  for (const [cmd, nom] of sondes) {
    let s;
    try {
      s = await invoke(cmd);
    } catch {
      continue;
    }
    if (!s.en_cours) continue;
    verrouillerActualisation(true);
    attendreFin(cmd, 2000, (e) => {
      if (!e.en_cours) {
        $("scan-etat").textContent = `${nom} — terminé.`;
        return;
      }
      const faits = e.faits ?? e.artistes ?? 0;
      avancementActu(
        "",
        e.total
          ? `${nom} : ${faits.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")}${pourcent(faits, e.total)}`
          : `${nom} : ${faits.toLocaleString("fr-FR")}…`,
        faits,
        e.total,
      );
    })
      .catch((err) => remonter(err, "actualisation"))
      .finally(async () => {
        $("scan-jauge").hidden = true;
        verrouillerActualisation(false);
        if (cmd === "popularite_state") {
          popARecalculee();
          chargerPopulariteFraicheur().catch((e) => remonter(e, "popularité"));
        }
        await dessinerRacines();
        await chargerStatsBibliotheque();
      });
    return;
  }
}

/* --------------------------------------------- stockage & cache */

/// Le dossier où l'application écrit tout — montré tel quel, l'utilisateur doit
/// pouvoir le retrouver dans son gestionnaire de fichiers, le sauvegarder ou le
/// purger.
async function chargerDossierDonnees() {
  $("dossier-donnees").textContent = await invoke("dossier_donnees");
}

/// Ce que l'audio dérivé occupe sur le disque — stems démixés et rendus HD.
///
/// Un jeu de quatre stems pèse 124 Mo, un cache HD grossit d'un morceau à
/// l'autre : sans ce compte, c'est une fuite qu'on ne découvre qu'en cherchant
/// pourquoi le disque se remplit.
async function majCache() {
  const [[oStems, nStems], [oHd, nHd]] = await Promise.all([
    invoke("stems_cache"),
    invoke("superres_cache"),
  ]);
  const octets = oStems + oHd;
  $("vider-cache").disabled = octets === 0;
  if (octets === 0) {
    $("cache-taille").textContent = "Aucun audio dérivé en cache pour l'instant.";
    return;
  }
  const bouts = [];
  if (nStems) bouts.push(`${nStems} morceau${nStems > 1 ? "x" : ""} démixé${nStems > 1 ? "s" : ""}`);
  if (nHd) bouts.push(`${nHd} rendu${nHd > 1 ? "s" : ""} HD`);
  $("cache-taille").textContent =
    `${bouts.join(" · ")} — ${(octets / 1e9).toFixed(2).replace(".", ",")} Go. ` +
    `Les vider force à refaire ces rendus, rien d'autre n'est perdu.`;
}

$("vider-cache").addEventListener("click", async () => {
  $("vider-cache").disabled = true;
  try {
    await invoke("stems_cache_vider");
    await invoke("vider_cache_hd");
  } catch (e) {
    remonter(e, "vidage du cache");
  }
  await majCache();
});

/// Écrit ce qu'on entend dans un dossier choisi.
///
/// **Une seule sortie, et c'est un choix.** La spec en prévoyait trois — un
/// stem, la sélection, le mélange — mais mettre un stem en solo *est* la
/// sélection : un menu de plus n'aurait dit que ce que le dock montre déjà.
///
/// Le moteur refuse d'écrire sous une racine surveillée ; le message le dit et
/// nomme le dossier fautif, plutôt que de laisser un rendu être ingéré comme un
/// morceau à la surveillance suivante.
$("exporter").addEventListener("click", async () => {
  if (!edition.stems.length) return;
  const dossier = await window.__TAURI__.dialog.open({
    directory: true,
    multiple: false,
    title: "Où écrire le rendu",
  });
  if (!dossier) return;

  // Le nom dit ce qu'on entend : le morceau, ce qui est isolé, et les réglages
  // qui ne sont pas neutres. Sans cela, deux rendus du même morceau seraient
  // indiscernables.
  const parts = [txt(edition.source?.title, "rendu")];
  if (edition.solo) parts.push(edition.solo);
  if (Math.abs(edition.vitesse - 1) > 1e-3) parts.push(`${Math.round(edition.vitesse * 100)}%`);
  if (edition.tonalite) parts.push(`${edition.tonalite > 0 ? "+" : ""}${edition.tonalite}`);
  // Un stem greffé ou réglé à part change le rendu autant que la vitesse
  // d'ensemble : le nom doit le porter, sinon deux exports diffèrent sans
  // qu'on sache en quoi.
  for (const st of edition.stems) {
    if (st.greffe) parts.push(`${st.nom} greffé`);
    else if (stemEcarte(st)) parts.push(`${st.nom} ${etiquetteStem(st)}`);
  }
  const nom = parts.join(" — ").replace(/[/\\:]/g, "-");

  $("exporter").disabled = true;
  $("dock-aide").textContent = "écriture…";
  try {
    const ecrit = await invoke("stems_exporter", {
      stems: edition.stems.map((s) => [s.nom, s.chemin]),
      niveaux: niveaux(),
      // Une valeur par stem : c'est le décalage entre eux qu'il faut rendre,
      // et il n'existe pas dans leur somme.
      vitesses: edition.stems.map(vitesseDe),
      demiTons: edition.stems.map(tonaliteDe),
      destination: dossier,
      nom,
    });
    $("dock-aide").textContent = `écrit : ${ecrit.split("/").pop()}`;
  } catch (e) {
    $("dock-aide").textContent = "";
    remonter(e, "export");
  }
  $("exporter").disabled = false;
});

/// Deux requêtes par artiste et une par seconde : c'est MusicBrainz qui
/// impose le rythme. Sert aux durées annoncées pendant la chaîne
/// « Analyser » d'une racine ([`analyserRacine`]).
///
/// L'adresse de contact n'est pas une formalité : MusicBrainz exige un agent
/// qui identifie l'application et donne un moyen de la joindre. Sans elle,
/// la passe de genres ne récolterait que des refus — `analyserRacine` la
/// saute plutôt que d'essayer.
const SECONDES_PAR_ARTISTE = 5;

/// Le transport du bas, quand ce sont les stems qui jouent.
///
/// Même barre, même minutage, mêmes commandes : seule la source change.
/// L'onde laisse la place au minutage — chaque stem a son spectrogramme dans
/// le dock, une onde du mélange n'apprendrait rien de plus.
let battementStemsEnVol = false;
async function battementStems() {
  // Même garde que `battement`, et pour la même raison : sans elle, cinq
  // sondages par seconde s'empilent sur le verrou des stems et tout ce qui
  // arrive après — un clic, par exemple — attend son tour.
  if (battementStemsEnVol) return;
  battementStemsEnVol = true;
  let e;
  try {
    e = await invoke("stems_state");
  } finally {
    battementStemsEnVol = false;
  }
  if (!e.actif && !e.en_pause) {
    await arreterStems();
    dessinerStems();
    return;
  }
  if (Date.now() >= ignorerEtatJusqua) {
    poserLecture(!e.en_pause);
  }
  $("tc").textContent = `${horloge(e.position_ms)} / ${horloge(e.duree_ms)}`;
  edition.deriveMs = e.derive_ms;
  majDerive();
  $("np-titre").textContent = `${edition.stems.length} stems`;
  $("np-artiste").textContent = edition.source
    ? `${txt(edition.source.artist, "?")} — ${txt(edition.source.title, "?")}`
    : "";

  const frac = e.duree_ms ? Math.min(1, e.position_ms / e.duree_ms) : 0;
  peindreTransport(frac);
  // La même fraction pour tout le monde : barre du bas et spectrogrammes
  // montrent le même instant parce qu'ils lisent la même valeur.
  poserTete(frac);
}

/* ---------------------------------------------------------- éditer */

// Le morceau que le mode Éditer travaille. Ce n'est pas forcément celui en
// lecture : on peut séparer un morceau tout en en écoutant un autre.
const edition = {
  source: null, // TrackRow
  variante: "htdemucs",
  // Un stem : { nom, chemin, origine, niveau, vitesse, tonalite, greffe, ouvert, voisins }
  // `chemin` est ce qu'on joue — il change quand on greffe ; `origine` est le
  // stem séparé, qu'on ne perd jamais de vue. `vitesse` et `tonalite` valent
  // `null` tant que le stem suit les réglages d'ensemble.
  stems: [],
  muets: new Set(),
  solo: null,
  enLecture: false, // un multipiste est-il chargé côté moteur ?
  tete: 0, // position de lecture, de 0 à 1 — partagée par tous les affichages
  // Vitesse et tonalité **d'ensemble**. Elles pilotent tous les stems que rien
  // n'en écarte ; un stem peut avoir les siennes (`docs/ui-spec-editeur.md`,
  // décision 4 : global par défaut, par stem en option).
  vitesse: 1.0,
  tonalite: 0,
  // Dernière dérive mesurée par le moteur, en millisecondes.
  deriveMs: 0,
};

/// Bornes et pas des deux réglages.
///
/// **Ce sont deux choses différentes, et elles ne coûtent pas la même chose.**
/// La vitesse est immédiate : la lecture avance d'un pas fractionnaire qu'on
/// écrit, rien n'est recalculé — et la hauteur ne bouge pas, `wsola` s'en
/// charge. La hauteur, elle, retraite le signal : quelques secondes de calcul
/// et 31 Mo par stem et par valeur, d'où un pas d'un demi-ton et pas de
/// glissière.
const REGLAGES = {
  vitesse: {
    pas: 0.05,
    min: 0.25,
    max: 4.0,
    ecrire: (v) => `${Math.round(v * 100)} %`,
    // **Toujours des pour cent, jamais un rapport.** « 2 » est ambigu — deux
    // pour cent ou deux fois ? Le champ affiche « % », il lit donc des pour
    // cent, et 2 est refusé par la borne basse plutôt qu'interprété.
    lire: (t) => {
      const n = Number(String(t).replace("%", "").replace(",", ".").trim());
      return Number.isFinite(n) ? n / 100 : null;
    },
    immediat: true,
  },
  tonalite: {
    pas: 1,
    min: -12,
    max: 12,
    ecrire: (v) => (v > 0 ? `+${v}` : v === 0 ? "±0" : `${v}`),
    // « ±0 » se relit, et « +3 » aussi : ce sont les formes que le champ écrit.
    lire: (t) => {
      const n = Number(String(t).replace("±", "").replace(",", ".").trim());
      return Number.isFinite(n) ? Math.round(n) : null;
    },
    immediat: false,
  },
};

/// Câble un champ de valeur : on tape, on valide, ça s'applique.
///
/// **Les pas-à-pas restent, et ce n'est pas une hésitation.** Ils servent au
/// réglage fin — chercher la bonne valeur en écoutant. Le champ sert à l'autre
/// geste, sauter d'un coup à une valeur qu'on a déjà en tête, ce qu'une
/// quinzaine de clics ne fait pas.
///
/// Une saisie hors bornes ou illisible **n'est pas corrigée en silence** : le
/// champ se marque et garde ce qui a été tapé, faute de quoi on ne saurait pas
/// ce qu'on a écrit de travers. Échap ou la perte du focus rendent la valeur
/// courante.
function cablerChamp(champ, nom, lire, poser) {
  const r = REGLAGES[nom];
  const rendre = () => {
    champ.classList.remove("reglage__val--faux");
    champ.value = r.ecrire(lire());
  };
  rendre();

  champ.addEventListener("focus", () => champ.select());
  champ.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      rendre();
      champ.blur();
    }
    // Les flèches font ce que font les boutons : c'est ce qu'on attend d'un
    // champ numérique, et cela évite de sortir du clavier pour un pas.
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      poser(calerReglage(nom, lire() + r.pas * (e.key === "ArrowUp" ? 1 : -1)));
    }
  });
  champ.addEventListener("change", () => {
    const v = r.lire(champ.value);
    if (v === null || v < r.min - 1e-9 || v > r.max + 1e-9) {
      champ.classList.add("reglage__val--faux");
      return;
    }
    champ.classList.remove("reglage__val--faux");
    poser(calerReglage(nom, v));
  });
  champ.addEventListener("blur", rendre);
  return rendre;
}

/// Cale une valeur sur les bornes et le pas de son réglage.
///
/// L'arrondi n'est pas cosmétique : sans lui, les additions de 0,05 dérivent
/// et le nom du dossier de cache change à chaque passage.
function calerReglage(nom, valeur) {
  const r = REGLAGES[nom];
  const borne = Math.min(r.max, Math.max(r.min, valeur));
  return Math.round(borne / r.pas) * r.pas;
}

/// Vitesse et hauteur effectives d'un stem : les siennes s'il en a, sinon
/// celles du dock.
function vitesseDe(s) {
  return s.vitesse ?? edition.vitesse;
}
function tonaliteDe(s) {
  return s.tonalite ?? edition.tonalite;
}
/// Ce stem s'écarte-t-il de l'ensemble, d'une façon ou d'une autre ?
function stemEcarte(s) {
  return s.vitesse !== null || s.tonalite !== null || !!s.greffe;
}

/// Pousse la vitesse d'ensemble, puis les écarts.
///
/// **L'ordre compte.** Régler la vitesse d'ensemble ramène tous les stems
/// dessus — c'est ce qu'on attend d'un réglage global, et c'est ce que fait le
/// moteur — après quoi on repose ceux qui s'en écartent. Rien n'est recalculé :
/// ce sont des flottants que la lecture relit à chaque trame.
async function appliquerVitesses() {
  if (!edition.enLecture) return;
  // L'ordre compte : la vitesse d'ensemble écrit sur **tous** les stems, les
  // écarts se reposent donc après, jamais avant.
  await invoke("stems_vitesse", { vitesse: edition.vitesse });
  for (const [i, s] of edition.stems.entries()) {
    if (s.vitesse !== null) {
      await invoke("stems_vitesse_stem", { index: i, vitesse: s.vitesse });
    }
  }

  // **On relit ce que le moteur a retenu.** Il rend déjà ses vitesses dans
  // `stems_state` et personne ne les regardait : un réglage qui n'arrivait pas
  // — commande rejetée, indices décalés — ne se voyait qu'à l'oreille, et
  // « ça ne marche pas » est tout ce qu'on pouvait en dire. Comparé ici, un
  // écart se nomme.
  try {
    const e = await invoke("stems_state");
    const attendu = edition.stems.map(vitesseDe);
    const ecarts = attendu
      .map((v, i) => [i, v, e.vitesses?.[i]])
      .filter(([, v, lu]) => lu === undefined || Math.abs(lu - v) > 1e-3);
    if (ecarts.length) {
      remonter(
        ecarts.map(([i, v, lu]) => `stem ${i} : demandé ${v}, moteur ${lu}`).join(" · "),
        "vitesses non appliquées",
      );
    }
  } catch (e) {
    remonter(e, "relecture des vitesses");
  }
  majDerive();
}

/// Applique un réglage, et **montre l'échec s'il y en a un**.
///
/// Les gestionnaires de clic n'avaient pas de garde : une commande rejetée
/// partait dans le rapporteur global, invisible depuis l'interface, et le
/// réglage paraissait simplement sans effet.
async function appliquerUn(nom) {
  try {
    if (REGLAGES[nom].immediat) await appliquerVitesses();
    else await appliquerReglages();
  } catch (e) {
    $("dock-aide").textContent = `échec du réglage « ${nom} »`;
    remonter(e, `réglage ${nom}`);
  }
}

/// Recalcule les stems transposés et les remet en lecture au même instant.
///
/// C'est le seul chemin qui recharge le multipiste : transposition, greffe et
/// retrait de greffe y passent tous. Le calcul dure quelques secondes par stem
/// transposé, et le moteur garde chaque valeur sur le disque — revenir à une
/// hauteur déjà entendue est immédiat, et changer celle d'un seul stem ne
/// recalcule que celui-là.
async function appliquerReglages() {
  if (!edition.stems.length) return;
  const boutons = document.querySelectorAll(".reglage button");
  boutons.forEach((b) => (b.disabled = true));
  const avant = edition.enLecture ? await invoke("stems_state") : null;
  $("dock-aide").textContent = "calcul…";
  try {
    const traites = await transposerStems(
      edition.stems.map((s) => [s.nom, s.chemin]),
      edition.stems.map(tonaliteDe),
    );
    await invoke("stems_play", { stems: traites });
    edition.enLecture = true;
    await appliquerVitesses();
    await appliquerNiveaux();
    if (avant) {
      // La position se conserve **en proportion** : le rechargement remet
      // toutes les têtes à zéro, y compris celles qui avaient dérivé.
      const frac = avant.duree_ms ? avant.position_ms / avant.duree_ms : 0;
      const e = await invoke("stems_state");
      await invoke("stems_transport", {
        action: "deplacer",
        position: (frac * e.duree_ms) / 1000,
      });
      if (avant.en_pause) await invoke("stems_transport", { action: "pause", position: null });
    }
    $("dock-aide").textContent = "clic sur un spectrogramme : se déplacer";
  } catch (e) {
    $("dock-aide").textContent = "";
    remonter(e, "vitesse et hauteur");
  }
  boutons.forEach((b) => (b.disabled = false));
  sonder(true);
}

function dessinerReglages() {
  for (const [nom, r] of Object.entries(REGLAGES)) {
    const champ = $(`val-${nom}`);
    // Ne pas écraser une saisie en cours : le sondage et les redessins
    // passent ici plusieurs fois par seconde.
    if (document.activeElement === champ) continue;
    champ.classList.remove("reglage__val--faux");
    champ.value = r.ecrire(edition[nom]);
  }
}

for (const nom of Object.keys(REGLAGES)) {
  cablerChamp(
    $(`val-${nom}`),
    nom,
    () => edition[nom],
    async (v) => {
      edition[nom] = v;
      dessinerReglages();
      if (REGLAGES[nom].immediat) await appliquerVitesses();
      else await appliquerReglages();
      dessinerStems();
    },
  );
}

dessinerReglages();

document.querySelectorAll(".dock__tete .reglage button").forEach((b) => {
  b.addEventListener("click", async () => {
    const nom = b.dataset.r;
    edition[nom] = calerReglage(nom, edition[nom] + REGLAGES[nom].pas * Number(b.dataset.d));
    dessinerReglages();
    if (REGLAGES[nom].immediat) {
      // Un flottant à écrire, rien de plus : ni recalcul, ni rechargement, ni
      // perte de position. Les stems qui s'en écartent sont reposés ensuite.
      await appliquerVitesses();
    } else {
      await appliquerReglages();
    }
    dessinerStems();
  });
});

/// Montre la dérive — et seulement quand il y en a une à montrer.
///
/// **La mesure, pas la mise en garde.** « Les stems peuvent se
/// désynchroniser » ne dit rien qu'on puisse vérifier ; « 1,4 s d'écart » se
/// contrôle à l'oreille et se corrige d'un bouton. Un avertissement permanent,
/// lui, ne serait pas lu.
function majDerive() {
  const el = $("derive");
  if (!el) return;
  const ecarte = edition.stems.some(
    (s) => s.vitesse !== null && Math.abs(s.vitesse - edition.vitesse) > 1e-3,
  );
  el.hidden = !ecarte || !edition.enLecture;
  if (el.hidden) return;
  const ms = edition.deriveMs || 0;
  $("derive-txt").textContent =
    ms > 250 ? `vitesses différentes — ${(ms / 1000).toFixed(1)} s d'écart` : "vitesses différentes";
}

$("realigner").addEventListener("click", async () => {
  try {
    await invoke("stems_transport", { action: "realigner", position: null });
  } catch (e) {
    remonter(e, "réalignement");
  }
});

/// Le morceau sur lequel travailler : celui de l'inspecteur, sinon celui en
/// lecture. L'inspecteur suit la sélection, c'est donc lui qui exprime
/// l'intention la plus récente.
function morceauAEditer() {
  const path = $("insp-titre").dataset.path || enLecture;
  return fileCourante.find((t) => t.path === path) ?? null;
}

function poserSourceEdition() {
  const t = morceauAEditer();
  edition.source = t;
  $("demix-source").textContent = t
    ? `${txt(t.artist, "?")} — ${txt(t.title, "?")}`
    : "Choisis un morceau dans la liste ou sur la carte.";
  $("lancer-demix").disabled = !t;
  if (t) chargerStemsExistants(t);
}

/// Les stems prennent la main sur la lecture, au même instant et dans le même
/// état.
///
/// **Sans cela, le dock montrait des stems inertes** : ils étaient affichés
/// mais pas chargés, si bien que solo et coupure n'agissaient sur rien et que
/// le bouton du bas commandait encore le morceau mêlé. Il fallait relancer la
/// lecture depuis le mode Éditer pour que quoi que ce soit réponde.
///
/// La règle est désormais simple : **si le dock montre des stems, ce sont eux
/// la source.** Le morceau mêlé se tait, les stems reprennent à sa position et
/// dans son état de lecture.
async function prendreLaMain() {
  if (!edition.stems.length || edition.enLecture) return;
  const avant = await invoke("playback_state");
  // Reprendre la position n'a de sens que si on écoutait bien ce morceau-là.
  const memeMorceau = avant.current && edition.source && avant.current === edition.source.path;

  await lireStems();
  if (!edition.enLecture) return;

  if (memeMorceau && avant.position_ms > 0) {
    await invoke("stems_transport", {
      action: "deplacer",
      position: avant.position_ms / 1000,
    });
  }
  // On n'impose pas la lecture : si rien ne jouait, les stems attendent.
  if (!memeMorceau || avant.paused || avant.finished) {
    await invoke("stems_transport", { action: "pause", position: null });
    poserLecture(false);
  } else {
    poserLecture(true);
  }
}

/// Retrouve un démixage d'une session précédente.
///
/// Une séparation coûte une trentaine de secondes : on ne la rejoue pas parce
/// que la fenêtre a été fermée.
/// Un stem fraîchement séparé : il suit l'ensemble et n'a pas de greffe.
///
/// `origine` ne bouge plus ensuite. C'est ce qui permet de retirer une greffe
/// sans rien recalculer, et c'est le fichier sur lequel la greffe suivante se
/// calera — greffer sur une greffe empilerait les étirements.
function stemNeuf([nom, chemin]) {
  return {
    nom,
    chemin,
    origine: chemin,
    niveau: 1,
    vitesse: null,
    tonalite: null,
    greffe: null,
    ouvert: false,
    voisins: null,
  };
}

async function chargerStemsExistants(t) {
  const trouves = await invoke("stems_existants", { path: t.path });
  if (trouves.length) {
    edition.stems = trouves.map(stemNeuf);
    dessinerStems();
    $("demix-etat").textContent = `${trouves.length} stems déjà calculés`;
    if (modeCourant === "editer") await prendreLaMain();
  } else {
    edition.stems = [];
    dessinerStems();
    $("demix-etat").textContent = "";
  }
}

document.querySelectorAll("[data-variante]").forEach((b) =>
  b.addEventListener("click", () => {
    edition.variante = b.dataset.variante;
    document
      .querySelectorAll("[data-variante]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
  }),
);

let sondageDemix = null;
$("lancer-demix").addEventListener("click", async () => {
  const t = edition.source;
  if (!t) return;
  try {
    await invoke("start_demix", { path: t.path, variant: edition.variante });
  } catch (e) {
    $("demix-etat").textContent = String(e);
    return;
  }
  $("lancer-demix").disabled = true;
  $("demix-etat").textContent = "séparation en cours… (compter ~30 s par morceau)";

  clearInterval(sondageDemix);
  sondageDemix = setInterval(async () => {
    const d = await invoke("demix_state");
    if (d.en_cours) return;
    clearInterval(sondageDemix);
    sondageDemix = null;
    $("lancer-demix").disabled = false;
    $("demix-etat").textContent = d.resultat ?? "";
    if (edition.enLecture) await arreterStems();
    edition.stems = d.stems.map(stemNeuf);
    edition.solo = null;
    edition.muets.clear();
    dessinerStems();
    // Même règle qu'au chargement d'un démixage existant : afficher des stems,
    // c'est en faire la source. Sans cela, solo, coupure, vitesse et hauteur
    // n'agissaient sur rien tant qu'on n'avait pas relancé la lecture.
    await prendreLaMain();
  }, 1500);
});

function dessinerStems() {
  const hote = $("dock-pistes");
  hote.replaceChildren();
  $("dock").hidden = edition.stems.length === 0 || modeCourant !== "editer";
  $("dock-source").textContent = edition.source ? txt(edition.source.title, "?") : "";
  if (!edition.enLecture) {
    $("dock-aide").textContent = edition.stems.length ? "▶ en bas pour écouter les stems" : "";
  }
  majDerive();

  for (const s of edition.stems) {
    // La ligne et, replié dessous, ce qui ne concerne que ce stem. Replié
    // parce que la ligne porte déjà quatre commandes : un éditeur est
    // précisément l'endroit où l'Atelier retomberait en panneau
    // d'administration (`docs/ui-spec-editeur.md`).
    const piste = document.createElement("div");
    piste.className = "piste";

    const el = document.createElement("div");
    el.className = "stem";
    const muet = edition.muets.has(s.nom);
    const solo = edition.solo === s.nom;
    // Le solo l'emporte : c'est la convention de toutes les tables de mixage.
    const audible = solo || (edition.solo === null && !muet);
    el.classList.toggle("stem--muet", !audible);
    el.classList.toggle("stem--solo", solo);

    el.innerHTML = `<span class="stem__nom"></span>
                    <button class="stem__b" data-a="solo">solo</button>
                    <button class="stem__b" data-a="muet">muet</button>
                    <button class="stem__b" data-a="regler"></button>
                    <span class="stem__jauge" title="Niveau — tirer pour régler"><i></i></span>
                    <canvas class="stem__spectre"></canvas>`;
    el.children[0].textContent = s.nom;
    el.children[1].classList.toggle("stem__b--actif", solo);
    el.children[2].classList.toggle("stem__b--actif", muet);
    el.querySelector("i").style.width = `${(audible ? s.niveau : 0) * 100}%`;

    // Le badge dit ce que ce stem a de particulier — et « régler » quand il
    // n'a rien de particulier à dire.
    const badge = el.children[3];
    badge.textContent = etiquetteStem(s);
    badge.classList.toggle("stem__b--regle", stemEcarte(s));
    badge.title = "Vitesse, hauteur et remplacement de ce stem seul";

    el.querySelector('[data-a="solo"]').addEventListener("click", () => {
      edition.solo = solo ? null : s.nom;
      dessinerStems();
      appliquerNiveaux();
    });
    el.querySelector('[data-a="muet"]').addEventListener("click", () => {
      if (muet) edition.muets.delete(s.nom);
      else edition.muets.add(s.nom);
      dessinerStems();
      appliquerNiveaux();
    });
    badge.addEventListener("click", () => {
      s.ouvert = !s.ouvert;
      dessinerStems();
    });

    // Le niveau se tire, comme sur une table : cliquer pose, glisser ajuste.
    const jauge = el.querySelector(".stem__jauge");
    const regler = (ev) => {
      const r = jauge.getBoundingClientRect();
      s.niveau = Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width));
      jauge.firstElementChild.style.width = `${s.niveau * 100}%`;
      appliquerNiveaux();
    };
    jauge.addEventListener("mousedown", (ev) => {
      regler(ev);
      const bouger = (e) => regler(e);
      const lacher = () => {
        window.removeEventListener("mousemove", bouger);
        window.removeEventListener("mouseup", lacher);
      };
      window.addEventListener("mousemove", bouger);
      window.addEventListener("mouseup", lacher);
    });

    const cnv = el.querySelector("canvas");
    // N'importe quel spectrogramme sert de règle : c'est le même axe des
    // temps que la barre du bas, donc le même geste.
    cnv.addEventListener("click", async (ev) => {
      const r = cnv.getBoundingClientRect();
      await deplacerLecture((ev.clientX - r.left) / r.width);
    });

    piste.appendChild(el);
    if (s.ouvert) piste.appendChild(panneauStem(s, badge));
    hote.appendChild(piste);
    dessinerSpectre(cnv, s);
  }
}

/// Ce que le badge d'une ligne affiche : rien de particulier, ou quoi.
function etiquetteStem(s) {
  const bouts = [];
  if (s.vitesse !== null) bouts.push(REGLAGES.vitesse.ecrire(s.vitesse));
  if (s.tonalite !== null) bouts.push(REGLAGES.tonalite.ecrire(s.tonalite));
  if (s.greffe) bouts.push("greffé");
  return bouts.length ? bouts.join(" · ") : "régler";
}

/// Le panneau d'un stem : sa vitesse, sa hauteur, et d'où le remplacer.
function panneauStem(s, badge) {
  const pan = document.createElement("div");
  pan.className = "stem__pan";

  const ligne = document.createElement("div");
  ligne.className = "stem__ligne";
  ligne.innerHTML = `
    <div class="reglage" role="group" aria-label="Vitesse de ce stem"
         title="Vitesse de ce stem seul. Il n'avance alors plus au même pas que les autres, et l'écart grandit tant que la lecture continue.">
      <b>vitesse</b>
      <button data-r="vitesse" data-d="-1" aria-label="Ralentir ce stem">−</button>
      <input class="reglage__val" data-c="vitesse" inputmode="decimal"
             aria-label="Vitesse de ce stem, en pour cent">
      <button data-r="vitesse" data-d="1" aria-label="Accélérer ce stem">+</button>
    </div>
    <div class="reglage" role="group" aria-label="Hauteur de ce stem"
         title="Transposition de ce stem seul, à durée inchangée. Quelques secondes de calcul, mises en cache.">
      <b>hauteur</b>
      <button data-r="tonalite" data-d="-1" aria-label="Baisser ce stem d'un demi-ton">−</button>
      <input class="reglage__val" data-c="tonalite" inputmode="numeric"
             aria-label="Transposition de ce stem, en demi-tons">
      <button data-r="tonalite" data-d="1" aria-label="Monter ce stem d'un demi-ton">+</button>
    </div>
    <button class="stem__b" data-a="ensemble">suivre l'ensemble</button>`;
  pan.appendChild(ligne);

  const groupes = ligne.querySelectorAll(".reglage");
  const ensemble = ligne.querySelector('[data-a="ensemble"]');
  const champs = {};
  const rafraichir = () => {
    for (const [nom, champ] of Object.entries(champs)) {
      if (document.activeElement === champ) continue;
      champ.classList.remove("reglage__val--faux");
      champ.value = REGLAGES[nom].ecrire(nom === "vitesse" ? vitesseDe(s) : tonaliteDe(s));
    }
    ensemble.disabled = s.vitesse === null && s.tonalite === null;
    badge.textContent = etiquetteStem(s);
    badge.classList.toggle("stem__b--regle", stemEcarte(s));
    majDerive();
  };

  // Les champs se câblent avant le premier rafraîchissement : c'est lui qui
  // pose leur valeur.
  for (const champ of ligne.querySelectorAll(".reglage__val")) {
    const nom = champ.dataset.c;
    champs[nom] = champ;
    cablerChamp(
      champ,
      nom,
      () => (nom === "vitesse" ? vitesseDe(s) : tonaliteDe(s)),
      async (v) => {
        s[nom] = v;
        rafraichir();
        await appliquerUn(nom);
      },
    );
  }
  rafraichir();

  ligne.querySelectorAll(".reglage button").forEach((b) => {
    b.addEventListener("click", async () => {
      const nom = b.dataset.r;
      // Un stem qui suivait l'ensemble part de la valeur d'ensemble : le
      // premier clic déplace d'un pas, il ne saute pas à 100 %.
      const depart = nom === "vitesse" ? vitesseDe(s) : tonaliteDe(s);
      s[nom] = calerReglage(nom, depart + REGLAGES[nom].pas * Number(b.dataset.d));
      rafraichir();
      await appliquerUn(nom);
    });
  });

  ensemble.addEventListener("click", async () => {
    const transpose = s.tonalite !== null && s.tonalite !== edition.tonalite;
    s.vitesse = null;
    s.tonalite = null;
    rafraichir();
    await appliquerVitesses();
    // Réaligner par la même occasion : remettre les vitesses à égalité arrête
    // la dérive mais laisserait l'écart déjà pris.
    await invoke("stems_transport", { action: "realigner", position: null });
    if (transpose) await appliquerReglages();
  });

  pan.appendChild(zoneGreffe(s));
  return pan;
}

/// La partie « remplacer » du panneau : le bouton, la greffe posée, ou la
/// liste des morceaux d'où tirer le stem.
function zoneGreffe(s) {
  const zone = document.createElement("div");

  if (s.greffe) {
    const note = document.createElement("p");
    note.className = "stem__note";
    note.textContent = decrireGreffe(s.greffe);
    zone.appendChild(note);

    // Dit une fois, là où c'est utile : la limite est réelle, et le recours
    // est à portée de main, juste au-dessus.
    const garde = document.createElement("p");
    garde.className = "stem__note";
    garde.textContent =
      "Les tempos concordent, pas les temps forts — il n'y a pas de grille de battements. La vitesse de ce stem sert à recaler.";
    zone.appendChild(garde);

    const ligne = document.createElement("div");
    ligne.className = "stem__ligne";
    const retirer = document.createElement("button");
    retirer.className = "stem__b";
    retirer.textContent = "retirer la greffe";
    retirer.addEventListener("click", async () => {
      s.chemin = s.origine;
      s.greffe = null;
      s.spectre = null;
      dessinerStems();
      await appliquerReglages();
    });
    ligne.appendChild(retirer);
    zone.appendChild(ligne);
  }

  if (!s.voisins) {
    const ligne = document.createElement("div");
    ligne.className = "stem__ligne";
    const b = document.createElement("button");
    b.className = "stem__b";
    b.textContent = s.greffe ? "en essayer un autre" : "remplacer par…";
    b.title = "Cherche, parmi les voisins soniques, ceux dont le tempo permet d'échanger ce stem";
    b.addEventListener("click", async () => {
      b.disabled = true;
      b.textContent = "recherche…";
      try {
        s.voisins = await invoke("voisins_de_stem", { id: edition.source.id, count: 12 });
      } catch (e) {
        remonter(e, "voisins pour la greffe");
        b.disabled = false;
        b.textContent = "remplacer par…";
        return;
      }
      dessinerStems();
    });
    ligne.appendChild(b);
    zone.appendChild(ligne);
    return zone;
  }

  const r = s.voisins;
  const note = document.createElement("p");
  note.className = "stem__note";
  if (!r.bpm) {
    // Sans tempo, rien n'est calable — et la cause est nommée avec le remède.
    note.textContent =
      "Ce morceau n'a pas de tempo mesuré : rien sur quoi caler un stem. Lance « Descripteurs » depuis les Réglages.";
    zone.appendChild(note);
    return zone;
  }
  // Ce qui a été écarté vaut d'être dit : sans les deux comptes, une liste
  // courte passe pour une bibliothèque pauvre en voisins.
  note.textContent =
    `${r.candidats.length} voisin${r.candidats.length > 1 ? "s" : ""} au tempo de ` +
    `${Math.round(r.bpm)} BPM — ${r.ecartes} écarté${r.ecartes > 1 ? "s" : ""} pour leur tempo, ` +
    `${r.sans_tempo} sans tempo mesuré.`;
  zone.appendChild(note);

  if (!r.candidats.length) {
    const vide = document.createElement("p");
    vide.className = "stem__note";
    vide.textContent = "Aucun voisin ne tombe assez près du tempo pour être échangé.";
    zone.appendChild(vide);
    return zone;
  }

  const liste = document.createElement("div");
  liste.className = "candidats";
  for (const c of r.candidats) {
    const b = document.createElement("button");
    b.className = "candidat";
    b.innerHTML = `<b></b><span></span><span></span>`;
    b.children[0].textContent = `${txt(c.artist, "?")} — ${txt(c.title, "?")}`;
    const octave = c.octaves ? ` ${c.octaves > 0 ? "×" : "÷"}${2 ** Math.abs(c.octaves)}` : "";
    b.children[1].textContent = `${Math.round(c.bpm)} BPM${octave}`;
    // Trente secondes de séparation, ou rien : c'est ce qui décide du clic.
    b.children[2].textContent = c.separe ? "séparé" : "~30 s";
    b.addEventListener("click", () => greffer(s, c, liste));
    liste.appendChild(b);
  }
  zone.appendChild(liste);
  return zone;
}

/// Ce qu'il a fallu faire au greffon, dit plutôt que laissé à deviner.
function decrireGreffe(g) {
  const bouts = [g.origine];
  const pct = Math.round((g.facteur - 1) * 1000) / 10;
  bouts.push(pct === 0 ? "au même tempo" : `étiré de ${pct > 0 ? "+" : ""}${pct} %`);
  if (g.octaves > 0) bouts.push(`joué à ${2 ** g.octaves} × son tempo`);
  if (g.octaves < 0) bouts.push(`joué à son tempo divisé par ${2 ** -g.octaves}`);
  if (g.retard_s > 0.05) bouts.push(`entrée à ${g.retard_s.toFixed(1)} s`);
  bouts.push(g.boucles > 1 ? `${g.boucles} passages` : "un passage");
  // Le calage change ce qu'on entend, pas seulement ce qu'on a calculé : sans
  // lui les deux matières pulsent au même tempo sans tomber sur le même temps,
  // et il faut le rattraper à la main avec la vitesse de ce stem.
  bouts.push(g.cale_aux_temps ? "calé sur les temps" : "calé sur la première attaque");
  return bouts.join(" · ");
}

/// Va chercher le stem d'un autre morceau et le met à la place de celui-ci.
///
/// Deux attentes possibles, et toutes deux annoncées : la séparation du
/// morceau voisin s'il n'est pas déjà séparé, puis le calage lui-même —
/// l'étirement d'un stem entier se compte en dizaines de secondes.
async function greffer(s, candidat, liste) {
  liste.querySelectorAll("button").forEach((b) => (b.disabled = true));
  try {
    if (!candidat.separe) {
      $("dock-aide").textContent = "séparation du morceau voisin…";
      await invoke("start_demix", { path: candidat.path, variant: edition.variante });
      await attendreDemix();
    }
    $("dock-aide").textContent = "calage du stem…";
    const g = await invoke("stems_greffer", {
      id: edition.source.id,
      stem: s.nom,
      // Toujours le stem d'origine : greffer sur une greffe empilerait les
      // étirements et le retard.
      remplace: s.origine,
      voisin: candidat.id,
    });
    s.chemin = g.chemin;
    s.greffe = g;
    s.spectre = null; // le spectrogramme montre autre chose, maintenant
    s.voisins = null;
    dessinerStems();
    await appliquerReglages();
  } catch (e) {
    $("dock-aide").textContent = "";
    remonter(e, "greffe");
    liste.querySelectorAll("button").forEach((b) => (b.disabled = false));
  }
}

/// Attend la fin d'un démixage lancé pour une greffe.
///
/// Le sondage est le même que celui du bouton « Séparer » — le moteur ne rend
/// pas de rapport intermédiaire — mais il ne touche pas au morceau ouvert :
/// c'est le voisin qu'on sépare, pas lui.
/// Lance la transposition et attend qu'elle finisse, en disant où elle en est.
///
/// **Le calcul ne bloque plus l'interface, et c'est le but de tout ceci.** Une
/// transposition demande une vingtaine de secondes par stem depuis le passage à
/// `wsola` ; la faire dans une commande qui ne rend pas la main mettait toutes
/// les autres en file d'attente derrière elle — le transport, l'état de
/// lecture, le moindre clic. Le moteur la fait maintenant dans son fil, et
/// l'interface sonde.
///
/// Le premier appel rend déjà l'état : quand tout est neutre ou en cache, il
/// arrive avec `en_cours: false` et l'on ne sonde pas du tout.
async function transposerStems(stems, demiTons) {
  const depart = await invoke("start_etirer", { stems, demiTons });
  if (!depart.en_cours) {
    if (depart.erreur) throw depart.erreur;
    return depart.stems;
  }

  return new Promise((resolve, reject) => {
    const t = setInterval(async () => {
      let e;
      try {
        e = await invoke("etirer_state");
      } catch (err) {
        clearInterval(t);
        reject(err);
        return;
      }
      // Un compte de stems, pas un pourcentage : à vingt secondes pièce, un
      // pourcentage global reste immobile assez longtemps pour qu'on le croie
      // bloqué.
      if (e.en_cours) {
        $("dock-aide").textContent = `transposition ${e.faits + 1}/${e.total}…`;
        return;
      }
      clearInterval(t);
      if (e.erreur) reject(e.erreur);
      else resolve(e.stems);
    }, 400);
  });
}

function attendreDemix() {
  return new Promise((resolve, reject) => {
    const t = setInterval(async () => {
      let d;
      try {
        d = await invoke("demix_state");
      } catch (e) {
        clearInterval(t);
        reject(e);
        return;
      }
      if (d.en_cours) return;
      clearInterval(t);
      if (d.stems.length) resolve(d);
      else reject(d.resultat ?? "la séparation du morceau voisin a échoué");
    }, 1500);
  });
}

/// Dessine le spectrogramme d'un stem, une fois, en mémoire.
///
/// Le calcul revient au moteur ; l'interface ne fait que colorer avec la rampe
/// séquentielle déjà retenue pour la carte. Une rampe n'oppose pas des
/// identités — c'est ce qui l'autorise là où trois teintes catégorielles
/// seraient déjà de trop.
async function dessinerSpectre(cnv, stem) {
  const largeur = Math.max(120, Math.round(cnv.getBoundingClientRect().width));
  const hauteur = 46;

  if (!stem.spectre) {
    try {
      stem.spectre = await invoke("stem_spectre", {
        path: stem.chemin,
        width: largeur,
        height: hauteur,
      });
    } catch (e) {
      remonter(e, "spectrogramme");
      return;
    }
  }
  // Le dock a pu être redessiné pendant le calcul : on ne peint que si ce
  // canevas est encore à l'écran.
  if (!cnv.isConnected) return;

  const { largeur: w, hauteur: h, pixels } = stem.spectre;
  // L'image est peinte une fois hors écran ; à chaque battement on la recopie
  // et on trace la tête de lecture par-dessus. Recalculer le spectrogramme
  // cinq fois par seconde serait absurde.
  const fond = document.createElement("canvas");
  fond.width = w;
  fond.height = h;
  const fctx = fond.getContext("2d");
  const img = fctx.createImageData(w, h);
  const table = rampeRGB();
  for (let i = 0; i < pixels.length; i++) {
    const c = table[pixels[i]];
    img.data[i * 4] = c[0];
    img.data[i * 4 + 1] = c[1];
    img.data[i * 4 + 2] = c[2];
    img.data[i * 4 + 3] = 255;
  }
  fctx.putImageData(img, 0, 0);

  cnv.width = w;
  cnv.height = h;
  stem.fond = fond;
  stem.canvas = cnv;
  peindreSpectre(stem);
}

/// Repose le spectrogramme et trace la tête de lecture dessus.
///
/// Une seule tête pour toute la fenêtre : les spectrogrammes et la barre du
/// bas montrent le même instant, puisqu'ils montrent le même morceau.
function peindreSpectre(stem) {
  if (!stem.fond || !stem.canvas || !stem.canvas.isConnected) return;
  const cnv = stem.canvas;
  const ctx = cnv.getContext("2d");
  ctx.drawImage(stem.fond, 0, 0);

  const x = Math.round(edition.tete * cnv.width);
  // Ce qui est déjà passé s'assombrit : la position se lit même quand le
  // trait tombe sur une zone claire du spectre.
  ctx.fillStyle = "rgba(0, 0, 0, .38)";
  ctx.fillRect(0, 0, x, cnv.height);
  ctx.fillStyle =
    getComputedStyle(document.documentElement).getPropertyValue("--txt").trim() || "#EDE8DC";
  ctx.fillRect(x, 0, 1, cnv.height);
}

/// Déplace la tête de lecture sur tous les spectrogrammes à la fois.
function poserTete(frac) {
  edition.tete = Math.min(1, Math.max(0, frac || 0));
  for (const s of edition.stems) peindreSpectre(s);
}

/// Table de 256 couleurs interpolée sur `--rampe`, calculée une fois.
let tableRampe = null;
function rampeRGB() {
  if (tableRampe) return tableRampe;
  const etapes = rampe().map((c) => {
    const n = parseInt(c.replace("#", ""), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  });
  tableRampe = [];
  for (let i = 0; i < 256; i++) {
    const t = (i / 255) * (etapes.length - 1);
    const a = etapes[Math.floor(t)];
    const b = etapes[Math.min(etapes.length - 1, Math.floor(t) + 1)];
    const f = t - Math.floor(t);
    tableRampe.push([
      Math.round(a[0] + (b[0] - a[0]) * f),
      Math.round(a[1] + (b[1] - a[1]) * f),
      Math.round(a[2] + (b[2] - a[2]) * f),
    ]);
  }
  return tableRampe;
}

/// Niveau de chaque stem, dans l'ordre affiché.
///
/// Le solo l'emporte sur la coupure — convention de toutes les tables de
/// mixage : mettre une piste en solo n'oblige pas à démuter les autres.
function niveaux() {
  return edition.stems.map((s) => {
    const audible =
      edition.solo === s.nom || (edition.solo === null && !edition.muets.has(s.nom));
    return audible ? s.niveau : 0.0;
  });
}

async function appliquerNiveaux() {
  if (!edition.enLecture) return;
  await invoke("stems_gain", { levels: niveaux() });
}

/// Déplace la lecture, que ce soient les stems ou le lecteur ordinaire.
///
/// Un seul chemin pour un seul geste : cliquer sur la barre du bas ou sur
/// n'importe quel spectrogramme revient au même, et la tête bouge partout.
async function deplacerLecture(frac) {
  frac = Math.min(1, Math.max(0, frac));
  if (edition.enLecture) {
    const e = await invoke("stems_state");
    await invoke("stems_transport", {
      action: "deplacer",
      position: (e.duree_ms / 1000) * frac,
    });
    poserTete(frac);
    await battement();
    return;
  }
  const t = fileCourante.find((x) => x.path === enLecture);
  if (!t?.duration_ms) return;
  await invoke("seek", { positionMs: Math.round(frac * t.duration_ms) });
  poserTete(frac);
}

/// Met les stems en lecture simultanée.
///
/// Le chargement décode tout en mémoire — 186 Mo pour quatre stems d'un
/// morceau de quatre minutes. C'est ce qui rend le solo instantané et le
/// déplacement gratuit.
async function lireStems() {
  if (!edition.stems.length || edition.enLecture) return;
  $("dock-aide").textContent = "chargement des stems…";
  try {
    // Passe par le traitement même à réglages neutres : le moteur rend alors
    // les chemins d'origine sans rien calculer, et il n'y a qu'un chemin de
    // code à suivre pour charger des stems.
    const stems = await transposerStems(
      edition.stems.map((s) => [s.nom, s.chemin]),
      edition.stems.map(tonaliteDe),
    );
    await invoke("stems_play", { stems });
    // Posé avant les vitesses : `appliquerVitesses` ne parle au moteur que si
    // un multipiste est chargé, et il l'est à partir d'ici.
    edition.enLecture = true;
    await appliquerVitesses();
  } catch (e) {
    edition.enLecture = false;
    $("dock-aide").textContent = "";
    $("demix-etat").textContent = String(e);
    return;
  }
  await appliquerNiveaux();
  $("dock-aide").textContent = "clic sur un spectrogramme : se déplacer";
  sonder(true);
}

async function arreterStems() {
  if (!edition.enLecture) return;
  edition.enLecture = false;
  await invoke("stems_transport", { action: "arreter", position: null });
  $("dock-aide").textContent = "";
  poserTete(0);
}

$("dock-fermer").addEventListener("click", async () => {
  if (edition.enLecture) await arreterStems();
  $("dock").hidden = true;
});

/* ---------------------------------------------------------- démarrage */

/// Recharge une vue de premier niveau — Albums par défaut à l'ouverture, ou
/// Artistes/Albums depuis le commutateur du rail. Aussi le point de retour
/// après un scan, l'oubli d'une racine, ou l'effacement de la recherche : dans
/// ces trois cas `quoi` est omis, et c'est la vue déjà affichée qui revient.
async function charger(quoi = sommet.quoi) {
  const [artistes, racines] = await Promise.all([invoke("artists"), invoke("roots")]);
  if (quoi === "albums") {
    poser("albums", "Albums", await invoke("albums", { artist: null, mbid: null }));
  } else {
    poser("artistes", "Artistes", artistes);
  }

  const total = racines.reduce((n, r) => n + r.tracks, 0);
  $("sommaire").textContent = `${total.toLocaleString("fr-FR")} morceaux\n${artistes.length.toLocaleString("fr-FR")} artistes`;
  $("sommaire").style.whiteSpace = "pre-line";
}

charger("albums")
  .then(() => chargerFamillesParAlbum())
  .then(() => {
    dessinerFamillesEcoute();
    rafraichirGrille();
  })
  .catch((e) => {
    $("fil-titre").textContent = "Erreur";
    $("sommaire").textContent = String(e);
  });


// Mode imposé par l'environnement, s'il y en a un. Voir `mode_initial`.
invoke("mode_initial")
  .then((m) => {
    if (m) basculerMode(m);
  })
  .catch(() => {});

/* ------------------------------------------------------------- itinéraires
 *
 * Le moteur vit dans `analysis::reseau` : un seul graphe, plusieurs fonctions
 * de coût. Cette section ne fait que poser la demande et rendre le résultat
 * — dont le **dénivelé**, la popularité le long du trajet, que le document
 * prévoyait pour le profil d'altitude.
 */

let itinProfil = "autoroute";

document.querySelectorAll("[data-profil]").forEach((b) =>
  b.addEventListener("click", () => {
    itinProfil = b.dataset.profil;
    document
      .querySelectorAll("[data-profil]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
    retracerItineraireSiPret();
  }),
);

const majReglette = (id, sortie, rendu) => {
  const e = $(id);
  if (!e) return;
  const poser = () => ($(sortie).textContent = rendu(e.value));
  e.addEventListener("input", poser);
  e.addEventListener("change", retracerItineraireSiPret);
  poser();
};
// 0 minute = pas de contrainte de durée (il faut alors une arrivée).
majReglette("itin-minutes", "itin-minutes-val", (v) => (+v > 0 ? `${v} min` : "libre"));

const barresDenivele = (popularite) =>
  popularite.map((p) => "▁▂▃▄▅▆▇█"[Math.min(7, Math.round(p * 7))]).join("");

/// Pose l'itinéraire calculé (un seul trajet — le choix, c'est le profil).
async function poserItineraire(t) {
  const surVoirie = typeof t.distance_m === "number";
  await poserChemin(t.pistes, "aucun itinéraire", t.polyligne || null);
  const distance = surVoirie
    ? `${(t.distance_m / 1000).toFixed(1)} km`
    : `distance ${t.distance_sonique.toFixed(2)}`;
  $("itin-etat").textContent =
    `${t.pistes.length} morceaux · ${(t.duree_ms / 60000).toFixed(0)} min · ` +
    `${distance} · ${barresDenivele(t.popularite)}`;
}

/// L'itinéraire musical (graphe des voisins) — le comportement d'origine, et le
/// repli quand la voirie ne peut pas répondre (pas de ville, morceau sans
/// adresse, ou vue Points).
async function itineraireMusical(minutes) {
  const trajets = await invoke("itineraire", {
    depart: carte.depart.id,
    arrivee: carte.arrivee ? carte.arrivee.id : null,
    profil: itinProfil,
    minutes: minutes > 0 ? minutes : null,
  });
  if (!trajets.length) {
    $("itin-etat").textContent = "aucun itinéraire";
    return;
  }
  await poserItineraire(trajets[0]);
}

/// Trace l'itinéraire courant : sur voirie si le plan de ville est actif,
/// musical sinon (ou en repli). Appelé automatiquement quand une borne est
/// posée ou un réglage changé, comme `direct`/`dessiné` — plus besoin du
/// bouton « Tracer » sur la carte de Paris.
async function tracerItineraire() {
  const etat = $("itin-etat");
  if (!carte.depart) {
    etat.textContent = "choisir un morceau de départ sur la carte";
    return;
  }
  const minutes = Number($("itin-minutes").value);
  etat.textContent = carteReelle()
    ? "calcul de l'itinéraire…"
    : "calcul… (le réseau se construit une fois par session)";
  if ($("itin-tracer")) $("itin-tracer").disabled = true;
  try {
    if (!carteReelle()) {
      await itineraireMusical(minutes);
      return;
    }
    const reponse = await invoke("itineraire_voirie", {
      depart: carte.depart.id,
      arrivee: carte.arrivee ? carte.arrivee.id : null,
      profil: itinProfil,
      minutes: minutes > 0 ? minutes : null,
      famille: carte.isolee,
      rayonM: null,
    });
    if (reponse.repli) {
      // Pas de ville / tuiles pas prêtes : le musical se suffit à lui-même.
      // Sinon on montre la raison et on enchaîne sur le musical.
      const muet = /aucune ville|pas encore générée/.test(reponse.repli);
      if (!muet) etat.textContent = reponse.repli + " — itinéraire musical";
      await itineraireMusical(minutes);
      return;
    }
    await poserItineraire(reponse.trajets[0]);
  } catch (e) {
    etat.textContent = "échec : " + e;
  } finally {
    if ($("itin-tracer")) $("itin-tracer").disabled = false;
  }
}

$("itin-tracer")?.addEventListener("click", tracerItineraire);

/// Rejoue l'itinéraire quand un de ses réglages change — mais seulement s'il y
/// a déjà un départ et qu'on est sur le plan de ville (le musical est trop lent
/// pour un recalcul à chaque cran). Court délai, comme le curseur de bruit.
let attenteItin = null;
function retracerItineraireSiPret() {
  if (carte.chemin !== "itineraire" || !carte.depart || !carteReelle()) return;
  clearTimeout(attenteItin);
  attenteItin = setTimeout(() => tracerItineraire().catch((e) => remonter(e, "itinéraire")), 200);
}


/* ------------------------------------------------- autotest de la carte
 *
 * Une webview du système ne se pilote pas de l'extérieur : sans ce banc,
 * « est-ce que le lasso marche encore ? » ne se vérifie qu'à la main, et donc
 * ne se vérifie pas. Il exerce les chemins de code que l'arrivée de MapLibre a
 * touchés — les deux transformations de coordonnées, le pointage, le lasso, la
 * bascule des modes — et rend son verdict au journal du processus.
 *
 * Déclenché par `RUSTY_MUSIC_AUTOTEST=1`.
 */
async function autotestCarte() {
  const resultats = [];
  // **Rendre compte au fil de l'eau, pas à la fin.** Grouper les résultats
  // rendait tout blocage opaque : le banc ne disait rien, et l'on ne savait
  // même pas s'il avait démarré. Chaque ligne part maintenant dès qu'elle est
  // connue, ce qui localise l'arrêt au pas près.
  const verifier = (nom, ok, detail = "") => {
    const v = { nom, ok, detail: String(detail).slice(0, 120) };
    resultats.push(v);
    journalCarte(`${v.ok ? "OK  " : "ÉCHEC"} ${v.nom}${v.detail ? " — " + v.detail : ""}`, v.ok ? "log" : "warn");
  };
  const etape = (nom) => journalCarte("… " + nom);
  const attendre = (ms) => new Promise((r) => setTimeout(r, ms));

  try {
    etape("bascule en mode explorer");
    if (modeCourant !== "explorer") await basculerMode("explorer");
    await attendre(500);
    verifier("morceaux chargés", carte.points.length > 0, carte.points.length);

    // --- mode nuage : le repère du canevas -------------------------------
    carte.affichage = "points";
    majAffichageGL();
    await attendre(200);
    const r = cnv.getBoundingClientRect();
    verifier("canevas dimensionné", r.width > 100 && r.height > 100, `${r.width}×${r.height}`);

    const p0 = carte.points[0];
    const [ex, ey] = versEcran(p0, r);
    const [rx, ry] = versCarte(ex, ey, r);
    verifier(
      "nuage : aller-retour des coordonnées",
      Math.abs(rx - p0.x) < 1e-3 && Math.abs(ry - p0.y) < 1e-3,
      `${p0.x.toFixed(4)}→${rx.toFixed(4)}`,
    );
    verifier("nuage : pointage", pointSous(ex, ey)?.id === p0.id, pointSous(ex, ey)?.id);

    const avantZoom = carte.vue.k;
    zoomer(1.4, r.width / 2, r.height / 2);
    verifier("nuage : zoom", carte.vue.k > avantZoom, `${avantZoom.toFixed(2)}→${carte.vue.k.toFixed(2)}`);
    carte.vue = { k: 1, dx: 0, dy: 0 };

    // --- mode carte : le repère de MapLibre ------------------------------
    etape("passage en mode carte");
    carte.affichage = "carte";
    const t0 = performance.now();
    majAffichageGL();
    for (let i = 0; i < 240 && !glPret; i++) await attendre(250);
    const ms = performance.now() - t0;
    verifier(
      "carte : tuiles chargées",
      glPret,
      glPret ? `${gl.getStyle().layers.length} couches en ${(ms / 1000).toFixed(1)} s` : "délai dépassé",
    );
    // Une carte qui met plus de trois secondes à paraître passe pour cassée.
    verifier("carte : délai d'apparition", glPret && ms < 3000, `${(ms / 1000).toFixed(1)} s`);

    if (glPret) {
      const [gx, gy] = versEcran(p0, r);
      const [gxr, gyr] = versCarte(gx, gy, r);
      verifier(
        "carte : aller-retour des coordonnées",
        Math.abs(gxr - p0.x) < 1e-3 && Math.abs(gyr - p0.y) < 1e-3,
        `${p0.x.toFixed(4)}→${gxr.toFixed(4)}`,
      );
      verifier("carte : pointage", pointSous(gx, gy)?.id === p0.id, pointSous(gx, gy)?.id);

      const zAvant = gl.getZoom();
      zoomer(1.4, r.width / 2, r.height / 2);
      verifier("carte : zoom", gl.getZoom() > zAvant, `${zAvant.toFixed(2)}→${gl.getZoom().toFixed(2)}`);

      // Les boutons +/− appellent `zoomer` sans coordonnées : ne doit pas
      // lever dans MapLibre (`unproject` d'un point indéfini).
      const zB = gl.getZoom();
      let boutonOk = true;
      try {
        $("zoom-plus").click();
        $("zoom-moins").click();
      } catch (err) {
        boutonOk = false;
      }
      verifier("carte : boutons +/−", boutonOk && Math.abs(gl.getZoom() - zB) < 1e-6, gl.getZoom().toFixed(2));

      const cAvant = gl.getCenter().lng;
      const g2 = carteGL();
      g2.panBy([-50, 0], { duration: 0 });
      verifier("carte : déplacement", gl.getCenter().lng !== cAvant, gl.getCenter().lng.toFixed(3));

      // La vue d'ensemble cadre l'emprise cible (limite communale, ou les
      // points sur le monde fictif) : elle dézoome sous le niveau d'accueil et
      // fait tenir l'emprise dans la fenêtre.
      const b = vueInitialeGL?.bounds || bornesGeoPoints();
      $("zoom-reset").click();
      const dedans = b && (() => {
        const sw = gl.project(b[0]);
        const ne = gl.project(b[1]);
        const { width, height } = cnv.getBoundingClientRect();
        return sw.x >= -2 && ne.x <= width + 2 && ne.y >= -2 && sw.y <= height + 2;
      })();
      verifier(
        "carte : vue d'ensemble",
        !!b && dedans && gl.getZoom() < (vueInitialeGL?.zoom ?? 14),
        `zoom ${gl.getZoom().toFixed(2)}`,
      );

      // Les entités des tuiles répondent-elles au pointage ?
      const rendues = gl.queryRenderedFeatures({ layers: ["territoires"] });
      verifier("carte : entités interrogeables", rendues.length > 0, rendues.length);
    }

    // --- le lasso, dans les deux repères ---------------------------------
    for (const mode of ["points", "carte"]) {
      carte.affichage = mode;
      majAffichageGL();
      await attendre(150);
      // Le contour part de l'écran, comme un vrai geste : on prend un
      // rectangle au centre du canevas et on le convertit dans le repère du
      // mode actif. C'est justement cette conversion que MapLibre a changée.
      const c = [
        versCarte(r.width * 0.3, r.height * 0.3, r),
        versCarte(r.width * 0.7, r.height * 0.3, r),
        versCarte(r.width * 0.7, r.height * 0.7, r),
        versCarte(r.width * 0.3, r.height * 0.7, r),
      ];
      try {
        etape(`lasso (${mode}) — appel`);
        const pris = await invoke("selection", { trace: c, reel: carteReelle() });
        verifier(
          `lasso (${mode})`,
          pris.length > 0 && pris.length < carte.points.length,
          `${pris.length} morceaux`,
        );
      } catch (e) {
        verifier(`lasso (${mode})`, false, e);
      }
    }

    // --- l'itinéraire ----------------------------------------------------
    try {
      etape("itinéraire — construction du réseau, une trentaine de secondes");
      const t = await invoke("itineraire", {
        depart: carte.points[0].id,
        arrivee: null,
        profil: "sentier",
        minutes: 20,
      });
      verifier(
        "itinéraire",
        t.length > 0 && t[0].pistes.length > 1,
        `${t[0]?.pistes.length} morceaux, ${(t[0]?.duree_ms / 60000).toFixed(0)} min`,
      );
    } catch (e) {
      verifier("itinéraire", false, e);
    }

    // --- l'itinéraire sur voirie réelle (si une ville est importée) ------
    if (villeReelle) {
      try {
        etape("itinéraire sur voirie — accrochage des morceaux aux rues");
        const rep = await invoke("itineraire_voirie", {
          depart: carte.points[0].id,
          arrivee: null,
          profil: "panoramique",
          minutes: 20,
          famille: null,
          rayonM: null,
        });
        const t = rep.trajets[0];
        verifier(
          "itinéraire voirie",
          !rep.repli && t && t.pistes.length > 1 && t.polyligne.length > 1,
          rep.repli || `${t?.pistes.length} morceaux, polyligne ${t?.polyligne.length} points`,
        );
      } catch (e) {
        verifier("itinéraire voirie", false, e);
      }
    }

    carte.affichage = "points";
    majAffichageGL();
  } catch (e) {
    verifier("autotest", false, (e && e.stack) || e);
  }

  journalCarte(
    `AUTOTEST ${resultats.filter((v) => v.ok).length}/${resultats.length}`,
  );
}

invoke("autotest_carte")
  .then((oui) => {
    if (oui) setTimeout(autotestCarte, 2500);
  })
  .catch(() => {});
