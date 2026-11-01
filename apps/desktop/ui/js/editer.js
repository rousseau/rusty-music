import "./bibliotheque.js"; // l'ordre d'exécution : voir main.js
import {
  $, horloge, invoke, remonter,
} from "./socle.js";
import {
  appliquerVitesses, deplacerLecture, dessinerReglages, majUsage, montrerPartition, REGLAGES,
} from "./pratiquer.js";

/* ---------------------------------------------------------- éditer */

// Le morceau que le mode Éditer travaille. Ce n'est pas forcément celui en
// lecture : on peut séparer un morceau tout en en écoutant un autre.
const edition = {
  source: null, // TrackRow
  variante: "htdemucs",
  // Tant que rien n'est choisi à la main, la variante suit l'usage :
  // BS-RoFormer pour pratiquer (batterie +2,4 dB, basse transcrite plus juste),
  // la standard pour créer (bien plus rapide). Voir `varianteParDefaut`.
  varianteChoisie: false,
  // Un stem : { nom, chemin, origine, niveau, vitesse, tonalite, greffe, voisins }
  // `chemin` est ce qu'on joue — il change quand on greffe ; `origine` est le
  // stem séparé, qu'on ne perd jamais de vue. `vitesse` et `tonalite` valent
  // `null` tant que le stem suit les réglages d'ensemble.
  stems: [],
  muets: new Set(),
  solo: null,
  // Le stem dont le détail est ouvert dans l'inspecteur (`#bloc-stem`), par nom.
  stemSel: null,
  // `true` : l'utilisateur est revenu au sélecteur de morceau depuis l'établi
  // (bouton « ← Choisir un morceau ») sans décharger les stems.
  montrerChoix: false,
  // `true` : une piste a été choisie explicitement (clic dans la liste) — on
  // passe alors de « choisir » à « séparer ». Sans ça, entrer dans le mode
  // reste sur le sélecteur, le morceau en lecture n'étant qu'un candidat.
  sourceChoisie: false,
  enLecture: false, // un multipiste est-il chargé côté moteur ?
  tete: 0, // position de lecture, de 0 à 1 — partagée par tous les affichages
  // Vitesse et tonalité **d'ensemble**. Elles pilotent tous les stems que rien
  // n'en écarte ; un stem peut avoir les siennes (`docs/ui-spec-editeur.md`,
  // décision 4 : global par défaut, par stem en option).
  vitesse: 1.0,
  tonalite: 0,
  // Le réglage « vitesse » se lit-il en BPM plutôt qu'en pour cent ? N'est
  // vrai que si `tempoSource` existe (pulsation franche) et que l'utilisateur
  // l'a choisi (bouton d'unité). `docs/ui-spec-editeur.md`, décision 9.
  // Vitesse en BPM (par défaut) ou en pour cent : une préférence gardée
  // d'un morceau à l'autre, appliquée quand le morceau pulse franchement.
  vitesseEnBpm: (() => {
    try {
      return localStorage.getItem("vitesse-unite") !== "%";
    } catch {
      return true;
    }
  })(),
  // { bpm, nettete } du morceau ouvert, ou `null` si sa pulsation n'est pas
  // assez franche pour viser un BPM (nappe, rubato). Rempli par `majTempoSource`
  // après que les stems sont là — la netteté se mesure sur le stem `drums`.
  tempoSource: null,
  // Dernière dérive mesurée par le moteur, en millisecondes.
  deriveMs: 0,
  // Temps et premiers temps de mesure du morceau ouvert (`pulsation`, Beat
  // This!) : { temps, premiers_temps, bpm, metrique }, ou `null` tant qu'ils
  // ne sont pas calculés. Sert la boucle et, plus tard, la partition.
  pulsation: null,
  // Sections du morceau ouvert (all-in-one) : [{ debut_s, fin_s, etiquette }],
  // ou `null` tant qu'elles ne sont pas calculées (ou poids absents).
  sections: null,
  // La boucle posée : { debut, fin, mesures } en secondes d'origine, ou `null`.
  boucle: null,
  // Nombre de mesures que « boucler » prend — retenu d'une boucle à l'autre.
  mesuresBoucle: 4,
  // Accélérer d'un cran à chaque passage (Soundslice) : jusqu'à 100 %.
  accelBoucle: false,
  passagesVus: 0,
  dureeMs: 0,
  // « creer » (la pile de stems) ou « pratiquer » (la partition au centre) —
  // choisi dans le rail (`docs/ui-spec-editeur.md`, décisions 11 et 12).
  usage: "creer",
  // La partie affichée en Pratiquer : « bass » (tablature) ou « drums »
  // (portée de percussion). Choisie dans le rail, sans lien avec le solo ni
  // la coupure : on peut lire sa partie en l'écoutant, ou la jouer à la place.
  instrument: "bass",
  // Dernière position lue au moteur, l'instant où on l'a lue et l'état de
  // pause : de quoi extrapoler la position entre deux sondages pour la
  // partition, qui veut une mise à jour toutes les 50 ms.
  posMs: 0,
  posLueA: 0,
  // Fin du décompte en cours (`performance.now()`) : jusque-là, la position
  // ne bouge pas, les stems attendent.
  decompteJusqua: 0,
  enPause: true,
  // Retard de ce qu'on entend sur la position du moteur (sortie audio, plus
  // de 200 ms en Bluetooth), et la position d'où l'écoute est repartie
  // (lecture, déplacement, boucle) : avant elle, la sortie joue encore ce
  // qu'elle avait reçu, le curseur l'attend.
  latenceMs: 0,
  ancreMs: 0,
};

