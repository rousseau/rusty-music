import "./ecouter-familles.js"; // l'ordre d'exécution : voir main.js
import {
  $, fileAttente, invoke, isoler, navigation, remonter, txt,
} from "./socle.js";
import {
  dessinerMiniNuage, verifierLama,
} from "./lama.js";

import {
  AIDE_CHEMIN, anneau, carte, chargerAnneau, chargerReseauAnneau, chargerTemps, CONTINUES,
  filtreAnnee, suivreAlbumEnLecture, temps,
} from "./carte.js";
import {
  carteReelle, dessinerCarte, dessinerFamilles, initialiserFiltreAnnee, journalCarte,
  majAffichageGL, majCouleurGL, majEtiquetteIntervalleAnnees, majFiltreGL, majPentesGL, modeAnneau,
  modeTemps,
  patienter, poserVignetteCarte, rafraichirFiltreAnnee, rejouerChemin, tracerChemin,
  tracerRouteSurCarte,
} from "./maplibre.js";
import {
  majSegmentsCouleur,
} from "./ecouter-familles.js";
import {
  retracerItineraireSiPret, tracerItineraire,
} from "./itineraires.js";

/* ------------------------------------------------------ fond de plan (thème) */

function majBoutonsTheme() {
  document
    .querySelectorAll("#carte-theme [data-theme]")
    .forEach((b) => b.classList.toggle("segment--actif", b.dataset.theme === carte.theme));
}
majBoutonsTheme();

document.querySelectorAll("#carte-theme [data-theme]").forEach((b) =>
  b.addEventListener("click", async () => {
    if (b.dataset.theme === carte.theme) return;
    carte.theme = b.dataset.theme;
    localStorage.setItem("carte-theme", carte.theme);
    majBoutonsTheme();
    // Pas d'instance MapLibre encore : `initialiserGL` relira `carte.theme`.
    if (!carte.gl) return;
    try {
      const style = await invoke("style_carte", { theme: carte.theme });
      // `setStyle` diffe : mêmes sources, mêmes couches, mêmes ids — seules les
      // couleurs de peinture changent, donc pas de rechargement de tuile et la
      // caméra ne bouge pas.
      carte.gl.setStyle(style);
      // Les mutations de style au runtime (couleur par famille, isolement,
      // visibilité des territoires) portaient sur l'ancien style : les rejouer
      // une fois le nouveau prêt. `setStyle` peut émettre plusieurs `styledata`
      // avant que le style soit complet — on attend `isStyleLoaded`.
      const rejouer = () => {
        if (!carte.gl || !carte.gl.isStyleLoaded()) return;
        carte.gl.off("styledata", rejouer);
        majCouleurGL();
        majFiltreGL();
        majPentesGL();
        majAffichageGL();
        poserVignetteCarte(carte.gl.getStyle());
      };
      carte.gl.on("styledata", rejouer);
      rejouer();
    } catch (e) {
      remonter(e, "thème de la carte");
    }
  }),
);

/// L'option « Pentes » : les flèches entre quartiers, gardées comme le fond.
$("carte-pentes").addEventListener("change", (e) => {
  carte.pentes = e.target.checked;
  try {
    localStorage.setItem("carte-pentes", carte.pentes ? "1" : "0");
  } catch {
    // Sans stockage, le réglage vaut pour la session.
  }
  majPentesGL();
});

/// Ce que le rail montre en Explorer selon l'affichage. Le Lama n'a ni carte
/// à colorer, ni intervalle d'années, ni familles à isoler, ni filtre de la
/// carte à saisir : il ne garde que le sélecteur d'affichage. Les autres
/// affichages retrouvent leurs réglages, chacun avec sa propre logique
/// (`majAffichageTemps`, `majAffichageAnneau`, `majVisibiliteIntervalleAnnees`).
function majRailExplorer() {
  const explorer = navigation.mode === "explorer";
  const enLama = explorer && carte.affichage === "lama";
  $("reglages-explorer").hidden = enLama;
  // La balade marche sur le nuage et sur la carte, pas sur la frise ni l'anneau.
  $("bloc-balade").hidden = !["points", "carte"].includes(carte.affichage);
  $("bloc-familles").hidden = !explorer || enLama || carte.couleur !== "famille";
  $("bloc-chercher").hidden =
    navigation.mode === "bibliotheque" ||
    navigation.mode === "decouvrir" ||
    navigation.mode === "valise" ||
    enLama;
}

