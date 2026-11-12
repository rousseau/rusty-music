import "./etabli-stems.js"; // l'ordre d'exécution : voir main.js
import {
  $, enStems, invoke, journal, ouvrirLien, txt, vue,
} from "./socle.js";
import { lecture } from "./etat-lecture.js";
import { demander } from "./ecrans.js";
import { edition, majBoucleUI, NOMS_SECTIONS } from "./etabli.js";
import { positionnerPlayhead, varianteParDefaut } from "./etabli-stems.js";
import { CHOIX, nommer } from "./accordage.js";
import { brancherCorrection, majCorrection, marquerCorrigees } from "./corriger.js";
import { apresDecompte, brancherCoach, calibrageDemande, majCoach } from "./coach.js";

/* ------------------------------------------------------- pratiquer */

// Le propre de Pratiquer : la partition, la partie jouée, le décompte, la
// transcription (MuScriptor compris). Le commun de l'établi : `etabli.js`,
// `etabli-reglages.js` et `etabli-stems.js`.

// La partition au centre (décision 11 de `ui-spec-editeur.md`), rendue par
// alphaTab (MPL-2.0, `vendor/alphatab/`), chargé seulement quand on pratique.
// alphaTab n'émet aucun son : il suit le lecteur de stems
// (`PlayerMode.EnabledExternalMedia`) — chaque commande qu'il donne devient
// un ordre au moteur, et le moteur lui rend la position.
//
// **Prototype** (chantier 1.4 de `plan-editer-pratique-creation.md`) : faute
// de transcription, la partition ne porte que la grille mesurée — une note
// par temps, une mesure par premier temps détecté, un `\sync` par mesure.
const partition = {
  api: null,
  // La création d'alphaTab en cours ou faite (`preparerPartition`).
  prete: null,
  // Le morceau dont la grille est chargée, pour ne pas la recharger.
  source: null,
  // Le chargement en cours : le suivant l'attend. Deux `api.tex` coup sur coup
  // (la grille, puis les sections qui arrivent) laissaient le lecteur externe
  // d'alphaTab décroché de la lecture.
  chargement: Promise.resolve(),
  // Instant du dernier clic dans la partition. alphaTab émet aussi des
  // `seekTo` de lui-même pendant un chargement (0, puis quelques ms — vu à
  // l'essai du 9 oct.) : on ne suit que ceux qui viennent d'un geste.
  gesteA: -Infinity,
  // Dernier défilement à la main : on ne ramène pas la vue au curseur
  // pendant 3 s, le temps de lire plus loin.
  moletteA: -Infinity,
  // Dernier défilement lancé ici : l'animation dure ~0,5 s, on ne la
  // relance pas à chaque tick de 50 ms pendant qu'elle se déroule.
  defileA: -Infinity,
  minuteur: null,
  // Notre curseur de temps (`placerCurseur`) et sa boucle d'images.
  curseur: null,
  image: null,
  // La basse transcrite affichée : par mesure, par temps alphaTex, la note
  // jouée (son rang dans la transcription, ou `null` pour un silence), où le
  // temps commence et combien il dure, en doubles croches (`texTablature`).
  // La correction (E2, `corriger.js`) s'en sert pour retrouver une note
  // cliquée ; `null` quand ce n'est pas une basse transcrite.
  temps: null,
  // La batterie transcrite affichée : pour chaque coup de la transcription
  // (son rang), le temps alphaTab qui le porte — mesure, voix (0 mains,
  // 1 pieds), rang du temps dans la voix (`texBatterie`). Le coach (E4,
  // `coach.js`) s'en sert pour colorer la note qu'il juge ; `null` sinon.
  batterie: null,
};

/// Charge un script une seule fois, même appelé plusieurs fois. Le
/// recharger redéfinissait alphaTab sous l'instance en cours — le « panneau
/// blanc » de l'essai du 9 oct.
const scriptsCharges = new Map();
function chargerScript(src) {
  if (!scriptsCharges.has(src)) scriptsCharges.set(src, chargerScriptUneFois(src));
  return scriptsCharges.get(src);
}
function chargerScriptUneFois(src) {
  return new Promise((ok, ko) => {
    const el = document.createElement("script");
    el.src = src;
    el.onload = ok;
    el.onerror = () => ko(new Error(`script introuvable : ${src}`));
    document.head.appendChild(el);
  });
}

/// Les points de synchro d'une mesure : un par temps détecté, pas seulement
/// le premier. alphaTab avance au tempo moyen entre deux points ; avec un
/// point par mesure, le curseur accélérait ou freinait dans la mesure puis
/// sautait à la barre (essai du 9 oct.). Un par temps suit le jeu réel.
function syncTemps(i, debut, fin, T) {
  const dans = T.filter((t) => t >= debut - 1e-3 && t < fin - 1e-3);
  if (!dans.length || Math.abs(dans[0] - debut) > 0.05) dans.unshift(debut);
  const n = dans.length;
  return dans
    .map((t, k) => (k === 0 ? `\\sync (${i} 0 ${Math.round(t * 1000)}) ` : `\\sync (${i} 0 ${Math.round(t * 1000)} ${(k / n).toFixed(4)}) `))
    .join("");
}

/// Le tempo affiché en tête de partition : celui de la pulsation, ou le temps
/// moyen du premier au dernier temps quand elle n'en donne pas (120 n'était
/// qu'un défaut ; la médiane d'un intervalle arrondi aux 10 ms donnait 130
/// pour un morceau à 129). La synchro, elle, suit les temps.
function tempoDe(p) {
  if (p.bpm) return Math.round(p.bpm);
  const T = p.temps ?? [];
  return T.length > 1 ? Math.round((60 * (T.length - 1)) / (T[T.length - 1] - T[0])) : 120;
}

