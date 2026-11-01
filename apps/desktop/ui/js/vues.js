import "./socle.js"; // l'ordre d'exécution : voir main.js
import {
  $, affecterAlchimieEnCours, affecterFileCompositionActive, affecterFileCourante,
  affecterOrigineFile, alchimieEnCours, duree, estListePistes, fileCompositionActive,
  fileCourante, invoke, LIGNE, origineFile, pochette, remonter, signalerErreur, txt, vue,
} from "./socle.js";
import {
  demarrerCompositionFile, finirCompositionFile, phaseCompositionFile, revelerFile,
  sonderGrapheProgres,
} from "./lama.js";
import {
  activer, favorisChemins, lancerLecture, lignePlaylist, majIndexActif, ouvrirAlbumsArtiste,
  pisteSelectionnee, poser, scrollActuel, selectionner,
} from "./ecouter.js";
import {
  basculerFile, demarrerLecture, dessinerFile, enLecture, modeCourant,
} from "./lecteur.js";
import {
  tracerRouteSurCarte,
} from "./maplibre.js";
import {
  cleAlbum, lignesCourantes, triAlbums,
} from "./ecouter-familles.js";
import {
  bruitChemin, preparerGraphe,
} from "./fond-de-plan.js";
import {
  basculerMode,
} from "./modes.js";
import {
  contactMb, depuisTexte,
} from "./decouvrir.js";
import {
  passePopularite, verrouillerActualisation,
} from "./bibliotheque.js";

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
  // Une liste de playlists vide garde de quoi montrer son message d'accueil.
  const vide = n === 0 && vue.quoi === "playlists";
  socle.style.height = `${(vide ? 2 : n) * LIGNE}px`;

  // On ne pose dans le DOM que ce qui est visible, plus une marge : 3 543
  // artistes en une fois figeraient la fenêtre.
  const haut = Math.max(0, Math.floor(liste.scrollTop / LIGNE) - 6);
  const bas = Math.min(n, Math.ceil((liste.scrollTop + liste.clientHeight) / LIGNE) + 6);

  fenetre.style.transform = `translateY(${haut * LIGNE}px)`;
  fenetre.replaceChildren();

  for (let i = haut; i < bas; i++) {
    fenetre.appendChild(ligne(vue.lignes[i], i));
  }
  if (vide) {
    const p = document.createElement("p");
    p.className = "aide liste__vide";
    p.textContent =
      "Aucune playlist pour l'instant. Ouvrez la file d'attente (bouton « file » du transport) et choisissez « enregistrer ».";
    fenetre.appendChild(p);
  }
  majIndexActif();
}

function ligne(item, index) {
  const el = document.createElement("div");
  el.className = "ligne";
  el.dataset.index = index;
  // Une playlist n'a pas de `path` : le comparer à `pisteSelectionnee?.path`
  // (lui aussi `undefined` tant que rien n'est choisi) la surlignerait à tort.
  if (vue.quoi === "playlists") return lignePlaylist(el, item);
  if (item.path === enLecture) el.classList.add("ligne--joue");
  if (item.path === pisteSelectionnee?.path) el.classList.add("ligne--select");
  return vue.quoi === "recherche" ? ligneRecherche(el, item) : lignePiste(el, item);
}

/// Une ligne de la liste des pistes d'un album. **Un clic sélectionne, il ne
/// lit pas** (`docs/interface-guidelines.md`, Règle 1 : « Sélectionner n'est pas
/// écouter ») : le morceau peuple l'inspecteur, s'affiche dans le transport s'il
/// n'y a rien en lecture, et devient la source d'Éditer. La lecture depuis cette
/// piste est un geste à part : le numéro (→ ▶ au survol) ou le double-clic. Le
/// nom d'artiste ouvre ses albums.
function lignePiste(el, item) {
  el.classList.add("ligne--pistes");

  el.appendChild(boutonJouer(item));

  const titre = document.createElement("span");
  titre.className = "ligne__nom";
  titre.textContent = txt(item.title, "(sans titre)");
  titre.title = "Sélectionner · double-clic pour lire";
  el.appendChild(titre);

  // Le nom d'artiste mène à ses albums, sans sélectionner la piste — même geste
  // que dans les résultats de recherche et l'inspecteur. `stopPropagation` (dans
  // `lienLigne`) le sépare du clic de la ligne, qui sélectionne.
  if (item.artist) {
    el.appendChild(
      lienLigne(item.artist, "ligne__sec", () =>
        ouvrirAlbumsArtiste(item.artist, item.artist_mbid, snapVue()),
      ),
    );
  } else {
    const sec = document.createElement("span");
    sec.className = "ligne__sec";
    el.appendChild(sec);
  }

  ajouterPopEtDuree(el, item);
  cablerSelection(el, item);
  return el;
}

