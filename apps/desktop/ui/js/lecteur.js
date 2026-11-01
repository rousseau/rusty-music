import "./ecouter.js"; // l'ordre d'exécution : voir main.js
import {
  $, affecterFileCourante, duree, fileCompositionActive, fileCourante, horloge, invoke, pochette,
  remonter, sommet, txt,
} from "./socle.js";
import {
  chargerPopularites, dessiner, jaugePop, popParPiste,
} from "./vues.js";
import {
  alerteTransportActive, inspecter, lancerLecture, majMinuteur, mesuresDuTransport,
  pisteSelectionnee, poser, qualiteDuTransport, rafraichirMinuteur, selectionner,
  signalerPistesIgnorees,
} from "./ecouter.js";
import {
  anneau, carte, chargerAnneau, chercherAlbums, retenu, suivreAlbumEnLecture,
} from "./carte.js";
import {
  dessinerCarte, modeAnneau,
} from "./maplibre.js";
import {
  dessinerBornes, poserBorne,
} from "./fond-de-plan.js";
import {
  battementStems,
} from "./bibliotheque.js";
import {
  edition,
} from "./editer.js";
import {
  deplacerLecture, lancerDecompte, oublierStems, poserTete, rampeRGB, rattacherStems,
} from "./pratiquer.js";
import {
  charger,
} from "./demarrage.js";
import { lecture, transport } from "./etat-lecture.js";

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
  // Anneau : la barre choisit un album, pas une borne de chemin — le
  // premier résultat de la recherche en cours devient l'album focal.
  if (modeAnneau()) {
    const id = anneau.resultats[0]?.id;
    if (id == null) {
      $("fil-compte").textContent = "aucun album trouvé";
      return;
    }
    await chargerAnneau(id);
    return;
  }
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
  selectionner(t);
});

$("q").addEventListener("input", (e) => {
  clearTimeout(minuteur);
  const q = e.target.value.trim();
  minuteur = setTimeout(async () => {
    // Anneau : la barre cherche des albums, pas des morceaux — l'identité
    // (le hachage) est calculée une fois côté moteur, jamais ici.
    if (modeCourant === "explorer" && modeAnneau()) {
      await chercherAlbums(q);
      return;
    }
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
    poser("recherche", `« ${q} »`, r, sommet);
  }, 180);
});

/* ------------------------------------------------------ file d'attente */

// Aléatoire et répétition : l'état affiché suit toujours le moteur (mis à
// jour par le sondage du transport, plus bas), jamais l'inverse — un clic
// envoie l'action puis attend la file résultante avant de se croire cru.
let etatAlea = false;
let etatRepetition = "aucune";
// Rang à partir duquel la file se laisse encore réordonner ou mélanger — les
// rangs qui précèdent sont déjà confiés à la sortie côté moteur
// (`Player::verrou`). Recopié du sondage ; peut donc retarder d'un battement
// (200 ms) sur la vérité — sans conséquence, le moteur reste seul juge et
// ignore silencieusement un échange qui déborderait sur ce qui est déjà parti.
let etatVerrou = 0;
// Glisser-déposer en cours (rang de la ligne saisie, ou `null`) — suspend le
// redessin du panneau (voir `dessinerFile`) : sans ça, un morceau qui avance
// ou une pochette qui arrive pendant le geste arracherait le nœud DOM en
// cours de glissement sous la main.
let glissementEnCours = false;
let rangGlisse = null;

const LIBELLE_REPETITION = { aucune: "répéter", toutes: "répéter tout", une: "répéter 1" };
const SUITE_REPETITION = { aucune: "toutes", toutes: "une", une: "aucune" };

function refletAlea() {
  $("file-alea").setAttribute("aria-pressed", String(etatAlea));
}

function refletRepetition() {
  const bouton = $("file-repeter");
  bouton.dataset.mode = etatRepetition;
  bouton.textContent = LIBELLE_REPETITION[etatRepetition];
  bouton.setAttribute("aria-pressed", String(etatRepetition !== "aucune"));
}

/// Reprend l'ordre réellement retenu par le moteur (`chemins`, dans l'ordre)
/// après un mélange ou un glisser-déposer — lui seul sait ce qui a pris,
/// [`Player::deplacer`]/`set_alea` ignorant en silence ce qui déborderait sur
/// ce qui est déjà confié à la sortie. Les métadonnées déjà chargées
/// (titre, popularité…) suivent chaque piste par son chemin.
function resynchroniserFile(chemins) {
  const parChemin = new Map(fileCourante.map((t) => [t.path, t]));
  const reordonnee = chemins.map((c) => parChemin.get(c)).filter(Boolean);
  if (reordonnee.length === chemins.length) affecterFileCourante(reordonnee);
  dessinerFile();
}

$("file-alea").addEventListener("click", async () => {
  etatAlea = !etatAlea;
  refletAlea();
  resynchroniserFile(await invoke("set_alea", { actif: etatAlea }));
});

$("file-repeter").addEventListener("click", async () => {
  etatRepetition = SUITE_REPETITION[etatRepetition];
  refletRepetition();
  await invoke("set_repetition", { mode: etatRepetition });
});

// L'interface connaît déjà la file : c'est elle qui l'a envoyée au moteur.
// Inutile de la redemander, `current` suffit à situer la lecture.
/// 3 720 000 ms → « 1 h 02 » ; sous l'heure → « 41 min » ; sous la minute →
/// « 35 s ». La durée d'une file se lit à la minute près.
function dureeFile(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const min = Math.round(s / 60);
  return min < 60 ? `${min} min` : `${Math.floor(min / 60)} h ${String(min % 60).padStart(2, "0")}`;
}

