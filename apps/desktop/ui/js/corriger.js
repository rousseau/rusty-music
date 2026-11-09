import "./memoire-pratique.js"; // l'ordre d'exécution : voir main.js
import { $, invoke, journal, txt } from "./socle.js";
import { edition } from "./etabli.js";
import { basseAffichee, bassesJouees, partition, remplacerBasse } from "./pratiquer.js";

/* ------------------------------------- pratiquer : corriger la transcription */

// E2 (`docs/plan-editer-pratique-creation.md` 1.5) : la basse transcrite se
// corrige depuis la partition. Un clic sur une note la choisit (alphaTab
// `beatMouseDown`) ; la barre au-dessus de la partition (`#partition-correction`,
// au centre : Règle 12, rien dans le rail) dit ce qu'elle est et offre les
// gestes — corde plus grave ou plus aiguë, octave, supprimer, rétablir ; sur un
// silence, ajouter une note. Le moteur garde la correction dans la mémoire de
// Pratiquer (`pratique_correction`) et rend la transcription corrigée
// (`corriger_transcription`) : une nouvelle transcription la réapplique, sans
// jamais réécrire une note corrigée. La même barre exporte la partie corrigée
// (MIDI, Guitar Pro 7), par alphaTab.

const NOMS = ["do", "do♯", "ré", "ré♯", "mi", "fa", "fa♯", "sol", "sol♯", "la", "la♯", "si"];
const nomNote = (h) => `${NOMS[h % 12]}${Math.floor(h / 12) - 1}`;
/// Les frettes du manche (`tablature::Accordage`, 20 sur toutes les basses).
const FRETTES = 20;

/// Le temps choisi dans la partition : sa mesure et l'instant où il commence
/// dans la mesure, en doubles croches. Retenu par position plutôt que par
/// rang : après une correction, la mesure se redécoupe autrement.
const choix = { mesure: null, debut: null, cadre: null, occupe: false };

function brancherCorrection(api) {
  api.beatMouseDown.on((beat) => {
    if (!basseAffichee() || !partition.temps || beat?.voice?.index !== 0) return;
    const mesure = beat.voice.bar?.index;
    const t = partition.temps[mesure]?.[beat.index];
    if (!t) return;
    Object.assign(choix, { mesure, debut: t.debut });
    majCorrection();
  });
  api.renderFinished.on(() => dessinerChoix());
}

/// Le temps choisi : `{ indice, note, debut, seiziemes }`, ou `null`.
function tempsChoisi() {
  const carte = partition.temps?.[choix.mesure];
  if (!carte || choix.debut === null) return null;
  const indice = carte.findIndex((t) => t.debut <= choix.debut && choix.debut < t.debut + t.seiziemes);
  return indice < 0 ? null : { indice, ...carte[indice] };
}

/// La corde la plus proche de `corde`, vers le grave (`pas` −1) ou l'aigu
/// (+1), où `hauteur` se joue encore sur le manche.
function cordeVoisine(hauteur, corde, pas, accordage) {
  for (let c = corde + pas; c >= 0 && c < accordage.length; c += pas) {
    const f = hauteur - accordage[c];
    if (f >= 0 && f <= FRETTES) return c;
  }
  return null;
}
const jouable = (hauteur, accordage) => accordage.some((v) => hauteur - v >= 0 && hauteur - v <= FRETTES);
const corrigee = (id) => !!id && (id.ajoutee || id.octaves !== 0 || id.corde_imposee);

/// Ce que la barre propose pour le temps choisi : `[geste, correction | null]`.
function gestes(tr, t) {
  const vue = tr.pret;
  const acc = vue.accordage ?? [];
  if (t.note === null) {
    return [["ajouter", ajout(vue, t)]];
  }
  const n = vue.notes[t.note];
  const id = vue.identites?.[t.note];
  if (!n || !id) return [];
  const vise = { debut_s: id.debut_s, hauteur: id.hauteur };
  const corde = (pas) => {
    const c = n.corde === null ? null : cordeVoisine(n.hauteur, n.corde, pas, acc);
    return c === null ? null : { genre: "corde", ...vise, corde: c };
  };
  const octave = (pas) => {
    const o = (id.octaves ?? 0) + pas;
    return Math.abs(o) <= 2 && jouable(n.hauteur + 12 * pas, acc) ? { genre: "octave", ...vise, octaves: o } : null;
  };
  const liste = [
    ["grave", corde(-1)],
    ["aigue", corde(1)],
    ["octave-bas", octave(-1)],
    ["octave-haut", octave(1)],
  ];
  // Une note ajoutée change de hauteur au demi-ton : elle est reposée.
  if (id.ajoutee) {
    for (const [geste, pas] of [["demi-bas", -1], ["demi-haut", 1]]) {
      const h = n.hauteur + pas;
      liste.push([geste, jouable(h, acc) ? { remplacer: true, hauteur: h } : null]);
    }
  }
  liste.push(["supprimer", { genre: "supprimer", ...vise }]);
  if (corrigee(id) && !id.ajoutee) liste.push(["retablir", { genre: "retablir", ...vise }]);
  return liste;
}