/// Les modes de chemin qui ont un sens dans chaque affichage.
///
/// L'itinéraire suit de vraies rues — rien à suivre sur un nuage t-SNE.
/// Sonique et errance sautent d'un voisin sonore au suivant sans égard pour
/// la géographie : sur la carte, ce sont des zigzags qui ne servent pas
/// l'exploration du plan, contrairement au nuage où c'est justement le
/// point.
const MODES_CHEMIN = {
  points: ["direct", "sonique", "errance", "dessine"],
  carte: ["direct", "dessine", "itineraire"],
  // Le streamgraph n'a ni géométrie d'écran (direct, dessiné) ni deux bornes
  // à relier (sonique) : seul un départ qui avance dans le temps a un sens.
  temps: ["voyage"],
  // L'anneau n'a aucun mode de chemin : ni chronologie à parcourir, ni
  // géométrie d'écran à tracer — juste un album focal et ses voisins.
  anneau: [],
  // Le Lama compose par la parole : aucun chemin à tracer à la souris.
  lama: [],
};

/// Montre/cache les réglages propres à chaque mode de chemin. « morceaux » et
/// « bruit » disparaissent pour l'itinéraire (qui a ses propres réglages :
/// profil, durée) ; le bouton « Tracer » ne reste que hors plan de ville, où le
/// calcul musical est trop lent pour se déclencher tout seul.
function majReglagesChemin() {
  const estItin = carte.chemin === "itineraire";
  if ($("bloc-itineraire")) $("bloc-itineraire").hidden = !estItin;
  if ($("reglette-morceaux")) $("reglette-morceaux").hidden = estItin;
  if ($("bloc-bruit")) $("bloc-bruit").hidden = estItin;
  if ($("itin-tracer")) $("itin-tracer").hidden = estItin && carteReelle();
  if (estItin && $("chemin-rejouer")) $("chemin-rejouer").hidden = true;
}

/// Affiche les boutons de mode de chemin qui ont un sens dans l'affichage
/// courant, et bascule sur « direct » si le mode actif n'en fait plus
/// partie.
function majModesChemin() {
  const disponibles = MODES_CHEMIN[carte.affichage] || MODES_CHEMIN.points;
  // L'anneau n'a aucun mode de chemin (voir `MODES_CHEMIN.anneau`) : sans ce
  // repli, le bloc « Chemin » restait affiché — titre, bornes vides et aide
  // du mode « direct » — alors qu'aucun de ses boutons n'a de sens ici.
  $("bloc-chemin").hidden = disponibles.length === 0;
  document.querySelectorAll("[data-chemin]").forEach((b) => {
    b.hidden = !disponibles.includes(b.dataset.chemin);
  });
  if (disponibles.length === 0) return;
  if (!disponibles.includes(carte.chemin)) {
    // « direct » n'existe pas dans tous les affichages (le streamgraph n'a
    // que « voyage ») : le premier mode disponible, pas une valeur fixe.
    // `false` : ce repli est automatique, pas un choix de l'utilisateur — il
    // ne doit jamais retracer (voir la note sur `retracer` de `poserModeChemin`).
    poserModeChemin(disponibles[0] ?? "direct", false);
  } else {
    majReglagesChemin();
  }
}

/// Montre/cache ce qui n'a de sens que sur le nuage ou la carte — couleur,
/// fond de plan, zoom — et ce qui n'a de sens que sur le streamgraph, le
/// seuil des fils d'artiste.
function majAffichageTemps() {
  const t = modeTemps();
  // La couleur n'a pas de sens non plus sur l'anneau (voir
  // `majAffichageAnneau`, appelée au même endroit) : les deux calculent la
  // même condition, pour que l'ordre d'appel ne fasse jamais gagner l'un sur
  // l'autre.
  $("bloc-colorer-par").hidden = t || modeAnneau();
  $("bloc-temps").hidden = !t;
  if (t) {
    $("zoom-val").textContent = `×${temps.vue.k.toFixed(1).replace(".", ",")}`;
    // Le focal a été remis à zéro par le changement d'affichage ; si un
    // disque joue, il le redevient (`suivreAlbumEnLecture` s'en assure).
    chargerTemps().then(() => {
      dessinerCarte();
      suivreAlbumEnLecture().catch((err) => remonter(err, "suivi de lecture (frise)"));
    });
  }
}