/// « 1 h 02 · reste 41 min » : le total de la file, et ce qu'il reste à jouer
/// depuis la piste en cours (le reste de celle-ci, plus les suivantes).
///
/// - une piste sans durée connue n'est pas comptée, et le total est alors
///   précédé de « ≈ » ;
/// - **pas de « reste » en répétition** : la file reboucle, il n'y a pas de fin ;
/// - `rang` négatif (rien en lecture) : le total seul.
function texteDureeFile(pistes, rang, positionMs, repetition) {
  const duree_ = (t) => (Number.isFinite(t.duration_ms) && t.duration_ms > 0 ? t.duration_ms : 0);
  const somme = (liste) => liste.reduce((n, t) => n + duree_(t), 0);
  const total = somme(pistes);
  if (total === 0) return "";
  const approx = pistes.some((t) => duree_(t) === 0) ? "≈ " : "";
  let texte = `${approx}${dureeFile(total)}`;
  if (rang >= 0 && repetition === "aucune") {
    const reste = Math.max(0, duree_(pistes[rang]) - positionMs) + somme(pistes.slice(rang + 1));
    texte += ` · reste ${approx}${dureeFile(reste)}`;
  }
  return texte;
}

/// Remet à jour la ligne de durée du panneau de file. Légère — un battement
/// l'appelle à 5 Hz — : le texte n'est réécrit que s'il change, et rien n'est
/// calculé tant que le panneau est fermé.
function majDureeFile() {
  if ($("file").hidden) return;
  const rang = fileCourante.findIndex((t) => t.path === lecture.courant);
  const texte = texteDureeFile(fileCourante, rang, lecture.positionMs, etatRepetition);
  const el = $("file-duree");
  if (el.textContent !== texte) el.textContent = texte;
}

function dessinerFile() {
  // La composition d'une playlist ✦ tient la main sur le panneau : elle y
  // fait défiler ses pistes elle-même (`revelerFile`), un rendu ordinaire
  // par-dessus effacerait l'animation.
  if (fileCompositionActive) return;
  // Un glissement en cours tient la ligne saisie : la remplacer sous la main
  // (un morceau qui avance, une pochette qui arrive) casserait le geste.
  if (glissementEnCours) return;
  const hote = $("file-liste");
  $("file-compte").textContent = `${fileCourante.length} morceaux`;
  majDureeFile();

  if (fileCourante.length === 0) {
    hote.innerHTML = '<p class="file__vide">Rien en file. Choisissez un morceau.</p>';
    return;
  }

  const rangCourant = fileCourante.findIndex((t) => t.path === lecture.courant);
  hote.replaceChildren();

  fileCourante.forEach((t, i) => {
    const el = document.createElement("div");
    el.className = "file__ligne";
    if (i === rangCourant) el.classList.add("file__ligne--joue");
    else if (rangCourant >= 0 && i < rangCourant) el.classList.add("file__ligne--passe");

    el.innerHTML = `<span class="file__rang"></span>
                    <span class="file__txt"><b></b><span></span></span>
                    <span class="file__pop"></span>
                    <span class="file__duree"></span>`;
    el.children[0].textContent = i === rangCourant ? "▶" : i + 1;
    el.children[1].children[0].textContent = txt(t.title, "(sans titre)");
    el.children[1].children[1].textContent = txt(t.artist, "(sans artiste)");
    el.children[2].appendChild(jaugePop(popParPiste.get(t.id)));
    el.children[3].textContent = duree(t.duration_ms);

    // Réordonnance manuelle par glisser-déposer : seule la suite pas encore
    // confiée à la sortie s'y prête (`etatVerrou`) — le moteur l'ignorerait
    // de toute façon, mais ne pas rendre la ligne saisissable évite un
    // glissement qui semble n'avoir aucun effet.
    if (i >= etatVerrou) {
      el.draggable = true;
      el.addEventListener("dragstart", (ev) => {
        glissementEnCours = true;
        rangGlisse = i;
        el.classList.add("file__ligne--glissee");
        ev.dataTransfer.effectAllowed = "move";
        // Certains moteurs exigent `setData` pour armer le glisser — même si
        // `rangGlisse` (fermeture JS) suffit ici à retrouver la source.
        ev.dataTransfer.setData("text/plain", String(i));
      });
      el.addEventListener("dragend", () => {
        glissementEnCours = false;
        rangGlisse = null;
        hote
          .querySelectorAll(".file__ligne--glissee, .file__ligne--depose-dessus, .file__ligne--depose-dessous")
          .forEach((n) =>
            n.classList.remove("file__ligne--glissee", "file__ligne--depose-dessus", "file__ligne--depose-dessous"),
          );
      });
      el.addEventListener("dragover", (ev) => {
        if (rangGlisse === null) return;
        ev.preventDefault();
        const rect = el.getBoundingClientRect();
        const dessous = ev.clientY > rect.top + rect.height / 2;
        el.classList.toggle("file__ligne--depose-dessus", !dessous);
        el.classList.toggle("file__ligne--depose-dessous", dessous);
      });
      el.addEventListener("dragleave", () => {
        el.classList.remove("file__ligne--depose-dessus", "file__ligne--depose-dessous");
      });
      el.addEventListener("drop", async (ev) => {
        ev.preventDefault();
        const dessous = el.classList.contains("file__ligne--depose-dessous");
        el.classList.remove("file__ligne--depose-dessus", "file__ligne--depose-dessous");
        if (rangGlisse === null || rangGlisse === i) return;
        // Par identité plutôt qu'un calcul de rang : sans ça, un décalage
        // suivant que la source est au-dessus ou en dessous de la cible est
        // facile à rater. `fileCourante` ne bouge pas pendant un glissement
        // (`glissementEnCours` ci-dessus), `t`/`piste` restent valides.
        const piste = fileCourante[rangGlisse];
        const sansGlissee = fileCourante.filter((x) => x !== piste);
        let a = sansGlissee.indexOf(t);
        if (dessous) a += 1;
        resynchroniserFile(await invoke("deplacer_file", { de: rangGlisse, a }));
      });
    }

    // Sauter conserve les pistes précédentes : on peut revenir en arrière.
    el.addEventListener("click", async () => {
      selectionner(t);
      await demarrerLecture(() => invoke("jump_to", { index: i }));
    });
    hote.appendChild(el);
  });

  // La popularité, en lot puis repaint — comme les pochettes de la grille.
  chargerPopularites(
    fileCourante.map((t) => t.id),
    () => {
      if (!$("file").hidden && !fileCompositionActive) dessinerFile();
    },
  );
}

