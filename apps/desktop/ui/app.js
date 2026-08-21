// Interface du mode Écoute. Pas de framework ni de bundler : `CLAUDE.md`
// retient HTML/CSS/JS simple, la carte WebGL du module 2 n'impose rien ici.

const { invoke } = window.__TAURI__.core;

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

let fileCourante = []; // pistes envoyées au lecteur, pour l'affichage

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

  if (vue.quoi === "artistes") {
    el.innerHTML = `<span class="ligne__nom"></span>
                    <span class="ligne__cpt"></span>`;
    el.children[0].textContent = item.name;
    el.children[1].textContent = `${item.albums} alb · ${item.tracks} morc`;
  } else {
    el.innerHTML = `<span class="ligne__no"></span>
                    <span class="ligne__nom"></span>
                    <span class="ligne__sec"></span>
                    <span class="ligne__cpt"></span>`;
    el.children[0].textContent = item.track_no ?? "";
    el.children[1].textContent = txt(item.title, "(sans titre)");
    el.children[2].textContent = txt(item.artist);
    el.children[3].textContent = duree(item.duration_ms);
    if (item.path === enLecture) el.classList.add("ligne--joue");
  }

  el.addEventListener("click", () => activer(item));
  return el;
}

liste.addEventListener("scroll", dessiner, { passive: true });
window.addEventListener("resize", dessiner);

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
  const n = vue.lignes.length;
  const cols = colonnesGrille();
  const rangs = Math.max(1, Math.ceil(n / cols));
  grilleSocle.style.height = `${rangs * ALBUM_HAUT}px`;

  const rangHaut = Math.max(0, Math.floor(grille.scrollTop / ALBUM_HAUT) - 2);
  const rangBas = Math.min(rangs, Math.ceil((grille.scrollTop + grille.clientHeight) / ALBUM_HAUT) + 2);

  grilleFenetre.style.transform = `translateY(${rangHaut * ALBUM_HAUT}px)`;
  grilleFenetre.style.gridTemplateColumns = `repeat(${cols}, ${ALBUM_LARG}px)`;
  grilleFenetre.replaceChildren();

  for (let i = rangHaut * cols; i < Math.min(n, rangBas * cols); i++) {
    grilleFenetre.appendChild(carteAlbum(vue.lignes[i]));
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
  el.children[2].textContent = `${txt(item.artist, "(sans artiste)")} · ${item.year ?? "————"}`;
  el.addEventListener("click", () => activer(item));

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

/// Lit l'album entier, dans l'ordre du disque, sans passer par sa liste de
/// pistes — le geste que propose le bouton posé sur la pochette.
async function lireAlbum(item) {
  const pistes = await invoke("tracks_of_album", { album: item.name, artist: item.artist ?? null });
  if (pistes.length === 0) return;
  inspecter(pistes[0]);
  fileCourante = pistes;
  tracerRouteSurCarte(pistes);
  await invoke("play", { paths: pistes.map((t) => t.path) });
  poserLecture(true);
  sonder(true);
}

const ALCHIMIE_PISTES = 20;

/// Playlist « dans l'esprit de l'album » : dérive depuis son morceau le plus
/// central vers des morceaux soniquement proches, ailleurs dans la
/// bibliothèque — l'équivalent local du « Song Alchemy » d'AudioMuse-AI.
///
/// Une graine neuve à chaque clic : le bouton est pensé pour surprendre, pas
/// pour rejouer toujours la même dérive sur le même album.
async function genererAlchimie(item, bouton) {
  bouton.disabled = true;
  try {
    const pistes = await invoke("path_album", {
      album: item.name,
      artist: item.artist ?? null,
      steps: ALCHIMIE_PISTES,
      seed: Math.floor(Math.random() * 2 ** 31),
      bruit: bruitChemin,
    });
    if (pistes.length === 0) return;
    inspecter(pistes[0]);
    fileCourante = pistes;
    tracerRouteSurCarte(pistes);
    await invoke("play", { paths: pistes.map((t) => t.path) });
    poserLecture(true);
    sonder(true);
  } catch (e) {
    // Le cas courant est un album pas encore analysé (`path_album` échoue
    // alors côté moteur) : pas de morceau à lire, pas de file à casser pour
    // autant — seul le journal en garde trace.
    remonter(e, "genererAlchimie");
  } finally {
    bouton.disabled = false;
  }
}

grille.addEventListener("scroll", dessinerGrille, { passive: true });
window.addEventListener("resize", () => {
  if (!grille.hidden) dessinerGrille();
});

/* ---------------------------------------------------------- navigation */

// Le défilement d'où l'on vient : lu par `activer()` juste avant de
// descendre dans un artiste ou un album, pour que « ← » retrouve la ligne
// quittée plutôt que de remonter en haut de la liste.
function scrollActuel() {
  return (vue.quoi === "albums" ? grille : liste).scrollTop;
}

function poser(quoi, titre, lignes, retour = null, scroll = 0) {
  vue.quoi = quoi;
  vue.titre = titre;
  vue.lignes = lignes;
  vue.retour = retour;
  if (retour === null) sommet = { quoi, titre, lignes };

  $("fil-titre").textContent = titre;
  $("fil-compte").textContent = `${lignes.length} ${quoi === "artistes" ? "artistes" : quoi === "albums" ? "albums" : "morceaux"}`;
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

  const enGrille = quoi === "albums";
  $("liste").hidden = enGrille;
  $("grille").hidden = !enGrille;
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
    dessinerGrille();
  } else {
    liste.scrollTop = scroll;
    dessiner();
  }

  // Repère alphabétique : seulement là où l'ordre affiché est celui des
  // noms — pas la liste des pistes d'un album (ordre du disque), ni une
  // recherche (ordre de pertinence).
  const avecIndex = quoi === "artistes" || quoi === "albums";
  $("index-alpha").hidden = !avecIndex;
  if (avecIndex) construireIndexAlpha();
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
  vue.lignes.forEach((item, i) => {
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
  if (vue.quoi === "albums") return Math.floor(grille.scrollTop / ALBUM_HAUT) * colonnesGrille();
  return Math.floor(liste.scrollTop / LIGNE);
}

function majIndexActif() {
  if ($("index-alpha").hidden) return;
  const i = Math.min(vue.lignes.length - 1, Math.max(0, rangVisible()));
  const lettre = vue.lignes[i] ? premiereLettre(vue.lignes[i].name) : null;
  indexAlphaHote.querySelectorAll(".index-alpha__lettre").forEach((b) =>
    b.classList.toggle("index-alpha__lettre--actif", b.dataset.lettre === lettre),
  );
}

function sauterALettre(l) {
  const rang = indexAlpha[l];
  if (rang === undefined) return;
  if (vue.quoi === "albums") {
    grille.scrollTop = Math.floor(rang / colonnesGrille()) * ALBUM_HAUT;
    dessinerGrille();
  } else {
    liste.scrollTop = rang * LIGNE;
    dessiner();
  }
}

document.querySelectorAll("[data-vuelib]").forEach((b) =>
  b.addEventListener("click", () => charger(b.dataset.vuelib)),
);

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
    await invoke("play", { paths: fileCourante.map((t) => t.path) });
    poserLecture(true);
    sonder(true);
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

/// Le nom de l'artiste, dans l'inspecteur, ouvre ses albums au centre — le
/// même geste que cliquer l'artiste depuis la liste « Artistes », mais depuis
/// n'importe quel morceau inspecté (piste, voisin sonique, point de la carte).
$("insp-artiste").addEventListener("click", async () => {
  const artiste = $("insp-artiste").dataset.artiste;
  if (!artiste) return;
  const mbid = $("insp-artiste").dataset.mbid || null;
  const albums = await invoke("albums", { artist: artiste, mbid });
  // « Au centre » suppose le mode Écoute : depuis Explorer ou Éditer, le
  // centre montre la carte ou le dock, pas la grille.
  if (modeCourant !== "ecoute") await basculerMode("ecoute");
  poser("albums", artiste, albums, sommet);
});

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

/// Playlist « dans l'esprit de ce morceau » (`genererAlchimieMorceau`) —
/// même mécanisme que le bouton ✦ d'une case d'album (`genererAlchimie`),
/// mais parti d'un seul morceau déjà connu de la carte : une errance sonique
/// depuis son point, pas besoin de lui chercher un centre au préalable.
$("insp-alchimie").addEventListener("click", async () => {
  const bouton = $("insp-alchimie");
  const id = Number($("insp-titre").dataset.id);
  if (!Number.isFinite(id)) return;
  bouton.disabled = true;
  try {
    const pistes = await invoke("path", {
      from: id,
      mode: "errance",
      steps: ALCHIMIE_PISTES,
      seed: Math.floor(Math.random() * 2 ** 31),
      bruit: bruitChemin,
    });
    if (pistes.length === 0) return;
    inspecter(pistes[0]);
    fileCourante = pistes;
    tracerRouteSurCarte(pistes);
    await invoke("set_queue", { paths: pistes.map((t) => t.path) });
    poserLecture(true);
    sonder(true);
  } catch (e) {
    remonter(e, "genererAlchimieMorceau");
  } finally {
    bouton.disabled = false;
  }
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
      await invoke("play", { paths: [v.path] });
      poserLecture(true);
      sonder(true);
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
                    <span class="file__duree"></span>`;
    el.children[0].textContent = i === rangCourant ? "▶" : i + 1;
    el.children[1].children[0].textContent = txt(t.title, "(sans titre)");
    el.children[1].children[1].textContent = txt(t.artist, "(sans artiste)");
    el.children[2].textContent = duree(t.duration_ms);

    // Sauter conserve les pistes précédentes : on peut revenir en arrière.
    el.addEventListener("click", async () => {
      await invoke("jump_to", { index: i });
      sonder(true);
    });
    hote.appendChild(el);
  });
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

// Onde : enveloppe crête (silhouette) avec noyau RMS (corps du son), calculée
// à partir du signal réel. Tant qu'elle n'est pas prête, un trait plat sert de
// repère de position.
const TRANCHES = 160;
const wave = $("wave");
for (let i = 0; i < TRANCHES; i++) {
  const b = document.createElement("i");
  b.appendChild(document.createElement("u")); // noyau RMS
  b.style.height = "12%";
  wave.appendChild(b);
}

const ondes = new Map(); // path → {peak, rms}, déjà rendues

function poserOnde(w) {
  for (let i = 0; i < TRANCHES; i++) {
    const barre = wave.children[i];
    const crete = w ? w.peak[i] ?? 0 : 0;
    const corps = w ? w.rms[i] ?? 0 : 0;
    // Racine : comprime la dynamique pour que les passages doux restent
    // visibles à côté des crêtes.
    barre.style.height = w ? `${8 + Math.sqrt(crete) * 92}%` : "12%";
    barre.firstChild.style.height = crete > 0 ? `${(corps / crete) * 100}%` : "0%";
  }
}

/// Demande l'onde d'une piste ; le moteur répond `null` puis la calcule.
async function chargerOnde(t) {
  if (!t) return poserOnde(null);
  if (ondes.has(t.path)) return poserOnde(ondes.get(t.path));

  poserOnde(null);
  const vise = t.path;
  // Le calcul décode tout le fichier : 3,5 s sur la carte SD au repos, mais
  // 13 s mesurées pendant qu'une passe d'analyse la sature. Le budget doit
  // couvrir ce cas, sinon l'onde reste plate sans que rien ne le signale.
  const echeance = Date.now() + 120_000;
  let attente = 300;
  while (Date.now() < echeance) {
    const w = await invoke("waveform", {
      path: vise,
      buckets: TRANCHES,
      durationMs: t.duration_ms ?? null,
    });
    if (w) {
      ondes.set(vise, w);
      if (enLecture === vise) poserOnde(w);
      return;
    }
    if (enLecture !== vise) return; // piste changée entre-temps
    await new Promise((r) => setTimeout(r, attente));
    attente = Math.min(attente * 1.4, 3000); // on espace, sans marteler
  }
  remonter(`onde non calculée après 120 s : ${vise}`, "chargerOnde");
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
    chargerOnde(t);
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
  const seuil = frac * wave.children.length;
  for (let i = 0; i < wave.children.length; i++) {
    wave.children[i].classList.toggle("on", i < seuil);
  }
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
  vue: { k: 1, dx: 0, dy: 0 },
  isolee: null, // famille mise en avant, ou null
  survole: null,
  depart: null, // borne de départ d'un chemin
  arrivee: null, // borne d'arrivée
  route: null, // chemin tracé, ou null
  lasso: null, // contour en cours de tracé, en coordonnées de carte
  couleur: "famille", // famille, ou une clé de CONTINUES
  affichage: "points", // points (nuage), ou densite (lignes de niveau)
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
  const { k, dx, dy } = carte.vue;
  const c = echelle(r);
  return [r.width / 2 + (p.x * c) * k + dx, r.height / 2 + (p.y * c) * k + dy];
}

/// L'inverse de `versEcran` : du pixel vers le repère du nuage. Le dessin en a
/// besoin — c'est le seul endroit où l'on part de l'écran pour aller vers les
/// données, et non l'inverse.
function versCarte(mx, my, r) {
  const { k, dx, dy } = carte.vue;
  const c = echelle(r) * k;
  return [(mx - r.width / 2 - dx) / c, (my - r.height / 2 - dy) / c];
}

/// Les douze teintes de famille, lues dans la feuille de style.
function couleursFamilles() {
  return getComputedStyle(document.documentElement)
    .getPropertyValue("--familles")
    .split(",")
    .map((c) => c.trim())
    .filter(Boolean);
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
const DENSITE_MARGE = 0.08; // déborde un peu [-1, 1] — même marge que `crates/core/src/density.rs`

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

const AUTRES = -2; // doit rester égal à `rusty_music_core::density::AUTRES`

let densite = null; // { bandes, image, demiCote } — voir `chargerDensite`

/// À rappeler quand les points rechargent : le résultat de densité vient du
/// moteur, mis en cache après chaque recalcul de la carte (ou de la densité
/// seule) — jamais reconstruit ici.
async function chargerDensite() {
  const r = await invoke("density_view");
  densite = { bandes: r.bandes, image: null, demiCote: 1 + DENSITE_MARGE };
}

/// Force de l'ombre portée (0..1), le seul des quatre réglages qui n'exige
/// pas d'aller-retour vers le moteur : décalage, flou et opacité de l'ombre
/// n'affectent que le rendu, pas le calcul. Mémorisé en local, comme le
/// bruit des chemins.
const OMBRE_DEFAUT = 0.6;
let forceOmbre = Number(localStorage.getItem("densite-ombre"));
if (!Number.isFinite(forceOmbre) || forceOmbre < 0 || forceOmbre > 1) forceOmbre = OMBRE_DEFAUT;

/// Construit (une fois par résultat de densité, par réglage d'ombre, par
/// isolement de famille ou par palier de zoom — voir `tailleDensiteEcran`)
/// l'image hors-écran du relief.
///
/// Chaque bande est peinte en aplat opaque — pas de dégradé, pas de
/// transparence qui laisserait deviner le territoire voisin — d'autant plus
/// sombre et saturé que son palier est haut : la densité s'y lit à la
/// luminosité, la teinte ne dit que le territoire. L'ombre portée vient de
/// `ctx.shadow*`, posée une fois ici : chaque bande projette son ombre sur
/// celle du dessous, du palier le plus bas au plus haut, comme des
/// découpes de papier empilées. Un trait nu, sans ombre, referme chaque
/// territoire sur son palier le plus bas — la frontière entre deux genres,
/// distincte du relief interne d'un même genre.
///
/// **Les teintes viennent de `--familles`**, indexées par identifiant de
/// famille — les mêmes que le nuage de points et la légende « Familles »
/// (`teintes[cluster % teintes.length]`, voir `dessinerNuage`). Un
/// `--territoires` séparé (Okabe-Ito) avait été essayé pour garantir la
/// distinction en deutéranopie ; retiré, la carte redessinée changeait de
/// teinte par famille selon le mode d'affichage, ce qui a été jugé pire que
/// le risque de confusion sur sept genres au plus (`AUTRES` prend déjà le
/// surplus, côté Rust).
///
/// Une image hors-écran plutôt qu'un tracé vectoriel à chaque image :
/// `shadowBlur` est coûteux, et le répéter sur des dizaines de bandes à
/// chaque geste de zoom aurait coûté la fluidité qu'on cherche à garder.
/// Peinte une fois, elle ne coûte plus ensuite qu'un `drawImage`.
function construireImageDensite(r) {
  const { bandes } = densite;
  const teintes = couleursFamilles();
  const style = getComputedStyle(document.documentElement);
  const gris = style.getPropertyValue("--mut").trim() || "#9A9284";
  const encre = style.getPropertyValue("--txt").trim() || "#EDE8DC";
  const teinteDe = (famille) => {
    if (famille === AUTRES) return gris;
    if (carte.isolee !== null && famille !== carte.isolee) return gris; // estompé, pas masqué
    return teintes[famille % teintes.length] ?? gris;
  };

  const gn = tailleDensiteEcran(r);
  const c = document.createElement("canvas");
  c.width = gn;
  c.height = gn;
  const g = c.getContext("2d");
  const echelleEcran = gn / (2 * densite.demiCote);
  g.setTransform(echelleEcran, 0, 0, echelleEcran, gn / 2, gn / 2);

  const chemin = (bande) => {
    const p = new Path2D();
    for (const polygone of bande.polygones) {
      for (const anneau of polygone) {
        if (anneau.length < 3) continue;
        p.moveTo(anneau[0][0], anneau[0][1]);
        for (let i = 1; i < anneau.length; i++) p.lineTo(anneau[i][0], anneau[i][1]);
        p.closePath();
      }
    }
    return p;
  };

  // 2 à 4 px de décalage, 6 à 10 px de flou, 25 à 35 % d'opacité — les
  // bornes demandées, parcourues par `forceOmbre`, en pixels de l'image
  // hors-écran. `shadowOffset*` et `shadowBlur` ne suivent **pas**
  // `ctx.setTransform` — vérifié à l'essai après un premier réglage qui les
  // divisait par `echelleEcran` et ne rendait plus rien : ils vivent dans
  // l'espace propre du canevas, pas dans le repère mis à l'échelle
  // ci-dessus. Aucune compensation à faire.
  const t = Math.min(1, Math.max(0, forceOmbre));
  g.shadowColor = `rgba(0,0,0,${(0.25 + 0.1 * t).toFixed(3)})`;
  g.shadowOffsetX = 2 + 2 * t;
  g.shadowOffsetY = 2 + 2 * t;
  g.shadowBlur = 6 + 4 * t;

  const parTerritoire = new Map();
  for (const b of bandes) {
    if (b.famille === null) continue; // la nappe globale ne se peint pas ici
    if (!parTerritoire.has(b.famille)) parTerritoire.set(b.famille, []);
    parTerritoire.get(b.famille).push(b);
  }

  // L'ordre entre territoires n'a pas d'importance pour le remplissage —
  // par construction (densité maximale gagnante côté Rust) ils ne se
  // recouvrent jamais. Seul l'ordre des paliers *à l'intérieur* d'un même
  // territoire compte, pour que l'ombre tombe dans le bon sens.
  for (const [famille, groupe] of parTerritoire) {
    groupe.sort((a, b) => a.palier - b.palier);
    const [h, s, l] = hexHSL(teinteDe(famille));
    const n = groupe.length;
    for (const bande of groupe) {
      const creux = n > 1 ? bande.palier / (n - 1) : 1;
      const sBande = 0.28 + (Math.min(0.85, s + 0.15) - 0.28) * creux;
      const lHaut = Math.min(0.68, l + 0.18);
      const lBande = lHaut - (lHaut - Math.max(0.3, l - 0.16)) * creux;
      const [rr, gg, bb] = hslRGB(h, sBande, lBande);
      g.fillStyle = `rgb(${rr},${gg},${bb})`;
      g.fill(chemin(bande));
    }
  }

  // La frontière entre genres : un trait net sur le contour extérieur de
  // chaque territoire (son palier le plus bas, trié plus haut), sans ombre —
  // la limite entre deux territoires, pas le relief d'une bande à l'autre
  // dans un même territoire.
  //
  // `lineWidth`, à la différence de `shadowOffset*`/`shadowBlur` plus haut,
  // **suit** `ctx.setTransform` : une valeur en pixels d'écran telle quelle
  // s'y retrouvait multipliée par `echelleEcran` (des centaines de fois),
  // un trait si large qu'il recouvrait le canevas entier — repéré au rendu
  // (un aplat uni au lieu des territoires), pas à la lecture du code.
  g.shadowColor = "transparent";
  g.strokeStyle = encre;
  g.globalAlpha = 0.55;
  g.lineJoin = "round";
  g.lineWidth = Math.max(1, gn / 700) / echelleEcran;
  for (const [, groupe] of parTerritoire) g.stroke(chemin(groupe[0]));
  g.globalAlpha = 1;

  densite.image = c;
  densite.kRef = carte.vue.k;
}

/// Résolution de l'image hors-écran, asservie au zoom courant plutôt que
/// fixe : une image bâtie une fois pour toutes à 1024 restait nette à
/// l'échelle d'origine mais se pixellisait en zoomant, puisqu'elle n'est
/// alors plus qu'agrandie. Bornée aux deux bouts — 768 en dessous, pour ne
/// pas repartir d'une image inutilement grossière à faible zoom ; 3072
/// au-dessus, pour qu'un zoom extrême ne construise pas une image
/// démesurée. Passé ce plafond, un léger flou reste préférable à la
/// mémoire et au temps qu'exigerait une image plus grande — voir
/// `zoomer`, qui referait cette image après un geste de zoom, pas pendant.
function tailleDensiteEcran(r) {
  const dpr = window.devicePixelRatio || 1;
  const cote = 2 * echelle(r) * carte.vue.k * densite.demiCote * dpr;
  return Math.round(Math.min(3072, Math.max(768, cote)));
}

/// Invalide l'image hors-écran sans recharger le résultat de densité —
/// isoler une famille, tirer le curseur d'ombre ou finir un geste de zoom
/// change le rendu, pas le calcul.
function invaliderImageDensite() {
  if (densite) densite.image = null;
}

/// Recopie l'image hors-écran à l'échelle de la vue courante — le seul
/// travail refait à chaque image de panoramique ou de zoom.
function dessinerDensiteFamille(r) {
  if (!densite || densite.bandes.length === 0) return;
  if (!densite.image) construireImageDensite(r);
  const d = densite.demiCote;
  const [x0, y0] = versEcran({ x: -d, y: -d }, r);
  const [x1, y1] = versEcran({ x: d, y: d }, r);
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(densite.image, x0, y0, x1 - x0, y1 - y0);
}

/* ---------------------------------------- densité, variable continue */

// Hors chantier : la coloration par année/tempo/énergie garde son ancien
// calcul, entièrement en JS — une seule nappe, jamais de recouvrement entre
// familles à résoudre, donc aucun besoin du pavage vectoriel ci-dessus.

const DENSITE_GN_CONTINU = 128;
const NIVEAUX_CONTINU = [0.1, 0.24, 0.4, 0.58, 0.76, 0.92];

let densiteContinue = null; // cache séparé — voir `invaliderDensiteContinue`

function invaliderDensiteContinue() {
  densiteContinue = null;
}

function versGrilleContinu(v, gn) {
  const lo = -1 - DENSITE_MARGE, hi = 1 + DENSITE_MARGE;
  return Math.min(gn - 1, Math.max(0, Math.floor(((v - lo) / (hi - lo)) * gn)));
}

/// Flou en boîte, trois passes séparables : approxime un noyau gaussien sans
/// le coût d'une vraie convolution — suffisant pour une seule nappe basse
/// résolution, contrairement à la grille bien plus fine calculée côté Rust.
function flouterChampContinu(champ, gn, rayon = 2, passes = 3) {
  let src = champ;
  for (let pass = 0; pass < passes; pass++) {
    const h = new Float32Array(gn * gn);
    for (let y = 0; y < gn; y++) {
      for (let x = 0; x < gn; x++) {
        let s = 0, n = 0;
        for (let dx = -rayon; dx <= rayon; dx++) {
          const xx = x + dx;
          if (xx < 0 || xx >= gn) continue;
          s += src[y * gn + xx];
          n++;
        }
        h[y * gn + x] = s / n;
      }
    }
    const v = new Float32Array(gn * gn);
    for (let x = 0; x < gn; x++) {
      for (let y = 0; y < gn; y++) {
        let s = 0, n = 0;
        for (let dy = -rayon; dy <= rayon; dy++) {
          const yy = y + dy;
          if (yy < 0 || yy >= gn) continue;
          s += h[yy * gn + x];
          n++;
        }
        v[y * gn + x] = s / n;
      }
    }
    src = v;
  }
  champ.set(src);
}

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

function bitmapDensiteContinue(couche, gn, etapes, v0, v1) {
  const c = document.createElement("canvas");
  c.width = gn;
  c.height = gn;
  const g = c.getContext("2d");
  const img = g.createImageData(gn, gn);
  for (let i = 0; i < gn * gn; i++) {
    const t = couche.max > 0 ? couche.champ[i] / couche.max : 0;
    if (t < NIVEAUX_CONTINU[0]) {
      img.data[i * 4 + 3] = 0;
      continue;
    }
    let n = 0;
    for (const seuil of NIVEAUX_CONTINU) if (t >= seuil) n++;
    const creux = (n - 1) / (NIVEAUX_CONTINU.length - 1);

    const v = couche.pesValeurs[i] > 1e-6 ? couche.valeurs[i] / couche.pesValeurs[i] : null;
    const hex = v != null && v1 > v0 ? surRampe(etapes, Math.min(1, Math.max(0, (v - v0) / (v1 - v0)))) : "#9A9284";
    const [rr, gg, bb] = hexRGB(hex);
    const o = i * 4;
    img.data[o] = rr;
    img.data[o + 1] = gg;
    img.data[o + 2] = bb;
    img.data[o + 3] = Math.round((0.7 + 0.25 * creux) * 255);
  }
  g.putImageData(img, 0, 0);
  return c;
}

function construireDensiteContinue() {
  const gn = DENSITE_GN_CONTINU;
  const continu = CONTINUES[carte.couleur];
  const etapes = rampe();
  const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 0];

  const champ = new Float32Array(gn * gn);
  const valeurs = new Float32Array(gn * gn);
  const pesValeurs = new Float32Array(gn * gn);
  for (const p of carte.points) {
    const i = versGrilleContinu(p.y, gn) * gn + versGrilleContinu(p.x, gn);
    champ[i] += 1;
    const v = p[continu.champ];
    if (v != null && (!continu.valide || continu.valide(v))) {
      valeurs[i] += v;
      pesValeurs[i] += 1;
    }
  }
  flouterChampContinu(champ, gn);
  flouterChampContinu(valeurs, gn);
  flouterChampContinu(pesValeurs, gn);

  const couche = { champ, valeurs, pesValeurs };
  couche.max = Math.max(...champ);
  couche.bitmap = couche.max > 0 ? bitmapDensiteContinue(couche, gn, etapes, v0, v1) : null;
  return { gn, couche };
}

function dessinerDensiteContinue(r) {
  if (!densiteContinue) densiteContinue = construireDensiteContinue();
  const { couche } = densiteContinue;
  if (!couche.bitmap) return;
  const bord = -1 - DENSITE_MARGE;
  const [x0, y0] = versEcran({ x: bord, y: bord }, r);
  const [x1, y1] = versEcran({ x: -bord, y: -bord }, r);
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(couche.bitmap, x0, y0, x1 - x0, y1 - y0);
}

function dessinerDensite(r) {
  if (CONTINUES[carte.couleur]) dessinerDensiteContinue(r);
  else dessinerDensiteFamille(r);
}

function dessinerCarte() {
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

  if (carte.affichage === "densite") {
    // La nappe est le sujet en mode densité : le nuage de points ne se
    // dessine plus par-dessus.
    dessinerDensite(r);
  } else {
    dessinerNuage(r);
  }

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

    const tracer = () => {
      ctx.beginPath();
      carte.route.forEach((p, i) => {
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

/// Point le plus proche du curseur, dans un rayon raisonnable.
function pointSous(mx, my) {
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
    carte.vue.dx += e.movementX;
    carte.vue.dy += e.movementY;
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
  reconstruireDensiteApresZoom();
}

/// L'image hors-écran de la densité est bâtie pour un niveau de zoom donné
/// (`tailleDensiteEcran`) : la redessiner à chaque cran de molette la
/// referait des dizaines de fois par seconde pour rien, un délai court
/// après la fin du geste suffit — même principe que le bruit des chemins
/// ou la force de l'ombre.
let attenteZoomDensite = null;
function reconstruireDensiteApresZoom() {
  if (carte.affichage !== "densite" || !densite || !densite.image) return;
  // En dessous d'un tiers d'écart, l'image en cache reste assez nette :
  // pas la peine de la reconstruire pour un cran de molette.
  if (densite.kRef && carte.vue.k / densite.kRef < 1.3 && densite.kRef / carte.vue.k < 1.3) return;
  clearTimeout(attenteZoomDensite);
  attenteZoomDensite = setTimeout(() => {
    invaliderImageDensite();
    dessinerCarte();
  }, 220);
}

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
  carte.vue = { k: 1, dx: 0, dy: 0 };
  $("zoom-val").textContent = "×1,0";
  invaliderImageDensite();
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
  await invoke("play", { paths: [p.path] });
  poserLecture(true);
  sonder(true);
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
  const rayon = 24 / (echelle(r) * carte.vue.k);
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
    });
  } finally {
    patienter(null);
  }
  poserChemin(pistes, "le trait n'a touché aucun morceau");
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
      });
    } catch (e) {
      remonter(e, "chemin dessiné");
      return;
    } finally {
      patienter(null);
    }
    poserChemin(pistes, "le trait n'a touché aucun morceau");
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
    pistes = await invoke("selection", { trace: contour });
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
function tracerRouteSurCarte(pistes) {
  if (!carte.points.length) return;
  const parId = new Map(carte.points.map((p) => [p.id, p]));
  carte.route =
    pistes && pistes.length >= 2
      ? pistes.map((t) => parId.get(t.id)).filter(Boolean)
      : null;
  if (modeCourant === "explorer") dessinerCarte();
}

async function poserChemin(pistes, vide = "aucun chemin trouvé") {
  if (!pistes || pistes.length < 2) {
    $("fil-compte").textContent = vide;
    carte.route = null;
    dessinerCarte();
    return;
  }

  tracerRouteSurCarte(pistes);
  fileCourante = pistes;
  // Sans ce redessin, le panneau « file » resté ouvert continue de montrer
  // l'ancienne liste : rien d'autre ne le rafraîchit ici tant que le premier
  // morceau ne change pas — exactement le cas quand on ajuste la pondération
  // de l'errance depuis le même départ, où seule la suite change.
  if (!$("file").hidden) dessinerFile();
  // `set_queue`, pas `play` : si le premier morceau ne change pas — un
  // simple réglage du curseur de bruit ou « Autre tirage » sur le même
  // départ —, la lecture en cours n'a aucune raison de repartir de zéro.
  await invoke("set_queue", { paths: pistes.map((t) => t.path) });
  poserLecture(true);
  sonder(true);
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

/// La légende des familles : pastille, nom, effectif.
///
/// Les noms viennent du moteur. Ni le genre le plus fréquent — « Rock » domine
/// six familles sur douze et ne les distinguerait pas — ni le plus
/// caractéristique, qui nommait « Ska Rock » une famille de 4 321 morceaux
/// menée par Bob Marley. Les deux à la fois : voir `nommer_les_familles`.
async function dessinerFamilles() {
  const hote = $("familles");
  if (!carte.familles) {
    try {
      carte.familles = await invoke("families");
    } catch (e) {
      remonter(e, "familles");
      carte.familles = [];
    }
  }

  const teintes = couleursFamilles();
  hote.replaceChildren();
  for (const [c, nom, n] of carte.familles) {
    const el = document.createElement("button");
    el.className = "famille" + (carte.isolee === c ? " famille--isolee" : "");
    el.innerHTML = `<span class="famille__pastille"></span>
                    <span></span><span class="famille__n"></span>`;
    el.children[0].style.background = teintes[c % teintes.length] ?? "currentColor";
    // Une famille dont aucun genre ne ressort garde son numéro : mieux vaut un
    // nom neutre qu'un nom faux.
    el.children[1].textContent = nom || `famille ${c + 1}`;
    el.children[1].title = nom || "";
    el.children[2].textContent = n.toLocaleString("fr-FR");
    el.addEventListener("click", () => {
      carte.isolee = carte.isolee === c ? null : c;
      invaliderImageDensite(); // les territoires écartés changent de teinte
      dessinerFamilles();
      dessinerCarte();
    });
    hote.appendChild(el);
  }
}

document.querySelectorAll("[data-couleur]").forEach((b) =>
  b.addEventListener("click", () => {
    carte.couleur = b.dataset.couleur;
    document
      .querySelectorAll("[data-couleur]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
    majLegendeContinue();
    // Les familles n'ont de sens qu'en coloration par famille.
    $("bloc-familles").hidden = carte.couleur !== "famille";
    // Le pavage par territoires (Rust) ne dépend pas de « Colorer par » — il
    // n'y a qu'une seule nappe continue à refaire ici.
    invaliderDensiteContinue();
    dessinerCarte();
  }),
);

document.querySelectorAll("[data-affichage]").forEach((b) =>
  b.addEventListener("click", () => {
    carte.affichage = b.dataset.affichage;
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
}

document.querySelectorAll("[data-borne]").forEach((b) =>
  b.addEventListener("click", () => {
    carte[b.dataset.borne] = null;
    carte.route = null;
    dessinerBornes();
    dessinerCarte();
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
  if (carte.depart && carte.arrivee) {
    await tracerChemin({
      from: carte.depart.id,
      to: carte.arrivee.id,
      mode: carte.chemin,
    });
  }
}

function poserModeChemin(mode) {
  carte.chemin = mode;
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
  $("chemin-rejouer").hidden = !carte.refaire;
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
    chargerDensite().catch((e) => remonter(e, "densité")),
  ]);
  carte.points = points;
  invaliderDensiteContinue(); // les points ont changé, la nappe continue doit se refaire
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
  dessinerCarte();
}

/* ---------------------------------------------------------- modes */

async function basculerMode(mode) {
  const explorer = mode === "explorer";
  const editer = mode === "editer";
  const bibliotheque = mode === "bibliotheque";
  const decouvrir = mode === "decouvrir";
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
  $("liste").hidden = explorer || bibliotheque || decouvrir || vue.quoi === "albums";
  $("grille").hidden = explorer || bibliotheque || decouvrir || vue.quoi !== "albums";
  $("retour").hidden = explorer || bibliotheque || decouvrir || vue.retour === null;
  $("index-alpha").hidden = $("index-alpha").hidden || bibliotheque || decouvrir;
  $("bloc-vue-lib").hidden = explorer || editer || bibliotheque || decouvrir;
  $("bloc-colorer").hidden = !explorer;
  $("bloc-chemin").hidden = !explorer;
  $("bloc-familles").hidden = !explorer || carte.couleur !== "famille";
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
    majCacheStems().catch((e) => remonter(e, "stems"));
    chargerStatsBibliotheque().catch((e) => remonter(e, "statistiques"));
    chargerVerifications().catch((e) => remonter(e, "vérifications"));
    chargerParametresCarte().catch((e) => remonter(e, "paramètres de la carte"));
  } else if (decouvrir) {
    $("fil-titre").textContent = "Découvrir";
    $("fil-compte").textContent = "";
    chargerArtistesDecouvrir().catch((e) => remonter(e, "découvrir"));
  } else {
    poser(vue.quoi, vue.titre, vue.lignes, vue.retour);
    // Le mode Éditer travaille sur la sélection courante : on la relit à
    // chaque entrée plutôt que de la mémoriser, elle a pu changer depuis.
    if (editer) poserSourceEdition();
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

$("decouvrir-contact").value = localStorage.getItem("decouvrir-contact") || "";
$("decouvrir-contact").addEventListener("change", (e) => {
  localStorage.setItem("decouvrir-contact", e.target.value.trim());
});

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
  return $("decouvrir-contact").value.trim();
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

$("recalculer-carte").addEventListener("click", async () => {
  const bouton = $("recalculer-carte");
  bouton.disabled = true;
  $("carte-parametres-etat").textContent = "Recalcul en cours…";
  try {
    const r = await invoke("recompute_map");
    $("carte-parametres-etat").textContent =
      `${r.empreintes.toLocaleString("fr-FR")} morceaux replacés, ${r.familles.toLocaleString("fr-FR")} familles.`;
    // La carte affichée, si elle l'est, montre des positions périmées ; les
    // familles nommées le sont tout autant.
    carte.familles = null;
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
      await chargerDensite();
      invaliderImageDensite();
      dessinerCarte();
    }
  } catch (e) {
    remonter(e, "recalcul de la densité");
    $("densite-parametres-etat").textContent = String(e);
  } finally {
    bouton.disabled = false;
  }
});

// Force de l'ombre : redessine seulement, aucun aller-retour vers le moteur
// — mêmes principe et délai que le bruit des chemins.
$("densite-ombre").value = forceOmbre;
$("densite-ombre-valeur").textContent = forceOmbre.toFixed(2).replace(".", ",");
let attenteOmbre = null;
$("densite-ombre").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  forceOmbre = Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : OMBRE_DEFAUT;
  $("densite-ombre-valeur").textContent = forceOmbre.toFixed(2).replace(".", ",");
  localStorage.setItem("densite-ombre", String(forceOmbre));
  clearTimeout(attenteOmbre);
  attenteOmbre = setTimeout(() => {
    invaliderImageDensite();
    if (modeCourant === "explorer") dessinerCarte();
  }, 120);
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

let sondageScan = null;
$("lancer-scan").addEventListener("click", async () => {
  const chemin = $("nouveau-dossier").value.trim();
  if (!chemin) return;
  try {
    // Pas de case « force » ici : un dossier qu'on vient d'ajouter n'a rien
    // à relire, tout y est encore neuf. C'est le bouton « Analyser » de la
    // racine, une fois apparue dans la liste, qui la propose pour la suite.
    await invoke("start_scan", { path: chemin, force: false });
  } catch (e) {
    $("scan-etat").textContent = String(e);
    return;
  }
  $("lancer-scan").disabled = true;

  // Le scan tourne dans son thread : on suit son avancement par sondage. Sur
  // un support lent il dure des dizaines de minutes, d'où le pas d'une seconde.
  clearInterval(sondageScan);
  sondageScan = setInterval(async () => {
    const s = await invoke("scan_state");
    if (s.en_cours) {
      $("scan-etat").textContent = `scan en cours… ${s.morceaux.toLocaleString("fr-FR")} morceaux en base`;
    } else {
      clearInterval(sondageScan);
      sondageScan = null;
      $("lancer-scan").disabled = false;
      $("scan-etat").textContent = s.resultat ?? "";
      await dessinerRacines();
      await charger();
    }
  }, 1000);
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
/// empreintes CLAP, tempo/tonalité/énergie, puis genres MusicBrainz si une
/// adresse de contact est renseignée. Un seul bouton par racine plutôt que
/// quatre sections séparées à faire tourner soi-même dans l'ordre — c'est
/// toujours le même ordre, autant l'écrire une fois.
///
/// Les quatre passes restent globales côté moteur (pas de filtre par
/// racine) : dans l'usage courant — une racine qu'on vient d'ajouter ou de
/// changer — ce qui est « en attente » est justement ce qu'on vient de
/// scanner, donc le résultat correspond à l'intention même sans filtrage
/// explicite.
async function analyserRacine(chemin) {
  document.querySelectorAll(".racine__analyser").forEach((b) => (b.disabled = true));
  const force = $("analyse-force").checked;
  const contact = $("analyse-contact").value.trim();
  const etat = $("racines-etat");
  const jauge = $("racines-jauge");
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
      majJauge("racines-jauge", a.en_cours, a.faits, a.total);
      if (a.en_cours) {
        const reste = Math.max(0, a.total - a.faits);
        etat.textContent = a.total
          ? `${chemin} — empreintes : ${a.faits.toLocaleString("fr-FR")} / ${a.total.toLocaleString("fr-FR")} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
          : `${chemin} — empreintes…`;
      }
    });
    // La projection a replacé tous les points : la carte affichée est
    // périmée, y compris ses familles.
    if (modeCourant === "explorer") await chargerCarte();

    etat.textContent = `${chemin} — tempo, tonalité, énergie…`;
    await invoke("start_descripteurs", { force });
    await attendreFin("descripteurs_state", 2000, (d) => {
      majJauge("racines-jauge", d.en_cours, d.faits, d.total);
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
        majJauge("racines-jauge", e.en_cours, e.artistes, e.total);
        if (e.en_cours) {
          const reste = Math.max(0, e.total - e.artistes);
          etat.textContent = e.total
            ? `${chemin} — genres : ${e.artistes.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")} artistes — reste ${dureeLongue(reste * SECONDES_PAR_ARTISTE)}`
            : `${chemin} — genres…`;
        }
      });
      // Nommées à la volée : il suffit de les redemander.
      carte.familles = null;
      if (modeCourant === "explorer") await dessinerFamilles();
    }

    jauge.hidden = true;
    etat.textContent = `${chemin} — terminé.`;
    await dessinerRacines();
    await chargerStatsBibliotheque();
    await chargerVerifications();
  } catch (e) {
    remonter(e, "analyse de la racine");
    etat.textContent = String(e);
  } finally {
    document.querySelectorAll(".racine__analyser").forEach((b) => (b.disabled = false));
  }
}

