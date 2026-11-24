import "./corriger.js"; // l'ordre d'exécution : voir main.js
import { $, invoke, journal } from "./socle.js";
import { edition, majBoucleUI, reposerBoucle } from "./etabli.js";
import { appliquerVitesses, dessinerReglages, REGLAGES, transpositionDe } from "./etabli-reglages.js";
import { deplacerLecture } from "./etabli-stems.js";
import { lecture } from "./etat-lecture.js";
import { accordeur, fermerAccordeur, majBoutonAccordeur } from "./accordeur.js";
import { aJouer, dire as direAccordage, nommer } from "./accordage.js";
import { choisirEntree, entreeAOuvrir, garderEntree, montrerEntrees } from "./entree-basse.js";
import { montrerPads, poserProfil } from "./coach-profil.js";
import {
  basseAffichee, batterieAffichee, calculerPartition, montrerPartition, partition, partitionAttendue,
} from "./pratiquer.js";

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
//
// E5 : la même barre sert à la basse en DI (`coach_basse.rs`) quand
// Pratiquer montre la basse transcrite. L'entrée audio remplace la batterie
// MIDI ; le moteur écoute tous les canaux de l'interface et juge celui où
// joue la basse (V34 : « auto », trouvé à la calibration ou dès qu'elle
// joue ; ou choisi à côté de l'interface). Chaque note jouée est jugée en
// temps et en hauteur (juste, fausse, mauvaise octave), seulement si le
// stem de basse confirme la note de la transcription. La calibration : une
// corde étouffée pincée sur les clics du décompte. Ni table par mesure, ni
// escalier, ni groove pour la basse : ils restent à la batterie.
//
// V34 : la barre se montre dès que Pratiquer affiche la basse ou la batterie,
// même sans partition prête ; elle offre alors de la calculer.
//
// V35 : la barre devient un bandeau de couleur d'accent au repos (le geste
// principal en bouton plein), sur papier pendant l'écoute, avec la légende
// du code des erreurs. Chaque verdict a sa couleur **et** son signe (jamais
// la couleur seule) : juste (cercle plein), ← en avance, → en retard, la note
// jouée à côté d'une fausse note, « 8 » à la mauvaise octave, × sur la
// mauvaise pièce, contour pointillé pour une note manquée, + pour une note en
// trop (posé après la note attendue qui la précède). La basse se juge à la
// hauteur qu'on entend : la transcription décalée du réglage de hauteur
// (`transpositionDe("bass")`), comme la tablature affichée.

const coach = {
  actif: false,
  // L'instrument que la barre écoute : « drums » (MIDI) ou « bass » (DI).
  instrument: null,
  peripherique: null,
  latenceMs: null,
  // Basse (V34) : canaux écoutés, canal jugé (depuis 0), choisi à la main,
  // et le canal où la basse semble jouer quand le choisi reste muet.
  canaux: 0,
  canal: 0,
  manuel: false,
  ailleurs: null,
  // La partition attendue (instrument affiché sans partition prête), ou null.
  attente: null,
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
  // Le groove (E5) : le dernier conseil dit, et les mesures relevées depuis
  // la dernière demande.
  conseil: null,
  relevesDepuis: 0,
  // Basse (V35) : la transposition que le moteur juge, en demi-tons.
  demiTons: 0,
  // Les notes en trop, par rang de la note attendue qui les précède.
  enTrop: new Map(),
  // La dernière note attendue jugée (son rang), d'un sondage à l'autre.
  derniere: null,
};

/// Mesures relevées entre deux demandes du groove hors boucle.
const RELEVES_PAR_CONSEIL = 4;

const ARRETE = { drums: "jouer avec ma batterie", bass: "jouer avec ma basse" };

const TITRES = {
  drums:
    "Écouter la batterie électronique (MIDI) pendant la lecture : chaque note de la partition est entourée — juste, en avance, en retard, manquée. Rien n'est dit des cymbales ni des notes fantômes, que la transcription ne sait pas sûres.",
  bass:
    "Écouter la basse branchée sur l'interface audio pendant la lecture (l'entrée où elle joue se trouve seule, ou se choisit à côté) : chaque note de la partition est entourée — juste, en avance, en retard, manquée, fausse ou à la mauvaise octave. Rien n'est dit des notes que le stem de basse ne confirme pas.",
};