function basculerFile(ouvrir) {
  const panneau = $("file");
  const visible = ouvrir ?? panneau.hidden;
  panneau.hidden = !visible;
  $("bascule-file").setAttribute("aria-expanded", String(visible));
  // Sans ça, le bouton restait étiqueté « file » même une fois le panneau
  // ouvert — rien ne distinguait alors « l'ouvrir » de « le fermer ».
  $("bascule-file").textContent = visible ? "fermer" : "file";
  if (visible) {
    dessinerFile();
    rafraichirMinuteur();
  }
}

$("bascule-file").addEventListener("click", () => basculerFile());
$("file-fermer").addEventListener("click", () => basculerFile(false));
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("file").hidden) basculerFile(false);
});

/* ---------------------------------------------------------- transport */

// Ce qui est chargé et ce que l'utilisateur a demandé : `etat-lecture.js`.
let clicEnVol = false;

/// Pose l'icône du transport **et** l'intention, ensemble.
///
/// Les deux ne peuvent pas diverger si un seul endroit les écrit. Divergentes,
/// elles produisaient un clic qui envoyait l'action inverse.
function poserLecture(joue) {
  transport.veutJouer = joue;
  $("lecture").textContent = joue ? "⏸" : "▶";
}

/// L'aperçu du transport : le morceau **sélectionné**, affiché quand rien n'est
/// chargé (ni lecture, ni pause, ni stems) — « prêt à lire », sans qu'aucun son
/// ne parte. Purement interface : le moteur n'a rien chargé, ▶ lance alors la
/// lecture de la sélection (`basculerLecture`). Un morceau en cours, lui, garde
/// le transport : choisir autre chose ne coupe rien et ne perd aucune position.
/// (L'indicateur : `transport.apercu`.)

// `transport.veutJouer` : une lecture vient d'être demandée, le moteur n'a juste pas encore
// répondu (`lecture.courant` est encore vide) — ce n'est pas « rien de chargé ».
const rienDeCharge = () =>
  (!lecture.courant || lecture.fini) && !lecture.stems.charges && !transport.veutJouer;

/// Ce que le transport montre à cet instant : le morceau joué, l'aperçu, ou —
/// quand les stems ont pris la main — le morceau qu'on est en train d'éditer.
const estAffiche = (chemin) =>
  lecture.courant === chemin ||
  (transport.apercu && pisteSelectionnee?.path === chemin) ||
  (lecture.stems.charges && edition.source?.path === chemin);

function majApercuTransport() {
  const t = pisteSelectionnee;
  if (!t || !rienDeCharge()) return;
  transport.apercu = true;
  document.querySelector(".transport").classList.add("transport--apercu");
  $("np-titre").textContent = txt(t.title, "(sans titre)");
  // Sans chemin : le clic sur le titre (navigation vers l'album) ne trouve
  // rien dans la file, no-op — l'aperçu n'est pas encore un morceau joué.
  $("np-titre").dataset.path = "";
  $("np-artiste").textContent = txt(t.artist, "(sans artiste)");
  $("np-artiste").dataset.artiste = t.artist ?? "";
  $("np-artiste").dataset.mbid = t.artist_mbid ?? "";
  if (!alerteTransportActive()) $("np-mesures").textContent = "";
  $("np-qualite").textContent = "";
  mesuresDuTransport(t);
  qualiteDuTransport(t);
  $("tc").textContent = `${horloge(0)} / ${horloge(t.duration_ms)}`;
  $("transport-art").style.backgroundImage = "";
  pochette(t.path).then((img) => {
    // La sélection a pu changer, ou une lecture démarrer, entre-temps.
    if (img && transport.apercu && pisteSelectionnee?.path === t.path) {
      $("transport-art").style.backgroundImage = `url("${img}")`;
    }
  });
  // Le spectrogramme de la sélection, comme pour un morceau chargé — et déjà en
  // cache quand la lecture démarre. Après un court délai : le calcul décode le
  // fichier entier, on ne le lance pas pour chaque ligne survolée au clic.
  chargerSpectreTransport(null);
  peindreTransport(0);
  clearTimeout(minuteurSpectreApercu);
  minuteurSpectreApercu = setTimeout(() => {
    if (transport.apercu && pisteSelectionnee?.path === t.path) chargerSpectreTransport(t);
  }, 400);
}
let minuteurSpectreApercu = null;

/// Quitte l'aperçu : une lecture démarre. `lecture.courant` retombe à `null` pour que
/// le prochain battement réécrive tout le transport, même si le moteur rejoue le
/// morceau qu'il avait fini.
function finApercuTransport() {
  if (!transport.apercu) return;
  transport.apercu = false;
  lecture.courant = null;
  clearTimeout(minuteurSpectreApercu);
  document.querySelector(".transport").classList.remove("transport--apercu");
}