/// Montre/cache ce qui n'a de sens que sur l'anneau — le curseur « voisins ».
/// Le zoom, lui, reste visible : voir `zoomerAnneau`. Jumelle de
/// `majAffichageTemps`, appelée au même endroit.
function majAffichageAnneau() {
  const a = modeAnneau();
  $("bloc-colorer-par").hidden = a || modeTemps();
  $("bloc-anneau").hidden = !a;
  if (a) {
    $("anneau-k").value = anneau.k;
    $("anneau-k-val").textContent = String(anneau.k);
    $("anneau-beta").value = anneau.beta;
    $("anneau-beta-val").textContent = anneau.beta.toFixed(2).replace(".", ",");
    $("zoom-val").textContent = `×${anneau.vue.k.toFixed(1).replace(".", ",")}`;
    chargerReseauAnneau().then(() => {
      dessinerCarte();
      suivreAlbumEnLecture().catch((err) => remonter(err, "suivi de lecture (anneau)"));
    });
  }
}

document.querySelectorAll("[data-affichage]").forEach((b) =>
  b.addEventListener("click", () => {
    const avant = carte.affichage;
    carte.affichage = b.dataset.affichage;
    // Entrer sur le Nuage = vue d'ensemble. `carte.vue` survit d'un affichage
    // à l'autre : la Carte MapLibre y recopie son zoom (`synchroniserVue`) et
    // un ancien glissement y laisse son décalage — le nuage s'ouvrait alors
    // zoomé, ou décentré, au lieu d'englober tous les points.
    if (carte.affichage === "points" && avant !== "points") {
      carte.vue = { k: 1, dx: 0, dy: 0 };
      $("zoom-val").textContent = "×1,0";
    }
    // Le Lama a son propre centre, sans canevas : on y entre sans toucher aux
    // redessins de la carte, et la route de la playlist en cours reste intacte
    // pour le jour où l'on revient sur le Nuage ou la Carte.
    const enLama = carte.affichage === "lama";
    $("carte-vue").hidden = enLama || navigation.mode !== "explorer";
    $("lama-vue").hidden = !enLama || navigation.mode !== "explorer";
    document
      .querySelectorAll("[data-affichage]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
    majRailExplorer();
    if (enLama) {
      majModesChemin();
      $("fil-titre").textContent = "Lama";
      dessinerMiniNuage();
      verifierLama();
      $("intention-texte").focus();
      return;
    }
    $("fil-titre").textContent = "Carte";
    // Un chemin *calculé* (le tracé d'un mode direct/sonique/dessiné) vit
    // dans un repère précis (t-SNE, lon-lat, temps) et n'a plus de sens dans
    // un autre : on l'efface plutôt que de rejouer des coordonnées
    // étrangères au repère courant. La playlist EN COURS DE LECTURE
    // (`fileAttente.pistes`) est une donnée complètement différente — elle ne doit
    // jamais dépendre de l'affichage, voir `tracerRouteSurCarte` juste après.
    carte.refaire = null;
    carte.route = null;
    carte.routeTrace = null;
    carte.survole = null;
    temps.survole = null;
    anneau.survole = null;
    anneau.focal = null;
    anneau.voisins = [];
    $("carte-info").hidden = true;
    majAffichageGL();
    majAffichageTemps();
    majAffichageAnneau();
    majModesChemin();
    // La légende gagne/perd le bouton d'extension verticale (§ 3) selon
    // qu'on entre ou quitte la frise — sans ce rafraîchissement, il resterait
    // affiché après être passé sur la Carte, ou absent en revenant sur Temps.
    dessinerFamilles();
    // `majModesChemin` ne rafraîchit le pied de carte que lorsqu'un mode de
    // chemin existe pour ce nouvel affichage (voir son repli sur l'anneau,
    // qui n'en a aucun) — le garantir ici plutôt que dans chaque branche.
    $("carte-aide").textContent = aideCourante();
    document
      .querySelectorAll("[data-affichage]")
      .forEach((s) => s.classList.toggle("segment--actif", s === b));
    // Redessine la playlist en cours dans le nouveau repère — nuage et carte
    // en ont besoin pour reconstruire `carte.route`/`carte.routeTrace`,
    // effacés juste au-dessus ; l'anneau, lui, lit `fileAttente.pistes` directement
    // à chaque image (voir `dessinerAnneau`) et n'en a pas besoin, mais
    // l'appeler ne coûte rien de plus qu'un redessin déjà prévu.
    tracerRouteSurCarte(fileAttente.pistes);
    dessinerCarte();
  }),
);

/// Rappel du geste attendu, en pied de carte. Le streamgraph a un zoom et un
/// glisser horizontaux, mais ni clic (rien à écouter directement dessus) ni
/// lasso (pas de sélection de zone sur un axe temporel).
function aideCourante() {
  // L'anneau n'a ni lasso, ni chemin — mais zoome et se glisse comme le
  // nuage (voir `zoomerAnneau`), en plus du clic pour changer d'album focal.
  // `carte.chemin` peut porter n'importe quelle valeur laissée par le
  // dernier affichage à en avoir eu un ; inutile d'y toucher ici.
  if (modeAnneau()) return "clic sur un voisin : nouvel album focal · glisser pour déplacer, molette pour zoomer";
  const [, court] = AIDE_CHEMIN[carte.chemin];
  return modeTemps()
    ? `molette : zoom · glisser : déplacer · ${court}`
    : `molette : zoom · glisser : déplacer · clic : écouter · ${court} · alt+glisser : lasso`;
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
      ? `${isoler(txt(t.artist, "?"))} — ${isoler(txt(t.title, "?"))}`
      : "—";
  }
  majDureeItin();
}