const CALIBRER = {
  drums: "Mesurer la latence de la batterie, de l'ordinateur et du casque : ▶, puis taper la caisse claire sur les seize clics du décompte. Gardée pour cette batterie.",
  bass: "Mesurer la latence de l'interface, de l'ordinateur et du casque : ▶, puis pincer une corde à vide, étouffée, sur les seize clics du décompte. Gardée pour cette entrée.",
};

/// Les commandes du moteur, par instrument.
const COMMANDES = {
  drums: { lister: "coach_peripheriques", ecouter: "coach_ecouter", juger: "coach_juger", calibrer: "coach_calibrer" },
  bass: { lister: "coach_basse_entrees", ecouter: "coach_basse_ecouter", juger: "coach_basse_juger", calibrer: "coach_basse_calibrer" },
};

/// L'argument qui nomme l'entrée écoutée, par instrument.
const ecouterArgs = (instrument, nom) => (instrument === "bass" ? { entree: nom } : { peripherique: nom });

/// La transcription affichée que le coach sait écouter, et son instrument.
function affichee() {
  const b = batterieAffichee();
  if (b) return { tr: b, instrument: "drums" };
  const s = basseAffichee();
  return s ? { tr: s, instrument: "bass" } : { tr: null, instrument: null };
}

/// Le code des erreurs (V35) : une classe (sa couleur, sa forme) et un signe
/// posé à côté de la note, pour ne jamais dire par la couleur seule.
const VERDICTS = {
  parfait: { classe: "juste", dit: "juste", signe: "" },
  bon: { classe: "juste", dit: "juste", signe: "" },
  avance: { classe: "avance", dit: "en avance", signe: "←" },
  retard: { classe: "retard", dit: "en retard", signe: "→" },
  manquee: { classe: "manquee", dit: "manquée", signe: "" },
  mauvaise_piece: { classe: "piece", dit: "mauvaise pièce", signe: "×" },
};

/// La légende du bandeau, par instrument : la marque, son signe, son nom.
const LEGENDE = {
  bass: [
    ["juste", "", "juste"],
    ["avance", "←", "en avance"],
    ["retard", "→", "en retard"],
    ["fausse", "fa", "fausse (note jouée)"],
    ["octave", "8", "octave"],
    ["manquee", "", "manquée"],
    ["trop", "+", "en trop"],
  ],
  drums: [
    ["juste", "", "juste"],
    ["avance", "←", "en avance"],
    ["retard", "→", "en retard"],
    ["piece", "×", "mauvaise pièce"],
    ["manquee", "", "manquée"],
    ["trop", "+", "en trop"],
  ],
};

/// Nom d'une hauteur MIDI, comme la correction l'écrit (« fa♯ »).
const NOMS_NOTES = ["do", "do♯", "ré", "ré♯", "mi", "fa", "fa♯", "sol", "sol♯", "la", "la♯", "si"];
const nomNote = (h) => NOMS_NOTES[((h % 12) + 12) % 12];

const ATTRAPEES = new Set(["parfait", "bon", "avance", "retard"]);

function bilanVide() {
  return { attendues: 0, attrapees: 0, en_trop: 0, ecarts: [], fausses: 0, octaves: 0 };
}

/// La barre du coach : là quand Pratiquer montre la batterie ou la basse —
/// transcrite, on la joue ; pas encore, on la calcule (V34). L'appelle
/// `montrerPartition` (et `cacherPartition`).
function majCoach() {
  const { tr, instrument } = affichee();
  coach.attente = tr ? null : partitionAttendue();
  const barre = $("partition-coach");
  barre.hidden = !tr && !coach.attente;
  if (barre.hidden) majBoutonAccordeur(false);
  if (coach.actif && instrument !== coach.instrument) {
    // Autre instrument affiché (ou plus de partition) : l'écoute se ferme.
    arreter();
  }
  if (!tr) {
    arreterSondage();
    effacerMarques();
    if (coach.attente) {
      barre.setAttribute("aria-label", coach.attente.instrument === "bass" ? "Coach de basse" : "Coach de batterie");
      dire();
    }
    return;
  }
  if (!coach.actif) coach.instrument = instrument;
  barre.setAttribute("aria-label", instrument === "bass" ? "Coach de basse" : "Coach de batterie");
  if (coach.actif && instrument === "bass") suivreTransposition();
  $("coach-basculer").title = TITRES[instrument];
  $("coach-calibrer").title = CALIBRER[instrument];
  $("coach-peripherique").setAttribute("aria-label", instrument === "bass" ? "Entrée audio" : "Batterie MIDI");
  if (coach.actif) armer(tr);
  dire();
}