/// L'alphaTex de la grille mesurée, pour l'instrument pratiqué : une note par
/// temps, une métrique par mesure — celle que les temps détectés donnent —, un
/// point de synchro par premier temps. La basse se lit en **tablature** (corde
/// de mi à vide, faute de transcription), la batterie en **portée de
/// percussion** (charleston fermé) — préférences arrêtées le 9 oct.
function texGrille(p, titre, instrument) {
  const P = p.premiers_temps;
  const T = p.temps;
  const guillemets = (x) => String(x).replace(/["\\]/g, " ");
  const batterie = instrument === "drums";
  const piste = batterie
    ? `\\track "Batterie" \\instrument percussion \\articulation defaults \\staff {score}`
    : `\\track "Basse" \\staff {tabs} \\tuning (G2 D2 A1 E1)`;
  const note = batterie ? `"Hi-Hat (closed)".4` : "0.4.4";
  const tete =
    `\\title "${guillemets(titre)}" \\subtitle "Grille mesurée — prototype, sans transcription"` +
    ` \\tempo ${tempoDe(p)}\n${piste}\n`;
  const mesures = [];
  const sections = marquesSections(P);
  let precedente = 0;
  let k = 0;
  for (let i = 0; i < P.length; i++) {
    const fin = i + 1 < P.length ? P[i + 1] : Infinity;
    while (k < T.length && T[k] < P[i] - 1e-3) k++;
    let n = 0;
    while (k + n < T.length && T[k + n] < fin - 1e-3) n++;
    if (i + 1 >= P.length) n = p.metrique ?? 4;
    n = Math.max(1, Math.min(n, 12));
    // Portée de percussion : clé neutre, pas la clé de sol par défaut.
    let m = (i === 0 && batterie ? "\\clef neutral " : "") + sections[i];
    m += n !== precedente ? `\\ts (${n} 4) ` : "";
    precedente = n;
    m += syncTemps(i, P[i], fin, T) + Array(n).fill(note).join(" ");
    mesures.push(m);
  }
  return tete + mesures.join(" |\n") + " |";
}

/// Les marques de section (`\section "Refrain"`) par mesure : chaque section
/// nommée va sur la mesure dont le début est le plus proche de sa frontière
/// (all-in-one et Beat This! ne posent pas les frontières au même endroit à
/// quelques centièmes près). Une mesure ne porte qu'une marque, la dernière.
function marquesSections(debuts) {
  const marques = debuts.map(() => "");
  if (!edition.sections || !debuts.length) return marques;
  const guillemets = (x) => String(x).replace(/["\\]/g, " ");
  for (const x of edition.sections) {
    const nom = NOMS_SECTIONS[x.etiquette] ?? x.etiquette;
    if (!nom) continue;
    let i = 0;
    for (let k = 1; k < debuts.length; k++) {
      if (Math.abs(debuts[k] - x.debut_s) < Math.abs(debuts[i] - x.debut_s)) i = k;
    }
    marques[i] = `\\section "${guillemets(nom[0].toUpperCase() + nom.slice(1))}" `;
  }
  return marques;
}

/// Durées alphaTex d'une longueur en doubles croches, la plus longue d'abord.
const DUREES_TEX = [[16, "1"], [12, "2{d}"], [8, "2"], [6, "4{d}"], [4, "4"], [3, "8{d}"], [2, "8"], [1, "16"]];
const SEIZIEMES_TEX = new Map(DUREES_TEX.map(([n, d]) => [d, n]));
function decouperDuree(seiziemes) {
  const morceaux = [];
  let reste = seiziemes;
  for (const [n, d] of DUREES_TEX) {
    while (reste >= n) {
      morceaux.push(d);
      reste -= n;
    }
  }
  return morceaux;
}

/// L'alphaTex d'une basse transcrite : les mesures quantifiées par le moteur
/// (`crates/transcription`, `quantification::Mesure`) — frettes, silences,
/// liaisons (`-.corde.durée`) quand une durée se découpe ou franchit une barre.
/// Corde 0 du moteur = la plus grave = corde 4 de l'alphaTex.
///
/// V37 : écrite pour l'accordage du morceau (`accordage`), elle ne suit pas
/// la hauteur des stems — les mêmes frettes à toute transposition ; le
/// bandeau du coach dit l'accordage à utiliser avec elle (`accordage.js`).
/// La partition en notes montre de même les notes du morceau.
function texTablature(mesures, titre, p, accordage, modele) {
  const guillemets = (x) => String(x).replace(/["\\]/g, " ");
  // Pas de silences multimesures : regrouper huit mesures de silence en une
  // casserait la largeur égale des mesures, et le curseur y filerait.
  // L'accordage vient du moteur (standard, drop D, cinq cordes), de la corde
  // aiguë à la grave.
  const cordes = accordage?.length ? [...accordage].reverse() : [43, 38, 33, 28];
  const portee = edition.notation === "tabs" ? "\\staff {tabs}" : "\\staff {score tabs}";
  const tete =
    `\\title "${guillemets(titre)}" \\subtitle "Transcription automatique (${modele === "muscriptor-medium" ? "MuScriptor" : "Basic Pitch"}) — à vérifier à l'oreille"` +
    ` \\tempo ${tempoDe(p)}\n` +
    `\\track "Basse" ${portee} \\tuning (${cordes.map(nomMidi).join(" ")})\n` +
    (edition.notation === "tabs" ? "" : "\\clef F4\n");
  let precedente = 0;
  const sections = marquesSections(mesures.map((m) => m.debut_s));
  partition.temps = [];
  const corps = mesures.map((m, i) => {
    let t = sections[i] + (m.temps !== precedente ? `\\ts (${m.temps} 4) ` : "");
    precedente = m.temps;
    const fin = mesures[i + 1]?.debut_s ?? Infinity;
    t += syncTemps(i, m.debut_s, fin, p.temps);
    const temps = [];
    // Chaque temps alphaTex, dans l'ordre : sa note, son début et sa durée.
    const carte = [];
    let curseur = 0;
    const poser = (tex, d, note) => {
      temps.push(tex);
      carte.push({ note: note ?? null, debut: curseur, seiziemes: SEIZIEMES_TEX.get(d) });
      curseur += SEIZIEMES_TEX.get(d);
    };
    for (const e of legato(m.evenements)) {
      const durees = decouperDuree(e.seiziemes);
      const jeu = e.jeu;
      if (!jeu || jeu.corde === null || jeu.frette === null) {
        for (const d of durees) poser(`r.${d}`, d, null);
        continue;
      }
      const corde = cordes.length - jeu.corde;
      durees.forEach((d, k) => poser(e.lie || k > 0 ? `-.${corde}.${d}` : `${jeu.frette}.${corde}.${d}`, d, e.note));
    }
    if (!temps.length) poser(`r.${m.temps === 4 ? "1" : "4"}`, m.temps === 4 ? "1" : "4", null);
    partition.temps.push(carte);
    return t + temps.join(" ");
  });
  return tete + corps.join(" |\n") + " |";
}

/// Notation de la basse : « score+tabs » (défaut, décidé le 10 oct. —
/// comme les livres « Bass Recorded Versions », la notation en clé de fa
/// au-dessus de la tablature) ou « tabs » (tablature seule). Bascule dans le
/// rail. Le navigateur retient le dernier choix, défaut d'un morceau neuf ;
/// chaque morceau garde le sien dans sa mémoire de Pratiquer (E1,
/// `memoire-pratique.js`).
edition.notation = (() => {
  try {
    return localStorage.getItem("notation-basse") === "tabs" ? "tabs" : "score+tabs";
  } catch {
    return "score+tabs";
  }
})();

/// Une note suivie d'un silence d'une double croche le garde pour elle :
/// la transcription coupe les notes un peu court, et une partition de basse
/// écrit la note jusqu'à la suivante (« do‿½ soupir » plutôt que « do 1/16 +
/// silence 1/16 »). Les vrais silences, plus longs, restent.
function legato(evenements) {
  const out = [];
  for (const e of evenements) {
    const prec = out[out.length - 1];
    if (!e.jeu && e.seiziemes === 1 && prec?.jeu) {
      out[out.length - 1] = { ...prec, seiziemes: prec.seiziemes + 1 };
    } else out.push(e);
  }
  return out;
}

/// Nom alphaTex d'une hauteur MIDI : 28 → « E1 », 43 → « G2 ».
function nomMidi(h) {
  const noms = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
  return `${noms[h % 12]}${Math.floor(h / 12) - 1}`;
}

/// Les pièces d'ADTOF en articulations Guitar Pro (`\articulation defaults`).
const ARTICULATIONS = {
  grosse_caisse: '"Kick (hit)"',
  caisse_claire: '"Snare (hit)"',
  toms: '"Mid Tom (hit)"',
  charleston: '"Hi-Hat (closed)"',
  cymbales: '"Crash high (hit)"',
};

/// L'alphaTex d'une batterie transcrite : les mesures quantifiées par le
/// moteur (`quantification::MesureBatterie`), écrites comme une partition de
/// batteur — deux voix, les mains en haut (charleston, cymbales, caisse
/// claire, toms ; hampes vers le haut), les pieds en bas (grosse caisse ;
/// hampes vers le bas). Chaque voix tient sa note jusqu'à sa frappe suivante,
/// coupée aux temps pour que chaque temps se lise d'un bloc (essai du 15 oct.
/// contre une partition publiée).
const PIEDS = new Set(["grosse_caisse"]);
function texBatterie(mesures, titre, p, coups = []) {
  const guillemets = (x) => String(x).replace(/["\\]/g, " ");
  const tete =
    `\\title "${guillemets(titre)}" \\subtitle "Transcription automatique (ADTOF) — à vérifier à l'oreille"` +
    ` \\tempo ${tempoDe(p)}\n\\track "Batterie" \\instrument percussion \\articulation defaults \\staff {score}\n`;
  const sections = marquesSections(mesures.map((m) => m.debut_s));
  const haut = [];
  const bas = [];
  let precedente = 0;
  const parRang = new Map();
  const garderMains = (x) => !PIEDS.has(x);
  const garderPieds = (x) => PIEDS.has(x);
  mesures.forEach((m, i) => {
    // La métrique, les sections et la synchro sont des réglages de mesure :
    // la première voix les porte.
    let t = (i === 0 ? "\\clef neutral " : "") + sections[i];
    t += m.temps !== precedente ? `\\ts (${m.temps} 4) ` : "";
    precedente = m.temps;
    const fin = mesures[i + 1]?.debut_s ?? Infinity;
    t += syncTemps(i, m.debut_s, fin, p.temps);
    const poser = (voix) => (rang, k) => parRang.set(rang, { mesure: i, voix, temps: k });
    haut.push(t + voixBatterie(m, garderMains, coups, poser(0)));
    bas.push(voixBatterie(m, garderPieds, coups, poser(1)));
  });
  partition.batterie = parRang;
  return tete + "\\voice\n" + haut.join(" |\n") + " |\n\\voice\n" + bas.join(" |\n") + " |";
}

/// Une voix d'une mesure de batterie : les frappes des pièces retenues, puis
/// des silences ; une note dure jusqu'à la frappe suivante de sa voix.
function voixBatterie(m, garder, coups = [], marquer = () => {}) {
  const total = m.temps * 4;
  const frappes = [];
  let pos = 0;
  for (const f of m.frappes) {
    const pieces = f.pieces.filter(garder);
    // Les coups de cette case que porte cette voix (E4 : le coach les
    // retrouve sur la partition).
    const rangs = (f.coups ?? []).filter((r) => coups[r] && garder(coups[r].piece));
    if (pieces.length) frappes.push({ pos, pieces, rangs });
    pos += f.seiziemes;
  }
  const out = [];
  // Une frappe s'écrit au plus noire, la suite en silences : une grosse
  // caisse sur 1 et 3 est une noire sur 1 et une sur 3. Les silences de la
  // voix des pieds ne se dessinent pas (`silencesInvisibles`).
  const poser = (debut, fin, tete) => {
    const noteFin = tete ? Math.min(fin, debut % 4 === 0 ? debut + 4 : (Math.floor(debut / 4) + 1) * 4) : debut;
    decouperAuxTemps(debut, noteFin).forEach((d, k) => out.push(k === 0 ? `${tete}.${d}` : `r.${d}`));
    decouperAuxTemps(noteFin, fin).forEach((d) => out.push(`r.${d}`));
  };
  if (!frappes.length || frappes[0].pos > 0) poser(0, frappes[0]?.pos ?? total, null);
  frappes.forEach((f, k) => {
    const acc = f.pieces.map((x) => ARTICULATIONS[x]).join(" ");
    // Le temps alphaTab de la tête est le prochain posé.
    for (const r of f.rangs) marquer(r, out.length);
    poser(f.pos, Math.min(total, frappes[k + 1]?.pos ?? total), f.pieces.length > 1 ? `(${acc})` : acc);
  });
  return out.join(" ");
}

/// Les durées alphaTex de [debut, fin) en doubles croches, coupées aux temps :
/// un bloc commencé sur un temps garde ses temps entiers (noire, blanche…),
/// le reste ne franchit jamais une barre de temps.
function decouperAuxTemps(debut, fin) {
  const durees = [];
  let x = debut;
  while (x < fin) {
    const prochainTemps = (Math.floor(x / 4) + 1) * 4;
    const y = x % 4 === 0 && fin - x >= 4 ? x + Math.floor((fin - x) / 4) * 4 : Math.min(fin, prochainTemps);
    durees.push(...decouperDuree(y - x));
    x = y;
  }
  return durees;
}

/// La transcription du morceau ouvert, par instrument : la promesse en cours
/// ou le résultat — calculée une fois, en cache côté moteur. Sans `lancer`,
/// seulement si elle est déjà en cache (`partitionsEnCache`) : le calcul ne
/// part que du bouton « Calculer » du bloc Partitions.
const transcriptions = new Map();
function transcription(instrument, lancer = false) {
  const id = edition.source?.id;
  if (id === undefined || (instrument !== "bass" && instrument !== "drums")) return null;
  const qualite = instrument === "bass";
  const cle = cleTranscription(id, instrument, qualite);
  if (!transcriptions.has(cle)) {
    if (!lancer && !enCache(id, instrument, qualite)) return null;
    const entree = { pret: null, erreur: null, qualite };
    if (qualite) suivreTranscription(id, entree);
    const basses = instrument === "bass" ? bassesJouees : undefined;
    entree.promesse = invoke("transcrire", { id, instrument, qualite, basses })
      .then((t) => {
        entree.pret = t;
      })
      .catch((e) => {
        entree.erreur = String(e);
        console.warn("transcription", e);
      })
      .finally(() => {
        if (edition.usage === "pratiquer" && edition.source?.id === id) montrerPartition();
        majBlocPartitions();
      });
    transcriptions.set(cle, entree);
    majBlocPartitions();
    // La barre du coach dit le calcul en cours (V34).
    if (edition.usage === "pratiquer") majCoach();
  }
  return transcriptions.get(cle);
}

/// Une partition par morceau, instrument, transcripteur — et, pour la basse,
/// par instrument joué : changer de basse en demande une autre, que le moteur
/// repose sur les mêmes notes sans retranscrire.
function cleTranscription(id, instrument, qualite) {
  const base = `${id}:${instrument}:${qualite ? "qualite" : "rapide"}`;
  return instrument === "bass" ? `${base}:${bassesJouees}` : base;
}

/// Les caches de partition du morceau ouvert (`transcriptions_en_cache`),
/// demandés une fois par morceau.
const partitionsEnCache = { id: null, cles: new Set(), demande: null };
function enCache(id, instrument, qualite) {
  if (partitionsEnCache.id !== id) return false;
  return partitionsEnCache.cles.has(instrument === "bass" && qualite ? "bass-muscriptor" : instrument);
}
function chargerPartitionsEnCache(id) {
  if (partitionsEnCache.demande === id) return;
  partitionsEnCache.demande = id;
  invoke("transcriptions_en_cache", { id })
    .then((cles) => {
      if (partitionsEnCache.demande !== id) return;
      partitionsEnCache.id = id;
      partitionsEnCache.cles = new Set(cles);
      majUsage();
    })
    .catch((e) => journal("partitions", e, "warn"));
}

/// Oublie les partitions d'un morceau après un nouveau démixage : celles du
/// moteur sont retirées avec les anciens stems, celles d'ici aussi.
function oublierPartitions(id) {
  for (const cle of [...transcriptions.keys()]) if (cle.startsWith(`${id}:`)) transcriptions.delete(cle);
  if (partitionsEnCache.id === id || partitionsEnCache.demande === id) {
    Object.assign(partitionsEnCache, { id: null, cles: new Set(), demande: null });
  }
  majBlocPartitions();
  if (edition.usage === "pratiquer") montrerPartition();
}

/// Le bloc « Partitions » du rail : par instrument séparé, son état et le
/// geste qui va avec — Calculer, la jauge pendant le calcul, puis Afficher.
function majBlocPartitions() {
  const id = edition.source?.id;
  demander("bloc-partitions", !$("bloc-usage").hidden && id !== undefined);
  if ($("bloc-partitions").hidden) return;
  chargerPartitionsEnCache(id);
  for (const ligne of document.querySelectorAll("#bloc-partitions [data-partition]")) {
    const instrument = ligne.dataset.partition;
    const present = edition.stems.some((s) => s.nom === instrument);
    const jauge = document.querySelector(`[data-partition-jauge="${instrument}"]`);
    ligne.hidden = !present;
    jauge.hidden = true;
    if (instrument === "bass") {
      $("basses").hidden = !present;
      for (const b of document.querySelectorAll("#basses [data-basses]")) {
        b.classList.toggle("segment--actif", b.dataset.basses === bassesJouees);
      }
      majAccordage(present);
      if (!present) $("muscriptor-conditions").hidden = true;
    }
    if (!present) continue;
    const tr = transcription(instrument);
    const affichee = edition.usage === "pratiquer" && instrumentPratique() === instrument;
    const etat = ligne.querySelector(".partition-ligne__etat");
    const action = ligne.querySelector(".partition-ligne__action");
    action.hidden = false;
    action.disabled = false;
    etat.title = "";
    if (tr?.pret) {
      etat.textContent = affichee ? "affichée" : "prête";
      action.textContent = "Afficher";
      action.hidden = affichee;
    } else if (tr?.erreur) {
      etat.textContent = "échec";
      etat.title = tr.erreur;
      action.textContent = "Réessayer";
    } else if (tr) {
      etat.textContent = tr.avancement ?? "calcul…";
      action.hidden = true;
      jauge.hidden = false;
      if (tr.fraction !== undefined) jauge.value = tr.fraction;
      else jauge.removeAttribute("value");
    } else {
      // Le bouton « Calculer » dit déjà l'état : la ligne garde la durée
      // (basse) et tient sur une ligne dans le rail (V6).
      etat.textContent = instrument === "bass" ? "≈ 1 min" : "";
      etat.title = instrument === "bass" ? "À calculer : environ une minute" : "";
      action.textContent = "Calculer";
    }
  }
}

for (const ligne of document.querySelectorAll("#bloc-partitions [data-partition]")) {
  ligne.querySelector(".partition-ligne__action").addEventListener("click", async () => {
    const instrument = ligne.dataset.partition;
    if (transcription(instrument)?.pret) {
      // Afficher : la partition se lit en Pratiquer.
      edition.usage = "pratiquer";
      edition.instrument = instrument;
      varianteParDefaut();
      majUsage();
      return;
    }
    await calculerPartition(instrument);
  });
}

/// Calcule la partition d'un instrument (ou la recalcule après un échec) :
/// le bouton « Calculer » du bloc Partitions, et celui de la barre du coach
/// (V34).
async function calculerPartition(instrument) {
  const tr = transcription(instrument);
  // La basse passe par MuScriptor : conditions acceptées et poids présents,
  // sinon le panneau qui les règle — il lance le calcul une fois prêt.
  if (instrument === "bass" && !(await muscriptorPret())) return;
  if (tr?.erreur) transcriptions.delete(cleTranscription(edition.source?.id, instrument, instrument === "bass"));
  transcription(instrument, true);
}

/// Le menu de l'accordage de la tablature (V37) : l'accordage choisi à la
/// main, sinon « déduit des notes », qui dit celui que le moteur a trouvé.
function majAccordage(present) {
  const ligne = $("accordage-ligne");
  const tr = present ? transcription("bass") : null;
  ligne.hidden = !present || !tr?.pret;
  if (ligne.hidden) return;
  const menu = $("accordage-basse");
  const deduit = tr.pret.choix ? "déduit des notes" : `déduit : ${nommer(tr.pret.accordage)}`;
  const options = [["", deduit], ...CHOIX];
  if (menu.options.length !== options.length || menu.options[0].textContent !== deduit) {
    menu.replaceChildren(...options.map(([v, t]) => new Option(t, v)));
  }
  menu.value = tr.pret.choix ?? "";
}

/// La basse de celui qui joue (`tablature::Basses`) : « quatre » ou
/// « cinq » cordes, dans l'accordage du disque.
let bassesJouees = (() => {
  try {
    return localStorage.getItem("basses") === "cinq" ? "cinq" : "quatre";
  } catch {
    return "quatre";
  }
})();

/// Avancement d'une transcription MuScriptor (une minute environ) : les
/// segments de 5 s faits, dans l'aide de la partition.
function suivreTranscription(id, entree) {
  const minuteur = setInterval(async () => {
    if (entree.pret || entree.erreur) return clearInterval(minuteur);
    try {
      const e = await invoke("etat_transcription");
      if (e.id === id && e.total) {
        entree.fraction = e.faits / e.total;
        entree.avancement = `${Math.round(100 * entree.fraction)} %`;
        if (edition.usage === "pratiquer" && edition.source?.id === id && !$("partition-aide").hidden) {
          $("partition-aide").textContent = `Transcription de qualité (MuScriptor) en cours… ${entree.avancement}`;
          $("partition-jauge").value = entree.fraction;
        }
        majBlocPartitions();
      }
    } catch {}
  }, 1000);
}

/// L'instrument dont on lit la partie : celui choisi dans le rail, s'il a
/// un stem ; sinon l'autre ; sinon aucun.
function instrumentPratique() {
  const present = (n) => edition.stems.some((s) => s.nom === n);
  if (present(edition.instrument)) return edition.instrument;
  return ["bass", "drums"].find(present) ?? null;
}

/// La position du moteur, extrapolée depuis le dernier sondage.
function positionMoteur() {
  if (lecture.stems.enPause || performance.now() < edition.decompteJusqua) {
    return lecture.stems.positionMs;
  }
  return lecture.stems.positionMs + (performance.now() - lecture.stems.lueA) * edition.vitesse;
}

/// La position qu'on entend : celle du moteur moins la latence de la sortie
/// — sans quoi le curseur de la partition court devant la musique (240 ms
/// avec le Mustang Micro, 29 oct.). Jamais avant l'endroit d'où la lecture
/// est repartie ; à l'arrêt, là où elle reprendra.
function positionEstimee() {
  const moteur = positionMoteur();
  if (lecture.stems.enPause) return moteur;
  return Math.max(moteur - lecture.stems.latenceMs * edition.vitesse, lecture.stems.ancreMs);
}

/// La position donnée à alphaTab, lissée : elle avance à la vitesse de
/// lecture et ne se recale que d'une fraction de l'écart à chaque tick.
/// L'estimation brute se corrige d'un coup à chaque relevé du moteur (5 par
/// seconde) — de petits sauts que le curseur rendait visibles. Un vrai
/// déplacement (plus de 250 ms d'écart) se suit sans lissage.
const lissage = { sortie: null, a: 0 };
function positionLissee() {
  const cible = positionEstimee();
  const maintenant = performance.now();
  if (lissage.sortie === null || lecture.stems.enPause || Math.abs(cible - lissage.sortie) > 250) {
    lissage.sortie = cible;
    lissage.a = maintenant;
    return cible;
  }
  const prevue = lissage.sortie + (maintenant - lissage.a) * edition.vitesse;
  lissage.a = maintenant;
  lissage.sortie = prevue + 0.15 * (cible - prevue);
  return lissage.sortie;
}

/// Une seule instance d'alphaTab, même quand deux affichages se croisent
/// (l'usage, puis les sections qui arrivent) : sans la promesse partagée,
/// chacun créait la sienne dans `#partition-rendu`, d'où deux curseurs — un
/// figé sur la première mesure — et une batterie recouverte (15 oct.).
function preparerPartition() {
  partition.prete ??= creerPartition().catch((e) => {
    partition.prete = null;
    throw e;
  });
  return partition.prete;
}

async function creerPartition() {
  await chargerScript("vendor/alphatab/alphaTab.min.js");
  // `#partition` défile ; alphaTab dessine dans `#partition-rendu`, à côté du
  // paragraphe d'aide — jamais d'`innerHTML` sur ce qu'il possède.
  const hote = $("partition");
  const api = new alphaTab.AlphaTabApi($("partition-rendu"), {
    // Sans worker : il ne démarre pas hors d'un serveur (essai du 9 oct.),
    // et le rendu sur le fil principal prend ~60 ms pour 130 mesures.
    // Sans rendu paresseux : il ne dessine que ce qu'il croit visible, et se
    // trompe quand c'est nous qui faisons défiler le conteneur — d'où des
    // pans de partition blancs (le « panneau blanc » de l'essai du 9 oct.,
    // reproduit avec la batterie de « Love Foolosophy »).
    // Les cadres des têtes de notes : le coach (E4) entoure celle qu'il juge.
    core: { fontDirectory: "vendor/alphatab/font/", useWorkers: false, enableLazyLoading: false, includeNoteBounds: true },
    // La seconde voix (les pieds, en batterie) en noir comme la première :
    // alphaTab la grise par défaut. Pas de nuance « f » par défaut.
    // Mise en page « du modèle » : chaque mesure prend la même largeur
    // (`displayScale` 1 partout), MESURES_PAR_LIGNE par ligne — voir
    // `mesuresEgales`.
    display: {
      layoutMode: "page",
      systemsLayoutMode: "useModelLayout",
      scale: 0.9,
      resources: { secondaryGlyphColor: "#000000" },
    },
    notation: { elements: { effectDynamics: false } },
    player: {
      playerMode: "enabledExternalMedia",
      enableCursor: true,
      // Le curseur de temps est le nôtre (`placerCurseur`) ; celui d'alphaTab
      // sautait de note en note.
      enableAnimatedBeatCursor: false,
      enableUserInteraction: true,
      // Le défilement est fait ici (`suivreCurseur`) : avec un lecteur
      // externe, celui d'alphaTab ne suit pas la lecture.
      scrollMode: "off",
    },
  });
  // Ce qu'alphaTab demande, le moteur le fait. La vitesse et le volume
  // restent ceux de la barre d'outils : la partition ne les règle pas.
  const commandes = {
    get backingTrackDuration() {
      return lecture.stems.dureeMs;
    },
    get playbackRate() {
      return edition.vitesse || 1;
    },
    set playbackRate(_) {},
    get masterVolume() {
      return 1;
    },
    set masterVolume(_) {},
    seekTo(ms) {
      if (!lecture.stems.charges || performance.now() - partition.gesteA > 1500) return;
      invoke("stems_transport", { action: "deplacer", position: ms / 1000 }).catch(console.warn);
    },
    // alphaTab arrête son lecteur à chaque partition chargée : sans le
    // garde-fou du geste, changer d'instrument mettait les stems en pause.
    play() {
      if (!lecture.stems.charges || performance.now() - partition.gesteA > 1500) return;
      invoke("stems_transport", { action: "reprendre", position: null }).catch(console.warn);
    },
    pause() {
      if (!lecture.stems.charges || performance.now() - partition.gesteA > 1500) return;
      invoke("stems_transport", { action: "pause", position: null }).catch(console.warn);
    },
  };
  const brancher = () => {
    if (api.player?.output) api.player.output.handler = commandes;
  };
  brancher();
  api.playerReady.on(brancher);
  api.error.on((e) => journal("partition", `alphaTab : ${e?.stack ?? e?.message ?? e}`, "error"));
  hote.addEventListener("pointerdown", () => {
    partition.gesteA = performance.now();
  });
  hote.addEventListener("wheel", () => {
    partition.moletteA = performance.now();
  }, { passive: true });
  partition.api = api;
  brancherCorrection(api);
  brancherCoach(api);
  return api;
}

/// Ramène la ligne jouée dans le haut de la vue quand elle sort de la zone
/// lisible — sauf juste après un défilement à la main.
///
/// La mesure jouée se déduit de notre pulsation (dernier premier temps passé),
/// sa place verticale de la table des positions d'alphaTab : pas besoin de lire
/// l'élément curseur, que le lecteur externe ne déplace qu'à son rythme.
function suivreCurseur() {
  const hote = $("partition");
  const P = edition.pulsation?.premiers_temps;
  const table = partition.api?.boundsLookup ?? partition.api?.renderer?.boundsLookup;
  const maintenant = performance.now();
  if (!P || !table || maintenant - partition.moletteA < 3000 || maintenant - partition.defileA < 600) return;
  const t = positionEstimee() / 1000;
  let i = 0;
  while (i + 1 < P.length && P[i + 1] <= t) i++;
  const b = table.findMasterBarByIndex(i)?.lineAlignedBounds;
  const surface = hote.querySelector(".at-surface");
  if (!b || !surface) return;
  // Par rectangles plutôt qu'`offsetTop` : la surface est dans
  // `#partition-rendu`, lui-même sous le paragraphe d'aide.
  const haut = surface.getBoundingClientRect().top - hote.getBoundingClientRect().top + hote.scrollTop + b.y;
  const bas = haut + b.h;
  const vue = hote.scrollTop;
  if (haut < vue || bas > vue + hote.clientHeight * 0.85) {
    partition.defileA = maintenant;
    hote.scrollTo({ top: Math.max(0, haut - hote.clientHeight * 0.15), behavior: "smooth" });
  }
}

/// Le curseur de temps : dans la mesure jouée, il glisse du bord gauche au
/// bord droit au prorata du temps écoulé entre ses deux premiers temps. Les
/// mesures ayant toutes la même largeur (`mesuresEgales`), il avance à vitesse
/// constante tant que le tempo l'est — le repère du musicien (24 oct.).
function placerCurseur() {
  partition.image = requestAnimationFrame(placerCurseur);
  const P = edition.pulsation?.premiers_temps;
  const table = partition.api?.boundsLookup ?? partition.api?.renderer?.boundsLookup;
  const rendu = $("partition-rendu");
  const surface = rendu.querySelector(".at-surface");
  let c = partition.curseur;
  if (!P?.length || !table || !surface || edition.usage !== "pratiquer" || !enStems()) {
    if (c) c.hidden = true;
    return;
  }
  if (!c?.isConnected) {
    c = partition.curseur = document.createElement("div");
    c.className = "partition__curseur";
    rendu.append(c);
  }
  const t = positionLissee() / 1000;
  let i = 0;
  while (i + 1 < P.length && P[i + 1] <= t) i++;
  // La dernière mesure n'a pas de premier temps suivant : la durée moyenne.
  const duree = i + 1 < P.length ? P[i + 1] - P[i] : P.length > 1 ? (P[P.length - 1] - P[0]) / (P.length - 1) : 2;
  const f = Math.min(1, Math.max(0, (t - P[i]) / duree));
  const m = table.findMasterBarByIndex(i);
  const x = m?.realBounds;
  const y = m?.lineAlignedBounds ?? m?.visualBounds;
  if (!x || !y) {
    c.hidden = true;
    return;
  }
  const s = surface.getBoundingClientRect();
  const r = rendu.getBoundingClientRect();
  c.hidden = false;
  c.style.height = `${y.h}px`;
  c.style.transform = `translate(${s.left - r.left + x.x + f * x.w}px, ${s.top - r.top + y.y}px)`;
}

async function montrerPartition() {
  const hote = $("partition");
  const p = edition.pulsation;
  const instrument = instrumentPratique();
  const tr = p && instrument ? transcription(instrument) : null;
  const message = !p
    ? `Pulsation en cours de calcul… La partition se pose sur les mesures du morceau. ${aidePartie()}`
    : !instrument
      ? "Ce morceau n'a ni basse ni batterie séparées."
      : null;
  // L'état vide du centre porte l'aide de la partie (Règle 12 : pas d'aide de
  // plus d'une ligne dans le rail).
  const aCalculer =
    !message && !tr
      ? `Partition pas encore calculée : « Calculer » dans le bloc Partitions du rail. En attendant, la grille des mesures. ${aidePartie()}`
      : null;
  // Pendant la transcription, ou sans elle (batterie), la grille mesurée.
  const note =
    tr && !tr.pret
      ? tr.erreur
        ? `Transcription impossible : ${tr.erreur}`
        : tr.qualite
          ? `Transcription de qualité (MuScriptor) en cours… ${tr.avancement ?? ""}`
          : "Transcription en cours…"
      : null;
  $("partition-aide").hidden = !(message ?? note ?? aCalculer);
  $("partition-aide").textContent = message ?? note ?? aCalculer ?? "";
  // La jauge pendant le calcul : MuScriptor dit où il en est (segments de
  // 5 s), Basic Pitch et ADTOF non — jauge indéterminée, quelques secondes.
  const jauge = $("partition-jauge");
  jauge.hidden = !(!message && tr && !tr.pret && !tr.erreur);
  if (tr?.fraction !== undefined) jauge.value = tr.fraction;
  else jauge.removeAttribute("value");
  $("partition-rendu").hidden = !!message;
  if (message) return;
  try {
    const api = await preparerPartition();
    // Les basses en font partie : revenir à une basse déjà calculée rend
    // une transcription prête d'emblée, sans passer par la grille — sans
    // elles, la clé ne changeait pas et la tablature restait l'ancienne.
    const basses = instrument === "bass" ? bassesJouees : "";
    // La révision change à chaque correction (E2) : la partie se redessine.
    // V37 : l'accordage en fait partie ; la hauteur des stems, non.
    const accord = instrument === "bass" ? (tr?.pret?.accordage ?? []).join(",") : "";
    const cle = `${edition.source?.id ?? ""}:${instrument}:${tr?.pret ? `transcrit${tr.revision ?? 0}` : "grille"}:${edition.notation}:${tr?.qualite ? "qualite" : "rapide"}:${basses}:${edition.sections?.length ?? 0}:${accord}`;
    if (partition.source !== cle) {
      partition.source = cle;
      const titre = txt(edition.source?.title, "?");
      partition.temps = null;
      partition.batterie = null;
      const tex = !tr?.pret
        ? texGrille(p, titre, instrument)
        : instrument === "drums"
          ? texBatterie(tr.pret.mesures, titre, p, tr.pret.coups)
          : texTablature(tr.pret.mesures, titre, p, tr.pret.accordage, tr.pret.modele);
      await partition.chargement;
      // Un appel plus récent a pris la main pendant l'attente : c'est lui
      // qui charge.
      if (partition.source === cle) {
        const corrigee = instrument === "bass" && tr?.pret ? tr.pret : null;
        partition.chargement = chargerPartition(api, tex, cle, corrigee);
      }
    }
  } catch (e) {
    $("partition-aide").hidden = false;
    $("partition-aide").textContent = `La partition n'a pas pu s'afficher : ${String(e)}`;
    $("partition-jauge").hidden = true;
    journal("partition", e?.stack ?? e, "warn");
  }
  majCorrection();
  majCoach();
  cancelAnimationFrame(partition.image);
  partition.image = requestAnimationFrame(placerCurseur);
  // 50 ms : la cadence que recommande alphaTab pour un lecteur externe.
  clearInterval(partition.minuteur);
  partition.minuteur = setInterval(() => {
    const out = partition.api?.player?.output;
    if (!out || edition.usage !== "pratiquer" || !enStems()) return;
    // alphaTab anime le curseur d'une note à la suivante sur la durée de la
    // note à *sa* vitesse : restée à 1 quand on ralentit les stems, le curseur
    // arrivait trop tôt puis attendait — en avance et par à-coups (24 oct.).
    const vitesse = edition.vitesse || 1;
    if (partition.api.playbackSpeed !== vitesse) partition.api.playbackSpeed = vitesse;
    out.updatePosition(positionLissee());
    suivreCurseur();
  }, 50);
}

/// Charge un alphaTex et attend la fin de son rendu (5 s au plus : un rendu
/// qui ne finit pas ne doit pas bloquer les suivants).
function chargerPartition(api, tex, cle, transcription = null) {
  return new Promise((fini) => {
    const t0 = performance.now();
    let off = null;
    const fin = (quoi) => {
      off?.();
      clearTimeout(minuteur);
      if (quoi !== "rendu") journal("partition", `${quoi} après ${Math.round(performance.now() - t0)} ms : ${cle}`, "warn");
      fini();
    };
    const minuteur = setTimeout(() => fin("délai dépassé"), 5000);
    off = api.renderFinished.on(() => fin("rendu"));
    try {
      // Par l'importeur plutôt qu'`api.tex` : la partition passe par
      // `silencesInvisibles` avant le rendu.
      const imp = new alphaTab.importer.AlphaTexImporter();
      imp.initFromString(tex, api.settings);
      const score = imp.readScore();
      silencesInvisibles(score);
      mesuresEgales(score);
      if (transcription) marquerCorrigees(score, transcription, partition.temps);
      api.renderScore(score);
    } catch (e) {
      journal("partition", e?.stack ?? e, "error");
      fin("échec");
    }
  });
}

/// Autant de mesures par ligne, toutes de la même largeur : le musicien lit
/// le temps à la place du curseur, qui avance alors à vitesse constante
/// (demande du 24 oct.). alphaTab, par défaut, élargit les mesures chargées
/// en notes — le curseur ralentissait dans les doubles croches et filait
/// dans les rondes.
const MESURES_PAR_LIGNE = 4;
function mesuresEgales(score) {
  score.defaultSystemsLayout = MESURES_PAR_LIGNE;
  score.systemsLayout = [];
  for (const t of score.tracks) {
    t.defaultSystemsLayout = MESURES_PAR_LIGNE;
    t.systemsLayout = [];
  }
}

/// Les silences des voix secondaires (les pieds, en batterie) restent dans le
/// temps mais ne se dessinent pas, comme dans une partition de batteur :
/// alphaTab les posait au-dessus de la portée, au milieu du charleston.
function silencesInvisibles(score) {
  for (const piste of score.tracks)
    for (const portee of piste.staves)
      for (const mesure of portee.bars)
        for (const voix of mesure.voices)
          if (voix.index > 0) for (const b of voix.beats) if (b.isRest) b.isEmpty = true;
}

function cacherPartition() {
  majCorrection();
  majCoach();
  clearInterval(partition.minuteur);
  partition.minuteur = null;
  cancelAnimationFrame(partition.image);
  partition.image = null;
}

/// La recherche Songsterr du morceau ouvert, dans le navigateur. Câblée une
/// fois : posée dans `majUsage`, elle s'ajoutait à chaque appel et un clic
/// ouvrait autant d'onglets.
// Une transposition appliquée rend la nouvelle hauteur au coach de basse
// (`majCoach`, V35) et au bandeau ; la tablature, elle, ne bouge pas (V37).
document.addEventListener("hauteur-appliquee", () => {
  if (edition.usage === "pratiquer" && enStems()) montrerPartition();
});

$("songsterr").addEventListener("click", () => {
  const src = edition.source;
  if (!src?.title) return;
  const motif = [src.artist, src.title].filter(Boolean).join(" ");
  ouvrirLien(`https://www.songsterr.com/?pattern=${encodeURIComponent(motif)}`);
});

/// L'aide de la partie, dans l'état vide de la partition (`montrerPartition`).
function aidePartie() {
  return (
    "Sa partie s'affiche ici, calée sur la lecture. S et M règlent l'écoute : l'écouter, la jouer seul par-dessus." +
    (edition.decompte ? " ▶ fait d’abord le décompte au tempo du morceau, puis repart du premier temps de la mesure en cours." : "")
  );
}

/// Range le centre selon l'usage. Appelée par `majEtatEditer`.
function majUsage() {
  const etabli = $("editer-etabli");
  const pratiquer = edition.usage === "pratiquer" && !etabli.hidden;
  // L'usage est le mode du rail (Pratiquer ou Créer) : le bloc ne porte plus
  // que les réglages de la partie, qui n'ont de sens qu'en Pratiquer (la
  // table des écrans ne le met pas dans Créer).
  demander("bloc-usage", !etabli.hidden);
  const instrument = instrumentPratique();
  $("instruments").hidden = edition.usage !== "pratiquer";
  for (const b of document.querySelectorAll("#instruments [data-instrument]")) {
    b.classList.toggle("segment--actif", b.dataset.instrument === instrument);
    b.disabled = !edition.stems.some((s) => s.nom === b.dataset.instrument);
  }
  $("notations").hidden = edition.usage !== "pratiquer" || instrument !== "bass";
  $("decomptes").hidden = edition.usage !== "pratiquer";
  $("songsterr").hidden = edition.usage !== "pratiquer" || !edition.source?.title;
  for (const b of document.querySelectorAll("#decomptes [data-decompte]")) {
    b.classList.toggle("segment--actif", Number(b.dataset.decompte) === edition.decompte);
  }
  for (const b of document.querySelectorAll("#notations [data-notation]")) {
    b.classList.toggle("segment--actif", b.dataset.notation === edition.notation);
  }
  etabli.classList.toggle("etabli--pratiquer", pratiquer);
  $("partition").hidden = !pratiquer;
  if (pratiquer) montrerPartition();
  else cacherPartition();
  majBlocPartitions();
  positionnerPlayhead();
  // La bande, et ce que seul Pratiquer montre de la boucle et de la vitesse
  // (nom, liste, vitesse atteinte).
  majBoucleUI();
}

for (const b of document.querySelectorAll("#instruments [data-instrument]")) {
  b.addEventListener("click", () => {
    edition.instrument = b.dataset.instrument;
    majUsage();
  });
}

for (const b of document.querySelectorAll("#notations [data-notation]")) {
  b.addEventListener("click", () => {
    edition.notation = b.dataset.notation;
    try {
      localStorage.setItem("notation-basse", edition.notation);
    } catch {}
    majUsage();
  });
}

/* ------------------------------------------------ pratiquer : décompte */

// Mesures de clics avant que la musique parte (0, 1 ou 2), en Pratiquer :
// même règle que la notation, le navigateur pour un morceau neuf, la mémoire
// de Pratiquer pour un morceau déjà travaillé.
edition.decompte = (() => {
  try {
    const n = Number(localStorage.getItem("decompte-mesures") ?? 1);
    return [0, 1, 2].includes(n) ? n : 1;
  } catch {
    return 1;
  }
})();

/// Le décompte pour repartir de `posS` (secondes du morceau) : on repart du
/// premier temps de la mesure en cours, et les clics tombent sur les temps
/// qui le précèdent, au tempo local de la pulsation — le dernier un temps
/// avant. Avant le premier temps du morceau (une levée), on part de `posS`
/// et les clics comptent vers ce premier temps : ceux qui tombent dans la
/// levée sonnent par-dessus la musique. `null` sans pulsation.
function planDecompte(posS, mesures = edition.decompte) {
  const p = edition.pulsation;
  if (!p || !mesures) return null;
  const P = p.premiers_temps;
  const T = p.temps ?? [];
  let debut = posS;
  for (const x of P) if (x <= posS + 0.05) debut = x;
  const cible = P.find((x) => x >= debut - 0.01);
  if (cible === undefined) return null;
  // Période : médiane des quatre temps qui suivent la cible.
  const j = T.findIndex((t) => t >= cible - 0.01);
  const ecarts = j < 0 ? [] : T.slice(j, j + 5).slice(1).map((t, k) => t - T[j + k]).sort((a, b) => a - b);
  const periode = ecarts.length ? ecarts[ecarts.length >> 1] : 60 / tempoDe(p);
  const m = p.metrique || 4;
  const n = mesures * m;
  const clics = [];
  for (let k = n; k >= 1; k--) clics.push([cible - k * periode, (n - k) % m === 0]);
  return { debut, clics };
}

/// ▶ en Pratiquer : décompte, puis la musique. Rend vrai si le moteur l'a pris.
async function lancerDecompte() {
  if (edition.usage !== "pratiquer" || $("partition").hidden) return false;
  // Une calibration du coach demandée (E4) : quatre mesures de clics, sur
  // lesquels on tape, quel que soit le décompte réglé.
  const calibrage = calibrageDemande();
  const plan = planDecompte(lecture.stems.positionMs / 1000, calibrage ? 4 : edition.decompte);
  if (!plan) return false;
  const attente = Math.max(0, plan.debut - plan.clics[0][0]) / (edition.vitesse || 1);
  try {
    await invoke("stems_decompter", plan);
  } catch (e) {
    console.warn("décompte", e);
    return false;
  }
  lecture.stems.positionMs = plan.debut * 1000;
  lecture.stems.ancreMs = lecture.stems.positionMs;
  lecture.stems.lueA = performance.now();
  edition.decompteJusqua = performance.now() + attente * 1000;
  apresDecompte(plan, attente * 1000);
  return true;
}

for (const b of document.querySelectorAll("#decomptes [data-decompte]")) {
  b.addEventListener("click", () => {
    edition.decompte = Number(b.dataset.decompte);
    try {
      localStorage.setItem("decompte-mesures", String(edition.decompte));
    } catch {}
    majUsage();
  });
}


/// MuScriptor est-il prêt pour la basse ? Ses conditions d'usage montrées la
/// première fois, puis vérification que les poids sont là. Sinon,
/// téléchargement depuis Hugging Face avec le jeton de l'utilisateur (le dépôt
/// est à accès restreint : il faut y avoir accepté ces conditions avec son
/// compte). Plus de choix « Rapide » pour la basse depuis le 29 oct. : la
/// qualité prime, Basic Pitch ne sert plus qu'au banc.
async function muscriptorPret() {
  let accepte = false;
  try {
    accepte = localStorage.getItem("muscriptor-conditions") === "1";
  } catch {}
  if (!accepte) {
    $("muscriptor-accepter").hidden = false;
    $("muscriptor-telecharger").hidden = true;
    $("muscriptor-jeton-ligne").hidden = true;
    $("muscriptor-lien-jeton").hidden = true;
    $("muscriptor-conditions").hidden = false;
    return false;
  }
  if (!(await invoke("muscriptor_disponible").catch(() => false))) {
    const jeton = await invoke("muscriptor_jeton_present").catch(() => false);
    $("muscriptor-texte").textContent =
      "Poids de MuScriptor absents (1,2 Go). Acceptez d'abord leurs conditions avec votre compte sur " +
      "la page du modèle, puis téléchargez-les ici" +
      (jeton ? " (jeton Hugging Face de cette machine)." : " avec un jeton d'accès en lecture.");
    $("muscriptor-accepter").hidden = true;
    $("muscriptor-jeton-ligne").hidden = jeton;
    $("muscriptor-lien-jeton").hidden = jeton;
    $("muscriptor-telecharger").hidden = false;
    $("muscriptor-telecharger").disabled = false;
    $("muscriptor-conditions").hidden = false;
    return false;
  }
  $("muscriptor-conditions").hidden = true;
  return true;
}

/// Après l'acceptation ou le téléchargement : si MuScriptor est prêt, la
/// partition de basse qu'on avait demandée se calcule.
async function lancerSiPret() {
  if (await muscriptorPret()) transcription("bass", true);
}

/// Télécharge les poids, avancement dans le texte du panneau ; le jeton saisi
/// part avec la requête et n'est gardé nulle part.
async function telechargerMuscriptor() {
  const bouton = $("muscriptor-telecharger");
  const jauge = $("muscriptor-jauge");
  bouton.disabled = true;
  jauge.removeAttribute("value");
  jauge.hidden = false;
  const minuteur = setInterval(async () => {
    try {
      const e = await invoke("etat_transcription");
      if (e.telechargement && e.octets_total) {
        jauge.value = e.octets_faits / e.octets_total;
        $("muscriptor-texte").textContent =
          `Téléchargement des poids de MuScriptor… ${Math.round((100 * e.octets_faits) / e.octets_total)} %`;
      }
    } catch {}
  }, 500);
  try {
    await invoke("telecharger_muscriptor", { jeton: $("muscriptor-jeton").value || null });
    $("muscriptor-jeton").value = "";
    clearInterval(minuteur);
    jauge.hidden = true;
    lancerSiPret();
  } catch (e) {
    clearInterval(minuteur);
    jauge.hidden = true;
    $("muscriptor-texte").textContent = `Échec du téléchargement : ${e}`;
    bouton.disabled = false;
  }
}

document.querySelectorAll("#muscriptor-conditions [data-url]").forEach((b) =>
  b.addEventListener("click", () => ouvrirLien(b.dataset.url)));

for (const b of document.querySelectorAll("#basses [data-basses]")) {
  b.addEventListener("click", () => {
    bassesJouees = b.dataset.basses;
    try {
      localStorage.setItem("basses", bassesJouees);
    } catch {}
    majUsage();
  });
}
$("accordage-basse").addEventListener("change", async (e) => {
  const id = edition.source?.id;
  if (id === undefined) return;
  const valeur = e.target.value || null;
  try {
    await invoke("pratique_accordage", { id, accordage: valeur });
  } catch (err) {
    journal("partition", `accordage : ${err}`, "warn");
    return;
  }
  // Le moteur repose le doigté sans retranscrire : la basse de ce morceau
  // est redemandée.
  for (const cle of [...transcriptions.keys()]) if (cle.startsWith(`${id}:bass:`)) transcriptions.delete(cle);
  majUsage();
});
$("muscriptor-accepter").addEventListener("click", () => {
  try {
    localStorage.setItem("muscriptor-conditions", "1");
  } catch {}
  lancerSiPret();
});
$("muscriptor-telecharger").addEventListener("click", telechargerMuscriptor);
$("muscriptor-annuler").addEventListener("click", () => {
  $("muscriptor-conditions").hidden = true;
});

/// La transcription affichée au centre, si c'est une basse prête (E2) :
/// l'entrée de `transcriptions` (`pret`, `qualite`, `revision`).
/// La batterie transcrite, si c'est elle que Pratiquer affiche.
function batterieAffichee() {
  if (edition.usage !== "pratiquer" || instrumentPratique() !== "drums" || $("partition").hidden) return null;
  const tr = transcription("drums");
  return tr?.pret ? tr : null;
}

function basseAffichee() {
  if (edition.usage !== "pratiquer" || instrumentPratique() !== "bass" || $("partition").hidden) return null;
  const tr = transcription("bass");
  return tr?.pret ? tr : null;
}

/// La basse ou la batterie que Pratiquer montre sans partition prête (pas
/// calculée, en calcul, ou en échec) : la barre du coach offre de la
/// calculer (V34). `tr` est l'entrée de `transcriptions`, ou null.
function partitionAttendue() {
  if (edition.usage !== "pratiquer" || $("partition").hidden || !edition.pulsation) return null;
  const instrument = instrumentPratique();
  if (instrument !== "bass" && instrument !== "drums") return null;
  const tr = transcription(instrument);
  return tr?.pret ? null : { instrument, tr };
}

/// Remplace la basse transcrite par sa version corrigée (rendue par
/// `corriger_transcription`) et redessine la partie.
function remplacerBasse(entree, vue) {
  entree.pret = vue;
  entree.revision = (entree.revision ?? 0) + 1;
  if (edition.usage === "pratiquer") montrerPartition();
}

export {
  basseAffichee, bassesJouees, batterieAffichee, cacherPartition, calculerPartition, lancerDecompte, majUsage,
  montrerPartition, oublierPartitions, partition, partitionAttendue, positionMoteur, remplacerBasse,
};