/// Une arrivée posée prime sur la durée : « va jusque-là » l'emporte. On grise
/// alors le curseur de durée pour que ce soit visible.
function majDureeItin() {
  const e = $("itin-minutes");
  if (!e) return;
  const inactif = !!carte.arrivee;
  e.disabled = inactif;
  const l = $("itin-minutes")?.closest(".reglette");
  if (l) l.style.opacity = inactif ? 0.4 : "";
  const out = $("itin-minutes-val");
  if (out) out.textContent = inactif ? "→ arrivée" : +e.value > 0 ? `${e.value} min` : "libre";
}

document.querySelectorAll("[data-borne]").forEach((b) =>
  b.addEventListener("click", () => {
    carte[b.dataset.borne] = null;
    carte.route = null;
    dessinerBornes();
    dessinerCarte();
    retracerItineraireSiPret();
  }),
);

/// Pose une borne et trace dès que les deux sont là.
///
/// L'errance et le voyage n'ont qu'une borne : ils partent dès le départ posé.
async function poserBorne(t) {
  if (carte.chemin === "errance" || carte.chemin === "voyage") {
    carte.depart = t;
    carte.arrivee = null;
    dessinerBornes();
    carte.graine = 1;
    await tracerChemin({ from: t.id, mode: carte.chemin });
    return;
  }
  if (!carte.depart) carte.depart = t;
  else carte.arrivee = t;
  dessinerBornes();
  if (carte.chemin === "itineraire") {
    // Sur le plan de ville, l'itinéraire se trace tout seul dès le départ posé
    // (l'arrivée est facultative si une durée est fixée) — comme `direct`.
    // Sans plan de ville, le calcul musical est trop lent pour ça : le bouton
    // « Tracer » reste visible et prend la main.
    if (carteReelle()) await tracerItineraire();
  } else if (carte.depart && carte.arrivee) {
    await tracerChemin({
      from: carte.depart.id,
      to: carte.arrivee.id,
      mode: carte.chemin,
    });
  }
}

/// Le texte du rail pour un mode de chemin donné. Le rappel « ou : chercher
/// puis Entrée » n'a de sens que là où un autre geste existe déjà — le
/// voyage, lui, ne se pose que par la recherche, l'ajouter ferait doublon.
function texteAideChemin(mode) {
  const base = AIDE_CHEMIN[mode][0];
  return mode === "voyage" ? base : `${base} Ou : chercher puis Entrée pour poser une borne.`;
}