/// La transposition a changé (V35) : le moteur juge contre la nouvelle
/// référence ; les marques posées sur l'ancienne s'effacent.
async function suivreTransposition() {
  const d = transpositionDe("bass");
  if (d === coach.demiTons || coach.armee === null) return;
  coach.demiTons = d;
  effacerMarques();
  try {
    await invoke("coach_basse_transposer", { demiTons: d });
    annoncer(d ? `Transposé de ${REGLAGES.tonalite.ecrire(d)} : le coach juge ce que vous jouez ; la tablature garde ses frettes, à jouer dans l'accordage indiqué.` : "Hauteur d'origine : le coach juge la partition telle quelle.");
  } catch (e) {
    journal("coach", `transposer : ${e}`, "warn");
  }
}

/// La légende du code des erreurs, dans le bandeau pendant l'écoute.
function dessinerLegende() {
  const legende = $("coach-legende");
  const instrument = coach.instrument ?? "drums";
  legende.hidden = !coach.actif || !!coach.attente;
  if (legende.hidden || legende.dataset.instrument === instrument) return;
  legende.dataset.instrument = instrument;
  legende.replaceChildren(
    ...LEGENDE[instrument].map(([classe, signe, nom]) => {
      const item = document.createElement("span");
      item.className = `coach__cle coach__cle--${classe}`;
      item.setAttribute("role", "listitem");
      const rond = document.createElement("span");
      rond.className = "coach__rond";
      rond.setAttribute("aria-hidden", "true");
      item.append(rond);
      if (signe) {
        const sg = document.createElement("b");
        sg.className = "coach__cle-signe";
        sg.setAttribute("aria-hidden", "true");
        sg.textContent = signe;
        item.append(sg);
      }
      item.append(nom);
      return item;
    }),
  );
}

/// L'accordage dans le bandeau (V37) : celui du disque, pour lequel la
/// tablature est écrite, et celui à utiliser avec la hauteur en cours — « disque :
/// ½ ton dessous (E♭ A♭ D♭ G♭) · avec +½ ton : standard (E A D G) ». Rien en
/// standard à la hauteur d'origine.
function direTransposition() {
  const t = $("coach-transpose");
  const { instrument, tr } = affichee();
  const cordes = instrument === "bass" ? tr?.pret?.accordage : null;
  const d = instrument === "bass" ? transpositionDe("bass") : 0;
  const texte = !cordes?.length
    ? d
      ? `transposé de ${REGLAGES.tonalite.ecrire(d)}`
      : ""
    : d
      ? `disque : ${direAccordage(cordes)} · avec ${REGLAGES.tonalite.ecrire(d)} : ${direAccordage(aJouer(cordes, d))}`
      : nommer(cordes) === "standard"
        ? ""
        : `accordage : ${direAccordage(cordes)}`;
  t.hidden = !texte || !!coach.attente;
  t.textContent = texte;
  t.title = texte
    ? "La tablature est écrite pour l'accordage du disque et ne change pas avec la hauteur des stems : accordez la basse comme indiqué et jouez les mêmes frettes. Le coach juge ce que vous jouez, à la hauteur entendue."
    : "";
}

function dire() {
  $("partition-coach").classList.toggle("coach--ecoute", coach.actif && !coach.attente);
  // V36 : le diapason, dès que le bandeau montre la basse (partition ou non).
  majBoutonAccordeur((coach.attente?.instrument ?? affichee().instrument) === "bass");
  dessinerLegende();
  direTransposition();
  direTexte();
  // La ligne coupe ce qui dépasse : le texte entier au survol.
  const etat = $("coach-etat");
  etat.title = etat.textContent;
}

