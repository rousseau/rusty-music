import "./corriger.js"; // l'ordre d'exécution : voir main.js
import { $, invoke, journal } from "./socle.js";
import { edition, majBoucleUI, reposerBoucle } from "./etabli.js";
import { appliquerVitesses, dessinerReglages } from "./etabli-reglages.js";
import { deplacerLecture } from "./etabli-stems.js";
import { lecture } from "./etat-lecture.js";
import { batterieAffichee, partition } from "./pratiquer.js";

/* ------------------------------------------- pratiquer : le coach de batterie */

// E4 (`docs/recherche-coach-pratique.md`) : jouer la batterie transcrite sur
// sa batterie électronique, et voir chaque note jugée sur la partition. Le
// moteur écoute la batterie (MIDI, `coach.rs`), situe chaque coup sur
// l'horloge où il joue les stems, et le compare à la transcription (ADTOF),
// en temps musical : sous un ralentissement, les fenêtres s'élargissent
// d'autant. Ici, seulement l'affichage : la barre au-dessus de la partition
// (`#partition-coach`, au centre : Règle 12) et, sur la partition, un cercle
// autour de chaque note jugée — juste, en avance ou en retard, manquée, sur
// la mauvaise pièce. Rien n'est dit d'une note dont la transcription n'est
// pas sûre (cymbales, notes fantômes).
//
// La calibration se fait sur le décompte : « calibrer », puis ▶ ; on tape
// la caisse claire sur les seize clics, le moteur prend la médiane des
// écarts (la latence de la batterie, de l'ordinateur et du casque) et la
// garde pour ce périphérique.
//
// Mesure après mesure, le moteur relève chaque passage (attrapées, manquées,
// en trop, écarts) et le cumule dans la table du morceau (`pratique_mesure`,
// score lissé d'une séance à l'autre). Il en tire deux propositions, que la
// barre offre sans jamais les appliquer d'elle-même : boucler les pires
// mesures à 70 %, et, en boucle, l'escalier de tempo (un cran de plus après
// deux passages propres, un de moins après un raté).

const coach = {
  actif: false,
  peripherique: null,
  latenceMs: null,
  // La transcription armée (sa révision), pour ne pas réarmer à chaque
  // redessin.
  armee: null,
  // Fin de la dernière fenêtre jugée, à l'horloge du moteur.
  depuis: null,
  minuteur: null,
  occupe: false,
  calibrage: false,
  // Pendant un décompte, on ne sonde pas : rien n'est attendu des clics.
  pauseJusqua: 0,
  // Le dernier jugement de chaque coup de la référence, par rang.
  marques: new Map(),
  // Les cercles posés sur la partition, réutilisés d'un dessin à l'autre.
  cercles: [],
  // Le décompte d'une pièce : son identifiant d'articulation dans alphaTab,
  // appris sur la partition (`articulations`).
  articulations: null,
  // Le passage en cours et ce qu'il a donné.
  passage: null,
  bilan: null,
  // Ce que le moteur propose : une boucle sur les pires mesures, une vitesse.
  boucleProposee: null,
  vitesseProposee: null,
  message: null,
  messageJusqua: 0,
};

const ARRETE = "jouer avec ma batterie";

const VERDICTS = {
  parfait: { classe: "juste", dit: "juste" },
  bon: { classe: "juste", dit: "juste" },
  avance: { classe: "avance", dit: "en avance" },
  retard: { classe: "retard", dit: "en retard" },
  manquee: { classe: "manquee", dit: "manquée" },
  mauvaise_piece: { classe: "faute", dit: "mauvaise pièce" },
};

const ATTRAPEES = new Set(["parfait", "bon", "avance", "retard"]);

function bilanVide() {
  return { attendues: 0, attrapees: 0, en_trop: 0, ecarts: [] };
}