/// `retracer` : ne relance un chemin depuis les bornes déjà choisies
/// (`carte.depart`/`carte.arrivee`) que sur un choix explicite de l'utilisateur
/// (le bouton `[data-chemin]`, tout en bas). Les réaffirmations automatiques
/// du mode — `majModesChemin` qui retombe sur un mode par défaut au
/// changement d'**affichage** (nuage/carte/temps/anneau), ou l'entrée dans
/// Explorer — passent `false` : la visualisation ne doit jamais avoir
/// d'effet de bord sur la playlist en cours de lecture. Sans cette
/// distinction, changer d'affichage avec un vieux départ/arrivée encore posé
/// relançait un chemin et écrasait silencieusement `fileAttente.pistes`.
function poserModeChemin(mode, retracer = true) {
  carte.chemin = mode;
  majReglagesChemin();
  carte.trace = null;
  // Change de mode sans changer de trajet : « Autre tirage » et le curseur
  // de bruit rejoueraient sinon la spécification de l'ancien mode sous le
  // nom du nouveau. Le tracé déjà à l'écran, lui, reste visible tel quel
  // jusqu'au prochain chemin calculé — seul le rejeu est désarmé.
  carte.refaire = null;
  document
    .querySelectorAll("[data-chemin]")
    .forEach((s) => s.classList.toggle("segment--actif", s.dataset.chemin === mode));
  $("chemin-aide").textContent = texteAideChemin(mode);
  $("carte-aide").textContent = aideCourante();
  // En itinéraire, la visibilité du bouton dépend des variantes — gérée par
  // `majReglagesChemin` ci-dessus.
  if (mode !== "itineraire") $("chemin-rejouer").hidden = !carte.refaire;
  dessinerBornes();
  dessinerCarte();

  // Sonique et errance passent par le graphe des voisins : on le prépare dès
  // le choix du mode, pour que le premier chemin ne paie pas la construction.
  //
  // **Le dire est indispensable.** Le calcul sature tous les cœurs ; muet, il
  // se lit comme un plantage, ventilateurs compris. On l'annonce donc dans le
  // rail et en pied de carte, et on efface dès que c'est prêt.
  if (mode === "sonique" || mode === "errance" || mode === "voyage") {
    const attente = "Préparation du graphe des voisins…";
    $("chemin-aide").textContent = attente;
    patienter(attente);
    preparerGraphe().finally(() => {
      // Le mode a pu changer entre-temps : on réaffiche l'aide du mode
      // courant, pas celle de celui qui avait lancé la préparation.
      $("chemin-aide").textContent = texteAideChemin(carte.chemin);
      patienter();
    });
  }

  // Le départ (et l'arrivée) choisis restent d'un mode à l'autre — voir
  // `carte.depart`, jamais effacé ci-dessus. Changer de mode *explicitement*
  // doit aussitôt retracer avec ces bornes-là, sans obliger à recliquer :
  // c'est la trajectoire qui change, pas le point d'où l'on veut explorer.
  // Mais seulement sur ce choix explicite (voir la note sur `retracer`).
  if (!retracer) return;
  if (mode === "errance" || mode === "voyage") {
    if (carte.depart) tracerChemin({ from: carte.depart.id, mode }).catch((e) => remonter(e, "chemin"));
  } else if (mode === "itineraire") {
    if (carte.depart && carteReelle()) tracerItineraire().catch((e) => remonter(e, "itinéraire"));
  } else if (mode !== "dessine" && carte.depart && carte.arrivee) {
    tracerChemin({ from: carte.depart.id, to: carte.arrivee.id, mode }).catch((e) => remonter(e, "chemin"));
  }
}

document
  .querySelectorAll("[data-chemin]")
  .forEach((b) => b.addEventListener("click", () => poserModeChemin(b.dataset.chemin)));

$("chemin-rejouer").addEventListener("click", async () => {
  if (!carte.refaire) return;
  carte.graine += 1;
  await rejouerChemin();
});