/// Sans partition prête : le geste qui la calcule, ou l'état du calcul.
function direAttente() {
  const { instrument, tr } = coach.attente;
  const nom = instrument === "bass" ? "la basse" : "la batterie";
  for (const id of ["coach-basculer", "coach-calibrer", "coach-pads", "coach-peripherique", "coach-canal", "coach-boucler", "coach-vitesse"]) {
    $(id).hidden = true;
  }
  const transcrire = $("coach-transcrire");
  const enCours = !!tr && !tr.erreur;
  transcrire.hidden = enCours;
  transcrire.textContent = `${tr?.erreur ? "retranscrire" : "transcrire"} ${nom} pour jouer avec`;
  transcrire.title = `Calculer la partition de ${nom} (comme « Calculer » du bloc Partitions) : le coach la suivra.`;
  $("coach-etat").textContent = enCours
    ? `Transcription de ${nom} en cours : le coach suivra.`
    : tr?.erreur
      ? `Transcription impossible : ${tr.erreur}`
      : "";
}

function direTexte() {
  const etat = $("coach-etat");
  const bascule = $("coach-basculer");
  if (coach.attente) return direAttente();
  $("coach-transcrire").hidden = true;
  bascule.hidden = false;
  $("coach-canal").hidden = !coach.actif || coach.instrument !== "bass" || coach.canaux < 2;
  bascule.setAttribute("aria-pressed", String(coach.actif));
  bascule.textContent = coach.actif ? "■ arrêter l'écoute" : ARRETE[coach.instrument ?? "drums"];
  $("coach-calibrer").hidden = !coach.actif || !coach.peripherique;
  montrerPads(coach.actif && coach.instrument === "drums" && !!coach.peripherique);
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
    etat.textContent = coach.message ?? (coach.instrument === "bass" ? "Aucune entrée audio." : "Aucune batterie MIDI branchée.");
    return;
  }
  if (coach.calibrage) {
    etat.textContent =
      coach.instrument === "bass"
        ? "Calibrer : ▶, puis pincer une corde à vide, étouffée, sur les seize clics du décompte."
        : "Calibrer : ▶, puis taper la caisse claire sur les seize clics du décompte.";
    return;
  }
  const calibree = coach.latenceMs === null ? "latence non calibrée" : `latence ${Math.round(coach.latenceMs)} ms`;
  // La basse : l'entrée jugée, et celle où la basse semble jouer (V34).
  const basse = coach.instrument === "bass" && coach.canaux > 1;
  const latence =
    (basse ? `entrée ${coach.canal + 1} · ` : "") +
    calibree +
    (basse && coach.ailleurs !== null ? ` · la basse semble sur l'entrée ${coach.ailleurs + 1}` : "");
  const b = coach.bilan;
  if (!b || !b.attendues) {
    etat.textContent = `${latence} · ▶ et jouer`;
    return;
  }
  const moyenne = b.ecarts.length ? b.ecarts.reduce((x, y) => x + y, 0) / b.ecarts.length : 0;
  const signe = moyenne > 0.5 ? "+" : moyenne < -0.5 ? "−" : "±";
  const passage = coach.passage > 0 ? `passage ${coach.passage + 1} : ` : "";
  const hauteurs = b.fausses || b.octaves ? ` · ${[b.fausses ? `${b.fausses} fausse${b.fausses > 1 ? "s" : ""}` : "", b.octaves ? `${b.octaves} à l'octave` : ""].filter(Boolean).join(", ")}` : "";
  etat.textContent =
    `${passage}${b.attrapees} / ${b.attendues} attrapées · écart moyen ${signe}${Math.abs(Math.round(moyenne))} ms` +
    hauteurs +
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
  if (coach.instrument === "bass") return armerBasse(tr);
  try {
    await invoke("coach_armer", {
      chemin: edition.source?.path ?? null,
      coups: tr.pret.coups ?? [],
      premiersTemps: edition.pulsation?.premiers_temps ?? [],
      temps: edition.pulsation?.temps ?? [],
    });
    coach.conseil = null;
    coach.relevesDepuis = 0;
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

/// La basse : chaque note de la transcription, vérifiée par le moteur sur
/// le stem de basse.
async function armerBasse(tr) {
  coach.vitesseProposee = null;
  coach.boucleProposee = null;
  try {
    coach.demiTons = transpositionDe("bass");
    const v = await invoke("coach_basse_armer", {
      id: edition.source?.id ?? 0,
      notes: (tr.pret.notes ?? []).map((n) => ({ debut_s: n.debut_s, hauteur: n.hauteur })),
      demiTons: coach.demiTons,
    });
    annoncer(
      v.stem
        ? `${v.sures} notes sur ${v.notes} confirmées par le stem : les autres ne sont pas notées.`
        : "Pas de stem de basse : aucune note n'est notée.",
    );
  } catch (e) {
    coach.armee = null;
    journal("coach", `armer la basse : ${e}`, "warn");
  }
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
  const accordeurFerme = accordeur.ouvert;
  if (accordeurFerme) await fermerAccordeur();
  coach.actif = true;
  coach.message = null;
  dire();
  let liste = [];
  try {
    liste = await invoke(COMMANDES[coach.instrument].lister);
  } catch (e) {
    annoncer(`${coach.instrument === "bass" ? "Audio" : "MIDI"} indisponible : ${e}`);
  }
  const choix = $("coach-peripherique");
  choix.replaceChildren(...liste.map((n) => new Option(n, n)));
  choix.hidden = liste.length < 2;
  let nom;
  if (coach.instrument === "bass") {
    // V42 : l'entrée par défaut de macOS, sauf choix fait à la main tant
    // qu'elle n'a pas changé (`entree-basse.js`).
    nom = entreeAOuvrir(liste);
    montrerEntrees(liste, nom);
  } else {
    let garde = null;
    try {
      garde = localStorage.getItem("coach-peripherique");
    } catch {}
    nom = liste.includes(garde) ? garde : (liste[0] ?? null);
  }
  await ecouter(nom);
  // V36 : l'accordeur a cédé l'entrée ; le dire, avant ce que l'armement a
  // annoncé.
  if (accordeurFerme) {
    const suite = coach.message && performance.now() < coach.messageJusqua ? ` ${coach.message}` : "";
    annoncer(`L'accordeur se ferme : le coach prend l'entrée.${suite}`);
  }
}

async function ecouter(nom) {
  coach.peripherique = null;
  coach.latenceMs = null;
  coach.canaux = 0;
  coach.ailleurs = null;
  poserProfil(null);
  if (nom) {
    try {
      const vue = await invoke(COMMANDES[coach.instrument].ecouter, ecouterArgs(coach.instrument, nom));
      coach.peripherique = vue?.peripherique ?? vue?.entree ?? null;
      coach.latenceMs = vue?.latence_ms ?? null;
      if (coach.instrument === "bass" && vue) poserCanaux(vue);
      if (coach.instrument === "drums") poserProfil(vue?.profil ?? null, coach.peripherique);
      $("coach-peripherique").value = nom;
      // Le dernier périphérique ouvert, gardé par instrument ; pour la basse,
      // avec le défaut de macOS du moment (V42).
      if (coach.instrument === "bass") garderEntree(nom);
      else {
        try {
          localStorage.setItem("coach-peripherique", nom);
        } catch {}
      }
    } catch (e) {
      annoncer(`${coach.instrument === "bass" ? "Entrée audio" : "Batterie"} injoignable : ${e}`);
    }
  }
  const { tr } = affichee();
  if (coach.actif && tr) {
    coach.armee = null;
    await armer(tr);
  }
  if (coach.peripherique) demarrerSondage();
  dire();
}

/// Les canaux de l'interface écoutée (basse) : « auto », puis une entrée
/// par canal.
function poserCanaux(vue) {
  coach.canaux = vue.canaux;
  coach.canal = vue.canal;
  coach.manuel = vue.manuel;
  const choix = $("coach-canal");
  const options = [new Option("auto", "auto")];
  for (let k = 0; k < vue.canaux; k++) options.push(new Option(`entrée ${k + 1}`, String(k)));
  choix.replaceChildren(...options);
  choix.value = vue.manuel ? String(vue.canal) : "auto";
}

async function choisirCanal(valeur) {
  try {
    const vue = await invoke("coach_basse_canal", { canal: valeur === "auto" ? null : Number(valeur) });
    poserCanaux(vue);
    coach.ailleurs = null;
    annoncer(vue.manuel ? `Entrée ${vue.canal + 1} écoutée.` : `Entrée trouvée seule (pour l'instant : entrée ${vue.canal + 1}).`);
  } catch (e) {
    annoncer(`Entrée non choisie : ${e}`);
  }
}

async function arreter() {
  coach.actif = false;
  coach.calibrage = false;
  coach.peripherique = null;
  poserProfil(null);
  arreterSondage();
  effacerMarques();
  dire();
  try {
    await invoke(COMMANDES[coach.instrument ?? "drums"].ecouter, ecouterArgs(coach.instrument, null));
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
    const v = await invoke(COMMANDES[coach.instrument].juger, { depuisS: coach.depuis });
    coach.depuis = v.jusqua_s;
    if (coach.instrument === "bass") recevoirBasse(v);
    else recevoir(v);
  } catch (e) {
    journal("coach", `juger : ${e}`, "warn");
  } finally {
    coach.occupe = false;
  }
}

/// Le premier conseil du groove (`coach_groove`, E5), ou `null` : rien sous
/// les seuils du moteur.
async function conseilGroove() {
  try {
    const g = await invoke("coach_groove");
    return g.conseils[0]?.texte ?? null;
  } catch (e) {
    journal("coach", `groove : ${e}`, "warn");
    return null;
  }
}

/// La fin d'un passage de boucle, et le conseil du groove s'il y en a un.
async function annoncerPassage(message) {
  coach.relevesDepuis = 0;
  const c = await conseilGroove();
  if (c) coach.conseil = c;
  annoncer(c ? `${message} · ${c}` : message, c ? 9000 : 6000);
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
    annoncerPassage(`Passage ${fini.propre ? "propre" : "à reprendre"} : ${fini.attrapees} / ${fini.attendues}${et}${suite}`);
    // Le compte de la barre repart avec le passage suivant.
    coach.bilan = null;
  }
  if (v.releves?.length && !fini) {
    coach.relevesDepuis += v.releves.length;
    if (coach.relevesDepuis >= RELEVES_PAR_CONSEIL) {
      coach.relevesDepuis = 0;
      conseilGroove().then((c) => {
        if (c && c !== coach.conseil) {
          coach.conseil = c;
          annoncer(c, 9000);
        }
      });
    }
  }
  if (!v.jugements.length && !v.places_perdues.length) {
    if (v.releves?.length) dire();
    return;
  }
  // La note attendue juste avant une note en trop : le + se pose après elle.
  let precedente = coach.derniere;
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
    if (j.reference !== null) {
      // Rejouée : ses notes en trop d'avant ne comptent plus.
      if (j.passage === coach.passage) coach.enTrop.delete(j.reference);
      precedente = j.reference;
    } else if (j.verdict === "en_trop" && precedente !== null) {
      coach.enTrop.set(precedente, (coach.enTrop.get(precedente) ?? 0) + 1);
    }
  }
  coach.derniere = precedente;
  const perdue = v.places_perdues.at(-1);
  if (perdue) {
    annoncer(
      `Mesure ${perdue.mesure + 1} : place perdue — c'est la mesure ${perdue.decalage < 0 ? "d'avant" : "d'après"} qui se joue.`,
    );
  } else dire();
  dessinerMarques();
}