/// La barre du coach : là seulement quand Pratiquer montre la batterie
/// transcrite. L'appelle `montrerPartition` (et `cacherPartition`).
function majCoach() {
  const tr = batterieAffichee();
  const barre = $("partition-coach");
  barre.hidden = !tr;
  if (!tr) {
    arreterSondage();
    effacerMarques();
    return;
  }
  if (coach.actif) armer(tr);
  dire();
}

function dire() {
  const etat = $("coach-etat");
  const bascule = $("coach-basculer");
  bascule.setAttribute("aria-pressed", String(coach.actif));
  bascule.textContent = coach.actif ? "arrêter" : ARRETE;
  $("coach-calibrer").hidden = !coach.actif || !coach.peripherique;
  $("coach-calibrer").setAttribute("aria-pressed", String(coach.calibrage));
  direPropositions();
  if (!coach.actif) {
    etat.textContent = "";
    return;
  }
  if (coach.message && performance.now() < coach.messageJusqua) {
    etat.textContent = coach.message;
    return;
  }
  if (!coach.peripherique) {
    etat.textContent = coach.message ?? "Aucune batterie MIDI branchée.";
    return;
  }
  if (coach.calibrage) {
    etat.textContent = "Calibrer : ▶, puis taper la caisse claire sur les seize clics du décompte.";
    return;
  }
  const latence = coach.latenceMs === null ? "latence non calibrée" : `latence ${Math.round(coach.latenceMs)} ms`;
  const b = coach.bilan;
  if (!b || !b.attendues) {
    etat.textContent = `${latence} · ▶ et jouer`;
    return;
  }
  const moyenne = b.ecarts.length ? b.ecarts.reduce((x, y) => x + y, 0) / b.ecarts.length : 0;
  const signe = moyenne > 0.5 ? "+" : moyenne < -0.5 ? "−" : "±";
  const passage = coach.passage > 0 ? `passage ${coach.passage + 1} : ` : "";
  etat.textContent =
    `${passage}${b.attrapees} / ${b.attendues} attrapées · écart moyen ${signe}${Math.abs(Math.round(moyenne))} ms` +
    (b.en_trop ? ` · ${b.en_trop} en trop` : "") +
    ` · ${latence}`;
}

function annoncer(message, ms = 6000) {
  coach.message = message;
  coach.messageJusqua = performance.now() + ms;
  dire();
}

/// Arme le moteur sur la transcription affichée (une fois par révision).
async function armer(tr) {
  const cle = `${edition.source?.id}:${tr.revision ?? 0}`;
  if (coach.armee === cle) return;
  coach.armee = cle;
  coach.articulations = null;
  effacerMarques();
  try {
    await invoke("coach_armer", {
      chemin: edition.source?.path ?? null,
      coups: tr.pret.coups ?? [],
      premiersTemps: edition.pulsation?.premiers_temps ?? [],
    });
  } catch (e) {
    coach.armee = null;
    journal("coach", `armer : ${e}`, "warn");
    return;
  }
  // Les passages des séances d'avant : la boucle à proposer dès l'ouverture.
  coach.vitesseProposee = null;
  coach.boucleProposee = null;
  if (!edition.source?.path) return dire();
  try {
    const t = await invoke("coach_table", { chemin: edition.source.path });
    coach.boucleProposee = t.boucle_proposee;
  } catch (e) {
    journal("coach", `table : ${e}`, "warn");
  }
  dire();
}

const memesBornes = (b, p) => !!b && Math.abs(b.debut - p.debut_s) < 1e-3 && Math.abs(b.fin - p.fin_s) < 1e-3;