/// Bruit commun aux quatre modes de chemin — un curseur du rail, toujours
/// visible en Explorer. 0 : trajet exact. Plus haut : dérive davantage, dans
/// le registre propre à chaque mode (voir la note en tête de `chemin.rs`
/// côté moteur). Mémorisé en local : ce n'est pas une donnée de la
/// bibliothèque, juste une préférence de cette installation.
const BRUIT_DEFAUT = 0.3;
let bruitChemin = Number(localStorage.getItem("bruit-chemin")) || BRUIT_DEFAUT;
$("bruit-chemin").value = bruitChemin;
$("bruit-valeur").textContent = bruitChemin.toFixed(2).replace(".", ",");

// Un délai court, pas un aller-retour à chaque cran du curseur : on laisse
// le geste se terminer avant de relancer le calcul et la lecture, sinon
// glisser le curseur bombarderait le moteur de dizaines de chemins.
let attenteBruit = null;
$("bruit-chemin").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  bruitChemin = Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : BRUIT_DEFAUT;
  $("bruit-valeur").textContent = bruitChemin.toFixed(2).replace(".", ",");
  localStorage.setItem("bruit-chemin", String(bruitChemin));

  // Un chemin est déjà tracé : on le rejoue au nouveau bruit, à graine
  // égale — seul le paramètre qu'on ajuste doit bouger, pas aussi le
  // tirage, sans quoi l'effet du curseur se confondrait avec celui d'un
  // nouveau tirage.
  if (!carte.refaire) return;
  clearTimeout(attenteBruit);
  attenteBruit = setTimeout(() => {
    rejouerChemin().catch((e) => remonter(e, "réglage du bruit"));
  }, 250);
});

/// Seuil de marquage des hubs de la frise — voir `temps.seuilHub` et
/// `dessinerTemps` § 6. Initialisé au chargement à une valeur calculée
/// (moyenne + 2 écarts-types des degrés sortants), ajustable ensuite ici.
$("temps-seuil-val").textContent = `${temps.seuilHub} connexions`;
$("temps-seuil").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  temps.seuilHub = Number.isFinite(v) ? Math.max(1, v) : temps.seuilHub;
  $("temps-seuil-val").textContent = `${temps.seuilHub} connexion${temps.seuilHub > 1 ? "s" : ""}`;
  if (modeTemps()) dessinerCarte();
});

/// Nombre de voisins demandés au moteur pour l'anneau — change la taille de
/// la requête `album_ring`, pas seulement le rendu : redemande donc l'anneau
/// courant plutôt que de se contenter de redessiner.
$("anneau-k").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  anneau.k = Number.isFinite(v) ? Math.min(20, Math.max(3, v)) : anneau.k;
  $("anneau-k-val").textContent = String(anneau.k);
});
$("anneau-k").addEventListener("change", () => {
  if (modeAnneau() && anneau.focal) chargerAnneau(anneau.focal.id);
});

/// Tension du bundling — pur rendu, pas un nouveau voisinage : redessiner
/// suffit, pas la peine de redemander l'anneau au moteur.
$("anneau-beta").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  anneau.beta = Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : anneau.beta;
  $("anneau-beta-val").textContent = anneau.beta.toFixed(2).replace(".", ",");
  if (modeAnneau()) dessinerCarte();
});

/// L'intervalle d'années (`filtreAnnee`), commun aux quatre visualisations.
/// Pur rendu côté données déjà chargées (tuiles ou `carte.points`) : glisser
/// refiltre et redessine, ça ne redemande jamais rien au moteur.
$("annee-debut").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  if (Number.isFinite(v)) filtreAnnee.debut = Math.min(v, filtreAnnee.fin);
  $("annee-debut").value = String(filtreAnnee.debut);
  majEtiquetteIntervalleAnnees();
  rafraichirFiltreAnnee();
});
$("annee-fin").addEventListener("input", (e) => {
  const v = Number(e.target.value);
  if (Number.isFinite(v)) filtreAnnee.fin = Math.max(v, filtreAnnee.debut);
  $("annee-fin").value = String(filtreAnnee.fin);
  majEtiquetteIntervalleAnnees();
  rafraichirFiltreAnnee();
});
$("annee-reinit").addEventListener("click", () => {
  filtreAnnee.debut = filtreAnnee.min;
  filtreAnnee.fin = filtreAnnee.max;
  $("annee-debut").value = String(filtreAnnee.min);
  $("annee-fin").value = String(filtreAnnee.max);
  majEtiquetteIntervalleAnnees();
  rafraichirFiltreAnnee();
});

