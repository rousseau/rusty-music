import "./bibliotheque.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, enStems, fileAttente, horloge, invoke, remonter, signalerErreur, txt, vue,
  vueEnGrille,
} from "./socle.js";
import {
  majUsage, montrerPartition, oublierPartitions,
} from "./pratiquer.js";
import {
  lecture, poserStemsCharges, transport,
} from "./etat-lecture.js";
import { dessiner, dessinerGrille } from "./vues.js";
import { pisteSelectionnee, texteCompte } from "./ecouter.js";
import {
  battement, chargerSpectreTransport, peindreTransport, poserImage, poserLecture, sonder,
  teteCourante,
} from "./lecteur.js";
import { rampe } from "./carte.js";
import { indexAlphaUtile, indexAnneesUtile } from "./ecouter-familles.js";
import { demander } from "./ecrans.js";
import { zoneGreffe } from "./creer.js";

/* ---------------------------------------------------------- l'établi */

// Ce que Pratiquer et Créer partagent (`docs/ui-spec-editeur.md`) : l'état
// `edition`, le parcours choisir → séparer → établi, les variantes de
// démixage, les stems et leur pile, vitesse et hauteur, sections et boucles,
// le playhead, les spectres et les niveaux. Le propre de chaque mode est dans
// `pratiquer.js` (partition, décompte, partie, transcription) et `creer.js`
// (greffe).

// Le morceau que Pratiquer ou Créer travaille. Ce n'est pas forcément celui en
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
  // `true` : « changer » a redéplié le bloc Démixage dans l'établi, qui n'en
  // montre sinon qu'une ligne (`majResumeDemix`).
  demixDeplie: false,
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
  // « creer » (la pile de stems) ou « pratiquer » (la partition au centre) :
  // le mode du rail, posé par `basculerMode` (`docs/ui-spec-editeur.md`,
  // décision 13).
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
  return { debut, fin: Math.min(fin, lecture.stems.dureeMs / 1000 || fin) };
}