/// Les deux propositions du moteur, chacune un bouton : rien ne s'applique
/// sans le geste.
function direPropositions() {
  const boucler = $("coach-boucler");
  const p = coach.boucleProposee;
  boucler.hidden = !coach.actif || !p || memesBornes(edition.boucle, p);
  if (!boucler.hidden) {
    const mesures = p.premiere === p.derniere ? `mes. ${p.premiere + 1}` : `mes. ${p.premiere + 1}–${p.derniere + 1}`;
    boucler.textContent = `boucler ${mesures} à ${Math.round(p.vitesse * 100)} %`;
  }
  const vitesse = $("coach-vitesse");
  const v = coach.vitesseProposee;
  vitesse.hidden = !coach.actif || !v || !edition.boucle || Math.abs(v.vitesse - edition.vitesse) < 1e-3;
  if (!vitesse.hidden) vitesse.textContent = `${v.monter ? "↑" : "↓"} ${Math.round(v.vitesse * 100)} %`;
}

/// Boucle les mesures proposées et repart à la vitesse de départ de
/// l'escalier (sans accélérer si l'on jouait déjà plus lentement).
async function bouclerProposee() {
  const p = coach.boucleProposee;
  if (!p) return;
  const n = p.derniere - p.premiere + 1;
  edition.mesuresBoucle = n;
  edition.boucle = { debut: p.debut_s, fin: p.fin_s, mesures: n };
  const duree = lecture.stems.dureeMs / 1000;
  if (duree > 0) await deplacerLecture(p.debut_s / duree);
  await reposerBoucle();
  majBoucleUI();
  coach.vitesseProposee = null;
  if (edition.vitesse > p.vitesse + 1e-3) await poserVitesse(p.vitesse);
  annoncer(`Boucle sur ${n > 1 ? `les mesures ${p.premiere + 1} à ${p.derniere + 1}` : `la mesure ${p.premiere + 1}`}, à ${Math.round(edition.vitesse * 100)} %.`);
}

async function poserVitesse(v) {
  edition.vitesse = v;
  await appliquerVitesses();
  dessinerReglages();
}

async function demarrer() {
  coach.actif = true;
  coach.message = null;
  dire();
  let liste = [];
  try {
    liste = await invoke("coach_peripheriques");
  } catch (e) {
    annoncer(`MIDI indisponible : ${e}`);
  }
  const choix = $("coach-peripherique");
  choix.replaceChildren(...liste.map((n) => new Option(n, n)));
  choix.hidden = liste.length < 2;
  let garde = null;
  try {
    garde = localStorage.getItem("coach-peripherique");
  } catch {}
  await ecouter(liste.includes(garde) ? garde : (liste[0] ?? null));
}

async function ecouter(nom) {
  coach.peripherique = null;
  coach.latenceMs = null;
  if (nom) {
    try {
      const vue = await invoke("coach_ecouter", { peripherique: nom });
      coach.peripherique = vue?.peripherique ?? null;
      coach.latenceMs = vue?.latence_ms ?? null;
      $("coach-peripherique").value = nom;
      try {
        localStorage.setItem("coach-peripherique", nom);
      } catch {}
    } catch (e) {
      annoncer(`Batterie injoignable : ${e}`);
    }
  }
  const tr = batterieAffichee();
  if (coach.actif && tr) {
    coach.armee = null;
    await armer(tr);
  }
  if (coach.peripherique) demarrerSondage();
  dire();
}

async function arreter() {
  coach.actif = false;
  coach.calibrage = false;
  coach.peripherique = null;
  arreterSondage();
  effacerMarques();
  dire();
  try {
    await invoke("coach_ecouter", { peripherique: null });
  } catch (e) {
    journal("coach", `fermer : ${e}`, "warn");
  }
}

function demarrerSondage() {
  clearInterval(coach.minuteur);
  coach.depuis = null;
  coach.minuteur = setInterval(sonder, 250);
}

function arreterSondage() {
  clearInterval(coach.minuteur);
  coach.minuteur = null;
}

/// Ce que le moteur a jugé depuis le dernier sondage.
async function sonder() {
  if (coach.occupe || !coach.actif || coach.calibrage || performance.now() < coach.pauseJusqua) return;
  coach.occupe = true;
  try {
    const v = await invoke("coach_juger", { depuisS: coach.depuis });
    coach.depuis = v.jusqua_s;
    recevoir(v);
  } catch (e) {
    journal("coach", `juger : ${e}`, "warn");
  } finally {
    coach.occupe = false;
  }
}

