import "./editer.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, enStems, fileAttente, invoke, journal, navigation, ouvrirLien, remonter, signalerErreur, txt,
  vue, vueEnGrille,
} from "./socle.js";
import {
  dessiner, dessinerGrille,
} from "./vues.js";

import {
  pisteSelectionnee, texteCompte,
} from "./ecouter.js";
import {
  battement, chargerSpectreTransport, peindreTransport, poserImage, poserLecture, sonder,
  teteCourante,
} from "./lecteur.js";
import {
  rampe,
} from "./carte.js";
import {
  indexAlphaUtile, indexAnneesUtile,
} from "./ecouter-familles.js";
import {
  bpmSource, edition, libelleStem, majPulsation, majSections, majTempoSource,
  marquerSectionCourante, NOMS_SECTIONS, octaveVitesse, pasReglage, positionnerBande,
  reposerBoucle, vitesseEnBpm,
} from "./editer.js";
import { lecture, poserStemsCharges, transport } from "./etat-lecture.js";
import { demander } from "./ecrans.js";

/* ------------------------------------------------ éditer : pratiquer */

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
function texTablature(mesures, titre, p, accordage, modele) {
  const guillemets = (x) => String(x).replace(/["\\]/g, " ");
  // Pas de silences multimesures : regrouper huit mesures de silence en une
  // casserait la largeur égale des mesures, et le curseur y filerait.
  // L'accordage vient du moteur (standard, drop D, cinq cordes), de la corde
  // aiguë à la grave.
  const cordes = accordage?.length ? [...accordage].reverse() : [43, 38, 33, 28];
  const portee = notationBasse === "tabs" ? "\\staff {tabs}" : "\\staff {score tabs}";
  const tete =
    `\\title "${guillemets(titre)}" \\subtitle "Transcription automatique (${modele === "muscriptor-medium" ? "MuScriptor" : "Basic Pitch"}) — à vérifier à l'oreille"` +
    ` \\tempo ${tempoDe(p)}\n` +
    `\\track "Basse" ${portee} \\tuning (${cordes.map(nomMidi).join(" ")})\n` +
    (notationBasse === "tabs" ? "" : "\\clef F4\n");
  let precedente = 0;
  const sections = marquesSections(mesures.map((m) => m.debut_s));
  const corps = mesures.map((m, i) => {
    let t = sections[i] + (m.temps !== precedente ? `\\ts (${m.temps} 4) ` : "");
    precedente = m.temps;
    const fin = mesures[i + 1]?.debut_s ?? Infinity;
    t += syncTemps(i, m.debut_s, fin, p.temps);
    const temps = [];
    for (const e of legato(m.evenements)) {
      const durees = decouperDuree(e.seiziemes);
      const jeu = e.jeu;
      if (!jeu || jeu.corde === null || jeu.frette === null) {
        for (const d of durees) temps.push(`r.${d}`);
        continue;
      }
      const corde = cordes.length - jeu.corde;
      durees.forEach((d, k) => temps.push(e.lie || k > 0 ? `-.${corde}.${d}` : `${jeu.frette}.${corde}.${d}`));
    }
    return t + (temps.length ? temps.join(" ") : `r.${m.temps === 4 ? "1" : "4"}`);
  });
  return tete + corps.join(" |\n") + " |";
}

/// Notation de la basse : « score+tabs » (défaut, décidé le 10 oct. —
/// comme les livres « Bass Recorded Versions », la notation en clé de fa
/// au-dessus de la tablature) ou « tabs » (tablature seule). Bascule dans le
/// rail, retenue par le navigateur (commodité, pas un état à partager).
let notationBasse = (() => {
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
function texBatterie(mesures, titre, p) {
  const guillemets = (x) => String(x).replace(/["\\]/g, " ");
  const tete =
    `\\title "${guillemets(titre)}" \\subtitle "Transcription automatique (ADTOF) — à vérifier à l'oreille"` +
    ` \\tempo ${tempoDe(p)}\n\\track "Batterie" \\instrument percussion \\articulation defaults \\staff {score}\n`;
  const sections = marquesSections(mesures.map((m) => m.debut_s));
  const haut = [];
  const bas = [];
  let precedente = 0;
  mesures.forEach((m, i) => {
    // La métrique, les sections et la synchro sont des réglages de mesure :
    // la première voix les porte.
    let t = (i === 0 ? "\\clef neutral " : "") + sections[i];
    t += m.temps !== precedente ? `\\ts (${m.temps} 4) ` : "";
    precedente = m.temps;
    const fin = mesures[i + 1]?.debut_s ?? Infinity;
    t += syncTemps(i, m.debut_s, fin, p.temps);
    haut.push(t + voixBatterie(m, (x) => !PIEDS.has(x)));
    bas.push(voixBatterie(m, (x) => PIEDS.has(x)));
  });
  return tete + "\\voice\n" + haut.join(" |\n") + " |\n\\voice\n" + bas.join(" |\n") + " |";
}

/// Une voix d'une mesure de batterie : les frappes des pièces retenues, puis
/// des silences ; une note dure jusqu'à la frappe suivante de sa voix.
function voixBatterie(m, garder) {
  const total = m.temps * 4;
  const frappes = [];
  let pos = 0;
  for (const f of m.frappes) {
    const pieces = f.pieces.filter(garder);
    if (pieces.length) frappes.push({ pos, pieces });
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
      etat.textContent = instrument === "bass" ? "à calculer (≈ 1 min)" : "à calculer";
      action.textContent = "Calculer";
    }
  }
}

for (const ligne of document.querySelectorAll("#bloc-partitions [data-partition]")) {
  ligne.querySelector(".partition-ligne__action").addEventListener("click", async () => {
    const instrument = ligne.dataset.partition;
    const tr = transcription(instrument);
    if (tr?.pret) {
      // Afficher : la partition se lit en Pratiquer.
      edition.usage = "pratiquer";
      edition.instrument = instrument;
      varianteParDefaut();
      majUsage();
      return;
    }
    // La basse passe par MuScriptor : conditions acceptées et poids présents,
    // sinon le panneau qui les règle — il lance le calcul une fois prêt.
    if (instrument === "bass" && !(await muscriptorPret())) return;
    if (tr?.erreur) transcriptions.delete(cleTranscription(edition.source?.id, instrument, instrument === "bass"));
    transcription(instrument, true);
  });
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
    core: { fontDirectory: "vendor/alphatab/font/", useWorkers: false, enableLazyLoading: false },
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
    const cle = `${edition.source?.id ?? ""}:${instrument}:${tr?.pret ? "transcrit" : "grille"}:${notationBasse}:${tr?.qualite ? "qualite" : "rapide"}:${basses}:${edition.sections?.length ?? 0}`;
    if (partition.source !== cle) {
      partition.source = cle;
      const titre = txt(edition.source?.title, "?");
      const tex = !tr?.pret
        ? texGrille(p, titre, instrument)
        : instrument === "drums"
          ? texBatterie(tr.pret.mesures, titre, p)
          : texTablature(tr.pret.mesures, titre, p, tr.pret.accordage, tr.pret.modele);
      await partition.chargement;
      // Un appel plus récent a pris la main pendant l'attente : c'est lui
      // qui charge.
      if (partition.source === cle) partition.chargement = chargerPartition(api, tex, cle);
    }
  } catch (e) {
    $("partition-aide").hidden = false;
    $("partition-aide").textContent = `La partition n'a pas pu s'afficher : ${String(e)}`;
    $("partition-jauge").hidden = true;
    journal("partition", e?.stack ?? e, "warn");
  }
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
function chargerPartition(api, tex, cle) {
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
  clearInterval(partition.minuteur);
  partition.minuteur = null;
  cancelAnimationFrame(partition.image);
  partition.image = null;
}

/// La recherche Songsterr du morceau ouvert, dans le navigateur. Câblée une
/// fois : posée dans `majUsage`, elle s'ajoutait à chaque appel et un clic
/// ouvrait autant d'onglets.
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
    (decompteMesures ? " ▶ fait d’abord le décompte au tempo du morceau, puis repart du premier temps de la mesure en cours." : "")
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
    b.classList.toggle("segment--actif", Number(b.dataset.decompte) === decompteMesures);
  }
  for (const b of document.querySelectorAll("#notations [data-notation]")) {
    b.classList.toggle("segment--actif", b.dataset.notation === notationBasse);
  }
  etabli.classList.toggle("etabli--pratiquer", pratiquer);
  $("partition").hidden = !pratiquer;
  if (pratiquer) montrerPartition();
  else cacherPartition();
  majBlocPartitions();
  positionnerPlayhead();
  positionnerBande();
}

for (const b of document.querySelectorAll("#instruments [data-instrument]")) {
  b.addEventListener("click", () => {
    edition.instrument = b.dataset.instrument;
    majUsage();
  });
}

for (const b of document.querySelectorAll("#notations [data-notation]")) {
  b.addEventListener("click", () => {
    notationBasse = b.dataset.notation;
    try {
      localStorage.setItem("notation-basse", notationBasse);
    } catch {}
    majUsage();
  });
}

/* ------------------------------------------------ pratiquer : décompte */

// Mesures de clics avant que la musique parte (0, 1 ou 2), en Pratiquer.
let decompteMesures = (() => {
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
function planDecompte(posS) {
  const p = edition.pulsation;
  if (!p || !decompteMesures) return null;
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
  const n = decompteMesures * m;
  const clics = [];
  for (let k = n; k >= 1; k--) clics.push([cible - k * periode, (n - k) % m === 0]);
  return { debut, clics };
}

/// ▶ en Pratiquer : décompte, puis la musique. Rend vrai si le moteur l'a pris.
async function lancerDecompte() {
  if (edition.usage !== "pratiquer" || $("partition").hidden) return false;
  const plan = planDecompte(lecture.stems.positionMs / 1000);
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
  return true;
}

for (const b of document.querySelectorAll("#decomptes [data-decompte]")) {
  b.addEventListener("click", () => {
    decompteMesures = Number(b.dataset.decompte);
    try {
      localStorage.setItem("decompte-mesures", String(decompteMesures));
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
  if (g.mesures > 0) {
    // Mesure par mesure : chaque mesure étirée sur la sienne ; le facteur
    // dit l'étirement typique.
    const pct = Math.round((g.facteur - 1) * 1000) / 10;
    bouts.push(`calé mesure par mesure (${g.mesures} mesures)`);
    if (g.mesures_par_section > 0) {
      bouts.push(g.mesures_par_section === g.mesures ? "section sur section" : `${g.mesures_par_section} section sur section`);
    }
    if (pct !== 0) bouts.push(`étiré d'environ ${pct > 0 ? "+" : ""}${pct} %`);
    if (g.octaves > 0) bouts.push(`${2 ** g.octaves} mesures pour une`);
    if (g.octaves < 0) bouts.push(`une demi-mesure pour une`);
    return bouts.join(" · ");
  }
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
  if (g.repli) bouts.push(`pas mesure par mesure : ${g.repli}`);
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
      oublierPartitions(candidat.id);
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
  appliquerVitesses, arreterStems, cacherPartition, deplacerLecture, dessinerReglages,
  dessinerStems, lancerDecompte, majDerive, majEtatEditer, majUsage, majVariantes,
  montrerPartition, niveaux, oublierStems, poserSourceEdition, poserTete, positionMoteur,
  positionnerPlayhead, prendreLaMain, rampeRGB, rattacherStems, reconcilierStems, REGLAGES,
  resumeStem, stemEcarte, tonaliteDe, varianteParDefaut, vitesseDe,
};