/// Pose (ou repose) la boucle côté moteur. Le multipiste est recréé à chaque
/// rechargement des stems (transposition, greffe) : la boucle doit le suivre.
async function reposerBoucle() {
  if (!lecture.stems.charges) return;
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
    const t = (edition.tete * lecture.stems.dureeMs) / 1000;
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
  if (
    !edition.boucle ||
    !premier ||
    etabli.hidden ||
    !lecture.stems.dureeMs ||
    edition.usage === "pratiquer"
  ) {
    bande.hidden = true;
    return;
  }
  const rc = premier.canvas.getBoundingClientRect();
  const re = etabli.getBoundingClientRect();
  const rp = $("dock-pistes").getBoundingClientRect();
  const d = lecture.stems.dureeMs / 1000;
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

// Le nom d'un stem à l'écran. Le moteur nomme ses stems en anglais (`drums`,
// `bass`…), et ce nom reste la clé partout (`edition.stems`, `data-stem`,
// fichiers, commandes) ; le rail dit « Basse » et « Batterie » : la pile aussi.
const LIBELLES_STEMS = {
  drums: "Batterie",
  bass: "Basse",
  vocals: "Voix",
  other: "Autres",
  guitar: "Guitare",
  piano: "Piano",
};
function libelleStem(nom) {
  return LIBELLES_STEMS[nom] ?? nom;
}

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
    // En pour cent par défaut ; en BPM quand le morceau pulse franchement et
    // que l'unité a été basculée — `edition.vitesse` reste le rapport, l'unité
    // n'est qu'une vue (`docs/ui-spec-editeur.md`, décision 9).
    // Le nombre seul : l'unité est dite à côté, par le bouton d'unité
    // (barre) ou un suffixe (ligne d'un stem) — demande du 29 oct.
    ecrire: (v) => `${Math.round(v * (vitesseEnBpm() ? bpmSource() : 100))}`,
    unite: () => (vitesseEnBpm() ? "BPM" : "%"),
    // **Ni pour cent ni BPM ambigus.** « 2 » en pour cent est refusé par la
    // borne basse plutôt qu'interprété comme « deux fois » ; en BPM, le champ
    // affiche « BPM » et lit donc un tempo, converti en rapport.
    lire: (t) => {
      const n = Number(
        String(t)
          .replace(/%|BPM/gi, "")
          .replace(",", ".")
          .trim(),
      );
      if (!Number.isFinite(n)) return null;
      if (vitesseEnBpm()) return bpmSource() ? n / bpmSource() : null;
      return n / 100;
    },
    immediat: true,
  },
  tonalite: {
    pas: 1,
    min: -12,
    max: 12,
    // La valeur reste en demi-tons (le pas des boutons, ce que le moteur
    // transpose) ; elle s'écrit **en tons**, comme on parle d'un accordage :
    // « −½ ton » pour jouer un morceau en mi♭ sans désaccorder la basse.
    ecrire: (v) => {
      if (v === 0) return "±0";
      const a = Math.abs(v);
      const tons = `${Math.floor(a / 2) || ""}${a % 2 ? "½" : ""}`;
      return `${v > 0 ? "+" : "−"}${tons} ton${a >= 4 ? "s" : ""}`;
    },
    // Se relit dans les formes que le champ écrit (« ±0 », « +1½ ton »,
    // « −½ ») et en décimal (« 1,5 ») — des tons, convertis en demi-tons.
    lire: (t) => {
      const brut = String(t)
        .replace(/tons?/gi, "")
        .replace("±", "")
        .replace("−", "-")
        .replace(",", ".")
        .trim();
      const n = Number(brut.replace(/(\d*)½/, (_, e) => `${e || 0}.5`));
      return Number.isFinite(n) ? Math.round(n * 2) : null;
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
      poser(calerReglage(nom, lire() + pasReglage(nom) * (e.key === "ArrowUp" ? 1 : -1)));
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
  // En mode BPM, on cale sur un tempo entier plutôt que sur un pas de rapport :
  // le champ montre « 128 BPM », pas « 128,3 ».
  if (nom === "vitesse" && vitesseEnBpm()) {
    return Math.round(borne * bpmSource()) / bpmSource();
  }
  const pas = pasReglage(nom);
  return Math.round(borne / pas) * pas;
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
  if (!lecture.stems.charges) return;
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
  return enSerie(async () => {
    if (!edition.stems.length) return;
    const boutons = document.querySelectorAll(".reglage button");
    boutons.forEach((b) => (b.disabled = true));
    try {
      // Dans le `try` avec le reste : un échec ici laissait auparavant les
      // boutons désactivés pour de bon, faute de passer par le `finally`
      // ci-dessous.
      $("dock-aide").textContent = "calcul…";
      const cibles = edition.stems;
      const traites = await transposerStems(
        edition.stems.map((s) => [s.nom, s.chemin]),
        edition.stems.map(tonaliteDe),
      );
      // Le calcul dure des dizaines de secondes : on a pu changer de morceau
      // ou de mode entre-temps. Charger alors ces stems-là remplacerait ceux
      // de l'écran par ceux d'un autre morceau.
      if (edition.stems !== cibles || !enStems()) {
        $("dock-aide").textContent = "";
        return;
      }
      // **L'état de lecture se relève après le calcul, pas avant** : la
      // lecture a continué (ou été mise en pause) pendant qu'il tournait, et
      // rendre la position d'avant le calcul faisait revenir en arrière d'autant.
      const avant = lecture.stems.charges ? await invoke("stems_state") : null;
      if (avant && !avant.en_pause) transport.ignorerEtatJusqua = Date.now() + 2000;
      // Le nouveau jeu naît en pause, à ses niveaux : ni la batterie coupée ni
      // le mélange depuis zéro ne s'entendent pendant qu'on reposera position
      // et vitesses.
      const noms = await invoke("stems_play", { stems: traites, niveaux: niveaux() });
      poserStemsCharges(true);
      verifierNoms(noms);
      await appliquerVitesses();
      await reposerBoucle();
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
      }
      if (!avant?.en_pause) await invoke("stems_transport", { action: "reprendre", position: null });
      $("dock-aide").textContent = "clic sur un spectrogramme : se déplacer";
    } catch (e) {
      $("dock-aide").textContent = "";
      remonter(e, "vitesse et hauteur");
    } finally {
      boutons.forEach((b) => (b.disabled = false));
    }
    sonder(true);
  });
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

  // Bascule d'unité : toujours là, puisque la valeur s'écrit sans unité ;
  // inactive (« % ») tant que le morceau ne pulse pas franchement. Elle porte
  // l'unité courante — cliquer passe à l'autre.
  const unite = $("vitesse-unite");
  unite.hidden = false;
  unite.disabled = !edition.tempoSource;
  unite.textContent = REGLAGES.vitesse.unite();
  unite.classList.toggle("reglage__unite--actif", vitesseEnBpm());

  // Repliement d'octave : « ½ » si viser ce BPM étire de plus d'une demi-octave
  // vers le haut, « ×2 » vers le bas, rien sinon.
  const oct = $("vitesse-octave");
  const f = octaveVitesse();
  oct.hidden = f === 0;
  oct.textContent = f === 0.5 ? "½" : f === 2 ? "×2" : "";

  // Au-delà de ±20 % d'étirement d'ensemble, `wsola` s'entend : on le dit, dans
  // les deux unités — c'est le rapport qui compte, pas la façon de l'écrire.
  $("vitesse-alerte").hidden = Math.abs(edition.vitesse - 1) <= 0.2;
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

document.querySelectorAll(".dock__tete .reglage button[data-r]").forEach((b) => {
  b.addEventListener("click", async () => {
    const nom = b.dataset.r;
    edition[nom] = calerReglage(nom, edition[nom] + pasReglage(nom) * Number(b.dataset.d));
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

/// Bascule le réglage « vitesse » entre pour cent et BPM. Ne touche pas à
/// `edition.vitesse` : c'est le même rapport, lu autrement. Caler ensuite fait
/// tomber l'affichage sur un tempo entier.
$("vitesse-unite").addEventListener("click", () => {
  if (!edition.tempoSource) return;
  edition.vitesseEnBpm = !edition.vitesseEnBpm;
  try {
    localStorage.setItem("vitesse-unite", edition.vitesseEnBpm ? "bpm" : "%");
  } catch {}
  if (edition.vitesseEnBpm) edition.vitesse = calerReglage("vitesse", edition.vitesse);
  dessinerReglages();
});

/// Replie le rapport de tempo à l'octave : le morceau se lit à demi- ou
/// double-tempo, et viser le BPM ne demande plus qu'un étirement léger.
$("vitesse-octave").addEventListener("click", async () => {
  const f = octaveVitesse();
  if (!f) return;
  edition.vitesse = calerReglage("vitesse", edition.vitesse * f);
  dessinerReglages();
  await appliquerVitesses();
  dessinerStems();
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
  el.hidden = !ecarte || !lecture.stems.charges;
  if (el.hidden) return;
  const ms = lecture.stems.deriveMs || 0;
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

/// Le morceau sur lequel travailler : le morceau sélectionné
/// (`pisteSelectionnee`), sinon celui de l'inspecteur ou celui en lecture.
function morceauAEditer() {
  // La sélection d'abord, sans passer par la file de lecture : un morceau
  // simplement choisi (recherche, carte) n'y est pas, et il faudrait le lire
  // pour pouvoir le séparer. À défaut de sélection, le morceau en lecture.
  if (pisteSelectionnee) return pisteSelectionnee;
  const path = $("insp-titre").dataset.path || lecture.courant;
  return fileAttente.pistes.find((t) => t.path === path) ?? null;
}

async function poserSourceEdition() {
  const t = morceauAEditer();
  const change = (t?.path ?? null) !== (edition.source?.path ?? null);
  // Changer de morceau abandonne les stems du précédent — et coupe leur
  // lecture : sans ça, les anciens stems continuaient de sonner par-dessus le
  // nouveau morceau.
  if (change && lecture.stems.charges) await arreterStems();
  edition.source = t;
  $("demix-source").textContent = t
    ? `${txt(t.artist, "?")} — ${txt(t.title, "?")}`
    : "Choisis un morceau dans la liste ou sur la carte.";
  $("lancer-demix").disabled = !t;
  $("separer-titre").textContent = t
    ? `« ${txt(t.title, "?")} » — choisis une variante et lance « Séparer » dans le panneau de gauche.`
    : "";
  // Revenir sur le même morceau (simple ré-entrée dans le mode) garde
  // l'établi et ses réglages en place.
  if (change) {
    edition.pulsation = null;
    edition.boucle = null;
    edition.stems = [];
    edition.stemSel = null;
    // Le tempo est propre au morceau : on l'oublie tant que le nouveau n'est
    // pas mesuré, plutôt que d'afficher un instant le BPM du précédent.
    edition.tempoSource = null;
    dessinerReglages();
  }
  if (t && (change || !edition.stems.length)) chargerStemsExistants(t);
  majEtatEditer();
}

/// Le centre de Pratiquer et de Créer a trois états (`docs/ui-spec-editeur.md`, « Le parcours ») :
///  1. **choisir** — la grille/liste, comme en Écoute ;
///  2. **séparer** — la carte de séparation au centre (`#editer-separer`) ;
///  3. **retoucher** — la pile de stems (`#editer-etabli`) + la barre d'outils
///     (`#dock`), le détail d'un stem dans l'inspecteur (`#bloc-stem`).
/// `montrerChoix` force le retour à l'état 1 depuis l'établi sans décharger.
function majEtatEditer() {
  if (!enStems()) return;
  const aStems = edition.stems.length > 0;
  // Par défaut on choisit ; on ne passe à « séparer » que sur un choix
  // explicite (clic sur une piste), et à « retoucher » dès qu'il y a des stems.
  const etat = edition.montrerChoix
    ? 1
    : aStems
      ? 3
      : edition.sourceChoisie && edition.source
        ? 2
        : 1;
  const choisir = etat === 1;

  demander("editer-separer", etat === 2);
  demander("editer-etabli", etat === 3);
  if (etat !== 3) edition.demixDeplie = false;
  majResumeDemix(etat === 3);
  demander("dock", etat === 3);
  demander("bloc-stem", etat === 3 && !!edition.stemSel);
  // « Filtrer » estompe la grille ou la liste : il n'a plus rien à filtrer
  // une fois le morceau choisi (séparer, établi).
  demander("bloc-chercher", choisir);

  if (choisir) {
    // État 1 = le navigateur d'Écoute, tel quel : on montre grille ou liste
    // et on les redessine ici, une fois visibles.
    const enGrille = vueEnGrille();
    demander("liste", !enGrille);
    demander("grille", enGrille);
    if (enGrille) {
      ecoute.grilleDernierRang = -1;
      dessinerGrille();
    } else {
      dessiner();
    }
    demander("index-alpha", indexAlphaUtile());
    demander("index-annees", indexAnneesUtile());
    $("fil-titre").textContent = vue.titre;
    $("fil-compte").textContent = texteCompte(vue.quoi, vue.lignes);
    demander("retour", vue.retour !== null);
    $("retour").textContent = `← ${vue.retour ? vue.retour.titre : ""}`;
    // Raccourci vers la séparation du morceau en lecture — seulement s'il
    // diffère de la sélection (qui, elle, ouvre directement « séparer »).
    const jouee = fileAttente.pistes.find((t) => t.path === lecture.courant) ?? null;
    const cand = jouee && jouee.path !== pisteSelectionnee?.path ? jouee : null;
    demander("editer-candidat", !!cand);
    if (cand) $("editer-candidat").textContent = `Séparer « ${txt(cand.title, "?")} »`;
  } else {
    for (const id of ["liste", "grille", "index-alpha", "index-annees", "editer-candidat"]) demander(id, false);
    const t = edition.source;
    $("fil-titre").textContent =
      `${etat === 2 ? "Séparer" : "Établi"} — ${txt(t?.title, "?")}`;
    $("fil-compte").textContent = "";
    demander("retour", true);
    $("retour").textContent = "← Choisir un morceau";
  }

  majUsage();
  if (etat === 3) positionnerPlayhead();
}

/// Le playhead unique en travers de la pile — un seul trait qui relie les
/// têtes de lecture peintes sur chaque spectrogramme (`docs/ui-spec-editeur.md`,
/// décision 8). Silencieux tant que la pile n'est pas à l'écran.
function positionnerPlayhead() {
  const ph = $("playhead");
  const etabli = $("editer-etabli");
  const pistes = $("dock-pistes");
  const premier = edition.stems.find((s) => s.canvas && s.canvas.isConnected);
  if (
    !enStems() ||
    etabli.hidden ||
    !premier ||
    !pistes.children.length ||
    edition.usage === "pratiquer"
  ) {
    ph.hidden = true;
    return;
  }
  // Deltas de rectangles : sans ambiguïté de padding ou d'offsetParent.
  // `.etabli` (le bloc englobant) n'a pas de bordure, `re.left` = bord interne.
  const rc = premier.canvas.getBoundingClientRect();
  const re = etabli.getBoundingClientRect();
  const rp = pistes.getBoundingClientRect();
  ph.style.left = `${Math.round(rc.left - re.left + etabli.scrollLeft + edition.tete * rc.width)}px`;
  ph.style.top = `${Math.round(rp.top - re.top + etabli.scrollTop)}px`;
  ph.style.height = `${pistes.offsetHeight}px`;
  ph.hidden = false;
}

/// Le détail du stem sélectionné, dans l'inspecteur commun — plus de panneau
/// déplié sous la ligne (`interface-guidelines.md` Règle 1).
function majInspecteurStem() {
  const hote = $("insp-stem");
  const s = edition.stems.find((x) => x.nom === edition.stemSel);
  const montrer = !!s && enStems() && edition.stems.length > 0;
  demander("bloc-stem", montrer);
  if (!montrer) {
    hote.replaceChildren();
    return;
  }
  $("insp-stem-nom").textContent = libelleStem(s.nom);
  hote.replaceChildren(panneauStem(s));
}

/// Met à jour, sur la ligne d'un stem dans la pile, le résumé de ce qui
/// s'écarte de l'ensemble (« +3 · greffé »…) et l'accent — appelé quand
/// l'inspecteur change un réglage, sans redessiner toute la pile.
function majResumeLigne(s) {
  const etat = document.querySelector(`.piste[data-stem="${s.nom}"] .stem__etat`);
  if (etat) etat.textContent = resumeStem(s);
}

/// Les stems prennent la main sur la lecture, au même instant et dans le même
/// état.
///
/// **Sans cela, le dock montrait des stems inertes** : ils étaient affichés
/// mais pas chargés, si bien que solo et coupure n'agissaient sur rien et que
/// le bouton du bas commandait encore le morceau mêlé. Il fallait relancer la
/// lecture depuis Pratiquer ou Créer pour que quoi que ce soit réponde.
///
/// La règle est désormais simple : **si le dock montre des stems, ce sont eux
/// la source.** Le morceau mêlé se tait, les stems reprennent à sa position et
/// dans son état de lecture.
async function prendreLaMain() {
  return enSerie(async () => {
    if (!edition.stems.length || lecture.stems.charges) return;
    const avant = await invoke("playback_state");
    // Reprendre la position n'a de sens que si on écoutait bien ce morceau-là.
    const memeMorceau = avant.current && edition.source && avant.current === edition.source.path;

    // Charge les stems **en pause**, à leurs niveaux : rien ne sonne avant
    // qu'on le décide plus bas.
    if (!(await lireStems())) return;

    if (memeMorceau) {
      // On relit la position : `stems_play` vient de mettre le lecteur en pause,
      // donc elle est celle de l'instant du basculement. `avant` datait d'avant
      // le décodage des stems — plus d'une seconde de retard.
      const apres = await invoke("playback_state");
      if (apres.position_ms > 0) {
        await invoke("stems_transport", {
          action: "deplacer",
          position: apres.position_ms / 1000,
        });
      }
    }
    // On n'impose pas la lecture : si rien ne jouait, les stems attendent.
    if (!memeMorceau || avant.paused || avant.finished) {
      poserLecture(false);
    } else {
      await invoke("stems_transport", { action: "reprendre", position: null });
      poserLecture(true);
    }
  });
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
    voisins: null,
  };
}

async function chargerStemsExistants(t) {
  const trouves = await invoke("stems_existants", { path: t.path });
  if (trouves.length) {
    edition.stems = trouves.map(stemNeuf);
    dessinerStems();
    $("demix-etat").textContent = `${trouves.length} stems déjà calculés`;
    majTempoSource();
    majPulsation();
    majSections();
    if (enStems()) await prendreLaMain();
  } else {
    edition.stems = [];
    dessinerStems();
    $("demix-etat").textContent = "";
  }
}

function choisirVariante(nom) {
  edition.variante = nom;
  document
    .querySelectorAll("[data-variante]")
    .forEach((s) => s.classList.toggle("segment--actif", s.dataset.variante === nom));
  majTexteBouton();
}

/// Pour pratiquer, BS-RoFormer : batterie et voix mieux séparées, basse
/// transcrite plus juste (banc `experiments/roformer/README.md`). Un choix
/// fait à la main n'est jamais repris.
function varianteParDefaut() {
  if (!edition.varianteChoisie) choisirVariante(edition.usage === "pratiquer" ? "bsrofo_sw" : "htdemucs");
}

document.querySelectorAll("[data-variante]").forEach((b) =>
  b.addEventListener("click", () => {
    edition.varianteChoisie = true;
    choisirVariante(b.dataset.variante);
  }),
);

/// Les variantes dont les poids ne sont pas encore sur la machine — nom → Mo.
/// Choisir l'une d'elles suffit à la demander : le premier démixage télécharge
/// les poids, une fois pour toutes.
const variantesAAcquerir = new Map();

/// Annonce le téléchargement **avant** le clic sur « Séparer », comme le coût
/// de l'analyse l'est avant son bouton : « 4 stems affinés — plus lent · 336 Mo
/// à télécharger la première fois ».
async function majVariantes() {
  let dispo;
  try {
    dispo = await invoke("demix_variantes");
  } catch (e) {
    remonter(e, "variantes de démixage");
    return;
  }
  variantesAAcquerir.clear();
  for (const v of dispo) if (!v.presente) variantesAAcquerir.set(v.nom, v.megaoctets);
  document.querySelectorAll("[data-variante]").forEach((b) => {
    b.dataset.base ??= b.textContent;
    const mo = variantesAAcquerir.get(b.dataset.variante);
    b.textContent = mo ? `${b.dataset.base} · ${mo} Mo à télécharger` : b.dataset.base;
  });
  majTexteBouton();
}

/// Le bouton dit ce qu'il va faire : télécharger d'abord, si les poids
/// manquent.
function majTexteBouton() {
  const mo = variantesAAcquerir.get(edition.variante);
  $("lancer-demix").textContent = mo ? "Télécharger et séparer" : "Séparer";
  // Les poids de SW n'ont ni auteur connu ni licence : on le dit au moment de
  // les télécharger, au centre de l'état « séparer » (et dans « À propos »).
  $("demix-provenance").hidden = !(mo && edition.variante === "bsrofo_sw");
  majResumeDemix();
}

/// L'établi ouvert, le Démixage tient sur une ligne : la variante, et
/// « changer » qui redéplie le bloc (Règle 13). Refermé à chaque entrée dans
/// l'établi.
function majResumeDemix(etabli = !$("editer-etabli").hidden) {
  const resume = etabli && !edition.demixDeplie;
  $("demix-resume").hidden = !resume;
  $("demix-detail").hidden = resume;
  const actif = document.querySelector("#variantes .segment--actif");
  $("demix-resume-variante").textContent = actif ? (actif.dataset.base ?? actif.textContent).split(" — ")[0].trim() : "";
}
$("demix-changer").addEventListener("click", () => {
  edition.demixDeplie = true;
  majResumeDemix();
});

/// Barre + texte pendant une séparation.
///
/// Tant que le moteur décode le morceau et chauffe le modèle, aucun segment
/// n'est connu : la barre reste indéterminée et le texte garde l'ordre de
/// grandeur. Ensuite elle se gradue segment par segment ; la variante affinée
/// nomme en plus le stem en cours, ses quatre réseaux passant l'un après
/// l'autre.
function majAvancementDemix(d) {
  const jauge = $("demix-jauge");
  // Poids absents : d'abord le téléchargement, avec sa barre — la taille est
  // connue d'avance, donc graduée d'emblée — puis, sans rien à refaire, la
  // séparation.
  if (d.phase === "telechargement") {
    const total = d.octets_total || 1;
    jauge.max = total;
    jauge.value = d.octets_faits;
    const pct = Math.min(100, Math.round((100 * d.octets_faits) / total));
    $("demix-etat").textContent =
      `téléchargement du modèle : ${Math.round(d.octets_faits / 1e6)} / ${Math.round(total / 1e6)} Mo (${pct} %) — une seule fois, puis la séparation`;
    return;
  }
  if (!d.segments_total) {
    jauge.removeAttribute("value");
    $("demix-etat").textContent = "séparation en cours… (compter ~30 s par morceau)";
    return;
  }
  jauge.max = d.segments_total;
  jauge.value = d.segments_faits;
  const pct = Math.round((100 * d.segments_faits) / d.segments_total);
  const stem = d.stem ? ` — ${d.stem} en cours` : "";
  $("demix-etat").textContent = `séparation : ${pct} %${stem}`;
}

let sondageDemix = null;
$("lancer-demix").addEventListener("click", async () => {
  const t = edition.source;
  if (!t) return;
  // Le morceau **séparé** : la sélection est libre (aucune lecture à lancer
  // pour la changer), elle peut donc bouger pendant les ~30 s de calcul.
  const cible = t.path;
  try {
    await invoke("start_demix", { path: t.path, variant: edition.variante });
  } catch (e) {
    $("demix-etat").textContent = String(e);
    return;
  }
  $("lancer-demix").disabled = true;
  const aTelecharger = variantesAAcquerir.get(edition.variante);
  // Lancée depuis le rail, quel que soit l'état du centre : on y montre le
  // morceau en cours de séparation plutôt que le sélecteur.
  edition.montrerChoix = false;
  edition.sourceChoisie = true;
  majEtatEditer();
  $("demix-etat").textContent = aTelecharger
    ? `téléchargement du modèle (${aTelecharger} Mo)…`
    : "séparation en cours… (compter ~30 s par morceau)";
  $("demix-jauge").hidden = false;
  $("demix-jauge").removeAttribute("value"); // indéterminée le temps du décodage et de la chauffe

  clearInterval(sondageDemix);
  sondageDemix = setInterval(async () => {
    const d = await invoke("demix_state");
    if (d.en_cours) {
      majAvancementDemix(d);
      return;
    }
    clearInterval(sondageDemix);
    sondageDemix = null;
    $("demix-jauge").hidden = true;
    $("demix-jauge").value = 0;
    $("lancer-demix").disabled = false;
    $("demix-etat").textContent = d.resultat ?? "";
    // Les poids sont peut-être arrivés : la variante n'est plus « à télécharger ».
    majVariantes();
    // Une autre source a été choisie entre-temps : ces stems sont ceux du
    // précédent, on ne les pose pas sur le nouveau. Ils restent calculés —
    // `stems_existants` les retrouvera si on y revient.
    if (edition.source?.path !== cible) return;
    // Le moteur a retiré les partitions calculées sur les anciens stems.
    oublierPartitions(edition.source.id);
    if (lecture.stems.charges) await arreterStems();
    edition.stems = d.stems.map(stemNeuf);
    edition.solo = null;
    edition.muets.clear();
    dessinerStems();
    majTempoSource();
    majPulsation();
    majSections();
    // Même règle qu'au chargement d'un démixage existant : afficher des stems,
    // c'est en faire la source. Sans cela, solo, coupure, vitesse et hauteur
    // n'agissaient sur rien tant qu'on n'avait pas relancé la lecture.
    await prendreLaMain();
  }, 800); // assez serré pour que la barre avance visiblement segment par segment
});

function dessinerStems() {
  const hote = $("dock-pistes");
  hote.replaceChildren();
  $("dock-source").textContent = edition.source ? txt(edition.source.title, "?") : "";
  if (!lecture.stems.charges) {
    $("dock-aide").textContent = edition.stems.length ? "▶ en bas pour écouter les stems" : "";
  }
  majDerive();

  // Ranger le centre **avant** de peindre : `#editer-etabli` doit être visible
  // pour que les canevas aient leur largeur réelle — un spectrogramme calculé à
  // largeur nulle est ensuite étiré et flou.
  majEtatEditer();

  for (const s of edition.stems) {
    // La ligne du stem. Ce qui ne concerne que lui — vitesse, hauteur,
    // remplacement — n'est plus déplié dessous : il va dans l'inspecteur
    // (`majInspecteurStem`), un seul inspecteur (`interface-guidelines.md`).
    const piste = document.createElement("div");
    piste.className = "piste";
    piste.dataset.stem = s.nom;

    const el = document.createElement("div");
    el.className = "stem";
    el.classList.toggle("stem--sel", edition.stemSel === s.nom);
    const muet = edition.muets.has(s.nom);
    const solo = edition.solo === s.nom;
    // Le solo l'emporte : c'est la convention de toutes les tables de mixage.
    const audible = solo || (edition.solo === null && !muet);
    el.classList.toggle("stem--muet", !audible);
    el.classList.toggle("stem--solo", solo);

    // Colonne de commande étroite à gauche (façon en-tête de piste de
    // séquenceur), le spectrogramme prend tout le reste de la largeur.
    // Cliquer le nom sélectionne le stem : son détail (vitesse, hauteur,
    // remplacement) s'ouvre dans l'inspecteur — pas de bouton dédié.
    el.innerHTML = `<div class="stem__ctrl">
                      <button class="stem__tete" data-a="sel"
                              title="Régler ce stem — vitesse, hauteur, remplacement">
                        <span class="stem__nom"></span>
                        <span class="stem__etat"></span>
                      </button>
                      <span class="stem__actions">
                        <button class="stem__b" data-a="solo" title="Solo">S</button>
                        <button class="stem__b" data-a="muet" title="Muet">M</button>
                      </span>
                      <span class="stem__jauge" title="Niveau — tirer pour régler"><i></i></span>
                    </div>
                    <canvas class="stem__spectre"></canvas>`;
    el.querySelector(".stem__nom").textContent = libelleStem(s.nom);
    el.querySelector(".stem__etat").textContent = resumeStem(s);
    el.querySelector('[data-a="solo"]').classList.toggle("stem__b--actif", solo);
    el.querySelector('[data-a="muet"]').classList.toggle("stem__b--actif", muet);
    el.querySelector(".stem__jauge i").style.width = `${(audible ? s.niveau : 0) * 100}%`;

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
    el.querySelector('[data-a="sel"]').addEventListener("click", () => {
      edition.stemSel = edition.stemSel === s.nom ? null : s.nom;
      // En Pratiquer, désigner la basse ou la batterie dans la pile choisit
      // aussi la partie lue — le geste qu'on fait d'instinct (essai du 15 oct.).
      if (edition.usage === "pratiquer" && (s.nom === "bass" || s.nom === "drums") && edition.instrument !== s.nom) {
        edition.instrument = s.nom;
        majUsage();
      }
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
    hote.appendChild(piste);
    dessinerSpectre(cnv, s);
  }

  majInspecteurStem();
  majEtatEditer();
}

/// Résumé de ce qui écarte un stem de l'ensemble, pour sa ligne dans la pile —
/// chaîne vide quand il suit tout (la ligne n'affiche alors rien).
function resumeStem(s) {
  const bouts = [];
  if (s.vitesse !== null) bouts.push(`${REGLAGES.vitesse.ecrire(s.vitesse)} ${REGLAGES.vitesse.unite()}`);
  if (s.tonalite !== null) bouts.push(REGLAGES.tonalite.ecrire(s.tonalite));
  if (s.greffe) bouts.push("greffé");
  return bouts.join(" · ");
}

/// Le panneau d'un stem : sa vitesse, sa hauteur, et d'où le remplacer.
function panneauStem(s) {
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
             aria-label="Vitesse de ce stem">
      <button data-r="vitesse" data-d="1" aria-label="Accélérer ce stem">+</button>
      <span class="reglage__suffixe" data-u="vitesse"></span>
    </div>
    <div class="reglage" role="group" aria-label="Hauteur de ce stem"
         title="Transposition de ce stem seul, à durée inchangée. Quelques secondes de calcul, mises en cache.">
      <b>hauteur</b>
      <button data-r="tonalite" data-d="-1" aria-label="Baisser ce stem d'un demi-ton">−</button>
      <input class="reglage__val reglage__val--tons" data-c="tonalite" inputmode="decimal"
             aria-label="Transposition de ce stem, en tons">
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
    ligne.querySelector('[data-u="vitesse"]').textContent = REGLAGES.vitesse.unite();
    ensemble.disabled = s.vitesse === null && s.tonalite === null;
    // Refléter le changement sur la ligne du stem dans la pile, sans la
    // redessiner entièrement (on garde le focus des champs en cours de saisie).
    majResumeLigne(s);
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

  // Barre graduée stem par stem, comme celle de la séparation : à une vingtaine
  // de secondes par stem, une barre qui bouge dit « ça avance » là où un texte
  // figé laisse craindre un blocage.
  const jauge = $("transpose-jauge");
  const finJauge = () => {
    jauge.hidden = true;
    jauge.value = 0;
  };
  jauge.hidden = false;
  jauge.max = depart.total;
  jauge.value = 0;

  return new Promise((resolve, reject) => {
    const t = setInterval(async () => {
      let e;
      try {
        e = await invoke("etirer_state");
      } catch (err) {
        clearInterval(t);
        finJauge();
        reject(err);
        return;
      }
      // Un compte de stems, pas un pourcentage : à vingt secondes pièce, un
      // pourcentage global reste immobile assez longtemps pour qu'on le croie
      // bloqué.
      if (e.en_cours) {
        jauge.max = e.total;
        jauge.value = e.faits;
        $("dock-aide").textContent = `transposition ${Math.min(e.faits + 1, e.total)}/${e.total}…`;
        return;
      }
      clearInterval(t);
      finJauge();
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
      if (d.en_cours) {
        if (d.segments_total) {
          const pct = Math.round((100 * d.segments_faits) / d.segments_total);
          $("dock-aide").textContent = `séparation du morceau voisin… ${pct} %`;
        }
        return;
      }
      clearInterval(t);
      if (d.stems.length) resolve(d);
      else reject(d.resultat ?? "la séparation du morceau voisin a échoué");
    }, 1000);
  });
}

/// Dessine le spectrogramme d'un stem, une fois, en mémoire.
///
/// Le calcul revient au moteur ; l'interface ne fait que colorer avec la rampe
/// séquentielle déjà retenue pour la carte. Une rampe n'oppose pas des
/// identités — c'est ce qui l'autorise là où trois teintes catégorielles
/// seraient déjà de trop.
/// Hauteur visible d'un spectrogramme de stem, en pixels CSS — doit suivre
/// `.stem__spectre { height }` dans `style.css`. Volontairement basse : la
/// pile doit rester lisible avec une dizaine de stems (variante 6 stems +
/// greffes).
const HAUT_STEM_SPECTRE = 64;

async function dessinerSpectre(cnv, stem) {
  // Rendu à la densité de l'écran : sur un écran Retina, un canevas calculé à
  // la taille CSS est étiré ×2 et devient flou. On calcule donc à
  // `devicePixelRatio` près et on laisse le CSS le réduire.
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const largeur = Math.max(240, Math.round(cnv.getBoundingClientRect().width * dpr));
  const hauteur = Math.round(HAUT_STEM_SPECTRE * dpr);

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
  // L'image est composée une fois (gardée côté CPU, voir `poserImage`) ; à
  // chaque battement on la repose et on trace la tête de lecture par-dessus.
  // Recalculer le spectrogramme cinq fois par seconde serait absurde.
  const img = new ImageData(w, h);
  const table = rampeRGB();
  for (let i = 0; i < pixels.length; i++) {
    const c = table[pixels[i]];
    img.data[i * 4] = c[0];
    img.data[i * 4 + 1] = c[1];
    img.data[i * 4 + 2] = c[2];
    img.data[i * 4 + 3] = 255;
  }

  cnv.width = w;
  cnv.height = h;
  stem.img = img;
  stem.canvas = cnv;
  peindreSpectre(stem);
  // Le canevas vient d'être dimensionné : le playhead unique peut se caler.
  positionnerPlayhead();
}

/// Repose le spectrogramme et trace la tête de lecture dessus.
///
/// Une seule tête pour toute la fenêtre : les spectrogrammes et la barre du
/// bas montrent le même instant, puisqu'ils montrent le même morceau.
function peindreSpectre(stem) {
  if (!stem.img || !stem.canvas || !stem.canvas.isConnected) return;
  const cnv = stem.canvas;
  const ctx = cnv.getContext("2d");
  poserImage(ctx, stem.img, cnv.width, cnv.height);

  const x = Math.round(edition.tete * cnv.width);
  // Ce qui est déjà passé s'assombrit : la position se lit même quand le
  // trait tombe sur une zone claire du spectre.
  ctx.fillStyle = "rgba(0, 0, 0, .38)";
  ctx.fillRect(0, 0, x, cnv.height);
  ctx.fillStyle =
    getComputedStyle(document.documentElement).getPropertyValue("--txt").trim() || "#EDE8DC";
  ctx.fillRect(x, 0, 1, cnv.height);
}

// Les canevas affichés peuvent eux aussi perdre leur contenu pendant que
// l'app est cachée ou la machine en veille : lecture en pause, rien ne les
// repeindrait. On les repose au retour.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) return;
  peindreTransport(teteCourante);
  for (const s of edition.stems) peindreSpectre(s);
});

/// Déplace la tête de lecture sur tous les spectrogrammes à la fois, et le
/// playhead unique qui les relie.
function poserTete(frac) {
  edition.tete = Math.min(1, Math.max(0, frac || 0));
  for (const s of edition.stems) peindreSpectre(s);
  positionnerPlayhead();
  positionnerBande();
  marquerSectionCourante();
}

/// Table de 256 couleurs interpolée sur `--rampe`, calculée une fois.
let tableRampe = null;
// La rampe change avec le thème : la table se refait au prochain spectrogramme.
document.addEventListener("theme", () => {
  tableRampe = null;
});
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

/// Envoie les niveaux au moteur. **Un envoi à la fois, toujours le dernier
/// état.**
///
/// Les commandes Tauri asynchrones s'exécutent en parallèle et rien
/// n'ordonne leur arrivée : en tirant un fader ou en enchaînant solo et
/// coupure, un ancien état pouvait arriver en dernier et l'emporter — la
/// batterie restait audible alors que l'écran la disait coupée. Pendant qu'un
/// envoi est en vol, les demandes suivantes se contentent de le noter ; la
/// boucle relit alors `niveaux()` et renvoie. L'interface est la référence :
/// c'est ce qu'elle affiche que le moteur doit jouer.
let niveauxEnVol = false;
let niveauxARenvoyer = false;
let dernierEchecNiveaux = "";
async function appliquerNiveaux() {
  if (!lecture.stems.charges) return;
  if (niveauxEnVol) {
    niveauxARenvoyer = true;
    return;
  }
  niveauxEnVol = true;
  try {
    do {
      niveauxARenvoyer = false;
      await invoke("stems_gain", { levels: niveaux() });
      dernierEchecNiveaux = "";
    } while (niveauxARenvoyer && lecture.stems.charges);
  } catch (e) {
    // Une fois par cause : le battement réessaie à 5 Hz.
    if (String(e) !== dernierEchecNiveaux) {
      dernierEchecNiveaux = String(e);
      signalerErreur("dock-aide", "niveaux des stems non appliqués", e, "niveaux des stems");
    }
  } finally {
    niveauxEnVol = false;
  }
}

/// Le moteur joue-t-il ce que l'écran affiche ? Compare à chaque battement les
/// stems chargés et leurs niveaux à ceux de l'interface, et corrige.
///
/// - **Niveaux** : un écart (envoi perdu, rechargement, quoi que ce soit) est
///   renvoyé — l'écran fait foi, jamais l'inverse.
/// - **Jeu de stems** : si le moteur tient d'autres stems que ceux affichés
///   (chargement lancé pour un morceau qu'on a quitté), on ne peut pas
///   « corriger » des niveaux qui ne parlent pas des mêmes pistes : on recharge.
function reconcilierStems(e) {
  if (!lecture.stems.charges) return;
  const noms = edition.stems.map((s) => s.nom);
  if (e.noms?.length && e.noms.join("|") !== noms.join("|")) {
    remonter(`moteur : ${e.noms.join(",")} · écran : ${noms.join(",")}`, "stems désaccordés");
    arreterStems().then(() => prendreLaMain());
    return;
  }
  const attendu = niveaux();
  const ecart =
    e.niveaux.length !== attendu.length ||
    e.niveaux.some((v, i) => Math.abs(v - attendu[i]) > 1e-3);
  if (ecart) appliquerNiveaux();
}

/// Déplace la lecture, que ce soient les stems ou le lecteur ordinaire.
///
/// Un seul chemin pour un seul geste : cliquer sur la barre du bas ou sur
/// n'importe quel spectrogramme revient au même, et la tête bouge partout.
async function deplacerLecture(frac) {
  frac = Math.min(1, Math.max(0, frac));
  if (lecture.stems.charges || (await rattacherStems())) {
    const e = await invoke("stems_state");
    await invoke("stems_transport", {
      action: "deplacer",
      position: (e.duree_ms / 1000) * frac,
    });
    poserTete(frac);
    await battement();
    return;
  }
  const t = fileAttente.pistes.find((x) => x.path === lecture.courant);
  if (!t?.duration_ms) return;
  await invoke("seek", { positionMs: Math.round(frac * t.duration_ms) });
  poserTete(frac);
}

/// En Pratiquer ou Créer, des stems affichés que plus rien ne pilote (coupés par le
/// moteur : ⏮, ⏭, touches média) : on les recharge, en pause. Sans ça, ▶ et
/// les clics sur un spectrogramme partaient au lecteur ordinaire — le morceau
/// d'origine jouait, solo et coupure ne touchaient plus rien et la partition
/// restait figée. Rend vrai si les stems sont de nouveau chargés.
async function rattacherStems() {
  if (!enStems() || !edition.stems.length || lecture.stems.charges) return false;
  await prendreLaMain();
  return lecture.stems.charges;
}

/// Charge les stems dans le moteur, **en pause**, à leurs niveaux et vitesses.
/// Ne les fait pas sonner : c'est à l'appelant, une fois la position posée.
/// Rend vrai si les stems sont chargés.
///
/// Le chargement décode tout en mémoire — 186 Mo pour quatre stems d'un
/// morceau de quatre minutes. C'est ce qui rend le solo instantané et le
/// déplacement gratuit.
///
/// Toujours appelée dans la file de `enSerie` : deux chargements simultanés
/// laissaient deux multipistes sonner un instant l'un sur l'autre, et le
/// second repartait à pleine échelle.
async function lireStems() {
  if (!edition.stems.length || lecture.stems.charges) return false;
  $("dock-aide").textContent = "chargement des stems…";
  const cibles = edition.stems;
  try {
    // Passe par le traitement même à réglages neutres : le moteur rend alors
    // les chemins d'origine sans rien calculer, et il n'y a qu'un chemin de
    // code à suivre pour charger des stems.
    const stems = await transposerStems(
      edition.stems.map((s) => [s.nom, s.chemin]),
      edition.stems.map(tonaliteDe),
    );
    // Pendant le décodage on a pu quitter le mode ou changer de morceau :
    // charger alors ces stems ferait jouer un autre morceau que celui affiché,
    // ou des stems en dehors de Pratiquer et de Créer.
    if (edition.stems !== cibles || !enStems()) {
      $("dock-aide").textContent = "";
      return false;
    }
    const noms = await invoke("stems_play", { stems, niveaux: niveaux() });
    // Posé avant les vitesses : `appliquerVitesses` ne parle au moteur que si
    // un multipiste est chargé, et il l'est à partir d'ici.
    poserStemsCharges(true);
    verifierNoms(noms);
    await appliquerVitesses();
    await reposerBoucle();
  } catch (e) {
    poserStemsCharges(false);
    // `stems_play` a pu réussir avant qu'une étape suivante échoue : on coupe
    // le multipiste côté moteur, sans quoi il resterait chargé sans que rien ne
    // le pilote (le sondage suit `lecture.stems.charges`).
    await invoke("stems_transport", { action: "arreter", position: null }).catch(() => {});
    $("dock-aide").textContent = "";
    $("demix-etat").textContent = String(e);
    return false;
  }
  await appliquerNiveaux();
  $("dock-aide").textContent = "clic sur un spectrogramme : se déplacer";
  // La barre du bas garde le spectrogramme du morceau édité. Il lui venait
  // jusqu'ici de la lecture ordinaire, écoutée avant d'entrer en Pratiquer ou Créer ; sans
  // lecture préalable (morceau seulement sélectionné), rien ne le chargeait.
  chargerSpectreTransport(edition.source);
  sonder(true);
  return true;
}

/// Le moteur a-t-il chargé les stems qu'on lui a demandés, dans l'ordre ?
/// Les niveaux se posent par rang : un ordre différent muterait la mauvaise
/// piste. Lève, ce qui fait échouer le chargement plutôt que de jouer faux.
function verifierNoms(noms) {
  const attendu = edition.stems.map((s) => s.nom);
  if (noms.join("|") !== attendu.join("|")) {
    throw new Error(`stems chargés (${noms.join(", ")}) ≠ stems affichés (${attendu.join(", ")})`);
  }
}

/// Une seule opération de chargement de stems à la fois, dans l'ordre
/// d'arrivée. Les gardes (`lecture.stems.charges`, stems affichés) sont relues
/// **à l'intérieur**, quand le tour vient : la suivante voit le résultat de la
/// précédente au lieu d'en refaire un doublon.
let chaineStems = Promise.resolve();
function enSerie(travail) {
  const suite = chaineStems.then(travail, travail);
  chaineStems = suite.catch(() => {});
  return suite;
}

/// L'interface cesse de suivre les stems, sans parler au moteur — qui les a
/// déjà coupés (commande de lecture ordinaire) ou va le faire.
function oublierStems() {
  const jouait = lecture.stems.charges;
  poserStemsCharges(false);
  $("dock-aide").textContent = "";
  if (jouait) poserTete(0);
}

/// Coupe la lecture des stems. Toujours sûre à appeler — même si l'interface
/// pense qu'ils ne jouent pas : l'ordre part quand même au moteur, ce qui
/// évite un multipiste orphelin qui sonnerait sans que rien ne le pilote.
async function arreterStems() {
  oublierStems();
  await invoke("stems_transport", { action: "arreter", position: null }).catch(() => {});
}

$("dock-fermer").addEventListener("click", async () => {
  if (lecture.stems.charges) await arreterStems();
  // Ferme l'établi : on repart de la carte de séparation, sans recalcul si on
  // rouvre (`stems_existants` les retrouve).
  edition.stems = [];
  edition.stemSel = null;
  dessinerStems();
});

export {
  accelererAuPassage, appliquerReglages, appliquerVitesses, arreterStems, attendreDemix,
  bpmSource, deplacerLecture, dessinerReglages, dessinerStems, edition, libelleStem, majDerive,
  majEtatEditer, majPulsation, majSections, majTempoSource, majVariantes,
  marquerSectionCourante, niveaux, NOMS_SECTIONS, octaveVitesse, oublierStems, pasReglage,
  poserSourceEdition, poserTete, positionnerBande, positionnerPlayhead, prendreLaMain,
  rampeRGB, rattacherStems, reconcilierStems, REGLAGES, reposerBoucle, resumeStem, stemEcarte,
  tonaliteDe, varianteParDefaut, vitesseDe, vitesseEnBpm,
};