/// Les notes de basse jugées : le bilan de la barre (timing et hauteur) et
/// un cercle par note notée.
function recevoirBasse(v) {
  if (v.erreur && v.erreur !== coach.erreurDite) {
    coach.erreurDite = v.erreur;
    annoncer(`Entrée audio : ${v.erreur}`);
  }
  // « Auto » : le moteur a entendu la basse sur un autre canal et l'y suit.
  if (v.canal !== coach.canal) {
    coach.canal = v.canal;
    annoncer(`Basse entendue sur l'entrée ${v.canal + 1} : c'est elle qui est écoutée.`);
  }
  if (v.ailleurs !== coach.ailleurs) {
    coach.ailleurs = v.ailleurs;
    dire();
  }
  // Une annonce échue laisse la place au bilan, même sans note nouvelle.
  if (coach.message && performance.now() >= coach.messageJusqua) {
    coach.message = null;
    dire();
  }
  if (!v.jugements.length) return;
  // La note attendue juste avant une note en trop : le + se pose après elle.
  let precedente = coach.derniere;
  for (const j of v.jugements) {
    if (j.reference !== null && j.passage !== coach.passage) {
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
        if (j.hauteur?.type === "fausse") b.fausses++;
        if (j.hauteur?.type === "octave") b.octaves++;
      }
    }
    if (j.reference !== null && VERDICTS[j.verdict]) coach.marques.set(j.reference, j);
    else if (j.reference !== null) coach.marques.delete(j.reference);
    if (j.reference !== null) {
      // Rejouée : ses notes en trop d'avant ne comptent plus.
      if (j.passage === coach.passage) coach.enTrop.delete(j.reference);
      precedente = j.reference;
    } else if (j.verdict === "en_trop" && precedente !== null) {
      coach.enTrop.set(precedente, (coach.enTrop.get(precedente) ?? 0) + 1);
    }
  }
  coach.derniere = precedente;
  dire();
  dessinerMarques();
}