/// Pas d'accélération par passage de boucle, et la vitesse où l'on s'arrête.
/// On ralentit pour apprendre un passage et on remonte au tempo d'origine ;
/// aller au-delà n'est pas l'objet.
const ACCEL_PAS = 0.02;
const ACCEL_CIBLE = 1.0;

/// En deçà, la pulsation n'est pas franche et on ne propose pas de BPM cible :
/// « au-delà de 2, la pulsation est franche » (`battements::Grille`).
const NETTETE_FRANCHE = 2.0;

/// Le BPM cible est-il actif ? Vrai seulement si le morceau pulse franchement
/// **et** que l'utilisateur a basculé l'unité.
function vitesseEnBpm() {
  return edition.vitesseEnBpm && !!edition.tempoSource;
}
function bpmSource() {
  return edition.tempoSource?.bpm ?? null;
}

/// Pas effectif d'un réglage : en mode BPM, un pas vaut 1 BPM, soit une
/// fraction de facteur qui dépend du tempo source.
function pasReglage(nom) {
  if (nom === "vitesse" && vitesseEnBpm()) return 1 / bpmSource();
  return REGLAGES[nom].pas;
}

/// Le repliement d'octave à proposer, ou 0 s'il n'y a pas lieu.
///
/// `edition.vitesse` est le rapport `bpm_cible / bpm_source`. Hors de
/// [1/√2, √2], viser ce BPM revient à un étirement de plus d'une demi-octave :
/// le bouton « ½ » ou « ×2 » ramène le rapport près de 1 en admettant que le
/// morceau se lit à demi- ou double-tempo (`greffe::tempo_replie`).
function octaveVitesse() {
  if (!vitesseEnBpm()) return 0;
  if (edition.vitesse > Math.SQRT2) return 0.5;
  if (edition.vitesse < Math.SQRT1_2) return 2;
  return 0;
}

/// Va chercher le tempo du morceau ouvert et la netteté de sa pulsation, et
/// décide si la barre d'outils peut proposer un BPM cible. Appelée une fois les
/// stems présents — la netteté se mesure sur le stem `drums`.
async function majTempoSource() {
  edition.tempoSource = null;
  const src = edition.source;
  if (src) {
    try {
      const t = await invoke("tempo_cible", { id: src.id });
      if (t.bpm && t.nettete !== null && t.nettete >= NETTETE_FRANCHE) {
        edition.tempoSource = { bpm: t.bpm, nettete: t.nettete };
      }
    } catch (e) {
      // Pas de BPM cible, on reste en pour cent — ce n'est pas une erreur à
      // remonter, juste un réglage de moins.
      console.warn("tempo_cible", e);
    }
  }
  dessinerReglages();
}

/// Va chercher la pulsation du morceau ouvert (temps et premiers temps de
/// mesure). Quelques secondes la première fois, en cache ensuite ; le réglage
/// « boucle » n'apparaît qu'une fois qu'elle est là.
async function majPulsation() {
  const src = edition.source;
  edition.pulsation = null;
  majBoucleUI();
  if (!src) return;
  try {
    const p = await invoke("pulsation", { id: src.id });
    // Un autre morceau a été ouvert pendant le calcul.
    if (edition.source?.id !== src.id) return;
    edition.pulsation = p.premiers_temps.length >= 2 ? p : null;
    if (edition.usage === "pratiquer") majUsage();
  } catch (e) {
    // Pas de boucle à la mesure : un réglage de moins, pas une erreur à
    // montrer (morceau sans pulsation, poids indisponibles hors ligne).
    console.warn("pulsation", e);
  }
  majBoucleUI();
}