/// Une note ajoutée sur un silence : à son premier temps, sur toute sa durée,
/// à la hauteur de la note jouée juste avant (ou après, ou la corde grave).
function ajout(vue, t, hauteur = null) {
  const debut = instantGrille(choix.mesure, t.debut);
  const fin = instantGrille(choix.mesure, t.debut + t.seiziemes);
  if (debut === null || fin === null || !(fin > debut)) return null;
  if (hauteur === null) {
    const avant = [];
    partition.temps.forEach((carte, m) =>
      carte.forEach((x) => {
        if (x.note !== null && (m < choix.mesure || (m === choix.mesure && x.debut < t.debut))) avant.push(x.note);
      }),
    );
    const voisine = avant.at(-1) ?? partition.temps.flat().find((x) => x.note !== null)?.note;
    hauteur = vue.notes[voisine]?.hauteur ?? vue.accordage?.[0] ?? 28;
  }
  return { genre: "ajouter", debut_s: debut, hauteur, duree_s: fin - debut };
}

/// L'instant (s) de la double croche `seiziemes` de la mesure `mesure`, sur la
/// pulsation mesurée — la grille sur laquelle le moteur quantifie.
function instantGrille(mesure, seiziemes) {
  const T = edition.pulsation?.temps;
  const m = basseAffichee()?.pret.mesures[mesure];
  if (!T || T.length < 2 || !m) return null;
  let j = 0;
  for (let k = 1; k < T.length; k++) if (Math.abs(T[k] - m.debut_s) < Math.abs(T[j] - m.debut_s)) j = k;
  const x = j + seiziemes / 4;
  const k = Math.max(0, Math.min(Math.floor(x), T.length - 2));
  return T[k] + (x - k) * (T[k + 1] - T[k]);
}

const LIBELLES_CORDE = (c, acc) => `corde de ${NOMS[acc[c] % 12]}`;

/// La barre de correction : visible quand une basse transcrite est au centre.
function majCorrection() {
  const barre = $("partition-correction");
  const tr = basseAffichee();
  barre.hidden = !tr;
  const t = tr ? tempsChoisi() : null;
  if (!t) Object.assign(choix, { mesure: null, debut: null });
  const offerts = tr && t ? gestes(tr, t) : [];
  const dits = new Map(offerts);
  for (const b of barre.querySelectorAll("[data-corriger]")) {
    const geste = b.dataset.corriger;
    b.hidden = !dits.has(geste);
    // Pas désactivé pendant l'appel au moteur (`choix.occupe` écarte le
    // second clic) : le bouton garderait sinon le focus du clavier.
    b.disabled = !dits.get(geste);
  }
  if (!tr) return dessinerChoix();
  const texte = $("correction-note");
  if (!t) {
    texte.textContent = "Cliquer une note pour la corriger, un silence pour en ajouter une.";
  } else if (t.note === null) {
    texte.textContent = "Silence";
  } else {
    const vue = tr.pret;
    const n = vue.notes[t.note];
    const id = vue.identites?.[t.note];
    const acc = vue.accordage ?? [];
    const position =
      n.corde === null ? "" : ` · ${LIBELLES_CORDE(n.corde, acc)}, ${n.frette === 0 ? "à vide" : `frette ${n.frette}`}`;
    const marque = id?.ajoutee ? " · ajoutée" : corrigee(id) ? " · corrigée" : "";
    texte.textContent = `${nomNote(n.hauteur)}${position}${marque}`;
  }
  const sansEffet = tr.pret.sans_effet ?? 0;
  texte.title = sansEffet
    ? `${sansEffet} correction${sansEffet > 1 ? "s" : ""} gardée${sansEffet > 1 ? "s" : ""} sans note à corriger dans cette transcription.`
    : "";
  dessinerChoix();
}

/// Le cadre du temps choisi, posé sur la partition (au centre, comme le
/// curseur de temps).
function dessinerChoix() {
  const rendu = $("partition-rendu");
  const t = basseAffichee() ? tempsChoisi() : null;
  const beat = t ? partition.api?.score?.tracks[0]?.staves[0]?.bars[choix.mesure]?.voices[0]?.beats[t.indice] : null;
  const table = partition.api?.boundsLookup ?? partition.api?.renderer?.boundsLookup;
  const b = beat ? table?.findBeat(beat) : null;
  const ligne = b?.barBounds?.masterBarBounds?.lineAlignedBounds;
  const surface = rendu.querySelector(".at-surface");
  let cadre = choix.cadre;
  if (!b || !ligne || !surface) {
    if (cadre) cadre.hidden = true;
    return;
  }
  if (!cadre?.isConnected) {
    cadre = choix.cadre = document.createElement("div");
    cadre.className = "partition__choix";
    rendu.append(cadre);
  }
  const s = surface.getBoundingClientRect();
  const r = rendu.getBoundingClientRect();
  const v = b.visualBounds;
  cadre.hidden = false;
  cadre.style.width = `${Math.max(14, v.w + 8)}px`;
  cadre.style.height = `${ligne.h}px`;
  cadre.style.transform = `translate(${s.left - r.left + v.x - 4}px, ${s.top - r.top + ligne.y}px)`;
}