/// Le verdict d'une note jugée, hauteur comprise (basse) : une note attrapée
/// mais fausse ou à l'octave est une faute.
///
/// V35 : la fausse note porte la note jouée (la référence transposée plus
/// l'écart), la mauvaise octave un « 8 ».
function verdictDe(j, rang) {
  const v = VERDICTS[j.verdict];
  const h = j.hauteur;
  if (!ATTRAPEES.has(j.verdict) || !h) return v;
  if (h.type === "fausse") {
    const d = h.demi_tons;
    const attendue = basseAffichee()?.pret?.notes?.[rang]?.hauteur;
    const jouee = attendue === undefined ? null : attendue + coach.demiTons + d;
    const ecart = `${d > 0 ? "+" : "−"}${Math.abs(d)} demi-ton${Math.abs(d) > 1 ? "s" : ""}`;
    return {
      classe: "fausse",
      dit: jouee === null ? `fausse note (${ecart})` : `fausse note : ${nomNote(jouee)} joué (${ecart})`,
      signe: jouee === null ? (d > 0 ? "♯" : "♭") : nomNote(jouee),
    };
  }
  if (h.type === "octave") {
    return { classe: "octave", dit: `mauvaise octave (${h.octaves > 0 ? "trop haut" : "trop bas"})`, signe: "8" };
  }
  return v;
}