/* --------------------------------------------------- cache de stems */

/// Ce que les séparations occupent sur le disque.
///
/// Un jeu de quatre stems pèse 124 Mo : quinze morceaux séparés remplissent
/// deux gigaoctets sans que rien ne le dise. Le montrer ici est la seule chose
/// qui empêche la fuite d'être silencieuse.
async function majCacheStems() {
  const [octets, morceaux] = await invoke("stems_cache");
  $("vider-stems").disabled = morceaux === 0;
  $("stems-cache").textContent = morceaux
    ? `${morceaux} morceau${morceaux > 1 ? "x" : ""} séparé${morceaux > 1 ? "s" : ""} — ` +
      `${(octets / 1e9).toFixed(2).replace(".", ",")} Go. ` +
      `Les vider force à redémixer, rien d'autre n'est perdu.`
    : "Aucun morceau séparé pour l'instant.";
}

$("vider-stems").addEventListener("click", async () => {
  try {
    await invoke("stems_cache_vider");
  } catch (e) {
    remonter(e, "vidage des stems");
  }
  await majCacheStems();
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
  const seuil = frac * wave.children.length;
  for (let i = 0; i < wave.children.length; i++) {
    wave.children[i].classList.toggle("on", i < seuil);
  }
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

charger("albums").catch((e) => {
  $("fil-titre").textContent = "Erreur";
  $("sommaire").textContent = String(e);
});