/// Une ligne de résultat de recherche. Le clic sur la ligne (titre compris)
/// **sélectionne** le morceau sans le lire ; le numéro (→ ▶ au survol) ou le
/// double-clic lance la lecture (la file se forme avec la suite des résultats,
/// comme `activer`). L'album et le nom d'artiste restent des liens : ils mènent
/// à l'album et à ses albums, sans rien sélectionner ni lancer.
function ligneRecherche(el, item) {
  el.classList.add("ligne--recherche");

  el.appendChild(boutonJouer(item, "·"));

  const titre = document.createElement("span");
  titre.className = "ligne__nom";
  titre.textContent = txt(item.title, "(sans titre)");
  titre.title = "Sélectionner · double-clic pour lire";
  el.appendChild(titre);
  if (item.album) {
    el.appendChild(
      lienLigne(item.album, "ligne__meta", () => ouvrirAlbumDePiste(item)),
    );
  }
  if (item.artist) {
    el.appendChild(
      lienLigne(item.artist, "ligne__meta", () =>
        ouvrirAlbumsArtiste(item.artist, item.artist_mbid, snapVue()),
      ),
    );
  }

  ajouterPopEtDuree(el, item);
  cablerSelection(el, item);
  return el;
}

/// Le geste des lignes de morceau : un clic sélectionne (silencieux), un
/// double-clic lit depuis ce morceau. Le double-clic passe d'abord par deux
/// clics : la sélection a déjà eu lieu, `lancerLecture` la reconfirme.
function cablerSelection(el, item) {
  el.addEventListener("click", () => selectionner(item));
  el.addEventListener("dblclick", () => {
    lancerLecture(item).catch((e) => remonter(e, "lecture"));
  });
}

/// Le n° de piste sous forme de bouton de lecture : il montre le numéro (ou
/// `fallback` s'il manque), qui cède la place à ▶ au survol / focus et sur le
/// morceau en cours. Clic → lit `item` via `activer`, sans laisser le clic
/// remonter à la ligne.
function boutonJouer(item, fallback = "") {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "ligne__jouer";
  b.title = "Lire ce morceau";
  b.setAttribute("aria-label", `Lire ${txt(item.title, "ce morceau")}`);
  b.innerHTML = `<span class="jouer__no"></span><span class="jouer__ic" aria-hidden="true">▶</span>`;
  b.children[0].textContent = item.track_no ?? fallback;
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    activer(item);
  });
  return b;
}

/// Jauge de popularité (si le morceau a un id) puis durée, en fin de ligne —
/// commun aux pistes d'un album et aux résultats de recherche.
function ajouterPopEtDuree(el, item) {
  if (favorisChemins.has(item.path)) {
    const coeur = document.createElement("span");
    coeur.className = "ligne__fav";
    coeur.textContent = "♥";
    coeur.title = "Favori";
    el.appendChild(coeur);
  }
  if (Number.isFinite(item.id)) {
    const cell = document.createElement("span");
    cell.className = "ligne__pop";
    cell.appendChild(jaugePop(popParPiste.get(item.id)));
    el.appendChild(cell);
  }
  const cpt = document.createElement("span");
  cpt.className = "ligne__cpt";
  cpt.textContent = duree(item.duration_ms);
  el.appendChild(cpt);
}

/// Un fragment de texte cliquable dans une ligne de liste (bouton nu) : ne
/// remonte pas au clic de la ligne, tronque proprement, et prend l'accent au
/// survol pour se signaler comme lien.
function lienLigne(texte, cls, onClick) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = `${cls} ligne__lien`;
  b.textContent = texte;
  b.title = texte;
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick();
  });
  return b;
}

/// Instantané de la vue de liste courante, à garder comme cible de retour quand
/// on descend d'une ligne vers un album ou un artiste (résultat de recherche ou
/// piste d'un album) — même forme que les instantanés construits par `activer`.
function snapVue() {
  return {
    quoi: vue.quoi,
    titre: vue.titre,
    lignes: vue.lignes,
    retour: vue.retour,
    scroll: scrollActuel(),
  };
}