/// Lance une lecture en affichant l'intention **avant** que le moteur réponde.
///
/// `demarrer` renvoie la promesse de l'`invoke("play")` / `invoke("set_queue")`.
/// Le moteur décode la première piste en mémoire avant de rendre la main —
/// jusqu'à ~1 s sur support lent. Poser l'icône après cet `await` (l'ancien
/// schéma) laissait le bouton sur ▶ pendant tout ce temps : le clic sur la
/// pochette paraissait ignoré. On bascule donc l'icône tout de suite ;
/// `transport.ignorerEtatJusqua` empêche un battement de sondage de remettre ▶ tant que
/// le moteur n'a pas confirmé la lecture.
///
/// Le sondage, lui, ne démarre qu'**après** que le moteur a répondu : lancé
/// avant, son premier battement voit le lecteur encore `finished` (l'ancienne
/// file épuisée) et se coupe aussitôt (`if (e.finished) sonder(false)`) —
/// plus rien n'interrogeait alors le moteur et le panneau d'écoute restait
/// figé sur « Rien en lecture ».
async function demarrerLecture(demarrer) {
  // Lecture ordinaire et lecture des stems s'excluent : `stems_play` met le
  // lecteur du module 1 en pause côté moteur, ici on fait l'inverse — sans ça,
  // choisir un autre morceau pendant que les stems jouent laissait les deux
  // sorties audio se superposer, et le bouton du bas ne pilotait plus que
  // l'une des deux.
  // **L'interface oublie les stems, le moteur les coupe** : chaque commande de
  // lecture (`play`, `set_queue`, `remplacer_file`, `jump_to`) le fait de son
  // côté et relance le lecteur, que `stems_play` avait mis en pause. Envoyer ici
  // un « arrêter » d'abord ne changeait rien à l'exclusion, mais laissait
  // `remplacer_file` sur la même piste sans personne pour relancer le lecteur.
  oublierStems();
  finApercuTransport();
  lecture.fini = false;
  poserLecture(true);
  transport.ignorerEtatJusqua = Date.now() + 2000;
  try {
    await demarrer();
  } catch (e) {
    poserLecture(false);
    throw e;
  }
  sonder(true);
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
  // Rien de chargé, un morceau seulement sélectionné : ▶ le lit — depuis sa
  // liste s'il en fait partie (la suite forme la file), seul sinon.
  if (transport.apercu && pisteSelectionnee && rienDeCharge()) {
    await lancerLecture(pisteSelectionnee);
    return;
  }
  const versPause = transport.veutJouer;
  if (!versPause && (await rattacherStems())) {
    await invoke("stems_transport", { action: "reprendre", position: null });
    poserLecture(true);
    sonder(true);
    return;
  }
  poserLecture(!versPause);
  transport.ignorerEtatJusqua = Date.now() + 400;

  if (lecture.stems.charges && !versPause && (await lancerDecompte())) return;
  if (lecture.stems.charges) {
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
// ⏮ / ⏭ (boutons et touches média) : en Éditer, le moteur coupe les stems et
// rend la main au lecteur. Avant, `previous` relançait le morceau d'origine
// par-dessus des stems qui continuaient de jouer.
async function pistePrecedente() {
  reprendreLaMainDuLecteur();
  await invoke("previous");
  sonder(true);
}
async function pisteSuivante() {
  reprendreLaMainDuLecteur();
  await invoke("skip");
  sonder(true);
}
/// Côté interface, ⏮/⏭ pendant des stems : on cesse de les suivre et l'icône
/// annonce la lecture — le moteur relance le lecteur. Hors stems, rien à
/// changer : ⏭ sur un lecteur en pause le laisse en pause, l'icône aussi.
function reprendreLaMainDuLecteur() {
  if (!lecture.stems.charges) return;
  oublierStems();
  poserLecture(true);
  transport.ignorerEtatJusqua = Date.now() + 2000;
}
$("precedent").addEventListener("click", pistePrecedente);
$("suivant").addEventListener("click", pisteSuivante);
$("volume").addEventListener("input", (e) => invoke("set_volume", { volume: e.target.value / 100 }));

// Bouton « E » : amélioration du son (excitation psychoacoustique). Choix
// retenu entre deux lancements, comme le bruit du chemin ou le mode de
// contact. La réglette d'intensité n'apparaît que quand « E » est actif.
const btnAmeliorer = $("ameliorer");
const forceAmeliorer = $("ameliorer-force");
let ameliorationActive = localStorage.getItem("ameliorer") === "1";
let ameliorationForce = clamp01(parseInt(localStorage.getItem("ameliorer-force"), 10), 60);
forceAmeliorer.value = ameliorationForce;

function clamp01(v, defaut) {
  return Number.isFinite(v) ? Math.min(100, Math.max(0, v)) : defaut;
}

function refletAmeliorer() {
  btnAmeliorer.setAttribute("aria-pressed", String(ameliorationActive));
  forceAmeliorer.hidden = !ameliorationActive;
}
refletAmeliorer();

// Applique l'état courant au moteur puis relance le sondage : le moteur
// réouvre le morceau en cours en tâche de fond (~1 s) et reconstruit le
// préchargement avec la nouvelle version. On désarme les commandes le temps
// de la bascule, sans figer l'UI.
async function poserAmelioration() {
  btnAmeliorer.disabled = true;
  forceAmeliorer.disabled = true;
  try {
    await invoke("set_amelioration", {
      actif: ameliorationActive,
      intensite: ameliorationForce / 100,
    });
  } finally {
    setTimeout(() => {
      btnAmeliorer.disabled = false;
      forceAmeliorer.disabled = false;
    }, 800);
  }
  sonder(true);
  rafraichirSpectreTransport();
}

// Au démarrage, si actif : on ne fait que reposer le drapeau (rien ne joue
// encore, `poserAmelioration` réouvrirait dans le vide).
if (ameliorationActive) {
  invoke("set_amelioration", {
    actif: true,
    intensite: ameliorationForce / 100,
  }).catch(() => {});
}

btnAmeliorer.addEventListener("click", () => {
  ameliorationActive = !ameliorationActive;
  localStorage.setItem("ameliorer", ameliorationActive ? "1" : "0");
  refletAmeliorer();
  poserAmelioration();
});

// `change` et non `input` : on ne réouvre le morceau qu'au relâché, pas à
// chaque pixel du glissement.
forceAmeliorer.addEventListener("change", () => {
  ameliorationForce = clamp01(parseInt(forceAmeliorer.value, 10), 60);
  localStorage.setItem("ameliorer-force", String(ameliorationForce));
  if (ameliorationActive) poserAmelioration();
});

// Normalisation de volume à la lecture (EBU R128) : réglage de bibliothèque
// (case + mode piste/album), doublé d'un raccourci « N » dans le transport
// pour l'essayer au vol sans rouvrir la Bibliothèque — voir
// docs/amelioration-audio.md. Un seul état (`normaliserActif`/`normaliserMode`,
// une seule commande `set_normalisation`) : case et bouton ne font que le
// refléter, jamais deux réglages qui pourraient diverger. Choix retenu entre
// deux lancements, comme « E ».
const caseNormaliser = $("normaliser-active");
const ligneNormaliserMode = $("normaliser-mode-ligne");
const selectNormaliserMode = $("normaliser-mode");
const btnNormaliser = $("normaliser");
let normaliserActif = localStorage.getItem("normaliser") === "1";
let normaliserMode = localStorage.getItem("normaliser-mode") === "album" ? "album" : "piste";

function refletNormaliser() {
  caseNormaliser.checked = normaliserActif;
  selectNormaliserMode.value = normaliserMode;
  ligneNormaliserMode.hidden = !normaliserActif;
  btnNormaliser.setAttribute("aria-pressed", String(normaliserActif));
}
refletNormaliser();

// Même recette que `poserAmelioration` : le moteur réouvre le morceau en
// cours en tâche de fond, sans qu'on ait à s'en soucier ici.
function poserNormalisation() {
  invoke("set_normalisation", {
    actif: normaliserActif,
    album: normaliserMode === "album",
  }).catch((e) => remonter(e, "normalisation"));
}
poserNormalisation();

caseNormaliser.addEventListener("change", () => {
  normaliserActif = caseNormaliser.checked;
  localStorage.setItem("normaliser", normaliserActif ? "1" : "0");
  refletNormaliser();
  poserNormalisation();
});

selectNormaliserMode.addEventListener("change", () => {
  normaliserMode = selectNormaliserMode.value === "album" ? "album" : "piste";
  localStorage.setItem("normaliser-mode", normaliserMode);
  if (normaliserActif) poserNormalisation();
});

btnNormaliser.addEventListener("click", () => {
  normaliserActif = !normaliserActif;
  localStorage.setItem("normaliser", normaliserActif ? "1" : "0");
  refletNormaliser();
  poserNormalisation();
});

// Bouton « HD » : super-résolution neuronale (AERO), rendue hors ligne.
const btnHd = $("hd");
let lectureHd = localStorage.getItem("lecture-hd") === "1";
let hdSonde = null; // intervalle de suivi d'une régénération en cours

// Le fond de plan de la carte MapLibre — eau, terre, voirie, bâti, toponymes.
// Chaque thème est un `style-<id>.json` déjà écrit par `engendrer_tuiles` (côté
// Rust, `rusty_music_carto::Palette`) ; basculer ne régénère aucune tuile.
// `osm-clair` = le `style.json` sans suffixe. L'overlay de familles (nuage,
// pastilles, territoires) ne dépend pas de ce choix. Déclaré ici, avec les
// autres préférences en `localStorage`, parce qu'`initialiserGL` le lit.
let carteTheme = localStorage.getItem("carte-theme") || "osm-clair";

if (lectureHd) invoke("set_lecture_hd", { actif: true }).catch(() => {});

/// Met le bouton « HD » à l'état du morceau `t` : absent / disponible / en cours.
async function majHd(t) {
  clearInterval(hdSonde);
  hdSonde = null;
  btnHd.classList.remove("travaille");
  btnHd.disabled = false;
  btnHd.textContent = "HD";
  btnHd.setAttribute("aria-pressed", "false");
  if (!t) {
    btnHd.disabled = true;
    return;
  }
  // Une régénération est-elle en cours pour ce morceau ?
  try {
    const s = await invoke("superres_state");
    if (s.en_cours && s.source === t.path) return suivreHd(t);
  } catch {}
  // Sinon : le cache existe-t-il ?
  try {
    const dispo = await invoke("superres_disponible", { path: t.path });
    btnHd.setAttribute("aria-pressed", String(dispo && lectureHd));
    btnHd.dataset.dispo = dispo ? "1" : "0";
  } catch {}
}

/// Suit l'avancement d'une régénération et rebascule le bouton à la fin.
function suivreHd(t) {
  btnHd.classList.add("travaille");
  btnHd.disabled = true;
  clearInterval(hdSonde);
  hdSonde = setInterval(async () => {
    let s;
    try {
      s = await invoke("superres_state");
    } catch {
      return;
    }
    if (s.source !== t.path) return;
    if (s.en_cours) {
      // Premier usage : les poids d'AERO arrivent avant le rendu.
      if (!s.total && s.a_telecharger) {
        btnHd.textContent = `↓${Math.round((100 * s.telecharges) / s.a_telecharger)}%`;
        btnHd.dataset.titre ??= btnHd.title;
        btnHd.title = "Téléchargement du modèle AERO (156 Mo, une seule fois)";
        return;
      }
      if (btnHd.dataset.titre !== undefined) btnHd.title = btnHd.dataset.titre;
      const pct = s.total ? Math.round((100 * s.faits) / s.total) : 0;
      btnHd.textContent = `${pct}%`;
    } else {
      clearInterval(hdSonde);
      hdSonde = null;
      // lecture.courant peut avoir changé entre-temps.
      const cour = fileCourante.find((x) => x.path === lecture.courant);
      majHd(cour);
      spectres.clear(); // le cache HD vient d'apparaître
      if (lectureHd) rafraichirSpectreTransport();
    }
  }, 700);
}

btnHd.addEventListener("click", async () => {
  const t = fileCourante.find((x) => x.path === lecture.courant);
  if (!t || btnHd.disabled) return;
  if (btnHd.dataset.dispo === "1") {
    // Le cache existe : bascule lecture originale ↔ HD.
    lectureHd = !lectureHd;
    localStorage.setItem("lecture-hd", lectureHd ? "1" : "0");
    btnHd.setAttribute("aria-pressed", String(lectureHd));
    btnHd.disabled = true;
    try {
      await invoke("set_lecture_hd", { actif: lectureHd });
    } finally {
      setTimeout(() => (btnHd.disabled = false), 800);
    }
    sonder(true);
    rafraichirSpectreTransport();
  } else {
    // Pas de cache : lancer la régénération.
    try {
      await invoke("start_superres", { path: t.path });
      suivreHd(t);
    } catch (e) {
      remonter(e, "start_superres");
    }
  }
});

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

// Barre de progression : spectrogramme du son réellement joué (version HD du
// cache si le HD est actif, sinon l'original), avec tête de lecture par
// dessus. Ce que le HD a ajouté au-dessus de l'original est teinté de
// l'accent. Le calcul décode tout le fichier — quelques secondes — donc il
// tourne en tâche de fond et l'image apparaît quand elle est prête ; d'ici là
// un fond neutre sert de repère.
const wave = $("wave");
const waveCnv = $("wave-cnv");
const waveCtx = waveCnv.getContext("2d");
const HAUT_SPECTRE = 42;

// clé path+hd → { img: ImageData } déjà composé. Borné : sans plafond, chaque
// morceau joué laissait son canvas en mémoire jusqu'à la fermeture — des
// centaines sur une journée d'écoute. Les plus anciens partent d'abord.
const SPECTRES_MAX = 24;
const spectres = new Map();
function garderSpectre(cle, fond) {
  spectres.delete(cle);
  spectres.set(cle, fond);
  while (spectres.size > SPECTRES_MAX) spectres.delete(spectres.keys().next().value);
}
let spectreCourant = null; // le { img } affiché

/// Pose une image gardée côté CPU sur un canevas de w×h.
///
/// Les spectrogrammes ne sont pas gardés dans des canevas hors écran : WebKit
/// peut jeter sans prévenir la mémoire GPU d'un canevas (pression mémoire,
/// veille, relance de son processus GPU), et `drawImage` d'un canevas perdu
/// peint du noir — au bout d'un moment, tous les spectrogrammes devenaient
/// noirs. Les pixels restent en `ImageData`, qu'on repose à chaque fois.
function poserImage(ctx, img, w, h) {
  if (img.width === w && img.height === h) {
    ctx.putImageData(img, 0, 0);
    return;
  }
  // Tailles différentes (fenêtre redimensionnée) : passage par un canevas
  // jetable, pour que `drawImage` étire.
  const tmp = document.createElement("canvas");
  tmp.width = img.width;
  tmp.height = img.height;
  tmp.getContext("2d").putImageData(img, 0, 0);
  ctx.drawImage(tmp, 0, 0, w, h);
}
let teteCourante = 0;

/// Recompose : image de fond + assombrissement de la partie écoulée + trait.
function peindreTransport(frac) {
  teteCourante = Math.min(1, Math.max(0, frac || 0));
  const w = waveCnv.width || 1;
  const h = waveCnv.height || 1;
  waveCtx.clearRect(0, 0, w, h);
  if (spectreCourant) {
    poserImage(waveCtx, spectreCourant.img, w, h);
  }
  const x = Math.round(teteCourante * w);
  waveCtx.fillStyle = "rgba(0,0,0,.34)";
  waveCtx.fillRect(0, 0, x, h);
  waveCtx.fillStyle =
    getComputedStyle(document.documentElement).getPropertyValue("--txt").trim() || "#EDE8DC";
  waveCtx.fillRect(x, 0, 1, h);
}

/// Compose l'image du spectrogramme, une fois — teinte l'ajout du HD.
function composerSpectre(s) {
  const { largeur: w, hauteur: h, pixels, pixels_ref, hd } = s;
  const img = new ImageData(w, h);
  const table = rampeRGB();
  const acc = (
    getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#c98a3b"
  )
    .replace("#", "")
    .match(/../g)
    .map((v) => parseInt(v, 16));
  for (let i = 0; i < pixels.length; i++) {
    const v = pixels[i];
    let c = table[v];
    // Là où le HD a mis nettement plus d'énergie que l'original, on vire vers
    // l'accent — d'autant plus fort que le gain est net.
    if (hd && pixels_ref) {
      const gain = v - pixels_ref[i];
      if (gain > 16) {
        const k = Math.min(1, (gain - 16) / 90);
        c = [
          c[0] + (acc[0] - c[0]) * k,
          c[1] + (acc[1] - c[1]) * k,
          c[2] + (acc[2] - c[2]) * k,
        ];
      }
    }
    img.data[i * 4] = c[0];
    img.data[i * 4 + 1] = c[1];
    img.data[i * 4 + 2] = c[2];
    img.data[i * 4 + 3] = 255;
  }
  return { img };
}

/// Demande le spectrogramme du son joué ; le fichier décodé prend quelques
/// secondes, on réessaie sans marteler.
async function chargerSpectreTransport(t, reessai = true) {
  spectreCourant = null;
  peindreTransport(teteCourante);
  if (!t) return;

  const largeur = Math.max(160, Math.round(wave.getBoundingClientRect().width));
  waveCnv.width = largeur;
  waveCnv.height = HAUT_SPECTRE;

  const hd = lectureHd ? 1 : 0;
  const cle = `${t.path}#${hd}`;
  if (spectres.has(cle)) {
    spectreCourant = spectres.get(cle);
    garderSpectre(cle, spectreCourant); // remis en tête : le dernier vu reste
    return peindreTransport(teteCourante);
  }

  const vise = t.path;
  // Un seul calcul par morceau : l'aperçu peut l'avoir lancé quand la lecture
  // démarre, le refaire décoderait le fichier deux fois.
  let calcul = spectresEnCours.get(cle);
  if (!calcul) {
    calcul = calculerSpectre(vise, largeur, cle).finally(() => spectresEnCours.delete(cle));
    spectresEnCours.set(cle, calcul);
  }
  const compose = await calcul;
  if (compose && estAffiche(vise)) {
    spectreCourant = compose;
    peindreTransport(teteCourante);
  } else if (compose === null && reessai && estAffiche(vise)) {
    // Le calcul partagé s'est arrêté parce que le morceau n'était plus affiché
    // à ce moment-là (l'aperçu, avant que la lecture ne démarre) : il l'est
    // maintenant, on le relance une fois.
    chargerSpectreTransport(t, false);
  }
}

const spectresEnCours = new Map(); // clé → promesse du { img } en calcul

/// Le calcul lui-même, avec ses reprises : rend le `{ img }` composé (et le met
/// en cache) ; `null` si le morceau a cessé d'être affiché en cours de route,
/// `false` si le moteur a échoué.
async function calculerSpectre(vise, largeur, cle) {
  const echeance = Date.now() + 120_000;
  let attente = 250;
  while (Date.now() < echeance && estAffiche(vise)) {
    let s;
    try {
      s = await invoke("spectre_transport", { path: vise, width: largeur, height: HAUT_SPECTRE });
    } catch (e) {
      remonter(e, "spectre_transport");
      return false; // échec du moteur : pas de nouvel essai
    }
    if (s && s.pixels && s.pixels.length) {
      const compose = composerSpectre(s);
      garderSpectre(cle, compose);
      return compose;
    }
    await new Promise((r) => setTimeout(r, attente));
    attente = Math.min(attente * 1.4, 3000);
  }
  return null;
}

/// À rappeler quand le son joué change sans changer de morceau (E ou HD).
function rafraichirSpectreTransport() {
  const t = fileCourante.find((x) => x.path === lecture.courant);
  chargerSpectreTransport(t);
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

// Reprise de session : le lecteur peut tenir une file avant même que la page
// n'existe, et il est alors en pause — donc jamais sondé. Un battement au
// démarrage suffit (il coupe le sondage tout seul quand rien ne joue), et le
// moteur prévient si la reprise n'est finie qu'ensuite. Différé d'un tour :
// `battement` lit des variables déclarées plus bas dans ce fichier.
setTimeout(() => sonder(true), 0);
window.__TAURI__.event.listen("session-restauree", () => sonder(true));

// Une pochette peut coûter 200 ms de lecture disque, soit toute la période du
// sondage : sans ce verrou les appels s'empilent et retardent les commandes de
// transport, qui attendent alors derrière eux.
let battementEnVol = false;
let volumeSynchronise = false;
let courantResynchronise = null;
// Un seul signalement par panne, pas un par sondage — à 5 Hz, `remonter`
// spammerait le journal sans rien apprendre de plus après le premier coup.
let battementEnErreur = false;

async function battement() {
  if (lecture.stems.charges) return battementStems();
  if (battementEnVol) return;
  battementEnVol = true;
  let e;
  const versionStems = lecture.stems.version;
  try {
    e = await invoke("playback_state");
    battementEnErreur = false;
  } catch (err) {
    if (!battementEnErreur) {
      battementEnErreur = true;
      remonter(err, "sondage de lecture");
    }
    return;
  } finally {
    battementEnVol = false;
  }

  // Le moteur tient des stems que l'interface croyait partis (une commande de
  // lecture a échoué avant de les couper) : ils sonnent, c'est eux que le
  // transport doit suivre. `battementStems` les compare à ceux du dock, et les
  // coupe s'ils n'en sont pas. Une réponse partie avant que l'interface ne
  // charge ou n'oublie les stems ne dit plus rien : on l'ignore.
  if (e.stems && !lecture.stems.charges && versionStems === lecture.stems.version) {
    lecture.stems.charges = true;
    return battementStems();
  }

  // Aléatoire, répétition et verrou de réordonnance : le moteur reste seul
  // juge, l'affichage ne fait que suivre. Pas de redessin ici — seulement
  // sur un vrai changement de morceau ou une action du panneau — sans quoi
  // la file se reconstruirait à 5 Hz sous le curseur de qui la parcourt.
  etatVerrou = e.verrou;
  lecture.positionMs = e.position_ms;
  majDureeFile();
  majMinuteur(e.minuteur);
  signalerPistesIgnorees(e.ignorees);
  // Le curseur de volume part à 100 dans la page ; le lecteur, lui, peut avoir
  // repris le volume de la session précédente. Une seule fois : ensuite c'est
  // le curseur qui commande.
  if (!volumeSynchronise) {
    volumeSynchronise = true;
    $("volume").value = String(Math.round(e.volume * 100));
  }
  // Le lecteur tient une file que la page ne connaît pas — la reprise de
  // session l'a installée avant que la fenêtre n'existe. On la lui demande,
  // une fois par morceau, sans quoi le transport afficherait « Rien en
  // lecture » devant un morceau pourtant chargé.
  if (
    e.current &&
    e.current !== courantResynchronise &&
    !fileCourante.some((x) => x.path === e.current)
  ) {
    courantResynchronise = e.current;
    try {
      const pistes = await invoke("file_courante");
      if (pistes.length) {
        affecterFileCourante(pistes);
        dessinerFile();
      }
    } catch (err) {
      remonter(err, "file courante");
    }
  }
  if (etatAlea !== e.alea) {
    etatAlea = e.alea;
    refletAlea();
  }
  if (etatRepetition !== e.repetition) {
    etatRepetition = e.repetition;
    refletRepetition();
  }

  lecture.fini = e.finished;
  if (e.current !== lecture.courant) {
    // Un morceau joué reprend le transport, l'aperçu s'efface.
    transport.apercu = false;
    document.querySelector(".transport").classList.remove("transport--apercu");
    lecture.courant = e.current;
    const t = fileCourante.find((x) => x.path === lecture.courant);
    $("np-titre").textContent = t ? txt(t.title, "(sans titre)") : "Rien en lecture";
    // Sert aux clics ci-dessous (§ « le transport navigue ») — même motif
    // que les `dataset.*` posés par `inspecter()`.
    $("np-titre").dataset.path = t?.path ?? "";
    $("np-artiste").textContent = t ? txt(t.artist, "(sans artiste)") : "Choisissez un morceau";
    $("np-artiste").dataset.artiste = t?.artist ?? "";
    $("np-artiste").dataset.mbid = t?.artist_mbid ?? "";
    // Hors du chemin critique, comme la pochette : la ligne se remplit quand
    // la mesure arrive, et reste vide si le morceau n'est pas mesuré.
    if (!alerteTransportActive()) $("np-mesures").textContent = "";
    if (t) mesuresDuTransport(t);
    $("np-qualite").textContent = "";
    if (t) qualiteDuTransport(t);
    majHd(t);
    // L'inspecteur suit la lecture au fil de la file, pas seulement le
    // morceau sur lequel on a cliqué en premier : sans ça, il restait figé
    // sur le départ d'un album ou d'une playlist « dans l'esprit de » pendant
    // que la lecture avançait. Ce bloc ne s'exécute qu'au changement de
    // morceau joué (`e.current !== lecture.courant` ci-dessus), pas à chaque
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
        if (img && lecture.courant === vise) {
          $("transport-art").style.backgroundImage = `url("${img}")`;
        }
      });
    }
    chargerSpectreTransport(t);
    dessiner(); // met à jour la ligne surlignée
    if (!$("file").hidden) dessinerFile();
    // Sans ça, le halo « en écoute » de la carte reste figé sur le premier
    // morceau : rien d'autre ne redessine la carte au fil d'une playlist qui
    // avance toute seule, hors interaction avec elle.
    if (modeCourant === "explorer") dessinerCarte();
  }

  if (Date.now() >= transport.ignorerEtatJusqua) {
    poserLecture(!(e.paused || e.finished));
  }

  const t = fileCourante.find((x) => x.path === lecture.courant);
  const total = t?.duration_ms ?? 0;
  $("tc").textContent = `${horloge(e.position_ms)} / ${horloge(total)}`;

  const frac = total ? Math.min(1, e.position_ms / total) : 0;
  peindreTransport(frac);
  // Les spectrogrammes suivent aussi la lecture ordinaire : tant que les
  // stems ne jouent pas, ils montrent où en est le morceau d'origine.
  if (modeCourant === "editer") poserTete(frac);

  // Frise et anneau : l'album focal partagé suit le morceau joué. Appelé à
  // chaque battement, hors du bloc de changement de morceau ci-dessus (qui met
  // à jour `lecture.courant` en première ligne puis peut lever avant la fin) —
  // `suivreAlbumEnLecture` est idempotente, elle sort aussitôt si l'album focal
  // est déjà le bon.
  suivreAlbumEnLecture().catch((err) => remonter(err, "suivi de lecture (anneau/frise)"));

  // Plus rien ne bouge : inutile de continuer à interroger le moteur. Toute
  // action de transport relance le sondage.
  if (e.finished || e.paused) sonder(false);
}

// Les autres modules ne peuvent pas affecter une liaison importée.
function affecterCarteTheme(v) {
  return (carteTheme = v);
}
function affecterModeCourant(v) {
  return (modeCourant = v);
}

export {
  affecterCarteTheme, affecterModeCourant, basculerFile, battement, carteTheme,
  chargerSpectreTransport, demarrerLecture, dessinerFile, dureeFile, estAffiche,
  majApercuTransport, modeCourant, peindreTransport, poserImage, poserLecture, sonder,
  teteCourante,
};