/// Où est la note `rang` de la basse sur la partition : son premier temps
/// (une note liée s'étend sur plusieurs).
function tempsBasseDe(rang) {
  const cartes = partition.temps;
  if (!cartes) return null;
  for (let m = 0; m < cartes.length; m++) {
    const i = cartes[m].findIndex((t) => t.note === rang);
    if (i >= 0) return { mesure: m, voix: 0, temps: i };
  }
  return null;
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
  const { tr, instrument } = affichee();
  const basse = instrument === "bass";
  let n = 0;
  if (surface && table && score && tr && instrument === coach.instrument && (basse ? partition.temps : partition.batterie)) {
    const s = surface.getBoundingClientRect();
    const r = rendu.getBoundingClientRect();
    const arts = basse ? new Map() : articulations();
    for (const [rang, j] of coach.marques) {
      const ou = basse ? tempsBasseDe(rang) : partition.batterie.get(rang);
      const beat = ou && score.tracks[0]?.staves[0]?.bars[ou.mesure]?.voices[ou.voix]?.beats[ou.temps];
      const b = beat ? table.findBeat(beat) : null;
      if (!b) continue;
      const notes = b.notes ?? [];
      const art = basse ? null : arts.get(tr.pret.coups[rang]?.piece);
      const nb = basse
        ? (notes[0] ?? null)
        : (notes.find((x) => x.note?.percussionArticulation === art) ?? (notes.length === 1 ? notes[0] : null));
      const t = nb?.noteHeadBounds ?? b.visualBounds;
      let c = coach.cercles[n];
      if (!c?.isConnected) {
        c = coach.cercles[n] = document.createElement("div");
        rendu.append(c);
      }
      n++;
      const v = verdictDe(j, rang);
      const trop = coach.enTrop.get(rang) ?? 0;
      c.className = `coach__marque coach__marque--${v.classe}`;
      const temps = ["avance", "retard", "juste"].includes(v.classe);
      c.title =
        v.dit +
        (j.ecart_musical_ms !== null && temps ? ` (${j.ecart_musical_ms > 0 ? "+" : ""}${Math.round(j.ecart_musical_ms)} ms)` : "") +
        (trop ? ` · puis ${trop} note${trop > 1 ? "s" : ""} en trop` : "");
      poserSigne(c, "coach__signe coach__signe--verdict", v.signe);
      poserSigne(c, "coach__signe coach__signe--trop", trop ? (trop > 1 ? `+${trop}` : "+") : "");
      const cote = Math.max(t.w, t.h) + 12;
      c.style.width = c.style.height = `${cote}px`;
      c.style.transform =
        `translate(${s.left - r.left + t.x + t.w / 2 - cote / 2}px, ${s.top - r.top + t.y + t.h / 2 - cote / 2}px)`;
      c.hidden = false;
    }
  }
  for (const c of coach.cercles.slice(n)) c.hidden = true;
}