/// Ouvre au centre les pistes de l'album qui contient `item`. À défaut d'album
/// (résultat sans métadonnée d'album), retombe sur l'inspecteur du morceau.
async function ouvrirAlbumDePiste(item) {
  if (!item.album) {
    selectionner(item);
    return;
  }
  const retour = snapVue();
  const pistes = await invoke("tracks_of_album", {
    album: item.album,
    artist: item.artist ?? null,
  });
  if (modeCourant !== "ecoute") await basculerMode("ecoute");
  poser("pistes", item.album, pistes, retour);
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
  if (!$("liste").hidden && estListePistes()) {
    dessiner();
    chargerPopularites(
      vue.lignes.map((l) => l.id),
      () => {
        if (estListePistes()) dessiner();
      },
    );
  }
}

/// La ligne d'alerte du mode Bibliothèque : n'apparaît que si de la popularité
/// a été récupérée **et** qu'une partie a dépassé le seuil de péremption —
/// une notoriété bouge lentement, on ne relance pas une passe de deux heures
/// pour rien. Le bouton « Rafraîchir » coche la case et relance la passe.
async function chargerPopulariteFraicheur() {
  const ligne = $("popularite-fraicheur");
  let couverts;
  let plusAncienne;
  let perimes;
  let peremptionJours;
  try {
    // Le seuil vient du moteur (`POP_PEREMPTION_JOURS`) plutôt que d'être
    // recopié en dur ici — sinon le texte pourrait mentir si la constante
    // change côté Rust sans qu'on pense à répercuter le texte.
    [couverts, plusAncienne, perimes, peremptionJours] = await invoke("popularite_fraicheur");
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
    `de plus de ${peremptionJours} jours` +
    (plusAncienne ? ` (la plus ancienne remonte à ${depuisTexte(plusAncienne)}).` : ".") +
    " ";
  ligne.hidden = false;
}

$("popularite-rafraichir").addEventListener("click", async () => {
  if ($("tout-rafraichir").disabled) return; // une passe tourne déjà
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
// `padding` horizontal de `.grille` : la gouttière du gabarit (`--gouttiere`), lue
// dans la feuille de style plutôt que recopiée.
const GRILLE_PAD = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--gouttiere")) || 26;
// Une seule largeur de case pour Écouter (virtualisée, ci-dessus) et pour la
// grille de Découvrir (CSS pur, `.decouvrir-grille`) : la constante fait foi.
document.documentElement.style.setProperty("--album-larg", `${ALBUM_LARG}px`);

const grille = $("grille");
// Parent flex de `.grille`/`.liste`/`#autour-artiste` — sa classe modificatrice
// (voir `majAutourArtiste`) fait défiler grille et bandeau comme une seule
// colonne, plutôt que le bandeau plaqué au bas de la fenêtre derrière un vide.
const centreCorps = document.querySelector(".centre__corps");
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

// Cases déjà construites, par clé d'identité de l'item (chemin d'album ou
// mbid|nom d'artiste) — réutilisées d'un redessin à l'autre pour qu'une case
// déjà visible, pochette déjà posée, ne disparaisse jamais le temps d'un
// nouveau `carteAlbum`/`carteArtiste` : sans ça, chaque cran de défilement
// vidait puis reconstruisait toute la bande visible, et le placeholder rayé
// de `.album__pochette` clignotait même sur des cases inchangées. Vidée à la
// bascule albums ↔ artistes : les deux balisages ne doivent jamais se
// répondre sur une collision de clé fortuite.
const grilleCartes = new Map();
let grilleCartesQuoi = null;

function cleCarteGrille(item) {
  return vue.quoi === "artistes" ? `${item.mbid ?? ""}|${item.name}` : cleAlbum(item);
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

  // Les cases d'albums montrent la date d'ajout sous « Ajoutés » et l'année
  // sinon : changer d'un affichage à l'autre doit les refaire, pas les déplacer.
  const affichageCartes = `${vue.quoi}${vue.quoi === "albums" && triAlbums === "ajoutes" ? ":ajoutes" : ""}`;
  if (affichageCartes !== grilleCartesQuoi) {
    // Vider la Map ne suffit pas : les nœuds restent dans `grilleFenetre`, et
    // les cartes de l'ancienne vue s'affichent devant les nouvelles (Artistes
    // sans effet, tris d'Albums figés derrière des cartes périmées).
    grilleFenetre.replaceChildren();
    grilleCartes.clear();
    grilleCartesQuoi = affichageCartes;
  }

  const carte = vue.quoi === "artistes" ? carteArtiste : carteAlbum;
  const vus = new Set();
  for (let i = rangHaut * cols; i < Math.min(n, rangBas * cols); i++) {
    const item = lignes[i];
    const cle = cleCarteGrille(item);
    vus.add(cle);
    let el = grilleCartes.get(cle);
    if (!el) {
      el = carte(item);
      grilleCartes.set(cle, el);
    }
    // Déplace le nœud à sa place dans l'ordre voulu s'il y est déjà — pas de
    // retrait/réinsertion, donc pas de repeinture parasite.
    grilleFenetre.appendChild(el);
  }

  for (const [cle, el] of grilleCartes) {
    if (!vus.has(cle)) {
      el.remove();
      grilleCartes.delete(cle);
    }
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
  // Sous l'ordre « Ajoutés », la date d'arrivée remplace l'année : sans elle,
  // on ne comprendrait pas l'ordre. L'année reste dans l'infobulle.
  if (triAlbums === "ajoutes" && item.ajoute_le) {
    const date = new Date(item.ajoute_le * 1000).toLocaleDateString("fr-FR", {
      day: "numeric", month: "short", year: "numeric",
    });
    el.children[2].children[1].textContent = date;
    el.children[2].children[1].title = `Ajouté le ${date}${item.year ? ` · sorti en ${item.year}` : ""}`;
  } else {
    el.children[2].children[1].textContent = item.year ?? "————";
  }
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
  selectionner(pistes[0]);
  affecterFileCourante(pistes);
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
    origine: `Dans l'esprit de « ${item.name} »`,
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
///
/// `cible` : la longueur annoncée dans le panneau pendant le calcul — les
/// trois boutons ✦ visent toujours `ALCHIMIE_PISTES`, mais le champ
/// d'intention d'Explorer (`composerPlanTexte`) laisse l'utilisateur choisir
/// son propre nombre de morceaux.
///
/// `graphe` : `false` pour une simple playlist mélangée (« Playlist de cet
/// artiste », `shuffle_artist`) — pas de dérive sonique, donc ni préparation
/// du graphe de voisins ni route sur la carte, et pas de piste « graine »
/// dans la file (aucun pivot, juste un mélange).
async function composerAlchimie({ bouton, chemin, demarrer, cible = ALCHIMIE_PISTES, graphe = true, origine = null }) {
  if (alchimieEnCours) return;
  affecterAlchimieEnCours(true);
  const fileEtaitOuverte = !$("file").hidden;
  let pistes = null;
  let echec = false;

  bouton.disabled = true;
  bouton.classList.add("alchimie--travail");
  demarrerCompositionFile(cible);
  basculerFile(true);
  const jauge = graphe ? sonderGrapheProgres() : null;

  try {
    if (graphe) {
      await preparerGraphe();
      clearInterval(jauge);
    }
    phaseCompositionFile("Composition de la playlist…");

    pistes = await chemin();
    if (!pistes || pistes.length === 0) return;

    selectionner(pistes[0]);
    affecterFileCourante(pistes);
    // D'où vient cette file, pour la proposer en nom et la garder si elle est
    // enregistrée (`origineDeLaFile`).
    affecterOrigineFile(origine ? { texte: origine, premier: pistes[0].path, n: pistes.length } : null);
    if (graphe) tracerRouteSurCarte(pistes);
    // Lecture tout de suite ; le sondage d'état qui démarre avec elle
    // voudra redessiner la file, `fileCompositionActive` l'en empêche
    // jusqu'à la fin du défilement.
    demarrerLecture(() => demarrer(pistes)).catch((e) => remonter(e, "composerAlchimie"));
    await revelerFile(pistes, { graine: graphe });
  } catch (e) {
    echec = true;
    // Auparavant : la file se refermait ou restait vide sans indice — un clic
    // sur ✦ suivi d'une jauge qui tourne puis de rien.
    signalerErreur("file-compo-phase", "échec de la composition de la playlist", e, "composerAlchimie");
  } finally {
    if (jauge) clearInterval(jauge);
    bouton.disabled = false;
    bouton.classList.remove("alchimie--travail");
    affecterAlchimieEnCours(false);
    if (echec) {
      // Laisse le message ci-dessus visible un instant plutôt que de
      // refermer aussitôt le panneau — `finirCompositionFile` l'effacerait
      // avec lui.
      affecterFileCompositionActive(false);
      setTimeout(() => {
        finirCompositionFile();
        if (!fileEtaitOuverte) basculerFile(false);
      }, 2500);
    } else {
      finirCompositionFile();
      if (pistes && pistes.length > 0) dessinerFile();
      else if (!fileEtaitOuverte) basculerFile(false);
      else dessinerFile();
    }
  }
}

export {
  ALBUM_HAUT, ALCHIMIE_PISTES, centreCorps, chargerPopulariteFraicheur, chargerPopularites,
  cleCarteGrille, colonnesGrille, composerAlchimie, dessiner, dessinerGrille, fenetre, grille,
  initiales, jaugePop, lireAlbum, liste, popARecalculee, popParPiste, teinteNom,
};