function recevoir(v) {
  if (v.boucle_proposee !== undefined && v.releves?.length) coach.boucleProposee = v.boucle_proposee;
  const fini = v.passage_fini;
  if (fini) {
    if (fini.proposition) coach.vitesseProposee = fini.proposition;
    const et = fini.ecart_et_ms === null ? "" : ` · ± ${Math.round(fini.ecart_et_ms)} ms`;
    const suite = fini.proposition
      ? fini.proposition.monter
        ? ` — passer à ${Math.round(fini.proposition.vitesse * 100)} % ?`
        : ` — redescendre à ${Math.round(fini.proposition.vitesse * 100)} % ?`
      : "";
    annoncer(`Passage ${fini.propre ? "propre" : "à reprendre"} : ${fini.attrapees} / ${fini.attendues}${et}${suite}`);
    // Le compte de la barre repart avec le passage suivant.
    coach.bilan = null;
  }
  if (!v.jugements.length && !v.places_perdues.length) {
    if (v.releves?.length) dire();
    return;
  }
  for (const j of v.jugements) {
    if (j.reference !== null && j.passage !== coach.passage) {
      // Un nouveau passage de boucle : son propre compte, les marques de
      // l'ancien restent jusqu'à ce que la même note soit rejouée.
      coach.passage = j.passage;
      coach.bilan = bilanVide();
    }
    coach.bilan ??= bilanVide();
    const b = coach.bilan;
    if (j.verdict === "en_trop") b.en_trop++;
    else if (j.verdict !== "non_notee") {
      b.attendues++;
      if (ATTRAPEES.has(j.verdict)) {
        b.attrapees++;
        if (j.ecart_musical_ms !== null) b.ecarts.push(j.ecart_musical_ms);
      }
    }
    if (j.reference !== null && VERDICTS[j.verdict]) coach.marques.set(j.reference, j);
    else if (j.reference !== null) coach.marques.delete(j.reference);
  }
  const perdue = v.places_perdues.at(-1);
  if (perdue) {
    annoncer(
      `Mesure ${perdue.mesure + 1} : place perdue — c'est la mesure ${perdue.decalage < 0 ? "d'avant" : "d'après"} qui se joue.`,
    );
  } else dire();
  dessinerMarques();
}

/// L'identifiant d'articulation de chaque pièce, lu sur la partition : sur
/// un temps qui ne porte qu'une pièce, sa seule note.
function articulations() {
  if (coach.articulations) return coach.articulations;
  const tr = batterieAffichee();
  const parRang = partition.batterie;
  const score = partition.api?.score;
  if (!tr || !parRang || !score) return new Map();
  const vu = new Map();
  for (const [rang, ou] of parRang) {
    const beat = score.tracks[0]?.staves[0]?.bars[ou.mesure]?.voices[ou.voix]?.beats[ou.temps];
    const piece = tr.pret.coups[rang]?.piece;
    if (beat?.notes?.length === 1 && piece && !vu.has(piece)) vu.set(piece, beat.notes[0].percussionArticulation);
  }
  coach.articulations = vu;
  return vu;
}