/// Noms français des étiquettes de Harmonix ; « start » et « end » (le
/// silence avant et après la musique) restent sans nom.
const NOMS_SECTIONS = {
  intro: "intro", verse: "couplet", chorus: "refrain", bridge: "pont",
  inst: "instrumental", solo: "solo", break: "pause", outro: "outro",
  start: "", end: "",
};

/// Va chercher les sections du morceau ouvert (all-in-one sur ses stems).
/// Quelques secondes la première fois, en cache ensuite ; sans les poids,
/// le bandeau ne s'affiche simplement pas.
async function majSections() {
  const src = edition.source;
  edition.sections = null;
  dessinerSections();
  if (!src) return;
  try {
    if (!(await invoke("sections_disponibles"))) return;
    const v = await invoke("sections", { id: src.id });
    if (edition.source?.id !== src.id) return;
    // Des sections contiguës de même étiquette se lisent comme une seule.
    const fusion = [];
    for (const x of v.sections) {
      const der = fusion[fusion.length - 1];
      if (der && der.etiquette === x.etiquette) der.fin_s = x.fin_s;
      else fusion.push({ ...x });
    }
    edition.sections = fusion.length ? fusion : null;
  } catch (e) {
    console.warn("sections", e);
  }
  dessinerSections();
  // La partition déjà affichée prend ses marques de section.
  if (edition.sections && edition.usage === "pratiquer" && !$("partition").hidden) montrerPartition();
}

function dessinerSections() {
  const bande = $("sections-bande");
  const pistes = $("sections-pistes");
  pistes.replaceChildren();
  const S = edition.sections;
  const d = S?.[S.length - 1]?.fin_s;
  bande.hidden = !S || !d;
  if (bande.hidden) return;
  for (const x of S) {
    const b = document.createElement("button");
    b.type = "button";
    const nom = NOMS_SECTIONS[x.etiquette] ?? x.etiquette;
    b.className = nom ? "section" : "section section--vide";
    b.style.left = `${(x.debut_s / d) * 100}%`;
    b.style.width = `${((x.fin_s - x.debut_s) / d) * 100}%`;
    b.textContent = nom;
    b.title = `${nom || "silence"} · ${horloge(x.debut_s * 1000)}–${horloge(x.fin_s * 1000)}\nClic : y aller · ⌥-clic : la boucler`;
    b.addEventListener("click", (e) =>
      (e.altKey ? bouclerSection(x) : deplacerLecture(x.debut_s / d)).catch((err) => remonter(err, "sections")),
    );
    pistes.append(b);
  }
  marquerSectionCourante();
}

/// Souligne la section sous la tête de lecture.
function marquerSectionCourante() {
  const S = edition.sections;
  if (!S || $("sections-bande").hidden) return;
  const t = edition.tete * S[S.length - 1].fin_s;
  const boutons = $("sections-pistes").children;
  S.forEach((x, i) => boutons[i]?.classList.toggle("section--courante", t >= x.debut_s && t < x.fin_s));
}

/// Boucle une section entière, bornes posées sur ses frontières ; un second
/// ⌥-clic sur la même section ôte la boucle.
async function bouclerSection(x) {
  const meme = edition.boucle && Math.abs(edition.boucle.debut - x.debut_s) < 1e-3 && Math.abs(edition.boucle.fin - x.fin_s) < 1e-3;
  edition.boucle = meme ? null : { debut: x.debut_s, fin: x.fin_s, mesures: null };
  if (!meme) await deplacerLecture(x.debut_s / edition.sections[edition.sections.length - 1].fin_s);
  await reposerBoucle();
  majBoucleUI();
}

/// Les bornes de `n` mesures à partir de celle qui contient `t`, en secondes.
/// Au-delà du dernier premier temps, la mesure se prolonge de la longueur de
/// la précédente.
function bornesMesures(t, n) {
  const p = edition.pulsation?.premiers_temps;
  if (!p || p.length < 2) return null;
  let i = 0;
  while (i + 1 < p.length && p[i + 1] <= t) i++;
  const debut = p[i];
  const derniere = p[p.length - 1] - p[p.length - 2];
  const j = i + n;
  const fin = j < p.length ? p[j] : p[p.length - 1] + (j - (p.length - 1)) * derniere;
  return { debut, fin: Math.min(fin, edition.dureeMs / 1000 || fin) };
}

