import "./etabli-reglages.js"; // l'ordre d'exécution : voir main.js
import {
  $, ecoute, enStems, fileAttente, invoke, journal, remonter, signalerErreur, txt, vue, vueEnGrille,
} from "./socle.js";
import { majUsage, oublierPartitions } from "./pratiquer.js";
import { lecture, poserStemsCharges } from "./etat-lecture.js";
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
import { quitterMemoirePratique, rendreMemoirePratique } from "./memoire-pratique.js";
import { majEnchainement } from "./pratiquer-playlist.js";
import {
  edition, majPulsation, majSections, majTempoSource, marquerSectionCourante, positionnerBande,
  reposerBoucle,
} from "./etabli.js";
import {
  appliquerReglages, appliquerUn, appliquerVitesses, cablerChamp, calerReglage, dessinerReglages,
  libelleStem, majDerive, REGLAGES, tonaliteDe, vitesseDe,
} from "./etabli-reglages.js";

/* --------------------------------------------- l'établi : les stems */

// Le parcours choisir → séparer → établi, les variantes et le démixage, la
// pile de stems et leur panneau, le playhead, les spectres, les niveaux et la
// lecture des stems. Découpé d'`etabli.js` (V4), qui garde l'état `edition`.

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
  // La séance du morceau qu'on quitte est gardée (E1, `memoire-pratique.js`).
  if (change) await quitterMemoirePratique();
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
  // Le bandeau de la playlist enchaînée et son sélecteur (P2).
  majEnchainement(choisir);
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
    mesurerPresence();
    $("demix-etat").textContent = `${trouves.length} stems déjà calculés`;
    majTempoSource();
    majPulsation();
    majSections();
    if (enStems()) await prendreLaMain();
    // En Pratiquer, la séance laissée sur ce morceau revient.
    await rendreMemoirePratique();
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
$("lancer-demix").addEventListener("click", () => lancerDemix());

/// Sépare le morceau ouvert avec la variante choisie : le bouton « Séparer »,
/// et Pratiquer quand il enchaîne une playlist (P2, `pratiquer-playlist.js`).
/// Rend la main une fois le calcul lancé, pas fini : vrai s'il l'est.
async function lancerDemix() {
  const t = edition.source;
  if (!t) return false;
  // Le morceau **séparé** : la sélection est libre (aucune lecture à lancer
  // pour la changer), elle peut donc bouger pendant les ~30 s de calcul.
  const cible = t.path;
  const variante = edition.variante;
  try {
    await invoke("start_demix", { path: t.path, variant: variante });
  } catch (e) {
    $("demix-etat").textContent = String(e);
    return false;
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
    mesurerPresence();
    majTempoSource();
    majPulsation();
    majSections();
    // Même règle qu'au chargement d'un démixage existant : afficher des stems,
    // c'est en faire la source. Sans cela, solo, coupure, vitesse et hauteur
    // n'agissaient sur rien tant qu'on n'avait pas relancé la lecture.
    // La variante des stems est retenue avec la séance (E1).
    edition.varianteStems = variante;
    await prendreLaMain();
    await rendreMemoirePratique();
  }, 800); // assez serré pour que la barre avance visiblement segment par segment
  return true;
}

/// V38 : la part de chaque stem dans le mélange (`stems_presence`, mesurée
/// une fois par démixage). Un stem presque vide — le piano d'un trio rock —
/// est masqué dans la pile, sans être coupé ni effacé : il joue toujours à
/// son niveau, la somme reste le morceau entier (le fondre dans « autres »
/// demanderait de réécrire les fichiers pour un gain inaudible). Un geste
/// sous la pile le montre.
async function mesurerPresence() {
  const src = edition.source;
  if (!src?.path || !edition.stems.length) return;
  edition.montrerVides = false;
  let parts;
  try {
    parts = await invoke("stems_presence", { path: src.path });
  } catch (e) {
    journal("stems", `présence : ${e}`, "warn");
    return;
  }
  // Un autre morceau a été ouvert pendant la mesure.
  if (edition.source?.path !== src.path) return;
  const vides = new Set(parts.filter((p) => p.presque_vide).map((p) => p.nom));
  for (const s of edition.stems) s.presqueVide = vides.has(s.nom);
  if (vides.size) dessinerStems();
}

/// Masqué dans la pile : presque vide, ni greffé ni écouté seul.
function masque(s) {
  return !!s.presqueVide && !edition.montrerVides && !s.greffe && edition.solo !== s.nom;
}

/// La ligne sous la pile qui dit les pistes presque vides et les montre ou
/// les masque.
function ligneVides(hote) {
  const vides = edition.stems.filter((s) => s.presqueVide && !s.greffe);
  if (!vides.length) return;
  const noms = vides.map((s) => libelleStem(s.nom)).join(", ");
  const b = document.createElement("button");
  b.className = "pile__vides";
  b.dataset.a = "vides";
  b.textContent = edition.montrerVides
    ? `masquer ${vides.length > 1 ? "les pistes presque vides" : "la piste presque vide"} : ${noms}`
    : `${vides.length} piste${vides.length > 1 ? "s" : ""} presque vide${vides.length > 1 ? "s" : ""} masquée${vides.length > 1 ? "s" : ""} : ${noms} · montrer`;
  b.title = "Ces stems ne reçoivent presque rien du morceau (moins de −30 dB du mélange). Masqués, ils jouent toujours : rien n'est coupé ni effacé.";
  b.addEventListener("click", () => {
    edition.montrerVides = !edition.montrerVides;
    dessinerStems();
  });
  hote.appendChild(b);
}

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
    if (masque(s)) continue;
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
  ligneVides(hote);

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

/// Lance la transposition et attend qu'elle finisse, en disant où elle en est.
///
/// **Le calcul ne bloque plus l'interface, et c'est le but de tout ceci.** Une
/// transposition demande quelques secondes par stem (8 s pour quatre minutes,
/// Signalsmith en réglage juste depuis E6) ; la faire dans une commande qui ne rend pas la main mettait toutes
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

  // Barre graduée stem par stem, comme celle de la séparation : à huit
  // secondes par stem, une barre qui bouge dit « ça avance » là où un texte
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
  appliquerNiveaux, arreterStems, choisirVariante, deplacerLecture, dessinerStems, enSerie,
  lancerDemix, majEtatEditer, majVariantes, niveaux, oublierStems, poserSourceEdition, poserTete,
  positionnerPlayhead, prendreLaMain, rampeRGB, rattacherStems, reconcilierStems, resumeStem,
  transposerStems, varianteParDefaut, verifierNoms,
};
