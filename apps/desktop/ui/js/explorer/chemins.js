import "./anneau.js"; // l'ordre d'exécution : voir main.js
// Explorer : les chemins entre morceaux — tracés par le moteur, dessinés à la
// main, rejoués, sélection au lasso — et leur passage au lecteur.
import {
  $, fileAttente, invoke, navigation, remonter, signalerErreur,
} from "../socle.js";
import { selectionner } from "../ecouter.js";
import { demarrerLecture, dessinerFile } from "../lecteur.js";
import { carte, cnv } from "../carte.js";
import { bruitChemin } from "../fond-de-plan.js";
import {
  carteReelle, journalCarte, metresParPixels,
} from "./carte-gl.js";
import {
  dessinerCarte, echelle, patienter,
} from "./nuage.js";

/// Combien de morceaux le chemin doit compter — plafond pour le mode
/// sonique, dont la longueur naturelle est celle du graphe.
function longueurChemin() {
  const n = Number.parseInt($("chemin-n").value, 10);
  return Number.isFinite(n) ? Math.min(120, Math.max(2, n)) : 12;
}

/// Demande un chemin au moteur et l'envoie au lecteur.
///
/// `spec` est conservé tel quel pour que « Autre tirage » puisse rejouer le
/// même geste avec une graine différente.
async function tracerChemin(spec) {
  carte.refaire = spec;
  patienter("calcul du chemin…");
  let pistes;
  try {
    // Le voyage n'a ni arrivée ni notion de familles isolées ou de plan de
    // ville — un axe temporel, pas un espace à filtrer — d'où une commande
    // à lui, plutôt qu'un mode de plus dans `path`.
    pistes =
      spec.mode === "voyage"
        ? await invoke("voyage", {
            from: spec.from,
            steps: longueurChemin(),
            seed: carte.graine,
            bruit: bruitChemin,
          })
        : await invoke("path", {
            ...spec,
            steps: longueurChemin(),
            seed: carte.graine,
            bruit: bruitChemin,
            reel: carteReelle(),
            familles: [...carte.isolees],
          });
  } catch (e) {
    // Sans ce catch, l'échec ne laissait qu'un rappel de gestes générique en
    // pied de carte — rien qui dise qu'un calcul a réellement échoué.
    signalerErreur("chemin-aide", "échec du calcul du chemin", e, "chemin");
    return;
  } finally {
    patienter(null);
  }
  poserChemin(pistes);
}

/// Chemin dessiné : le tracé part en coordonnées de carte, avec le rayon de
/// cueillette que vaut le zoom courant. 24 px à l'écran, quel que soit le
/// facteur — c'est ce que l'utilisateur croit toucher avec son trait.
///
/// `trace` et `rayon` sont mémorisés dans `carte.refaire` (forme distincte de
/// celle de `tracerChemin`, voir `rejouerChemin`) : contrairement aux trois
/// autres modes, il n'y a pas de `{from, to, mode}` à rejouer, juste ce
/// tracé-ci, avec un nouveau bruit ou une nouvelle graine.
async function tracerDessin(trace) {
  const r = cnv.getBoundingClientRect();
  // Sur le plan de ville, le tracé et les points sont en lon/lat projetés en
  // mètres côté moteur : le rayon aussi (24 px du zoom courant, en mètres).
  // Sur le nuage t-SNE, le rayon reste dans le repère de la vue.
  const rayon = carteReelle()
    ? metresParPixels(24)
    : 24 / (echelle(r) * carte.vue.k);
  carte.refaire = { dessine: true, trace, rayon };
  patienter("cueillette sous le trait…");
  let pistes;
  try {
    pistes = await invoke("path_drawn", {
      trace,
      steps: longueurChemin(),
      radius: rayon,
      seed: carte.graine,
      bruit: bruitChemin,
      reel: carteReelle(),
      familles: [...carte.isolees],
    });
  } finally {
    patienter(null);
  }
  // Le trait dessiné EST l'intention : sur le plan de ville, on l'affiche tel
  // quel, sans le remplacer par un routage de rues entre les morceaux cueillis.
  poserChemin(pistes, "le trait n'a touché aucun morceau", carteReelle() ? trace : null);
}

/// Rejoue le dernier chemin tracé — bouton « Autre tirage » ou curseur de
/// bruit. `carte.refaire` prend deux formes : celle de `tracerChemin`
/// (`{from, to, mode}`, pour direct/sonique/errance) ou celle de
/// `tracerDessin` (`{dessine: true, trace, rayon}`), qui n'a pas de bornes à
/// repasser à `path` — chacune retrouve son propre chemin d'appel.
async function rejouerChemin() {
  if (!carte.refaire) return;
  if (carte.refaire.dessine) {
    const { trace, rayon } = carte.refaire;
    patienter("cueillette sous le trait…");
    let pistes;
    try {
      pistes = await invoke("path_drawn", {
        trace,
        steps: longueurChemin(),
        radius: rayon,
        seed: carte.graine,
        bruit: bruitChemin,
        reel: carteReelle(),
        familles: [...carte.isolees],
      });
    } catch (e) {
      remonter(e, "chemin dessiné");
      return;
    } finally {
      patienter(null);
    }
    poserChemin(pistes, "le trait n'a touché aucun morceau", carteReelle() ? trace : null);
  } else {
    await tracerChemin(carte.refaire);
  }
}