/// Pose (ou repose) la boucle côté moteur. Le multipiste est recréé à chaque
/// rechargement des stems (transposition, greffe) : la boucle doit le suivre.
async function reposerBoucle() {
  if (!edition.enLecture) return;
  if (edition.boucle) {
    await invoke("stems_boucle", { debut: edition.boucle.debut, fin: edition.boucle.fin });
  } else {
    await invoke("stems_transport", { action: "sans_boucle", position: null });
  }
  edition.passagesVus = 0;
}

/// Boucle `edition.mesuresBoucle` mesures autour de la position courante, ou
/// ôte la boucle. Ancrée sur la mesure qui contient la tête de lecture : on
/// boucle ce qu'on vient d'entendre.
async function basculerBoucle() {
  if (edition.boucle) {
    edition.boucle = null;
  } else {
    const t = (edition.tete * edition.dureeMs) / 1000;
    const b = bornesMesures(t, edition.mesuresBoucle);
    if (!b) return;
    edition.boucle = { ...b, mesures: edition.mesuresBoucle };
  }
  await reposerBoucle();
  majBoucleUI();
}

/// Change le nombre de mesures d'une boucle posée, sans déplacer son début.
async function allongerBoucle(d) {
  edition.mesuresBoucle = Math.max(1, Math.min(32, edition.mesuresBoucle + d));
  if (edition.boucle) {
    const b = bornesMesures(edition.boucle.debut + 1e-3, edition.mesuresBoucle);
    if (b) edition.boucle = { ...b, mesures: edition.mesuresBoucle };
    await reposerBoucle();
  }
  majBoucleUI();
}

/// Un passage de plus : un cran de vitesse, jusqu'au tempo d'origine.
async function accelererAuPassage(passages) {
  if (!edition.accelBoucle || passages <= edition.passagesVus) {
    edition.passagesVus = passages;
    return;
  }
  const crans = passages - edition.passagesVus;
  edition.passagesVus = passages;
  if (edition.vitesse >= ACCEL_CIBLE) return;
  edition.vitesse = Math.min(ACCEL_CIBLE, edition.vitesse + crans * ACCEL_PAS);
  await appliquerVitesses();
  dessinerReglages();
}

function majBoucleUI() {
  const r = $("boucle-reglage");
  r.hidden = !edition.pulsation;
  const active = !!edition.boucle;
  $("boucle").setAttribute("aria-pressed", String(active));
  $("boucle").classList.toggle("reglage__unite--actif", active);
  $("boucle").textContent = active ? "boucle" : "boucler";
  $("boucle-detail").hidden = !active;
  $("boucle-mesures").textContent = `${edition.mesuresBoucle} mes.`;
  $("boucle-accel").setAttribute("aria-pressed", String(edition.accelBoucle));
  $("boucle-accel").classList.toggle("reglage__unite--actif", edition.accelBoucle);
  positionnerBande();
}

/// La bande de la boucle, en travers de la pile — calée comme le playhead.
function positionnerBande() {
  const bande = $("boucle-bande");
  const premier = edition.stems.find((s) => s.canvas && s.canvas.isConnected);
  const etabli = $("editer-etabli");
  if (!edition.boucle || !premier || etabli.hidden || !edition.dureeMs || edition.usage === "pratiquer") {
    bande.hidden = true;
    return;
  }
  const rc = premier.canvas.getBoundingClientRect();
  const re = etabli.getBoundingClientRect();
  const rp = $("dock-pistes").getBoundingClientRect();
  const d = edition.dureeMs / 1000;
  const x0 = rc.left - re.left + etabli.scrollLeft + (edition.boucle.debut / d) * rc.width;
  const x1 = rc.left - re.left + etabli.scrollLeft + (edition.boucle.fin / d) * rc.width;
  bande.style.left = `${Math.round(x0)}px`;
  bande.style.width = `${Math.max(1, Math.round(x1 - x0))}px`;
  bande.style.top = `${Math.round(rp.top - re.top + etabli.scrollTop)}px`;
  bande.style.height = `${$("dock-pistes").offsetHeight}px`;
  bande.hidden = false;
}

$("boucle").addEventListener("click", () => basculerBoucle().catch((e) => remonter(e, "boucle")));
for (const b of document.querySelectorAll("[data-boucle]")) {
  b.addEventListener("click", () => allongerBoucle(Number(b.dataset.boucle)).catch((e) => remonter(e, "boucle")));
}
$("boucle-accel").addEventListener("click", () => {
  edition.accelBoucle = !edition.accelBoucle;
  majBoucleUI();
});

export {
  accelererAuPassage, bpmSource, edition, majPulsation, majSections, majTempoSource,
  marquerSectionCourante, NOMS_SECTIONS, octaveVitesse, pasReglage, positionnerBande,
  reposerBoucle, vitesseEnBpm,
};
