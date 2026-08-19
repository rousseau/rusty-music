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
  quoi: "artistes", // artistes | albums | pistes | recherche
  lignes: [],
  titre: "Artistes",
  retour: null, // état à restaurer en remontant
};

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
  } else if (vue.quoi === "albums") {
    el.innerHTML = `<span class="ligne__nom"></span>
                    <span class="ligne__sec"></span>
                    <span class="ligne__cpt"></span>`;
    el.children[0].textContent = item.name;
    el.children[1].textContent = txt(item.artist, "(sans artiste)");
    el.children[2].textContent = `${item.year ?? "————"} · ${item.tracks}`;
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

/* ---------------------------------------------------------- navigation */

function poser(quoi, titre, lignes, retour = null) {
  vue.quoi = quoi;
  vue.titre = titre;
  vue.lignes = lignes;
  vue.retour = retour;
  $("fil-titre").textContent = titre;
  $("fil-compte").textContent = `${lignes.length} ${quoi === "artistes" ? "artistes" : quoi === "albums" ? "albums" : "morceaux"}`;
  $("retour").hidden = retour === null;
  $("retour").textContent = `← ${retour ? retour.titre : ""}`;
  liste.scrollTop = 0;
  dessiner();
}

async function activer(item) {
  if (vue.quoi === "artistes") {
    // Les deux sont nécessaires : un artiste réunit ses pistes étiquetées
    // MusicBrainz et les autres.
    const albums = await invoke("albums", { mbid: item.mbid ?? null, artist: item.name });
    poser("albums", item.name, albums, { quoi: "artistes", titre: "Artistes", lignes: vue.lignes });
  } else if (vue.quoi === "albums") {
    const pistes = await invoke("tracks_of_album", { album: item.name, artist: item.artist ?? null });
    poser("pistes", item.name, pistes, { quoi: vue.quoi, titre: vue.titre, lignes: vue.lignes, retour: vue.retour });
  } else {
    inspecter(item);
    // Lire depuis la piste choisie : la suite de la liste forme la file.
    const depart = vue.lignes.indexOf(item);
    fileCourante = vue.lignes.slice(depart);
    await invoke("play", { paths: fileCourante.map((t) => t.path) });
    poserLecture(true);
    sonder(true);
  }
}

$("retour").addEventListener("click", () => {
  const r = vue.retour;
  if (r) poser(r.quoi, r.titre, r.lignes, r.retour ?? null);
});

/* ---------------------------------------------------------- inspecteur */

async function inspecter(t) {
  $("insp-vide").hidden = true;
  $("insp").hidden = false;
  // Sert à savoir, au retour d'un calcul, si l'inspecteur montre encore le
  // même morceau.
  $("insp-titre").dataset.path = t.path;
  $("insp-titre").textContent = txt(t.title, "(sans titre)");
  $("insp-artiste").textContent = txt(t.artist, "(sans artiste)");
  $("insp-album").textContent = txt(t.album, "—");
  $("insp-annee").textContent = t.year ?? "—";
  $("insp-piste").textContent = t.track_no ?? "—";
  $("insp-duree").textContent = duree(t.duration_ms);

  const img = await pochette(t.path);
  const el = $("pochette");
  el.style.backgroundImage = img ? `url("${img}")` : "";
  el.classList.toggle("pochette--pleine", Boolean(img));

  montrerVoisins(t);
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
    poser("pistes", `« ${q} »`, r, { quoi: "artistes", titre: "Artistes", lignes: [] });
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

$("lecture").addEventListener("click", async () => {
  // Un clic à la fois : les suivants sont ignorés, pas empilés. Empilés, ils
  // s'annulaient deux à deux.
  if (clicEnVol) return;
  clicEnVol = true;
  try {
    await basculerLecture();
  } finally {
    clicEnVol = false;
  }
});

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
$("precedent").addEventListener("click", async () => {
  await invoke("previous");
  sonder(true);
});
$("suivant").addEventListener("click", async () => {
  await invoke("skip");
  sonder(true);
});
$("volume").addEventListener("input", (e) => invoke("set_volume", { volume: e.target.value / 100 }));

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
    // Pochette hors du chemin critique : elle arrivera quand elle arrivera.
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
  familles: null, // [[rang, nom, effectif]], chargé une fois
  bornes: {}, // min et max de chaque variable continue, pour la rampe
  filtre: "", // texte du filtre ; les exclus s'estompent, jamais ne disparaissent
  chemin: "direct", // direct | lisse | errance | dessine
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
  lisse: [
    "Clic : le départ. Maj+clic : l'arrivée. Le trajet ne passe que d'un proche voisin au suivant : plus long, sans à-coup. Le nombre de morceaux n'est ici qu'un plafond.",
    "maj+clic : l'arrivée",
  ],
  errance: [
    "Maj+clic : une promenade au hasard part de ce morceau et dérive sans jamais revenir sur ses pas.",
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
  annee: { champ: "year", format: (v) => String(Math.round(v)) },
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

  const rayon = Math.max(1.1, 1.9 * Math.sqrt(carte.vue.k));
  const etapes = rampe();
  const teintes = couleursFamilles();
  const continu = CONTINUES[carte.couleur];
  const [v0, v1] = carte.bornes[carte.couleur] ?? [0, 0];
  const neutre = carte.isolee === null && !carte.filtre;

  // Deux passes : le fond estompé d'abord, la sélection par-dessus, pour
  // qu'elle ne soit jamais recouverte.
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
  if (carte.route && carte.route.length > 1) {
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.globalAlpha = 0.85;
    ctx.beginPath();
    carte.route.forEach((p, i) => {
      const [x, y] = versEcran(p, r);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();
    ctx.fillStyle = accent;
    for (const p of carte.route) {
      const [x, y] = versEcran(p, r);
      ctx.beginPath();
      ctx.arc(x, y, 3.5, 0, Math.PI * 2);
      ctx.fill();
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

/// Combien de morceaux le chemin doit compter — plafond pour le mode lisse,
/// dont la longueur naturelle est celle du graphe.
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
    });
  } finally {
    patienter(null);
  }
  poserChemin(pistes);
}

/// Chemin dessiné : le tracé part en coordonnées de carte, avec le rayon de
/// cueillette que vaut le zoom courant. 24 px à l'écran, quel que soit le
/// facteur — c'est ce que l'utilisateur croit toucher avec son trait.
async function tracerDessin(trace) {
  const r = cnv.getBoundingClientRect();
  const rayon = 24 / (echelle(r) * carte.vue.k);
  carte.refaire = null;
  patienter("cueillette sous le trait…");
  let pistes;
  try {
    pistes = await invoke("path_drawn", {
      trace,
      steps: longueurChemin(),
      radius: rayon,
    });
  } finally {
    patienter(null);
  }
  poserChemin(pistes, "le trait n'a touché aucun morceau");
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
async function poserChemin(pistes, vide = "aucun chemin trouvé") {
  if (!pistes || pistes.length < 2) {
    $("fil-compte").textContent = vide;
    carte.route = null;
    dessinerCarte();
    return;
  }

  // Le tracé suit les points de la carte, dans l'ordre du trajet.
  const parId = new Map(carte.points.map((p) => [p.id, p]));
  carte.route = pistes.map((t) => parId.get(t.id)).filter(Boolean);

  fileCourante = pistes;
  await invoke("play", { paths: pistes.map((t) => t.path) });
  poserLecture(true);
  sonder(true);
  inspecter(pistes[0]);
  dessinerCarte();
  $("fil-compte").textContent = `chemin de ${pistes.length} morceaux`;
  $("chemin-rejouer").hidden = carte.refaire?.mode !== "errance";
}

/// Signale un calcul en cours dans le pied de carte, ou l'efface.
///
/// Le premier chemin lisse ou errant construit le graphe des voisins : une
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
  document
    .querySelectorAll("[data-chemin]")
    .forEach((s) => s.classList.toggle("segment--actif", s.dataset.chemin === mode));
  $("chemin-aide").textContent =
    `${AIDE_CHEMIN[mode][0]} Ou : chercher puis Entrée pour poser une borne.`;
  $("carte-aide").textContent = aideCourante();
  $("chemin-rejouer").hidden = mode !== "errance" || !carte.route;
  dessinerBornes();
  dessinerCarte();

  // Lisse et errance passent par le graphe des voisins : on le prépare dès le
  // choix du mode, pour que le premier chemin ne paie pas la construction.
  if (mode === "lisse" || mode === "errance") preparerGraphe();
}

document
  .querySelectorAll("[data-chemin]")
  .forEach((b) => b.addEventListener("click", () => poserModeChemin(b.dataset.chemin)));

$("chemin-rejouer").addEventListener("click", async () => {
  if (!carte.refaire) return;
  carte.graine += 1;
  await tracerChemin(carte.refaire);
});

/// Construit le graphe des voisins en tâche de fond, une seule fois à la fois.
///
/// Le balayage est complet : une vingtaine de secondes sur la bibliothèque
/// entière, davantage à mesure que l'analyse la remplit. Rien n'attend le
/// résultat — la commande met le graphe en cache côté moteur, les chemins
/// suivants le trouvent prêt.
///
/// **Le dire est indispensable.** Le calcul sature tous les cœurs ; muet, il se
/// lit comme un plantage, ventilateurs compris. On l'annonce donc dans le rail
/// et en pied de carte, et on efface dès que c'est prêt.
let graphePret = null;
function preparerGraphe() {
  if (graphePret) return graphePret;
  const attente = "Préparation du graphe des voisins…";
  $("chemin-aide").textContent = attente;
  patienter(attente);
  graphePret = invoke("prepare_graph")
    .catch((e) => remonter(e, "préparation du graphe"))
    .finally(() => {
      graphePret = null;
      // Le mode a pu changer entre-temps : on réaffiche l'aide du mode
      // courant, pas celle de celui qui avait lancé la préparation.
      $("chemin-aide").textContent =
        `${AIDE_CHEMIN[carte.chemin][0]} Ou : chercher puis Entrée pour poser une borne.`;
      patienter();
    });
  return graphePret;
}

async function chargerCarte() {
  carte.points = await invoke("map_view");
  const [faits, total] = await invoke("map_progress");

  // Les bornes de chaque variable, une fois pour toutes au chargement.
  for (const [cle, { champ }] of Object.entries(CONTINUES)) {
    const vs = carte.points.map((p) => p[champ]).filter((v) => v != null);
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
  document.querySelectorAll(".mode").forEach((b) =>
    b.classList.toggle("mode--actif", b.dataset.mode === mode),
  );
  modeCourant = mode;
  // Entrer dans l'éditeur avec des stems déjà affichés doit les rendre
  // audibles : c'est ce qu'on vient y faire.
  if (editer) prendreLaMain().catch((e) => remonter(e, "stems"));
  $("carte-vue").hidden = !explorer;
  $("liste").hidden = explorer;
  $("retour").hidden = explorer || vue.retour === null;
  $("bloc-colorer").hidden = !explorer;
  $("bloc-chemin").hidden = !explorer;
  $("bloc-familles").hidden = !explorer || carte.couleur !== "famille";
  $("bloc-demix").hidden = !editer;
  $("dock").hidden = !editer || edition.stems.length === 0;

  // Sortir de l'édition rend la sortie au lecteur ordinaire : garder les
  // stems chargés tiendrait 186 Mo et une sortie audio pour rien.
  if (!editer && edition.enLecture) await arreterStems();

  if (explorer) {
    poserModeChemin(carte.chemin);
    await chargerCarte();
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

/* ---------------------------------------------------------- réglages */

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
                    <button class="bouton bouton--danger">Oublier</button>`;
    el.children[0].textContent = r.path;
    el.children[1].textContent = `${r.tracks.toLocaleString("fr-FR")} morceaux`;
    el.children[2].addEventListener("click", async () => {
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

function basculerReglages(ouvrir) {
  const v = $("voile");
  const visible = ouvrir ?? v.hidden;
  v.hidden = !visible;
  if (visible) {
    dessinerRacines();
    // Le nombre en attente change dès qu'un scan passe : on le recompte à
    // chaque ouverture plutôt que de le garder en mémoire.
    compterEnAttente();
  }
}

$("ouvrir-reglages").addEventListener("click", () => {
  basculerReglages(true);
  compterGenres().catch((e) => remonter(e, "genres"));
  majCacheStems().catch((e) => remonter(e, "stems"));
});
$("reglages-fermer").addEventListener("click", () => basculerReglages(false));
$("voile").addEventListener("click", (e) => {
  if (e.target === $("voile")) basculerReglages(false);
});

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
    await invoke("start_scan", { path: chemin });
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
      await compterEnAttente();
    }
  }, 1000);
});

/* ---------------------------------------------------------- analyse */

/// Combien de morceaux attendent leur empreinte, et ce que ça coûtera.
///
/// Le chiffre est affiché avant le bouton, jamais après : une passe se compte
/// en heures sur un support lent, et l'utilisateur doit savoir à quoi il
/// s'engage. 1,1 s/morceau est la mesure faite sur la carte SD — le stockage
/// interne va bien plus vite, l'estimation est donc un plafond.
const SECONDES_PAR_MORCEAU = 1.1;

async function compterEnAttente() {
  const [faits, total] = await invoke("map_progress");
  const restants = total - faits;
  const el = $("analyse-attente");
  $("lancer-analyse").disabled = restants === 0;
  if (restants === 0) {
    el.textContent = `Les ${total.toLocaleString("fr-FR")} morceaux sont analysés.`;
    return;
  }
  el.textContent =
    `${restants.toLocaleString("fr-FR")} morceaux en attente d'empreinte — ` +
    `compter environ ${dureeLongue(restants * SECONDES_PAR_MORCEAU)}. ` +
    `La carte se remplit à la fin, quand la projection replace l'ensemble.`;
}

/// Durée en heures ou minutes, pour annoncer une passe qui dure.
function dureeLongue(s) {
  if (s < 90) return `${Math.round(s)} s`;
  if (s < 5400) return `${Math.round(s / 60)} min`;
  return `${(s / 3600).toFixed(1)} h`.replace(".", ",");
}

let sondageAnalyse = null;
$("lancer-analyse").addEventListener("click", async () => {
  try {
    await invoke("start_analysis");
  } catch (e) {
    $("analyse-etat").textContent = String(e);
    return;
  }
  $("lancer-analyse").disabled = true;

  clearInterval(sondageAnalyse);
  sondageAnalyse = setInterval(async () => {
    const a = await invoke("analysis_state");
    if (a.en_cours) {
      const reste = Math.max(0, a.total - a.faits);
      $("analyse-etat").textContent = a.total
        ? `${a.faits.toLocaleString("fr-FR")} / ${a.total.toLocaleString("fr-FR")} — reste ${dureeLongue(reste * SECONDES_PAR_MORCEAU)}`
        : "démarrage…";
    } else {
      clearInterval(sondageAnalyse);
      sondageAnalyse = null;
      $("analyse-etat").textContent = a.resultat ?? "";
      await compterEnAttente();
      // La projection a replacé tous les points : la carte affichée est
      // périmée, y compris ses familles.
      if (modeCourant === "explorer") await chargerCarte();
    }
  }, 2000);
});

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

/* ------------------------------------------------ genres MusicBrainz */

/// L'aspiration des genres, dans les réglages à côté de l'analyse.
///
/// Deux requêtes par artiste et une par seconde : c'est MusicBrainz qui impose
/// le rythme. On annonce donc la durée **avant** le bouton, comme pour
/// l'analyse — s'engager dans deux heures sans le savoir n'est pas un choix.
///
/// L'adresse de contact n'est pas une formalité : MusicBrainz exige un agent
/// qui identifie l'application et donne un moyen de la joindre. Sans elle, la
/// passe ne récolterait que des refus.
const SECONDES_PAR_ARTISTE = 5;

async function compterGenres() {
  const e = await invoke("enrichment_state");
  const [faits, total] = [e.artistes, e.total];
  const el = $("genres-attente");
  if (e.en_cours) {
    el.textContent = "Aspiration en cours.";
    return;
  }
  const restants = Math.max(0, total - faits);
  $("lancer-genres").disabled = false;
  el.textContent = restants
    ? `Les familles de la carte sont nommées par les tags des fichiers. ` +
      `MusicBrainz les nomme mieux : compter environ ` +
      `${dureeLongue(restants * SECONDES_PAR_ARTISTE)} pour ${restants.toLocaleString("fr-FR")} artistes.`
    : `Genres à jour. Les familles portent le vocabulaire de MusicBrainz.`;
}

let sondageGenres = null;
$("lancer-genres").addEventListener("click", async () => {
  try {
    await invoke("start_enrichment", { contact: $("genres-contact").value });
  } catch (e) {
    $("genres-etat").textContent = String(e);
    return;
  }
  $("lancer-genres").disabled = true;

  clearInterval(sondageGenres);
  sondageGenres = setInterval(async () => {
    const e = await invoke("enrichment_state");
    if (e.en_cours) {
      const reste = Math.max(0, e.total - e.artistes);
      $("genres-etat").textContent = e.total
        ? `${e.artistes.toLocaleString("fr-FR")} / ${e.total.toLocaleString("fr-FR")} artistes · ` +
          `${e.avec_genre.toLocaleString("fr-FR")} avec un genre — reste ${dureeLongue(reste * SECONDES_PAR_ARTISTE)}`
        : "démarrage…";
    } else {
      clearInterval(sondageGenres);
      sondageGenres = null;
      $("genres-etat").textContent = e.resultat ?? "";
      await compterGenres();
      // Les familles sont nommées à la volée : il suffit de les redemander.
      carte.familles = null;
      if (modeCourant === "explorer") await dessinerFamilles();
    }
  }, 3000);
});

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
    immediat: true,
  },
  tonalite: {
    pas: 1,
    min: -12,
    max: 12,
    ecrire: (v) => (v > 0 ? `+${v}` : v === 0 ? "±0" : `${v}`),
    immediat: false,
  },
};

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
  await invoke("stems_vitesse", { vitesse: edition.vitesse });
  for (const [i, s] of edition.stems.entries()) {
    if (s.vitesse !== null) {
      await invoke("stems_vitesse_stem", { index: i, vitesse: s.vitesse });
    }
  }
  majDerive();
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
    $(`val-${nom}`).textContent = r.ecrire(edition[nom]);
  }
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
      <span></span>
      <button data-r="vitesse" data-d="1" aria-label="Accélérer ce stem">+</button>
    </div>
    <div class="reglage" role="group" aria-label="Hauteur de ce stem"
         title="Transposition de ce stem seul, à durée inchangée. Quelques secondes de calcul, mises en cache.">
      <b>hauteur</b>
      <button data-r="tonalite" data-d="-1" aria-label="Baisser ce stem d'un demi-ton">−</button>
      <span></span>
      <button data-r="tonalite" data-d="1" aria-label="Monter ce stem d'un demi-ton">+</button>
    </div>
    <button class="stem__b" data-a="ensemble">suivre l'ensemble</button>`;
  pan.appendChild(ligne);

  const groupes = ligne.querySelectorAll(".reglage");
  const ensemble = ligne.querySelector('[data-a="ensemble"]');
  const rafraichir = () => {
    groupes[0].querySelector("span").textContent = REGLAGES.vitesse.ecrire(vitesseDe(s));
    groupes[1].querySelector("span").textContent = REGLAGES.tonalite.ecrire(tonaliteDe(s));
    ensemble.disabled = s.vitesse === null && s.tonalite === null;
    badge.textContent = etiquetteStem(s);
    badge.classList.toggle("stem__b--regle", stemEcarte(s));
    majDerive();
  };
  rafraichir();

  ligne.querySelectorAll(".reglage button").forEach((b) => {
    b.addEventListener("click", async () => {
      const nom = b.dataset.r;
      // Un stem qui suivait l'ensemble part de la valeur d'ensemble : le
      // premier clic déplace d'un pas, il ne saute pas à 100 %.
      const depart = nom === "vitesse" ? vitesseDe(s) : tonaliteDe(s);
      s[nom] = calerReglage(nom, depart + REGLAGES[nom].pas * Number(b.dataset.d));
      rafraichir();
      if (REGLAGES[nom].immediat) await appliquerVitesses();
      else await appliquerReglages();
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

async function charger() {
  const [artistes, racines] = await Promise.all([invoke("artists"), invoke("roots")]);
  poser("artistes", "Artistes", artistes);

  const total = racines.reduce((n, r) => n + r.tracks, 0);
  $("sommaire").textContent = `${total.toLocaleString("fr-FR")} morceaux\n${artistes.length.toLocaleString("fr-FR")} artistes`;
  $("sommaire").style.whiteSpace = "pre-line";
}

charger().catch((e) => {
  $("fil-titre").textContent = "Erreur";
  $("sommaire").textContent = String(e);
});