/// Le signe d'une marque (← → 8 × la note jouée +), réutilisé d'un dessin à
/// l'autre ; vide, il disparaît.
function poserSigne(marque, classe, texte) {
  const sel = `.${classe.split(" ").at(-1)}`;
  let e = marque.querySelector(sel);
  if (!texte) {
    e?.remove();
    return;
  }
  if (!e) {
    e = document.createElement("span");
    e.className = classe;
    e.setAttribute("aria-hidden", "true");
    marque.append(e);
  }
  e.textContent = texte;
}

function effacerMarques() {
  coach.marques.clear();
  coach.enTrop.clear();
  coach.derniere = null;
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
      const v = await invoke(COMMANDES[coach.instrument].calibrer, { clics: plan.clics.map((c) => c[0]) });
      coach.latenceMs = v.latence_ms;
      const quoi = coach.instrument === "bass" ? "notes" : "frappes";
      // La basse : le canal où sont arrivées les attaques (V34).
      const ou = coach.instrument === "bass" && coach.canaux > 1 ? ` sur l'entrée ${v.canal + 1}` : "";
      if (coach.instrument === "bass") {
        coach.canal = v.canal;
        coach.ailleurs = null;
      }
      annoncer(`Calibrée${ou} : ${Math.round(v.latence_ms)} ms (± ${Math.round(v.dispersion_ms)}), ${v.retenues} ${quoi} retenues.`);
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
$("coach-peripherique").addEventListener("change", (e) =>
  coach.instrument === "bass" ? choisirEntree(e.target.value) : ecouter(e.target.value),
);
// V42 : l'entrée de la basse choisie ici, dans l'accordeur ou dans le retour.
document.addEventListener("entree-basse", (e) => {
  if (!coach.actif || coach.instrument !== "bass") return;
  e.detail.prise = true;
  $("coach-peripherique").value = e.detail.nom;
  if (coach.peripherique !== e.detail.nom) ecouter(e.detail.nom).catch((err) => journal("coach", `${err}`, "warn"));
});
$("coach-canal").addEventListener("change", (e) => choisirCanal(e.target.value));
$("coach-transcrire").addEventListener("click", async () => {
  const instrument = coach.attente?.instrument;
  if (!instrument) return;
  await calculerPartition(instrument);
  // L'aide de la partition et la barre disent le calcul en cours.
  montrerPartition();
});
$("coach-calibrer").addEventListener("click", () => {
  coach.calibrage = !coach.calibrage;
  coach.message = null;
  dire();
});
// V43 : le repli des réglages de l'entrée se ferme par Échap (le focus revient
// à son bouton) ou par un clic ailleurs, comme un menu ; de même la légende
// derrière « ? » (24 nov.). Ouvrir l'un ferme l'autre.
const replis = [$("coach-reglages"), $("coach-cle")];
for (const repli of replis) {
  repli.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !repli.open) return;
    e.stopPropagation();
    repli.open = false;
    repli.querySelector("summary").focus();
  });
  repli.addEventListener("toggle", () => {
    if (repli.open) for (const autre of replis) if (autre !== repli) autre.open = false;
  });
}
document.addEventListener("pointerdown", (e) => {
  for (const repli of replis) if (repli.open && !repli.contains(e.target)) repli.open = false;
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

export { annoncer, apresDecompte, arreter, brancherCoach, calibrageDemande, coach, majCoach };