/// Joue les morceaux d'une zone dessinée au lasso.
///
/// Le moteur les rend déjà ordonnés en parcours de proche en proche : une zone
/// donne des dizaines de morceaux, et les enchaîner dans l'ordre de la base
/// produirait une playlist qui saute d'un bout à l'autre de la sélection.
async function jouerSelection(contour) {
  if (contour.length < 3) return;
  patienter("sélection…");
  let pistes;
  try {
    pistes = await invoke("selection", {
      trace: contour,
      reel: carteReelle(),
      familles: [...carte.isolees],
    });
  } finally {
    patienter(null);
  }
  if (!pistes || pistes.length === 0) {
    $("fil-compte").textContent = "le lasso n'a rien attrapé";
    return;
  }
  // Le tracé de la zone laisse la place au parcours qu'on va suivre.
  carte.refaire = null;
  await poserChemin(pistes, "sélection vide");
  $("fil-compte").textContent = `${pistes.length} morceaux de la zone`;
}

/// Affiche un chemin reçu du moteur et le met en lecture.
/// Reflète une file sur la carte : le chemin entre ses morceaux, s'il y en a
/// au moins deux — un seul morceau est déjà signalé par le halo de lecture
/// (`lecture.courant`), indépendant de `carte.route`. Sans effet tant que la carte
/// n'a pas chargé ses points ; `basculerMode` rejoue l'appel pour la file en
/// cours en entrant dans Explorer, pour rattraper une file lancée avant.
let traceRuesGraine = 0; // écarte une réponse `trace_rues` périmée par un trajet plus récent

function tracerRouteSurCarte(pistes, polyligne = null) {
  if (!carte.points.length) return;
  const parId = new Map(carte.points.map((p) => [p.id, p]));
  carte.route =
    pistes && pistes.length >= 2
      ? pistes.map((t) => parId.get(t.id)).filter(Boolean)
      : null;
  // Le trait suit d'abord la droite d'étape en étape — même donnée que les
  // repères, tant que rien de mieux n'est arrivé.
  carte.routeTrace = carte.route;
  traceRuesGraine++;

  // L'itinéraire sur voirie (et le tracé dessiné) livrent déjà leur ligne : on
  // l'affiche telle quelle, sans repasser par `trace_rues` (qui router-ait
  // entre les morceaux dans l'ordre de la playlist et pourrait boucler).
  // `{x, y}` = lon/lat, comme les segments de `trace_rues`.
  if (carte.villeReelle && polyligne && polyligne.length && carte.route) {
    carte.routeTrace =
      polyligne.length >= 2
        ? [
            carte.route[0],
            ...polyligne.map(([x, y]) => ({ x, y })),
            carte.route[carte.route.length - 1],
          ]
        : carte.route; // polyligne dégénérée : la droite entre morceaux, mais pas d'habillage
    if (navigation.mode === "explorer") dessinerCarte();
    return;
  }

  if (navigation.mode === "explorer") dessinerCarte();

  // Sur le plan de ville réel, habiller ce trait avec les vraies rues entre
  // chaque paire consécutive — **le choix des morceaux ne change pas**,
  // seule la ligne dessinée. Asynchrone et non bloquant : le trait droit
  // reste affiché jusqu'à ce que les rues arrivent, puis le remplace.
  if (carte.villeReelle && carte.route && carte.route.length >= 2) {
    const ids = carte.route.map((p) => p.id);
    const graine = traceRuesGraine;
    invoke("trace_rues", { ids })
      .then((segments) => {
        // Un autre trajet est arrivé entre-temps, ou la route a été effacée :
        // cette réponse ne concerne plus ce qui est à l'écran.
        if (graine !== traceRuesGraine || !carte.route) return;
        const habille = [carte.route[0]];
        segments.forEach((segment, i) => {
          for (const [x, y] of segment) habille.push({ x, y });
          habille.push(carte.route[i + 1]);
        });
        carte.routeTrace = habille;
        if (navigation.mode === "explorer") dessinerCarte();
      })
      .catch((e) => journalCarte("habillage du trait par les rues : " + e, "warn"));
  }
}

async function poserChemin(pistes, vide = "aucun chemin trouvé", polyligne = null) {
  if (!pistes || pistes.length < 2) {
    $("fil-compte").textContent = vide;
    carte.route = null;
    dessinerCarte();
    return;
  }

  tracerRouteSurCarte(pistes, polyligne);
  fileAttente.pistes = pistes;
  // Sans ce redessin, le panneau « file » resté ouvert continue de montrer
  // l'ancienne liste : rien d'autre ne le rafraîchit ici tant que le premier
  // morceau ne change pas — exactement le cas quand on ajuste la pondération
  // de l'errance depuis le même départ, où seule la suite change.
  if (!$("file").hidden) dessinerFile();
  // `set_queue`, pas `play` : si le premier morceau ne change pas — un
  // simple réglage du curseur de bruit ou « Autre tirage » sur le même
  // départ —, la lecture en cours n'a aucune raison de repartir de zéro.
  await demarrerLecture(() => invoke("set_queue", { paths: pistes.map((t) => t.path) }));
  selectionner(pistes[0]);
  dessinerCarte();
  $("fil-compte").textContent = `chemin de ${pistes.length} morceaux`;
  $("chemin-rejouer").hidden = !carte.refaire;
}

export {
  jouerSelection, longueurChemin, poserChemin, rejouerChemin, tracerChemin, tracerDessin,
  tracerRouteSurCarte,
};