/// Les notes corrigées, à l'encre d'accent dans la partition (avant le rendu).
function marquerCorrigees(score, vue, temps) {
  const M = globalThis.alphaTab?.model;
  if (!M?.NoteStyle || !M.NoteSubElement || !temps || !vue.identites?.length) return;
  try {
    const css = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#C07C4A";
    const couleur = M.Color.fromJson(css);
    const elements = [M.NoteSubElement.StandardNotationNoteHead, M.NoteSubElement.GuitarTabFretNumber];
    score.tracks[0]?.staves[0]?.bars.forEach((bar, m) =>
      bar.voices[0]?.beats.forEach((beat, k) => {
        const t = temps[m]?.[k];
        if (t?.note === null || t?.note === undefined || !corrigee(vue.identites[t.note])) return;
        for (const note of beat.notes) {
          note.style ??= new M.NoteStyle();
          for (const el of elements) note.style.colors.set(el, couleur);
        }
      }),
    );
  } catch (e) {
    journal("correction", `notes corrigées non marquées : ${e}`, "warn");
  }
}

async function corriger(geste) {
  const tr = basseAffichee();
  const t = tempsChoisi();
  const id = edition.source?.id;
  const correction = tr && t ? new Map(gestes(tr, t)).get(geste) : null;
  if (!correction || choix.occupe || id === undefined) return;
  choix.occupe = true;
  const envoyer = (c) => invoke("corriger_transcription", { id, qualite: tr.qualite, basses: bassesJouees, correction: c });
  try {
    let vue;
    if (correction.remplacer) {
      // Une note ajoutée change de hauteur : l'ancienne s'oublie, la nouvelle
      // se pose au même temps de la grille.
      const ident = tr.pret.identites[t.note];
      const nouvelle = ajout(tr.pret, t, correction.hauteur);
      if (!nouvelle) return;
      await envoyer({ genre: "supprimer", debut_s: ident.debut_s, hauteur: ident.hauteur });
      vue = await envoyer(nouvelle);
    } else {
      vue = await envoyer(correction);
    }
    if (edition.source?.id === id) remplacerBasse(tr, vue);
  } catch (e) {
    journal("correction", e, "warn");
    $("correction-note").textContent = `Correction impossible : ${e}`;
    return;
  } finally {
    choix.occupe = false;
  }
  majCorrection();
}

for (const b of document.querySelectorAll("#partition-correction [data-corriger]")) {
  b.addEventListener("click", () => corriger(b.dataset.corriger));
}
$("partition-correction").addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || choix.mesure === null) return;
  e.stopPropagation();
  Object.assign(choix, { mesure: null, debut: null });
  majCorrection();
});

/* ---------------------------------------------- exporter la partie corrigée */

/// Les octets de la partie affichée, par alphaTab : un fichier MIDI
/// (`MidiFileGenerator`) ou Guitar Pro 7 (`Gp7Exporter`). La partition est
/// celle qu'on lit : la basse corrigée, quantifiée sur la grille.
function octetsPartie(format) {
  const api = partition.api;
  const score = api?.score;
  if (!score) return null;
  if (format === "midi") {
    const midi = new alphaTab.midi.MidiFile();
    const generateur = new alphaTab.midi.MidiFileGenerator(score, api.settings, new alphaTab.midi.AlphaSynthMidiFileHandler(midi, true));
    generateur.generate();
    return midi.toBinary();
  }
  return new alphaTab.exporter.Gp7Exporter().export(score, api.settings);
}

const FORMATS = {
  midi: { extension: "mid", nom: "MIDI" },
  gp: { extension: "gp", nom: "Guitar Pro" },
};

async function exporter(format) {
  const f = FORMATS[format];
  const statut = $("correction-note");
  try {
    const octets = octetsPartie(format);
    if (!octets?.length) throw new Error("partition vide");
    const titre = txt(edition.source?.title, "partie").replace(/[\\/:*?"<>|]/g, " ");
    const chemin = await window.__TAURI__.dialog.save({
      defaultPath: `${titre} - basse.${f.extension}`,
      filters: [{ name: f.nom, extensions: [f.extension] }],
    });
    if (!chemin) return;
    await invoke("exporter_partie", { chemin, octets: Array.from(octets) });
    statut.textContent = `Partie exportée (${f.nom}) : ${chemin}`;
  } catch (e) {
    journal("export", e?.stack ?? e, "warn");
    statut.textContent = `Export impossible : ${e?.message ?? e}`;
  }
}

for (const b of document.querySelectorAll("#partition-correction [data-exporter]")) {
  b.addEventListener("click", () => exporter(b.dataset.exporter));
}

export { brancherCorrection, majCorrection, marquerCorrigees };