/// Construit le graphe des voisins en tâche de fond, une seule fois à la
/// fois. Nu : ne dit rien de l'attente, c'est à l'appelant de le faire dans
/// son propre contexte — `poserModeChemin` pour la carte, `poser` pour la
/// grille d'albums.
///
/// Le balayage est complet : une vingtaine de secondes sur la bibliothèque
/// entière, davantage à mesure que l'analyse la remplit. Rien n'attend le
/// résultat — la commande met le graphe en cache côté moteur, les appels
/// suivants (`path` en mode sonique/errance, `path_album`) le trouvent prêt.
let graphePret = null;
function preparerGraphe() {
  if (graphePret) return graphePret;
  graphePret = invoke("prepare_graph")
    .catch((e) => remonter(e, "préparation du graphe"))
    .finally(() => {
      graphePret = null;
    });
  return graphePret;
}

async function chargerCarte() {
  const [points] = await Promise.all([
    invoke("map_view"),
  ]);
  carte.points = points;

  // Plan de ville réel : la vraie adresse (lon/lat) de chaque morceau logé,
  // **à côté** de `p.x`/`p.y` — jamais à la place. `p.x`/`p.y` restent la
  // position t-SNE partout : c'est ce que lit le mode Nuage, qui doit rester
  // correct que la ville soit active ou non. `versEcran` choisit laquelle
  // utiliser selon `carte.villeReelle` et l'affichage courant. `positions_carte`
  // échoue sur le chemin fictif : c'est le signal, pas une erreur à
  // remonter.
  let positionsReelles = null;
  try {
    positionsReelles = await invoke("positions_carte");
  } catch {
    positionsReelles = null;
  }
  carte.villeReelle = !!(positionsReelles && Object.keys(positionsReelles).length > 0);
  carte.positionsReelles = carte.villeReelle
    ? new Map(Object.entries(positionsReelles).map(([id, p]) => [+id, p]))
    : new Map();
  majSegmentsCouleur();
  if (carte.villeReelle) {
    const sansAdresse = carte.points.filter((p) => !carte.positionsReelles.has(p.id)).length;
    if (sansAdresse) {
      // Un morceau que l'affectation n'a pas logé n'a pas d'entrée ici :
      // `versEcran` retombe alors sur sa position t-SNE plutôt que de le
      // faire disparaître.
      journalCarte(`${sansAdresse} morceau(x) sans adresse réelle, position t-SNE conservée`, "warn");
    }
  }

  const [faits, total] = await invoke("map_progress");

  // Les bornes de chaque variable, une fois pour toutes au chargement.
  for (const [cle, { champ, valide }] of Object.entries(CONTINUES)) {
    const vs = carte.points
      .map((p) => p[champ])
      .filter((v) => v != null && (!valide || valide(v)));
    carte.bornes[cle] = vs.length ? [Math.min(...vs), Math.max(...vs)] : [0, 0];
  }
  majLegendeContinue();
  initialiserFiltreAnnee();
  $("fil-titre").textContent = "Carte";
  $("fil-compte").textContent =
    faits < total
      ? `${carte.points.length.toLocaleString("fr-FR")} placés · ${(total - faits).toLocaleString("fr-FR")} en attente`
      : `${carte.points.length.toLocaleString("fr-FR")} morceaux`;
  dessinerFamilles();
  // Le Lama n'a pas de canevas de carte à redessiner : seulement son mini-nuage.
  if (carte.affichage === "lama") {
    majModesChemin();
    dessinerMiniNuage();
  } else {
    majAffichageGL();
    majModesChemin();
  }
  chargerTemps().then(() => {
    if (modeTemps()) dessinerCarte();
  });
}

export {
  aideCourante, bruitChemin, chargerCarte, dessinerBornes, majAffichageAnneau, majAffichageTemps,
  majLegendeContinue, majRailExplorer, poserBorne, poserModeChemin, preparerGraphe,
};