/// Un cercle autour de chaque note jugée, sur la partition dessinée.
function dessinerMarques() {
  const rendu = $("partition-rendu");
  const surface = rendu.querySelector(".at-surface");
  const table = partition.api?.boundsLookup ?? partition.api?.renderer?.boundsLookup;
  const score = partition.api?.score;
  const tr = batterieAffichee();
  let n = 0;
  if (surface && table && score && tr && partition.batterie) {
    const s = surface.getBoundingClientRect();
    const r = rendu.getBoundingClientRect();
    const arts = articulations();
    for (const [rang, j] of coach.marques) {
      const ou = partition.batterie.get(rang);
      const beat = ou && score.tracks[0]?.staves[0]?.bars[ou.mesure]?.voices[ou.voix]?.beats[ou.temps];
      const b = beat ? table.findBeat(beat) : null;
      if (!b) continue;
      const art = arts.get(tr.pret.coups[rang]?.piece);
      const notes = b.notes ?? [];
      const nb = notes.find((x) => x.note?.percussionArticulation === art) ?? (notes.length === 1 ? notes[0] : null);
      const t = nb?.noteHeadBounds ?? b.visualBounds;
      let c = coach.cercles[n];
      if (!c?.isConnected) {
        c = coach.cercles[n] = document.createElement("div");
        rendu.append(c);
      }
      n++;
      const v = VERDICTS[j.verdict];
      c.className = `coach__marque coach__marque--${v.classe}`;
      c.title =
        v.dit + (j.ecart_musical_ms !== null && v.classe !== "faute" ? ` (${j.ecart_musical_ms > 0 ? "+" : ""}${Math.round(j.ecart_musical_ms)} ms)` : "");
      const cote = Math.max(t.w, t.h) + 12;
      c.style.width = c.style.height = `${cote}px`;
      c.style.transform =
        `translate(${s.left - r.left + t.x + t.w / 2 - cote / 2}px, ${s.top - r.top + t.y + t.h / 2 - cote / 2}px)`;
      c.hidden = false;
    }
  }
  for (const c of coach.cercles.slice(n)) c.hidden = true;
}

function effacerMarques() {
  coach.marques.clear();
  coach.passage = null;
  coach.bilan = null;
  for (const c of coach.cercles) c.remove();
  coach.cercles = [];
}

/// La partition redessinée (redimensionnement, autre morceau) : les cercles
/// se reposent sur ses nouvelles positions.
function brancherCoach(api) {
  api.renderFinished.on(() => {
    coach.articulations = null;
    dessinerMarques();
  });
}

/// `lancerDecompte` demande si ce décompte sert à calibrer.
function calibrageDemande() {
  return coach.actif && coach.calibrage && !!coach.peripherique;
}

/// Le décompte est parti : s'il calibre, on mesure une fois les clics
/// passés (le moteur situe les clics par l'ancre de la musique qui suit).
function apresDecompte(plan, attenteMs) {
  // Le jugement reprend avec la musique, sans rien juger des clics : le
  // premier sondage d'après ne fait que poser son point de départ.
  coach.depuis = null;
  coach.pauseJusqua = performance.now() + attenteMs + 400;
  if (!calibrageDemande()) return;
  setTimeout(async () => {
    try {
      const v = await invoke("coach_calibrer", { clics: plan.clics.map((c) => c[0]) });
      coach.latenceMs = v.latence_ms;
      annoncer(`Calibrée : ${Math.round(v.latence_ms)} ms (± ${Math.round(v.dispersion_ms)}), ${v.retenues} frappes retenues.`);
    } catch (e) {
      annoncer(`Calibration manquée : ${e}`);
    } finally {
      coach.calibrage = false;
      coach.depuis = null;
      dire();
    }
  }, attenteMs + 400);
}

$("coach-basculer").addEventListener("click", () => (coach.actif ? arreter() : demarrer()));
$("coach-peripherique").addEventListener("change", (e) => ecouter(e.target.value));
$("coach-calibrer").addEventListener("click", () => {
  coach.calibrage = !coach.calibrage;
  coach.message = null;
  dire();
});
$("coach-boucler").addEventListener("click", () => bouclerProposee().catch((e) => annoncer(`Boucle non posée : ${e}`)));
$("coach-vitesse").addEventListener("click", async () => {
  const v = coach.vitesseProposee;
  if (!v) return;
  coach.vitesseProposee = null;
  try {
    await poserVitesse(v.vitesse);
  } catch (e) {
    annoncer(`Vitesse non appliquée : ${e}`);
  }
  dire();
});

export { apresDecompte, brancherCoach, calibrageDemande, coach, majCoach };
